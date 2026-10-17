/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "stream_panel.hpp"
#include "screens/home_screen.hpp"
#include "resources.h"
#include "screen_manager.hpp"
#include "settings.hpp"
#include "widgets.hpp"

static constexpr int32_t kPadding = 24;
static constexpr int32_t kTitleIndent = 48;
static constexpr int32_t kCloseButton = 56;
static constexpr int32_t kHeaderHeight = kPadding + kCloseButton;
static constexpr int32_t kStatusDotSize = 16;
static constexpr int32_t kExitHeight = 72;

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

StreamStatusRow stream_panel_status_build(lv_obj_t *contents) {
    auto section = lv_setting_section_create(contents, "Status", &kPanelColors);
    auto row = lv_container_create(section, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 16, 0);
    StreamStatusRow status;
    status.dot = lv_obj_create(row);
    lv_obj_remove_style_all(status.dot);
    lv_obj_set_size(status.dot, kStatusDotSize, kStatusDotSize);
    lv_obj_set_style_radius(status.dot, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_opa(status.dot, LV_OPA_COVER, 0);
    status.label = lv_label_create(row);
    lv_obj_set_font_role(status.label, LV_WIDGETS_FONT_BODY);
    return status;
}

void stream_panel_status_set(const StreamStatusRow &row, UsbStatus status) {
    const char *text = "Not connected";
    uint32_t color = 0x808080;
    if (status == UsbStatus::Configured) {
        text = "Connected";
        color = 0xffc107;
    } else if (status == UsbStatus::Active) {
        text = "Streaming";
        color = 0x4caf50;
    }
    lv_label_set_text(row.label, text);
    lv_obj_set_style_bg_color(row.dot, lv_color_hex(color), 0);
}

StreamBrightness stream_panel_brightness_build(lv_obj_t *contents,
                                               std::function<void(int)> changed) {
    auto section = lv_setting_section_create(contents, "Display", &kPanelColors);
    auto row = lv_setting_row_create(section, "Brightness");
    StreamBrightness brightness;
    brightness.value = lv_setting_value_create(row, &kPanelColors);
    lv_label_set_text_fmt(brightness.value, "%d%%", settings_display_brightness());
    brightness.slider = lv_setting_slider_create(section, kMinDisplayBrightness, 100,
                                                 settings_display_brightness(), &kPanelColors);
    lv_obj_add_event_fn(brightness.slider, LV_EVENT_VALUE_CHANGED,
                        [brightness, changed = std::move(changed)](lv_event_t *) {
        const int percent = lv_slider_get_value(brightness.slider);
        settings_set_display_brightness(percent);
        lv_label_set_text_fmt(brightness.value, "%d%%", percent);
        changed(settings_display_brightness());
    });
    lv_obj_add_event_fn(brightness.slider, LV_EVENT_RELEASED, [](lv_event_t *) {
        settings_commit();
    });
    return brightness;
}

void stream_panel_brightness_set(const StreamBrightness &brightness, int percent) {
    lv_slider_set_value(brightness.slider, percent, LV_ANIM_OFF);
    lv_label_set_text_fmt(brightness.value, "%d%%", percent);
}

void stream_panel_exit_build(lv_obj_t *contents) {
    lv_spacer_create(contents, LV_PCT(100), 0, 1);

    auto exit = lv_button_create(contents, LV_BUTTON_STYLE_SECONDARY);
    lv_obj_set_size(exit, LV_PCT(100), kExitHeight);
    lv_obj_set_style_bg_color(exit, lv_color_hex(kPanelColors.track), 0);
    lv_obj_set_style_text_color(exit, lv_color_white(), 0);
    lv_button_set_text(exit, "Exit to Home", lv_widgets_resolved_font(LV_WIDGETS_FONT_HEADING));
    lv_obj_add_event_fn(exit, LV_EVENT_CLICKED, [](lv_event_t *) {
        lv_async_call([] {
            settings_set_last_mode(AppMode::None);
            settings_commit();
            screen_manager.load(std::make_shared<HomeScreen>());
        });
    });
}
