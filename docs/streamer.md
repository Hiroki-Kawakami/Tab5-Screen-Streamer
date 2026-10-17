# Tab5 Streamer (`tools/streamer-rs`)

The PC side of the [USB protocol](protocol.md). "Tab5 Streamer" is the name
of the PC tools (the App, its window and tray titles, bundle name and
settings folder); "Tab5 Screen Streamer" is the device, its USB product
string and this repository. There are three clients:

| client | role | audio | input |
|---|---|---|---|
| CLI (`streamer`) | scripting and power use: capture a display, play a file or an ffmpeg pipe | bulk, for `play` | touch injection |
| App (`Tab5 Streamer`) | resident in the menu bar or notification area, like a display adapter's manager: streams displays and changes the Tab5's settings | none; UAC covers it | touch injection |
| Web | zero install: plays video files, or shares a screen through `getDisplayMedia` | bulk, in the player only | none |

The CLI and the App run on macOS and Windows; Linux has no capture backend.
The Web client needs a Chromium-based browser (WebUSB, WebCodecs,
`MediaStreamTrackProcessor`).

```sh
nix develop -c bash -c 'cd tools/streamer-rs && cargo run --release -- capture --virtual -v'
nix develop -c bash -c 'cd tools/streamer-rs && cargo run --release -- play in.mp4'
nix develop -c bash -c 'cd tools/streamer-rs && cargo run --release -p streamer-app'
nix develop -c bash -c 'cd tools/streamer-rs && cargo xtask dist'
nix develop -c bash -c 'cd tools/streamer-rs/web && npm ci && npm run dev'   # http://localhost:5174
```

- **Always build with `--release`.** A debug build encodes a 720p JPEG in
  50-150 ms, about 15 fps at best.
- **The CLI and the App cannot use one Tab5 at the same time**: claiming its
  interfaces is exclusive.

## Crates

| crate | holds | platform |
|---|---|---|
| `crates/core` | wire format (`protocol.rs`), layout, JPEG encoder, LZ4, draw batching, the sans-IO presenter, I420 to panel pixels, MP4/MKV demuxers, PCM, scaling and colour conversion | portable, no dependencies; builds for wasm32 |
| `crates/device` | the USB link (nusb), the USB/IP client, sessions and reconnection | portable |
| `crates/capture` | display lists, capture, the virtual display | macOS, Windows |
| `crates/input` | touch injection | macOS, Windows |
| `crates/stream` | the presenter on the wall clock, threaded JPEG bands, the display streaming loop | native |
| `crates/cli` | `streamer` | native |
| `crates/app` | the App (egui) | macOS, Windows |
| `crates/wasm` | `core` for the Web client (wasm-bindgen) | wasm32 |
| `crates/xtask` | `cargo xtask dist` | — |

- **The direction is core ← device ← stream ← cli/app**, with capture and
  input feeding stream. wasm uses core alone; the Web client does its own
  USB in TypeScript.
- **Code from Tab5-Media-Player is copied, not depended on** (the Matroska
  and MP4 demuxers, I/O, JPEG tables, DCT and Huffman, H.264 SPS, YUV,
  resampling, PCM). The copies diverge.
- **`color`, `yuv`, `resample` and `pcm` are used only by wasm.** Native
  capture gets NV12 already scaled from the OS, and `play` gets I420 at the
  panel size from ffmpeg.
- **No per-architecture code** (`std::arch`). Hot loops are written so that
  LLVM vectorises them, plus `simd128` variants for wasm.
  `.cargo/config.toml` builds Windows x86_64 with `target-cpu=x86-64-v2`
  (Windows 11 24H2 requires it anyway; no measurable gain) and wasm with
  `+simd128`.
- **Unit tests are in core**: `cargo test -p streamer-core`.

## USB link

- **The serial number is the device's MAC address** in 12 upper-case hex
  digits. It names the device in `devices`, `--serial`, the App's settings
  and the Web client's storage. USB/IP has none.
