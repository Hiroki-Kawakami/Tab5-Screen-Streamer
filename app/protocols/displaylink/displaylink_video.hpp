/*
 * SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Hiroki Kawakami
 */

#pragma once
#include <cstdint>
#include <functional>
#include "bsp.h"
#include "display/panel_damage.hpp"
#include "displaylink_memory.hpp"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#ifdef ESP_PLATFORM
#include "driver/ppa.h"
#endif

/* Runs the host's command stream on its own task against the adapter's
 * graphics memory, and puts the 16 bit framebuffer it shows on the panel,
 * which is taken while a mode is set. */
class DisplayLinkVideo {
public:
    using Done = void (*)(void *context);

    /* `streaming` runs on the video task when the panel is taken and given
     * back, `display_power` when the host blanks or unblanks. */
    DisplayLinkVideo(std::function<void(bool)> streaming, std::function<void(bool)> display_power)
        : streaming_(std::move(streaming)), display_power_(std::move(display_power)) {}
    ~DisplayLinkVideo();
    esp_err_t start(bsp_rotation_t rotation);

    void rotate(bsp_rotation_t rotation);
    /* The next bytes of the stream. `done` runs on the video task once they
     * are no longer read. */
    void submit(const uint8_t *data, uint32_t length, Done done, void *context);
    /* The stream starts over. */
    void end();

private:
    enum class Op : uint8_t { Data, Rotate, End, Quit };

    struct Item {
        Op op;
        uint32_t value;
        const uint8_t *data;
        uint32_t length;
        Done done;
        void *context;
    };

    enum class State : uint8_t {
        Idle,
        Command,
        Header,
        RawPixels,
        RunCount,
        RunPixel,
        SpanCount,
        SpanPixels,
        RepeatCount,
        TableEntries,
        Bits,
    };

    /* What a bit read in a state of the decompression table adds to the
     * color, and the state it leads to; state 0 ends a pixel. */
    struct Branch {
        uint16_t color;
        uint16_t jump;
    };

    /* Four bits from a state at once: the color added up to each pixel they
     * end, by all of them, and the offset of the steps from the state after. */
    struct Step {
        uint16_t sums[4];
        uint16_t total;
        uint16_t pixels;
        uint32_t next;
    };

    /* A framebuffer the host has shown, with the rows to draw from memory
     * rather than from a window. */
    struct Region {
        uint32_t base;
        int first;
        int last;
        uint32_t used;
    };

    static constexpr int kTileRows = 32;

    /* A tile of a region held in the panel's strips while it is written;
     * `dirty_*` are the rows written since the last flush. A row is read
     * from memory only once what is written to it would not cover it: until
     * then [`left`, `right`) is what holds pixels. */
    struct Window {
        uint8_t *pixels;
        int region;
        int row;
        int x;
        int width;
        int rows;
        int dirty_first;
        int dirty_last;
        uint32_t used;
        uint32_t loaded;
        uint32_t read_until;
        bool blitted;
        int16_t left[kTileRows];
        int16_t right[kTileRows];
        bool valid;
    };

    static constexpr int kRegions = 4;
    static constexpr int kMaxWindows = 16;
    static constexpr int kSpareWindows = 1;

    /* A strip slice out of the windows, swapped with the one a window leaves
     * so that its pixels may still be read by the copies queued at its flush.
     * `read_until` is the write-back reading it last; `blitted`, that a draw
     * may read it after that. */
    struct Spare {
        uint8_t *pixels;
        uint32_t read_until;
        bool blitted;
    };

