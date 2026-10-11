# USB Protocol

How a PC drives the Tab5 over USB: drawing into the panel's framebuffers,
reading the device state, receiving input and playing audio.

## Interfaces

| Interface | Endpoints | Carries |
|---|---|---|
| Vendor: control | bulk OUT, bulk IN | Requests that do not depend on ordering; replies and device events |
| Vendor: video | bulk OUT | Drawing commands |
| Vendor: audio | bulk OUT | PCM and its format |
| UAC 2.0 speaker | isochronous OUT | Audio played by the host OS |
| HID | interrupt IN | Keyboard and touch digitizer, one report ID each |

The endpoints are split by ordering and by latency. Messages on one endpoint
are handled in the order they were sent; nothing is ordered across endpoints.
Audio has its own endpoint so it never waits behind a large drawing transfer,
and the control endpoint answers while the video endpoint is busy.

The HID interface is always enumerated. Whether reports are sent on it is the
input route (see [Input](#input)), so switching routes never re-enumerates.

## Framing

Every vendor endpoint carries messages, each a header followed by its payload.
All integers are little endian and all structures are packed.

```
MsgHeader (16 bytes)
  u16 magic     0x3554 ("T5")
  u8  type
  u8  flags     0
  u32 length    payload bytes after the header
  u32 seq
  u32 epoch     see Display epoch; 0 where unused
```

- The PC writes whole messages per OUT transfer, back to back, and ends the
  transfer with a short packet (a ZLP when it is a multiple of the packet
  size). A message never spans two transfers: the JPEG decoder needs its input
  contiguous.
- A message may not exceed the endpoint's `max_message` from `INFO`. A message
  that does not fit in the rest of its transfer, or has a wrong magic, is
  reported with `EV_ERROR` and the rest of the transfer is dropped.
- `seq` is chosen by the PC and increases per endpoint. Replies and errors
  carry the `seq` of the message they answer; unsolicited events carry 0.
- The PC keeps at least one IN transfer pending on the control endpoint at all
  times. The device never blocks on it: when its event queue is full, newer
  events replace older ones of the same type.

## Coordinates and orientation

Coordinates are in panel-native pixels (720x1280 portrait on the Tab5),
whatever the orientation. Rotations are counter-clockwise quarter turns, 0 to 3.

The orientation reported by the device is the rotation the content needs to
appear upright to the user, so the PC can pass it as `DRAW_BLIT.rotation`.

## Pixel formats

| Value | Format | Memory layout |
|---|---|---|
| 1 | RGB565 | u16 per pixel, R in the high bits |
| 2 | RGB888 | bytes B, G, R |

The panel's format is the LCD color format setting, reported in `INFO` and
`EV_FORMAT`.

## Encodings

| Value | Encoding | Payload |
|---|---|---|
| 0 | JPEG | Baseline JPEG, decoded as BT.601 full range |
| 1 | RAW | Pixels, rows top to bottom, no padding |
| 2 | LZ4 | RAW compressed as one LZ4 block (block format, no frame) |

The decompressed size of LZ4 follows from the command's dimensions and pixel
format, so it is not sent.

## Control endpoint

### Requests (OUT)

| Type | Name | Payload |
|---|---|---|
| 0x01 | `GET_INFO` | none; answered with `INFO` |
| 0x02 | `SET_INPUT_ROUTE` | `u8 route`, `u8 reserved[3]` |
| 0x03 | `SET_VOLUME` | `u8 stream`, `u8 mute`, `i16 volume` |

### Replies and events (IN)

| Type | Name | Payload |
|---|---|---|
| 0x81 | `INFO` | see below |
| 0x82 | `SWAP_DONE` | `u32 time_us` |
| 0x83 | `EV_ERROR` | `u8 endpoint`, `u8 code`, `u16 reserved` |
| 0x84 | `EV_ORIENTATION` | `u8 orientation`, `u8 reserved[3]` |
| 0x85 | `EV_FORMAT` | `u8 pixel_format`, `u8 reserved[3]`; header `epoch` is the new epoch |
| 0x86 | `EV_OVERLAY` | `u8 visible`, `u8 reserved`, `u16 x, y, w, h` |
| 0x87 | `EV_DAMAGE` | `u16 x, y, w, h` |
| 0x88 | `EV_TOUCH` | see [Input](#input) |
| 0x89 | `EV_KEY` | see [Input](#input) |
| 0x8a | `EV_INPUT_ROUTE` | `u8 route`, `u8 reserved[3]` |
| 0x8b | `EV_VOLUME` | `u8 stream`, `u8 mute`, `i16 volume` |
| 0x8c | `EV_AUDIO` | `u32 time_us`, `u32 buffered_frames`, `u32 capacity_frames` |
| 0x8d | `EV_MAX_FPS` | `u16 max_fps`, `u16 reserved` |

`time_us` is the low 32 bits of the device clock in microseconds.

`EV_ERROR` refers to the message with the header's `seq` on `endpoint`
(0 control, 1 video, 2 audio). Codes:

| Code | Meaning |
|---|---|
| 1 | Malformed: bad magic, unknown type, or does not fit the transfer |
| 2 | Too large for `max_message` or `blit_max_*` |
| 3 | Unsupported: encoding, pixel format, audio format or stream |
| 4 | Out of bounds of the panel |
| 5 | Decode failed |

### INFO

```
INFO
  u16 protocol_version         1
  u16 panel_width, panel_height
  u8  pixel_format
  u8  orientation
  u8  framebuffers
  u8  swap_window              SWAPs the PC should keep in flight at most
  u16 direct_align             DRAW_DIRECT y and height are multiples of this
  u16 blit_max_width, blit_max_height
  u32 encodings                bit n set: encoding n supported
  u32 max_message[3]           control, video, audio
  u32 epoch
  u8  overlay_visible
  u8  input_route
  u16 overlay_x, overlay_y, overlay_w, overlay_h
  u16 max_fps                  frames per second the PC sends at most
  Volume volume[2]             UAC, BULK
  u32 audio_max_rate           highest AUDIO_FORMAT sample rate

Volume
  u8  mute
  u8  reserved
  i16 volume
  i16 min, max, resolution
```

## Video endpoint

| Type | Name |
|---|---|
| 0x10 | `DRAW_DIRECT` |
| 0x11 | `DRAW_BLIT` |
| 0x12 | `FILL` |
| 0x13 | `SWAP` |

```
DRAW_DIRECT
  u8  target       0 FRONT, 1 BACK
  u8  encoding
  u16 y, height
  ... payload

DRAW_BLIT
  u8  target
  u8  encoding
  u8  src_format   pixel format of RAW and LZ4; ignored for JPEG
  u8  rotation
  u16 src_w, src_h
  u16 dst_x, dst_y, dst_w, dst_h
  ... payload

FILL
  u8  target
  u8  reserved
  u16 x, y, w, h
  u32 color        0x00RRGGBB

SWAP
  u32 reserved
```

- **`DRAW_DIRECT`** writes full-width rows straight into the framebuffer. RAW
  and LZ4 are in the panel's pixel format, and a JPEG is exactly panel width
  by `height`. `y` and `height` are multiples of `direct_align`.
- **`DRAW_BLIT`** draws anything else: a rectangle narrower than the panel, a
  source in another pixel format, rotated or scaled. The source is rotated
  first; `dst_w` and `dst_h` are the size after rotation, and differing from
  the rotated source size scales it. Each axis scales by a whole number of
  sixteenths (`dst_w` = rotated width × n / 16, likewise for the height);
  any other size is answered with `EV_ERROR` (unsupported). `src_w` and
  `src_h` are at most `blit_max_width` and `blit_max_height`.
- **`FILL`** paints a solid rectangle, such as letterbox bars. The color is
  RGB888 whatever the panel format.
- Commands are executed in order and never dropped, except for the epoch
  check below.

### Framebuffers

`FRONT` is the framebuffer on screen; `BACK` is the one the next `SWAP` puts
on screen.

- **Partial updates go to `FRONT`.** They may tear and need no `SWAP`.
- **Tear-free updates draw the whole panel into `BACK` and then `SWAP`.** After
  a `SWAP` the content of `BACK` is undefined, so every frame covers the panel
  completely, letterbox included.
- Moving from `BACK` + `SWAP` to `FRONT` updates needs nothing. Moving the
  other way, the first frame drawn into `BACK` covers the whole panel.
- `SWAP_DONE` is sent when the device accepts the swap, not at the vertical
  sync. The PC keeps at most `swap_window` swaps unanswered; waiting for each
  one before sending the next frame would serialise encoding, transfer and
  decoding.
- The PC sends at most `max_fps` frames per second, counting `SWAP`s, or OUT
  transfers on the video endpoint for `FRONT` updates. The device does not
  enforce it; it lowers the value to leave room for other work, at any time
  during a session, and announces every change with `EV_MAX_FPS`.

### Display epoch

The epoch increases whenever the panel's pixel format changes and is announced
with `EV_FORMAT`. The PC puts the latest epoch it has seen in every video
message. A `DRAW_DIRECT` with RAW or LZ4 whose epoch differs is dropped
without an error, since its bytes are in the old format. Every framebuffer is
undefined after a format change, so on `EV_FORMAT` the PC redraws the whole
panel.

### Overlay

The overlay is the Tab5's own UI drawn over the stream. `EV_OVERLAY` and
`INFO` report whether it is visible and the rectangle left for the stream.

While it is visible:

- Every draw is clipped to that rectangle.
- The device stops swapping. `BACK` refers to the framebuffer on screen, and a
  `SWAP` is answered with `SWAP_DONE` at once. Drawing tears, but the PC need
  not change anything.
- A `DRAW_DIRECT` JPEG that crosses the clip goes through the slower
  `DRAW_BLIT` path on the device.

When showing the overlay changes the framebuffer on screen, the device sends
`EV_DAMAGE` for the stream rectangle. When the overlay hides, it sends
`EV_DAMAGE` for the area the overlay covered. The device keeps no copy of what
is under the overlay; the PC redraws damaged areas.

## Audio endpoint

| Type | Name | Payload |
|---|---|---|
| 0x20 | `AUDIO_FORMAT` | `u32 sample_rate`, `u8 channels`, `u8 bits`, `u16 reserved` |
| 0x21 | `AUDIO_DATA` | PCM, interleaved, signed, `bits / 8` bytes per sample |

- The PC sends `AUDIO_FORMAT` after connecting and before every format change.
  `AUDIO_DATA` before the first `AUDIO_FORMAT` is dropped.
- PCM already buffered plays out in its own format before the new one applies.
- The UAC speaker and bulk audio are exclusive. While the host streams to the
  speaker (48 kHz, 16-bit stereo), every `AUDIO_DATA` is answered with
  `EV_ERROR` (unsupported) and dropped.
- Any sample rate up to `audio_max_rate` is taken, with 1 or 2 channels of 16,
  24 or 32 bits. Anything else is answered with `EV_ERROR`, and the data
  following it is dropped until the next `AUDIO_FORMAT`.
- Bulk has no clock. While bulk audio plays, the device sends `EV_AUDIO`
  periodically and the PC paces its writes from that buffer level, the usual
  feedback for a sink that owns the clock.
- The PC keeps the buffer level well below `capacity_frames`. A full audio
  endpoint refuses data, and some hosts then stall the device's other
  endpoints along with it.

### Volume

UAC and bulk audio each have their own volume, applied to that stream alone.
`stream` is 0 for UAC and 1 for bulk. Volume is in 1/256 dB, the unit of the
UAC feature unit; the range is in `INFO`.

- The UAC volume belongs to the host OS, which sets it through the feature
  unit. `SET_VOLUME` on it is rejected with `EV_ERROR`, since the OS volume
  control would not follow.
- The bulk volume is set with `SET_VOLUME`. Out-of-range values are clamped.
- Every change, from any source, is sent as `EV_VOLUME` with the value
  applied.

## Input

Touch and keyboard input goes to the vendor events, to the HID interface, or
to both. `route` is a bit mask: bit 0 vendor events, bit 1 HID. It is set with
`SET_INPUT_ROUTE` and every change is announced with `EV_INPUT_ROUTE`.

```
EV_TOUCH
  u8  count        0 when every point is released
  u8  reserved[3]
  TouchPoint points[count]

TouchPoint
  u8  id
  u8  reserved
  u16 x, y         panel-native
  u16 reserved

EV_KEY
  u8  modifiers    HID boot keyboard report
  u8  reserved
  u8  keys[6]
```

`EV_TOUCH` is sent on every change of the touch points, and `EV_KEY` on every
change of the keyboard state. Both carry the same data as the HID reports.

## Open

- Whether the input route is switched from the Tab5 UI, the PC, or both.
