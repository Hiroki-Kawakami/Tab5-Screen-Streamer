use std::sync::Mutex;
use std::time::Instant;

/// Steps smaller than this are jitter of the audio reports, not drift.
const STEP_US: i64 = 15_000;

/// The input time being presented, in µs. Bulk audio sets it from what the
/// device is playing; otherwise it runs from where the first frame set it.
#[derive(Default)]
pub struct Clock {
    anchor: Mutex<Option<(Instant, i64)>>,
}

impl Clock {
    pub fn now(&self) -> Option<i64> {
        let anchor = *self.anchor.lock().unwrap();
        anchor.map(|(at, pts)| pts + at.elapsed().as_micros() as i64)
    }

    /// The current time, starting the clock at `pts` if it has not started.
    pub fn now_or_start(&self, pts: i64) -> i64 {
        let mut anchor = self.anchor.lock().unwrap();
        let (at, base) = *anchor.get_or_insert((Instant::now(), pts));
        base + at.elapsed().as_micros() as i64
    }

    pub fn set(&self, pts: i64) {
        let mut anchor = self.anchor.lock().unwrap();
        let now = anchor.map(|(at, base)| base + at.elapsed().as_micros() as i64);
        if now.is_none_or(|now| (now - pts).abs() > STEP_US) {
            *anchor = Some((Instant::now(), pts));
        }
    }
}
