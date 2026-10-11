//! Baseline 4:2:0 JPEG encoder that rotates and places the image while
//! reading it, so a frame is converted, transformed and coded in one pass.

mod dct;
mod huffman;
mod tables;

use std::ops::Range;
use std::sync::OnceLock;

use huffman::Table;
use tables::{QuantTables, ZIGZAG};

pub enum Pixels<'a> {
    /// Bytes B, G, R and one ignored, per pixel.
    Bgrx { data: &'a [u8], stride: usize },
    /// Planar YUV 4:2:0, BT.601 full range; width and height are even.
    I420 {
        y: &'a [u8],
        y_stride: usize,
        cb: &'a [u8],
        cr: &'a [u8],
        c_stride: usize,
    },
}

pub struct Image<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: Pixels<'a>,
}

/// The image rotated by `rotation` counter-clockwise quarter turns and put
/// at `origin` on a black `canvas`. `origin` is even for I420.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub rotation: u32,
    pub canvas: (usize, usize),
    pub origin: (usize, usize),
}

struct Huffman {
    dc: [Table; 2],
    ac: [Table; 2],
}

fn huffman() -> &'static Huffman {
    static TABLES: OnceLock<Huffman> = OnceLock::new();
    TABLES.get_or_init(|| Huffman {
        dc: [
            Table::new(tables::standard_dc_luma()),
            Table::new(tables::standard_dc_chroma()),
        ],
        ac: [
            Table::new(tables::standard_ac_luma()),
            Table::new(tables::standard_ac_chroma()),
        ],
    })
}

/// Maps content coordinates to a sample index: base + x * step_x + y * step_y.
#[derive(Clone, Copy)]
struct Walk {
    base: isize,
    step_x: isize,
    step_y: isize,
}

impl Walk {
    fn new(rotation: u32, size: (usize, usize), stride: usize, bytes: usize) -> Self {
        let (w, h) = (size.0 as isize, size.1 as isize);
        let (s, b) = (stride as isize, bytes as isize);
        match rotation % 4 {
            1 => Walk {
                base: (w - 1) * b,
                step_x: s,
                step_y: -b,
            },
            2 => Walk {
                base: (h - 1) * s + (w - 1) * b,
                step_x: -b,
                step_y: -s,
            },
            3 => Walk {
                base: (h - 1) * s,
                step_x: -s,
                step_y: b,
            },
            _ => Walk {
                base: 0,
                step_x: b,
                step_y: s,
            },
        }
    }

    fn at(&self, x: usize, y: usize) -> usize {
        (self.base + x as isize * self.step_x + y as isize * self.step_y) as usize
    }
}

/// One MCU of samples, level shifted.
struct Mcu {
    y: [[f32; 64]; 4],
    cb: [f32; 64],
    cr: [f32; 64],
}

const BLACK_Y: f32 = -128.0;

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b - 128.0
}

fn chroma(r: f32, g: f32, b: f32) -> (f32, f32) {
    (
        -0.168_736 * r - 0.331_264 * g + 0.5 * b,
        0.5 * r - 0.418_688 * g - 0.081_312 * b,
    )
}

struct Reader<'a> {
    image: &'a Image<'a>,
    placement: Placement,
    rotated: (usize, usize),
    luma: Walk,
    chroma: Walk,
}

impl<'a> Reader<'a> {
    fn new(image: &'a Image<'a>, placement: Placement) -> Self {
        let size = (image.width, image.height);
        let rotated = if placement.rotation % 2 == 1 {
            (size.1, size.0)
        } else {
            size
        };
        let (luma, chroma) = match image.pixels {
            Pixels::Bgrx { stride, .. } => {
                let walk = Walk::new(placement.rotation, size, stride, 4);
                (walk, walk)
            }
            Pixels::I420 {
                y_stride, c_stride, ..
            } => (
                Walk::new(placement.rotation, size, y_stride, 1),
                Walk::new(placement.rotation, (size.0 / 2, size.1 / 2), c_stride, 1),
            ),
        };
        Reader {
            image,
            placement,
            rotated,
            luma,
            chroma,
        }
    }

