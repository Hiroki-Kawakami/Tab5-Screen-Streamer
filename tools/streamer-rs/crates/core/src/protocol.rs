//! Wire format of docs/protocol.md.

pub const MAGIC: u16 = 0x3554;
pub const HEADER_BYTES: usize = 16;

pub const OPEN: u8 = 0x01;
pub const CLOSE: u8 = 0x02;
pub const KEEPALIVE: u8 = 0x03;
pub const SET_INPUT_MODE: u8 = 0x04;
pub const SET_VOLUME: u8 = 0x05;
pub const INFO: u8 = 0x81;
pub const CLOSED: u8 = 0x82;
pub const ERROR: u8 = 0x83;
pub const SWAP_DONE: u8 = 0x84;
pub const FORMAT: u8 = 0x86;
pub const ORIENTATION: u8 = 0x87;
pub const OVERLAY: u8 = 0x88;
pub const DAMAGE: u8 = 0x89;
pub const TOUCH: u8 = 0x8a;
pub const INPUT_MODE: u8 = 0x8c;
pub const VOLUME: u8 = 0x8d;
pub const AUDIO: u8 = 0x8e;
pub const AUDIO_FORMAT: u8 = 0x20;
pub const AUDIO_DATA: u8 = 0x21;
pub const BULK: u8 = 1;
pub const DRAW_DIRECT: u8 = 0x10;
pub const DRAW_BLIT: u8 = 0x11;
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
    Lz4 = 2,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PixelFormat {
    Rgb565,
    Rgb888,
}

impl PixelFormat {
    pub fn wire(self) -> u8 {
        match self {
            PixelFormat::Rgb565 => 1,
            PixelFormat::Rgb888 => 2,
        }
    }

