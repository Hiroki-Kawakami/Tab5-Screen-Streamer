/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_audio.hpp"
#include <algorithm>
#include <cmath>
#include "audf_convert.h"
#include "bsp.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"
#include "tab5_events.hpp"

static const char *TAG = "tab5_audio";

static constexpr int kTaskPriority = 7;
static constexpr int kTaskCore = 1;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 8;
static constexpr uint32_t kIdleCloseMs = 200;
static constexpr TickType_t kIdleClose = pdMS_TO_TICKS(kIdleCloseMs);
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

static constexpr uint32_t kHostTargetMs = 60;
// From the arrival of PCM to its playing: the buffer, the output's DMA ring
// and some margin, so the start can always be aligned by adding silence.
static constexpr uint32_t kHostLatencyMs = 100;
static constexpr uint32_t kHostCapacityMs = 250;
static constexpr uint32_t kHostChunksPerSecond = 250;
// The host clock output stays stereo, so changing the channels never reopens
// it, which takes tens of ms and clicks.
static constexpr uint8_t kHostChannels = 2;
static constexpr int64_t kHostStatsPeriodUs = 5 * 1000 * 1000;

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
    close_host();
}

void Tab5Audio::send(Op op) {
    const Item item = {op, nullptr, 0, 0, nullptr, nullptr, 0};
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
    const Item item = {Op::Transfer, data, length, session, done, context, esp_timer_get_time()};
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
        if (self->host_ || (self->uac_ && !self->bulk_)) {
            if (xQueueReceive(self->queue_, &item, 0)) {
                self->run(item);
                if (item.op == Op::Quit) break;
                continue;
            }
            if (self->host_) {
                self->play_host();
            } else {
                self->play_uac();
            }
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
            if (!bulk_) close();
            uac_ = true;
            update_playing();
            if (uac_feedback_) uac_feedback_(kUacRate);
            return;
        case Op::UacStop:
            if (!bulk_) close();
            uac_ = false;
            update_playing();
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
    received_us_ = item.received_us;
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
                           (format.bits == 16 || format.bits == 24 || format.bits == 32) &&
                           format.clock <= static_cast<uint8_t>(tab5::AudioClock::Host);
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
    if (!have_format_) return;
    const uint32_t frame = frame_bytes();
    if (header.length % frame) {
        events_.error(Channel::Audio, ErrorCode::Malformed, header.seq);
        return;
    }
    if (uac_ && !bulk_) {
        close();
        if (uac_feedback_) uac_feedback_(kUacRate);
    }
    if (format_.clock == static_cast<uint8_t>(tab5::AudioClock::Host)) {
        if (!host_ || host_format_.sample_rate != format_.sample_rate ||
            (host_format_.bits == 16) != (format_.bits == 16)) {
            esp_err_t err = open_host();
            if (err != ESP_OK) {
                ESP_LOGW(TAG, "open host %u Hz %u ch %u bit: %s", (unsigned)format_.sample_rate,
                         format_.channels, format_.bits, esp_err_to_name(err));
                events_.error(Channel::Audio, ErrorCode::Unsupported, header.seq);
                have_format_ = false;
                return;
            }
            bulk_ = true;
            update_playing();
        }
        write_host(pcm, header.length);
        played_ += header.length;
        dequeue(header.length);
        return;
    }
    if (host_) {
        drain_host();
        close_host();
    }
    if (!open_output(format_.sample_rate, format_.bits, format_.channels)) {
        events_.error(Channel::Audio, ErrorCode::Unsupported, header.seq);
        have_format_ = false;
        return;
    }
    bulk_ = true;
    update_playing();

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

bool Tab5Audio::open_output(uint32_t rate, uint8_t bits, uint8_t channels) {
    const tab5::AudioFormat output = {rate, channels, bits, 0, 0};
    if (open_ && memcmp(&opened_, &output, sizeof(output)) == 0) return true;
    esp_err_t err = bsp_audio_open(rate, bits, channels);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "open %u Hz %u ch %u bit: %s", (unsigned)rate, channels, bits,
                 esp_err_to_name(err));
        return false;
    }
    open_ = true;
    opened_ = output;
    return true;
}

