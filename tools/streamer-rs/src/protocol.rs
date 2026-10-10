//! Wire format of docs/protocol.md.

pub const MAGIC: u16 = 0x3554;
pub const HEADER_BYTES: usize = 16;

pub const GET_INFO: u8 = 0x01;
pub const INFO: u8 = 0x81;
pub const SWAP_DONE: u8 = 0x82;
pub const ERROR: u8 = 0x83;
pub const DRAW_DIRECT: u8 = 0x10;
pub const FILL: u8 = 0x12;
pub const SWAP: u8 = 0x13;

pub const VIDEO: usize = 1;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Front = 0,
    Back = 1,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Encoding {
    Jpeg = 0,
    Raw = 1,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PixelFormat {
    Rgb565,
    Rgb888,
}

impl PixelFormat {
    pub fn bytes(self) -> usize {
        match self {
            PixelFormat::Rgb565 => 2,
            PixelFormat::Rgb888 => 3,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Info {
    pub protocol_version: u16,
    pub panel_width: u16,
    pub panel_height: u16,
    pub pixel_format: PixelFormat,
    pub orientation: u8,
    pub framebuffers: u8,
    pub swap_window: u8,
    pub direct_align: u16,
    pub blit_max_width: u16,
    pub blit_max_height: u16,
    pub encodings: u32,
    pub max_message: [u32; 3],
    pub epoch: u32,
    pub overlay_visible: bool,
    pub max_fps: u16,
}

pub enum Event {
    Info(Info),
    SwapDone { seq: u32 },
    Error { seq: u32, channel: u8, code: u8 },
    Other,
}

pub fn message(kind: u8, seq: u32, epoch: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + payload.len());
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.push(kind);
    out.push(0);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&epoch.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn draw_direct(
    target: Target,
    encoding: Encoding,
    y: u16,
    height: u16,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + data.len());
    out.push(target as u8);
    out.push(encoding as u8);
    out.extend_from_slice(&y.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub fn fill(target: Target, x: u16, y: u16, w: u16, h: u16, rgb: u32) -> Vec<u8> {
    let mut out = vec![target as u8, 0];
    for value in [x, y, w, h] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&rgb.to_le_bytes());
    out
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn parse_info(p: &[u8]) -> Option<Info> {
    if p.len() < 72 {
        return None;
    }
    let pixel_format = match p[6] {
        1 => PixelFormat::Rgb565,
        2 => PixelFormat::Rgb888,
        _ => return None,
    };
    Some(Info {
        protocol_version: u16_at(p, 0),
        panel_width: u16_at(p, 2),
        panel_height: u16_at(p, 4),
        pixel_format,
        orientation: p[7],
        framebuffers: p[8],
        swap_window: p[9],
        direct_align: u16_at(p, 10),
        blit_max_width: u16_at(p, 12),
        blit_max_height: u16_at(p, 14),
        encodings: u32_at(p, 16),
        max_message: [u32_at(p, 20), u32_at(p, 24), u32_at(p, 28)],
        epoch: u32_at(p, 32),
        overlay_visible: p[36] != 0,
        max_fps: u16_at(p, 46),
    })
}

/// Splits one IN transfer into events; stops at the first malformed message.
pub fn parse_events(data: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut pos = 0;
    while pos + HEADER_BYTES <= data.len() {
        let kind = data[pos + 2];
        let seq = u32_at(data, pos + 8);
        let length = u32_at(data, pos + 4) as usize;
        if u16_at(data, pos) != MAGIC || pos + HEADER_BYTES + length > data.len() {
            break;
        }
        let payload = &data[pos + HEADER_BYTES..pos + HEADER_BYTES + length];
        pos += HEADER_BYTES + length;
        let event = match kind {
            INFO => match parse_info(payload) {
                Some(info) => Event::Info(info),
                None => continue,
            },
            SWAP_DONE => Event::SwapDone { seq },
            ERROR if payload.len() >= 2 => Event::Error {
                seq,
                channel: payload[0],
                code: payload[1],
            },
            _ => Event::Other,
        };
        events.push(event);
    }
    events
}

pub fn error_name(code: u8) -> &'static str {
    match code {
        1 => "malformed",
        2 => "too large",
        3 => "unsupported",
        4 => "out of bounds",
        5 => "decode failed",
        _ => "unknown",
    }
}
