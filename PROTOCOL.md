# Tab5 Screen Streamer — USB Protocol

This document is the single source of truth for the USB wire protocol between the
PC tool (`streamer-rs`) and the Tab5 firmware (`esp32p4/`). Both sides are pinned
to the same protocol version through the USB **`bcdDevice`** field: the PC tool
refuses to talk to a device whose `bcdDevice` does not exactly match
`DEVICE_VERSION` (`streamer-rs/src/main.rs`). Bump both whenever the wire format
changes.

| Protocol | `bcdDevice` |
|----------|-------------|
| Video + audio only      | `0x0200` |
| Video + audio + touch   | `0x0300` |

## USB device overview

The device is a USB 2.0 High-Speed composite/IAD device.

- **VID/PID**: `0xF055` / `0x1118`
- **Interface 0 — Vendor (WinUSB / libusb)**: the screen + touch channel.
  - Bulk **OUT `0x01`**: PC → Tab5 video stream (JPEG bands). See below.
  - Bulk **IN `0x81`**: Tab5 → PC touch reports. See below.
- **Interfaces 1/2 — UAC 2.0 speaker**: PC → Tab5 audio (48 kHz / stereo /
  16-bit PCM, isochronous). Independent of this document.

Windows binds WinUSB to interface 0 only (via the MS OS 2.0 descriptor), so the
vendor interface is usable with `nusb`/libusb while the UAC interfaces keep the
in-box audio driver.

## PC → Tab5: video (bulk OUT `0x01`)

Each captured frame is split into `SPLIT_COUNT` (16) horizontal **bands** in the
device's portrait orientation. Every band is an independent JPEG with an 8-byte
header:

```
byte 0     : type  (0x50 = decode only, 0x51 = last band of the frame → present)
bytes 1..3 : data size, 24-bit little-endian = 4 coord bytes + JPEG payload length
byte 4     : x / 16       (always 0)
byte 5     : y / 16
byte 6     : width / 16   (always 720/16 = 45)
byte 7     : height / 16
```

The stream has no global sync marker; the firmware re-aligns on the first band of
a frame (`type ∈ {0x50,0x51}`, `x==0`, `y==0`, `width==720`, JPEG `SOI`). This
channel is unchanged by the touch feature.

### Orientation / scaling (PC side)

The PC captures its display and scales it so the device always receives a
**720×1280 portrait** image:

- **Landscape source** (`src_width > src_height`, e.g. a normal monitor; on macOS
  the capture is always forced to 1280×720): each band is a vertical source strip
  rotated **270° (90° CCW)**. The right edge of the desktop ends up at the **top**
  of the panel, the top of the desktop ends up at the **left** of the panel.
- **Portrait source** (`src_width ≤ src_height`): bands are contiguous row blocks,
  no rotation; the image maps 1:1.

This transform is what the touch coordinate mapping below has to invert.

## Tab5 → PC: touch reports (bulk IN `0x81`)

The Tab5 reports its capacitive touch panel (GT911, up to 5 simultaneous points)
back to the PC. The PC tool injects them as OS pointer/mouse events on the
captured display — there is **no HID digitizer** and **no negotiation**: the
firmware streams reports unconditionally while in remote mode, and matching
versions are guaranteed by `bcdDevice`.

### When the firmware sends

- The device-side LVGL settings overlay (brightness / quality) acts as a mode
  switch:
  - **Overlay hidden** = *remote mode*: touches are forwarded to the PC.
  - **Overlay visible** = *settings mode*: touches drive the on-device UI and are
    **not** forwarded.
- A **3-or-more-finger touch** toggles the overlay (edge-triggered: fires once
  when the count first reaches 3, re-arms when all fingers lift). That gesture is
  never forwarded, so 1- and 2-finger interactions (including pinch/scroll) pass
  through to the PC cleanly. On USB disconnect the overlay is forced visible so
  the settings UI is always reachable.

### Message format

All multi-byte fields are **little-endian** (consistent with the video channel).
One message describes the **full set of currently-touching points** (a snapshot,
not a delta).

```
offset  size  field
0       1     type      = 0x01  (touch report; other values reserved)
1       1     count     number of active points, 0..5
2 + i*6       points[i] for i in 0..count:
  +0    1     track_id  GT911 track id (stable across moves while a finger stays down)
  +1    1     reserved  = 0
  +2    2     x         panel X, 0..719   (u16 LE)
  +4    2     y         panel Y, 0..1279  (u16 LE)
```

Total length = `2 + count*6` (max 32 bytes for 5 points). The firmware writes one
message per USB transfer and flushes, so each bulk-IN transfer carries exactly one
message. The PC parser must nonetheless tolerate coalesced/split reads: it
resynchronizes on the `type` byte and derives message length from `count`.

Coordinates are the **raw panel pixels** (720×1280, the device's native portrait
orientation). The firmware does **no** rotation/scaling — the PC owns the inverse
transform (it is the side that knows how it mapped its screen into 720×1280).

### Semantics (PC side)

Each report is the complete active set. The PC diffs it against the previous
report **by `track_id`**:

- a `track_id` that newly appears → pointer **down**,
- a `track_id` present in both → pointer **move**,
- a `track_id` that disappears → pointer **up**,
- `count == 0` → all points released.

Multi-touch is carried end to end. The current PC implementation injects a single
OS pointer (it follows the primary/first track id and maps it to the left mouse
button); richer multi-touch injection is a PC-side concern and can be added
without changing this protocol.

### Coordinate mapping (panel → display)

Given a panel point `(tx, ty)` with `tx ∈ [0,720)`, `ty ∈ [0,1280)`, compute the
normalized display position `(nx, ny) ∈ [0,1]²`:

- **Landscape display** (captured frame was rotated 270°):
  ```
  nx = 1 - ty / 1280
  ny =     tx / 720
  ```
- **Portrait display** (no rotation):
  ```
  nx = tx / 720
  ny = ty / 1280
  ```

Then the injected pixel position is `display_origin + (nx * display_width,
ny * display_height)`. The orientation used here must match the one the capture
side used for that display (`src_width > src_height`).
