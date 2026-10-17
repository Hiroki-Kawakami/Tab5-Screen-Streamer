/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "gud_profile.hpp"
#include <algorithm>
#include <cstring>
#include "display/panel.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "usb/usb.hpp"

static const char *TAG = "gud_profile";

static constexpr size_t kBufferAlignment = 128;
static constexpr uint32_t kHighSpeedPacketBytes = 512;
static constexpr uint32_t kFullSpeedPacketBytes = 64;
// The most one bulk OUT transfer of the USB stack takes.
static constexpr uint32_t kMaxTransferBytes = 16 * 65024;
static constexpr uint32_t kRefreshHz = 60;
static constexpr TickType_t kSlotWait = pdMS_TO_TICKS(1000);
static constexpr uint8_t kRequestTypeMask = 0x60;
static constexpr uint8_t kRequestTypeVendor = 0x40;
static constexpr uint8_t kDirectionIn = 0x80;

static bool transposed(bsp_rotation_t rotation) {
    return rotation == BSP_ROTATION_90 || rotation == BSP_ROTATION_270;
}

static gud::DisplayMode display_mode(bsp_size_t size) {
    gud::DisplayMode mode = {};
    mode.hdisplay = size.width;
    mode.hsync_start = size.width + 8;
    mode.hsync_end = size.width + 40;
    mode.htotal = size.width + 80;
    mode.vdisplay = size.height;
    mode.vsync_start = size.height + 3;
    mode.vsync_end = size.height + 8;
    mode.vtotal = size.height + 20;
    mode.clock = mode.htotal * mode.vtotal * kRefreshHz / 1000;
    mode.flags = gud::kModeFlagPreferred;
    return mode;
}

GudProfile::GudProfile(Listeners listeners, bsp_rotation_t rotation, int brightness)
    : listeners_(std::move(listeners)), rotation_(rotation), brightness_(brightness),
      checked_brightness_(brightness) {}

GudProfile::~GudProfile() {
    video_.reset();
    for (Slot *slot : slots_) {
        if (!slot) continue;
        heap_caps_free(slot->transfer.buffer);
        delete slot;
    }
    if (free_slots_) vQueueDelete(free_slots_);
}

usb_device::DeviceInfo GudProfile::info() const {
    usb_device::DeviceInfo info;
    info.vendor_id = gud::kVendorId;
    info.product_id = gud::kProductId;
    info.bcd_device = 0x0100;
    info.manufacturer = "M5Stack";
    info.product = "Tab5 Screen Streamer";
    info.serial = usb_serial();
    return info;
}

esp_err_t GudProfile::start() {
    const bsp_size_t panel = panel_size();
    format_ = panel_format() == BSP_PIXEL_FORMAT_RGB888 ? BSP_PIXEL_FORMAT_RGB888
                                                         : BSP_PIXEL_FORMAT_RGB565;
    wire_format_ = format_ == BSP_PIXEL_FORMAT_RGB888 ? gud::kPixelFormatRgb888
                                                      : gud::kPixelFormatRgb565;
    buffer_bytes_ = std::min<uint32_t>(panel.width * panel.height * bsp_pixel_format_bytes(format_),
                                       kMaxTransferBytes);
    free_slots_ = xQueueCreate(kSlots, sizeof(Slot *));
    if (!free_slots_) return ESP_ERR_NO_MEM;
    const uint32_t slot_bytes = (buffer_bytes_ + kHighSpeedPacketBytes - 1) /
                                kHighSpeedPacketBytes * kHighSpeedPacketBytes;
    for (Slot *&slot : slots_) {
        auto *buffer = static_cast<uint8_t *>(
            heap_caps_aligned_alloc(kBufferAlignment, slot_bytes, MALLOC_CAP_SPIRAM));
        if (!buffer) {
            ESP_LOGE(TAG, "no memory for a %u byte buffer", (unsigned)slot_bytes);
            return ESP_ERR_NO_MEM;
        }
        slot = new Slot{};
        slot->owner = this;
        slot->transfer.buffer = buffer;
        slot->transfer.callback = receive_done;
        slot->transfer.context = slot;
        xQueueSend(free_slots_, &slot, 0);
    }
    video_ = std::make_unique<GudVideo>([this](bool streaming) { this->streaming(streaming); });
    return video_->start(rotation_, format_, buffer_bytes_);
}

