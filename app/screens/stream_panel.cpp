/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "stream_panel.hpp"
#include "resources.h"
#include "widgets.hpp"

static constexpr int32_t kPadding = 24;
static constexpr int32_t kTitleIndent = 48;
static constexpr int32_t kCloseButton = 56;
static constexpr int32_t kHeaderHeight = kPadding + kCloseButton;

static void build_header(lv_obj_t *root, const char *title, std::function<void()> on_close) {
    auto header = lv_container_create(root, LV_FLEX_FLOW_ROW);
    lv_obj_set_size(header, lv_pct(100), kHeaderHeight);
    lv_obj_set_style_pad_left(header, kTitleIndent, 0);
    lv_obj_set_style_pad_right(header, kPadding, 0);
    lv_obj_set_style_pad_top(header, kPadding, 0);
    lv_obj_set_style_bg_color(header, lv_color_hex(kPanelColors.page_bg), 0);
    lv_obj_set_style_bg_opa(header, LV_OPA_COVER, 0);

    auto label = lv_label_create(header);
    lv_obj_set_flex_grow(label, 1);
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_TITLE);
    lv_label_set_text(label, title);

    auto close = lv_button_create(header, LV_BUTTON_STYLE_PLAIN);
    lv_obj_set_size(close, kCloseButton, kCloseButton);
    lv_obj_set_style_pad_all(close, 0, 0);
    lv_obj_set_style_radius(close, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_color(close, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(close, LV_OPA_20, LV_STATE_PRESSED);
    lv_obj_set_style_text_color(close, lv_color_white(), 0);
    lv_button_set_text(close, TABLER_X, &icon_36);
    lv_obj_add_event_fn(close, LV_EVENT_CLICKED, [on_close](lv_event_t *) { on_close(); });
}

lv_obj_t *stream_panel_build(lv_obj_t *root, const char *title, std::function<void()> on_close) {
    lv_obj_set_style_bg_color(root, lv_color_hex(kPanelColors.page_bg), 0);
    lv_obj_set_style_bg_opa(root, LV_OPA_COVER, 0);
    lv_obj_set_style_text_color(root, lv_color_white(), 0);
    lv_obj_set_flex_flow(root, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_scrollable(root, false);

    build_header(root, title, std::move(on_close));

    auto contents = lv_spacer_create(root, lv_pct(100), LV_SIZE_CONTENT, 1);
    lv_obj_set_flex_flow(contents, LV_FLEX_FLOW_COLUMN);
    lv_setting_page_style(contents, &kPanelColors);
    return contents;
}
