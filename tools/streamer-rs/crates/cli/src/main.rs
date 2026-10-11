mod source;

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use source::{AudioFormat, AudioSource, Input, Output, Source};
use streamer_core::draw::{self, Batch};
use streamer_core::layout::Layout;
use streamer_core::protocol::{self, Encoding, Event, Info, PixelFormat, Target};
use streamer_device::connection::{Address, AudioOut, Connection, Device, TIMEOUT, report};

const STATS_PERIOD: Duration = Duration::from_secs(5);

type Result<T> = std::result::Result<T, String>;

struct PlayOptions {
    input: Input,
    encoding: Encoding,
    front: bool,
    blit: bool,
    audio: Option<AudioFormat>,
    rotation: Option<u32>,
    quality: u32,
}

struct Stream {
    layout: Layout,
    source: Source,
    content: (u32, u32, u32, u32),
}

fn start_stream(options: &PlayOptions, info: &Info, rotation: u32) -> Result<Stream> {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    let size = options.input.probe()?;
    let layout = if options.blit {
        let max = (info.blit_max_width as u32, info.blit_max_height as u32);
        if max.0 < 16 || max.1 < 16 {
            return Err("the device takes no DRAW_BLIT".into());
        }
        Layout::blit(size, rotation, panel, max)?
    } else {
        Layout::direct(size, rotation, panel, info.direct_align.max(1) as u32)
    };
    let output = match options.encoding {
        Encoding::Jpeg => Output::Jpeg {
            quality: options.quality,
        },
        Encoding::Raw | Encoding::Lz4 => Output::Raw(info.pixel_format),
    };
    let source = Source::spawn(
        &options.input,
        &layout,
        panel.0,
        info.max_fps.max(1) as u32,
        output,
    )?;
    let content = layout.content(panel.0);
    let frame = layout.frame(panel.0);
    println!(
        "{}x{} as {}x{} -> {}x{} at {},{}, rotation {rotation}, {} {:?} {:?} into {}",
        size.0,
        size.1,
        frame.0,
        frame.1,
        content.2,
        content.3,
        content.0,
        content.1,
        if options.blit { "BLIT" } else { "DIRECT" },
        options.encoding,
        info.pixel_format,
        if options.front {
            "FRONT"
        } else {
            "BACK + SWAP"
        }
    );
    Ok(Stream {
        layout,
        source,
        content,
    })
}

/// Applies a device event; true when the stream has to start over. `damaged`
/// is set when the letterbox has to be drawn again.
fn apply(
    event: Event,
    info: &mut Info,
    swaps: &mut VecDeque<u32>,
    audio: &mut Option<(u32, u32)>,
    damaged: &mut bool,
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
            *damaged = true;
            false
        }
        Event::Format { format, epoch } => {
            println!("pixel format {format:?}, epoch {epoch}");
            info.pixel_format = format;
            info.epoch = epoch;
            true
        }
        event => {
            report(&event);
            false
        }
    }
}

const AUDIO_CHUNKS_PER_SECOND: u32 = 50;
const AUDIO_TARGET_CHUNKS: u32 = 3;
const AUDIO_POLL: Duration = Duration::from_millis(2);

/// Bulk audio to one connection, keeping about AUDIO_TARGET_CHUNKS queued on
/// the device so its endpoint never fills and stalls the others.
struct AudioSink {
    out: AudioOut,
    pending: Vec<u8>,
    seq: u32,
    reports: u32,
    sent: u32,
}

impl AudioSink {
    fn new(out: AudioOut, format: AudioFormat) -> Self {
        let pending = protocol::message(
            protocol::AUDIO_FORMAT,
            1,
            0,
            &protocol::audio_format(format.rate, format.channels, format.bits),
        );
        let reports = out.level.reports.load(Ordering::Acquire);
        AudioSink {
            out,
            pending,
            seq: 1,
            reports,
            sent: 0,
        }
    }

