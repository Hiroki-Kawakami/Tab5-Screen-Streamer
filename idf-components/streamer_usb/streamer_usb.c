#include "streamer_usb.h"
#include "streamer_usb_uac.h"
#include "esp_check.h"
#include "esp_err.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "esp_private/usb_phy.h"
#include "freertos/FreeRTOS.h"
#include "freertos/stream_buffer.h"
#include "freertos/task.h"
#include "tusb.h"

const static char *TAG = "USBDevice";

// ---------------------------------------------------------------------------
// UAC 2.0 speaker state
// ---------------------------------------------------------------------------
// Volume range advertised to the host, in 1/256 dB. -50 dB .. 0 dB maps to the
// 0..100 range handed to the playback sink.
enum {
    VOLUME_CTRL_0_DB   = 0,
    VOLUME_CTRL_50_DB  = 12800,
};

static streamer_usb_audio_pcm_cb_t    s_pcm_cb;
static streamer_usb_audio_volume_cb_t s_volume_cb;
static streamer_usb_audio_mute_cb_t   s_mute_cb;
static void                          *s_cb_ctx;

// Feature unit state: index 0 is the master channel, 1..N the logical channels.
static int8_t  s_mute[UAC_SPK_CHANNELS + 1];
static int16_t s_volume[UAC_SPK_CHANNELS + 1];
static uint32_t s_sample_rate = UAC_SAMPLE_RATE;

static uint8_t s_spk_buf[UAC_EP_OUT_SW_BUF_SZ];  // scratch for one EP-FIFO drain
static bool    s_spk_active;
static TaskHandle_t s_spk_task_handle;

// Decouple the USB receive cadence (a tiny chunk every 125us microframe) from
// the codec, which prefers larger, less frequent writes. The USB side pushes
// into this ring; the writer task pulls big chunks out and blocks on I2S. If
// the codec can't keep up the ring fills and the USB side drops the newest
// data — a brief glitch, never a stall of the USB endpoint drain.
#define SPK_STREAM_SZ    (16 * 1024)
#define SPK_WRITE_CHUNK  2048
static StreamBufferHandle_t s_spk_stream;
static uint8_t s_spk_write_buf[SPK_WRITE_CHUNK];

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------
static void uac_spk_task(void *arg) {
    while (true) {
        size_t n = xStreamBufferReceive(s_spk_stream, s_spk_write_buf,
                                        sizeof(s_spk_write_buf), portMAX_DELAY);
        if (n > 0 && s_pcm_cb) {
            s_pcm_cb(s_spk_write_buf, n, s_cb_ctx);
        }
    }
}

esp_err_t streamer_usb_init(void) {
    // Configure USB PHY
    usb_phy_handle_t phy_hdl = NULL;
    usb_phy_config_t phy_conf = {
        .controller = USB_PHY_CTRL_OTG,
        .target = USB_PHY_TARGET_UTMI,
        .otg_mode = USB_OTG_MODE_DEVICE,
        .otg_speed = USB_PHY_SPEED_HIGH,
    };
    ESP_RETURN_ON_ERROR(usb_new_phy(&phy_conf, &phy_hdl), TAG, "Install USB PHY failed");

    s_sample_rate = UAC_SAMPLE_RATE;
    s_spk_stream = xStreamBufferCreate(SPK_STREAM_SZ, 1);
    ESP_RETURN_ON_FALSE(s_spk_stream != NULL, ESP_ERR_NO_MEM, TAG, "Failed to create spk stream");
    BaseType_t ok = xTaskCreatePinnedToCore(uac_spk_task, "uac_spk", 4096, NULL, 6,
                                            &s_spk_task_handle, 0);
    ESP_RETURN_ON_FALSE(ok == pdPASS, ESP_FAIL, TAG, "Failed to create uac_spk task");

    tusb_init();
    return ESP_OK;
}
void streamer_usb_task(void) {
    while (true) {
        tud_task();
    }
}

bool streamer_usb_mounted(void) { return tud_mounted(); }

// Vendor specific class
uint32_t streamer_usb_vendor_available(void) { return tud_vendor_available(); }
uint32_t streamer_usb_vendor_read(void *buffer, uint32_t bufsize) { return tud_vendor_read(buffer, bufsize); }

// ---------------------------------------------------------------------------
// UAC 2.0 audio (speaker) public API
// ---------------------------------------------------------------------------
void streamer_usb_audio_set_callbacks(streamer_usb_audio_pcm_cb_t pcm_cb,
                                      streamer_usb_audio_volume_cb_t volume_cb,
                                      streamer_usb_audio_mute_cb_t mute_cb,
                                      void *ctx) {
    s_pcm_cb = pcm_cb;
    s_volume_cb = volume_cb;
    s_mute_cb = mute_cb;
    s_cb_ctx = ctx;
}

