/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include <functional>
#include "usb_device.hpp"

/* Active: the host is sending what the profile shows. Reported from the USB
 * tasks. */
enum class UsbStatus : uint8_t { Unconfigured, Configured, Active };
using UsbStatusListener = std::function<void(UsbStatus)>;

/* One way of appearing on USB: the device identity and its functions.
 * Destroyed after the Device it was added to. */
class UsbProfile {
public:
    virtual ~UsbProfile() = default;
    virtual usb_device::DeviceInfo info() const = 0;
    virtual esp_err_t add_functions(usb_device::Device &device) = 0;
};
