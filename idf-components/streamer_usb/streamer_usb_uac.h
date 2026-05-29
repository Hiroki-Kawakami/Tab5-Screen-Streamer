#pragma once

// Internal UAC 2.0 (USB Audio Class) parameters, shared between tusb_config.h,
// usb_descriptors.c and streamer_usb.c.
//
// This adds a speaker (host -> device, OUT) audio function alongside the
// existing vendor-class JPEG interface, so the device shows up on the PC as a
// playback sound card. Format is fixed at 48 kHz / stereo / 16-bit PCM.

// ---- Stream format ----
#define UAC_SAMPLE_RATE          48000
#define UAC_SPK_CHANNELS         2
#define UAC_BYTES_PER_SAMPLE     2
#define UAC_BIT_RESOLUTION       16
#define UAC_SPK_INTERVAL_MS      10     // SW FIFO depth / first-read chunk, in ms
#define UAC_NEW_PLAY_INTERVAL_MS 100    // gap that resets buffering for a new stream

#define UAC_FRAME_SZ             (UAC_BYTES_PER_SAMPLE * UAC_SPK_CHANNELS)
// One ISO (micro)frame of audio plus one frame of headroom.
#define UAC_EP_SZ_OUT            ((UAC_SAMPLE_RATE / 1000 * UAC_FRAME_SZ) + UAC_FRAME_SZ)
#define UAC_EP_OUT_SW_BUF_SZ     (UAC_EP_SZ_OUT * (UAC_SPK_INTERVAL_MS + 1))

// ---- Interface / endpoint numbering ----
// The vendor JPEG interface keeps itf 0 and endpoints 0x01 / 0x81.
#define ITF_NUM_VENDOR           0
#define ITF_NUM_AUDIO_CONTROL    1
#define ITF_NUM_AUDIO_STREAMING  2
#define ITF_NUM_TOTAL            3

#define EPNUM_AUDIO_OUT          0x02   // ISO data, host -> device (speaker)
#define EPNUM_AUDIO_FB           0x82   // ISO feedback, device -> host

// ---- AudioControl entity IDs (arbitrary, must be unique within the function) ----
#define UAC2_ENTITY_CLOCK               0x04
#define UAC2_ENTITY_SPK_INPUT_TERMINAL  0x01
#define UAC2_ENTITY_SPK_FEATURE_UNIT    0x02
#define UAC2_ENTITY_SPK_OUTPUT_TERMINAL 0x03
