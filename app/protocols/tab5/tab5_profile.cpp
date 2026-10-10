/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_profile.hpp"
#include "display/panel.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"

static const char *TAG = "tab5_profile";

static constexpr uint32_t kControlBytes = Tab5Events::kTransferBytes;
static constexpr uint32_t kSlotBytes = 16 * 65024;
static constexpr size_t kBufferAlignment = 128;
static constexpr uint16_t kMaxFps = 60;

enum class SlotState : uint8_t { Idle, Usb, Video };

struct Tab5Profile::Slot {
    usb_device::Transfer transfer;
    Tab5Profile *owner;
    SlotState state;
};

static uint8_t *alloc_buffer(uint32_t bytes, uint32_t caps) {
    auto *buffer = static_cast<uint8_t *>(heap_caps_aligned_alloc(kBufferAlignment, bytes, caps));
    if (!buffer) ESP_LOGE(TAG, "no memory for a %u byte buffer", (unsigned)bytes);
    return buffer;
}

Tab5Profile::Tab5Profile() : video_(std::make_unique<Tab5Video>(events_)) {}

Tab5Profile::~Tab5Profile() {
    video_.reset();
    heap_caps_free(control_transfer_.buffer);
    for (Slot *slot : slots_) {
        if (!slot) continue;
        heap_caps_free(slot->transfer.buffer);
        delete slot;
    }
}

usb_device::DeviceInfo Tab5Profile::info() const {
    usb_device::DeviceInfo info;
    info.vendor_id = 0xf055;
    info.product_id = 0x1118;
    info.bcd_device = 0x0300;
    info.manufacturer = "M5Stack";
    info.product = "Tab5 Screen Streamer";
    info.ms_os_20 = true;
    return info;
}

esp_err_t Tab5Profile::add_functions(usb_device::Device &device) {
    esp_err_t err = events_.init();
    if (err == ESP_OK) err = video_->start();
    if (err != ESP_OK) return err;

    control_transfer_.buffer = alloc_buffer(kControlBytes, MALLOC_CAP_DMA);
    if (!control_transfer_.buffer) return ESP_ERR_NO_MEM;
    control_transfer_.length = kControlBytes;
    control_transfer_.callback = control_done;
    control_transfer_.context = this;

    for (Slot *&slot : slots_) {
        uint8_t *buffer = alloc_buffer(kSlotBytes, MALLOC_CAP_SPIRAM);
        if (!buffer) return ESP_ERR_NO_MEM;
        slot = new Slot{};
        slot->owner = this;
        slot->state = SlotState::Idle;
        slot->transfer.buffer = buffer;
        slot->transfer.length = kSlotBytes;
        slot->transfer.callback = video_done;
        slot->transfer.context = slot;
    }

    usb_device::Vendor::Config control;
    control.name = "Tab5 Control";
    control.winusb = true;
    control.device_interface_guid = "{17A6BA36-2C39-40B2-941A-42436642AF2C}";
    control.configured = [this] { control_configured(); };
    control.unconfigured = [this] { control_unconfigured(); };
    control_ = std::make_shared<usb_device::Vendor>(std::move(control));

    usb_device::Vendor::Config video;
    video.name = "Tab5 Video";
    video.in = false;
    video.winusb = true;
    video.device_interface_guid = "{65B4EEDE-ACA0-4045-803A-B2454CFF9993}";
    video.configured = [this] { video_configured(); };
    video.unconfigured = [this] { video_unconfigured(); };
    video_vendor_ = std::make_shared<usb_device::Vendor>(std::move(video));

    err = device.add(control_);
    if (err == ESP_OK) err = device.add(video_vendor_);
    return err;
}

void Tab5Profile::control_configured() {
    events_.start(control_->in());
    esp_err_t err = control_->out().submit(&control_transfer_);
    if (err != ESP_OK) ESP_LOGE(TAG, "submit control: %s", esp_err_to_name(err));
}

void Tab5Profile::control_unconfigured() {
    events_.stop();
}

