/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <memory>
#include "esp_err.h"
#include "usb_profile.hpp"

esp_err_t usb_init();
/* Both return at once; the switch runs on the USB task, stopping the current
 * device first. A profile is released there after its device. After
 * usb_stop() the PHY is off. */
void usb_start(std::shared_ptr<UsbProfile> profile);
void usb_stop();