esp_err_t Tab5Audio::open_host() {
    drain_host();
    close_host();
    const uint32_t rate = format_.sample_rate;
    const uint8_t channels = kHostChannels;
    const audf_fmt_t fmt = format_.bits == 16 ? AUDF_FMT_S16 : AUDF_FMT_S32;
    const size_t target = (size_t)rate * kHostTargetMs / 1000;
    host_chunk_ = std::max<uint32_t>(rate / kHostChunksPerSecond, 1);

    audf_fifo_config_t fifo = {};
    fifo.fmt = fmt;
    fifo.channels = channels;
    fifo.capacity = (size_t)rate * kHostCapacityMs / 1000;
    fifo.prefill = target;
    fifo.underrun = AUDF_FIFO_UNDERRUN_SILENCE;
    fifo.overrun = AUDF_FIFO_OVERRUN_DROP_OLDEST;
    fifo.alloc_caps = MALLOC_CAP_SPIRAM;
    esp_err_t err = audf_fifo_create(&fifo, &host_pcm_);
    if (err == ESP_OK) {
        audf_resampler_config_t resampler = {};
        resampler.kind = AUDF_RESAMPLER_CUBIC;
        resampler.fmt = fmt;
        resampler.channels = channels;
        resampler.in_rate = rate;
        resampler.out_rate = rate;
        err = audf_resampler_create(&resampler, &host_resampler_);
    }
    if (err == ESP_OK) {
        audf_graph_config_t graph = {};
        graph.max_frames = host_chunk_;
        err = audf_graph_create(&graph, &host_graph_);
    }
    if (err == ESP_OK) {
        audf_drift_config_t drift = {};
        drift.sample_rate = rate;
        drift.target_frames = (size_t)rate * kHostLatencyMs / 1000;
        audf_drift_init(&host_drift_, &drift);
        audf_node_t *queued = audf_graph_add_fifo(host_graph_, nullptr, host_pcm_);
        host_out_ = audf_graph_add_resampler(host_graph_, queued, host_resampler_, nullptr);
        err = audf_graph_build(host_graph_);
    }
    if (err == ESP_OK) {
        host_buffer_ = static_cast<uint8_t *>(heap_caps_malloc(
            host_chunk_ * channels * sizeof(int32_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT));
        if (!host_buffer_) err = ESP_ERR_NO_MEM;
    }
    if (err == ESP_OK && !open_output(rate, fmt == AUDF_FMT_S16 ? 16 : 32, channels)) {
        err = ESP_ERR_NOT_SUPPORTED;
    }
    if (err != ESP_OK) {
        close_host();
        return err;
    }
    host_ = true;
    host_started_ = false;
    host_format_ = format_;
    host_input_us_ = esp_timer_get_time();
    return ESP_OK;
}

void Tab5Audio::close_host() {
    host_ = false;
    if (host_graph_) audf_graph_destroy(host_graph_);
    if (host_resampler_) audf_resampler_destroy(host_resampler_);
    if (host_pcm_) audf_fifo_destroy(host_pcm_);
    heap_caps_free(host_buffer_);
    host_graph_ = nullptr;
    host_out_ = nullptr;
    host_resampler_ = nullptr;
    host_pcm_ = nullptr;
    host_buffer_ = nullptr;
}

void Tab5Audio::drain_host() {
    if (!host_) return;
    while (audf_fifo_primed(host_pcm_) && audf_fifo_level(host_pcm_) > 0) output_host();
}

template <typename T>
static void duplicate_mono(void *data, uint32_t frames) {
    T *samples = static_cast<T *>(data);
    for (uint32_t i = frames; i-- > 0;) {
        const T sample = samples[i];
        samples[2 * i] = sample;
        samples[2 * i + 1] = sample;
    }
}

// The arrival is when the transfer came in, not when this task got to it,
// which reopening the output or a burst of transfers can hold up.
void Tab5Audio::write_host(const uint8_t *pcm, uint32_t bytes) {
    host_input_us_ = received_us_;
    const uint8_t channels = format_.channels;
    const uint32_t frame = frame_bytes();
    const audf_pcm_t layout = format_.bits == 24 ? AUDF_PCM_S24 : AUDF_PCM_S32;
    for (uint32_t frames = bytes / frame; frames > 0;) {
        const uint32_t n = std::min(frames, host_chunk_);
        if (format_.bits == 16) {
            memcpy(host_buffer_, pcm, n * frame);
            if (channels == 1) duplicate_mono<int16_t>(host_buffer_, n);
        } else {
            audf_convert_from_pcm(pcm, layout, host_buffer_, AUDF_FMT_S32, n * channels);
            if (channels == 1) duplicate_mono<int32_t>(host_buffer_, n);
        }
        audf_fifo_write(host_pcm_, host_buffer_, n);
        pcm += n * frame;
        frames -= n;
    }
}

// Plays silence or drops PCM so the newest PCM plays kHostLatencyMs after it
// arrived, which the same PCM does on every device fed at the same time.
void Tab5Audio::start_host(double offset_frames) {
    const uint32_t frame = kHostChannels * (host_format_.bits == 16 ? 2 : 4);
    if (offset_frames >= 0) {
        memset(host_buffer_, 0, host_chunk_ * frame);
        for (uint32_t left = (uint32_t)offset_frames; left > 0;) {
            const uint32_t n = std::min(left, host_chunk_);
            bsp_audio_write(host_buffer_, n * frame);
            left -= n;
        }
        return;
    }
    uint32_t drop = std::min<uint32_t>((uint32_t)-offset_frames, audf_fifo_level(host_pcm_));
    while (drop > 0) {
        const uint32_t n = std::min(drop, host_chunk_);
        audf_fifo_read(host_pcm_, host_buffer_, n);
        drop -= n;
    }
}

// The latency is counted from the arrival of the newest PCM, which stays the
// same between arrivals: what has waited since then has played from the FIFO.
void Tab5Audio::output_host() {
    const uint32_t samples = host_chunk_ * kHostChannels;
    if (!audf_fifo_primed(host_pcm_)) {
        host_started_ = false;
    } else {
        const uint32_t rate = host_format_.sample_rate;
        size_t queued = 0;
        bsp_audio_get_queued(&queued);
        const int64_t waited = esp_timer_get_time() - host_input_us_;
        host_latency_ = (double)waited * rate / 1e6 + audf_fifo_level(host_pcm_) + queued;
        const double target = (double)rate * kHostLatencyMs / 1000;
        if (!host_started_) {
            host_started_ = true;
            start_host(target - host_latency_);
            audf_drift_reset(&host_drift_);
            audf_resampler_set_adjust(host_resampler_, 0);
        } else {
            const size_t latency = (size_t)std::max(host_latency_, 0.0);
            audf_resampler_set_adjust(host_resampler_,
                                      audf_drift_update(&host_drift_, latency, host_chunk_));
        }
    }
    esp_err_t err = audf_graph_read(host_graph_, host_out_, host_buffer_, host_chunk_);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "host read: %s", esp_err_to_name(err));
        memset(host_buffer_, 0, samples * sizeof(int32_t));
    }
    if (host_format_.bits == 16) {
        bsp_audio_write(host_buffer_, samples * sizeof(int16_t));
        return;
    }
    audf_convert_to_pcm(host_buffer_, AUDF_FMT_S32, host_buffer_, AUDF_PCM_S32, samples);
    bsp_audio_write(host_buffer_, samples * sizeof(int32_t));
}

