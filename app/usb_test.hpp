/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Vendor function driven by tools/usb-test-rs: bulk loopback, bulk sink and
 * control echo.
 */

#pragma once
#include <memory>
#include "usb_device.hpp"

std::shared_ptr<usb_device::Function> usb_test_function();
