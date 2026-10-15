//! The first touch point as the left mouse button; macOS has no public API to
//! inject touches.

use std::time::{Duration, Instant};

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventType, CGMouseButton,
    CGPreflightPostEventAccess, CGRequestPostEventAccess,
};

use crate::Point;

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f64 = 8.0;

enum Contact {
    Up,
    Down(u8),
    /// The mouse's point is up while others are still down.
    Released,
}

pub struct Injector {
    contact: Contact,
    position: CGPoint,
    clicks: i64,
    last_down: Option<(Instant, CGPoint)>,
}

impl Injector {
    pub fn new() -> Result<Self, String> {
        if !CGPreflightPostEventAccess() && !CGRequestPostEventAccess() {
            return Err("posting input events is not permitted".into());
        }
        Ok(Injector {
            contact: Contact::Up,
            position: CGPoint::new(0.0, 0.0),
            clicks: 0,
            last_down: None,
        })
    }

    pub fn touch(&mut self, points: &[Point]) -> Result<(), String> {
        match self.contact {
            Contact::Down(id) => match points.iter().find(|p| p.id == id) {
                Some(point) => {
                    let at = CGPoint::new(point.x, point.y);
                    if at != self.position {
                        self.position = at;
                        self.post(CGEventType::LeftMouseDragged)?;
                    }
                }
                None => {
                    self.post(CGEventType::LeftMouseUp)?;
                    self.contact = Contact::Released;
                }
            },
            Contact::Up => {
                if let Some(point) = points.first() {
                    let at = CGPoint::new(point.x, point.y);
                    let repeated = self.last_down.is_some_and(|(time, last)| {
                        time.elapsed() < DOUBLE_CLICK
                            && (at.x - last.x).abs() < DOUBLE_CLICK_SLOP
                            && (at.y - last.y).abs() < DOUBLE_CLICK_SLOP
                    });
                    self.clicks = if repeated { self.clicks + 1 } else { 1 };
                    self.last_down = Some((Instant::now(), at));
                    self.position = at;
                    self.contact = Contact::Down(point.id);
                    self.post(CGEventType::LeftMouseDown)?;
                }
            }
            Contact::Released => {}
        }
        if points.is_empty() {
            self.contact = Contact::Up;
        }
        Ok(())
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        Ok(())
    }

    // Apps count double clicks from the click state, which synthetic events
    // do not get by themselves.
    fn post(&self, kind: CGEventType) -> Result<(), String> {
        let event = CGEvent::new_mouse_event(None, kind, self.position, CGMouseButton::Left)
            .ok_or("no mouse event")?;
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::MouseEventClickState,
            self.clicks,
        );
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        Ok(())
    }
}
