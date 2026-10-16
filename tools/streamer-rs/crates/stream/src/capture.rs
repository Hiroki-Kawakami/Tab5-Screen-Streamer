//! Streaming a display. Capture runs only while the device is connected.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use streamer_capture::{self as capture, Capture, Format, Frame, VirtualDisplay};
use streamer_core::jpeg::{Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::protocol::{self, Encoding, Info, InputMode, InputTarget, TouchPoint};
use streamer_device::connection::{Connection, Device};
use streamer_input::{self as input, Injector};

use crate::present::{self, LayoutKey, Presenter, Quality};
use crate::{Control, Notice, Notify, Request};

/// How long to wait for a frame before handling device events.
const POLL: Duration = Duration::from_millis(50);
const DISPLAY_CHECK: Duration = Duration::from_secs(1);
/// Before restarting a capture the OS has stopped.
const RESTART_DELAY: Duration = Duration::from_millis(500);
/// How long a new virtual display may take to become capturable.
const VIRTUAL_APPEAR: Duration = Duration::from_secs(3);
const VIRTUAL_NAME: &str = "M5Stack Tab5";
/// For a device without a USB serial number, such as over USB/IP.
const VIRTUAL_SERIAL: u32 = 0x1234;
/// The size declared for the virtual display, portrait: larger than the panel,
/// see docs/macos-virtual-display.md.
const PANEL_MILLIMETERS: (f64, f64) = (169.0, 300.0);
type Result<T> = std::result::Result<T, String>;

pub struct CaptureOptions {
    /// An index into the displays list; the main display when None.
    pub display: Option<usize>,
    /// Capture a virtual display made for the device instead.
    pub virtual_display: bool,
    pub rotation: Option<u32>,
    pub quality: Quality,
    /// Inject the device's touches into the OS.
    pub touch: bool,
    pub verbose: bool,
}

fn image(frame: &Frame) -> Image<'_> {
    let (w, h) = (frame.width, frame.height);
    let pixels = match frame.format {
        Format::Bgrx => Pixels::Bgrx {
            data: &frame.data,
            stride: w * 4,
        },
        Format::Nv12 => {
            let (y, uv) = frame.data.split_at(w * h);
            Pixels::Nv12 {
                y,
                y_stride: w,
                uv,
                uv_stride: w,
            }
        }
    };
    Image {
        width: w,
        height: h,
        pixels,
    }
}

/// Streams the display until it cannot be captured; the device may come and
/// go meanwhile.
pub fn capture(device: &Device, options: CaptureOptions, control: &Control) -> Result<()> {
    if !capture::authorize() {
        return Err("screen capture is not permitted".into());
    }
    let display = match (options.virtual_display, options.display) {
        (true, _) => None,
        (false, Some(index)) => Some(
            *capture::displays()?
                .get(index)
                .ok_or_else(|| format!("no display {index}"))?,
        ),
        (false, None) => Some(capture::main_display()?),
    };
    let mut injector = Injector::new()
        .inspect_err(|e| (control.notify)(Notice::Warning(format!("touch input off: {e}"))))
        .ok();
    let mut touch = options.touch;
    while let Some(mut connection) = crate::connect(device, control.stop) {
        (control.notify)(Notice::Connected(connection.info.clone()));
        let (_, brightness) = requests(control, &mut touch);
        let mode = connection.info.input_mode[InputTarget::Touch as usize];
        let started = set_touch(&mut connection, mode, injector.is_some() && touch)
            .and_then(|()| set_brightness(&mut connection, brightness));
        if let Err(e) = started {
            lost(device, connection, control, e);
            continue;
        }
        let created = match display {
            Some(_) => None,
            None => {
                let rotation = options
                    .rotation
                    .unwrap_or(connection.info.orientation as u32);
                let serial = virtual_serial(connection.serial.as_deref());
                Some(create_virtual(
                    &connection.info,
                    serial,
                    rotation,
                    &control.notify,
                )?)
            }
        };
        let stop = loop {
            let source = match (&created, display) {
                (Some(virtual_display), _) => Source::of(virtual_display),
                (None, Some(display)) => Source::Display(display.id),
                (None, None) => unreachable!(),
            };
            let injector = injector.as_mut();
            match Session::run(&mut connection, source, &options, injector, control, &mut touch) {
                Err(Changed) => continue,
                stop => break stop,
            }
        };
        if let Some(injector) = injector.as_mut() {
            let _ = injector.touch(&[]);
        }
        match stop {
            Ok(()) => return connection.close(),
            Err(Lost(e)) => lost(device, connection, control, e),
            Err(Fatal(e)) => {
                let _ = connection.close();
                return Err(e);
            }
            Err(Changed) => unreachable!(),
        }
    }
    Ok(())
}

