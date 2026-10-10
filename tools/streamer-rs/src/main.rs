mod link;
mod protocol;
mod source;
mod usbip;

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use link::{InPipe, Link, OutPipe};
use protocol::{Encoding, Event, Info, Target};
use source::{Input, Output, Source};

const IN_BYTES: usize = 512;
const IN_QUEUE: usize = 2;
const VIDEO_QUEUE: usize = 2;
const TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);
const STATS_PERIOD: Duration = Duration::from_secs(5);

type Result<T> = std::result::Result<T, String>;

struct Session {
    control_out: Box<dyn OutPipe>,
    video_out: Box<dyn OutPipe>,
    events: Receiver<Event>,
    control_seq: u32,
    video_seq: u32,
}

fn read_events(mut input: Box<dyn InPipe>, events: mpsc::Sender<Event>) {
    for _ in 0..IN_QUEUE {
        input.submit(IN_BYTES);
    }
    loop {
        match input.wait(POLL) {
            Ok(data) => {
                for event in protocol::parse_events(&data) {
                    if events.send(event).is_err() {
                        return;
                    }
                }
                input.submit(IN_BYTES);
            }
            Err(link::Error::Timeout) => {}
            Err(e) => {
                eprintln!("control in: {e}");
                return;
            }
        }
    }
}

impl Session {
    fn new(link: Link) -> Self {
        let (sender, events) = mpsc::channel();
        let control_in = link.control_in;
        thread::spawn(move || read_events(control_in, sender));
        Session {
            control_out: link.control_out,
            video_out: link.video_out,
            events,
            control_seq: 0,
            video_seq: 0,
        }
    }

    fn info(&mut self) -> Result<Info> {
        self.control_seq += 1;
        let seq = self.control_seq;
        self.control_out
            .submit(protocol::message(protocol::GET_INFO, seq, 0, &[]));
        self.control_out
            .wait(TIMEOUT)
            .map_err(|e| format!("get info: {e}"))?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Info(info)) => return Ok(info),
                Ok(event) => report(&event),
                Err(_) => return Err("no INFO".into()),
            }
        }
    }

    fn next_video_seq(&mut self) -> u32 {
        self.video_seq += 1;
        self.video_seq
    }

    fn send_video(&mut self, transfer: Vec<u8>) -> Result<()> {
        while self.video_out.pending() >= VIDEO_QUEUE {
            self.video_out
                .wait(TIMEOUT)
                .map_err(|e| format!("video: {e}"))?;
        }
        self.video_out.submit(transfer);
        Ok(())
    }

    fn flush_video(&mut self) -> Result<()> {
        while self.video_out.pending() > 0 {
            self.video_out
                .wait(TIMEOUT)
                .map_err(|e| format!("video: {e}"))?;
        }
        Ok(())
    }
}

fn report(event: &Event) {
    if let Event::Error { seq, channel, code } = event {
        eprintln!(
            "device: {} on channel {channel} seq {seq}",
            protocol::error_name(*code)
        );
    }
}

/// Packs messages into transfers of at most `limit` bytes.
struct Batch {
    limit: usize,
    transfers: Vec<Vec<u8>>,
}

impl Batch {
    fn new(limit: usize) -> Self {
        Batch {
            limit,
            transfers: vec![Vec::new()],
        }
    }

    fn push(&mut self, message: Vec<u8>) -> Result<()> {
        if message.len() > self.limit {
            return Err(format!(
                "{} byte message over the {} byte limit",
                message.len(),
                self.limit
            ));
        }
        if self.transfers.last().unwrap().len() + message.len() > self.limit {
            self.transfers.push(Vec::new());
        }
        self.transfers
            .last_mut()
            .unwrap()
            .extend_from_slice(&message);
        Ok(())
    }
}

struct PlayOptions {
    input: Input,
    encoding: Encoding,
    front: bool,
    rotation: u32,
    quality: u32,
}

fn letterbox(
    session: &mut Session,
    batch: &mut Batch,
    target: Target,
    info: &Info,
    y: u32,
    height: u32,
) -> Result<()> {
    let width = info.panel_width;
    let bottom = y + height;
    for (top, rows) in [(0, y), (bottom, info.panel_height as u32 - bottom)] {
        if rows == 0 {
            continue;
        }
        let payload = protocol::fill(target, 0, top as u16, width, rows as u16, 0);
        let seq = session.next_video_seq();
        batch.push(protocol::message(protocol::FILL, seq, info.epoch, &payload))?;
    }
    Ok(())
}

