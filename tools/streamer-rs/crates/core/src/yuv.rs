use crate::bail;
use crate::color::Matrix;
use crate::error::Result;
use crate::resample::Resampler;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    I420,
    Nv12,
    I422,
    I444,
}

impl Layout {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "I420" | "I420A" => Self::I420,
            "NV12" => Self::Nv12,
            "I422" | "I422A" => Self::I422,
            "I444" | "I444A" => Self::I444,
            _ => return None,
        })
    }
}

pub struct Source<'a> {
    pub layout: Layout,
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
    pub planes: &'a [(usize, usize)],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Plane {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0; width * height],
        }
    }

    fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    fn rotate_cw(&self, degrees: u32) -> Self {
        let (w, h) = (self.width, self.height);
        match degrees % 360 {
            0 => self.clone(),
            180 => Self {
                width: w,
                height: h,
                data: self.data.iter().rev().copied().collect(),
            },
            d => {
                const TILE: usize = 16;
                let mut out = Self::new(h, w);
                for ty in (0..w).step_by(TILE) {
                    for tx in (0..h).step_by(TILE) {
                        for y in ty..(ty + TILE).min(w) {
                            let row = &mut out.data[y * h..(y + 1) * h];
                            for (x, o) in
                                row.iter_mut().enumerate().take((tx + TILE).min(h)).skip(tx)
                            {
                                *o = if d == 90 {
                                    self.data[(h - 1 - x) * w + y]
                                } else {
                                    self.data[x * w + (w - 1 - y)]
                                };
                            }
                        }
                    }
                }
                out
            }
        }
    }

    fn turned(self, degrees: u32) -> Self {
        if degrees.is_multiple_of(360) {
            self
        } else {
            self.rotate_cw(degrees)
        }
    }

    fn crop(self, x0: usize, y0: usize, width: usize, height: usize) -> Self {
        if (x0, y0, width, height) == (0, 0, self.width, self.height) {
            return self;
        }
        let mut out = Self::new(width, height);
        for y in 0..height {
            let start = (y0 + y) * self.width + x0;
            out.data[y * width..(y + 1) * width].copy_from_slice(&self.data[start..start + width]);
        }
        out
    }

    fn resize(self, width: usize, height: usize, cache: &mut Option<Resampler>) -> Self {
        if (width, height) == (self.width, self.height) {
            return self;
        }
        let fresh = |r: &Resampler| r.src != (self.width, self.height) || r.dst != (width, height);
        if cache.as_ref().is_none_or(fresh) {
            *cache = Some(Resampler::new(self.width, self.height, width, height));
        }
        let mut out = Self::new(width, height);
        let resampler = cache.as_mut().expect("resampler was just made");
        resampler.plane(&self.data, self.width, &mut out.data);
        out
    }
}

/// Plane `index` of `source` from its offset, and its stride.
fn plane<'a>(
    source: &Source<'a>,
    index: usize,
    width: usize,
    height: usize,
    step: usize,
) -> Result<(&'a [u8], usize)> {
    let Some(&(offset, stride)) = source.planes.get(index) else {
        bail!("missing plane {index}");
    };
    let needed = offset + stride * (height.max(1) - 1) + width * step;
    if source.data.len() < needed {
        bail!("plane {index} is truncated");
    }
    Ok((&source.data[offset..], stride))
}

fn read(
    source: &Source,
    index: usize,
    width: usize,
    height: usize,
    step: usize,
    phase: usize,
) -> Result<Plane> {
    let (data, stride) = plane(source, index, width, height, step)?;
    let mut out = Plane::new(width, height);
    for (y, dst) in out.data.chunks_exact_mut(width).enumerate() {
        let row = &data[y * stride..];
        if step == 1 {
            dst.copy_from_slice(&row[..width]);
        } else {
            for (x, d) in dst.iter_mut().enumerate() {
                *d = row[x * step + phase];
            }
        }
    }
    Ok(out)
}

fn halve(plane: &Plane, horizontal: bool, vertical: bool) -> Plane {
    let w = if horizontal {
        plane.width.div_ceil(2)
    } else {
        plane.width
    };
    let h = if vertical {
        plane.height.div_ceil(2)
    } else {
        plane.height
    };
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let xs: &[usize] = if horizontal {
                &[2 * x, 2 * x + 1]
            } else {
                &[x]
            };
            let ys: &[usize] = if vertical { &[2 * y, 2 * y + 1] } else { &[y] };
            let (mut sum, mut n) = (0u32, 0u32);
            for &sy in ys {
                for &sx in xs {
                    let (sx, sy) = (sx.min(plane.width - 1), sy.min(plane.height - 1));
                    sum += plane.at(sx, sy) as u32;
                    n += 1;
                }
            }
            out.data[y * w + x] = ((sum + n / 2) / n) as u8;
        }
    }
    out
}

