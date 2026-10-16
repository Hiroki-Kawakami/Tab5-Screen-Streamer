//! Demuxing a `File`, for the player worker.

use js_sys::{Array, Function, Uint8Array};
use streamer_core::bail;
use streamer_core::container::demux::{self, Codec, Demuxer, TrackKind};
use streamer_core::container::io::Source;
use streamer_core::error::{self, Context};
use wasm_bindgen::prelude::*;

use crate::object;

/// Reads with `read(offset, length)`, which returns a `Uint8Array`.
struct JsSource {
    size: u64,
    read: Function,
}

impl Source for JsSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> error::Result<()> {
        let value = self
            .read
            .call2(
                &JsValue::NULL,
                &JsValue::from_f64(offset as f64),
                &JsValue::from_f64(buf.len() as f64),
            )
            .map_err(|e| format!("{e:?}"))
            .context("reading the file failed")?;
        let array = Uint8Array::new(&value);
        if array.length() as usize != buf.len() {
            bail!("short read from the file");
        }
        array.copy_to(buf);
        Ok(())
    }
}

#[wasm_bindgen]
pub struct Packet {
    track: u32,
    data: Vec<u8>,
    pts: i64,
    duration: i64,
    key: bool,
}

#[wasm_bindgen]
impl Packet {
    #[wasm_bindgen(getter)]
    pub fn track(&self) -> u32 {
        self.track
    }

    #[wasm_bindgen(getter)]
    pub fn data(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// µs.
    #[wasm_bindgen(getter)]
    pub fn pts(&self) -> f64 {
        self.pts as f64
    }

    /// µs.
    #[wasm_bindgen(getter)]
    pub fn duration(&self) -> f64 {
        self.duration as f64
    }

    #[wasm_bindgen(getter)]
    pub fn key(&self) -> bool {
        self.key
    }
}

#[wasm_bindgen]
pub struct Media {
    demuxer: Demuxer<JsSource>,
}

#[wasm_bindgen]
impl Media {
    #[wasm_bindgen(constructor)]
    pub fn new(size: f64, read: Function) -> Result<Media, JsError> {
        let source = JsSource {
            size: size as u64,
            read,
        };
        let demuxer = demux::open(source).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Media { demuxer })
    }

    /// Seconds.
    pub fn duration(&self) -> Option<f64> {
        self.demuxer.info().duration
    }

    /// Every track: `kind`, `codec`, `codecString` (WebCodecs), and the
    /// video size, rotation and colour or the audio format.
    ///
    /// `matrix` is 1 for BT.709, otherwise BT.601; `fullRange` may be
    /// undefined. Untagged H.264 is BT.601 limited range, as in ffmpeg.
    pub fn tracks(&self) -> Array {
        self.demuxer
            .info()
            .tracks
            .iter()
            .map(|t| {
                let codec_string = t.codec_string().map_or(JsValue::NULL, |s| s.into());
                let mut entries = vec![
                    ("index", t.index.into()),
                    ("codec", t.codec.name().into()),
                    ("codecString", codec_string),
                ];
                match &t.kind {
                    TrackKind::Video(v) => {
                        let sar = v
                            .sample_aspect
                            .filter(|&(h, v)| h > 0 && v > 0)
                            .map_or(1.0, |(h, v)| h as f64 / v as f64);
                        let color = v.color.or_else(|| {
                            (t.codec == Codec::H264).then_some(demux::ColorInfo {
                                primaries: 2,
                                transfer: 2,
                                matrix: 6,
                                full_range: Some(false),
                            })
                        });
                        let matrix = color.and_then(|c| match c.matrix {
                            1 => Some(1u8),
                            5 | 6 => Some(6),
                            _ => None,
                        });
                        let full_range = color.and_then(|c| c.full_range);
                        entries.extend([
                            ("kind", "video".into()),
                            ("width", v.width.into()),
                            ("height", v.height.into()),
                            ("displayWidth", (v.width as f64 * sar).round().into()),
                            ("displayHeight", v.height.into()),
                            ("rotation", v.rotation.into()),
                            ("matrix", matrix.map_or(JsValue::UNDEFINED, JsValue::from)),
                            ("fullRange", full_range.map_or(JsValue::UNDEFINED, JsValue::from)),
                        ]);
                    }
                    TrackKind::Audio(a) => entries.extend([
                        ("kind", "audio".into()),
                        ("channels", a.channels.into()),
                        ("sampleRate", a.sample_rate.into()),
                    ]),
                    TrackKind::Other => entries.push(("kind", "other".into())),
                }
                object(&entries)
            })
            .collect()
    }

    pub fn extradata(&self, track: u32) -> Vec<u8> {
        self.demuxer
            .info()
            .tracks
            .get(track as usize)
            .map(|t| t.extradata.clone())
            .unwrap_or_default()
    }

    pub fn select(&mut self, tracks: Vec<u32>) {
        self.demuxer.select(&tracks);
    }

    #[wasm_bindgen(js_name = nextPacket)]
    pub fn next_packet(&mut self) -> Result<Option<Packet>, JsError> {
        let packet = self
            .demuxer
            .next_packet()
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(packet.map(|p| Packet {
            track: p.track,
            data: p.data,
            pts: p.pts,
            duration: p.duration,
            key: p.key,
        }))
    }

    /// Goes back to a key frame at or before `us`; the packets before it
    /// are the caller's to drop after decoding.
    pub fn seek(&mut self, us: f64) -> Result<(), JsError> {
        self.demuxer
            .seek(us as i64)
            .map_err(|e| JsError::new(&e.to_string()))
    }
}
