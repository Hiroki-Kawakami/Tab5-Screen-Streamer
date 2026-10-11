//! I420 frames to panel pixels, for RAW and LZ4.

use crate::jpeg::Placement;
use crate::layout::rotated;
use crate::protocol::PixelFormat;

/// A BT.601 full range I420 frame of even width and height, planes packed.
pub struct I420<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
}

fn rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let (y, cb, cr) = (y as f32, cb as f32 - 128.0, cr as f32 - 128.0);
    let clamp = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    (
        clamp(y + 1.402 * cr),
        clamp(y - 0.344_136 * cb - 0.714_136 * cr),
        clamp(y + 1.772 * cb),
    )
}

/// The canvas of `placement` in `format`, black around the frame.
pub fn to_panel(frame: &I420, placement: Placement, format: PixelFormat) -> Vec<u8> {
    let (w, h) = (frame.width, frame.height);
    let (cw, ch) = (w / 2, h / 2);
    let (cb, cr) = frame.data[w * h..].split_at(cw * ch);
    let (rw, rh) = rotated((w as u32, h as u32), placement.rotation);
    let (rw, rh) = (rw as usize, rh as usize);
    let (ox, oy) = placement.origin;
    let (width, height) = placement.canvas;
    let bytes = format.bytes();
    let mut out = vec![0u8; width * height * bytes];
    for y in oy..(oy + rh).min(height) {
        for x in ox..(ox + rw).min(width) {
            let (cx, cy) = (x - ox, y - oy);
            let (sx, sy) = match placement.rotation % 4 {
                1 => (w - 1 - cy, cx),
                2 => (w - 1 - cx, h - 1 - cy),
                3 => (cy, h - 1 - cx),
                _ => (cx, cy),
            };
            let c = sy / 2 * cw + sx / 2;
            let (r, g, b) = rgb(frame.data[sy * w + sx], cb[c], cr[c]);
            let at = (y * width + x) * bytes;
            match format {
                PixelFormat::Rgb565 => {
                    let v = (r as u16 >> 3) << 11 | (g as u16 >> 2) << 5 | b as u16 >> 3;
                    out[at..at + 2].copy_from_slice(&v.to_le_bytes());
                }
                PixelFormat::Rgb888 => out[at..at + 3].copy_from_slice(&[b, g, r]),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_pads() {
        let mut data = vec![0u8; 4 * 2];
        data.extend([128, 128, 128, 128]);
        data[3] = 255;
        let frame = I420 {
            width: 4,
            height: 2,
            data: &data,
        };
        let placement = Placement {
            rotation: 1,
            canvas: (4, 4),
            origin: (2, 0),
        };
        let out = to_panel(&frame, placement, PixelFormat::Rgb888);
        let white: Vec<usize> = (0..16).filter(|&i| out[i * 3] == 255).collect();
        assert_eq!(white, [2]);
    }
}
