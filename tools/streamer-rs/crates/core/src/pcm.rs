use std::f64::consts::PI;

const HALF_TAPS: usize = 16;
const PHASES: usize = 256;
const MINUS_3DB: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub fn downmix(input: &[Vec<f32>], channels: usize) -> Vec<Vec<f32>> {
    let frames = input.first().map_or(0, Vec::len);
    let mix = |weights: &[(usize, f32)]| -> Vec<f32> {
        let total: f32 = weights.iter().map(|(_, w)| w).sum();
        (0..frames)
            .map(|i| weights.iter().map(|&(c, w)| input[c][i] * w).sum::<f32>() / total)
            .collect()
    };
    match (input.len(), channels) {
        (n, m) if n == m => input.to_vec(),
        (1, _) => vec![input[0].clone(); channels],
        (6, 2) => vec![
            mix(&[(0, 1.0), (2, MINUS_3DB), (4, MINUS_3DB)]),
            mix(&[(1, 1.0), (2, MINUS_3DB), (5, MINUS_3DB)]),
        ],
        (6, 1) => vec![mix(&[(0, 1.0), (1, 1.0), (2, 1.0), (4, 1.0), (5, 1.0)])],
        (_, 1) => vec![mix(&(0..input.len()).map(|c| (c, 1.0)).collect::<Vec<_>>())],
        (_, _) => input[..channels].to_vec(),
    }
}

pub struct Resampler {
    step: f64,
    half: usize,
    table: Vec<f32>,
    history: Vec<Vec<f32>>,
    pos: f64,
}

fn blackman(t: f64) -> f64 {
    0.42 + 0.5 * (PI * t).cos() + 0.08 * (2.0 * PI * t).cos()
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Self {
        let step = input_rate as f64 / output_rate as f64;
        let cutoff = (1.0 / step).min(1.0) * 0.97;
        let half = (HALF_TAPS as f64 / cutoff).ceil() as usize;
        let width = 2 * half;
        let mut table = vec![0f32; (PHASES + 1) * width];
        for phase in 0..=PHASES {
            let frac = phase as f64 / PHASES as f64;
            for k in 0..width {
                let x = k as f64 - (half as f64 - 1.0) - frac;
                let t = x / half as f64;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    (PI * cutoff * x).sin() / (PI * cutoff * x)
                };
                table[phase * width + k] = (cutoff * sinc * blackman(t.clamp(-1.0, 1.0))) as f32;
            }
        }
        Self {
            step,
            half,
            table,
            history: vec![vec![0.0; half]; channels],
            pos: half as f64,
        }
    }

    pub fn process(&mut self, input: &[Vec<f32>]) -> Vec<Vec<f32>> {
        for (history, samples) in self.history.iter_mut().zip(input) {
            history.extend_from_slice(samples);
        }
        let width = 2 * self.half;
        let available = self.history.first().map_or(0, Vec::len);
        let mut out = vec![Vec::new(); self.history.len()];
        while (self.pos.floor() as usize) + self.half < available {
            let base = self.pos.floor() as usize;
            let frac = self.pos - base as f64;
            let phase = frac * PHASES as f64;
            let p = phase.floor() as usize;
            let mix = (phase - p as f64) as f32;
            let (lo, hi) = (
                &self.table[p * width..(p + 1) * width],
                &self.table[(p + 1) * width..(p + 2) * width],
            );
            let first = base + 1 - self.half;
            for (history, out) in self.history.iter().zip(out.iter_mut()) {
                let window = &history[first..first + width];
                let mut acc = 0f32;
                for k in 0..width {
                    acc += window[k] * (lo[k] + (hi[k] - lo[k]) * mix);
                }
                out.push(acc);
            }
            self.pos += self.step;
        }
        let keep_from = (self.pos.floor() as usize).saturating_sub(self.half);
        for history in &mut self.history {
            history.drain(..keep_from.min(history.len()));
        }
        self.pos -= keep_from as f64;
        out
    }

    pub fn flush(&mut self) -> Vec<Vec<f32>> {
        let padding = vec![vec![0.0; self.half + 1]; self.history.len()];
        self.process(&padding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32 * 0.5)
            .collect()
    }

    fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
        let mut r = Resampler::new(from, to, 1);
        let mut out = Vec::new();
        for chunk in input.chunks(1000) {
            out.extend(r.process(&[chunk.to_vec()]).remove(0));
        }
        out.extend(r.flush().remove(0));
        out
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn upsampling_keeps_a_tone() {
        let out = resample(&sine(1000.0, 8000, 8000), 8000, 16000);
        assert!((out.len() as i64 - 16000).abs() <= 2, "{}", out.len());
        let expected = sine(1000.0, 16000, 16000);
        let error: Vec<f32> = out[100..15900]
            .iter()
            .zip(&expected[100..15900])
            .map(|(a, b)| a - b)
            .collect();
        assert!(rms(&error) < 1e-3, "{}", rms(&error));
    }

    #[test]
    fn downsampling_removes_what_no_longer_fits() {
        let out = resample(&sine(10000.0, 48000, 48000), 48000, 16000);
        assert!((out.len() as i64 - 16000).abs() <= 2);
        assert!(rms(&out[100..15900]) < 0.01, "{}", rms(&out[100..15900]));
        let out = resample(&sine(3000.0, 44100, 44100), 44100, 22050);
        assert!((rms(&out[100..21950]) - 0.3535).abs() < 0.01);
    }

    #[test]
    fn downmix_layouts() {
        let ch = |v: f32| vec![v; 4];
        let surround: Vec<Vec<f32>> = [1.0, 0.0, 1.0, 5.0, 0.0, 0.0].map(ch).to_vec();
        let stereo = downmix(&surround, 2);
        assert!((stereo[0][0] - (1.0 + MINUS_3DB) / (1.0 + 2.0 * MINUS_3DB)).abs() < 1e-6);
        assert!((stereo[1][0] - MINUS_3DB / (1.0 + 2.0 * MINUS_3DB)).abs() < 1e-6);
        assert_eq!(downmix(&[ch(0.5)], 2), vec![ch(0.5), ch(0.5)]);
        assert_eq!(downmix(&[ch(1.0), ch(0.0)], 1), vec![ch(0.5)]);
    }
}
