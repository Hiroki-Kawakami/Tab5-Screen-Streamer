//! Drawing frames on the device, without I/O: its state from the events,
//! pacing, JPEG quality and each frame's messages. Times are µs on any
//! monotonic clock.

use std::collections::VecDeque;

use crate::draw::{self, Band, Batch, Encoded};
use crate::jpeg::{self, Image};
use crate::layout::Layout;
use crate::protocol::{self, Encoding, Event, Info, InputMode, InputTarget, Target, TouchPoint};

pub const JPEG_BANDS: u32 = 4;
const RATE_WINDOW: u64 = 1_000_000;
const MIN_QUALITY: u8 = 20;
const RAISE_INTERVAL: u64 = 100_000;

/// Rows [row, row + rows) of `height` in `count` bands of multiples of `unit`.
fn bands(height: u32, unit: u32, count: u32) -> Vec<(u32, u32)> {
    let step = height.div_ceil(count).div_ceil(unit).max(1) * unit;
    (0..height)
        .step_by(step as usize)
        .map(|row| (row, step.min(height - row)))
        .collect()
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The canvas rows of each JPEG band of a frame.
pub fn jpeg_bands(layout: &Layout, info: &Info) -> Vec<(u32, u32)> {
    let placement = layout.placement(info.panel_width as u32);
    let unit = match layout {
        Layout::Direct { .. } => {
            let align = info.direct_align.max(1) as u32;
            align * 16 / gcd(align, 16)
        }
        Layout::Blit { .. } => 16,
    };
    bands(placement.canvas.1 as u32, unit, JPEG_BANDS)
}

pub fn encode_band(
    image: &Image,
    layout: &Layout,
    info: &Info,
    quality: u8,
    (row, rows): (u32, u32),
) -> Band {
    let placement = layout.placement(info.panel_width as u32);
    let mut out = Vec::new();
    let range = row as usize..(row + rows) as usize;
    jpeg::encode(image, placement, range, quality, &mut out);
    Band {
        row,
        rows,
        jpeg: out,
    }
}

/// What a layout is built from.
pub type LayoutKey = (u16, u16, u16, u16, u16, u32);

pub fn layout_key(info: &Info, rotation: u32) -> LayoutKey {
    (
        info.panel_width,
        info.panel_height,
        info.direct_align,
        info.blit_max_width,
        info.blit_max_height,
        rotation,
    )
}

/// JPEG quality: `ceiling`, lowered to keep the video near `max_rate`
/// bytes/s unless that is None.
#[derive(Clone, Copy)]
pub struct Quality {
    pub ceiling: u8,
    pub max_rate: Option<f64>,
}

impl Quality {
    pub const DEFAULT: Quality = Quality {
        ceiling: 80,
        max_rate: Some(10e6),
    };
}

struct RateControl {
    options: Quality,
    quality: u8,
    sent: VecDeque<u64>,
    last_bytes: usize,
    raised: Option<u64>,
}

impl RateControl {
    fn new(options: Quality) -> Self {
        RateControl {
            options,
            quality: options.ceiling,
            sent: VecDeque::new(),
            last_bytes: 0,
            raised: None,
        }
    }

    fn next(&mut self, now: u64) -> u8 {
        let Some(max_rate) = self.options.max_rate else {
            return self.quality;
        };
        while self
            .sent
            .front()
            .is_some_and(|&at| now.saturating_sub(at) >= RATE_WINDOW)
        {
            self.sent.pop_front();
        }
        let need = (self.last_bytes * self.sent.len()) as f64 / max_rate;
        let floor = MIN_QUALITY.min(self.options.ceiling);
        let raised = *self.raised.get_or_insert(now);
        if need > 1.0 {
            let step = ((need - 1.0) * 20.0).ceil().max(1.0) as u8;
            self.quality = self.quality.saturating_sub(step).max(floor);
        } else if need < 0.8 && now.saturating_sub(raised) >= RAISE_INTERVAL {
            let step = if need < 0.5 { 5 } else { 1 };
            self.quality = self.quality.saturating_add(step).min(self.options.ceiling);
            self.raised = Some(now);
        }
        self.quality
    }

    fn record(&mut self, bytes: usize, now: u64) {
        self.sent.push_back(now);
        self.last_bytes = bytes;
    }
}

pub struct Presenter {
    pub info: Info,
    pub layout: Layout,
    pub encoding: Encoding,
    front: bool,
    /// Fixed, or the device's orientation when None.
    rotation: Option<u32>,
    swaps: VecDeque<u32>,
    /// The next frame covers the whole content.
    pub redraw: bool,
    /// The next frame starts with CLEAR.
    clear: bool,
    /// The display is off until the next frame.
    off: bool,
    pub audio_level: Option<(u32, u32)>,
    /// EV_TOUCH while touch is in Event mode, oldest first; an empty one
    /// also when it leaves the mode.
    pub touches: Vec<Vec<TouchPoint>>,
    rate: RateControl,
    last_start: Option<u64>,
}

impl Presenter {
    pub fn new(
        info: Info,
        layout: Layout,
        encoding: Encoding,
        front: bool,
        rotation: Option<u32>,
        quality: Quality,
    ) -> Self {
        Presenter {
            info,
            layout,
            encoding,
            front,
            rotation,
            swaps: VecDeque::new(),
            redraw: true,
            clear: true,
            off: false,
            audio_level: None,
            touches: Vec::new(),
            rate: RateControl::new(quality),
            last_start: None,
        }
    }

    /// Blackens the panel before the next frame, which covers the whole content.
    pub fn clear(&mut self) {
        self.clear = true;
        self.redraw = true;
    }

    /// The JPEG quality for the frame about to be encoded.
    pub fn quality(&mut self, now: u64) -> u8 {
        self.rate.next(now)
    }

    pub fn rotation(&self) -> u32 {
        self.rotation.unwrap_or(self.info.orientation as u32)
    }

    pub fn connected(&mut self, info: Info) {
        self.info = info;
        self.swaps.clear();
        self.clear();
        self.off = false;
        self.audio_level = None;
    }

    pub fn describe(&self) -> String {
        let panel_width = self.info.panel_width as u32;
        let (fw, fh) = self.layout.frame();
        let (x, y, w, h) = self.layout.content(panel_width);
        format!(
            "{fw}x{fh} -> {w}x{h} at {x},{y}, rotation {}, {} {:?} {:?} into {}",
            self.rotation(),
            match self.layout {
                Layout::Direct { .. } => "DIRECT",
                Layout::Blit { .. } => "BLIT",
            },
            self.encoding,
            self.info.pixel_format,
            if self.front { "FRONT" } else { "BACK + SWAP" }
        )
    }

    /// DISPLAY_POWER off, which blackens the display; the next frame turns
    /// it on.
    pub fn power_off(&mut self, seq: u32) -> Vec<u8> {
        self.off = true;
        self.redraw = true;
        protocol::message(protocol::DISPLAY_POWER, seq, self.info.epoch, &[0; 4])
    }

    /// The device may have turned the display off by itself, as its waiting
    /// screen does when idle; the next frame turns it on.
    pub fn assume_off(&mut self) {
        self.off = true;
    }

    /// How long until a frame stays within max_fps; zero when it does now.
    pub fn until_due(&self, now: u64) -> u64 {
        let interval = (900_000 / self.info.max_fps.max(1) as u64) as i64;
        self.last_start.map_or(0, |last| {
            (interval - now.saturating_sub(last) as i64).max(0) as u64
        })
    }

    pub fn due(&self, now: u64) -> bool {
        self.until_due(now) == 0
    }

    /// Marks a frame as started, before it is encoded: max_fps spaces frame
    /// starts, so encoding time does not count against the next frame.
    pub fn start_frame(&mut self, now: u64) {
        self.last_start = Some(now);
    }

    /// swap_window swaps are unanswered; the next frame waits for SWAP_DONE.
    pub fn swaps_full(&self) -> bool {
        !self.front && self.swaps.len() >= self.info.swap_window.max(1) as usize
    }

    /// Applies `event`: None when it changes nothing the user sees, otherwise
    /// whether the layout has to be rebuilt.
    pub fn apply(&mut self, event: &Event) -> Option<bool> {
        let mut relayout = false;
        match event {
            Event::SwapDone { seq } => {
                while self.swaps.front().is_some_and(|s| s <= seq) {
                    self.swaps.pop_front();
                }
                return None;
            }
            Event::Orientation(orientation) => {
                self.info.orientation = *orientation as u8;
                relayout = self.rotation.is_none();
            }
            Event::Volume { .. } => {}
            Event::Brightness(brightness) => self.info.brightness = *brightness,
            Event::Audio { buffered, capacity } => {
                self.audio_level = Some((*buffered, *capacity));
                return None;
            }
            Event::Overlay { visible, .. } => self.info.overlay_visible = *visible,
            Event::Touch(points) => {
                if self.info.input_mode[InputTarget::Touch as usize] == InputMode::Event {
                    self.touches.push(points.clone());
                }
                return None;
            }
            Event::InputMode(target, mode) => {
                let current = &mut self.info.input_mode[*target as usize];
                if *target == InputTarget::Touch && *current == InputMode::Event {
                    self.touches.push(Vec::new());
                }
                *current = *mode;
            }
            Event::Damage(_) => self.redraw = true,
            Event::Format { format, epoch } => {
                self.info.pixel_format = *format;
                self.info.epoch = *epoch;
                self.clear();
            }
            _ => return None,
        }
        Some(relayout)
    }

    /// The transfers drawing `frame`, laid out by the current layout.
    pub fn frame(
        &mut self,
        frame: Encoded,
        now: u64,
        seq: &mut dyn FnMut() -> u32,
    ) -> Result<Vec<Vec<u8>>, String> {
        let target = if self.front {
            Target::Front
        } else {
            Target::Back
        };
        let mut batch = Batch::new(self.info.max_message[protocol::VIDEO] as usize);
        let mut messages = Vec::new();
        if self.clear {
            messages.push((protocol::CLEAR, vec![0; 4]));
        }
        self.clear = false;
        self.redraw = false;
        messages.extend(draw::frame_messages(
            &self.layout,
            self.encoding,
            target,
            &self.info,
            frame,
        ));
        let epoch = self.info.epoch;
        for (kind, payload) in messages {
            batch.push(protocol::message(kind, seq(), epoch, &payload))?;
        }
        if !self.front {
            let seq = seq();
            batch.push(protocol::message(protocol::SWAP, seq, epoch, &[0; 4]))?;
            self.swaps.push_back(seq);
        }
        if self.off {
            batch.push(protocol::message(
                protocol::DISPLAY_POWER,
                seq(),
                epoch,
                &[1, 0, 0, 0],
            ))?;
            self.off = false;
        }
        let bytes = batch.transfers.iter().map(Vec::len).sum();
        self.rate.record(bytes, now);
        Ok(batch.transfers)
    }
}
