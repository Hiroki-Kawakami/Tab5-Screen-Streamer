/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <functional>
#include "lvgl.h"
#include "screens/setup/settings_widgets.hpp"
#include "usb/usb_profile.hpp"

inline constexpr SettingColors kPanelColors = {
    .page_bg    = 0x101010,
    .section_bg = 0x1e1e1e,
    .value      = 0x9e9e9e,
    .separator  = 0x3a3a3a,
    .track      = 0x404040,
    .accent     = 0x2196f3,
};

/* Dark panel over the stream: title row with a close button, and the contents
 * container it returns, which the caller fills with sections. */
lv_obj_t *stream_panel_build(lv_obj_t *root, const char *title, std::function<void()> on_close);

struct StreamStatusRow {
    lv_obj_t *dot;
    lv_obj_t *label;
};

StreamStatusRow stream_panel_status_build(lv_obj_t *contents);
void stream_panel_status_set(const StreamStatusRow &row, UsbStatus status);

struct StreamBrightness {
    lv_obj_t *slider;
    lv_obj_t *value;
};

/* `changed` runs with every value the slider takes, once it is in the
 * settings. */
StreamBrightness stream_panel_brightness_build(lv_obj_t *contents,
                                               std::function<void(int)> changed);
void stream_panel_brightness_set(const StreamBrightness &brightness, int percent);

/* Pinned to the bottom of the contents. */
void stream_panel_exit_build(lv_obj_t *contents);
