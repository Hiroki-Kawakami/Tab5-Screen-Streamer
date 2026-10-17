/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "panel.hpp"
#include <algorithm>
#include <atomic>
#include <cstring>
#include <numeric>
#include "display_manager.hpp"
#include "driver/ppa.h"
#include "esp_attr.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "jpeg_ppa_pipeline.h"
#include "lvgl.hpp"
#include "lz4_block.h"
#ifdef ESP_PLATFORM
#include "esp_cache.h"
#endif

static const char *TAG = "panel";

static constexpr int kRowAlign = 16;
static constexpr uint32_t kScaleDenominator = 16;
static constexpr size_t kLz4Window = 65536;
static constexpr int kStrips = 2;
static constexpr int kMaxPendingBlits = 6;

static lv_display_t *s_ui;
static uint8_t *s_sram;
static size_t s_sram_bytes;
static lv_display_t *s_idle;
static bsp_rotation_t s_idle_rotation;
static SemaphoreHandle_t s_lock;
static std::atomic<int> s_preempting;
static PanelFormatListener s_format_listener;
static void *s_format_listener_arg;
static uint8_t *s_strips[kStrips];
static size_t s_strip_bytes;
static jpeg_ppa_pipeline_handle_t s_pipeline;
static ppa_client_handle_t s_fill;
static ppa_client_handle_t s_copy;
static SemaphoreHandle_t s_strip_done[kStrips];
static SemaphoreHandle_t s_blits_done;
static int s_blits_pending;
static bool s_strip_busy[kStrips];

static bsp_size_t s_size;
static bsp_pixel_format_t s_format;
static size_t s_stride;
static int s_framebuffers;
static int s_front;
static bool s_acquired;
static bool s_clipped;
static bsp_rect_t s_clip;
static uint8_t *s_saved;
static PanelClipListener s_clip_listener;
static void *s_clip_listener_arg;

int panel_buffer_index(PanelBuffer buffer) {
    return buffer == PanelBuffer::Front || s_clipped ? s_front : (s_front + 1) % s_framebuffers;
}

uint8_t *panel_strip(int index) {
    return s_strips[index];
}

size_t panel_strip_bytes() {
    return s_strip_bytes;
}

static uint8_t *framebuffer_at(int index) {
    return static_cast<uint8_t *>(bsp_display_get_frame_buffer(index));
}

static uint8_t *framebuffer(PanelBuffer buffer) {
    return framebuffer_at(panel_buffer_index(buffer));
}

static bool rows_valid(int y, int height) {
    return y >= 0 && height > 0 && y + height <= s_size.height;
}

static bool intersect(bsp_rect_t *area, const bsp_rect_t &clip) {
    const int x0 = std::max(area->origin.x, clip.origin.x);
    const int y0 = std::max(area->origin.y, clip.origin.y);
    const int x1 = std::min(area->origin.x + area->size.width, clip.origin.x + clip.size.width);
    const int y1 = std::min(area->origin.y + area->size.height, clip.origin.y + clip.size.height);
    if (x1 <= x0 || y1 <= y0) return false;
    *area = {{x0, y0}, {x1 - x0, y1 - y0}};
    return true;
}

/* The parts of the panel outside `clip`; some may be empty. */
static void outside(const bsp_rect_t &clip, bsp_rect_t parts[4]) {
    const int right = clip.origin.x + clip.size.width;
    const int bottom = clip.origin.y + clip.size.height;
    parts[0] = {{0, 0}, {s_size.width, clip.origin.y}};
    parts[1] = {{0, bottom}, {s_size.width, s_size.height - bottom}};
    parts[2] = {{0, clip.origin.y}, {clip.origin.x, clip.size.height}};
    parts[3] = {{right, clip.origin.y}, {s_size.width - right, clip.size.height}};
}

static bool empty(const bsp_rect_t &area) {
    return area.size.width <= 0 || area.size.height <= 0;
}

static void clear_area(uint8_t *fb, bsp_rect_t area) {
    if (empty(area)) return;
    const size_t bytes = bsp_pixel_format_bytes(s_format);
    uint8_t *rows = fb + area.origin.y * s_stride;
    for (int y = 0; y < area.size.height; y++) {
        memset(rows + y * s_stride + area.origin.x * bytes, 0, area.size.width * bytes);
    }
#ifdef ESP_PLATFORM
    esp_cache_msync(rows, area.size.height * s_stride,
                    ESP_CACHE_MSYNC_FLAG_DIR_C2M | ESP_CACHE_MSYNC_FLAG_UNALIGNED);
#endif
}

static void clear_area(int index, bsp_rect_t area) {
    clear_area(framebuffer_at(index), area);
}

