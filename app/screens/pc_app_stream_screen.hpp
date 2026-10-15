/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <memory>
#include <mutex>
#include "bsp.h"
#include "input/trackpad.hpp"
#include "protocols/tab5/tab5_profile.hpp"
#include "protocols/tab5/tab5_service.hpp"
#include "screen.hpp"

class PcAppStreamScreen : public Screen {
public:
    PcAppStreamScreen(bool audio, bool hid, bool touch)
        : audio_(audio), hid_(hid), touch_(touch) {}
    void build() override;
    void onEnter() override;
    void onExit() override;

private:
    bool audio_;
    bool hid_;
    bool touch_;
    std::shared_ptr<Tab5Service> service_;
    std::mutex profile_lock_;
    std::shared_ptr<Tab5Profile> profile_;
    bool keyboard_enumerated_ = false;
    UsbStatus status_ = UsbStatus::Unconfigured;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;
    std::atomic<bsp_rotation_t> input_rotation_ = BSP_ROTATION_0;

    lv_obj_t *waiting_ = nullptr;
    lv_obj_t *waiting_icon_ = nullptr;
    lv_obj_t *waiting_message_ = nullptr;

    lv_display_t *overlay_ = nullptr;
    lv_obj_t *status_dot_ = nullptr;
    lv_obj_t *status_label_ = nullptr;

    enum class Gesture : uint8_t { None, Corner, Swiped, Input, Overlay };

    std::unique_ptr<Trackpad> trackpad_;
    std::mutex keyboard_lock_;
    bool keyboard_hid_ = false;
    std::atomic<bool> overlay_open_ = false;
    Gesture gesture_ = Gesture::None;
    bool moved_ = false;
    bsp_touch_point_t touch_start_ = {};

    void post(void (PcAppStreamScreen::*method)());
    void setStatus(UsbStatus status);
    void rotate(bsp_rotation_t rotation);
    void layoutWaiting();
    void updateWaiting();
    void updateStatusRow();
    void toggleOverlay();
    void openOverlay();
    void closeOverlay();
    void buildOverlay(lv_obj_t *screen);
    void startUsb();
    void keyboardChanged();
    void inputModeChanged(tab5::InputTarget target, tab5::InputMode mode);
    void keyboardReport(const usb_device::KeyboardReport &report);
    void mouseReport(const usb_device::MouseReport &report);
    void handleTouch(const bsp_touch_point_t *points, int count);
    void sendTouch(const bsp_touch_point_t *points, int count);
    static void outsideTouch(const bsp_touch_point_t *points, int count, void *arg);
    static void keyboardEvent(const bsp_keyboard_event_t *event, void *arg);
};
