pub mod capture;
pub mod present;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use streamer_core::protocol::{Event, Info};
use streamer_device::connection::{Connection, Device};

/// How often a wait looks whether to stop.
pub const STOP_POLL: Duration = Duration::from_millis(100);

/// What streaming has done or seen, for the user.
pub enum Notice {
    Connected(Info),
    Disconnected(String),
    /// How frames are laid out on the panel.
    Layout(String),
    /// A device event that changed the state.
    Event(Event),
    VirtualDisplay { id: u64, size: (u32, u32) },
    CaptureRestarting(String),
    MirroringChanged,
    DisplayGone,
    DisplaySize((u32, u32)),
    DisplayOff(String),
    DisplayOn,
    Stats(String),
    Warning(String),
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

/// Changes to a running stream.
pub enum Request {
    /// Whether the device's touches are injected into the OS.
    Touch(bool),
    /// 0-100.
    Brightness(u8),
}

pub struct Control<'a> {
    pub stop: &'a AtomicBool,
    pub requests: Option<&'a Receiver<Request>>,
    pub notify: Notify,
}

impl Control<'_> {
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// The next connection; None once stopping.
pub fn connect(device: &Device, stop: &AtomicBool) -> Option<Connection> {
    while !stop.load(Ordering::Relaxed) {
        if let Some(connection) = device.wait(STOP_POLL) {
            return Some(connection);
        }
    }
    None
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Notice::Connected(_) => write!(f, "connected"),
            Notice::Disconnected(reason) => write!(f, "disconnected: {reason}"),
            Notice::Layout(layout) => write!(f, "{layout}"),
            Notice::Event(event) => event_text(event, f),
            Notice::VirtualDisplay { size, .. } => {
                write!(f, "virtual display {}x{}", size.0, size.1)
            }
            Notice::CaptureRestarting(reason) => {
                write!(f, "capture stopped ({reason}), restarting")
            }
            Notice::MirroringChanged => write!(f, "display mirroring changed"),
            Notice::DisplayGone => write!(f, "display gone"),
            Notice::DisplaySize(size) => write!(f, "display now {}x{}", size.0, size.1),
            Notice::DisplayOff(reason) => write!(f, "display off ({reason})"),
            Notice::DisplayOn => write!(f, "display on"),
            Notice::Stats(stats) => write!(f, "{stats}"),
            Notice::Warning(warning) => write!(f, "{warning}"),
        }
    }
}

fn event_text(event: &Event, f: &mut fmt::Formatter) -> fmt::Result {
    match event {
        Event::Orientation(orientation) => write!(f, "orientation {orientation}"),
        Event::Volume {
            stream,
            mute,
            volume,
        } => write!(
            f,
            "volume {stream}: {:.1} dB{}",
            *volume as f64 / 256.0,
            if *mute { ", muted" } else { "" }
        ),
        Event::Overlay { visible, stream } => write!(
            f,
            "overlay {}, stream {}x{} at {},{}",
            if *visible { "shown" } else { "hidden" },
            stream.2,
            stream.3,
            stream.0,
            stream.1
        ),
        Event::InputMode(target, mode) => write!(f, "{target:?} input {mode:?}"),
        Event::Damage((x, y, w, h)) => write!(f, "damage {w}x{h} at {x},{y}"),
        Event::Format { format, epoch } => write!(f, "pixel format {format:?}, epoch {epoch}"),
        Event::Brightness(brightness) => write!(f, "brightness {brightness}%"),
        _ => Ok(()),
    }
}
