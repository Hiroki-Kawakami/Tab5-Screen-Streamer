/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "home_screen.hpp"
#include "screen_manager.hpp"
#include "usb_mode_screen.hpp"

void HomeScreen::build() {
    createNavigation("Tab5 Screen Streamer");

    auto button = lv_button_create(contents_, LV_BUTTON_STYLE_PRIMARY);
    lv_obj_set_size(button, LV_PCT(100), 96);
    lv_button_set_text(button, "USB Stream");
    lv_obj_add_event_fn(button, LV_EVENT_CLICKED, [](lv_event_t *) {
        screen_manager.load(std::make_shared<UsbModeScreen>(UsbProfileId::Tab5, "USB Stream"));
    });
}
