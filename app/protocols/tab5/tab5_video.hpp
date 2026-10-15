/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <cstdint>
#include <functional>
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "bsp_types.h"
#include "protocol.hpp"

class Tab5Events;

tab5::PixelFormat tab5_pixel_format(bsp_pixel_format_t format);

/* Runs video endpoint transfers in order on its own task. The panel is taken
 * at the first message and given back by end(). */
class Tab5Video {
public:
    using Done = void (*)(void *context);

    static constexpr int kSwapWindow = 2;
    static constexpr int kDirectAlign = 16;
    static constexpr int kBlitMaxSize = 1280;

    /* `streaming` runs on the video task when the panel is taken and given back.
     * Transfers of a session other than `session` are dropped unread. */
    Tab5Video(Tab5Events &events, const std::atomic<uint32_t> &session,
              std::function<void(bool)> streaming)
        : events_(events), session_(session), streaming_(std::move(streaming)) {}
    ~Tab5Video();
    esp_err_t start();
    uint32_t epoch() const { return epoch_; }
    /* Called with the panel lock held, between transfers. */
    void format_changed(bsp_pixel_format_t format);

    /* `done` runs on the video task once the data is no longer read. */
    void submit(const uint8_t *data, uint32_t length, uint32_t session, Done done,
                void *context);
    void end();
    /* Returns once everything submitted before has been done. */
    void sync();

private:
    enum class Op : uint8_t { Transfer, End, Sync, Quit };

    struct Item {
        Op op;
        const uint8_t *data;
        uint32_t length;
        uint32_t session;
        Done done;
        void *context;
    };

    static void task(void *arg);
    void run(const Item &item);
    void execute_item(const Item &item);
    void execute(const tab5::MsgHeader &header, const uint8_t *payload);
    void draw_direct(const tab5::MsgHeader &header, const uint8_t *payload);
    void draw_blit(const tab5::MsgHeader &header, const uint8_t *payload);
    void fill(const tab5::MsgHeader &header, const uint8_t *payload);
    void swap(const tab5::MsgHeader &header);
    void count(uint32_t bytes);

    Tab5Events &events_;
    const std::atomic<uint32_t> &session_;
    std::function<void(bool)> streaming_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;
    SemaphoreHandle_t synced_ = nullptr;
    std::atomic<uint32_t> epoch_ = 0;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_swaps_ = 0;
    uint32_t stats_jpegs_ = 0;
    int64_t stats_jpeg_us_ = 0;
    int64_t stats_busy_us_ = 0;
};
