/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_profile.hpp"
#include "display/panel.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "lvgl.hpp"
#include "ui_orientation.hpp"

static const char *TAG = "tab5_profile";

static constexpr uint32_t kControlBytes = Tab5Events::kTransferBytes;
static constexpr uint32_t kVideoSlotBytes = 16 * 65024;
static constexpr uint32_t kAudioSlotBytes = 16384;
static constexpr size_t kBufferAlignment = 128;
static constexpr uint16_t kMaxFps = 60;

enum class SlotState : uint8_t { Idle, Usb, Busy };

struct Tab5Profile::Slot {
    usb_device::Transfer transfer;
    Receiver *receiver;
    SlotState state;
};

static uint8_t *alloc_buffer(uint32_t bytes, uint32_t caps) {
    auto *buffer = static_cast<uint8_t *>(heap_caps_aligned_alloc(kBufferAlignment, bytes, caps));
    if (!buffer) ESP_LOGE(TAG, "no memory for a %u byte buffer", (unsigned)bytes);
    return buffer;
}

Tab5Profile::Tab5Profile(bool uac, UsbStatusListener status)
    : video_(std::make_unique<Tab5Video>(events_, [this](bool streaming) {
          portENTER_CRITICAL(&lock_);
          streaming_ = streaming;
          portEXIT_CRITICAL(&lock_);
          report_status();
      })),
      audio_(std::make_unique<Tab5Audio>(events_)), uac_(uac), status_(std::move(status)) {
    video_in_ = {this, tab5::Channel::Video, kVideoSlotBytes, nullptr, {}, false};
    audio_in_ = {this, tab5::Channel::Audio, kAudioSlotBytes, nullptr, {}, false};
}

