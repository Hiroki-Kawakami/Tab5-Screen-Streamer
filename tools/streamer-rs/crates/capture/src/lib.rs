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
    platform: platform::Capture,
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
        Ok(Capture { platform })
    }

    /// Why the OS has stopped the capture, once it has; for instance when the
    /// display is reconfigured.
    pub fn stopped(&self) -> Option<String> {
        self.platform.stopped()
    }
}

/// A display the OS shows as a monitor, until dropped. macOS only.
pub struct VirtualDisplay(platform::VirtualDisplay);

impl VirtualDisplay {
    /// `millimeters` is the physical size the OS scales its interface by.
    pub fn new(name: &str, size: (u32, u32), millimeters: (f64, f64)) -> Result<Self, String> {
        platform::VirtualDisplay::new(name, size, millimeters).map(VirtualDisplay)
    }

    pub fn id(&self) -> u64 {
        self.0.id()
    }

    /// Changes the resolution, up to the longer side it was created with.
    pub fn resize(&self, size: (u32, u32)) -> Result<(), String> {
        self.0.resize(size)
    }
}
