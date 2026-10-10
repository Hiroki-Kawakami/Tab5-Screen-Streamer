/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Decompresses one LZ4 block (block format, no frame). Returns the bytes
 * written, or -1 when the block is malformed or does not fit `dst`. Matches
 * are read back from `dst`. */
ptrdiff_t lz4_block_decompress(const uint8_t *src, size_t src_size, uint8_t *dst,
                               size_t dst_size);

/* One block decompressed piece by piece into separate windows. */
typedef struct {
    const uint8_t *ip;
    const uint8_t *ip_end;
    size_t literals;
    size_t match;
    size_t offset;
    uint8_t nibble;
    bool need_match;
} lz4_stream_t;

void lz4_stream_init(lz4_stream_t *stream, const uint8_t *src, size_t src_size);
/* Fills `dst` and returns `dst_size`, or fewer bytes once the block ends; -1
 * when malformed. `dict` is the output that came right before `dst`, which
 * matches reaching back past `dst` read from. */
ptrdiff_t lz4_stream_decompress(lz4_stream_t *stream, uint8_t *dst, size_t dst_size,
                                const uint8_t *dict, size_t dict_size);
bool lz4_stream_finished(const lz4_stream_t *stream);

#ifdef __cplusplus
}
#endif
