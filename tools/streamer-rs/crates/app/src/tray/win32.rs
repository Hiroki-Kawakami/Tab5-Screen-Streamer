use std::cell::Cell;

use egui_glow::egui_winit::winit::dpi::{LogicalSize, PhysicalPosition, Position};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyWindow, FindWindowW, GWLP_USERDATA, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowThreadProcessId, HICON, LR_DEFAULTCOLOR, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_APP,
    WM_LBUTTONUP, WM_RBUTTONUP, WM_SETTINGCHANGE, WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::Icon;

const CLASS: PCWSTR = w!("StreamerTray");
/// Held by the running instance, for its login session.
const INSTANCE_MUTEX: PCWSTR = w!("Local\\Tab5Streamer");
const CALLBACK: u32 = WM_APP + 1;
const ID: u32 = 1;
const TIP: &str = "Tab5 Streamer";
/// Between the taskbar and the panel, as for the taskbar's own flyouts, and
/// between the panel and the screen edges.
const GAP: f64 = 12.0;
/// CreateIconFromResourceEx's resource format version for icons.
const ICON_VERSION: u32 = 0x0003_0000;

fn ico(icon: Icon, light: bool) -> &'static [u8] {
    match (icon, light) {
        (Icon::Disabled, true) => include_bytes!("../../icons/tray-disabled-light.ico"),
        (Icon::Disabled, false) => include_bytes!("../../icons/tray-disabled-dark.ico"),
        (Icon::Disconnected, true) => include_bytes!("../../icons/tray-disconnected-light.ico"),
        (Icon::Disconnected, false) => include_bytes!("../../icons/tray-disconnected-dark.ico"),
        (Icon::Streaming, true) => include_bytes!("../../icons/tray-streaming-light.ico"),
        (Icon::Streaming, false) => include_bytes!("../../icons/tray-streaming-dark.ico"),
    }
}

/// The image of the smallest entry at least `size` wide, or else the largest.
fn ico_image(ico: &[u8], size: u32) -> Option<&[u8]> {
    let count = u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?) as usize;
    let mut best: Option<(u32, &[u8])> = None;
    for index in 0..count {
        let entry = ico.get(6 + index * 16..6 + (index + 1) * 16)?;
        let width = match entry[0] {
            0 => 256,
            width => width as u32,
        };
        let bytes = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let image = ico.get(offset..offset + bytes)?;
        let better = match best {
            None => true,
            Some((best, _)) if best < size => width > best,
            Some((best, _)) => width >= size && width < best,
        };
        if better {
            best = Some((width, image));
        }
    }
    best.map(|(_, image)| image)
}

/// The running instance's tray window gets a click on its icon, and may take
/// the foreground from this one.
pub fn hand_over() -> bool {
    let mutex = unsafe { CreateMutexW(None, false, INSTANCE_MUTEX) };
    if mutex.is_err() || unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return false;
    }
    let Ok(window) = (unsafe { FindWindowW(CLASS, PCWSTR::null()) }) else {
        return true;
    };
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    unsafe {
        let _ = AllowSetForegroundWindow(process);
        let _ = PostMessageW(Some(window), CALLBACK, WPARAM(0), LPARAM(WM_LBUTTONUP as isize));
    }
    true
}

/// Whether the taskbar is light, so the icon is drawn dark.
fn light_taskbar() -> bool {
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut bytes),
        )
    };
    result == ERROR_SUCCESS && value != 0
}

fn load_icon(icon: Icon) -> Option<HICON> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    let image = ico_image(ico(icon, light_taskbar()), size as u32)?;
    let loaded =
        unsafe { CreateIconFromResourceEx(image, true, ICON_VERSION, size, size, LR_DEFAULTCOLOR) };
    loaded.inspect_err(|e| eprintln!("tray icon: {e}")).ok()
}

struct State {
    clicked: Box<dyn Fn()>,
    icon: Cell<Icon>,
    handle: Cell<Option<HICON>>,
    /// Sent when Explorer restarts, which drops every icon.
    taskbar_created: u32,
}

