/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "displaylink_profile.hpp"
#include <cstring>
#include "display/panel.hpp"
#include "displaylink.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "usb/usb.hpp"

static const char *TAG = "displaylink_profile";

static constexpr size_t kBufferAlignment = 128;
static constexpr uint32_t kSlotBytes = 16384;
static constexpr TickType_t kIdle = pdMS_TO_TICKS(30);
static constexpr uint32_t kRefreshHz = 60;
static constexpr uint8_t kRequestTypeMask = 0x60;
static constexpr uint8_t kRequestTypeVendor = 0x40;
static constexpr uint8_t kDirectionIn = 0x80;

/* EDID 1.3 with the panel's size as the one detailed timing. */
static void build_edid(uint8_t edid[128], bsp_size_t size) {
    static const uint8_t kHeader[] = {0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00};
    static const uint8_t kChromaticity[] = {0xee, 0x91, 0xa3, 0x54, 0x4c,
                                            0x99, 0x26, 0x0f, 0x50, 0x54};
    memset(edid, 0, 128);
    memcpy(edid, kHeader, sizeof(kHeader));
    const uint16_t vendor = ('T' - '@') << 10 | ('S' - '@') << 5 | ('S' - '@');
    edid[8] = vendor >> 8;
    edid[9] = vendor & 0xff;
    edid[10] = 0x05;
    edid[16] = 1;
    edid[17] = 2026 - 1990;
    edid[18] = 1;
    edid[19] = 3;
    edid[20] = 0x80;
    const bool landscape = size.width > size.height;
    edid[21] = landscape ? 11 : 6;
    edid[22] = landscape ? 6 : 11;
    edid[23] = 120;
    edid[24] = 0x0a;
    memcpy(edid + 25, kChromaticity, sizeof(kChromaticity));
    for (int i = 38; i < 54; i++) edid[i] = 0x01;

    const int hblank = 80, hfront = 8, hsync = 32;
    const int vblank = 20, vfront = 3, vsync = 5;
    const uint32_t clock =
        (size.width + hblank) * (size.height + vblank) * kRefreshHz / 10000;
    const int hmm = landscape ? 110 : 62, vmm = landscape ? 62 : 110;
    uint8_t *d = edid + 54;
    d[0] = clock & 0xff;
    d[1] = clock >> 8;
    d[2] = size.width & 0xff;
    d[3] = hblank & 0xff;
    d[4] = (size.width >> 8) << 4 | hblank >> 8;
    d[5] = size.height & 0xff;
    d[6] = vblank & 0xff;
    d[7] = (size.height >> 8) << 4 | vblank >> 8;
    d[8] = hfront;
    d[9] = hsync;
    d[10] = (vfront & 0xf) << 4 | (vsync & 0xf);
    d[11] = 0;
    d[12] = hmm & 0xff;
    d[13] = vmm & 0xff;
    d[14] = (hmm >> 8) << 4 | vmm >> 8;
    d[17] = 0x1e;

    static const char kName[] = "Tab5\n       ";
    d = edid + 72;
    d[3] = 0xfc;
    memcpy(d + 5, kName, 13);
    d = edid + 90;
    d[3] = 0x10;
    d = edid + 108;
    d[3] = 0x10;

    uint8_t sum = 0;
    for (int i = 0; i < 127; i++) sum += edid[i];
    edid[127] = -sum;
}

DisplayLinkProfile::DisplayLinkProfile(Listeners listeners, bsp_rotation_t rotation)
    : listeners_(std::move(listeners)), rotation_(rotation) {
    build_edid(edid_, panel_logical_size(rotation));
}

DisplayLinkProfile::~DisplayLinkProfile() {
    if (idle_timer_) xTimerDelete(idle_timer_, portMAX_DELAY);
    video_.reset();
    for (Slot *slot : slots_) {
        if (!slot) continue;
        heap_caps_free(slot->transfer.buffer);
        delete slot;
    }
}

usb_device::DeviceInfo DisplayLinkProfile::info() const {
    usb_device::DeviceInfo info;
    info.vendor_id = displaylink::kVendorId;
    info.product_id = displaylink::kProductId;
    info.bcd_device = 0x0100;
    info.manufacturer = "M5Stack";
    info.product = "Tab5 Screen Streamer";
    info.serial = usb_serial();
    return info;
}

esp_err_t DisplayLinkProfile::start() {
    for (Slot *&slot : slots_) {
        auto *buffer = static_cast<uint8_t *>(
            heap_caps_aligned_alloc(kBufferAlignment, kSlotBytes, MALLOC_CAP_SPIRAM));
        if (!buffer) {
            ESP_LOGE(TAG, "no memory for a %u byte buffer", (unsigned)kSlotBytes);
            return ESP_ERR_NO_MEM;
        }
        slot = new Slot{};
        slot->owner = this;
        slot->state = SlotState::Idle;
        slot->transfer.buffer = buffer;
        slot->transfer.length = kSlotBytes;
        slot->transfer.callback = receive_done;
        slot->transfer.context = slot;
    }
    idle_timer_ = xTimerCreate("dl_idle", kIdle, pdFALSE, this, idle);
    if (!idle_timer_) return ESP_ERR_NO_MEM;
    video_ = std::make_unique<DisplayLinkVideo>(
        [this](bool streaming) { this->streaming(streaming); },
        [this](bool on) { listeners_.display_power(on); });
    return video_->start(rotation_);
}

