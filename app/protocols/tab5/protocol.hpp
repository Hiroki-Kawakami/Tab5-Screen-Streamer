/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Wire format of docs/protocol.md.
 */

#pragma once
#include <cstdint>
#include <cstring>

namespace tab5 {

constexpr uint16_t kMagic = 0x3554;
constexpr uint16_t kProtocolVersion = 1;

enum class Type : uint8_t {
    GetInfo = 0x01,
    SetVolume = 0x03,
    Info = 0x81,
    SwapDone = 0x82,
    Error = 0x83,
    Orientation = 0x84,
    Format = 0x85,
    Overlay = 0x86,
    Damage = 0x87,
    Volume = 0x8b,
    Audio = 0x8c,
    DrawDirect = 0x10,
    DrawBlit = 0x11,
    Fill = 0x12,
    Swap = 0x13,
    AudioFormat = 0x20,
    AudioData = 0x21,
};

enum class Channel : uint8_t { Control = 0, Video = 1, Audio = 2 };

enum class ErrorCode : uint8_t {
    Malformed = 1,
    TooLarge = 2,
    Unsupported = 3,
    OutOfBounds = 4,
    DecodeFailed = 5,
};

enum class Stream : uint8_t { Uac = 0, Bulk = 1 };
enum class Target : uint8_t { Front = 0, Back = 1 };
enum class Encoding : uint8_t { Jpeg = 0, Raw = 1, Lz4 = 2 };
enum class PixelFormat : uint8_t { Rgb565 = 1, Rgb888 = 2 };

struct __attribute__((packed)) MsgHeader {
    uint16_t magic;
    uint8_t type;
    uint8_t flags;
    uint32_t length;
    uint32_t seq;
    uint32_t epoch;
};
static_assert(sizeof(MsgHeader) == 16);

struct __attribute__((packed)) DrawDirect {
    uint8_t target;
    uint8_t encoding;
    uint16_t y;
    uint16_t height;
};

struct __attribute__((packed)) DrawBlit {
    uint8_t target;
    uint8_t encoding;
    uint8_t src_format;
    uint8_t rotation;
    uint16_t src_w, src_h;
    uint16_t dst_x, dst_y, dst_w, dst_h;
};

struct __attribute__((packed)) Fill {
    uint8_t target;
    uint8_t reserved;
    uint16_t x, y, w, h;
    uint32_t color;
};

struct __attribute__((packed)) ErrorEvent {
    uint8_t channel;
    uint8_t code;
    uint16_t reserved;
};

struct __attribute__((packed)) ByteEvent {
    uint8_t value;
    uint8_t reserved[3];
};

struct __attribute__((packed)) Rect {
    uint16_t x, y, w, h;
};

struct __attribute__((packed)) OverlayEvent {
    uint8_t visible;
    uint8_t reserved;
    Rect stream;
};

struct __attribute__((packed)) SwapDoneEvent {
    uint32_t time_us;
};

struct __attribute__((packed)) Volume {
    uint8_t mute;
    uint8_t reserved;
    int16_t volume;
    int16_t min, max, resolution;
};

struct __attribute__((packed)) VolumeMessage {
    uint8_t stream;
    uint8_t mute;
    int16_t volume;
};

struct __attribute__((packed)) AudioFormat {
    uint32_t sample_rate;
    uint8_t channels;
    uint8_t bits;
    uint16_t reserved;
};

struct __attribute__((packed)) AudioEvent {
    uint32_t time_us;
    uint32_t buffered_frames;
    uint32_t capacity_frames;
};

struct __attribute__((packed)) Info {
    uint16_t protocol_version;
    uint16_t panel_width, panel_height;
    uint8_t pixel_format;
    uint8_t orientation;
    uint8_t framebuffers;
    uint8_t swap_window;
    uint16_t direct_align;
    uint16_t blit_max_width, blit_max_height;
    uint32_t encodings;
    uint32_t max_message[3];
    uint32_t epoch;
    uint8_t overlay_visible;
    uint8_t input_route;
    uint16_t overlay_x, overlay_y, overlay_w, overlay_h;
    uint16_t max_fps;
    Volume volume[2];
    uint32_t audio_max_rate;
};

/* Calls `fn(header, payload)` for each message. Returns false at the first
 * malformed one, with its seq (0 when its header is cut short) in `bad_seq`;
 * the rest of the transfer is not parsed. */
template <typename Fn>
bool for_each_message(const uint8_t *data, uint32_t length, uint32_t *bad_seq, Fn &&fn) {
    uint32_t pos = 0;
    while (pos < length) {
        MsgHeader header;
        if (length - pos < sizeof(header)) {
            *bad_seq = 0;
            return false;
        }
        memcpy(&header, data + pos, sizeof(header));
        pos += sizeof(header);
        if (header.magic != kMagic || header.length > length - pos) {
            *bad_seq = header.seq;
            return false;
        }
        fn(header, data + pos);
        pos += header.length;
    }
    return true;
}

}  // namespace tab5
