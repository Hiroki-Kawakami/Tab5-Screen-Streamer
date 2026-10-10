use std::fmt;
use std::time::Duration;

use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient};
use nusb::{Endpoint, Interface, MaybeFuture};

pub const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Standard,
    Vendor,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Device,
    Endpoint,
}

#[derive(Clone, Copy)]
pub struct Request {
    pub kind: Kind,
    pub target: Target,
    pub request: u8,
    pub value: u16,
    pub index: u16,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    Stall,
    Timeout,
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Stall => write!(f, "stalled"),
            Error::Timeout => write!(f, "timed out"),
            Error::Other(e) => write!(f, "{e}"),
        }
    }
}

/// Endpoint 0 plus one bulk OUT and one bulk IN endpoint; bulk transfers
/// queue up per direction and complete in order.
pub trait Transport {
    fn speed(&self) -> String;
    fn max_packet(&self) -> usize;
    fn control_in(&mut self, request: Request, length: u16) -> Result<Vec<u8>, Error>;
    fn control_out(&mut self, request: Request, data: &[u8]) -> Result<(), Error>;
    fn clear_halt_out(&mut self) -> Result<(), Error>;
    fn submit_out(&mut self, data: Vec<u8>);
    fn submit_in(&mut self, length: usize);
    fn pending_out(&self) -> usize;
    fn wait_out(&mut self) -> Result<usize, Error>;
    fn wait_in(&mut self) -> Result<Vec<u8>, Error>;
}

pub struct Nusb {
    device: nusb::Device,
    interface: Interface,
    out: Endpoint<Bulk, Out>,
    input: Endpoint<Bulk, In>,
}

fn nusb_error(e: nusb::transfer::TransferError) -> Error {
    match e {
        nusb::transfer::TransferError::Stall => Error::Stall,
        other => Error::Other(other.to_string()),
    }
}

impl Nusb {
    pub fn open(vendor_id: u16, product_id: u16, out: u8, input: u8) -> Result<Self, String> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| e.to_string())?
            .find(|d| d.vendor_id() == vendor_id && d.product_id() == product_id)
            .ok_or("device not found")?;
        let device = info.open().wait().map_err(|e| format!("open: {e}"))?;
        let interface = device
            .claim_interface(0)
            .wait()
            .map_err(|e| format!("claim: {e}"))?;
        let out = interface
            .endpoint::<Bulk, Out>(out)
            .map_err(|e| format!("endpoint out: {e}"))?;
        let input = interface
            .endpoint::<Bulk, In>(input)
            .map_err(|e| format!("endpoint in: {e}"))?;
        Ok(Nusb {
            device,
            interface,
            out,
            input,
        })
    }

    fn control_type(kind: Kind) -> ControlType {
        match kind {
            Kind::Standard => ControlType::Standard,
            Kind::Vendor => ControlType::Vendor,
        }
    }

    fn recipient(target: Target) -> Recipient {
        match target {
            Target::Device => Recipient::Device,
            Target::Endpoint => Recipient::Endpoint,
        }
    }
}

impl Transport for Nusb {
    fn speed(&self) -> String {
        format!("{:?}", self.device.speed())
    }

    fn max_packet(&self) -> usize {
        self.out.max_packet_size()
    }

    fn control_in(&mut self, r: Request, length: u16) -> Result<Vec<u8>, Error> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: Self::control_type(r.kind),
                    recipient: Self::recipient(r.target),
                    request: r.request,
                    value: r.value,
                    index: r.index,
                    length,
                },
                TIMEOUT,
            )
            .wait()
            .map_err(nusb_error)
    }

    fn control_out(&mut self, r: Request, data: &[u8]) -> Result<(), Error> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: Self::control_type(r.kind),
                    recipient: Self::recipient(r.target),
                    request: r.request,
                    value: r.value,
                    index: r.index,
                    data,
                },
                TIMEOUT,
            )
            .wait()
            .map_err(nusb_error)
    }

    fn clear_halt_out(&mut self) -> Result<(), Error> {
        self.out
            .clear_halt()
            .wait()
            .map_err(|e| Error::Other(e.to_string()))
    }

    fn submit_out(&mut self, data: Vec<u8>) {
        self.out.submit(Buffer::from(data));
    }

    fn submit_in(&mut self, length: usize) {
        self.input.submit(Buffer::new(length));
    }

    fn pending_out(&self) -> usize {
        self.out.pending()
    }

    fn wait_out(&mut self) -> Result<usize, Error> {
        let completion = self.out.wait_next_complete(TIMEOUT).ok_or(Error::Timeout)?;
        completion.status.map_err(nusb_error)?;
        Ok(completion.actual_len)
    }

    fn wait_in(&mut self) -> Result<Vec<u8>, Error> {
        let completion = self
            .input
            .wait_next_complete(TIMEOUT)
            .ok_or(Error::Timeout)?;
        completion.status.map_err(nusb_error)?;
        Ok(completion.buffer.into_vec())
    }
}
