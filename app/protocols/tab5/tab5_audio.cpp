/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_audio.hpp"
#include <algorithm>
#include <cmath>
#include "bsp.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"
#include "tab5_events.hpp"

static const char *TAG = "tab5_audio";

static constexpr int kTaskPriority = 7;
static constexpr int kTaskCore = 1;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 8;
static constexpr TickType_t kIdleClose = pdMS_TO_TICKS(200);
static constexpr int64_t kReportPeriodUs = 20 * 1000;
static constexpr uint32_t kChunksPerSecond = 100;

static constexpr int16_t kVolumeMin = -40 * 256;
static constexpr int16_t kVolumeMax = 6 * 256;
static constexpr int16_t kVolumeStep = 103;
static constexpr int16_t kVolumeDefault = -10 * 256;

static constexpr uint8_t kUacChannels = 2;
static constexpr uint8_t kUacBits = 16;
static constexpr uint32_t kUacFrameBytes = kUacChannels * kUacBits / 8;
static constexpr uint32_t kUacBufferFrames = Tab5Audio::kUacRate / 20;
static constexpr uint32_t kUacTargetFrames = kUacBufferFrames / 2;
static constexpr uint32_t kUacChunkFrames = Tab5Audio::kUacRate / 250;
// Feedback leans this far off the nominal rate at an empty or full buffer.
static constexpr double kUacFeedbackGain = 0.002;
static constexpr int64_t kUacStatsPeriodUs = 5 * 1000 * 1000;

using tab5::Channel;
using tab5::ErrorCode;

/* The BSP volume is linear in dB: 1 is -40 dB, 100 is 0 dB, 150 is +6 dB. */
static int bsp_volume(int16_t volume) {
    const float db = volume / 256.0f;
    if (db <= 0) return std::clamp((int)lroundf(100 + db * 99 / 40), 1, 100);
    return std::clamp((int)lroundf(100 + db * 50 / 6), 100, 150);
}

Tab5Audio::~Tab5Audio() {
    if (!queue_) return;
    send(Op::Quit);
    xSemaphoreTake(exited_, portMAX_DELAY);
    vSemaphoreDelete(exited_);
    vSemaphoreDelete(synced_);
    vQueueDelete(queue_);
    if (uac_pcm_) audf_fifo_destroy(uac_pcm_);
}

void Tab5Audio::send(Op op) {
    const Item item = {op, nullptr, 0, 0, nullptr, nullptr};
    xQueueSend(queue_, &item, portMAX_DELAY);
}

