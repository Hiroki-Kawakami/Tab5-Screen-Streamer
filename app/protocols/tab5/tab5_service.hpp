/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 *
 * The device side of docs/protocol.md, apart from the transport carrying it.
 */

#pragma once
#include <atomic>
#include <functional>
#include <memory>
#include <mutex>
#include "bsp.h"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "tab5_audio.hpp"
#include "tab5_events.hpp"
#include "tab5_video.hpp"
#include "usb/usb_profile.hpp"

/* Sessions, drawing, audio and device events. Transports attach to it one at
 * a time and hand it what the host sends. */
class Tab5Service {
public:
    static constexpr uint32_t kControlBytes = Tab5Events::kTransferBytes;
    static constexpr uint32_t kVideoBytes = 16 * 65024;
    static constexpr uint32_t kAudioBytes = 16384;
    static constexpr int kQueuedMessages = 4;
    static constexpr int kMaxTouchPoints = 10;

    /* Called on every input mode change with the mode taking over, after the
     * input sent in events is released; the listener releases what it sent on
     * HID. It must not call back into the service. */
    using InputListener = std::function<void(tab5::InputTarget target, tab5::InputMode mode)>;
    /* Called with every display power the host sets, and with on when a
     * session opens. Off is blackened once the listener returns. */
    using DisplayPowerListener = std::function<void(bool on)>;
    /* Called with the brightness the host sets, 0-100. The listener applies
     * it and announces what it applied with brightness_changed(). */
    using BrightnessListener = std::function<void(int percent)>;

    explicit Tab5Service(UsbStatusListener status);
    ~Tab5Service();
    esp_err_t start();

    Tab5Video &video() { return *video_; }
    Tab5Audio &audio() { return *audio_; }
    /* Transfers of other sessions are dropped unread; 0 is none. */
    uint32_t session() const { return session_; }

    /* `flush` drops what the transport holds of the host's messages. detach()
     * returns once video and audio are done with the transport's buffers. */
    void attach(std::function<void()> flush);
    void detach();
    void connected(usb_device::Endpoint events);
    void disconnected();
    void suspended();
    void resumed();
    void received();
    void control(const uint8_t *data, uint32_t length);

    /* Returns once the previous listener is no longer running. */
    void set_input_listener(InputListener listener);
    /* A disabled target stays None. Either way the target goes back to its
     * default mode. */
    void set_input_enabled(tab5::InputTarget target, bool enabled);
    tab5::InputMode input_mode(tab5::InputTarget target) const;
    /* False when the target is not in Event mode, and nothing is sent. */
    bool touch(const bsp_touch_point_t *points, int count);
    bool key(const tab5::KeyEvent &event);

    /* Returns once the previous listener is no longer running. */
    void set_display_power_listener(DisplayPowerListener listener);
    /* Returns once the previous listener is no longer running. */
    void set_brightness_listener(BrightnessListener listener);
    void brightness_changed(int percent);

private:
    void handle_control(const tab5::MsgHeader &header, const uint8_t *payload);
    void open_session(uint32_t seq);
    void close_session();
    bool end_session_locked();
    static void check_timeout(TimerHandle_t timer);
    void send_info(uint32_t seq);
    void set_input_mode(tab5::InputTarget target, tab5::InputMode mode);
    void reset_input_modes();
    void release_events_locked(tab5::InputTarget target);
    void send_touch_locked(const tab5::TouchPoint *points, int count);
    void report_status();
    void set_display_power(bool on);
    static void orientation_changed(bsp_rotation_t rotation, void *arg);
    static void format_changed(bsp_pixel_format_t format, void *arg);
    static void clip_changed(const bsp_rect_t *clip, void *arg);

    std::atomic<uint32_t> session_ = 0;
    Tab5Events events_;
    std::unique_ptr<Tab5Video> video_;
    std::unique_ptr<Tab5Audio> audio_;
    portMUX_TYPE lock_ = portMUX_INITIALIZER_UNLOCKED;
    std::atomic<uint8_t> orientation_ = 0;
    bool overlay_visible_ = false;
    bsp_rect_t stream_ = {};
    UsbStatusListener status_;
    bool configured_ = false;
    bool suspended_ = false;
    bool streaming_ = false;
    UsbStatus reported_ = UsbStatus::Unconfigured;
    std::mutex session_lock_;
    std::function<void()> flush_;
    uint32_t sessions_ = 0;
    int64_t received_us_ = 0;
    TimerHandle_t timeout_timer_ = nullptr;
    std::mutex input_lock_;
    InputListener input_listener_;
    std::atomic<bool> input_enabled_[tab5::kInputTargets] = {};
    std::atomic<tab5::InputMode> input_mode_[tab5::kInputTargets] = {};
    int touch_count_ = 0;
    tab5::TouchPoint touch_points_[kMaxTouchPoints] = {};
    tab5::KeyEvent key_ = {};
    std::mutex display_lock_;
    DisplayPowerListener display_listener_;
    std::mutex brightness_lock_;
    BrightnessListener brightness_listener_;
    std::atomic<uint8_t> brightness_ = 0;
};
