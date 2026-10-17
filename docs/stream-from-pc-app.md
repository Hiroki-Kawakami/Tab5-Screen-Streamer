# Stream from PC App

Show your computer's screen on Tab5 with the Tab5 Streamer client.

## Choose a Client

Tab5 Streamer comes in three forms. Use the **App** or the **Web** version
unless you need the command line.

- **App**: lives in the macOS menu bar or the Windows notification area.
  [Download](https://github.com/Hiroki-Kawakami/Tab5-Screen-Streamer/releases/tag/streamer-latest)
- **Web**: runs in the browser, nothing to install.
  [Open](https://hiroki-kawakami.github.io/Tab5-Screen-Streamer/)
- **CLI**: for scripting and advanced use. Included in the App download.

### Platform Support

|     | macOS                   | Windows                 | Linux                   |
|-----|-------------------------|-------------------------|-------------------------|
| App | ✓ (Apple silicon)       | ✓                       | –                       |
| Web | Chrome, Edge            | Chrome, Edge            | Chrome, Edge            |
| CLI | ✓ (Apple silicon)       | ✓                       | –                       |

The Web version needs a Chromium-based browser (WebUSB and WebCodecs).
Safari and Firefox are not supported.

### Features

|                                | App        | Web                   | CLI             |
|--------------------------------|------------|-----------------------|-----------------|
| Mirror a display               | ✓          | ✓ (screen or window)  | ✓               |
| Extended display (Tab5 as a second screen) | macOS only | –          | macOS only      |
| Touch input to the PC          | ✓          | – (works as a trackpad) | ✓             |
| Brightness control             | ✓          | ✓                     | –               |
| Multiple Tab5s at once         | ✓          | ✓                     | One per process |
| Play video files               | –          | ✓                     | ✓ (needs ffmpeg) |

- On macOS, touch acts as a single-finger mouse. On Windows, it is
  multi-touch.
- Screen streaming does not carry sound. To hear your computer's audio from
  Tab5, turn on **USB Audio** (see [Options](#options)) and pick Tab5 as the
  speaker on your computer.
- The Web version can also play video files from your computer, with a
  playlist and a seek bar. Their sound plays on Tab5 directly.

## 1. Install Tab5 Streamer

### App and CLI

Download the zip for your OS from the
[release page](https://github.com/Hiroki-Kawakami/Tab5-Screen-Streamer/releases/tag/streamer-latest)
and extract it.

- **macOS**: run `Tab5 Streamer.app`. You can move it to the Applications
  folder if you like. If macOS says the app cannot be opened, open
  **System Settings > Privacy & Security** and click **Open Anyway**.
- **Windows**: run `Tab5 Streamer.exe`.

The CLI is `tab5-streamer` in the same zip.

### Web

Open <https://hiroki-kawakami.github.io/Tab5-Screen-Streamer/> in Chrome or
Edge.

## 2. Connect Tab5

Connect Tab5's USB-A port to your computer with a USB-A to USB-A cable.
See [USB Cable](usb-cable.md) for how to get one and how to connect from a
USB-C port.

## 3. Start Streaming

1. On Tab5, tap **Start**.
2. Open Tab5 Streamer on your computer.
   - **App**: click the icon in the menu bar (macOS) or the notification
     area (Windows), pick the display to show, and start.
   - **Web**: click the connect button, choose Tab5 in the browser's device
     list, then choose the screen or window to share.

To open the stream menu on Tab5, swipe in from the bottom-right corner.

If Tab5 is disconnected, the App and the CLI wait and reconnect
automatically when it comes back.

### macOS Permissions

The first time you stream, macOS asks for:

- **Screen Recording**: needed to capture the screen.
- **Accessibility**: needed for touch input. Without it, streaming works but
  touch does not. Restart streaming after allowing it.

## Options

These are on the Tab5 screen and work with every client.

- **USB Audio**: Tab5 appears as a USB speaker on your computer.
- **Touch Input**: touching Tab5 controls the computer. With the App and the
  CLI, touch goes to the point you touch on the screen. Otherwise Tab5 acts
  as a trackpad.
- **Keyboard Input**: the Tab5 Keyboard appears as a USB keyboard on your
  computer.

## Troubleshooting

### Tab5 is not found

- Check the cable and how it is connected. See [USB Cable](usb-cable.md).
- Make sure you tapped **Start** on Tab5.

### Windows: Tab5 is not found after using an older firmware

If you installed a driver with [Zadig](https://zadig.akeo.ie) for an older
firmware, remove it:

1. Open Device Manager and find "Tab5 Screen Streamer".
2. Right-click it and select **Uninstall device**.
3. Check **Delete the driver software for this device**, then reconnect
   Tab5.

Tab5 tells Windows which driver to use, so no driver installation is needed.
