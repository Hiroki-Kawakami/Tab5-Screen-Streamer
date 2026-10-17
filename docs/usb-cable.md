# USB Cable

Tab5 talks to your computer at USB 2.0 High Speed (480 Mbps) only through
its USB-A port, so you need a USB-A to USB-A cable. Tab5's USB-C port runs
at Full Speed (12 Mbps) only, which is too slow for streaming.

## Which Connection Works

| Computer's port | What you need                                          |
|-----------------|--------------------------------------------------------|
| USB-A           | A USB-A to USB-A cable                                 |
| USB-C           | A USB-C (male) to USB-A (female) adapter, then a USB-A to USB-A cable |

A USB-C to USB-A cable plugged straight into a USB-C port does not work: the
port's role detection decides the computer is the device. Go through a
USB-C to USB-A adapter instead.

## Getting a USB-A to USB-A Cable

Tab5 does not output 5 V on its USB-A port while connected to a computer,
so a plain cable works. Still, covering the VBUS and GND pins of one plug
with tape is safer, since the cable connects the computer's VBUS to Tab5's.

### Option 1: Commercial USB-A to USB-A Cable

Use a USB-A male to USB-A male cable.

### Option 2: USB-A to USB-C Cable and an Adapter

Use a regular USB-A male to USB-C male cable with a USB-C female to USB-A
male adapter on the USB-C end.

### Option 3: Make Your Own

Connect only the D+ and D- pins between two USB-A male connectors.
See [Espressif's documentation](https://github.com/espressif/esp-idf/tree/8f1e7bc4e08878f5a2aab55fdf302789d3a3eac4/examples/peripherals/usb)
for details.