esp_err_t GudProfile::add_functions(usb_device::Device &device) {
    device_ = &device;
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

void GudProfile::rotate(bsp_rotation_t rotation) {
    if (transposed(rotation_.exchange(rotation)) != transposed(rotation)) changed_ = true;
    video_->rotate(rotation);
}

bool GudProfile::control(const usb_device::ControlRequest &request, uint8_t *data,
                         size_t *bytes) {
    if ((request.request_type & kRequestTypeMask) != kRequestTypeVendor) return false;
    const auto code = static_cast<gud::Request>(request.request);
    if (code == gud::Request::GetStatus) {
        if (*bytes < 1) return false;
        data[0] = static_cast<uint8_t>(status_);
        *bytes = 1;
        return true;
    }
    status_ = gud::Status::Ok;
    const bool ok = request.request_type & kDirectionIn ? get(code, request.value, data, bytes)
                                                        : set(code, request.value, data, *bytes);
    if (!ok && status_ == gud::Status::Ok) status_ = gud::Status::RequestNotSupported;
    if (!ok) ESP_LOGW(TAG, "request 0x%02x: status %d", request.request, (int)status_);
    return ok;
}

template <typename T>
static bool reply(const T &value, uint8_t *data, size_t *bytes) {
    if (*bytes < sizeof(value)) return false;
    memcpy(data, &value, sizeof(value));
    *bytes = sizeof(value);
    return true;
}

bool GudProfile::get(gud::Request request, uint16_t value, uint8_t *data, size_t *bytes) {
    const bsp_size_t panel = panel_size();
    switch (request) {
        case gud::Request::GetDescriptor: {
            gud::DisplayDescriptor descriptor = {};
            descriptor.magic = gud::kMagic;
            descriptor.version = gud::kVersion;
            descriptor.compression = gud::kCompressionLz4;
            descriptor.max_buffer_size = buffer_bytes_;
            descriptor.min_width = std::min(panel.width, panel.height);
            descriptor.max_width = std::max(panel.width, panel.height);
            descriptor.min_height = descriptor.min_width;
            descriptor.max_height = descriptor.max_width;
            return reply(descriptor, data, bytes);
        }
        case gud::Request::GetFormats:
            return reply(wire_format_, data, bytes);
        case gud::Request::GetProperties:
            *bytes = 0;
            return true;
        case gud::Request::GetConnectors:
            return reply(gud::ConnectorDescriptor{gud::kConnectorTypePanel,
                                                  gud::kConnectorFlagPollStatus},
                         data, bytes);
        default:
            break;
    }
    if (value != 0) {
        status_ = gud::Status::InvalidParameter;
        return false;
    }
    switch (request) {
        case gud::Request::GetConnectorProperties:
            return reply(gud::Property{gud::kPropertyBacklightBrightness, (uint64_t)brightness_},
                         data, bytes);
        case gud::Request::GetConnectorStatus: {
            const bool changed = changed_.exchange(false);
            return reply<uint8_t>(gud::kConnectorStatusConnected |
                                      (changed ? gud::kConnectorStatusChanged : 0),
                                  data, bytes);
        }
        case gud::Request::GetConnectorModes:
            return reply(display_mode(panel_logical_size(rotation_)), data, bytes);
        case gud::Request::GetConnectorEdid:
            *bytes = 0;
            return true;
        default:
            return false;
    }
}

bool GudProfile::set(gud::Request request, uint16_t value, const uint8_t *data, size_t bytes) {
    switch (request) {
        case gud::Request::SetConnectorForceDetect:
            return true;
        case gud::Request::SetBuffer:
            return set_buffer(data, bytes);
        case gud::Request::SetStateCheck:
            return check_state(data, bytes);
        case gud::Request::SetStateCommit:
            mode_ = checked_mode_;
            video_->set_mode(mode_);
            if (checked_brightness_ != brightness_) {
                brightness_ = checked_brightness_;
                listeners_.brightness(brightness_);
            }
            return true;
        case gud::Request::SetControllerEnable:
            if (bytes != 1) break;
            video_->set_enabled(data[0] != 0);
            return true;
        case gud::Request::SetDisplayEnable:
            if (bytes != 1) break;
            listeners_.display_power(data[0] != 0);
            return true;
        default:
            return false;
    }
    status_ = gud::Status::ProtocolError;
    return false;
}

bool GudProfile::check_state(const uint8_t *data, size_t bytes) {
    gud::State state;
    if (bytes < sizeof(state) || (bytes - sizeof(state)) % sizeof(gud::Property)) {
        status_ = gud::Status::ProtocolError;
        return false;
    }
    memcpy(&state, data, sizeof(state));
    const bsp_size_t panel = panel_size();
    const bsp_size_t size = {state.mode.hdisplay, state.mode.vdisplay};
    const bool fits = (size.width == panel.width && size.height == panel.height) ||
                      (size.width == panel.height && size.height == panel.width);
    if (!fits || state.format != wire_format_ || state.connector != 0) {
        ESP_LOGW(TAG, "state %dx%d format 0x%02x connector %u rejected", size.width, size.height,
                 state.format, state.connector);
        status_ = gud::Status::InvalidParameter;
        return false;
    }
    int brightness = brightness_;
    for (size_t offset = sizeof(state); offset < bytes; offset += sizeof(gud::Property)) {
        gud::Property property;
        memcpy(&property, data + offset, sizeof(property));
        if (property.prop == gud::kPropertyBacklightBrightness) {
            brightness = std::min<uint64_t>(property.val, 100);
        }
    }
    checked_mode_ = size;
    checked_brightness_ = brightness;
    return true;
}

bool GudProfile::set_buffer(const uint8_t *data, size_t bytes) {
    gud::SetBuffer buffer;
    if (bytes != sizeof(buffer)) {
        status_ = gud::Status::ProtocolError;
        return false;
    }
    memcpy(&buffer, data, sizeof(buffer));
    const bool lz4 = buffer.compression == gud::kCompressionLz4;
    const uint32_t bytes_per_pixel = bsp_pixel_format_bytes(format_);
    const uint32_t length = lz4 ? buffer.compressed_length : buffer.length;
    if ((buffer.compression && !lz4) || !buffer.width || !buffer.height ||
        buffer.x + buffer.width > (uint32_t)mode_.width ||
        buffer.y + buffer.height > (uint32_t)mode_.height ||
        buffer.length != buffer.width * buffer.height * bytes_per_pixel || !length ||
        length > buffer_bytes_) {
        ESP_LOGW(TAG, "buffer %ux%u at %u,%u, %u bytes, compression %u rejected",
                 (unsigned)buffer.width, (unsigned)buffer.height, (unsigned)buffer.x,
                 (unsigned)buffer.y, (unsigned)length, buffer.compression);
        status_ = gud::Status::InvalidParameter;
        return false;
    }
    // A transfer still armed is from a bulk transfer the host gave up on.
    if (armed_) vendor_->out().flush();
    Slot *slot;
    if (xQueueReceive(free_slots_, &slot, kSlotWait) != pdTRUE) {
        ESP_LOGW(TAG, "no free buffer");
        status_ = gud::Status::Busy;
        return false;
    }
    const uint32_t packet = device_->speed() == usb_device::Speed::High ? kHighSpeedPacketBytes
                                                                        : kFullSpeedPacketBytes;
    slot->buffer = buffer;
    slot->transfer.length = (length + packet - 1) / packet * packet;
    armed_ = slot;
    const esp_err_t err = vendor_->out().submit(&slot->transfer);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "submit: %s", esp_err_to_name(err));
        armed_ = nullptr;
        xQueueSend(free_slots_, &slot, 0);
        status_ = gud::Status::Error;
        return false;
    }
    return true;
}