    /// Drops the chunk when the device has not drained the last ones within
    /// TIMEOUT, so the input keeps its pace.
    fn send(&mut self, chunk: &[u8], chunk_frames: u32) -> Result<()> {
        if self.out.is_closed() {
            return Err("audio: connection closed".into());
        }
        let target = chunk_frames * AUDIO_TARGET_CHUNKS;
        let level = &self.out.level;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let now = level.reports.load(Ordering::Acquire);
            if now != self.reports {
                self.reports = now;
                self.sent = 0;
            }
            if level.buffered.load(Ordering::Relaxed) + self.sent < target {
                break;
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            thread::sleep(AUDIO_POLL);
        }
        self.seq += 1;
        self.pending.extend_from_slice(&protocol::message(
            protocol::AUDIO_DATA,
            self.seq,
            0,
            chunk,
        ));
        self.flush()?;
        self.out.pipe.submit(std::mem::take(&mut self.pending));
        self.sent += chunk_frames;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        while self.out.pipe.pending() > 0 {
            self.out
                .pipe
                .wait(TIMEOUT)
                .map_err(|e| format!("audio: {e}"))?;
        }
        Ok(())
    }
}

/// Reads the input's audio at its own pace and sends it to the latest
/// connection's endpoint from `outs`, if any.
fn stream_audio(input: Input, format: AudioFormat, outs: Receiver<AudioOut>) -> Result<()> {
    let mut source = AudioSource::spawn(&input, format)?;
    let chunk_frames = format.rate / AUDIO_CHUNKS_PER_SECOND;
    let mut chunk = vec![0u8; chunk_frames as usize * format.frame_bytes()];
    let mut sink: Option<AudioSink> = None;
    while source.read(&mut chunk)? {
        while let Ok(out) = outs.try_recv() {
            sink = Some(AudioSink::new(out, format));
        }
        if let Some(active) = &mut sink
            && let Err(e) = active.send(&chunk, chunk_frames)
        {
            eprintln!("{e}");
            sink = None;
        }
    }
    if let Some(active) = &mut sink {
        active.flush()?;
    }
    Ok(())
}

/// What a stream's layout and ffmpeg output were built from.
fn stream_key(info: &Info, rotation: u32) -> (u16, u16, PixelFormat, u32, u16) {
    (
        info.panel_width,
        info.panel_height,
        info.pixel_format,
        rotation,
        info.max_fps,
    )
}

struct Stats {
    start: Instant,
    frames: u32,
    bytes: usize,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start: Instant::now(),
            frames: 0,
            bytes: 0,
        }
    }

    fn frame(&mut self, bytes: usize, audio: Option<(u32, u32)>) {
        self.frames += 1;
        self.bytes += bytes;
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
            "{:.1} frames/s, {:.1} MB/s{audio}",
            self.frames as f64 / seconds,
            self.bytes as f64 / seconds / 1e6
        );
        *self = Stats::new();
    }
}

struct Player {
    options: PlayOptions,
    target: Target,
    info: Info,
    stream: Stream,
    key: (u16, u16, PixelFormat, u32, u16),
    swaps: VecDeque<u32>,
    first: bool,
    audio_level: Option<(u32, u32)>,
    audio_outs: Option<Sender<AudioOut>>,
    next_frame: Instant,
    stats: Stats,
}

impl Player {
    fn rotation(&self, info: &Info) -> u32 {
        self.options.rotation.unwrap_or(info.orientation as u32)
    }

    /// Takes over a new connection; the stream restarts only when the device
    /// differs from the one it was built for.
    fn connected(&mut self, connection: &mut Connection) -> Result<()> {
        self.info = connection.info.clone();
        self.swaps.clear();
        self.first = true;
        self.audio_level = None;
        let key = stream_key(&self.info, self.rotation(&self.info));
        if key != self.key {
            self.restart()?;
        }
        if let (Some(outs), Some(format)) = (&self.audio_outs, self.options.audio) {
            if format.rate > self.info.audio_max_rate {
                eprintln!("the device plays up to {} Hz", self.info.audio_max_rate);
            } else if let Some(out) = connection.take_audio() {
                let _ = outs.send(out);
            }
        }
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        let rotation = self.rotation(&self.info);
        self.stream = start_stream(&self.options, &self.info, rotation)?;
        self.key = stream_key(&self.info, rotation);
        self.first = true;
        Ok(())
    }

