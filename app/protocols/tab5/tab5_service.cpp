/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "tab5_service.hpp"
#include <algorithm>
#include <cstring>
#include "display/panel.hpp"
#include "esp_log.h"
#include "esp_timer.h"
#include "lvgl.hpp"
#include "ui_orientation.hpp"

static const char *TAG = "tab5_service";

static constexpr uint16_t kMaxFps = 60;
static constexpr int64_t kSessionTimeoutUs = 3 * 1000 * 1000;
static constexpr TickType_t kTimeoutCheck = pdMS_TO_TICKS(250);

Tab5Service::Tab5Service(UsbStatusListener status)
    : video_(std::make_unique<Tab5Video>(
          events_, session_,
          [this](bool streaming) {
              portENTER_CRITICAL(&lock_);
              streaming_ = streaming;
              portEXIT_CRITICAL(&lock_);
              report_status();
          },
          [this](bool on) { set_display_power(on); })),
      audio_(std::make_unique<Tab5Audio>(events_, session_)), status_(std::move(status)) {}

Tab5Service::~Tab5Service() {
    if (timeout_timer_) xTimerDelete(timeout_timer_, portMAX_DELAY);
    video_.reset();
    audio_.reset();
}

esp_err_t Tab5Service::start() {
    esp_err_t err = events_.init();
    if (err == ESP_OK) err = video_->start();
    if (err == ESP_OK) err = audio_->start(kQueuedMessages * kAudioBytes);
    if (err != ESP_OK) return err;
    timeout_timer_ = xTimerCreate("tab5_session", kTimeoutCheck, pdTRUE, this, check_timeout);
    return timeout_timer_ ? ESP_OK : ESP_ERR_NO_MEM;
}

void Tab5Service::attach(std::function<void()> flush) {
    {
        std::lock_guard<std::mutex> guard(session_lock_);
        flush_ = std::move(flush);
    }
    lv_lock();
    orientation_ = ui_orientation_current();
    ui_orientation_set_observer(orientation_changed, this);
    lv_unlock();
    panel_set_format_listener(format_changed, this);
    panel_set_clip_listener(clip_changed, this);
    panel_lock();
    bsp_rect_t clip;
    const bool clipped = panel_clip(&clip);
    portENTER_CRITICAL(&lock_);
    overlay_visible_ = clipped;
    stream_ = clipped ? clip : bsp_rect_t{{0, 0}, panel_size()};
    portEXIT_CRITICAL(&lock_);
    panel_unlock();
}

void Tab5Service::detach() {
    lv_lock();
    ui_orientation_set_observer(nullptr, nullptr);
    lv_unlock();
    panel_set_format_listener(nullptr, nullptr);
    panel_set_clip_listener(nullptr, nullptr);
    close_session();
    video_->sync();
    audio_->sync();
    std::lock_guard<std::mutex> guard(session_lock_);
    flush_ = nullptr;
}

void Tab5Service::orientation_changed(bsp_rotation_t rotation, void *arg) {
    auto *self = static_cast<Tab5Service *>(arg);
    if (rotation == self->orientation_) return;
    self->orientation_ = rotation;
    const tab5::ByteEvent event = {static_cast<uint8_t>(rotation), {}};
    self->events_.send(tab5::Type::Orientation, 0, &event, sizeof(event));
}

void Tab5Service::format_changed(bsp_pixel_format_t format, void *arg) {
    static_cast<Tab5Service *>(arg)->video_->format_changed(format);
}

static tab5::Rect wire_rect(const bsp_rect_t &rect) {
    return {static_cast<uint16_t>(rect.origin.x), static_cast<uint16_t>(rect.origin.y),
            static_cast<uint16_t>(rect.size.width), static_cast<uint16_t>(rect.size.height)};
}