fn lost(device: &Device, connection: Connection, control: &Control, reason: String) {
    device.lost(connection, &reason);
    (control.notify)(Notice::Disconnected(reason));
}

/// Takes the pending requests: true when touch injection has changed, and
/// the last brightness asked for.
fn requests(control: &Control, touch: &mut bool) -> (bool, Option<u8>) {
    let Some(requests) = control.requests else {
        return (false, None);
    };
    let before = *touch;
    let mut brightness = None;
    for request in requests.try_iter() {
        match request {
            Request::Touch(on) => *touch = on,
            Request::Brightness(percent) => brightness = Some(percent),
        }
    }
    (*touch != before, brightness)
}

fn set_brightness(connection: &mut Connection, brightness: Option<u8>) -> Result<()> {
    if let Some(brightness) = brightness {
        let payload = protocol::set_brightness(brightness);
        connection.control(protocol::SET_BRIGHTNESS, &payload)?;
    }
    Ok(())
}

/// Takes the device's touches while they are injected, and leaves them to
/// its trackpad otherwise.
fn set_touch(connection: &mut Connection, current: InputMode, inject: bool) -> Result<()> {
    let mode = match inject {
        true => InputMode::Event,
        false => InputMode::Hid,
    };
    if current == InputMode::None || current == mode {
        return Ok(());
    }
    let payload = protocol::set_input_mode(InputTarget::Touch, mode);
    connection.control(protocol::SET_INPUT_MODE, &payload)?;
    Ok(())
}

/// What a session captures.
#[derive(Clone, Copy)]
enum Source<'a> {
    Display(u64),
    /// A virtual display on its own, sized by the session to the rotation.
    Virtual(&'a VirtualDisplay),
    /// The display a virtual display mirrors, or is mirrored to: macOS gives
    /// the set one mode, so it is captured like any display.
    Mirrored {
        virtual_id: u64,
        display: u64,
    },
}

impl<'a> Source<'a> {
    fn of(virtual_display: &'a VirtualDisplay) -> Self {
        let virtual_id = virtual_display.id();
        match capture::mirror_primary(virtual_id) {
            Some(display) => Source::Mirrored {
                virtual_id,
                display,
            },
            None => Source::Virtual(virtual_display),
        }
    }

    fn display(&self) -> u64 {
        match *self {
            Source::Display(id) | Source::Mirrored { display: id, .. } => id,
            Source::Virtual(display) => display.id(),
        }
    }

    /// Whether a virtual display has gone into or out of a mirror set.
    fn changed(&self) -> bool {
        match *self {
            Source::Display(_) => false,
            Source::Virtual(display) => capture::mirror_primary(display.id()).is_some(),
            Source::Mirrored {
                virtual_id,
                display,
            } => capture::mirror_primary(virtual_id) != Some(display),
        }
    }
}

/// The display size that fills the panel at `rotation`.
fn virtual_size(info: &Info, rotation: u32) -> (u32, u32) {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    streamer_core::layout::rotated(panel, rotation)
}

