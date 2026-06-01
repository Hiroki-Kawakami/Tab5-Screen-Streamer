// The CGDisplayStream API is deprecated in favour of ScreenCaptureKit, but it is
// intentionally used here as a lighter-weight capture path. Silence the warnings.
#![allow(deprecated)]

use crate::capture::{CaptureConfig, FrameConvertedData};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, sel};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRunLoop, CFString, CFType, CGSize,
};
use objc2_core_graphics::{
    kCGDisplayStreamMinimumFrameTime, kCGDisplayStreamShowCursor, CGDirectDisplayID,
    CGDisplayChangeSummaryFlags, CGDisplayIsInMirrorSet, CGDisplayMirrorsDisplay,
    CGDisplayRegisterReconfigurationCallback, CGDisplayStream, CGDisplayStreamFrameStatus,
    CGDisplayStreamUpdate, CGError, CGGetActiveDisplayList,
};
use objc2_foundation::{NSArray, NSString};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use objc2_core_foundation::CFRetained;
use std::{os::raw::c_void, panic, process, sync::mpsc, thread, time::Duration};

const BUF_SIZE: usize = 512 * 1024;
const JPEG_QUALITY_LEVELS: [i32; 7] = [20, 30, 40, 50, 60, 70, 80];

/// Output frame size requested from the display stream. Matches the device panel
/// (the captured display is scaled into this buffer, preserving aspect ratio).
const OUTPUT_WIDTH: usize = 1280;
const OUTPUT_HEIGHT: usize = 720;

/// CoreVideo/CoreMedia-style four-character pixel format code for packed BGRA8888.
const PIXEL_FORMAT_BGRA: i32 = fourcc(b"BGRA");

const fn fourcc(code: &[u8; 4]) -> i32 {
    ((code[0] as i32) << 24)
        | ((code[1] as i32) << 16)
        | ((code[2] as i32) << 8)
        | (code[3] as i32)
}

/// A single captured frame, copied out of the stream's IOSurface into a tightly
/// packed BGRA buffer (`width * height * 4` bytes, no row padding) so it can be
/// handed to another thread and fed directly to `encode_split_frame`.
struct CapturedFrame {
    data: Vec<u8>,
    width: usize,
    height: usize,
}

pub struct Context {
    rx: mpsc::Receiver<FrameConvertedData>,
}

static mut DISPLAY_WATCH: Option<CGDirectDisplayID> = None;
static mut DISPLAY_UPDATED: bool = false;
unsafe extern "C-unwind" fn display_settings_changed(
    display: u32,
    _flags: CGDisplayChangeSummaryFlags,
    _user_info: *mut c_void,
) {
    // println!("Display Settings Changed: display={}, flags={:?}", display, flags);
    if unsafe { DISPLAY_WATCH } == Some(display) {
        unsafe {
            DISPLAY_UPDATED = true;
        }
    }
}

pub fn check_permission() -> bool {
    if !objc2_core_graphics::CGPreflightScreenCaptureAccess() {
        println!("Permission not granted. Requesting permission...");
        if !objc2_core_graphics::CGRequestScreenCaptureAccess() {
            println!("Permission denied");
            return false;
        }
    }
    return true;
}

