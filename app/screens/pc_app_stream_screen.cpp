/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "pc_app_stream_screen.hpp"
#include <cstdlib>
#include "display/panel.hpp"
#include "display_manager.hpp"
#include "esp_log.h"
#include "home_screen.hpp"
#include "pc_app_screen.hpp"
#include "resources.h"
#include "screen_manager.hpp"
#include "settings.hpp"
#include "stream_panel.hpp"
#include "ui_orientation.hpp"
#include "usb/usb.hpp"
#include "widgets.hpp"

static const char *TAG = "pc_app_stream";

namespace {

constexpr int32_t kLandscapePanelWidth = 560;
constexpr int32_t kPortraitPanelHeight = 640;
constexpr int kOverlayBufferLines = 32;
constexpr int kTapSlop = 24;
constexpr int32_t kStatusDotSize = 16;
constexpr int32_t kExitHeight = 72;
constexpr uint32_t kHintColor = 0x808080;

bool transposed(bsp_rotation_t rotation) {
    return rotation == BSP_ROTATION_90 || rotation == BSP_ROTATION_270;
}

bsp_size_t logical_size(bsp_rotation_t rotation) {
    const bsp_size_t size = panel_size();
    return transposed(rotation) ? bsp_size_t{size.height, size.width} : size;
}

bool landscape(bsp_rotation_t rotation) {
    const bsp_size_t size = logical_size(rotation);
    return size.width > size.height;
}

bsp_rect_t overlay_area(bsp_rotation_t rotation) {
    const bsp_size_t size = logical_size(rotation);
    if (landscape(rotation)) {
        return {{size.width - kLandscapePanelWidth, 0}, {kLandscapePanelWidth, size.height}};
    }
    return {{0, size.height - kPortraitPanelHeight}, {size.width, kPortraitPanelHeight}};
}

bsp_rect_t stream_area(bsp_rotation_t rotation) {
    const bsp_size_t size = logical_size(rotation);
    if (landscape(rotation)) return {{0, 0}, {size.width - kLandscapePanelWidth, size.height}};
    return {{0, 0}, {size.width, size.height - kPortraitPanelHeight}};
}

bsp_rect_t native_rect(const bsp_rect_t &rect, bsp_rotation_t rotation) {
    const bsp_size_t size = logical_size(rotation);
    const int x = rect.origin.x, y = rect.origin.y;
    const int w = rect.size.width, h = rect.size.height;
    switch (rotation) {
        case BSP_ROTATION_90: return {{y, size.width - x - w}, {h, w}};
        case BSP_ROTATION_180: return {{size.width - x - w, size.height - y - h}, {w, h}};
        case BSP_ROTATION_270: return {{size.height - y - h, x}, {h, w}};
        default: return rect;
    }
}

}

