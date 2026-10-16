//! Drawing frames on the device: its state and events, and each frame's
//! messages.

use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::draw::{self, Band, Batch, Encoded};
use streamer_core::jpeg::{self, Image};
use streamer_core::layout::Layout;
use streamer_core::protocol::{
    self, Encoding, Event, Info, InputMode, InputTarget, Target, TouchPoint,
};
use streamer_device::connection::{Connection, TIMEOUT, report};

use crate::{Notice, Notify};

const STATS_PERIOD: Duration = Duration::from_secs(1);
const JPEG_BANDS: u32 = 4;
const RATE_WINDOW: Duration = Duration::from_secs(1);
const MIN_QUALITY: u8 = 20;
const RAISE_INTERVAL: Duration = Duration::from_millis(100);

type Result<T> = std::result::Result<T, String>;

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

/// `image` as JPEG bands, encoded in parallel.
pub fn encode_jpeg(image: &Image, layout: &Layout, info: &Info, quality: u8) -> Encoded {
    let placement = layout.placement(info.panel_width as u32);
    let unit = match layout {
        Layout::Direct { .. } => {
            let align = info.direct_align.max(1) as u32;
            align * 16 / gcd(align, 16)
        }
        Layout::Blit { .. } => 16,
    };
    let rows = bands(placement.canvas.1 as u32, unit, JPEG_BANDS);
    thread::scope(|scope| {
        let workers: Vec<_> = rows
            .into_iter()
            .map(|(row, rows)| {
                scope.spawn(move || {
                    let mut out = Vec::new();
                    let range = row as usize..(row + rows) as usize;
                    jpeg::encode(image, placement, range, quality, &mut out);
                    Band {
                        row,
                        rows,
                        jpeg: out,
                    }
                })
            })
            .collect();
        Encoded::Jpeg(workers.into_iter().map(|w| w.join().unwrap()).collect())
    })
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
    sent: VecDeque<Instant>,
    last_bytes: usize,
    raised: Instant,
}

impl RateControl {
    fn new(options: Quality) -> Self {
        RateControl {
            options,
            quality: options.ceiling,
            sent: VecDeque::new(),
            last_bytes: 0,
            raised: Instant::now(),
        }
    }

    fn next(&mut self) -> u8 {
        let Some(max_rate) = self.options.max_rate else {
            return self.quality;
        };
        let now = Instant::now();
        while self.sent.front().is_some_and(|&at| now - at >= RATE_WINDOW) {
            self.sent.pop_front();
        }
        let need = (self.last_bytes * self.sent.len()) as f64 / max_rate;
        let floor = MIN_QUALITY.min(self.options.ceiling);
        if need > 1.0 {
            let step = ((need - 1.0) * 20.0).ceil().max(1.0) as u8;
            self.quality = self.quality.saturating_sub(step).max(floor);
        } else if need < 0.8 && now - self.raised >= RAISE_INTERVAL {
            let step = if need < 0.5 { 5 } else { 1 };
            self.quality = self.quality.saturating_add(step).min(self.options.ceiling);
            self.raised = now;
        }
        self.quality
    }

    fn record(&mut self, bytes: usize) {
        self.sent.push_back(Instant::now());
        self.last_bytes = bytes;
    }
}

