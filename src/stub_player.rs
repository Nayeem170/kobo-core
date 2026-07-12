//! `StubPlayer` - a headless stub proving the control + shared-clock + highlight
//! loop (decision 1 multi-thread + decision 2 single clock) WITHOUT real audio
//! or a Slint window. The desktop sim drives it with fake `WordMark`s; the real
//! Player (`kobo-audio`) and the real Slint sim reuse the same [`SharedClock`] +
//! control pattern once a build env with cc/display is available.
//!
//! Proves: a Player on a worker task advances ONE shared clock; the UI's timer
//! reads that clock and highlights the word whose `time_s` <= position; Play /
//! Pause / Stop control it. Highlight and audio read one clock -> can't drift.

use crate::clock::SharedClock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

pub struct StubPlayer {
    clock: SharedClock,
    playing: Arc<AtomicBool>,
    /// Accumulated playback position in ns (frozen on pause, reset on stop).
    base_ns: Arc<AtomicU64>,
    /// Speed x 1000 (1.0 = 1000). The ticker scales elapsed time by this so
    /// higher speed advances the clock (and thus highlight) faster.
    speed: Arc<AtomicU32>,
    task: Option<JoinHandle<()>>,
}

impl StubPlayer {
    pub fn new() -> Self {
        StubPlayer {
            clock: SharedClock::new(),
            playing: Arc::new(AtomicBool::new(false)),
            base_ns: Arc::new(AtomicU64::new(0)),
            speed: Arc::new(AtomicU32::new(1000)),
            task: None,
        }
    }

    /// The single shared clock the UI timer should read.
    pub fn clock(&self) -> SharedClock {
        self.clock.clone()
    }

    /// Spawn the clock-ticker task on the current tokio runtime (call once,
    /// from within a runtime context - e.g. under `rt.enter()`). After this,
    /// [`play`]/[`pause`]/[`stop`] are plain atomic setters safe to call from
    /// any thread (including the Slint UI main thread).
    pub fn spawn_ticker(&mut self) {
        if self.task.is_some() {
            return;
        }
        let clock = self.clock.clone();
        let playing = self.playing.clone();
        let base_ns = self.base_ns.clone();
        let speed = self.speed.clone();
        self.task = Some(tokio::spawn(async move {
            let mut last = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let now = Instant::now();
                let spd = speed.load(Ordering::Relaxed) as f64 / 1000.0;
                let delta_ns = ((now.duration_since(last).as_nanos() as f64) * spd) as u64;
                last = now;
                if playing.load(Ordering::Relaxed) {
                    let pos = base_ns.fetch_add(delta_ns, Ordering::Relaxed) + delta_ns;
                    clock.set_ns(pos);
                }
            }
        }));
    }

    /// Begin/resume playback (atomic; ticker must already be spawned).
    pub fn play(&mut self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    /// Freeze the clock (position held).
    pub fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    /// Stop and reset to the start.
    pub fn stop(&mut self) {
        self.playing.store(false, Ordering::Relaxed);
        self.base_ns.store(0, Ordering::Relaxed);
        self.clock.set_ns(0);
    }

    /// Seek by `delta_s` seconds (clamped at 0). Real Player maps this to the
    /// audio sink / re-synthesizes; here it just jumps the clock.
    pub fn seek_by(&mut self, delta_s: f64) {
        let cur_ns = self.base_ns.load(Ordering::Relaxed) as f64;
        let new_ns = (cur_ns + delta_s * 1e9).max(0.0) as u64;
        self.base_ns.store(new_ns, Ordering::Relaxed);
        self.clock.set_ns(new_ns);
    }

    /// Seek to an absolute position (`target_s`). Used by the draggable progress bar.
    pub fn seek_to(&mut self, target_s: f64) {
        let new_ns = (target_s.max(0.0) * 1e9) as u64;
        self.base_ns.store(new_ns, Ordering::Relaxed);
        self.clock.set_ns(new_ns);
    }

    /// Set playback speed (e.g. 0.5 ... 2.0). The ticker scales elapsed time.
    pub fn set_speed(&self, speed: f64) {
        self.speed
            .store((speed * 1000.0).round() as u32, Ordering::Relaxed);
    }

    /// Current playback speed.
    pub fn speed(&self) -> f64 {
        self.speed.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Current playback position in seconds (same clock the UI reads).
    pub fn position_s(&self) -> f64 {
        self.clock.position_s()
    }

    /// Whether the clock is currently advancing.
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

impl Default for StubPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight_index;

    /// The shared-clock + control + highlight loop, headless (no audio/window).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn control_clock_highlight_loop() {
        let mut p = StubPlayer::new();
        let words = [0.0_f64, 0.3, 0.6, 0.9, 1.2]; // fake WordMark time_s

        p.spawn_ticker();
        p.play();

        // Simulate the UI's ~15 Hz timer: read the SAME clock, pick the word.
        let mut max_highlight = 0usize;
        for _ in 0..25 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let pos = p.position_s();
            if let Some(i) = highlight_index(&words, pos) {
                max_highlight = max_highlight.max(i);
            }
        }
        // Clock advanced -> highlight progressed past the early words.
        assert!(
            max_highlight >= 2,
            "highlight should reach word 2+; got {max_highlight}"
        );

        // Pause freezes the clock (no drift while paused).
        p.pause();
        let frozen = p.position_s();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let drift = (p.position_s() - frozen).abs();
        assert!(drift < 0.02, "pause should freeze the clock; drift={drift}");

        // The UI clock (a clone) sees exactly what the Player wrote (single source).
        let ui_clock = p.clock();
        assert!((ui_clock.position_s() - p.position_s()).abs() < 1e-6);

        p.stop();
        assert!(p.position_s() < 1e-6, "stop should reset the clock");
    }
}