    /// Fills `mcu` with the canvas MCU at (x, y).
    fn read(&self, x: usize, y: usize, mcu: &mut Mcu) {
        let (ox, oy) = self.placement.origin;
        let (rw, rh) = self.rotated;
        let inside = |cx: usize, cy: usize| cx >= ox && cy >= oy && cx < ox + rw && cy < oy + rh;
        for py in 0..8 {
            for px in 0..8 {
                let (gx, gy) = (x + 2 * px, y + 2 * py);
                let slot = (py % 4) * 16 + (px % 4) * 2;
                let block = &mut mcu.y[(py / 4) * 2 + px / 4];
                let c = py * 8 + px;
                if !(inside(gx, gy) && inside(gx + 1, gy + 1)) {
                    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        block[slot + dy * 8 + dx] = if inside(gx + dx, gy + dy) {
                            self.luma_at(gx + dx - ox, gy + dy - oy)
                        } else {
                            BLACK_Y
                        };
                    }
                    (mcu.cb[c], mcu.cr[c]) = if inside(gx, gy) {
                        self.chroma_at(gx - ox, gy - oy)
                    } else {
                        (0.0, 0.0)
                    };
                    continue;
                }
                let (cx, cy) = (gx - ox, gy - oy);
                match self.image.pixels {
                    Pixels::Bgrx { data, .. } => {
                        let mut sum = [0f32; 3];
                        for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                            let i = self.luma.at(cx + dx, cy + dy);
                            let (b, g, r) =
                                (data[i] as f32, data[i + 1] as f32, data[i + 2] as f32);
                            block[slot + dy * 8 + dx] = luma(r, g, b);
                            sum[0] += r;
                            sum[1] += g;
                            sum[2] += b;
                        }
                        (mcu.cb[c], mcu.cr[c]) =
                            chroma(sum[0] * 0.25, sum[1] * 0.25, sum[2] * 0.25);
                    }
                    Pixels::I420 { y, cb, cr, .. } => {
                        for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                            block[slot + dy * 8 + dx] =
                                y[self.luma.at(cx + dx, cy + dy)] as f32 - 128.0;
                        }
                        let i = self.chroma.at(cx / 2, cy / 2);
                        mcu.cb[c] = cb[i] as f32 - 128.0;
                        mcu.cr[c] = cr[i] as f32 - 128.0;
                    }
                }
            }
        }
    }

    fn luma_at(&self, cx: usize, cy: usize) -> f32 {
        let i = self.luma.at(cx, cy);
        match self.image.pixels {
            Pixels::Bgrx { data, .. } => {
                luma(data[i + 2] as f32, data[i + 1] as f32, data[i] as f32)
            }
            Pixels::I420 { y, .. } => y[i] as f32 - 128.0,
        }
    }

    /// Chroma for a 2x2 group the content edge cuts.
    fn chroma_at(&self, cx: usize, cy: usize) -> (f32, f32) {
        match self.image.pixels {
            Pixels::Bgrx { data, .. } => {
                let i = self.luma.at(cx, cy);
                chroma(data[i + 2] as f32, data[i + 1] as f32, data[i] as f32)
            }
            Pixels::I420 { cb, cr, .. } => {
                let i = self.chroma.at(cx / 2, cy / 2);
                (cb[i] as f32 - 128.0, cr[i] as f32 - 128.0)
            }
        }
    }
}

struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    acc: u64,
    count: u32,
}

impl BitWriter<'_> {
    fn put(&mut self, bits: u32, len: u32) {
        self.acc = (self.acc << len) | bits as u64;
        self.count += len;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.acc >> self.count) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0x00);
            }
        }
    }

    fn finish(mut self) {
        if self.count > 0 {
            let pad = 8 - self.count;
            self.put((1 << pad) - 1, pad);
        }
    }
}

