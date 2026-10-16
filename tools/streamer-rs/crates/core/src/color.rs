fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

fn clamp(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
}

impl Matrix {
    fn weights(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
        }
    }
}

pub fn rgba_to_yuv420_limited(
    rgba: &[u8],
    width: usize,
    height: usize,
    matrix: Matrix,
    out: &mut [u8],
) {
    let (kr, kb) = matrix.weights();
    let kg = 1.0 - kr - kb;
    let luma = |r: f32, g: f32, b: f32| kr * r + kg * g + kb * b;
    let (cw, ch) = (width / 2, height / 2);
    let (y_plane, chroma) = out.split_at_mut(width * height);
    let (cb_plane, cr_plane) = chroma.split_at_mut(cw * ch);
    let pixel = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        (rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32)
    };
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = pixel(x, y);
            y_plane[y * width + x] = clamp(16.0 + luma(r, g, b) * (219.0 / 255.0));
        }
    }
    let scale = 224.0 / 255.0;
    for y in 0..ch {
        for x in 0..cw {
            let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (pr, pg, pb) = pixel(2 * x + dx, 2 * y + dy);
                r += pr;
                g += pg;
                b += pb;
            }
            let (r, g, b) = (r / 4.0, g / 4.0, b / 4.0);
            let l = luma(r, g, b);
            cb_plane[y * cw + x] = clamp(128.0 + (b - l) / (2.0 * (1.0 - kb)) * scale);
            cr_plane[y * cw + x] = clamp(128.0 + (r - l) / (2.0 * (1.0 - kr)) * scale);
        }
    }
}

pub fn rgba_to_yuv420_bt601_full(rgba: &[u8], width: usize, height: usize, out: &mut [u8]) {
    let (cw, ch) = (width / 2, height / 2);
    let (y_plane, chroma) = out.split_at_mut(width * height);
    let (cb_plane, cr_plane) = chroma.split_at_mut(cw * ch);
    let pixel = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        (rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32)
    };
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = pixel(x, y);
            y_plane[y * width + x] = clamp(luma(r, g, b));
        }
    }
    for y in 0..ch {
        for x in 0..cw {
            let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (pr, pg, pb) = pixel(2 * x + dx, 2 * y + dy);
                r += pr;
                g += pg;
                b += pb;
            }
            let (r, g, b) = (r / 4.0, g / 4.0, b / 4.0);
            let l = luma(r, g, b);
            cb_plane[y * cw + x] = clamp(128.0 + (b - l) * 0.564);
            cr_plane[y * cw + x] = clamp(128.0 + (r - l) * 0.713);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primaries_and_greys() {
        let rgba: Vec<u8> = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [128, 128, 128, 255],
        ]
        .concat();
        let mut out = vec![0; 6];
        rgba_to_yuv420_bt601_full(&rgba, 2, 2, &mut out);
        assert_eq!(&out[..4], &[76, 150, 29, 128]);
        let grey = [200u8, 200, 200, 255].repeat(4);
        rgba_to_yuv420_bt601_full(&grey, 2, 2, &mut out);
        assert_eq!(out, [200, 200, 200, 200, 128, 128]);
    }

    #[test]
    fn limited_range() {
        let rgba: Vec<u8> = [
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
        ]
        .concat();
        let mut out = vec![0; 6];
        rgba_to_yuv420_limited(&rgba, 2, 2, Matrix::Bt709, &mut out);
        assert_eq!(&out[..4], &[235, 16, 63, 32]);
        let red = [255u8, 0, 0, 255].repeat(4);
        rgba_to_yuv420_limited(&red, 2, 2, Matrix::Bt709, &mut out);
        assert_eq!(&out[4..], &[102, 240]);
        rgba_to_yuv420_limited(&red, 2, 2, Matrix::Bt601, &mut out);
        assert_eq!(out, [81, 81, 81, 81, 90, 240]);
    }
}
