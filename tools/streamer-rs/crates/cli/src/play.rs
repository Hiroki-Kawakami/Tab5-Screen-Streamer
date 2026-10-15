//! Streaming a video from ffmpeg: pacing and encoding its frames.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::thread;
use std::time::Duration;

use streamer_core::convert::{self, I420};
use streamer_core::draw::Encoded;
use streamer_core::jpeg::{Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::protocol::{Encoding, Info};
use streamer_device::connection::{AudioOut, Connection, Device};

use crate::audio;
use crate::clock::Clock;
use crate::media::{AudioFormat, Frame, Input, Media};
use crate::present::{self, LayoutKey, Presenter};
use crate::signal;

const AUDIO_QUEUE: usize = 64;
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

fn encode(frame: &Frame, size: (u32, u32), presenter: &Presenter, quality: u8) -> Encoded {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let info = &presenter.info;
    let layout = &presenter.layout;
    if presenter.encoding != Encoding::Jpeg {
        let i420 = I420 {
            width: w,
            height: h,
            data: &frame.data,
        };
        let placement = layout.placement(info.panel_width as u32);
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
    present::encode_jpeg(&image, layout, info, quality)
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

struct Player {
    options: PlayOptions,
    presenter: Presenter,
    media: Media,
    key: LayoutKey,
    probed: Option<(u32, u32)>,
    audio_packets: Option<SyncSender<Frame>>,
    audio_outs: Option<Sender<AudioOut>>,
    clock: Arc<Clock>,
    last_pts: i64,
}

impl Player {
    /// Lays the input out again for the current device and rotation. ffmpeg
    /// restarts where it was, since it does the scaling; stdin cannot be
    /// scaled, and keeps its layout when the new one does not fit.
    fn relayout(&mut self) -> Result<()> {
        let rotation = self.presenter.rotation();
        let info = &self.presenter.info;
        let key = present::layout_key(info, rotation);
        if key == self.key {
            return Ok(());
        }
        match self.probed {
            Some(size) => {
                let layout = layout(&self.options, info, size, rotation, true)?;
                let start = self.last_pts.max(0);
                self.media = Media::spawn(
                    &self.options.input,
                    layout.frame(),
                    self.options.audio,
                    start,
                    self.audio_packets.clone(),
                )?;
                self.presenter.layout = layout;
            }
            None => match layout(&self.options, info, self.media.size, rotation, false) {
                Ok(layout) => self.presenter.layout = layout,
                Err(e) => {
                    eprintln!("keeping the layout: {e}");
                    return Ok(());
                }
            },
        }
        self.key = key;
        self.presenter.clear();
        self.presenter.describe();
        Ok(())
    }

    fn connected(&mut self, connection: &mut Connection) -> Result<()> {
        self.presenter.connected(connection.info.clone());
        self.relayout()?;
        let info = &self.presenter.info;
        if let (Some(outs), Some(format)) = (&self.audio_outs, self.media.audio) {
            if format.rate > info.audio_max_rate {
                eprintln!("the device plays up to {} Hz", info.audio_max_rate);
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
            } else if -ahead > LATE_US && !self.presenter.redraw {
                return false;
            }
        }
        self.presenter.due()
    }

    /// Sends one frame; true when the layout has to be rebuilt first.
    fn frame(&mut self, connection: &mut Connection, frame: &Frame) -> Result<bool> {
        if self.presenter.events(connection)? {
            return Ok(true);
        }
        self.presenter.start_frame();
        let encoded = encode(
            frame,
            self.media.size,
            &self.presenter,
            self.options.quality,
        );
        self.presenter.send(connection, encoded)?;
        Ok(false)
    }
}

/// Streams until the input ends or on stopping. While the device is away
/// frames are paced and dropped, so the input keeps its time.
pub fn play(device: &Device, options: PlayOptions) -> Result<()> {
    let mut connection = crate::connect(device);
    let info = match &connection {
        Some(connection) => connection.info.clone(),
        None => return Ok(()),
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
    let presenter = Presenter::new(
        info.clone(),
        layout,
        options.encoding,
        options.front,
        options.rotation,
    );
    let mut player = Player {
        key: present::layout_key(&info, rotation),
        presenter,
        media,
        probed,
        options,
        audio_packets,
        audio_outs,
        clock,
        last_pts: 0,
    };
    player.presenter.describe();
    if let Some(connection) = &mut connection {
        player.connected(connection)?;
    }

    while !signal::stopping() {
        let frame = match player.media.frames.recv_timeout(crate::STOP_POLL) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        player.last_pts = frame.pts;
        if connection.is_none() {
            connection = device.poll();
            if let Some(connection) = &mut connection {
                player.connected(connection)?;
                continue;
            }
        }
        if !player.pace(&frame) {
            player.presenter.stats.dropped += 1;
            continue;
        }
        if let Some(live) = &mut connection {
            match player.frame(live, &frame) {
                Ok(true) => player.relayout()?,
                Ok(false) => {}
                Err(e) => device.lost(connection.take().unwrap(), &e),
            }
        }
        let audio_level = player.presenter.audio_level;
        player.presenter.stats.report(audio_level);
    }
    match connection {
        Some(connection) => connection.close(),
        None => Ok(()),
    }
}
