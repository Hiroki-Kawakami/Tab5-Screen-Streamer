//! Frames to the devices' video endpoints, for the video worker. Times are
//! milliseconds from `performance.now()`.

use js_sys::{Array, Uint8Array, Uint32Array};
use streamer_core::color::Matrix;
use streamer_core::draw::{Band, Encoded};
use streamer_core::jpeg::{self, Image, Pixels, Placement};
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

/// What a device's layout is built from.
type Built = (LayoutKey, (u32, u32));

/// The session with one device.
struct Session {
    device: u32,
    info: Info,
    presenter: Option<Presenter>,
    seq: u32,
    built: Option<Built>,
}

impl Session {
    fn info(&self) -> &Info {
        self.presenter.as_ref().map_or(&self.info, |p| &p.info)
    }
}

/// The devices sharing a layout, and the bands of a frame in it.
struct Group {
    devices: Vec<u32>,
    bands: Vec<(u32, u32)>,
}

/// A frame whose bands are being encoded.
struct Pending {
    generation: u32,
    now: u64,
    groups: Vec<Group>,
}

/// Shows the same frames on every device. A frame goes to all of them at
/// once, so none runs ahead of the slowest; devices with the same layout
/// share its JPEGs.
#[wasm_bindgen]
#[derive(Default)]
pub struct Video {
    sessions: Vec<Session>,
    frame: Option<Frame>,
    /// Where the next frame's pixels are copied; the data of the frame before.
    input: Vec<u8>,
    /// Changes whenever bands being encoded no longer fit the presenters.
    generation: u32,
    pending: Option<Pending>,
    /// `frame` at each layout's frame size: I420 or BGRX.
    pictures: Vec<((u32, u32), Vec<u8>)>,
    converters: Vec<((u32, u32), Converter)>,
    resamplers: Vec<Resampler>,
    quality: u8,
}

fn us(ms: f64) -> u64 {
    (ms * 1000.0).max(0.0) as u64
}

fn image(picture: &[u8], width: usize, height: usize, yuv: bool) -> Image<'_> {
    let pixels = match yuv {
        true => {
            let (y, chroma) = picture.split_at(width * height);
            let (cb, cr) = chroma.split_at(width * height / 4);
            Pixels::I420 {
                y,
                y_stride: width,
                cb,
                cr,
                c_stride: width / 2,
            }
        }
        false => Pixels::Bgrx {
            data: picture,
            stride: width * 4,
        },
    };
    Image {
        width,
        height,
        pixels,
    }
}

/// Rows [row, row + rows) of the canvas of a frame from `Video::prepare`, as
/// a JPEG.
#[wasm_bindgen(js_name = encodeBand)]
#[allow(clippy::too_many_arguments)]
pub fn encode_band(
    picture: &[u8],
    width: usize,
    height: usize,
    yuv: bool,
    rotation: u32,
    canvas_width: usize,
    canvas_height: usize,
    origin_x: usize,
    origin_y: usize,
    quality: u8,
    row: usize,
    rows: usize,
) -> Vec<u8> {
    let placement = Placement {
        rotation,
        canvas: (canvas_width, canvas_height),
        origin: (origin_x, origin_y),
    };
    let mut out = Vec::new();
    let image = image(picture, width, height, yuv);
    jpeg::encode(&image, placement, row..row + rows, quality, &mut out);
    out
}