fn category(value: i32) -> u32 {
    32 - value.unsigned_abs().leading_zeros()
}

fn magnitude_bits(value: i32, category: u32) -> u32 {
    let v = if value < 0 { value - 1 } else { value };
    v as u32 & ((1u32 << category) - 1)
}

/// Codes one block; `predictor` carries the component's last DC.
fn code_block(
    writer: &mut BitWriter,
    samples: &[f32; 64],
    reciprocal: &[f32; 64],
    dc: &Table,
    ac: &Table,
    predictor: &mut i32,
) {
    let coefficients = dct::forward(samples);
    let natural: [i32; 64] = std::array::from_fn(|i| {
        let v = coefficients[i] * reciprocal[i];
        (v + 0.5f32.copysign(v)) as i32
    });
    let diff = natural[0] - *predictor;
    *predictor = natural[0];
    let cat = category(diff);
    let code = dc.codes[cat as usize];
    writer.put(code.bits as u32, code.len as u32);
    writer.put(magnitude_bits(diff, cat), cat);

    let mut run = 0;
    for &n in &ZIGZAG[1..] {
        let value = natural[n].clamp(-1023, 1023);
        if value == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            let zrl = ac.codes[0xF0];
            writer.put(zrl.bits as u32, zrl.len as u32);
            run -= 16;
        }
        let cat = category(value);
        let code = ac.codes[(run << 4 | cat) as usize];
        writer.put(code.bits as u32, code.len as u32);
        writer.put(magnitude_bits(value, cat), cat);
        run = 0;
    }
    if run > 0 {
        let eob = ac.codes[0x00];
        writer.put(eob.bits as u32, eob.len as u32);
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend([0xFF, marker]);
    out.extend(((payload.len() + 2) as u16).to_be_bytes());
    out.extend(payload);
}

