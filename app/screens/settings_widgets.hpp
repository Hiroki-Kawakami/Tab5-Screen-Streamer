/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <functional>
#include "lvgl.h"

lv_obj_t *lv_setting_section_create(lv_obj_t *contents, const char *title = nullptr);
lv_obj_t *lv_setting_row_create(lv_obj_t *section, const char *label);
lv_obj_t *lv_setting_switch_create(lv_obj_t *row, bool checked,
                                   std::function<void(lv_obj_t *, bool)> on_change);
