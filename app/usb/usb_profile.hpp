/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "usb_device.hpp"

/* One way of appearing on USB: the device identity, its functions and what
 * consumes their data. Destroyed after the Device it was added to. */
class UsbProfile {
public:
    virtual ~UsbProfile() = default;
    virtual usb_device::DeviceInfo info() const = 0;
    virtual esp_err_t add_functions(usb_device::Device &device) = 0;
};
