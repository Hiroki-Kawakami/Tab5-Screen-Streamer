/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include "bsp.h"

inline constexpr int kMinDisplayBrightness = 1;

enum class AppMode : uint8_t {
    None,
    PcApp,
};

void settings_init();
void settings_commit();

int settings_display_brightness();
void settings_set_display_brightness(int percent);

bsp_pixel_format_t settings_display_pixel_format();
esp_err_t settings_set_display_pixel_format(bsp_pixel_format_t format);

bool settings_rotation_locked();
void settings_set_rotation_lock(bool locked);
bsp_rotation_t settings_rotation();
void settings_set_rotation(bsp_rotation_t rotation);

bool settings_equalizer_enabled();
void settings_set_equalizer_enabled(bool enabled);

AppMode settings_last_mode();
void settings_set_last_mode(AppMode mode);

bool settings_usb_audio();
void settings_set_usb_audio(bool enabled);

bool settings_usb_hid();
void settings_set_usb_hid(bool enabled);
