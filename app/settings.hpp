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
    UsbMonitor,
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

enum class PcAppOption : uint8_t {
    UsbAudio = 1 << 0,
    TouchInput = 1 << 1,
    KeyboardInput = 1 << 2,
};

bool settings_pc_app_option(PcAppOption option);
void settings_set_pc_app_option(PcAppOption option, bool enabled);

enum class UsbMonitorAdapter : uint8_t {
    DisplayLink,
    Gud,
};

UsbMonitorAdapter settings_usb_monitor_adapter();
void settings_set_usb_monitor_adapter(UsbMonitorAdapter adapter);