uint32_t streamer_usb_audio_sample_rate(void) { return UAC_SAMPLE_RATE; }
uint8_t  streamer_usb_audio_channels(void)    { return UAC_SPK_CHANNELS; }
uint8_t  streamer_usb_audio_bits(void)        { return UAC_BIT_RESOLUTION; }

// ---------------------------------------------------------------------------
// UAC 2.0 TinyUSB callbacks
// ---------------------------------------------------------------------------
void tud_mount_cb(void) {
    s_spk_active = false;
    ESP_LOGI(TAG, "USB mounted");
}
void tud_umount_cb(void) {
    s_spk_active = false;
    ESP_LOGI(TAG, "USB unmounted");
}
void tud_suspend_cb(bool remote_wakeup_en) {
    (void)remote_wakeup_en;
    s_spk_active = false;
}
void tud_resume_cb(void) {}

// ---- Clock source (sample rate) ----
static bool clock_get_request(uint8_t rhport, audio_control_request_t const *request) {
    TU_ASSERT(request->bEntityID == UAC2_ENTITY_CLOCK);

    if (request->bControlSelector == AUDIO_CS_CTRL_SAM_FREQ) {
        if (request->bRequest == AUDIO_CS_REQ_CUR) {
            audio_control_cur_4_t curf = { (int32_t)tu_htole32(s_sample_rate) };
            return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &curf, sizeof(curf));
        } else if (request->bRequest == AUDIO_CS_REQ_RANGE) {
            // Single fixed sample rate.
            audio_control_range_4_n_t(1) rangef = {
                .wNumSubRanges = tu_htole16(1),
                .subrange[0] = { .bMin = (int32_t)UAC_SAMPLE_RATE, .bMax = (int32_t)UAC_SAMPLE_RATE, .bRes = 0 },
            };
            return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &rangef, sizeof(rangef));
        }
    } else if (request->bControlSelector == AUDIO_CS_CTRL_CLK_VALID && request->bRequest == AUDIO_CS_REQ_CUR) {
        audio_control_cur_1_t cur_valid = { .bCur = 1 };
        return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &cur_valid, sizeof(cur_valid));
    }
    return false;
}

static bool clock_set_request(uint8_t rhport, audio_control_request_t const *request, uint8_t const *buf) {
    (void)rhport;
    TU_ASSERT(request->bEntityID == UAC2_ENTITY_CLOCK);
    TU_VERIFY(request->bRequest == AUDIO_CS_REQ_CUR);

    if (request->bControlSelector == AUDIO_CS_CTRL_SAM_FREQ) {
        TU_VERIFY(request->wLength == sizeof(audio_control_cur_4_t));
        // Only the single advertised rate is supported.
        uint32_t rate = (uint32_t)((audio_control_cur_4_t const *)buf)->bCur;
        return rate == s_sample_rate;
    }
    return false;
}

// ---- Speaker feature unit (mute / volume) ----
static bool feature_unit_get_request(uint8_t rhport, audio_control_request_t const *request) {
    TU_ASSERT(request->bEntityID == UAC2_ENTITY_SPK_FEATURE_UNIT);

    if (request->bControlSelector == AUDIO_FU_CTRL_MUTE && request->bRequest == AUDIO_CS_REQ_CUR) {
        audio_control_cur_1_t mute1 = { .bCur = s_mute[request->bChannelNumber] };
        return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &mute1, sizeof(mute1));
    } else if (request->bControlSelector == AUDIO_FU_CTRL_VOLUME) {
        if (request->bRequest == AUDIO_CS_REQ_RANGE) {
            audio_control_range_2_n_t(1) range_vol = {
                .wNumSubRanges = tu_htole16(1),
                .subrange[0] = { .bMin = tu_htole16(-VOLUME_CTRL_50_DB), tu_htole16(VOLUME_CTRL_0_DB), tu_htole16(256) },
            };
            return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &range_vol, sizeof(range_vol));
        } else if (request->bRequest == AUDIO_CS_REQ_CUR) {
            audio_control_cur_2_t cur_vol = { .bCur = tu_htole16(s_volume[request->bChannelNumber]) };
            return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &cur_vol, sizeof(cur_vol));
        }
    }
    return false;
}

