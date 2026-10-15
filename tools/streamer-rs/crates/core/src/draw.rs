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

/// One JPEG of rows [row, row + rows) of a frame's pixels.
pub struct Band {
    pub row: u32,
    pub rows: u32,
    pub jpeg: Vec<u8>,
}

/// A frame ready to send, laid out by `Layout::placement`.
pub enum Encoded {
    Jpeg(Vec<Band>),
    /// Rows in the panel's pixel format, sent as RAW or LZ4.
    Pixels(Vec<u8>),
}

/// The messages drawing `frame`.
pub fn frame_messages(
    layout: &Layout,
    encoding: Encoding,
    target: Target,
    info: &Info,
    frame: Encoded,
) -> Vec<(u8, Vec<u8>)> {
    let limit = info.max_message[protocol::VIDEO] as usize;
    let panel_width = info.panel_width as u32;
    let placement = layout.placement(panel_width);
    let (width, height) = (placement.canvas.0 as u32, placement.canvas.1 as u32);
    let mut bands = Vec::new();
    match frame {
        Encoded::Jpeg(jpegs) => {
            for band in jpegs {
                bands.push((band.row, band.rows, band.jpeg));
            }
        }
        Encoded::Pixels(pixels) => {
            let row_bytes = width as usize * info.pixel_format.bytes();
            let header = match layout {
                Layout::Direct { .. } => protocol::DRAW_DIRECT_BYTES,
                Layout::Blit { .. } => protocol::DRAW_BLIT_BYTES,
            };
            let band = band_rows(limit, header, row_bytes);
            let mut row = 0;
            while row < height {
                let rows = band.min(height - row);
                let data = &pixels[row as usize * row_bytes..(row + rows) as usize * row_bytes];
                bands.push((row, rows, encode(encoding, data)));
                row += rows;
            }
        }
    }
    bands
        .into_iter()
        .map(|(row, rows, data)| match *layout {
            Layout::Direct { y, .. } => {
                let payload =
                    protocol::draw_direct(target, encoding, (y + row) as u16, rows as u16, &data);
                (protocol::DRAW_DIRECT, payload)
            }
            Layout::Blit { rotation, .. } => {
                let blit = protocol::Blit {
                    target,
                    encoding,
                    format: info.pixel_format,
                    rotation,
                    source: (width, rows),
                    area: layout.blit_band(row, rows),
                };
                (protocol::DRAW_BLIT, protocol::draw_blit(&blit, &data))
            }
        })
        .collect()
}
