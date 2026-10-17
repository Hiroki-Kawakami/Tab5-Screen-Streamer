//! Windows.Graphics.Capture, scaled and converted to NV12 by the D3D11 video
//! processor.

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, Once};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, LUID, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEX2D_VPIV,
    D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW,
    HDC, HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSOR_SUPPRESSED, CURSORINFO, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE,
    DefWindowProcW, DispatchMessageW, GetCursorInfo, GetMessageW, HWND_MESSAGE, MSG,
    PBT_POWERSETTINGCHANGE, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_UNLOCK,
};
use windows::core::{BOOL, Interface, PCWSTR, Ref, factory, w};

use crate::{Display, Format, Frame};

const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

/// D3D11_VIDEO_PROCESSOR_COLOR_SPACE bits: YCbCr_Matrix 0 is BT.601, and
/// Nominal_Range 2 (bits 4-5) is 0-255.
const RGB_FULL: D3D11_VIDEO_PROCESSOR_COLOR_SPACE =
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
const YCBCR_601_FULL: D3D11_VIDEO_PROCESSOR_COLOR_SPACE =
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 2 << 4 };

pub fn authorize() -> bool {
    true
}

fn monitor(id: u64) -> HMONITOR {
    HMONITOR(id as *mut std::ffi::c_void)
}

fn describe(id: u64) -> Option<Display> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor(id), &mut info.monitorInfo) }.as_bool() {
        return None;
    }
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let name = PCWSTR(info.szDevice.as_ptr());
    unsafe { EnumDisplaySettingsW(name, ENUM_CURRENT_SETTINGS, &mut mode) }
        .as_bool()
        .then_some(Display {
            id,
            size: (mode.dmPelsWidth, mode.dmPelsHeight),
        })
}

fn header<T>(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: size_of::<T>() as u32,
        adapterId: adapter,
        id,
    }
}

pub fn name(id: u64) -> Option<String> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor(id), &mut info.monitorInfo) }.as_bool() {
        return None;
    }
    let (mut path_count, mut mode_count) = (0, 0);
    unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) }
        .ok()
        .ok()?;
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    }
    .ok()
    .ok()?;
    paths.truncate(path_count as usize);
    paths.iter().find_map(|path| {
        let source_info = &path.sourceInfo;
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                source_info.adapterId,
                source_info.id,
            ),
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0
            || source.viewGdiDeviceName != info.szDevice
        {
            return None;
        }
        let target_info = &path.targetInfo;
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                target_info.adapterId,
                target_info.id,
            ),
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0 {
            return None;
        }
        let name = &target.monitorFriendlyDeviceName;
        let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        (length > 0).then(|| String::from_utf16_lossy(&name[..length]))
    })
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    ids: LPARAM,
) -> BOOL {
    unsafe { (*(ids.0 as *mut Vec<u64>)).push(monitor.0 as u64) };
    true.into()
}

pub fn displays() -> Result<Vec<Display>, String> {
    let mut ids: Vec<u64> = Vec::new();
    let ids_param = LPARAM(&mut ids as *mut Vec<u64> as isize);
    unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), ids_param) }
        .ok()
        .map_err(|e| format!("monitors: {e}"))?;
    Ok(ids.into_iter().filter_map(describe).collect())
}

pub fn display(id: u64) -> Option<Display> {
    describe(id)
}

pub fn bounds(id: u64) -> Option<(f64, f64, f64, f64)> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor(id), &mut info) }
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
    let primary = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTONULL) };
    if primary.is_invalid() {
        return Err("no primary monitor".into());
    }
    describe(primary.0 as u64).ok_or_else(|| "primary monitor has no size".into())
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

fn d3d_device() -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    Ok((device.unwrap(), context.unwrap()))
}

fn winrt_device(device: &ID3D11Device) -> windows::core::Result<IDirect3DDevice> {
    let dxgi: IDXGIDevice = device.cast()?;
    unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }?.cast()
}

/// Windows hides the cursor while touch or pen input drives it, but the
/// capture draws it regardless of that.
fn cursor_shown() -> bool {
    let mut info = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetCursorInfo(&mut info) }.is_err() {
        return true;
    }
    info.flags.0 & (CURSOR_SHOWING.0 | CURSOR_SUPPRESSED.0) == CURSOR_SHOWING.0
}

/// Polled rather than checked per frame: while the cursor is left out of the
/// capture, moving it brings no frame.
fn follow_cursor(session: GraphicsCaptureSession, interval: Duration, stop: Receiver<()>) {
    let mut shown = cursor_shown();
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(interval) {
        let now = cursor_shown();
        if now == shown {
            continue;
        }
        if let Err(e) = session.SetIsCursorCaptureEnabled(now) {
            eprintln!("cursor capture: {e}");
            return;
        }
        shown = now;
    }
}

struct Sink {
    frames: SyncSender<Frame>,
    size: (u32, u32),
    pool_size: SizeInt32,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    scaler: Option<Scaler>,
}

