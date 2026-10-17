/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "stream_waiting.hpp"
#include "display/panel.hpp"
#include "resources.h"
#include "widgets.hpp"

static constexpr uint32_t kHintColor = 0x808080;

StreamWaiting stream_waiting_build(lv_obj_t *screen, const char *hint) {
    lv_obj_set_style_bg_color(screen, lv_color_black(), 0);
    lv_obj_set_style_bg_opa(screen, LV_OPA_COVER, 0);
    lv_obj_set_style_text_color(screen, lv_color_white(), 0);
    lv_obj_set_scrollable(screen, false);

    StreamWaiting waiting;
    waiting.container = lv_container_create(screen, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_clickable(waiting.container, false);
    lv_obj_set_style_pad_row(waiting.container, 16, 0);

    waiting.icon = lv_label_create(waiting.container);
    lv_obj_set_style_text_font(waiting.icon, &icon_96, 0);
    waiting.message = lv_label_create(waiting.container);
    lv_obj_set_font_role(waiting.message, LV_WIDGETS_FONT_HEADING);
    lv_obj_set_width(waiting.message, LV_PCT(90));
    lv_obj_set_style_text_align(waiting.message, LV_TEXT_ALIGN_CENTER, 0);
    auto label = lv_label_create(waiting.container);
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_BODY);
    lv_obj_set_style_text_color(label, lv_color_hex(kHintColor), 0);
    lv_label_set_text(label, hint);
    return waiting;
}

void stream_waiting_layout(const StreamWaiting &waiting, bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    lv_obj_set_size(waiting.container, size.width, size.height);
}

void stream_waiting_set(const StreamWaiting &waiting, const char *icon, const char *message) {
    lv_label_set_text(waiting.icon, icon);
    lv_label_set_text(waiting.message, message);
}
