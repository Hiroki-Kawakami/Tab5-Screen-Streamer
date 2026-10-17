/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "pc_app_stream_screen.hpp"
#include <algorithm>
#include <cstdlib>
#include "display/panel.hpp"
#include "display_manager.hpp"
#include "esp_log.h"
#include "pc_app_screen.hpp"
#include "resources.h"
#include "settings.hpp"
#include "stream/stream_overlay.hpp"
#include "ui_orientation.hpp"
#include "usb/usb.hpp"

static const char *TAG = "pc_app_stream";

namespace {

constexpr int kTapSlop = 24;
constexpr int kCornerSize = 96;
constexpr int kSwipeDistance = 48;
constexpr TickType_t kIdleTimeout = pdMS_TO_TICKS(3 * 60 * 1000);

bool in_corner(const bsp_touch_point_t &point, bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    return point.x >= size.width - kCornerSize && point.y >= size.height - kCornerSize;
}

}

PcAppStreamScreen::~PcAppStreamScreen() {
    if (idle_timer_) xTimerDelete(idle_timer_, portMAX_DELAY);
}

void PcAppStreamScreen::build() {
    lv_obj_set_style_bg_color(root_, lv_color_black(), 0);
    lv_obj_set_style_bg_opa(root_, LV_OPA_COVER, 0);
    rotation_ = ui_orientation_current();
    input_rotation_ = rotation_;
}

void PcAppStreamScreen::buildWaiting(lv_obj_t *screen) {
    waiting_ = stream_waiting_build(screen, touch_ ? "Swipe in from the bottom-right corner to show menu"
                                                   : "Tap to show menu");
    stream_waiting_layout(waiting_, rotation_);
    updateWaiting();
}

void PcAppStreamScreen::onEnter() {
    ui_orientation_set_listener([](bsp_rotation_t rotation, void *arg) {
        static_cast<PcAppStreamScreen *>(arg)->rotate(rotation);
    }, this);
    waiting_display_ = panel_idle_display_create(rotation_);
    buildWaiting(waiting_display_ ? lv_display_get_screen_active(waiting_display_) : root_);
    idle_timer_ = xTimerCreate("pc_app_idle", kIdleTimeout, pdFALSE, this, idleTimeout);
    if (!idle_timer_) ESP_LOGE(TAG, "no memory for the idle timer");
    idle_enabled_ = true;
    restartIdle();

    std::weak_ptr<Screen> weak = weak_from_this();
    service_ = std::make_shared<Tab5Service>([weak](UsbStatus status) {
        lv_lock();
        lv_async_call([weak, status] {
            if (auto self = weak.lock()) static_cast<PcAppStreamScreen &>(*self).setStatus(status);
        });
        lv_unlock();
    });
    esp_err_t err = service_->start();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "tab5 service: %s", esp_err_to_name(err));
        service_.reset();
        return;
    }
    trackpad_ = std::make_unique<Trackpad>([this](const usb_device::MouseReport &report) {
        mouseReport(report);
    });
    if (trackpad_->start() != ESP_OK) trackpad_.reset();
    service_->set_input_listener([this](tab5::InputTarget target, tab5::InputMode mode) {
        inputModeChanged(target, mode);
    });
    service_->set_display_power_listener([this](bool on) {
        display_on_ = on;
        applyBrightness();
        if (on) restartIdle();
    });
    service_->set_brightness_listener([weak](int percent) {
        lv_lock();
        lv_async_call([weak, percent] {
            if (auto self = weak.lock()) {
                static_cast<PcAppStreamScreen &>(*self).setBrightness(percent);
            }
        });
        lv_unlock();
    });
    service_->brightness_changed(settings_display_brightness());
    service_->set_input_enabled(tab5::InputTarget::Touch, touch_);
    display_manager.set_outside_touch_callback(outsideTouch, this);
    bsp_keyboard_set_event_cb(keyboardEvent, this);
    startUsb();
}

// The service and the trackpad stay until the screen is destroyed: the touch
// and keyboard callbacks may still be running after they are unregistered.
void PcAppStreamScreen::onExit() {
    display_manager.set_outside_touch_callback(nullptr);
    bsp_keyboard_set_event_cb(nullptr, nullptr);
    if (service_) {
        service_->set_input_listener(nullptr);
        service_->set_display_power_listener(nullptr);
        service_->set_brightness_listener(nullptr);
    }
    if (trackpad_) trackpad_->set_active(false);
    idle_enabled_ = false;
    if (idle_timer_) xTimerStop(idle_timer_, portMAX_DELAY);
    display_on_ = true;
    closeOverlay();
    applyBrightness();
    panel_idle_display_delete();
    waiting_display_ = nullptr;
    usb_stop();
    std::shared_ptr<Tab5Profile> profile;
    {
        std::lock_guard<std::mutex> guard(profile_lock_);
        profile = std::move(profile_);
    }
    ui_orientation_set_listener(nullptr, nullptr);
}