Tab5Profile::~Tab5Profile() {
    lv_lock();
    ui_orientation_set_observer(nullptr, nullptr);
    lv_unlock();
    panel_set_format_listener(nullptr, nullptr);
    panel_set_clip_listener(nullptr, nullptr);
    video_.reset();
    audio_->set_uac_feedback(nullptr);
    audio_.reset();
    heap_caps_free(control_transfer_.buffer);
    destroy(video_in_);
    destroy(audio_in_);
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

esp_err_t Tab5Profile::create(Receiver &receiver, const char *name, const char *guid,
                              uint32_t caps) {
    for (Slot *&slot : receiver.slots) {
        uint8_t *buffer = alloc_buffer(receiver.slot_bytes, caps);
        if (!buffer) return ESP_ERR_NO_MEM;
        slot = new Slot{};
        slot->receiver = &receiver;
        slot->state = SlotState::Idle;
        slot->transfer.buffer = buffer;
        slot->transfer.length = receiver.slot_bytes;
        slot->transfer.callback = receive_done;
        slot->transfer.context = slot;
    }
    usb_device::Vendor::Config config;
    config.name = name;
    config.in = false;
    config.winusb = true;
    config.device_interface_guid = guid;
    config.configured = [this, &receiver] { configured(receiver); };
    config.unconfigured = [this, &receiver] { unconfigured(receiver); };
    receiver.vendor = std::make_shared<usb_device::Vendor>(std::move(config));
    return ESP_OK;
}

void Tab5Profile::destroy(Receiver &receiver) {
    for (Slot *slot : receiver.slots) {
        if (!slot) continue;
        heap_caps_free(slot->transfer.buffer);
        delete slot;
    }
}

esp_err_t Tab5Profile::add_functions(usb_device::Device &device) {
    esp_err_t err = events_.init();
    if (err == ESP_OK) err = video_->start();
    if (err == ESP_OK) err = audio_->start(kSlots * kAudioSlotBytes);
    if (err == ESP_OK) {
        err = create(video_in_, "Tab5 Video", "{65B4EEDE-ACA0-4045-803A-B2454CFF9993}",
                     MALLOC_CAP_SPIRAM);
    }
    if (err == ESP_OK) {
        err = create(audio_in_, "Tab5 Audio", "{2F8AB360-5EA4-4EA4-A2C7-6055646488D6}",
                     MALLOC_CAP_SPIRAM);
    }
    if (err != ESP_OK) return err;

    control_transfer_.buffer = alloc_buffer(kControlBytes, MALLOC_CAP_DMA);
    if (!control_transfer_.buffer) return ESP_ERR_NO_MEM;
    control_transfer_.length = kControlBytes;
    control_transfer_.callback = control_done;
    control_transfer_.context = this;

    usb_device::Vendor::Config control;
    control.name = "Tab5 Control";
    control.winusb = true;
    control.device_interface_guid = "{17A6BA36-2C39-40B2-941A-42436642AF2C}";
    control.configured = [this] { control_configured(); };
    control.unconfigured = [this] { control_unconfigured(); };
    control_ = std::make_shared<usb_device::Vendor>(std::move(control));

    lv_lock();
    orientation_ = ui_orientation_current();
    ui_orientation_set_observer(orientation_changed, this);
    lv_unlock();
    panel_set_format_listener(format_changed, this);
    panel_set_clip_listener(clip_changed, this);
    panel_lock();
    bsp_rect_t clip;
    const bool clipped = panel_clip(&clip);
    portENTER_CRITICAL(&lock_);
    overlay_visible_ = clipped;
    stream_ = clipped ? clip : bsp_rect_t{{0, 0}, panel_size()};
    portEXIT_CRITICAL(&lock_);
    panel_unlock();

    err = device.add(control_);
    if (err == ESP_OK) err = device.add(video_in_.vendor);
    if (err == ESP_OK) err = device.add(audio_in_.vendor);
    if (err != ESP_OK || !uac_) return err;

    usb_device::UacSpeaker::Config speaker;
    speaker.name = "Tab5 Speaker";
    speaker.sync = usb_device::UacSpeaker::Sync::Async;
    speaker.formats = {{2, 16}};
    speaker.sample_rates = {Tab5Audio::kUacRate};
    speaker.volume_min = Tab5Audio::kUacVolumeMin;
    speaker.volume_max = Tab5Audio::kUacVolumeMax;
    speaker.volume_resolution = Tab5Audio::kUacVolumeStep;
    speaker.volume = audio_->uac_volume().volume;
    speaker.started = [this](const usb_device::UacSpeaker::Format &, uint32_t) {
        audio_->uac_started();
    };
    speaker.stopped = [this] { audio_->uac_stopped(); };
    speaker.received = [this](const uint8_t *pcm, size_t bytes) {
        audio_->uac_received(pcm, bytes);
    };
    speaker.volume_changed = [this](bool mute, int16_t volume) {
        audio_->set_uac_volume(mute, volume);
    };
    speaker_ = std::make_shared<usb_device::UacSpeaker>(std::move(speaker));
    audio_->set_uac_feedback([speaker = speaker_.get()](double rate) {
        speaker->set_feedback(rate);
    });

    return device.add(speaker_);
}

void Tab5Profile::orientation_changed(bsp_rotation_t rotation, void *arg) {
    auto *self = static_cast<Tab5Profile *>(arg);
    if (rotation == self->orientation_) return;
    self->orientation_ = rotation;
    const tab5::ByteEvent event = {static_cast<uint8_t>(rotation), {}};
    self->events_.send(tab5::Type::Orientation, 0, &event, sizeof(event));
}

void Tab5Profile::format_changed(bsp_pixel_format_t format, void *arg) {
    static_cast<Tab5Profile *>(arg)->video_->format_changed(format);
}

static tab5::Rect wire_rect(const bsp_rect_t &rect) {
    return {static_cast<uint16_t>(rect.origin.x), static_cast<uint16_t>(rect.origin.y),
            static_cast<uint16_t>(rect.size.width), static_cast<uint16_t>(rect.size.height)};
}

void Tab5Profile::clip_changed(const bsp_rect_t *clip, bool front_changed, void *arg) {
    auto *self = static_cast<Tab5Profile *>(arg);
    const bsp_size_t size = panel_size();
    const bsp_rect_t stream = clip ? *clip : bsp_rect_t{{0, 0}, size};
    portENTER_CRITICAL(&self->lock_);
    const bool was_visible = self->overlay_visible_;
    const bsp_rect_t covered = self->stream_;
    self->overlay_visible_ = clip != nullptr;
    self->stream_ = stream;
    portEXIT_CRITICAL(&self->lock_);

    const tab5::OverlayEvent event = {clip != nullptr, 0, wire_rect(stream)};
    self->events_.send(tab5::Type::Overlay, 0, &event, sizeof(event));
    if (front_changed) {
        const tab5::Rect wire = wire_rect(stream);
        self->events_.send(tab5::Type::Damage, 0, &wire, sizeof(wire));
    }
    if (!was_visible || (clip && clip->origin.x == covered.origin.x &&
                         clip->origin.y == covered.origin.y &&
                         clip->size.width == covered.size.width &&
                         clip->size.height == covered.size.height)) {
        return;
    }
    const int right = covered.origin.x + covered.size.width;
    const int bottom = covered.origin.y + covered.size.height;
    const bsp_rect_t damage[] = {
        {{0, 0}, {size.width, covered.origin.y}},
        {{0, bottom}, {size.width, size.height - bottom}},
        {{0, covered.origin.y}, {covered.origin.x, covered.size.height}},
        {{right, covered.origin.y}, {size.width - right, covered.size.height}},
    };
    for (const bsp_rect_t &rect : damage) {
        if (rect.size.width <= 0 || rect.size.height <= 0) continue;
        const tab5::Rect wire = wire_rect(rect);
        self->events_.send(tab5::Type::Damage, 0, &wire, sizeof(wire));
    }
}

void Tab5Profile::report_status() {
    portENTER_CRITICAL(&lock_);
    const UsbStatus status = !configured_ ? UsbStatus::Unconfigured
                             : streaming_ ? UsbStatus::Active
                                          : UsbStatus::Configured;
    const bool changed = status != reported_;
    reported_ = status;
    portEXIT_CRITICAL(&lock_);
    if (changed && status_) status_(status);
}

void Tab5Profile::control_configured() {
    portENTER_CRITICAL(&lock_);
    configured_ = true;
    portEXIT_CRITICAL(&lock_);
    report_status();
    events_.start(control_->in());
    esp_err_t err = control_->out().submit(&control_transfer_);
    if (err != ESP_OK) ESP_LOGE(TAG, "submit control: %s", esp_err_to_name(err));
}

void Tab5Profile::control_unconfigured() {
    portENTER_CRITICAL(&lock_);
    configured_ = false;
    portEXIT_CRITICAL(&lock_);
    report_status();
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

void Tab5Profile::handle_control(const tab5::MsgHeader &header, const uint8_t *payload) {
    switch (static_cast<tab5::Type>(header.type)) {
        case tab5::Type::GetInfo:
            send_info(header.seq);
            return;
        case tab5::Type::SetVolume: {
            tab5::VolumeMessage message;
            if (header.length != sizeof(message)) break;
            memcpy(&message, payload, sizeof(message));
            if (message.stream != static_cast<uint8_t>(tab5::Stream::Bulk)) {
                events_.error(tab5::Channel::Control, tab5::ErrorCode::Unsupported, header.seq);
                return;
            }
            audio_->set_volume(message.mute, message.volume);
            return;
        }
        default:
            break;
    }
    events_.error(tab5::Channel::Control, tab5::ErrorCode::Malformed, header.seq);
}

void Tab5Profile::send_info(uint32_t seq) {
    const bsp_size_t size = panel_size();
    tab5::Info info = {};
    info.protocol_version = tab5::kProtocolVersion;
    info.panel_width = size.width;
    info.panel_height = size.height;
    info.pixel_format = static_cast<uint8_t>(tab5_pixel_format(panel_format()));
    info.orientation = orientation_;
    info.framebuffers = panel_framebuffers();
    info.swap_window = Tab5Video::kSwapWindow;
    info.direct_align = Tab5Video::kDirectAlign;
    info.blit_max_width = Tab5Video::kBlitMaxSize;
    info.blit_max_height = Tab5Video::kBlitMaxSize;
    info.encodings = 1u << static_cast<int>(tab5::Encoding::Jpeg) |
                     1u << static_cast<int>(tab5::Encoding::Raw) |
                     1u << static_cast<int>(tab5::Encoding::Lz4);
    info.max_message[static_cast<int>(tab5::Channel::Control)] = kControlBytes;
    info.max_message[static_cast<int>(tab5::Channel::Video)] = kVideoSlotBytes;
    info.max_message[static_cast<int>(tab5::Channel::Audio)] = kAudioSlotBytes;
    info.epoch = video_->epoch();
    portENTER_CRITICAL(&lock_);
    info.overlay_visible = overlay_visible_;
    const tab5::Rect stream = wire_rect(stream_);
    portEXIT_CRITICAL(&lock_);
    info.overlay_x = stream.x;
    info.overlay_y = stream.y;
    info.overlay_w = stream.w;
    info.overlay_h = stream.h;
    info.max_fps = kMaxFps;
    info.volume[static_cast<int>(tab5::Stream::Uac)] = audio_->uac_volume();
    info.volume[static_cast<int>(tab5::Stream::Bulk)] = audio_->volume();
    info.audio_max_rate = Tab5Audio::kMaxRate;
    events_.send(tab5::Type::Info, seq, &info, sizeof(info), info.epoch);
}

void Tab5Profile::configured(Receiver &receiver) {
    portENTER_CRITICAL(&lock_);
    receiver.active = true;
    portEXIT_CRITICAL(&lock_);
    for (Slot *slot : receiver.slots) {
        if (slot->state == SlotState::Idle) submit(slot);
    }
}

void Tab5Profile::unconfigured(Receiver &receiver) {
    portENTER_CRITICAL(&lock_);
    receiver.active = false;
    portEXIT_CRITICAL(&lock_);
    if (receiver.channel == tab5::Channel::Video) {
        video_->end();
    } else {
        audio_->end();
    }
}

void Tab5Profile::submit(Slot *slot) {
    Receiver &receiver = *slot->receiver;
    portENTER_CRITICAL(&lock_);
    const bool active = receiver.active;
    slot->state = active ? SlotState::Usb : SlotState::Idle;
    portEXIT_CRITICAL(&lock_);
    if (!active) return;
    esp_err_t err = receiver.vendor->out().submit(&slot->transfer);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "submit %u: %s", (unsigned)receiver.channel, esp_err_to_name(err));
        slot->state = SlotState::Idle;
    }
}

void Tab5Profile::release(void *context) {
    auto *slot = static_cast<Slot *>(context);
    slot->receiver->owner->submit(slot);
}

void Tab5Profile::receive_done(usb_device::Transfer *transfer) {
    auto *slot = static_cast<Slot *>(transfer->context);
    Receiver &receiver = *slot->receiver;
    if (transfer->status == usb_device::TransferStatus::Canceled) {
        slot->state = SlotState::Idle;
        return;
    }
    if (transfer->status != usb_device::TransferStatus::Completed) {
        ESP_LOGW(TAG, "channel %u status %d", (unsigned)receiver.channel, (int)transfer->status);
        receiver.owner->submit(slot);
        return;
    }
    slot->state = SlotState::Busy;
    if (receiver.channel == tab5::Channel::Video) {
        receiver.owner->video_->submit(transfer->buffer, transfer->actual, release, slot);
    } else {
        receiver.owner->audio_->submit(transfer->buffer, transfer->actual, release, slot);
    }
}