/// Waits for a display to take a new size, which the OS applies later.
fn wait_for_size(id: u64, size: (u32, u32)) -> Result<()> {
    let deadline = Instant::now() + VIRTUAL_APPEAR;
    while capture::display(id).is_none_or(|d| d.size != size) {
        if Instant::now() >= deadline {
            return Err(format!("display did not become {}x{}", size.0, size.1));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// A virtual display for the panel, once it or the display it mirrors can be
/// captured.
/// The low 32 bits of the device's MAC address, its USB serial number in hex,
/// so each device gets its own display arrangement.
fn virtual_serial(usb_serial: Option<&str>) -> u32 {
    usb_serial
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        .map_or(VIRTUAL_SERIAL, |mac| mac as u32)
}

fn create_virtual(
    info: &Info,
    serial: u32,
    rotation: u32,
    notify: &Notify,
) -> Result<VirtualDisplay> {
    let size = virtual_size(info, rotation);
    let millimeters = match rotation % 2 {
        1 => (PANEL_MILLIMETERS.1, PANEL_MILLIMETERS.0),
        _ => PANEL_MILLIMETERS,
    };
    let deadline = Instant::now() + VIRTUAL_APPEAR;
    // Fails while the one before, with the same identity, is still going away.
    let display = loop {
        match VirtualDisplay::new(VIRTUAL_NAME, serial, size, millimeters) {
            Ok(display) => break display,
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    };
    loop {
        let source = capture::mirror_primary(display.id()).unwrap_or(display.id());
        if capture::displays()?.iter().any(|d| d.id == source) {
            break;
        }
        if Instant::now() >= deadline {
            return Err("virtual display did not appear".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    notify(Notice::VirtualDisplay(size));
    Ok(display)
}

enum Stop {
    /// The device is gone.
    Lost(String),
    /// What to capture has changed.
    Changed,
    Fatal(String),
}

use Stop::{Changed, Fatal, Lost};

struct Session<'a> {
    options: &'a CaptureOptions,
    injector: Option<&'a mut Injector>,
    control: &'a Control<'a>,
    touch: &'a mut bool,
    source: Source<'a>,
    display: u64,
    /// Sized by the session to the rotation, rather than watched.
    virtual_display: Option<&'a VirtualDisplay>,
    size: (u32, u32),
    presenter: Presenter,
    /// What the running capture was started for.
    started: (LayoutKey, (u32, u32)),
    capture: Option<Capture>,
    /// The device's display is off, with no capture, until the display wakes.
    asleep: bool,
    frames: mpsc::Receiver<Frame>,
    sender: mpsc::SyncSender<Frame>,
    last: Option<Frame>,
    /// `last` has not been sent.
    pending: bool,
}

fn desktop_point(
    presenter: &Presenter,
    (x, y, w, h): (f64, f64, f64, f64),
    point: &TouchPoint,
) -> input::Point {
    let panel_width = presenter.info.panel_width as u32;
    let at = (point.x as f64, point.y as f64);
    let (u, v) = presenter.layout.source_point(panel_width, at);
    input::Point {
        id: point.id,
        x: x + u * w,
        y: y + v * h,
    }
}

fn layout(size: (u32, u32), info: &Info, rotation: u32) -> Result<Layout> {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    Layout::direct(size, rotation, panel, info.direct_align.max(1) as u32, true)
}

impl<'a> Session<'a> {
    /// Ok(()) when the display is gone or on stopping.
    fn run(
        connection: &mut Connection,
        source: Source<'a>,
        options: &'a CaptureOptions,
        injector: Option<&'a mut Injector>,
        control: &'a Control<'a>,
        touch: &'a mut bool,
    ) -> std::result::Result<(), Stop> {
        let display = source.display();
        let virtual_display = match source {
            Source::Virtual(virtual_display) => Some(virtual_display),
            _ => None,
        };
        let info = connection.info.clone();
        let rotation = options.rotation.unwrap_or(info.orientation as u32);
        if let Some(virtual_display) = virtual_display {
            // A mirror set leaves it at the set's mode.
            let size = virtual_size(&info, rotation);
            if capture::display(display).is_none_or(|d| d.size != size) {
                virtual_display.resize(size).map_err(Fatal)?;
                wait_for_size(display, size).map_err(Fatal)?;
            }
        }
        let Some(found) = capture::display(display) else {
            return Ok(());
        };
        let layout = layout(found.size, &info, rotation).map_err(Fatal)?;
        let (sender, frames) = mpsc::sync_channel(1);
        let mut session = Session {
            options,
            injector,
            control,
            touch,
            source,
            display,
            virtual_display,
            size: found.size,
            started: (present::layout_key(&info, rotation), found.size),
            presenter: Presenter::new(
                info,
                layout,
                Encoding::Jpeg,
                false,
                options.rotation,
                options.quality,
                control.notify.clone(),
            ),
            capture: None,
            asleep: false,
            frames,
            sender,
            last: None,
            pending: false,
        };
        session.start().map_err(Fatal)?;
        let mut checked = Instant::now();
        while !control.stopping() {
            let wait = match session.pending {
                true => session.presenter.until_due().min(POLL),
                false => POLL,
            };
            match session.frames.recv_timeout(wait) {
                Ok(frame) => {
                    session.last = Some(frame);
                    session.pending = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Fatal("capture stopped".into()));
                }
            }
            if let Some(reason) = session.capture.as_ref().and_then(Capture::stopped) {
                (control.notify)(Notice::CaptureRestarting(reason));
                thread::sleep(RESTART_DELAY);
                let restarted = match capture::asleep(display) {
                    true => Err("display asleep".into()),
                    false => session.restart(),
                };
                if let Err(e) = restarted {
                    session.sleep(connection, &e)?;
                }
            }
            if session.presenter.events(connection).map_err(Lost)? {
                session.relayout().map_err(Fatal)?;
            }
            let (touch_changed, brightness) = requests(control, session.touch);
            if touch_changed {
                let mode = session.presenter.info.input_mode[InputTarget::Touch as usize];
                let inject = session.injector.is_some() && *session.touch;
                set_touch(connection, mode, inject).map_err(Lost)?;
            }
            set_brightness(connection, brightness).map_err(Lost)?;
            session.inject();
            if checked.elapsed() >= DISPLAY_CHECK {
                checked = Instant::now();
                if session.asleep {
                    if !capture::asleep(display) {
                        session.wake();
                    }
                } else if capture::asleep(display) {
                    session.sleep(connection, "display asleep")?;
                }
                if session.source.changed() {
                    (control.notify)(Notice::MirroringChanged);
                    return Err(Changed);
                }
                if session.virtual_display.is_none() {
                    let Some(now) = capture::display(display) else {
                        (control.notify)(Notice::DisplayGone);
                        return Ok(());
                    };
                    if now.size != session.size {
                        (control.notify)(Notice::DisplaySize(now.size));
                        session.size = now.size;
                        session.relayout().map_err(Fatal)?;
                    }
                }
            }
            let wanted = session.pending || session.presenter.redraw;
            if wanted && session.last.is_some() && session.presenter.due() {
                session.send(connection).map_err(Lost)?;
            }
            if session.options.verbose {
                let audio_level = session.presenter.audio_level;
                session.presenter.stats.report(audio_level, &control.notify);
            }
        }
        Ok(())
    }

    fn inject(&mut self) {
        let touches = std::mem::take(&mut self.presenter.touches);
        let Some(injector) = self.injector.as_deref_mut() else {
            return;
        };
        let result = match capture::bounds(self.display) {
            Some(bounds) => touches.iter().try_for_each(|points| {
                let points: Vec<_> = points
                    .iter()
                    .map(|p| desktop_point(&self.presenter, bounds, p))
                    .collect();
                injector.touch(&points)
            }),
            None => Ok(()),
        };
        if let Err(e) = result.and_then(|()| injector.refresh()) {
            (self.control.notify)(Notice::Warning(format!("touch: {e}")));
        }
    }

    fn start(&mut self) -> Result<()> {
        let info = &self.presenter.info;
        let max_fps = info.max_fps.max(1) as u32;
        let capture = Capture::start(
            self.display,
            self.presenter.layout.frame(),
            max_fps,
            self.sender.clone(),
        )?;
        self.capture = Some(capture);
        self.presenter.describe();
        Ok(())
    }

    /// Restarts the capture at the size the current rotation and display take.
    fn relayout(&mut self) -> Result<()> {
        let rotation = self.presenter.rotation();
        if let Some(display) = self.virtual_display {
            let size = virtual_size(&self.presenter.info, rotation);
            if size != self.size {
                display.resize(size)?;
                wait_for_size(display.id(), size)?;
                (self.control.notify)(Notice::VirtualDisplay(size));
                self.size = size;
            }
        }
        let started = (
            present::layout_key(&self.presenter.info, rotation),
            self.size,
        );
        if started == self.started {
            return Ok(());
        }
        self.presenter.layout = layout(self.size, &self.presenter.info, rotation)?;
        self.presenter.clear();
        self.started = started;
        self.restart()
    }

    fn restart(&mut self) -> Result<()> {
        self.capture = None;
        while self.frames.try_recv().is_ok() {}
        self.last = None;
        self.pending = false;
        match self.asleep {
            true => Ok(()),
            false => self.start(),
        }
    }

    /// Stops capturing and turns the device's display off.
    fn sleep(
        &mut self,
        connection: &mut Connection,
        reason: &str,
    ) -> std::result::Result<(), Stop> {
        (self.control.notify)(Notice::DisplayOff(reason.into()));
        self.asleep = true;
        self.restart().map_err(Fatal)?;
        self.presenter.power_off(connection).map_err(Lost)
    }

    /// Captures again; the next frame sent turns the device's display on.
    fn wake(&mut self) {
        match self.start() {
            Ok(()) => {
                (self.control.notify)(Notice::DisplayOn);
                self.asleep = false;
            }
            Err(e) => (self.control.notify)(Notice::Warning(format!("capture: {e}"))),
        }
    }

    fn send(&mut self, connection: &mut Connection) -> Result<()> {
        let Some(frame) = &self.last else {
            return Ok(());
        };
        self.presenter.start_frame();
        let size = self.presenter.layout.frame();
        if (frame.width, frame.height) != (size.0 as usize, size.1 as usize) {
            return Ok(());
        }
        let quality = self.presenter.quality();
        let encoded = present::encode_jpeg(
            &image(frame),
            &self.presenter.layout,
            &self.presenter.info,
            quality,
        );
        self.pending = false;
        self.presenter.send(connection, encoded)
    }
}
