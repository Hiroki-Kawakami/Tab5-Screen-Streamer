/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <functional>
#include <memory>
#include <mutex>
#include "bsp.h"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "protocols/displaylink/displaylink_profile.hpp"
#include "protocols/gud/gud_profile.hpp"
#include "screen.hpp"
#include "settings.hpp"
#include "stream/stream_panel.hpp"
#include "stream/stream_waiting.hpp"

class UsbMonitorStreamScreen : public Screen {
public:
    explicit UsbMonitorStreamScreen(UsbMonitorAdapter adapter) : adapter_(adapter) {}
    ~UsbMonitorStreamScreen() override;
    void build() override;
    void onEnter() override;
    void onExit() override;

private:
    UsbMonitorAdapter adapter_;
    std::shared_ptr<GudProfile> gud_;
    std::shared_ptr<DisplayLinkProfile> displaylink_;
    bsp_rotation_t usb_rotation_ = BSP_ROTATION_0;
    UsbStatus status_ = UsbStatus::Unconfigured;
    std::atomic<bool> streaming_ = false;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;

    lv_display_t *waiting_display_ = nullptr;
    StreamWaiting waiting_ = {};

    lv_display_t *overlay_ = nullptr;
    StreamStatusRow status_row_ = {};
    StreamBrightness brightness_ = {};

    std::atomic<bool> overlay_open_ = false;
    std::mutex brightness_lock_;
    std::atomic<bool> display_on_ = true;
    TimerHandle_t idle_timer_ = nullptr;
    std::atomic<bool> idle_enabled_ = false;
    bool touching_ = false;
    bool moved_ = false;
    bool woke_ = false;
    bsp_touch_point_t touch_start_ = {};

    static void post(std::weak_ptr<Screen> weak,
                     std::function<void(UsbMonitorStreamScreen &)> fn);
    void startUsb();
    void setStatus(UsbStatus status);
    void rotate(bsp_rotation_t rotation);
    void updateWaiting();
    void applyBrightness();
    void setBrightness(int percent);
    void setDisplayPower(bool on);
    void restartIdle();
    static void idleTimeout(TimerHandle_t timer);
    void toggleOverlay();
    void openOverlay();
    void closeOverlay();
    void buildOverlay(lv_obj_t *screen);
    void handleTouch(const bsp_touch_point_t *points, int count);
    static void outsideTouch(const bsp_touch_point_t *points, int count, void *arg);
};
