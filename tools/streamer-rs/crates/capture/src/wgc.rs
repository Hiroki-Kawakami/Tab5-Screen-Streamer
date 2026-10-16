//! Windows.Graphics.Capture, scaled and converted to NV12 by the D3D11 video
//! processor.

use std::mem::ManuallyDrop;
use std::sync::Once;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DispatchMessageW, GetMessageW,
    HWND_MESSAGE, MSG, PBT_POWERSETTINGCHANGE, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_UNLOCK,
};
use windows::core::{Interface, w};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame as CapturedFrame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::{Display, Format, Frame};

type Error = Box<dyn std::error::Error + Send + Sync>;

/// D3D11_VIDEO_PROCESSOR_COLOR_SPACE bits: YCbCr_Matrix 0 is BT.601, and
/// Nominal_Range 2 (bits 4-5) is 0-255.
const RGB_FULL: D3D11_VIDEO_PROCESSOR_COLOR_SPACE =
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
const YCBCR_601_FULL: D3D11_VIDEO_PROCESSOR_COLOR_SPACE =
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 2 << 4 };

pub fn authorize() -> bool {
    true
}

fn describe(monitor: Monitor) -> Option<Display> {
    let size = (monitor.width().ok()?, monitor.height().ok()?);
    Some(Display {
        id: monitor.as_raw_hmonitor() as u64,
        size,
    })
}

pub fn displays() -> Result<Vec<Display>, String> {
    let monitors = Monitor::enumerate().map_err(|e| format!("monitors: {e}"))?;
    Ok(monitors.into_iter().filter_map(describe).collect())
}

pub fn display(id: u64) -> Option<Display> {
    describe(Monitor::from_raw_hmonitor(id as *mut std::ffi::c_void))
}

pub fn bounds(id: u64) -> Option<(f64, f64, f64, f64)> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(HMONITOR(id as *mut std::ffi::c_void), &mut info) }
        .as_bool()
        .then(|| {
            let r = info.rcMonitor;
            let (x, y) = (r.left as f64, r.top as f64);
            (x, y, (r.right - r.left) as f64, (r.bottom - r.top) as f64)
        })
}

/// GUID_CONSOLE_DISPLAY_STATE: 0 off, 1 on, 2 dimmed.
static DISPLAY_STATE: AtomicU32 = AtomicU32::new(1);
static UNLOCKS: AtomicU32 = AtomicU32::new(0);
static WATCH_SESSION: Once = Once::new();

fn watch_session() {
    WATCH_SESSION.call_once(|| {
        thread::spawn(|| {
            if let Err(e) = receive_session_events() {
                eprintln!("session events: {e}");
            }
        });
    });
}

/// Windows reports one state for all displays.
pub fn asleep(_id: u64) -> bool {
    watch_session();
    DISPLAY_STATE.load(Ordering::Relaxed) == 0
}

/// Receives display state and session changes on a message-only window, the
/// current display state first; returns only on failure.
fn receive_session_events() -> windows::core::Result<()> {
    let class = w!("StreamerSessionEvents");
    let instance = unsafe { GetModuleHandleW(None) }?;
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(session_events_proc),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            None,
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
    }?;
    unsafe {
        RegisterPowerSettingNotification(
            HANDLE(window.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
    }?;
    unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) }?;
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe { DispatchMessageW(&message) };
    }
    Ok(())
}

