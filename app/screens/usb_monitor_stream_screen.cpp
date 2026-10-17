/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb_monitor_stream_screen.hpp"
#include <cstdlib>
#include "display/panel.hpp"
#include "display_manager.hpp"
#include "esp_log.h"
#include "resources.h"
#include "stream/stream_overlay.hpp"
#include "ui_orientation.hpp"
#include "usb/usb.hpp"
#include "usb_monitor_screen.hpp"

static const char *TAG = "usb_monitor_stream";

static constexpr int kTapSlop = 24;
static constexpr TickType_t kIdleTimeout = pdMS_TO_TICKS(3 * 60 * 1000);

UsbMonitorStreamScreen::~UsbMonitorStreamScreen() {
    if (idle_timer_) xTimerDelete(idle_timer_, portMAX_DELAY);
}

void UsbMonitorStreamScreen::build() {
    lv_obj_set_style_bg_color(root_, lv_color_black(), 0);
    lv_obj_set_style_bg_opa(root_, LV_OPA_COVER, 0);
    rotation_ = ui_orientation_current();
}

void UsbMonitorStreamScreen::onEnter() {
    ui_orientation_set_listener([](bsp_rotation_t rotation, void *arg) {
        static_cast<UsbMonitorStreamScreen *>(arg)->rotate(rotation);
    }, this);
    waiting_display_ = panel_idle_display_create(rotation_);
    waiting_ = stream_waiting_build(
        waiting_display_ ? lv_display_get_screen_active(waiting_display_) : root_,
        "Tap to show menu");
    stream_waiting_layout(waiting_, rotation_);
    idle_timer_ = xTimerCreate("usb_monitor_idle", kIdleTimeout, pdFALSE, this, idleTimeout);
    if (!idle_timer_) ESP_LOGE(TAG, "no memory for the idle timer");
    idle_enabled_ = true;
    restartIdle();
    display_manager.set_outside_touch_callback(outsideTouch, this);
    startUsb();
    updateWaiting();
}

void UsbMonitorStreamScreen::onExit() {
    display_manager.set_outside_touch_callback(nullptr);
    idle_enabled_ = false;
    if (idle_timer_) xTimerStop(idle_timer_, portMAX_DELAY);
    display_on_ = true;
    closeOverlay();
    applyBrightness();
    panel_idle_display_delete();
    waiting_display_ = nullptr;
    usb_stop();
    profile_.reset();
    ui_orientation_set_listener(nullptr, nullptr);
}

void UsbMonitorStreamScreen::post(std::weak_ptr<Screen> weak,
                                  std::function<void(UsbMonitorStreamScreen &)> fn) {
    lv_lock();
    lv_async_call([weak, fn = std::move(fn)] {
        if (auto self = weak.lock()) fn(static_cast<UsbMonitorStreamScreen &>(*self));
    });
    lv_unlock();
}

void UsbMonitorStreamScreen::startUsb() {
    if (adapter_ != UsbMonitorAdapter::Gud) {
        ESP_LOGE(TAG, "adapter %d is not supported", (int)adapter_);
        return;
    }
    std::weak_ptr<Screen> weak = weak_from_this();
    GudProfile::Listeners listeners;
    listeners.status = [weak](UsbStatus status) {
        post(weak, [status](UsbMonitorStreamScreen &self) { self.setStatus(status); });
    };
    listeners.display_power = [weak](bool on) {
        post(weak, [on](UsbMonitorStreamScreen &self) { self.setDisplayPower(on); });
    };
    listeners.brightness = [weak](int percent) {
        post(weak, [percent](UsbMonitorStreamScreen &self) { self.setBrightness(percent); });
    };
    auto profile = std::make_shared<GudProfile>(std::move(listeners), rotation_,
                                                settings_display_brightness());
    const esp_err_t err = profile->start();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "gud: %s", esp_err_to_name(err));
        return;
    }
    profile_ = profile;
    usb_start(std::move(profile));
}

void UsbMonitorStreamScreen::setStatus(UsbStatus status) {
    status_ = status;
    const bool streaming = status == UsbStatus::Active;
    if (streaming_.exchange(streaming) && !streaming) restartIdle();
    updateWaiting();
    if (status_row_.label) stream_panel_status_set(status_row_, status_);
}

