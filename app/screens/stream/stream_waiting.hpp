/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "bsp.h"
#include "lvgl.h"

/* What a stream screen shows while nothing is streamed: an icon, a message
 * and a fixed hint, centered. */
struct StreamWaiting {
    lv_obj_t *container;
    lv_obj_t *icon;
    lv_obj_t *message;
};

StreamWaiting stream_waiting_build(lv_obj_t *screen, const char *hint);
void stream_waiting_layout(const StreamWaiting &waiting, bsp_rotation_t rotation);
void stream_waiting_set(const StreamWaiting &waiting, const char *icon, const char *message);