static PanelBlit rows_blit(PanelBuffer buffer, int y, int height) {
    return {buffer, {s_size.width, height}, s_format, BSP_ROTATION_0, {{0, y}, {s_size.width, height}}};
}

static ppa_srm_color_mode_t srm_mode(bsp_pixel_format_t format) {
    return format == BSP_PIXEL_FORMAT_RGB888 ? PPA_SRM_COLOR_MODE_RGB888
                                             : PPA_SRM_COLOR_MODE_RGB565;
}

static ppa_srm_rotation_angle_t srm_angle(bsp_rotation_t rotation) {
    switch (rotation) {
        case BSP_ROTATION_90: return PPA_SRM_ROTATION_ANGLE_90;
        case BSP_ROTATION_180: return PPA_SRM_ROTATION_ANGLE_180;
        case BSP_ROTATION_270: return PPA_SRM_ROTATION_ANGLE_270;
        default: return PPA_SRM_ROTATION_ANGLE_0;
    }
}

/* `area` of `src` into `dst` at the same place, or turned upside down about
 * the panel's center. */
static esp_err_t copy_area(uint8_t *dst, const uint8_t *src, bsp_rect_t area,
                           bsp_rotation_t rotation = BSP_ROTATION_0) {
    if (empty(area)) return ESP_OK;
    ppa_srm_oper_config_t config = {};
    config.in.buffer = src;
    config.in.pic_w = s_size.width;
    config.in.pic_h = s_size.height;
    config.in.block_offset_x = area.origin.x;
    config.in.block_offset_y = area.origin.y;
    config.in.block_w = area.size.width;
    config.in.block_h = area.size.height;
    config.in.srm_cm = srm_mode(s_format);
    config.out.buffer = dst;
    config.out.buffer_size = s_stride * s_size.height;
    config.out.pic_w = s_size.width;
    config.out.pic_h = s_size.height;
    config.out.block_offset_x = area.origin.x;
    config.out.block_offset_y = area.origin.y;
    if (rotation == BSP_ROTATION_180) {
        config.out.block_offset_x = s_size.width - area.origin.x - area.size.width;
        config.out.block_offset_y = s_size.height - area.origin.y - area.size.height;
    }
    config.out.srm_cm = srm_mode(s_format);
    config.rotation_angle = srm_angle(rotation);
    config.scale_x = 1;
    config.scale_y = 1;
    config.mode = PPA_TRANS_MODE_BLOCKING;
    return ppa_do_scale_rotate_mirror(s_copy, &config);
}

static void free_saved() {
    heap_caps_free(s_saved);
    s_saved = nullptr;
}

/* What lies outside the clip, kept in a copy while the overlay covers it. */
static void save_outside() {
    if (!s_saved) {
        s_saved = static_cast<uint8_t *>(
            heap_caps_aligned_alloc(64, s_stride * s_size.height, MALLOC_CAP_SPIRAM));
    }
    if (!s_saved) {
        ESP_LOGW(TAG, "no memory to keep what the clip covers");
        return;
    }
    bsp_rect_t parts[4];
    outside(s_clip, parts);
    for (const bsp_rect_t &part : parts) copy_area(s_saved, framebuffer_at(s_front), part);
}

static void restore_outside() {
    bsp_rect_t parts[4];
    outside(s_clip, parts);
    for (const bsp_rect_t &part : parts) {
        if (s_saved) {
            copy_area(framebuffer_at(s_front), s_saved, part);
        } else {
            clear_area(s_front, part);
        }
    }
    free_saved();
}

int64_t panel_diag_lz4_us = 0;  // DIAG

static bool IRAM_ATTR strip_done(ppa_client_handle_t, ppa_event_data_t *, void *user_data) {
    if (!user_data) return false;
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(static_cast<SemaphoreHandle_t>(user_data), &woken);
    return woken == pdTRUE;
}

static void wait_strip(int index) {
    if (!s_strip_busy[index]) return;
    xSemaphoreTake(s_strip_done[index], portMAX_DELAY);
    s_strip_busy[index] = false;
}

