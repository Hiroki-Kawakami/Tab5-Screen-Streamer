/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "screen_streamer.hpp"
#include "bsp.h"
#include "display_manager.hpp"
#include "lvgl.hpp"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "screens/home_screen.hpp"
#include "usb_device.hpp"

static const char *TAG = "screen_streamer";

static lv_display_t *s_display;
static std::unique_ptr<usb_device::Device> s_usb;

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

static void usb_init() {
    usb_device::DeviceConfig config;
    config.info.vendor_id = 0xf055;
    config.info.product_id = 0x1118;
    config.info.bcd_device = 0x0300;
    config.info.manufacturer = "M5Stack";
    config.info.product = "Tab5 Screen Streamer";
    s_usb = std::make_unique<usb_device::Device>(std::move(config));
    esp_err_t err = s_usb->start();
    if (err != ESP_OK) ESP_LOGW(TAG, "usb device: %s", esp_err_to_name(err));
}

void app_entry() {
    bsp_config_t bsp_config = {};
    bsp_config.display.pixel_format = BSP_PIXEL_FORMAT_RGB565;
    bsp_config.dispatch.task_priority = 6;
    bsp_config.dispatch.task_affinity = 1;
    bsp_init(&bsp_config);

    esp_err_t err = display_init();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "display init: %s", esp_err_to_name(err));
        return;
    }
    usb_init();

    lv_async_call([] {
        screen_manager.load(std::make_shared<HomeScreen>());
        bsp_display_set_brightness(80);
    });
}
