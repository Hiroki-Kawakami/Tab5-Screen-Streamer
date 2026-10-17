/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "pc_app_screen.hpp"
#include "home_screen.hpp"
#include "pc_app_stream_screen.hpp"
#include "screen_manager.hpp"
#include "settings.hpp"
#include "setup/settings_widgets.hpp"
#include "setup/setup_guide.hpp"
#include "setup/setup_steps.hpp"

namespace {

constexpr const char *kGuideUrl = "https://github.com/Hiroki-Kawakami/Tab5-Screen-Streamer/blob/main/docs/stream-from-pc-app.md";
constexpr int32_t kStartHeight = 72;

}

std::shared_ptr<Screen> PcAppScreen::stream_screen() {
    return std::make_shared<PcAppStreamScreen>(settings_pc_app_option(PcAppOption::UsbAudio),
                                               settings_pc_app_option(PcAppOption::KeyboardInput),
                                               settings_pc_app_option(PcAppOption::TouchInput));
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

    setup_steps_create(section, {
        "Install Tab5 Streamer v1.0.0 or later on your computer. Windows and macOS apps, or the web version.",
        "Connect Tab5 to your computer with a USB-A to USB-A cable.",
        "Tap Start, then open Tab5 Streamer.",
    });

    lv_hor_separator_create(section);

    setup_guide_create(section, kGuideUrl);

    section = lv_setting_section_create(options_pane_, "Options");
    auto row = lv_setting_row_create(section, "USB Audio");
    lv_setting_switch_create(row, settings_pc_app_option(PcAppOption::UsbAudio), [](lv_obj_t *, bool enabled) {
        settings_set_pc_app_option(PcAppOption::UsbAudio, enabled);
        settings_commit();
    });
    lv_setting_separator_create(section);
    row = lv_setting_row_create(section, "Touch Input");
    lv_setting_switch_create(row, settings_pc_app_option(PcAppOption::TouchInput), [](lv_obj_t *, bool enabled) {
        settings_set_pc_app_option(PcAppOption::TouchInput, enabled);
        settings_commit();
    });
    lv_setting_separator_create(section);
    row = lv_setting_row_create(section, "Keyboard Input");
    lv_setting_switch_create(row, settings_pc_app_option(PcAppOption::KeyboardInput), [](lv_obj_t *, bool enabled) {
        settings_set_pc_app_option(PcAppOption::KeyboardInput, enabled);
        settings_commit();
    });
    lv_setting_description_create(section, "Use the Tab5 Keyboard on the PC.");
    keyboard_warning_ = lv_setting_warning_create(section, "Tab5 Keyboard is not connected.");
    updateKeyboardWarning();

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

void PcAppScreen::onEnter() {
    bsp_keyboard_set_event_cb(keyboardEvent, this);
    updateKeyboardWarning();
}

void PcAppScreen::onExit() {
    bsp_keyboard_set_event_cb(nullptr, nullptr);
}

void PcAppScreen::updateKeyboardWarning() {
    lv_obj_set_hidden(keyboard_warning_, bsp_keyboard_is_connected());
}

void PcAppScreen::keyboardEvent(const bsp_keyboard_event_t *event, void *arg) {
    if (event->type != BSP_KEYBOARD_EVENT_CONNECTED &&
        event->type != BSP_KEYBOARD_EVENT_DISCONNECTED) {
        return;
    }
    std::weak_ptr<Screen> weak = static_cast<PcAppScreen *>(arg)->weak_from_this();
    lv_lock();
    lv_async_call([weak] {
        if (auto self = weak.lock()) static_cast<PcAppScreen &>(*self).updateKeyboardWarning();
    });
    lv_unlock();
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