esp_err_t panel_init(lv_display_t *ui, uint8_t *sram, size_t sram_bytes) {
    s_ui = ui;
    s_sram = sram;
    s_sram_bytes = sram_bytes;
    s_lock = xSemaphoreCreateMutex();
    if (!s_lock) return ESP_ERR_NO_MEM;
    s_strip_bytes = sram_bytes / kStrips;
    for (int i = 0; i < kStrips; i++) {
        s_strips[i] = sram + i * s_strip_bytes;
        s_strip_done[i] = xSemaphoreCreateBinary();
        if (!s_strip_done[i]) return ESP_ERR_NO_MEM;
    }
    s_blits_done = xSemaphoreCreateCounting(kMaxPendingBlits, 0);
    if (!s_blits_done) return ESP_ERR_NO_MEM;
    s_size = bsp_display_get_size();
    s_format = bsp_display_get_pixel_format();
    s_stride = s_size.width * bsp_pixel_format_bytes(s_format);
    while (s_framebuffers < BSP_DISPLAY_MAX_FRAME_BUFFERS &&
           bsp_display_get_frame_buffer(s_framebuffers)) {
        s_framebuffers++;
    }
    if (s_framebuffers < 2) {
        ESP_LOGE(TAG, "needs two framebuffers, has %d", s_framebuffers);
        return ESP_ERR_INVALID_STATE;
    }

    ppa_client_config_t client = {};
    client.oper_type = PPA_OPERATION_FILL;
    esp_err_t err = ppa_register_client(&client, &s_fill);
    if (err == ESP_OK) {
        client.oper_type = PPA_OPERATION_SRM;
        client.max_pending_trans_num = kStrips + kMaxPendingBlits;
        err = ppa_register_client(&client, &s_copy);
    }
    if (err == ESP_OK) {
        ppa_event_callbacks_t callbacks = {};
        callbacks.on_trans_done = strip_done;
        err = ppa_client_register_event_callbacks(s_copy, &callbacks);
    }
    if (err != ESP_OK) ESP_LOGE(TAG, "ppa client: %s", esp_err_to_name(err));
    return err;
}

static void create_pipeline() {
    jpeg_ppa_pipeline_cfg_t config = {};
    config.strip_bufs[0] = s_strips[0];
    config.strip_bufs[1] = s_strips[1];
    config.strip_buf_size = s_strip_bytes;
    config.strip_color_mode = srm_mode(s_format);
    config.rgb_order = JPEG_DEC_RGB_ELEMENT_ORDER_BGR;
    config.conv_std = JPEG_YUV_RGB_CONV_STD_BT601;
    config.yuv_full_range = true;
    esp_err_t err = jpeg_ppa_pipeline_new(&config, &s_pipeline);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "jpeg pipeline: %s", esp_err_to_name(err));
        s_pipeline = nullptr;
    }
}

static void delete_pipeline() {
    if (!s_pipeline) return;
    jpeg_ppa_pipeline_del(s_pipeline);
    s_pipeline = nullptr;
}

static void set_ui_visible(bool visible) {
    display_manager.set_visible(s_ui, visible);
    if (!visible) bsp_display_wait_draw();
}

// The video task takes the lock back as soon as it gives it while frames
// queue up, starving any lower priority task waiting for it.
static void preempt_lock() {
    s_preempting++;
    xSemaphoreTake(s_lock, portMAX_DELAY);
    s_preempting--;
}

void panel_lock() {
    while (s_preempting.load()) vTaskDelay(1);
    xSemaphoreTake(s_lock, portMAX_DELAY);
}

void panel_unlock() {
    xSemaphoreGive(s_lock);
}

void panel_acquire() {
    lv_lock();
    if (s_idle) {
        lv_timer_pause(lv_display_get_refr_timer(s_idle));
    } else {
        set_ui_visible(false);
    }
    panel_lock();
    // Framebuffer 0 is on screen while the panel is free.
    s_front = 0;
    s_acquired = true;
    create_pipeline();
    if (s_clipped) {
        save_outside();
        if (s_saved) clear_area(s_saved, {{0, 0}, s_size});
    }
    panel_unlock();
    lv_unlock();
}

void panel_release() {
    lv_lock();
    panel_lock();
    delete_pipeline();
    free_saved();
    // The UI's partial blits land in the framebuffer last presented, and the
    // rest of the UI is only in framebuffer 0.
    display_manager.present(0);
    s_front = 0;
    s_acquired = false;
    panel_unlock();
    if (s_idle) {
        lv_timer_resume(lv_display_get_refr_timer(s_idle));
        lv_obj_invalidate(lv_display_get_screen_active(s_idle));
    } else {
        set_ui_visible(true);
    }
    lv_unlock();
}

static void idle_flush(lv_display_t *display, const lv_area_t *area, uint8_t *pixels) {
    const bsp_size_t size = panel_logical_size(s_idle_rotation);
    const int x = area->x1, y = area->y1;
    const int w = lv_area_get_width(area), h = lv_area_get_height(area);
    bsp_rect_t native;
    switch (s_idle_rotation) {
        case BSP_ROTATION_90: native = {{y, size.width - x - w}, {h, w}}; break;
        case BSP_ROTATION_180: native = {{size.width - x - w, size.height - y - h}, {w, h}}; break;
        case BSP_ROTATION_270: native = {{size.height - y - h, x}, {h, w}}; break;
        default: native = {{x, y}, {w, h}}; break;
    }
    const PanelBlit blit = {PanelBuffer::Front, {w, h}, BSP_PIXEL_FORMAT_RGB565, s_idle_rotation,
                            native};
    panel_lock();
    // An invalidation resumes the refresh timer panel_acquire() paused.
    const esp_err_t err = s_acquired ? ESP_OK : panel_blit(blit, pixels);
    panel_unlock();
    if (err != ESP_OK) ESP_LOGE(TAG, "idle display flush: %s", esp_err_to_name(err));
    lv_display_flush_ready(display);
}

