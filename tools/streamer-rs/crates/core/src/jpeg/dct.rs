pub type Block = [[f32; 8]; 8];

// Lane-wise loops rather than per-row array helpers: LLVM vectorizes these,
// and did not vectorize the helpers (10x slower).
fn butterfly(d: &Block) -> Block {
    let mut o = [[0f32; 8]; 8];
    for i in 0..8 {
        let (t0, t7) = (d[0][i] + d[7][i], d[0][i] - d[7][i]);
        let (t1, t6) = (d[1][i] + d[6][i], d[1][i] - d[6][i]);
        let (t2, t5) = (d[2][i] + d[5][i], d[2][i] - d[5][i]);
        let (t3, t4) = (d[3][i] + d[4][i], d[3][i] - d[4][i]);
        let (e0, e3) = (t0 + t3, t0 - t3);
        let (e1, e2) = (t1 + t2, t1 - t2);
        let z1 = (e2 + e3) * 0.707_106_77;
        let (o0, o1, o2) = (t4 + t5, t5 + t6, t6 + t7);
        let z5 = (o0 - o2) * 0.382_683_43;
        let z2 = o0 * 0.541_196_1 + z5;
        let z4 = o2 * 1.306_563 + z5;
        let z3 = o1 * 0.707_106_77;
        let (z11, z13) = (t7 + z3, t7 - z3);
        o[0][i] = e0 + e1;
        o[1][i] = z11 + z4;
        o[2][i] = e3 + z1;
        o[3][i] = z13 - z2;
        o[4][i] = e0 - e1;
        o[5][i] = z13 + z2;
        o[6][i] = e3 - z1;
        o[7][i] = z11 - z4;
    }
    o
}

fn transpose(m: &Block) -> Block {
    let mut o = [[0f32; 8]; 8];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m[j][i];
        }
    }
    o
}

/// Output [u][v] of `forward` times SCALE[v] * SCALE[u] is the DCT
/// coefficient of vertical frequency v and horizontal frequency u.
pub const SCALE: [f32; 8] = [
    0.353_553_38,
    0.254_897_8,
    0.270_598_05,
    0.300_672_44,
    0.353_553_38,
    0.449_988_1,
    0.653_281_5,
    1.281_457_7,
];

pub fn forward(block: &Block) -> Block {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    return wasm::forward(block);
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    scalar(block)
}

