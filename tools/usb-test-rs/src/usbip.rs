use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;

use crate::transport::{Error, Kind, Request, Target, Transport, TIMEOUT};

const VERSION: u16 = 0x0111;
const OP_REQ_DEVLIST: u16 = 0x8005;
const OP_REQ_IMPORT: u16 = 0x8003;
const CMD_SUBMIT: u32 = 1;
const RET_SUBMIT: u32 = 3;
const DIR_OUT: u32 = 0;
const DIR_IN: u32 = 1;
const HEADER_BYTES: usize = 48;
const DEVICE_RECORD_BYTES: usize = 312;
const BUSID_BYTES: usize = 32;
const STATUS_PIPE: i32 = -32;
const SPEED_NAMES: [&str; 6] = ["Unknown", "Low", "Full", "High", "Wireless", "Super"];
const REQUEST_CLEAR_FEATURE: u8 = 0x01;
const FEATURE_ENDPOINT_HALT: u16 = 0;
const REQUEST_GET_DESCRIPTOR: u8 = 0x06;
const REQUEST_SET_CONFIGURATION: u8 = 0x09;
const DESC_CONFIG: u16 = 0x02;
const DESC_ENDPOINT: u16 = 0x05;

struct Ret {
    status: i32,
    actual: usize,
    data: Vec<u8>,
}

pub struct Usbip {
    stream: TcpStream,
    devid: u32,
    speed: u32,
    packet: usize,
    out: u8,
    input: u8,
    seqnum: u32,
    expects_data: HashMap<u32, bool>,
    done: HashMap<u32, Ret>,
    out_queue: VecDeque<u32>,
    in_queue: VecDeque<u32>,
}

fn io_error(e: std::io::Error) -> Error {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Error::Timeout,
        _ => Error::Other(e.to_string()),
    }
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

impl Usbip {
    /// Imports the first exported device and selects its configuration 1.
    pub fn connect(address: &str, out: u8, input: u8) -> Result<Self, String> {
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
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|e| e.to_string())?;
        let mut usbip = Usbip {
            stream,
            devid: be32(&record[288..]) << 16 | be32(&record[292..]),
            speed: be32(&record[296..]),
            packet: 0,
            out,
            input,
            seqnum: 0,
            expects_data: HashMap::new(),
            done: HashMap::new(),
            out_queue: VecDeque::new(),
            in_queue: VecDeque::new(),
        };
        usbip.configure().map_err(|e| format!("configure: {e}"))?;
        Ok(usbip)
    }

    fn configure(&mut self) -> Result<(), Error> {
        let get_config = |value| Request {
            kind: Kind::Standard,
            target: Target::Device,
            request: REQUEST_GET_DESCRIPTOR,
            value,
            index: 0,
        };
        let header = self.control_in(get_config(DESC_CONFIG << 8), 9)?;
        if header.len() < 4 {
            return Err(Error::Other("short configuration descriptor".into()));
        }
        let total = u16::from_le_bytes([header[2], header[3]]);
        let config = self.control_in(get_config(DESC_CONFIG << 8), total)?;
        let mut pos = 0;
        while pos + 6 < config.len() && config[pos] > 0 {
            if config[pos + 1] == DESC_ENDPOINT as u8 && config[pos + 2] == self.out {
                self.packet =
                    (u16::from_le_bytes([config[pos + 4], config[pos + 5]]) & 0x7ff) as usize;
            }
            pos += config[pos] as usize;
        }
        let set_config = Request {
            kind: Kind::Standard,
            target: Target::Device,
            request: REQUEST_SET_CONFIGURATION,
            value: 1,
            index: 0,
        };
        self.control_out(set_config, &[])
    }

    fn submit(&mut self, endpoint: u8, length: usize, setup: [u8; 8], data: &[u8]) -> u32 {
        self.seqnum += 1;
        let seqnum = self.seqnum;
        let input = endpoint & 0x80 != 0;
        let mut out = Vec::with_capacity(HEADER_BYTES + data.len());
        for value in [
            CMD_SUBMIT,
            seqnum,
            self.devid,
            if input { DIR_IN } else { DIR_OUT },
            (endpoint & 0x7f) as u32,
            0,
            length as u32,
            0,
            0,
            0,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&setup);
        out.extend_from_slice(data);
        self.expects_data.insert(seqnum, input);
        if let Err(e) = self.stream.write_all(&out) {
            self.done.insert(
                seqnum,
                Ret {
                    status: -1,
                    actual: 0,
                    data: e.to_string().into_bytes(),
                },
            );
        }
        seqnum
    }

    fn wait(&mut self, seqnum: u32) -> Result<Ret, Error> {
        loop {
            if let Some(ret) = self.done.remove(&seqnum) {
                return Ok(ret);
            }
            let mut header = [0u8; HEADER_BYTES];
            self.stream.read_exact(&mut header).map_err(io_error)?;
            if be32(&header) != RET_SUBMIT {
                return Err(Error::Other(format!(
                    "unexpected command {}",
                    be32(&header)
                )));
            }
            let id = be32(&header[4..]);
            let status = be32(&header[20..]) as i32;
            let actual = be32(&header[24..]) as usize;
            let mut data = Vec::new();
            if self.expects_data.remove(&id).unwrap_or(false) && actual > 0 {
                data.resize(actual, 0);
                self.stream.read_exact(&mut data).map_err(io_error)?;
            }
            self.done.insert(
                id,
                Ret {
                    status,
                    actual,
                    data,
                },
            );
        }
    }

    fn check(ret: Ret) -> Result<Ret, Error> {
        match ret.status {
            0 => Ok(ret),
            STATUS_PIPE => Err(Error::Stall),
            -1 => Err(Error::Other(
                String::from_utf8_lossy(&ret.data).into_owned(),
            )),
            status => Err(Error::Other(format!("status {status}"))),
        }
    }

    fn setup(request: Request, input: bool, length: u16) -> [u8; 8] {
        let kind = match request.kind {
            Kind::Standard => 0x00,
            Kind::Vendor => 0x40,
        };
        let target = match request.target {
            Target::Device => 0x00,
            Target::Endpoint => 0x02,
        };
        let value = request.value.to_le_bytes();
        let index = request.index.to_le_bytes();
        let length = length.to_le_bytes();
        [
            if input { 0x80 } else { 0 } | kind | target,
            request.request,
            value[0],
            value[1],
            index[0],
            index[1],
            length[0],
            length[1],
        ]
    }
}

