//! Drawing frames on a connection: core's presenter on the wall clock, with
//! the frames encoded in parallel.

use std::ops::{Deref, DerefMut};
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::draw::Encoded;
use streamer_core::jpeg::Image;
use streamer_core::layout::Layout;
use streamer_core::present;
use streamer_core::protocol::{Encoding, Info};
use streamer_device::connection::{Connection, TIMEOUT, report};

pub use streamer_core::present::{LayoutKey, Quality, layout_key};

use crate::{Notice, Notify};

const STATS_PERIOD: Duration = Duration::from_secs(1);

type Result<T> = std::result::Result<T, String>;

/// `image` as JPEG bands, encoded in parallel.
pub fn encode_jpeg(image: &Image, layout: &Layout, info: &Info, quality: u8) -> Encoded {
    let rows = present::jpeg_bands(layout, info);
    thread::scope(|scope| {
        let workers: Vec<_> = rows
            .into_iter()
            .map(|rows| {
                scope.spawn(move || present::encode_band(image, layout, info, quality, rows))
            })
            .collect();
        Encoded::Jpeg(workers.into_iter().map(|w| w.join().unwrap()).collect())
    })
}

pub struct Stats {
    start: Instant,
    pub frames: u32,
    pub dropped: u32,
    pub bytes: usize,
    quality: Option<(u8, u8)>,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start: Instant::now(),
            frames: 0,
            dropped: 0,
            bytes: 0,
            quality: None,
        }
    }

    fn quality(&mut self, quality: u8) {
        let (low, high) = self.quality.get_or_insert((quality, quality));
        *low = (*low).min(quality);
        *high = (*high).max(quality);
    }

    pub fn report(&mut self, audio: Option<(u32, u32)>, notify: &Notify) {
        let elapsed = self.start.elapsed();
        if elapsed < STATS_PERIOD {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        let audio = match audio {
            Some((buffered, capacity)) => format!(", audio {buffered}/{capacity} frames buffered"),
            None => String::new(),
        };
        let quality = match self.quality {
            Some((low, high)) if low == high => format!(", quality {low}"),
            Some((low, high)) => format!(", quality {low}-{high}"),
            None => String::new(),
        };
        notify(Notice::Stats(format!(
            "{:.1} frames/s, {} dropped, {:.1} MB/s{quality}{audio}",
            self.frames as f64 / seconds,
            self.dropped,
            self.bytes as f64 / seconds / 1e6
        )));
        *self = Stats::new();
    }
}

pub struct Presenter {
    core: present::Presenter,
    start: Instant,
    pub stats: Stats,
    pub notify: Notify,
}

impl Deref for Presenter {
    type Target = present::Presenter;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for Presenter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Presenter {
    pub fn new(
        info: Info,
        layout: Layout,
        encoding: Encoding,
        front: bool,
        rotation: Option<u32>,
        quality: Quality,
        notify: Notify,
    ) -> Self {
        Presenter {
            core: present::Presenter::new(info, layout, encoding, front, rotation, quality),
            start: Instant::now(),
            stats: Stats::new(),
            notify,
        }
    }

    fn now(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    /// The JPEG quality for the frame about to be encoded.
    pub fn quality(&mut self) -> u8 {
        let now = self.now();
        let quality = self.core.quality(now);
        self.stats.quality(quality);
        quality
    }

    pub fn describe(&self) {
        (self.notify)(Notice::Layout(self.core.describe()));
    }

    /// Turns the display off, which blackens it; the next frame turns it on.
    pub fn power_off(&mut self, connection: &mut Connection) -> Result<()> {
        let message = self.core.power_off(connection.next_video_seq());
        connection.send_video(message)
    }

    /// How long until a frame stays within max_fps; zero when it does now.
    pub fn until_due(&self) -> Duration {
        Duration::from_micros(self.core.until_due(self.now()))
    }

    pub fn due(&self) -> bool {
        self.core.due(self.now())
    }

    pub fn start_frame(&mut self) {
        let now = self.now();
        self.core.start_frame(now);
    }

    /// Applies the pending events, waiting for SWAP_DONE while swap_window
    /// swaps are unanswered; true when the layout has to be rebuilt.
    pub fn events(&mut self, connection: &mut Connection) -> Result<bool> {
        let mut relayout = false;
        loop {
            let wait = match self.core.swaps_full() {
                true => TIMEOUT,
                false => Duration::ZERO,
            };
            match connection.event(wait)? {
                Some(event) => match self.core.apply(&event) {
                    Some(changed) => {
                        relayout |= changed;
                        (self.notify)(Notice::Event(event));
                    }
                    None => report(&event),
                },
                None if wait > Duration::ZERO => return Err("no SWAP_DONE".into()),
                None => return Ok(relayout),
            }
        }
    }

    /// Draws `frame`, laid out by the current layout.
    pub fn send(&mut self, connection: &mut Connection, frame: Encoded) -> Result<()> {
        let now = self.now();
        let transfers = self
            .core
            .frame(frame, now, &mut || connection.next_video_seq())?;
        let mut bytes = 0;
        for transfer in transfers {
            bytes += transfer.len();
            connection.send_video(transfer)?;
        }
        self.stats.bytes += bytes;
        self.stats.frames += 1;
        Ok(())
    }
}