void PcAppStreamScreen::build() {
    lv_obj_set_style_bg_color(root_, lv_color_black(), 0);
    lv_obj_set_style_bg_opa(root_, LV_OPA_COVER, 0);
    lv_obj_set_style_text_color(root_, lv_color_white(), 0);
    lv_obj_set_scrollable(root_, false);
    lv_obj_add_event_fn(root_, LV_EVENT_CLICKED, [this](lv_event_t *) { toggleOverlay(); });

    waiting_ = lv_container_create(root_, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_clickable(waiting_, false);
    lv_obj_set_style_pad_row(waiting_, 16, 0);

    waiting_icon_ = lv_label_create(waiting_);
    lv_obj_set_style_text_font(waiting_icon_, &icon_96, 0);
    waiting_message_ = lv_label_create(waiting_);
    lv_obj_set_font_role(waiting_message_, LV_WIDGETS_FONT_HEADING);
    auto hint = lv_label_create(waiting_);
    lv_obj_set_font_role(hint, LV_WIDGETS_FONT_BODY);
    lv_obj_set_style_text_color(hint, lv_color_hex(kHintColor), 0);
    lv_label_set_text(hint, "Tap to show menu");

    rotation_ = ui_orientation_current();
    layoutWaiting();
    updateWaiting();
}

void PcAppStreamScreen::onEnter() {
    ui_orientation_set_listener([](bsp_rotation_t rotation, void *arg) {
        static_cast<PcAppStreamScreen *>(arg)->rotate(rotation);
    }, this);
    display_manager.set_outside_touch_callback(outsideTouch, this);

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
    bsp_keyboard_set_event_cb(keyboardEvent, this);
    startUsb();
}

void PcAppStreamScreen::onExit() {
    bsp_keyboard_set_event_cb(nullptr, nullptr);
    closeOverlay();
    usb_stop();
    std::shared_ptr<Tab5Profile> profile;
    {
        std::lock_guard<std::mutex> guard(profile_lock_);
        profile = std::move(profile_);
    }
    service_.reset();
    display_manager.set_outside_touch_callback(nullptr);
    ui_orientation_set_listener(nullptr, nullptr);
}

// The previous profile is let go before the next one is queued, so that it
// detaches from the service before the next one attaches.
void PcAppStreamScreen::startUsb() {
    keyboard_enumerated_ = hid_ && bsp_keyboard_is_connected();
    auto profile = std::make_shared<Tab5Profile>(service_, audio_, keyboard_enumerated_);
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
    usb_device::KeyboardReport report = {};
    report.modifiers = event->hid.modifiers;
    report.keys[0] = event->hid.keycode;
    std::lock_guard<std::mutex> guard(self->profile_lock_);
    if (self->profile_) self->profile_->keyboard_report(report);
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
    auto *self = static_cast<PcAppStreamScreen *>(arg);
    if (count > 0) {
        if (!self->touching_) {
            self->touching_ = true;
            self->moved_ = false;
            self->touch_start_ = points[0];
        } else if (abs(points[0].x - self->touch_start_.x) > kTapSlop ||
                   abs(points[0].y - self->touch_start_.y) > kTapSlop) {
            self->moved_ = true;
        }
        return;
    }
    if (!self->touching_) return;
    self->touching_ = false;
    if (!self->moved_) self->post(&PcAppStreamScreen::toggleOverlay);
}

void PcAppStreamScreen::setStatus(UsbStatus status) {
    status_ = status;
    updateWaiting();
    updateStatusRow();
    // The UI draws over the overlay, both when it comes back after a stream
    // and when the waiting text under the overlay changes.
    if (overlay_ && !panel_acquired()) {
        lv_refr_now(lv_obj_get_display(root_));
        lv_obj_invalidate(lv_display_get_screen_active(overlay_));
    }
}

void PcAppStreamScreen::rotate(bsp_rotation_t rotation) {
    const bool reopen = overlay_ != nullptr;
    if (reopen) closeOverlay();
    display_manager.set_rotation(lv_obj_get_display(root_), rotation);
    rotation_ = rotation;
    layoutWaiting();
    if (reopen) openOverlay();
}

void PcAppStreamScreen::layoutWaiting() {
    const bsp_size_t size = logical_size(rotation_);
    lv_obj_set_size(waiting_, size.width, size.height);
}

void PcAppStreamScreen::updateWaiting() {
    const bool connected = status_ != UsbStatus::Unconfigured;
    lv_label_set_text(waiting_icon_, connected ? TABLER_DEVICE_DESKTOP : TABLER_USB);
    lv_label_set_text(waiting_message_, connected ? "Open Tab5 Streamer on your computer"
                                                  : "Connect Tab5 to your computer");
}

void PcAppStreamScreen::updateStatusRow() {
    if (!status_label_) return;
    const char *text = "Not connected";
    uint32_t color = 0x808080;
    if (status_ == UsbStatus::Configured) {
        text = "Connected";
        color = 0xffc107;
    } else if (status_ == UsbStatus::Active) {
        text = "Streaming";
        color = 0x4caf50;
    }
    lv_label_set_text(status_label_, text);
    lv_obj_set_style_bg_color(status_dot_, lv_color_hex(color), 0);
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
    const bsp_rect_t stream = native_rect(stream_area(rotation_), rotation_);
    if (panel_set_clip(&stream) != ESP_OK) {
        ESP_LOGE(TAG, "clip to %dx%d at %d,%d failed", stream.size.width, stream.size.height,
                 stream.origin.x, stream.origin.y);
        return;
    }

    DisplayManagerConfig config = {};
    config.present_mode = DisplayPresentMode::Immediate;
    config.render_mode = DisplayRenderMode::Partial;
    config.color_format = LV_COLOR_FORMAT_RGB565;
    config.make_default = false;
    config.viewport.output_area = native_rect(overlay_area(rotation_), rotation_);
    config.viewport.rotation = rotation_;
    config.buffer.lines = kOverlayBufferLines;
    lv_display_t *overlay = nullptr;
    const esp_err_t err = display_manager.create_display(config, &overlay);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "overlay display: %s", esp_err_to_name(err));
        panel_set_clip(nullptr);
        return;
    }

    overlay_ = overlay;
    buildOverlay(lv_display_get_screen_active(overlay_));
}

