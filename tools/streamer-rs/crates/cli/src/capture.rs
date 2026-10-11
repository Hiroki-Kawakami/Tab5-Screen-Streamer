//! Streaming a display. Capture runs only while the device is connected.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use streamer_capture::{self as capture, Capture, Format, Frame};
use streamer_core::jpeg::{Image, Pixels};
use streamer_core::layout::Layout;
use streamer_core::protocol::{Encoding, Info};
use streamer_device::connection::{Connection, Device};

use crate::present::{self, LayoutKey, Presenter};

/// How long to wait for a frame before handling device events.
const POLL: Duration = Duration::from_millis(50);
const DISPLAY_CHECK: Duration = Duration::from_secs(1);

type Result<T> = std::result::Result<T, String>;

pub struct CaptureOptions {
    /// An index into the displays list; the main display when None.
    pub display: Option<usize>,
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
    let display = match options.display {
        Some(index) => *capture::displays()?
            .get(index)
            .ok_or_else(|| format!("no display {index}"))?,
        None => capture::main_display()?,
    };
    while let Some(mut connection) = device.wait() {
        match Session::run(&mut connection, display.id, &options) {
            Ok(()) => return Ok(()),
            Err(Lost(e)) => device.lost(connection, &e),
            Err(Fatal(e)) => return Err(e),
        }
    }
    Ok(())
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

impl Session<'_> {
    /// Ok(()) when the display is gone.
    fn run(
        connection: &mut Connection,
        display: u64,
        options: &CaptureOptions,
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
            if session.presenter.events(connection).map_err(Lost)? {
                session.relayout().map_err(Fatal)?;
            }
            if checked.elapsed() >= DISPLAY_CHECK {
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
