/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
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

#ifdef __cplusplus
}
#endif
