/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The USB side of docs/protocol.md.
 */

#pragma once
#include <memory>
#include "freertos/FreeRTOS.h"
#include "tab5_events.hpp"
#include "tab5_video.hpp"
#include "usb/usb_profile.hpp"
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

    static void control_done(usb_device::Transfer *transfer);
    static void video_done(usb_device::Transfer *transfer);
    static void release(void *context);
    void control_configured();
    void control_unconfigured();
    void video_configured();
    void video_unconfigured();
    void submit(Slot *slot);
    void handle_control(const tab5::MsgHeader &header, const uint8_t *payload);
    void send_info(uint32_t seq);

    Tab5Events events_;
    std::unique_ptr<Tab5Video> video_;
    std::shared_ptr<usb_device::Vendor> control_;
    std::shared_ptr<usb_device::Vendor> video_vendor_;
    usb_device::Transfer control_transfer_ = {};
    Slot *slots_[kSlots] = {};
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    bool video_active_ = false;
};