void Tab5Service::clip_changed(const bsp_rect_t *clip, bool front_changed, void *arg) {
    auto *self = static_cast<Tab5Service *>(arg);
    const bsp_size_t size = panel_size();
    const bsp_rect_t stream = clip ? *clip : bsp_rect_t{{0, 0}, size};
    portENTER_CRITICAL(&self->lock_);
    const bool was_visible = self->overlay_visible_;
    const bsp_rect_t covered = self->stream_;
    self->overlay_visible_ = clip != nullptr;
    self->stream_ = stream;
    portEXIT_CRITICAL(&self->lock_);

    const tab5::OverlayEvent event = {clip != nullptr, 0, wire_rect(stream)};
    self->events_.send(tab5::Type::Overlay, 0, &event, sizeof(event));
    if (front_changed) {
        const tab5::Rect wire = wire_rect(stream);
        self->events_.send(tab5::Type::Damage, 0, &wire, sizeof(wire));
    }
    if (!was_visible || (clip && clip->origin.x == covered.origin.x &&
                         clip->origin.y == covered.origin.y &&
                         clip->size.width == covered.size.width &&
                         clip->size.height == covered.size.height)) {
        return;
    }
    const int right = covered.origin.x + covered.size.width;
    const int bottom = covered.origin.y + covered.size.height;
    const bsp_rect_t damage[] = {
        {{0, 0}, {size.width, covered.origin.y}},
        {{0, bottom}, {size.width, size.height - bottom}},
        {{0, covered.origin.y}, {covered.origin.x, covered.size.height}},
        {{right, covered.origin.y}, {size.width - right, covered.size.height}},
    };
    for (const bsp_rect_t &rect : damage) {
        if (rect.size.width <= 0 || rect.size.height <= 0) continue;
        const tab5::Rect wire = wire_rect(rect);
        self->events_.send(tab5::Type::Damage, 0, &wire, sizeof(wire));
    }
}

void Tab5Service::report_status() {
    portENTER_CRITICAL(&lock_);
    const UsbStatus status = !configured_ || suspended_ ? UsbStatus::Unconfigured
                             : streaming_ ? UsbStatus::Active
                                          : UsbStatus::Configured;
    const bool changed = status != reported_;
    reported_ = status;
    portEXIT_CRITICAL(&lock_);
    if (changed && status_) status_(status);
}

void Tab5Service::connected(usb_device::Endpoint events) {
    portENTER_CRITICAL(&lock_);
    configured_ = true;
    portEXIT_CRITICAL(&lock_);
    report_status();
    events_.start(events);
    xTimerStart(timeout_timer_, portMAX_DELAY);
}

void Tab5Service::disconnected() {
    portENTER_CRITICAL(&lock_);
    configured_ = false;
    suspended_ = false;
    portEXIT_CRITICAL(&lock_);
    report_status();
    xTimerStop(timeout_timer_, portMAX_DELAY);
    close_session();
    events_.stop();
}

void Tab5Service::suspended() {
    portENTER_CRITICAL(&lock_);
    suspended_ = true;
    portEXIT_CRITICAL(&lock_);
    report_status();
    close_session();
}

void Tab5Service::resumed() {
    portENTER_CRITICAL(&lock_);
    suspended_ = false;
    portEXIT_CRITICAL(&lock_);
    report_status();
}

void Tab5Service::control(const uint8_t *data, uint32_t length) {
    uint32_t bad_seq = 0;
    const bool ok = tab5::for_each_message(
        data, length, &bad_seq, [this](const tab5::MsgHeader &header, const uint8_t *payload) {
            handle_control(header, payload);
        });
    if (!ok) events_.error(tab5::Channel::Control, tab5::ErrorCode::Malformed, bad_seq);
}

void Tab5Service::handle_control(const tab5::MsgHeader &header, const uint8_t *payload) {
    switch (static_cast<tab5::Type>(header.type)) {
        case tab5::Type::Open:
            open_session(header.seq);
            return;
        case tab5::Type::Close:
            close_session();
            return;
        case tab5::Type::KeepAlive:
            return;
        case tab5::Type::SetInputMode: {
            tab5::InputModeMessage message;
            if (header.length != sizeof(message)) break;
            memcpy(&message, payload, sizeof(message));
            if (!session_) return;
            if (message.target >= tab5::kInputTargets ||
                message.mode > static_cast<uint8_t>(tab5::InputMode::Hid) ||
                !input_enabled_[message.target]) {
                events_.error(tab5::Channel::Control, tab5::ErrorCode::Unsupported, header.seq);
                return;
            }
            set_input_mode(static_cast<tab5::InputTarget>(message.target),
                           static_cast<tab5::InputMode>(message.mode));
            return;
        }
        case tab5::Type::SetVolume: {
            tab5::VolumeMessage message;
            if (header.length != sizeof(message)) break;
            memcpy(&message, payload, sizeof(message));
            if (message.stream != static_cast<uint8_t>(tab5::Stream::Bulk)) {
                events_.error(tab5::Channel::Control, tab5::ErrorCode::Unsupported, header.seq);
                return;
            }
            audio_->set_volume(message.mute, message.volume);
            return;
        }
        case tab5::Type::SetBrightness: {
            tab5::ByteEvent message;
            if (header.length != sizeof(message)) break;
            memcpy(&message, payload, sizeof(message));
            if (!session_) return;
            std::lock_guard<std::mutex> guard(brightness_lock_);
            if (brightness_listener_) brightness_listener_(std::min<int>(message.value, 100));
            return;
        }
        default:
            break;
    }
    events_.error(tab5::Channel::Control, tab5::ErrorCode::Malformed, header.seq);
}