pub fn start<F>(config: CaptureConfig, tx_thread: F)
where
    F: FnOnce(Context) + Send + 'static,
{
    let (conv_tx, conv_rx) = mpsc::sync_channel::<FrameConvertedData>(1);

    // Capture Thread
    thread::spawn(move || {
        let (capt_tx, capt_rx) = mpsc::sync_channel::<CapturedFrame>(1);

        let (display_id, _virtual_display) = if let Some(i) = config.display {
            let displays = active_display_list();
            (
                *displays.get(i).expect("Selected display index out of range."),
                None,
            )
        } else {
            let virtual_display = VirtualDisplay::new("M5Stack Tab5", (1280, 720), (110.0, 62.0));
            (virtual_display.get_id(), Some(virtual_display))
        };
        let mut compressor =
            turbojpeg::Compressor::new().expect("Failed to create turbojpeg Compressor");
        let mut transformer =
            turbojpeg::Transformer::new().expect("Failed to create turbojpeg Transformer");

        let mut quality_level: usize = 0;
        compressor
            .set_quality(config.quality.unwrap_or(JPEG_QUALITY_LEVELS[0]))
            .expect("set jpeg quality failed!");
        compressor
            .set_optimize(false)
            .expect("set jpeg optimize failed!");
        compressor
            .set_subsamp(turbojpeg::Subsamp::Sub2x2)
            .expect("set jpeg subsamp failed!");

        loop {
            let (stream, _queue, _handler) =
                start_display_stream(capt_tx.clone(), display_id)
                    .expect("Failed to start CGDisplayStream!");

            let mut compress_buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
            let mut frames = 0;
            let mut start = std::time::Instant::now();
            let mut last = std::time::Instant::now();
            loop {
                let frame = capt_rx.recv_timeout(Duration::from_millis(100));
                if unsafe { DISPLAY_UPDATED } {
                    break;
                }
                let frame = match frame {
                    Ok(f) => f,
                    Err(_) => continue,
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

                let mut converted =
                    unsafe { Box::<[u8]>::new_uninit_slice(BUF_SIZE).assume_init() };
                let size = crate::capture::encode_split_frame(
                    &mut compressor,
                    &mut transformer,
                    &frame.data,
                    frame.width,
                    frame.height,
                    turbojpeg::PixelFormat::BGRA,
                    &mut compress_buffer,
                    &mut converted,
                );
                let data = FrameConvertedData {
                    data: converted,
                    data_size: size,
                    quality: config.quality.unwrap_or(JPEG_QUALITY_LEVELS[quality_level]),
                    fps,
                };
                let _ = conv_tx.try_send(data);

                if config.quality.is_none() {
                    let tx_speed = (size as f64) / last.elapsed().as_secs_f64();
                    if quality_level > 0 && tx_speed > 7e6 {
                        quality_level -= 1;
                        compressor
                            .set_quality(JPEG_QUALITY_LEVELS[quality_level])
                            .expect("set jpeg quality failed!");
                    } else if quality_level + 1 < JPEG_QUALITY_LEVELS.len() && tx_speed < 4e6 {
                        quality_level += 1;
                        compressor
                            .set_quality(JPEG_QUALITY_LEVELS[quality_level])
                            .expect("set jpeg quality failed!");
                    }
                }
                last = std::time::Instant::now();
            }
            println!("Display Settings Changed, Reopening Stream...");
            let _ = CGDisplayStream::stop(Some(&stream));
            thread::sleep(Duration::from_millis(100));
            unsafe {
                DISPLAY_UPDATED = false;
            }
        }
    });

    thread::spawn(move || {
        tx_thread(Context { rx: conv_rx });
    });

    // Run Loop
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        default_hook(panic_info);
        process::exit(1);
    }));
    unsafe {
        let _ = CGDisplayRegisterReconfigurationCallback(
            Some(display_settings_changed),
            std::ptr::null_mut(),
        );
        NSApplication::load();
    }
    CFRunLoop::run();
}

/// Enumerate the currently active displays in the same order CoreGraphics reports
/// them, so a `--display N` index stays stable.
fn active_display_list() -> Vec<CGDirectDisplayID> {
    let mut count: u32 = 0;
    unsafe {
        CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count);
    }
    let mut displays = vec![0 as CGDirectDisplayID; count as usize];
    unsafe {
        CGGetActiveDisplayList(count, displays.as_mut_ptr(), &mut count);
    }
    displays.truncate(count as usize);
    displays
}

/// If `display_id` is mirroring another display, capture the display it mirrors;
/// otherwise capture it directly.
fn capture_source_display(display_id: CGDirectDisplayID) -> CGDirectDisplayID {
    if CGDisplayIsInMirrorSet(display_id) {
        let mirrored = CGDisplayMirrorsDisplay(display_id);
        if mirrored != 0 {
            return mirrored;
        }
    }
    display_id
}

/// Build the CGDisplayStream property dictionary: cap the frame rate at ~60fps and
/// keep the cursor embedded in the captured frames.
fn stream_properties() -> CFRetained<CFDictionary<CFType, CFType>> {
    let key_min_frame_time: &CFString = unsafe { kCGDisplayStreamMinimumFrameTime };
    let key_show_cursor: &CFString = unsafe { kCGDisplayStreamShowCursor };

    let min_frame_time = CFNumber::new_f64(1.0 / 60.0);
    let show_cursor = CFBoolean::new(true);

    CFDictionary::<CFType, CFType>::from_slices(
        &[key_min_frame_time.as_ref(), key_show_cursor.as_ref()],
        &[min_frame_time.as_ref(), show_cursor.as_ref()],
    )
}

/// The frame-available handler block. Closes over the IOSurface copy logic; held
/// alive (along with the dispatch queue) for as long as the stream runs.
type FrameHandler = RcBlock<
    dyn Fn(CGDisplayStreamFrameStatus, u64, *mut IOSurfaceRef, *const CGDisplayStreamUpdate),
>;