static bool feature_unit_set_request(uint8_t rhport, audio_control_request_t const *request, uint8_t const *buf) {
    (void)rhport;
    TU_ASSERT(request->bEntityID == UAC2_ENTITY_SPK_FEATURE_UNIT);
    TU_VERIFY(request->bRequest == AUDIO_CS_REQ_CUR);

    if (request->bControlSelector == AUDIO_FU_CTRL_MUTE) {
        TU_VERIFY(request->wLength == sizeof(audio_control_cur_1_t));
        s_mute[request->bChannelNumber] = ((audio_control_cur_1_t const *)buf)->bCur;
        if (s_mute_cb) {
            s_mute_cb(s_mute[request->bChannelNumber] != 0, s_cb_ctx);
        }
        return true;
    } else if (request->bControlSelector == AUDIO_FU_CTRL_VOLUME) {
        TU_VERIFY(request->wLength == sizeof(audio_control_cur_2_t));
        s_volume[request->bChannelNumber] = ((audio_control_cur_2_t const *)buf)->bCur;
        int volume_db = s_volume[request->bChannelNumber] / 256;  // -50 .. 0 dB
        int volume = (volume_db + 50) * 2;                        // -> 0 .. 100
        if (volume < 0)   volume = 0;
        if (volume > 100) volume = 100;
        if (s_volume_cb) {
            s_volume_cb(volume, s_cb_ctx);
        }
        return true;
    }
    return false;
}

bool tud_audio_get_req_entity_cb(uint8_t rhport, tusb_control_request_t const *p_request) {
    audio_control_request_t const *request = (audio_control_request_t const *)p_request;
    if (request->bEntityID == UAC2_ENTITY_CLOCK) {
        return clock_get_request(rhport, request);
    }
    if (request->bEntityID == UAC2_ENTITY_SPK_FEATURE_UNIT) {
        return feature_unit_get_request(rhport, request);
    }
    return false;
}

bool tud_audio_set_req_entity_cb(uint8_t rhport, tusb_control_request_t const *p_request, uint8_t *buf) {
    audio_control_request_t const *request = (audio_control_request_t const *)p_request;
    if (request->bEntityID == UAC2_ENTITY_SPK_FEATURE_UNIT) {
        return feature_unit_set_request(rhport, request, buf);
    }
    if (request->bEntityID == UAC2_ENTITY_CLOCK) {
        return clock_set_request(rhport, request, buf);
    }
    return false;
}

// ---- Streaming interface open/close ----
bool tud_audio_set_itf_cb(uint8_t rhport, tusb_control_request_t const *p_request) {
    (void)rhport;
    uint8_t const itf = tu_u16_low(tu_le16toh(p_request->wIndex));
    uint8_t const alt = tu_u16_low(tu_le16toh(p_request->wValue));

    if (itf == ITF_NUM_AUDIO_STREAMING && alt != 0) {
        s_spk_active = true;
        ESP_LOGI(TAG, "Speaker streaming started (alt %d)", alt);
    }
    return true;
}

bool tud_audio_set_itf_close_EP_cb(uint8_t rhport, tusb_control_request_t const *p_request) {
    (void)rhport;
    uint8_t const itf = tu_u16_low(tu_le16toh(p_request->wIndex));
    uint8_t const alt = tu_u16_low(tu_le16toh(p_request->wValue));

    if (itf == ITF_NUM_AUDIO_STREAMING && alt == 0) {
        s_spk_active = false;
        ESP_LOGI(TAG, "Speaker streaming stopped");
    }
    return true;
}

// ---- Data path: drain the EP FIFO and push it into the ring ----
bool tud_audio_rx_done_isr(uint8_t rhport, uint16_t n_bytes_received, uint8_t func_id, uint8_t ep_out, uint8_t cur_alt_setting) {
    (void)rhport;
    (void)n_bytes_received;
    (void)ep_out;
    (void)cur_alt_setting;

    static int64_t last_time = 0;
    int64_t now = esp_timer_get_time();

    // A long gap means a new stream is starting: drop stale FIFO data.
    if (now - last_time > 1000 * UAC_NEW_PLAY_INTERVAL_MS) {
        tud_audio_n_clear_ep_out_ff(func_id);
    }
    last_time = now;

    // Always drain the endpoint FIFO so the audio feedback stays healthy and
    // the host keeps streaming, even if the codec writer is momentarily behind.
    uint16_t n = tud_audio_n_read(func_id, s_spk_buf, sizeof(s_spk_buf));
    if (n > 0 && s_spk_stream) {
        // Non-blocking: drop the newest data if the ring is full.
        xStreamBufferSend(s_spk_stream, s_spk_buf, n, 0);
    }
    return true;
}
