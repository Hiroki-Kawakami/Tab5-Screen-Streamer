use crate::capture::FrameConvertedData;
use std::{sync::mpsc::{self, SyncSender}, thread, time::Duration};
use core_foundation::error::CFError;
use core_media_rs::{cm_time::CMTime, cm_sample_buffer::CMSampleBuffer};
use screencapturekit::{
    output::LockTrait, shareable_content::SCShareableContent, stream::{
        configuration::{pixel_format::PixelFormat, SCStreamConfiguration},
        content_filter::SCContentFilter,
        output_trait::SCStreamOutputTrait,
        output_type::SCStreamOutputType,
        SCStream
    }
};

const BUF_SIZE: usize = 512 * 1024;
const JPEG_QUALITY_LEVELS: [i32; 4] = [40, 60, 70, 80];

pub struct Context {
    rx: mpsc::Receiver<FrameConvertedData>,
}

pub fn start(display_index: Option<usize>) -> Context {
    let (conv_tx, conv_rx) = mpsc::sync_channel::<FrameConvertedData>(1);

    // Capture Thread
    thread::spawn(move || {
        let (capt_tx, capt_rx) = mpsc::sync_channel::<CMSampleBuffer>(1);
        let _stream = start_screen_capture_kit(capt_tx, display_index)
            .expect("Failed to start ScreenCaptureKit!");

        let mut compressor = turbojpeg::Compressor::new().expect("Failed to create turbojpeg Compressor");
        let mut transformer = turbojpeg::Transformer::new().expect("Failed to create turbojpeg Transformer");

        let mut quality_level: usize = 0;
        compressor.set_quality(JPEG_QUALITY_LEVELS[0]).expect("set jpeg quality failed!");
        compressor.set_optimize(false).expect("set jpeg optimize failed!");
        compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2).expect("set jpeg subsamp failed!");

        let mut compress_buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
        let mut frames = 0;
        let mut start = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        for sample_buffer in capt_rx {
            let pixel_buffer = if let Ok(pb) = sample_buffer.get_pixel_buffer() {
                pb
            } else {
                continue
            };

            frames += 1;
            let fps = if start.elapsed() >= Duration::from_secs(1) {
                let fps = Some(frames);
                frames = 0;
                start = std::time::Instant::now();
                fps
            } else {
                None
            };

            let size = (pixel_buffer.get_width(), pixel_buffer.get_height());
            let data = if let Ok(d) = pixel_buffer.lock() {
                d
            } else {
                continue
            };

            let image = turbojpeg::Image {
                pixels: data.0.as_slice(),
                width: size.0 as usize,
                pitch: (size.0 as usize) * 4,
                height: size.1 as usize,
                format: turbojpeg::PixelFormat::BGRA,
            };

            let mut converted = unsafe { Box::<[u8]>::new_uninit_slice(BUF_SIZE).assume_init() };
            let size = if size.0 > size.1 { // need rotate
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
            let data = FrameConvertedData { data: converted, data_size: size, quality: JPEG_QUALITY_LEVELS[quality_level], fps };
            let _ = conv_tx.try_send(data);

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

fn start_screen_capture_kit(tx: SyncSender<CMSampleBuffer>, display_index: Option<usize>) -> Result<SCStream, CFError> {
    let content = SCShareableContent::get()?;
    let display = &content.displays()[display_index.unwrap_or(0)];
    let filter = SCContentFilter::new().with_display_excluding_windows(display, &[]);

    let config = SCStreamConfiguration::new()
        .set_width(1280)?
        .set_height(720)?
        .set_minimum_frame_interval(&CMTime { value: 1, timescale: 60, flags: 0, epoch: 0 })?
        .set_pixel_format(PixelFormat::BGRA)?
        .set_captures_audio(false)?;

    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(SCStreamOutput { tx }, SCStreamOutputType::Screen);
    stream.start_capture()?;
    Ok(stream)
}

struct SCStreamOutput {
    tx: mpsc::SyncSender<CMSampleBuffer>,
}
impl SCStreamOutputTrait for SCStreamOutput {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, _of_type: SCStreamOutputType) {
        self.tx.send(sample_buffer).expect("Send CMSampleBuffer failed!");
    }
}

impl Context {
    pub fn get_frame(&self) -> FrameConvertedData {
        self.rx.recv().expect("Recv FrameConvertedData failed!")
    }
}