#[cfg_attr(
    all(target_arch = "wasm32", target_feature = "simd128"),
    allow(dead_code)
)]
fn scalar(block: &Block) -> Block {
    butterfly(&transpose(&butterfly(block)))
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use core::arch::wasm32::*;

    type Row = [v128; 2];

    fn add(a: Row, b: Row) -> Row {
        [f32x4_add(a[0], b[0]), f32x4_add(a[1], b[1])]
    }

    fn sub(a: Row, b: Row) -> Row {
        [f32x4_sub(a[0], b[0]), f32x4_sub(a[1], b[1])]
    }

    fn mul(a: Row, k: f32) -> Row {
        let k = f32x4_splat(k);
        [f32x4_mul(a[0], k), f32x4_mul(a[1], k)]
    }

    fn butterfly(d: [Row; 8]) -> [Row; 8] {
        let (t0, t7) = (add(d[0], d[7]), sub(d[0], d[7]));
        let (t1, t6) = (add(d[1], d[6]), sub(d[1], d[6]));
        let (t2, t5) = (add(d[2], d[5]), sub(d[2], d[5]));
        let (t3, t4) = (add(d[3], d[4]), sub(d[3], d[4]));
        let (e0, e3) = (add(t0, t3), sub(t0, t3));
        let (e1, e2) = (add(t1, t2), sub(t1, t2));
        let z1 = mul(add(e2, e3), 0.707_106_77);
        let (o0, o1, o2) = (add(t4, t5), add(t5, t6), add(t6, t7));
        let z5 = mul(sub(o0, o2), 0.382_683_43);
        let z2 = add(mul(o0, 0.541_196_1), z5);
        let z4 = add(mul(o2, 1.306_563), z5);
        let z3 = mul(o1, 0.707_106_77);
        let (z11, z13) = (add(t7, z3), sub(t7, z3));
        [
            add(e0, e1),
            add(z11, z4),
            add(e3, z1),
            sub(z13, z2),
            sub(e0, e1),
            add(z13, z2),
            sub(e3, z1),
            sub(z11, z4),
        ]
    }

    fn transpose4(a: v128, b: v128, c: v128, d: v128) -> [v128; 4] {
        let ab0 = i32x4_shuffle::<0, 4, 1, 5>(a, b);
        let ab1 = i32x4_shuffle::<2, 6, 3, 7>(a, b);
        let cd0 = i32x4_shuffle::<0, 4, 1, 5>(c, d);
        let cd1 = i32x4_shuffle::<2, 6, 3, 7>(c, d);
        [
            i64x2_shuffle::<0, 2>(ab0, cd0),
            i64x2_shuffle::<1, 3>(ab0, cd0),
            i64x2_shuffle::<0, 2>(ab1, cd1),
            i64x2_shuffle::<1, 3>(ab1, cd1),
        ]
    }

    fn transpose(m: [Row; 8]) -> [Row; 8] {
        let tl = transpose4(m[0][0], m[1][0], m[2][0], m[3][0]);
        let tr = transpose4(m[0][1], m[1][1], m[2][1], m[3][1]);
        let bl = transpose4(m[4][0], m[5][0], m[6][0], m[7][0]);
        let br = transpose4(m[4][1], m[5][1], m[6][1], m[7][1]);
        std::array::from_fn(|i| {
            if i < 4 {
                [tl[i], bl[i]]
            } else {
                [tr[i - 4], br[i - 4]]
            }
        })
    }

    pub fn forward(block: &super::Block) -> super::Block {
        let p = block.as_ptr() as *const v128;
        let rows: [Row; 8] = std::array::from_fn(|y| unsafe {
            [v128_load(p.add(2 * y)), v128_load(p.add(2 * y + 1))]
        });
        let out = butterfly(transpose(butterfly(rows)));
        let mut result = [[0f32; 8]; 8];
        let q = result.as_mut_ptr() as *mut v128;
        for (v, row) in out.iter().enumerate() {
            for h in 0..2 {
                unsafe { v128_store(q.add(2 * v + h), row[h]) };
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(block: &[f32; 64]) -> [f32; 64] {
        let rows = std::array::from_fn(|y| std::array::from_fn(|x| block[y * 8 + x]));
        let out = forward(&rows);
        std::array::from_fn(|i| out[i % 8][i / 8] * SCALE[i / 8] * SCALE[i % 8])
    }

    #[test]
    fn flat_block_has_only_dc() {
        let out = scaled(&[10.0; 64]);
        assert!((out[0] - 80.0).abs() < 1e-3);
        assert!(out[1..].iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn matches_the_jpeg_definition() {
        let block: [f32; 64] = std::array::from_fn(|i| ((i * 37) % 255) as f32 - 128.0);
        let out = scaled(&block);
        for v in 0..8 {
            for u in 0..8 {
                let cu = if u == 0 { 1.0 / 2f64.sqrt() } else { 1.0 };
                let cv = if v == 0 { 1.0 / 2f64.sqrt() } else { 1.0 };
                let mut sum = 0.0f64;
                for y in 0..8 {
                    for x in 0..8 {
                        sum += block[y * 8 + x] as f64
                            * (((2 * x + 1) * u) as f64 * std::f64::consts::PI / 16.0).cos()
                            * (((2 * y + 1) * v) as f64 * std::f64::consts::PI / 16.0).cos();
                    }
                }
                let expected = 0.25 * cu * cv * sum;
                assert!((out[v * 8 + u] as f64 - expected).abs() < 1e-2);
            }
        }
    }
}
