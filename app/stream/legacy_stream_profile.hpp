/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The band-JPEG stream of streamer-rs on master: a vendor bulk OUT carrying
 * 720x1280 frames as 16 JPEG bands, each behind an 8-byte header.
 */

#pragma once
#include <memory>
#include "freertos/FreeRTOS.h"
#include "usb/usb_profile.hpp"
#include "usb_device_vendor.hpp"

struct StreamFrame;

class LegacyStreamProfile : public UsbProfile {
public:
    ~LegacyStreamProfile() override;
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;

private:
    struct Slot;
    static constexpr int kSlots = 4;

    static void out_done(usb_device::Transfer *transfer);
    static void release(StreamFrame *frame);
    void submit(Slot *slot);
    void configured();
    void count(uint32_t bytes, bool frame, bool resynced);

    std::shared_ptr<usb_device::Vendor> vendor_;
    Slot *slots_[kSlots] = {};
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    bool active_ = false;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_frames_ = 0;
    uint32_t stats_dropped_ = 0;
    uint32_t stats_resyncs_ = 0;
};
