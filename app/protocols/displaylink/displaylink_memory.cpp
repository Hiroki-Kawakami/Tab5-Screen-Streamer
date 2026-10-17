/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "displaylink_memory.hpp"
#include <algorithm>
#include <cstring>
#include <iterator>
#include "esp_heap_caps.h"
#include "esp_log.h"

static const char *TAG = "displaylink_memory";

static constexpr uint32_t kAddressMask = 0xffffff;

DisplayLinkMemory::~DisplayLinkMemory() {
    clear();
}

void DisplayLinkMemory::clear() {
    for (int i = 0; i < kPages; i++) {
        if (block_of(i) < 0) heap_caps_free(pages_[i]);
        pages_[i] = nullptr;
    }
    for (Block &block : blocks_) {
        heap_caps_free(block.data);
        block = {};
    }
    exhausted_ = false;
}

int DisplayLinkMemory::block_of(int page) const {
    for (int i = 0; i < kBlocks; i++) {
        if (blocks_[i].data && page >= blocks_[i].first &&
            page < blocks_[i].first + blocks_[i].count) {
            return i;
        }
    }
    return -1;
}

esp_err_t DisplayLinkMemory::pin(uint32_t address, uint32_t bytes) {
    address &= kAddressMask;
    const int first = address >> kPageShift;
    const int last = (address + bytes - 1) >> kPageShift;
    if (last >= kPages) return ESP_ERR_INVALID_ARG;
    const int existing = block_of(first);
    if (existing >= 0 && block_of(last) == existing) return ESP_OK;
    for (int page = first; page <= last; page++) {
        if (block_of(page) >= 0) return ESP_ERR_INVALID_STATE;
    }
    int slot = -1;
    for (int i = 0; i < kBlocks; i++) {
        if (!blocks_[i].data) slot = i;
    }
    if (slot < 0) return ESP_ERR_NO_MEM;
    const int count = last + 1 - first;
    auto *data = static_cast<uint8_t *>(
        heap_caps_aligned_alloc(128, count * kPageBytes, MALLOC_CAP_SPIRAM));
    if (!data) {
        ESP_LOGE(TAG, "no memory to pin %u pages", (unsigned)count);
        return ESP_ERR_NO_MEM;
    }
    for (int i = 0; i < count; i++) {
        uint8_t *&page = pages_[first + i];
        uint8_t *slice = data + i * kPageBytes;
        if (page) {
            memcpy(slice, page, kPageBytes);
            heap_caps_free(page);
        } else {
            memset(slice, 0, kPageBytes);
        }
        page = slice;
    }
    blocks_[slot] = {data, first, count};
    return ESP_OK;
}

uint8_t *DisplayLinkMemory::contiguous(uint32_t address, uint32_t bytes) const {
    address &= kAddressMask;
    const int first = address >> kPageShift;
    const int last = (address + bytes - 1) >> kPageShift;
    const int block = block_of(first);
    if (block < 0 || last >= blocks_[block].first + blocks_[block].count) return nullptr;
    return pages_[first] + (address & (kPageBytes - 1));
}

size_t DisplayLinkMemory::pages() const {
    return std::count_if(std::begin(pages_), std::end(pages_), [](uint8_t *p) { return p; });
}

uint8_t *DisplayLinkMemory::page(uint32_t address) {
    uint8_t *&page = pages_[(address & kAddressMask) >> kPageShift];
    if (page || exhausted_) return page;
    page = static_cast<uint8_t *>(heap_caps_calloc(1, kPageBytes, MALLOC_CAP_SPIRAM));
    if (!page) {
        ESP_LOGE(TAG, "no memory past %u pages", (unsigned)pages());
        exhausted_ = true;
    }
    return page;
}

void DisplayLinkMemory::write16(uint32_t address, uint16_t value) {
    address &= kAddressMask;
    uint8_t *p = pages_[address >> kPageShift];
    if (!p) {
        if (!value || !(p = page(address))) return;
    }
    memcpy(p + (address & (kPageBytes - 1)), &value, sizeof(value));
}

void DisplayLinkMemory::copy(uint32_t target, uint32_t source, uint32_t bytes) {
    while (bytes) {
        target &= kAddressMask;
        source &= kAddressMask;
        const uint32_t to = target & (kPageBytes - 1);
        const uint32_t from = source & (kPageBytes - 1);
        const uint32_t n = std::min({bytes, kPageBytes - to, kPageBytes - from});
        const uint8_t *src = pages_[source >> kPageShift];
        uint8_t *dst = pages_[target >> kPageShift];
        if (src) {
            if (!dst) dst = page(target);
            if (dst) memmove(dst + to, src + from, n);
        } else if (dst) {
            memset(dst + to, 0, n);
        }
        target += n;
        source += n;
        bytes -= n;
    }
}

void DisplayLinkMemory::read(uint32_t address, uint8_t *data, uint32_t bytes) const {
    while (bytes) {
        address &= kAddressMask;
        const uint32_t offset = address & (kPageBytes - 1);
        const uint32_t n = std::min(bytes, kPageBytes - offset);
        const uint8_t *src = pages_[address >> kPageShift];
        if (src) {
            memcpy(data, src + offset, n);
        } else {
            memset(data, 0, n);
        }
        address += n;
        data += n;
        bytes -= n;
    }
}

void DisplayLinkMemory::write(uint32_t address, const uint8_t *data, uint32_t bytes) {
    while (bytes) {
        address &= kAddressMask;
        const uint32_t offset = address & (kPageBytes - 1);
        const uint32_t n = std::min(bytes, kPageBytes - offset);
        uint8_t *dst = page(address);
        if (dst) memcpy(dst + offset, data, n);
        address += n;
        data += n;
        bytes -= n;
    }
}

uint8_t *DisplayLinkMemory::span(uint32_t address, uint32_t bytes) {
    address &= kAddressMask;
    const uint32_t offset = address & (kPageBytes - 1);
    uint8_t *p = pages_[address >> kPageShift];
    return p && offset + bytes <= kPageBytes ? p + offset : nullptr;
}
