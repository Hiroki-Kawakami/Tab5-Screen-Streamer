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
/* The panel as seen upright at `rotation`, and a rect of that view on the
 * panel. */
bsp_size_t panel_logical_size(bsp_rotation_t rotation);
bsp_rect_t panel_native_rect(const bsp_rect_t &rect, bsp_rotation_t rotation);
bsp_pixel_format_t panel_format();
int panel_framebuffers();

/* Rebuilds the panel in `format`, waiting for panel_unlock(). Every
 * framebuffer is undefined afterwards. The listener runs inside, before
 * drawing resumes. */
using PanelFormatListener = void (*)(bsp_pixel_format_t format, void *arg);
esp_err_t panel_set_format(bsp_pixel_format_t format);
void panel_set_format_listener(PanelFormatListener listener, void *arg);

/* While a clip is set, swaps are held: Back is the framebuffer on screen and
 * panel_swap() presents nothing. Every draw is cut to the clip, leaving up to
 * a scaled source pixel along its edges undrawn. Null clears it. While
 * acquired, what lies outside the clip is kept in a copy that panel_blit()
 * and panel_blit_lz4() keep drawing into, and comes back when the clip
 * changes; other draws outside the clip are lost, and so is the copy when
 * memory runs short, leaving it black. Waits for panel_unlock(); the listener
 * runs inside. */
using PanelClipListener = void (*)(const bsp_rect_t *clip, void *arg);
esp_err_t panel_set_clip(const bsp_rect_t *clip);
bool panel_clip(bsp_rect_t *clip);
void panel_set_clip_listener(PanelClipListener listener, void *arg);

/* The LVGL lock is always taken before the panel lock. acquire and release
 * take both, so they are called without either. The rest runs on one task,
 * holding the lock; drawing and swapping only between acquire and release. */
void panel_lock();
void panel_unlock();
void panel_acquire();
void panel_release();
bool panel_acquired();

/* An LVGL display drawn into the front buffer, upright at `rotation`, while
 * the panel is not acquired. It stands in for the main UI, which stays hidden
 * until the display is deleted: it renders in the main UI's draw buffers. It
 * has no input device, so touches go to the outside touch callback. Call these
 * from the LVGL context. */
lv_display_t *panel_idle_display_create(bsp_rotation_t rotation);
void panel_idle_display_set_rotation(bsp_rotation_t rotation);
void panel_idle_display_delete();

int panel_buffer_index(PanelBuffer buffer);
/* The two internal RAM strips panel_blit_lz4() and the JPEG draws decode
 * into. Between acquire and release, a renderer using neither may fill them
 * itself, and blit from them. */
uint8_t *panel_strip(int index);
size_t panel_strip_bytes();
void panel_swap();
/* `area` of the front buffer into the back buffer; nothing while a clip is
 * set, when they are the same. */
esp_err_t panel_copy_to_back(bsp_rect_t area);
/* Turns what is shown upside down: by a swap, or in place while a clip is set,
 * which needs three framebuffers. */
esp_err_t panel_rotate_180();
/* Blackens every framebuffer with the CPU; the one on screen only inside the
 * clip while one is set. PPA fills of large areas starve the DSI of PSRAM
 * bandwidth and underrun it. */
void panel_clear();
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
/* panel_blit() returning once queued: `pixels` are read until
 * panel_blit_wait(), which panel_swap() calls first. */
esp_err_t panel_blit_async(const PanelBlit &blit, const uint8_t *pixels);
void panel_blit_wait();
esp_err_t panel_blit_lz4(const PanelBlit &blit, const uint8_t *lz4, uint32_t bytes);
esp_err_t panel_blit_jpeg(const PanelBlit &blit, const uint8_t *jpeg, uint32_t bytes);

extern int64_t panel_diag_lz4_us;  // DIAG
