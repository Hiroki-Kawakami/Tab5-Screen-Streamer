/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb_test.hpp"
#include <algorithm>
#include <cstring>
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "usb_device_vendor.hpp"

static const char *TAG = "usb_test";

static constexpr uint8_t kRequestTypeMask = 0x60;
static constexpr uint8_t kRequestTypeVendor = 0x40;
static constexpr uint8_t kRequestDirIn = 0x80;
static constexpr uint8_t kRequestSetMode = 0x01;
static constexpr uint8_t kRequestStats = 0x02;
static constexpr uint8_t kRequestEcho = 0x03;

static constexpr uint16_t kModeLoopback = 0;
static constexpr uint16_t kModeSinkPsram = 1;
static constexpr uint16_t kModeSinkInternal = 2;

static constexpr uint32_t kDefaultTransferKb = 128;
static constexpr int kSinkSlots = 4;
static constexpr size_t kBufferAlignment = 128;
static constexpr size_t kEchoBytes = 256;

struct Slot {
    usb_device::Transfer out;
    usb_device::Transfer in;
    uint32_t generation;
};

struct Stats {
    uint64_t bytes;
    uint32_t transfers;
    uint32_t errors;
};

static std::shared_ptr<usb_device::Vendor> s_vendor;
static uint32_t s_generation;
static uint16_t s_mode;
static Stats s_stats;
static uint8_t s_echo[kEchoBytes];
static size_t s_echo_bytes;

static void free_slot(Slot *slot) {
    heap_caps_free(slot->out.buffer);
    delete slot;
}

static bool stale(Slot *slot, usb_device::Transfer *transfer) {
    const bool canceled = transfer->status == usb_device::TransferStatus::Canceled;
    if (slot->generation == s_generation && !canceled) return false;
    free_slot(slot);
    return true;
}

static void submit_out(Slot *slot) {
    esp_err_t err = s_vendor->out().submit(&slot->out);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "submit out: %s", esp_err_to_name(err));
        free_slot(slot);
    }
}

static void in_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    if (stale(slot, transfer)) return;
    if (transfer->status != usb_device::TransferStatus::Completed) s_stats.errors++;
    submit_out(slot);
}

static void out_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    if (stale(slot, transfer)) return;
    if (transfer->status != usb_device::TransferStatus::Completed) {
        s_stats.errors++;
        submit_out(slot);
        return;
    }
    s_stats.bytes += transfer->actual;
    s_stats.transfers++;
    if (s_mode != kModeLoopback) {
        submit_out(slot);
        return;
    }
    slot->in.length = transfer->actual;
    slot->in.zlp = transfer->actual < transfer->length;
    esp_err_t err = s_vendor->in().submit(&slot->in);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "submit in: %s", esp_err_to_name(err));
        free_slot(slot);
    }
}

static bool add_slot(uint32_t bytes, uint32_t caps) {
    auto *buffer = static_cast<uint8_t *>(heap_caps_aligned_alloc(kBufferAlignment, bytes, caps));
    if (!buffer) return false;
    auto *slot = new Slot{};
    slot->generation = s_generation;
    slot->out.buffer = buffer;
    slot->out.length = bytes;
    slot->out.callback = out_done;
    slot->out.context = slot;
    slot->in.buffer = buffer;
    slot->in.callback = in_done;
    slot->in.context = slot;
    submit_out(slot);
    return true;
}

static void stop() {
    s_generation++;
    s_vendor->out().flush();
    s_vendor->in().flush();
}

static bool start(uint16_t mode, uint32_t kb) {
    stop();
    s_mode = mode;
    s_stats = {};
    const uint32_t bytes = (kb ? kb : kDefaultTransferKb) * 1024;
    const uint32_t caps = mode == kModeSinkInternal ? MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL
                                                    : MALLOC_CAP_SPIRAM;
    const int slots = mode == kModeLoopback ? 1 : kSinkSlots;
    for (int i = 0; i < slots; i++) {
        if (!add_slot(bytes, caps)) {
            ESP_LOGE(TAG, "no memory for %u byte slots", (unsigned)bytes);
            return false;
        }
    }
    ESP_LOGI(TAG, "mode %u, %u x %u bytes", mode, slots, (unsigned)bytes);
    return true;
}

static bool control(const usb_device::ControlRequest &request, uint8_t *data, size_t *bytes) {
    if ((request.request_type & kRequestTypeMask) != kRequestTypeVendor) return false;
    const bool in = request.request_type & kRequestDirIn;
    switch (request.request) {
        case kRequestSetMode:
            if (in || request.value > kModeSinkInternal) return false;
            return start(request.value, request.index);
        case kRequestStats:
            if (!in || *bytes < sizeof(s_stats)) return false;
            memcpy(data, &s_stats, sizeof(s_stats));
            *bytes = sizeof(s_stats);
            return true;
        case kRequestEcho:
            if (in) {
                *bytes = std::min(*bytes, s_echo_bytes);
                memcpy(data, s_echo, *bytes);
            } else {
                if (*bytes > kEchoBytes) return false;
                s_echo_bytes = *bytes;
                memcpy(s_echo, data, *bytes);
            }
            return true;
        default:
            return false;
    }
}

std::shared_ptr<usb_device::Function> usb_test_function() {
    usb_device::Vendor::Config config;
    config.name = "Tab5 USB Test";
    config.winusb = true;
    config.device_interface_guid = "{3BD7BBF0-6EC5-41B0-A15F-698A5490FA1E}";
    config.configured = [] { start(kModeLoopback, kDefaultTransferKb); };
    config.unconfigured = [] { s_generation++; };
    config.control = control;
    s_vendor = std::make_shared<usb_device::Vendor>(std::move(config));
    return s_vendor;
}
