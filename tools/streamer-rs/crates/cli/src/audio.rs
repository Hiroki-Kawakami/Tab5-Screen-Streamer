//! Bulk audio, paced by the device's buffer level.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use streamer_core::protocol;
use streamer_device::connection::{AudioOut, TIMEOUT};

use crate::clock::Clock;
use crate::media::{AudioFormat, Frame};

const CHUNKS_PER_SECOND: u32 = 50;
const TARGET_CHUNKS: u32 = 3;
const POLL: Duration = Duration::from_millis(2);

/// Bulk audio to one connection, keeping about TARGET_CHUNKS queued on the
/// device so its endpoint never fills and stalls the others.
struct Sink {
    out: AudioOut,
    format: AudioFormat,
    pending: Vec<u8>,
    seq: u32,
    reports: u32,
    /// When the latest report was seen; frames sent since then are `sent`.
    reported_at: Instant,
    sent: u32,
    sent_end: i64,
    /// What the device was playing at its latest report, once. None while it
    /// has run dry: video must not wait for audio that has not come yet.
    playing: Option<i64>,
    primed: bool,
}

impl Sink {
    fn new(out: AudioOut, format: AudioFormat) -> Self {
        let pending = protocol::message(
            protocol::AUDIO_FORMAT,
            1,
            0,
            &protocol::audio_format(format.rate, format.channels, format.bits),
        );
        let reports = out.level.reports.load(Ordering::Acquire);
        Sink {
            out,
            format,
            pending,
            seq: 1,
            reports,
            reported_at: Instant::now(),
            sent: 0,
            sent_end: 0,
            playing: None,
            primed: false,
        }
    }

    /// Drops the chunk when the device has not drained the last ones within
    /// TIMEOUT, so the input keeps its pace. Between reports the device is
    /// taken to drain at its rate, so sparse reports do not starve it.
    fn send(&mut self, chunk: &[u8], pts: i64, frames: u32) -> Result<(), String> {
        if self.out.is_closed() {
            return Err("audio: connection closed".into());
        }
        let target = frames * TARGET_CHUNKS;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let level = &self.out.level;
            let reports = level.reports.load(Ordering::Acquire);
            let buffered = level.buffered.load(Ordering::Relaxed);
            if reports != self.reports {
                self.reports = reports;
                self.reported_at = Instant::now();
                self.sent = 0;
                let buffered_us = buffered as i64 * 1_000_000 / self.format.rate as i64;
                self.playing = (buffered > 0).then_some(self.sent_end - buffered_us);
            }
            let drained = (self.reported_at.elapsed().as_micros() * self.format.rate as u128
                / 1_000_000) as u32;
            if (buffered + self.sent).saturating_sub(drained) < target {
                break;
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            thread::sleep(POLL);
        }
        self.seq += 1;
        self.pending.extend_from_slice(&protocol::message(
            protocol::AUDIO_DATA,
            self.seq,
            0,
            chunk,
        ));
        self.flush()?;
        self.out.pipe.submit(std::mem::take(&mut self.pending));
        self.sent += frames;
        self.sent_end = pts + frames as i64 * 1_000_000 / self.format.rate as i64;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        while self.out.pipe.pending() > 0 {
            self.out
                .pipe
                .wait(TIMEOUT)
                .map_err(|e| format!("audio: {e}"))?;
        }
        Ok(())
    }
}

/// Sends `packets` in fixed chunks to the latest connection's endpoint from
/// `outs`, and keeps `clock` on what the device plays. Without a device the
/// chunks are dropped at the clock's pace.
///
/// Sending to a connection starts only once TARGET_CHUNKS are at hand: an
/// input that comes in real time would otherwise keep the device's buffer at
/// the edge of running dry.
pub fn stream(
    packets: Receiver<Frame>,
    format: AudioFormat,
    outs: Receiver<AudioOut>,
    clock: Arc<Clock>,
) {
    let frames = format.rate / CHUNKS_PER_SECOND;
    let chunk_bytes = frames as usize * format.frame_bytes();
    let chunk_us = frames as i64 * 1_000_000 / format.rate as i64;
    let mut pending = Vec::new();
    let mut pending_pts = 0;
    let mut sink: Option<Sink> = None;
    let mut queue = VecDeque::new();
    for packet in packets {
        if pending.is_empty() {
            pending_pts = packet.pts;
        }
        pending.extend_from_slice(&packet.data);
        while pending.len() >= chunk_bytes {
            let chunk: Vec<u8> = pending.drain(..chunk_bytes).collect();
            let pts = pending_pts;
            pending_pts += chunk_us;
            while let Ok(out) = outs.try_recv() {
                sink = Some(Sink::new(out, format));
            }
            match &mut sink {
                Some(active) => {
                    queue.push_back((pts, chunk));
                    if !active.primed && queue.len() < TARGET_CHUNKS as usize {
                        continue;
                    }
                    active.primed = true;
                    while let Some((pts, chunk)) = queue.pop_front() {
                        if let Err(e) = active.send(&chunk, pts, frames) {
                            eprintln!("{e}");
                            sink = None;
                            queue.clear();
                            break;
                        }
                        if let Some(playing) = active.playing.take() {
                            clock.set(playing);
                        }
                    }
                }
                None => {
                    if let Some(ahead) = clock.now().map(|now| pts - now).filter(|&a| a > 0) {
                        thread::sleep(Duration::from_micros(ahead.min(1_000_000) as u64));
                    }
                }
            }
        }
    }
    if let Some(active) = &mut sink
        && let Err(e) = active.flush()
    {
        eprintln!("{e}");
    }
}
