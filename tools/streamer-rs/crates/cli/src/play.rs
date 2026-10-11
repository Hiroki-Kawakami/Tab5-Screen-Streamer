//! Streaming a video from ffmpeg: pacing, encoding and drawing frames.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::convert::{self, I420};
use streamer_core::draw::{self, Band, Batch, Encoded};
use streamer_core::jpeg::{self, Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::protocol::{self, Encoding, Event, Info, Target};
use streamer_device::connection::{AudioOut, Connection, Device, TIMEOUT, report};

use crate::audio;
use crate::clock::Clock;
use crate::media::{AudioFormat, Frame, Input, Media};

const STATS_PERIOD: Duration = Duration::from_secs(5);
const AUDIO_QUEUE: usize = 64;
const JPEG_BANDS: u32 = 4;
/// A frame later than this is dropped to catch up.
const LATE_US: i64 = 50_000;
/// Further ahead than this, the input has jumped; the clock follows it.
const JUMP_US: i64 = 2_000_000;

type Result<T> = std::result::Result<T, String>;

pub struct PlayOptions {
    pub input: Input,
    pub encoding: Encoding,
    pub front: bool,
    pub blit: bool,
    pub audio: Option<AudioFormat>,
    pub rotation: Option<u32>,
    pub quality: u8,
    pub realtime: bool,
}

/// Rows [row, row + rows) of `height` in `count` bands of multiples of `unit`.
fn bands(height: u32, unit: u32, count: u32) -> Vec<(u32, u32)> {
    let step = height.div_ceil(count).div_ceil(unit).max(1) * unit;
    (0..height)
        .step_by(step as usize)
        .map(|row| (row, step.min(height - row)))
        .collect()
}

fn encode(
    frame: &Frame,
    size: (u32, u32),
    layout: &Layout,
    info: &Info,
    options: &PlayOptions,
) -> Encoded {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let placement = layout.placement(info.panel_width as u32);
    if options.encoding != Encoding::Jpeg {
        let i420 = I420 {
            width: w,
            height: h,
            data: &frame.data,
        };
        return Encoded::Pixels(convert::to_panel(&i420, placement, info.pixel_format));
    }
    let (y, chroma) = frame.data.split_at(w * h);
    let (cb, cr) = chroma.split_at(w * h / 4);
    let image = Image {
        width: w,
        height: h,
        pixels: Pixels::I420 {
            y,
            y_stride: w,
            cb,
            cr,
            c_stride: w / 2,
        },
    };
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
                let image = &image;
                scope.spawn(move || {
                    let mut out = Vec::new();
                    let range = row as usize..(row + rows) as usize;
                    jpeg::encode(image, placement, range, options.quality, &mut out);
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

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// What a layout was built from.
fn layout_key(info: &Info, rotation: u32) -> (u16, u16, u16, u16, u16, u32) {
    (
        info.panel_width,
        info.panel_height,
        info.direct_align,
        info.blit_max_width,
        info.blit_max_height,
        rotation,
    )
}

fn layout(
    options: &PlayOptions,
    info: &Info,
    size: (u32, u32),
    rotation: u32,
    scale: bool,
) -> Result<Layout> {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    if options.blit {
        let max = (info.blit_max_width as u32, info.blit_max_height as u32);
        if max.0 < 16 || max.1 < 16 {
            return Err("the device takes no DRAW_BLIT".into());
        }
        Layout::blit(size, rotation, panel, max, scale)
    } else {
        Layout::direct(
            size,
            rotation,
            panel,
            info.direct_align.max(1) as u32,
            scale,
        )
    }
}

/// Applies a device event; true when the layout has to be rebuilt.
fn apply(
    event: Event,
    info: &mut Info,
    swaps: &mut VecDeque<u32>,
    audio: &mut Option<(u32, u32)>,
    redraw: &mut bool,
    options: &PlayOptions,
) -> bool {
    match event {
        Event::SwapDone { seq } => {
            while swaps.front().is_some_and(|&s| s <= seq) {
                swaps.pop_front();
            }
            false
        }
        Event::Orientation(orientation) => {
            println!("orientation {orientation}");
            info.orientation = orientation as u8;
            options.rotation.is_none()
        }
        Event::Volume {
            stream,
            mute,
            volume,
        } => {
            println!(
                "volume {stream}: {:.1} dB{}",
                volume as f64 / 256.0,
                if mute { ", muted" } else { "" }
            );
            false
        }
        Event::Audio { buffered, capacity } => {
            *audio = Some((buffered, capacity));
            false
        }
        Event::Overlay { visible, stream } => {
            println!(
                "overlay {}, stream {}x{} at {},{}",
                if visible { "shown" } else { "hidden" },
                stream.2,
                stream.3,
                stream.0,
                stream.1
            );
            info.overlay_visible = visible;
            false
        }
        Event::Damage((x, y, w, h)) => {
            println!("damage {w}x{h} at {x},{y}");
            *redraw = true;
            false
        }
        Event::Format { format, epoch } => {
            println!("pixel format {format:?}, epoch {epoch}");
            info.pixel_format = format;
            info.epoch = epoch;
            *redraw = true;
            false
        }
        event => {
            report(&event);
            false
        }
    }
}

struct Stats {
    start: Instant,
    frames: u32,
    dropped: u32,
    bytes: usize,
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

    fn report(&mut self, audio: Option<(u32, u32)>) {
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

struct Player {
    options: PlayOptions,
    target: Target,
    info: Info,
    media: Media,
    layout: Layout,
    key: (u16, u16, u16, u16, u16, u32),
    probed: Option<(u32, u32)>,
    audio_packets: Option<SyncSender<Frame>>,
    audio_outs: Option<Sender<AudioOut>>,
    clock: Arc<Clock>,
    last_pts: i64,
    last_sent: Option<Instant>,
    swaps: VecDeque<u32>,
    redraw: bool,
    audio_level: Option<(u32, u32)>,
    stats: Stats,
}

impl Player {
    fn rotation(&self) -> u32 {
        self.options
            .rotation
            .unwrap_or(self.info.orientation as u32)
    }

    fn describe(&self) {
        let panel_width = self.info.panel_width as u32;
        let (fw, fh) = self.layout.frame();
        let (x, y, w, h) = self.layout.content(panel_width);
        println!(
            "{}x{} -> {w}x{h} at {x},{y}, rotation {}, {} {:?} {:?} into {}",
            fw,
            fh,
            self.rotation(),
            if self.options.blit { "BLIT" } else { "DIRECT" },
            self.options.encoding,
            self.info.pixel_format,
            if self.options.front {
                "FRONT"
            } else {
                "BACK + SWAP"
            }
        );
    }

    /// Lays the input out again for the current device and rotation. ffmpeg
    /// restarts where it was, since it does the scaling; stdin cannot be
    /// scaled, and keeps its layout when the new one does not fit.
    fn relayout(&mut self) -> Result<()> {
        let rotation = self.rotation();
        let key = layout_key(&self.info, rotation);
        if key == self.key {
            return Ok(());
        }
        match self.probed {
            Some(size) => {
                let layout = layout(&self.options, &self.info, size, rotation, true)?;
                let start = self.last_pts.max(0);
                self.media = Media::spawn(
                    &self.options.input,
                    layout.frame(),
                    self.options.audio,
                    start,
                    self.audio_packets.clone(),
                )?;
                self.layout = layout;
            }
            None => match layout(&self.options, &self.info, self.media.size, rotation, false) {
                Ok(layout) => self.layout = layout,
                Err(e) => {
                    eprintln!("keeping the layout: {e}");
                    return Ok(());
                }
            },
        }
        self.key = key;
        self.redraw = true;
        self.describe();
        Ok(())
    }

    fn connected(&mut self, connection: &mut Connection) -> Result<()> {
        self.info = connection.info.clone();
        self.swaps.clear();
        self.redraw = true;
        self.audio_level = None;
        self.relayout()?;
        if let (Some(outs), Some(format)) = (&self.audio_outs, self.media.audio) {
            if format.rate > self.info.audio_max_rate {
                eprintln!("the device plays up to {} Hz", self.info.audio_max_rate);
            } else if let Some(out) = connection.take_audio() {
                let _ = outs.send(out);
            }
        }
        Ok(())
    }

    /// Waits until `frame` is due; false when it is late or over max_fps, and
    /// is dropped.
    fn pace(&mut self, frame: &Frame) -> bool {
        if self.options.realtime {
            let now = self.clock.now_or_start(frame.pts);
            let ahead = frame.pts - now;
            if ahead > JUMP_US {
                self.clock.set(frame.pts);
            } else if ahead > 0 {
                thread::sleep(Duration::from_micros(ahead as u64));
            } else if -ahead > LATE_US && !self.redraw {
                return false;
            }
        }
        let interval = Duration::from_secs_f64(0.9 / self.info.max_fps.max(1) as f64);
        self.last_sent.is_none_or(|last| last.elapsed() >= interval)
    }

    /// Sends one frame; true when the layout has to be rebuilt first.
    fn frame(&mut self, connection: &mut Connection, frame: &Frame) -> Result<bool> {
        let window = self.info.swap_window.max(1) as usize;
        let mut relayout = false;
        loop {
            let wait = if !self.options.front && self.swaps.len() >= window {
                TIMEOUT
            } else {
                Duration::ZERO
            };
            match connection.event(wait)? {
                Some(event) => {
                    relayout |= apply(
                        event,
                        &mut self.info,
                        &mut self.swaps,
                        &mut self.audio_level,
                        &mut self.redraw,
                        &self.options,
                    )
                }
                None if wait > Duration::ZERO => return Err("no SWAP_DONE".into()),
                None => break,
            }
        }
        if relayout {
            return Ok(true);
        }
        let encoded = encode(
            frame,
            self.media.size,
            &self.layout,
            &self.info,
            &self.options,
        );
        let panel_width = self.info.panel_width as u32;
        let mut batch = Batch::new(self.info.max_message[protocol::VIDEO] as usize);
        let mut messages = Vec::new();
        if !self.options.front || self.redraw {
            messages = draw::letterbox(self.target, &self.info, self.layout.content(panel_width));
        }
        self.redraw = false;
        messages.extend(draw::frame_messages(
            &self.layout,
            self.options.encoding,
            self.target,
            &self.info,
            encoded,
        ));
        let epoch = self.info.epoch;
        for (kind, payload) in messages {
            let seq = connection.next_video_seq();
            batch.push(protocol::message(kind, seq, epoch, &payload))?;
        }
        if !self.options.front {
            let seq = connection.next_video_seq();
            batch.push(protocol::message(protocol::SWAP, seq, epoch, &[0; 4]))?;
            self.swaps.push_back(seq);
        }
        for transfer in batch.transfers {
            self.stats.bytes += transfer.len();
            connection.send_video(transfer)?;
        }
        self.stats.frames += 1;
        self.last_sent = Some(Instant::now());
        Ok(false)
    }
}

/// Streams until the input ends. While the device is away frames are paced
/// and dropped, so the input keeps its time.
pub fn play(device: &Device, options: PlayOptions) -> Result<()> {
    let mut connection = device.wait();
    let info = match &connection {
        Some(connection) => connection.info.clone(),
        None => return Err("no device".into()),
    };
    let rotation = options.rotation.unwrap_or(info.orientation as u32);
    let wants_audio = options.audio.is_some();
    let (audio_packets, packets) = match wants_audio {
        true => {
            let (sender, receiver) = mpsc::sync_channel(AUDIO_QUEUE);
            (Some(sender), Some(receiver))
        }
        false => (None, None),
    };
    let (media, layout, probed) = if options.input.is_stdin() {
        let media = Media::stdin(audio_packets.clone())?;
        let layout = layout(&options, &info, media.size, rotation, false)?;
        (media, layout, None)
    } else {
        let size = options.input.probe()?;
        let layout = layout(&options, &info, size, rotation, true)?;
        let media = Media::spawn(
            &options.input,
            layout.frame(),
            options.audio,
            0,
            audio_packets.clone(),
        )?;
        (media, layout, Some(size))
    };
    let clock = Arc::new(Clock::default());
    let audio_outs = match (media.audio, packets) {
        (Some(format), Some(packets)) => {
            let (sender, outs) = mpsc::channel();
            let clock = clock.clone();
            thread::spawn(move || audio::stream(packets, format, outs, clock));
            Some(sender)
        }
        _ => None,
    };
    let mut player = Player {
        target: if options.front {
            Target::Front
        } else {
            Target::Back
        },
        key: layout_key(&info, rotation),
        info,
        media,
        layout,
        probed,
        options,
        audio_packets,
        audio_outs,
        clock,
        last_pts: 0,
        last_sent: None,
        swaps: VecDeque::new(),
        redraw: true,
        audio_level: None,
        stats: Stats::new(),
    };
    player.describe();
    if let Some(connection) = &mut connection {
        player.connected(connection)?;
    }

    while let Ok(frame) = player.media.frames.recv() {
        player.last_pts = frame.pts;
        if connection.is_none() {
            connection = device.poll();
            if let Some(connection) = &mut connection {
                player.connected(connection)?;
                continue;
            }
        }
        if !player.pace(&frame) {
            player.stats.dropped += 1;
            continue;
        }
        if let Some(live) = &mut connection {
            match player.frame(live, &frame) {
                Ok(true) => player.relayout()?,
                Ok(false) => {}
                Err(e) => device.lost(connection.take().unwrap(), &e),
            }
        }
        player.stats.report(player.audio_level);
    }
    match &mut connection {
        Some(connection) => connection.flush_video(),
        None => Ok(()),
    }
}
