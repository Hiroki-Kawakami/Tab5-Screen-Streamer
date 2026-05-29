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

## JPEG streaming wire format (PC → Tab5)
`send_image.py`: `struct.pack("<I", len(jpeg)+4) + jpeg`
- First 4 bytes = **little-endian total frame size, including the 4-byte header**.
  So the JPEG payload length = header value − 4.
- `preview_screen.cpp` splits receive (renderer task) from decode + framebuffer write
  (decoder task). It uses 8 ring input buffers (512KB each, PSRAM) and a capacity-1
  `xQueueOverwrite`, so the decoder always processes only the newest frame.

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
