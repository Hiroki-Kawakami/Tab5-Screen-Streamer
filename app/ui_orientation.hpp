/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "bsp_types.h"
#include "lvgl.h"

using OrientationListener = void (*)(bsp_rotation_t rotation, void *arg);

void ui_orientation_start(lv_display_t *main_display, bool locked, bsp_rotation_t rotation);
bsp_rotation_t ui_orientation_current();
void ui_orientation_set_locked(bool locked, bsp_rotation_t rotation);
/* The listener takes over rotating the UI; the observer is told after either. */
void ui_orientation_set_listener(OrientationListener listener, void *arg);
void ui_orientation_set_observer(OrientationListener observer, void *arg);