impl Sink {
    fn receive(&mut self, pool: &Direct3D11CaptureFramePool) -> Result<(), String> {
        let Ok(frame) = pool.TryGetNextFrame() else {
            return Ok(());
        };
        let content = frame
            .ContentSize()
            .map_err(|e| format!("frame size: {e}"))?;
        if content != self.pool_size {
            drop(frame);
            winrt_device(&self.device)
                .and_then(|device| pool.Recreate(&device, PIXEL_FORMAT, 1, content))
                .map_err(|e| format!("recreate frame pool: {e}"))?;
            self.pool_size = content;
            return Ok(());
        }
        let texture: ID3D11Texture2D = frame
            .Surface()
            .and_then(|surface| surface.cast::<IDirect3DDxgiInterfaceAccess>())
            .and_then(|access| unsafe { access.GetInterface() })
            .map_err(|e| format!("frame texture: {e}"))?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let input_size = (desc.Width, desc.Height);
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
            .scale(&texture, &self.context, &self.video_context)
            .map_err(|e| format!("scale: {e}"))?;
        let _ = self.frames.try_send(scaled);
        Ok(())
    }
}

pub struct Capture {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    closed_token: i64,
    arrived_token: i64,
    cursor: Option<(Sender<()>, JoinHandle<()>)>,
    ended: Arc<Mutex<Option<String>>>,
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
        let interval = Duration::from_secs_f64(1.0 / max_fps.max(1) as f64);
        let (device, context) = d3d_device().map_err(|e| format!("D3D11 device: {e}"))?;
        let video = device
            .cast::<ID3D11VideoDevice>()
            .map_err(|e| format!("D3D11 video device: {e}"))?;
        let video_context = context
            .cast::<ID3D11VideoContext>()
            .map_err(|e| format!("D3D11 video context: {e}"))?;
        let item: GraphicsCaptureItem =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .and_then(|interop| unsafe { interop.CreateForMonitor(monitor(id)) })
                .map_err(|e| format!("capture item: {e}"))?;
        let pool_size = item.Size().map_err(|e| format!("capture size: {e}"))?;
        let pool = winrt_device(&device)
            .and_then(|winrt| {
                Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, PIXEL_FORMAT, 1, pool_size)
            })
            .map_err(|e| format!("frame pool: {e}"))?;
        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|e| format!("capture session: {e}"))?;
        session
            .SetIsCursorCaptureEnabled(cursor_shown())
            .and_then(|()| session.SetIsBorderRequired(false))
            .and_then(|()| session.SetMinUpdateInterval(interval.into()))
            .map_err(|e| format!("capture settings: {e}"))?;

        let ended = Arc::new(Mutex::new(None));
        let closed_token = item
            .Closed(&TypedEventHandler::new({
                let ended = ended.clone();
                move |_, _| {
                    ended.lock().unwrap().get_or_insert("capture ended".into());
                    Ok(())
                }
            }))
            .map_err(|e| format!("capture closed handler: {e}"))?;
        let sink = Mutex::new(Sink {
            frames,
            size,
            pool_size,
            device,
            context,
            video,
            video_context,
            scaler: None,
        });
        let arrived_token = pool
            .FrameArrived(&TypedEventHandler::new({
                let ended = ended.clone();
                move |pool: Ref<Direct3D11CaptureFramePool>, _| {
                    let Some(pool) = pool.as_ref() else {
                        return Ok(());
                    };
                    if ended.lock().unwrap().is_some() {
                        return Ok(());
                    }
                    if let Err(e) = sink.lock().unwrap().receive(pool) {
                        ended.lock().unwrap().get_or_insert(e);
                    }
                    Ok(())
                }
            }))
            .map_err(|e| format!("frame handler: {e}"))?;
        let mut capture = Capture {
            item,
            pool,
            session,
            closed_token,
            arrived_token,
            cursor: None,
            ended,
            unlocks,
        };
        capture
            .session
            .StartCapture()
            .map_err(|e| format!("start capture: {e}"))?;
        let (stop, stopped) = mpsc::channel();
        let session = capture.session.clone();
        let thread = thread::spawn(move || follow_cursor(session, interval, stopped));
        capture.cursor = Some((stop, thread));
        Ok(capture)
    }
}

impl Capture {
    pub fn stopped(&self) -> Option<String> {
        if let Some(reason) = self.ended.lock().unwrap().clone() {
            return Some(reason);
        }
        (UNLOCKS.load(Ordering::Relaxed) != self.unlocks).then(|| "session unlocked".into())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some((stop, thread)) = self.cursor.take() {
            drop(stop);
            let _ = thread.join();
        }
        let _ = self.pool.RemoveFrameArrived(self.arrived_token);
        let _ = self.item.RemoveClosed(self.closed_token);
        if let Err(e) = self.session.Close().and_then(|()| self.pool.Close()) {
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