void Tab5Service::received() {
    const int64_t now = esp_timer_get_time();
    portENTER_CRITICAL(&lock_);
    received_us_ = now;
    portEXIT_CRITICAL(&lock_);
}

// What a session that ended mid-transfer left on the OUT endpoints would
// otherwise run into the first transfers of this one.
void Tab5Service::open_session(uint32_t seq) {
    std::lock_guard<std::mutex> guard(session_lock_);
    end_session_locked();
    if (flush_) flush_();
    events_.clear();
    received();
    if (++sessions_ == 0) sessions_ = 1;
    session_ = sessions_;
    ESP_LOGI(TAG, "session %u opened", (unsigned)sessions_);
    set_display_power(true);
    send_info(seq);
}

void Tab5Service::close_session() {
    std::lock_guard<std::mutex> guard(session_lock_);
    if (end_session_locked()) ESP_LOGI(TAG, "session closed");
}

bool Tab5Service::end_session_locked() {
    if (!session_) return false;
    session_ = 0;
    video_->end();
    audio_->end();
    reset_input_modes();
    return true;
}

void Tab5Service::check_timeout(TimerHandle_t timer) {
    auto *self = static_cast<Tab5Service *>(pvTimerGetTimerID(timer));
    std::lock_guard<std::mutex> guard(self->session_lock_);
    if (!self->session_) return;
    portENTER_CRITICAL(&self->lock_);
    const int64_t idle = esp_timer_get_time() - self->received_us_;
    portEXIT_CRITICAL(&self->lock_);
    if (idle < kSessionTimeoutUs) return;
    ESP_LOGW(TAG, "session timed out");
    self->end_session_locked();
    self->events_.send(tab5::Type::Closed, 0, nullptr, 0);
}

void Tab5Service::send_info(uint32_t seq) {
    const bsp_size_t size = panel_size();
    tab5::Info info = {};
    info.protocol_version = tab5::kProtocolVersion;
    info.panel_width = size.width;
    info.panel_height = size.height;
    info.framebuffers = panel_framebuffers();
    info.swap_window = Tab5Video::kSwapWindow;
    info.max_message[static_cast<int>(tab5::Channel::Control)] = kControlBytes;
    info.max_message[static_cast<int>(tab5::Channel::Video)] = kVideoBytes;
    info.max_message[static_cast<int>(tab5::Channel::Audio)] = kAudioBytes;
    info.audio_max_rate = Tab5Audio::kMaxRate;
    info.encodings = 1u << static_cast<int>(tab5::Encoding::Jpeg) |
                     1u << static_cast<int>(tab5::Encoding::Raw) |
                     1u << static_cast<int>(tab5::Encoding::Lz4);
    info.direct_align = Tab5Video::kDirectAlign;
    info.blit_max_width = Tab5Video::kBlitMaxSize;
    info.blit_max_height = Tab5Video::kBlitMaxSize;
    info.pixel_format = static_cast<uint8_t>(tab5_pixel_format(panel_format()));
    info.orientation = orientation_;
    info.epoch = video_->epoch();
    info.max_fps = kMaxFps;
    for (int i = 0; i < tab5::kInputTargets; i++) {
        info.input_mode[i] = static_cast<uint8_t>(input_mode_[i].load());
    }
    info.volume[static_cast<int>(tab5::Stream::Uac)] = audio_->uac_volume();
    info.volume[static_cast<int>(tab5::Stream::Bulk)] = audio_->volume();
    portENTER_CRITICAL(&lock_);
    info.overlay.visible = overlay_visible_;
    info.overlay.stream = wire_rect(stream_);
    portEXIT_CRITICAL(&lock_);
    info.brightness = brightness_;
    events_.send(tab5::Type::Info, seq, &info, sizeof(info), info.epoch);
}

void Tab5Service::set_input_listener(InputListener listener) {
    std::lock_guard<std::mutex> guard(input_lock_);
    input_listener_ = std::move(listener);
}

