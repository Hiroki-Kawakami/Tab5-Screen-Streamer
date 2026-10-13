/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_events.hpp"
#include <cstring>
#include "esp_heap_caps.h"
#include "esp_log.h"

static const char *TAG = "tab5_events";

Tab5Events::~Tab5Events() {
    heap_caps_free(transfer_.buffer);
}

esp_err_t Tab5Events::init() {
    transfer_.buffer = static_cast<uint8_t *>(heap_caps_aligned_alloc(4, kTransferBytes,
                                                                      MALLOC_CAP_DMA));
    if (!transfer_.buffer) return ESP_ERR_NO_MEM;
    transfer_.callback = in_done;
    transfer_.context = this;
    transfer_.zlp = true;
    return ESP_OK;
}

void Tab5Events::start(usb_device::Endpoint in) {
    std::lock_guard<std::mutex> guard(lock_);
    in_ = in;
    active_ = true;
    head_ = 0;
    count_ = 0;
}

void Tab5Events::stop() {
    std::lock_guard<std::mutex> guard(lock_);
    active_ = false;
    count_ = 0;
}

void Tab5Events::clear() {
    std::lock_guard<std::mutex> guard(lock_);
    count_ = 0;
}

void Tab5Events::send(tab5::Type type, uint32_t seq, const void *payload, uint16_t bytes,
                      uint32_t epoch) {
    const uint16_t length = sizeof(tab5::MsgHeader) + bytes;
    if (length > kMaxMessageBytes) {
        ESP_LOGE(TAG, "message 0x%02x of %u bytes", (unsigned)type, (unsigned)length);
        return;
    }
    std::lock_guard<std::mutex> guard(lock_);
    if (!active_) return;

    Message *message = nullptr;
    if (count_ < kQueueLength) {
        message = &queue_[(head_ + count_++) % kQueueLength];
    } else {
        for (int i = 0; i < count_ && !message; i++) {
            Message &queued = queue_[(head_ + i) % kQueueLength];
            if (queued.type == type) message = &queued;
        }
        if (!message) {
            ESP_LOGW(TAG, "queue full, 0x%02x dropped", (unsigned)type);
            return;
        }
    }
    const tab5::MsgHeader header = {tab5::kMagic, static_cast<uint8_t>(type), 0, bytes, seq,
                                    epoch};
    message->type = type;
    message->length = length;
    memcpy(message->bytes, &header, sizeof(header));
    if (bytes) memcpy(message->bytes + sizeof(header), payload, bytes);
    if (!busy_) submit_locked();
}

void Tab5Events::error(tab5::Channel channel, tab5::ErrorCode code, uint32_t seq) {
    const tab5::ErrorEvent event = {static_cast<uint8_t>(channel), static_cast<uint8_t>(code), 0};
    send(tab5::Type::Error, seq, &event, sizeof(event));
}

void Tab5Events::submit_locked() {
    uint32_t length = 0;
    while (count_ && length + queue_[head_].length <= kTransferBytes) {
        memcpy(transfer_.buffer + length, queue_[head_].bytes, queue_[head_].length);
        length += queue_[head_].length;
        head_ = (head_ + 1) % kQueueLength;
        count_--;
    }
    if (!length) return;
    transfer_.length = length;
    esp_err_t err = in_.submit(&transfer_);
    busy_ = err == ESP_OK;
    if (err != ESP_OK) ESP_LOGW(TAG, "submit: %s", esp_err_to_name(err));
}

void Tab5Events::in_done(usb_device::Transfer *transfer) {
    auto *self = static_cast<Tab5Events *>(transfer->context);
    std::lock_guard<std::mutex> guard(self->lock_);
    self->busy_ = false;
    if (transfer->status != usb_device::TransferStatus::Completed) return;
    if (self->active_) self->submit_locked();
}
