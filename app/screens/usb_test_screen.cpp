/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb_test_screen.hpp"
#include "home_screen.hpp"
#include "screen_manager.hpp"
#include "usb/usb.hpp"

void UsbTestScreen::build() {
    createNavigation("USB Test", LV_NAVIGATION_STYLE_DEFAULT | LV_NAVIGATION_STYLE_BACK);
    auto label = lv_label_create(contents_);
    lv_label_set_text(label, "Connect the USB-C port to a PC.");
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_BODY);
}

void UsbTestScreen::back() {
    screen_manager.load(std::make_shared<HomeScreen>());
}

void UsbTestScreen::onEnter() {
    usb_start(UsbProfileId::Test);
}

void UsbTestScreen::onExit() {
    usb_stop();
}
