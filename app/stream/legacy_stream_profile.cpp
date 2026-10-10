/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "legacy_stream_profile.hpp"
#include <algorithm>
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "presenter.hpp"

static const char *TAG = "legacy_stream";

static constexpr uint32_t kSlotBytes = 16 * 65024;
static constexpr size_t kSlotAlignment = 128;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

static constexpr uint32_t kHeaderBytes = 8;
static constexpr uint32_t kCoordBytes = 4;
static constexpr uint8_t kTypeBand = 0x50;
static constexpr uint8_t kTypeLastBand = 0x51;
static constexpr int kUnit = 16;
static constexpr int kFrameWidth = 720;
static constexpr int kFrameHeight = 1280;

enum class SlotState : uint8_t { Idle, Usb, Presenter };

struct LegacyStreamProfile::Slot {
    usb_device::Transfer transfer;
    StreamFrame frame;
    LegacyStreamProfile *owner;
    SlotState state;
};

/* Finds the last complete frame in a transfer. The stream marks no frame
 * boundaries: a transfer normally ends with the short packet closing one
 * frame, but a frame whose length is a multiple of the packet size runs into
 * the next, so parsing hunts byte by byte for a first band when it is lost. */
static bool parse(const uint8_t *data, uint32_t length, StreamFrame *out, bool *resynced) {
    StreamFrame current = {};
    bool found = false;
    int next_y = 0;
    uint32_t pos = 0;
    *resynced = false;
    while (pos + kHeaderBytes <= length) {
        const uint8_t *header = data + pos;
        const uint8_t type = header[0];
        const uint32_t size = header[1] | header[2] << 8 | header[3] << 16;
        const int x = header[4] * kUnit;
        const int y = header[5] * kUnit;
        const int width = header[6] * kUnit;
        const int height = header[7] * kUnit;
        const uint32_t end = pos + kCoordBytes + size;
        const bool valid = (type == kTypeBand || type == kTypeLastBand) &&
                           size > kCoordBytes + 2 && end <= length && x == 0 &&
                           width == kFrameWidth && height > 0 && y == next_y &&
                           y + height <= kFrameHeight && current.count < StreamFrame::kMaxBands &&
                           header[kHeaderBytes] == 0xff && header[kHeaderBytes + 1] == 0xd8;
        if (!valid) {
            current.count = 0;
            next_y = 0;
            *resynced = true;
            pos++;
            continue;
        }
        current.bands[current.count++] = {header + kHeaderBytes, size - kCoordBytes,
                                          static_cast<uint16_t>(y),
                                          static_cast<uint16_t>(height)};
        next_y = y + height;
        pos = end;
        if (type == kTypeLastBand) {
            if (next_y == kFrameHeight) {
                std::copy(current.bands, current.bands + current.count, out->bands);
                out->count = current.count;
                found = true;
            }
            current.count = 0;
            next_y = 0;
        }
    }
    return found;
}

LegacyStreamProfile::~LegacyStreamProfile() {
    portENTER_CRITICAL(&lock_);
    active_ = false;
    portEXIT_CRITICAL(&lock_);
    presenter_flush();
    presenter_end();
    for (Slot *slot : slots_) {
        if (!slot) continue;
        heap_caps_free(slot->transfer.buffer);
        delete slot;
    }
}

usb_device::DeviceInfo LegacyStreamProfile::info() const {
    usb_device::DeviceInfo info;
    info.vendor_id = 0xf055;
    info.product_id = 0x1118;
    info.bcd_device = 0x0200;
    info.manufacturer = "M5Stack";
    info.product = "Tab5 Screen Streamer";
    info.ms_os_20 = true;
    return info;
}

esp_err_t LegacyStreamProfile::add_functions(usb_device::Device &device) {
    for (Slot *&slot : slots_) {
        auto *buffer = static_cast<uint8_t *>(
            heap_caps_aligned_alloc(kSlotAlignment, kSlotBytes, MALLOC_CAP_SPIRAM));
        if (!buffer) {
            ESP_LOGE(TAG, "no memory for %u byte slots", (unsigned)kSlotBytes);
            return ESP_ERR_NO_MEM;
        }
        slot = new Slot{};
        slot->owner = this;
        slot->state = SlotState::Idle;
        slot->transfer.buffer = buffer;
        slot->transfer.length = kSlotBytes;
        slot->transfer.callback = out_done;
        slot->transfer.context = slot;
        slot->frame.release = release;
        slot->frame.context = slot;
    }

    usb_device::Vendor::Config config;
    config.name = "Tab5 JPEG Stream";
    config.in = false;
    config.winusb = true;
    config.device_interface_guid = "{3BD7BBF0-6EC5-41B0-A15F-698A5490FA1E}";
    config.configured = [this] { configured(); };
    config.unconfigured = [this] {
        portENTER_CRITICAL(&lock_);
        active_ = false;
        portEXIT_CRITICAL(&lock_);
        presenter_end();
    };
    vendor_ = std::make_shared<usb_device::Vendor>(std::move(config));
    return device.add(vendor_);
}

void LegacyStreamProfile::configured() {
    portENTER_CRITICAL(&lock_);
    active_ = true;
    portEXIT_CRITICAL(&lock_);
    stats_start_us_ = esp_timer_get_time();
    for (Slot *slot : slots_) {
        if (slot->state == SlotState::Idle) submit(slot);
    }
}

void LegacyStreamProfile::submit(Slot *slot) {
    portENTER_CRITICAL(&lock_);
    const bool active = active_;
    slot->state = active ? SlotState::Usb : SlotState::Idle;
    portEXIT_CRITICAL(&lock_);
    if (!active) return;
    if (vendor_->out().submit(&slot->transfer) != ESP_OK) slot->state = SlotState::Idle;
}

void LegacyStreamProfile::release(StreamFrame *frame) {
    auto *slot = static_cast<Slot *>(frame->context);
    slot->owner->submit(slot);
}

void LegacyStreamProfile::out_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    LegacyStreamProfile *self = slot->owner;
    if (transfer->status == usb_device::TransferStatus::Canceled) {
        slot->state = SlotState::Idle;
        return;
    }
    if (transfer->status != usb_device::TransferStatus::Completed) {
        ESP_LOGW(TAG, "transfer status %d", (int)transfer->status);
        self->submit(slot);
        return;
    }

    bool resynced = false;
    const bool found = parse(transfer->buffer, transfer->actual, &slot->frame, &resynced);
    self->count(transfer->actual, found, resynced);
    if (!found) {
        self->submit(slot);
        return;
    }
    slot->state = SlotState::Presenter;
    presenter_submit(&slot->frame);
}

void LegacyStreamProfile::count(uint32_t bytes, bool frame, bool resynced) {
    stats_bytes_ += bytes;
    if (frame) {
        stats_frames_++;
    } else {
        stats_dropped_++;
    }
    if (resynced) stats_resyncs_++;

    const int64_t now = esp_timer_get_time();
    if (now - stats_start_us_ < kStatsPeriodUs) return;
    const double seconds = (now - stats_start_us_) / 1e6;
    ESP_LOGI(TAG, "%.1f frames/s, %.1f MB/s, %u dropped, %u resyncs", stats_frames_ / seconds,
             stats_bytes_ / seconds / 1e6, (unsigned)stats_dropped_, (unsigned)stats_resyncs_);
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_frames_ = 0;
    stats_dropped_ = 0;
    stats_resyncs_ = 0;
}