- **"Free" means its interfaces can be claimed.** Without `--serial` the CLI
  opens the first Tab5 it can claim; there is no lock file. `devices` claims
  each one briefly to print "(in use)". A reconnect takes any free Tab5
  again, so one process can move to another device.
- **OPEN is resent every 200 ms until an INFO with its seq arrives** (2 s at
  most), and events before it are dropped. A transfer that a killed client
  cut short is joined to the next one on the device, which then drops the
  first OPEN as malformed. In-flight transfers are never cancelled on exit
  for the same reason.
- **KEEPALIVE goes out after 1 s with nothing sent on any OUT endpoint**,
  well inside the device's 3 s session timeout.
- **Reconnection polls every 500 ms** instead of using
  `nusb::watch_devices`: the same loop retries USB/IP, and a device that
  has just arrived cannot always be opened yet.
- **"no SWAP_DONE"** means two swaps went unanswered for 2 s; the session is
  dropped and reopened. A `claim interface` error right after it is
  transient while the old reader thread lets go of the interface.
- **Ctrl+C adds no dependency.** The first SIGINT/SIGTERM (or console close,
  logoff, shutdown on Windows, where the handler waits for it) flushes the
  video and sends CLOSE; a second one exits at once.
- **`--usbip <host:port>` reaches the simulator.** It imports the first
  exported device; `--usbip` and `--serial` exclude each other.

## Presenter

`core/src/present.rs` is everything about a session that is not I/O: the
swap window, the display epoch, redraws, rate control and building the
messages, with times passed in as µs on any monotonic clock.
`crates/stream/src/present.rs` drives it with the wall clock and encodes
JPEG bands on threads; the Web client drives it from wasm.

- **`max_fps` spaces frame starts**, at 0.9 of the interval. Measuring from
  the end of the previous frame dropped every other frame on a slow CPU
  (26-29 fps instead of 51-54 on an Intel N150).
- **The letterbox is cleared, not filled every frame.** Large fills take the
  PSRAM bandwidth the panel's refresh needs. CLEAR goes out on connect,
  relayout and format change only; EV_DAMAGE redraws the content.
- **DISPLAY_POWER on follows the first frame's SWAP** in the same batch, so
  the backlight comes on over the new picture. The Web client holds a
  session while the Tab5 shows its own screen, which may turn the panel off
  in the meantime, so it does this at the start of every stream.
- **Touches are queued only in Event mode.** Leaving it gives an empty
  touch set, so nothing stays pressed.

### JPEG

Baseline 4:2:0 with the Annex K tables scaled by quality. Reading, rotating
and placing the picture on the panel's canvas is one pass: blocks are read
in the source's orientation and the DCT coefficients are rotated
(transposed, signs flipped), not the pixels. Inputs are BGRX, I420 and NV12
in BT.601 full range.

- **Not turbojpeg**: its x86 build needs nasm and breaks, and it does not
  rotate. A 1280x720 frame at quality 80 takes 1.5-1.9 ms on one Apple
  Silicon core (turbojpeg 1.0-1.2 ms without rotation, 2.0-2.4 ms with it),
  0.4-0.5 ms split into 8 bands, at the same PSNR in all four rotations.
- **A frame is `JPEG_BANDS` (4) JPEGs** of whole rows, encoded in parallel
  each drawn as its own band.
- **The DCT is element-wise loops.** Helper functions per row were not
  vectorised and ran 10 times slower; pixels are loaded as whole `u32`s for
  the same reason. An integer DCT is not done.

### Rate control

The USB link, not the encoder, limits the frame rate, so the quality follows
a byte rate:

| options | quality |
|---|---|
| none | from 80 down, to stay near 10 MB/s |
| `--quality N` | fixed at N |
| `--max-rate R` | from 80 down, to stay near R MB/s |
| both | from N down, to stay near R MB/s |

The need is the last frame's size times the frames sent in the last second.
Above the rate the quality drops by `ceil((need/rate - 1) x 20)`, to 20 at
least; below 0.8 of it it rises by 1 (5 below 0.5) at most every 100 ms.
Frames are not skipped to hold the rate: it is a target, not a cap. RAW and
LZ4 are not rate controlled. `-v` prints fps, drops, MB/s and the quality
range every second.

