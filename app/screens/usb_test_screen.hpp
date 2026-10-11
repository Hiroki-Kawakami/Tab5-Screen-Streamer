/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "widgets.hpp"

/* Runs the USB test profile while it is on screen. */
class UsbTestScreen : public NavigationScreen {
public:
    void build() override;
    void back() override;
    void onEnter() override;
    void onExit() override;
};
