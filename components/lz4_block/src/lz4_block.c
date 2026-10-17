/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "lz4_block.h"
#include <string.h>

#define MIN_MATCH 4
#define FAST_INPUT 18
#define FAST_OUTPUT 40

static size_t min_size(size_t a, size_t b) {
    return a < b ? a : b;
}

static inline void copy8(uint8_t *dst, const uint8_t *src) {
    memcpy(dst, src, 8);
}

static inline void copy16(uint8_t *dst, const uint8_t *src) {
    memcpy(dst, src, 16);
}

/* Whole sequences of at most 14 literals and 18 match bytes, copied in fixed
 * chunks that may write past the sequence: the room left in the window and
 * the input covers them. Stops at anything else, leaving it to the general
 * loop. */
static uint8_t *decode_short(lz4_stream_t *s, uint8_t *dst, uint8_t *op, uint8_t *op_end) {
    const uint8_t *ip = s->ip;
    while (s->ip_end - ip >= FAST_INPUT && op_end - op >= FAST_OUTPUT) {
        const uint8_t token = *ip;
        const size_t literals = token >> 4;
        if (literals == 15 || (token & 15) == 15) break;
        copy16(op, ip + 1);
        const uint8_t *offset_bytes = ip + 1 + literals;
        const size_t offset = offset_bytes[0] | offset_bytes[1] << 8;
        uint8_t *const match_op = op + literals;
        if (offset < 8 || offset > (size_t)(match_op - dst)) break;
        const uint8_t *match = match_op - offset;
        copy8(match_op, match);
        copy8(match_op + 8, match + 8);
        copy8(match_op + 16, match + 16);
        ip = offset_bytes + 2;
        op = match_op + (token & 15) + MIN_MATCH;
    }
    s->ip = ip;
    return op;
}

static int read_length(lz4_stream_t *s, size_t *length) {
    uint8_t byte;
    do {
        if (s->ip >= s->ip_end) return -1;
        byte = *s->ip++;
        *length += byte;
    } while (byte == 255);
    return 0;
}

void lz4_stream_init(lz4_stream_t *s, const uint8_t *src, size_t src_size) {
    memset(s, 0, sizeof(*s));
    s->ip = src;
    s->ip_end = src + src_size;
}

bool lz4_stream_finished(const lz4_stream_t *s) {
    return s->ip == s->ip_end && !s->literals && !s->match;
}

ptrdiff_t lz4_stream_decompress(lz4_stream_t *s, uint8_t *dst, size_t dst_size,
                                const uint8_t *dict, size_t dict_size) {
    uint8_t *op = dst;
    uint8_t *const op_end = dst + dst_size;

    while (true) {
        if (s->literals) {
            const size_t n = min_size(s->literals, (size_t)(op_end - op));
            memcpy(op, s->ip, n);
            s->ip += n;
            op += n;
            s->literals -= n;
            if (s->literals) break;
        }
        if (s->need_match) {
            if (s->ip == s->ip_end) break;
            if (s->ip_end - s->ip < 2) return -1;
            s->offset = s->ip[0] | s->ip[1] << 8;
            s->ip += 2;
            size_t length = s->nibble;
            if (length == 15 && read_length(s, &length)) return -1;
            s->match = length + MIN_MATCH;
            s->need_match = false;
            if (s->offset == 0) return -1;
        }
        while (s->match && op < op_end) {
            const size_t written = op - dst;
            if (s->offset > written) {
                const size_t back = s->offset - written;
                if (back > dict_size) return -1;
                const size_t n = min_size(min_size(back, s->match), (size_t)(op_end - op));
                memcpy(op, dict + dict_size - back, n);
                op += n;
                s->match -= n;
                continue;
            }
            // The source stays put while the output moves on, so a short
            // offset repeats its pattern in doubling chunks instead of byte
            // by byte.
            const uint8_t *match = op - s->offset;
            while (s->match && op < op_end) {
                const size_t n = min_size(min_size((size_t)(op - match), s->match),
                                          (size_t)(op_end - op));
                memcpy(op, match, n);
                op += n;
                s->match -= n;
            }
        }
        if (s->match || op == op_end || s->ip == s->ip_end) break;
        op = decode_short(s, dst, op, op_end);
        if (op == op_end || s->ip == s->ip_end) break;

        const uint8_t token = *s->ip++;
        size_t literals = token >> 4;
        if (literals == 15 && read_length(s, &literals)) return -1;
        if (literals > (size_t)(s->ip_end - s->ip)) return -1;
        s->literals = literals;
        s->nibble = token & 15;
        s->need_match = true;
    }
    return op - dst;
}

ptrdiff_t lz4_block_decompress(const uint8_t *src, size_t src_size, uint8_t *dst,
                               size_t dst_size) {
    lz4_stream_t stream;
    lz4_stream_init(&stream, src, src_size);
    const ptrdiff_t written = lz4_stream_decompress(&stream, dst, dst_size, NULL, 0);
    if (written < 0 || !lz4_stream_finished(&stream)) return -1;
    return written;
}
