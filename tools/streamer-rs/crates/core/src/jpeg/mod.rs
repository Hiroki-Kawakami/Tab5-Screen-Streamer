//! Baseline 4:2:0 JPEG encoder that rotates and places the image while
//! reading it, so a frame is converted, transformed and coded in one pass.

mod dct;
mod huffman;
mod tables;

use std::ops::Range;
use std::sync::OnceLock;

use dct::Block;
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
    /// Y plane and interleaved Cb, Cr plane, otherwise as I420.
    Nv12 {
        y: &'a [u8],
        y_stride: usize,
        uv: &'a [u8],
        uv_stride: usize,
    },
}

/// I420 and NV12 alike: chroma samples are `c_step` bytes apart.
#[derive(Clone, Copy)]
struct Planar<'a> {
    y: &'a [u8],
    y_stride: usize,
    cb: &'a [u8],
    cr: &'a [u8],
    c_stride: usize,
    c_step: usize,
}

#[derive(Clone, Copy)]
enum Source<'a> {
    Bgrx { data: &'a [u8], stride: usize },
    Yuv(Planar<'a>),
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

/// One MCU of level shifted samples. Each block is in the image's own
/// orientation; `Coding` rotates the coefficients instead of the pixels.
struct Mcu {
    y: [Block; 4],
    cb: Block,
    cr: Block,
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

/// Where canvas pixel (x, y) of an 8x8 block sits in the unrotated block.
fn unrotated_slot(rotation: u32, x: usize, y: usize) -> usize {
    match rotation % 4 {
        1 => x * 8 + 7 - y,
        2 => (7 - y) * 8 + 7 - x,
        3 => (7 - x) * 8 + y,
        _ => y * 8 + x,
    }
}

struct Reader<'a> {
    image: &'a Image<'a>,
    source: Source<'a>,
    placement: Placement,
    rotated: (usize, usize),
    luma: Walk,
    chroma: Walk,
    /// The canvas block of each image block of an MCU, in image order.
    blocks: [usize; 4],
}

impl<'a> Reader<'a> {
    fn new(image: &'a Image<'a>, placement: Placement) -> Self {
        let size = (image.width, image.height);
        let rotated = if placement.rotation % 2 == 1 {
            (size.1, size.0)
        } else {
            size
        };
        let source = match image.pixels {
            Pixels::Bgrx { data, stride } => Source::Bgrx { data, stride },
            Pixels::I420 {
                y,
                y_stride,
                cb,
                cr,
                c_stride,
            } => Source::Yuv(Planar {
                y,
                y_stride,
                cb,
                cr,
                c_stride,
                c_step: 1,
            }),
            Pixels::Nv12 {
                y,
                y_stride,
                uv,
                uv_stride,
            } => Source::Yuv(Planar {
                y,
                y_stride,
                cb: uv,
                cr: &uv[1..],
                c_stride: uv_stride,
                c_step: 2,
            }),
        };
        let (luma, chroma) = match source {
            Source::Bgrx { stride, .. } => {
                let walk = Walk::new(placement.rotation, size, stride, 4);
                (walk, walk)
            }
            Source::Yuv(p) => (
                Walk::new(placement.rotation, size, p.y_stride, 1),
                Walk::new(
                    placement.rotation,
                    (size.0 / 2, size.1 / 2),
                    p.c_stride,
                    p.c_step,
                ),
            ),
        };
        let blocks = match placement.rotation % 4 {
            1 => [2, 0, 3, 1],
            2 => [3, 2, 1, 0],
            3 => [1, 3, 0, 2],
            _ => [0, 1, 2, 3],
        };
        Reader {
            image,
            source,
            placement,
            rotated,
            luma,
            chroma,
            blocks,
        }
    }

    /// Fills `mcu` with the canvas MCU at (x, y).
    fn read(&self, x: usize, y: usize, mcu: &mut Mcu) {
        let (ox, oy) = self.placement.origin;
        let (rw, rh) = self.rotated;
        if x >= ox && y >= oy && x + 16 <= ox + rw && y + 16 <= oy + rh {
            self.read_inside(x - ox, y - oy, mcu);
        } else {
            self.read_edge(x, y, mcu);
        }
    }

    /// An MCU wholly inside the content at content (cx, cy): a 16x16 square
    /// of the image, read row by row.
    fn read_inside(&self, cx: usize, cy: usize, mcu: &mut Mcu) {
        let (w, h) = (self.image.width, self.image.height);
        let (sx, sy) = match self.placement.rotation % 4 {
            1 => (w - 16 - cy, cx),
            2 => (w - 16 - cx, h - 16 - cy),
            3 => (cy, h - 16 - cx),
            _ => (cx, cy),
        };
        let blocks = self.blocks;
        match self.source {
            Source::Bgrx { data, stride } => {
                let mut upper = [0u32; 16];
                for row in 0..16 {
                    let line = &data[(sy + row) * stride + sx * 4..][..64];
                    // Whole u32 pixels rather than byte loads: LLVM vectorizes the
                    // shifts, not the interleaved bytes.
                    let mut pixels = [0u32; 16];
                    for (p, bytes) in pixels.iter_mut().zip(line.chunks_exact(4)) {
                        *p = u32::from_le_bytes(bytes.try_into().unwrap());
                    }
                    let mut y = [0f32; 16];
                    for (y, &p) in y.iter_mut().zip(&pixels) {
                        let (r, g, b) = (p >> 16 & 0xFF, p >> 8 & 0xFF, p & 0xFF);
                        *y = (19595 * r + 38470 * g + 7471 * b) as f32 * (1.0 / 65536.0) - 128.0;
                    }
                    let top = row / 8 * 2;
                    mcu.y[blocks[top]][row % 8].copy_from_slice(&y[..8]);
                    mcu.y[blocks[top + 1]][row % 8].copy_from_slice(&y[8..]);
                    if row % 2 == 0 {
                        upper = pixels;
                        continue;
                    }
                    let (cb, cr) = (&mut mcu.cb[row / 2], &mut mcu.cr[row / 2]);
                    for i in 0..8 {
                        let quad = [
                            upper[2 * i],
                            upper[2 * i + 1],
                            pixels[2 * i],
                            pixels[2 * i + 1],
                        ];
                        let channel =
                            |shift: u32| quad.iter().map(|p| p >> shift & 0xFF).sum::<u32>();
                        let (r, g, b) = (channel(16) as f32, channel(8) as f32, channel(0) as f32);
                        (cb[i], cr[i]) = chroma(r * 0.25, g * 0.25, b * 0.25);
                    }
                }
            }
            Source::Yuv(p) => {
                for row in 0..16 {
                    let line = &p.y[(sy + row) * p.y_stride + sx..][..16];
                    let top = row / 8 * 2;
                    for (half, out) in [blocks[top], blocks[top + 1]].into_iter().enumerate() {
                        for (o, &v) in mcu.y[out][row % 8].iter_mut().zip(&line[half * 8..]) {
                            *o = v as f32 - 128.0;
                        }
                    }
                }
                for row in 0..8 {
                    let at = (sy / 2 + row) * p.c_stride + sx / 2 * p.c_step;
                    for (plane, out) in [(p.cb, &mut mcu.cb), (p.cr, &mut mcu.cr)] {
                        for (col, o) in out[row].iter_mut().enumerate() {
                            *o = plane[at + col * p.c_step] as f32 - 128.0;
                        }
                    }
                }
            }
        }
    }

    /// An MCU the content edge cuts, read pixel by pixel.
    fn read_edge(&self, x: usize, y: usize, mcu: &mut Mcu) {
        let (ox, oy) = self.placement.origin;
        let (rw, rh) = self.rotated;
        let rotation = self.placement.rotation;
        let inside = |cx: usize, cy: usize| cx >= ox && cy >= oy && cx < ox + rw && cy < oy + rh;
        for py in 0..8 {
            for px in 0..8 {
                let (gx, gy) = (x + 2 * px, y + 2 * py);
                for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let (lx, ly) = (2 * px + dx, 2 * py + dy);
                    let block = &mut mcu.y[(ly / 8) * 2 + lx / 8];
                    let slot = unrotated_slot(rotation, lx % 8, ly % 8);
                    block[slot / 8][slot % 8] = if inside(gx + dx, gy + dy) {
                        self.luma_at(gx + dx - ox, gy + dy - oy)
                    } else {
                        BLACK_Y
                    };
                }
                let c = unrotated_slot(rotation, px, py);
                (mcu.cb[c / 8][c % 8], mcu.cr[c / 8][c % 8]) = if inside(gx, gy) {
                    self.chroma_at(gx - ox, gy - oy)
                } else {
                    (0.0, 0.0)
                };
            }
        }
    }

    fn luma_at(&self, cx: usize, cy: usize) -> f32 {
        let i = self.luma.at(cx, cy);
        match self.source {
            Source::Bgrx { data, .. } => {
                luma(data[i + 2] as f32, data[i + 1] as f32, data[i] as f32)
            }
            Source::Yuv(p) => p.y[i] as f32 - 128.0,
        }
    }

    /// Chroma for a 2x2 group the content edge cuts.
    fn chroma_at(&self, cx: usize, cy: usize) -> (f32, f32) {
        match self.source {
            Source::Bgrx { data, .. } => {
                let i = self.luma.at(cx, cy);
                chroma(data[i + 2] as f32, data[i + 1] as f32, data[i] as f32)
            }
            Source::Yuv(p) => {
                let i = self.chroma.at(cx / 2, cy / 2);
                (p.cb[i] as f32 - 128.0, p.cr[i] as f32 - 128.0)
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
    /// `len` is at most 32.
    fn put(&mut self, bits: u32, len: u32) {
        self.acc = (self.acc << len) | bits as u64;
        self.count += len;
        if self.count >= 32 {
            self.count -= 32;
            let word = (self.acc >> self.count) as u32;
            if (!word).wrapping_sub(0x0101_0101) & word & 0x8080_8080 == 0 {
                self.out.extend_from_slice(&word.to_be_bytes());
            } else {
                for byte in word.to_be_bytes() {
                    self.out.push(byte);
                    if byte == 0xFF {
                        self.out.push(0x00);
                    }
                }
            }
        }
    }

    fn finish(mut self) {
        let pad = (8 - self.count % 8) % 8;
        self.acc = (self.acc << pad) | ((1 << pad) - 1);
        self.count += pad;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.acc >> self.count) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0x00);
            }
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

struct Codes {
    dc: [(u32, u32); 12],
    ac: [(u32, u32); 256],
}

impl Codes {
    fn new(dc: &Table, ac: &Table) -> Self {
        Codes {
            dc: std::array::from_fn(|i| (dc.codes[i].bits as u32, dc.codes[i].len as u32)),
            ac: std::array::from_fn(|i| (ac.codes[i].bits as u32, ac.codes[i].len as u32)),
        }
    }
}

/// How one component's blocks become zigzag coefficients.
struct Coding {
    /// Per unscaled DCT output: rotation sign, DCT scale and quantizer.
    factors: Block,
    /// Per zigzag position: the DCT output it is taken from, as [u][v] flat.
    order: [usize; 64],
    codes: Codes,
}

impl Coding {
    fn new(rotation: u32, quant: &[u8; 64], dc: &Table, ac: &Table) -> Self {
        let mut factors = [[0f32; 8]; 8];
        let mut order = [0; 64];
        for (k, &n) in ZIGZAG.iter().enumerate() {
            let (v, u) = (n / 8, n % 8);
            let flip = |f: usize| if f % 2 == 1 { -1.0 } else { 1.0 };
            let (sv, su, sign) = match rotation % 4 {
                1 => (u, v, flip(v)),
                2 => (v, u, flip(u + v)),
                3 => (u, v, flip(u)),
                _ => (v, u, 1.0),
            };
            factors[su][sv] = sign * dct::SCALE[sv] * dct::SCALE[su] / quant[n] as f32;
            order[k] = su * 8 + sv;
        }
        Coding {
            factors,
            order,
            codes: Codes::new(dc, ac),
        }
    }

    /// Codes one block; `predictor` carries the component's last DC.
    fn block(&self, writer: &mut BitWriter, samples: &Block, predictor: &mut i32) {
        let raw = dct::forward(samples);
        let mut quantized = [0i32; 64];
        for (i, q) in quantized.iter_mut().enumerate() {
            let v = raw[i / 8][i % 8] * self.factors[i / 8][i % 8];
            *q = (v + 0.5f32.copysign(v)) as i32;
        }
        let zigzag: [i32; 64] = std::array::from_fn(|k| quantized[self.order[k] & 63]);
        let diff = zigzag[0] - *predictor;
        *predictor = zigzag[0];
        let cat = category(diff);
        let (code, len) = self.codes.dc[cat as usize];
        writer.put(code << cat | magnitude_bits(diff, cat), len + cat);

        let mut nonzero = 0u64;
        for (k, &value) in zigzag.iter().enumerate().skip(1) {
            nonzero |= ((value != 0) as u64) << k;
        }
        let mut last = 0;
        while nonzero != 0 {
            let k = nonzero.trailing_zeros();
            nonzero &= nonzero - 1;
            let mut run = k - last - 1;
            last = k;
            while run > 15 {
                let (code, len) = self.codes.ac[0xF0];
                writer.put(code, len);
                run -= 16;
            }
            let value = zigzag[k as usize].clamp(-1023, 1023);
            let cat = category(value);
            let (code, len) = self.codes.ac[(run << 4 | cat) as usize];
            writer.put(code << cat | magnitude_bits(value, cat), len + cat);
        }
        if last < 63 {
            let (code, len) = self.codes.ac[0x00];
            writer.put(code, len);
        }
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
    let width = placement.canvas.0;
    headers(out, &quant, width, rows.len());

    let huffman = huffman();
    let rotation = placement.rotation;
    let luma = Coding::new(rotation, &quant.luma, &huffman.dc[0], &huffman.ac[0]);
    let chroma = Coding::new(rotation, &quant.chroma, &huffman.dc[1], &huffman.ac[1]);
    let reader = Reader::new(image, placement);
    let mut writer = BitWriter {
        out,
        acc: 0,
        count: 0,
    };
    let mut predictors = [0i32; 3];
    let mut mcu = Mcu {
        y: [[[0.0; 8]; 8]; 4],
        cb: [[0.0; 8]; 8],
        cr: [[0.0; 8]; 8],
    };
    for y in rows.step_by(16) {
        for x in (0..width).step_by(16) {
            reader.read(x, y, &mut mcu);
            for block in &mcu.y {
                luma.block(&mut writer, block, &mut predictors[0]);
            }
            chroma.block(&mut writer, &mcu.cb, &mut predictors[1]);
            chroma.block(&mut writer, &mcu.cr, &mut predictors[2]);
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
    fn inside_reads_match_edge_reads() {
        let (w, h) = (48, 32);
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w / 2, i / w / 2);
                [(x * 9) as u8, (y * 17) as u8, (x * y) as u8, 0]
            })
            .collect();
        let luma: Vec<u8> = (0..w * h).map(|i| (i * 13 % 251) as u8).collect();
        let cb: Vec<u8> = (0..w * h / 4).map(|i| (i * 7 % 253) as u8).collect();
        let cr: Vec<u8> = (0..w * h / 4).map(|i| (i * 11 % 241) as u8).collect();
        let uv: Vec<u8> = cb.iter().zip(&cr).flat_map(|(&b, &r)| [b, r]).collect();
        let images = [
            Pixels::Bgrx {
                data: &data,
                stride: w * 4,
            },
            Pixels::I420 {
                y: &luma,
                y_stride: w,
                cb: &cb,
                cr: &cr,
                c_stride: w / 2,
            },
            Pixels::Nv12 {
                y: &luma,
                y_stride: w,
                uv: &uv,
                uv_stride: w,
            },
        ];
        for pixels in images {
            let image = Image {
                width: w,
                height: h,
                pixels,
            };
            for rotation in 0..4 {
                let canvas = if rotation % 2 == 1 { (h, w) } else { (w, h) };
                let placement = Placement {
                    rotation,
                    canvas,
                    origin: (0, 0),
                };
                let reader = Reader::new(&image, placement);
                for (x, y) in [(0, 0), (16, 16), (canvas.0 - 16, canvas.1 - 16)] {
                    let blank = || Mcu {
                        y: [[[0.0; 8]; 8]; 4],
                        cb: [[0.0; 8]; 8],
                        cr: [[0.0; 8]; 8],
                    };
                    let (mut inside, mut edge) = (blank(), blank());
                    reader.read_inside(x, y, &mut inside);
                    reader.read_edge(x, y, &mut edge);
                    let close = |a: &Block, b: &Block| {
                        let (a, b) = (a.as_flattened(), b.as_flattened());
                        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.5)
                    };
                    for k in 0..4 {
                        assert!(close(&inside.y[k], &edge.y[k]), "rotation {rotation} y{k}");
                    }
                    assert!(close(&inside.cb, &edge.cb), "rotation {rotation} cb");
                    assert!(close(&inside.cr, &edge.cr), "rotation {rotation} cr");
                }
            }
        }
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
