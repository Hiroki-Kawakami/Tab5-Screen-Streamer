/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <array>
#include "widgets.hpp"

class HomeScreen : public NavigationScreen {
public:
    void build() override;

private:
    lv_obj_t *hero_;
    std::array<lv_obj_t *, 4> cards_;

    void layout();
};