void UsbMonitorStreamScreen::rotate(bsp_rotation_t rotation) {
    const bool reopen = overlay_ != nullptr;
    if (reopen) closeOverlay();
    display_manager.set_rotation(lv_obj_get_display(root_), rotation);
    panel_idle_display_set_rotation(rotation);
    rotation_ = rotation;
    stream_waiting_layout(waiting_, rotation);
    if (profile_) profile_->rotate(rotation);
    if (reopen) openOverlay();
}

void UsbMonitorStreamScreen::updateWaiting() {
    if (!profile_) {
        stream_waiting_set(waiting_, TABLER_USB, "This adapter is not available yet");
    } else if (status_ == UsbStatus::Unconfigured) {
        stream_waiting_set(waiting_, TABLER_USB, "Connect Tab5 to your computer");
    } else {
        stream_waiting_set(waiting_, TABLER_DEVICE_DESKTOP,
                           "Waiting for your computer to use Tab5 as a display");
    }
}

void UsbMonitorStreamScreen::applyBrightness() {
    std::lock_guard<std::mutex> guard(brightness_lock_);
    bsp_display_set_brightness(display_on_ || overlay_open_ ? settings_display_brightness() : 0);
}

void UsbMonitorStreamScreen::setBrightness(int percent) {
    settings_set_display_brightness(percent);
    settings_commit();
    applyBrightness();
    if (brightness_.slider) stream_panel_brightness_set(brightness_, settings_display_brightness());
}

void UsbMonitorStreamScreen::setDisplayPower(bool on) {
    display_on_ = on;
    applyBrightness();
    if (on) restartIdle();
}

void UsbMonitorStreamScreen::restartIdle() {
    if (idle_timer_) xTimerReset(idle_timer_, 0);
}

void UsbMonitorStreamScreen::idleTimeout(TimerHandle_t timer) {
    auto *self = static_cast<UsbMonitorStreamScreen *>(pvTimerGetTimerID(timer));
    if (!self->idle_enabled_ || self->streaming_ || self->overlay_open_ || !self->display_on_) {
        return;
    }
    ESP_LOGI(TAG, "display off (idle)");
    self->display_on_ = false;
    self->applyBrightness();
}

void UsbMonitorStreamScreen::toggleOverlay() {
    if (overlay_) {
        closeOverlay();
    } else {
        openOverlay();
    }
}

void UsbMonitorStreamScreen::openOverlay() {
    if (overlay_) return;
    overlay_ = stream_overlay_open(rotation_);
    if (!overlay_) return;
    overlay_open_ = true;
    buildOverlay(lv_display_get_screen_active(overlay_));
    applyBrightness();
}

void UsbMonitorStreamScreen::closeOverlay() {
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

void UsbMonitorStreamScreen::buildOverlay(lv_obj_t *screen) {
    lv_obj_clean(screen);
    lv_obj_set_style_pad_all(screen, 0, 0);
    // Both buttons delete the overlay display their event comes from.
    auto contents = stream_panel_build(screen, UsbMonitorScreen::kTitle, [this] {
        post(weak_from_this(), [](UsbMonitorStreamScreen &self) { self.closeOverlay(); });
    });
    status_row_ = stream_panel_status_build(contents);
    stream_panel_status_set(status_row_, status_);
    brightness_ = stream_panel_brightness_build(contents, [](int) {});
    stream_panel_exit_build(contents);
}

void UsbMonitorStreamScreen::outsideTouch(const bsp_touch_point_t *points, int count, void *arg) {
    static_cast<UsbMonitorStreamScreen *>(arg)->handleTouch(points, count);
}

void UsbMonitorStreamScreen::handleTouch(const bsp_touch_point_t *points, int count) {
    if (count == 0) {
        if (touching_ && !moved_ && !woke_) {
            post(weak_from_this(), [](UsbMonitorStreamScreen &self) { self.toggleOverlay(); });
        }
        touching_ = false;
        return;
    }
    if (!touching_) {
        touching_ = true;
        touch_start_ = points[0];
        moved_ = false;
        woke_ = false;
        restartIdle();
        if (!display_on_ && !overlay_open_ && !streaming_) {
            ESP_LOGI(TAG, "display on (tap)");
            woke_ = true;
            display_on_ = true;
            applyBrightness();
        }
    }
    moved_ |= abs(points[0].x - touch_start_.x) > kTapSlop ||
              abs(points[0].y - touch_start_.y) > kTapSlop;
}