pub struct Stats {
    start: Instant,
    pub frames: u32,
    pub dropped: u32,
    pub bytes: usize,
    quality: Option<(u8, u8)>,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start: Instant::now(),
            frames: 0,
            dropped: 0,
            bytes: 0,
            quality: None,
        }
    }

    fn quality(&mut self, quality: u8) {
        let (low, high) = self.quality.get_or_insert((quality, quality));
        *low = (*low).min(quality);
        *high = (*high).max(quality);
    }

    pub fn report(&mut self, audio: Option<(u32, u32)>, notify: &Notify) {
        let elapsed = self.start.elapsed();
        if elapsed < STATS_PERIOD {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        let audio = match audio {
            Some((buffered, capacity)) => format!(", audio {buffered}/{capacity} frames buffered"),
            None => String::new(),
        };
        let quality = match self.quality {
            Some((low, high)) if low == high => format!(", quality {low}"),
            Some((low, high)) => format!(", quality {low}-{high}"),
            None => String::new(),
        };
        notify(Notice::Stats(format!(
            "{:.1} frames/s, {} dropped, {:.1} MB/s{quality}{audio}",
            self.frames as f64 / seconds,
            self.dropped,
            self.bytes as f64 / seconds / 1e6
        )));
        *self = Stats::new();
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
    pub stats: Stats,
    rate: RateControl,
    last_sent: Option<Instant>,
    pub notify: Notify,
}

impl Presenter {
    pub fn new(
        info: Info,
        layout: Layout,
        encoding: Encoding,
        front: bool,
        rotation: Option<u32>,
        quality: Quality,
        notify: Notify,
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
            stats: Stats::new(),
            rate: RateControl::new(quality),
            last_sent: None,
            notify,
        }
    }

    /// Blackens the panel before the next frame, which covers the whole content.
    pub fn clear(&mut self) {
        self.clear = true;
        self.redraw = true;
    }

    /// The JPEG quality for the frame about to be encoded.
    pub fn quality(&mut self) -> u8 {
        let quality = self.rate.next();
        self.stats.quality(quality);
        quality
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

    pub fn describe(&self) {
        let panel_width = self.info.panel_width as u32;
        let (fw, fh) = self.layout.frame();
        let (x, y, w, h) = self.layout.content(panel_width);
        (self.notify)(Notice::Layout(format!(
            "{fw}x{fh} -> {w}x{h} at {x},{y}, rotation {}, {} {:?} {:?} into {}",
            self.rotation(),
            match self.layout {
                Layout::Direct { .. } => "DIRECT",
                Layout::Blit { .. } => "BLIT",
            },
            self.encoding,
            self.info.pixel_format,
            if self.front { "FRONT" } else { "BACK + SWAP" }
        )));
    }

    /// Turns the display off, which blackens it; the next frame turns it on.
    pub fn power_off(&mut self, connection: &mut Connection) -> Result<()> {
        let seq = connection.next_video_seq();
        let message = protocol::message(protocol::DISPLAY_POWER, seq, self.info.epoch, &[0; 4]);
        connection.send_video(message)?;
        self.off = true;
        self.redraw = true;
        Ok(())
    }

    /// How long until a frame stays within max_fps; zero when it does now.
    pub fn until_due(&self) -> Duration {
        let interval = Duration::from_secs_f64(0.9 / self.info.max_fps.max(1) as f64);
        self.last_sent.map_or(Duration::ZERO, |last| {
            interval.saturating_sub(last.elapsed())
        })
    }

    pub fn due(&self) -> bool {
        self.until_due().is_zero()
    }

    /// Marks a frame as started, before it is encoded: max_fps spaces frame
    /// starts, so encoding time does not count against the next frame.
    pub fn start_frame(&mut self) {
        self.last_sent = Some(Instant::now());
    }

    /// Applies the pending events, waiting for SWAP_DONE while swap_window
    /// swaps are unanswered; true when the layout has to be rebuilt.
    pub fn events(&mut self, connection: &mut Connection) -> Result<bool> {
        let window = self.info.swap_window.max(1) as usize;
        let mut relayout = false;
        loop {
            let wait = if !self.front && self.swaps.len() >= window {
                TIMEOUT
            } else {
                Duration::ZERO
            };
            match connection.event(wait)? {
                Some(event) => relayout |= self.apply(event),
                None if wait > Duration::ZERO => return Err("no SWAP_DONE".into()),
                None => return Ok(relayout),
            }
        }
    }

    fn apply(&mut self, event: Event) -> bool {
        let mut relayout = false;
        match &event {
            Event::SwapDone { seq } => {
                while self.swaps.front().is_some_and(|s| s <= seq) {
                    self.swaps.pop_front();
                }
                return false;
            }
            Event::Orientation(orientation) => {
                self.info.orientation = *orientation as u8;
                relayout = self.rotation.is_none();
            }
            Event::Volume { .. } => {}
            Event::Brightness(brightness) => self.info.brightness = *brightness,
            Event::Audio { buffered, capacity } => {
                self.audio_level = Some((*buffered, *capacity));
                return false;
            }
            Event::Overlay { visible, .. } => self.info.overlay_visible = *visible,
            Event::Touch(points) => {
                if self.info.input_mode[InputTarget::Touch as usize] == InputMode::Event {
                    self.touches.push(points.clone());
                }
                return false;
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
            event => {
                report(event);
                return false;
            }
        }
        (self.notify)(Notice::Event(event));
        relayout
    }

    /// Draws `frame`, laid out by the current layout.
    pub fn send(&mut self, connection: &mut Connection, frame: Encoded) -> Result<()> {
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
            let seq = connection.next_video_seq();
            batch.push(protocol::message(kind, seq, epoch, &payload))?;
        }
        if !self.front {
            let seq = connection.next_video_seq();
            batch.push(protocol::message(protocol::SWAP, seq, epoch, &[0; 4]))?;
            self.swaps.push_back(seq);
        }
        if self.off {
            let seq = connection.next_video_seq();
            batch.push(protocol::message(
                protocol::DISPLAY_POWER,
                seq,
                epoch,
                &[1, 0, 0, 0],
            ))?;
            self.off = false;
        }
        let mut bytes = 0;
        for transfer in batch.transfers {
            bytes += transfer.len();
            connection.send_video(transfer)?;
        }
        self.stats.bytes += bytes;
        self.stats.frames += 1;
        self.rate.record(bytes);
        Ok(())
    }
}
