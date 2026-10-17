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
    WriteMemory = 0x03,
    ReadMemory = 0x04,
    ReadStatus = 0x06,
    SetKey = 0x12,
    EndTransfer = 0x14,
};

/* Status bits of an ASIC booted standalone, without protocol errors. */
constexpr uint32_t kStatus = 0x3u << 30 | 0x3u << 28 | 0x5u << 12 | 0x80;
constexpr uint32_t kMemoryBytes = 65536;
constexpr uint8_t kNullKey[16] = {0x57, 0xcd, 0xdc, 0xa7, 0x1c, 0x88, 0x5e, 0x15,
                                  0x60, 0xfe, 0xc6, 0x97, 0x16, 0x3d, 0x47, 0xf2};

/* The low byte of wIndex; the high byte is set when several bytes are read. */
constexpr uint8_t kEdidIndex = 0xa1;

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
    WriteCompressed8 = 0x70,
    WriteCompressed16 = 0x78,
    LoadTable = 0xe0,
};

constexpr uint32_t kTableMagic = 0xcd713826;
constexpr uint32_t kTableEntryBytes = 9;
constexpr uint16_t kMaxTableStates = 4096;
constexpr uint16_t kTableStart8 = 0;
constexpr uint16_t kTableStart16 = 8;

enum Register : uint8_t {
    kColorDepth = 0x00,
    kHPixels = 0x0f,
    kVPixels = 0x17,
    kBlankMode = 0x1f,
    kBase16 = 0x20,
    kStride16 = 0x23,
    kBase8 = 0x26,
    kVideoLock = 0xff,
};


constexpr uint8_t kBlankModeOn = 0x00;
constexpr uint8_t kVideoUnlocked = 0xff;

}  // namespace displaylink