/// Plane `index` of `source`, read straight into the resampler when it is
/// scaled.
fn scaled(
    source: &Source,
    index: usize,
    (width, height): (usize, usize),
    (dw, dh): (usize, usize),
    cache: &mut Option<Resampler>,
) -> Result<Plane> {
    if (dw, dh) == (width, height) {
        return read(source, index, width, height, 1, 0);
    }
    let (data, stride) = plane(source, index, width, height, 1)?;
    let fresh = |r: &Resampler| r.src != (width, height) || r.dst != (dw, dh);
    if cache.as_ref().is_none_or(fresh) {
        *cache = Some(Resampler::new(width, height, dw, dh));
    }
    let mut out = Plane::new(dw, dh);
    let resampler = cache.as_mut().expect("resampler was just made");
    resampler.plane(data, stride, &mut out.data);
    Ok(out)
}

fn chroma_planes(source: &Source) -> Result<[Plane; 2]> {
    let (w, h) = (source.width, source.height);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (u, v) = match source.layout {
        Layout::I420 => (
            read(source, 1, cw, ch, 1, 0)?,
            read(source, 2, cw, ch, 1, 0)?,
        ),
        Layout::Nv12 => (
            read(source, 1, cw, ch, 2, 0)?,
            read(source, 1, cw, ch, 2, 1)?,
        ),
        Layout::I422 => (
            halve(&read(source, 1, cw, h, 1, 0)?, false, true),
            halve(&read(source, 2, cw, h, 1, 0)?, false, true),
        ),
        Layout::I444 => (
            halve(&read(source, 1, w, h, 1, 0)?, true, true),
            halve(&read(source, 2, w, h, 1, 0)?, true, true),
        ),
    };
    Ok([u, v])
}

pub fn to_i420(source: &Source) -> Result<[Plane; 3]> {
    let y = read(source, 0, source.width, source.height, 1, 0)?;
    let [u, v] = chroma_planes(source)?;
    Ok([y, u, v])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Limited,
    Bt601Full,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub scaled: (usize, usize),
    pub crop: (usize, usize),
    pub source_rotation: u32,
    pub output_rotation: u32,
    pub stored: (usize, usize),
    pub output: Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceColor {
    pub matrix: Matrix,
    pub full_range: bool,
}

fn weights(matrix: Matrix) -> (f32, f32) {
    match matrix {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
    }
}

struct Transform {
    rows: [[f32; 3]; 3],
    input: [(f32, f32); 3],
    output: [(f32, f32); 3],
    folded: [[f32; 3]; 3],
}

impl Transform {
    fn new(color: SourceColor, output: Output) -> Self {
        let range = |full: bool| {
            if full {
                [(0.0, 255.0), (128.0, 255.0), (128.0, 255.0)]
            } else {
                [(16.0, 219.0), (128.0, 224.0), (128.0, 224.0)]
            }
        };
        let (kr, kb) = weights(color.matrix);
        let kg = 1.0 - kr - kb;
        let to_rgb = [
            [1.0, 0.0, 2.0 * (1.0 - kr)],
            [
                1.0,
                -2.0 * (1.0 - kb) * kb / kg,
                -2.0 * (1.0 - kr) * kr / kg,
            ],
            [1.0, 2.0 * (1.0 - kb), 0.0],
        ];
        let target = match output {
            Output::Limited => color.matrix,
            Output::Bt601Full => Matrix::Bt601,
        };
        let (tr, tb) = weights(target);
        let tg = 1.0 - tr - tb;
        let from_rgb = [
            [tr, tg, tb],
            [-tr / (2.0 * (1.0 - tb)), -tg / (2.0 * (1.0 - tb)), 0.5],
            [0.5, -tg / (2.0 * (1.0 - tr)), -tb / (2.0 * (1.0 - tr))],
        ];
        let mut rows = [[0f32; 3]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| from_rgb[i][k] * to_rgb[k][j]).sum();
            }
        }
        let (input, output) = (range(color.full_range), range(output == Output::Bt601Full));
        let folded =
            std::array::from_fn(|r| std::array::from_fn(|k| rows[r][k] * output[r].1 / input[k].1));
        Self {
            rows,
            input,
            output,
            folded,
        }
    }

    fn is_identity(&self) -> bool {
        let unit = (0..3)
            .all(|i| (0..3).all(|j| (self.rows[i][j] - f32::from(u8::from(i == j))).abs() < 1e-6));
        unit && self.input == self.output
    }

    fn apply_row(&self, row: usize, y: &[u8], u: &[u8], v: &[u8], out: &mut Vec<u8>) {
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        let done = wasm::apply_row(self, row, y, u, v, out);
        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        let done = 0;
        for i in done..y.len() {
            out.push(self.apply(row, [y[i], u[i], v[i]]));
        }
    }

    fn apply(&self, row: usize, yuv: [u8; 3]) -> u8 {
        let f = &self.folded[row];
        let d: [f32; 3] = std::array::from_fn(|k| yuv[k] as f32 - self.input[k].0);
        let v = f[0] * d[0] + f[1] * d[1] + f[2] * d[2];
        (self.output[row].0 + v).round().clamp(0.0, 255.0) as u8
    }
}

