pub fn round(x: f32) -> f32 {
    let t = x.trunc();
    if (x - t).abs() >= 0.5 {
        t + 1f32.copysign(x)
    } else {
        t
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub mod wasm {
    use core::arch::wasm32::*;

    pub fn round(x: v128) -> v128 {
        let t = f32x4_trunc(x);
        let away = f32x4_ge(f32x4_abs(f32x4_sub(x, t)), f32x4_splat(0.5));
        let step = v128_or(v128_and(x, f32x4_splat(-0.0)), f32x4_splat(1.0));
        f32x4_add(t, v128_and(step, away))
    }

    pub fn round_i16x8(lo: v128, hi: v128) -> v128 {
        i16x8_narrow_i32x4(
            i32x4_trunc_sat_f32x4(round(lo)),
            i32x4_trunc_sat_f32x4(round(hi)),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_std_round() {
        let mut values = vec![
            0.0f32,
            -0.0,
            0.5,
            -0.5,
            1.5,
            -1.5,
            2.5,
            0.499_999_97,
            -0.499_999_97,
            8_388_607.5,
            1e9,
            -1e9,
        ];
        let mut state = 1u32;
        for _ in 0..100_000 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            values.push(f32::from_bits(state) % 1e6);
            values.push((state >> 8) as f32 / 64.0 - 1e5);
        }
        for v in values.into_iter().filter(|v| v.is_finite()) {
            assert_eq!(super::round(v).to_bits(), v.round().to_bits(), "{v}");
        }
    }
}
