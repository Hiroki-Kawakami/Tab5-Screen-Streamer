//! ScreenCaptureKit, scaling and converting to NV12 on the GPU.

use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_core_graphics::{
    CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID, CGPreflightScreenCaptureAccess,
    CGRequestScreenCaptureAccess, kCGDisplayStreamYCbCrMatrix_ITU_R_601_4,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType,
};

use crate::{Display, Format, Frame};

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Moves an object out of a completion handler's thread.
struct Sendable<T>(T);

// SAFETY: only ScreenCaptureKit objects and NSErrors cross, which it hands
// to arbitrary queues itself.
unsafe impl<T> Send for Sendable<T> {}

fn describe(error: *mut NSError) -> String {
    match unsafe { error.as_ref() } {
        Some(error) => error.localizedDescription().to_string(),
        None => "unknown error".into(),
    }
}

/// Runs a ScreenCaptureKit call taking a completion handler, and waits for it.
fn wait_for(call: impl FnOnce(&RcBlock<dyn Fn(*mut NSError)>)) -> Result<(), String> {
    let (sender, reply) = mpsc::channel();
    let block = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            Err(describe(error))
        };
        let _ = sender.send(result);
    });
    call(&block);
    reply
        .recv_timeout(REPLY_TIMEOUT)
        .map_err(|_| "no reply".to_string())?
}

fn shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (sender, reply) = mpsc::channel();
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(Sendable(content)),
                None => Err(describe(error)),
            };
            let _ = sender.send(result);
        },
    );
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    let content = reply
        .recv_timeout(REPLY_TIMEOUT)
        .map_err(|_| "shareable content: no reply".to_string())?;
    content
        .map(|c| c.0)
        .map_err(|e| format!("shareable content: {e}"))
}

pub fn authorize() -> bool {
    CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()
}

pub fn displays() -> Result<Vec<Display>, String> {
    let content = shareable_content()?;
    let displays = unsafe { content.displays() };
    Ok(displays
        .iter()
        .map(|d| Display {
            id: unsafe { d.displayID() } as u64,
            size: unsafe { (d.width() as u32, d.height() as u32) },
        })
        .collect())
}

pub fn display(id: u64) -> Option<Display> {
    let display = id as u32;
    let size = (
        CGDisplayPixelsWide(display) as u32,
        CGDisplayPixelsHigh(display) as u32,
    );
    (size.0 > 0 && size.1 > 0).then_some(Display { id, size })
}

pub fn main_display() -> Result<Display, String> {
    display(CGMainDisplayID() as u64).ok_or_else(|| "no main display".into())
}

struct Ivars {
    frames: SyncSender<Frame>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "StreamerCaptureOutput"]
    #[ivars = Ivars]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            if let Some(frame) = unsafe { copy_frame(sample) } {
                let _ = self.ivars().frames.try_send(frame);
            }
        }
    }
);

impl Output {
    fn new(frames: SyncSender<Frame>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { frames });
        unsafe { msg_send![super(this), init] }
    }
}

/// Copies the NV12 image out at once: ScreenCaptureKit stalls once its few
/// buffers are all held. None for frames without an image (screen idle).
unsafe fn copy_frame(sample: &CMSampleBuffer) -> Option<Frame> {
    let buffer = unsafe { sample.image_buffer() }?;
    let flags = CVPixelBufferLockFlags::ReadOnly;
    if unsafe { CVPixelBufferLockBaseAddress(&buffer, flags) } != 0 {
        return None;
    }
    let (width, height) = (
        CVPixelBufferGetWidth(&buffer),
        CVPixelBufferGetHeight(&buffer),
    );
    let mut data = Vec::with_capacity(width * height * 3 / 2);
    for (plane, rows) in [(0, height), (1, height / 2)] {
        let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane) as *const u8;
        let stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane);
        if base.is_null() {
            unsafe { CVPixelBufferUnlockBaseAddress(&buffer, flags) };
            return None;
        }
        for row in 0..rows {
            let line = unsafe { std::slice::from_raw_parts(base.add(row * stride), width) };
            data.extend_from_slice(line);
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&buffer, flags) };
    Some(Frame {
        width,
        height,
        format: Format::Nv12,
        data,
    })
}

pub struct Capture {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    _queue: DispatchRetained<DispatchQueue>,
}

impl Capture {
    pub fn start(
        id: u64,
        size: (u32, u32),
        max_fps: u32,
        frames: SyncSender<Frame>,
    ) -> Result<Self, String> {
        let content = shareable_content()?;
        let displays = unsafe { content.displays() };
        let display = displays
            .iter()
            .find(|d| unsafe { d.displayID() } as u64 == id)
            .ok_or_else(|| format!("display {id} cannot be captured"))?;
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            )
        };
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(size.0 as usize);
            config.setHeight(size.1 as usize);
            config.setMinimumFrameInterval(CMTime::new(1, max_fps.max(1) as i32));
            config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
            config.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_601_4);
            config.setShowsCursor(true);
        }
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                None,
            )
        };
        let output = Output::new(frames);
        let queue = DispatchQueue::new("streamer.capture", None);
        unsafe {
            stream.addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*output),
                SCStreamOutputType::Screen,
                Some(&queue),
            )
        }
        .map_err(|e| format!("capture output: {}", e.localizedDescription()))?;
        wait_for(|done| unsafe { stream.startCaptureWithCompletionHandler(Some(done)) })
            .map_err(|e| format!("start capture: {e}"))?;
        Ok(Capture {
            stream,
            _output: output,
            _queue: queue,
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let stream = &self.stream;
        if let Err(e) =
            wait_for(|done| unsafe { stream.stopCaptureWithCompletionHandler(Some(done)) })
        {
            eprintln!("stop capture: {e}");
        }
    }
}
