/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <functional>
#include "lvgl.h"
#include "settings_widgets.hpp"

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
