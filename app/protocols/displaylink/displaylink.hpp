/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Wire format of DisplayLink DL-1x0/1x5 adapters (Linux drivers/gpu/drm/udl).
 */

#pragma once
#include <cstdint>

namespace displaylink {

constexpr uint16_t kVendorId = 0x17e9;
constexpr uint16_t kProductId = 0x0198;

enum class Request : uint8_t {
    ReadEdid = 0x02,
    SelectChannel = 0x12,
};

constexpr uint16_t kEdidIndex = 0xa1;

constexpr uint8_t kCommandStart = 0xaf;

enum class Command : uint8_t {
    WriteRegister = 0x20,
    WriteRaw8 = 0x60,
    WriteRl8 = 0x61,
    WriteCopy8 = 0x62,
    WriteRlx8 = 0x63,
    Sync = 0xa0,
    WriteRaw16 = 0x68,
    WriteRl16 = 0x69,
    WriteCopy16 = 0x6a,
    WriteRlx16 = 0x6b,
};

enum Register : uint8_t {
    kColorDepth = 0x00,
    kHPixels = 0x0f,
    kVPixels = 0x17,
    kBlankMode = 0x1f,
    kBase16 = 0x20,
    kBase8 = 0x26,
    kVideoLock = 0xff,
};

constexpr uint8_t kBlankModeOn = 0x00;
constexpr uint8_t kVideoUnlocked = 0xff;

}  // namespace displaylink
