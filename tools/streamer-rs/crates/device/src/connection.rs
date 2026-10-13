use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::protocol::{self, Event, Info};

use crate::link::{self, InPipe, Link, OutPipe};
use crate::usbip;

const IN_BYTES: usize = 512;
const IN_QUEUE: usize = 2;
const VIDEO_QUEUE: usize = 2;
pub const TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);
const RETRY: Duration = Duration::from_millis(500);

type Result<T> = std::result::Result<T, String>;

#[derive(Clone)]
pub enum Address {
    Usb,
    Usbip(String),
}

impl Address {
    fn link(&self) -> Result<Link> {
        match self {
            Address::Usb => link::open_nusb(),
            Address::Usbip(address) => usbip::connect(address),
        }
    }
}

/// The device's last EV_AUDIO, and how many have come.
#[derive(Default)]
pub struct AudioLevel {
    pub buffered: AtomicU32,
    pub reports: AtomicU32,
}

/// The bulk audio endpoint of one connection, handed to whoever streams audio.
/// Drop it once the connection is closed, or the next one cannot claim the
/// interface.
pub struct AudioOut {
    pub pipe: Box<dyn OutPipe>,
    pub level: Arc<AudioLevel>,
    closed: Arc<AtomicBool>,
}

impl AudioOut {
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}

/// One session with the device, from GET_INFO until a transfer fails. Every
/// error from it means the device is gone; open a new one.
pub struct Connection {
    control: Control,
    video_out: Box<dyn OutPipe>,
    audio: Option<AudioOut>,
    events: Receiver<Event>,
    _reader: Reader,
    pub info: Info,
    pub serial: Option<String>,
    video_seq: u32,
}

struct Control {
    out: Box<dyn OutPipe>,
    seq: u32,
}

impl Control {
    fn send(&mut self, kind: u8, payload: &[u8]) -> Result<u32> {
        self.seq += 1;
        self.out
            .submit(protocol::message(kind, self.seq, 0, payload));
        self.out
            .wait(TIMEOUT)
            .map_err(|e| format!("control out: {e}"))?;
        Ok(self.seq)
    }
}

/// Stops the control IN reader, releasing its endpoint, when dropped.
struct Reader {
    stop: Arc<AtomicBool>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sends GET_INFO and waits for INFO; events before it are dropped.
fn query_info(control: &mut Control, events: &Receiver<Event>) -> Result<Info> {
    control.send(protocol::GET_INFO, &[])?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(Event::Info(info)) => return Ok(info),
            Ok(event) => report(&event),
            Err(RecvTimeoutError::Timeout) => return Err("no INFO".into()),
            Err(RecvTimeoutError::Disconnected) => return Err("control in closed".into()),
        }
    }
}

fn read_events(
    mut input: Box<dyn InPipe>,
    events: Sender<Event>,
    level: Arc<AudioLevel>,
    stop: Arc<AtomicBool>,
) {
    for _ in 0..IN_QUEUE {
        input.submit(IN_BYTES);
    }
    while !stop.load(Ordering::Relaxed) {
        match input.wait(POLL) {
            Ok(data) => {
                for event in protocol::parse_events(&data) {
                    if let Event::Audio { buffered, .. } = event {
                        level.buffered.store(buffered, Ordering::Relaxed);
                        level.reports.fetch_add(1, Ordering::Release);
                    }
                    if events.send(event).is_err() {
                        return;
                    }
                }
                input.submit(IN_BYTES);
            }
            Err(link::Error::Timeout) => {}
            Err(e) => {
                eprintln!("control in: {e}");
                return;
            }
        }
    }
}

impl Connection {
    pub fn open(address: &Address) -> Result<Self> {
        let link = address.link()?;
        let (sender, events) = mpsc::channel();
        let level = Arc::new(AudioLevel::default());
        let reader = Reader {
            stop: Arc::new(AtomicBool::new(false)),
        };
        let control_in = link.control_in;
        let (reader_level, reader_stop) = (level.clone(), reader.stop.clone());
        thread::spawn(move || read_events(control_in, sender, reader_level, reader_stop));
        let mut control = Control {
            out: link.control_out,
            seq: 0,
        };
        let info = query_info(&mut control, &events)?;
        Ok(Connection {
            control,
            video_out: link.video_out,
            audio: Some(AudioOut {
                pipe: link.audio_out,
                level,
                closed: reader.stop.clone(),
            }),
            events,
            _reader: reader,
            info,
            serial: link.serial,
            video_seq: 0,
        })
    }

    /// Sends a control request; returns its seq.
    pub fn control(&mut self, kind: u8, payload: &[u8]) -> Result<u32> {
        self.control.send(kind, payload)
    }

    pub fn query_info(&mut self) -> Result<Info> {
        query_info(&mut self.control, &self.events)
    }

    /// The next event within `timeout`, or None.
    pub fn event(&self, timeout: Duration) -> Result<Option<Event>> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("control in closed".into()),
        }
    }

    /// Hands the audio endpoint over; None after the first call.
    pub fn take_audio(&mut self) -> Option<AudioOut> {
        self.audio.take()
    }

    pub fn next_video_seq(&mut self) -> u32 {
        self.video_seq += 1;
        self.video_seq
    }

    pub fn send_video(&mut self, transfer: Vec<u8>) -> Result<()> {
        while self.video_out.pending() >= VIDEO_QUEUE {
            self.video_out
                .wait(TIMEOUT)
                .map_err(|e| format!("video: {e}"))?;
        }
        self.video_out.submit(transfer);
        Ok(())
    }

    pub fn flush_video(&mut self) -> Result<()> {
        while self.video_out.pending() > 0 {
            self.video_out
                .wait(TIMEOUT)
                .map_err(|e| format!("video: {e}"))?;
        }
        Ok(())
    }
}

pub fn report(event: &Event) {
    if let Event::Error { seq, channel, code } = event {
        eprintln!(
            "device: {} on channel {channel} seq {seq}",
            protocol::error_name(*code)
        );
    }
}

/// Keeps a connection to the device: opens one in the background whenever
/// the previous one is given up.
pub struct Device {
    connections: Receiver<Connection>,
    lost: Sender<()>,
}

impl Device {
    pub fn spawn(address: Address) -> Self {
        let (sender, connections) = mpsc::channel();
        let (lost, lost_receiver) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let connection = connect(&address);
                println!("connected");
                if sender.send(connection).is_err() || lost_receiver.recv().is_err() {
                    return;
                }
            }
        });
        Device { connections, lost }
    }

    /// A new connection, if one has been opened.
    pub fn poll(&self) -> Option<Connection> {
        self.connections.try_recv().ok()
    }

    pub fn wait(&self) -> Option<Connection> {
        self.connections.recv().ok()
    }

    /// Gives up `connection` and starts opening the next one.
    pub fn lost(&self, connection: Connection, reason: &str) {
        eprintln!("disconnected: {reason}");
        drop(connection);
        let _ = self.lost.send(());
    }
}

// Polled rather than nusb::watch_devices: the same loop retries USB/IP, and a
// device that has just arrived cannot always be opened yet.
fn connect(address: &Address) -> Connection {
    let mut last_error = String::new();
    loop {
        match Connection::open(address) {
            Ok(connection) => return connection,
            Err(e) => {
                if e != last_error {
                    eprintln!("waiting for the device: {e}");
                    last_error = e;
                }
                thread::sleep(RETRY);
            }
        }
    }
}