pub struct Tray {
    window: HWND,
    state: Box<State>,
}

fn notify_data(window: HWND, icon: Option<HICON>) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ID,
        uFlags: NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: CALLBACK,
        ..Default::default()
    };
    if let Some(icon) = icon {
        data.uFlags |= NIF_ICON;
        data.hIcon = icon;
    }
    for (to, from) in data.szTip.iter_mut().zip(TIP.encode_utf16()) {
        *to = from;
    }
    data
}

impl State {
    /// Loads the icon again for the current taskbar theme.
    fn update(&self, window: HWND, add: bool) {
        let previous = self.handle.replace(load_icon(self.icon.get()));
        let data = notify_data(window, self.handle.get());
        let message = if add { NIM_ADD } else { NIM_MODIFY };
        if !unsafe { Shell_NotifyIconW(message, &data) }.as_bool() {
            eprintln!("tray: Shell_NotifyIconW failed");
        }
        if let Some(previous) = previous {
            let _ = unsafe { DestroyIcon(previous) };
        }
    }
}

extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let state = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const State;
    if let Some(state) = unsafe { state.as_ref() } {
        if message == CALLBACK {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP {
                (state.clicked)();
            }
            return LRESULT(0);
        }
        if message == WM_SETTINGCHANGE {
            state.update(window, false);
        } else if message == state.taskbar_created {
            state.update(window, true);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

impl Tray {
    /// The window is a hidden top-level one, not message-only: those do not
    /// get the broadcasts for theme changes and Explorer restarts.
    pub fn new(icon: Icon, clicked: Box<dyn Fn()>) -> Result<Self, String> {
        let class = CLASS;
        let instance = unsafe { GetModuleHandleW(None) }.map_err(|e| format!("tray: {e}"))?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(format!("tray: {}", windows::core::Error::from_thread()));
        }
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|e| format!("tray window: {e}"))?;
        let state = Box::new(State {
            clicked,
            icon: Cell::new(icon),
            handle: Cell::new(None),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        });
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, &*state as *const State as isize) };
        state.update(window, true);
        Ok(Tray { window, state })
    }

    pub fn set_icon(&mut self, icon: Icon) {
        self.state.icon.set(icon);
        self.state.update(self.window, false);
    }

    /// Next to the icon, on the side of the screen away from the taskbar.
    pub fn place(&self, size: LogicalSize<f64>) -> Option<Position> {
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.window,
            uID: ID,
            ..Default::default()
        };
        let icon = unsafe { Shell_NotifyIconGetRect(&identifier) }.ok()?;
        let monitor = unsafe { MonitorFromRect(&icon, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }
        let (mut dpi, mut dpi_y) = (96, 96);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) }.ok()?;
        let scale = dpi as f64 / 96.0;
        let (width, height, gap) = (size.width * scale, size.height * scale, GAP * scale);
        let (work, full): (RECT, RECT) = (info.rcWork, info.rcMonitor);
        let center = (
            (icon.left + icon.right) as f64 / 2.0,
            (icon.top + icon.bottom) as f64 / 2.0,
        );
        let (x, y) = if work.left > full.left {
            (work.left as f64 + gap, center.1 - height / 2.0)
        } else if work.right < full.right {
            (work.right as f64 - width - gap, center.1 - height / 2.0)
        } else if work.top > full.top {
            (center.0 - width / 2.0, work.top as f64 + gap)
        } else {
            (center.0 - width / 2.0, work.bottom as f64 - height - gap)
        };
        let x = x.min(work.right as f64 - width - gap).max(work.left as f64 + gap);
        let y = y.min(work.bottom as f64 - height - gap).max(work.top as f64 + gap);
        Some(PhysicalPosition::new(x.round() as i32, y.round() as i32).into())
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = notify_data(self.window, None);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.window);
        }
        if let Some(handle) = self.state.handle.take() {
            let _ = unsafe { DestroyIcon(handle) };
        }
    }
}
