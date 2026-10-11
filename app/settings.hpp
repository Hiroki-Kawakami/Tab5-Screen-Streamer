/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "bsp.h"

void settings_init();
void settings_commit();

bool settings_rotation_locked();
void settings_set_rotation_lock(bool locked);
bsp_rotation_t settings_rotation();
void settings_set_rotation(bsp_rotation_t rotation);
