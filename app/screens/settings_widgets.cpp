/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "settings_widgets.hpp"
#include "widgets.hpp"

static constexpr int32_t kPadding = 24;
static constexpr int32_t kRowGap = 12;
static constexpr int32_t kKnobSize = 28;
static constexpr int32_t kSwitchWidth = 84;
static constexpr int32_t kSwitchHeight = 48;
static constexpr int32_t kSwitchKnobInset = 4;
static constexpr uint32_t kTrackColor = 0xe0e0e0;
static constexpr uint32_t kActiveColor = 0x2196f3;
static constexpr uint32_t kSecondaryTextColor = 0x808080;

void lv_setting_page_style(lv_obj_t *contents, const SettingColors *colors) {
    lv_obj_set_style_bg_color(contents, lv_color_hex(colors ? colors->page_bg : 0xeeeeee), 0);
    lv_obj_set_style_bg_opa(contents, LV_OPA_COVER, 0);
    lv_obj_set_style_pad_all(contents, kPadding, 0);
    lv_obj_set_style_pad_row(contents, kRowGap, 0);
}

lv_obj_t *lv_setting_section_create(lv_obj_t *contents, const char *title,
                                    const SettingColors *colors) {
    if (title) {
        auto label = lv_label_create(contents);
        if (lv_obj_get_child_count(contents) > 1) lv_obj_set_style_pad_top(label, kPadding / 2, 0);
        lv_label_set_text(label, title);
        lv_obj_set_font_role(label, LV_WIDGETS_FONT_BODY);
        lv_obj_set_style_text_color(label, lv_color_hex(colors ? colors->value : kSecondaryTextColor), 0);
        lv_obj_set_style_pad_hor(label, kPadding, 0);
    }
    auto section = lv_container_create(contents, colors ? lv_color_hex(colors->section_bg) : lv_color_white());
    lv_obj_set_size(section, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_style_radius(section, 16, 0);
    lv_obj_set_style_clip_corner(section, true, 0);
    lv_obj_set_flex_flow(section, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_all(section, kPadding, 0);
    lv_obj_set_style_pad_row(section, kRowGap, 0);
    return section;
}

lv_obj_t *lv_setting_row_create(lv_obj_t *section, const char *label) {
    auto row = lv_container_create(section, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);

    auto text = lv_label_create(row);
    lv_obj_set_flex_grow(text, 1);
    lv_obj_set_font_role(text, LV_WIDGETS_FONT_BODY);
    lv_label_set_text(text, label);
    return row;
}

lv_obj_t *lv_setting_value_create(lv_obj_t *row, const SettingColors *colors) {
    auto value = lv_label_create(row);
    lv_obj_set_font_role(value, LV_WIDGETS_FONT_BODY);
    lv_obj_set_style_text_color(value, lv_color_hex(colors ? colors->value : kSecondaryTextColor), 0);
    return value;
}

lv_obj_t *lv_setting_slider_create(lv_obj_t *section, int32_t min, int32_t max, int32_t value,
                                   const SettingColors *colors) {
    auto box = lv_container_create(section, LV_FLEX_FLOW_ROW);
    lv_obj_set_style_pad_all(box, kKnobSize / 2, 0);

    auto slider = lv_slider_create(box);
    lv_obj_set_flex_grow(slider, 1);
    lv_slider_set_range(slider, min, max);
    lv_slider_set_value(slider, value, LV_ANIM_OFF);
    lv_obj_set_style_width(slider, kKnobSize, LV_PART_KNOB);
    lv_obj_set_style_height(slider, kKnobSize, LV_PART_KNOB);
    lv_obj_set_style_radius(slider, LV_RADIUS_CIRCLE, LV_PART_KNOB);
    if (colors) {
        lv_obj_set_style_bg_color(slider, lv_color_hex(colors->track), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(slider, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_bg_color(slider, lv_color_hex(colors->accent), LV_PART_INDICATOR);
        lv_obj_set_style_bg_color(slider, lv_color_hex(colors->accent), LV_PART_KNOB);
    }
    return slider;
}

lv_obj_t *lv_setting_separator_create(lv_obj_t *section, const SettingColors *colors) {
    if (!colors) return lv_hor_separator_create(section);
    return lv_hor_separator_create(section, lv_color_hex(colors->separator));
}

lv_obj_t *lv_setting_switch_create(lv_obj_t *row, bool checked,
                                   std::function<void(lv_obj_t *, bool)> on_change,
                                   const SettingColors *colors) {
    auto sw = lv_switch_create(row);
    lv_obj_set_size(sw, kSwitchWidth, kSwitchHeight);
    lv_obj_set_style_radius(sw, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_color(sw, lv_color_hex(colors ? colors->track : kTrackColor), 0);
    lv_obj_set_style_bg_opa(sw, LV_OPA_COVER, 0);
    lv_obj_set_style_bg_opa(sw, LV_OPA_TRANSP, LV_PART_INDICATOR);
    lv_obj_set_style_radius(sw, LV_RADIUS_CIRCLE, LV_PART_INDICATOR);
    lv_obj_set_style_bg_color(sw, lv_color_hex(colors ? colors->accent : kActiveColor),
                              (lv_style_selector_t)LV_PART_INDICATOR | LV_STATE_CHECKED);
    lv_obj_set_style_bg_opa(sw, LV_OPA_COVER,
                            (lv_style_selector_t)LV_PART_INDICATOR | LV_STATE_CHECKED);
    lv_obj_set_style_bg_color(sw, lv_color_white(), LV_PART_KNOB);
    lv_obj_set_style_radius(sw, LV_RADIUS_CIRCLE, LV_PART_KNOB);
    lv_obj_set_style_pad_all(sw, -kSwitchKnobInset, LV_PART_KNOB);
    lv_obj_set_style_shadow_width(sw, 6, LV_PART_KNOB);
    lv_obj_set_style_shadow_opa(sw, LV_OPA_30, LV_PART_KNOB);
    lv_obj_set_style_shadow_offset_y(sw, 2, LV_PART_KNOB);
    if (checked) lv_obj_add_state(sw, LV_STATE_CHECKED);
    lv_obj_add_event_fn(sw, LV_EVENT_VALUE_CHANGED, [sw, on_change](lv_event_t *) {
        on_change(sw, lv_obj_has_state(sw, LV_STATE_CHECKED));
    });
    return sw;
}
