use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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
const OPEN_RETRY: Duration = Duration::from_millis(200);
/// Well inside the device's 3 s session timeout.
const KEEPALIVE: Duration = Duration::from_secs(1);

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

/// One session with the device, from OPEN until it is closed or a transfer
/// fails. Every error from it means the session is gone; open a new one.
pub struct Connection {
    control: Arc<Mutex<Control>>,
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

/// When anything was last submitted on any OUT endpoint.
struct Activity {
    start: Instant,
    last_ms: AtomicU64,
}

impl Activity {
    fn touch(&self) {
        let now = self.start.elapsed().as_millis() as u64;
        self.last_ms.store(now, Ordering::Relaxed);
    }

    fn idle(&self) -> Duration {
        let last = Duration::from_millis(self.last_ms.load(Ordering::Relaxed));
        self.start.elapsed().saturating_sub(last)
    }
}

struct Tracked {
    pipe: Box<dyn OutPipe>,
    activity: Arc<Activity>,
}

impl OutPipe for Tracked {
    fn submit(&mut self, data: Vec<u8>) {
        self.activity.touch();
        self.pipe.submit(data);
    }

    fn pending(&self) -> usize {
        self.pipe.pending()
    }

    fn wait(&mut self, timeout: Duration) -> std::result::Result<(), link::Error> {
        self.pipe.wait(timeout)
    }
}

/// Stops the control IN reader and the keep-alive, releasing their endpoints,
/// when dropped.
struct Reader {
    stop: Arc<AtomicBool>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sends OPEN until it is answered with INFO; events before it are from an
/// earlier session and dropped. A retry gets past a transfer an earlier
/// session cut short, which the device drops along with the OPEN after it.
fn open_session(control: &mut Control, events: &Receiver<Event>) -> Result<Info> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let seq = control.send(protocol::OPEN, &[])?;
        let retry = (Instant::now() + OPEN_RETRY).min(deadline);
        loop {
            let left = retry.saturating_duration_since(Instant::now());
            match events.recv_timeout(left) {
                Ok(Event::Info { seq: answered, info }) if answered == seq => return Ok(info),
                Ok(event) => report(&event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Err("control in closed".into()),
            }
        }
        if Instant::now() >= deadline {
            return Err("no INFO".into());
        }
    }
}

fn keep_alive(control: Arc<Mutex<Control>>, activity: Arc<Activity>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        let idle = activity.idle();
        if idle < KEEPALIVE {
            thread::sleep((KEEPALIVE - idle).min(POLL));
            continue;
        }
        if control.lock().unwrap().send(protocol::KEEPALIVE, &[]).is_err() {
            return;
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
        let activity = Arc::new(Activity {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
        });
        let tracked = |pipe| -> Box<dyn OutPipe> {
            Box::new(Tracked {
                pipe,
                activity: activity.clone(),
            })
        };
        let mut control = Control {
            out: tracked(link.control_out),
            seq: 0,
        };
        let info = open_session(&mut control, &events)?;
        let control = Arc::new(Mutex::new(control));
        let (keep_control, keep_stop) = (control.clone(), reader.stop.clone());
        let video_out = tracked(link.video_out);
        let audio_out = tracked(link.audio_out);
        thread::spawn(move || keep_alive(keep_control, activity, keep_stop));
        Ok(Connection {
            control,
            video_out,
            audio: Some(AudioOut {
                pipe: audio_out,
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
        self.control.lock().unwrap().send(kind, payload)
    }

    /// The next event within `timeout`, or None. EV_CLOSED is an error.
    pub fn event(&self, timeout: Duration) -> Result<Option<Event>> {
        match self.events.recv_timeout(timeout) {
            Ok(Event::Closed) => Err("the device closed the session".into()),
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

    /// Ends the session once the video sent so far is taken; the device goes
    /// back to its own screen.
    pub fn close(mut self) -> Result<()> {
        self.flush_video()?;
        self.control(protocol::CLOSE, &[])?;
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
/// the previous one is given up, until dropped.
pub struct Device {
    connections: Receiver<Connection>,
    lost: Sender<()>,
    stop: Arc<AtomicBool>,
}

impl Device {
    pub fn spawn(address: Address) -> Self {
        let (sender, connections) = mpsc::channel();
        let (lost, lost_receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        thread::spawn(move || {
            while let Some(connection) = connect(&address, &stopped) {
                println!("connected");
                if sender.send(connection).is_err() || lost_receiver.recv().is_err() {
                    return;
                }
            }
        });
        Device {
            connections,
            lost,
            stop,
        }
    }

    /// A new connection, if one has been opened.
    pub fn poll(&self) -> Option<Connection> {
        self.connections.try_recv().ok()
    }

    pub fn wait(&self, timeout: Duration) -> Option<Connection> {
        self.connections.recv_timeout(timeout).ok()
    }

    /// Gives up `connection` and starts opening the next one.
    pub fn lost(&self, connection: Connection, reason: &str) {
        eprintln!("disconnected: {reason}");
        drop(connection);
        let _ = self.lost.send(());
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Polled rather than nusb::watch_devices: the same loop retries USB/IP, and a
// device that has just arrived cannot always be opened yet.
fn connect(address: &Address, stop: &AtomicBool) -> Option<Connection> {
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        match Connection::open(address) {
            Ok(connection) => return Some(connection),
            Err(e) => {
                if e != last_error {
                    eprintln!("waiting for the device: {e}");
                    last_error = e;
                }
                thread::sleep(RETRY);
            }
        }
    }
    None
}
