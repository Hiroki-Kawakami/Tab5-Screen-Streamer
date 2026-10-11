//! Frames from ffmpeg, fitted to the panel.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};

use streamer_core::layout::Layout;
use streamer_core::protocol::PixelFormat;

#[derive(Clone)]
pub struct Input {
    pub path: String,
    pub format: Option<String>,
    pub realtime: bool,
    pub looped: bool,
}

pub enum Output {
    Jpeg { quality: u32 },
    Raw(PixelFormat),
}

impl Input {
    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(format) = &self.format {
            args.extend(["-f".into(), format.clone()]);
        }
        args.extend(["-i".into(), self.path.clone()]);
        args
    }

    pub fn probe(&self) -> Result<(u32, u32), String> {
        let output = Command::new("ffprobe")
            .args(["-v", "error"])
            .args(self.args())
            .args([
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .output()
            .map_err(|e| format!("ffprobe: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut size = text.trim().split(',').map(|v| v.trim().parse::<u32>());
        match (size.next(), size.next()) {
            (Some(Ok(w)), Some(Ok(h))) if w > 0 && h > 0 => Ok((w, h)),
            _ => Err(format!(
                "ffprobe: no video size ({})",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        }
    }
}

fn rotate_filter(rotation: u32) -> Option<&'static str> {
    match rotation % 4 {
        1 => Some("transpose=2"),
        2 => Some("hflip,vflip"),
        3 => Some("transpose=1"),
        _ => None,
    }
}

fn filter(layout: &Layout, panel_width: u32) -> String {
    let mut filters: Vec<String> = Vec::new();
    match *layout {
        Layout::Direct {
            scaled_width,
            height,
            rotation,
            ..
        } => {
            if let Some(rotate) = rotate_filter(rotation) {
                filters.push(rotate.into());
            }
            filters.push(format!("scale={scaled_width}:{height}"));
            if scaled_width < panel_width {
                filters.push(format!("pad={panel_width}:{height}:(ow-iw)/2:0"));
            }
        }
        Layout::Blit { width, height, .. } => filters.push(format!("scale={width}:{height}")),
    }
    filters.push("setsar=1".into());
    filters.join(",")
}

pub struct Source {
    child: Child,
    stdout: ChildStdout,
    output: Output,
    frame_bytes: usize,
    pending: Vec<u8>,
}

impl Source {
    pub fn spawn(
        input: &Input,
        layout: &Layout,
        panel_width: u32,
        max_fps: u32,
        output: Output,
    ) -> Result<Self, String> {
        let (frame_width, frame_height) = layout.frame(panel_width);

        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if input.realtime {
            command.arg("-re");
        }
        if input.looped {
            command.args(["-stream_loop", "-1"]);
        }
        command
            .args(input.args())
            .args(["-an", "-sn", "-vf", &filter(layout, panel_width)])
            .args(["-fpsmax", &max_fps.to_string()]);
        let frame_bytes = match &output {
            Output::Jpeg { quality } => {
                command.args([
                    "-c:v",
                    "mjpeg",
                    "-pix_fmt",
                    "yuvj420p",
                    "-q:v",
                    &quality.to_string(),
                    "-f",
                    "mjpeg",
                ]);
                0
            }
            Output::Raw(format) => {
                let pix_fmt = match format {
                    PixelFormat::Rgb565 => "rgb565le",
                    PixelFormat::Rgb888 => "bgr24",
                };
                command.args(["-f", "rawvideo", "-pix_fmt", pix_fmt]);
                (frame_width * frame_height) as usize * format.bytes()
            }
        };
        let mut child = command
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        Ok(Source {
            child,
            stdout,
            output,
            frame_bytes,
            pending: Vec::new(),
        })
    }

    /// None at the end of the input.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let complete = match self.output {
                Output::Jpeg { .. } => jpeg_length(&self.pending),
                Output::Raw(_) => {
                    (self.pending.len() >= self.frame_bytes).then_some(self.frame_bytes)
                }
            };
            if let Some(length) = complete {
                let rest = self.pending.split_off(length);
                return Ok(Some(std::mem::replace(&mut self.pending, rest)));
            }
            let mut chunk = [0u8; 65536];
            let read = self
                .stdout
                .read(&mut chunk)
                .map_err(|e| format!("ffmpeg output: {e}"))?;
            if read == 0 {
                return Ok(None);
            }
            self.pending.extend_from_slice(&chunk[..read]);
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Length of the JPEG at the start of `data`, once all of it is there.
fn jpeg_length(data: &[u8]) -> Option<usize> {
    if data.len() < 2 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut pos = 2;
    loop {
        while pos < data.len() && data[pos] == 0xff && data.get(pos + 1) == Some(&0xff) {
            pos += 1;
        }
        if pos + 4 > data.len() || data[pos] != 0xff {
            return None;
        }
        let marker = data[pos + 1];
        let length = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 2 + length;
        if marker != 0xda {
            continue;
        }
        while pos + 1 < data.len() {
            if data[pos] == 0xff {
                match data[pos + 1] {
                    0x00 | 0xd0..=0xd7 => pos += 2,
                    0xd9 => return Some(pos + 2),
                    _ => break,
                }
            } else {
                pos += 1;
            }
        }
        if pos + 1 >= data.len() {
            return None;
        }
    }
}

#[derive(Clone, Copy)]
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

/// Interleaved little-endian PCM of the input's audio from ffmpeg.
pub struct AudioSource {
    child: Child,
    stdout: ChildStdout,
}

impl AudioSource {
    pub fn spawn(input: &Input, format: AudioFormat) -> Result<Self, String> {
        let sample = match format.bits {
            16 => "s16le",
            24 => "s24le",
            32 => "s32le",
            bits => return Err(format!("{bits} bit audio")),
        };
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if input.realtime {
            command.arg("-re");
        }
        if input.looped {
            command.args(["-stream_loop", "-1"]);
        }
        let mut child = command
            .args(input.args())
            .args(["-vn", "-sn", "-ac", &format.channels.to_string()])
            .args(["-ar", &format.rate.to_string(), "-f", sample, "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        Ok(AudioSource { child, stdout })
    }

    /// Fills `buffer`, or returns false at the end of the input.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<bool, String> {
        match self.stdout.read_exact(buffer) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
            Err(e) => Err(format!("ffmpeg audio: {e}")),
        }
    }
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