fn direct(info: &Info, display: (u32, u32), rotation: u32) -> Result<Layout, JsError> {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    let align = info.direct_align.max(1) as u32;
    Layout::direct(display, rotation, panel, align, true).map_err(|e| JsError::new(&e))
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

    /// A session with `device` opened with the INFO carrying `seq` in
    /// `data`, a control IN transfer.
    pub fn connected(&mut self, device: u32, data: &[u8], seq: u32) -> Result<(), JsError> {
        let info = protocol::parse_events(data)
            .into_iter()
            .find_map(|event| match event {
                Event::Info { seq: s, info } if s == seq => Some(info),
                _ => None,
            })
            .ok_or_else(|| JsError::new("no INFO in the transfer"))?;
        self.generation += 1;
        match self.sessions.iter_mut().find(|s| s.device == device) {
            Some(session) => {
                session.seq = 0;
                if let Some(presenter) = &mut session.presenter {
                    presenter.connected(info.clone());
                }
                session.info = info;
            }
            None => self.sessions.push(Session {
                device,
                info,
                presenter: None,
                seq: 0,
                built: None,
            }),
        }
        self.relayout()
    }

    pub fn lost(&mut self, device: u32) {
        self.sessions.retain(|s| s.device != device);
        self.generation += 1;
    }

    /// Applies the events of a control IN transfer from `device`.
    pub fn events(&mut self, device: u32, data: &[u8]) -> Result<(), JsError> {
        let Some(presenter) = self
            .sessions
            .iter_mut()
            .find(|s| s.device == device)
            .and_then(|s| s.presenter.as_mut())
        else {
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

    /// The size a frame shown at `display` after `turn` is scaled to, or none
    /// before a session opens or when the devices differ in it.
    pub fn size(
        &self,
        display_width: u32,
        display_height: u32,
        turn: u32,
    ) -> Result<Option<Vec<u32>>, JsError> {
        let mut size = None;
        for session in &self.sessions {
            let info = session.info();
            let rotation = (info.orientation as u32 + turn) % 4;
            let frame = direct(info, (display_width, display_height), rotation)?.frame();
            if size.is_some_and(|s| s != frame) {
                return Ok(None);
            }
            size = Some(frame);
        }
        Ok(size.map(|(w, h)| vec![w, h]))
    }

    /// `size` bytes of wasm memory to copy the next frame's pixels into. The
    /// view is detached if the memory grows before the frame is taken.
    pub fn input(&mut self, size: usize) -> Uint8Array {
        self.input.resize(size, 0);
        unsafe { Uint8Array::view(&self.input) }
    }

    /// Takes a frame in a planar YUV `format` of `VideoFrame` from `input`;
    /// `planes` are its offsets and strides.
    #[wasm_bindgen(js_name = frameYuv)]
    #[allow(clippy::too_many_arguments)]
    pub fn frame_yuv(
        &mut self,
        format: &str,
        width: usize,
        height: usize,
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
        let data = std::mem::take(&mut self.input);
        self.take(Frame {
            width,
            height,
            data,
            kind,
            display: (display_width, display_height),
            turn,
        })
    }

    /// Takes a frame of B, G, R and one ignored byte per pixel from `input`.
    #[wasm_bindgen(js_name = frameBgrx)]
    pub fn frame_bgrx(
        &mut self,
        width: usize,
        height: usize,
        display_width: u32,
        display_height: u32,
        turn: u32,
    ) -> Result<(), JsError> {
        let data = std::mem::take(&mut self.input);
        self.take(Frame {
            width,
            height,
            data,
            kind: Kind::Bgrx,
            display: (display_width, display_height),
            turn,
        })
    }

    /// Whether a frame can be sent now to the devices it goes to: a new frame
    /// goes to every device, otherwise only to those that need a redraw.
    /// Each is within max_fps and not waiting for SWAP_DONE.
    pub fn ready(&self, now: f64, fresh: bool) -> bool {
        let mut any = false;
        for presenter in self.targets(fresh) {
            if presenter.swaps_full() || !presenter.due(us(now)) {
                return false;
            }
            any = true;
        }
        any
    }

    /// Forgets the latest frame, for a new source.
    pub fn reset(&mut self) {
        self.frame = None;
        self.pictures.clear();
        self.generation += 1;
    }

    /// Milliseconds until a frame stays within max_fps on every device.
    #[wasm_bindgen(js_name = untilDue)]
    pub fn until_due(&self, now: f64) -> f64 {
        self.presenters()
            .map(|p| p.until_due(us(now)) as f64 / 1000.0)
            .fold(0.0, f64::max)
    }

    /// Some device needs the whole content drawn again.
    pub fn redraw(&self) -> bool {
        self.frame.is_some() && self.presenters().any(|p| p.redraw)
    }

    pub fn describe(&self) -> Option<String> {
        let lines: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|s| {
                Some(format!(
                    "device {}: {}",
                    s.device,
                    s.presenter.as_ref()?.describe()
                ))
            })
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// The quality of the last frame encoded.
    #[wasm_bindgen(getter)]
    pub fn quality(&self) -> u8 {
        self.quality
    }

    /// Starts sending the latest frame to the devices `ready` checked: for
    /// each layout among them, the picture and placement to encode with
    /// `encodeBand` for each pair of `bands`; null when there is none.
    pub fn prepare(&mut self, now: f64, fresh: bool) -> Result<JsValue, JsError> {
        let now = us(now);
        self.convert()?;
        let Some(frame) = &self.frame else {
            return Ok(JsValue::NULL);
        };
        let mut groups: Vec<(Option<Built>, Vec<&mut Session>)> = Vec::new();
        for session in &mut self.sessions {
            let Some(presenter) = &session.presenter else {
                continue;
            };
            if !fresh && !presenter.redraw {
                continue;
            }
            match groups.iter_mut().find(|(key, _)| *key == session.built) {
                Some((_, members)) => members.push(session),
                None => groups.push((session.built, vec![session])),
            }
        }
        if groups.is_empty() {
            return Ok(JsValue::NULL);
        }
        let jobs = Array::new();
        let mut pending = Vec::new();
        let mut lowest = u8::MAX;
        for (_, mut members) in groups {
            let mut quality = u8::MAX;
            let mut devices = Vec::new();
            for session in members.iter_mut() {
                let presenter = session.presenter.as_mut().unwrap();
                presenter.start_frame(now);
                quality = quality.min(presenter.quality(now));
                devices.push(session.device);
            }
            lowest = lowest.min(quality);
            let presenter = members[0].presenter.as_ref().unwrap();
            let (w, h) = presenter.layout.frame();
            let Some((_, picture)) = self.pictures.iter().find(|(size, _)| *size == (w, h)) else {
                return Ok(JsValue::NULL);
            };
            let placement = presenter
                .layout
                .placement(presenter.info.panel_width as u32);
            let bands = present::jpeg_bands(&presenter.layout, &presenter.info);
            let rows: Vec<u32> = bands.iter().flat_map(|&(row, rows)| [row, rows]).collect();
            pending.push(Group { devices, bands });
            jobs.push(&crate::object(&[
                ("picture", Uint8Array::from(&picture[..]).into()),
                ("width", w.into()),
                ("height", h.into()),
                ("yuv", matches!(frame.kind, Kind::Yuv { .. }).into()),
                ("rotation", placement.rotation.into()),
                ("canvasWidth", (placement.canvas.0 as u32).into()),
                ("canvasHeight", (placement.canvas.1 as u32).into()),
                ("originX", (placement.origin.0 as u32).into()),
                ("originY", (placement.origin.1 as u32).into()),
                ("quality", quality.into()),
                ("bands", Uint32Array::from(&rows[..]).into()),
            ]));
        }
        self.quality = lowest;
        self.pending = Some(Pending {
            generation: self.generation,
            now,
            groups: pending,
        });
        Ok(jobs.into())
    }

    /// The transfers drawing the prepared frame from the JPEGs of each job's
    /// bands, as `{ device, transfers }` with `Uint8Array`s; none if a
    /// session or layout changed meanwhile.
    pub fn finish(&mut self, jpegs: Array) -> Result<Array, JsError> {
        let out = Array::new();
        let Some(pending) = self.pending.take() else {
            return Ok(out);
        };
        if pending.generation != self.generation {
            return Ok(out);
        }
        for (group, group_jpegs) in pending.groups.into_iter().zip(jpegs.iter()) {
            let group_jpegs = Array::from(&group_jpegs);
            let bands: Vec<Band> = group
                .bands
                .into_iter()
                .zip(group_jpegs.iter())
                .map(|((row, rows), jpeg)| Band {
                    row,
                    rows,
                    jpeg: Uint8Array::new(&jpeg).to_vec(),
                })
                .collect();
            for device in group.devices {
                let Some(Session {
                    presenter: Some(presenter),
                    seq,
                    ..
                }) = self.sessions.iter_mut().find(|s| s.device == device)
                else {
                    continue;
                };
                let mut next = || {
                    *seq += 1;
                    *seq
                };
                let copy = bands
                    .iter()
                    .map(|b| Band {
                        row: b.row,
                        rows: b.rows,
                        jpeg: b.jpeg.clone(),
                    })
                    .collect();
                let transfers = presenter
                    .frame(Encoded::Jpeg(copy), pending.now, &mut next)
                    .map_err(|e| JsError::new(&e))?;
                let transfers: Array = transfers
                    .iter()
                    .map(|t| JsValue::from(Uint8Array::from(&t[..])))
                    .collect();
                out.push(&crate::object(&[
                    ("device", device.into()),
                    ("transfers", transfers.into()),
                ]));
            }
        }
        Ok(out)
    }
}

impl Video {
    fn presenters(&self) -> impl Iterator<Item = &Presenter> {
        self.sessions.iter().filter_map(|s| s.presenter.as_ref())
    }

    fn targets(&self, fresh: bool) -> impl Iterator<Item = &Presenter> {
        self.presenters().filter(move |p| fresh || p.redraw)
    }

    fn take(&mut self, frame: Frame) -> Result<(), JsError> {
        let starting = match self.frame.replace(frame) {
            Some(old) => {
                self.input = old.data;
                false
            }
            None => true,
        };
        self.pictures.clear();
        self.relayout()?;
        if starting {
            for session in &mut self.sessions {
                if let Some(presenter) = &mut session.presenter {
                    presenter.assume_off();
                }
            }
        }
        Ok(())
    }

    /// Lays the latest frame out for each device, once both are known.
    fn relayout(&mut self) -> Result<(), JsError> {
        let Some(frame) = &self.frame else {
            return Ok(());
        };
        let turn = frame.turn;
        let display = frame.display;
        for session in &mut self.sessions {
            let info = session.info().clone();
            let rotation = (info.orientation as u32 + turn) % 4;
            let key = (present::layout_key(&info, rotation), display);
            if session.built == Some(key) {
                continue;
            }
            let layout = direct(&info, display, rotation)?;
            match &mut session.presenter {
                Some(presenter) => {
                    presenter.layout = layout;
                    presenter.clear();
                }
                None => {
                    session.presenter = Some(Presenter::new(
                        info,
                        layout,
                        Encoding::Jpeg,
                        false,
                        None,
                        Quality::DEFAULT,
                    ));
                }
            }
            session.built = Some(key);
            self.generation += 1;
        }
        Ok(())
    }

    /// Scales the latest frame to each layout's frame size.
    fn convert(&mut self) -> Result<(), JsError> {
        let Some(frame) = &self.frame else {
            return Ok(());
        };
        let mut sizes: Vec<(u32, u32)> = Vec::new();
        for presenter in self.sessions.iter().filter_map(|s| s.presenter.as_ref()) {
            let size = presenter.layout.frame();
            if !sizes.contains(&size) {
                sizes.push(size);
            }
        }
        for &(w, h) in &sizes {
            if self.pictures.iter().any(|(size, _)| *size == (w, h)) {
                continue;
            }
            let picture = match &frame.kind {
                Kind::Yuv {
                    layout,
                    planes,
                    color,
                } => {
                    let at = match self.converters.iter().position(|(size, _)| *size == (w, h)) {
                        Some(at) => at,
                        None => {
                            let size = (w as usize, h as usize);
                            let geometry = Geometry {
                                scaled: size,
                                crop: size,
                                source_rotation: 0,
                                output_rotation: 0,
                                stored: size,
                                output: Output::Bt601Full,
                            };
                            self.converters.push(((w, h), Converter::new(geometry)));
                            self.converters.len() - 1
                        }
                    };
                    let source = yuv::Source {
                        layout: *layout,
                        width: frame.width,
                        height: frame.height,
                        data: &frame.data,
                        planes,
                    };
                    self.converters[at]
                        .1
                        .convert(&source, *color)
                        .map_err(|e| JsError::new(&e.to_string()))?
                }
                Kind::Bgrx => {
                    let (sw, sh) = (frame.width, frame.height);
                    let (dw, dh) = (w as usize, h as usize);
                    if (sw, sh) == (dw, dh) {
                        frame.data.clone()
                    } else {
                        let at = match self
                            .resamplers
                            .iter()
                            .position(|r| r.src == (sw, sh) && r.dst == (dw, dh))
                        {
                            Some(at) => at,
                            None => {
                                self.resamplers.push(Resampler::new(sw, sh, dw, dh));
                                self.resamplers.len() - 1
                            }
                        };
                        let mut out = vec![0; dw * dh * 4];
                        self.resamplers[at].rgba(&frame.data[..sw * sh * 4], &mut out);
                        out
                    }
                }
            };
            self.pictures.push(((w, h), picture));
        }
        self.converters.retain(|(size, _)| sizes.contains(size));
        self.resamplers.retain(|r| {
            sizes.contains(&(r.dst.0 as u32, r.dst.1 as u32))
                && r.src == (frame.width, frame.height)
        });
        Ok(())
    }
}
