//! Screen capture: a display's frames, scaled to a requested size.

use std::sync::mpsc::SyncSender;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod wgc;
#[cfg(windows)]
use wgc as platform;

pub enum Format {
    /// Bytes B, G, R and one ignored, per pixel.
    Bgrx,
    /// Y rows, then interleaved Cb, Cr rows, BT.601 full range.
    Nv12,
}

/// Rows packed without padding.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub format: Format,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct Display {
    pub id: u64,
    pub size: (u32, u32),
}

/// Asks for the permission to capture when the OS needs one; false when it
/// is not granted.
pub fn authorize() -> bool {
    platform::authorize()
}

pub fn displays() -> Result<Vec<Display>, String> {
    platform::displays()
}

pub fn main_display() -> Result<Display, String> {
    platform::main_display()
}

/// The display's current size, or None when it is gone.
pub fn display(id: u64) -> Option<Display> {
    platform::display(id)
}

/// Capturing until dropped.
pub struct Capture {
    _platform: platform::Capture,
}

impl Capture {
    /// Frames of `size` (even) come on `frames`, at most `max_fps` a second
    /// and only when the screen changes. They are dropped while `frames` is
    /// full.
    pub fn start(
        display: u64,
        size: (u32, u32),
        max_fps: u32,
        frames: SyncSender<Frame>,
    ) -> Result<Self, String> {
        let platform = platform::Capture::start(display, size, max_fps, frames)?;
        Ok(Capture {
            _platform: platform,
        })
    }
}
