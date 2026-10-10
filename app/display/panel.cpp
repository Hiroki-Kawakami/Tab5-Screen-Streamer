/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel.hpp"
#include <algorithm>
#include "display_manager.hpp"
#include "driver/ppa.h"
#include "esp_attr.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "jpeg_ppa_pipeline.h"
#include "lvgl.hpp"
#include "lz4_block.h"
#ifdef ESP_PLATFORM
#include "esp_cache.h"
#endif

static const char *TAG = "panel";

static constexpr int kRowAlign = 16;
static constexpr uint32_t kScaleDenominator = 16;
static constexpr size_t kLz4Window = 65536;
static constexpr int kStrips = 2;

static lv_display_t *s_ui;
static SemaphoreHandle_t s_lock;
static PanelFormatListener s_format_listener;
static void *s_format_listener_arg;
static uint8_t *s_strips[kStrips];
static size_t s_strip_bytes;
static jpeg_ppa_pipeline_handle_t s_pipeline;
static ppa_client_handle_t s_fill;
static ppa_client_handle_t s_copy;
static SemaphoreHandle_t s_strip_done[kStrips];
static bool s_strip_busy[kStrips];

static bsp_size_t s_size;
static bsp_pixel_format_t s_format;
static size_t s_stride;
static int s_framebuffers;
static int s_front;
static bool s_acquired;

static uint8_t *framebuffer(PanelBuffer buffer) {
    const int index = buffer == PanelBuffer::Front ? s_front : (s_front + 1) % s_framebuffers;
    return static_cast<uint8_t *>(bsp_display_get_frame_buffer(index));
}

static bool rows_valid(int y, int height) {
    return y >= 0 && height > 0 && y + height <= s_size.height;
}

static ppa_srm_color_mode_t srm_mode(bsp_pixel_format_t format) {
    return format == BSP_PIXEL_FORMAT_RGB888 ? PPA_SRM_COLOR_MODE_RGB888
                                             : PPA_SRM_COLOR_MODE_RGB565;
}

static ppa_srm_rotation_angle_t srm_angle(bsp_rotation_t rotation) {
    switch (rotation) {
        case BSP_ROTATION_90: return PPA_SRM_ROTATION_ANGLE_90;
        case BSP_ROTATION_180: return PPA_SRM_ROTATION_ANGLE_180;
        case BSP_ROTATION_270: return PPA_SRM_ROTATION_ANGLE_270;
        default: return PPA_SRM_ROTATION_ANGLE_0;
    }
}

static bool IRAM_ATTR strip_done(ppa_client_handle_t, ppa_event_data_t *, void *user_data) {
    if (!user_data) return false;
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(static_cast<SemaphoreHandle_t>(user_data), &woken);
    return woken == pdTRUE;
}

static void wait_strip(int index) {
    if (!s_strip_busy[index]) return;
    xSemaphoreTake(s_strip_done[index], portMAX_DELAY);
    s_strip_busy[index] = false;
}

esp_err_t panel_init(lv_display_t *ui, uint8_t *sram, size_t sram_bytes) {
    s_ui = ui;
    s_lock = xSemaphoreCreateMutex();
    if (!s_lock) return ESP_ERR_NO_MEM;
    s_strip_bytes = sram_bytes / kStrips;
    for (int i = 0; i < kStrips; i++) {
        s_strips[i] = sram + i * s_strip_bytes;
        s_strip_done[i] = xSemaphoreCreateBinary();
        if (!s_strip_done[i]) return ESP_ERR_NO_MEM;
    }
    s_size = bsp_display_get_size();
    s_format = bsp_display_get_pixel_format();
    s_stride = s_size.width * bsp_pixel_format_bytes(s_format);
    while (s_framebuffers < BSP_DISPLAY_MAX_FRAME_BUFFERS &&
           bsp_display_get_frame_buffer(s_framebuffers)) {
        s_framebuffers++;
    }
    if (s_framebuffers < 2) {
        ESP_LOGE(TAG, "needs two framebuffers, has %d", s_framebuffers);
        return ESP_ERR_INVALID_STATE;
    }

    ppa_client_config_t client = {};
    client.oper_type = PPA_OPERATION_FILL;
    esp_err_t err = ppa_register_client(&client, &s_fill);
    if (err == ESP_OK) {
        client.oper_type = PPA_OPERATION_SRM;
        client.max_pending_trans_num = kStrips;
        err = ppa_register_client(&client, &s_copy);
    }
    if (err == ESP_OK) {
        ppa_event_callbacks_t callbacks = {};
        callbacks.on_trans_done = strip_done;
        err = ppa_client_register_event_callbacks(s_copy, &callbacks);
    }
    if (err != ESP_OK) ESP_LOGE(TAG, "ppa client: %s", esp_err_to_name(err));
    return err;
}

