/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "stream_overlay.hpp"
#include "display/panel.hpp"
#include "display_manager.hpp"
#include "esp_log.h"

static const char *TAG = "stream_overlay";

static constexpr int32_t kLandscapePanelWidth = 560;
static constexpr int32_t kPortraitPanelHeight = 640;
static constexpr int kOverlayBufferLines = 32;

bool stream_landscape(bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    return size.width > size.height;
}

static bsp_rect_t overlay_area(bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    if (stream_landscape(rotation)) {
        return {{size.width - kLandscapePanelWidth, 0}, {kLandscapePanelWidth, size.height}};
    }
    return {{0, size.height - kPortraitPanelHeight}, {size.width, kPortraitPanelHeight}};
}

static bsp_rect_t stream_area(bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    if (stream_landscape(rotation)) {
        return {{0, 0}, {size.width - kLandscapePanelWidth, size.height}};
    }
    return {{0, 0}, {size.width, size.height - kPortraitPanelHeight}};
}

bsp_touch_point_t stream_logical_point(const bsp_touch_point_t &point, bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    bsp_touch_point_t logical = point;
    switch (rotation) {
        case BSP_ROTATION_90:
            logical.x = size.width - 1 - point.y;
            logical.y = point.x;
            break;
        case BSP_ROTATION_180:
            logical.x = size.width - 1 - point.x;
            logical.y = size.height - 1 - point.y;
            break;
        case BSP_ROTATION_270:
            logical.x = point.y;
            logical.y = size.height - 1 - point.x;
            break;
        default:
            break;
    }
    return logical;
}

lv_display_t *stream_overlay_open(bsp_rotation_t rotation) {
    const bsp_rect_t stream = panel_native_rect(stream_area(rotation), rotation);
    if (panel_set_clip(&stream) != ESP_OK) {
        ESP_LOGE(TAG, "clip to %dx%d at %d,%d failed", stream.size.width, stream.size.height,
                 stream.origin.x, stream.origin.y);
        return nullptr;
    }

    DisplayManagerConfig config = {};
    config.present_mode = DisplayPresentMode::Immediate;
    config.render_mode = DisplayRenderMode::Partial;
    config.color_format = LV_COLOR_FORMAT_RGB565;
    config.make_default = false;
    config.viewport.output_area = panel_native_rect(overlay_area(rotation), rotation);
    config.viewport.rotation = rotation;
    config.buffer.lines = kOverlayBufferLines;
    lv_display_t *overlay = nullptr;
    const esp_err_t err = display_manager.create_display(config, &overlay);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "overlay display: %s", esp_err_to_name(err));
        panel_set_clip(nullptr);
        return nullptr;
    }
    return overlay;
}

void stream_overlay_close(lv_display_t *overlay) {
    display_manager.delete_display(overlay);
    panel_set_clip(nullptr);
}
