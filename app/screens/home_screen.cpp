/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "home_screen.hpp"
#include "pc_app_screen.hpp"
#include "resources.h"
#include "screen_manager.hpp"
#include "settings_screen.hpp"
#include "usb_monitor_screen.hpp"

namespace {

const int32_t kLandscapeColumns[] = {LV_GRID_FR(2), LV_GRID_FR(1), LV_GRID_FR(1), LV_GRID_TEMPLATE_LAST};
const int32_t kLandscapeRows[] = {LV_GRID_FR(1), LV_GRID_FR(1), LV_GRID_TEMPLATE_LAST};
const int32_t kPortraitColumns[] = {LV_GRID_FR(1), LV_GRID_FR(1), LV_GRID_TEMPLATE_LAST};
const int32_t kPortraitRows[] = {LV_GRID_FR(4), LV_GRID_FR(3), LV_GRID_FR(3), LV_GRID_TEMPLATE_LAST};

struct Mode {
    const char *icon;
    const char *title;
    const char *subtitle;
};

const Mode kPcApp = {TABLER_SCREEN_SHARE, PcAppScreen::kTitle, "Stream your screen or videos from your computer with the Tab5 Streamer app"};
const Mode kModes[] = {
    {TABLER_USB, UsbMonitorScreen::kTitle, "Emulates a standard USB display adapter"},
    {TABLER_BROADCAST, "Network Stream", "Receive from OBS, FFmpeg and more over the network (UDP / SRT)"},
    {TABLER_VIDEO, "Camera View", "Show video from a USB camera or HDMI capture device"},
    {TABLER_PHOTO, "Photo Frame", "Appears as a USB drive. Copy images to show them"},
};

lv_obj_t *card_create(lv_obj_t *parent, const Mode &mode, bool hero) {
    auto card = lv_button_create(parent, hero ? LV_BUTTON_STYLE_PRIMARY : LV_BUTTON_STYLE_PLAIN);
    if (!hero) {
        lv_obj_set_style_bg_color(card, lv_color_white(), 0);
        lv_obj_set_style_bg_opa(card, LV_OPA_COVER, 0);
    }
    lv_obj_set_style_pad_all(card, hero ? 40 : 24, 0);
    lv_obj_set_style_pad_row(card, 8, 0);
    lv_obj_set_flex_flow(card, LV_FLEX_FLOW_COLUMN);

    auto icon = lv_label_create(card);
    lv_label_set_text(icon, mode.icon);
    lv_obj_set_style_text_font(icon, hero ? &icon_96 : &icon_56, 0);

    lv_spacer_create(card, 0, 0, 1);

    auto title = lv_label_create(card);
    lv_label_set_text(title, mode.title);
    lv_obj_set_font_role(title, hero ? LV_WIDGETS_FONT_TITLE : LV_WIDGETS_FONT_HEADING);

    auto subtitle = lv_label_create(card);
    lv_label_set_text(subtitle, mode.subtitle);
    lv_obj_set_width(subtitle, LV_PCT(100));
    lv_obj_set_font_role(subtitle, hero ? LV_WIDGETS_FONT_BODY : LV_WIDGETS_FONT_CAPTION);
    if (!hero) lv_obj_set_style_text_color(subtitle, lv_color_hex(0x666666), 0);
    return card;
}

}

void HomeScreen::build() {
    createNavigation("Tab5 Screen Streamer");

    auto settings = lv_button_create(lv_navigation_actions(navigation_), LV_BUTTON_STYLE_NAVIGATION);
    auto settings_icon = lv_label_create(settings);
    lv_label_set_text(settings_icon, TABLER_SETTINGS);
    lv_obj_set_style_text_font(settings_icon, &icon_48, 0);
    lv_obj_add_event_fn(settings, LV_EVENT_CLICKED, [](lv_event_t *) {
        screen_manager.load(std::make_shared<SettingsScreen>());
    });

    lv_obj_set_style_pad_column(contents_, 24, 0);

    hero_ = card_create(contents_, kPcApp, true);
    lv_obj_add_event_fn(hero_, LV_EVENT_CLICKED, [](lv_event_t *) {
        screen_manager.load(std::make_shared<PcAppScreen>());
    });
    for (size_t i = 0; i < cards_.size(); i++) {
        cards_[i] = card_create(contents_, kModes[i], false);
    }
    lv_obj_add_event_fn(cards_[0], LV_EVENT_CLICKED, [](lv_event_t *) {
        screen_manager.load(std::make_shared<UsbMonitorScreen>());
    });

    layout();
    lv_obj_add_event_fn(root_, LV_EVENT_SIZE_CHANGED, [this](lv_event_t *) { layout(); });
}

void HomeScreen::layout() {
    auto display = lv_obj_get_display(root_);
    const bool landscape = lv_display_get_horizontal_resolution(display) > lv_display_get_vertical_resolution(display);
    if (landscape) {
        lv_obj_set_grid_dsc_array(contents_, kLandscapeColumns, kLandscapeRows);
        lv_obj_set_grid_cell(hero_, LV_GRID_ALIGN_STRETCH, 0, 1, LV_GRID_ALIGN_STRETCH, 0, 2);
    } else {
        lv_obj_set_grid_dsc_array(contents_, kPortraitColumns, kPortraitRows);
        lv_obj_set_grid_cell(hero_, LV_GRID_ALIGN_STRETCH, 0, 2, LV_GRID_ALIGN_STRETCH, 0, 1);
    }
    for (size_t i = 0; i < cards_.size(); i++) {
        const int32_t column = landscape ? 1 + i % 2 : i % 2;
        const int32_t row = landscape ? i / 2 : 1 + i / 2;
        lv_obj_set_grid_cell(cards_[i], LV_GRID_ALIGN_STRETCH, column, 1, LV_GRID_ALIGN_STRETCH, row, 1);
    }
}
