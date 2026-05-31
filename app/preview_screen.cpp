#include "preview_screen.hpp"
#include "platform_port.hpp"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "streamer_usb.h"
#include "streamer.hpp"
#include "lvgl.hpp"
#include "nvs.hpp"
#include "bsp_tab5.h"
#include <cstring>

static const char *TAG = "preview";

namespace {

// Panel geometry. The PC streams JPEGs sized to the native panel, so each
// frame is decoded straight into a panel framebuffer (no rotation/scaling).
constexpr int DISPLAY_WIDTH = 720;
constexpr int DISPLAY_HEIGHT = 1280;
constexpr int FRAME_BUFFER_NUM = 3;

// Ring of input buffers the renderer fills from USB. Plenty of slack so the
// renderer can keep receiving while the decoder is still busy with an older
// frame; the decoder only ever consumes the newest one.
constexpr size_t JPEG_BUFFER_SIZE = 512 * 1024;
constexpr int JPEG_BUFFER_NUM = 8;

// The PC now streams each frame as a sequence of horizontal JPEG bands. Every
// band has its own 8-byte header (see renderer_task) carrying its position. We
// assume (per the PC sender): x == 0 always, width == 720 always, y arrives in
// increasing order, and the band marked 0x51 ("present") is the last one of a
// frame; the next band after it restarts at y == 0.
//
// Because x == 0 and width == 720 == DISPLAY_WIDTH, a band of `height` rows maps
// to a contiguous region of the framebuffer starting at row `y`, so each band is
// decoded straight into `fb + y * stride`.
constexpr int MAX_BANDS = DISPLAY_HEIGHT / 16;  // smallest band is 16px tall

struct JpegBand {
    const uint8_t *data;  // JPEG payload (lives inside a ring buffer)
    size_t size;          // payload length in bytes
    int y;                // destination row offset in pixels
    int height;           // band height in pixels
};

// A full frame: the bands accumulated up to (and including) the 0x51 band.
struct JpegFrame {
    JpegBand bands[MAX_BANDS];
    int band_count;
};

uint8_t *s_jpeg_buffers[JPEG_BUFFER_NUM] = {};
// Capacity-1 queue used with xQueueOverwrite: the renderer always replaces the
// pending frame with the latest, so the decoder never falls behind on stale
// frames.
QueueHandle_t s_jpeg_queue = nullptr;

// Persisted settings live in the "dstr" namespace (shared with streamer.cpp,
// which reads "pixfmt" at boot to pick the framebuffer format).
NVS settings_nvs("dstr");
constexpr const char *NVS_KEY_BRIGHTNESS = "brt";     // uint8 1..100
constexpr const char *NVS_KEY_PIX_FMT    = "pixfmt";  // 0=RGB888, 1=RGB565

uint8_t load_setting(const char *key, uint8_t fallback) {
    uint8_t v = fallback;
    if (settings_nvs.get(key, &v) != NVS::Error::OK) v = fallback;
    return v;
}

void save_setting(const char *key, uint8_t value) {
    if (settings_nvs.set(key, value) == NVS::Error::OK) {
        settings_nvs.commit();
    }
}

// Set once in build(); lets the decoder task post status updates back to the
// LVGL thread without reaching through the screen stack.
PreviewScreen *s_screen = nullptr;
// True while video frames are flowing. Owned by the decoder task; build() reads
// it once to pick the initial status text.
volatile bool s_connected = false;

// Read exactly `len` bytes into `dst`, blocking on the USB FIFO. Returns false
// if the device goes away mid-read so the caller can resynchronize.
bool read_exact(uint8_t *dst, size_t len) {
    size_t received = 0;
    while (received < len) {
        if (!streamer_usb_mounted()) {
            return false;
        }
        uint32_t avail = streamer_usb_vendor_available();
        if (avail == 0) {
            taskYIELD();
            continue;
        }
        uint32_t want = len - received;
        if (avail < want) {
            want = avail;
        }
        received += streamer_usb_vendor_read(dst + received, want);
    }
    return true;
}

// Scans the USB byte stream for the start of a fresh frame and leaves the stream
// positioned right after that band's 8-byte header (i.e. at its JPEG payload).
//
// The stream carries no global sync marker, so once the byte offset drifts (e.g.
// the PC pauses mid-band and resumes with a brand-new frame) every subsequent
// 8-byte "header" is read from mid-payload garbage and never realigns on its own.
// The first band of every frame, however, is unmistakable: type 0x50/0x51, x==0,
// y==0, width==DISPLAY_WIDTH and a plausible size. We slide a 1-byte-at-a-time
// window (this is a rare recovery path, so simplicity beats throughput) until it
// matches, which leaves us byte-aligned to a frame boundary again.
bool resync_to_frame_start(uint8_t hdr_out[8]) {
    uint8_t win[8] = {};
    size_t filled = 0;
    while (true) {
        if (!streamer_usb_mounted()) {
            return false;
        }
        uint8_t b;
        if (!read_exact(&b, 1)) {
            return false;
        }
        if (filled < sizeof(win)) {
            win[filled++] = b;
            if (filled < sizeof(win)) {
                continue;
            }
        } else {
            for (size_t i = 0; i < sizeof(win) - 1; i++) {
                win[i] = win[i + 1];
            }
            win[sizeof(win) - 1] = b;
        }

        uint8_t type = win[0];
        uint32_t data_size = (uint32_t)win[1] | ((uint32_t)win[2] << 8) |
                             ((uint32_t)win[3] << 16);
        int x_px = (int)win[4] * 16;
        int y_px = (int)win[5] * 16;
        int w_px = (int)win[6] * 16;
        int h_px = (int)win[7] * 16;
        bool first_band = (type == 0x50 || type == 0x51) &&
                          data_size >= 4 && (data_size - 4) <= JPEG_BUFFER_SIZE &&
                          x_px == 0 && y_px == 0 && w_px == DISPLAY_WIDTH &&
                          h_px > 0 && h_px <= DISPLAY_HEIGHT;
        if (first_band) {
            for (size_t i = 0; i < sizeof(win); i++) {
                hdr_out[i] = win[i];
            }
            return true;
        }
    }
}

// Reads banded JPEGs from USB. Per-band 8-byte header:
//   byte 0     : type (0x50 = decode only, 0x51 = last band of the frame -> present)
//   bytes 1..3 : data size, 24-bit little endian = 4 coord bytes + JPEG payload
//   byte 4     : x / 16        (always 0)
//   byte 5     : y / 16
//   byte 6     : width / 16    (always 720/16)
//   byte 7     : height / 16
// The renderer accumulates bands into one ring buffer and, on the 0x51 band,
// hands the assembled frame to the decoder via the capacity-1 overwrite queue.
void renderer_task(void *) {
    int idx = 0;
    static JpegFrame frame;  // static: too large for the task stack
    frame.band_count = 0;
    size_t write_off = 0;
    // Whether the byte stream is aligned to a band boundary. Starts false so we
    // lock onto the first real frame instead of assuming we joined cleanly, and
    // is cleared whenever the framing looks wrong so the next iteration rescans.
    bool synced = false;

    auto reset = [&]() {
        frame.band_count = 0;
        write_off = 0;
    };
    auto desync = [&]() {
        reset();
        synced = false;
    };

    while (true) {
        if (!streamer_usb_mounted()) {
            vTaskDelay(pdMS_TO_TICKS(100));
            desync();
            continue;
        }

        // 8-byte per-band header.
        uint8_t hdr[8];
        if (synced) {
            if (!read_exact(hdr, sizeof(hdr))) {
                desync();
                continue;
            }
        } else {
            // Realign to a frame boundary after a stall/corruption, then start a
            // fresh frame from the band the scan landed on.
            if (!resync_to_frame_start(hdr)) {
                desync();
                continue;
            }
            reset();
            synced = true;
        }

        uint8_t type = hdr[0];
        uint32_t data_size = (uint32_t)hdr[1] | ((uint32_t)hdr[2] << 8) |
                             ((uint32_t)hdr[3] << 16);
        int y_px = (int)hdr[5] * 16;
        int h_px = (int)hdr[7] * 16;

        if ((type != 0x50 && type != 0x51) || data_size < 4) {
            ESP_LOGW(TAG, "bad band header (type=0x%02x size=%u), resyncing",
                     type, (unsigned)data_size);
            desync();
            continue;
        }
        size_t jpeg_len = data_size - 4;

        bool fits = jpeg_len > 0 &&
                    frame.band_count < MAX_BANDS &&
                    write_off + jpeg_len <= JPEG_BUFFER_SIZE &&
                    y_px + h_px <= DISPLAY_HEIGHT;
        if (!fits) {
            // A band that doesn't fit means the framing is suspect; don't trust
            // (and drain) a possibly-bogus length — rescan for a frame boundary.
            ESP_LOGW(TAG, "band doesn't fit (len=%u y=%d h=%d), resyncing",
                     (unsigned)jpeg_len, y_px, h_px);
            desync();
            continue;
        }

        uint8_t *buf = s_jpeg_buffers[idx];
        if (!read_exact(buf + write_off, jpeg_len)) {
            desync();
            continue;
        }
        // Every band's payload is a standalone JPEG, so it must open with the SOI
        // marker (FF D8). If it doesn't, our byte offset has drifted — resync.
        if (jpeg_len < 2 || buf[write_off] != 0xFF || buf[write_off + 1] != 0xD8) {
            ESP_LOGW(TAG, "band payload not JPEG (y=%d), resyncing", y_px);
            desync();
            continue;
        }

        JpegBand &band = frame.bands[frame.band_count++];
        band.data = buf + write_off;
        band.size = jpeg_len;
        band.y = y_px;
        band.height = h_px;
        write_off += jpeg_len;

        if (type == 0x51) {
            if (frame.band_count > 0) {
                xQueueOverwrite(s_jpeg_queue, &frame);
                idx = (idx + 1) % JPEG_BUFFER_NUM;
            }
            reset();
        }
    }
}

// Decodes the latest received frame's bands into the next panel framebuffer and
// flips to it. Each band is full-width (x=0, width=720) so it lands in a
// contiguous region at `fb + y * stride`.
void decoder_task(void *) {
    pf_port::PixelFormat pf = pf_port::display_pixel_format();
    size_t stride = (size_t)DISPLAY_WIDTH * pf_port::bytes_per_pixel(pf);

    pf_port::JpegDecoder decoder;
    decoder.setOutputFormat(pf);

    int fb_index = 0;
    int frame_count = 0;
    int64_t window_start = esp_timer_get_time();

    // `frame` is static and only overwritten on a successful xQueueReceive, so
    // between frames it still holds the most recent one. Unlike the UVC case the
    // PC only streams when its screen changes, so we must keep that last JPEG
    // around: when the user hides the overlay while the screen is static, no new
    // frame arrives to repaint, and the framebuffer still has the UI painted
    // over its top band. We re-decode this frame to clear the overlay. The
    // band pointers stay valid because, with no new frames, the renderer isn't
    // reusing the ring buffer slot they point into.
    static JpegFrame frame;  // static: too large for the task stack
    bool have_frame = false;
    // Whether the overlay was composited into the last flushed framebuffer.
    // Starts false so the first visible iteration paints the panel.
    bool last_gui_v = false;

    // Decode every band of `frame` into framebuffer `fb`. Each band is
    // full-width (x=0, width=720), so it lands at fb + y*stride. Returns false
    // (and logs) on the first band that fails to decode.
    auto decode_frame = [&](uint8_t *fb) -> bool {
        for (int i = 0; i < frame.band_count; i++) {
            const JpegBand &band = frame.bands[i];
            uint8_t *dst = fb + (size_t)band.y * stride;
            size_t dst_size = (size_t)band.height * stride;
            if (decoder.decode(band.data, band.size, dst, dst_size, nullptr) != pf_port::Error::Ok) {
                ESP_LOGW(TAG, "band decode failed (y=%d, %u bytes)", band.y, (unsigned)band.size);
                return false;
            }
        }
        return true;
    };

    while (true) {
        // Short timeout (not portMAX_DELAY) so the GUI overlay keeps refreshing
        // when no video is arriving: brightness-slider feedback and the
        // connection status still need to repaint while disconnected/static.
        bool got_frame = xQueueReceive(s_jpeg_queue, &frame, pdMS_TO_TICKS(33)) == pdTRUE;
        // Snapshot visibility for the whole iteration so a mid-render flip can't
        // leave a half-composited framebuffer.
        bool gui_v = gui_is_visible();

        int next = (fb_index + 1) % FRAME_BUFFER_NUM;
        uint8_t *fb = (uint8_t *)pf_port::display_get_frame_buffer(next);
        bool flush_next = false;

        if (got_frame) {
            have_frame = true;
            // First frame after a (re)connect: flip the status pill to
            // "Connected" and auto-hide the overlay so the full image shows.
            if (!s_connected) {
                s_connected = true;
                gui_set_visible(false);
                gui_v = false;
                if (s_screen) {
                    auto scr = s_screen;
                    lv_async_call([scr]{ scr->set_status_ui(true); });
                }
            }
            if (decode_frame(fb)) {
                // Overlay the LVGL UI on top of the freshly decoded video. The
                // top GUI_PANEL_H rows we just decoded get overwritten — the
                // wasted decode is hidden behind the (blocking) PPA compose and
                // only happens while the overlay is actually up.
                if (gui_v) gui_compose(fb);
                flush_next = true;

                frame_count++;
                int64_t now = esp_timer_get_time();
                if (now - window_start >= 1000000) {
                    ESP_LOGI(TAG, "%d fps", frame_count);
                    frame_count = 0;
                    window_start = now;
                }
            }
        } else {
            // No new frame within the timeout. If the host went away, fall back
            // to the controls panel with a "Disconnected" status.
            if (s_connected && !streamer_usb_mounted()) {
                s_connected = false;
                gui_set_visible(true);
                gui_v = true;
                if (s_screen) {
                    auto scr = s_screen;
                    lv_async_call([scr]{ scr->set_status_ui(false); });
                }
            }

            if (last_gui_v && !gui_v) {
                // The overlay was just hidden and no fresh frame is coming. The
                // on-screen frame still has the UI over its top band, so
                // re-render the last JPEG in full to reveal the video the panel
                // was covering. Falls back to black if we never got a frame.
                if (have_frame) {
                    flush_next = decode_frame(fb);
                } else {
                    memset(fb, 0, (size_t)DISPLAY_HEIGHT * stride);
                    flush_next = true;
                }
            } else if (gui_v) {
                // Overlay up: repaint so slider drags / status changes stay
                // responsive. Carry the last decoded video forward under the
                // panel while connected; clear it once disconnected so a stale
                // frame doesn't stay frozen behind the controls.
                size_t video_off  = (size_t)GUI_PANEL_H * stride;
                size_t video_size = (size_t)(DISPLAY_HEIGHT - GUI_PANEL_H) * stride;
                if (s_connected && have_frame) {
                    uint8_t *cur = (uint8_t *)pf_port::display_get_frame_buffer(fb_index);
                    memcpy(fb + video_off, cur + video_off, video_size);
                } else {
                    memset(fb + video_off, 0, video_size);
                }
                gui_compose(fb);
                flush_next = true;
            }
            // else: overlay hidden and already hidden — leave the last frame on
            // screen (the PC will repaint it only when it actually changes).
        }

        if (flush_next) {
            pf_port::display_flush(next);
            fb_index = next;
            last_gui_v = gui_v;
        }
    }
}

} // namespace

