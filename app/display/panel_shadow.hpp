/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * A host's picture kept at its own size, put on the panel by the rows that
 * changed.
 */

#pragma once
#include <cstddef>
#include <cstdint>
#include "bsp.h"
#include "esp_err.h"

class PanelShadow {
public:
    PanelShadow() = default;
    ~PanelShadow();
    PanelShadow(const PanelShadow &) = delete;
    PanelShadow &operator=(const PanelShadow &) = delete;

    /* Black afterwards. */
    esp_err_t resize(bsp_size_t size, bsp_pixel_format_t format);
    bsp_size_t size() const { return size_; }
    size_t stride() const { return stride_; }
    uint8_t *row(int y) { return pixels_ + y * stride_; }

    void damage(int y, int height);
    void invalidate();
    bool dirty() const { return pending_.start < pending_.end; }
    /* Called holding the panel lock, with the shadow the panel's size upright
     * at `rotation`. Draws what the back buffer lacks and swaps. */
    esp_err_t present(bsp_rotation_t rotation);

private:
    struct Rows {
        int start;
        int end;
    };

    uint8_t *pixels_ = nullptr;
    bsp_size_t size_ = {};
    bsp_pixel_format_t format_ = BSP_PIXEL_FORMAT_RGB565;
    size_t stride_ = 0;
    Rows pending_ = {};
    Rows history_[BSP_DISPLAY_MAX_FRAME_BUFFERS] = {};
};
