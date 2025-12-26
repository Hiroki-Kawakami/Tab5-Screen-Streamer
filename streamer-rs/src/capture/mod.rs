pub struct FrameConvertedData {
    pub data: Box<[u8]>,
    pub data_size: usize,
    pub quality: i32,
    pub fps: Option<usize>,
}

pub struct CaptureConfig {
    pub display: Option<usize>,
    pub quality: Option<i32>
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
