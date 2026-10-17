/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "displaylink_video.hpp"
#include <algorithm>
#include <cstring>
#include "display/panel.hpp"
#include "displaylink.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"

static const char *TAG = "displaylink_video";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 16;
static constexpr size_t kVramAlignment = 64;
static constexpr int64_t kMaxPresentDelayUs = 33 * 1000;
static constexpr uint32_t kAddressMask = 0xffffff;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

using displaylink::Command;

DisplayLinkVideo::~DisplayLinkVideo() {
    if (queue_) {
        send(Op::Quit);
        xSemaphoreTake(exited_, portMAX_DELAY);
        vQueueDelete(queue_);
    }
    if (exited_) vSemaphoreDelete(exited_);
    heap_caps_free(vram_);
}

esp_err_t DisplayLinkVideo::start(bsp_rotation_t rotation) {
    rotation_ = rotation;
    queue_ = xQueueCreate(kQueueLength, sizeof(Item));
    exited_ = xSemaphoreCreateBinary();
    if (!queue_ || !exited_) return ESP_ERR_NO_MEM;
    if (xTaskCreatePinnedToCore(task, "dl_video", kTaskStackBytes, this, kTaskPriority, nullptr,
                                kTaskCore) != pdPASS) {
        vQueueDelete(queue_);
        queue_ = nullptr;
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void DisplayLinkVideo::send(Op op, uint32_t value) {
    Item item = {};
    item.op = op;
    item.value = value;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void DisplayLinkVideo::rotate(bsp_rotation_t rotation) {
    send(Op::Rotate, rotation);
}

void DisplayLinkVideo::submit(const uint8_t *data, uint32_t length, Done done, void *context) {
    Item item = {};
    item.op = Op::Data;
    item.data = data;
    item.length = length;
    item.done = done;
    item.context = context;
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void DisplayLinkVideo::end() {
    send(Op::End);
}

void DisplayLinkVideo::task(void *arg) {
    auto *self = static_cast<DisplayLinkVideo *>(arg);
    Item item;
    do {
        xQueueReceive(self->queue_, &item, portMAX_DELAY);
        const int64_t start = esp_timer_get_time();
        self->run(item);
        self->stats_busy_us_ += esp_timer_get_time() - start;
    } while (item.op != Op::Quit);
    xSemaphoreGive(self->exited_);
    vTaskDelete(nullptr);
}

bool DisplayLinkVideo::fits(bsp_rotation_t rotation) const {
    const bsp_size_t panel = panel_logical_size(rotation);
    return panel.width == mode_.width && panel.height == mode_.height;
}

// The stream has no frames: what changed shows once the host pauses, or
// after a while of it sending on.
void DisplayLinkVideo::run(const Item &item) {
    switch (item.op) {
        case Op::Data: {
            parse(item.data, item.length);
            item.done(item.context);
            stats_bytes_ += item.length;
            const bool pending = dirty_first_ <= dirty_last_;
            if (pending && (!uxQueueMessagesWaiting(queue_) ||
                            esp_timer_get_time() - dirty_since_us_ >= kMaxPresentDelayUs)) {
                present();
            }
            break;
        }
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
            reset_stream();
            mode_ = {};
            update_panel();
            break;
    }

    const int64_t now = esp_timer_get_time();
    if (!stats_start_us_) stats_start_us_ = now;
    if (now - stats_start_us_ < kStatsPeriodUs || !stats_bytes_) return;
    const double seconds = (now - stats_start_us_) / 1e6;
    ESP_LOGI(TAG, "%.1f presents/s, %.2f MB/s, busy %.0f%%", stats_presents_ / seconds,
             stats_bytes_ / seconds / 1e6, stats_busy_us_ / 1e4 / seconds);
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_presents_ = 0;
    stats_busy_us_ = 0;
}

void DisplayLinkVideo::reset_stream() {
    state_ = State::Idle;
    header_bytes_ = 0;
    pixel_half_ = false;
}

// RAW: pixels. RL: runs of a count and a pixel. RLX: spans of a count and
// raw pixels, each but the one ending the command followed by how many more
// times its last pixel repeats. A count of 0 is 256.
void DisplayLinkVideo::parse(const uint8_t *data, uint32_t length) {
    for (const uint8_t *p = data, *end = data + length; p < end; p++) {
        const uint8_t byte = *p;
        switch (state_) {
            case State::Idle:
                if (byte == displaylink::kCommandStart) state_ = State::Command;
                break;
            case State::Command:
                if (byte == displaylink::kCommandStart) break;
                command_ = byte;
                header_bytes_ = 0;
                switch (static_cast<Command>(byte)) {
                    case Command::WriteRegister:
                        header_length_ = 2;
                        break;
                    case Command::WriteRaw8:
                    case Command::WriteRl8:
                    case Command::WriteRlx8:
                    case Command::WriteRaw16:
                    case Command::WriteRl16:
                    case Command::WriteRlx16:
                        header_length_ = 4;
                        break;
                    case Command::WriteCopy8:
                    case Command::WriteCopy16:
                        header_length_ = 7;
                        break;
                    case Command::Sync:
                        state_ = State::Idle;
                        continue;
                    default:
                        ESP_LOGW(TAG, "unknown command 0x%02x", byte);
                        state_ = State::Idle;
                        continue;
                }
                state_ = State::Header;
                break;
            case State::Header:
                header_[header_bytes_++] = byte;
                if (header_bytes_ == header_length_) begin_command();
                break;
            case State::RawPixels:
                if (!pixel_byte(byte)) break;
                put(pixel_);
                if (written_ == total_) state_ = State::Idle;
                break;
            case State::RunCount:
                count_ = byte ? byte : 256;
                state_ = State::RunPixel;
                break;
            case State::RunPixel:
                if (!pixel_byte(byte)) break;
                while (count_-- && written_ < total_) put(pixel_);
                state_ = written_ < total_ ? State::RunCount : State::Idle;
                break;
            case State::SpanCount:
                count_ = byte ? byte : 256;
                state_ = State::SpanPixels;
                break;
            case State::SpanPixels:
                if (!pixel_byte(byte)) break;
                put(pixel_);
                if (written_ == total_) {
                    state_ = State::Idle;
                } else if (!--count_) {
                    state_ = State::RepeatCount;
                }
                break;
            case State::RepeatCount:
                for (uint32_t i = 0; i < byte && written_ < total_; i++) put(pixel_);
                state_ = written_ < total_ ? State::SpanCount : State::Idle;
                break;
        }
    }
}

// True once a whole pixel is in pixel_.
bool DisplayLinkVideo::pixel_byte(uint8_t byte) {
    if (pixel_bytes_ == 1) {
        pixel_ = byte;
        return true;
    }
    if (!pixel_half_) {
        pixel_high_ = byte;
        pixel_half_ = true;
        return false;
    }
    pixel_half_ = false;
    pixel_ = pixel_high_ << 8 | byte;
    return true;
}

void DisplayLinkVideo::begin_command() {
    const auto command = static_cast<Command>(command_);
    if (command == Command::WriteRegister) {
        write_register(header_[0], header_[1]);
        state_ = State::Idle;
        return;
    }
    address_ = header_[0] << 16 | header_[1] << 8 | header_[2];
    total_ = header_[3] ? header_[3] : 256;
    written_ = 0;
    pixel_half_ = false;
    pixel_bytes_ = command_ >= static_cast<uint8_t>(Command::WriteRaw16) ? 2 : 1;
    switch (command) {
        case Command::WriteCopy8:
        case Command::WriteCopy16:
            copy();
            state_ = State::Idle;
            break;
        case Command::WriteRaw8:
        case Command::WriteRaw16:
            state_ = State::RawPixels;
            break;
        case Command::WriteRl8:
        case Command::WriteRl16:
            state_ = State::RunCount;
            break;
        default:
            state_ = State::SpanCount;
            break;
    }
}

// 8 bit writes go to the low bits of 24 bit color, which is not shown.
void DisplayLinkVideo::put(uint16_t pixel) {
    if (pixel_bytes_ == 2 && vram_) {
        const uint32_t offset = (address_ - base16_) & kAddressMask;
        const uint32_t index = offset / 2;
        if (index < (uint32_t)(mode_.width * mode_.height)) {
            vram_[index] = pixel;
            if (dirty_first_ > dirty_last_) dirty_since_us_ = esp_timer_get_time();
            dirty_first_ = std::min(dirty_first_, index);
            dirty_last_ = std::max(dirty_last_, index);
        }
    }
    address_ = (address_ + pixel_bytes_) & kAddressMask;
    written_++;
}

void DisplayLinkVideo::copy() {
    if (pixel_bytes_ != 2 || !vram_) return;
    const uint32_t from = ((address_ - base16_) & kAddressMask) / 2;
    const uint32_t target = header_[4] << 16 | header_[5] << 8 | header_[6];
    const uint32_t to = ((target - base16_) & kAddressMask) / 2;
    const uint32_t pixels = mode_.width * mode_.height;
    if (from + total_ > pixels || to + total_ > pixels) return;
    memmove(vram_ + to, vram_ + from, total_ * 2);
    if (dirty_first_ > dirty_last_) dirty_since_us_ = esp_timer_get_time();
    dirty_first_ = std::min(dirty_first_, to);
    dirty_last_ = std::max(dirty_last_, to + total_ - 1);
}

void DisplayLinkVideo::write_register(uint8_t reg, uint8_t value) {
    registers_[reg] = value;
    if (reg == displaylink::kVideoLock) {
        locked_ = value != displaylink::kVideoUnlocked;
        if (!locked_) apply_registers();
        return;
    }
    if (!locked_) apply_registers();
}

void DisplayLinkVideo::apply_registers() {
    const uint8_t *r = registers_;
    const bsp_size_t size = {r[displaylink::kHPixels] << 8 | r[displaylink::kHPixels + 1],
                             r[displaylink::kVPixels] << 8 | r[displaylink::kVPixels + 1]};
    base16_ = r[displaylink::kBase16] << 16 | r[displaylink::kBase16 + 1] << 8 |
              r[displaylink::kBase16 + 2];
    if (size.width && size.height &&
        (size.width != mode_.width || size.height != mode_.height)) {
        set_mode(size);
    }
    const bool blank = r[displaylink::kBlankMode] != displaylink::kBlankModeOn;
    if (blank != blank_) {
        blank_ = blank;
        display_power_(!blank);
    }
}

void DisplayLinkVideo::set_mode(bsp_size_t size) {
    heap_caps_free(vram_);
    mode_ = size;
    vram_ = static_cast<uint16_t *>(heap_caps_aligned_alloc(
        kVramAlignment, size.width * size.height * 2, MALLOC_CAP_SPIRAM));
    if (!vram_) {
        ESP_LOGE(TAG, "no memory for %dx%d", size.width, size.height);
        mode_ = {};
    } else {
        memset(vram_, 0, size.width * size.height * 2);
    }
    if (fits(rotation_)) {
        shown_rotation_ = rotation_;
    } else {
        shown_rotation_ = (bsp_rotation_t)((rotation_ + 1) % 4);
        if (mode_.width && !fits(shown_rotation_)) {
            ESP_LOGW(TAG, "mode %dx%d does not fit the panel", size.width, size.height);
        }
    }
    ESP_LOGI(TAG, "mode %dx%d, rotation %d", size.width, size.height, shown_rotation_);
    dirty_first_ = UINT32_MAX;
    dirty_last_ = 0;
    update_panel();
    if (acquired_) {
        panel_lock();
        panel_clear();
        damage_.reset();
        panel_unlock();
    }
}

void DisplayLinkVideo::update_panel() {
    const bool show = mode_.width && fits(shown_rotation_);
    if (show == acquired_) return;
    if (show) {
        panel_acquire();
        ESP_LOGI(TAG, "display started");
    } else {
        panel_release();
        damage_.reset();
        ESP_LOGI(TAG, "display stopped");
    }
    acquired_ = show;
    streaming_(show);
}

void DisplayLinkVideo::present() {
    if (!acquired_) {
        dirty_first_ = UINT32_MAX;
        dirty_last_ = 0;
        return;
    }
    const int width = mode_.width;
    const int first = dirty_first_ / width;
    const int rows = dirty_last_ / width + 1 - first;
    dirty_first_ = UINT32_MAX;
    dirty_last_ = 0;
    const bsp_rect_t band = {{0, first}, {width, rows}};
    PanelBlit blit = {};
    blit.buffer = PanelBuffer::Back;
    blit.source = band.size;
    blit.format = BSP_PIXEL_FORMAT_RGB565;
    blit.rotation = shown_rotation_;
    blit.area = panel_native_rect(band, shown_rotation_);
    panel_lock();
    esp_err_t err = damage_.drawing(blit.area);
    if (err == ESP_OK) {
        err = panel_blit(blit, reinterpret_cast<const uint8_t *>(vram_ + first * width));
    }
    damage_.drawn(blit.area);
    if (err == ESP_OK) err = damage_.present();
    panel_unlock();
    if (err != ESP_OK) ESP_LOGW(TAG, "rows %d+%d: %s", first, rows, esp_err_to_name(err));
    stats_presents_++;
}