void GudProfile::receive_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    GudProfile *self = slot->owner;
    if (self->armed_ == slot) self->armed_ = nullptr;
    const gud::SetBuffer &buffer = slot->buffer;
    const uint32_t length =
        buffer.compression ? buffer.compressed_length : buffer.length;
    if (transfer->status != usb_device::TransferStatus::Completed || transfer->actual != length) {
        if (transfer->status != usb_device::TransferStatus::Canceled) {
            ESP_LOGW(TAG, "bulk status %d, %u of %u bytes", (int)transfer->status,
                     (unsigned)transfer->actual, (unsigned)length);
        }
        xQueueSend(self->free_slots_, &slot, 0);
        return;
    }
    self->video_->submit(buffer, transfer->buffer, length, release, slot);
}

void GudProfile::release(void *context) {
    auto *slot = static_cast<Slot *>(context);
    xQueueSend(slot->owner->free_slots_, &slot, 0);
}

void GudProfile::configured() {
    portENTER_CRITICAL(&lock_);
    configured_ = true;
    portEXIT_CRITICAL(&lock_);
    report();
}

void GudProfile::unconfigured() {
    armed_ = nullptr;
    video_->end();
    portENTER_CRITICAL(&lock_);
    configured_ = false;
    portEXIT_CRITICAL(&lock_);
    report();
}

void GudProfile::streaming(bool streaming) {
    portENTER_CRITICAL(&lock_);
    streaming_ = streaming;
    portEXIT_CRITICAL(&lock_);
    report();
}

void GudProfile::report() {
    portENTER_CRITICAL(&lock_);
    UsbStatus status = UsbStatus::Unconfigured;
    if (configured_) status = streaming_ ? UsbStatus::Active : UsbStatus::Configured;
    const bool changed = status != reported_;
    reported_ = status;
    portEXIT_CRITICAL(&lock_);
    if (changed) listeners_.status(status);
}
