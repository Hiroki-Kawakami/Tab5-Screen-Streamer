#include "audio_manager.hpp"

#include <cstring>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/stream_buffer.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "bsp_tab5.h"
#include "streamer_usb.h"

static const char *TAG = "audio";

namespace {

// USB wire / codec format: 48 kHz, stereo, 16-bit LE interleaved (4 B/frame).
constexpr size_t FRAME_BYTES = 4;

// SPSC buffer between the USB speaker task (producer, via the pcm callback)
// and the resampler consumer task. ~32 kB ≈ 170 ms of slack — large enough
// to absorb USB/codec jitter and give the drift controller something to
// steer toward (target: half full).
constexpr size_t STREAM_BUF_SIZE = 32 * 1024;

// Per-iteration input chunk.
constexpr size_t CHUNK         = 4096;
constexpr size_t MAX_IN_FRAMES = CHUNK / FRAME_BYTES;
// Worst-case output frames per chunk is ceil(MAX_IN_FRAMES / STEP_MIN) + 1.
// With STEP_MIN = 0.99 that's < MAX_IN_FRAMES + 32 — round up generously so
// the inner guard never trips in practice.
constexpr size_t MAX_OUT_FRAMES = MAX_IN_FRAMES + 32;
constexpr size_t MAX_OUT_BYTES  = MAX_OUT_FRAMES * FRAME_BYTES;

// Drift correction. With Kp = 0.02 the equilibrium offset for a 1000 ppm
// clock drift settles at fill error ≈ 5% of capacity, well within ±50%
// bounds. The ±1% STEP clamp caps the worst-case pitch wobble far below
// audibility for music/speech.
constexpr float Kp       = 0.02f;
constexpr float STEP_MIN = 0.99f;
constexpr float STEP_MAX = 1.01f;

StreamBufferHandle_t s_tx_buf = nullptr;

// USB speaker task -> here. Non-blocking push: if the consumer can't keep up
// (codec write blocking on I2S DMA), drop the tail of this chunk instead of
// stalling the streamer_usb task. A brief glitch beats backing up the USB
// pipeline and accumulating drift.
void pcm_cb(const void *pcm, size_t len, void *) {
    if (!s_tx_buf) return;
    size_t free = xStreamBufferSpacesAvailable(s_tx_buf);
    size_t to_send = len < free ? len : free;
    if (to_send > 0) {
        xStreamBufferSend(s_tx_buf, pcm, to_send, 0);
    }
}

void volume_cb(int volume, void *) {
    bsp_tab5_audio_set_volume(volume);
}

void mute_cb(bool mute, void *) {
    bsp_tab5_audio_set_mute(mute);
}

void consumer_task(void *) {
    uint8_t *in_buf  = static_cast<uint8_t*>(heap_caps_malloc(CHUNK,
        MALLOC_CAP_SPIRAM | MALLOC_CAP_CACHE_ALIGNED));
    int16_t *out_buf = static_cast<int16_t*>(heap_caps_malloc(MAX_OUT_BYTES,
        MALLOC_CAP_SPIRAM | MALLOC_CAP_CACHE_ALIGNED));
    if (!in_buf || !out_buf) {
        ESP_LOGE(TAG, "consumer: buffer alloc failed");
        if (in_buf)  heap_caps_free(in_buf);
        if (out_buf) heap_caps_free(out_buf);
        vTaskDelete(nullptr);
        return;
    }

    // Resampler state. Catmull-Rom cubic Hermite needs 4 samples per output
    // (s_{-1}, s_0, s_1, s_2) around the fractional position phase ∈ [0, 1).
    // hist_{l,r}[0..2] = the three most-recent samples consumed from the
    // input stream (time order, hist[2] = newest). The fourth sample for the
    // current interpolation window is the new input frame being processed; we
    // slide the window forward by 1 per input frame.
    int16_t hist_l[3] = {0, 0, 0};
    int16_t hist_r[3] = {0, 0, 0};
    float   phase     = 0.0f;

    // Up to FRAME_BYTES-1 bytes carried between iterations so the input we
    // feed to the resampler is always frame-aligned. xStreamBufferReceive
    // returns whatever's available, with no alignment guarantee.
    uint8_t leftover[FRAME_BYTES] = {0};
    size_t  leftover_bytes = 0;

    while (true) {
        if (leftover_bytes > 0) {
            memcpy(in_buf, leftover, leftover_bytes);
        }
        size_t cap = CHUNK - leftover_bytes;
        size_t got = xStreamBufferReceive(s_tx_buf, in_buf + leftover_bytes,
                                          cap, portMAX_DELAY);
        if (got == 0) continue;

        // Drift feedback: sample buffer occupancy AFTER drain. Target is
        // half full; if it's filling up, USB supplies faster than I2S drains,
        // so step > 1 (consume one extra phase tick per output) gradually
        // drains it. If it's emptying, step < 1. Continuous fractional
        // resampling — no sample insert/drop.
        size_t fill_after = xStreamBufferBytesAvailable(s_tx_buf);
        float  fill_ratio = STREAM_BUF_SIZE
            ? (float)fill_after / (float)STREAM_BUF_SIZE : 0.5f;
        float  step = 1.0f + Kp * (fill_ratio - 0.5f);
        if      (step < STEP_MIN) step = STEP_MIN;
        else if (step > STEP_MAX) step = STEP_MAX;

        size_t total_bytes    = leftover_bytes + got;
        size_t frames         = total_bytes / FRAME_BYTES;
        size_t consumed_bytes = frames * FRAME_BYTES;
        leftover_bytes        = total_bytes - consumed_bytes;
        if (leftover_bytes > 0) {
            memcpy(leftover, in_buf + consumed_bytes, leftover_bytes);
        }

        const int16_t *in = reinterpret_cast<const int16_t*>(in_buf);
        size_t out_count = 0;
        bool   out_full  = false;
        for (size_t i = 0; i < frames && !out_full; i++) {
            // Sliding 4-sample window: s_{-1}, s_0, s_1, s_2 in time order.
            // Catmull-Rom interpolates between s_0 and s_1 at fraction phase,
            // using s_{-1} and s_2 for slope continuity.
            float sm_l = hist_l[0], s0_l = hist_l[1], s1_l = hist_l[2];
            float sm_r = hist_r[0], s0_r = hist_r[1], s1_r = hist_r[2];
            float s2_l = in[2 * i];
            float s2_r = in[2 * i + 1];
            // Coefficients depend only on the window, not on phase — compute
            // once and reuse for every output that falls inside [s_0, s_1).
            float c0_l = s0_l;
            float c1_l = 0.5f * (s1_l - sm_l);
            float c2_l = sm_l - 2.5f * s0_l + 2.0f * s1_l - 0.5f * s2_l;
            float c3_l = 0.5f * (s2_l - sm_l) + 1.5f * (s0_l - s1_l);
            float c0_r = s0_r;
            float c1_r = 0.5f * (s1_r - sm_r);
            float c2_r = sm_r - 2.5f * s0_r + 2.0f * s1_r - 0.5f * s2_r;
            float c3_r = 0.5f * (s2_r - sm_r) + 1.5f * (s0_r - s1_r);
            while (phase < 1.0f) {
                if (out_count >= MAX_OUT_FRAMES) { out_full = true; break; }
                float t = phase;
                // Horner evaluation: ((c3·t + c2)·t + c1)·t + c0
                float yl = ((c3_l * t + c2_l) * t + c1_l) * t + c0_l;
                float yr = ((c3_r * t + c2_r) * t + c1_r) * t + c0_r;
                // Cubic Hermite can overshoot the input range — saturate.
                if (yl >  32767.0f) yl =  32767.0f;
                if (yl < -32768.0f) yl = -32768.0f;
                if (yr >  32767.0f) yr =  32767.0f;
                if (yr < -32768.0f) yr = -32768.0f;
                out_buf[2 * out_count]     = (int16_t)yl;
                out_buf[2 * out_count + 1] = (int16_t)yr;
                out_count++;
                phase += step;
            }
            if (out_full) break;
            // Advance: window slides forward by one input frame.
            phase -= 1.0f;  // step ≤ 1.01 keeps phase < 1.01 → in [0,1) after subtract
            hist_l[0] = hist_l[1]; hist_l[1] = hist_l[2]; hist_l[2] = (int16_t)s2_l;
            hist_r[0] = hist_r[1]; hist_r[1] = hist_r[2]; hist_r[2] = (int16_t)s2_r;
        }
        if (out_full) {
            // Output buffer ran out before the whole chunk was consumed —
            // resync rather than carry a corrupted phase/history into the
            // next iteration. Worst case: a small audio glitch this chunk.
            phase = 0.0f;
        }

        if (out_count > 0) {
            bsp_tab5_audio_write(out_buf, out_count * FRAME_BYTES);
        }
    }
}

}  // namespace

void audio_manager_init() {
    // Route UAC speaker audio (48 kHz / stereo / 16-bit from the PC) to the
    // ES8388 codec. The codec is already opened (48k/16/2) and unmuted by
    // bsp_tab5_init(), so we only set the playback level here. Only the left
    // channel is wired on the Tab5, so downmix to mono. Start at a sensible
    // volume in case the host never sets one.
    bsp_tab5_audio_set_mono_mix(true);
    bsp_tab5_audio_set_volume(80);

    s_tx_buf = xStreamBufferCreate(STREAM_BUF_SIZE, 1);
    if (!s_tx_buf) {
        ESP_LOGE(TAG, "failed to create stream buffer");
        return;
    }

    if (xTaskCreatePinnedToCore(consumer_task, "audio_resamp", 4096, nullptr,
                                10, nullptr, 1) != pdPASS) {
        ESP_LOGE(TAG, "failed to create resampler task");
        vStreamBufferDelete(s_tx_buf);
        s_tx_buf = nullptr;
        return;
    }

    streamer_usb_audio_set_callbacks(pcm_cb, volume_cb, mute_cb, nullptr);
}