// The previous profile is let go before the next one is queued, so that it
// detaches from the service before the next one attaches.
void PcAppStreamScreen::startUsb() {
    keyboard_enumerated_ = hid_ && bsp_keyboard_is_connected();
    service_->set_input_enabled(tab5::InputTarget::Keyboard, keyboard_enumerated_);
    auto profile = std::make_shared<Tab5Profile>(service_, audio_, keyboard_enumerated_, touch_);
    std::shared_ptr<Tab5Profile> previous;
    {
        std::lock_guard<std::mutex> guard(profile_lock_);
        previous = std::move(profile_);
        profile_ = profile;
    }
    previous.reset();
    usb_start(std::move(profile));
}

void PcAppStreamScreen::keyboardChanged() {
    if (!service_ || (hid_ && bsp_keyboard_is_connected()) == keyboard_enumerated_) return;
    ESP_LOGI(TAG, "keyboard %s, restarting usb", keyboard_enumerated_ ? "detached" : "attached");
    startUsb();
}

void PcAppStreamScreen::keyboardEvent(const bsp_keyboard_event_t *event, void *arg) {
    auto *self = static_cast<PcAppStreamScreen *>(arg);
    if (event->type == BSP_KEYBOARD_EVENT_CONNECTED ||
        event->type == BSP_KEYBOARD_EVENT_DISCONNECTED) {
        self->post(&PcAppStreamScreen::keyboardChanged);
        return;
    }
    if (event->type != BSP_KEYBOARD_EVENT_HID) return;
    self->restartIdle();
    const tab5::KeyEvent key = {event->hid.modifiers, 0, {event->hid.keycode}};
    if (self->service_->key(key)) return;
    std::lock_guard<std::mutex> keyboard(self->keyboard_lock_);
    if (!self->keyboard_hid_) return;
    usb_device::KeyboardReport report = {};
    report.modifiers = event->hid.modifiers;
    report.keys[0] = event->hid.keycode;
    self->keyboardReport(report);
}

void PcAppStreamScreen::inputModeChanged(tab5::InputTarget target, tab5::InputMode mode) {
    if (target == tab5::InputTarget::Touch) {
        if (trackpad_) trackpad_->set_active(mode == tab5::InputMode::Hid);
        return;
    }
    std::lock_guard<std::mutex> guard(keyboard_lock_);
    const bool released = keyboard_hid_ && mode != tab5::InputMode::Hid;
    keyboard_hid_ = mode == tab5::InputMode::Hid;
    if (released) keyboardReport({});
}

void PcAppStreamScreen::keyboardReport(const usb_device::KeyboardReport &report) {
    std::lock_guard<std::mutex> guard(profile_lock_);
    if (profile_) profile_->keyboard_report(report);
}

void PcAppStreamScreen::mouseReport(const usb_device::MouseReport &report) {
    std::lock_guard<std::mutex> guard(profile_lock_);
    if (profile_) profile_->mouse_report(report);
}

void PcAppStreamScreen::post(void (PcAppStreamScreen::*method)()) {
    std::weak_ptr<Screen> weak = weak_from_this();
    lv_lock();
    lv_async_call([weak, method] {
        if (auto self = weak.lock()) (static_cast<PcAppStreamScreen &>(*self).*method)();
    });
    lv_unlock();
}

void PcAppStreamScreen::outsideTouch(const bsp_touch_point_t *points, int count, void *arg) {
    static_cast<PcAppStreamScreen *>(arg)->handleTouch(points, count);
}

// A touch starting in the bottom-right corner is held back until it either
// swipes inward, opening the overlay, or turns out to be input.
void PcAppStreamScreen::handleTouch(const bsp_touch_point_t *points, int count) {
    const bsp_rotation_t rotation = input_rotation_;
    if (count == 0) {
        if (gesture_ == Gesture::Corner) {
            sendTouch(&touch_start_, 1);
            sendTouch(points, 0);
        } else if (gesture_ == Gesture::Input) {
            sendTouch(points, 0);
        } else if (gesture_ == Gesture::Menu && !moved_) {
            post(&PcAppStreamScreen::toggleOverlay);
        }
        gesture_ = Gesture::None;
    } else {
        if (gesture_ == Gesture::None) {
            touch_start_ = points[0];
            moved_ = false;
            restartIdle();
            if (!display_on_ && !overlay_open_ && !streaming_) {
                gesture_ = Gesture::Wake;
                ESP_LOGI(TAG, "display on (tap)");
                display_on_ = true;
                applyBrightness();
            } else if (overlay_open_ || !touch_) {
                gesture_ = Gesture::Menu;
            } else if (count == 1 && in_corner(stream_logical_point(points[0], rotation), rotation)) {
                gesture_ = Gesture::Corner;
            } else {
                gesture_ = Gesture::Input;
            }
        }
        moved_ |= abs(points[0].x - touch_start_.x) > kTapSlop ||
                  abs(points[0].y - touch_start_.y) > kTapSlop;
        if (gesture_ == Gesture::Corner) {
            const bsp_touch_point_t start = stream_logical_point(touch_start_, rotation);
            const bsp_touch_point_t now = stream_logical_point(points[0], rotation);
            const int left = start.x - now.x, up = start.y - now.y;
            if (count != 1 || left < -kTapSlop || up < -kTapSlop) {
                gesture_ = Gesture::Input;
                sendTouch(&touch_start_, 1);
            } else if (left >= kSwipeDistance || up >= kSwipeDistance) {
                gesture_ = Gesture::Swiped;
                post(&PcAppStreamScreen::openOverlay);
            }
        }
        if (gesture_ == Gesture::Input) sendTouch(points, count);
    }
}