// lv_display_create() makes the display the default one, which the screen
// manager loads its screens into.
lv_display_t *panel_idle_display_create(bsp_rotation_t rotation) {
    if (s_idle) return nullptr;
    const bsp_size_t size = panel_logical_size(rotation);
    lv_display_t *main = lv_display_get_default();
    lv_display_t *display = lv_display_create(size.width, size.height);
    lv_display_set_default(main);
    if (!display) {
        ESP_LOGE(TAG, "no memory for the idle display");
        return nullptr;
    }
    if (!s_acquired) set_ui_visible(false);
    s_idle = display;
    s_idle_rotation = rotation;
    lv_display_set_color_format(display, LV_COLOR_FORMAT_RGB565);
    lv_display_set_buffers(display, s_sram, s_sram + s_sram_bytes / 2, s_sram_bytes / 2,
                           LV_DISPLAY_RENDER_MODE_PARTIAL);
    lv_display_set_flush_cb(display, idle_flush);
    if (s_acquired) lv_timer_pause(lv_display_get_refr_timer(display));
    return display;
}

void panel_idle_display_set_rotation(bsp_rotation_t rotation) {
    if (!s_idle || rotation == s_idle_rotation) return;
    s_idle_rotation = rotation;
    const bsp_size_t size = panel_logical_size(rotation);
    lv_display_set_resolution(s_idle, size.width, size.height);
    lv_obj_invalidate(lv_display_get_screen_active(s_idle));
}

void panel_idle_display_delete() {
    if (!s_idle) return;
    lv_display_delete(s_idle);
    s_idle = nullptr;
    if (!s_acquired) set_ui_visible(true);
}

esp_err_t panel_set_format(bsp_pixel_format_t format) {
    lv_lock();
    preempt_lock();
    if (format == s_format) {
        panel_unlock();
        lv_unlock();
        return ESP_OK;
    }
    if (s_acquired) {
        delete_pipeline();
        free_saved();
    } else if (!s_idle) {
        set_ui_visible(false);
    }
    esp_err_t err = bsp_display_reconfigure(format, 0);
    display_manager.set_color_format(s_ui);
    s_format = bsp_display_get_pixel_format();
    s_stride = s_size.width * bsp_pixel_format_bytes(s_format);
    if (s_acquired) {
        s_front = 0;
        create_pipeline();
    } else if (s_idle) {
        lv_obj_invalidate(lv_display_get_screen_active(s_idle));
    } else {
        set_ui_visible(true);
    }
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "reconfigure: %s", esp_err_to_name(err));
    } else if (s_format_listener) {
        s_format_listener(s_format, s_format_listener_arg);
    }
    panel_unlock();
    lv_unlock();
    return err;
}

void panel_set_format_listener(PanelFormatListener listener, void *arg) {
    panel_lock();
    s_format_listener = listener;
    s_format_listener_arg = arg;
    panel_unlock();
}

bool panel_acquired() {
    return s_acquired;
}

esp_err_t panel_set_clip(const bsp_rect_t *clip) {
    if (clip && (clip->origin.x < 0 || clip->origin.y < 0 || clip->size.width <= 0 ||
                 clip->size.height <= 0 || clip->origin.x + clip->size.width > s_size.width ||
                 clip->origin.y + clip->size.height > s_size.height)) {
        return ESP_ERR_INVALID_ARG;
    }
    preempt_lock();
    if (s_clipped && s_acquired) restore_outside();
    s_clipped = clip != nullptr;
    if (clip) s_clip = *clip;
    if (s_clipped && s_acquired) save_outside();
    if (s_clip_listener) s_clip_listener(clip, s_clip_listener_arg);
    panel_unlock();
    return ESP_OK;
}

bool panel_clip(bsp_rect_t *clip) {
    if (s_clipped) *clip = s_clip;
    return s_clipped;
}

void panel_set_clip_listener(PanelClipListener listener, void *arg) {
    panel_lock();
    s_clip_listener = listener;
    s_clip_listener_arg = arg;
    panel_unlock();
}

bsp_size_t panel_size() {
    return s_size;
}

