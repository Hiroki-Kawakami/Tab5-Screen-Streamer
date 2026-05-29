#pragma once
#include "esp_err.h"
#include "stdbool.h"
#include "stddef.h"
#include "stdint.h"

#ifdef __cplusplus
extern "C" {
#endif

// Initialization
esp_err_t streamer_usb_init(void);
void streamer_usb_task(void);

// Common
bool streamer_usb_mounted(void);

// vendor specific class
uint32_t streamer_usb_vendor_available(void);
uint32_t streamer_usb_vendor_read(void *buffer, uint32_t bufsize);

// ---- UAC 2.0 audio (speaker) ----
// PCM received from the host. Fixed format: 48 kHz, stereo, 16-bit LE,
// interleaved L/R. `pcm`/`len` is one chunk (see streamer_usb_audio_*()).
typedef void (*streamer_usb_audio_pcm_cb_t)(const void *pcm, size_t len, void *ctx);
// Host volume change, mapped to 0..100 (0 = silence).
typedef void (*streamer_usb_audio_volume_cb_t)(int volume, void *ctx);
// Host mute change.
typedef void (*streamer_usb_audio_mute_cb_t)(bool mute, void *ctx);

// Register the playback sink. Any callback may be NULL. Safe to call before or
// after streamer_usb_init(); callbacks run from the internal speaker task.
void streamer_usb_audio_set_callbacks(streamer_usb_audio_pcm_cb_t pcm_cb,
                                      streamer_usb_audio_volume_cb_t volume_cb,
                                      streamer_usb_audio_mute_cb_t mute_cb,
                                      void *ctx);

// Negotiated stream format (constant: 48000 / 2 / 16).
uint32_t streamer_usb_audio_sample_rate(void);
uint8_t  streamer_usb_audio_channels(void);
uint8_t  streamer_usb_audio_bits(void);

#ifdef __cplusplus
}
#endif
