/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "setup_steps.hpp"
#include "widgets.hpp"

static constexpr int32_t kStepNumberSize = 40;

static void step_create(lv_obj_t *parent, int number, const char *text) {
    auto row = lv_container_create(parent, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_column(row, 16, 0);

    const int32_t line_height = lv_font_get_line_height(lv_widgets_resolved_font(LV_WIDGETS_FONT_BODY));
    const int32_t inset = (kStepNumberSize - line_height) / 2;

    auto badge = lv_label_create(row);
    lv_obj_set_size(badge, kStepNumberSize, kStepNumberSize);
    lv_obj_set_style_radius(badge, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_color(badge, lv_color_hex(0x2196f3), 0);
    lv_obj_set_style_bg_opa(badge, LV_OPA_COVER, 0);
    lv_obj_set_style_text_color(badge, lv_color_white(), 0);
    lv_obj_set_style_text_align(badge, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_set_font_role(badge, LV_WIDGETS_FONT_BODY);
    lv_obj_set_style_pad_top(badge, inset, 0);
    lv_label_set_text_fmt(badge, "%d", number);

    auto label = lv_label_create(row);
    lv_obj_set_flex_grow(label, 1);
    lv_obj_set_style_pad_top(label, inset, 0);
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_BODY);
    lv_label_set_text(label, text);
}

lv_obj_t *setup_steps_create(lv_obj_t *parent, std::initializer_list<const char *> steps) {
    auto list = lv_container_create(parent, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_size(list, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_flex_align(list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(list, 20, 0);
    int number = 1;
    for (const char *step : steps) step_create(list, number++, step);
    return list;
}
