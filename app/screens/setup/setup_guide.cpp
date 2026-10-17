/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "setup_guide.hpp"
#include <cstring>
#include "esp_log.h"
#include "widgets.hpp"

static const char *TAG = "setup_guide";

static constexpr int32_t kQrSize = 200;

lv_obj_t *setup_guide_create(lv_obj_t *parent, const char *url) {
    auto guide = lv_container_create(parent, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(guide, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_column(guide, 24, 0);

    auto text = lv_container_create(guide, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_size(text, 0, LV_SIZE_CONTENT);
    lv_obj_set_flex_grow(text, 1);
    lv_obj_set_flex_align(text, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(text, 8, 0);
    auto title = lv_label_create(text);
    lv_obj_set_font_role(title, LV_WIDGETS_FONT_HEADING);
    lv_label_set_text(title, "Setup guide");
    auto label = lv_label_create(text);
    lv_obj_set_width(label, LV_PCT(100));
    lv_obj_set_font_role(label, LV_WIDGETS_FONT_CAPTION);
    lv_obj_set_style_text_color(label, lv_color_hex(0x808080), 0);
    lv_label_set_text(label, url);

    auto qr = lv_qrcode_create(guide);
    lv_qrcode_set_size(qr, kQrSize);
    if (lv_qrcode_update(qr, url, strlen(url)) != LV_RESULT_OK) {
        ESP_LOGE(TAG, "qrcode update failed");
    }
    return guide;
}