void Tab5Profile::control_done(usb_device::Transfer *transfer) {
    auto *self = static_cast<Tab5Profile *>(transfer->context);
    if (transfer->status == usb_device::TransferStatus::Canceled) return;
    if (transfer->status == usb_device::TransferStatus::Completed) {
        uint32_t bad_seq = 0;
        const bool ok = tab5::for_each_message(
            transfer->buffer, transfer->actual, &bad_seq,
            [self](const tab5::MsgHeader &header, const uint8_t *payload) {
                self->handle_control(header, payload);
            });
        if (!ok) self->events_.error(tab5::Channel::Control, tab5::ErrorCode::Malformed, bad_seq);
    } else {
        ESP_LOGW(TAG, "control status %d", (int)transfer->status);
    }
    esp_err_t err = self->control_->out().submit(transfer);
    if (err != ESP_OK) ESP_LOGE(TAG, "submit control: %s", esp_err_to_name(err));
}

void Tab5Profile::handle_control(const tab5::MsgHeader &header, const uint8_t *) {
    switch (static_cast<tab5::Type>(header.type)) {
        case tab5::Type::GetInfo:
            send_info(header.seq);
            return;
        default:
            events_.error(tab5::Channel::Control, tab5::ErrorCode::Malformed, header.seq);
            return;
    }
}

void Tab5Profile::send_info(uint32_t seq) {
    const bsp_size_t size = panel_size();
    tab5::Info info = {};
    info.protocol_version = tab5::kProtocolVersion;
    info.panel_width = size.width;
    info.panel_height = size.height;
    info.pixel_format = static_cast<uint8_t>(panel_format() == BSP_PIXEL_FORMAT_RGB888
                                                 ? tab5::PixelFormat::Rgb888
                                                 : tab5::PixelFormat::Rgb565);
    info.framebuffers = panel_framebuffers();
    info.swap_window = Tab5Video::kSwapWindow;
    info.direct_align = Tab5Video::kDirectAlign;
    info.encodings = 1u << static_cast<int>(tab5::Encoding::Jpeg) |
                     1u << static_cast<int>(tab5::Encoding::Raw) |
                     1u << static_cast<int>(tab5::Encoding::Lz4);
    info.max_message[static_cast<int>(tab5::Channel::Control)] = kControlBytes;
    info.max_message[static_cast<int>(tab5::Channel::Video)] = kSlotBytes;
    info.epoch = video_->epoch();
    info.overlay_w = size.width;
    info.overlay_h = size.height;
    info.max_fps = kMaxFps;
    events_.send(tab5::Type::Info, seq, &info, sizeof(info), info.epoch);
}

void Tab5Profile::video_configured() {
    portENTER_CRITICAL(&lock_);
    video_active_ = true;
    portEXIT_CRITICAL(&lock_);
    for (Slot *slot : slots_) {
        if (slot->state == SlotState::Idle) submit(slot);
    }
}

void Tab5Profile::video_unconfigured() {
    portENTER_CRITICAL(&lock_);
    video_active_ = false;
    portEXIT_CRITICAL(&lock_);
    video_->end();
}

void Tab5Profile::submit(Slot *slot) {
    portENTER_CRITICAL(&lock_);
    const bool active = video_active_;
    slot->state = active ? SlotState::Usb : SlotState::Idle;
    portEXIT_CRITICAL(&lock_);
    if (!active) return;
    esp_err_t err = video_vendor_->out().submit(&slot->transfer);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "submit video: %s", esp_err_to_name(err));
        slot->state = SlotState::Idle;
    }
}

void Tab5Profile::release(void *context) {
    auto *slot = static_cast<Slot *>(context);
    slot->owner->submit(slot);
}

void Tab5Profile::video_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    Tab5Profile *self = slot->owner;
    if (transfer->status == usb_device::TransferStatus::Canceled) {
        slot->state = SlotState::Idle;
        return;
    }
    if (transfer->status != usb_device::TransferStatus::Completed) {
        ESP_LOGW(TAG, "video status %d", (int)transfer->status);
        self->submit(slot);
        return;
    }
    slot->state = SlotState::Video;
    self->video_->submit(transfer->buffer, transfer->actual, release, slot);
}
