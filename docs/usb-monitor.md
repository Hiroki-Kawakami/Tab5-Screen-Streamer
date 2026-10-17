# USB Monitor

Use Tab5 as a USB display. Tab5 shows up on your computer like a USB display
adapter, so there is no app or driver to install.

## USB Monitor or PC App

Tab5 can also show your computer's screen through the Tab5 Streamer app.
See [Stream from PC App](stream-from-pc-app.md). Pick the mode that fits
what you want to do.

|                                   | USB Monitor        | PC App                         |
|-----------------------------------|--------------------|--------------------------------|
| Install on the computer           | Nothing            | Tab5 Streamer (app or web)     |
| Extended display (a second screen) | Windows, Linux    | macOS only                     |
| Mirror a display                  | Through the OS's display settings | Windows, macOS, Linux (web) |
| Drawing speed                     | Slower             | Faster                         |
| Touch input to the PC             | –                  | ✓                              |

Use **USB Monitor** when:

- You do not want to install anything on the computer.
- You want Tab5 as a second screen on Windows or Linux. PC App can only
  extend the desktop on macOS, and its desktop app does not run on Linux.

Use **PC App** when:

- You want smoother drawing, such as for video or scrolling.
- You want to control the computer by touching Tab5.
- You use a Mac. USB Monitor does not work with macOS.

## Compatibility

Tab5 offers two kinds of display adapter. Choose one in **Options >
Adapter** on Tab5.

|                     | Windows | Linux | macOS |
|---------------------|---------|-------|-------|
| DisplayLink         | ✓       | ✓     | –     |
| Generic USB Display | –       | ✓     | –     |

- **Windows**: use **DisplayLink**. Windows installs the DisplayLink driver
  through Windows Update the first time Tab5 is connected.
- **Linux**: use **Generic USB Display**. It works with the `gud` driver
  built into Linux 5.13 and later. DisplayLink also works, with the `udl`
  driver built into Linux.
- **macOS**: not supported. Use [PC App](stream-from-pc-app.md) instead.

## 1. Choose the Adapter

On Tab5, open **USB Monitor** and choose the adapter for your computer in
**Options > Adapter**.

## 2. Connect Tab5

Connect Tab5's USB-A port to your computer with a USB-A to USB-A cable.
See [USB Cable](usb-cable.md) for how to get one and how to connect from a
USB-C port.

## 3. Start

Tap **Start**. Tab5 appears on your computer as a display. Arrange it or
switch between extending and mirroring in your computer's display settings.

## Troubleshooting

### Tab5 does not appear as a display

- Check the cable and how it is connected. See [USB Cable](usb-cable.md).
- Make sure you tapped **Start** on Tab5.
- Make sure the adapter matches your computer. See
  [Compatibility](#compatibility).

### Windows: the display takes a while to appear the first time

Windows is installing the driver through Windows Update. Wait until it
finishes.