static void create_pipeline() {
    jpeg_ppa_pipeline_cfg_t config = {};
    config.strip_bufs[0] = s_strips[0];
    config.strip_bufs[1] = s_strips[1];
    config.strip_buf_size = s_strip_bytes;
    config.strip_color_mode = srm_mode(s_format);
    config.rgb_order = JPEG_DEC_RGB_ELEMENT_ORDER_BGR;
    config.conv_std = JPEG_YUV_RGB_CONV_STD_BT601;
    config.yuv_full_range = true;
    esp_err_t err = jpeg_ppa_pipeline_new(&config, &s_pipeline);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "jpeg pipeline: %s", esp_err_to_name(err));
        s_pipeline = nullptr;
    }
}

static void delete_pipeline() {
    if (!s_pipeline) return;
    jpeg_ppa_pipeline_del(s_pipeline);
    s_pipeline = nullptr;
}

static void set_ui_visible(bool visible) {
    lv_lock();
    display_manager.set_visible(s_ui, visible);
    lv_unlock();
    if (!visible) bsp_display_wait_draw();
}

void panel_lock() {
    xSemaphoreTake(s_lock, portMAX_DELAY);
}

void panel_unlock() {
    xSemaphoreGive(s_lock);
}

void panel_acquire() {
    set_ui_visible(false);
    // A partial LVGL display always presents framebuffer 0.
    s_front = 0;
    s_acquired = true;
    create_pipeline();
}

void panel_release() {
    delete_pipeline();
    // The UI's partial blits land in the framebuffer last presented, then
    // present 0; anything else on screen would hide them.
    display_manager.present(0);
    s_acquired = false;
    set_ui_visible(true);
}

esp_err_t panel_set_format(bsp_pixel_format_t format) {
    panel_lock();
    if (format == s_format) {
        panel_unlock();
        return ESP_OK;
    }
    if (s_acquired) {
        delete_pipeline();
    } else {
        set_ui_visible(false);
    }
    esp_err_t err = bsp_display_reconfigure(format, 0);
    lv_lock();
    display_manager.set_color_format(s_ui);
    lv_unlock();
    s_format = bsp_display_get_pixel_format();
    s_stride = s_size.width * bsp_pixel_format_bytes(s_format);
    if (s_acquired) {
        s_front = 0;
        create_pipeline();
    } else {
        set_ui_visible(true);
    }
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "reconfigure: %s", esp_err_to_name(err));
    } else if (s_format_listener) {
        s_format_listener(s_format, s_format_listener_arg);
    }
    panel_unlock();
    return err;
}

void panel_set_format_listener(PanelFormatListener listener, void *arg) {
    panel_lock();
    s_format_listener = listener;
    s_format_listener_arg = arg;
    panel_unlock();
}

bool panel_acquired() {
    return s_acquired;
}

bsp_size_t panel_size() {
    return s_size;
}

bsp_pixel_format_t panel_format() {
    return s_format;
}

int panel_framebuffers() {
    return s_framebuffers;
}

void panel_swap() {
    s_front = (s_front + 1) % s_framebuffers;
    display_manager.present(s_front);
}

esp_err_t panel_fill(PanelBuffer buffer, bsp_rect_t area, uint32_t rgb888) {
    if (area.origin.x < 0 || area.origin.y < 0 || area.size.width <= 0 ||
        area.size.height <= 0 || area.origin.x + area.size.width > s_size.width ||
        area.origin.y + area.size.height > s_size.height) {
        return ESP_ERR_INVALID_ARG;
    }
    ppa_fill_oper_config_t config = {};
    config.out.buffer = framebuffer(buffer);
    config.out.buffer_size = s_stride * s_size.height;
    config.out.pic_w = s_size.width;
    config.out.pic_h = s_size.height;
    config.out.block_offset_x = area.origin.x;
    config.out.block_offset_y = area.origin.y;
    config.out.fill_cm = s_format == BSP_PIXEL_FORMAT_RGB888 ? PPA_FILL_COLOR_MODE_RGB888
                                                             : PPA_FILL_COLOR_MODE_RGB565;
    config.fill_block_w = area.size.width;
    config.fill_block_h = area.size.height;
    config.fill_argb_color.a = 0xff;
    config.fill_argb_color.r = rgb888 >> 16;
    config.fill_argb_color.g = rgb888 >> 8;
    config.fill_argb_color.b = rgb888;
    config.mode = PPA_TRANS_MODE_BLOCKING;
    return ppa_do_fill(s_fill, &config);
}