PreviewScreen::PreviewScreen() {}

void PreviewScreen::set_status_ui(bool connected) {
    if (!status_container_ || !status_label_) return;
    lv_obj_set_style_bg_color(status_container_, lv_color_hex(connected ? 0x0EBC00 : 0xC20000), 0);
    lv_label_set_text(status_label_, connected ? "Connected" : "Disconnected");
}

// The UI is composited onto the top GUI_PANEL_H rows of every flushed frame
// (see decoder_task + gui_compose). Input Format and Volume are driven from the
// PC, so the panel only exposes Brightness and the quality/framerate trade-off.
void PreviewScreen::build() {
    s_screen = this;

    lv_obj_set_style_bg_color(root_, lv_color_hex(0xCCCCCC), 0);
    lv_obj_set_style_bg_opa(root_, LV_OPA_COVER, 0);
    lv_obj_set_flex_flow(root_, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(root_, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_all(root_, 20, 0);

    status_container_ = lv_obj_create(root_);
    lv_obj_remove_style_all(status_container_);
    lv_obj_set_size(status_container_, LV_PCT(100), 100);
    lv_obj_set_style_radius(status_container_, 15, 0);
    lv_obj_set_style_bg_opa(status_container_, LV_OPA_COVER, 0);
    status_label_ = lv_label_create(status_container_);
    lv_obj_center(status_label_);
    lv_obj_set_style_text_color(status_label_, lv_color_white(), 0);
    lv_obj_set_style_text_font(status_label_, &lv_font_montserrat_20, 0);
    set_status_ui(s_connected);

    auto br_lbl = lv_label_create(root_);
    lv_label_set_text(br_lbl, "Brightness");
    lv_obj_set_width(br_lbl, LV_PCT(100));
    lv_obj_set_style_margin_top(br_lbl, 20, 0);

    brightness_slider_ = lv_slider_create(root_);
    lv_obj_set_width(brightness_slider_, LV_PCT(100));
    lv_slider_set_range(brightness_slider_, 1, 100);
    lv_slider_set_value(brightness_slider_, load_setting(NVS_KEY_BRIGHTNESS, 50), LV_ANIM_OFF);
    lv_obj_add_event_fn(brightness_slider_, LV_EVENT_VALUE_CHANGED, [](lv_event_t *e){
        auto s = (lv_obj_t*)lv_event_get_target(e);
        pf_port::display_set_brightness(lv_slider_get_value(s));
    });
    // Persist only on release so we don't hammer NVS with every drag tick.
    lv_obj_add_event_fn(brightness_slider_, LV_EVENT_RELEASED, [](lv_event_t *e){
        auto s = (lv_obj_t*)lv_event_get_target(e);
        save_setting(NVS_KEY_BRIGHTNESS, (uint8_t)lv_slider_get_value(s));
    });

    auto pf_lbl = lv_label_create(root_);
    lv_label_set_text(pf_lbl, "Optimize For");
    lv_obj_set_width(pf_lbl, LV_PCT(100));
    lv_obj_set_style_margin_top(pf_lbl, 20, 0);

    auto pf_dd = lv_dropdown_create(root_);
    lv_obj_set_width(pf_dd, LV_PCT(100));
    lv_dropdown_set_options_static(pf_dd, "Image Quality\nFramerate");
    lv_dropdown_set_selected(pf_dd,
        pf_port::display_pixel_format() == pf_port::PixelFormat::RGB565 ? 1 : 0);
    // The pixel format is fixed by pf_port::init at boot — switching it requires
    // re-allocating framebuffers and re-configuring JPEG/PPA. Save and reboot
    // rather than tear the pipeline down at runtime.
    lv_obj_add_event_fn(pf_dd, LV_EVENT_VALUE_CHANGED, [](lv_event_t *e){
        auto d = (lv_obj_t*)lv_event_get_target(e);
        save_setting(NVS_KEY_PIX_FMT, (uint8_t)lv_dropdown_get_selected(d));
        bsp_tab5_restart();
    });

    pf_port::display_set_brightness(lv_slider_get_value(brightness_slider_));
}

void PreviewScreen::onEnter() {
    if (s_jpeg_queue != nullptr) {
        return; // tasks already running
    }

    for (int i = 0; i < JPEG_BUFFER_NUM; i++) {
        s_jpeg_buffers[i] = (uint8_t *)pf_port::psram_malloc(JPEG_BUFFER_SIZE);
        if (s_jpeg_buffers[i] == nullptr) {
            ESP_LOGE(TAG, "failed to allocate jpeg buffer %d", i);
            return;
        }
    }

    s_jpeg_queue = xQueueCreate(1, sizeof(JpegFrame));
    if (s_jpeg_queue == nullptr) {
        ESP_LOGE(TAG, "failed to create jpeg queue");
        return;
    }

    // Decoder runs at higher priority: it spends most of its time blocked on
    // the queue, so the renderer gets the core to keep draining USB.
    xTaskCreatePinnedToCore(decoder_task, "jpeg_decoder", 8192, nullptr, 15, nullptr, 0);
    xTaskCreatePinnedToCore(renderer_task, "jpeg_renderer", 4096, nullptr, 4, nullptr, 0);
}
