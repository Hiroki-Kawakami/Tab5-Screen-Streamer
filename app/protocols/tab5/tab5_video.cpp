/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_video.hpp"
#include "display/panel.hpp"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/task.h"
#include "tab5_events.hpp"

static const char *TAG = "tab5_video";

static constexpr int kTaskPriority = 6;
static constexpr int kTaskCore = 0;
static constexpr uint32_t kTaskStackBytes = 4096;
static constexpr int kQueueLength = 8;
static constexpr int64_t kStatsPeriodUs = 5 * 1000 * 1000;

using tab5::Channel;
using tab5::ErrorCode;

Tab5Video::~Tab5Video() {
    if (!queue_) return;
    const Item quit = {Op::Quit, nullptr, 0, 0, nullptr, nullptr};
    xQueueSend(queue_, &quit, portMAX_DELAY);
    xSemaphoreTake(exited_, portMAX_DELAY);
    vSemaphoreDelete(exited_);
    vSemaphoreDelete(synced_);
    vQueueDelete(queue_);
}

esp_err_t Tab5Video::start() {
    queue_ = xQueueCreate(kQueueLength, sizeof(Item));
    exited_ = xSemaphoreCreateBinary();
    synced_ = xSemaphoreCreateBinary();
    if (!queue_ || !exited_ || !synced_) return ESP_ERR_NO_MEM;
    if (xTaskCreatePinnedToCore(task, "tab5_video", kTaskStackBytes, this, kTaskPriority,
                                nullptr, kTaskCore) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void Tab5Video::submit(const uint8_t *data, uint32_t length, uint32_t session, Done done,
                       void *context) {
    const Item item = {Op::Transfer, data, length, session, done, context};
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void Tab5Video::end() {
    const Item item = {Op::End, nullptr, 0, 0, nullptr, nullptr};
    xQueueSend(queue_, &item, portMAX_DELAY);
}

void Tab5Video::sync() {
    const Item item = {Op::Sync, nullptr, 0, 0, nullptr, nullptr};
    xQueueSend(queue_, &item, portMAX_DELAY);
    xSemaphoreTake(synced_, portMAX_DELAY);
}

void Tab5Video::task(void *arg) {
    auto *self = static_cast<Tab5Video *>(arg);
    Item item;
    do {
        xQueueReceive(self->queue_, &item, portMAX_DELAY);
        self->run(item);
    } while (item.op != Op::Quit);
    xSemaphoreGive(self->exited_);
    vTaskDelete(nullptr);
}

tab5::PixelFormat tab5_pixel_format(bsp_pixel_format_t format) {
    return format == BSP_PIXEL_FORMAT_RGB888 ? tab5::PixelFormat::Rgb888
                                             : tab5::PixelFormat::Rgb565;
}

void Tab5Video::format_changed(bsp_pixel_format_t format) {
    const uint32_t epoch = ++epoch_;
    const tab5::ByteEvent event = {static_cast<uint8_t>(tab5_pixel_format(format)), {}};
    events_.send(tab5::Type::Format, 0, &event, sizeof(event), epoch);
}

void Tab5Video::run(const Item &item) {
    if (item.op == Op::Sync) {
        xSemaphoreGive(synced_);
        return;
    }
    if (item.op == Op::Transfer && item.session != session_) {
        item.done(item.context);
        return;
    }
    if (item.op == Op::Transfer && !panel_acquired()) {
        panel_acquire();
        ESP_LOGI(TAG, "stream started");
        streaming_(true);
    }
    panel_lock();
    execute_item(item);
    panel_unlock();
    if (item.op != Op::Transfer && panel_acquired()) {
        panel_release();
        ESP_LOGI(TAG, "stream stopped");
        streaming_(false);
    }
}

void Tab5Video::execute_item(const Item &item) {
    if (item.op != Op::Transfer) {
        stats_start_us_ = 0;
        stats_bytes_ = 0;
        stats_swaps_ = 0;
        stats_jpegs_ = 0;
        stats_jpeg_us_ = 0;
        stats_busy_us_ = 0;
        return;
    }

    const int64_t start = esp_timer_get_time();
    uint32_t bad_seq = 0;
    const bool ok = tab5::for_each_message(
        item.data, item.length, &bad_seq,
        [this](const tab5::MsgHeader &header, const uint8_t *payload) {
            execute(header, payload);
        });
    if (!ok) events_.error(Channel::Video, ErrorCode::Malformed, bad_seq);
    item.done(item.context);
    stats_busy_us_ += esp_timer_get_time() - start;
    count(item.length);
}

void Tab5Video::count(uint32_t bytes) {
    stats_bytes_ += bytes;
    const int64_t now = esp_timer_get_time();
    if (!stats_start_us_) stats_start_us_ = now;
    if (now - stats_start_us_ < kStatsPeriodUs) return;
    const double seconds = (now - stats_start_us_) / 1e6;
    ESP_LOGI(TAG, "%.1f swaps/s, %.1f MB/s, jpeg %.1f ms, busy %.0f%%", stats_swaps_ / seconds,
             stats_bytes_ / seconds / 1e6,
             stats_jpegs_ ? stats_jpeg_us_ / 1000.0 / stats_jpegs_ : 0.0,
             stats_busy_us_ / 1e4 / seconds);
    stats_start_us_ = now;
    stats_bytes_ = 0;
    stats_swaps_ = 0;
    stats_jpegs_ = 0;
    stats_jpeg_us_ = 0;
    stats_busy_us_ = 0;
}

void Tab5Video::execute(const tab5::MsgHeader &header, const uint8_t *payload) {
    switch (static_cast<tab5::Type>(header.type)) {
        case tab5::Type::DrawDirect:
            draw_direct(header, payload);
            return;
        case tab5::Type::Fill:
            fill(header, payload);
            return;
        case tab5::Type::Swap:
            swap(header);
            return;
        case tab5::Type::DrawBlit:
            draw_blit(header, payload);
            return;
        case tab5::Type::Clear:
            clear(header);
            return;
        case tab5::Type::DisplayPower:
            display_power(header, payload);
            return;
        default:
            events_.error(Channel::Video, ErrorCode::Malformed, header.seq);
            return;
    }
}

void Tab5Video::draw_direct(const tab5::MsgHeader &header, const uint8_t *payload) {
    const auto fail = [&](ErrorCode code) { events_.error(Channel::Video, code, header.seq); };
    tab5::DrawDirect draw;
    if (header.length < sizeof(draw)) return fail(ErrorCode::Malformed);
    memcpy(&draw, payload, sizeof(draw));
    if (draw.target > static_cast<uint8_t>(tab5::Target::Back) || draw.y % kDirectAlign ||
        draw.height % kDirectAlign) {
        return fail(ErrorCode::Malformed);
    }
    const bsp_size_t size = panel_size();
    if (draw.height == 0 || draw.y + draw.height > size.height) {
        return fail(ErrorCode::OutOfBounds);
    }

    const uint8_t *data = payload + sizeof(draw);
    const uint32_t bytes = header.length - sizeof(draw);
    const auto buffer = static_cast<PanelBuffer>(draw.target);
    switch (static_cast<tab5::Encoding>(draw.encoding)) {
        case tab5::Encoding::Jpeg: {
            const int64_t start = esp_timer_get_time();
            const esp_err_t err = panel_decode_rows(buffer, draw.y, draw.height, data, bytes);
            stats_jpeg_us_ += esp_timer_get_time() - start;
            stats_jpegs_++;
            if (err != ESP_OK) fail(ErrorCode::DecodeFailed);
            return;
        }
        case tab5::Encoding::Raw:
            if (header.epoch != epoch_) return;
            if (bytes != draw.height * size.width * bsp_pixel_format_bytes(panel_format())) {
                return fail(ErrorCode::Malformed);
            }
            panel_write_rows(buffer, draw.y, draw.height, data);
            return;
        case tab5::Encoding::Lz4:
            if (header.epoch != epoch_) return;
            if (panel_write_lz4_rows(buffer, draw.y, draw.height, data, bytes) != ESP_OK) {
                fail(ErrorCode::DecodeFailed);
            }
            return;
        default:
            return fail(ErrorCode::Unsupported);
    }
}

static bool pixel_format(uint8_t value, bsp_pixel_format_t *format) {
    switch (static_cast<tab5::PixelFormat>(value)) {
        case tab5::PixelFormat::Rgb565: *format = BSP_PIXEL_FORMAT_RGB565; return true;
        case tab5::PixelFormat::Rgb888: *format = BSP_PIXEL_FORMAT_RGB888; return true;
    }
    return false;
}

void Tab5Video::draw_blit(const tab5::MsgHeader &header, const uint8_t *payload) {
    const auto fail = [&](ErrorCode code) { events_.error(Channel::Video, code, header.seq); };
    tab5::DrawBlit draw;
    if (header.length < sizeof(draw)) return fail(ErrorCode::Malformed);
    memcpy(&draw, payload, sizeof(draw));
    if (draw.target > static_cast<uint8_t>(tab5::Target::Back) || draw.rotation > 3 ||
        !draw.src_w || !draw.src_h || !draw.dst_w || !draw.dst_h) {
        return fail(ErrorCode::Malformed);
    }
    if (draw.src_w > kBlitMaxSize || draw.src_h > kBlitMaxSize) return fail(ErrorCode::TooLarge);
    const bsp_size_t size = panel_size();
    if (draw.dst_x + draw.dst_w > size.width || draw.dst_y + draw.dst_h > size.height) {
        return fail(ErrorCode::OutOfBounds);
    }

    PanelBlit blit = {};
    blit.buffer = static_cast<PanelBuffer>(draw.target);
    blit.source = {draw.src_w, draw.src_h};
    blit.rotation = static_cast<bsp_rotation_t>(draw.rotation);
    blit.area = {{draw.dst_x, draw.dst_y}, {draw.dst_w, draw.dst_h}};
    const uint8_t *data = payload + sizeof(draw);
    const uint32_t bytes = header.length - sizeof(draw);
    const auto encoding = static_cast<tab5::Encoding>(draw.encoding);
    if (encoding != tab5::Encoding::Jpeg && !pixel_format(draw.src_format, &blit.format)) {
        return fail(ErrorCode::Unsupported);
    }

    esp_err_t err;
    switch (encoding) {
        case tab5::Encoding::Jpeg:
            err = panel_blit_jpeg(blit, data, bytes);
            break;
        case tab5::Encoding::Raw:
            if (bytes != draw.src_w * draw.src_h * bsp_pixel_format_bytes(blit.format)) {
                return fail(ErrorCode::Malformed);
            }
            err = panel_blit(blit, data);
            break;
        case tab5::Encoding::Lz4:
            err = panel_blit_lz4(blit, data, bytes);
            break;
        default:
            return fail(ErrorCode::Unsupported);
    }
    if (err == ESP_ERR_NOT_SUPPORTED) {
        fail(ErrorCode::Unsupported);
    } else if (err != ESP_OK) {
        fail(ErrorCode::DecodeFailed);
    }
}

void Tab5Video::fill(const tab5::MsgHeader &header, const uint8_t *payload) {
    tab5::Fill fill;
    if (header.length != sizeof(fill)) {
        events_.error(Channel::Video, ErrorCode::Malformed, header.seq);
        return;
    }
    memcpy(&fill, payload, sizeof(fill));
    if (fill.target > static_cast<uint8_t>(tab5::Target::Back)) {
        events_.error(Channel::Video, ErrorCode::Malformed, header.seq);
        return;
    }
    const bsp_rect_t area = {{fill.x, fill.y}, {fill.w, fill.h}};
    esp_err_t err = panel_fill(static_cast<PanelBuffer>(fill.target), area, fill.color);
    if (err == ESP_ERR_INVALID_ARG) {
        events_.error(Channel::Video, ErrorCode::OutOfBounds, header.seq);
    } else if (err != ESP_OK) {
        ESP_LOGW(TAG, "fill: %s", esp_err_to_name(err));
    }
}

void Tab5Video::swap(const tab5::MsgHeader &header) {
    panel_swap();
    stats_swaps_++;
    const tab5::SwapDoneEvent done = {static_cast<uint32_t>(esp_timer_get_time())};
    events_.send(tab5::Type::SwapDone, header.seq, &done, sizeof(done));
}

void Tab5Video::clear(const tab5::MsgHeader &header) {
    if (header.length != sizeof(uint32_t)) {
        events_.error(Channel::Video, ErrorCode::Malformed, header.seq);
        return;
    }
    panel_clear();
}

void Tab5Video::display_power(const tab5::MsgHeader &header, const uint8_t *payload) {
    tab5::ByteEvent power;
    if (header.length != sizeof(power)) {
        events_.error(Channel::Video, ErrorCode::Malformed, header.seq);
        return;
    }
    memcpy(&power, payload, sizeof(power));
    display_power_(power.value != 0);
    if (!power.value) panel_clear();
}
