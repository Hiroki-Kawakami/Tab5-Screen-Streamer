/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "esp_err.h"

enum class UsbProfileId {
    Tab5,
    Test,
};

esp_err_t usb_init();
/* Both return at once; the switch runs on the USB task, stopping the current
 * device first. After usb_stop() the PHY is off. */
void usb_start(UsbProfileId id);
void usb_stop();
