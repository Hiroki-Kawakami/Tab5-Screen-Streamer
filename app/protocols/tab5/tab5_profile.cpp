/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_profile.hpp"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_mac.h"

static const char *TAG = "tab5_profile";

static constexpr size_t kBufferAlignment = 128;

enum class SlotState : uint8_t { Idle, Usb, Flushing, Busy };

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

Tab5Profile::Tab5Profile(std::shared_ptr<Tab5Service> service, bool uac, bool keyboard,
                         bool mouse)
    : service_(std::move(service)), uac_(uac) {
    video_in_ = {this, tab5::Channel::Video, Tab5Service::kVideoBytes, nullptr, {}, false};
    audio_in_ = {this, tab5::Channel::Audio, Tab5Service::kAudioBytes, nullptr, {}, false};
    if (keyboard) {
        usb_device::Hid::Config config;
        config.name = "Tab5 Keyboard";
        config.boot = usb_device::Hid::Boot::Keyboard;
        keyboard_ = std::make_shared<usb_device::Hid>(std::move(config));
    }
    if (mouse) {
        usb_device::Hid::Config config;
        config.name = "Tab5 Trackpad";
        config.boot = usb_device::Hid::Boot::Mouse;
        mouse_ = std::make_shared<usb_device::Hid>(std::move(config));
    }
}

Tab5Profile::~Tab5Profile() {
    if (attached_) service_->detach();
    if (speaker_) service_->audio().set_uac_feedback(nullptr);
    heap_caps_free(control_transfer_.buffer);
    destroy(video_in_);
    destroy(audio_in_);
}

usb_device::DeviceInfo Tab5Profile::info() const {
    usb_device::DeviceInfo info;
    info.vendor_id = 0xf055;
    info.product_id = 0x1118;
    // Windows caches MS OS 2.0 descriptors per bcdDevice; distinguish each interface layout.
    info.bcd_device = 0x0300 | (uac_ ? 0x01 : 0) | (keyboard_ ? 0x02 : 0) | (mouse_ ? 0x04 : 0);
    info.manufacturer = "M5Stack";
    info.product = "Tab5 Screen Streamer";
    uint8_t mac[6];
    if (esp_efuse_mac_get_default(mac) == ESP_OK) {
        char serial[13];
        snprintf(serial, sizeof(serial), "%02X%02X%02X%02X%02X%02X", mac[0], mac[1], mac[2],
                 mac[3], mac[4], mac[5]);
        info.serial = serial;
    }
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
    esp_err_t err = create(video_in_, "Tab5 Video", "{65B4EEDE-ACA0-4045-803A-B2454CFF9993}",
                           MALLOC_CAP_SPIRAM);
    if (err == ESP_OK) {
        err = create(audio_in_, "Tab5 Audio", "{2F8AB360-5EA4-4EA4-A2C7-6055646488D6}",
                     MALLOC_CAP_SPIRAM);
    }
    if (err != ESP_OK) return err;

    control_transfer_.buffer = alloc_buffer(Tab5Service::kControlBytes, MALLOC_CAP_DMA);
    if (!control_transfer_.buffer) return ESP_ERR_NO_MEM;
    control_transfer_.length = Tab5Service::kControlBytes;
    control_transfer_.callback = control_done;
    control_transfer_.context = this;

    usb_device::Vendor::Config control;
    control.name = "Tab5 Control";
    control.winusb = true;
    control.device_interface_guid = "{17A6BA36-2C39-40B2-941A-42436642AF2C}";
    control.configured = [this] { control_configured(); };
    control.unconfigured = [this] { service_->disconnected(); };
    control.suspended = [this] { service_->suspended(); };
    control.resumed = [this] { service_->resumed(); };
    control_ = std::make_shared<usb_device::Vendor>(std::move(control));

    service_->attach([this] {
        flush(video_in_);
        flush(audio_in_);
    });
    attached_ = true;

    err = device.add(control_);
    if (err == ESP_OK) err = device.add(video_in_.vendor);
    if (err == ESP_OK) err = device.add(audio_in_.vendor);
    if (err == ESP_OK && keyboard_) err = device.add(keyboard_);
    if (err == ESP_OK && mouse_) err = device.add(mouse_);
    if (err != ESP_OK || !uac_) return err;

    Tab5Audio &audio = service_->audio();
    usb_device::UacSpeaker::Config speaker;
    speaker.name = "Tab5 Speaker";
    speaker.sync = usb_device::UacSpeaker::Sync::Async;
    speaker.formats = {{2, 16}};
    speaker.sample_rates = {Tab5Audio::kUacRate};
    speaker.volume_min = Tab5Audio::kUacVolumeMin;
    speaker.volume_max = Tab5Audio::kUacVolumeMax;
    speaker.volume_resolution = Tab5Audio::kUacVolumeStep;
    speaker.volume = audio.uac_volume().volume;
    speaker.started = [&audio](const usb_device::UacSpeaker::Format &, uint32_t) {
        audio.uac_started();
    };
    speaker.stopped = [&audio] { audio.uac_stopped(); };
    speaker.received = [&audio](const uint8_t *pcm, size_t bytes) {
        audio.uac_received(pcm, bytes);
    };
    speaker.volume_changed = [&audio](bool mute, int16_t volume) {
        audio.set_uac_volume(mute, volume);
    };
    speaker_ = std::make_shared<usb_device::UacSpeaker>(std::move(speaker));
    audio.set_uac_feedback([speaker = speaker_.get()](double rate) {
        speaker->set_feedback(rate);
    });

    return device.add(speaker_);
}