void PcAppStreamScreen::closeOverlay() {
    if (!overlay_) return;
    display_manager.delete_display(overlay_);
    overlay_ = nullptr;
    status_dot_ = nullptr;
    status_label_ = nullptr;
    panel_set_clip(nullptr);
    lv_obj_invalidate(root_);
}

void PcAppStreamScreen::buildOverlay(lv_obj_t *screen) {
    lv_obj_clean(screen);
    lv_obj_set_style_pad_all(screen, 0, 0);
    // Both buttons delete the overlay display their event comes from.
    auto contents = stream_panel_build(screen, PcAppScreen::kTitle,
                                       [this] { post(&PcAppStreamScreen::closeOverlay); });

    auto section = lv_setting_section_create(contents, "Status", &kPanelColors);
    auto row = lv_container_create(section, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 16, 0);
    status_dot_ = lv_obj_create(row);
    lv_obj_remove_style_all(status_dot_);
    lv_obj_set_size(status_dot_, kStatusDotSize, kStatusDotSize);
    lv_obj_set_style_radius(status_dot_, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_opa(status_dot_, LV_OPA_COVER, 0);
    status_label_ = lv_label_create(row);
    lv_obj_set_font_role(status_label_, LV_WIDGETS_FONT_BODY);
    updateStatusRow();

    section = lv_setting_section_create(contents, "Display", &kPanelColors);
    row = lv_setting_row_create(section, "Brightness");
    auto value = lv_setting_value_create(row, &kPanelColors);
    lv_label_set_text_fmt(value, "%d%%", settings_display_brightness());
    auto brightness = lv_setting_slider_create(section, kMinDisplayBrightness, 100,
                                               settings_display_brightness(), &kPanelColors);
    lv_obj_add_event_fn(brightness, LV_EVENT_VALUE_CHANGED, [brightness, value](lv_event_t *) {
        const int percent = lv_slider_get_value(brightness);
        settings_set_display_brightness(percent);
        lv_label_set_text_fmt(value, "%d%%", percent);
    });
    lv_obj_add_event_fn(brightness, LV_EVENT_RELEASED, [](lv_event_t *) { settings_commit(); });

    lv_spacer_create(contents, LV_PCT(100), 0, 1);

    auto exit = lv_button_create(contents, LV_BUTTON_STYLE_SECONDARY);
    lv_obj_set_size(exit, LV_PCT(100), kExitHeight);
    lv_obj_set_style_bg_color(exit, lv_color_hex(kPanelColors.track), 0);
    lv_obj_set_style_text_color(exit, lv_color_white(), 0);
    lv_button_set_text(exit, "Exit to Home", lv_widgets_resolved_font(LV_WIDGETS_FONT_HEADING));
    lv_obj_add_event_fn(exit, LV_EVENT_CLICKED, [](lv_event_t *) {
        lv_async_call([] {
            settings_set_last_mode(AppMode::None);
            settings_commit();
            screen_manager.load(std::make_shared<HomeScreen>());
        });
    });
}
