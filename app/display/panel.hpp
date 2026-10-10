/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The panel's framebuffers as a front buffer on screen and a back buffer the
 * next swap shows, for whatever draws in place of the LVGL UI.
 */

#pragma once
#include <cstdint>
#include "bsp.h"
#include "esp_err.h"
#include "lvgl.h"

enum class PanelBuffer : uint8_t { Front, Back };

esp_err_t panel_init(lv_display_t *ui);

bsp_size_t panel_size();
bsp_pixel_format_t panel_format();
int panel_framebuffers();

/* The rest runs on one task; drawing and swapping only between acquire and
 * release. */
void panel_acquire();
void panel_release();
bool panel_acquired();

void panel_swap();
esp_err_t panel_fill(PanelBuffer buffer, bsp_rect_t area, uint32_t rgb888);
/* Full-width rows in the panel's format. */
esp_err_t panel_write_rows(PanelBuffer buffer, int y, int height, const uint8_t *pixels);
/* A JPEG of exactly panel width by `height`; `y` and `height` are multiples
 * of 16. */
esp_err_t panel_decode_rows(PanelBuffer buffer, int y, int height, const uint8_t *jpeg,
                            uint32_t bytes);