impl Transport for Usbip {
    fn speed(&self) -> String {
        SPEED_NAMES
            .get(self.speed as usize)
            .unwrap_or(&"?")
            .to_string()
    }

    fn max_packet(&self) -> usize {
        self.packet
    }

    fn control_in(&mut self, request: Request, length: u16) -> Result<Vec<u8>, Error> {
        let seqnum = self.submit(
            0x80,
            length as usize,
            Self::setup(request, true, length),
            &[],
        );
        Ok(Self::check(self.wait(seqnum)?)?.data)
    }

    fn control_out(&mut self, request: Request, data: &[u8]) -> Result<(), Error> {
        let setup = Self::setup(request, false, data.len() as u16);
        let seqnum = self.submit(0x00, data.len(), setup, data);
        Self::check(self.wait(seqnum)?).map(|_| ())
    }

    fn clear_halt_out(&mut self) -> Result<(), Error> {
        let request = Request {
            kind: Kind::Standard,
            target: Target::Endpoint,
            request: REQUEST_CLEAR_FEATURE,
            value: FEATURE_ENDPOINT_HALT,
            index: self.out as u16,
        };
        self.control_out(request, &[])
    }

    fn submit_out(&mut self, data: Vec<u8>) {
        let seqnum = self.submit(self.out, data.len(), [0; 8], &data);
        self.out_queue.push_back(seqnum);
    }

    fn submit_in(&mut self, length: usize) {
        let seqnum = self.submit(self.input, length, [0; 8], &[]);
        self.in_queue.push_back(seqnum);
    }

    fn pending_out(&self) -> usize {
        self.out_queue.len()
    }

    fn wait_out(&mut self) -> Result<usize, Error> {
        let seqnum = self
            .out_queue
            .pop_front()
            .ok_or(Error::Other("nothing queued".into()))?;
        Ok(Self::check(self.wait(seqnum)?)?.actual)
    }

    fn wait_in(&mut self) -> Result<Vec<u8>, Error> {
        let seqnum = self
            .in_queue
            .pop_front()
            .ok_or(Error::Other("nothing queued".into()))?;
        Ok(Self::check(self.wait(seqnum)?)?.data)
    }
}
