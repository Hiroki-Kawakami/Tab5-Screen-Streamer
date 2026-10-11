/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The USB side of docs/protocol.md.
 */

#pragma once
#include <atomic>
#include <memory>
#include "bsp.h"
#include "freertos/FreeRTOS.h"
#include "tab5_audio.hpp"
#include "tab5_events.hpp"
#include "tab5_video.hpp"
#include "usb/usb_profile.hpp"
#include "usb_device_uac.hpp"
#include "usb_device_vendor.hpp"

class Tab5Profile : public UsbProfile {
public:
    Tab5Profile();
    ~Tab5Profile() override;
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;

private:
    struct Slot;
    static constexpr int kSlots = 4;

    /* A bulk OUT endpoint whose transfers go to the video or audio task. */
    struct Receiver {
        Tab5Profile *owner;
        tab5::Channel channel;
        uint32_t slot_bytes;
        std::shared_ptr<usb_device::Vendor> vendor;
        Slot *slots[kSlots];
        bool active;
    };

    static void control_done(usb_device::Transfer *transfer);
    static void receive_done(usb_device::Transfer *transfer);
    static void release(void *context);
    esp_err_t create(Receiver &receiver, const char *name, const char *guid, uint32_t caps);
    void destroy(Receiver &receiver);
    void configured(Receiver &receiver);
    void unconfigured(Receiver &receiver);
    void submit(Slot *slot);
    void control_configured();
    void control_unconfigured();
    void handle_control(const tab5::MsgHeader &header, const uint8_t *payload);
    void send_info(uint32_t seq);
    static void orientation_changed(bsp_rotation_t rotation, void *arg);
    static void format_changed(bsp_pixel_format_t format, void *arg);

    Tab5Events events_;
    std::unique_ptr<Tab5Video> video_;
    std::unique_ptr<Tab5Audio> audio_;
    std::shared_ptr<usb_device::Vendor> control_;
    std::shared_ptr<usb_device::UacSpeaker> speaker_;
    usb_device::Transfer control_transfer_ = {};
    Receiver video_in_ = {};
    Receiver audio_in_ = {};
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    std::atomic<uint8_t> orientation_ = 0;
};