## CLI

The arguments are parsed by hand; `streamer` with no command prints the
usage. Global options (`--serial`, `--usbip`) come before the command.

- **`play <file>` runs ffmpeg itself**: `ffprobe` gives the display size
  (its width and height are before rotation, so the display matrix and SAR
  are applied), the layout gives the output size, and ffmpeg writes raw
  I420 (BT.601 full range) and PCM as live Matroska to stdout, read by
  core's demuxer. ffmpeg and ffprobe come from the dev shell.
- **`play -` reads the same Matroska from another ffmpeg.** It cannot be
  rescaled, so a layout that does not fit is an error.
- **A rotation or device change restarts ffmpeg at `-ss <last pts>`** with
  the new size.
- **Real time**: frames more than 50 ms late are dropped, jumps over 2 s are
  followed, and `max_fps` gates the rest. While the device is away the
  input keeps its time and frames are dropped.
- **Audio goes in 20 ms chunks on the bulk endpoint, about 3 queued on the
  device**, paced by EV_AUDIO's buffered level (device clock). A full bulk
  endpoint left NAKing starves the video endpoint on macOS (30 to 9 fps).
  The device's playing position is the video clock. Rates above
  `audio_max_rate` play without sound.
- **`capture` always injects touches.** `--virtual` makes a display for the
  Tab5 on macOS.

## Capture

The session (`crates/stream/src/capture.rs`) checks once a second for a
mirror set change (restart), the display going away (end), a size change
(relayout) and display sleep: a sleeping display stops the capture and
turns the Tab5's panel off, and waking restarts it.

### macOS

ScreenCaptureKit through objc2 crates only. SCK scales to the panel size
and outputs NV12 (`420f`, BT.601), so the PC never scales.

- **Each frame is copied out in the callback** (0.15-0.28 ms): SCK stalls
  once its few buffers are all held.
- **Below 60 fps is SCK's delivery**: a 60 fps video gives 50-56 callbacks
  and 40-55 new images a second with nothing dropped by the client.
- **Another SCK client (`streamer displays`) stops a running capture**
  without an error.
- **A sleeping display makes SCK stop with "display not found"**;
  `CGDisplayIsAsleep` tells it apart from a display that is gone.
- **Display names come from `NSScreen.localizedName`**, main thread only.
- The virtual display, its identity and mirroring are in
  [`macos-virtual-display.md`](macos-virtual-display.md).
  `tools/scripts/forget-displays.py` resets WindowServer's settings for
  displays that are not connected.

### Windows

Windows.Graphics.Capture through the `windows` crate, scaled and converted
to NV12 (BT.601 full range) by the D3D11 VideoProcessor. It needs D3D11
feature level 11 and Windows 10 1803; there is no second capture path for
older machines.

- **Not the windows-capture crate**: it does not expose the session, which
  is needed to turn `IsCursorCaptureEnabled` on and off while running.
- **The cursor is left out while Windows hides it for touch or pen**,
  polled with `GetCursorInfo` on its own thread, because a cursor that only
  moves brings no frame.
- **WGC keeps running while the display is off**, so sleep comes from
  `GUID_CONSOLE_DISPLAY_STATE` (one state for all displays).
- **A capture started on the lock screen gives black frames after
  unlock**, so every unlock restarts it (a hiccup of about 0.5 s).
- **Display names come from `QueryDisplayConfig`.**
- On an Intel N150 a capture at about 52 fps takes about 68% of one core.
- The firmware attaches WinUSB through MS OS 2.0 descriptors, so no driver
  is installed.

## Touch input

While a client injects touches it sends `SET_INPUT_MODE(Touch, Event)`;
otherwise the Tab5's trackpad (a HID boot mouse) stays, which works
everywhere. A device whose touch input is off (None) is left alone. Panel
points map back into the captured display's desktop rectangle
(`Layout::source_point`).

- **macOS has no public API for touches**: the first finger is the left
  mouse button through CGEvent, with the click count set by hand so double
  clicks work (500 ms, 8 px). It needs Accessibility access, which takes
  effect from the next launch.
