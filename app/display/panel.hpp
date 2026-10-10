/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The panel's framebuffers as a front buffer on screen and a back buffer the
 * next swap shows, for whatever draws in place of the LVGL UI.
 */

#pragma once
#include <cstddef>
#include <cstdint>
#include "bsp.h"
#include "esp_err.h"
#include "lvgl.h"

enum class PanelBuffer : uint8_t { Front, Back };

/* `sram` is the two draw buffers of `ui`, back to back, borrowed while it is
 * hidden. */
esp_err_t panel_init(lv_display_t *ui, uint8_t *sram, size_t sram_bytes);

bsp_size_t panel_size();
bsp_pixel_format_t panel_format();
int panel_framebuffers();

/* Rebuilds the panel in `format`, waiting for panel_unlock(); from any task
 * but the LVGL one. Every framebuffer is undefined afterwards. The listener
 * runs inside, before drawing resumes. */
using PanelFormatListener = void (*)(bsp_pixel_format_t format, void *arg);
esp_err_t panel_set_format(bsp_pixel_format_t format);
void panel_set_format_listener(PanelFormatListener listener, void *arg);

/* The rest runs on one task, holding the lock; drawing and swapping only
 * between acquire and release. */
void panel_lock();
void panel_unlock();
void panel_acquire();
void panel_release();
bool panel_acquired();

void panel_swap();
esp_err_t panel_fill(PanelBuffer buffer, bsp_rect_t area, uint32_t rgb888);
/* Full-width rows in the panel's format. */
esp_err_t panel_write_rows(PanelBuffer buffer, int y, int height, const uint8_t *pixels);
/* The same rows as one LZ4 block; `y` and `height` are multiples of 16. */
esp_err_t panel_write_lz4_rows(PanelBuffer buffer, int y, int height, const uint8_t *lz4,
                               uint32_t bytes);
/* A JPEG of exactly panel width by `height`; `y` and `height` are multiples
 * of 16. */
esp_err_t panel_decode_rows(PanelBuffer buffer, int y, int height, const uint8_t *jpeg,
                            uint32_t bytes);

/* `source` rotated counter-clockwise by `rotation`, then scaled to `area`;
 * each axis scales by a whole number of sixteenths, ESP_ERR_NOT_SUPPORTED
 * otherwise. `format` is that of RAW and LZ4 sources. */
struct PanelBlit {
    PanelBuffer buffer;
    bsp_size_t source;
    bsp_pixel_format_t format;
    bsp_rotation_t rotation;
    bsp_rect_t area;
};

esp_err_t panel_blit(const PanelBlit &blit, const uint8_t *pixels);
esp_err_t panel_blit_lz4(const PanelBlit &blit, const uint8_t *lz4, uint32_t bytes);
esp_err_t panel_blit_jpeg(const PanelBlit &blit, const uint8_t *jpeg, uint32_t bytes);
