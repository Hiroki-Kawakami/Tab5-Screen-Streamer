/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include "esp_err.h"
#include "lvgl.h"

struct StreamBand {
    const uint8_t *data;
    uint32_t size;
    uint16_t y;
    uint16_t height;
};

/* A full-width frame as JPEG bands stacked from the top of the panel. */
struct StreamFrame {
    static constexpr int kMaxBands = 80;
    StreamBand bands[kMaxBands];
    int count;
    void (*release)(StreamFrame *frame);
    void *context;
};

/* `ui` is hidden from the first frame until presenter_end(). */
esp_err_t presenter_init(lv_display_t *ui);
/* Takes the frame until it calls release(), from any task. A frame not yet
 * decoded is released at once when a newer one arrives. */
void presenter_submit(StreamFrame *frame);
/* Drops the frame not yet decoded and hands the panel back to `ui`. */
void presenter_end();
/* Releases every frame the presenter holds, waiting for one being decoded. */
void presenter_flush();
