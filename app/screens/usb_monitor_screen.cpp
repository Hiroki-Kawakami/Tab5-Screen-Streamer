/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb_monitor_screen.hpp"
#include "home_screen.hpp"
#include "screen_manager.hpp"
#include "settings.hpp"
#include "setup/settings_widgets.hpp"
#include "setup/setup_steps.hpp"
#include "usb_monitor_stream_screen.hpp"

static constexpr int32_t kStartHeight = 72;

std::shared_ptr<Screen> UsbMonitorScreen::stream_screen() {
    return std::make_shared<UsbMonitorStreamScreen>(settings_usb_monitor_adapter());
}

void UsbMonitorScreen::build() {
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
    setup_steps_create(section, {
        "Connect Tab5 to your computer with a USB-A to USB-A cable.",
        "Tap Start. Tab5 appears on your computer as a display.",
    });

    section = lv_setting_section_create(options_pane_, "Options");
    auto row = lv_setting_row_create(section, "Adapter");
    lv_setting_dropdown_create(row, "DisplayLink\nGeneric USB Display", (uint32_t)settings_usb_monitor_adapter(),
                               [this](lv_obj_t *, uint32_t index) {
        settings_set_usb_monitor_adapter((UsbMonitorAdapter)index);
        settings_commit();
        updateAdapterDescription();
    });
    adapter_description_ = lv_setting_description_create(section, "");
    updateAdapterDescription();

    footer_ = lv_container_create(root_, LV_FLEX_FLOW_ROW);
    lv_obj_set_style_pad_hor(footer_, 24, 0);
    lv_obj_set_style_pad_bottom(footer_, 24, 0);

    start_ = lv_button_create(footer_, LV_BUTTON_STYLE_PRIMARY);
    lv_obj_set_size(start_, LV_PCT(100), kStartHeight);
    lv_button_set_text(start_, "Start", lv_widgets_resolved_font(LV_WIDGETS_FONT_HEADING));
    lv_obj_add_event_fn(start_, LV_EVENT_CLICKED, [](lv_event_t *) {
        settings_set_last_mode(AppMode::UsbMonitor);
        settings_commit();
        screen_manager.load(stream_screen());
    });

    layout();
    lv_obj_add_event_fn(root_, LV_EVENT_SIZE_CHANGED, [this](lv_event_t *) { layout(); });
}

void UsbMonitorScreen::back() {
    screen_manager.load(std::make_shared<HomeScreen>());
}

void UsbMonitorScreen::updateAdapterDescription() {
    lv_label_set_text(adapter_description_,
                      settings_usb_monitor_adapter() == UsbMonitorAdapter::Gud
                          ? "Works with the gud driver built into Linux."
                          : "Works with the DisplayLink driver.");
}

void UsbMonitorScreen::layout() {
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
