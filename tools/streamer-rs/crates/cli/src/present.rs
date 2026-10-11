//! Drawing frames on the device: its state and events, and each frame's
//! messages.

use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::draw::{self, Band, Batch, Encoded};
use streamer_core::jpeg::{self, Image};
use streamer_core::layout::Layout;
use streamer_core::protocol::{self, Encoding, Event, Info, Target};
use streamer_device::connection::{Connection, TIMEOUT, report};

const STATS_PERIOD: Duration = Duration::from_secs(5);
const JPEG_BANDS: u32 = 4;

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

pub struct Stats {
    start: Instant,
    pub frames: u32,
    pub dropped: u32,
    pub bytes: usize,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start: Instant::now(),
            frames: 0,
            dropped: 0,
            bytes: 0,
        }
    }

    pub fn report(&mut self, audio: Option<(u32, u32)>) {
        let elapsed = self.start.elapsed();
        if elapsed < STATS_PERIOD {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        let audio = match audio {
            Some((buffered, capacity)) => format!(", audio {buffered}/{capacity} frames buffered"),
            None => String::new(),
        };
        println!(
            "{:.1} frames/s, {} dropped, {:.1} MB/s{audio}",
            self.frames as f64 / seconds,
            self.dropped,
            self.bytes as f64 / seconds / 1e6
        );
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
    /// The next frame covers the whole panel, letterbox included.
    pub redraw: bool,
    pub audio_level: Option<(u32, u32)>,
    pub stats: Stats,
    last_sent: Option<Instant>,
}

impl Presenter {
    pub fn new(
        info: Info,
        layout: Layout,
        encoding: Encoding,
        front: bool,
        rotation: Option<u32>,
    ) -> Self {
        Presenter {
            info,
            layout,
            encoding,
            front,
            rotation,
            swaps: VecDeque::new(),
            redraw: true,
            audio_level: None,
            stats: Stats::new(),
            last_sent: None,
        }
    }

    pub fn rotation(&self) -> u32 {
        self.rotation.unwrap_or(self.info.orientation as u32)
    }

    pub fn connected(&mut self, info: Info) {
        self.info = info;
        self.swaps.clear();
        self.redraw = true;
        self.audio_level = None;
    }

    pub fn describe(&self) {
        let panel_width = self.info.panel_width as u32;
        let (fw, fh) = self.layout.frame();
        let (x, y, w, h) = self.layout.content(panel_width);
        println!(
            "{fw}x{fh} -> {w}x{h} at {x},{y}, rotation {}, {} {:?} {:?} into {}",
            self.rotation(),
            match self.layout {
                Layout::Direct { .. } => "DIRECT",
                Layout::Blit { .. } => "BLIT",
            },
            self.encoding,
            self.info.pixel_format,
            if self.front { "FRONT" } else { "BACK + SWAP" }
        );
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
        match event {
            Event::SwapDone { seq } => {
                while self.swaps.front().is_some_and(|&s| s <= seq) {
                    self.swaps.pop_front();
                }
            }
            Event::Orientation(orientation) => {
                println!("orientation {orientation}");
                self.info.orientation = orientation as u8;
                return self.rotation.is_none();
            }
            Event::Volume {
                stream,
                mute,
                volume,
            } => println!(
                "volume {stream}: {:.1} dB{}",
                volume as f64 / 256.0,
                if mute { ", muted" } else { "" }
            ),
            Event::Audio { buffered, capacity } => self.audio_level = Some((buffered, capacity)),
            Event::Overlay { visible, stream } => {
                println!(
                    "overlay {}, stream {}x{} at {},{}",
                    if visible { "shown" } else { "hidden" },
                    stream.2,
                    stream.3,
                    stream.0,
                    stream.1
                );
                self.info.overlay_visible = visible;
            }
            Event::Damage((x, y, w, h)) => {
                println!("damage {w}x{h} at {x},{y}");
                self.redraw = true;
            }
            Event::Format { format, epoch } => {
                println!("pixel format {format:?}, epoch {epoch}");
                self.info.pixel_format = format;
                self.info.epoch = epoch;
                self.redraw = true;
            }
            event => report(&event),
        }
        false
    }

    /// Draws `frame`, laid out by the current layout.
    pub fn send(&mut self, connection: &mut Connection, frame: Encoded) -> Result<()> {
        let target = if self.front {
            Target::Front
        } else {
            Target::Back
        };
        let panel_width = self.info.panel_width as u32;
        let mut batch = Batch::new(self.info.max_message[protocol::VIDEO] as usize);
        let mut messages = Vec::new();
        if !self.front || self.redraw {
            messages = draw::letterbox(target, &self.info, self.layout.content(panel_width));
        }
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
        for transfer in batch.transfers {
            self.stats.bytes += transfer.len();
            connection.send_video(transfer)?;
        }
        self.stats.frames += 1;
        Ok(())
    }
}
