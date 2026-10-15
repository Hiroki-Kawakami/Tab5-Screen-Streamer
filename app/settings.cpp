/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#include "settings.hpp"
#include "display/panel.hpp"
#include "nvs_flash.h"
#include "ui_orientation.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <type_traits>
#include <utility>

#ifndef NVS_KEY_NAME_MAX_SIZE
#define NVS_KEY_NAME_MAX_SIZE 16
#endif

namespace {

nvs_handle_t s_nvs;
bool s_nvs_open;

template <std::size_t N>
struct Key {
    static_assert(N <= NVS_KEY_NAME_MAX_SIZE, "NVS names are limited to 15 characters");

    char text[N];
    consteval Key(const char (&literal)[N]) {
        for (std::size_t i = 0; i < N; i++) text[i] = literal[i];
    }
};

constexpr Key kNamespace{"tab5streamer"};

template <typename T>
struct Codec;

template <>
struct Codec<uint8_t> {
    static esp_err_t get(nvs_handle_t nvs, const char *key, uint8_t &value) {
        return nvs_get_u8(nvs, key, &value);
    }
    static esp_err_t set(nvs_handle_t nvs, const char *key, uint8_t value) {
        return nvs_set_u8(nvs, key, value);
    }
};

template <typename F>
struct SanitizeInput {
    using type = void;
};
template <typename R, typename A>
struct SanitizeInput<R (*)(A)> {
    using type = A;
};

template <Key K, typename T, auto Sanitize = nullptr>
class Setting {
    static constexpr bool kSanitized = !std::is_null_pointer_v<decltype(Sanitize)>;

public:
    using Input = std::conditional_t<kSanitized, typename SanitizeInput<decltype(Sanitize)>::type, T>;

    constexpr explicit Setting(T value) : value_(std::move(value)) {}

    const T &get() const { return value_; }

    bool set(Input value) {
        T next = sanitize(std::move(value));
        if (next == value_) return false;
        value_ = std::move(next);
        modified_ = true;
        return true;
    }

    void load(nvs_handle_t nvs) {
        T value{};
        if (Codec<T>::get(nvs, K.text, value) != ESP_OK) return;
        value_ = sanitize(std::move(value));
    }

    bool commit(nvs_handle_t nvs) {
        if (!modified_ || Codec<T>::set(nvs, K.text, value_) != ESP_OK) return false;
        modified_ = false;
        return true;
    }

private:
    static T sanitize(Input value) {
        if constexpr (kSanitized) {
            return Sanitize(std::move(value));
        } else {
            return value;
        }
    }

    T value_;
    bool modified_ = false;
};

Setting<"brightness", uint8_t, +[](int percent) -> uint8_t {
    return std::clamp(percent, kMinDisplayBrightness, 100);
}> s_display_brightness{80};

Setting<"colordepth", uint8_t, +[](int bits) -> uint8_t {
    return bits == 24 ? 24 : 16;
}> s_display_color_depth{16};

constexpr uint8_t kRotationLocked = 4;

Setting<"rotation", uint8_t, +[](int value) -> uint8_t {
    return value >= 0 && value <= (kRotationLocked | BSP_ROTATION_270) ? value : 0;
}> s_rotation{0};

constexpr auto sanitize_flag = +[](bool enabled) -> uint8_t {
    return enabled ? 1 : 0;
};

Setting<"equalizer", uint8_t, sanitize_flag> s_equalizer{1};

Setting<"lastmode", uint8_t, +[](int mode) -> uint8_t {
    return mode >= 0 && mode <= (int)AppMode::PcApp ? mode : 0;
}> s_last_mode{(uint8_t)AppMode::None};

Setting<"usbaudio", uint8_t, sanitize_flag> s_usb_audio{1};

Setting<"usbhid", uint8_t, sanitize_flag> s_usb_hid{1};

Setting<"touchinput", uint8_t, sanitize_flag> s_touch_input{1};

template <typename Fn>
void for_each_setting(Fn &&fn) {
    fn(s_display_brightness);
    fn(s_display_color_depth);
    fn(s_rotation);
    fn(s_equalizer);
    fn(s_last_mode);
    fn(s_usb_audio);
    fn(s_usb_hid);
    fn(s_touch_input);
}

}  // namespace

void settings_init() {
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        nvs_flash_erase();
        err = nvs_flash_init();
    }
    s_nvs_open = err == ESP_OK && nvs_open(kNamespace.text, NVS_READWRITE, &s_nvs) == ESP_OK;
    if (s_nvs_open) for_each_setting([](auto &setting) { setting.load(s_nvs); });
}

void settings_commit() {
    if (!s_nvs_open) return;
    bool written = false;
    for_each_setting([&written](auto &setting) { written |= setting.commit(s_nvs); });
    if (written) nvs_commit(s_nvs);
}

int settings_display_brightness() {
    return s_display_brightness.get();
}

void settings_set_display_brightness(int percent) {
    if (!s_display_brightness.set(percent)) return;
    bsp_display_set_brightness(s_display_brightness.get());
}

bsp_pixel_format_t settings_display_pixel_format() {
    return s_display_color_depth.get() == 24 ? BSP_PIXEL_FORMAT_RGB888 : BSP_PIXEL_FORMAT_RGB565;
}

esp_err_t settings_set_display_pixel_format(bsp_pixel_format_t format) {
    if (format == settings_display_pixel_format()) return ESP_OK;
    const esp_err_t err = panel_set_format(format);
    if (err != ESP_OK) return err;
    s_display_color_depth.set((int)bsp_pixel_format_bytes(format) * 8);
    return ESP_OK;
}

bool settings_rotation_locked() {
    return (s_rotation.get() & kRotationLocked) != 0;
}

bsp_rotation_t settings_rotation() {
    return (bsp_rotation_t)(s_rotation.get() & ~kRotationLocked);
}

void settings_set_rotation(bsp_rotation_t rotation) {
    s_rotation.set((s_rotation.get() & kRotationLocked) | rotation);
}

void settings_set_rotation_lock(bool locked) {
    const bsp_rotation_t rotation = ui_orientation_current();
    if (!s_rotation.set((locked ? kRotationLocked : 0) | rotation)) return;
    ui_orientation_set_locked(locked, rotation);
}

bool settings_equalizer_enabled() {
    return s_equalizer.get() != 0;
}

void settings_set_equalizer_enabled(bool enabled) {
    if (!s_equalizer.set(enabled)) return;
    bsp_audio_set_eq_enabled(enabled);
}

AppMode settings_last_mode() {
    return (AppMode)s_last_mode.get();
}

void settings_set_last_mode(AppMode mode) {
    s_last_mode.set((int)mode);
}

bool settings_usb_audio() {
    return s_usb_audio.get() != 0;
}

void settings_set_usb_audio(bool enabled) {
    s_usb_audio.set(enabled);
}

bool settings_usb_hid() {
    return s_usb_hid.get() != 0;
}

void settings_set_usb_hid(bool enabled) {
    s_usb_hid.set(enabled);
}

bool settings_touch_input() {
    return s_touch_input.get() != 0;
}

void settings_set_touch_input(bool enabled) {
    s_touch_input.set(enabled);
}
