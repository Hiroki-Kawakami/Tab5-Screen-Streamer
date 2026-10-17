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
#ifdef ESP_PLATFORM
#include "esp_cache.h"
#include "esp_cpu.h"
#endif

// DIAG: cycle accounting
static uint32_t diag_cycles() {
#ifdef ESP_PLATFORM
    return esp_cpu_get_cycle_count();
#else
    return 0;
#endif
}
enum { kDiagParse, kDiagBits16, kDiagBits8, kDiagDraw, kDiagWriteBack, kDiagLoad, kDiagPresent,
       kDiagCopy, kDiagDamage, kDiagBlit, kDiagWait, kDiagTarget, kDiagCount };
static uint64_t diag_time[kDiagCount];
static uint64_t diag_bytes16, diag_bytes8, diag_pixels16, diag_pixels8;
static uint64_t diag_command_time[256];
static uint32_t diag_command_count[256];
static uint32_t diag_command_start;
static uint32_t diag_loads;
static uint64_t diag_command_pixels[256];

static const char *TAG = "displaylink_video";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 16;
static constexpr int kStepBits = 4;
static constexpr int kStepValues = 1 << kStepBits;
static constexpr int64_t kMaxPresentDelayUs = 33 * 1000;
static constexpr uint32_t kAddressMask = 0xffffff;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;
static constexpr int kTileWidth = 256;
static constexpr int kMaxWriteBacks = 8;

#ifdef ESP_PLATFORM
static bool IRAM_ATTR write_back_done(ppa_client_handle_t, ppa_event_data_t *, void *done) {
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(static_cast<SemaphoreHandle_t>(done), &woken);
    return woken == pdTRUE;
}
#endif

using displaylink::Command;

DisplayLinkVideo::~DisplayLinkVideo() {
    if (queue_) {
        send(Op::Quit);
        xSemaphoreTake(exited_, portMAX_DELAY);
        vQueueDelete(queue_);
    }
    if (exited_) vSemaphoreDelete(exited_);
    heap_caps_free(table_);
    heap_caps_free(steps_);
    heap_caps_free(steps8_);
#ifdef ESP_PLATFORM
    if (copier_) ppa_unregister_client(copier_);
#endif
    if (write_backs_done_) vSemaphoreDelete(write_backs_done_);
}

esp_err_t DisplayLinkVideo::start(bsp_rotation_t rotation) {
    rotation_ = rotation;
    write_backs_done_ = xSemaphoreCreateCounting(kMaxWriteBacks, 0);
    if (!write_backs_done_) return ESP_ERR_NO_MEM;
#ifdef ESP_PLATFORM
    ppa_client_config_t client = {};
    client.oper_type = PPA_OPERATION_SRM;
    client.max_pending_trans_num = kMaxWriteBacks;
    esp_err_t err = ppa_register_client(&client, &copier_);
    if (err == ESP_OK) {
        ppa_event_callbacks_t callbacks = {};
        callbacks.on_trans_done = write_back_done;
        err = ppa_client_register_event_callbacks(copier_, &callbacks);
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "ppa copier: %s", esp_err_to_name(err));
        if (copier_) ppa_unregister_client(copier_);
        copier_ = nullptr;
    }
#endif
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

bool DisplayLinkVideo::pending() const {
    if (shown_ < 0) return false;
    if (regions_[shown_].first <= regions_[shown_].last) return true;
    for (int i = 0; i < window_count_; i++) {
        const Window &window = windows_[i];
        if (window.valid && window.region == shown_ && window.dirty_first <= window.dirty_last) {
            return true;
        }
    }
    return false;
}

