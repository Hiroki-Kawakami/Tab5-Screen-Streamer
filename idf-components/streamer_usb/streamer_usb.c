#include "streamer_usb.h"
#include "streamer_usb_uac.h"
#include "esp_check.h"
#include "esp_err.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "esp_private/usb_phy.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/stream_buffer.h"
#include "freertos/task.h"
#include "tusb.h"
#include <string.h>

const static char *TAG = "USBDevice";

// ---------------------------------------------------------------------------
// Vendor IN (device -> host) transmit queue
// ---------------------------------------------------------------------------
// tud_vendor_write() must only run from the USB task (TinyUSB is not
// thread-safe). Producers (e.g. the touch task) enqueue a copy of their message
// here; the USB task drains it after each tud_task() and does the actual write.
typedef struct {
    uint8_t len;
    uint8_t data[STREAMER_USB_VENDOR_MSG_MAX];
} vendor_tx_msg_t;
#define VENDOR_TX_QUEUE_LEN 8
static QueueHandle_t s_vendor_tx_queue;

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
// Seed volume to -10 dB (≈80% in the 0..100 mapping below), matching the codec's
// startup level — so a GET_CUR before the host sends anything (or right after a
// re-enumeration) reports a sane value instead of the 0 dB default (= full).
#define VOLUME_DEFAULT  (-10 * 256)
static int8_t  s_mute[UAC_SPK_CHANNELS + 1];
_Static_assert(UAC_SPK_CHANNELS == 2, "s_volume seed below assumes master + 2 channels");
static int16_t s_volume[UAC_SPK_CHANNELS + 1] = {
    VOLUME_DEFAULT, VOLUME_DEFAULT, VOLUME_DEFAULT,
};
static uint32_t s_sample_rate = UAC_SAMPLE_RATE;

static uint8_t s_spk_buf[UAC_EP_OUT_SW_BUF_SZ];  // scratch for one EP-FIFO drain
static bool    s_spk_active;
static TaskHandle_t s_spk_task_handle;

// Set while the USB bus is suspended. On the ESP32-P4's internal PHY (no VBUS
// sensing) a cable pull is not reported as an unmount — the device only sees
// the bus go idle and fires tud_suspend_cb. So "connected" must mean mounted
// AND not suspended, otherwise the panel keeps showing "USB Connected" forever
// after unplug. During active streaming the host always sends SOF, so suspend
// only fires on a real disconnect / host sleep.
static volatile bool s_suspended;

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
    s_vendor_tx_queue = xQueueCreate(VENDOR_TX_QUEUE_LEN, sizeof(vendor_tx_msg_t));
    ESP_RETURN_ON_FALSE(s_vendor_tx_queue != NULL, ESP_ERR_NO_MEM, TAG, "Failed to create vendor tx queue");
    s_spk_stream = xStreamBufferCreate(SPK_STREAM_SZ, 1);
    ESP_RETURN_ON_FALSE(s_spk_stream != NULL, ESP_ERR_NO_MEM, TAG, "Failed to create spk stream");
    BaseType_t ok = xTaskCreatePinnedToCore(uac_spk_task, "uac_spk", 4096, NULL, 6,
                                            &s_spk_task_handle, 0);
    ESP_RETURN_ON_FALSE(ok == pdPASS, ESP_FAIL, TAG, "Failed to create uac_spk task");

    tusb_init();
    return ESP_OK;
}
// Drain the vendor IN queue and write each message to the host. Runs in the USB
// task so all tud_vendor_* calls stay single-threaded.
static void vendor_tx_drain(void) {
    if (!s_vendor_tx_queue) return;
    vendor_tx_msg_t msg;
    while (xQueueReceive(s_vendor_tx_queue, &msg, 0) == pdTRUE) {
        if (!tud_mounted()) continue;  // discard while detached
        tud_vendor_write(msg.data, msg.len);
        tud_vendor_write_flush();
    }
}

void streamer_usb_task(void) {
    while (true) {
        // Bounded timeout (vs the default UINT32_MAX) so the loop also wakes to
        // flush queued vendor IN messages (touch reports) within a few ms even
        // when no USB event is pending.
        tud_task_ext(5, false);
        vendor_tx_drain();
    }
}

bool streamer_usb_mounted(void) { return tud_mounted() && !s_suspended; }

// Vendor specific class
uint32_t streamer_usb_vendor_available(void) { return tud_vendor_available(); }
uint32_t streamer_usb_vendor_read(void *buffer, uint32_t bufsize) { return tud_vendor_read(buffer, bufsize); }

