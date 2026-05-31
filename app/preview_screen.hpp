#pragma once
#include "screen_manager.hpp"

class PreviewScreen: public Screen {
public:
    PreviewScreen();
    virtual void build();
    virtual void onEnter();

    // Flip the status pill between "Connected"/"Disconnected". Called from the
    // decoder task via lv_async_call so it runs on the LVGL thread.
    void set_status_ui(bool connected);

private:
    lv_obj_t *status_container_ = nullptr;
    lv_obj_t *status_label_ = nullptr;
    lv_obj_t *brightness_slider_ = nullptr;
};
