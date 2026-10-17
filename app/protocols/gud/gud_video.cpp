/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "gud_video.hpp"
#include <cstring>
#include "display/panel.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"
#include "lz4_block.h"

static const char *TAG = "gud_video";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 8;
static constexpr size_t kScratchAlignment = 64;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

GudVideo::~GudVideo() {
    if (queue_) {
        send(Op::Quit);
        xSemaphoreTake(exited_, portMAX_DELAY);
        vQueueDelete(queue_);
    }
    if (exited_) vSemaphoreDelete(exited_);
    heap_caps_free(scratch_);
}

esp_err_t GudVideo::start(bsp_rotation_t rotation, bsp_pixel_format_t format,
                          uint32_t max_buffer_bytes) {
    rotation_ = rotation;
    format_ = format;
    scratch_ = static_cast<uint8_t *>(
        heap_caps_aligned_alloc(kScratchAlignment, max_buffer_bytes, MALLOC_CAP_SPIRAM));
    scratch_bytes_ = max_buffer_bytes;
    queue_ = xQueueCreate(kQueueLength, sizeof(Item));
    exited_ = xSemaphoreCreateBinary();
    if (!scratch_ || !queue_ || !exited_) return ESP_ERR_NO_MEM;
    if (xTaskCreatePinnedToCore(task, "gud_video", kTaskStackBytes, this, kTaskPriority, nullptr,
                                kTaskCore) != pdPASS) {
        vQueueDelete(queue_);
        queue_ = nullptr;
        return ESP_ERR_NO_MEM;
    }
    panel_set_format_listener(format_changed, this);
    panel_set_clip_listener(clip_changed, this);
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
    item.data = data;
    item.length = length;
    item.done = done;
    item.context = context;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void GudVideo::end() {
    send(Op::End);
}

// The panel listeners run holding the panel lock, so they only flag the redraw.
void GudVideo::refresh() {
    invalidated_ = true;
    Item item = {};
    item.op = Op::Refresh;
    xQueueSend(queue_, &item, 0);
}

void GudVideo::format_changed(bsp_pixel_format_t, void *arg) {
    static_cast<GudVideo *>(arg)->refresh();
}

void GudVideo::clip_changed(const bsp_rect_t *, bool, void *arg) {
    static_cast<GudVideo *>(arg)->refresh();
}

void GudVideo::task(void *arg) {
    auto *self = static_cast<GudVideo *>(arg);
    Item item;
    do {
        xQueueReceive(self->queue_, &item, portMAX_DELAY);
        self->run(item);
    } while (item.op != Op::Quit);
    panel_set_format_listener(nullptr, nullptr);
    panel_set_clip_listener(nullptr, nullptr);
    xSemaphoreGive(self->exited_);
    vTaskDelete(nullptr);
}

bool GudVideo::fits(bsp_rotation_t rotation) const {
    const bsp_size_t panel = panel_logical_size(rotation);
    const bsp_size_t size = shadow_.size();
    return panel.width == size.width && panel.height == size.height;
}

void GudVideo::run(const Item &item) {
    switch (item.op) {
        case Op::Mode: {
            const bsp_size_t size = {(int)(item.value >> 16), (int)(item.value & 0xffff)};
            has_mode_ = shadow_.resize(size, format_) == ESP_OK;
            if (fits(rotation_)) {
                shown_rotation_ = rotation_;
            } else {
                shown_rotation_ = (bsp_rotation_t)((rotation_ + 1) % 4);
            }
            ESP_LOGI(TAG, "mode %dx%d, rotation %d", size.width, size.height, shown_rotation_);
            break;
        }
        case Op::Enable:
            enabled_ = item.value != 0;
            break;
        case Op::Rotate:
            rotation_ = (bsp_rotation_t)item.value;
            if (rotation_ != shown_rotation_ && fits(rotation_)) {
                shown_rotation_ = rotation_;
                shadow_.invalidate();
            }
            break;
        case Op::Buffer:
            apply(item);
            item.done(item.context);
            break;
        case Op::End:
        case Op::Quit:
            enabled_ = false;
            break;
        case Op::Refresh:
            break;
    }
    update_panel();
}

void GudVideo::update_panel() {
    const bool show = enabled_ && has_mode_;
    if (show != acquired_) {
        if (show) {
            panel_acquire();
            shadow_.invalidate();
            ESP_LOGI(TAG, "display started");
        } else {
            panel_release();
            stats_start_us_ = 0;
            ESP_LOGI(TAG, "display stopped");
        }
        acquired_ = show;
        streaming_(show);
    }
    if (invalidated_.exchange(false)) shadow_.invalidate();
    if (!acquired_ || !shadow_.dirty() || uxQueueMessagesWaiting(queue_)) return;
    panel_lock();
    shadow_.present(shown_rotation_);
    panel_unlock();
    stats_presents_++;
}

void GudVideo::apply(const Item &item) {
    const gud::SetBuffer &buffer = item.buffer;
    const bsp_size_t size = shadow_.size();
    if (!has_mode_ || buffer.x + buffer.width > (uint32_t)size.width ||
        buffer.y + buffer.height > (uint32_t)size.height) {
        ESP_LOGW(TAG, "%ux%u at %u,%u is outside %dx%d", (unsigned)buffer.width,
                 (unsigned)buffer.height, (unsigned)buffer.x, (unsigned)buffer.y, size.width,
                 size.height);
        return;
    }
    count(item.length);
    const size_t pixel_bytes = bsp_pixel_format_bytes(format_);
    const size_t row_bytes = buffer.width * pixel_bytes;
    const size_t bytes = row_bytes * buffer.height;
    const uint8_t *rows = item.data;
    if (buffer.compression & gud::kCompressionLz4) {
        const bool whole_rows = buffer.width == (uint32_t)size.width;
        uint8_t *dst = whole_rows ? shadow_.row(buffer.y) : scratch_;
        if (!whole_rows && bytes > scratch_bytes_) {
            ESP_LOGW(TAG, "%u bytes do not fit the scratch buffer", (unsigned)bytes);
            return;
        }
        const ptrdiff_t written = lz4_block_decompress(item.data, item.length, dst, bytes);
        if (written != (ptrdiff_t)bytes) {
            ESP_LOGW(TAG, "lz4: %d of %u bytes", (int)written, (unsigned)bytes);
            return;
        }
        if (whole_rows) {
            shadow_.damage(buffer.y, buffer.height);
            return;
        }
        rows = scratch_;
    } else if (item.length != bytes) {
        ESP_LOGW(TAG, "%u bytes for %ux%u", (unsigned)item.length, (unsigned)buffer.width,
                 (unsigned)buffer.height);
        return;
    }
    for (uint32_t y = 0; y < buffer.height; y++) {
        memcpy(shadow_.row(buffer.y + y) + buffer.x * pixel_bytes, rows + y * row_bytes,
               row_bytes);
    }
    shadow_.damage(buffer.y, buffer.height);
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
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_presents_ = 0;
}