bsp_size_t panel_logical_size(bsp_rotation_t rotation) {
    const bool transposed = rotation == BSP_ROTATION_90 || rotation == BSP_ROTATION_270;
    return transposed ? bsp_size_t{s_size.height, s_size.width} : s_size;
}

bsp_rect_t panel_native_rect(const bsp_rect_t &rect, bsp_rotation_t rotation) {
    const bsp_size_t size = panel_logical_size(rotation);
    const int x = rect.origin.x, y = rect.origin.y;
    const int w = rect.size.width, h = rect.size.height;
    switch (rotation) {
        case BSP_ROTATION_90: return {{y, size.width - x - w}, {h, w}};
        case BSP_ROTATION_180: return {{size.width - x - w, size.height - y - h}, {w, h}};
        case BSP_ROTATION_270: return {{size.height - y - h, x}, {h, w}};
        default: return rect;
    }
}

bsp_pixel_format_t panel_format() {
    return s_format;
}

int panel_framebuffers() {
    return s_framebuffers;
}

void panel_blit_wait() {
    for (; s_blits_pending; s_blits_pending--) xSemaphoreTake(s_blits_done, portMAX_DELAY);
}

void panel_swap() {
    panel_blit_wait();
    if (s_clipped) return;
    s_front = (s_front + 1) % s_framebuffers;
    display_manager.present(s_front);
}

void panel_clear() {
    for (int i = 0; i < s_framebuffers; i++) {
        clear_area(i, s_clipped && i == s_front ? s_clip : bsp_rect_t{{0, 0}, s_size});
    }
    if (s_saved) clear_area(s_saved, {{0, 0}, s_size});
}

esp_err_t panel_copy_to_back(bsp_rect_t area) {
    if (s_clipped || !intersect(&area, {{0, 0}, s_size})) return ESP_OK;
    return copy_area(framebuffer(PanelBuffer::Back), framebuffer(PanelBuffer::Front), area);
}

esp_err_t panel_rotate_180() {
    const bsp_rect_t all = {{0, 0}, s_size};
    if (!s_clipped) {
        esp_err_t err = copy_area(framebuffer(PanelBuffer::Back), framebuffer(PanelBuffer::Front),
                                  all, BSP_ROTATION_180);
        if (err == ESP_OK) panel_swap();
        return err;
    }
    if (s_framebuffers < 3 || !s_saved) return ESP_ERR_NOT_SUPPORTED;
    uint8_t *whole = framebuffer_at((s_front + 1) % s_framebuffers);
    uint8_t *turned = framebuffer_at((s_front + 2) % s_framebuffers);
    bsp_rect_t parts[4];
    outside(s_clip, parts);
    esp_err_t err = copy_area(whole, framebuffer_at(s_front), s_clip);
    for (const bsp_rect_t &part : parts) {
        if (err == ESP_OK) err = copy_area(whole, s_saved, part);
    }
    if (err == ESP_OK) err = copy_area(turned, whole, all, BSP_ROTATION_180);
    if (err == ESP_OK) err = copy_area(framebuffer_at(s_front), turned, s_clip);
    for (const bsp_rect_t &part : parts) {
        if (err == ESP_OK) err = copy_area(s_saved, turned, part);
    }
    return err;
}

esp_err_t panel_fill(PanelBuffer buffer, bsp_rect_t area, uint32_t rgb888) {
    if (area.origin.x < 0 || area.origin.y < 0 || area.size.width <= 0 ||
        area.size.height <= 0 || area.origin.x + area.size.width > s_size.width ||
        area.origin.y + area.size.height > s_size.height) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_clipped && !intersect(&area, s_clip)) return ESP_OK;
    ppa_fill_oper_config_t config = {};
    config.out.buffer = framebuffer(buffer);
    config.out.buffer_size = s_stride * s_size.height;
    config.out.pic_w = s_size.width;
    config.out.pic_h = s_size.height;
    config.out.block_offset_x = area.origin.x;
    config.out.block_offset_y = area.origin.y;
    config.out.fill_cm = s_format == BSP_PIXEL_FORMAT_RGB888 ? PPA_FILL_COLOR_MODE_RGB888
                                                             : PPA_FILL_COLOR_MODE_RGB565;
    config.fill_block_w = area.size.width;
    config.fill_block_h = area.size.height;
    config.fill_argb_color.a = 0xff;
    config.fill_argb_color.r = rgb888 >> 16;
    config.fill_argb_color.g = rgb888 >> 8;
    config.fill_argb_color.b = rgb888;
    config.mode = PPA_TRANS_MODE_BLOCKING;
    return ppa_do_fill(s_fill, &config);
}