// The stream has no frames: what changed shows once the host pauses, or
// after a while of it sending on.
void DisplayLinkVideo::run(const Item &item) {
    switch (item.op) {
        case Op::Data: {
            const uint32_t diag_start = diag_cycles();
            parse(item.data, item.length);
            diag_time[kDiagParse] += diag_cycles() - diag_start;
            item.done(item.context);
            stats_bytes_ += item.length;
            if (pending() && (!uxQueueMessagesWaiting(queue_) ||
                              esp_timer_get_time() - dirty_since_us_ >= kMaxPresentDelayUs)) {
                commit();
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
            base16_ = 0;
            stride16_ = 0;
            shown_ = -1;
            for (Region &region : regions_) region = {};
            for (Window &window : windows_) window.valid = false;
            wait_write_backs();
            vram_.clear();
            update_panel();
            break;
    }

    const int64_t now = esp_timer_get_time();
    if (!stats_start_us_) stats_start_us_ = now;
    if (now - stats_start_us_ < kStatsPeriodUs || !stats_bytes_) return;
    const double seconds = (now - stats_start_us_) / 1e6;
    ESP_LOGI(TAG, "%.1f presents/s (%.0f rows, %.1f ms each), %.2f MB/s, busy %.0f%%, %u pages",
             stats_presents_ / seconds,
             stats_presents_ ? (double)stats_rows_ / stats_presents_ : 0.0,
             stats_presents_ ? stats_present_us_ / 1e3 / stats_presents_ : 0.0,
             stats_bytes_ / seconds / 1e6, stats_busy_us_ / 1e4 / seconds,
             (unsigned)vram_.pages());
    {  // DIAG
#ifndef CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ
#define CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ 1
#endif
        const double ms = 1.0 / (seconds * CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ * 1e3);
        ESP_LOGI(TAG, "DIAG ms/s parse %.0f (bits16 %.0f bits8 %.0f copy %.0f) draw %.0f "
                 "writeback %.0f load %.0f present %.0f | 16: %.0f KB %.0f Kpx, 8: %.0f KB %.0f Kpx",
                 diag_time[kDiagParse] * ms, diag_time[kDiagBits16] * ms,
                 diag_time[kDiagBits8] * ms, diag_time[kDiagCopy] * ms,
                 diag_time[kDiagDraw] * ms, diag_time[kDiagWriteBack] * ms,
                 diag_time[kDiagLoad] * ms, diag_time[kDiagPresent] * ms,
                 diag_bytes16 / seconds / 1e3, diag_pixels16 / seconds / 1e3,
                 diag_bytes8 / seconds / 1e3, diag_pixels8 / seconds / 1e3);
        char line[220];
        int n = 0;
        for (int op = 0; op < 256 && n < 200; op++) {
            if (!diag_command_count[op]) continue;
            n += snprintf(line + n, sizeof(line) - n, " %02x:%.0fms/%u/%.0fKpx", op,
                          diag_command_time[op] * ms, (unsigned)diag_command_count[op],
                          diag_command_pixels[op] / 1e3);
        }
        ESP_LOGI(TAG, "DIAG commands%s", line);
        ESP_LOGI(TAG, "DIAG loads %u, damage %.0f blit %.0f", (unsigned)diag_loads,
                 diag_time[kDiagDamage] * ms, diag_time[kDiagBlit] * ms);
        diag_loads = 0;
        ESP_LOGI(TAG, "DIAG wait %.0f target %.0f", diag_time[kDiagWait] * ms,
                 diag_time[kDiagTarget] * ms);
        memset(diag_command_time, 0, sizeof(diag_command_time));
        memset(diag_command_count, 0, sizeof(diag_command_count));
        memset(diag_command_pixels, 0, sizeof(diag_command_pixels));
        memset(diag_time, 0, sizeof(diag_time));
        diag_bytes16 = diag_bytes8 = diag_pixels16 = diag_pixels8 = 0;
    }
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_presents_ = 0;
    stats_rows_ = 0;
    stats_busy_us_ = 0;
    stats_present_us_ = 0;
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
        if (state_ == State::Idle && diag_command_start) {  // DIAG
            diag_command_time[command_] += diag_cycles() - diag_command_start;
            diag_command_count[command_]++;
            diag_command_pixels[command_] += written_;
            diag_command_start = 0;
        }
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
                    case Command::WriteCompressed8:
                    case Command::WriteCompressed16:
                        header_length_ = 4;
                        break;
                    case Command::WriteCopy8:
                    case Command::WriteCopy16:
                        header_length_ = 7;
                        break;
                    case Command::LoadTable:
                        header_length_ = 8;
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
                if (header_bytes_ == header_length_) {
                    diag_command_start = diag_cycles();  // DIAG
                    begin_command();
                }
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
                repeat(pixel_, std::min(count_, total_ - written_));
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
                repeat(pixel_, std::min<uint32_t>(byte, total_ - written_));
                state_ = written_ < total_ ? State::SpanCount : State::Idle;
                break;
            case State::TableEntries:
                header_[header_bytes_++] = byte;
                if (header_bytes_ == displaylink::kTableEntryBytes) table_entry();
                break;
            case State::Bits: {
                const uint32_t diag_start = diag_cycles();
                const uint8_t *diag_from = p;
                const bool wide = pixel_bytes_ == 2;
                p = wide ? bits(p, end) : skip_bits(p, end);
                diag_time[wide ? kDiagBits16 : kDiagBits8] += diag_cycles() - diag_start;
                (wide ? diag_bytes16 : diag_bytes8) += p + 1 - diag_from;
                break;
            }
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
    if (command == Command::LoadTable) {
        load_table();
        return;
    }
    address_ = header_[0] << 16 | header_[1] << 8 | header_[2];
    total_ = header_[3] ? header_[3] : 256;
    written_ = 0;
    pixel_half_ = false;
    pixel_bytes_ = command_ >= static_cast<uint8_t>(Command::WriteRaw16) &&
                           command != Command::WriteCompressed8
                       ? 2
                       : 1;
    switch (command) {
        case Command::WriteCopy8:
            state_ = State::Idle;
            return;
        case Command::WriteCopy16: {
            // The target comes first, then the count and the source.
            const uint32_t source = header_[4] << 16 | header_[5] << 8 | header_[6];
            const uint32_t bytes = total_ * pixel_bytes_;
            const uint32_t diag_start = diag_cycles();
            invalidate(source, bytes);
            invalidate(address_, bytes);
            wait_write_backs();
            vram_.copy(address_, source, bytes);
#ifdef ESP_PLATFORM
            if (uint8_t *target = vram_.contiguous(address_, bytes)) {
                esp_cache_msync(target, bytes,
                                ESP_CACHE_MSYNC_FLAG_DIR_C2M | ESP_CACHE_MSYNC_FLAG_UNALIGNED);
            }
#endif
            diag_time[kDiagCopy] += diag_cycles() - diag_start;
            mark(address_, bytes);
            state_ = State::Idle;
            return;
        }
        case Command::WriteRaw8:
        case Command::WriteRaw16:
            target();
            state_ = State::RawPixels;
            break;
        case Command::WriteRl8:
        case Command::WriteRl16:
            target();
            state_ = State::RunCount;
            break;
        case Command::WriteCompressed8:
        case Command::WriteCompressed16:
            if (!table_loaded_) {
                ESP_LOGW(TAG, "compressed pixels without a table");
                state_ = State::Idle;
                return;
            }
            if (command == Command::WriteCompressed16) {
                active_steps_ = steps_;
                table_state_ = displaylink::kTableStart16;
            } else {
                active_steps_ = steps8_;
                table_state_ = displaylink::kTableStart8;
            }
            color_ = 0;
            target();
            state_ = State::Bits;
            break;
        default:
            target();
            state_ = State::SpanCount;
            break;
    }
}

void DisplayLinkVideo::load_table() {
    uint32_t magic;
    memcpy(&magic, header_, sizeof(magic));
    const uint16_t states = header_[6] << 8 | header_[7];
    if (magic != displaylink::kTableMagic || states <= displaylink::kTableStart16 ||
        states > displaylink::kMaxTableStates) {
        ESP_LOGW(TAG, "table of %u states, magic 0x%08x", states, (unsigned)magic);
        state_ = State::Idle;
        return;
    }
    if (states != table_states_) {
        heap_caps_free(table_);
        heap_caps_free(steps_);
        heap_caps_free(steps8_);
        const size_t bytes = states * kStepValues * sizeof(Step);
        table_ = static_cast<Branch *>(
            heap_caps_malloc(states * 2 * sizeof(Branch), MALLOC_CAP_SPIRAM));
        steps_ = static_cast<Step *>(heap_caps_malloc(bytes, MALLOC_CAP_SPIRAM));
        steps8_ = static_cast<Step *>(heap_caps_malloc(bytes, MALLOC_CAP_SPIRAM));
        if (!table_ || !steps_ || !steps8_) {
            ESP_LOGE(TAG, "no memory for a table of %u states", states);
            heap_caps_free(table_);
            heap_caps_free(steps_);
            heap_caps_free(steps8_);
            table_ = nullptr;
            steps_ = nullptr;
            steps8_ = nullptr;
        }
        table_states_ = table_ ? states : 0;
    }
    table_loaded_ = 0;
    total_ = states;
    written_ = 0;
    header_bytes_ = 0;
    state_ = State::TableEntries;
}

// Each entry holds both branches of a state: the color of A, its repeat and
// the high jump bits, the low jump bits of A and B, then the same for B.
void DisplayLinkVideo::table_entry() {
    header_bytes_ = 0;
    if (table_) {
        const uint8_t *e = header_;
        Branch *branch = table_ + written_ * 2;
        branch[0].color = e[0] << 8 | e[1];
        branch[0].jump = (e[3] & 0x1f) << 4 | e[4] >> 4;
        branch[1].color = e[5] << 8 | e[6];
        branch[1].jump = (e[8] & 0x1f) << 4 | (e[4] & 0xf);
        for (int i = 0; i < 2; i++) {
            if (branch[i].jump >= total_) branch[i].jump = 0;
        }
    }
    if (++written_ < total_) return;
    if (table_) {
        build_steps(steps_, displaylink::kTableStart16);
        build_steps(steps8_, displaylink::kTableStart8);
        table_loaded_ = table_states_;
    }
    state_ = State::Idle;
}

// A pixel ends where a branch leads to state 0, and the next starts over at
// `start`; the color carries on from pixel to pixel.
void DisplayLinkVideo::build_steps(Step *steps, uint16_t start) {
    for (uint32_t state = 0; state < table_states_; state++) {
        for (int value = 0; value < kStepValues; value++) {
            Step &step = steps[state * kStepValues + value];
            step = {};
            uint16_t at = state;
            uint16_t color = 0;
            int pixels = 0;
            for (int bit = 0; bit < kStepBits; bit++) {
                const Branch &branch = table_[at * 2 + (value >> bit & 1)];
                color += branch.color;
                at = branch.jump;
                if (at) continue;
                step.sums[pixels++] = color;
                at = start;
            }
            step.total = color;
            step.pixels = pixels;
            step.next = at * (kStepValues * sizeof(Step));
        }
    }
}

// Bits are read from the least significant; the rest of the byte holding the
// last pixel is padding. Returns the last byte read; the command goes on into
// the next transfer unless it is done.
const uint8_t *DisplayLinkVideo::bits(const uint8_t *p, const uint8_t *end) {
    using Pixel = uint16_t;
    const auto *steps = reinterpret_cast<const uint8_t *>(active_steps_);
    uint32_t next = table_state_ * (kStepValues * sizeof(Step));
    uint16_t color = color_;
    uint32_t left = total_ - written_;
    auto *out = reinterpret_cast<Pixel *>(out_);
    uint32_t room = out_ ? (out_end_ - out_) / sizeof(Pixel) : 0;
    const auto step_at = [steps](uint32_t next, uint32_t value) -> const Step & {
        return *reinterpret_cast<const Step *>(steps + next +
                                               (value & (kStepValues - 1)) * sizeof(Step));
    };
    for (;; p++) {
        const uint32_t safe = std::min(room, left);
        if (safe >= 8) {
            // Each step writes at least two pixels whatever it ends: the
            // slots past its own are the command's, which the pixels after it
            // overwrite.
            Pixel *const from = out;
            const Pixel *const limit = out + safe - 8;
            for (;;) {
                const uint32_t byte = *p;
                const Step &low = step_at(next, byte);
                out[0] = color + low.sums[0];
                out[1] = color + low.sums[1];
                if (low.pixels > 2) {
                    out[2] = color + low.sums[2];
                    out[3] = color + low.sums[3];
                }
                out += low.pixels;
                color += low.total;
                const Step &high = step_at(low.next, byte >> kStepBits);
                out[0] = color + high.sums[0];
                out[1] = color + high.sums[1];
                if (high.pixels > 2) {
                    out[2] = color + high.sums[2];
                    out[3] = color + high.sums[3];
                }
                out += high.pixels;
                color += high.total;
                next = high.next;
                if (out > limit || p + 1 == end) break;
                p++;
            }
            const uint32_t pixels = out - from;
            room -= pixels;
            left -= pixels;
            if (!left) break;
            if (p + 1 == end) break;
            continue;
        }
        uint32_t byte = *p;
        for (int half = 0; half < 2; half++, byte >>= kStepBits) {
            const Step &step = step_at(next, byte);
            const uint32_t pixels = std::min<uint32_t>(step.pixels, left);
            if (pixels <= room) {
                for (uint32_t i = 0; i < pixels; i++) out[i] = color + step.sums[i];
                out += pixels;
                room -= pixels;
            } else {
                out_ = reinterpret_cast<uint8_t *>(out);
                written_ = total_ - left;
                for (uint32_t i = 0; i < pixels; i++) put(color + step.sums[i]);
                out = reinterpret_cast<Pixel *>(out_);
                room = out_ ? (out_end_ - out_) / sizeof(Pixel) : 0;
            }
            left -= pixels;
            if (!left) break;
            color += step.total;
            next = step.next;
        }
        if (!left || p + 1 == end) break;
    }
    out_ = reinterpret_cast<uint8_t *>(out);
    written_ = total_ - left;
    if (!left) {
        diag_pixels16 += total_;
        state_ = State::Idle;
        return p;
    }
    table_state_ = next / (kStepValues * sizeof(Step));
    color_ = color;
    return p;
}

const uint8_t *DisplayLinkVideo::skip_bits(const uint8_t *p, const uint8_t *end) {
    const auto *steps = reinterpret_cast<const uint8_t *>(active_steps_);
    uint32_t next = table_state_ * (kStepValues * sizeof(Step));
    uint32_t left = total_ - written_;
    for (;; p++) {
        uint32_t byte = *p;
        for (int half = 0; half < 2; half++, byte >>= kStepBits) {
            const auto &step = *reinterpret_cast<const Step *>(
                steps + next + (byte & (kStepValues - 1)) * sizeof(Step));
            if (step.pixels >= left) {
                written_ = total_;
                diag_pixels8 += total_;
                state_ = State::Idle;
                return p;
            }
            left -= step.pixels;
            next = step.next;
        }
        if (p + 1 == end) break;
    }
    written_ = total_ - left;
    table_state_ = next / (kStepValues * sizeof(Step));
    return p;
}

// The 8-bit plane holds the low bits of 24-bit color, which is not shown:
// its pixels are parsed and dropped.
void DisplayLinkVideo::put(uint16_t pixel) {
    if (pixel_bytes_ == 2) {
        if (out_ && out_ == out_end_) target();
        if (out_) {
            memcpy(out_, &pixel, sizeof(pixel));
            out_ += sizeof(pixel);
        } else {
            vram_.write16(here(), pixel);
        }
    }
    written_++;
}

void DisplayLinkVideo::repeat(uint16_t pixel, uint32_t count) {
    if (pixel_bytes_ == 1) {
        written_ += count;
        return;
    }
    bool retargeted = false;
    while (count) {
        if (!out_ || out_ == out_end_) {
            put(pixel);
            count--;
            // The first pixel may have brought the page into memory.
            if (!out_ && count && !retargeted) {
                target();
                retargeted = true;
            }
            continue;
        }
        auto *out = reinterpret_cast<uint16_t *>(out_);
        const uint32_t pixels = std::min<uint32_t>(count, (out_end_ - out_) / sizeof(pixel));
        for (uint32_t i = 0; i < pixels; i++) out[i] = pixel;
        out_ += pixels * sizeof(pixel);
        written_ += pixels;
        count -= pixels;
    }
}

uint32_t DisplayLinkVideo::here() const {
    return (address_ + written_ * pixel_bytes_) & kAddressMask;
}

// Where the rest of the command's pixels can go in place, up to the edge of
// a window of a region shown before, or of a page of graphics memory; past
// it put() looks again. Elsewhere they go pixel by pixel.
void DisplayLinkVideo::target() {
    const uint32_t diag_start = diag_cycles();
    target_();
    diag_time[kDiagTarget] += diag_cycles() - diag_start;
}

void DisplayLinkVideo::target_() {
    const uint32_t bytes = (total_ - written_) * pixel_bytes_;
    const uint32_t address = here();
    out_ = nullptr;
    out_end_ = nullptr;
    if (pixel_bytes_ == 1) return;
    uint32_t offset;
    const int region = region_of(address, &offset);
    if (region >= 0) {
        if (!window_count_) return;
        const int row = offset / stride16_;
        const int x = offset % stride16_ / 2;
        Window &window = windows_[window_at(region, row - row % kTileRows, x - x % kTileWidth)];
        const int at = row - window.row;
        const int left = x - window.x;
        if (window.dirty_first > window.dirty_last && window.region == shown_) {
            dirty_since_us_ = esp_timer_get_time();
        }
        window.dirty_first = std::min(window.dirty_first, at);
        window.dirty_last = std::max(window.dirty_last, at);
        const uint32_t span = std::min<uint32_t>(bytes, (window.width - left) * 2);
        cover(window, at, left, left + span / 2);
        out_ = window.pixels + (at * window.width + left) * 2;
        out_end_ = out_ + span;
        return;
    }
    const uint32_t page_left = 0x10000 - (address & 0xffff);
    const uint32_t span = std::min(bytes, page_left);
    out_ = vram_.span(address, span);
    out_end_ = out_ ? out_ + span : nullptr;
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
    const uint32_t base = r[displaylink::kBase16] << 16 | r[displaylink::kBase16 + 1] << 8 |
                          r[displaylink::kBase16 + 2];
    uint32_t stride = r[displaylink::kStride16] << 16 | r[displaylink::kStride16 + 1] << 8 |
                      r[displaylink::kStride16 + 2];
    if (!stride) stride = size.width * 2;
    if (size.width && size.height &&
        (size.width != mode_.width || size.height != mode_.height)) {
        set_mode(size);
    }
    if (stride != stride16_) {
        drop_windows();
        stride16_ = stride;
        setup_windows();
        if (stride != (uint32_t)size.width * 2) {
            ESP_LOGW(TAG, "stride %u for %d pixels is not supported", (unsigned)stride,
                     size.width);
        }
        shown_ = -1;
        for (Region &region : regions_) region = {};
    }
    if (shown_ < 0 || base != base16_) show(base);
    const bool blank = r[displaylink::kBlankMode] != displaylink::kBlankModeOn;
    if (blank != blank_) {
        blank_ = blank;
        display_power_(!blank);
    }
}

// A framebuffer the host flips to holds every change since it was last
// shown, all of which went through the windows into the back buffer; one
// never shown before is drawn whole from memory.
void DisplayLinkVideo::show(uint32_t base) {
    const bool flip = shown_ >= 0;
    base16_ = base;
    int index = -1;
    for (int i = 0; i < kRegions; i++) {
        if (regions_[i].used && regions_[i].base == base) index = i;
    }
    if (index < 0) {
        index = 0;
        for (int i = 1; i < kRegions; i++) {
            if (regions_[i].used < regions_[index].used) index = i;
        }
        for (Window &window : windows_) {
            if (window.region == index) window.valid = false;
        }
        regions_[index] = {base, 0, mode_.height - 1, 0};
        wait_write_backs();
        vram_.pin(base, stride16_ * mode_.height);
    }
    regions_[index].used = ++region_clock_;
    shown_ = index;
    if (flip) {
        commit();
    } else if (pending()) {
        dirty_since_us_ = esp_timer_get_time();
    }
}

int DisplayLinkVideo::region_of(uint32_t address, uint32_t *offset) const {
    if (!stride16_) return -1;
    const uint32_t size = stride16_ * mode_.height;
    for (int i = 0; i < kRegions; i++) {
        if (!regions_[i].used) continue;
        const uint32_t at = (address - regions_[i].base) & kAddressMask;
        if (at < size) {
            *offset = at;
            return i;
        }
    }
    return -1;
}

// Rows a copy changed, to draw from memory.
void DisplayLinkVideo::mark(uint32_t address, uint32_t bytes) {
    for (uint32_t at = 0; at < bytes;) {
        uint32_t offset;
        const int index = region_of(address + at, &offset);
        if (index < 0) {
            at += 2;
            continue;
        }
        Region &region = regions_[index];
        const uint32_t size = stride16_ * mode_.height;
        const uint32_t span = std::min(bytes - at, size - offset);
        if (index == shown_ && !pending()) dirty_since_us_ = esp_timer_get_time();
        region.first = std::min<int>(region.first, offset / stride16_);
        region.last = std::max<int>(region.last, (offset + span - 1) / stride16_);
        at += span;
    }
}

// The window holding a tile, loaded if need be.
int DisplayLinkVideo::window_at(int region, int row, int x) {
    int index = -1;
    int victim = 0;
    for (int i = 0; i < window_count_; i++) {
        const Window &window = windows_[i];
        if (window.valid && window.region == region && window.row == row && window.x == x) {
            index = i;
            break;
        }
        if (!windows_[victim].valid) continue;
        if (!window.valid || window.used < windows_[victim].used) victim = i;
    }
    if (index < 0) {
        index = victim;
        Window &window = windows_[index];
        flush(window);
        Spare *spare = &spares_[0];
        for (Spare &s : spares_) {
            if (s.read_until < spare->read_until) spare = &s;
        }
        std::swap(window.pixels, spare->pixels);
        std::swap(window.read_until, spare->read_until);
        std::swap(window.blitted, spare->blitted);
        const uint32_t diag_start = diag_cycles();
        release(window.read_until, window.blitted);
        diag_time[kDiagWait] += diag_cycles() - diag_start;
        window.read_until = 0;
        window.blitted = false;
        load(windows_[index], region, row, x);
    }
    windows_[index].used = ++window_clock_;
    return index;
}

void DisplayLinkVideo::setup_windows() {
    drop_windows();
    const int bytes = kTileWidth * kTileRows * 2;
    const int per_strip = panel_strip_bytes() / bytes;
    window_count_ = std::min(kMaxWindows, 2 * per_strip - kSpareWindows);
    for (int i = 0; i < window_count_ + kSpareWindows; i++) {
        uint8_t *pixels = panel_strip(i / per_strip) + i % per_strip * bytes;
        if (i < window_count_) {
            windows_[i].pixels = pixels;
            windows_[i].read_until = 0;
            windows_[i].blitted = false;
        } else {
            spares_[i - window_count_] = {pixels, 0, false};
        }
    }
}

void DisplayLinkVideo::load(Window &window, int region, int row, int x) {
    window.region = region;
    window.row = row;
    window.x = x;
    window.width = std::min(kTileWidth, mode_.width - x);
    window.rows = std::min(kTileRows, mode_.height - row);
    window.dirty_first = INT32_MAX;
    window.dirty_last = -1;
    window.loaded = 0;
    for (int r = 0; r < window.rows; r++) {
        window.left[r] = window.width;
        window.right[r] = 0;
    }
    window.valid = true;
}

void DisplayLinkVideo::cover(Window &window, int row, int left, int right) {
    if (window.loaded & 1u << row) return;
    if (window.left[row] >= window.right[row] ||
        (left <= window.right[row] && right >= window.left[row])) {
        window.left[row] = std::min<int>(window.left[row], left);
        window.right[row] = std::max<int>(window.right[row], right);
        return;
    }
    complete(window, row, row);
}

// Reads what the rows do not hold yet around what was written to them.
void DisplayLinkVideo::complete(Window &window, int first, int last) {
    bool waited = false;
    const uint32_t diag_start = diag_cycles();
    const uint32_t start = regions_[window.region].base + window.row * stride16_ + window.x * 2;
    for (int row = first; row <= last; row++) {
        if (window.loaded & 1u << row) continue;
        if (!waited) {
            wait_rows(window.region, window.row + first, window.row + last);
            waited = true;
        }
        window.loaded |= 1u << row;
        uint8_t *pixels = window.pixels + row * window.width * 2;
        const uint32_t address = start + row * stride16_;
        const int left = window.left[row], right = window.right[row];
        if (left >= right) {
            vram_.read(address, pixels, window.width * 2);
            diag_loads++;
            continue;
        }
        if (left > 0) vram_.read(address, pixels, left * 2);
        if (right < window.width) {
            vram_.read(address + right * 2, pixels + right * 2, (window.width - right) * 2);
        }
        if (left > 0 || right < window.width) diag_loads++;
    }
    diag_time[kDiagLoad] += diag_cycles() - diag_start;
}

// The whole tile goes to the back buffer, so that it covers whole cells of
// the panel's damage tracking, and the rows written back to memory; the
// window stays, holding what memory holds.
void DisplayLinkVideo::flush(Window &window) {
    if (!window.valid || window.dirty_first > window.dirty_last) return;
    const int first = window.dirty_first;
    const int rows = window.dirty_last + 1 - first;
    complete(window, 0, window.rows - 1);
    uint32_t diag_start = diag_cycles();
    draw(window.row, window.rows, window.x, window.width, window.pixels);
    diag_time[kDiagDraw] += diag_cycles() - diag_start;
    diag_start = diag_cycles();
    write_back(window, first, rows);
    diag_time[kDiagWriteBack] += diag_cycles() - diag_start;
    window.dirty_first = INT32_MAX;
    window.dirty_last = -1;
}

// Rows of a window back to memory, by PPA where they lie in one piece. PPA
// invalidates the cache over whole rows, so nothing written by the CPU may
// be pending there, and the CPU reads none of them until it is done.
void DisplayLinkVideo::write_back(Window &window, int first, int rows) {
    const int pitch = window.width * 2;
    const uint8_t *pixels = window.pixels + first * pitch;
    const uint32_t row_start = regions_[window.region].base + (window.row + first) * stride16_;
#ifdef ESP_PLATFORM
    uint8_t *out = copier_ ? vram_.contiguous(row_start, rows * stride16_) : nullptr;
    if (out) {
        if (write_backs_sent_ - write_backs_done_count_ == kMaxWriteBacks) {
            wait_write_backs(write_backs_done_count_ + 1);
        }
        ppa_srm_oper_config_t config = {};
        config.in.buffer = pixels;
        config.in.pic_w = window.width;
        config.in.pic_h = rows;
        config.in.block_w = window.width;
        config.in.block_h = rows;
        config.in.srm_cm = PPA_SRM_COLOR_MODE_RGB565;
        config.out.buffer = out;
        config.out.buffer_size = rows * stride16_;
        config.out.pic_w = stride16_ / 2;
        config.out.pic_h = rows;
        config.out.block_offset_x = window.x;
        config.out.srm_cm = PPA_SRM_COLOR_MODE_RGB565;
        config.rotation_angle = PPA_SRM_ROTATION_ANGLE_0;
        config.scale_x = 1;
        config.scale_y = 1;
        config.mode = PPA_TRANS_MODE_NON_BLOCKING;
        config.user_data = write_backs_done_;
        if (ppa_do_scale_rotate_mirror(copier_, &config) == ESP_OK) {
            const uint32_t sequence = ++write_backs_sent_;
            write_backs_[sequence % kMaxWriteBacks] = {sequence, window.region,
                                                       window.row + first,
                                                       window.row + first + rows - 1};
            window.read_until = sequence;
            window.blitted = false;
            return;
        }
    }
#endif
    wait_write_backs();
    for (int r = 0; r < rows; r++) {
        vram_.write(row_start + r * stride16_ + window.x * 2, pixels + r * pitch, pitch);
    }
    window.blitted = true;
}

void DisplayLinkVideo::wait_write_backs() {
    wait_write_backs(write_backs_sent_);
}

void DisplayLinkVideo::wait_write_backs(uint32_t sequence) {
    while (write_backs_done_count_ < sequence) {
        xSemaphoreTake(write_backs_done_, portMAX_DELAY);
        write_backs_done_count_++;
    }
}

// Until no write back in flight covers rows the CPU is about to read.
void DisplayLinkVideo::wait_rows(int region, int first, int last) {
    uint32_t sequence = 0;
    for (uint32_t s = write_backs_done_count_ + 1; s <= write_backs_sent_; s++) {
        const WriteBack &w = write_backs_[s % kMaxWriteBacks];
        if (w.region == region && w.first <= last && first <= w.last) sequence = s;
    }
    wait_write_backs(sequence);
}

// Until no write back in flight reads the window's pixels.
// The copy engine runs its queue in order, so the write-back being done
// means the draws queued before it are too.
void DisplayLinkVideo::release(uint32_t read_until, bool blitted) {
    if (blitted) {
        panel_lock();
        panel_blit_wait();
        panel_unlock();
        return;
    }
    wait_write_backs(read_until);
}

void DisplayLinkVideo::invalidate(uint32_t address, uint32_t bytes) {
    uint32_t offset;
    const int region = region_of(address, &offset);
    if (region < 0) return;
    const int row0 = offset / stride16_;
    const int row1 = (offset + bytes - 1) / stride16_;
    const int x0 = row0 == row1 ? offset % stride16_ / 2 : 0;
    const int x1 = row0 == row1 ? (offset + bytes - 1) % stride16_ / 2 : mode_.width - 1;
    for (int i = 0; i < window_count_; i++) {
        Window &window = windows_[i];
        if (!window.valid || window.region != region || row1 < window.row ||
            row0 >= window.row + window.rows || x1 < window.x || x0 >= window.x + window.width) {
            continue;
        }
        flush(window);
        window.valid = false;
    }
}

void DisplayLinkVideo::drop_windows() {
    for (int i = 0; i < window_count_; i++) {
        flush(windows_[i]);
        windows_[i].valid = false;
    }
}

void DisplayLinkVideo::draw(int row, int rows, int x, int width, const uint8_t *pixels) {
    if (!acquired_ || stride16_ != (uint32_t)mode_.width * 2 || !fits(shown_rotation_)) return;
    const int64_t start = esp_timer_get_time();
    const bsp_rect_t area = {{x, row}, {width, rows}};
    PanelBlit blit = {};
    blit.buffer = PanelBuffer::Back;
    blit.source = area.size;
    blit.format = BSP_PIXEL_FORMAT_RGB565;
    blit.rotation = shown_rotation_;
    blit.area = panel_native_rect(area, shown_rotation_);
    panel_lock();
    uint32_t diag_start = diag_cycles();
    esp_err_t err = damage_.drawing(blit.area);
    diag_time[kDiagDamage] += diag_cycles() - diag_start;
    diag_start = diag_cycles();
    if (err == ESP_OK) err = panel_blit_async(blit, pixels);
    diag_time[kDiagBlit] += diag_cycles() - diag_start;
    diag_start = diag_cycles();
    damage_.drawn(blit.area);
    diag_time[kDiagDamage] += diag_cycles() - diag_start;
    panel_unlock();
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "%dx%d at %d,%d: %s", width, rows, x, row, esp_err_to_name(err));
    }
    stats_rows_ += rows;
    stats_present_us_ += esp_timer_get_time() - start;
}