- **Windows injects a synthetic touch pointer** with 10 contacts. Contacts
  that are not updated are cancelled, so they are re-sent periodically.
  The process is per-monitor DPI aware (v2) so bounds and points are
  physical pixels.
- **On Windows injected touches do not reach elevated windows or the secure
  desktop** (UIPI); on the lock screen injection is refused. Fixing it needs
  running elevated or `uiAccess` (signed, installed in Program Files).
- **Two Tab5s cannot be touched at once on macOS**: there is one cursor.

## App

A tray icon and a panel that opens from it. The panel has the Tab5
selector, Streaming, Display, Rotation, Touch input, Brightness and Quit.
There is no playback, audio, quality setting or USB/IP.

- **egui on winit + glutin + egui_glow, without eframe.** eframe exits when
  its root window closes, while the panel is created on open and destroyed
  on close. Slint's licences do not suit the MIT repository, a webview opens
  too slowly (WebView2 starts processes), and native UI toolkits leave no
  way to Linux.
- **No tray crates**: `NSStatusItem` on macOS; on Windows
  `Shell_NotifyIconW` on a hidden top-level window (not a message-only one,
  which gets no theme change or Explorer restart broadcasts).
- **Panel quirks**: macOS sends `Focused(false)` before the window is shown,
  so focus loss counts only after it was focused; AppKit does not follow GL
  drawing, so the shadow is invalidated each draw. Windows sends no redraw
  to a hidden window, so the first one is made directly.
- **One instance**: on Windows a named mutex, and a second launch opens the
  running App's panel; on macOS LaunchServices, which works only for the
  `.app`.
- **A session exists only while Streaming is on**, so brightness and touch
  can be changed only then. Touch toggles Event and HID live; changing the
  display or rotation restarts the session. Brightness is not saved; the
  Tab5 keeps it.
- **Each plugged-in Tab5 has its own session** (`Address::Usb(Some(serial))`),
  found by listing serials every 500 ms without opening the devices. They are
  named `Tab5 #<last 4 digits of the serial>`. "All Devices" applies every
  setting, and values that differ show as "Mixed". Mirroring the same
  display does not share capture or encoding: one slow Tab5 would hold the
  others back on SWAP_DONE, for little gain.
- **Displays include the physical ones**; Windows would otherwise mirror the
  main display on every Tab5. A display is saved as its id on macOS (main
  display and a message if it is gone) and as its list index elsewhere.
  Only a Tab5's own virtual display is left out of its list: one Tab5 may
  mirror another's.
- **Settings** are `<serial>.<key>=<value>` lines in
  `Tab5 Streamer/settings.txt` under `~/Library/Application Support`,
  `%APPDATA%` or `$XDG_CONFIG_HOME` (`~/.config`).
- **The App needs to be a `.app` on macOS** for one instance, for the
  Screen Recording permission to be the App's rather than the terminal's,
  and for no Dock icon from launch.

## Web client

```
main.ts ── device.worker (device.ts)   WebUSB, sessions, audio pacing
        ├─ video.worker                scaling, wasm presenter, preview canvas
        │    └─ encoder.worker x 4     one JPEG band each
        └─ player.worker               demux (wasm), WebCodecs, A/V clock
```

The workers talk over MessageChannels (`messages.ts`); every time is epoch
ms, so one clock serves them all.

- **USB runs in a worker** because page timers are throttled while the tab
  is hidden. If the worker has no `navigator.usb`, it runs on the page.
- **No `SharedArrayBuffer`**, so the page needs no COOP/COEP headers.
- **Chrome's macOS file picker blocks the browser's UI thread while it
  opens and closes**, which every WebUSB transfer goes through, so video
  and audio stall a moment. Drag and drop avoids it.