fn play(session: &mut Session, options: PlayOptions) -> Result<()> {
    let info = session.info()?;
    let align = info.direct_align.max(1) as u32;
    let panel = (info.panel_width as u32, info.panel_height as u32);
    let size = options.input.probe()?;
    let geometry = source::fit(size, options.rotation, panel, align);
    let output = match options.encoding {
        Encoding::Jpeg => Output::Jpeg {
            quality: options.quality,
        },
        Encoding::Raw => Output::Raw(info.pixel_format),
    };
    let mut source = Source::spawn(
        &options.input,
        &geometry,
        panel.0,
        options.rotation,
        info.max_fps.max(1) as u32,
        output,
    )?;
    println!(
        "{}x{} -> {}x{} at y {}, {:?} into {}",
        size.0,
        size.1,
        panel.0,
        geometry.height,
        geometry.y,
        options.encoding,
        if options.front {
            "FRONT"
        } else {
            "BACK + SWAP"
        }
    );

    let limit = info.max_message[protocol::VIDEO] as usize;
    let target = if options.front {
        Target::Front
    } else {
        Target::Back
    };
    let frame_interval = Duration::from_secs_f64(1.0 / info.max_fps.max(1) as f64);
    let row_bytes = panel.0 as usize * info.pixel_format.bytes();
    let band_rows = ((limit - protocol::HEADER_BYTES - 6) / row_bytes) as u32 / align * align;
    let mut swaps: VecDeque<u32> = VecDeque::new();
    let mut next_frame = Instant::now();
    let mut first = true;
    let mut stats_start = Instant::now();
    let mut stats_frames = 0u32;
    let mut stats_bytes = 0usize;

    while let Some(frame) = source.next_frame()? {
        if !options.front {
            loop {
                match session.events.recv_timeout(
                    if swaps.len() >= info.swap_window.max(1) as usize {
                        TIMEOUT
                    } else {
                        Duration::ZERO
                    },
                ) {
                    Ok(Event::SwapDone { seq }) => {
                        while swaps.front().is_some_and(|&s| s <= seq) {
                            swaps.pop_front();
                        }
                    }
                    Ok(event) => report(&event),
                    Err(RecvTimeoutError::Timeout)
                        if swaps.len() >= info.swap_window.max(1) as usize =>
                    {
                        return Err("no SWAP_DONE".into())
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return Err("control in closed".into()),
                }
            }
        } else {
            while let Ok(event) = session.events.try_recv() {
                report(&event);
            }
        }
        let now = Instant::now();
        if next_frame > now {
            thread::sleep(next_frame - now);
        }
        next_frame = (next_frame + frame_interval).max(now);

        let mut batch = Batch::new(limit);
        if !options.front || first {
            letterbox(
                session,
                &mut batch,
                target,
                &info,
                geometry.y,
                geometry.height,
            )?;
        }
        first = false;
        match options.encoding {
            Encoding::Jpeg => {
                let payload = protocol::draw_direct(
                    target,
                    Encoding::Jpeg,
                    geometry.y as u16,
                    geometry.height as u16,
                    &frame,
                );
                let seq = session.next_video_seq();
                batch.push(protocol::message(
                    protocol::DRAW_DIRECT,
                    seq,
                    info.epoch,
                    &payload,
                ))?;
            }
            Encoding::Raw => {
                let mut row = 0;
                while row < geometry.height {
                    let rows = band_rows.min(geometry.height - row);
                    let data = &frame[row as usize * row_bytes..(row + rows) as usize * row_bytes];
                    let payload = protocol::draw_direct(
                        target,
                        Encoding::Raw,
                        (geometry.y + row) as u16,
                        rows as u16,
                        data,
                    );
                    let seq = session.next_video_seq();
                    batch.push(protocol::message(
                        protocol::DRAW_DIRECT,
                        seq,
                        info.epoch,
                        &payload,
                    ))?;
                    row += rows;
                }
            }
        }
        if !options.front {
            let seq = session.next_video_seq();
            batch.push(protocol::message(protocol::SWAP, seq, info.epoch, &[0; 4]))?;
            swaps.push_back(seq);
        }
        for transfer in batch.transfers {
            stats_bytes += transfer.len();
            session.send_video(transfer)?;
        }

        stats_frames += 1;
        let elapsed = stats_start.elapsed();
        if elapsed >= STATS_PERIOD {
            let seconds = elapsed.as_secs_f64();
            println!(
                "{:.1} frames/s, {:.1} MB/s",
                stats_frames as f64 / seconds,
                stats_bytes as f64 / seconds / 1e6
            );
            stats_start = Instant::now();
            stats_frames = 0;
            stats_bytes = 0;
        }
    }
    session.flush_video()
}

fn fill(session: &mut Session, color: u32) -> Result<()> {
    let info = session.info()?;
    let payload = protocol::fill(
        Target::Front,
        0,
        0,
        info.panel_width,
        info.panel_height,
        color,
    );
    let seq = session.next_video_seq();
    session.send_video(protocol::message(protocol::FILL, seq, info.epoch, &payload))?;
    session.flush_video()?;
    thread::sleep(Duration::from_millis(200));
    while let Ok(event) = session.events.try_recv() {
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
  play [options] <input>     stream a video through ffmpeg
    --encoding jpeg|raw      default jpeg
    --front                  draw into FRONT without swapping
    --rotate <0-3>           counter-clockwise quarter turns
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
        rotation: 0,
        quality: 4,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--encoding" => {
                options.encoding = match args.next()?.as_str() {
                    "jpeg" => Encoding::Jpeg,
                    "raw" => Encoding::Raw,
                    _ => return None,
                }
            }
            "--front" => options.front = true,
            "--rotate" => options.rotation = args.next()?.parse().ok().filter(|r| *r < 4)?,
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
        Play(PlayOptions),
    }
    let command = match (args.first().map(String::as_str), &args[1.min(args.len())..]) {
        (Some("info"), []) => Command::Info,
        (Some("fill"), [color]) => match u32::from_str_radix(color, 16) {
            Ok(color) => Command::Fill(color),
            Err(_) => return usage(),
        },
        (Some("play"), rest) => match parse_play(rest) {
            Some(options) => Command::Play(options),
            None => return usage(),
        },
        _ => return usage(),
    };

    let link = match &usbip {
        Some(address) => usbip::connect(address),
        None => link::open_nusb(),
    };
    let mut session = match link {
        Ok(link) => Session::new(link),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match command {
        Command::Info => session.info().map(|info| println!("{info:#?}")),
        Command::Fill(color) => fill(&mut session, color),
        Command::Play(options) => play(&mut session, options),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
