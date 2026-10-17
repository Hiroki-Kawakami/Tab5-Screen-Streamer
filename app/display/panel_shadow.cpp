/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel_shadow.hpp"
#include <algorithm>
#include <cstring>
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "panel.hpp"

static const char *TAG = "panel_shadow";

static constexpr size_t kAlignment = 64;

PanelShadow::~PanelShadow() {
    heap_caps_free(pixels_);
}

esp_err_t PanelShadow::resize(bsp_size_t size, bsp_pixel_format_t format) {
    const size_t stride = size.width * bsp_pixel_format_bytes(format);
    if (size.width != size_.width || size.height != size_.height || format != format_) {
        heap_caps_free(pixels_);
        pixels_ = static_cast<uint8_t *>(
            heap_caps_aligned_alloc(kAlignment, stride * size.height, MALLOC_CAP_SPIRAM));
        if (!pixels_) {
            ESP_LOGE(TAG, "no memory for %dx%d", size.width, size.height);
            size_ = {};
            stride_ = 0;
            pending_ = {};
            return ESP_ERR_NO_MEM;
        }
        size_ = size;
        format_ = format;
        stride_ = stride;
    }
    memset(pixels_, 0, stride_ * size_.height);
    invalidate();
    return ESP_OK;
}

void PanelShadow::damage(int y, int height) {
    if (dirty()) {
        pending_.start = std::min(pending_.start, y);
        pending_.end = std::max(pending_.end, y + height);
    } else {
        pending_ = {y, y + height};
    }
}

void PanelShadow::invalidate() {
    pending_ = {0, size_.height};
    for (Rows &rows : history_) rows = pending_;
}

esp_err_t PanelShadow::present(bsp_rotation_t rotation) {
    if (!dirty()) return ESP_OK;
    const int previous = panel_framebuffers() - 1;
    Rows rows = pending_;
    for (int i = 0; i < previous; i++) {
        if (history_[i].start >= history_[i].end) continue;
        rows.start = std::min(rows.start, history_[i].start);
        rows.end = std::max(rows.end, history_[i].end);
    }
    for (int i = previous - 1; i > 0; i--) history_[i] = history_[i - 1];
    if (previous > 0) history_[0] = pending_;
    pending_ = {};

    const int height = rows.end - rows.start;
    PanelBlit blit = {};
    blit.buffer = PanelBuffer::Back;
    blit.source = {size_.width, height};
    blit.format = format_;
    blit.rotation = rotation;
    blit.area = panel_native_rect({{0, rows.start}, {size_.width, height}}, rotation);
    const esp_err_t err = panel_blit(blit, row(rows.start));
    if (err != ESP_OK) ESP_LOGW(TAG, "rows %d-%d: %s", rows.start, rows.end, esp_err_to_name(err));
    panel_swap();
    return err;
}