esp_err_t panel_write_rows(PanelBuffer buffer, int y, int height, const uint8_t *pixels) {
    if (!rows_valid(y, height)) return ESP_ERR_INVALID_ARG;
    return panel_blit(rows_blit(buffer, y, height), pixels);
}

esp_err_t panel_write_lz4_rows(PanelBuffer buffer, int y, int height, const uint8_t *lz4,
                               uint32_t bytes) {
    if (!rows_valid(y, height) || y % kRowAlign || height % kRowAlign) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_clipped) return panel_blit_lz4(rows_blit(buffer, y, height), lz4, bytes);
    uint8_t *dst = framebuffer(buffer) + y * s_stride;
    const size_t size = height * s_stride;
    const ptrdiff_t written = lz4_block_decompress(lz4, bytes, dst, size);
#ifdef ESP_PLATFORM
    esp_cache_msync(dst, size, ESP_CACHE_MSYNC_FLAG_DIR_C2M);
#endif
    if (written != (ptrdiff_t)size) {
        ESP_LOGW(TAG, "lz4 at %d: %d of %u bytes", y, (int)written, (unsigned)size);
        return ESP_ERR_INVALID_SIZE;
    }
    return ESP_OK;
}

esp_err_t panel_decode_rows(PanelBuffer buffer, int y, int height, const uint8_t *jpeg,
                            uint32_t bytes) {
    if (!rows_valid(y, height) || y % kRowAlign || height % kRowAlign) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_pipeline) return ESP_ERR_INVALID_STATE;
    if (s_clipped) return panel_blit_jpeg(rows_blit(buffer, y, height), jpeg, bytes);
    jpeg_enh_frame_info_t info = {};
    esp_err_t err = jpeg_enh_decoder_process(jpeg_ppa_pipeline_get_decoder(s_pipeline), jpeg,
                                             bytes, framebuffer(buffer) + y * s_stride,
                                             height * s_stride, &info);
    if (err == ESP_OK && (info.origin_w != (uint32_t)s_size.width ||
                          info.origin_h != (uint32_t)height)) {
        err = ESP_ERR_INVALID_SIZE;
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "jpeg at %d: %s, %ux%u", y, esp_err_to_name(err),
                 (unsigned)info.origin_w, (unsigned)info.origin_h);
    }
    return err;
}

/* Sixteenths each input axis scales by; ESP_ERR_NOT_SUPPORTED when `area` is
 * not a whole number of sixteenths of the rotated source. */
static esp_err_t blit_scale(const PanelBlit &blit, uint32_t *scale_x, uint32_t *scale_y) {
    const bsp_rect_t &area = blit.area;
    if (blit.source.width <= 0 || blit.source.height <= 0 || area.origin.x < 0 ||
        area.origin.y < 0 || area.size.width <= 0 || area.size.height <= 0 ||
        area.origin.x + area.size.width > s_size.width ||
        area.origin.y + area.size.height > s_size.height) {
        return ESP_ERR_INVALID_ARG;
    }
    const bool transposed = blit.rotation == BSP_ROTATION_90 || blit.rotation == BSP_ROTATION_270;
    const uint32_t out_x = (transposed ? area.size.height : area.size.width) * kScaleDenominator;
    const uint32_t out_y = (transposed ? area.size.width : area.size.height) * kScaleDenominator;
    if (out_x % blit.source.width || out_y % blit.source.height) return ESP_ERR_NOT_SUPPORTED;
    *scale_x = out_x / blit.source.width;
    *scale_y = out_y / blit.source.height;
    return *scale_x && *scale_y ? ESP_OK : ESP_ERR_NOT_SUPPORTED;
}

static uint32_t scaled(uint32_t value, uint32_t scale) {
    return value * scale / kScaleDenominator;
}

/* Source pixels [start, end) of an axis whose output stays inside [out0,
 * out1). Edges round inward to a whole output pixel: PPA never finishes a
 * block that ends mid pixel. */
static bool clip_axis(uint32_t out0, uint32_t out1, uint32_t scale, uint32_t length,
                      uint32_t *start, uint32_t *end) {
    const uint32_t step = kScaleDenominator / std::gcd(kScaleDenominator, scale);
    uint32_t a = (out0 * kScaleDenominator + scale - 1) / scale;
    uint32_t b = std::min(out1 * kScaleDenominator / scale, length);
    a = (a + step - 1) / step * step;
    if (b < length) b -= b % step;
    if (b <= a) return false;
    *start = a;
    *end = b;
    return true;
}

struct Target {
    bsp_rect_t visible;
    uint8_t *buffer;
};

/* Where a draw over `blit.area` lands: the framebuffer inside the clip, and
 * the copy kept of what lies outside it. */
