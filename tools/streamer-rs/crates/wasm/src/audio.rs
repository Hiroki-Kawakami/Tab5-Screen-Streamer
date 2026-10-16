//! Decoded audio to the PCM the device takes.

use streamer_core::pcm::{self, Resampler};
use wasm_bindgen::prelude::*;

/// f32 planar at any rate and channel count to 16-bit interleaved PCM at
/// `output_rate` with at most two channels.
#[wasm_bindgen]
pub struct AudioConverter {
    input_channels: usize,
    output_channels: usize,
    resampler: Option<Resampler>,
}

fn s16(planes: &[Vec<f32>]) -> Vec<u8> {
    let frames = planes.first().map_or(0, Vec::len);
    let mut out = Vec::with_capacity(frames * planes.len() * 2);
    for i in 0..frames {
        for plane in planes {
            let v = (plane[i] * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

#[wasm_bindgen]
impl AudioConverter {
    #[wasm_bindgen(constructor)]
    pub fn new(input_rate: u32, input_channels: usize, output_rate: u32) -> AudioConverter {
        let output_channels = input_channels.clamp(1, 2);
        AudioConverter {
            input_channels,
            output_channels,
            resampler: (input_rate != output_rate)
                .then(|| Resampler::new(input_rate, output_rate, output_channels)),
        }
    }

    #[wasm_bindgen(getter, js_name = outputChannels)]
    pub fn output_channels(&self) -> usize {
        self.output_channels
    }

    /// `samples` holds every input channel's samples, one after another.
    pub fn process(&mut self, samples: &[f32]) -> Vec<u8> {
        let frames = samples.len() / self.input_channels.max(1);
        if frames == 0 {
            return Vec::new();
        }
        let planes: Vec<Vec<f32>> = samples.chunks_exact(frames).map(<[f32]>::to_vec).collect();
        let mixed = pcm::downmix(&planes, self.output_channels);
        match &mut self.resampler {
            Some(resampler) => s16(&resampler.process(&mixed)),
            None => s16(&mixed),
        }
    }
}
