/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel.hpp"
#include "display_manager.hpp"
#include "driver/ppa.h"
#include "esp_log.h"
#include "jpeg_decode_enhanced.h"
#include "lvgl.hpp"

static const char *TAG = "panel";

static constexpr int kRowAlign = 16;

static lv_display_t *s_ui;
static jpeg_enh_strip_decoder_handle_t s_decoder;
static ppa_client_handle_t s_fill;
static ppa_client_handle_t s_copy;

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

esp_err_t panel_init(lv_display_t *ui) {
    s_ui = ui;
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

    jpeg_enh_strip_decoder_cfg_t config = {};
    config.decode.output_format = s_format == BSP_PIXEL_FORMAT_RGB888
        ? JPEG_DECODE_OUT_FORMAT_RGB888 : JPEG_DECODE_OUT_FORMAT_RGB565;
    config.decode.rgb_order = JPEG_DEC_RGB_ELEMENT_ORDER_BGR;
    config.decode.conv_std = JPEG_YUV_RGB_CONV_STD_BT601;
    config.decode.yuv_full_range = true;
    esp_err_t err = jpeg_enh_strip_decoder_new(&config, &s_decoder);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "jpeg decoder: %s", esp_err_to_name(err));
        return err;
    }

    ppa_client_config_t client = {};
    client.oper_type = PPA_OPERATION_FILL;
    err = ppa_register_client(&client, &s_fill);
    if (err == ESP_OK) {
        client.oper_type = PPA_OPERATION_SRM;
        err = ppa_register_client(&client, &s_copy);
    }
    if (err != ESP_OK) ESP_LOGE(TAG, "ppa client: %s", esp_err_to_name(err));
    return err;
}

void panel_acquire() {
    lv_lock();
    display_manager.set_visible(s_ui, false);
    lv_unlock();
    bsp_display_wait_draw();
    // A partial LVGL display always presents framebuffer 0.
    s_front = 0;
    s_acquired = true;
}

void panel_release() {
    // The UI's partial blits land in the framebuffer last presented, then
    // present 0; anything else on screen would hide them.
    display_manager.present(0);
    s_acquired = false;
    lv_lock();
    display_manager.set_visible(s_ui, true);
    lv_unlock();
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
    const ppa_srm_color_mode_t mode = s_format == BSP_PIXEL_FORMAT_RGB888
        ? PPA_SRM_COLOR_MODE_RGB888 : PPA_SRM_COLOR_MODE_RGB565;
    ppa_srm_oper_config_t config = {};
    config.in.buffer = pixels;
    config.in.pic_w = s_size.width;
    config.in.pic_h = height;
    config.in.block_w = s_size.width;
    config.in.block_h = height;
    config.in.srm_cm = mode;
    config.out.buffer = framebuffer(buffer);
    config.out.buffer_size = s_stride * s_size.height;
    config.out.pic_w = s_size.width;
    config.out.pic_h = s_size.height;
    config.out.block_offset_y = y;
    config.out.srm_cm = mode;
    config.rotation_angle = PPA_SRM_ROTATION_ANGLE_0;
    config.scale_x = 1.0f;
    config.scale_y = 1.0f;
    config.mode = PPA_TRANS_MODE_BLOCKING;
    return ppa_do_scale_rotate_mirror(s_copy, &config);
}

esp_err_t panel_decode_rows(PanelBuffer buffer, int y, int height, const uint8_t *jpeg,
                            uint32_t bytes) {
    if (!rows_valid(y, height) || y % kRowAlign || height % kRowAlign) {
        return ESP_ERR_INVALID_ARG;
    }
    jpeg_enh_frame_info_t info = {};
    esp_err_t err = jpeg_enh_decoder_process(s_decoder, jpeg, bytes,
                                             framebuffer(buffer) + y * s_stride,
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
