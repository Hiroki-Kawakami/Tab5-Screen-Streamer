/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include <functional>
#include "lvgl.h"

/* A null SettingColors leaves the widget on the LVGL theme's light palette. */
struct SettingColors {
    uint32_t page_bg;
    uint32_t section_bg;
    uint32_t value;
    uint32_t separator;
    uint32_t track;
    uint32_t accent;
};

void lv_setting_page_style(lv_obj_t *contents, const SettingColors *colors = nullptr);
lv_obj_t *lv_setting_section_create(lv_obj_t *contents, const char *title = nullptr,
                                    const SettingColors *colors = nullptr);
lv_obj_t *lv_setting_row_create(lv_obj_t *section, const char *label);
lv_obj_t *lv_setting_value_create(lv_obj_t *row, const SettingColors *colors = nullptr);
lv_obj_t *lv_setting_slider_create(lv_obj_t *section, int32_t min, int32_t max, int32_t value,
                                   const SettingColors *colors = nullptr);
lv_obj_t *lv_setting_separator_create(lv_obj_t *section, const SettingColors *colors = nullptr);
lv_obj_t *lv_setting_switch_create(lv_obj_t *row, bool checked,
                                   std::function<void(lv_obj_t *, bool)> on_change,
                                   const SettingColors *colors = nullptr);
