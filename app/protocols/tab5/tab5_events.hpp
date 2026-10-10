/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include <mutex>
#include "protocol.hpp"
#include "usb_device.hpp"

/* Replies and events on the control IN endpoint. Callable from any task and
 * never blocks on the host: when the queue is full, a message replaces the
 * oldest queued one of its type. */
class Tab5Events {
public:
    static constexpr uint32_t kTransferBytes = 512;

    ~Tab5Events();
    esp_err_t init();
    void start(usb_device::Endpoint in);
    void stop();

    void send(tab5::Type type, uint32_t seq, const void *payload, uint16_t bytes,
              uint32_t epoch = 0);
    void error(tab5::Channel channel, tab5::ErrorCode code, uint32_t seq);

private:
    static constexpr int kQueueLength = 16;
    static constexpr uint16_t kMaxMessageBytes = 256;

    struct Message {
        tab5::Type type;
        uint16_t length;
        uint8_t bytes[kMaxMessageBytes];
    };

    static void in_done(usb_device::Transfer *transfer);
    void submit_locked();

    std::mutex lock_;
    usb_device::Endpoint in_;
    usb_device::Transfer transfer_ = {};
    Message queue_[kQueueLength] = {};
    int head_ = 0;
    int count_ = 0;
    bool active_ = false;
    bool busy_ = false;
};
