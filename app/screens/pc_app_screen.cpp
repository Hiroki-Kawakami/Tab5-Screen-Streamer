/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "pc_app_screen.hpp"
#include <cstring>
#include "esp_log.h"
#include "home_screen.hpp"
#include "pc_app_stream_screen.hpp"
#include "screen_manager.hpp"
#include "settings.hpp"
#include "settings_widgets.hpp"

static const char *TAG = "pc_app_screen";

namespace {

constexpr const char *kGuideUrl = "https://github.com/Hiroki-Kawakami/Tab5-Screen-Streamer";
constexpr int32_t kQrSize = 200;
constexpr int32_t kStepNumberSize = 40;
constexpr int32_t kStartHeight = 72;

const char *const kSteps[] = {
    "Install Tab5 Streamer on your computer. Windows and macOS apps, or the web version.",
    "Connect Tab5 to your computer with a USB-A to USB-A cable.",
    "Tap Start, then open Tab5 Streamer.",
};

void step_create(lv_obj_t *parent, int number, const char *text) {
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

}

std::shared_ptr<Screen> PcAppScreen::stream_screen() {
    return std::make_shared<PcAppStreamScreen>(settings_usb_audio());
}

void PcAppScreen::build() {
    createNavigation(kTitle, LV_NAVIGATION_STYLE_DEFAULT | LV_NAVIGATION_STYLE_BACK);
    lv_obj_set_style_pad_row(contents_, 36, 0);
    lv_obj_set_style_pad_column(contents_, 24, 0);

    setup_pane_ = lv_container_create(contents_, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(setup_pane_, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(setup_pane_, 12, 0);
    right_pane_ = lv_container_create(contents_, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(right_pane_, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(right_pane_, 24, 0);
    options_pane_ = lv_container_create(right_pane_, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(options_pane_, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(options_pane_, 12, 0);

    auto section = lv_setting_section_create(setup_pane_, "Setup");
    lv_obj_set_style_pad_row(section, 24, 0);

    auto steps = lv_container_create(section, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_size(steps, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_flex_align(steps, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(steps, 20, 0);
    for (size_t i = 0; i < sizeof(kSteps) / sizeof(kSteps[0]); i++) {
        step_create(steps, i + 1, kSteps[i]);
    }

    lv_hor_separator_create(section);

    auto guide = lv_container_create(section, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(guide, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_column(guide, 24, 0);

    auto guide_text = lv_container_create(guide, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_size(guide_text, 0, LV_SIZE_CONTENT);
    lv_obj_set_flex_grow(guide_text, 1);
    lv_obj_set_flex_align(guide_text, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(guide_text, 8, 0);
    auto title = lv_label_create(guide_text);
    lv_obj_set_font_role(title, LV_WIDGETS_FONT_HEADING);
    lv_label_set_text(title, "Setup guide");
    auto url = lv_label_create(guide_text);
    lv_obj_set_width(url, LV_PCT(100));
    lv_obj_set_font_role(url, LV_WIDGETS_FONT_CAPTION);
    lv_obj_set_style_text_color(url, lv_color_hex(0x808080), 0);
    lv_label_set_text(url, kGuideUrl);

    auto qr = lv_qrcode_create(guide);
    lv_qrcode_set_size(qr, kQrSize);
    if (lv_qrcode_update(qr, kGuideUrl, strlen(kGuideUrl)) != LV_RESULT_OK) {
        ESP_LOGE(TAG, "qrcode update failed");
    }

    section = lv_setting_section_create(options_pane_, "Options");
    auto row = lv_setting_row_create(section, "USB Audio");
    lv_setting_switch_create(row, settings_usb_audio(), [](lv_obj_t *, bool enabled) {
        settings_set_usb_audio(enabled);
        settings_commit();
    });

    footer_ = lv_container_create(root_, LV_FLEX_FLOW_ROW);
    lv_obj_set_style_pad_hor(footer_, 24, 0);
    lv_obj_set_style_pad_bottom(footer_, 24, 0);

    start_ = lv_button_create(footer_, LV_BUTTON_STYLE_PRIMARY);
    lv_obj_set_size(start_, LV_PCT(100), kStartHeight);
    lv_button_set_text(start_, "Start", lv_widgets_resolved_font(LV_WIDGETS_FONT_HEADING));
    lv_obj_add_event_fn(start_, LV_EVENT_CLICKED, [](lv_event_t *) {
        settings_set_last_mode(AppMode::PcApp);
        settings_commit();
        screen_manager.load(stream_screen());
    });

    layout();
    lv_obj_add_event_fn(root_, LV_EVENT_SIZE_CHANGED, [this](lv_event_t *) { layout(); });
}

void PcAppScreen::back() {
    screen_manager.load(std::make_shared<HomeScreen>());
}

void PcAppScreen::layout() {
    auto display = lv_obj_get_display(root_);
    const bool landscape = lv_display_get_horizontal_resolution(display) > lv_display_get_vertical_resolution(display);
    lv_obj_set_flex_flow(contents_, landscape ? LV_FLEX_FLOW_ROW : LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(contents_, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    for (lv_obj_t *pane : {setup_pane_, right_pane_}) {
        lv_obj_set_size(pane, landscape ? 0 : LV_PCT(100), landscape ? LV_PCT(100) : LV_SIZE_CONTENT);
    }
    lv_obj_set_flex_grow(setup_pane_, landscape ? 3 : 0);
    lv_obj_set_flex_grow(right_pane_, landscape ? 2 : 0);
    lv_obj_set_size(options_pane_, LV_PCT(100), landscape ? 0 : LV_SIZE_CONTENT);
    lv_obj_set_flex_grow(options_pane_, landscape ? 1 : 0);

    lv_obj_set_scrollable(contents_, !landscape);
    lv_obj_set_scrollable(setup_pane_, landscape);
    lv_obj_set_scrollable(right_pane_, false);
    lv_obj_set_scrollable(options_pane_, landscape);

    lv_obj_set_parent(start_, landscape ? right_pane_ : footer_);
    lv_obj_set_hidden(footer_, landscape);
}
