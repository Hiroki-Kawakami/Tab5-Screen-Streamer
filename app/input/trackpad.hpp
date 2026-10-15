/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <functional>
#include <mutex>
#include "bsp.h"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "usb_device_hid.hpp"

/* Touch gestures as boot mouse reports. Reports are sent under the
 * trackpad's lock, from the touch task or its timer. */
class Trackpad {
public:
    using Sender = std::function<void(const usb_device::MouseReport &report)>;

    explicit Trackpad(Sender send);
    ~Trackpad();
    esp_err_t start();
    /* Inactive, touches are ignored; going inactive releases the buttons. */
    void set_active(bool active);
    /* Points are in the coordinates the user sees. */
    void touch(const bsp_touch_point_t *points, int count);

private:
    static void click_timeout(TimerHandle_t timer);
    void begin(int x, int y, int count, int64_t now);
    void end(int64_t now);
    void move(int dx, int dy);
    void scroll(int dy);
    void click(uint8_t button);
    void set_buttons(uint8_t buttons);
    void flush_click();
    void send(int x, int y, int wheel);

    Sender send_;
    std::mutex lock_;
    TimerHandle_t click_timer_ = nullptr;
    bool active_ = false;
    uint8_t buttons_ = 0;
    bool down_ = false;
    int fingers_ = 0;
    int max_fingers_ = 0;
    int64_t start_us_ = 0;
    int start_x_ = 0, start_y_ = 0;
    int last_x_ = 0, last_y_ = 0;
    bool moved_ = false;
    bool drag_ = false;
    bool click_pending_ = false;
    float rest_x_ = 0, rest_y_ = 0, rest_wheel_ = 0;
};
