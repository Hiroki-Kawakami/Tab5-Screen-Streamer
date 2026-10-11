/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb.hpp"
#include <memory>
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "protocols/tab5/tab5_profile.hpp"
#include "usb_profile.hpp"
#include "usb_test_profile.hpp"

static const char *TAG = "usb";

static constexpr int kTaskPriority = 5;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 4;

struct Request {
    bool start;
    UsbProfileId id;
    UsbOptions options;
};

static QueueHandle_t s_requests;
static std::unique_ptr<usb_device::Device> s_device;
static std::unique_ptr<UsbProfile> s_profile;

static std::unique_ptr<UsbProfile> make_profile(UsbProfileId id, const UsbOptions &options) {
    switch (id) {
        case UsbProfileId::Tab5: return std::make_unique<Tab5Profile>(options.audio, options.status);
        case UsbProfileId::Test: return std::make_unique<UsbTestProfile>();
    }
    return nullptr;
}

static void start(UsbProfileId id, const UsbOptions &options) {
    s_profile = make_profile(id, options);
    usb_device::DeviceConfig config;
    config.info = s_profile->info();
    s_device = std::make_unique<usb_device::Device>(std::move(config));
    esp_err_t err = s_profile->add_functions(*s_device);
    if (err == ESP_OK) err = s_device->start();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "usb device: %s", esp_err_to_name(err));
        s_device.reset();
        s_profile.reset();
    }
}

static void usb_task(void *) {
    Request *request;
    while (true) {
        xQueueReceive(s_requests, &request, portMAX_DELAY);
        s_device.reset();
        s_profile.reset();
        if (request->start) start(request->id, request->options);
        delete request;
    }
}

esp_err_t usb_init() {
    s_requests = xQueueCreate(kQueueLength, sizeof(Request *));
    if (!s_requests) return ESP_ERR_NO_MEM;
    if (xTaskCreate(usb_task, "usb", kTaskStackBytes, nullptr, kTaskPriority, nullptr) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void usb_start(UsbProfileId id, UsbOptions options) {
    auto *request = new Request{true, id, std::move(options)};
    xQueueSend(s_requests, &request, portMAX_DELAY);
}

void usb_stop() {
    auto *request = new Request{false, UsbProfileId::Tab5, {}};
    xQueueSend(s_requests, &request, portMAX_DELAY);
}