static int draw_targets(const PanelBlit &blit, Target targets[5]) {
    int count = 0;
    bsp_rect_t visible = blit.area;
    if (!s_clipped || intersect(&visible, s_clip)) {
        targets[count++] = {visible, framebuffer(blit.buffer)};
    }
    if (!s_clipped || !s_saved) return count;
    bsp_rect_t parts[4];
    outside(s_clip, parts);
    for (bsp_rect_t part : parts) {
        if (!empty(part) && intersect(&part, blit.area)) targets[count++] = {part, s_saved};
    }
    return count;
}

/* Source rows [row, row + rows) as their own block, placed where they land
 * in the rotated, scaled source and cut to `target`; false when nothing of
 * them lands there. */
static bool strip_config(const PanelBlit &blit, uint32_t scale_x, uint32_t scale_y,
                         const void *pixels, int row, int rows, const Target &target,
                         ppa_srm_oper_config_t *config) {
    const bsp_rect_t &area = blit.area;
    const bsp_rect_t &visible = target.visible;
    const uint32_t ex0 = visible.origin.x - area.origin.x;
    const uint32_t ex1 = ex0 + visible.size.width;
    const uint32_t ey0 = visible.origin.y - area.origin.y;
    const uint32_t ey1 = ey0 + visible.size.height;
    const uint32_t sw = scaled(blit.source.width, scale_x);
    const uint32_t sh = scaled(blit.source.height, scale_y);
    uint32_t u0, u1, v0, v1;
    switch (blit.rotation) {
        case BSP_ROTATION_90: u0 = sw - ey1; u1 = sw - ey0; v0 = ex0; v1 = ex1; break;
        case BSP_ROTATION_180: u0 = sw - ex1; u1 = sw - ex0; v0 = sh - ey1; v1 = sh - ey0; break;
        case BSP_ROTATION_270: u0 = ey0; u1 = ey1; v0 = sh - ex1; v1 = sh - ex0; break;
        default: u0 = ex0; u1 = ex1; v0 = ey0; v1 = ey1; break;
    }
    uint32_t col0, col1, row0, row1;
    if (!clip_axis(u0, u1, scale_x, blit.source.width, &col0, &col1) ||
        !clip_axis(v0, v1, scale_y, blit.source.height, &row0, &row1)) {
        return false;
    }
    row0 = std::max<uint32_t>(row0, row);
    row1 = std::min<uint32_t>(row1, row + rows);
    if (row1 <= row0) return false;

    const uint32_t cs = scaled(col0, scale_x), ce = scaled(col1, scale_x);
    const uint32_t rs = scaled(row0, scale_y), re = scaled(row1, scale_y);
    int x = area.origin.x;
    int y = area.origin.y;
    switch (blit.rotation) {
        case BSP_ROTATION_90: x += rs; y += sw - ce; break;
        case BSP_ROTATION_180: x += sw - ce; y += sh - re; break;
        case BSP_ROTATION_270: x += sh - re; y += cs; break;
        default: x += cs; y += rs; break;
    }

    *config = {};
    config->in.buffer = pixels;
    config->in.pic_w = blit.source.width;
    config->in.pic_h = rows;
    config->in.block_offset_x = col0;
    config->in.block_offset_y = row0 - row;
    config->in.block_w = col1 - col0;
    config->in.block_h = row1 - row0;
    config->in.srm_cm = srm_mode(blit.format);
    config->out.buffer = target.buffer;
    config->out.buffer_size = s_stride * s_size.height;
    config->out.pic_w = s_size.width;
    config->out.pic_h = s_size.height;
    config->out.block_offset_x = x;
    config->out.block_offset_y = y;
    config->out.srm_cm = srm_mode(s_format);
    config->rotation_angle = srm_angle(blit.rotation);
    config->scale_x = (float)scale_x / kScaleDenominator;
    config->scale_y = (float)scale_y / kScaleDenominator;
    config->mode = PPA_TRANS_MODE_BLOCKING;
    return true;
}

static esp_err_t blit(const PanelBlit &blit, const uint8_t *pixels, bool wait) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    Target targets[5];
    const int count = draw_targets(blit, targets);
    for (int i = 0; i < count && err == ESP_OK; i++) {
        ppa_srm_oper_config_t config;
        if (!strip_config(blit, scale_x, scale_y, pixels, 0, blit.source.height, targets[i],
                          &config)) {
            continue;
        }
        if (!wait && i == count - 1) {
            if (s_blits_pending == kMaxPendingBlits) {
                xSemaphoreTake(s_blits_done, portMAX_DELAY);
                s_blits_pending--;
            }
            config.mode = PPA_TRANS_MODE_NON_BLOCKING;
            config.user_data = s_blits_done;
            err = ppa_do_scale_rotate_mirror(s_copy, &config);
            if (err == ESP_OK) s_blits_pending++;
            continue;
        }
        err = ppa_do_scale_rotate_mirror(s_copy, &config);
    }
    return err;
}

