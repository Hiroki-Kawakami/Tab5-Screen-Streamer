/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb.hpp"
#include <cstdio>
#include <memory>
#include "esp_log.h"
#include "esp_mac.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "usb_profile.hpp"

static const char *TAG = "usb";

static constexpr int kTaskPriority = 5;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 4;

struct Request {
    std::shared_ptr<UsbProfile> profile;
};

static QueueHandle_t s_requests;
static std::unique_ptr<usb_device::Device> s_device;
static std::shared_ptr<UsbProfile> s_profile;

static void start(std::shared_ptr<UsbProfile> profile) {
    s_profile = std::move(profile);
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
        if (request->profile) start(std::move(request->profile));
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

void usb_start(std::shared_ptr<UsbProfile> profile) {
    auto *request = new Request{std::move(profile)};
    xQueueSend(s_requests, &request, portMAX_DELAY);
}

void usb_stop() {
    auto *request = new Request{};
    xQueueSend(s_requests, &request, portMAX_DELAY);
}

std::string usb_serial() {
    uint8_t mac[6];
    if (esp_efuse_mac_get_default(mac) != ESP_OK) return {};
    char serial[13];
    snprintf(serial, sizeof(serial), "%02X%02X%02X%02X%02X%02X", mac[0], mac[1], mac[2], mac[3],
             mac[4], mac[5]);
    return serial;
}