void Tab5Service::set_input_enabled(tab5::InputTarget target, bool enabled) {
    input_enabled_[static_cast<int>(target)] = enabled;
    set_input_mode(target, enabled ? tab5::InputMode::Hid : tab5::InputMode::None);
}

tab5::InputMode Tab5Service::input_mode(tab5::InputTarget target) const {
    return input_mode_[static_cast<int>(target)];
}

void Tab5Service::reset_input_modes() {
    for (int i = 0; i < tab5::kInputTargets; i++) {
        set_input_mode(static_cast<tab5::InputTarget>(i),
                       input_enabled_[i] ? tab5::InputMode::Hid : tab5::InputMode::None);
    }
}

void Tab5Service::set_input_mode(tab5::InputTarget target, tab5::InputMode mode) {
    const int index = static_cast<int>(target);
    std::lock_guard<std::mutex> guard(input_lock_);
    if (input_mode_[index] == mode) return;
    if (input_mode_[index] == tab5::InputMode::Event) release_events_locked(target);
    input_mode_[index] = mode;
    if (input_listener_) input_listener_(target, mode);
    const tab5::InputModeMessage message = {static_cast<uint8_t>(index),
                                            static_cast<uint8_t>(mode), 0};
    events_.send(tab5::Type::InputMode, 0, &message, sizeof(message));
}

void Tab5Service::release_events_locked(tab5::InputTarget target) {
    if (target == tab5::InputTarget::Touch) {
        if (touch_count_) send_touch_locked(nullptr, 0);
        return;
    }
    static constexpr tab5::KeyEvent kReleased = {};
    if (memcmp(&key_, &kReleased, sizeof(key_)) == 0) return;
    key_ = kReleased;
    events_.send(tab5::Type::Key, 0, &key_, sizeof(key_));
}

void Tab5Service::send_touch_locked(const tab5::TouchPoint *points, int count) {
    if (count == touch_count_ &&
        (!count || memcmp(points, touch_points_, count * sizeof(*points)) == 0)) {
        return;
    }
    touch_count_ = count;
    if (count) memcpy(touch_points_, points, count * sizeof(*points));
    uint8_t payload[sizeof(tab5::TouchEvent) + sizeof(touch_points_)];
    const tab5::TouchEvent event = {static_cast<uint8_t>(count), {}};
    memcpy(payload, &event, sizeof(event));
    if (count) memcpy(payload + sizeof(event), points, count * sizeof(*points));
    events_.send(tab5::Type::Touch, 0, payload, sizeof(event) + count * sizeof(*points));
}

bool Tab5Service::touch(const bsp_touch_point_t *points, int count) {
    std::lock_guard<std::mutex> guard(input_lock_);
    if (input_mode_[static_cast<int>(tab5::InputTarget::Touch)] != tab5::InputMode::Event) {
        return false;
    }
    tab5::TouchPoint wire[kMaxTouchPoints];
    count = std::min(count, kMaxTouchPoints);
    for (int i = 0; i < count; i++) {
        wire[i] = {static_cast<uint8_t>(points[i].id), 0, static_cast<uint16_t>(points[i].x),
                   static_cast<uint16_t>(points[i].y), 0};
    }
    send_touch_locked(wire, count);
    return true;
}

bool Tab5Service::key(const tab5::KeyEvent &event) {
    std::lock_guard<std::mutex> guard(input_lock_);
    if (input_mode_[static_cast<int>(tab5::InputTarget::Keyboard)] != tab5::InputMode::Event) {
        return false;
    }
    if (memcmp(&event, &key_, sizeof(key_)) == 0) return true;
    key_ = event;
    events_.send(tab5::Type::Key, 0, &key_, sizeof(key_));
    return true;
}

void Tab5Service::set_display_power_listener(DisplayPowerListener listener) {
    std::lock_guard<std::mutex> guard(display_lock_);
    display_listener_ = std::move(listener);
}

void Tab5Service::set_brightness_listener(BrightnessListener listener) {
    std::lock_guard<std::mutex> guard(brightness_lock_);
    brightness_listener_ = std::move(listener);
}

void Tab5Service::brightness_changed(int percent) {
    brightness_ = percent;
    const tab5::ByteEvent event = {static_cast<uint8_t>(percent), {}};
    events_.send(tab5::Type::Brightness, 0, &event, sizeof(event));
}

void Tab5Service::set_display_power(bool on) {
    std::lock_guard<std::mutex> guard(display_lock_);
    ESP_LOGI(TAG, "display %s", on ? "on" : "off");
    if (display_listener_) display_listener_(on);
}
