//! Frames from ffmpeg, fitted to the panel.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::protocol::PixelFormat;

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

/// Where frames land on the panel.
pub enum Layout {
    /// Panel-wide rows, `height` tall from row `y`; ffmpeg rotates, scales and
    /// pads the source.
    Direct {
        scaled_width: u32,
        height: u32,
        y: u32,
        rotation: u32,
    },
    /// `width` x `height` frames the device rotates by `rotation` and scales by
    /// `scale` / 16 into `area` (x, y, w, h).
    Blit {
        width: u32,
        height: u32,
        rotation: u32,
        scale: u32,
        area: (u32, u32, u32, u32),
    },
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

fn rotated(size: (u32, u32), rotation: u32) -> (u32, u32) {
    if rotation % 2 == 1 {
        (size.1, size.0)
    } else {
        size
    }
}

impl Layout {
    pub fn direct(source: (u32, u32), rotation: u32, panel: (u32, u32), align: u32) -> Self {
        let (sw, sh) = rotated(source, rotation);
        let (pw, ph) = panel;
        let scale = (pw as f64 / sw as f64).min(ph as f64 / sh as f64);
        let scaled_width = (((sw as f64 * scale).round() as u32) / 2 * 2).clamp(2, pw);
        let height = (((sh as f64 * scale) / align as f64).round() as u32 * align)
            .clamp(align, ph / align * align);
        Layout::Direct {
            scaled_width,
            height,
            y: (ph - height) / 2 / align * align,
            rotation,
        }
    }

    /// Shrinks the source to fit `max` on both sides in multiples of 16, so any
    /// whole number of sixteenths scales it to whole pixels.
    pub fn blit(
        source: (u32, u32),
        rotation: u32,
        panel: (u32, u32),
        max: (u32, u32),
    ) -> Result<Self, String> {
        let shrink = (max.0 as f64 / source.0 as f64)
            .min(max.1 as f64 / source.1 as f64)
            .min(1.0);
        let side = |length: u32, limit: u32| {
            (((length as f64 * shrink / 16.0).round() as u32) * 16).clamp(16, limit / 16 * 16)
        };
        let (width, height) = (side(source.0, max.0), side(source.1, max.1));
        let (rw, rh) = rotated((width, height), rotation);
        let (pw, ph) = panel;
        let scale = (16 * pw / rw).min(16 * ph / rh);
        if scale == 0 {
            return Err(format!("{rw}x{rh} does not fit the panel"));
        }
        let (dw, dh) = (rw * scale / 16, rh * scale / 16);
        Ok(Layout::Blit {
            width,
            height,
            rotation,
            scale,
            area: ((pw - dw) / 2, (ph - dh) / 2, dw, dh),
        })
    }

    /// The decoded frame's size.
    pub fn frame(&self, panel_width: u32) -> (u32, u32) {
        match *self {
            Layout::Direct { height, .. } => (panel_width, height),
            Layout::Blit { width, height, .. } => (width, height),
        }
    }

    /// The panel area the frames cover (x, y, w, h).
    pub fn content(&self, panel_width: u32) -> (u32, u32, u32, u32) {
        match *self {
            Layout::Direct { height, y, .. } => (0, y, panel_width, height),
            Layout::Blit { area, .. } => area,
        }
    }

    fn filter(&self, panel_width: u32) -> String {
        let mut filters: Vec<String> = Vec::new();
        match *self {
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
            .args(["-an", "-sn", "-vf", &layout.filter(panel_width)])
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