esp_err_t DisplayLinkProfile::add_functions(usb_device::Device &device) {
    usb_device::Vendor::Config config;
    config.name = "Tab5 Display";
    config.in = false;
    config.configured = [this] { configured(); };
    config.unconfigured = [this] { unconfigured(); };
    config.control = [this](const usb_device::ControlRequest &request, uint8_t *data,
                            size_t *bytes) { return control(request, data, bytes); };
    vendor_ = std::make_shared<usb_device::Vendor>(std::move(config));
    return device.add(vendor_);
}

void DisplayLinkProfile::rotate(bsp_rotation_t rotation) {
    video_->rotate(rotation);
}

bool DisplayLinkProfile::control(const usb_device::ControlRequest &request, uint8_t *data,
                                 size_t *bytes) {
    if ((request.request_type & kRequestTypeMask) != kRequestTypeVendor) return false;
    const auto code = static_cast<displaylink::Request>(request.request);
    if (request.request_type & kDirectionIn) {
        if (code != displaylink::Request::ReadEdid || request.index != displaylink::kEdidIndex ||
            *bytes < 2) {
            ESP_LOGW(TAG, "request 0x%02x in, index 0x%04x", request.request, request.index);
            return false;
        }
        const uint32_t offset = request.value >> 8;
        data[0] = 0;
        data[1] = offset < sizeof(edid_) ? edid_[offset] : 0;
        *bytes = 2;
        return true;
    }
    if (code == displaylink::Request::SelectChannel) return true;
    ESP_LOGW(TAG, "request 0x%02x out, %u bytes", request.request, (unsigned)*bytes);
    return false;
}

void DisplayLinkProfile::configured() {
    portENTER_CRITICAL(&lock_);
    active_ = true;
    configured_ = true;
    portEXIT_CRITICAL(&lock_);
    for (Slot *slot : slots_) {
        if (slot->state == SlotState::Idle) submit(slot);
    }
    report();
}

void DisplayLinkProfile::unconfigured() {
    portENTER_CRITICAL(&lock_);
    active_ = false;
    configured_ = false;
    portEXIT_CRITICAL(&lock_);
    xTimerStop(idle_timer_, 0);
    video_->end();
    report();
}

void DisplayLinkProfile::submit(Slot *slot) {
    portENTER_CRITICAL(&lock_);
    const bool active = active_;
    slot->state = active ? SlotState::Usb : SlotState::Idle;
    portEXIT_CRITICAL(&lock_);
    if (!active) return;
    const esp_err_t err = vendor_->out().submit(&slot->transfer);
    if (err != ESP_OK) {
        if (err != ESP_ERR_INVALID_STATE) ESP_LOGE(TAG, "submit: %s", esp_err_to_name(err));
        slot->state = SlotState::Idle;
    }
}

// A transfer the host ends on a whole packet only completes with the next
// one, so the queue is flushed once the host has paused, handing over what
// the waiting transfer holds.
void DisplayLinkProfile::receive_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    DisplayLinkProfile *self = slot->owner;
    const bool completed = transfer->status == usb_device::TransferStatus::Completed;
    const bool canceled = transfer->status == usb_device::TransferStatus::Canceled;
    if (!transfer->actual || (!completed && !canceled)) {
        if (!completed && !canceled) ESP_LOGW(TAG, "bulk status %d", (int)transfer->status);
        self->submit(slot);
        return;
    }
    if (completed) xTimerReset(self->idle_timer_, 0);
    slot->state = SlotState::Busy;
    self->video_->submit(transfer->buffer, transfer->actual, release, slot);
}

void DisplayLinkProfile::release(void *context) {
    auto *slot = static_cast<Slot *>(context);
    slot->owner->submit(slot);
}

void DisplayLinkProfile::idle(TimerHandle_t timer) {
    auto *self = static_cast<DisplayLinkProfile *>(pvTimerGetTimerID(timer));
    portENTER_CRITICAL(&self->lock_);
    const bool active = self->active_;
    portEXIT_CRITICAL(&self->lock_);
    if (active) self->vendor_->out().flush();
}

void DisplayLinkProfile::streaming(bool streaming) {
    portENTER_CRITICAL(&lock_);
    streaming_ = streaming;
    portEXIT_CRITICAL(&lock_);
    report();
}

void DisplayLinkProfile::report() {
    portENTER_CRITICAL(&lock_);
    UsbStatus status = UsbStatus::Unconfigured;
    if (configured_) status = streaming_ ? UsbStatus::Active : UsbStatus::Configured;
    const bool changed = status != reported_;
    reported_ = status;
    portEXIT_CRITICAL(&lock_);
    if (changed) listeners_.status(status);
}
