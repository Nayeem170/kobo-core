//! `SharedClock` - the single playback time source (decision 2).
//!
//! Both the Player (worker thread) and the UI (main thread) read the SAME
//! clock, so highlight and audio physically can't drift. The Player writes
//! position; the UI's ~15 Hz timer reads [`SharedClock::position_s`] and
//! highlights the `WordMark` whose `time_s` <= position.
//!
//! Cross-thread via a relaxed `AtomicU64` (nanoseconds) - cheap and
//! single-producer (Player) / single-consumer (UI timer).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Shared playback position in nanoseconds (0 = start of playback).
#[derive(Clone, Debug)]
pub struct SharedClock(Arc<AtomicU64>);

impl Default for SharedClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedClock {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
    /// Player (producer) sets the current position.
    pub fn set_ns(&self, ns: u64) {
        self.0.store(ns, Ordering::Relaxed);
    }
    /// UI timer (consumer) reads the current position in seconds.
    pub fn position_s(&self) -> f64 {
        self.0.load(Ordering::Relaxed) as f64 / 1e9
    }
}

/// Pick the index of the last `WordMark` whose `time_s` is <= `position_s`
/// (the currently-highlighted word). Returns None before the first word.
pub fn highlight_index(word_times: &[f64], position_s: f64) -> Option<usize> {
    // word_times assumed ascending; find the last index with time_s <= position.
    let mut found = None;
    for (i, &t) in word_times.iter().enumerate() {
        if t <= position_s {
            found = Some(i);
        } else {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_advances_with_clock() {
        let times = [0.0, 0.5, 1.0, 1.5, 2.0]; // word start times
        assert_eq!(highlight_index(&times, 0.0), Some(0));
        assert_eq!(highlight_index(&times, 0.4), Some(0));
        assert_eq!(highlight_index(&times, 0.5), Some(1));
        assert_eq!(highlight_index(&times, 1.24), Some(2));
        assert_eq!(highlight_index(&times, 2.0), Some(4));
        assert_eq!(highlight_index(&times, 5.0), Some(4)); // past end -> last
    }

    #[test]
    fn clock_is_shared_across_clone() {
        let player_clock = SharedClock::new();
        let ui_clock = player_clock.clone(); // shared (Arc)
        assert_eq!(ui_clock.position_s(), 0.0);
        player_clock.set_ns(1_500_000_000); // 1.5s
        assert_eq!(ui_clock.position_s(), 1.5); // UI sees the Player's write
    }
}
