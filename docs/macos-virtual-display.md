# macOS Virtual Display

How `streamer capture --virtual` makes the display it captures on macOS, and
the platform behavior that shapes it.

## Creation

The display is a `CGVirtualDisplay`, which is private API in CoreGraphics:
a `CGVirtualDisplayDescriptor` (name, vendor, product and serial numbers,
maximum size, physical size, dispatch queue, termination handler), then one
`applySettings:` with a `CGVirtualDisplaySettings` holding the modes. Its
resolution is the panel's, rotated to the device's orientation; turning the
device applies the other orientation's mode. HiDPI is off: at 720p a HiDPI
mode would leave too little room to be useful.

- `CGVirtualDisplayMode`'s `initWithWidth:height:refreshRate:` takes
  `unsigned int` sizes on older macOS and `NSUInteger` on newer, so the
  argument type is read from the runtime before calling it.
- Two virtual displays with the same vendor, product and serial numbers
  cannot exist at once. Creating one fails while the previous one is still
  going away, so creation is retried for a few seconds.
- The display exists while the `CGVirtualDisplay` object does; dropping it
  removes the display.

## Identity

WindowServer keeps each display's arrangement by its vendor, product and
serial numbers. The serial number is the low 32 bits of the device's MAC
address, which the firmware reports as its USB serial number, so each
device has its own arrangement. Without one, over USB/IP, a fixed serial
number is used.

## Mirroring

WindowServer remembers whether displays mirror or extend per set of
displays connected together. For a set it has no configuration for it
applies a default, which follows the user's `TVConnectPolicy` (in the
ByHost `com.apple.windowserver.displays` preferences, read when
WindowServer starts): with 1, the default puts the built-in display in a
mirror set with the new display. Display settings are then saved for the
set, so later the user's choice is applied instead. The capture follows
whatever macOS and the user choose:

- A mirror set runs at one mode, so a mirrored virtual display is not at the
  resolution it was given, and a display that mirrors another is not listed
  by ScreenCaptureKit. While the virtual display is in a mirror set, the
  set's primary display (`CGDisplayPrimaryDisplay`, which may be the virtual
  display itself) is captured like any other display, at whatever size it
  has.
- While it is on its own, it is captured at the resolution for the device's
  orientation, which is applied again if a mirror set left it at another.
- Leaving or joining a mirror set is noticed within a second and the
  capture starts over.

## Declared size

The descriptor's physical size is 300 x 169 mm (13.6 inches, about 108 ppi
at 720p), not the panel's real size of about 110 x 62 mm. macOS reads two
things from it.

- **The default mode.** Besides the modes it is given, the display gets
  lower resolutions macOS adds (1024 x 576, 1024 x 768, 800 x 600), and macOS
  marks one mode as the default to start in. At a pixel density it finds
  too high for its interface it marks a lower resolution, so the display does
  not come up at the panel's resolution.
- **The Screen Mirroring menu bar item.** With the real size, macOS shows the
  Control Center "Screen Mirroring" item as active for as long as the display
  exists. It comes from the virtual display alone: capturing it is not
  involved, and neither is mirroring, as it shows for a mirrored display of
  the real size and not for a mirrored display of a larger one.

| Physical size | Resolution | Default mode | Screen Mirroring |
|---|---|---|---|
| 110 x 62 mm (real) | 1280 x 720 | 1280 x 720 | shown |
| 220 x 124 mm | 1280 x 720 | 1024 x 576 | not shown |
| 300 x 169 mm, 400 x 225 mm, 550 x 310 mm, 700 x 394 mm | 1280 x 720 | 1280 x 720 | not shown |
| 225 x 400 mm, 310 x 550 mm | 720 x 1280 | 720 x 1280 | not shown |

The Screen Mirroring item also showed at 130 x 73 mm and 160 x 90 mm, and
not at 190 x 107 mm, all at 1280 x 720. The exact rules are not known. The
physical size is set at creation only, so after the device turns the display
keeps the other orientation's size.

These do not change the item: `hiDPI`, color primaries, `isReference`,
`rotation` (which only makes the display report rotation support), the
dispatch queue, the name, vendor and product numbers, and offering several
modes. Applying a different mode after creation clears it, but only as a
side effect.

DisplayLink Manager makes its displays with `CGVirtualDisplay` as well, with
one `applySettings:`, and declares the physical size of the monitor attached
to the adapter, which is far larger than the panel.
