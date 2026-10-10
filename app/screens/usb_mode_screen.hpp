/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "usb/usb.hpp"
#include "widgets.hpp"

/* Runs a USB profile while it is on screen. */
class UsbModeScreen : public NavigationScreen {
public:
    UsbModeScreen(UsbProfileId id, const char *title) : id_(id), title_(title) {}
    void build() override;
    void back() override;
    void onEnter() override;
    void onExit() override;

private:
    UsbProfileId id_;
    const char *title_;
};
