/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <cstdint>
#include <functional>
#include "bsp.h"
#include "display/panel_damage.hpp"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "gud.hpp"

/* Draws the host's buffers into the panel in order on its own task. The panel
 * is taken while the controller is enabled with a mode set. */
class GudVideo {
public:
    using Done = void (*)(void *context);

    /* `streaming` runs on the video task when the panel is taken and given back. */
    explicit GudVideo(std::function<void(bool)> streaming) : streaming_(std::move(streaming)) {}
    ~GudVideo();
    /* The host splits a buffer larger than `max_buffer_bytes` into rows. */
    esp_err_t start(bsp_rotation_t rotation, bsp_pixel_format_t format, uint32_t max_buffer_bytes);

    void set_mode(bsp_size_t size);
    void set_enabled(bool enabled);
    void rotate(bsp_rotation_t rotation);
    /* `done` runs on the video task once the data is no longer read. */
    void submit(const gud::SetBuffer &buffer, const uint8_t *data, uint32_t length, Done done,
                void *context);
    /* Rows [row, row + rows) of an uncompressed buffer; `last` when no more
     * of it follows. */
    void submit_rows(const gud::SetBuffer &buffer, uint32_t row, uint32_t rows, bool last,
                     const uint8_t *data, Done done, void *context);
    void end();
    /* The panel is taken, and its strips are free for the host's pixels. */
    bool drawing() const { return drawing_; }

private:
    enum class Op : uint8_t { Mode, Enable, Rotate, Buffer, End, Quit };

    struct Item {
        Op op;
        uint32_t value;
        gud::SetBuffer buffer;
        uint32_t row;
        uint32_t rows;
        bool last;
        const uint8_t *data;
        uint32_t length;
        Done done;
        void *context;
    };

    static void task(void *arg);
    void send(Op op, uint32_t value = 0);
    void run(const Item &item);
    void draw(const Item &item);
    bool continues(const gud::SetBuffer &buffer) const;
    void present();
    void update_panel();
    void reset_panel();
    bool fits(bsp_rotation_t rotation) const;
    void count(uint32_t bytes);

    std::function<void(bool)> streaming_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;
    PanelDamage damage_;
    bsp_pixel_format_t format_ = BSP_PIXEL_FORMAT_RGB565;
    uint32_t max_buffer_bytes_ = 0;
    bsp_size_t mode_ = {};
    bsp_rotation_t rotation_ = BSP_ROTATION_0;
    bsp_rotation_t shown_rotation_ = BSP_ROTATION_0;
    bool enabled_ = false;
    bool acquired_ = false;
    std::atomic<bool> drawing_ = false;
    bool more_ = false;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_presents_ = 0;
    // DIAG
    uint64_t diag_raw_bytes_ = 0;
    uint32_t diag_rects_ = 0;
    int64_t diag_draw_us_ = 0;
    int64_t diag_present_us_ = 0;
    int64_t diag_busy_us_ = 0;
    int64_t diag_latency_us_ = 0;
    int64_t diag_first_us_ = 0;
};
