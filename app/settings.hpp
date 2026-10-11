/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include "bsp.h"

enum class AppMode : uint8_t {
    None,
    PcApp,
};

void settings_init();
void settings_commit();

bool settings_rotation_locked();
void settings_set_rotation_lock(bool locked);
bsp_rotation_t settings_rotation();
void settings_set_rotation(bsp_rotation_t rotation);

AppMode settings_last_mode();
void settings_set_last_mode(AppMode mode);

bool settings_usb_audio();
void settings_set_usb_audio(bool enabled);