fn headers(out: &mut Vec<u8>, quant: &QuantTables, width: usize, height: usize) {
    out.extend([0xFF, 0xD8]);
    let mut dqt = Vec::with_capacity(130);
    for (id, table) in [(0u8, &quant.luma), (1, &quant.chroma)] {
        dqt.push(id);
        dqt.extend(ZIGZAG.map(|n| table[n]));
    }
    segment(out, 0xDB, &dqt);

    let mut sof = vec![8];
    sof.extend((height as u16).to_be_bytes());
    sof.extend((width as u16).to_be_bytes());
    sof.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segment(out, 0xC0, &sof);

    let huffman = huffman();
    let mut dht = Vec::new();
    for (class, set) in [(0u8, &huffman.dc), (1, &huffman.ac)] {
        for (id, table) in set.iter().enumerate() {
            dht.push(class << 4 | id as u8);
            dht.extend(table.spec.bits);
            dht.extend(&table.spec.values);
        }
    }
    segment(out, 0xC4, &dht);
    segment(out, 0xDA, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
}

/// Appends canvas rows `rows` as one JPEG at `quality` (1-100) to `out`.
/// `rows.start` is a multiple of 16.
pub fn encode(
    image: &Image,
    placement: Placement,
    rows: Range<usize>,
    quality: u8,
    out: &mut Vec<u8>,
) {
    let quant = QuantTables::for_quality(quality);
    let reciprocal = [&quant.luma, &quant.chroma].map(|t| t.map(|q| 1.0 / q as f32));
    let width = placement.canvas.0;
    headers(out, &quant, width, rows.len());

    let reader = Reader::new(image, placement);
    let huffman = huffman();
    let mut writer = BitWriter {
        out,
        acc: 0,
        count: 0,
    };
    let mut predictors = [0i32; 3];
    let mut mcu = Mcu {
        y: [[0.0; 64]; 4],
        cb: [0.0; 64],
        cr: [0.0; 64],
    };
    for y in rows.step_by(16) {
        for x in (0..width).step_by(16) {
            reader.read(x, y, &mut mcu);
            for block in &mcu.y {
                let (dc, ac) = (&huffman.dc[0], &huffman.ac[0]);
                code_block(
                    &mut writer,
                    block,
                    &reciprocal[0],
                    dc,
                    ac,
                    &mut predictors[0],
                );
            }
            for (i, block) in [&mcu.cb, &mcu.cr].into_iter().enumerate() {
                let (dc, ac) = (&huffman.dc[1], &huffman.ac[1]);
                let predictor = &mut predictors[1 + i];
                code_block(&mut writer, block, &reciprocal[1], dc, ac, predictor);
            }
        }
    }
    writer.finish();
    out.extend([0xFF, 0xD9]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bgrx(width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| {
                let (x, y) = (i % width, i / width);
                [(x * 7) as u8, (y * 5) as u8, (x ^ y) as u8, 0]
            })
            .collect()
    }

    fn jpeg(image: &Image, placement: Placement, rows: Range<usize>) -> Vec<u8> {
        let mut out = Vec::new();
        encode(image, placement, rows, 80, &mut out);
        out
    }

    #[test]
    fn writes_a_baseline_jpeg_of_the_rows() {
        let data = bgrx(40, 24);
        let image = Image {
            width: 40,
            height: 24,
            pixels: Pixels::Bgrx {
                data: &data,
                stride: 160,
            },
        };
        let placement = Placement {
            rotation: 1,
            canvas: (48, 64),
            origin: (12, 8),
        };
        let out = jpeg(&image, placement, 16..64);
        assert_eq!(&out[..2], &[0xFF, 0xD8]);
        assert_eq!(&out[out.len() - 2..], &[0xFF, 0xD9]);
        let sof = out.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        assert_eq!(&out[sof + 5..sof + 9], &[0, 48, 0, 48]);
    }

    #[test]
    fn walks_match_rotations() {
        let size = (3, 2);
        let stride = 3;
        let at = |rotation, x, y| Walk::new(rotation, size, stride, 1).at(x, y);
        assert_eq!(at(0, 2, 1), 5);
        assert_eq!(at(1, 0, 0), 2);
        assert_eq!(at(1, 1, 2), 3);
        assert_eq!(at(2, 0, 0), 5);
        assert_eq!(at(3, 0, 0), 3);
        assert_eq!(at(3, 1, 2), 2);
    }

    #[test]
    fn black_canvas_codes_like_black_pixels() {
        let data = vec![0u8; 16 * 16 * 4];
        let image = Image {
            width: 16,
            height: 16,
            pixels: Pixels::Bgrx {
                data: &data,
                stride: 64,
            },
        };
        let placed = Placement {
            rotation: 0,
            canvas: (32, 16),
            origin: (16, 0),
        };
        let filled = Placement {
            rotation: 0,
            canvas: (16, 16),
            origin: (0, 0),
        };
        let wide = jpeg(&image, placed, 0..16);
        let narrow = jpeg(&image, filled, 0..16);
        assert!(wide.len() > narrow.len());
        let i420 = vec![0u8; 16 * 16];
        let chroma = vec![128u8; 8 * 8];
        let planar = Image {
            width: 16,
            height: 16,
            pixels: Pixels::I420 {
                y: &i420,
                y_stride: 16,
                cb: &chroma,
                cr: &chroma,
                c_stride: 8,
            },
        };
        assert_eq!(jpeg(&planar, filled, 0..16), narrow);
    }

    #[test]
    fn category_and_magnitude_bits() {
        assert_eq!(category(0), 0);
        assert_eq!(category(-1), 1);
        assert_eq!(category(1023), 10);
        assert_eq!(magnitude_bits(-1, 1), 0);
        assert_eq!(magnitude_bits(-3, 2), 0);
        assert_eq!(magnitude_bits(3, 2), 3);
    }
}
