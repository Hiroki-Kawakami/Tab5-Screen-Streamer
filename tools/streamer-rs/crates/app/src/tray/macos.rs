use egui_glow::egui_winit::winit::dpi::{LogicalPosition, LogicalSize, Position};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{NSImage, NSScreen, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSSize};

use super::Icon;

const ICON_POINTS: f64 = 18.0;
/// Where the menu bar's own panels go.
const BELOW_MENU_BAR: f64 = 2.0;
const SCREEN_MARGIN: f64 = 6.0;

struct Ivars {
    clicked: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "Tab5StreamerTrayTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            (self.ivars().clicked)();
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, clicked: Box<dyn Fn()>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { clicked });
        unsafe { msg_send![super(this), init] }
    }
}

fn png(icon: Icon) -> &'static [u8] {
    match icon {
        Icon::Disabled => include_bytes!("../../icons/tray-disabled.png"),
        Icon::Disconnected => include_bytes!("../../icons/tray-disconnected.png"),
        Icon::Idle => include_bytes!("../../icons/tray-idle.png"),
        Icon::Streaming => include_bytes!("../../icons/tray-streaming.png"),
    }
}

fn image(icon: Icon) -> Option<Retained<NSImage>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png(icon)))?;
    image.setSize(NSSize::new(ICON_POINTS, ICON_POINTS));
    image.setTemplate(true);
    Some(image)
}

/// macOS keeps an app bundle to one instance itself.
pub fn hand_over() -> bool {
    false
}

pub struct Tray {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    _target: Retained<Target>,
}

impl Tray {
    pub fn new(icon: Icon, clicked: Box<dyn Fn()>) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("the tray is not on the main thread")?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let button = item.button(mtm).ok_or("the status item has no button")?;
        let target = Target::new(mtm, clicked);
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(clicked:)));
        }
        let mut tray = Tray {
            mtm,
            item,
            _target: target,
        };
        tray.set_icon(icon);
        Ok(tray)
    }

    pub fn set_icon(&mut self, icon: Icon) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setImage(image(icon).as_deref());
        }
    }

    /// Below the icon. Positions are from the top left of the main screen,
    /// AppKit's from its bottom left.
    pub fn place(&self, size: LogicalSize<f64>) -> Option<Position> {
        let window = self.item.button(self.mtm)?.window()?;
        let icon = window.frame();
        let visible = window.screen()?.visibleFrame();
        let main = NSScreen::screens(self.mtm).firstObject()?.frame();
        let left = visible.origin.x + SCREEN_MARGIN;
        let right = visible.origin.x + visible.size.width - size.width - SCREEN_MARGIN;
        let x = (icon.origin.x + icon.size.width / 2.0 - size.width / 2.0).min(right).max(left);
        let top = icon.origin.y - BELOW_MENU_BAR;
        Some(LogicalPosition::new(x, main.size.height - top).into())
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
