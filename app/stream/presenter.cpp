/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "presenter.hpp"
#include "bsp.h"
#include "display_manager.hpp"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "jpeg_decode_enhanced.h"
#include "lvgl.hpp"

static const char *TAG = "presenter";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

static lv_display_t *s_ui;
static jpeg_enh_strip_decoder_handle_t s_decoder;
static portMUX_TYPE s_lock = portMUX_INITIALIZER_UNLOCKED;
static StreamFrame *s_pending;
static bool s_end_pending;
static SemaphoreHandle_t s_wake;
static SemaphoreHandle_t s_decoding;

static bsp_size_t s_panel;
static size_t s_stride;
static int s_framebuffers;
static int s_shown;
static bool s_active;

static uint32_t s_stats_frames;
static uint32_t s_stats_failed;
static int64_t s_stats_decode_us;
static int64_t s_stats_start_us;

static void release(StreamFrame *frame) {
    if (frame && frame->release) frame->release(frame);
}

static void activate() {
    lv_lock();
    display_manager.set_visible(s_ui, false);
    lv_unlock();
    bsp_display_wait_draw();
    s_shown = 0;
    s_active = true;
    s_stats_start_us = esp_timer_get_time();
    ESP_LOGI(TAG, "stream started");
}

static void deactivate() {
    s_active = false;
    lv_lock();
    display_manager.set_visible(s_ui, true);
    lv_obj_invalidate(lv_display_get_screen_active(s_ui));
    lv_unlock();
    ESP_LOGI(TAG, "stream stopped");
}

static bool decode(const StreamFrame &frame, uint8_t *framebuffer) {
    for (int i = 0; i < frame.count; i++) {
        const StreamBand &band = frame.bands[i];
        if (band.y + band.height > s_panel.height) return false;
        jpeg_enh_frame_info_t info = {};
        esp_err_t err = jpeg_enh_decoder_process(s_decoder, band.data, band.size,
                                                 framebuffer + band.y * s_stride,
                                                 band.height * s_stride, &info);
        if (err != ESP_OK || info.origin_w != (uint32_t)s_panel.width ||
            info.origin_h != band.height) {
            ESP_LOGW(TAG, "band at %u: %s, %ux%u", band.y, esp_err_to_name(err),
                     (unsigned)info.origin_w, (unsigned)info.origin_h);
            return false;
        }
    }
    return true;
}

static void log_stats(int64_t now) {
    if (now - s_stats_start_us < kStatsPeriodUs) return;
    const double seconds = (now - s_stats_start_us) / 1e6;
    ESP_LOGI(TAG, "%.1f fps, decode %.1f ms, %u failed", s_stats_frames / seconds,
             s_stats_frames ? s_stats_decode_us / 1000.0 / s_stats_frames : 0.0,
             (unsigned)s_stats_failed);
    s_stats_frames = 0;
    s_stats_failed = 0;
    s_stats_decode_us = 0;
    s_stats_start_us = now;
}

static void presenter_task(void *) {
    while (true) {
        xSemaphoreTake(s_wake, portMAX_DELAY);
        xSemaphoreTake(s_decoding, portMAX_DELAY);
        portENTER_CRITICAL(&s_lock);
        StreamFrame *frame = s_pending;
        const bool end = s_end_pending;
        s_pending = nullptr;
        s_end_pending = false;
        portEXIT_CRITICAL(&s_lock);

        if (!frame) {
            xSemaphoreGive(s_decoding);
            if (end && s_active) deactivate();
            continue;
        }
        if (!s_active) activate();
        const int64_t now = esp_timer_get_time();

        const int next = (s_shown + 1) % s_framebuffers;
        auto *framebuffer = static_cast<uint8_t *>(bsp_display_get_frame_buffer(next));
        const bool ok = decode(*frame, framebuffer);
        release(frame);
        xSemaphoreGive(s_decoding);

        const int64_t done = esp_timer_get_time();
        if (ok) {
            display_manager.present(next);
            s_shown = next;
            s_stats_frames++;
            s_stats_decode_us += done - now;
        } else {
            s_stats_failed++;
        }
        log_stats(done);
    }
}

esp_err_t presenter_init(lv_display_t *ui) {
    s_ui = ui;
    s_panel = bsp_display_get_size();
    const bsp_pixel_format_t format = bsp_display_get_pixel_format();
    s_stride = s_panel.width * bsp_pixel_format_bytes(format);
    while (s_framebuffers < BSP_DISPLAY_MAX_FRAME_BUFFERS &&
           bsp_display_get_frame_buffer(s_framebuffers)) {
        s_framebuffers++;
    }
    if (s_framebuffers < 2) {
        ESP_LOGE(TAG, "needs two framebuffers, has %d", s_framebuffers);
        return ESP_ERR_INVALID_STATE;
    }

    jpeg_enh_strip_decoder_cfg_t config = {};
    config.decode.output_format = format == BSP_PIXEL_FORMAT_RGB888
        ? JPEG_DECODE_OUT_FORMAT_RGB888 : JPEG_DECODE_OUT_FORMAT_RGB565;
    config.decode.rgb_order = JPEG_DEC_RGB_ELEMENT_ORDER_BGR;
    config.decode.conv_std = JPEG_YUV_RGB_CONV_STD_BT601;
    config.decode.yuv_full_range = true;
    esp_err_t err = jpeg_enh_strip_decoder_new(&config, &s_decoder);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "jpeg decoder: %s", esp_err_to_name(err));
        return err;
    }

    s_wake = xSemaphoreCreateBinary();
    s_decoding = xSemaphoreCreateMutex();
    if (!s_wake || !s_decoding) return ESP_ERR_NO_MEM;
    if (xTaskCreatePinnedToCore(presenter_task, "presenter", kTaskStackBytes, nullptr,
                                kTaskPriority, nullptr, kTaskCore) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void presenter_submit(StreamFrame *frame) {
    if (!s_wake) {
        release(frame);
        return;
    }
    portENTER_CRITICAL(&s_lock);
    StreamFrame *replaced = s_pending;
    s_pending = frame;
    s_end_pending = false;
    portEXIT_CRITICAL(&s_lock);
    release(replaced);
    xSemaphoreGive(s_wake);
}

void presenter_end() {
    if (!s_wake) return;
    portENTER_CRITICAL(&s_lock);
    StreamFrame *pending = s_pending;
    s_pending = nullptr;
    s_end_pending = true;
    portEXIT_CRITICAL(&s_lock);
    release(pending);
    xSemaphoreGive(s_wake);
}

void presenter_flush() {
    if (!s_wake) return;
    portENTER_CRITICAL(&s_lock);
    StreamFrame *pending = s_pending;
    s_pending = nullptr;
    portEXIT_CRITICAL(&s_lock);
    release(pending);
    xSemaphoreTake(s_decoding, portMAX_DELAY);
    xSemaphoreGive(s_decoding);
}
