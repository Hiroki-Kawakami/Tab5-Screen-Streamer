//! Touch input injected into the OS.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
use win32 as platform;

/// A touch point in desktop coordinates, see streamer_capture::bounds.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub id: u8,
    pub x: f64,
    pub y: f64,
}

/// Releases what it holds when dropped.
pub struct Injector(platform::Injector);

impl Injector {
    /// Fails when the OS does not permit injecting input; macOS then asks the
    /// user for the permission, which takes effect from the next start.
    pub fn new() -> Result<Self, String> {
        platform::Injector::new().map(Injector)
    }

    /// The points down now; empty releases them all. macOS takes only the
    /// first point as the mouse.
    pub fn touch(&mut self, points: &[Point]) -> Result<(), String> {
        self.0.touch(points)
    }

    /// Call it periodically: Windows cancels contacts that are not updated.
    pub fn refresh(&mut self) -> Result<(), String> {
        self.0.refresh()
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        let _ = self.0.touch(&[]);
    }
}
