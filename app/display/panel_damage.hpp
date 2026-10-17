/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Keeps the framebuffers whole for a renderer that only draws what changed.
 */

#pragma once
#include <cstdint>
#include "bsp.h"
#include "esp_err.h"

/* Called holding the panel lock. Areas are on the panel. */
class PanelDamage {
public:
    /* Every framebuffer shows the same. */
    void reset();
    /* Every framebuffer but the front lacks everything. */
    void invalidate();
    /* Before drawing `area` into the back buffer. */
    esp_err_t drawing(const bsp_rect_t &area);
    /* After drawing `area` into the back buffer. */
    void drawn(const bsp_rect_t &area);
    bool pending() const { return pending_; }
    /* Fills in what the back buffer lacks from the front and swaps. */
    esp_err_t present();

private:
    static constexpr int kMaxCellRows = 64;

    /* Per framebuffer, a bit for each cell it lacks; a row of cells per word. */
    using Cells = uint32_t[kMaxCellRows];

    static esp_err_t fill(const uint32_t *rows, int first, int last);

    Cells stale_[BSP_DISPLAY_MAX_FRAME_BUFFERS] = {};
    bool pending_ = false;
};
