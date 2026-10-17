//! Streaming on its own thread, and the state the panel and the tray show.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use streamer_core::protocol::{Event, InputMode, InputTarget};
use streamer_device::connection::{Address, Device};
use streamer_stream::capture::{self, CaptureOptions};
use streamer_stream::present::Quality;
use streamer_stream::{Control, Notice, Notify, Request};

use crate::settings::{DeviceSettings, Source};

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Off,
    Waiting,
    Streaming,
}

#[derive(Clone)]
pub struct Status {
    pub phase: Phase,
    /// Touch input is enabled on the device.
    pub touch: bool,
    /// While connected.
    pub brightness: Option<u8>,
    /// The id of the display made for the device, while there is one.
    pub virtual_display: Option<u64>,
    pub message: Option<String>,
}

impl Status {
    pub const OFF: Status = Status {
        phase: Phase::Off,
        touch: false,
        brightness: None,
        virtual_display: None,
        message: None,
    };
}

/// True when the panel shows the change. `fallback` is why another display
/// than the chosen one is captured.
fn update(status: &mut Status, notice: Notice, fallback: &Option<String>) -> bool {
    match notice {
        Notice::Connected(info) => {
            status.phase = Phase::Streaming;
            status.touch = info.input_mode[InputTarget::Touch as usize] != InputMode::None;
            status.brightness = Some(info.brightness);
            status.message = fallback.clone();
        }
        Notice::Disconnected(_) => {
            status.phase = Phase::Waiting;
            status.brightness = None;
            status.virtual_display = None;
        }
        Notice::VirtualDisplay { id, .. } => status.virtual_display = Some(id),
        Notice::Event(Event::InputMode(InputTarget::Touch, mode)) => {
            status.touch = mode != InputMode::None;
        }
        Notice::Event(Event::Brightness(brightness)) => status.brightness = Some(brightness),
        Notice::Warning(warning) => status.message = Some(warning),
        _ => return false,
    }
    true
}

pub struct Streaming {
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    requests: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl Streaming {
    /// `changed` runs on the streaming thread after each change of the
    /// status.
    pub fn start(
        serial: &str,
        settings: &DeviceSettings,
        changed: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (requests, receiver) = mpsc::channel();
        let source = settings.source;
        let serial = serial.to_string();
        let mut options = CaptureOptions {
            display: None,
            virtual_display: settings.source == Source::Virtual,
            rotation: settings.rotation,
            quality: Quality::DEFAULT,
            touch: settings.touch,
            verbose: false,
        };
        let status = Arc::new(Mutex::new(Status {
            phase: Phase::Waiting,
            ..Status::OFF
        }));
        let shared = status.clone();
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            let status = shared;
            let mut fallback = None;
            if let Source::Display(saved) = source {
                options.display = find_display(saved);
                if options.display.is_none() {
                    fallback = Some("display not found, streaming the main display".into());
                    status.lock().unwrap().message = fallback.clone();
                    changed();
                }
            }
            let notify: Notify = {
                let status = status.clone();
                let changed = changed.clone();
                Arc::new(move |notice| {
                    if update(&mut status.lock().unwrap(), notice, &fallback) {
                        changed();
                    }
                })
            };
            let control = Control {
                stop: &stopping,
                requests: Some(&receiver),
                notify,
            };
            let waiting = {
                let status = status.clone();
                let changed = changed.clone();
                move |reason| {
                    status.lock().unwrap().message = Some(reason);
                    changed();
                }
            };
            let device = Device::spawn(Address::Usb(Some(serial)), waiting);
            let result = capture::capture(&device, options, &control);
            *status.lock().unwrap() = Status {
                message: result.err(),
                ..Status::OFF
            };
            changed();
        });
        Streaming {
            status,
            stop,
            requests,
            thread: Some(thread),
        }
    }

    /// Off once streaming has ended by itself, with the reason if it failed.
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn request(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// Waits for the session to close.
    pub fn join(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What `Source::Display` keeps for the display at `index` in the displays
/// list.
pub fn saved_display(index: usize, display: &streamer_capture::Display) -> u64 {
    match cfg!(target_os = "macos") {
        true => display.id,
        false => index as u64,
    }
}

fn find_display(saved: u64) -> Option<u64> {
    let displays = streamer_capture::displays().ok()?;
    let mut displays = displays.iter().enumerate();
    displays
        .find(|(index, display)| saved_display(*index, display) == saved)
        .map(|(_, display)| display.id)
}

impl Drop for Streaming {
    /// Returns at once; the session closes in the background.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