esp_err_t panel_write_rows(PanelBuffer buffer, int y, int height, const uint8_t *pixels) {
    if (!rows_valid(y, height)) return ESP_ERR_INVALID_ARG;
    const PanelBlit blit = {buffer, {s_size.width, height}, s_format, BSP_ROTATION_0,
                            {{0, y}, {s_size.width, height}}};
    return panel_blit(blit, pixels);
}

esp_err_t panel_write_lz4_rows(PanelBuffer buffer, int y, int height, const uint8_t *lz4,
                               uint32_t bytes) {
    if (!rows_valid(y, height) || y % kRowAlign || height % kRowAlign) {
        return ESP_ERR_INVALID_ARG;
    }
    uint8_t *dst = framebuffer(buffer) + y * s_stride;
    const size_t size = height * s_stride;
    const ptrdiff_t written = lz4_block_decompress(lz4, bytes, dst, size);
#ifdef ESP_PLATFORM
    esp_cache_msync(dst, size, ESP_CACHE_MSYNC_FLAG_DIR_C2M);
#endif
    if (written != (ptrdiff_t)size) {
        ESP_LOGW(TAG, "lz4 at %d: %d of %u bytes", y, (int)written, (unsigned)size);
        return ESP_ERR_INVALID_SIZE;
    }
    return ESP_OK;
}

esp_err_t panel_decode_rows(PanelBuffer buffer, int y, int height, const uint8_t *jpeg,
                            uint32_t bytes) {
    if (!rows_valid(y, height) || y % kRowAlign || height % kRowAlign) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_pipeline) return ESP_ERR_INVALID_STATE;
    jpeg_enh_frame_info_t info = {};
    esp_err_t err = jpeg_enh_decoder_process(jpeg_ppa_pipeline_get_decoder(s_pipeline), jpeg,
                                             bytes, framebuffer(buffer) + y * s_stride,
                                             height * s_stride, &info);
    if (err == ESP_OK && (info.origin_w != (uint32_t)s_size.width ||
                          info.origin_h != (uint32_t)height)) {
        err = ESP_ERR_INVALID_SIZE;
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "jpeg at %d: %s, %ux%u", y, esp_err_to_name(err),
                 (unsigned)info.origin_w, (unsigned)info.origin_h);
    }
    return err;
}

/* Sixteenths each input axis scales by; ESP_ERR_NOT_SUPPORTED when `area` is
 * not a whole number of sixteenths of the rotated source. */
static esp_err_t blit_scale(const PanelBlit &blit, uint32_t *scale_x, uint32_t *scale_y) {
    const bsp_rect_t &area = blit.area;
    if (blit.source.width <= 0 || blit.source.height <= 0 || area.origin.x < 0 ||
        area.origin.y < 0 || area.size.width <= 0 || area.size.height <= 0 ||
        area.origin.x + area.size.width > s_size.width ||
        area.origin.y + area.size.height > s_size.height) {
        return ESP_ERR_INVALID_ARG;
    }
    const bool transposed = blit.rotation == BSP_ROTATION_90 || blit.rotation == BSP_ROTATION_270;
    const uint32_t out_x = (transposed ? area.size.height : area.size.width) * kScaleDenominator;
    const uint32_t out_y = (transposed ? area.size.width : area.size.height) * kScaleDenominator;
    if (out_x % blit.source.width || out_y % blit.source.height) return ESP_ERR_NOT_SUPPORTED;
    *scale_x = out_x / blit.source.width;
    *scale_y = out_y / blit.source.height;
    return *scale_x && *scale_y ? ESP_OK : ESP_ERR_NOT_SUPPORTED;
}

/* Source rows [row, row + rows) as their own block, placed where they land
 * in the rotated, scaled source. */
