//! Frames to the device's video endpoint, for the video worker. Times are
//! milliseconds from `performance.now()`.

use js_sys::{Array, Uint8Array};
use streamer_core::color::Matrix;
use streamer_core::draw::Encoded;
use streamer_core::jpeg::{Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::present::{self, LayoutKey, Presenter, Quality};
use streamer_core::protocol::{self, Encoding, Event, Info};
use streamer_core::resample::Resampler;
use streamer_core::yuv::{self, Converter, Geometry, Output, SourceColor};
use wasm_bindgen::prelude::*;

enum Kind {
    Yuv {
        layout: yuv::Layout,
        planes: Vec<(usize, usize)>,
        color: SourceColor,
    },
    Bgrx,
}

/// The latest frame as it came, kept to draw again after a relayout.
struct Frame {
    width: usize,
    height: usize,
    data: Vec<u8>,
    kind: Kind,
    /// The size it is shown at, before `turn`.
    display: (u32, u32),
    /// Counter-clockwise quarter turns it needs on top of the device's.
    turn: u32,
}

#[wasm_bindgen]
#[derive(Default)]
pub struct Video {
    info: Option<Info>,
    presenter: Option<Presenter>,
    seq: u32,
    frame: Option<Frame>,
    /// `frame` at the layout's frame size: I420 or BGRX.
    picture: Option<Vec<u8>>,
    built: Option<(LayoutKey, (u32, u32))>,
    converter: Option<((u32, u32), Converter)>,
    resampler: Option<Resampler>,
    quality: u8,
}

fn us(ms: f64) -> u64 {
    (ms * 1000.0).max(0.0) as u64
}

fn matrix(code: u8) -> Matrix {
    match code {
        1 => Matrix::Bt709,
        _ => Matrix::Bt601,
    }
}

#[wasm_bindgen]
impl Video {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Video {
        Video::default()
    }

    /// A session opened with the INFO carrying `seq` in `data`, a control IN
    /// transfer.
    pub fn connected(&mut self, data: &[u8], seq: u32) -> Result<(), JsError> {
        let info = protocol::parse_events(data)
            .into_iter()
            .find_map(|event| match event {
                Event::Info { seq: s, info } if s == seq => Some(info),
                _ => None,
            })
            .ok_or_else(|| JsError::new("no INFO in the transfer"))?;
        self.seq = 0;
        if let Some(presenter) = &mut self.presenter {
            presenter.connected(info.clone());
        }
        self.info = Some(info);
        self.relayout()
    }

    pub fn lost(&mut self) {
        self.info = None;
        self.presenter = None;
        self.built = None;
    }

    /// Applies the events of a control IN transfer.
    pub fn events(&mut self, data: &[u8]) -> Result<(), JsError> {
        let Some(presenter) = &mut self.presenter else {
            return Ok(());
        };
        let mut relayout = false;
        for event in protocol::parse_events(data) {
            relayout |= presenter.apply(&event) == Some(true);
        }
        match relayout {
            true => self.relayout(),
            false => Ok(()),
        }
    }

    /// Takes a frame in a planar YUV `format` of `VideoFrame`; `planes` are
    /// its offsets and strides.
    #[wasm_bindgen(js_name = frameYuv)]
    #[allow(clippy::too_many_arguments)]
    pub fn frame_yuv(
        &mut self,
        format: &str,
        width: usize,
        height: usize,
        data: Vec<u8>,
        planes: &[u32],
        matrix_code: u8,
        full_range: bool,
        display_width: u32,
        display_height: u32,
        turn: u32,
    ) -> Result<(), JsError> {
        let layout = yuv::Layout::parse(format)
            .ok_or_else(|| JsError::new(&format!("unsupported frame format {format}")))?;
        let planes = planes
            .chunks_exact(2)
            .map(|p| (p[0] as usize, p[1] as usize))
            .collect();
        let color = SourceColor {
            matrix: matrix(matrix_code),
            full_range,
        };
        let kind = Kind::Yuv {
            layout,
            planes,
            color,
        };
        self.take(Frame {
            width,
            height,
            data,
            kind,
            display: (display_width, display_height),
            turn,
        })
    }

    /// Takes a frame of B, G, R and one ignored byte per pixel.
    #[wasm_bindgen(js_name = frameBgrx)]
    pub fn frame_bgrx(
        &mut self,
        width: usize,
        height: usize,
        data: Vec<u8>,
        display_width: u32,
        display_height: u32,
        turn: u32,
    ) -> Result<(), JsError> {
        self.take(Frame {
            width,
            height,
            data,
            kind: Kind::Bgrx,
            display: (display_width, display_height),
            turn,
        })
    }

    /// Whether a frame can be sent now: the session is open, within
    /// max_fps, and not waiting for SWAP_DONE.
    pub fn ready(&self, now: f64) -> bool {
        self.info.is_some()
            && self
                .presenter
                .as_ref()
                .is_none_or(|p| !p.swaps_full() && p.due(us(now)))
    }

    /// Forgets the latest frame, for a new source.
    pub fn reset(&mut self) {
        self.frame = None;
        self.picture = None;
    }

    /// Milliseconds until a frame stays within max_fps.
    #[wasm_bindgen(js_name = untilDue)]
    pub fn until_due(&self, now: f64) -> f64 {
        self.presenter
            .as_ref()
            .map_or(0.0, |p| p.until_due(us(now)) as f64 / 1000.0)
    }

    /// The device needs the whole content drawn again.
    pub fn redraw(&self) -> bool {
        self.frame.is_some() && self.presenter.as_ref().is_some_and(|p| p.redraw)
    }

    pub fn describe(&self) -> Option<String> {
        self.presenter.as_ref().map(Presenter::describe)
    }

    /// The quality of the last frame encoded.
    #[wasm_bindgen(getter)]
    pub fn quality(&self) -> u8 {
        self.quality
    }

    /// The transfers drawing the latest frame, as `Uint8Array`s.
    pub fn encode(&mut self, now: f64) -> Result<Array, JsError> {
        let now = us(now);
        self.convert()?;
        let (Some(presenter), Some(frame), Some(picture)) =
            (&mut self.presenter, &self.frame, &self.picture)
        else {
            return Ok(Array::new());
        };
        let (w, h) = presenter.layout.frame();
        let (w, h) = (w as usize, h as usize);
        let pixels = match frame.kind {
            Kind::Yuv { .. } => {
                let (y, chroma) = picture.split_at(w * h);
                let (cb, cr) = chroma.split_at(w * h / 4);
                Pixels::I420 {
                    y,
                    y_stride: w,
                    cb,
                    cr,
                    c_stride: w / 2,
                }
            }
            Kind::Bgrx => Pixels::Bgrx {
                data: picture,
                stride: w * 4,
            },
        };
        let image = Image {
            width: w,
            height: h,
            pixels,
        };
        presenter.start_frame(now);
        self.quality = presenter.quality(now);
        let bands = present::jpeg_bands(&presenter.layout, &presenter.info)
            .into_iter()
            .map(|rows| {
                present::encode_band(&image, &presenter.layout, &presenter.info, self.quality, rows)
            })
            .collect();
        let seq = &mut self.seq;
        let mut next = || {
            *seq += 1;
            *seq
        };
        let transfers = presenter
            .frame(Encoded::Jpeg(bands), now, &mut next)
            .map_err(|e| JsError::new(&e))?;
        Ok(transfers
            .iter()
            .map(|t| JsValue::from(Uint8Array::from(&t[..])))
            .collect())
    }
}

impl Video {
    fn take(&mut self, frame: Frame) -> Result<(), JsError> {
        self.frame = Some(frame);
        self.picture = None;
        self.relayout()
    }

    /// Lays the latest frame out for the device, once both are known.
    fn relayout(&mut self) -> Result<(), JsError> {
        let (Some(info), Some(frame)) = (&self.info, &self.frame) else {
            return Ok(());
        };
        let info = self
            .presenter
            .as_ref()
            .map_or(info, |presenter| &presenter.info)
            .clone();
        let turn = frame.turn;
        let display = frame.display;
        let rotation = (info.orientation as u32 + turn) % 4;
        let key = (present::layout_key(&info, rotation), display);
        if self.built == Some(key) {
            return Ok(());
        }
        let panel = (info.panel_width as u32, info.panel_height as u32);
        let align = info.direct_align.max(1) as u32;
        let layout = Layout::direct(display, rotation, panel, align, true)
            .map_err(|e| JsError::new(&e))?;
        match &mut self.presenter {
            Some(presenter) => {
                presenter.layout = layout;
                presenter.clear();
            }
            None => {
                self.presenter = Some(Presenter::new(
                    info,
                    layout,
                    Encoding::Jpeg,
                    false,
                    None,
                    Quality::DEFAULT,
                ));
            }
        }
        self.built = Some(key);
        self.picture = None;
        Ok(())
    }

    /// Scales the latest frame to the layout's frame size.
    fn convert(&mut self) -> Result<(), JsError> {
        let (Some(presenter), Some(frame), None) = (&self.presenter, &self.frame, &self.picture)
        else {
            return Ok(());
        };
        let (w, h) = presenter.layout.frame();
        let picture = match &frame.kind {
            Kind::Yuv {
                layout,
                planes,
                color,
            } => {
                if self.converter.as_ref().is_none_or(|(size, _)| *size != (w, h)) {
                    let size = (w as usize, h as usize);
                    let geometry = Geometry {
                        scaled: size,
                        crop: size,
                        source_rotation: 0,
                        output_rotation: 0,
                        stored: size,
                        output: Output::Bt601Full,
                    };
                    self.converter = Some(((w, h), Converter::new(geometry)));
                }
                let source = yuv::Source {
                    layout: *layout,
                    width: frame.width,
                    height: frame.height,
                    data: &frame.data,
                    planes,
                };
                let (_, converter) = self.converter.as_mut().unwrap();
                converter
                    .convert(&source, *color)
                    .map_err(|e| JsError::new(&e.to_string()))?
            }
            Kind::Bgrx => {
                let (sw, sh) = (frame.width, frame.height);
                let (dw, dh) = (w as usize, h as usize);
                if (sw, sh) == (dw, dh) {
                    frame.data.clone()
                } else {
                    let fresh = |r: &Resampler| r.src != (sw, sh) || r.dst != (dw, dh);
                    if self.resampler.as_ref().is_none_or(fresh) {
                        self.resampler = Some(Resampler::new(sw, sh, dw, dh));
                    }
                    let mut out = vec![0; dw * dh * 4];
                    let resampler = self.resampler.as_mut().unwrap();
                    resampler.rgba(&frame.data[..sw * sh * 4], &mut out);
                    out
                }
            }
        };
        self.picture = Some(picture);
        Ok(())
    }
}
