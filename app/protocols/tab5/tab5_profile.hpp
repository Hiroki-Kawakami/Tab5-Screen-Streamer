/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The USB transport of docs/protocol.md.
 */

#pragma once
#include <memory>
#include "tab5_service.hpp"
#include "usb/usb_profile.hpp"
#include "usb_device_hid.hpp"
#include "usb_device_uac.hpp"
#include "usb_device_vendor.hpp"

class Tab5Profile : public UsbProfile {
public:
    Tab5Profile(std::shared_ptr<Tab5Service> service, bool uac, bool keyboard, bool mouse);
    ~Tab5Profile() override;
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;
    /* From any task; dropped while the device is not enumerated. */
    void keyboard_report(const usb_device::KeyboardReport &report);
    void mouse_report(const usb_device::MouseReport &report);

private:
    struct Slot;

    /* A bulk OUT endpoint whose transfers go to the video or audio task. */
    struct Receiver {
        Tab5Profile *owner;
        tab5::Channel channel;
        uint32_t slot_bytes;
        std::shared_ptr<usb_device::Vendor> vendor;
        Slot *slots[Tab5Service::kQueuedMessages];
        bool active;
    };

    static void control_done(usb_device::Transfer *transfer);
    static void receive_done(usb_device::Transfer *transfer);
    static void release(void *context);
    static void send_report(usb_device::Hid *hid, const char *name, const void *report,
                            size_t bytes);
    esp_err_t create(Receiver &receiver, const char *name, const char *guid, uint32_t caps);
    void destroy(Receiver &receiver);
    void configured(Receiver &receiver);
    void unconfigured(Receiver &receiver);
    void submit(Slot *slot);
    void control_configured();
    void flush(Receiver &receiver);

    std::shared_ptr<Tab5Service> service_;
    std::shared_ptr<usb_device::Vendor> control_;
    std::shared_ptr<usb_device::UacSpeaker> speaker_;
    std::shared_ptr<usb_device::Hid> keyboard_;
    std::shared_ptr<usb_device::Hid> mouse_;
    usb_device::Transfer control_transfer_ = {};
    Receiver video_in_ = {};
    Receiver audio_in_ = {};
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    bool uac_;
    bool attached_ = false;
};