- **Devices are named `Tab5 #<n>`**, one more than the highest shown. Output
  (Off/L/R/L+R) and volume are saved per serial in `localStorage`
  (`tab5-streamer:audio:<serial>`, prefixed because Pages shares one origin
  with the account's other project pages); brightness is not. "All Devices"
  sets brightness and volume on all of them.
- **Stopping a stream ends every session and opens a new one** ("release"),
  so each Tab5 shows its own screen while the page stays connected.

### Video

Frames from the screen or the player go to the video worker, which keeps
only the latest. Tab5s with the same layout share the JPEGs, and a frame
goes to all of them at once, so none runs ahead of the slowest.

- **Scaling is WebGL2, with mipmaps**, writing BT.601 full-range I420
  straight into wasm memory; only the scaled picture is read back. A canvas
  skips pixels beyond a 2x reduction. Without WebGL, or when the Tab5s need
  different sizes, `copyTo` gives YUV or BGRX and wasm scales (bicubic).
- **The output stays BT.601 full range**: WebGL takes the `VideoFrame` as
  RGB, so passing BT.709 limited through would save nothing.
- **A `copyTo` into wasm memory copies nothing if the memory grew
  meanwhile** (the view is detached), so it is retried.
- Chrome's "READ-usage buffer ... shadow copy" warning is harmless.

### Player

MP4/MOV and Matroska/WebM, demuxed in wasm, decoded with WebCodecs.

- **Over 4 GiB works**: offsets are f64 in JS, and lengths are clamped in
  u64 before narrowing to the 32-bit `usize`.
- **Reads go through a read-ahead** (1 MiB blocks, 32 ahead, 8 behind,
  2 in flight) because the demuxer reads synchronously with
  `FileReaderSync` and a slow file (network storage) would block the
  worker. Probing and thumbnails read synchronously.
- **Few decoded frames are held** (4, 16 while catching up after a seek):
  held frames stall the decoder.
- **After a seek, PCM and the clock wait for the first frame at the
  target**, so sound and picture start together.
- **Audio is the clock.** With one audible Tab5 it plays in the device's
  clock, paced by EV_AUDIO. With two or more it is sent in the host clock
  to all of them; the device plays it 100 ms after it arrives, and the A/V
  clock comes from EV_AUDIO so that latency does not offset the video
  (about 25-30 ms in the I2S DMA is not counted). A change restarts all of
  them after a 100 ms gap.
- Thumbnails are the frame at 10 s, or the middle one in a video of 20 s
  or less. The queue is not saved.

### Build

- **`wasm-bindgen` is pinned to the nix CLI's version** (`=0.2.126`); they
  must match exactly. `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER` points at
  lld's `wasm-ld` instead of putting lld on `PATH`.
- **`base: "./"` in `vite.config.ts`** lets the build run under the
  repository's sub-path on GitHub Pages. `src/icons.ts` inlines the
  repository's `assets/app-icon.svg` at build time; `server.fs.allow` is
  only for the dev server.
- **Fonts come from Google Fonts** (Montserrat, as on the Tab5;
  JetBrains Mono for numbers) and icons from `@tabler/icons`.

## Releases

Tags carry the `streamer-` prefix, separate from the firmware's `v*`, so
the PC tools are released on their own when the protocol does not change.

- **Pushing a `streamer-vX.Y.Z` tag deploys the Web client to GitHub
  Pages** (`.github/workflows/pages.yml`); `workflow_dispatch` deploys
  too. Pages builds without nix, since the dev shell pulls in ESP-IDF
  through `esp-devkit`. The workflow pins rustc and `wasm-bindgen` itself,
  so bump them there together with the flake.
- **`cargo xtask dist` packs the App** into `target/dist/`:
  - macOS: `Tab5 Streamer.app` (`LSUIElement`), signed with
    `CODESIGN_IDENTITY` or ad hoc, zipped with `ditto` to keep symlinks and
    attributes. The build machine's architecture only, not notarised.
  - Windows: `Tab5 Streamer.exe`, zipped with `System32\tar.exe`, because a
    GNU tar earlier on `PATH` (Git's) does not write zip.
- **Windows builds are checked by building there**: copy the sources
  (without macOS `._*` files), `cargo build --release -p streamer -p
  streamer-app` and clippy. A release build fails while the App runs,
  because its exe is locked.
