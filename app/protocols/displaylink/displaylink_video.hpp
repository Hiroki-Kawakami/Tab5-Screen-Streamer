/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include <functional>
#include "bsp.h"
#include "display/panel_damage.hpp"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"

/* Runs the host's command stream on its own task against an emulated
 * framebuffer, and puts what it changes on the panel, which is taken while a
 * mode is set. */
class DisplayLinkVideo {
public:
    using Done = void (*)(void *context);

    /* `streaming` runs on the video task when the panel is taken and given
     * back, `display_power` when the host blanks or unblanks. */
    DisplayLinkVideo(std::function<void(bool)> streaming, std::function<void(bool)> display_power)
        : streaming_(std::move(streaming)), display_power_(std::move(display_power)) {}
    ~DisplayLinkVideo();
    esp_err_t start(bsp_rotation_t rotation);

    void rotate(bsp_rotation_t rotation);
    /* The next bytes of the stream. `done` runs on the video task once they
     * are no longer read. */
    void submit(const uint8_t *data, uint32_t length, Done done, void *context);
    /* The stream starts over. */
    void end();

private:
    enum class Op : uint8_t { Data, Rotate, End, Quit };

    struct Item {
        Op op;
        uint32_t value;
        const uint8_t *data;
        uint32_t length;
        Done done;
        void *context;
    };

    enum class State : uint8_t {
        Idle,
        Command,
        Header,
        RawPixels,
        RunCount,
        RunPixel,
        SpanCount,
        SpanPixels,
        RepeatCount,
    };

    static void task(void *arg);
    void send(Op op, uint32_t value = 0);
    void run(const Item &item);
    void parse(const uint8_t *data, uint32_t length);
    void begin_command();
    bool pixel_byte(uint8_t byte);
    void put(uint16_t pixel);
    void copy();
    void write_register(uint8_t reg, uint8_t value);
    void apply_registers();
    void set_mode(bsp_size_t size);
    void present();
    void update_panel();
    bool fits(bsp_rotation_t rotation) const;
    void reset_stream();

    std::function<void(bool)> streaming_;
    std::function<void(bool)> display_power_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;

    State state_ = State::Idle;
    uint8_t command_ = 0;
    uint8_t header_[7] = {};
    uint8_t header_bytes_ = 0;
    uint8_t header_length_ = 0;
    uint8_t pixel_bytes_ = 2;
    uint8_t pixel_high_ = 0;
    bool pixel_half_ = false;
    uint16_t pixel_ = 0;
    uint32_t address_ = 0;
    uint32_t total_ = 0;
    uint32_t written_ = 0;
    uint32_t count_ = 0;
    uint8_t registers_[256] = {};
    bool locked_ = false;
    bool blank_ = true;

    uint16_t *vram_ = nullptr;
    bsp_size_t mode_ = {};
    uint32_t base16_ = 0;
    uint32_t dirty_first_ = UINT32_MAX;
    uint32_t dirty_last_ = 0;
    int64_t dirty_since_us_ = 0;

    PanelDamage damage_;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;
    bsp_rotation_t shown_rotation_ = BSP_ROTATION_0;
    bool acquired_ = false;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_presents_ = 0;
    int64_t stats_busy_us_ = 0;
};
