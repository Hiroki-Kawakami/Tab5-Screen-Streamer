const WEIGHT_BITS: u32 = 14;
const EXTRA_BITS: u32 = 4;
const B: f64 = 0.0;
const C: f64 = 0.6;

fn bicubic(x: f64) -> f64 {
    let x = x.abs();
    let v = if x < 1.0 {
        (12.0 - 9.0 * B - 6.0 * C) * x * x * x
            + (-18.0 + 12.0 * B + 6.0 * C) * x * x
            + (6.0 - 2.0 * B)
    } else if x < 2.0 {
        (-B - 6.0 * C) * x * x * x
            + (6.0 * B + 30.0 * C) * x * x
            + (-12.0 * B - 48.0 * C) * x
            + (8.0 * B + 24.0 * C)
    } else {
        0.0
    };
    v / 6.0
}

pub struct Taps {
    pub len: usize,
    pub start: Vec<u32>,
    pub weights: Vec<i16>,
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_feature = "simd128")),
        allow(dead_code)
    )]
    padded_len: usize,
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_feature = "simd128")),
        allow(dead_code)
    )]
    padded: Vec<i16>,
}

impl Taps {
    pub fn new(src: usize, dst: usize) -> Self {
        let scale = src as f64 / dst as f64;
        let stretch = scale.max(1.0);
        let len = ((4.0 * stretch).ceil() as usize + 1).min(src);
        let mut start = Vec::with_capacity(dst);
        let mut weights = Vec::with_capacity(dst * len);
        for d in 0..dst {
            let centre = (d as f64 + 0.5) * scale - 0.5;
            let first = ((centre - 2.0 * stretch).floor() as isize + 1)
                .clamp(0, (src - len) as isize) as usize;
            let mut w = vec![0f64; len];
            let lo = (centre - 2.0 * stretch).floor() as isize;
            let hi = (centre + 2.0 * stretch).ceil() as isize;
            for s in lo..=hi {
                let k = bicubic((s as f64 - centre) / stretch);
                if k != 0.0 {
                    let at = s.clamp(0, src as isize - 1) as usize;
                    let slot = at.clamp(first, first + len - 1) - first;
                    w[slot] += k;
                }
            }
            let sum: f64 = w.iter().sum();
            let one = 1i32 << WEIGHT_BITS;
            let mut fixed: Vec<i32> = w
                .iter()
                .map(|v| (v / sum * one as f64).round() as i32)
                .collect();
            let error = one - fixed.iter().sum::<i32>();
            let peak = (0..len)
                .max_by(|&a, &b| w[a].total_cmp(&w[b]))
                .expect("at least one tap");
            fixed[peak] += error;
            start.push(first as u32);
            weights.extend(fixed.iter().map(|&v| v as i16));
        }
        let padded_len = len.next_multiple_of(8);
        let padded = weights
            .chunks_exact(len)
            .flat_map(|w| {
                w.iter()
                    .copied()
                    .chain(std::iter::repeat_n(0, padded_len - len))
            })
            .collect();
        Self {
            len,
            start,
            weights,
            padded_len,
            padded,
        }
    }
}

