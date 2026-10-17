/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstddef>
#include <cstdint>
#include "esp_err.h"

/* The adapter's 16 MiB of graphics memory. Only pages written with something
 * other than zero take memory; the rest read as zero. Addresses wrap. */
class DisplayLinkMemory {
public:
    DisplayLinkMemory() = default;
    ~DisplayLinkMemory();
    DisplayLinkMemory(const DisplayLinkMemory &) = delete;
    DisplayLinkMemory &operator=(const DisplayLinkMemory &) = delete;

    void clear();
    void write8(uint32_t address, uint8_t value);
    /* `address` is even. */
    void write16(uint32_t address, uint16_t value);
    void copy(uint32_t target, uint32_t source, uint32_t bytes);
    void read(uint32_t address, uint8_t *data, uint32_t bytes) const;
    void write(uint32_t address, const uint8_t *data, uint32_t bytes);
    /* Where `bytes` at `address` can be written in place, or null when they
     * cross a page or the page holds no memory yet: writing zeros to it
     * takes none. */
    uint8_t *span(uint32_t address, uint32_t bytes);
    /* Moves the pages holding `bytes` at `address` into one block, so that
     * contiguous() covers them. */
    esp_err_t pin(uint32_t address, uint32_t bytes);
    /* Where `bytes` at `address` lie in one piece, or null. */
    uint8_t *contiguous(uint32_t address, uint32_t bytes) const;
    size_t pages() const;

private:
    static constexpr int kPageShift = 16;
    static constexpr uint32_t kPageBytes = 1 << kPageShift;
    static constexpr int kPages = 256;

    static constexpr int kBlocks = 4;

    struct Block {
        uint8_t *data;
        int first;
        int count;
    };

    uint8_t *page(uint32_t address);
    int block_of(int page) const;

    uint8_t *pages_[kPages] = {};
    Block blocks_[kBlocks] = {};
    bool exhausted_ = false;
};
