//! USB/IP client for the simulator, which exports the device on TCP.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::link::{self, Error, InPipe, Link, OutPipe};

const VERSION: u16 = 0x0111;
const OP_REQ_DEVLIST: u16 = 0x8005;
const OP_REQ_IMPORT: u16 = 0x8003;
const CMD_SUBMIT: u32 = 1;
const RET_SUBMIT: u32 = 3;
const DIR_OUT: u32 = 0;
const DIR_IN: u32 = 1;
const URB_ZERO_PACKET: u32 = 0x40;
const HEADER_BYTES: usize = 48;
const DEVICE_RECORD_BYTES: usize = 312;
const BUSID_BYTES: usize = 32;
const SPEED_HIGH: u32 = 3;
const REQUEST_SET_CONFIGURATION: u8 = 0x09;
const SETUP_TIMEOUT: Duration = Duration::from_secs(2);

struct Ret {
    status: i32,
    data: Vec<u8>,
}

#[derive(Default)]
struct State {
    expects_data: HashMap<u32, bool>,
    done: HashMap<u32, Ret>,
    closed: Option<String>,
}

struct Shared {
    writer: Mutex<TcpStream>,
    seqnum: AtomicU32,
    devid: u32,
    state: Mutex<State>,
    changed: Condvar,
}

fn op_header(code: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.extend_from_slice(&code.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes[..4].try_into().unwrap())
}

fn first_busid(address: &str) -> Result<[u8; BUSID_BYTES], String> {
    let mut stream = TcpStream::connect(address).map_err(|e| format!("connect: {e}"))?;
    stream
        .write_all(&op_header(OP_REQ_DEVLIST))
        .map_err(|e| e.to_string())?;
    let mut header = [0u8; 12];
    stream
        .read_exact(&mut header)
        .map_err(|e| format!("devlist: {e}"))?;
    if be32(&header[8..]) == 0 {
        return Err("no device exported".into());
    }
    let mut record = [0u8; DEVICE_RECORD_BYTES];
    stream
        .read_exact(&mut record)
        .map_err(|e| format!("devlist: {e}"))?;
    let mut busid = [0u8; BUSID_BYTES];
    busid.copy_from_slice(&record[256..256 + BUSID_BYTES]);
    Ok(busid)
}

impl Shared {
    fn submit(&self, endpoint: u8, length: usize, flags: u32, setup: [u8; 8], data: &[u8]) -> u32 {
        let seqnum = self.seqnum.fetch_add(1, Ordering::Relaxed) + 1;
        let input = endpoint & 0x80 != 0;
        let mut out = Vec::with_capacity(HEADER_BYTES + data.len());
        for value in [
            CMD_SUBMIT,
            seqnum,
            self.devid,
            if input { DIR_IN } else { DIR_OUT },
            (endpoint & 0x7f) as u32,
            flags,
            length as u32,
            0,
            0,
            0,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&setup);
        out.extend_from_slice(data);
        self.state
            .lock()
            .unwrap()
            .expects_data
            .insert(seqnum, input);
        if let Err(e) = self.writer.lock().unwrap().write_all(&out) {
            let mut state = self.state.lock().unwrap();
            state.closed = Some(e.to_string());
            self.changed.notify_all();
        }
        seqnum
    }

    fn wait(&self, seqnum: u32, timeout: Duration) -> Result<Ret, Error> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(ret) = state.done.remove(&seqnum) {
                return match ret.status {
                    0 => Ok(ret),
                    status => Err(Error::Other(format!("status {status}"))),
                };
            }
            if let Some(reason) = &state.closed {
                return Err(Error::Other(reason.clone()));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            state = self.changed.wait_timeout(state, deadline - now).unwrap().0;
        }
    }

    fn read_loop(&self, mut stream: TcpStream) {
        let reason = loop {
            let mut header = [0u8; HEADER_BYTES];
            if let Err(e) = stream.read_exact(&mut header) {
                break e.to_string();
            }
            if be32(&header) != RET_SUBMIT {
                break format!("unexpected command {}", be32(&header));
            }
            let id = be32(&header[4..]);
            let status = be32(&header[20..]) as i32;
            let actual = be32(&header[24..]) as usize;
            let input = self
                .state
                .lock()
                .unwrap()
                .expects_data
                .remove(&id)
                .unwrap_or(false);
            let mut data = Vec::new();
            if input && actual > 0 {
                data.resize(actual, 0);
                if let Err(e) = stream.read_exact(&mut data) {
                    break e.to_string();
                }
            }
            let mut state = self.state.lock().unwrap();
            state.done.insert(id, Ret { status, data });
            self.changed.notify_all();
        };
        let mut state = self.state.lock().unwrap();
        state.closed = Some(reason);
        self.changed.notify_all();
    }
}

