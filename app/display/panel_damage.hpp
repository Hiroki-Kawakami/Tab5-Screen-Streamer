/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * Keeps the framebuffers whole for a renderer that only draws what changed.
 */

#pragma once
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
    static constexpr int kMaxRects = 16;

    struct Region {
        bsp_rect_t rects[kMaxRects];
        int count;
    };

    static void add(Region &region, const bsp_rect_t &area);
    static bool subtract(Region &region, const bsp_rect_t &area);
    static esp_err_t fill(Region &region);

    Region stale_[BSP_DISPLAY_MAX_FRAME_BUFFERS] = {};
    bool pending_ = false;
};
