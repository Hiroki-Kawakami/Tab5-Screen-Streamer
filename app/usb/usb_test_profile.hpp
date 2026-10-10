/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Vendor function driven by tools/usb-test-rs: bulk loopback, bulk sink and
 * control echo.
 */

#pragma once
#include "usb_profile.hpp"

class UsbTestProfile : public UsbProfile {
public:
    ~UsbTestProfile() override;
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;
};