fn start_display_stream(
    tx: mpsc::SyncSender<CapturedFrame>,
    display_id: CGDirectDisplayID,
) -> Option<(CFRetained<CGDisplayStream>, DispatchRetained<DispatchQueue>, FrameHandler)> {
    // Watch the originally selected display for reconfiguration, but capture from
    // the display it mirrors (if any).
    unsafe { DISPLAY_WATCH = Some(display_id) };
    let source_display = capture_source_display(display_id);

    let handler: FrameHandler = RcBlock::new(
        move |status: CGDisplayStreamFrameStatus,
              _display_time: u64,
              surface: *mut IOSurfaceRef,
              _update: *const CGDisplayStreamUpdate| {
            if status != CGDisplayStreamFrameStatus::FrameComplete || surface.is_null() {
                return;
            }
            let surface = unsafe { &*surface };

            let mut seed: u32 = 0;
            // Read-only lock; bail if the surface couldn't be mapped.
            if unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, &mut seed) } != 0 {
                return;
            }

            let width = surface.width();
            let height = surface.height();
            let bytes_per_row = surface.bytes_per_row();
            let base = surface.base_address().as_ptr() as *const u8;
            let row_bytes = width * 4;

            let mut data = vec![0u8; row_bytes * height];
            unsafe {
                if bytes_per_row == row_bytes {
                    std::ptr::copy_nonoverlapping(base, data.as_mut_ptr(), row_bytes * height);
                } else {
                    // IOSurface rows may be padded; repack into tightly packed rows.
                    for y in 0..height {
                        std::ptr::copy_nonoverlapping(
                            base.add(y * bytes_per_row),
                            data.as_mut_ptr().add(y * row_bytes),
                            row_bytes,
                        );
                    }
                }
                let _ = surface.unlock(IOSurfaceLockOptions::ReadOnly, &mut seed);
            }

            let _ = tx.try_send(CapturedFrame {
                data,
                width,
                height,
            });
        },
    );

    let properties = stream_properties();
    let queue = DispatchQueue::new("com.tab5.screen-streamer.capture", None);

    let stream = unsafe {
        CGDisplayStream::with_dispatch_queue(
            source_display,
            OUTPUT_WIDTH,
            OUTPUT_HEIGHT,
            PIXEL_FORMAT_BGRA,
            Some(properties.as_ref()),
            &queue,
            RcBlock::as_ptr(&handler),
        )
    }?;

    if CGDisplayStream::start(Some(&stream)) != CGError::Success {
        return None;
    }

    // Keep the queue and handler block alive for the lifetime of the stream.
    Some((stream, queue, handler))
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
    pub fn new(name: &str, resolution: (u32, u32), size: (f64, f64)) -> Self {
        let descriptor_cls = class!(CGVirtualDisplayDescriptor);
        let descriptor: Retained<AnyObject> =
            unsafe { msg_send![msg_send![descriptor_cls, alloc], init] };
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
        let display: Retained<AnyObject> =
            unsafe { msg_send![msg_send![display_cls, alloc], initWithDescriptor: &*descriptor] };

        let mode_cls = class!(CGVirtualDisplayMode);
        let arg1_type = (|| {
            let sel = sel!(initWithWidth:height:refreshRate:);
            let method = mode_cls.instance_method(sel)?;
            let encoding = method.argument_type(2)?;
            Some(encoding.to_str().ok()?.to_owned())
        })();
        let is_u32 = if let Some(arg1_type) = arg1_type {
            arg1_type == "I"
        } else {
            false
        };
        let modes: [Retained<AnyObject>; 2] = [60, 30].map(|framerate| unsafe {
            if is_u32 {
                msg_send![msg_send![mode_cls, alloc],
                    initWithWidth: resolution.0 as u32,
                    height: resolution.1 as u32,
                    refreshRate: framerate as f64
                ]
            } else {
                msg_send![msg_send![mode_cls, alloc],
                    initWithWidth: resolution.0 as u64,
                    height: resolution.1 as u64,
                    refreshRate: framerate as f64
                ]
            }
        });
        let modes = NSArray::from_retained_slice(&modes);

        let settings_cls = class!(CGVirtualDisplaySettings);
        let settings: Retained<AnyObject> =
            unsafe { msg_send![msg_send![settings_cls, alloc], init] };
        unsafe {
            let _: () = msg_send![&*settings, setHiDPI: 1u32];
            let _: () = msg_send![&*settings, setModes: &*modes];
        }

        let virtual_display = VirtualDisplay { display };
        let success: bool =
            unsafe { msg_send![&*virtual_display.display, applySettings: &*settings] };
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
