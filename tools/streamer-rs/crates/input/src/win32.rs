//! Touches through a synthetic pointer device.

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_DEFAULT, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
    POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_FLAGS, POINTER_INFO,
    POINTER_TOUCH_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::PT_TOUCH;

use crate::Point;

/// Pointer IDs have to be below it.
const MAX_CONTACTS: u32 = 10;

pub struct Injector {
    device: HSYNTHETICPOINTERDEVICE,
    /// In the order they went down; the first is the primary contact.
    down: Vec<(u8, POINT)>,
}

fn contact(id: u8, at: POINT, flags: POINTER_FLAGS) -> POINTER_TYPE_INFO {
    POINTER_TYPE_INFO {
        r#type: PT_TOUCH,
        Anonymous: POINTER_TYPE_INFO_0 {
            touchInfo: POINTER_TOUCH_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_TOUCH,
                    pointerId: id as u32,
                    pointerFlags: flags,
                    ptPixelLocation: at,
                    ..Default::default()
                },
                ..Default::default()
            },
        },
    }
}

impl Injector {
    // Monitor bounds and injected points are physical pixels only in a
    // per-monitor aware process. It fails when the awareness is already set.
    pub fn new() -> Result<Self, String> {
        let _ =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let device = unsafe {
            CreateSyntheticPointerDevice(PT_TOUCH, MAX_CONTACTS, POINTER_FEEDBACK_DEFAULT)
        }
        .map_err(|e| format!("synthetic pointer device: {e}"))?;
        Ok(Injector {
            device,
            down: Vec::new(),
        })
    }

    pub fn touch(&mut self, points: &[Point]) -> Result<(), String> {
        let points: Vec<(u8, POINT)> = points
            .iter()
            .filter(|p| (p.id as u32) < MAX_CONTACTS)
            .map(|p| {
                let at = POINT {
                    x: p.x.round() as i32,
                    y: p.y.round() as i32,
                };
                (p.id, at)
            })
            .collect();
        let mut frame = Vec::new();
        let mut down = Vec::new();
        for &(id, at) in &self.down {
            match points.iter().find(|(p, _)| *p == id) {
                Some(&(_, now)) => {
                    frame.push((
                        id,
                        now,
                        POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                    ));
                    down.push((id, now));
                }
                None => frame.push((id, at, POINTER_FLAG_UP)),
            }
        }
        for &(id, at) in &points {
            if !self.down.iter().any(|(d, _)| *d == id) {
                frame.push((
                    id,
                    at,
                    POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                ));
                down.push((id, at));
            }
        }
        let primary = self.down.first().or(down.first()).map(|(id, _)| *id);
        self.down = down;
        self.inject(&frame, primary)
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let frame: Vec<_> = self
            .down
            .iter()
            .map(|&(id, at)| {
                (
                    id,
                    at,
                    POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                )
            })
            .collect();
        let primary = self.down.first().map(|(id, _)| *id);
        self.inject(&frame, primary)
    }

    fn inject(
        &self,
        frame: &[(u8, POINT, POINTER_FLAGS)],
        primary: Option<u8>,
    ) -> Result<(), String> {
        if frame.is_empty() {
            return Ok(());
        }
        let infos: Vec<POINTER_TYPE_INFO> = frame
            .iter()
            .map(|&(id, at, flags)| {
                let flags = match Some(id) == primary {
                    true => flags | POINTER_FLAG_PRIMARY,
                    false => flags,
                };
                contact(id, at, flags)
            })
            .collect();
        unsafe { InjectSyntheticPointerInput(self.device, &infos) }
            .map_err(|e| format!("inject touch: {e}"))
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        unsafe { DestroySyntheticPointerDevice(self.device) };
    }
}