extern "system" fn session_events_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_POWERBROADCAST && wparam.0 as u32 == PBT_POWERSETTINGCHANGE {
        let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
        if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 4 {
            let state = unsafe { setting.Data.as_ptr().cast::<u32>().read_unaligned() };
            DISPLAY_STATE.store(state, Ordering::Relaxed);
        }
        return LRESULT(1);
    }
    if message == WM_WTSSESSION_CHANGE && wparam.0 as u32 == WTS_SESSION_UNLOCK {
        UNLOCKS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

pub fn mirror_primary(_id: u64) -> Option<u64> {
    None
}

pub fn main_display() -> Result<Display, String> {
    let monitor = Monitor::primary().map_err(|e| format!("primary monitor: {e}"))?;
    describe(monitor).ok_or_else(|| "primary monitor has no size".into())
}

fn texture(
    device: &ID3D11Device,
    size: (u32, u32),
    format: DXGI_FORMAT,
    staging: bool,
) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: if staging {
            D3D11_USAGE_STAGING
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: if staging {
            0
        } else {
            D3D11_BIND_RENDER_TARGET.0 as u32
        },
        CPUAccessFlags: if staging {
            D3D11_CPU_ACCESS_READ.0 as u32
        } else {
            0
        },
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
    Ok(texture.unwrap())
}

/// Scales a BGRA texture of one size into NV12 of another.
struct Scaler {
    input_size: (u32, u32),
    output_size: (u32, u32),
    input: ID3D11Texture2D,
    input_view: ID3D11VideoProcessorInputView,
    processor: ID3D11VideoProcessor,
    output_view: ID3D11VideoProcessorOutputView,
    output: ID3D11Texture2D,
    staging: ID3D11Texture2D,
}

impl Scaler {
    fn new(
        device: &ID3D11Device,
        video: &ID3D11VideoDevice,
        context: &ID3D11VideoContext,
        input_size: (u32, u32),
        output_size: (u32, u32),
    ) -> windows::core::Result<Self> {
        let rate = DXGI_RATIONAL {
            Numerator: 60,
            Denominator: 1,
        };
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: input_size.0,
            InputHeight: input_size.1,
            OutputFrameRate: rate,
            OutputWidth: output_size.0,
            OutputHeight: output_size.1,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let enumerator: ID3D11VideoProcessorEnumerator =
            unsafe { video.CreateVideoProcessorEnumerator(&content) }?;
        let processor = unsafe { video.CreateVideoProcessor(&enumerator, 0) }?;

        let input = texture(device, input_size, DXGI_FORMAT_B8G8R8A8_UNORM, false)?;
        let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        unsafe {
            video.CreateVideoProcessorInputView(
                &input,
                &enumerator,
                &input_desc,
                Some(&mut input_view),
            )
        }?;

        let output = texture(device, output_size, DXGI_FORMAT_NV12, false)?;
        let output_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view = None;
        unsafe {
            video.CreateVideoProcessorOutputView(
                &output,
                &enumerator,
                &output_desc,
                Some(&mut output_view),
            )
        }?;
        let staging = texture(device, output_size, DXGI_FORMAT_NV12, true)?;

        unsafe {
            context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            context.VideoProcessorSetStreamColorSpace(&processor, 0, &RGB_FULL);
            context.VideoProcessorSetOutputColorSpace(&processor, &YCBCR_601_FULL);
        }
        Ok(Scaler {
            input_size,
            output_size,
            input,
            input_view: input_view.unwrap(),
            processor,
            output_view: output_view.unwrap(),
            output,
            staging,
        })
    }

    fn scale(
        &self,
        source: &ID3D11Texture2D,
        context: &ID3D11DeviceContext,
        video: &ID3D11VideoContext,
    ) -> windows::core::Result<Frame> {
        unsafe { context.CopyResource(&self.input, source) };
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(self.input_view.clone())),
            ..Default::default()
        };
        let streams = [stream];
        let blt =
            unsafe { video.VideoProcessorBlt(&self.processor, &self.output_view, 0, &streams) };
        let [stream] = streams;
        drop(ManuallyDrop::into_inner(stream.pInputSurface));
        blt?;
        unsafe { context.CopyResource(&self.staging, &self.output) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }?;
        let (width, height) = (self.output_size.0 as usize, self.output_size.1 as usize);
        let pitch = mapped.RowPitch as usize;
        let base = mapped.pData as *const u8;
        let mut data = Vec::with_capacity(width * height * 3 / 2);
        // The CbCr plane follows the Y plane's `height` rows of `pitch`.
        for row in (0..height).chain(height..height + height / 2) {
            let line = unsafe { std::slice::from_raw_parts(base.add(row * pitch), width) };
            data.extend_from_slice(line);
        }
        unsafe { context.Unmap(&self.staging, 0) };
        Ok(Frame {
            width,
            height,
            format: Format::Nv12,
            data,
        })
    }
}

struct Handler {
    frames: SyncSender<Frame>,
    size: (u32, u32),
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    scaler: Option<Scaler>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = (SyncSender<Frame>, (u32, u32));
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let video = ctx
            .device
            .cast::<ID3D11VideoDevice>()
            .map_err(|e| format!("D3D11 video device: {e}"))?;
        let video_context = ctx
            .device_context
            .cast::<ID3D11VideoContext>()
            .map_err(|e| format!("D3D11 video context: {e}"))?;
        Ok(Handler {
            frames: ctx.flags.0,
            size: ctx.flags.1,
            device: ctx.device,
            context: ctx.device_context,
            video,
            video_context,
            scaler: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut CapturedFrame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let input_size = (frame.width(), frame.height());
        if self
            .scaler
            .as_ref()
            .is_none_or(|s| s.input_size != input_size)
        {
            let scaler = Scaler::new(
                &self.device,
                &self.video,
                &self.video_context,
                input_size,
                self.size,
            )
            .map_err(|e| format!("video processor: {e}"))?;
            self.scaler = Some(scaler);
        }
        let scaler = self.scaler.as_ref().unwrap();
        let scaled = scaler
            .scale(frame.as_raw_texture(), &self.context, &self.video_context)
            .map_err(|e| format!("scale: {e}"))?;
        let _ = self.frames.try_send(scaled);
        Ok(())
    }
}

pub struct Capture {
    control: Option<CaptureControl<Handler, Error>>,
    /// A capture started on the lock screen yields only black frames once the
    /// session is unlocked, so every unlock after the start restarts it.
    unlocks: u32,
}

impl Capture {
    pub fn start(
        id: u64,
        size: (u32, u32),
        max_fps: u32,
        frames: SyncSender<Frame>,
    ) -> Result<Self, String> {
        watch_session();
        let unlocks = UNLOCKS.load(Ordering::Relaxed);
        let monitor = Monitor::from_raw_hmonitor(id as *mut std::ffi::c_void);
        let interval = Duration::from_secs_f64(1.0 / max_fps.max(1) as f64);
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(interval),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            (frames, size),
        );
        let control =
            Handler::start_free_threaded(settings).map_err(|e| format!("start capture: {e}"))?;
        Ok(Capture {
            control: Some(control),
            unlocks,
        })
    }
}

impl Capture {
    pub fn stopped(&self) -> Option<String> {
        if self.control.as_ref().is_none_or(|c| c.is_finished()) {
            return Some("capture ended".into());
        }
        (UNLOCKS.load(Ordering::Relaxed) != self.unlocks).then(|| "session unlocked".into())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(control) = self.control.take()
            && let Err(e) = control.stop()
        {
            eprintln!("stop capture: {e}");
        }
    }
}

pub struct VirtualDisplay(std::convert::Infallible);

impl VirtualDisplay {
    pub fn new(
        _name: &str,
        _serial: u32,
        _size: (u32, u32),
        _millimeters: (f64, f64),
    ) -> Result<Self, String> {
        Err("virtual displays are macOS only".into())
    }

    pub fn id(&self) -> u64 {
        match self.0 {}
    }

    pub fn resize(&self, _size: (u32, u32)) -> Result<(), String> {
        match self.0 {}
    }
}
