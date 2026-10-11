/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include "widgets.hpp"

class PcAppScreen : public NavigationScreen {
public:
    static constexpr const char *kTitle = "Stream from PC App";

    static std::shared_ptr<Screen> stream_screen();
    void build() override;
    void back() override;

private:
    lv_obj_t *setup_pane_;
    lv_obj_t *right_pane_;
    lv_obj_t *options_pane_;
    lv_obj_t *footer_;
    lv_obj_t *start_;

    void layout();
};
