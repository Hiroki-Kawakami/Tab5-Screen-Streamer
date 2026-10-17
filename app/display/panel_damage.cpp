/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel_damage.hpp"
#include <algorithm>
#include <cstring>
#include "panel.hpp"

static constexpr int kCell = 32;

static int cell_rows() {
    return (panel_size().height + kCell - 1) / kCell;
}

/* Bits of the cells touching columns [x0, x1). */
static uint32_t touched_bits(int x0, int x1) {
    uint32_t bits = 0;
    for (int cell = x0 / kCell; cell * kCell < x1; cell++) bits |= 1u << cell;
    return bits;
}

/* Bits of the cells within columns [x0, x1). */
static uint32_t covered_bits(int x0, int x1) {
    const int width = panel_size().width;
    uint32_t bits = 0;
    for (int cell = x0 / kCell; cell * kCell < x1; cell++) {
        if (cell * kCell >= x0 && std::min((cell + 1) * kCell, width) <= x1) bits |= 1u << cell;
    }
    return bits;
}

void PanelDamage::reset() {
    memset(stale_, 0, sizeof(stale_));
    pending_ = false;
}

void PanelDamage::invalidate() {
    const int front = panel_buffer_index(PanelBuffer::Front);
    const uint32_t all = touched_bits(0, panel_size().width);
    for (int i = 0; i < panel_framebuffers(); i++) {
        for (int row = 0; row < cell_rows(); row++) stale_[i][row] = i == front ? 0 : all;
    }
}

// Runs of lacking cells in each cell row, copied from the front.
esp_err_t PanelDamage::fill(const uint32_t *rows, int first, int last) {
    esp_err_t err = ESP_OK;
    for (int row = first; row <= last && err == ESP_OK; row++) {
        uint32_t bits = rows[row];
        while (bits && err == ESP_OK) {
            const int start = __builtin_ctz(bits);
            const uint32_t rest = ~(bits >> start);
            const int length = rest ? __builtin_ctz(rest) : 32 - start;
            err = panel_copy_to_back({{start * kCell, row * kCell}, {length * kCell, kCell}});
            const uint32_t run = length >= 32 ? UINT32_MAX : (1u << length) - 1;
            bits &= ~(run << start);
        }
    }
    return err;
}

// Cells the area covers whole are drawn over; the others it touches are
// filled in first, so filling them in later cannot cover what is drawn.
esp_err_t PanelDamage::drawing(const bsp_rect_t &area) {
    uint32_t *stale = stale_[panel_buffer_index(PanelBuffer::Back)];
    const int x0 = area.origin.x, x1 = area.origin.x + area.size.width;
    const int y0 = area.origin.y, y1 = area.origin.y + area.size.height;
    if (x1 <= x0 || y1 <= y0) return ESP_OK;
    const int height = panel_size().height;
    const uint32_t touched = touched_bits(x0, x1);
    const uint32_t covered = covered_bits(x0, x1);
    uint32_t partial[kMaxCellRows] = {};
    const int first = y0 / kCell, last = (y1 - 1) / kCell;
    for (int row = first; row <= last; row++) {
        const bool whole_row = row * kCell >= y0 && std::min((row + 1) * kCell, height) <= y1;
        partial[row] = stale[row] & touched & ~(whole_row ? covered : 0);
        stale[row] &= ~touched;
    }
    return fill(partial, first, last);
}

void PanelDamage::drawn(const bsp_rect_t &area) {
    const int x1 = area.origin.x + area.size.width;
    const int y1 = area.origin.y + area.size.height;
    if (x1 <= area.origin.x || y1 <= area.origin.y) return;
    const uint32_t touched = touched_bits(area.origin.x, x1);
    const int back = panel_buffer_index(PanelBuffer::Back);
    for (int i = 0; i < panel_framebuffers(); i++) {
        if (i == back) continue;
        for (int row = area.origin.y / kCell; row <= (y1 - 1) / kCell; row++) {
            stale_[i][row] |= touched;
        }
    }
    pending_ = true;
}

esp_err_t PanelDamage::present() {
    uint32_t *stale = stale_[panel_buffer_index(PanelBuffer::Back)];
    const esp_err_t err = fill(stale, 0, cell_rows() - 1);
    memset(stale, 0, sizeof(Cells));
    panel_swap();
    pending_ = false;
    return err;
}
