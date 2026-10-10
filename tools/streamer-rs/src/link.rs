use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use nusb::transfer::{Buffer, Bulk, In, Out};
use nusb::{Endpoint, MaybeFuture};

pub const VENDOR_ID: u16 = 0xf055;
pub const PRODUCT_ID: u16 = 0x1118;
pub const CONTROL_INTERFACE: u8 = 0;
pub const VIDEO_INTERFACE: u8 = 1;
pub const CONTROL_OUT: u8 = 0x01;
pub const CONTROL_IN: u8 = 0x81;
pub const VIDEO_OUT: u8 = 0x02;

#[derive(Debug)]
pub enum Error {
    Timeout,
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout => write!(f, "timed out"),
            Error::Other(e) => write!(f, "{e}"),
        }
    }
}

/// A bulk OUT endpoint. Each submitted buffer is one transfer, ended with a
/// short or zero-length packet.
pub trait OutPipe: Send {
    fn submit(&mut self, data: Vec<u8>);
    fn pending(&self) -> usize;
    fn wait(&mut self, timeout: Duration) -> Result<(), Error>;
}

pub trait InPipe: Send {
    fn submit(&mut self, length: usize);
    fn wait(&mut self, timeout: Duration) -> Result<Vec<u8>, Error>;
}

pub struct Link {
    pub control_out: Box<dyn OutPipe>,
    pub control_in: Box<dyn InPipe>,
    pub video_out: Box<dyn OutPipe>,
}

struct NusbOut {
    endpoint: Endpoint<Bulk, Out>,
    parts: VecDeque<usize>,
}

impl OutPipe for NusbOut {
    fn submit(&mut self, data: Vec<u8>) {
        let zlp = data.is_empty() || data.len().is_multiple_of(self.endpoint.max_packet_size());
        self.endpoint.submit(Buffer::from(data));
        if zlp {
            self.endpoint.submit(Buffer::new(0));
        }
        self.parts.push_back(if zlp { 2 } else { 1 });
    }

    fn pending(&self) -> usize {
        self.parts.len()
    }

    fn wait(&mut self, timeout: Duration) -> Result<(), Error> {
        let parts = *self
            .parts
            .front()
            .ok_or(Error::Other("nothing queued".into()))?;
        for _ in 0..parts {
            let completion = self
                .endpoint
                .wait_next_complete(timeout)
                .ok_or(Error::Timeout)?;
            *self.parts.front_mut().unwrap() -= 1;
            completion.status.map_err(|e| Error::Other(e.to_string()))?;
        }
        self.parts.pop_front();
        Ok(())
    }
}

struct NusbIn {
    endpoint: Endpoint<Bulk, In>,
}

impl InPipe for NusbIn {
    fn submit(&mut self, length: usize) {
        self.endpoint.submit(Buffer::new(length));
    }

    fn wait(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let completion = self
            .endpoint
            .wait_next_complete(timeout)
            .ok_or(Error::Timeout)?;
        completion.status.map_err(|e| Error::Other(e.to_string()))?;
        Ok(completion.buffer.into_vec())
    }
}

pub fn open_nusb() -> Result<Link, String> {
    let info = nusb::list_devices()
        .wait()
        .map_err(|e| e.to_string())?
        .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        .ok_or("device not found")?;
    let device = info.open().wait().map_err(|e| format!("open: {e}"))?;
    let claim = |number| {
        device
            .claim_interface(number)
            .wait()
            .map_err(|e| format!("claim interface {number}: {e}"))
    };
    let control = claim(CONTROL_INTERFACE)?;
    let video = claim(VIDEO_INTERFACE)?;
    let control_out = control
        .endpoint::<Bulk, Out>(CONTROL_OUT)
        .map_err(|e| format!("control out: {e}"))?;
    let control_in = control
        .endpoint::<Bulk, In>(CONTROL_IN)
        .map_err(|e| format!("control in: {e}"))?;
    let video_out = video
        .endpoint::<Bulk, Out>(VIDEO_OUT)
        .map_err(|e| format!("video out: {e}"))?;
    Ok(Link {
        control_out: Box::new(NusbOut {
            endpoint: control_out,
            parts: VecDeque::new(),
        }),
        control_in: Box::new(NusbIn {
            endpoint: control_in,
        }),
        video_out: Box::new(NusbOut {
            endpoint: video_out,
            parts: VecDeque::new(),
        }),
    })
}
