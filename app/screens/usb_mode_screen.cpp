/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb_mode_screen.hpp"
#include "home_screen.hpp"
#include "screen_manager.hpp"

void UsbModeScreen::build() {
    createNavigation(title_, LV_NAVIGATION_STYLE_DEFAULT | LV_NAVIGATION_STYLE_BACK);
    auto label = lv_label_create(contents_);
    lv_label_set_text(label, "Connect the USB-C port to a PC.");
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_BODY);
}

void UsbModeScreen::back() {
    screen_manager.load(std::make_shared<HomeScreen>());
}

void UsbModeScreen::onEnter() {
    usb_start(id_, options_);
}

void UsbModeScreen::onExit() {
    usb_stop();
}
