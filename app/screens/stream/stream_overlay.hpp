/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "bsp.h"
#include "lvgl.h"

bool stream_landscape(bsp_rotation_t rotation);
bsp_touch_point_t stream_logical_point(const bsp_touch_point_t &point, bsp_rotation_t rotation);

/* An LVGL display beside the stream, which is clipped to the rest of the
 * panel while it is open. Null when it could not be opened. */
lv_display_t *stream_overlay_open(bsp_rotation_t rotation);
void stream_overlay_close(lv_display_t *overlay);
