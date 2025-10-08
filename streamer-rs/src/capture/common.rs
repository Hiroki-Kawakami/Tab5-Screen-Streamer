use crate::capture::FrameConvertedData;
use std::{thread, sync::mpsc, time::Duration};
use scap::{
    capturer::{self, Capturer},
    frame::{Frame, FrameType},
};
use fast_image_resize as fir;

const BUF_SIZE: usize = 512 * 1024;
const JPEG_QUALITY_LEVELS: [i32; 4] = [40, 60, 70, 80];

pub struct FrameCaptureData {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub fps: Option<usize>,
}

pub struct Context {
    rx: mpsc::Receiver<FrameConvertedData>,
}

pub fn start(display_index: Option<usize>) -> Context {
    let (resz_tx, resz_rx) = mpsc::sync_channel::<FrameCaptureData>(1);
    let (jpeg_tx, jpeg_rx) = mpsc::sync_channel::<FrameCaptureData>(1);
    let (conv_tx, conv_rx) = mpsc::sync_channel::<FrameConvertedData>(1);

    // Capture Thread
    let resz_tx_capture = resz_tx.clone();
    let jpeg_tx_capture = jpeg_tx.clone();
    thread::spawn(move || {
        let options = capturer::Options {
            fps: 60,
            target: capture_target(display_index),
            show_cursor: true,
            show_highlight: true,
            excluded_targets: None,
            output_type: FrameType::BGRAFrame,
            output_resolution: capturer::Resolution::_720p,
            ..Default::default()
        };
        let mut capturer = Capturer::build(options).unwrap();
        capturer.start_capture();

        let mut frames = 0;
        let mut start = std::time::Instant::now();
        loop {
            let frame = if let Frame::BGRA(frame) = capturer.get_next_frame().expect("Capture Recv Failed!") {
                frame
            } else {
                panic!("Unsupported Frame Format!");
            };
            if frame.data.len() == 0 { continue }

            frames += 1;
            let fps = if start.elapsed() >= Duration::from_secs(1) {
                let fps = Some(frames);
                frames = 0;
                start = std::time::Instant::now();
                fps
            } else {
                None
            };

            let frame_size = (frame.width, frame.height);
            let data = FrameCaptureData { data: frame.data, width: frame.width as usize, height: frame.height as usize, fps };
            if frame_size == (1280, 720) || frame_size == (720, 1280) {
                let _ = jpeg_tx_capture.try_send(data);
            } else {
                let _ = resz_tx_capture.try_send(data);
            }
        }
    });

    // Resize Thread
    let jpeg_tx_resize = jpeg_tx.clone();
    thread::spawn(move || {
        let mut resizer = fir::Resizer::new();
        for frame in resz_rx {
            let (rwidth, rheight) = if frame.width > frame.height { (1280, 720) } else { (720, 1280) };
            let original = fir::images::Image::from_vec_u8(
                frame.width as u32, frame.height as u32, frame.data, fir::PixelType::U8x4
            ).expect("Failed to create original image container");
            let mut resized = fir::images::Image::from_vec_u8(
                rwidth as u32, rheight as u32, vec![0; rwidth * rheight * 4], fir::PixelType::U8x4
            ).expect("Failed to create resized image container");

            resizer.resize(&original, &mut resized, &fir::ResizeOptions {
                algorithm: fir::ResizeAlg::Nearest,
                cropping: fir::SrcCropping::None,
                mul_div_alpha: false,
            }).expect("Resize Image Failed!");
            let data = FrameCaptureData { data: resized.into_vec(), width: rwidth, height: rheight, fps: frame.fps };
            let _ = jpeg_tx_resize.try_send(data);
        }
    });

    // JPEG Encode Thread
    let conv_tx_jpeg = conv_tx.clone();
    thread::spawn(move || {
        let mut compressor = turbojpeg::Compressor::new().expect("Failed to create turbojpeg Compressor");
        let mut transformer = turbojpeg::Transformer::new().expect("Failed to create turbojpeg Transformer");

        let mut quality_level: usize = 0;
        compressor.set_quality(JPEG_QUALITY_LEVELS[0]).expect("set jpeg quality failed!");
        compressor.set_optimize(false).expect("set jpeg optimize failed!");
        compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2).expect("set jpeg subsamp failed!");

        let mut compress_buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
        let mut last = std::time::Instant::now();

        for frame in jpeg_rx {
            let image = turbojpeg::Image {
                pixels: frame.data.as_ref(),
                width: 1280,
                pitch: frame.width * 4,
                height: 720,
                format: turbojpeg::PixelFormat::BGRA,
            };

            let mut converted = unsafe { Box::<[u8]>::new_uninit_slice(BUF_SIZE).assume_init() };
            let size = if frame.width > frame.height { // need rotate
                compressor.compress_to_slice(image, &mut compress_buffer)
                    .expect("JPEG Encode Failed!");

                let transform = turbojpeg::Transform::op(turbojpeg::TransformOp::Rot270);
                // transform.optimize = true;
                transformer.transform_to_slice(&transform, &compress_buffer, &mut converted[4..])
                    .expect("JPEG Rotate Failed!")
            } else {
                compressor.compress_to_slice(image, &mut converted[4..])
                    .expect("JPEG Encode Failed!")
            };

            let size = size + 4;
            let bytes = (size as u32).to_le_bytes();
            converted[..4].copy_from_slice(&bytes);
            let data = FrameConvertedData { data: converted, data_size: size, quality: JPEG_QUALITY_LEVELS[quality_level], fps: frame.fps };
            let _ = conv_tx_jpeg.try_send(data);

            let tx_speed = (size as f64) / last.elapsed().as_secs_f64();
            if quality_level > 0 && tx_speed > 7e6 {
                quality_level -= 1;
                compressor.set_quality(JPEG_QUALITY_LEVELS[quality_level]).expect("set jpeg quality failed!");
            } else if quality_level + 1 < JPEG_QUALITY_LEVELS.len() && tx_speed < 4e6 {
                quality_level += 1;
                compressor.set_quality(JPEG_QUALITY_LEVELS[quality_level]).expect("set jpeg quality failed!");
            }
            last = std::time::Instant::now();
        }
    });

    Context { rx: conv_rx }
}

impl Context {
    pub fn get_frame(&self) -> FrameConvertedData {
        self.rx.recv().expect("Recv capture frame failed!")
    }
}

fn capture_target(index: Option<usize>) -> Option<scap::Target> {
    if let Some(index) = index {
        let mut current_index = 0;
        for target in scap::get_all_targets() {
            if let scap::Target::Display(d) = target {
                if current_index == index { return Some(scap::Target::Display(d)); }
                current_index += 1;
            }
        }
    }
    None
}
