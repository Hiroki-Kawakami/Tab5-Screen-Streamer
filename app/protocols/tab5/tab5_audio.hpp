/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <cstdint>
#include <mutex>
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "protocol.hpp"

class Tab5Events;

/* Plays audio endpoint transfers on its own task, paced by the output. */
class Tab5Audio {
public:
    using Done = void (*)(void *context);

    static constexpr uint32_t kMaxRate = 48000;

    explicit Tab5Audio(Tab5Events &events) : events_(events) {}
    ~Tab5Audio();
    /* `capacity_bytes` is what the endpoint can hold queued. */
    esp_err_t start(uint32_t capacity_bytes);

    /* `done` runs on the audio task once the data is no longer read. */
    void submit(const uint8_t *data, uint32_t length, Done done, void *context);
    void end();

    /* From any task. Clamps, applies and announces the bulk stream's volume. */
    void set_volume(bool mute, int16_t volume);
    tab5::Volume volume();

private:
    enum class Op : uint8_t { Transfer, End, Quit };

    struct Item {
        Op op;
        const uint8_t *data;
        uint32_t length;
        Done done;
        void *context;
    };

    static void task(void *arg);
    void run(const Item &item);
    void execute(const tab5::MsgHeader &header, const uint8_t *payload);
    void play(const tab5::MsgHeader &header, const uint8_t *pcm);
    void dequeue(uint32_t bytes);
    void close();
    void report(bool force);
    uint32_t frame_bytes() const;

    Tab5Events &events_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;
    uint32_t capacity_bytes_ = 0;
    std::atomic<uint32_t> queued_bytes_ = 0;
    uint32_t played_ = 0;

    tab5::AudioFormat format_ = {};
    tab5::AudioFormat opened_ = {};
    bool have_format_ = false;
    bool open_ = false;
    int64_t last_report_us_ = 0;

    std::mutex volume_lock_;
    bool mute_ = false;
    int16_t volume_ = 0;
};
