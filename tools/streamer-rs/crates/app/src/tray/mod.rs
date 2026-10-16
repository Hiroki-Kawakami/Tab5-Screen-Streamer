//! The icon in the menu bar or the notification area.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
use win32 as platform;

use egui_glow::egui_winit::winit::dpi::{LogicalSize, Position};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Disabled,
    Disconnected,
    Idle,
    Streaming,
}

/// When the app is already running, has it open its panel and returns true;
/// this one then exits.
pub fn hand_over() -> bool {
    platform::hand_over()
}

/// Removes the icon when dropped.
pub struct Tray(platform::Tray);

impl Tray {
    /// On the main thread, which `clicked` runs on too.
    pub fn new(icon: Icon, clicked: Box<dyn Fn()>) -> Result<Self, String> {
        platform::Tray::new(icon, clicked).map(Tray)
    }

    pub fn set_icon(&mut self, icon: Icon) {
        self.0.set_icon(icon);
    }

    /// Where a panel of `size` goes next to the icon, inside the screen.
    pub fn place(&self, size: LogicalSize<f64>) -> Option<Position> {
        self.0.place(size)
    }
}