void Tab5Profile::send_report(usb_device::Hid *hid, const char *name, const void *report,
                              size_t bytes) {
    if (!hid) return;
    esp_err_t err = hid->send(report, bytes);
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "%s report: %s", name, esp_err_to_name(err));
    }
}

void Tab5Profile::keyboard_report(const usb_device::KeyboardReport &report) {
    send_report(keyboard_.get(), "keyboard", &report, sizeof(report));
}

void Tab5Profile::mouse_report(const usb_device::MouseReport &report) {
    send_report(mouse_.get(), "mouse", &report, sizeof(report));
}

void Tab5Profile::control_configured() {
    service_->connected(control_->in());
    esp_err_t err = control_->out().submit(&control_transfer_);
    if (err != ESP_OK) ESP_LOGE(TAG, "submit control: %s", esp_err_to_name(err));
}

void Tab5Profile::control_done(usb_device::Transfer *transfer) {
    auto *self = static_cast<Tab5Profile *>(transfer->context);
    if (transfer->status == usb_device::TransferStatus::Canceled) return;
    if (transfer->status == usb_device::TransferStatus::Completed) {
        self->service_->received();
        self->service_->control(transfer->buffer, transfer->actual);
    } else {
        ESP_LOGW(TAG, "control status %d", (int)transfer->status);
    }
    esp_err_t err = self->control_->out().submit(transfer);
    if (err != ESP_OK) ESP_LOGE(TAG, "submit control: %s", esp_err_to_name(err));
}

void Tab5Profile::flush(Receiver &receiver) {
    portENTER_CRITICAL(&lock_);
    for (Slot *slot : receiver.slots) {
        if (slot->state == SlotState::Usb) slot->state = SlotState::Flushing;
    }
    portEXIT_CRITICAL(&lock_);
    receiver.vendor->out().flush();
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
        service_->video().end();
    } else {
        service_->audio().end();
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
    Tab5Profile *owner = receiver.owner;
    if (transfer->status == usb_device::TransferStatus::Canceled) {
        if (slot->state == SlotState::Flushing) {
            owner->submit(slot);
        } else {
            slot->state = SlotState::Idle;
        }
        return;
    }
    if (transfer->status != usb_device::TransferStatus::Completed) {
        ESP_LOGW(TAG, "channel %u status %d", (unsigned)receiver.channel, (int)transfer->status);
        owner->submit(slot);
        return;
    }
    Tab5Service &service = *owner->service_;
    service.received();
    const uint32_t session = service.session();
    if (!session) {
        owner->submit(slot);
        return;
    }
    slot->state = SlotState::Busy;
    if (receiver.channel == tab5::Channel::Video) {
        service.video().submit(transfer->buffer, transfer->actual, session, release, slot);
    } else {
        service.audio().submit(transfer->buffer, transfer->actual, session, release, slot);
    }
}
