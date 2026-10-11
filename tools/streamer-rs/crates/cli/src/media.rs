//! I420 frames and PCM as live Matroska, from stdin or from an ffmpeg we run.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use streamer_core::container::demux::{Codec, TrackKind};
use streamer_core::container::io::Stream;
use streamer_core::container::mkv::Demuxer;

const FRAME_QUEUE: usize = 2;

#[derive(Clone)]
pub struct Input {
    pub path: String,
    pub format: Option<String>,
    pub looped: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AudioFormat {
    pub rate: u32,
    pub channels: u8,
    pub bits: u8,
}

impl AudioFormat {
    pub fn frame_bytes(&self) -> usize {
        self.channels as usize * self.bits as usize / 8
    }
}

/// Times are microseconds of the input.
pub struct Frame {
    pub pts: i64,
    pub data: Vec<u8>,
}

pub struct Media {
    child: Option<Child>,
    pub size: (u32, u32),
    pub audio: Option<AudioFormat>,
    pub frames: Receiver<Frame>,
}

impl Input {
    pub fn is_stdin(&self) -> bool {
        self.path == "-"
    }

    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(format) = &self.format {
            args.extend(["-f".into(), format.clone()]);
        }
        args.extend(["-i".into(), self.path.clone()]);
        args
    }

    /// The size the video is shown at: pixel aspect ratio applied, then the
    /// display rotation ffmpeg applies when decoding.
    pub fn probe(&self) -> Result<(u32, u32), String> {
        let output = Command::new("ffprobe")
            .args(["-v", "error"])
            .args(self.args())
            .args([
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,sample_aspect_ratio:stream_side_data=rotation",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .output()
            .map_err(|e| format!("ffprobe: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .map(str::trim)
        };
        let value = |key: &str| field(key).and_then(|v| v.parse::<i64>().ok());
        let sar = field("sample_aspect_ratio")
            .and_then(|v| v.split_once(':'))
            .and_then(|(n, d)| Some((n.parse::<f64>().ok()?, d.parse::<f64>().ok()?)))
            .filter(|&(n, d)| n > 0.0 && d > 0.0)
            .map_or(1.0, |(n, d)| n / d);
        match (value("width"), value("height")) {
            (Some(w), Some(h)) if w > 0 && h > 0 => {
                let quarter = value("rotation").unwrap_or(0).rem_euclid(180) == 90;
                let (w, h) = (((w as f64 * sar).round() as u32).max(1), h as u32);
                Ok(if quarter { (h, w) } else { (w, h) })
            }
            _ => Err(format!(
                "ffprobe: no video size ({})",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        }
    }
}

impl Media {
    /// Runs ffmpeg on `input` from `start` µs, scaled to `size`, with audio
    /// in `audio` when given. Frame times continue from `start`.
    pub fn spawn(
        input: &Input,
        size: (u32, u32),
        audio: Option<AudioFormat>,
        start: i64,
        audio_out: Option<SyncSender<Frame>>,
    ) -> Result<Self, String> {
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if input.looped {
            command.args(["-stream_loop", "-1"]);
        }
        if start > 0 {
            command.args(["-ss", &format!("{:.6}", start as f64 / 1e6)]);
        }
        let filter = format!(
            "scale={}:{}:out_range=full:out_color_matrix=bt601,setsar=1",
            size.0, size.1
        );
        command
            .args(input.args())
            .args(["-map", "0:v:0", "-vf", &filter])
            .args(["-pix_fmt", "yuv420p", "-c:v", "rawvideo"]);
        match audio {
            Some(format) => {
                let codec = match format.bits {
                    16 => "pcm_s16le",
                    24 => "pcm_s24le",
                    32 => "pcm_s32le",
                    bits => return Err(format!("{bits} bit audio")),
                };
                command
                    .args(["-map", "0:a:0?", "-c:a", codec])
                    .args(["-ar", &format.rate.to_string()])
                    .args(["-ac", &format.channels.to_string()]);
            }
            None => {
                command.arg("-an");
            }
        }
        let mut child = command
            .args(["-sn", "-f", "matroska", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        let mut media = open(stdout, start, audio_out)?;
        media.child = Some(child);
        Ok(media)
    }

    pub fn stdin(audio_out: Option<SyncSender<Frame>>) -> Result<Self, String> {
        open(std::io::stdin(), 0, audio_out)
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Reads the header, then demuxes on a thread: video into `frames`, audio
/// into `audio_out`, dropped when that is full so audio never holds up video.
fn open(
    reader: impl Read + Send + 'static,
    offset: i64,
    audio_out: Option<SyncSender<Frame>>,
) -> Result<Media, String> {
    let mut demuxer = Demuxer::open(Stream::new(reader)).map_err(|e| format!("input: {e}"))?;
    let info = demuxer.info();
    let mut video = None;
    let mut audio = None;
    for track in &info.tracks {
        match &track.kind {
            TrackKind::Video(v) if video.is_none() => {
                if track.codec != Codec::Raw || v.fourcc != Some(*b"I420") {
                    return Err(format!("input: video is {}, not raw I420", track.tag));
                }
                if v.width % 2 == 1 || v.height % 2 == 1 {
                    return Err(format!("input: {}x{} is not even", v.width, v.height));
                }
                video = Some((track.index, (v.width, v.height)));
            }
            TrackKind::Audio(a) if audio.is_none() => {
                let bits = a.bits.unwrap_or(16);
                if track.tag != "A_PCM/INT/LIT" || !matches!(bits, 16 | 24 | 32) {
                    return Err(format!("input: audio is {} {bits} bit", track.tag));
                }
                let format = AudioFormat {
                    rate: a.sample_rate,
                    channels: a.channels as u8,
                    bits: bits as u8,
                };
                audio = Some((track.index, format));
            }
            _ => {}
        }
    }
    let (video_track, size) = video.ok_or("input: no video")?;
    let audio_out = audio_out.filter(|_| audio.is_some());
    let audio_track = audio.map(|(index, _)| index);
    let mut selected = vec![video_track];
    selected.extend(audio_track.filter(|_| audio_out.is_some()));
    demuxer.select(&selected);
    let frame_bytes = size.0 as usize * size.1 as usize * 3 / 2;

    let (sender, frames) = mpsc::sync_channel(FRAME_QUEUE);
    thread::spawn(move || {
        loop {
            let packet = match demuxer.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return,
                Err(e) => {
                    eprintln!("input: {e}");
                    return;
                }
            };
            let frame = Frame {
                pts: packet.pts + offset,
                data: packet.data,
            };
            if packet.track == video_track {
                if frame.data.len() != frame_bytes {
                    eprintln!("input: {} byte frame", frame.data.len());
                    return;
                }
                if sender.send(frame).is_err() {
                    return;
                }
            } else if let Some(out) = &audio_out
                && let Err(TrySendError::Disconnected(_)) = out.try_send(frame)
            {
                return;
            }
        }
    });
    Ok(Media {
        child: None,
        size,
        audio: audio.map(|(_, format)| format),
        frames,
    })
}