// Plays silence while the FIFO fills to its target; sustained silence closes
// the output.
void Tab5Audio::play_host() {
    const int64_t now = esp_timer_get_time();
    if (!audf_fifo_primed(host_pcm_) && now - host_input_us_ >= kIdleCloseMs * 1000) {
        close();
        return;
    }
    output_host();
    report(false);
    if (now - host_stats_us_ >= kHostStatsPeriodUs) {
        host_stats_us_ = now;
        ESP_LOGI(TAG, "host: %u frames buffered, latency %.2f ms, adjust %.1f ppm",
                 (unsigned)audf_fifo_level(host_pcm_),
                 host_latency_ * 1000 / host_format_.sample_rate, host_drift_.ppm);
    }
}

// The FIFO plays silence until half full and whenever it runs dry; the
// feedback rate steers the host to keep it at half.
void Tab5Audio::play_uac() {
    if (!open_) audf_fifo_flush(uac_pcm_);
    if (!open_output(kUacRate, kUacBits, kUacChannels)) {
        vTaskDelay(kIdleClose);
        return;
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
    close_host();
    if (!open_) return;
    bsp_audio_close();
    open_ = false;
    report(true);
    bulk_ = false;
    update_playing();
}

void Tab5Audio::update_playing() {
    const bool uac = uac_ && !bulk_;
    if (uac == uac_playing_) return;
    uac_playing_ = uac;
    apply_volume();
}

void Tab5Audio::report(bool force) {
    if (!have_format_) return;
    const int64_t now = esp_timer_get_time();
    if (!force && now - last_report_us_ < kReportPeriodUs) return;
    last_report_us_ = now;
    const uint32_t frame = frame_bytes();
    uint32_t buffered = queued_bytes_ / frame;
    uint32_t capacity = capacity_bytes_ / frame;
    if (host_) {
        buffered += audf_fifo_level(host_pcm_);
        capacity = host_format_.sample_rate * kHostCapacityMs / 1000;
    }
    const tab5::AudioEvent event = {static_cast<uint32_t>(now), buffered, capacity};
    events_.send(tab5::Type::Audio, 0, &event, sizeof(event));
}