    static void task(void *arg);
    void send(Op op, uint32_t value = 0);
    void run(const Item &item);
    void parse(const uint8_t *data, uint32_t length);
    void begin_command();
    void load_table();
    void table_entry();
    void build_steps(Step *steps, uint16_t start);
    template <typename Pixel>
    [[gnu::noinline]] const uint8_t *bits(const uint8_t *p, const uint8_t *end);
    bool pixel_byte(uint8_t byte);
    void put(uint16_t pixel);
    void repeat(uint16_t pixel, uint32_t count);
    uint32_t here() const;
    void write_register(uint8_t reg, uint8_t value);
    void apply_registers();
    void set_mode(bsp_size_t size);
    void show(uint32_t base);
    int region_of(uint32_t address, uint32_t *offset) const;
    int window_at(int region, int row, int x);
    void target();
    void target_();  // DIAG
    void load(Window &window, int region, int row, int x);
    void cover(Window &window, int row, int left, int right);
    void complete(Window &window, int first, int last);
    void flush(Window &window);
    void invalidate(uint32_t address, uint32_t bytes);
    void drop_windows();
    void setup_windows();
    void write_back(Window &window, int first, int rows);
    void wait_write_backs();
    void wait_write_backs(uint32_t sequence);
    void wait_rows(int region, int first, int last);
    void release(uint32_t read_until, bool blitted);
    void draw(int row, int rows, int x, int width, const uint8_t *pixels);
    void draw_from_memory(int region);
    void mark(uint32_t address, uint32_t bytes);
    bool pending() const;
    void commit();
    void update_panel();
    bool fits(bsp_rotation_t rotation) const;
    void reset_stream();

    std::function<void(bool)> streaming_;
    std::function<void(bool)> display_power_;
    QueueHandle_t queue_ = nullptr;
    SemaphoreHandle_t exited_ = nullptr;

    State state_ = State::Idle;
    uint8_t command_ = 0;
    uint8_t header_[9] = {};
    uint8_t header_bytes_ = 0;
    uint8_t header_length_ = 0;
    uint8_t pixel_bytes_ = 2;
    uint8_t pixel_high_ = 0;
    bool pixel_half_ = false;
    uint16_t pixel_ = 0;
    uint32_t address_ = 0;
    uint32_t total_ = 0;
    uint32_t written_ = 0;
    uint32_t count_ = 0;
    uint8_t *out_ = nullptr;
    uint8_t *out_end_ = nullptr;
    Branch *table_ = nullptr;
    Step *steps_ = nullptr;
    Step *steps8_ = nullptr;
    const Step *active_steps_ = nullptr;
    uint16_t table_states_ = 0;
    uint16_t table_loaded_ = 0;
    uint16_t table_state_ = 0;
    uint16_t color_ = 0;
    uint8_t registers_[256] = {};
    bool locked_ = false;
    bool blank_ = true;

    DisplayLinkMemory vram_;
    bsp_size_t mode_ = {};
    uint32_t base16_ = 0;
    uint32_t stride16_ = 0;
    Region regions_[kRegions] = {};
    int shown_ = -1;
    uint32_t region_clock_ = 0;
    Window windows_[kMaxWindows] = {};
    Spare spares_[kSpareWindows] = {};
#ifdef ESP_PLATFORM
    ppa_client_handle_t copier_ = nullptr;
#endif
    /* A write back in flight: PPA finishes them in order. */
    struct WriteBack {
        uint32_t sequence;
        int region;
        int first;
        int last;
    };

    SemaphoreHandle_t write_backs_done_ = nullptr;
    WriteBack write_backs_[8] = {};
    uint32_t write_backs_sent_ = 0;
    uint32_t write_backs_done_count_ = 0;
    int window_count_ = 0;
    uint32_t window_clock_ = 0;
    int64_t dirty_since_us_ = 0;

    PanelDamage damage_;
    bsp_rotation_t rotation_ = BSP_ROTATION_0;
    bsp_rotation_t shown_rotation_ = BSP_ROTATION_0;
    bool acquired_ = false;

    int64_t stats_start_us_ = 0;
    uint64_t stats_bytes_ = 0;
    uint32_t stats_presents_ = 0;
    uint64_t stats_rows_ = 0;
    int64_t stats_busy_us_ = 0;
    int64_t stats_present_us_ = 0;
};
