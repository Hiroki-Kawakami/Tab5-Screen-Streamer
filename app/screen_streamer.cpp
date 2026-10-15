/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "screen_streamer.hpp"
#include <cstdlib>
#include <cstring>
#include "bsp.h"
#include "display_manager.hpp"
#include "lvgl.hpp"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "display/panel.hpp"
#include "harness.h"
#include "screen_manager.hpp"
#include "screens/home_screen.hpp"
#include "screens/pc_app_screen.hpp"
#include "screens/pc_app_stream_screen.hpp"
#include "screens/usb_test_screen.hpp"
#include "sdkconfig.h"
#include "settings.hpp"
#include "ui_orientation.hpp"
#include "usb/usb.hpp"

static const char *TAG = "screen_streamer";

static constexpr size_t kSharedSramBytes = 245760;

// The main display's draw buffers, which the panel takes as JPEG and LZ4
// strip buffers while the display is hidden. Internal RAM: .bss stays out of
// PSRAM.
alignas(64) static uint8_t s_shared_sram[kSharedSramBytes];

static lv_display_t *s_display;

static esp_err_t display_init() {
    lvgl_port_cfg_t config = {
        .task_priority     = 4,
        .task_stack        = 8192,
        .task_affinity     = 1,
        .task_max_sleep_ms = 500,
        .task_stack_caps   = MALLOC_CAP_INTERNAL | MALLOC_CAP_DEFAULT,
        .timer_period_ms   = 5,
    };
    esp_err_t err = lvgl_port_init(&config);
    if (err != ESP_OK) return err;

    DisplayManagerConfig display_config = {};
    display_config.render_mode = DisplayRenderMode::Partial;
    display_config.buffer.buffers[0] = s_shared_sram;
    display_config.buffer.buffers[1] = s_shared_sram + kSharedSramBytes / 2;
    display_config.buffer.buffer_size = kSharedSramBytes / 2;
    return display_manager.create_display(display_config, &s_display);
}

#ifdef CONFIG_HARNESS
static bool harness_mode(int argc, const char *const *argv, void *) {
    if (argc != 2) return false;
    std::shared_ptr<Screen> screen;
    if (strcmp(argv[1], "home") == 0) {
        screen = std::make_shared<HomeScreen>();
    } else if (strcmp(argv[1], "pc-app") == 0) {
        screen = std::make_shared<PcAppScreen>();
    } else if (strcmp(argv[1], "usb") == 0) {
        screen = std::make_shared<PcAppStreamScreen>(true, true);
    } else if (strcmp(argv[1], "usb-test") == 0) {
        screen = std::make_shared<UsbTestScreen>();
    } else {
        return false;
    }
    lv_async_call([screen] { screen_manager.load(screen); });
    return true;
}

static bool harness_format(int argc, const char *const *argv, void *) {
    if (argc != 2) return false;
    bsp_pixel_format_t format;
    if (strcmp(argv[1], "rgb565") == 0) {
        format = BSP_PIXEL_FORMAT_RGB565;
    } else if (strcmp(argv[1], "rgb888") == 0) {
        format = BSP_PIXEL_FORMAT_RGB888;
    } else {
        return false;
    }
    return panel_set_format(format) == ESP_OK;
}

static bool harness_clip(int argc, const char *const *argv, void *) {
    if (argc == 2 && strcmp(argv[1], "off") == 0) return panel_set_clip(nullptr) == ESP_OK;
    if (argc != 5) return false;
    const bsp_rect_t clip = {{atoi(argv[1]), atoi(argv[2])}, {atoi(argv[3]), atoi(argv[4])}};
    return panel_set_clip(&clip) == ESP_OK;
}
#endif

void app_entry() {
    settings_init();

    bsp_config_t bsp_config = {};
    bsp_config.display.pixel_format = settings_display_pixel_format();
    bsp_config.display.fb_num = 3;
    bsp_config.keyboard.detect = BSP_KEYBOARD_DETECT_AUTO;
    bsp_config.dispatch.task_priority = 6;
    bsp_config.dispatch.task_affinity = 1;
    bsp_init(&bsp_config);
    bsp_audio_set_eq_enabled(settings_equalizer_enabled());

    esp_err_t err = display_init();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "display init: %s", esp_err_to_name(err));
        return;
    }
    err = panel_init(s_display, s_shared_sram, kSharedSramBytes);
    if (err != ESP_OK) ESP_LOGE(TAG, "panel init: %s", esp_err_to_name(err));
    err = usb_init();
    if (err != ESP_OK) ESP_LOGE(TAG, "usb init: %s", esp_err_to_name(err));
#ifdef CONFIG_HARNESS
    harness_register("mode", harness_mode, nullptr);
    harness_register("format", harness_format, nullptr);
    harness_register("clip", harness_clip, nullptr);
#endif

    lv_async_call([] {
        ui_orientation_start(s_display, settings_rotation_locked(), settings_rotation());
        if (settings_last_mode() == AppMode::PcApp) {
            screen_manager.load(PcAppScreen::stream_screen());
        } else {
            screen_manager.load(std::make_shared<HomeScreen>());
        }
        bsp_display_set_brightness(settings_display_brightness());
    });
}
