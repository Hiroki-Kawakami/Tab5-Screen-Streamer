/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "home_screen.hpp"

void HomeScreen::build() {
    auto label = lv_label_create(root_);
    lv_label_set_text(label, "Tab5 Screen Streamer");
    lv_obj_set_style_text_font(label, &lv_font_montserrat_48, 0);
    lv_obj_center(label);
}
