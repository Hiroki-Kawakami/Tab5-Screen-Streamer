#include "touch_input.hpp"
#include "platform_port.hpp"
#include "streamer.hpp"
#include "streamer_usb.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <cstdint>

namespace {

constexpr int MAX_POINTS = 5;
constexpr int POLL_MS = 8;          // ~125 Hz touch poll
constexpr int TOGGLE_FINGERS = 3;   // 3+ fingers toggles the settings overlay
constexpr uint8_t MSG_TYPE_TOUCH = 0x01;

// Touch report wire size: 2-byte header + 6 bytes per point. See PROTOCOL.md.
constexpr int REPORT_HEADER = 2;
constexpr int REPORT_PER_POINT = 6;

// Primary contact published for the LVGL indev. int reads/writes are atomic on
// this core; the short critical section keeps x/y/pressed mutually consistent.
portMUX_TYPE s_lock = portMUX_INITIALIZER_UNLOCKED;
bool s_primary_pressed = false;
int s_primary_x = 0;
int s_primary_y = 0;

void publish_primary(bool pressed, int x, int y) {
    portENTER_CRITICAL(&s_lock);
    s_primary_pressed = pressed;
    s_primary_x = x;
    s_primary_y = y;
    portEXIT_CRITICAL(&s_lock);
}

// Pack the active contacts into a touch report and queue it for the host.
void send_report(const pf_port::TouchPoint *pts, int n) {
    uint8_t buf[REPORT_HEADER + MAX_POINTS * REPORT_PER_POINT];
    buf[0] = MSG_TYPE_TOUCH;
    buf[1] = (uint8_t)n;
    int off = REPORT_HEADER;
    for (int i = 0; i < n; i++) {
        uint16_t x = (uint16_t)pts[i].x;
        uint16_t y = (uint16_t)pts[i].y;
        buf[off + 0] = pts[i].id;
        buf[off + 1] = 0;  // reserved
        buf[off + 2] = (uint8_t)(x & 0xff);
        buf[off + 3] = (uint8_t)(x >> 8);
        buf[off + 4] = (uint8_t)(y & 0xff);
        buf[off + 5] = (uint8_t)(y >> 8);
        off += REPORT_PER_POINT;
    }
    streamer_usb_vendor_write(buf, off);
}

void touch_task(void *) {
    bool toggle_armed = true;     // can the next 3-finger touch toggle the overlay?
    bool was_forwarding = false;  // did we send a non-empty report last tick?

    while (true) {
        pf_port::TouchPoint pts[MAX_POINTS];
        int n = pf_port::touch_read(pts, MAX_POINTS);

        // 3+-finger touch toggles the settings overlay, edge-triggered: it fires
        // once when the count first reaches the threshold and re-arms only after
        // all fingers lift.
        if (n >= TOGGLE_FINGERS) {
            if (toggle_armed) {
                gui_set_visible(!gui_is_visible());
                toggle_armed = false;
            }
        } else if (n == 0) {
            toggle_armed = true;
        }

        // Primary contact for the LVGL indev (only consumed while the overlay is
        // up, but always published so a press is visible the moment it lands).
        if (n > 0) {
            publish_primary(true, pts[0].x, pts[0].y);
        } else {
            publish_primary(false, 0, 0);
        }

        // Forward to the PC only in remote mode: overlay hidden, device mounted,
        // and not in the middle of a toggle gesture (so the 3-finger touch isn't
        // injected as clicks).
        bool forward = !gui_is_visible() && streamer_usb_mounted() &&
                       n < TOGGLE_FINGERS;
        if (forward && n > 0) {
            send_report(pts, n);
            was_forwarding = true;
        } else if (was_forwarding) {
            // Tell the PC to release everything (count == 0) exactly once when
            // forwarding stops — finger lift, toggle gesture, or mode change.
            send_report(nullptr, 0);
            was_forwarding = false;
        }

        vTaskDelay(pdMS_TO_TICKS(POLL_MS));
    }
}

}  // namespace

namespace touch_input {

void start() {
    xTaskCreatePinnedToCore(touch_task, "touch_in", 4096, nullptr, 5, nullptr, 1);
}

bool primary(int *x, int *y) {
    bool pressed;
    portENTER_CRITICAL(&s_lock);
    pressed = s_primary_pressed;
    *x = s_primary_x;
    *y = s_primary_y;
    portEXIT_CRITICAL(&s_lock);
    return pressed;
}

}  // namespace touch_input
