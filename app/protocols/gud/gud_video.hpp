/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <cstdint>
#include <functional>
#include "bsp.h"
#include "display/panel_shadow.hpp"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "gud.hpp"

/* Puts the host's buffers on the panel in order on its own task. The panel is
 * taken while the controller is enabled with a mode set. */
class GudVideo {
public:
    using Done = void (*)(void *context);

    /* `streaming` runs on the video task when the panel is taken and given back. */
    explicit GudVideo(std::function<void(bool)> streaming) : streaming_(std::move(streaming)) {}
    ~GudVideo();
    esp_err_t start(bsp_rotation_t rotation, bsp_pixel_format_t format, uint32_t max_buffer_bytes);

    void set_mode(bsp_size_t size);
    void set_enabled(bool enabled);
    void rotate(bsp_rotation_t rotation);
    /* `done` runs on the video task once the data is no longer read. */
    void submit(const gud::SetBuffer &buffer, const uint8_t *data, uint32_t length, Done done,
                void *context);
    void end();

private:
    enum class Op : uint8_t { Mode, Enable, Rotate, Refresh, Buffer, End, Quit };

    struct Item {
        Op op;
        uint32_t value;
        gud::SetBuffer buffer;
        const uint8_t *data;
        uint32_t length;
        Done done;
        void *context;
    };

    static void task(void *arg);
    static void format_changed(bsp_pixel_format_t format, void *arg);
    static void clip_changed(const bsp_rect_t *clip, bool front_changed, void *arg);
    void refresh();
    void send(Op op, uint32_t value = 0);
    void run(const Item &item);
    void apply(const Item &item);
    void update_panel();
    bool fits(bsp_rotation_t rotation) const;
    void count(uint32_t bytes);

    std::function<void(bool)> streaming_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;
    std::atomic<bool> invalidated_ = false;
    uint8_t *scratch_ = nullptr;
    uint32_t scratch_bytes_ = 0;
    PanelShadow shadow_;
    bsp_pixel_format_t format_ = BSP_PIXEL_FORMAT_RGB565;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;
    bsp_rotation_t shown_rotation_ = BSP_ROTATION_0;
    bool has_mode_ = false;
    bool enabled_ = false;
    bool acquired_ = false;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_presents_ = 0;
};
