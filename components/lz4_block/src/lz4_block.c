/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "lz4_block.h"
#include <string.h>

#define MIN_MATCH 4

static int read_length(const uint8_t **ip, const uint8_t *end, size_t *length) {
    uint8_t byte;
    do {
        if (*ip >= end) return -1;
        byte = *(*ip)++;
        *length += byte;
    } while (byte == 255);
    return 0;
}

ptrdiff_t lz4_block_decompress(const uint8_t *src, size_t src_size, uint8_t *dst,
                               size_t dst_size) {
    const uint8_t *ip = src;
    const uint8_t *const ip_end = src + src_size;
    uint8_t *op = dst;
    uint8_t *const op_end = dst + dst_size;

    while (ip < ip_end) {
        const uint8_t token = *ip++;

        size_t literals = token >> 4;
        if (literals == 15 && read_length(&ip, ip_end, &literals)) return -1;
        if (literals > (size_t)(ip_end - ip) || literals > (size_t)(op_end - op)) return -1;
        memcpy(op, ip, literals);
        ip += literals;
        op += literals;
        if (ip == ip_end) break;

        if (ip_end - ip < 2) return -1;
        const size_t offset = ip[0] | ip[1] << 8;
        ip += 2;
        if (offset == 0 || offset > (size_t)(op - dst)) return -1;

        size_t length = token & 15;
        if (length == 15 && read_length(&ip, ip_end, &length)) return -1;
        length += MIN_MATCH;
        if (length > (size_t)(op_end - op)) return -1;

        // Copies grow with the distance to the match, so a short offset
        // repeats its pattern in doubling chunks instead of byte by byte.
        const uint8_t *match = op - offset;
        while (length) {
            size_t chunk = (size_t)(op - match);
            if (chunk > length) chunk = length;
            memcpy(op, match, chunk);
            op += chunk;
            length -= chunk;
        }
    }
    return op - dst;
}