bool streamer_usb_vendor_write(const void *data, uint32_t len) {
    if (!s_vendor_tx_queue || len == 0 || len > STREAMER_USB_VENDOR_MSG_MAX) {
        return false;
    }
    vendor_tx_msg_t msg;
    msg.len = (uint8_t)len;
    memcpy(msg.data, data, len);
    // Non-blocking: drop the message if the queue is backed up (the host isn't
    // draining). Touch reports are snapshots, so a dropped one self-heals.
    return xQueueSend(s_vendor_tx_queue, &msg, 0) == pdTRUE;
}

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
    s_suspended = false;
    ESP_LOGI(TAG, "USB mounted");
}
void tud_umount_cb(void) {
    s_spk_active = false;
    ESP_LOGI(TAG, "USB unmounted");
}
void tud_suspend_cb(bool remote_wakeup_en) {
    (void)remote_wakeup_en;
    s_spk_active = false;
    s_suspended = true;
}
void tud_resume_cb(void) { s_suspended = false; }

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

    // Bound the channel index: the host can request any channel and the state
    // arrays only cover master (0) + UAC_SPK_CHANNELS. All entries are mirrored
    // on SET, so an in-range index always reflects the current value.
    uint8_t ch = request->bChannelNumber;
    if (ch > UAC_SPK_CHANNELS) ch = 0;

    if (request->bControlSelector == AUDIO_FU_CTRL_MUTE && request->bRequest == AUDIO_CS_REQ_CUR) {
        audio_control_cur_1_t mute1 = { .bCur = s_mute[ch] };
        return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &mute1, sizeof(mute1));
    } else if (request->bControlSelector == AUDIO_FU_CTRL_VOLUME) {
        if (request->bRequest == AUDIO_CS_REQ_RANGE) {
            audio_control_range_2_n_t(1) range_vol = {
                .wNumSubRanges = tu_htole16(1),
                .subrange[0] = { .bMin = tu_htole16(-VOLUME_CTRL_50_DB), tu_htole16(VOLUME_CTRL_0_DB), tu_htole16(256) },
            };
            return tud_audio_buffer_and_schedule_control_xfer(rhport, (tusb_control_request_t const *)request, &range_vol, sizeof(range_vol));
        } else if (request->bRequest == AUDIO_CS_REQ_CUR) {
            audio_control_cur_2_t cur_vol = { .bCur = tu_htole16(s_volume[ch]) };
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
        int8_t mute = ((audio_control_cur_1_t const *)buf)->bCur;
        // Mirror to master + every channel so a later GET_CUR on any channel
        // (e.g. the host re-reading state on re-enumeration) is consistent.
        for (int ch = 0; ch <= UAC_SPK_CHANNELS; ch++) s_mute[ch] = mute;
        if (s_mute_cb) {
            s_mute_cb(mute != 0, s_cb_ctx);
        }
        return true;
    } else if (request->bControlSelector == AUDIO_FU_CTRL_VOLUME) {
        TU_VERIFY(request->wLength == sizeof(audio_control_cur_2_t));
        int16_t cur = (int16_t)((audio_control_cur_2_t const *)buf)->bCur;
        // The host may drive volume on the master channel (0) or per channel
        // (L/R). Mirror the value across all of them so the master never lags
        // at its 0 dB default — otherwise a re-enumeration that reads back the
        // master would jump the volume to full. Keep master/channels in sync.
        for (int ch = 0; ch <= UAC_SPK_CHANNELS; ch++) s_volume[ch] = cur;
        int volume_db = cur / 256;                 // -50 .. 0 dB
        int volume = (volume_db + 50) * 2;         // -> 0 .. 100
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
        // This callback runs in true ISR context (the DWC2 dcd dispatches the
        // audio xfer-complete via driver->xfer_isr, directly inside the USB
        // interrupt). So the ISR-safe API is mandatory: the task-level
        // xStreamBufferSend() reaches vTaskSuspendAll()/xTaskResumeAll() when it
        // wakes the blocked writer task, which corrupts the FreeRTOS scheduler
        // state from an ISR and hangs the kernel with no panic output.
        BaseType_t hpw = pdFALSE;
        xStreamBufferSendFromISR(s_spk_stream, s_spk_buf, n, &hpw);
        portYIELD_FROM_ISR(hpw);
    }
    return true;
}
