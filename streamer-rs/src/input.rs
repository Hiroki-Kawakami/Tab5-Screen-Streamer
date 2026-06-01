//! Tab5 -> PC touch input: read touch reports from the vendor IN endpoint, map
//! panel coordinates to the captured display, and inject OS pointer events.
//!
//! See PROTOCOL.md for the wire format. The firmware sends one report per USB
//! transfer describing the full set of active contacts; we diff successive
//! reports by track id and drive a single OS pointer (the primary contact) as
//! the left mouse button. Multi-touch ids are carried but currently collapsed to
//! the primary pointer.

use std::io::Read;
use std::sync::Mutex;
use nusb::{io::EndpointRead, transfer::Bulk};

/// Touch report message type (byte 0). Other values are reserved.
const MSG_TYPE_TOUCH: u8 = 0x01;
const MAX_POINTS: usize = 5;
const REPORT_HEADER: usize = 2;
const REPORT_PER_POINT: usize = 6;

#[derive(Clone, Copy)]
struct TouchPoint {
    id: u8,
    x: u16,
    y: u16,
}

/// Target display geometry, resolved by the capture backend once it knows which
/// display it is mirroring. Coordinates are in the OS' global event space.
#[derive(Clone, Copy)]
pub struct Geometry {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
    /// True when the captured frame was rotated 270° (landscape source). See
    /// PROTOCOL.md "Coordinate mapping".
    pub rotate: bool,
}

static GEOMETRY: Mutex<Option<Geometry>> = Mutex::new(None);

/// Publish the target display geometry. Called by the platform capture backend.
pub fn set_geometry(g: Geometry) {
    *GEOMETRY.lock().unwrap() = Some(g);
}

fn geometry() -> Option<Geometry> {
    *GEOMETRY.lock().unwrap()
}

/// Map a panel point (720x1280 portrait) to a global display pixel position.
fn map_point(p: &TouchPoint, g: &Geometry) -> (f64, f64) {
    let tx = p.x as f64;
    let ty = p.y as f64;
    let (nx, ny) = if g.rotate {
        (1.0 - ty / 1280.0, tx / 720.0)
    } else {
        (tx / 720.0, ty / 1280.0)
    };
    let nx = nx.clamp(0.0, 1.0);
    let ny = ny.clamp(0.0, 1.0);
    (g.origin_x + nx * g.width, g.origin_y + ny * g.height)
}

/// Drives a single OS pointer from the stream of touch reports.
struct Injector {
    backend: backend::Backend,
    tracked: Option<u8>, // track id currently mapped to the pointer
    pressed: bool,
    last: (f64, f64),
}

impl Injector {
    fn new() -> Self {
        Self {
            backend: backend::Backend::new(),
            tracked: None,
            pressed: false,
            last: (0.0, 0.0),
        }
    }

    fn handle(&mut self, pts: &[TouchPoint]) {
        let Some(g) = geometry() else { return }; // drop reports until geometry is known

        if pts.is_empty() {
            if self.pressed {
                self.backend.up(self.last.0, self.last.1);
                self.pressed = false;
            }
            self.tracked = None;
            return;
        }

        // Keep following the same contact if it is still down, otherwise adopt
        // the first reported contact as the new primary.
        let primary = self
            .tracked
            .and_then(|id| pts.iter().find(|p| p.id == id))
            .unwrap_or(&pts[0]);
        self.tracked = Some(primary.id);

        let (x, y) = map_point(primary, &g);
        self.last = (x, y);
        if self.pressed {
            self.backend.drag(x, y);
        } else {
            self.backend.moveto(x, y);
            self.backend.down(x, y);
            self.pressed = true;
        }
    }
}

/// Read touch reports from the vendor IN endpoint until the device goes away.
pub fn run(mut reader: EndpointRead<Bulk>) {
    let mut injector = Injector::new();
    let mut acc: Vec<u8> = Vec::with_capacity(256);
    let mut tmp = [0u8; 256];

    loop {
        let n = match reader.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break, // device unplugged / endpoint closed
        };
        acc.extend_from_slice(&tmp[..n]);

        // Parse every complete message; resync on the type byte if the stream
        // ever drifts (bulk transfers may in principle coalesce or split).
        let mut i = 0;
        while i < acc.len() {
            if acc[i] != MSG_TYPE_TOUCH {
                i += 1;
                continue;
            }
            if i + REPORT_HEADER > acc.len() {
                break; // need the count byte
            }
            let count = acc[i + 1] as usize;
            if count > MAX_POINTS {
                i += 1; // bogus length: treat as garbage and resync
                continue;
            }
            let msg_len = REPORT_HEADER + count * REPORT_PER_POINT;
            if i + msg_len > acc.len() {
                break; // wait for the rest of the message
            }

            let mut pts: Vec<TouchPoint> = Vec::with_capacity(count);
            for k in 0..count {
                let off = i + REPORT_HEADER + k * REPORT_PER_POINT;
                pts.push(TouchPoint {
                    id: acc[off],
                    x: u16::from_le_bytes([acc[off + 2], acc[off + 3]]),
                    y: u16::from_le_bytes([acc[off + 4], acc[off + 5]]),
                });
            }
            injector.handle(&pts);
            i += msg_len;
        }
        acc.drain(0..i);
    }
}

// ---------------------------------------------------------------------------
// Platform pointer injection
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod backend {
    use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGPoint;

    pub struct Backend {
        source: Option<CGEventSource>,
    }

    impl Backend {
        pub fn new() -> Self {
            let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok();
            if source.is_none() {
                eprintln!("Touch: failed to create CGEventSource; pointer injection disabled.");
            }
            Self { source }
        }

        fn post(&self, ty: CGEventType, x: f64, y: f64) {
            let Some(source) = self.source.clone() else { return };
            if let Ok(ev) =
                CGEvent::new_mouse_event(source, ty, CGPoint::new(x, y), CGMouseButton::Left)
            {
                ev.post(CGEventTapLocation::HID);
            }
        }

        pub fn down(&self, x: f64, y: f64) {
            self.post(CGEventType::LeftMouseDown, x, y);
        }
        pub fn up(&self, x: f64, y: f64) {
            self.post(CGEventType::LeftMouseUp, x, y);
        }
        pub fn drag(&self, x: f64, y: f64) {
            self.post(CGEventType::LeftMouseDragged, x, y);
        }
        pub fn moveto(&self, x: f64, y: f64) {
            self.post(CGEventType::MouseMoved, x, y);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use std::sync::atomic::{AtomicBool, Ordering};

    static WARNED: AtomicBool = AtomicBool::new(false);

    pub struct Backend;

    impl Backend {
        pub fn new() -> Self {
            Backend
        }
        fn warn_once(&self) {
            if !WARNED.swap(true, Ordering::Relaxed) {
                eprintln!("Touch: OS pointer injection is not implemented on this platform yet.");
            }
        }
        pub fn down(&self, _x: f64, _y: f64) {
            self.warn_once();
        }
        pub fn up(&self, _x: f64, _y: f64) {}
        pub fn drag(&self, _x: f64, _y: f64) {}
        pub fn moveto(&self, _x: f64, _y: f64) {}
    }
}