    pub fn bytes(self) -> usize {
        match self {
            PixelFormat::Rgb565 => 2,
            PixelFormat::Rgb888 => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InputTarget {
    Touch = 0,
    Keyboard = 1,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InputMode {
    None = 0,
    Event = 1,
    Hid = 2,
}

fn input_mode(value: u8) -> Option<InputMode> {
    match value {
        0 => Some(InputMode::None),
        1 => Some(InputMode::Event),
        2 => Some(InputMode::Hid),
        _ => None,
    }
}

fn input_target(value: u8) -> Option<InputTarget> {
    match value {
        0 => Some(InputTarget::Touch),
        1 => Some(InputTarget::Keyboard),
        _ => None,
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
    /// By InputTarget.
    pub input_mode: [InputMode; 2],
    pub volume: [Volume; 2],
    pub audio_max_rate: u32,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Volume {
    pub mute: bool,
    pub volume: i16,
    pub min: i16,
    pub max: i16,
    pub resolution: i16,
}

fn volume_at(p: &[u8], at: usize) -> Volume {
    Volume {
        mute: p[at] != 0,
        volume: u16_at(p, at + 2) as i16,
        min: u16_at(p, at + 4) as i16,
        max: u16_at(p, at + 6) as i16,
        resolution: u16_at(p, at + 8) as i16,
    }
}

/// x, y, w, h
pub type Rect = (u16, u16, u16, u16);

fn rect_at(p: &[u8], at: usize) -> Rect {
    (
        u16_at(p, at),
        u16_at(p, at + 2),
        u16_at(p, at + 4),
        u16_at(p, at + 6),
    )
}

/// Panel-native.
#[derive(Clone, Copy, Debug)]
pub struct TouchPoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

pub enum Event {
    Info { seq: u32, info: Info },
    Closed,
    SwapDone { seq: u32 },
    Error { seq: u32, channel: u8, code: u8 },
    Orientation(u32),
    Format { format: PixelFormat, epoch: u32 },
    Overlay { visible: bool, stream: Rect },
    Damage(Rect),
    Touch(Vec<TouchPoint>),
    InputMode(InputTarget, InputMode),
    Volume { stream: u8, mute: bool, volume: i16 },
    Audio { buffered: u32, capacity: u32 },
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

pub const DRAW_DIRECT_BYTES: usize = 6;
pub const DRAW_BLIT_BYTES: usize = 16;

pub struct Blit {
    pub target: Target,
    pub encoding: Encoding,
    pub format: PixelFormat,
    pub rotation: u32,
    pub source: (u32, u32),
    pub area: (u32, u32, u32, u32),
}

pub fn draw_blit(blit: &Blit, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DRAW_BLIT_BYTES + data.len());
    out.extend_from_slice(&[
        blit.target as u8,
        blit.encoding as u8,
        blit.format.wire(),
        blit.rotation as u8,
    ]);
    let (x, y, w, h) = blit.area;
    for value in [blit.source.0, blit.source.1, x, y, w, h] {
        out.extend_from_slice(&(value as u16).to_le_bytes());
    }
    out.extend_from_slice(data);
    out
}

pub fn audio_format(rate: u32, channels: u8, bits: u8) -> Vec<u8> {
    let mut out = rate.to_le_bytes().to_vec();
    out.extend_from_slice(&[channels, bits, 0, 0]);
    out
}

pub fn set_input_mode(target: InputTarget, mode: InputMode) -> Vec<u8> {
    vec![target as u8, mode as u8, 0, 0]
}

pub fn set_volume(stream: u8, mute: bool, volume: i16) -> Vec<u8> {
    let mut out = vec![stream, mute as u8];
    out.extend_from_slice(&volume.to_le_bytes());
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

fn pixel_format(value: u8) -> Option<PixelFormat> {
    match value {
        1 => Some(PixelFormat::Rgb565),
        2 => Some(PixelFormat::Rgb888),
        _ => None,
    }
}

fn parse_info(p: &[u8]) -> Option<Info> {
    if p.len() < 74 {
        return None;
    }
    Some(Info {
        protocol_version: u16_at(p, 0),
        panel_width: u16_at(p, 2),
        panel_height: u16_at(p, 4),
        framebuffers: p[6],
        swap_window: p[7],
        max_message: [u32_at(p, 8), u32_at(p, 12), u32_at(p, 16)],
        audio_max_rate: u32_at(p, 20),
        encodings: u32_at(p, 24),
        direct_align: u16_at(p, 28),
        blit_max_width: u16_at(p, 30),
        blit_max_height: u16_at(p, 32),
        pixel_format: pixel_format(p[34])?,
        orientation: p[35],
        epoch: u32_at(p, 36),
        max_fps: u16_at(p, 40),
        input_mode: [input_mode(p[42])?, input_mode(p[43])?],
        volume: [volume_at(p, 44), volume_at(p, 54)],
        overlay_visible: p[64] != 0,
    })
}

fn parse_touch(p: &[u8]) -> Option<Vec<TouchPoint>> {
    let count = *p.first()? as usize;
    let points = p.get(4..4 + count * 8)?;
    Some(
        points
            .chunks_exact(8)
            .map(|point| TouchPoint {
                id: point[0],
                x: u16_at(point, 2),
                y: u16_at(point, 4),
            })
            .collect(),
    )
}

/// Splits one IN transfer into events; stops at the first malformed message.
pub fn parse_events(data: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut pos = 0;
    while pos + HEADER_BYTES <= data.len() {
        let kind = data[pos + 2];
        let seq = u32_at(data, pos + 8);
        let epoch = u32_at(data, pos + 12);
        let length = u32_at(data, pos + 4) as usize;
        if u16_at(data, pos) != MAGIC || pos + HEADER_BYTES + length > data.len() {
            break;
        }
        let payload = &data[pos + HEADER_BYTES..pos + HEADER_BYTES + length];
        pos += HEADER_BYTES + length;
        let event = match kind {
            INFO => match parse_info(payload) {
                Some(info) => Event::Info { seq, info },
                None => continue,
            },
            CLOSED => Event::Closed,
            SWAP_DONE => Event::SwapDone { seq },
            ERROR if payload.len() >= 2 => Event::Error {
                seq,
                channel: payload[0],
                code: payload[1],
            },
            ORIENTATION if !payload.is_empty() => Event::Orientation(payload[0] as u32),
            FORMAT if !payload.is_empty() => match pixel_format(payload[0]) {
                Some(format) => Event::Format { format, epoch },
                None => continue,
            },
            OVERLAY if payload.len() >= 10 => Event::Overlay {
                visible: payload[0] != 0,
                stream: rect_at(payload, 2),
            },
            DAMAGE if payload.len() >= 8 => Event::Damage(rect_at(payload, 0)),
            TOUCH => match parse_touch(payload) {
                Some(points) => Event::Touch(points),
                None => continue,
            },
            INPUT_MODE if payload.len() >= 2 => {
                match (input_target(payload[0]), input_mode(payload[1])) {
                    (Some(target), Some(mode)) => Event::InputMode(target, mode),
                    _ => continue,
                }
            }
            VOLUME if payload.len() >= 4 => Event::Volume {
                stream: payload[0],
                mute: payload[1] != 0,
                volume: u16_at(payload, 2) as i16,
            },
            AUDIO if payload.len() >= 12 => Event::Audio {
                buffered: u32_at(payload, 4),
                capacity: u32_at(payload, 8),
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
