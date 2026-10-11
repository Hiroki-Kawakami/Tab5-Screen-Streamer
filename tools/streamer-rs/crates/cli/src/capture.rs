//! Streaming a display. Capture runs only while the device is connected.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use streamer_capture::{self as capture, Capture, Format, Frame, VirtualDisplay};
use streamer_core::jpeg::{Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::protocol::{Encoding, Info};
use streamer_device::connection::{Connection, Device};

use crate::present::{self, LayoutKey, Presenter};

/// How long to wait for a frame before handling device events.
const POLL: Duration = Duration::from_millis(50);
const DISPLAY_CHECK: Duration = Duration::from_secs(1);
/// Before restarting a capture the OS has stopped.
const RESTART_DELAY: Duration = Duration::from_millis(500);
/// How long a new virtual display may take to become capturable.
const VIRTUAL_APPEAR: Duration = Duration::from_secs(3);
const VIRTUAL_NAME: &str = "M5Stack Tab5";
/// The panel's physical size, portrait.
const PANEL_MILLIMETERS: (f64, f64) = (62.0, 110.0);
type Result<T> = std::result::Result<T, String>;

pub struct CaptureOptions {
    /// An index into the displays list; the main display when None.
    pub display: Option<usize>,
    /// Capture a virtual display made for the device instead.
    pub virtual_display: bool,
    pub rotation: Option<u32>,
    pub quality: u8,
}

pub fn list() -> Result<()> {
    if !capture::authorize() {
        return Err("screen capture is not permitted".into());
    }
    let main = capture::main_display()?.id;
    for (index, display) in capture::displays()?.iter().enumerate() {
        println!(
            "{index}: {}x{}{}",
            display.size.0,
            display.size.1,
            if display.id == main { " (main)" } else { "" }
        );
    }
    Ok(())
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
pub fn capture(device: &Device, options: CaptureOptions) -> Result<()> {
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
    while let Some(mut connection) = device.wait() {
        let created;
        let (id, virtual_display) = match display {
            Some(display) => (display.id, None),
            None => {
                let rotation = options
                    .rotation
                    .unwrap_or(connection.info.orientation as u32);
                created = create_virtual(&connection.info, rotation)?;
                (created.id(), Some(&created))
            }
        };
        match Session::run(&mut connection, id, virtual_display, &options) {
            Ok(()) => return Ok(()),
            Err(Lost(e)) => device.lost(connection, &e),
            Err(Fatal(e)) => return Err(e),
        }
    }
    Ok(())
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

/// A virtual display for the panel, once it can be captured.
fn create_virtual(info: &Info, rotation: u32) -> Result<VirtualDisplay> {
    let size = virtual_size(info, rotation);
    let millimeters = match rotation % 2 {
        1 => (PANEL_MILLIMETERS.1, PANEL_MILLIMETERS.0),
        _ => PANEL_MILLIMETERS,
    };
    let display = VirtualDisplay::new(VIRTUAL_NAME, size, millimeters)?;
    let deadline = Instant::now() + VIRTUAL_APPEAR;
    while !capture::displays()?.iter().any(|d| d.id == display.id()) {
        if Instant::now() >= deadline {
            return Err("virtual display did not appear".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    println!("virtual display {}x{}", size.0, size.1);
    Ok(display)
}

enum Stop {
    /// The device is gone.
    Lost(String),
    Fatal(String),
}

use Stop::{Fatal, Lost};

struct Session<'a> {
    options: &'a CaptureOptions,
    display: u64,
    /// Sized by the session to the rotation, rather than watched.
    virtual_display: Option<&'a VirtualDisplay>,
    size: (u32, u32),
    presenter: Presenter,
    /// What the running capture was started for.
    started: (LayoutKey, (u32, u32)),
    capture: Option<Capture>,
    frames: mpsc::Receiver<Frame>,
    sender: mpsc::SyncSender<Frame>,
    last: Option<Frame>,
    /// `last` has not been sent.
    pending: bool,
}

fn layout(size: (u32, u32), info: &Info, rotation: u32) -> Result<Layout> {
    let panel = (info.panel_width as u32, info.panel_height as u32);
    Layout::direct(size, rotation, panel, info.direct_align.max(1) as u32, true)
}

impl<'a> Session<'a> {
    /// Ok(()) when the display is gone.
    fn run(
        connection: &mut Connection,
        display: u64,
        virtual_display: Option<&'a VirtualDisplay>,
        options: &'a CaptureOptions,
    ) -> std::result::Result<(), Stop> {
        let Some(found) = capture::display(display) else {
            return Ok(());
        };
        let info = connection.info.clone();
        let rotation = options.rotation.unwrap_or(info.orientation as u32);
        let layout = layout(found.size, &info, rotation).map_err(Fatal)?;
        let (sender, frames) = mpsc::sync_channel(1);
        let mut session = Session {
            options,
            display,
            virtual_display,
            size: found.size,
            started: (present::layout_key(&info, rotation), found.size),
            presenter: Presenter::new(info, layout, Encoding::Jpeg, false, options.rotation),
            capture: None,
            frames,
            sender,
            last: None,
            pending: false,
        };
        session.start().map_err(Fatal)?;
        let mut checked = Instant::now();
        loop {
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
                println!("capture stopped ({reason}), restarting");
                thread::sleep(RESTART_DELAY);
                session.restart().map_err(Fatal)?;
            }
            if session.presenter.events(connection).map_err(Lost)? {
                session.relayout().map_err(Fatal)?;
            }
            if session.virtual_display.is_none() && checked.elapsed() >= DISPLAY_CHECK {
                checked = Instant::now();
                let Some(now) = capture::display(display) else {
                    println!("display gone");
                    return Ok(());
                };
                if now.size != session.size {
                    println!("display now {}x{}", now.size.0, now.size.1);
                    session.size = now.size;
                    session.relayout().map_err(Fatal)?;
                }
            }
            let wanted = session.pending || session.presenter.redraw;
            if wanted && session.last.is_some() && session.presenter.due() {
                session.send(connection).map_err(Lost)?;
            }
            let audio_level = session.presenter.audio_level;
            session.presenter.stats.report(audio_level);
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
                println!("virtual display {}x{}", size.0, size.1);
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
        self.presenter.redraw = true;
        self.started = started;
        self.restart()
    }

    fn restart(&mut self) -> Result<()> {
        self.capture = None;
        while self.frames.try_recv().is_ok() {}
        self.last = None;
        self.pending = false;
        self.start()
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
        let encoded = present::encode_jpeg(
            &image(frame),
            &self.presenter.layout,
            &self.presenter.info,
            self.options.quality,
        );
        self.pending = false;
        self.presenter.send(connection, encoded)
    }
}
