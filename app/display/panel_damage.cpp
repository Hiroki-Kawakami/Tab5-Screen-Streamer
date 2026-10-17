/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel_damage.hpp"
#include <algorithm>
#include "panel.hpp"

static int right(const bsp_rect_t &r) {
    return r.origin.x + r.size.width;
}

static int bottom(const bsp_rect_t &r) {
    return r.origin.y + r.size.height;
}

static bool contains(const bsp_rect_t &outer, const bsp_rect_t &inner) {
    return inner.origin.x >= outer.origin.x && inner.origin.y >= outer.origin.y &&
           right(inner) <= right(outer) && bottom(inner) <= bottom(outer);
}

static bool overlaps(const bsp_rect_t &a, const bsp_rect_t &b) {
    return a.origin.x < right(b) && b.origin.x < right(a) && a.origin.y < bottom(b) &&
           b.origin.y < bottom(a);
}

static bsp_rect_t bounds(const bsp_rect_t &a, const bsp_rect_t &b) {
    const int x0 = std::min(a.origin.x, b.origin.x);
    const int y0 = std::min(a.origin.y, b.origin.y);
    return {{x0, y0}, {std::max(right(a), right(b)) - x0, std::max(bottom(a), bottom(b)) - y0}};
}

void PanelDamage::reset() {
    for (Region &region : stale_) region.count = 0;
    pending_ = false;
}

void PanelDamage::invalidate() {
    const int front = panel_buffer_index(PanelBuffer::Front);
    for (int i = 0; i < panel_framebuffers(); i++) {
        stale_[i].count = 0;
        if (i != front) add(stale_[i], {{0, 0}, panel_size()});
    }
}

// Lacking too much only costs a copy, so a full region merges into one rect.
void PanelDamage::add(Region &region, const bsp_rect_t &area) {
    for (int i = 0; i < region.count; i++) {
        if (contains(region.rects[i], area)) return;
    }
    if (region.count < kMaxRects) {
        region.rects[region.count++] = area;
        return;
    }
    bsp_rect_t all = area;
    for (int i = 0; i < region.count; i++) all = bounds(all, region.rects[i]);
    region.rects[0] = all;
    region.count = 1;
}

// False when the pieces left do not fit; the region is unchanged then.
bool PanelDamage::subtract(Region &region, const bsp_rect_t &area) {
    Region result = {};
    for (int i = 0; i < region.count; i++) {
        const bsp_rect_t &r = region.rects[i];
        if (!overlaps(r, area)) {
            if (result.count == kMaxRects) return false;
            result.rects[result.count++] = r;
            continue;
        }
        const int top = std::max(r.origin.y, area.origin.y);
        const int end = std::min(bottom(r), bottom(area));
        const bsp_rect_t pieces[] = {
            {{r.origin.x, r.origin.y}, {r.size.width, top - r.origin.y}},
            {{r.origin.x, end}, {r.size.width, bottom(r) - end}},
            {{r.origin.x, top}, {std::max(area.origin.x, r.origin.x) - r.origin.x, end - top}},
            {{right(area), top}, {right(r) - right(area), end - top}},
        };
        for (const bsp_rect_t &piece : pieces) {
            if (piece.size.width <= 0 || piece.size.height <= 0) continue;
            if (result.count == kMaxRects) return false;
            result.rects[result.count++] = piece;
        }
    }
    region = result;
    return true;
}

esp_err_t PanelDamage::fill(Region &region) {
    esp_err_t err = ESP_OK;
    for (int i = 0; i < region.count && err == ESP_OK; i++) {
        err = panel_copy_to_back(region.rects[i]);
    }
    region.count = 0;
    return err;
}

// What the back buffer lacks is filled in before anything is drawn over it
// when the region would no longer fit around the new area.
esp_err_t PanelDamage::drawing(const bsp_rect_t &area) {
    Region &stale = stale_[panel_buffer_index(PanelBuffer::Back)];
    if (subtract(stale, area)) return ESP_OK;
    const esp_err_t err = fill(stale);
    return err;
}

void PanelDamage::drawn(const bsp_rect_t &area) {
    const int back = panel_buffer_index(PanelBuffer::Back);
    for (int i = 0; i < panel_framebuffers(); i++) {
        if (i != back) add(stale_[i], area);
    }
    pending_ = true;
}

esp_err_t PanelDamage::present() {
    const esp_err_t err = fill(stale_[panel_buffer_index(PanelBuffer::Back)]);
    panel_swap();
    pending_ = false;
    return err;
}