esp_err_t panel_blit(const PanelBlit &blit, const uint8_t *pixels) {
    return ::blit(blit, pixels, true);
}

esp_err_t panel_blit_async(const PanelBlit &blit, const uint8_t *pixels) {
    return ::blit(blit, pixels, false);
}

esp_err_t panel_blit_lz4(const PanelBlit &blit, const uint8_t *lz4, uint32_t bytes) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    const size_t row_bytes = blit.source.width * bsp_pixel_format_bytes(blit.format);
    const int strip_rows = s_strip_bytes / row_bytes / kRowAlign * kRowAlign;
    // Every match reaches back at most 64 KiB, so a whole strip before the
    // one being written is all the history it needs.
    if (strip_rows * row_bytes < kLz4Window) return ESP_ERR_NOT_SUPPORTED;

    Target targets[5];
    const int count = draw_targets(blit, targets);
    lz4_stream_t stream;
    lz4_stream_init(&stream, lz4, bytes);
    size_t previous = 0;
    int strip = 0;
    for (int row = 0; row < blit.source.height && err == ESP_OK; row += strip_rows) {
        const int rows = std::min(strip_rows, blit.source.height - row);
        const size_t size = rows * row_bytes;
        wait_strip(strip);
        const int64_t diag_start = esp_timer_get_time();
        const ptrdiff_t written = lz4_stream_decompress(&stream, s_strips[strip], size,
                                                        s_strips[strip ^ 1], previous);
        panel_diag_lz4_us += esp_timer_get_time() - diag_start;
        if (written != (ptrdiff_t)size) {
            err = ESP_ERR_INVALID_SIZE;
            break;
        }
        previous = size;
        // Only the last block of a strip runs behind the decoding of the next.
        for (int i = 0; i < count && err == ESP_OK; i++) {
            ppa_srm_oper_config_t config;
            if (!strip_config(blit, scale_x, scale_y, s_strips[strip], row, rows, targets[i],
                              &config)) {
                continue;
            }
            if (i == count - 1) {
                config.mode = PPA_TRANS_MODE_NON_BLOCKING;
                config.user_data = s_strip_done[strip];
                s_strip_busy[strip] = true;
            }
            err = ppa_do_scale_rotate_mirror(s_copy, &config);
            if (err != ESP_OK) s_strip_busy[strip] = false;
        }
        strip ^= 1;
    }
    for (int i = 0; i < kStrips; i++) wait_strip(i);
    if (err == ESP_OK && !lz4_stream_finished(&stream)) err = ESP_ERR_INVALID_SIZE;
    if (err != ESP_OK) ESP_LOGW(TAG, "lz4 blit: %s", esp_err_to_name(err));
    return err;
}

esp_err_t panel_blit_jpeg(const PanelBlit &blit, const uint8_t *jpeg, uint32_t bytes) {
    uint32_t scale_x, scale_y;
    esp_err_t err = blit_scale(blit, &scale_x, &scale_y);
    if (err != ESP_OK) return err;
    if (!s_pipeline) return ESP_ERR_INVALID_STATE;

    jpeg_ppa_output_t out = {};
    out.buffer = framebuffer(blit.buffer);
    out.buffer_size = s_stride * s_size.height;
    out.pic_w = s_size.width;
    out.pic_h = s_size.height;
    out.color_mode = srm_mode(s_format);
    jpeg_ppa_transform_t transform = {};
    transform.rotation = srm_angle(blit.rotation);
    transform.scale_x = (float)scale_x / kScaleDenominator;
    transform.scale_y = (float)scale_y / kScaleDenominator;
    transform.out_offset_x = blit.area.origin.x;
    transform.out_offset_y = blit.area.origin.y;
    if (s_clipped) {
        transform.out_clip = {(uint32_t)s_clip.origin.x, (uint32_t)s_clip.origin.y,
                              (uint32_t)s_clip.size.width, (uint32_t)s_clip.size.height};
    }
    jpeg_enh_frame_info_t info = {};
    err = jpeg_ppa_pipeline_process(s_pipeline, jpeg, bytes, &out, &transform, &info);
    if (err == ESP_OK && (info.origin_w != (uint32_t)blit.source.width ||
                          info.origin_h != (uint32_t)blit.source.height)) {
        err = ESP_ERR_INVALID_SIZE;
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "jpeg blit: %s, %ux%u", esp_err_to_name(err), (unsigned)info.origin_w,
                 (unsigned)info.origin_h);
    }
    return err;
}
