/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "usb.hpp"
#include <cstring>
#include <memory>
#include <mutex>
#include "esp_log.h"
#include "harness.h"
#include "stream/legacy_stream_profile.hpp"
#include "usb_profile.hpp"
#include "usb_test_profile.hpp"

static const char *TAG = "usb";

static std::mutex s_lock;
static std::unique_ptr<usb_device::Device> s_device;
static std::unique_ptr<UsbProfile> s_profile;

static std::unique_ptr<UsbProfile> make_profile(UsbProfileId id) {
    switch (id) {
        case UsbProfileId::LegacyStream: return std::make_unique<LegacyStreamProfile>();
        case UsbProfileId::Test:         return std::make_unique<UsbTestProfile>();
    }
    return nullptr;
}

esp_err_t usb_switch_profile(UsbProfileId id) {
    std::lock_guard<std::mutex> guard(s_lock);
    s_device.reset();
    s_profile = make_profile(id);

    usb_device::DeviceConfig config;
    config.info = s_profile->info();
    s_device = std::make_unique<usb_device::Device>(std::move(config));
    esp_err_t err = s_profile->add_functions(*s_device);
    if (err == ESP_OK) err = s_device->start();
    if (err != ESP_OK) ESP_LOGW(TAG, "usb device: %s", esp_err_to_name(err));
    return err;
}

#ifdef CONFIG_HARNESS
static bool harness_command(int argc, const char *const *argv, void *) {
    if (argc != 2) return false;
    UsbProfileId id;
    if (strcmp(argv[1], "legacy") == 0) {
        id = UsbProfileId::LegacyStream;
    } else if (strcmp(argv[1], "test") == 0) {
        id = UsbProfileId::Test;
    } else {
        return false;
    }
    return usb_switch_profile(id) == ESP_OK;
}
#endif

void usb_init() {
#ifdef CONFIG_HARNESS
    harness_register("usb-profile", harness_command, nullptr);
#endif
    usb_switch_profile(UsbProfileId::LegacyStream);
}
