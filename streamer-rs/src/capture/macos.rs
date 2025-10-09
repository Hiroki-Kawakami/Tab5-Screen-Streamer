use crate::capture::FrameConvertedData;
use std::{os::raw::c_void, sync::mpsc, thread, time::Duration};
use core_foundation::{error::CFError, runloop::CFRunLoopRun};
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
use objc2::{class, msg_send};
use objc2::runtime::AnyObject;
use objc2::rc::Retained;
use objc2_foundation::{NSString, NSArray};
use objc2_core_foundation::CGSize;
use objc2_core_graphics::{CGDirectDisplayID, CGDisplayChangeSummaryFlags, CGDisplayIsInMirrorSet, CGDisplayMirrorsDisplay, CGDisplayRegisterReconfigurationCallback};
use objc2_app_kit::NSApplication;
use dispatch2::DispatchQueue;

const BUF_SIZE: usize = 512 * 1024;
const JPEG_QUALITY_LEVELS: [i32; 4] = [40, 60, 70, 80];

pub struct Context {
    rx: mpsc::Receiver<FrameConvertedData>,
}

static mut DISPLAY_WATCH: Option<CGDirectDisplayID> = None;
static mut DISPLAY_UPDATED: bool = false;
unsafe extern "C-unwind" fn display_settings_changed(display: u32, _flags: CGDisplayChangeSummaryFlags, _user_info: *mut c_void) {
    // println!("Display Settings Changed: display={}, flags={:?}", display, flags);
    if unsafe { DISPLAY_WATCH } == Some(display) {
        unsafe { DISPLAY_UPDATED = true; }
    }
}

pub fn start<F>(display_index: Option<usize>, tx_thread: F)
where
    F: FnOnce(Context) + Send + 'static,
{
    let (conv_tx, conv_rx) = mpsc::sync_channel::<FrameConvertedData>(1);

    // Capture Thread
    thread::spawn(move || {
        let (capt_tx, capt_rx) = mpsc::sync_channel::<CMSampleBuffer>(1);

        let (display_id, _virtual_display) = if let Some(i) = display_index {
            let contents = SCShareableContent::get()
                .expect("Failed to get display list.");
            (contents.displays()[i].display_id(), None)
        } else {
            let virtual_display = VirtualDisplay::new(
                "M5Stack Tab5",
                (1280, 720),
                (110.0, 62.0)
            );
            (virtual_display.get_id(), Some(virtual_display))
        };
        let output = SCStreamOutput { tx: capt_tx };
        let mut compressor = turbojpeg::Compressor::new().expect("Failed to create turbojpeg Compressor");
        let mut transformer = turbojpeg::Transformer::new().expect("Failed to create turbojpeg Transformer");

        let mut quality_level: usize = 0;
        compressor.set_quality(JPEG_QUALITY_LEVELS[0]).expect("set jpeg quality failed!");
        compressor.set_optimize(false).expect("set jpeg optimize failed!");
        compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2).expect("set jpeg subsamp failed!");

        loop {
            let stream = start_screen_capture_kit(output.clone(), display_id)
                .expect("Failed to start ScreenCaptureKit!");

            let mut compress_buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
            let mut frames = 0;
            let mut start = std::time::Instant::now();
            let mut last = std::time::Instant::now();
            loop {
                let sample_buffer = capt_rx.recv_timeout(Duration::from_millis(100));
                if unsafe { DISPLAY_UPDATED } { break; }
                let sample_buffer = match sample_buffer {
                    Ok(sb) => sb,
                    Err(_) => continue,
                };

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
            println!("Display Settings Changed, Reopening Stream...");
            let _ = stream.stop_capture();
            thread::sleep(Duration::from_millis(100));
            unsafe { DISPLAY_UPDATED = false; }
        }
    });

    thread::spawn(move || {
        tx_thread(Context { rx: conv_rx });
    });

    // Run Loop
    unsafe {
        let _ = CGDisplayRegisterReconfigurationCallback(Some(display_settings_changed), std::ptr::null_mut());
        NSApplication::load();
        CFRunLoopRun();
    }
}

