#pragma once
#include "screen_manager.hpp"

class PreviewScreen: public Screen {
public:
    PreviewScreen();
    virtual void build();
    virtual void onEnter();

private:
};
