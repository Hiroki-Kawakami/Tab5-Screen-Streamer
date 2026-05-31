pub struct FrameConvertedData {
    pub data: Box<[u8]>,
    pub data_size: usize,
    pub quality: i32,
    pub fps: Option<usize>,
}

pub struct CaptureConfig {
    pub display: Option<usize>,
    pub quality: Option<i32>,
}

/// Number of horizontal bands the device-side image is split into.
/// Must evenly divide the long edge (1280) and yield a multiple of 16 per band.
pub const SPLIT_COUNT: usize = 16;

/// Per-chunk header size (bytes) prepended to every JPEG band.
///
/// Layout:
/// - byte 0     : sync / data type (`0x50` = decode only, `0x51` = last chunk, decode then present)
/// - bytes 1..4 : data size (24-bit little endian) = coordinate bytes (4) + JPEG payload
/// - byte 4     : x / 16
/// - byte 5     : y / 16
/// - byte 6     : width / 16
/// - byte 7     : height / 16
const HEADER_SIZE: usize = 8;

/// Number of coordinate bytes (x, y, width, height) that follow the size field and
/// are counted as part of the data size.
const COORD_SIZE: usize = 4;

/// Sync byte for an intermediate chunk: decode ahead but do not present yet.
const SYNC_CHUNK: u8 = 0x50;
/// Sync byte for the final chunk: decode then flush the framebuffer.
const SYNC_LAST: u8 = 0x51;

/// Split a captured BGRA/RGBX frame into [`SPLIT_COUNT`] horizontal bands (in the
/// device's portrait orientation), JPEG-encode each band independently and write a
/// framed message (`HEADER_SIZE` byte header + JPEG payload) per band into `out`.
///
/// Landscape captures (`src_width > src_height`) are rotated 270° so the device sees
/// a `src_height` x `src_width` portrait image; each band is one vertical source strip
/// rotated independently, which is pixel-identical to rotating the whole frame.
///
/// Returns the total number of bytes written into `out`.
pub fn encode_split_frame(
    compressor: &mut turbojpeg::Compressor,
    transformer: &mut turbojpeg::Transformer,
    pixels: &[u8],
    src_width: usize,
    src_height: usize,
    pixel_format: turbojpeg::PixelFormat,
    scratch: &mut [u8],
    out: &mut [u8],
) -> usize {
    let need_rotate = src_width > src_height;
    let mut offset = 0;
    for band in 0..SPLIT_COUNT {
        let payload = &mut out[offset + HEADER_SIZE..];
        let (dev_x, dev_y, dev_w, dev_h, jpeg_size) = if need_rotate {
            // Each device band maps to a full-height vertical strip of the source.
            // Bands are ordered top-to-bottom on the device, i.e. right-to-left in source.
            let strip_w = src_width / SPLIT_COUNT;
            let col_start = src_width - strip_w * (band + 1);
            let strip = turbojpeg::Image {
                pixels: &pixels[col_start * 4..],
                width: strip_w,
                pitch: src_width * 4,
                height: src_height,
                format: pixel_format,
            };
            let compressed = compressor
                .compress_to_slice(strip, scratch)
                .expect("JPEG Encode Failed!");
            let transform = turbojpeg::Transform::op(turbojpeg::TransformOp::Rot270);
            let rotated = transformer
                .transform_to_slice(&transform, &scratch[..compressed], payload)
                .expect("JPEG Rotate Failed!");
            // After Rot270 the strip becomes src_height wide x strip_w tall.
            (0, band * strip_w, src_height, strip_w, rotated)
        } else {
            // Portrait source: each band is a contiguous block of rows, no rotation.
            let band_h = src_height / SPLIT_COUNT;
            let row_start = band * band_h;
            let strip = turbojpeg::Image {
                pixels: &pixels[row_start * src_width * 4..],
                width: src_width,
                pitch: src_width * 4,
                height: band_h,
                format: pixel_format,
            };
            let compressed = compressor
                .compress_to_slice(strip, payload)
                .expect("JPEG Encode Failed!");
            (0, band * band_h, src_width, band_h, compressed)
        };

        // Data size covers the coordinate bytes plus the JPEG payload.
        let data_size = COORD_SIZE + jpeg_size;
        let header = &mut out[offset..offset + HEADER_SIZE];
        header[0] = if band + 1 == SPLIT_COUNT {
            SYNC_LAST
        } else {
            SYNC_CHUNK
        };
        header[1] = (data_size & 0xff) as u8;
        header[2] = ((data_size >> 8) & 0xff) as u8;
        header[3] = ((data_size >> 16) & 0xff) as u8;
        header[4] = (dev_x / 16) as u8;
        header[5] = (dev_y / 16) as u8;
        header[6] = (dev_w / 16) as u8;
        header[7] = (dev_h / 16) as u8;

        offset += HEADER_SIZE + jpeg_size;
    }
    offset
}

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::*;

#[cfg(target_os = "linux")]
pub mod common;
#[cfg(target_os = "linux")]
pub use self::common::*;
