# Tab5-Screen-Streamer

A "PC screen streamer" for the M5Stack Tab5 (ESP32-P4). It decodes JPEG frames
streamed from a PC over USB and displays them on the panel.

## Build (important)
- The toolchain (`idf.py` / `riscv32-esp-elf-gcc` / `ninja` / `cmake`) is available
  **only inside the nix devShell** — it is not on the bare shell PATH. `.envrc` is `use flake`.
- Build command:
  ```
  nix develop --command bash -c 'cd esp32p4 && idf.py build'
  ```
  (run from the repo root; `esp32p4/` is the IDF project root)
- Target is `esp32p4`. `build/` already exists, so re-configure is fast.
- macOS here has no `timeout` command (no GNU coreutils). Use the Bash tool's
  `timeout` argument instead of the CLI.

## Layout
- `esp32p4/` — IDF project root. `EXTRA_COMPONENT_DIRS=../idf-components`.
- `idf-components/main/` — the component is **named `main`**, so IDF treats it specially
  and it **auto-depends on every component**. That's why it can include other components'
  public headers without listing `REQUIRES` (add a new component and `main` sees it automatically).
  - `main.cpp` — implements the `pf_port` namespace (`platform_port.hpp`): `display_*`,
    `psram_malloc`, `init()`, etc. Panel is **720x1280**; `init(3, pf)` allocates **3 framebuffers**.
  - `platform_port_jpeg.cpp` — `pf_port::JpegDecoder` (wrapper around the JPEG decoder).
  - `platform_port_ppa.cpp` — `pf_port::SRMClient` (PPA scale/rotate/mirror).
- `idf-components/main/CMakeLists.txt` — **globs `app/` and `components/*` into MAIN_SRCS**,
  so files like `app/preview_screen.cpp` need no explicit registration (drop them in and they build).
- `app/` — screen/app logic (`streamer.cpp`, `preview_screen.cpp`).
- `components/` — `lvgl++`, `screen_manager` (the `Screen` base class + lifecycle).
- `idf-components/streamer_usb/` — USB vendor-class receive API (`streamer_usb_vendor_read`, etc.).
- `streamer-rs/` — the **PC-side sender** (Rust crate `tab5-screen-streamer`). Captures the
  screen (ScreenCaptureKit on macOS, windows-capture on Windows, scap on Linux), resizes to
  720x1280-equivalent, JPEG-encodes via turbojpeg, and streams over USB with `nusb`.
  - `src/main.rs` — USB device open + the send loop. Opens VID `0xf055` / PID `0x1118`,
    bulk OUT endpoint `0x01`, and checks `bcdDevice` matches `DEVICE_VERSION` (0x0200) — the
    PC tool and firmware must be version-matched. The bulk writer uses `.writer(8192)
    .with_num_transfers(8)` (8 KB per transfer, 8 in-flight URBs), so the PC side already
    pipelines; receive throughput is gated device-side (see EP DMA buffer note below).
  - `src/capture/mod.rs` — `encode_split_frame()` builds the wire format (band header below).
  - `src/capture/{macos,windows,common}.rs` — per-platform capture backends.

## JPEG streaming wire format (PC → Tab5)
Built by `encode_split_frame()` in `streamer-rs/src/capture/mod.rs`; parsed by
`renderer_task` in `app/preview_screen.cpp`. **Not** a single JPEG per frame — each frame is
split into `SPLIT_COUNT = 16` horizontal bands (in the device's portrait orientation), and
each band is independently JPEG-encoded and prefixed with an **8-byte band header**:
- byte 0     : **type / sync** — `0x50` = decode only, `0x51` = last band of the frame → decode then present (flip framebuffer)
- bytes 1..3 : **data size**, 24-bit little-endian = the 4 coordinate bytes + JPEG payload length.
  So **JPEG payload length = data_size − 4** (`COORD_SIZE`).
- byte 4     : x / 16  (always 0 — bands are full-width)
- byte 5     : y / 16
- byte 6     : width / 16  (always 720/16)
- byte 7     : height / 16
Landscape captures (`src_width > src_height`) are rotated 270° so the device always receives a
portrait image; each band is one source strip rotated independently.
- `preview_screen.cpp` splits receive (renderer task) from decode + framebuffer write
  (decoder task). The renderer accumulates a frame's 16 bands into one of 8 ring input buffers
  (512KB each, PSRAM) and, on the `0x51` band, hands the assembled frame to the decoder via a
  capacity-1 `xQueueOverwrite`, so the decoder always processes only the newest frame.
- The stream carries no global sync marker. After a stall/corruption `resync_to_frame_start()`
  slides a 1-byte window until it finds a plausible first band (type 0x50/0x51, x==0, y==0,
  width==720) to re-align to a frame boundary.

## USB receive throughput (EP DMA buffer)
The bulk OUT max packet size is 512 (HS), fixed in the vendor descriptor. Throughput is **not**
limited by that but by `CFG_TUD_VENDOR_EPSIZE` in `idf-components/streamer_usb/tusb_config.h` —
the size of one OUT DMA transfer. DWC2 in DMA mode receives multiple 512-byte packets back-to-back
into that buffer in a single transfer (`tu_edpt_stream_read_xfer` caps the xfer length at
`ep_bufsize`), so 512 there meant one re-arm per packet (~80 Mbps ceiling). It is set to **4096**
(8 packets/transfer) with `CFG_TUD_VENDOR_RX_BUFSIZE = 32768` to keep the host bursting. EPSIZE
must stay a multiple of 512 and ≥ 512 (a smaller buffer overflows the per-EP DMA region).

## Full-range JPEG color conversion
- **Why**: the IDF `jpeg_decoder_process` does YUV→RGB with **limited-range BT.601** (Y∈[16,235]),
  but MJPEG/JFIF content is **full-range** (Y∈[0,255]). Using the limited-range matrix lifts blacks
  and compresses whites, giving a washed-out image.
- **Fix**: a new component `idf-components/jpeg_fullrange_decode/` writes a full-range BT.601 matrix
  into the 2D-DMA CSC registers. It exposes `jpeg_decoder_process_full_range()`, which
  `JpegDecoder::decode()` in `platform_port_jpeg.cpp` calls.
- **Why a separate component**: the CSC matrix write (`dma2d_configure_color_space_conversion`)
  happens inside `jpeg_decoder_process`'s `on_job_picked` callback, immediately followed by the DMA
  start. There's no hook between "configured" and "transfer started", so the **process step is
  reimplemented** to overwrite the matrix right after CSC config. Engine creation still reuses the
  IDF `jpeg_new_decoder_engine`, so there's no symbol clash.
- **Never modify IDF itself** (it's also read-only in the nix store). When a driver change is needed,
  copy/reimplement it under `idf-components/`.
- Because it reaches into the IDF JPEG decoder internals, `CMakeLists.txt` adds the IDF private header
  paths (`$ENV{IDF_PATH}/components/esp_driver_jpeg` and `.../private`) via `target_include_directories`.
- Full-range BT.601 coefficients (×256): R=Y+1.402·(Cr−128) / G=Y−0.344·(Cb−128)−0.714·(Cr−128) /
  B=Y+1.772·(Cb−128). Only BT.601 RGB565/RGB888 output is range-adjusted; BT.709 / YUV / GRAY behave
  exactly as the IDF driver does.
