pub struct FrameConvertedData {
    pub data: Box<[u8]>,
    pub data_size: usize,
    pub quality: i32,
    pub fps: Option<usize>,
}

pub fn check_permission() -> bool {
    if !scap::is_supported() {
        println!("Platform not supported!");
        return false;
    }
    if !scap::has_permission() {
        println!("Permission not granted. Requesting permission...");
        if !scap::request_permission() {
            println!("Permission denied");
            return false;
        }
    }
    return true;
}

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(target_os = "windows")]
pub mod common;
#[cfg(target_os = "windows")]
pub use self::common::*;

#[cfg(target_os = "linux")]
pub mod common;
#[cfg(target_os = "linux")]
pub use self::common::*;
