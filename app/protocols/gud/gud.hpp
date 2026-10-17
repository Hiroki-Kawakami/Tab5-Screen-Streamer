/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Wire format of the Generic USB Display protocol (Linux include/drm/gud.h).
 */

#pragma once
#include <cstdint>

namespace gud {

constexpr uint16_t kVendorId = 0x1d50;
constexpr uint16_t kProductId = 0x614d;
constexpr uint32_t kMagic = 0x1d50614d;
constexpr uint8_t kVersion = 1;

enum class Request : uint8_t {
    GetStatus = 0x00,
    GetDescriptor = 0x01,
    GetFormats = 0x40,
    GetProperties = 0x41,
    GetConnectors = 0x50,
    GetConnectorProperties = 0x51,
    GetConnectorTvModeValues = 0x52,
    SetConnectorForceDetect = 0x53,
    GetConnectorStatus = 0x54,
    GetConnectorModes = 0x55,
    GetConnectorEdid = 0x56,
    SetBuffer = 0x60,
    SetStateCheck = 0x61,
    SetStateCommit = 0x62,
    SetControllerEnable = 0x63,
    SetDisplayEnable = 0x64,
};

enum class Status : uint8_t {
    Ok = 0x00,
    Busy = 0x01,
    RequestNotSupported = 0x02,
    ProtocolError = 0x03,
    InvalidParameter = 0x04,
    Error = 0x05,
};

constexpr uint8_t kCompressionLz4 = 0x01;
constexpr uint8_t kPixelFormatRgb565 = 0x40;
constexpr uint8_t kPixelFormatRgb888 = 0x50;
constexpr uint8_t kConnectorTypePanel = 0;
constexpr uint32_t kConnectorFlagPollStatus = 1 << 0;
constexpr uint8_t kConnectorStatusConnected = 0x01;
constexpr uint8_t kConnectorStatusChanged = 0x80;
constexpr uint32_t kModeFlagPreferred = 1 << 10;
constexpr uint16_t kPropertyBacklightBrightness = 12;

struct __attribute__((packed)) DisplayDescriptor {
    uint32_t magic;
    uint8_t version;
    uint32_t flags;
    uint8_t compression;
    uint32_t max_buffer_size;
    uint32_t min_width;
    uint32_t max_width;
    uint32_t min_height;
    uint32_t max_height;
};

struct __attribute__((packed)) Property {
    uint16_t prop;
    uint64_t val;
};

struct __attribute__((packed)) DisplayMode {
    uint32_t clock;
    uint16_t hdisplay;
    uint16_t hsync_start;
    uint16_t hsync_end;
    uint16_t htotal;
    uint16_t vdisplay;
    uint16_t vsync_start;
    uint16_t vsync_end;
    uint16_t vtotal;
    uint32_t flags;
};

struct __attribute__((packed)) ConnectorDescriptor {
    uint8_t connector_type;
    uint32_t flags;
};

struct __attribute__((packed)) SetBuffer {
    uint32_t x;
    uint32_t y;
    uint32_t width;
    uint32_t height;
    uint32_t length;
    uint8_t compression;
    uint32_t compressed_length;
};

/* Followed by the properties. */
struct __attribute__((packed)) State {
    DisplayMode mode;
    uint8_t format;
    uint8_t connector;
};

static_assert(sizeof(DisplayDescriptor) == 30);
static_assert(sizeof(Property) == 10);
static_assert(sizeof(DisplayMode) == 24);
static_assert(sizeof(ConnectorDescriptor) == 5);
static_assert(sizeof(SetBuffer) == 25);
static_assert(sizeof(State) == 26);

}  // namespace gud
