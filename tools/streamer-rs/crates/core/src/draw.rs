//! Video endpoint messages for whole frames.

use crate::layout::Layout;
use crate::lz4;
use crate::protocol::{self, Encoding, Info, Target};

/// Packs messages into transfers of at most `limit` bytes.
pub struct Batch {
    limit: usize,
    pub transfers: Vec<Vec<u8>>,
}

impl Batch {
    pub fn new(limit: usize) -> Self {
        Batch {
            limit,
            transfers: vec![Vec::new()],
        }
    }

    pub fn push(&mut self, message: Vec<u8>) -> Result<(), String> {
        if message.len() > self.limit {
            return Err(format!(
                "{} byte message over the {} byte limit",
                message.len(),
                self.limit
            ));
        }
        if self.transfers.last().unwrap().len() + message.len() > self.limit {
            self.transfers.push(Vec::new());
        }
        self.transfers
            .last_mut()
            .unwrap()
            .extend_from_slice(&message);
        Ok(())
    }
}

/// FILLs painting the panel around `content` (x, y, w, h) in black.
pub fn letterbox(target: Target, info: &Info, content: (u32, u32, u32, u32)) -> Vec<(u8, Vec<u8>)> {
    let (pw, ph) = (info.panel_width as u32, info.panel_height as u32);
    let (x, y, w, h) = content;
    let rects = [
        (0, 0, pw, y),
        (0, y + h, pw, ph - y - h),
        (0, y, x, h),
        (x + w, y, pw - x - w, h),
    ];
    rects
        .into_iter()
        .filter(|&(_, _, rw, rh)| rw > 0 && rh > 0)
        .map(|(rx, ry, rw, rh)| {
            let payload = protocol::fill(target, rx as u16, ry as u16, rw as u16, rh as u16, 0);
            (protocol::FILL, payload)
        })
        .collect()
}

/// Rows of `row_bytes` per message, in multiples of 16, that fit `limit` even
/// when LZ4 cannot shrink them.
fn band_rows(limit: usize, header: usize, row_bytes: usize) -> u32 {
    let room = limit - protocol::HEADER_BYTES - header;
    let mut rows = (room / row_bytes) as u32 / 16 * 16;
    while rows > 16 && lz4::max_compressed(rows as usize * row_bytes) > room {
        rows -= 16;
    }
    rows
}

fn encode(encoding: Encoding, pixels: &[u8]) -> Vec<u8> {
    match encoding {
        Encoding::Lz4 => lz4::compress(pixels),
        _ => pixels.to_vec(),
    }
}

/// The messages drawing `frame`, a JPEG or rows in the panel's pixel format
/// sized `layout.frame()`.
pub fn frame_messages(
    layout: &Layout,
    encoding: Encoding,
    target: Target,
    info: &Info,
    frame: &[u8],
) -> Vec<(u8, Vec<u8>)> {
    let limit = info.max_message[protocol::VIDEO] as usize;
    let panel_width = info.panel_width as u32;
    let (width, height) = layout.frame(panel_width);
    let row_bytes = width as usize * info.pixel_format.bytes();
    let pixels =
        |row: u32, rows: u32| &frame[row as usize * row_bytes..(row + rows) as usize * row_bytes];
    let mut messages = Vec::new();
    match *layout {
        Layout::Direct { y, .. } => {
            if encoding == Encoding::Jpeg {
                let payload =
                    protocol::draw_direct(target, encoding, y as u16, height as u16, frame);
                return vec![(protocol::DRAW_DIRECT, payload)];
            }
            let band = band_rows(limit, protocol::DRAW_DIRECT_BYTES, row_bytes);
            let mut row = 0;
            while row < height {
                let rows = band.min(height - row);
                let data = encode(encoding, pixels(row, rows));
                let payload =
                    protocol::draw_direct(target, encoding, (y + row) as u16, rows as u16, &data);
                messages.push((protocol::DRAW_DIRECT, payload));
                row += rows;
            }
        }
        Layout::Blit { rotation, area, .. } => {
            let mut blit = protocol::Blit {
                target,
                encoding,
                format: info.pixel_format,
                rotation,
                source: (width, height),
                area,
            };
            if encoding == Encoding::Jpeg {
                return vec![(protocol::DRAW_BLIT, protocol::draw_blit(&blit, frame))];
            }
            let band = band_rows(limit, protocol::DRAW_BLIT_BYTES, row_bytes);
            let mut row = 0;
            while row < height {
                let rows = band.min(height - row);
                blit.source = (width, rows);
                blit.area = layout.blit_band(row, rows);
                let data = encode(encoding, pixels(row, rows));
                messages.push((protocol::DRAW_BLIT, protocol::draw_blit(&blit, &data)));
                row += rows;
            }
        }
    }
    messages
}
