#include <cstdio>
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "bsp_tab5.h"
#include "streamer.hpp"
#include "platform_port.hpp"
#include "streamer_usb.h"

static const char *TAG = "main";

namespace pf_port {

static PixelFormat s_pixel_format = PixelFormat::RGB888;

PixelFormat display_pixel_format() {
    return s_pixel_format;
}

void init(int fb_num, PixelFormat pixel_format) {
    s_pixel_format = pixel_format;
    bsp_tab5_config_t bsp_config = {};
    bsp_config.display.fb_num = fb_num;
    bsp_config.display.pixel_format = (pixel_format == PixelFormat::RGB888)
        ? BSP_PIXEL_FORMAT_RGB888
        : BSP_PIXEL_FORMAT_RGB565;
    bsp_config.usb.usb5v_en = false;
    bsp_config.audio.eq.enable = true;
    bsp_config.audio.eq.max_stages = 8;
    bsp_config.audio.speaker_mode = BSP_SPEAKER_MODE_AUTO;
    ESP_ERROR_CHECK(bsp_tab5_init(&bsp_config));
    // EQ coefficients themselves are owned by PreviewScreen (speaker vs HP
    // presets, swapped by the HP-detect callback).

    lvgl_port_cfg_t config = {
        .task_priority = 4,
        .task_stack = 7168,
        .task_affinity = 1,
        .task_max_sleep_ms = 500,
        .task_stack_caps = MALLOC_CAP_INTERNAL | MALLOC_CAP_DEFAULT,
        .timer_period_ms = 5,
    };
    esp_err_t err = lvgl_port_init(&config);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Failed to start LVGL: %s", esp_err_to_name(err));
        assert(0);
    }

    // Initialize streamer_usb
    streamer_usb_init();

    // Route UAC speaker audio (48 kHz / stereo / 16-bit from the PC) to the
    // ES8388 codec. The codec is already opened (48k/16/2) and unmuted by
    // bsp_tab5_init(), so we only set the playback level here. Only the left
    // channel is wired on the Tab5, so downmix to mono. Start at a sensible
    // volume in case the host never sets one.
    bsp_tab5_audio_set_mono_mix(true);
    bsp_tab5_audio_set_volume(80);
    streamer_usb_audio_set_callbacks(
        [](const void *pcm, size_t len, void *) {
            bsp_tab5_audio_write(const_cast<void *>(pcm), len);
        },
        [](int volume, void *) {
            bsp_tab5_audio_set_volume(volume);
        },
        [](bool mute, void *) {
            bsp_tab5_audio_set_mute(mute);
        },
        nullptr);

    xTaskCreatePinnedToCore([](void*){
        streamer_usb_task();
    }, "streamer_usb", 4096, NULL, 5, NULL, 0);
}

void display_set_brightness(int value) {
    bsp_tab5_display_set_brightness(value);
}

void *display_get_frame_buffer(int fb_index) {
    return bsp_tab5_display_get_frame_buffer(fb_index);
}

void display_flush(int fb_index) {
    bsp_tab5_display_flush(fb_index);
}

std::optional<std::tuple<int, int>> touch_get_point() {
    esp_lcd_touch_point_data_t touch;
    int touch_num = bsp_tab5_touch_read(&touch, 1);
    if (touch_num > 0) {
        return std::make_tuple(touch.x, touch.y);
    }
    return std::nullopt;
}

void *psram_malloc(size_t size) {
    return heap_caps_malloc(size, MALLOC_CAP_SPIRAM);
}
void *psram_malloc_dma(size_t size) {
    return heap_caps_malloc(size, MALLOC_CAP_SPIRAM | MALLOC_CAP_DMA);
}

}

extern "C" void app_main() {
    streamer_app();
}
