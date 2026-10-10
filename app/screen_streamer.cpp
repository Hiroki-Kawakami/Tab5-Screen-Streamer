/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "screen_streamer.hpp"
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
#include "screens/usb_mode_screen.hpp"
#include "sdkconfig.h"
#include "usb/usb.hpp"

static const char *TAG = "screen_streamer";

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

    const bsp_size_t panel = bsp_display_get_size();
    const size_t long_edge = panel.width > panel.height ? panel.width : panel.height;
    const size_t buffer_size = long_edge * 24 *
        bsp_pixel_format_bytes(bsp_display_get_pixel_format());

    DisplayManagerConfig display_config = {};
    display_config.render_mode = DisplayRenderMode::Partial;
    display_config.buffer.buffer_size = buffer_size;
    for (auto &buffer : display_config.buffer.buffers) {
        buffer = heap_caps_aligned_alloc(64, buffer_size, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
        if (!buffer) {
            ESP_LOGE(TAG, "no internal memory for a %u byte draw buffer", (unsigned)buffer_size);
            return ESP_ERR_NO_MEM;
        }
    }
    return display_manager.create_display(display_config, &s_display);
}

#ifdef CONFIG_HARNESS
static bool harness_mode(int argc, const char *const *argv, void *) {
    if (argc != 2) return false;
    std::shared_ptr<Screen> screen;
    if (strcmp(argv[1], "home") == 0) {
        screen = std::make_shared<HomeScreen>();
    } else if (strcmp(argv[1], "usb") == 0) {
        screen = std::make_shared<UsbModeScreen>(UsbProfileId::Tab5, "USB Stream");
    } else if (strcmp(argv[1], "usb-test") == 0) {
        screen = std::make_shared<UsbModeScreen>(UsbProfileId::Test, "USB Test");
    } else {
        return false;
    }
    lv_async_call([screen] { screen_manager.load(screen); });
    return true;
}
#endif

void app_entry() {
    bsp_config_t bsp_config = {};
    bsp_config.display.pixel_format = BSP_PIXEL_FORMAT_RGB565;
    bsp_config.display.fb_num = 3;
    bsp_config.dispatch.task_priority = 6;
    bsp_config.dispatch.task_affinity = 1;
    bsp_init(&bsp_config);

    esp_err_t err = display_init();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "display init: %s", esp_err_to_name(err));
        return;
    }
    err = panel_init(s_display);
    if (err != ESP_OK) ESP_LOGE(TAG, "panel init: %s", esp_err_to_name(err));
    err = usb_init();
    if (err != ESP_OK) ESP_LOGE(TAG, "usb init: %s", esp_err_to_name(err));
#ifdef CONFIG_HARNESS
    harness_register("mode", harness_mode, nullptr);
#endif

    lv_async_call([] {
        screen_manager.load(std::make_shared<HomeScreen>());
        bsp_display_set_brightness(80);
    });
}
