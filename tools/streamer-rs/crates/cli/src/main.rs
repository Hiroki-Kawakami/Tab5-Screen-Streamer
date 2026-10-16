mod audio;
mod clock;
mod media;
mod play;
mod signal;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use media::{AudioFormat, Input};
use play::PlayOptions;
use streamer_capture as capture;
use streamer_core::protocol::{self, Encoding, Event, Target};
use streamer_device::connection::{Address, Connection, Device, TIMEOUT, report};
use streamer_stream::capture::CaptureOptions;
use streamer_stream::present::Quality;
use streamer_stream::{Control, Notice, Notify};

type Result<T> = std::result::Result<T, String>;

fn print() -> Notify {
    Arc::new(|notice| match notice {
        Notice::Connected(_) | Notice::Disconnected(_) => {}
        Notice::Warning(warning) => eprintln!("{warning}"),
        notice => println!("{notice}"),
    })
}

fn list() -> Result<()> {
    if !capture::authorize() {
        return Err("screen capture is not permitted".into());
    }
    let main = capture::main_display()?.id;
    for (index, display) in capture::displays()?.iter().enumerate() {
        println!(
            "{index}: {}x{}{}",
            display.size.0,
            display.size.1,
            if display.id == main { " (main)" } else { "" }
        );
    }
    Ok(())
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
  play [options] -           stream Matroska from stdin: raw I420 video in
                             BT.601 full range, and PCM audio if any, e.g.
                             ffmpeg -i <input> -vf scale=w:h:out_range=full:
                             out_color_matrix=bt601 -pix_fmt yuv420p
                             -c:v rawvideo -c:a pcm_s16le -f matroska -
    --encoding jpeg|raw|lz4  default jpeg
    --front                  draw into FRONT without swapping
    --blit                   send the source as is; the device rotates and scales
    --audio <rate:ch:bits>   format of the input's audio, default 48000:2:16;
                             stdin keeps its own
    --no-audio               send no audio
    --rotate <0-3>|auto      counter-clockwise quarter turns, or the device's
                             orientation (default auto)
    --quality <1-100>        fixed JPEG quality; with --max-rate, its ceiling
    --max-rate <MB/s>        lower the JPEG quality from 80 to keep the video
                             near this rate, default 10
    --format <fmt>           ffmpeg input format, e.g. lavfi
    --loop                   repeat the input
    --no-realtime            send frames as fast as they come
    -v, --verbose            print stats every second
  displays                   list the displays capture takes
  capture [options]          stream a display
    --display <n>            index from displays, default the main display
    --virtual                a virtual display for the device instead (macOS),
                             present while it is connected
    --rotate <0-3>|auto      as for play, default auto
    --quality <1-100>        as for play
    --max-rate <MB/s>        as for play
    -v, --verbose            as for play"
    );
    ExitCode::from(2)
}

fn parse_rotation(value: &str) -> Option<Option<u32>> {
    match value {
        "auto" => Some(None),
        value => Some(Some(value.parse().ok().filter(|r| *r < 4)?)),
    }
}

fn parse_quality(value: &str) -> Option<u8> {
    value.parse().ok().filter(|q| (1..=100).contains(q))
}

fn parse_rate(value: &str) -> Option<f64> {
    value
        .parse()
        .ok()
        .filter(|r: &f64| r.is_finite() && *r > 0.0)
}

fn quality(quality: Option<u8>, max_rate: Option<f64>) -> Quality {
    Quality {
        ceiling: quality.unwrap_or(Quality::DEFAULT.ceiling),
        max_rate: match (quality, max_rate) {
            (Some(_), None) => None,
            (_, rate) => rate.map(|rate| rate * 1e6).or(Quality::DEFAULT.max_rate),
        },
    }
}

fn parse_capture(args: &[String]) -> Option<CaptureOptions> {
    let mut options = CaptureOptions {
        display: None,
        virtual_display: false,
        rotation: None,
        quality: quality(None, None),
        touch: true,
        verbose: false,
    };
    let (mut fixed, mut max_rate) = (None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--display" => options.display = Some(args.next()?.parse().ok()?),
            "--virtual" => options.virtual_display = true,
            "--rotate" => options.rotation = parse_rotation(args.next()?)?,
            "--quality" => fixed = Some(parse_quality(args.next()?)?),
            "--max-rate" => max_rate = Some(parse_rate(args.next()?)?),
            "-v" | "--verbose" => options.verbose = true,
            _ => return None,
        }
    }
    options.quality = quality(fixed, max_rate);
    Some(options)
}

fn parse_play(args: &[String]) -> Option<PlayOptions> {
    let mut options = PlayOptions {
        input: Input {
            path: String::new(),
            format: None,
            looped: false,
        },
        encoding: Encoding::Jpeg,
        front: false,
        blit: false,
        audio: Some(AudioFormat {
            rate: 48000,
            channels: 2,
            bits: 16,
        }),
        rotation: None,
        quality: quality(None, None),
        realtime: true,
        verbose: false,
    };
    let (mut fixed, mut max_rate) = (None, None);
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
                let mut parts = args.next()?.split(':').map(|v| v.parse::<u32>().ok());
                options.audio = Some(AudioFormat {
                    rate: parts.next()??,
                    channels: parts.next()?? as u8,
                    bits: parts.next()?? as u8,
                });
            }
            "--no-audio" => options.audio = None,
            "--rotate" => options.rotation = parse_rotation(args.next()?)?,
            "--quality" => fixed = Some(parse_quality(args.next()?)?),
            "--max-rate" => max_rate = Some(parse_rate(args.next()?)?),
            "--format" => options.input.format = Some(args.next()?.clone()),
            "--loop" => options.input.looped = true,
            "--no-realtime" => options.realtime = false,
            "-v" | "--verbose" => options.verbose = true,
            path if (path == "-" || !path.starts_with("-")) && options.input.path.is_empty() => {
                options.input.path = path.into()
            }
            _ => return None,
        }
    }
    options.quality = quality(fixed, max_rate);
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
        Displays,
        Capture(CaptureOptions),
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
        (Some("displays"), []) => Command::Displays,
        (Some("capture"), rest) => match parse_capture(rest) {
            Some(options) => Command::Capture(options),
            None => return usage(),
        },
        _ => return usage(),
    };

    let address = match usbip {
        Some(address) => Address::Usbip(address),
        None => Address::Usb,
    };
    if matches!(command, Command::Play(_) | Command::Capture(_)) {
        signal::install();
    }
    let result = match command {
        Command::Play(options) => play::play(&Device::spawn(address), options),
        Command::Displays => list(),
        Command::Capture(options) => {
            let control = Control {
                stop: signal::flag(),
                requests: None,
                notify: print(),
            };
            streamer_stream::capture::capture(&Device::spawn(address), options, &control)
        }
        command => Connection::open(&address).and_then(|mut connection| {
            match command {
                Command::Info => println!("{:#?}", connection.info),
                Command::Fill(color) => fill(&mut connection, color)?,
                Command::Volume(db, mute) => volume(&mut connection, db, mute)?,
                _ => unreachable!(),
            }
            connection.close()
        }),
    };
    signal::finished();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
