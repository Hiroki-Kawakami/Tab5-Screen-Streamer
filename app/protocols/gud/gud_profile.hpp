/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * A Generic USB Display: one connector showing the panel upright at the UI
 * rotation.
 */

#pragma once
#include <atomic>
#include <functional>
#include <memory>
#include <mutex>
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "gud.hpp"
#include "gud_video.hpp"
#include "usb/usb_profile.hpp"
#include "usb_device_vendor.hpp"

class GudProfile : public UsbProfile {
public:
    /* Run from the USB tasks. `brightness` gets each brightness the host
     * sets, 0-100. */
    struct Listeners {
        UsbStatusListener status;
        std::function<void(bool on)> display_power;
        std::function<void(int percent)> brightness;
    };

    GudProfile(Listeners listeners, bsp_rotation_t rotation, int brightness);
    ~GudProfile() override;
    /* Before the profile is started on USB. The host is offered the panel's
     * pixel format at this point only. */
    esp_err_t start();
    usb_device::DeviceInfo info() const override;
    esp_err_t add_functions(usb_device::Device &device) override;
    /* The host is told to probe the modes again when the orientation flips;
     * it polls for that every 10 seconds. */
    void rotate(bsp_rotation_t rotation);

private:
    static constexpr int kSlots = 2;

    struct Slot {
        usb_device::Transfer transfer;
        GudProfile *owner;
        gud::SetBuffer buffer;
    };

    /* Rows of an uncompressed buffer, received into a panel strip. */
    struct Strip {
        usb_device::Transfer transfer;
        GudProfile *owner;
        gud::SetBuffer buffer;
        uint32_t row;
        uint32_t rows;
        uint32_t bytes;
        bool last;
        bool queued;
    };

    bool control(const usb_device::ControlRequest &request, uint8_t *data, size_t *bytes);
    bool get(gud::Request request, uint16_t value, uint8_t *data, size_t *bytes);
    bool set(gud::Request request, uint16_t value, const uint8_t *data, size_t bytes);
    bool check_state(const uint8_t *data, size_t bytes);
    bool set_buffer(const uint8_t *data, size_t bytes);
    void configured();
    void unconfigured();
    void streaming(bool streaming);
    void report();
    static void receive_done(usb_device::Transfer *transfer);
    static void release(void *context);
    uint32_t packet_bytes() const;
    uint32_t strip_rows(uint32_t row_bytes) const;
    void receive_rows(const gud::SetBuffer &buffer, uint32_t rows);
    void stop_rows();
    void submit_rows();
    static void rows_done(usb_device::Transfer *transfer);
    static void release_rows(void *context);

    Listeners listeners_;
    usb_device::Device *device_ = nullptr;
    std::unique_ptr<GudVideo> video_;
    std::shared_ptr<usb_device::Vendor> vendor_;
    Slot *slots_[kSlots] = {};
    QueueHandle_t free_slots_ = nullptr;
    Slot *armed_ = nullptr;
    Strip strips_[2] = {};
    std::mutex strip_lock_;
    uint8_t free_strips_ = 0;
    gud::SetBuffer rows_buffer_ = {};
    uint32_t rows_per_strip_ = 0;
    uint32_t next_row_ = 0;
    bool receiving_rows_ = false;
    uint32_t buffer_bytes_ = 0;
    bsp_pixel_format_t format_ = BSP_PIXEL_FORMAT_RGB565;
    uint8_t wire_format_ = gud::kPixelFormatRgb565;
    gud::Status status_ = gud::Status::Ok;
    std::atomic<bsp_rotation_t> rotation_;
    std::atomic<bool> changed_ = false;
    bsp_size_t mode_ = {};
    bsp_size_t checked_mode_ = {};
    int brightness_;
    int checked_brightness_;
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    bool configured_ = false;
    bool streaming_ = false;
    UsbStatus reported_ = UsbStatus::Unconfigured;
    // DIAG
    int64_t diag_start_us_ = 0;
    int64_t diag_submit_us_ = 0;
    int64_t diag_done_us_ = 0;
    int64_t diag_usb_us_ = 0;
    int64_t diag_host_us_ = 0;
    int64_t diag_wait_us_ = 0;
    uint64_t diag_bytes_ = 0;
    uint32_t diag_transfers_ = 0;
    void diag_log(int64_t now);
};
