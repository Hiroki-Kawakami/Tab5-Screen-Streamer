/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "gud_video.hpp"
#include "display/panel.hpp"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"

static const char *TAG = "gud_video";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 8;
static constexpr TickType_t kContinuationWait = pdMS_TO_TICKS(20);
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

GudVideo::~GudVideo() {
    if (queue_) {
        send(Op::Quit);
        xSemaphoreTake(exited_, portMAX_DELAY);
        vQueueDelete(queue_);
    }
    if (exited_) vSemaphoreDelete(exited_);
}

esp_err_t GudVideo::start(bsp_rotation_t rotation, bsp_pixel_format_t format,
                          uint32_t max_buffer_bytes) {
    rotation_ = rotation;
    format_ = format;
    max_buffer_bytes_ = max_buffer_bytes;
    queue_ = xQueueCreate(kQueueLength, sizeof(Item));
    exited_ = xSemaphoreCreateBinary();
    if (!queue_ || !exited_) return ESP_ERR_NO_MEM;
    if (xTaskCreatePinnedToCore(task, "gud_video", kTaskStackBytes, this, kTaskPriority, nullptr,
                                kTaskCore) != pdPASS) {
        vQueueDelete(queue_);
        queue_ = nullptr;
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void GudVideo::send(Op op, uint32_t value) {
    Item item = {};
    item.op = op;
    item.value = value;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void GudVideo::set_mode(bsp_size_t size) {
    send(Op::Mode, (uint32_t)size.width << 16 | (uint32_t)size.height);
}

void GudVideo::set_enabled(bool enabled) {
    send(Op::Enable, enabled);
}

void GudVideo::rotate(bsp_rotation_t rotation) {
    send(Op::Rotate, rotation);
}

void GudVideo::submit(const gud::SetBuffer &buffer, const uint8_t *data, uint32_t length,
                      Done done, void *context) {
    Item item = {};
    item.op = Op::Buffer;
    item.buffer = buffer;
    item.rows = buffer.height;
    item.last = true;
    item.data = data;
    item.length = length;
    item.done = done;
    item.context = context;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void GudVideo::submit_rows(const gud::SetBuffer &buffer, uint32_t row, uint32_t rows, bool last,
                           const uint8_t *data, Done done, void *context) {
    Item item = {};
    item.op = Op::Buffer;
    item.buffer = buffer;
    item.row = row;
    item.rows = rows;
    item.last = last;
    item.data = data;
    item.length = buffer.width * rows * bsp_pixel_format_bytes(format_);
    item.done = done;
    item.context = context;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void GudVideo::end() {
    send(Op::End);
}

void GudVideo::task(void *arg) {
    auto *self = static_cast<GudVideo *>(arg);
    Item item;
    do {
        const TickType_t wait = self->more_ ? kContinuationWait : portMAX_DELAY;
        if (xQueueReceive(self->queue_, &item, wait) != pdTRUE) {
            self->present();
            continue;
        }
        const int64_t diag_start = esp_timer_get_time();
        self->run(item);
        self->diag_busy_us_ += esp_timer_get_time() - diag_start;
    } while (item.op != Op::Quit);
    xSemaphoreGive(self->exited_);
    vTaskDelete(nullptr);
}

bool GudVideo::fits(bsp_rotation_t rotation) const {
    const bsp_size_t panel = panel_logical_size(rotation);
    return panel.width == mode_.width && panel.height == mode_.height;
}

bool GudVideo::continues(const gud::SetBuffer &buffer) const {
    const uint32_t row_bytes = buffer.width * bsp_pixel_format_bytes(format_);
    return buffer.height == max_buffer_bytes_ / row_bytes &&
           buffer.y + buffer.height < (uint32_t)mode_.height;
}

void GudVideo::run(const Item &item) {
    // What was drawn shows once the host is done with the area: the rest of a
    // buffer it split into rows is waited for, for a while.
    if (item.op == Op::Buffer) {
        draw(item);
        item.done(item.context);
        if (!more_) present();
        return;
    }
    present();
    switch (item.op) {
        case Op::Mode:
            mode_ = {(int)(item.value >> 16), (int)(item.value & 0xffff)};
            shown_rotation_ = fits(rotation_) ? rotation_ : (bsp_rotation_t)((rotation_ + 1) % 4);
            ESP_LOGI(TAG, "mode %dx%d, rotation %d", mode_.width, mode_.height, shown_rotation_);
            if (acquired_) reset_panel();
            break;
        case Op::Enable:
            enabled_ = item.value != 0;
            break;
        case Op::Rotate:
            rotation_ = (bsp_rotation_t)item.value;
            if (rotation_ == shown_rotation_ || !fits(rotation_)) break;
            shown_rotation_ = rotation_;
            if (!acquired_) break;
            panel_lock();
            if (panel_rotate_180() == ESP_OK) {
                damage_.invalidate();
            } else {
                ESP_LOGW(TAG, "rotation failed, clearing");
                panel_clear();
                damage_.reset();
            }
            panel_unlock();
            break;
        case Op::End:
        case Op::Quit:
            enabled_ = false;
            break;
        case Op::Buffer:
            break;
    }
    update_panel();
}

void GudVideo::reset_panel() {
    panel_lock();
    panel_clear();
    damage_.reset();
    panel_unlock();
}

void GudVideo::update_panel() {
    const bool show = enabled_ && mode_.width;
    if (show == acquired_) return;
    if (show) {
        panel_acquire();
        reset_panel();
        drawing_ = true;
        ESP_LOGI(TAG, "display started");
    } else {
        drawing_ = false;
        panel_release();
        damage_.reset();
        stats_start_us_ = 0;
        ESP_LOGI(TAG, "display stopped");
    }
    acquired_ = show;
    streaming_(show);
}

void GudVideo::present() {
    if (!damage_.pending()) return;
    const int64_t diag_start = esp_timer_get_time();
    panel_lock();
    damage_.present();
    panel_unlock();
    diag_present_us_ += esp_timer_get_time() - diag_start;
    if (diag_first_us_) diag_latency_us_ += diag_start - diag_first_us_;
    diag_first_us_ = 0;
    more_ = false;
    stats_presents_++;
}

void GudVideo::draw(const Item &item) {
    const gud::SetBuffer &buffer = item.buffer;
    if (!acquired_ || buffer.x + buffer.width > (uint32_t)mode_.width ||
        buffer.y + buffer.height > (uint32_t)mode_.height) {
        ESP_LOGW(TAG, "%ux%u at %u,%u is outside %dx%d", (unsigned)buffer.width,
                 (unsigned)buffer.height, (unsigned)buffer.x, (unsigned)buffer.y, mode_.width,
                 mode_.height);
        return;
    }
    count(item.length);
    const int64_t diag_start = esp_timer_get_time();
    if (!diag_first_us_) diag_first_us_ = diag_start;
    const bsp_rect_t rect = {{(int)buffer.x, (int)(buffer.y + item.row)},
                             {(int)buffer.width, (int)item.rows}};
    PanelBlit blit = {};
    blit.buffer = PanelBuffer::Back;
    blit.source = rect.size;
    blit.format = format_;
    blit.rotation = shown_rotation_;
    blit.area = panel_native_rect(rect, shown_rotation_);
    const uint32_t bytes = buffer.width * item.rows * bsp_pixel_format_bytes(format_);
    diag_raw_bytes_ += bytes;
    diag_rects_++;

    panel_lock();
    esp_err_t err = damage_.drawing(blit.area);
    if (err == ESP_OK) {
        if (buffer.compression & gud::kCompressionLz4) {
            err = panel_blit_lz4(blit, item.data, item.length);
        } else if (item.length == bytes) {
            err = panel_blit(blit, item.data);
        } else {
            err = ESP_ERR_INVALID_SIZE;
        }
    }
    damage_.drawn(blit.area);
    panel_unlock();
    diag_draw_us_ += esp_timer_get_time() - diag_start;
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "%ux%u at %u,%u: %s", (unsigned)buffer.width, (unsigned)buffer.height,
                 (unsigned)buffer.x, (unsigned)buffer.y, esp_err_to_name(err));
    }
    more_ = !item.last || continues(buffer);
}

void GudVideo::count(uint32_t bytes) {
    stats_bytes_ += bytes;
    const int64_t now = esp_timer_get_time();
    if (!stats_start_us_) {
        stats_start_us_ = now;
        stats_bytes_ = bytes;
        stats_presents_ = 0;
    }
    if (now - stats_start_us_ < kStatsPeriodUs) return;
    const double seconds = (now - stats_start_us_) / 1e6;
    ESP_LOGI(TAG, "%.1f presents/s, %.2f MB/s", stats_presents_ / seconds,
             stats_bytes_ / seconds / 1e6);
    ESP_LOGI(TAG, "DIAG rects %u, raw %.2f MB/s, draw %.1f ms/s (%.0f MB/s), present %.1f ms/s "
             "(%.2f ms each), draw-to-present %.1f ms, busy %.0f%%, lz4 %.1f ms/s (%.0f MB/s)",
             (unsigned)diag_rects_, diag_raw_bytes_ / seconds / 1e6, diag_draw_us_ / 1e3 / seconds,
             diag_draw_us_ ? diag_raw_bytes_ / (double)diag_draw_us_ : 0.0,
             diag_present_us_ / 1e3 / seconds,
             stats_presents_ ? diag_present_us_ / 1e3 / stats_presents_ : 0.0,
             stats_presents_ ? diag_latency_us_ / 1e3 / stats_presents_ : 0.0,
             diag_busy_us_ / 1e4 / seconds, panel_diag_lz4_us / 1e3 / seconds,
             panel_diag_lz4_us ? diag_raw_bytes_ / (double)panel_diag_lz4_us : 0.0);
    panel_diag_lz4_us = 0;
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_presents_ = 0;
    diag_raw_bytes_ = 0;
    diag_rects_ = 0;
    diag_draw_us_ = 0;
    diag_present_us_ = 0;
    diag_latency_us_ = 0;
    diag_busy_us_ = 0;
}
