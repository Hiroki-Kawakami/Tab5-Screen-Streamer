/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <atomic>
#include <cstdint>
#include <functional>
#include <mutex>
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "audf_fifo.h"
#include "protocol.hpp"

class Tab5Events;

/* Plays audio endpoint transfers on its own task, paced by the output. While
 * the UAC stream runs, it plays that instead and bulk audio is refused. */
class Tab5Audio {
public:
    using Done = void (*)(void *context);

    static constexpr uint32_t kMaxRate = 48000;

    /* Transfers of a session other than `session` are dropped unread. */
    Tab5Audio(Tab5Events &events, const std::atomic<uint32_t> &session)
        : events_(events), session_(session) {}
    ~Tab5Audio();
    /* `capacity_bytes` is what the endpoint can hold queued. */
    esp_err_t start(uint32_t capacity_bytes);

    /* `done` runs on the audio task once the data is no longer read. */
    void submit(const uint8_t *data, uint32_t length, uint32_t session, Done done,
                void *context);
    void end();
    /* Returns once everything submitted before has been done. */
    void sync();

    /* From any task. Clamps, applies and announces the bulk stream's volume. */
    void set_volume(bool mute, int16_t volume);
    tab5::Volume volume();

    /* The UAC stream, from the device task. `feedback` takes the rate the
     * output consumes samples at. */
    static constexpr uint32_t kUacRate = 48000;
    static constexpr int16_t kUacVolumeMin = -40 * 256;
    static constexpr int16_t kUacVolumeMax = 0;
    static constexpr int16_t kUacVolumeStep = 128;
    void set_uac_feedback(std::function<void(double samples_per_second)> feedback);
    void uac_started();
    void uac_stopped();
    void uac_received(const uint8_t *pcm, size_t bytes);
    void set_uac_volume(bool mute, int16_t volume);
    tab5::Volume uac_volume();

private:
    enum class Op : uint8_t { Transfer, End, Sync, Quit, UacStart, UacStop };

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
    void execute(const tab5::MsgHeader &header, const uint8_t *payload);
    void play(const tab5::MsgHeader &header, const uint8_t *pcm);
    void dequeue(uint32_t bytes);
    void close();
    void report(bool force);
    void play_uac();
    void apply_volume();
    void send(Op op);
    uint32_t frame_bytes() const;

    Tab5Events &events_;
    const std::atomic<uint32_t> &session_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;
    SemaphoreHandle_t synced_ = nullptr;
    uint32_t capacity_bytes_ = 0;
    std::atomic<uint32_t> queued_bytes_ = 0;
    uint32_t played_ = 0;

    tab5::AudioFormat format_ = {};
    tab5::AudioFormat opened_ = {};
    bool have_format_ = false;
    bool open_ = false;
    int64_t last_report_us_ = 0;

    audf_fifo_t *uac_pcm_ = nullptr;
    std::function<void(double)> uac_feedback_;
    bool uac_ = false;
    int64_t uac_stats_us_ = 0;
    std::atomic<bool> uac_playing_ = false;

    std::mutex volume_lock_;
    bool mute_ = false;
    int16_t volume_ = 0;
    bool uac_mute_ = false;
    int16_t uac_volume_ = 0;
};
