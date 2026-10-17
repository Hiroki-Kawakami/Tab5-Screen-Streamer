//! Which Tab5s are plugged in, polled on a thread of its own.

use std::thread;
use std::time::Duration;

use streamer_device::link;

const POLL: Duration = Duration::from_millis(500);

/// `changed` gets the serial numbers whenever they change, and once at the
/// start.
pub fn watch(changed: impl Fn(Vec<String>) + Send + 'static) {
    thread::spawn(move || {
        let mut last = None;
        let mut last_error = String::new();
        loop {
            match link::serials_nusb() {
                Ok(mut serials) => {
                    serials.sort();
                    serials.dedup();
                    if last.as_ref() != Some(&serials) {
                        changed(serials.clone());
                        last = Some(serials);
                    }
                    last_error.clear();
                }
                Err(e) => {
                    if e != last_error {
                        eprintln!("devices: {e}");
                        last_error = e;
                    }
                }
            }
            thread::sleep(POLL);
        }
    });
}