void PcAppStreamScreen::sendTouch(const bsp_touch_point_t *points, int count) {
    if (service_->touch(points, count) || !trackpad_) return;
    const bsp_rotation_t rotation = input_rotation_;
    bsp_touch_point_t logical[Tab5Service::kMaxTouchPoints];
    count = std::min(count, Tab5Service::kMaxTouchPoints);
    for (int i = 0; i < count; i++) logical[i] = stream_logical_point(points[i], rotation);
    trackpad_->touch(logical, count);
}

void PcAppStreamScreen::setStatus(UsbStatus status) {
    status_ = status;
    const bool streaming = status == UsbStatus::Active;
    if (streaming_.exchange(streaming) && !streaming) restartIdle();
    updateWaiting();
    updateStatusRow();
}

void PcAppStreamScreen::rotate(bsp_rotation_t rotation) {
    const bool reopen = overlay_ != nullptr;
    if (reopen) closeOverlay();
    display_manager.set_rotation(lv_obj_get_display(root_), rotation);
    panel_idle_display_set_rotation(rotation);
    rotation_ = rotation;
    input_rotation_ = rotation;
    stream_waiting_layout(waiting_, rotation);
    if (reopen) openOverlay();
}

void PcAppStreamScreen::updateWaiting() {
    const bool connected = status_ != UsbStatus::Unconfigured;
    stream_waiting_set(waiting_, connected ? TABLER_DEVICE_DESKTOP : TABLER_USB,
                       connected ? "Open Tab5 Streamer and start stream on your computer"
                                 : "Connect Tab5 to your computer");
}

void PcAppStreamScreen::updateStatusRow() {
    if (status_row_.label) stream_panel_status_set(status_row_, status_);
}

void PcAppStreamScreen::toggleOverlay() {
    if (overlay_) {
        closeOverlay();
    } else {
        openOverlay();
    }
}

void PcAppStreamScreen::openOverlay() {
    if (overlay_) return;
    overlay_ = stream_overlay_open(rotation_);
    if (!overlay_) return;
    overlay_open_ = true;
    buildOverlay(lv_display_get_screen_active(overlay_));
    applyBrightness();
}

void PcAppStreamScreen::closeOverlay() {
    if (!overlay_) return;
    stream_overlay_close(overlay_);
    overlay_ = nullptr;
    overlay_open_ = false;
    status_row_ = {};
    brightness_ = {};
    lv_obj_invalidate(lv_obj_get_screen(waiting_.container));
    applyBrightness();
    restartIdle();
}

void PcAppStreamScreen::applyBrightness() {
    std::lock_guard<std::mutex> guard(brightness_lock_);
    bsp_display_set_brightness(display_on_ || overlay_open_ ? settings_display_brightness() : 0);
}

void PcAppStreamScreen::setBrightness(int percent) {
    settings_set_display_brightness(percent);
    settings_commit();
    applyBrightness();
    percent = settings_display_brightness();
    service_->brightness_changed(percent);
    if (brightness_.slider) stream_panel_brightness_set(brightness_, percent);
}

void PcAppStreamScreen::restartIdle() {
    if (idle_timer_) xTimerReset(idle_timer_, 0);
}

void PcAppStreamScreen::idleTimeout(TimerHandle_t timer) {
    auto *self = static_cast<PcAppStreamScreen *>(pvTimerGetTimerID(timer));
    if (!self->idle_enabled_ || self->streaming_ || self->overlay_open_ || !self->display_on_) {
        return;
    }
    ESP_LOGI(TAG, "display off (idle)");
    self->display_on_ = false;
    self->applyBrightness();
}

void PcAppStreamScreen::buildOverlay(lv_obj_t *screen) {
    lv_obj_clean(screen);
    lv_obj_set_style_pad_all(screen, 0, 0);
    // Both buttons delete the overlay display their event comes from.
    auto contents = stream_panel_build(screen, PcAppScreen::kTitle,
                                       [this] { post(&PcAppStreamScreen::closeOverlay); });

    status_row_ = stream_panel_status_build(contents);
    updateStatusRow();
    brightness_ = stream_panel_brightness_build(contents, [this](int percent) {
        service_->brightness_changed(percent);
    });
    stream_panel_exit_build(contents);
}