fn create_filter_from_display_id(display_id: CGDirectDisplayID) -> Result<(SCContentFilter, CGDirectDisplayID), CFError> {
    for d in SCShareableContent::get()?.displays() {
        if d.display_id() == display_id {
            return Ok((SCContentFilter::new().with_display_excluding_windows(&d, &[]), display_id));
        }
    }
    if CGDisplayIsInMirrorSet(display_id) {
        let mirrored_id = CGDisplayMirrorsDisplay(display_id);
        for d in SCShareableContent::get()?.displays() {
            if d.display_id() == mirrored_id {
                return Ok((SCContentFilter::new().with_display_excluding_windows(&d, &[]), mirrored_id));
            }
        }
    }
    panic!("Target Display not found in Shareable Content!");
}
fn start_screen_capture_kit(output: SCStreamOutput, display_id: CGDirectDisplayID) -> Result<SCStream, CFError> {
    let (filter, _selected_display_id) = create_filter_from_display_id(display_id)?;
    unsafe { DISPLAY_WATCH = Some(display_id) };

    let config = SCStreamConfiguration::new()
        .set_width(1280)?
        .set_height(720)?
        .set_minimum_frame_interval(&CMTime { value: 1, timescale: 60, flags: 0, epoch: 0 })?
        .set_pixel_format(PixelFormat::BGRA)?
        .set_captures_audio(false)?;

    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(output, SCStreamOutputType::Screen);
    stream.start_capture()?;
    Ok(stream)
}

#[derive(Clone)]
struct SCStreamOutput {
    tx: mpsc::SyncSender<CMSampleBuffer>,
}
impl SCStreamOutputTrait for SCStreamOutput {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, _of_type: SCStreamOutputType) {
        let _ = self.tx.try_send(sample_buffer);
    }
}

impl Context {
    pub fn get_frame(&self) -> FrameConvertedData {
        self.rx.recv().expect("Recv FrameConvertedData failed!")
    }
}

struct VirtualDisplay {
    pub display: Retained<AnyObject>,
}
impl VirtualDisplay {
    #[allow(unexpected_cfgs)]
    pub fn new(
        name: &str,
        resolution: (u32, u32),
        size: (f64, f64),
    ) -> Self {
        let descriptor_cls = class!(CGVirtualDisplayDescriptor);
        let descriptor: Retained<AnyObject> = unsafe { msg_send![msg_send![descriptor_cls, alloc], init] };
        unsafe {
            let name = NSString::from_str(name);
            let _: () = msg_send![&*descriptor, setName: &*name];
            let _: () = msg_send![&*descriptor, setMaxPixelsWide: resolution.0];
            let _: () = msg_send![&*descriptor, setMaxPixelsHigh: resolution.1];

            let size = CGSize::new(size.0, size.1);
            let _: () = msg_send![&*descriptor, setSizeInMillimeters: size];

            let _: () = msg_send![&*descriptor, setProductID: 0x303au32];
            let _: () = msg_send![&*descriptor, setVendorID: 0x4020u32];
            let _: () = msg_send![&*descriptor, setSerialNum: 0x1234u32];

            let _: () = msg_send![&*descriptor, setDispatchQueue: DispatchQueue::main()];
        }

        let display_cls = class!(CGVirtualDisplay);
        let display: Retained<AnyObject> = unsafe { msg_send![msg_send![display_cls, alloc], initWithDescriptor: &*descriptor] };

        let mode_cls = class!(CGVirtualDisplayMode);
        let modes: [Retained<AnyObject>; 2] = [60, 30].map(|framerate| unsafe {
            msg_send![msg_send![mode_cls, alloc],
                initWithWidth: resolution.0 as u64,
                height: resolution.1 as u64,
                refreshRate: framerate as f64
            ]
        });
        let modes = NSArray::from_retained_slice(&modes);

        let settings_cls = class!(CGVirtualDisplaySettings);
        let settings: Retained<AnyObject> = unsafe { msg_send![msg_send![settings_cls, alloc], init] };
        unsafe {
            let _: () = msg_send![&*settings, setHiDPI: 1u32];
            let _: () = msg_send![&*settings, setModes: &*modes];
        }

        let virtual_display = VirtualDisplay { display };
        let success: bool = unsafe { msg_send![&*virtual_display.display, applySettings: &*settings] };
        if success {
            println!("Virtual Display Created: {}", virtual_display.get_id());
        } else {
            panic!("Failed to create Virtual Display");
        }

        virtual_display
    }

    pub fn get_id(&self) -> CGDirectDisplayID {
        unsafe { msg_send![&*self.display, displayID] }
    }
}