pub struct Converter {
    geometry: Geometry,
    luma: Option<Resampler>,
    chroma: Option<Resampler>,
}

impl Converter {
    pub fn new(geometry: Geometry) -> Self {
        Self {
            geometry,
            luma: None,
            chroma: None,
        }
    }

    pub fn convert(&mut self, source: &Source, color: SourceColor) -> Result<Vec<u8>> {
        let g = self.geometry;
        let (sw, sh) = g.scaled;
        let (dw, dh) = if g.source_rotation % 180 == 90 {
            (sh, sw)
        } else {
            (sw, sh)
        };
        let (w, h) = g.crop;
        if w > sw || h > sh {
            bail!("crop {w}x{h} is larger than the scaled {sw}x{sh}");
        }
        let (x0, y0) = (((sw - w) / 2) & !1, ((sh - h) / 2) & !1);
        let back = (360 - g.output_rotation % 360) % 360;
        let size = (source.width, source.height);
        let luma = scaled(source, 0, size, (dw, dh), &mut self.luma)?
            .turned(g.source_rotation)
            .crop(x0, y0, w, h)
            .turned(back);
        let chroma_size = (dw.div_ceil(2), dh.div_ceil(2));
        let finish = |p: Plane| {
            p.turned(g.source_rotation)
                .crop(x0 / 2, y0 / 2, w.div_ceil(2), h.div_ceil(2))
                .turned(back)
        };
        let (u, v) = if source.layout == Layout::I420 {
            let size = (size.0.div_ceil(2), size.1.div_ceil(2));
            (
                scaled(source, 1, size, chroma_size, &mut self.chroma)?,
                scaled(source, 2, size, chroma_size, &mut self.chroma)?,
            )
        } else {
            let [u, v] = chroma_planes(source)?;
            (
                u.resize(chroma_size.0, chroma_size.1, &mut self.chroma),
                v.resize(chroma_size.0, chroma_size.1, &mut self.chroma),
            )
        };
        let (u, v) = (finish(u), finish(v));
        if (luma.width, luma.height) != g.stored {
            bail!(
                "converted picture is {}x{}, expected {}x{}",
                luma.width,
                luma.height,
                g.stored.0,
                g.stored.1
            );
        }
        let transform = Transform::new(color, g.output);
        let mut out = Vec::with_capacity(luma.data.len() * 3 / 2);
        if transform.is_identity() {
            for plane in [&luma, &u, &v] {
                out.extend_from_slice(&plane.data);
            }
            return Ok(out);
        }
        let (w, h) = (luma.width, luma.height);
        let mut chroma = [
            Vec::with_capacity(u.data.len()),
            Vec::with_capacity(u.data.len()),
        ];
        let (mut cu, mut cv) = (vec![0; w], vec![0; w]);
        let doubled = |dst: &mut [u8], src: &[u8]| {
            let pairs = dst.len() / 2;
            for (pair, &c) in dst.chunks_exact_mut(2).zip(&src[..pairs]) {
                pair[0] = c;
                pair[1] = c;
            }
            if dst.len() % 2 == 1 {
                dst[dst.len() - 1] = src[pairs];
            }
        };
        for (y, row) in luma.data.chunks_exact(w).enumerate() {
            let cy = y / 2;
            doubled(&mut cu, &u.data[cy * u.width..(cy + 1) * u.width]);
            doubled(&mut cv, &v.data[cy * v.width..(cy + 1) * v.width]);
            transform.apply_row(0, row, &cu, &cv, &mut out);
        }
        let mut mean = vec![0; u.width];
        for cy in 0..u.height {
            let a = &luma.data[2 * cy * w..][..w];
            let b = &luma.data[(2 * cy + 1).min(h - 1) * w..][..w];
            for ((m, a), b) in mean
                .iter_mut()
                .zip(a.chunks_exact(2))
                .zip(b.chunks_exact(2))
            {
                let sum = a[0] as u16 + a[1] as u16 + b[0] as u16 + b[1] as u16;
                *m = ((sum + 2) / 4) as u8;
            }
            if w % 2 == 1 {
                let sum = 2 * a[w - 1] as u16 + 2 * b[w - 1] as u16;
                mean[w / 2] = ((sum + 2) / 4) as u8;
            }
            let at = cy * u.width..(cy + 1) * u.width;
            let (cb, cr) = (&u.data[at.clone()], &v.data[at]);
            transform.apply_row(1, &mean, cb, cr, &mut chroma[0]);
            transform.apply_row(2, &mean, cb, cr, &mut chroma[1]);
        }
        out.extend_from_slice(&chroma[0]);
        out.extend_from_slice(&chroma[1]);
        Ok(out)
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use core::arch::wasm32::*;

    use super::Transform;
    use crate::num::wasm::round;

    fn apply(f: &[v128; 3], o: &[v128; 3], offset: v128, k: [v128; 3]) -> v128 {
        let d: [v128; 3] = std::array::from_fn(|c| f32x4_sub(k[c], o[c]));
        let mut v = f32x4_mul(f[0], d[0]);
        v = f32x4_add(v, f32x4_mul(f[1], d[1]));
        v = f32x4_add(v, f32x4_mul(f[2], d[2]));
        let r = round(f32x4_add(offset, v));
        i32x4_trunc_sat_f32x4(f32x4_min(
            f32x4_max(r, f32x4_splat(0.0)),
            f32x4_splat(255.0),
        ))
    }

    fn quarters(v: v128) -> [v128; 4] {
        let (lo, hi) = (u16x8_extend_low_u8x16(v), u16x8_extend_high_u8x16(v));
        [
            u32x4_extend_low_u16x8(lo),
            u32x4_extend_high_u16x8(lo),
            u32x4_extend_low_u16x8(hi),
            u32x4_extend_high_u16x8(hi),
        ]
        .map(|q| f32x4_convert_u32x4(q))
    }

    pub fn apply_row(
        t: &Transform,
        row: usize,
        y: &[u8],
        u: &[u8],
        v: &[u8],
        out: &mut Vec<u8>,
    ) -> usize {
        let n = y.len() / 16 * 16;
        let (u, v) = (&u[..y.len()], &v[..y.len()]);
        let f = t.folded[row].map(|c| f32x4_splat(c));
        let o = t.input.map(|(o, _)| f32x4_splat(o));
        let offset = f32x4_splat(t.output[row].0);
        for i in (0..n).step_by(16) {
            let load = |p: &[u8]| unsafe { quarters(v128_load(p.as_ptr().add(i) as *const v128)) };
            let (ys, us, vs) = (load(y), load(u), load(v));
            let w: [v128; 4] =
                std::array::from_fn(|q| apply(&f, &o, offset, [ys[q], us[q], vs[q]]));
            let bytes = u8x16_narrow_i16x8(
                i16x8_narrow_i32x4(w[0], w[1]),
                i16x8_narrow_i32x4(w[2], w[3]),
            );
            let mut chunk = [0u8; 16];
            unsafe { v128_store(chunk.as_mut_ptr() as *mut v128, bytes) };
            out.extend_from_slice(&chunk);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(width: usize, height: usize, data: &[u8]) -> Plane {
        Plane {
            width,
            height,
            data: data.to_vec(),
        }
    }

    #[test]
    fn rotations() {
        let p = plane(3, 2, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(p.rotate_cw(90), plane(2, 3, &[4, 1, 5, 2, 6, 3]));
        assert_eq!(p.rotate_cw(270), plane(2, 3, &[3, 6, 2, 5, 1, 4]));
        assert_eq!(p.rotate_cw(180), plane(3, 2, &[6, 5, 4, 3, 2, 1]));
        assert_eq!(p.rotate_cw(90).rotate_cw(270), p);
    }

    #[test]
    fn layouts_become_i420() {
        let (w, h) = (4, 2);
        let mut data = vec![10u8; w * h];
        data.extend([1, 2, 3, 4]);
        let nv12 = Source {
            layout: Layout::Nv12,
            width: w,
            height: h,
            data: &data,
            planes: &[(0, w), (w * h, w)],
        };
        let [y, u, v] = to_i420(&nv12).unwrap();
        assert_eq!((y.data.len(), u.data, v.data), (8, vec![1, 3], vec![2, 4]));

        let mut data = vec![0u8; w * h];
        data.extend([10, 20, 30, 40, 50, 60, 70, 80]);
        data.extend([0; 8]);
        let i444 = Source {
            layout: Layout::I444,
            width: w,
            height: h,
            data: &data,
            planes: &[(0, w), (8, w), (16, w)],
        };
        let [_, u, _] = to_i420(&i444).unwrap();
        assert_eq!(u.data, vec![35, 55]);
    }

    #[test]
    fn scales_strided_planes() {
        let (w, h, stride) = (16, 8, 20);
        let data: Vec<u8> = (0..stride * h + 2 * 10 * h / 2)
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        let source = Source {
            layout: Layout::I420,
            width: w,
            height: h,
            data: &data,
            planes: &[(0, stride), (stride * h, 10), (stride * h + 10 * h / 2, 10)],
        };
        let [y, u, _] = to_i420(&source).unwrap();
        let mut cache = None;
        let direct = scaled(&source, 0, (w, h), (6, 4), &mut cache).unwrap();
        assert_eq!(direct, y.resize(6, 4, &mut None));
        let direct = scaled(&source, 1, (8, 4), (3, 2), &mut cache).unwrap();
        assert_eq!(direct, u.resize(3, 2, &mut None));
    }

    #[test]
    fn identity_and_full_range() {
        let (w, h) = (4, 2);
        let data: Vec<u8> = (0..12).map(|i| i * 20).collect();
        let source = Source {
            layout: Layout::I420,
            width: w,
            height: h,
            data: &data,
            planes: &[(0, w), (8, 2), (10, 2)],
        };
        let geometry = Geometry {
            scaled: (w, h),
            crop: (w, h),
            source_rotation: 0,
            output_rotation: 0,
            stored: (w, h),
            output: Output::Limited,
        };
        let limited = SourceColor {
            matrix: Matrix::Bt709,
            full_range: false,
        };
        assert_eq!(
            Converter::new(geometry).convert(&source, limited).unwrap(),
            data
        );
        let full = SourceColor {
            matrix: Matrix::Bt709,
            full_range: true,
        };
        let out = Converter::new(geometry).convert(&source, full).unwrap();
        assert_eq!((out[0], out[7], out[8]), (16, 136, 156));
    }

    #[test]
    fn matrix_conversion_matches_rgb_round_trip() {
        let geometry = Geometry {
            scaled: (2, 2),
            crop: (2, 2),
            source_rotation: 0,
            output_rotation: 0,
            stored: (2, 2),
            output: Output::Bt601Full,
        };
        let red_709 = [63u8, 63, 63, 63, 102, 240];
        let source = Source {
            layout: Layout::I420,
            width: 2,
            height: 2,
            data: &red_709,
            planes: &[(0, 2), (4, 1), (5, 1)],
        };
        let color = SourceColor {
            matrix: Matrix::Bt709,
            full_range: false,
        };
        let out = Converter::new(geometry).convert(&source, color).unwrap();
        let mut expected = vec![0; 6];
        crate::color::rgba_to_yuv420_bt601_full(&[255, 0, 0, 255].repeat(4), 2, 2, &mut expected);
        for (a, b) in out.iter().zip(&expected) {
            assert!(a.abs_diff(*b) <= 1, "{out:?} {expected:?}");
        }
    }

    #[test]
    fn wrong_geometry_is_an_error() {
        let data = vec![0u8; 24];
        let source = Source {
            layout: Layout::I420,
            width: 4,
            height: 4,
            data: &data,
            planes: &[(0, 4), (16, 2), (20, 2)],
        };
        let geometry = Geometry {
            scaled: (4, 4),
            crop: (4, 4),
            source_rotation: 0,
            output_rotation: 90,
            stored: (2, 8),
            output: Output::Limited,
        };
        let color = SourceColor {
            matrix: Matrix::Bt601,
            full_range: false,
        };
        assert!(Converter::new(geometry).convert(&source, color).is_err());
    }
}
