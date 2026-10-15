/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "bsp.h"
#include "screen.hpp"
#include "usb/usb.hpp"

class PcAppStreamScreen : public Screen {
public:
    PcAppStreamScreen(bool audio, bool hid) : audio_(audio), hid_(hid) {}
    void build() override;
    void onEnter() override;
    void onExit() override;

private:
    bool audio_;
    bool hid_;
    UsbStatus status_ = UsbStatus::Unconfigured;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;

    lv_obj_t *waiting_ = nullptr;
    lv_obj_t *waiting_icon_ = nullptr;
    lv_obj_t *waiting_message_ = nullptr;

    lv_display_t *overlay_ = nullptr;
    lv_obj_t *status_dot_ = nullptr;
    lv_obj_t *status_label_ = nullptr;

    bool touching_ = false;
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
    static void outsideTouch(const bsp_touch_point_t *points, int count, void *arg);
};
