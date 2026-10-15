/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "trackpad.hpp"
#include <algorithm>
#include <cstdlib>
#include "esp_log.h"
#include "esp_timer.h"

static const char *TAG = "trackpad";

namespace {

constexpr uint8_t kLeft = 1 << 0;
constexpr uint8_t kRight = 1 << 1;
constexpr int kTapSlop = 24;
constexpr int64_t kTapMaxUs = 250 * 1000;
constexpr TickType_t kClickDelay = pdMS_TO_TICKS(200);
constexpr float kGain = 1.5f;
constexpr float kScrollStep = 32.0f;

}

Trackpad::Trackpad(Sender send) : send_(std::move(send)) {}

Trackpad::~Trackpad() {
    if (click_timer_) xTimerDelete(click_timer_, portMAX_DELAY);
}

esp_err_t Trackpad::start() {
    click_timer_ = xTimerCreate("trackpad", kClickDelay, pdFALSE, this, click_timeout);
    if (!click_timer_) ESP_LOGE(TAG, "no memory for the click timer");
    return click_timer_ ? ESP_OK : ESP_ERR_NO_MEM;
}

void Trackpad::set_active(bool active) {
    std::lock_guard<std::mutex> guard(lock_);
    if (active == active_) return;
    active_ = active;
    if (active) return;
    xTimerStop(click_timer_, 0);
    click_pending_ = false;
    down_ = false;
    drag_ = false;
    if (buttons_) set_buttons(0);
}

void Trackpad::touch(const bsp_touch_point_t *points, int count) {
    std::lock_guard<std::mutex> guard(lock_);
    if (!active_) return;
    const int64_t now = esp_timer_get_time();
    if (count <= 0) {
        if (down_) end(now);
        return;
    }
    int x = 0, y = 0;
    for (int i = 0; i < count; i++) {
        x += points[i].x;
        y += points[i].y;
    }
    x /= count;
    y /= count;
    if (!down_) {
        begin(x, y, count, now);
        return;
    }
    if (count != fingers_) {
        fingers_ = count;
        max_fingers_ = std::max(max_fingers_, count);
        last_x_ = x;
        last_y_ = y;
        if (drag_ && !(buttons_ & kLeft)) {
            drag_ = false;
            click(kLeft);
        }
        return;
    }
    const int dx = x - last_x_, dy = y - last_y_;
    last_x_ = x;
    last_y_ = y;
    if (!moved_ && (abs(x - start_x_) > kTapSlop || abs(y - start_y_) > kTapSlop)) {
        moved_ = true;
        if (drag_ && count == 1) set_buttons(buttons_ | kLeft);
    }
    if (count == 1) {
        move(dx, dy);
    } else if (count == 2 && moved_) {
        scroll(dy);
    }
}

void Trackpad::begin(int x, int y, int count, int64_t now) {
    down_ = true;
    fingers_ = max_fingers_ = count;
    start_us_ = now;
    start_x_ = last_x_ = x;
    start_y_ = last_y_ = y;
    moved_ = false;
    rest_x_ = rest_y_ = rest_wheel_ = 0;
    drag_ = click_pending_ && count == 1;
    if (drag_) {
        xTimerStop(click_timer_, 0);
        click_pending_ = false;
    } else {
        flush_click();
    }
}

void Trackpad::end(int64_t now) {
    down_ = false;
    const bool tap = !moved_ && now - start_us_ < kTapMaxUs;
    if (drag_) {
        drag_ = false;
        if (buttons_ & kLeft) {
            set_buttons(buttons_ & ~kLeft);
            return;
        }
        click(kLeft);
        if (tap && max_fingers_ == 1) click(kLeft);
        return;
    }
    if (!tap) return;
    if (max_fingers_ == 1) {
        click_pending_ = true;
        xTimerStart(click_timer_, 0);
    } else if (max_fingers_ == 2) {
        click(kRight);
    }
}

void Trackpad::click_timeout(TimerHandle_t timer) {
    auto *self = static_cast<Trackpad *>(pvTimerGetTimerID(timer));
    std::lock_guard<std::mutex> guard(self->lock_);
    self->flush_click();
}

void Trackpad::flush_click() {
    if (!click_pending_) return;
    click_pending_ = false;
    click(kLeft);
}

void Trackpad::move(int dx, int dy) {
    rest_x_ += dx * kGain;
    rest_y_ += dy * kGain;
    int x = static_cast<int>(rest_x_);
    int y = static_cast<int>(rest_y_);
    rest_x_ -= x;
    rest_y_ -= y;
    while (x || y) {
        const int step_x = std::clamp(x, -127, 127);
        const int step_y = std::clamp(y, -127, 127);
        send(step_x, step_y, 0);
        x -= step_x;
        y -= step_y;
    }
}

void Trackpad::scroll(int dy) {
    rest_wheel_ += dy / kScrollStep;
    const int wheel = static_cast<int>(rest_wheel_);
    rest_wheel_ -= wheel;
    if (wheel) send(0, 0, std::clamp(wheel, -127, 127));
}

void Trackpad::click(uint8_t button) {
    set_buttons(buttons_ | button);
    set_buttons(buttons_ & ~button);
}

void Trackpad::set_buttons(uint8_t buttons) {
    buttons_ = buttons;
    send(0, 0, 0);
}

void Trackpad::send(int x, int y, int wheel) {
    const usb_device::MouseReport report = {buttons_, static_cast<int8_t>(x),
                                            static_cast<int8_t>(y), static_cast<int8_t>(wheel)};
    send_(report);
}
