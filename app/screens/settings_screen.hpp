/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "widgets.hpp"

class SettingsScreen : public NavigationScreen {
public:
    void build() override;
    void back() override;
};
