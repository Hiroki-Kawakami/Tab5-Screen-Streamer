#include "preview_screen.hpp"
#include "platform_port.hpp"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "streamer_usb.h"

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

struct JpegFrame {
    const uint8_t *data;  // start of the JPEG payload (past the 4-byte header)
    size_t size;          // payload length in bytes
};

uint8_t *s_jpeg_buffers[JPEG_BUFFER_NUM] = {};
// Capacity-1 queue used with xQueueOverwrite: the renderer always replaces the
// pending frame with the latest, so the decoder never falls behind on stale
// frames.
QueueHandle_t s_jpeg_queue = nullptr;

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

// Reads framed JPEGs from USB. Wire format (see send_image.py):
//   [uint32 LE total-size-including-header][JPEG bytes...]
void renderer_task(void *) {
    int idx = 0;
    while (true) {
        if (!streamer_usb_mounted()) {
            vTaskDelay(pdMS_TO_TICKS(100));
            continue;
        }

        uint8_t *buf = s_jpeg_buffers[idx];

        // 4-byte little-endian header: total frame size including itself.
        if (!read_exact(buf, 4)) {
            continue;
        }
        uint32_t frame_total = (uint32_t)buf[0] | ((uint32_t)buf[1] << 8) |
                               ((uint32_t)buf[2] << 16) | ((uint32_t)buf[3] << 24);
        if (frame_total <= 4 || frame_total > JPEG_BUFFER_SIZE) {
            ESP_LOGW(TAG, "invalid frame size %u, resyncing", (unsigned)frame_total);
            continue;
        }

        // Remaining bytes are the JPEG payload.
        if (!read_exact(buf + 4, frame_total - 4)) {
            continue;
        }

        JpegFrame frame{buf + 4, frame_total - 4};
        xQueueOverwrite(s_jpeg_queue, &frame);
        idx = (idx + 1) % JPEG_BUFFER_NUM;
    }
}

// Decodes the latest received JPEG straight into the next panel framebuffer
// and flips to it.
void decoder_task(void *) {
    pf_port::PixelFormat pf = pf_port::display_pixel_format();
    size_t fb_size = (size_t)DISPLAY_WIDTH * DISPLAY_HEIGHT * pf_port::bytes_per_pixel(pf);

    pf_port::JpegDecoder decoder;
    decoder.setOutputFormat(pf);

    int fb_index = 0;
    int frame_count = 0;
    int64_t window_start = esp_timer_get_time();

    JpegFrame frame;
    while (true) {
        if (xQueueReceive(s_jpeg_queue, &frame, portMAX_DELAY) != pdTRUE) {
            continue;
        }

        int next = (fb_index + 1) % FRAME_BUFFER_NUM;
        void *fb = pf_port::display_get_frame_buffer(next);
        if (decoder.decode(frame.data, frame.size, fb, fb_size, nullptr) != pf_port::Error::Ok) {
            ESP_LOGW(TAG, "jpeg decode failed (%u bytes)", (unsigned)frame.size);
            continue;
        }

        pf_port::display_flush(next);
        fb_index = next;

        frame_count++;
        int64_t now = esp_timer_get_time();
        if (now - window_start >= 1000000) {
            ESP_LOGI(TAG, "%d fps", frame_count);
            frame_count = 0;
            window_start = now;
        }
    }
}

} // namespace

PreviewScreen::PreviewScreen() {}

void PreviewScreen::build() {}

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
