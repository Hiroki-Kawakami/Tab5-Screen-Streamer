/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <initializer_list>
#include "lvgl.h"

lv_obj_t *setup_steps_create(lv_obj_t *parent, std::initializer_list<const char *> steps);