    /// Sends one frame; true when it was dropped since the stream has to
    /// restart.
    fn frame(&mut self, connection: &mut Connection, frame: &[u8]) -> Result<bool> {
        let window = self.info.swap_window.max(1) as usize;
        let mut restart = false;
        loop {
            let wait = if !self.options.front && self.swaps.len() >= window {
                TIMEOUT
            } else {
                Duration::ZERO
            };
            match connection.event(wait)? {
                Some(event) => {
                    restart |= apply(
                        event,
                        &mut self.info,
                        &mut self.swaps,
                        &mut self.audio_level,
                        &mut self.first,
                        &self.options,
                    )
                }
                None if wait > Duration::ZERO => return Err("no SWAP_DONE".into()),
                None => break,
            }
        }
        if restart {
            return Ok(true);
        }
        let now = Instant::now();
        if self.next_frame > now {
            thread::sleep(self.next_frame - now);
        }
        let interval = Duration::from_secs_f64(1.0 / self.info.max_fps.max(1) as f64);
        self.next_frame = (self.next_frame + interval).max(now);

        let mut batch = Batch::new(self.info.max_message[protocol::VIDEO] as usize);
        let mut messages = Vec::new();
        if !self.options.front || self.first {
            messages = draw::letterbox(self.target, &self.info, self.stream.content);
        }
        self.first = false;
        messages.extend(draw::frame_messages(
            &self.stream.layout,
            self.options.encoding,
            self.target,
            &self.info,
            frame,
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
        let mut bytes = 0;
        for transfer in batch.transfers {
            bytes += transfer.len();
            connection.send_video(transfer)?;
        }
        self.stats.frame(bytes, self.audio_level);
        Ok(false)
    }
}

/// Streams until the input ends. Frames that come while the device is away
/// are dropped, so the input keeps its pace.
fn play(device: &Device, options: PlayOptions) -> Result<()> {
    let mut connection = device.wait();
    let info = match &connection {
        Some(connection) => connection.info.clone(),
        None => return Err("no device".into()),
    };
    let rotation = options.rotation.unwrap_or(info.orientation as u32);
    let stream = start_stream(&options, &info, rotation)?;
    let audio_outs = match options.audio {
        Some(format) => {
            let (sender, outs) = mpsc::channel();
            let input = options.input.clone();
            thread::spawn(move || {
                if let Err(e) = stream_audio(input, format, outs) {
                    eprintln!("{e}");
                }
            });
            Some(sender)
        }
        None => None,
    };
    let mut player = Player {
        target: if options.front {
            Target::Front
        } else {
            Target::Back
        },
        key: stream_key(&info, rotation),
        info,
        stream,
        options,
        swaps: VecDeque::new(),
        first: true,
        audio_level: None,
        audio_outs,
        next_frame: Instant::now(),
        stats: Stats::new(),
    };
    if let Some(connection) = &mut connection {
        player.connected(connection)?;
    }

    while let Some(frame) = player.stream.source.next_frame()? {
        if connection.is_none() {
            connection = device.poll();
            if let Some(connection) = &mut connection {
                player.connected(connection)?;
                continue;
            }
        }
        let Some(live) = &mut connection else {
            continue;
        };
        match player.frame(live, &frame) {
            Ok(true) => player.restart()?,
            Ok(false) => {}
            Err(e) => device.lost(connection.take().unwrap(), &e),
        }
    }
    match &mut connection {
        Some(connection) => connection.flush_video(),
        None => Ok(()),
    }
}

fn volume(connection: &mut Connection, db: f64, mute: bool) -> Result<()> {
    let payload = protocol::set_volume(protocol::BULK, mute, (db * 256.0).round() as i16);
    connection.control(protocol::SET_VOLUME, &payload)?;
    loop {
        match connection.event(TIMEOUT)? {
            Some(Event::Volume { mute, volume, .. }) => {
                println!(
                    "bulk volume {:.1} dB{}",
                    volume as f64 / 256.0,
                    if mute { ", muted" } else { "" }
                );
                return Ok(());
            }
            Some(event) => report(&event),
            None => return Err("no EV_VOLUME".into()),
        }
    }
}

fn fill(connection: &mut Connection, color: u32) -> Result<()> {
    let info = connection.info.clone();
    let payload = protocol::fill(
        Target::Front,
        0,
        0,
        info.panel_width,
        info.panel_height,
        color,
    );
    let seq = connection.next_video_seq();
    connection.send_video(protocol::message(protocol::FILL, seq, info.epoch, &payload))?;
    connection.flush_video()?;
    while let Some(event) = connection.event(Duration::from_millis(200))? {
        report(&event);
    }
    Ok(())
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: streamer [--usbip <host:port>] <command>
commands:
  info                       print the device's INFO
  fill <rrggbb>              fill the panel with a color
  volume <db> [mute]         set the bulk audio volume
  play [options] <input>     stream a video through ffmpeg
    --encoding jpeg|raw|lz4  default jpeg
    --front                  draw into FRONT without swapping
    --blit                   send the source as is; the device rotates and scales
    --audio [rate:ch:bits]   also send the input's audio, default 48000:2:16
    --rotate <0-3>|auto      counter-clockwise quarter turns, or the device's
    --quality <2-31>         ffmpeg mjpeg -q:v, default 4
    --format <fmt>           ffmpeg input format, e.g. lavfi
    --loop                   repeat the input
    --no-realtime            read the input as fast as it decodes"
    );
    ExitCode::from(2)
}

fn parse_play(args: &[String]) -> Option<PlayOptions> {
    let mut options = PlayOptions {
        input: Input {
            path: String::new(),
            format: None,
            realtime: true,
            looped: false,
        },
        encoding: Encoding::Jpeg,
        front: false,
        blit: false,
        audio: None,
        rotation: Some(0),
        quality: 4,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--encoding" => {
                options.encoding = match args.next()?.as_str() {
                    "jpeg" => Encoding::Jpeg,
                    "raw" => Encoding::Raw,
                    "lz4" => Encoding::Lz4,
                    _ => return None,
                }
            }
            "--front" => options.front = true,
            "--blit" => options.blit = true,
            "--audio" => {
                let mut format = AudioFormat {
                    rate: 48000,
                    channels: 2,
                    bits: 16,
                };
                if let Some(spec) = args.clone().next().filter(|a| a.contains(':')) {
                    args.next();
                    let mut parts = spec.split(':').map(|v| v.parse::<u32>().ok());
                    format.rate = parts.next()??;
                    format.channels = parts.next()?? as u8;
                    format.bits = parts.next()?? as u8;
                }
                options.audio = Some(format);
            }
            "--rotate" => {
                options.rotation = match args.next()?.as_str() {
                    "auto" => None,
                    value => Some(value.parse().ok().filter(|r| *r < 4)?),
                }
            }
            "--quality" => options.quality = args.next()?.parse().ok()?,
            "--format" => options.input.format = Some(args.next()?.clone()),
            "--loop" => options.input.looped = true,
            "--no-realtime" => options.input.realtime = false,
            path if !path.starts_with("--") && options.input.path.is_empty() => {
                options.input.path = path.into()
            }
            _ => return None,
        }
    }
    (!options.input.path.is_empty()).then_some(options)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut usbip = None;
    if args.first().map(String::as_str) == Some("--usbip") {
        if args.len() < 2 {
            return usage();
        }
        usbip = Some(args[1].clone());
        args.drain(..2);
    }
    enum Command {
        Info,
        Fill(u32),
        Volume(f64, bool),
        Play(PlayOptions),
    }
    let command = match (args.first().map(String::as_str), &args[1.min(args.len())..]) {
        (Some("info"), []) => Command::Info,
        (Some("fill"), [color]) => match u32::from_str_radix(color, 16) {
            Ok(color) => Command::Fill(color),
            Err(_) => return usage(),
        },
        (Some("volume"), [db]) | (Some("volume"), [db, _]) => match db.parse() {
            Ok(db) => Command::Volume(db, args.get(2).map(String::as_str) == Some("mute")),
            Err(_) => return usage(),
        },
        (Some("play"), rest) => match parse_play(rest) {
            Some(options) => Command::Play(options),
            None => return usage(),
        },
        _ => return usage(),
    };

    let address = match usbip {
        Some(address) => Address::Usbip(address),
        None => Address::Usb,
    };
    let result = match command {
        Command::Play(options) => play(&Device::spawn(address), options),
        command => Connection::open(&address).and_then(|mut connection| match command {
            Command::Info => {
                println!("{:#?}", connection.info);
                Ok(())
            }
            Command::Fill(color) => fill(&mut connection, color),
            Command::Volume(db, mute) => volume(&mut connection, db, mute),
            Command::Play(_) => unreachable!(),
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