static ppa_srm_oper_config_t strip_config(const PanelBlit &blit, uint32_t scale_x,
                                          uint32_t scale_y, const void *pixels, int row,
                                          int rows) {
    const bsp_rect_t &area = blit.area;
    const int start = row * scale_y / kScaleDenominator;
    const int extent = rows * scale_y / kScaleDenominator;
    int x = area.origin.x;
    int y = area.origin.y;
    switch (blit.rotation) {
        case BSP_ROTATION_90: x += start; break;
        case BSP_ROTATION_180: y += area.size.height - start - extent; break;
        case BSP_ROTATION_270: x += area.size.width - start - extent; break;
        default: y += start; break;
    }

    ppa_srm_oper_config_t config = {};
    config.in.buffer = pixels;
    config.in.pic_w = blit.source.width;
    config.in.pic_h = rows;
    config.in.block_w = blit.source.width;
    config.in.block_h = rows;
    config.in.srm_cm = srm_mode(blit.format);
    config.out.buffer = framebuffer(blit.buffer);
    config.out.buffer_size = s_stride * s_size.height;
    config.out.pic_w = s_size.width;
    config.out.pic_h = s_size.height;
    config.out.block_offset_x = x;
    config.out.block_offset_y = y;
    config.out.srm_cm = srm_mode(s_format);
    config.rotation_angle = srm_angle(blit.rotation);
    config.scale_x = (float)scale_x / kScaleDenominator;
    config.scale_y = (float)scale_y / kScaleDenominator;
    config.mode = PPA_TRANS_MODE_BLOCKING;
    return config;
}

esp_err_t panel_blit(const PanelBlit &blit, const uint8_t *pixels) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    const ppa_srm_oper_config_t config =
        strip_config(blit, scale_x, scale_y, pixels, 0, blit.source.height);
    return ppa_do_scale_rotate_mirror(s_copy, &config);
}

esp_err_t panel_blit_lz4(const PanelBlit &blit, const uint8_t *lz4, uint32_t bytes) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    const size_t row_bytes = blit.source.width * bsp_pixel_format_bytes(blit.format);
    const int strip_rows = s_strip_bytes / row_bytes / kRowAlign * kRowAlign;
    // Every match reaches back at most 64 KiB, so a whole strip before the
    // one being written is all the history it needs.
    if (strip_rows * row_bytes < kLz4Window) return ESP_ERR_NOT_SUPPORTED;

    lz4_stream_t stream;
    lz4_stream_init(&stream, lz4, bytes);
    size_t previous = 0;
    int strip = 0;
    for (int row = 0; row < blit.source.height && err == ESP_OK; row += strip_rows) {
        const int rows = std::min(strip_rows, blit.source.height - row);
        const size_t size = rows * row_bytes;
        wait_strip(strip);
        const ptrdiff_t written = lz4_stream_decompress(&stream, s_strips[strip], size,
                                                        s_strips[strip ^ 1], previous);
        if (written != (ptrdiff_t)size) {
            err = ESP_ERR_INVALID_SIZE;
            break;
        }
        previous = size;
        ppa_srm_oper_config_t config =
            strip_config(blit, scale_x, scale_y, s_strips[strip], row, rows);
        config.mode = PPA_TRANS_MODE_NON_BLOCKING;
        config.user_data = s_strip_done[strip];
        s_strip_busy[strip] = true;
        err = ppa_do_scale_rotate_mirror(s_copy, &config);
        if (err != ESP_OK) s_strip_busy[strip] = false;
        strip ^= 1;
    }
    for (int i = 0; i < kStrips; i++) wait_strip(i);
    if (err == ESP_OK && !lz4_stream_finished(&stream)) err = ESP_ERR_INVALID_SIZE;
    if (err != ESP_OK) ESP_LOGW(TAG, "lz4 blit: %s", esp_err_to_name(err));
    return err;
}

esp_err_t panel_blit_jpeg(const PanelBlit &blit, const uint8_t *jpeg, uint32_t bytes) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    if (!s_pipeline) return ESP_ERR_INVALID_STATE;

    jpeg_ppa_output_t out = {};
    out.buffer = framebuffer(blit.buffer);
    out.buffer_size = s_stride * s_size.height;
    out.pic_w = s_size.width;
    out.pic_h = s_size.height;
    out.color_mode = srm_mode(s_format);
    jpeg_ppa_transform_t transform = {};
    transform.rotation = srm_angle(blit.rotation);
    transform.scale_x = (float)scale_x / kScaleDenominator;
    transform.scale_y = (float)scale_y / kScaleDenominator;
    transform.out_offset_x = blit.area.origin.x;
    transform.out_offset_y = blit.area.origin.y;
    jpeg_enh_frame_info_t info = {};
    err = jpeg_ppa_pipeline_process(s_pipeline, jpeg, bytes, &out, &transform, &info);
    if (err == ESP_OK && (info.origin_w != (uint32_t)blit.source.width ||
                          info.origin_h != (uint32_t)blit.source.height)) {
        err = ESP_ERR_INVALID_SIZE;
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "jpeg blit: %s, %ux%u", esp_err_to_name(err), (unsigned)info.origin_w,
                 (unsigned)info.origin_h);
    }
    return err;
}
