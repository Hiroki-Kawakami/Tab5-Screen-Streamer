/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * A DisplayLink DL-1x5 adapter with a monitor showing the panel upright at
 * the UI rotation.
 */

#pragma once
#include <functional>
#include <memory>
#include "displaylink_video.hpp"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "usb/usb_profile.hpp"
#include "usb_device_vendor.hpp"

class DisplayLinkProfile : public UsbProfile {
public:
    /* Run from the USB tasks. */
    struct Listeners {
        UsbStatusListener status;
        std::function<void(bool on)> display_power;
    };

    /* The monitor reports only the panel's size at `rotation`; a turn that
     * swaps width and height takes a new profile. */
    DisplayLinkProfile(Listeners listeners, bsp_rotation_t rotation);
    ~DisplayLinkProfile() override;
    /* Before the profile is started on USB. */
    esp_err_t start();
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;
    /* From the LVGL task, for turns that keep width and height. */
    void rotate(bsp_rotation_t rotation);

private:
    static constexpr int kSlots = 8;

    enum class SlotState : uint8_t { Idle, Usb, Busy };

    struct Slot {
        usb_device::Transfer transfer;
        DisplayLinkProfile *owner;
        SlotState state;
    };

    bool control(const usb_device::ControlRequest &request, uint8_t *data, size_t *bytes);
    void configured();
    void unconfigured();
    void streaming(bool streaming);
    void report();
    void submit(Slot *slot);
    static void receive_done(usb_device::Transfer *transfer);
    static void release(void *context);
    static void idle(TimerHandle_t timer);

    Listeners listeners_;
    bsp_rotation_t rotation_;
    uint8_t edid_[128] = {};
    uint8_t *memory_ = nullptr;
    std::unique_ptr<DisplayLinkVideo> video_;
    std::shared_ptr<usb_device::Vendor> vendor_;
    Slot *slots_[kSlots] = {};
    TimerHandle_t idle_timer_ = nullptr;
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    bool active_ = false;
    bool configured_ = false;
    bool streaming_ = false;
    UsbStatus reported_ = UsbStatus::Unconfigured;
};