esp_err_t Tab5Audio::start(uint32_t capacity_bytes) {
    capacity_bytes_ = capacity_bytes;
    queue_ = xQueueCreate(kQueueLength, sizeof(Item));
    exited_ = xSemaphoreCreateBinary();
    synced_ = xSemaphoreCreateBinary();
    if (!queue_ || !exited_ || !synced_) return ESP_ERR_NO_MEM;
    audf_fifo_config_t fifo = {};
    fifo.fmt = AUDF_FMT_S16;
    fifo.channels = kUacChannels;
    fifo.capacity = kUacBufferFrames;
    fifo.prefill = kUacTargetFrames;
    fifo.underrun = AUDF_FIFO_UNDERRUN_SILENCE;
    fifo.overrun = AUDF_FIFO_OVERRUN_DROP_OLDEST;
    esp_err_t err = audf_fifo_create(&fifo, &uac_pcm_);
    if (err != ESP_OK) return err;
    {
        std::lock_guard<std::mutex> guard(volume_lock_);
        volume_ = kVolumeDefault;
        uac_volume_ = kVolumeDefault;
    }
    apply_volume();
    if (xTaskCreatePinnedToCore(task, "tab5_audio", kTaskStackBytes, this, kTaskPriority,
                                nullptr, kTaskCore) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void Tab5Audio::submit(const uint8_t *data, uint32_t length, uint32_t session, Done done,
                       void *context) {
    queued_bytes_ += length;
    const Item item = {Op::Transfer, data, length, session, done, context};
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void Tab5Audio::end() {
    send(Op::End);
}

void Tab5Audio::sync() {
    send(Op::Sync);
    xSemaphoreTake(synced_, portMAX_DELAY);
}

void Tab5Audio::apply_volume() {
    std::lock_guard<std::mutex> guard(volume_lock_);
    const bool uac = uac_playing_;
    bsp_audio_set_volume(bsp_volume(uac ? uac_volume_ : volume_));
    bsp_audio_set_mute(uac ? uac_mute_ : mute_);
}

void Tab5Audio::set_volume(bool mute, int16_t volume) {
    tab5::VolumeMessage message;
    {
        std::lock_guard<std::mutex> guard(volume_lock_);
        mute_ = mute;
        volume_ = std::clamp(volume, kVolumeMin, kVolumeMax);
        message = {static_cast<uint8_t>(tab5::Stream::Bulk), mute_, volume_};
    }
    apply_volume();
    events_.send(tab5::Type::Volume, 0, &message, sizeof(message));
}

tab5::Volume Tab5Audio::volume() {
    std::lock_guard<std::mutex> guard(volume_lock_);
    return {mute_, 0, volume_, kVolumeMin, kVolumeMax, kVolumeStep};
}

void Tab5Audio::set_uac_feedback(std::function<void(double)> feedback) {
    uac_feedback_ = std::move(feedback);
}

void Tab5Audio::uac_started() {
    send(Op::UacStart);
}

void Tab5Audio::uac_stopped() {
    send(Op::UacStop);
}

void Tab5Audio::uac_received(const uint8_t *pcm, size_t bytes) {
    audf_fifo_write(uac_pcm_, pcm, bytes / kUacFrameBytes);
}

void Tab5Audio::set_uac_volume(bool mute, int16_t volume) {
    tab5::VolumeMessage message;
    {
        std::lock_guard<std::mutex> guard(volume_lock_);
        uac_mute_ = mute;
        uac_volume_ = std::clamp(volume, kUacVolumeMin, kUacVolumeMax);
        message = {static_cast<uint8_t>(tab5::Stream::Uac), uac_mute_, uac_volume_};
    }
    apply_volume();
    events_.send(tab5::Type::Volume, 0, &message, sizeof(message));
}

tab5::Volume Tab5Audio::uac_volume() {
    std::lock_guard<std::mutex> guard(volume_lock_);
    return {uac_mute_, 0, uac_volume_, kUacVolumeMin, kUacVolumeMax, kUacVolumeStep};
}

void Tab5Audio::task(void *arg) {
    auto *self = static_cast<Tab5Audio *>(arg);
    Item item;
    while (true) {
        if (self->uac_) {
            if (xQueueReceive(self->queue_, &item, 0)) {
                self->run(item);
                if (item.op == Op::Quit) break;
                continue;
            }
            self->play_uac();
            continue;
        }
        if (!xQueueReceive(self->queue_, &item, self->open_ ? kIdleClose : portMAX_DELAY)) {
            self->close();
            continue;
        }
        self->run(item);
        if (item.op == Op::Quit) break;
    }
    xSemaphoreGive(self->exited_);
    vTaskDelete(nullptr);
}

void Tab5Audio::run(const Item &item) {
    switch (item.op) {
        case Op::Transfer:
            break;
        case Op::UacStart:
            close();
            audf_fifo_flush(uac_pcm_);
            uac_ = true;
            uac_playing_ = true;
            apply_volume();
            if (uac_feedback_) uac_feedback_(kUacRate);
            return;
        case Op::UacStop:
            close();
            uac_ = false;
            uac_playing_ = false;
            apply_volume();
            return;
        case Op::Sync:
            xSemaphoreGive(synced_);
            return;
        default:
            close();
            have_format_ = false;
            return;
    }
    if (item.session != session_) {
        dequeue(item.length);
        item.done(item.context);
        return;
    }
    played_ = 0;
    uint32_t bad_seq = 0;
    const bool ok = tab5::for_each_message(
        item.data, item.length, &bad_seq,
        [this](const tab5::MsgHeader &header, const uint8_t *payload) {
            execute(header, payload);
        });
    if (!ok) events_.error(Channel::Audio, ErrorCode::Malformed, bad_seq);
    dequeue(item.length - played_);
    item.done(item.context);
    report(false);
}

void Tab5Audio::execute(const tab5::MsgHeader &header, const uint8_t *payload) {
    switch (static_cast<tab5::Type>(header.type)) {
        case tab5::Type::AudioFormat: {
            tab5::AudioFormat format;
            if (header.length != sizeof(format)) {
                events_.error(Channel::Audio, ErrorCode::Malformed, header.seq);
                return;
            }
            memcpy(&format, payload, sizeof(format));
            have_format_ = format.sample_rate && format.sample_rate <= kMaxRate &&
                           (format.channels == 1 || format.channels == 2) &&
                           (format.bits == 16 || format.bits == 24 || format.bits == 32);
            if (!have_format_) {
                events_.error(Channel::Audio, ErrorCode::Unsupported, header.seq);
                return;
            }
            format_ = format;
            return;
        }
        case tab5::Type::AudioData:
            play(header, payload);
            return;
        default:
            events_.error(Channel::Audio, ErrorCode::Malformed, header.seq);
            return;
    }
}

uint32_t Tab5Audio::frame_bytes() const {
    return format_.channels * format_.bits / 8;
}

void Tab5Audio::play(const tab5::MsgHeader &header, const uint8_t *pcm) {
    if (uac_) {
        events_.error(Channel::Audio, ErrorCode::Unsupported, header.seq);
        return;
    }
    if (!have_format_) return;
    const uint32_t frame = frame_bytes();
    if (header.length % frame) {
        events_.error(Channel::Audio, ErrorCode::Malformed, header.seq);
        return;
    }
    if (!open_ || memcmp(&opened_, &format_, sizeof(format_)) != 0) {
        esp_err_t err = bsp_audio_open(format_.sample_rate, format_.bits, format_.channels);
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "open %u Hz %u ch %u bit: %s", (unsigned)format_.sample_rate,
                     format_.channels, format_.bits, esp_err_to_name(err));
            events_.error(Channel::Audio, ErrorCode::Unsupported, header.seq);
            have_format_ = false;
            return;
        }
        open_ = true;
        opened_ = format_;
    }

    const uint32_t chunk = std::max<uint32_t>(format_.sample_rate / kChunksPerSecond, 1) * frame;
    // bsp_audio_write filters in place; the transfer buffer is ours until done.
    auto *data = const_cast<uint8_t *>(pcm);
    for (uint32_t pos = 0; pos < header.length; pos += chunk) {
        const uint32_t n = std::min(chunk, header.length - pos);
        bsp_audio_write(data + pos, n);
        played_ += n;
        dequeue(n);
        report(false);
    }
}

// The FIFO plays silence until half full and whenever it runs dry; the
// feedback rate steers the host to keep it at half.
void Tab5Audio::play_uac() {
    if (!open_) {
        esp_err_t err = bsp_audio_open(kUacRate, kUacBits, kUacChannels);
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "open uac: %s", esp_err_to_name(err));
            vTaskDelay(kIdleClose);
            return;
        }
        open_ = true;
        opened_ = {kUacRate, kUacChannels, kUacBits, 0};
    }
    alignas(4) uint8_t chunk[kUacChunkFrames * kUacFrameBytes];
    audf_fifo_read(uac_pcm_, chunk, kUacChunkFrames);
    bsp_audio_write(chunk, sizeof(chunk));
    const size_t level = audf_fifo_level(uac_pcm_);
    const double error = ((double)level - kUacTargetFrames) / kUacTargetFrames;
    const double rate = kUacRate * (1.0 - kUacFeedbackGain * error);
    if (uac_feedback_) uac_feedback_(rate);

    const int64_t now = esp_timer_get_time();
    if (now - uac_stats_us_ >= kUacStatsPeriodUs) {
        uac_stats_us_ = now;
        ESP_LOGI(TAG, "uac: %u frames buffered, feedback %.1f Hz", (unsigned)level, rate);
    }
}

void Tab5Audio::dequeue(uint32_t bytes) {
    queued_bytes_ -= std::min(queued_bytes_.load(), bytes);
}

void Tab5Audio::close() {
    if (!open_) return;
    bsp_audio_close();
    open_ = false;
    report(true);
}

void Tab5Audio::report(bool force) {
    if (!have_format_) return;
    const int64_t now = esp_timer_get_time();
    if (!force && now - last_report_us_ < kReportPeriodUs) return;
    last_report_us_ = now;
    const uint32_t frame = frame_bytes();
    const tab5::AudioEvent event = {static_cast<uint32_t>(now), queued_bytes_ / frame,
                                    capacity_bytes_ / frame};
    events_.send(tab5::Type::Audio, 0, &event, sizeof(event));
}
