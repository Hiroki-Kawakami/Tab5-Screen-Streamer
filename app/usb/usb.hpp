/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "esp_err.h"

enum class UsbProfileId {
    LegacyStream,
    Test,
};

void usb_init();
/* Re-enumerates as the given profile; blocks for the disconnect. */
esp_err_t usb_switch_profile(UsbProfileId id);