struct UsbipOut {
    shared: Arc<Shared>,
    endpoint: u8,
    packet: usize,
    queue: VecDeque<u32>,
}

impl OutPipe for UsbipOut {
    fn submit(&mut self, data: Vec<u8>) {
        let flags = if data.len().is_multiple_of(self.packet) {
            URB_ZERO_PACKET
        } else {
            0
        };
        let seqnum = self
            .shared
            .submit(self.endpoint, data.len(), flags, [0; 8], &data);
        self.queue.push_back(seqnum);
    }

    fn pending(&self) -> usize {
        self.queue.len()
    }

    fn wait(&mut self, timeout: Duration) -> Result<(), Error> {
        let seqnum = *self
            .queue
            .front()
            .ok_or(Error::Other("nothing queued".into()))?;
        let result = self.shared.wait(seqnum, timeout);
        if !matches!(result, Err(Error::Timeout)) {
            self.queue.pop_front();
        }
        result.map(|_| ())
    }
}

struct UsbipIn {
    shared: Arc<Shared>,
    endpoint: u8,
    queue: VecDeque<u32>,
}

impl InPipe for UsbipIn {
    fn submit(&mut self, length: usize) {
        let seqnum = self.shared.submit(self.endpoint, length, 0, [0; 8], &[]);
        self.queue.push_back(seqnum);
    }

    fn wait(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let seqnum = *self
            .queue
            .front()
            .ok_or(Error::Other("nothing queued".into()))?;
        let result = self.shared.wait(seqnum, timeout);
        if !matches!(result, Err(Error::Timeout)) {
            self.queue.pop_front();
        }
        result.map(|ret| ret.data)
    }
}

/// Imports the first exported device and selects its configuration 1.
pub fn connect(address: &str) -> Result<Link, String> {
    let busid = first_busid(address)?;
    let mut stream = TcpStream::connect(address).map_err(|e| format!("connect: {e}"))?;
    let mut request = op_header(OP_REQ_IMPORT);
    request.extend_from_slice(&busid);
    stream.write_all(&request).map_err(|e| e.to_string())?;
    let mut header = [0u8; 8];
    stream
        .read_exact(&mut header)
        .map_err(|e| format!("import: {e}"))?;
    if be32(&header[4..]) != 0 {
        return Err("import refused".into());
    }
    let mut record = [0u8; DEVICE_RECORD_BYTES];
    stream
        .read_exact(&mut record)
        .map_err(|e| format!("import: {e}"))?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let reader = stream.try_clone().map_err(|e| e.to_string())?;

    let shared = Arc::new(Shared {
        writer: Mutex::new(stream),
        seqnum: AtomicU32::new(0),
        devid: be32(&record[288..]) << 16 | be32(&record[292..]),
        state: Mutex::new(State::default()),
        changed: Condvar::new(),
    });
    let packet = if be32(&record[296..]) == SPEED_HIGH {
        512
    } else {
        64
    };
    let background = shared.clone();
    thread::spawn(move || background.read_loop(reader));

    let set_configuration = [0x00, REQUEST_SET_CONFIGURATION, 1, 0, 0, 0, 0, 0];
    let seqnum = shared.submit(0x00, 0, 0, set_configuration, &[]);
    shared
        .wait(seqnum, SETUP_TIMEOUT)
        .map_err(|e| format!("set configuration: {e}"))?;

    let out = |endpoint| UsbipOut {
        shared: shared.clone(),
        endpoint,
        packet,
        queue: VecDeque::new(),
    };
    Ok(Link {
        control_out: Box::new(out(link::CONTROL_OUT)),
        control_in: Box::new(UsbipIn {
            shared: shared.clone(),
            endpoint: link::CONTROL_IN,
            queue: VecDeque::new(),
        }),
        video_out: Box::new(out(link::VIDEO_OUT)),
    })
}