fn run<const STRIDE: usize, const CH: usize>(
    h: &Taps,
    v: &Taps,
    rows: &mut Vec<i16>,
    src: &[u8],
    (src_stride, sh): (usize, usize),
    (dw, dh): (usize, usize),
    dst: &mut [u8],
) {
    rows.resize(sh * dw * STRIDE, 0);
    let round = 1i32 << (WEIGHT_BITS - EXTRA_BITS - 1);
    for (y, out) in rows.chunks_exact_mut(dw * STRIDE).enumerate() {
        let row = &src[y * src_stride..];
        for x in 0..dw {
            let first = h.start[x] as usize;
            let weights = &h.weights[x * h.len..(x + 1) * h.len];
            let mut acc = [0i32; CH];
            for (t, &w) in weights.iter().enumerate() {
                let p = &row[(first + t) * STRIDE..];
                for c in 0..CH {
                    acc[c] += p[c] as i32 * w as i32;
                }
            }
            for c in 0..CH {
                out[x * STRIDE + c] = ((acc[c] + round) >> (WEIGHT_BITS - EXTRA_BITS)) as i16;
            }
        }
    }
    let shift = WEIGHT_BITS + EXTRA_BITS;
    let round = 1i32 << (shift - 1);
    for y in 0..dh {
        let first = v.start[y] as usize;
        let weights = &v.weights[y * v.len..(y + 1) * v.len];
        let out = &mut dst[y * dw * STRIDE..(y + 1) * dw * STRIDE];
        for x in 0..dw {
            let mut acc = [0i32; CH];
            for (t, &w) in weights.iter().enumerate() {
                let p = &rows[((first + t) * dw + x) * STRIDE..];
                for c in 0..CH {
                    acc[c] += p[c] as i32 * w as i32;
                }
            }
            for c in 0..CH {
                out[x * STRIDE + c] = ((acc[c] + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

pub struct Resampler {
    pub src: (usize, usize),
    pub dst: (usize, usize),
    pub horizontal: Taps,
    pub vertical: Taps,
    rows: Vec<i16>,
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_feature = "simd128")),
        allow(dead_code)
    )]
    line: Vec<u8>,
}

impl Resampler {
    pub fn new(src_width: usize, src_height: usize, dst_width: usize, dst_height: usize) -> Self {
        Self {
            src: (src_width, src_height),
            dst: (dst_width, dst_height),
            horizontal: Taps::new(src_width, dst_width),
            vertical: Taps::new(src_height, dst_height),
            rows: Vec::new(),
            line: Vec::new(),
        }
    }

    pub fn rgba(&mut self, src: &[u8], dst: &mut [u8]) {
        let (sw, sh) = self.src;
        let (dw, dh) = self.dst;
        assert_eq!(src.len(), sw * sh * 4);
        assert_eq!(dst.len(), dw * dh * 4);
        run::<4, 3>(
            &self.horizontal,
            &self.vertical,
            &mut self.rows,
            src,
            (sw * 4, sh),
            self.dst,
            dst,
        );
        for p in dst.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }

    pub fn plane(&mut self, src: &[u8], stride: usize, dst: &mut [u8]) {
        let (sw, sh) = self.src;
        let (dw, dh) = self.dst;
        assert!(stride >= sw && src.len() >= (sh - 1) * stride + sw);
        assert_eq!(dst.len(), dw * dh);
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            let _ = dh;
            wasm::plane(self, src, stride, dst);
        }
        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        run::<1, 1>(
            &self.horizontal,
            &self.vertical,
            &mut self.rows,
            src,
            (stride, sh),
            self.dst,
            dst,
        );
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use core::arch::wasm32::*;

    use super::{EXTRA_BITS, Resampler, Taps, WEIGHT_BITS};

    const H_SHIFT: u32 = WEIGHT_BITS - EXTRA_BITS;
    const V_SHIFT: u32 = WEIGHT_BITS + EXTRA_BITS;

    unsafe fn dot(line: *const u8, weights: *const i16, len: usize) -> v128 {
        let mut acc = i32x4_splat(0);
        for k in (0..len).step_by(8) {
            unsafe {
                let p = u16x8_load_extend_u8x8(line.add(k));
                let w = v128_load(weights.add(k) as *const v128);
                acc = i32x4_add(acc, i32x4_dot_i16x8(p, w));
            }
        }
        acc
    }

    fn horizontal(h: &Taps, line: &[u8], out: &mut [i16]) {
        let len = h.padded_len;
        let round = 1i32 << (H_SHIFT - 1);
        let tap = |x: usize| unsafe {
            dot(
                line.as_ptr().add(h.start[x] as usize),
                h.padded.as_ptr().add(x * len),
                len,
            )
        };
        let mut x = 0;
        while x + 4 <= out.len() {
            let [a, b, c, d] = [tap(x), tap(x + 1), tap(x + 2), tap(x + 3)];
            let ab = i32x4_add(
                i32x4_shuffle::<0, 4, 1, 5>(a, b),
                i32x4_shuffle::<2, 6, 3, 7>(a, b),
            );
            let cd = i32x4_add(
                i32x4_shuffle::<0, 4, 1, 5>(c, d),
                i32x4_shuffle::<2, 6, 3, 7>(c, d),
            );
            let sum = i32x4_add(
                i32x4_shuffle::<0, 1, 4, 5>(ab, cd),
                i32x4_shuffle::<2, 3, 6, 7>(ab, cd),
            );
            let r = i32x4_shr(i32x4_add(sum, i32x4_splat(round)), H_SHIFT);
            let n = i16x8_narrow_i32x4(r, r);
            unsafe { v128_store64_lane::<0>(n, out.as_mut_ptr().add(x) as *mut u64) };
            x += 4;
        }
        for (x, o) in out.iter_mut().enumerate().skip(x) {
            let v = tap(x);
            let sum = i32x4_extract_lane::<0>(v)
                + i32x4_extract_lane::<1>(v)
                + i32x4_extract_lane::<2>(v)
                + i32x4_extract_lane::<3>(v);
            *o = ((sum + round) >> H_SHIFT) as i16;
        }
    }

    fn vertical(v: &Taps, rows: &[i16], width: usize, y: usize, out: &mut [u8]) {
        let first = v.start[y] as usize;
        let weights = &v.weights[y * v.len..(y + 1) * v.len];
        let round = 1i32 << (V_SHIFT - 1);
        let mut x = 0;
        while x + 8 <= width {
            let (mut lo, mut hi) = (i32x4_splat(round), i32x4_splat(round));
            for (t, &w) in weights.iter().enumerate() {
                let r =
                    unsafe { v128_load(rows.as_ptr().add((first + t) * width + x) as *const v128) };
                let w = i16x8_splat(w);
                lo = i32x4_add(lo, i32x4_extmul_low_i16x8(r, w));
                hi = i32x4_add(hi, i32x4_extmul_high_i16x8(r, w));
            }
            let n = i16x8_narrow_i32x4(i32x4_shr(lo, V_SHIFT), i32x4_shr(hi, V_SHIFT));
            let b = u8x16_narrow_i16x8(n, n);
            unsafe { v128_store64_lane::<0>(b, out.as_mut_ptr().add(x) as *mut u64) };
            x += 8;
        }
        for x in x..width {
            let mut acc = round;
            for (t, &w) in weights.iter().enumerate() {
                acc += rows[(first + t) * width + x] as i32 * w as i32;
            }
            out[x] = (acc >> V_SHIFT).clamp(0, 255) as u8;
        }
    }

    pub fn plane(r: &mut Resampler, src: &[u8], stride: usize, dst: &mut [u8]) {
        let (sw, sh) = r.src;
        let (dw, dh) = r.dst;
        r.rows.resize(sh * dw, 0);
        r.line.resize(sw + r.horizontal.padded_len, 0);
        for (y, out) in r.rows.chunks_exact_mut(dw).enumerate() {
            r.line[..sw].copy_from_slice(&src[y * stride..y * stride + sw]);
            horizontal(&r.horizontal, &r.line, out);
        }
        for (y, out) in dst.chunks_exact_mut(dw).take(dh).enumerate() {
            vertical(&r.vertical, &r.rows, dw, y, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y);
                out.extend([v, v, v, 255]);
            }
        }
        out
    }

    #[test]
    fn weights_sum_to_one() {
        for (src, dst) in [(1920, 640), (1080, 360), (1920, 853), (100, 7), (5, 2)] {
            let taps = Taps::new(src, dst);
            for x in 0..dst {
                let sum: i32 = taps.weights[x * taps.len..(x + 1) * taps.len]
                    .iter()
                    .map(|&w| w as i32)
                    .sum();
                assert_eq!(sum, 1 << WEIGHT_BITS);
                assert!(taps.start[x] as usize + taps.len <= src);
            }
        }
    }

    #[test]
    fn flat_stays_flat() {
        let mut r = Resampler::new(90, 60, 30, 20);
        let mut out = vec![0; 30 * 20 * 4];
        r.rgba(&image(90, 60, |_, _| 77), &mut out);
        assert!(out.chunks(4).all(|p| p == [77, 77, 77, 255]));
    }

    #[test]
    fn thin_lines_are_averaged_not_dropped() {
        let (w, h) = (1920, 1080);
        let src = image(w, h, |x, y| if (x + y) % 3 == 0 { 255 } else { 0 });
        let mut r = Resampler::new(w, h, 640, 360);
        let mut out = vec![0; 640 * 360 * 4];
        r.rgba(&src, &mut out);
        let mean = out.chunks(4).map(|p| p[0] as f64).sum::<f64>() / (640.0 * 360.0);
        assert!((mean - 85.0).abs() < 2.0, "{mean}");
    }
}
