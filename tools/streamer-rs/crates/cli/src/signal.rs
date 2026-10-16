//! Ctrl+C and termination: the first asks the streaming loops to stop and
//! close the session, the second exits at once.

use std::sync::atomic::{AtomicBool, Ordering};

static STOPPING: AtomicBool = AtomicBool::new(false);

pub fn install() {
    os::install();
}

pub fn stopping() -> bool {
    STOPPING.load(Ordering::Relaxed)
}

pub fn flag() -> &'static AtomicBool {
    &STOPPING
}

/// Lets a console that is being closed end the process.
pub fn finished() {
    os::finished();
}

#[cfg(unix)]
mod os {
    use std::ffi::c_int;
    use std::sync::atomic::Ordering;

    const SIGINT: c_int = 2;
    const SIGTERM: c_int = 15;

    unsafe extern "C" {
        fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> usize;
        fn _exit(status: c_int) -> !;
    }

    extern "C" fn handle(_: c_int) {
        if super::STOPPING.swap(true, Ordering::Relaxed) {
            unsafe { _exit(130) };
        }
    }

    pub fn install() {
        unsafe {
            signal(SIGINT, handle);
            signal(SIGTERM, handle);
        }
    }

    pub fn finished() {}
}

#[cfg(windows)]
mod os {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    const CTRL_BREAK_EVENT: u32 = 1;

    static FINISHED: AtomicBool = AtomicBool::new(false);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    // Runs on its own thread. Returning false hands the event to the default
    // handler, which exits; after a console close, logoff or shutdown the
    // process is ended as soon as this returns.
    unsafe extern "system" fn handle(kind: u32) -> i32 {
        if super::STOPPING.swap(true, Ordering::Relaxed) {
            return 0;
        }
        if kind > CTRL_BREAK_EVENT {
            while !FINISHED.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(10));
            }
        }
        1
    }

    pub fn install() {
        unsafe { SetConsoleCtrlHandler(Some(handle), 1) };
    }

    pub fn finished() {
        FINISHED.store(true, Ordering::Relaxed);
    }
}
