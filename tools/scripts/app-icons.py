#!/usr/bin/env python3
"""Render assets/*.svg into the icons streamer-app embeds and bundles, in
tools/streamer-rs/crates/app/icons. macOS only: it renders with sips.
"""
import os
import re
import shutil
import struct
import subprocess
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ASSETS = os.path.join(ROOT, 'assets')
OUT = os.path.join(ROOT, 'tools', 'streamer-rs', 'crates', 'app', 'icons')
TRAY_STATES = ['disabled', 'disconnected', 'idle', 'streaming']
# 18 pt at 2x in the menu bar.
MENU_BAR_SIZE = 36
# The small icon size at 100, 125, 150 and 200 % scaling.
NOTIFY_SIZES = [16, 20, 24, 32]
APP_ICO_SIZES = [16, 24, 32, 48, 64, 256]
ICONSET = [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2),
           (512, 1), (512, 2)]


def render(svg, size, out):
    subprocess.run(['sips', '-s', 'format', 'png', '-z', str(size), str(size), svg,
                    '--out', out], check=True, stdout=subprocess.DEVNULL)


def white(svg, out):
    with open(svg) as f:
        text = f.read()
    text = re.sub(r'#000000\b|#000\b', '#FFFFFF', text)
    text = text.replace('<svg ', '<svg fill="#FFFFFF" ', 1)
    with open(out, 'w') as f:
        f.write(text)


def ico(pngs, out):
    datas = [open(png, 'rb').read() for _, png in pngs]
    header = struct.pack('<HHH', 0, 1, len(pngs))
    offset = len(header) + 16 * len(pngs)
    entries = b''
    for (size, _), data in zip(pngs, datas):
        side = 0 if size >= 256 else size
        entries += struct.pack('<BBBBHHII', side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    with open(out, 'wb') as f:
        f.write(header + entries + b''.join(datas))


def main():
    os.makedirs(OUT, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for state in TRAY_STATES:
            svg = os.path.join(ASSETS, f'tray-{state}.svg')
            render(svg, MENU_BAR_SIZE, os.path.join(OUT, f'tray-{state}.png'))
            light = os.path.join(tmp, f'{state}-white.svg')
            white(svg, light)
            for theme, source in [('light', svg), ('dark', light)]:
                pngs = []
                for size in NOTIFY_SIZES:
                    png = os.path.join(tmp, f'{state}-{theme}-{size}.png')
                    render(source, size, png)
                    pngs.append((size, png))
                ico(pngs, os.path.join(OUT, f'tray-{state}-{theme}.ico'))

        app = os.path.join(ASSETS, 'app-icon.svg')
        pngs = []
        for size in APP_ICO_SIZES:
            png = os.path.join(tmp, f'app-{size}.png')
            render(app, size, png)
            pngs.append((size, png))
        ico(pngs, os.path.join(OUT, 'app.ico'))
        iconset = os.path.join(tmp, 'app.iconset')
        os.makedirs(iconset)
        for size, scale in ICONSET:
            suffix = '' if scale == 1 else f'@{scale}x'
            render(app, size * scale, os.path.join(iconset, f'icon_{size}x{size}{suffix}.png'))
        subprocess.run(['iconutil', '-c', 'icns', iconset, '-o', os.path.join(OUT, 'app.icns')],
                       check=True)


if __name__ == '__main__':
    main()