void DisplayLinkVideo::draw_from_memory(int index) {
    Region &region = regions_[index];
    const int first = region.first;
    const int last = region.last;
    region.first = INT32_MAX;
    region.last = -1;
    if (!window_count_) return;
    for (int top = first - first % kTileRows; top <= last; top += kTileRows) {
        for (int x = 0; x < mode_.width; x += kTileWidth) {
            Window &window = windows_[window_at(index, top, x)];
            if (window.dirty_first <= window.dirty_last) {
                flush(window);
                continue;
            }
            complete(window, 0, window.rows - 1);
            const uint32_t diag_start = diag_cycles();
            draw(window.row, window.rows, window.x, window.width, window.pixels);
            diag_time[kDiagDraw] += diag_cycles() - diag_start;
            window.blitted = true;
        }
    }
}

void DisplayLinkVideo::set_mode(bsp_size_t size) {
    mode_ = size;
    if (fits(rotation_)) {
        shown_rotation_ = rotation_;
    } else {
        shown_rotation_ = (bsp_rotation_t)((rotation_ + 1) % 4);
        if (!fits(shown_rotation_)) {
            ESP_LOGW(TAG, "mode %dx%d does not fit the panel", size.width, size.height);
        }
    }
    ESP_LOGI(TAG, "mode %dx%d, rotation %d", size.width, size.height, shown_rotation_);
    for (Window &window : windows_) window.valid = false;
    shown_ = -1;
    for (Region &region : regions_) region = {};
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

// What the shown framebuffer got goes on screen: the rows written to it
// through the windows, then those to draw from memory.
void DisplayLinkVideo::commit() {
    if (shown_ < 0) return;
    for (int i = 0; i < window_count_; i++) {
        if (windows_[i].region == shown_) flush(windows_[i]);
    }
    draw_from_memory(shown_);
    if (!acquired_) return;
    const int64_t start = esp_timer_get_time();
    const uint32_t diag_start = diag_cycles();
    panel_lock();
    const esp_err_t err = damage_.present();
    panel_unlock();
    diag_time[kDiagPresent] += diag_cycles() - diag_start;
    if (err != ESP_OK) ESP_LOGW(TAG, "present: %s", esp_err_to_name(err));
    stats_presents_++;
    stats_present_us_ += esp_timer_get_time() - start;
}
