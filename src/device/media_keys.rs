// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! AVRCP media-button capture over evdev.
//!
//! A Bluetooth headset surfaces media buttons as EV_KEY events on a dynamic
//! `/dev/input/eventN` device. This monitor opens every event device the app
//! does not already own (touch, power), polls them, and signals the main loop
//! via [`MediaSignals`].
//!
//! Play/pause codes fire on key-up. Next/prev fire immediately on key-down.
//! Volume+/- hold (>= 500 ms) fires next/prev. There is no multi-press gesture:
//! headsets with real transport buttons resolve multi-taps in firmware and send
//! one semantic code. Bookmark lives on the touch UI.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::info;

use crate::device::input::{decode_input_event, EV_KEY};
use crate::device::paths;

pub const KEY_VOLUMEDOWN: u16 = 114;
pub const KEY_VOLUMEUP: u16 = 115;
pub const KEY_NEXTSONG: u16 = 163;
pub const KEY_PLAYPAUSE: u16 = 164;
pub const KEY_PREVIOUSSONG: u16 = 165;
pub const KEY_REWIND: u16 = 168;
pub const KEY_PLAYCD: u16 = 200;
pub const KEY_PAUSECD: u16 = 201;
pub const KEY_PLAY: u16 = 207;
pub const KEY_FASTFORWARD: u16 = 208;

const PLAY_PAUSE_CODES: &[u16] = &[KEY_PLAYPAUSE, KEY_PLAYCD, KEY_PAUSECD, KEY_PLAY];
const NEXT_CODES: &[u16] = &[KEY_NEXTSONG, KEY_FASTFORWARD];
const PREV_CODES: &[u16] = &[KEY_PREVIOUSSONG, KEY_REWIND];

const RESCAN_INTERVAL_MS: u128 = 2000;
const POLL_TIMEOUT_MS: libc::c_int = 100;
const IDLE_SLEEP_MS: u64 = 500;
const VOL_HOLD_MS: u128 = 500;
const MAX_ERROR_DROPS: u32 = 3;

fn is_play_pause(code: u16) -> bool {
    PLAY_PAUSE_CODES.contains(&code)
}
fn is_next(code: u16) -> bool {
    NEXT_CODES.contains(&code)
}
fn is_prev(code: u16) -> bool {
    PREV_CODES.contains(&code)
}
fn is_volume_up(code: u16) -> bool {
    code == KEY_VOLUMEUP
}
fn is_volume_down(code: u16) -> bool {
    code == KEY_VOLUMEDOWN
}

/// One-shot signal flags shared between the monitor thread and the main loop.
/// Each flag is swapped to false by the consumer.
#[derive(Clone, Default)]
pub struct MediaSignals {
    pub play: Arc<AtomicBool>,
    pub next: Arc<AtomicBool>,
    pub prev: Arc<AtomicBool>,
}

impl MediaSignals {
    pub fn new() -> Self {
        Self::default()
    }
}

struct MediaDevice {
    path: String,
    file: std::fs::File,
}

#[derive(Default)]
struct PressState {
    pp_down: bool,
    vol_down_at: Option<Instant>,
    vol_code: Option<u16>,
    vol_fired: bool,
}

impl PressState {
    fn tick(&mut self, sig: &MediaSignals) {
        if let Some(t) = self.vol_down_at {
            if !self.vol_fired && t.elapsed().as_millis() >= VOL_HOLD_MS {
                if let Some(code) = self.vol_code {
                    if is_volume_up(code) {
                        sig.next.store(true, Ordering::SeqCst);
                    } else {
                        sig.prev.store(true, Ordering::SeqCst);
                    }
                    self.vol_fired = true;
                    info!(
                        "media-keys: volume {} hold",
                        if is_volume_up(code) { "+" } else { "-" }
                    );
                }
            }
        }
    }

    fn on_pp_down(&mut self) {
        self.pp_down = true;
    }

    fn on_pp_up(&mut self, sig: &MediaSignals) {
        if self.pp_down {
            self.pp_down = false;
            sig.play.store(true, Ordering::SeqCst);
            info!("media-keys: play/pause");
        }
    }

    fn on_vol_down(&mut self, code: u16) {
        if self.vol_down_at.is_none() {
            self.vol_down_at = Some(Instant::now());
            self.vol_code = Some(code);
            self.vol_fired = false;
        }
    }

    fn on_vol_up(&mut self) {
        self.vol_down_at = None;
        self.vol_code = None;
    }
}

struct Monitor {
    open: Vec<MediaDevice>,
    ps: PressState,
    sig: MediaSignals,
    exit: Arc<AtomicBool>,
    skip_devs: Vec<String>,
    since_scan: Instant,
    error_counts: HashMap<String, u32>,
}

impl Monitor {
    fn run(&mut self) {
        info!("media-keys: monitor started");
        loop {
            if self.exit.load(Ordering::SeqCst) {
                break;
            }
            self.ps.tick(&self.sig);
            self.maybe_rescan();
            if self.open.is_empty() {
                std::thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }
            if !self.poll_devices() {
                break;
            }
        }
        info!("media-keys: monitor exiting");
    }

    fn maybe_rescan(&mut self) {
        if self.since_scan.elapsed().as_millis() >= RESCAN_INTERVAL_MS {
            refresh_devices(&mut self.open, &self.skip_devs, &mut self.error_counts);
            self.since_scan = Instant::now();
        }
    }

    fn poll_devices(&mut self) -> bool {
        let mut pfds: Vec<libc::pollfd> = self
            .open
            .iter()
            .map(|d| libc::pollfd {
                fd: d.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: pfds is a local Vec of initialized pollfd backed by self open fds; poll only writes revents; nfds matches pfds.len().
        let n = unsafe {
            libc::poll(
                pfds.as_mut_ptr(),
                pfds.len() as libc::nfds_t,
                POLL_TIMEOUT_MS,
            )
        };
        if n <= 0 {
            return !self.exit.load(Ordering::SeqCst);
        }
        self.dispatch_ready(&pfds);
        true
    }

    fn dispatch_ready(&mut self, pfds: &[libc::pollfd]) {
        for i in (0..self.open.len()).rev() {
            let rv = pfds[i].revents;
            if rv & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                let path = self.open.remove(i).path;
                let count = self.error_counts.entry(path.clone()).or_insert(0);
                *count += 1;
                if *count >= MAX_ERROR_DROPS {
                    self.skip_devs.push(path.clone());
                    info!(
                        "media-keys: blacklisted {} after {} error drops",
                        path, count
                    );
                } else {
                    info!(
                        "media-keys: dropped {} (revents={}, drop {}/{MAX_ERROR_DROPS})",
                        path, rv, count
                    );
                }
                continue;
            }
            if rv & libc::POLLIN != 0 {
                self.drain_device(i);
            }
        }
    }

    fn drain_device(&mut self, i: usize) {
        let mut buf = [0u8; 16];
        let mut got_data = false;
        loop {
            match self.open[i].file.read(&mut buf) {
                Ok(n) if n >= 16 => {
                    got_data = true;
                    let (typ, code, val) = decode_input_event(&buf);
                    if typ == EV_KEY {
                        self.handle_key(code, val);
                    }
                }
                _ => break,
            }
        }
        if got_data {
            self.error_counts.remove(&self.open[i].path);
        }
    }

    fn handle_key(&mut self, code: u16, val: i32) {
        info!(
            "media-keys: code={} val={} (play={} next={} prev={} vol={})",
            code,
            val,
            is_play_pause(code),
            is_next(code),
            is_prev(code),
            is_volume_up(code) || is_volume_down(code),
        );
        if is_volume_up(code) || is_volume_down(code) {
            if val == 1 {
                self.ps.on_vol_down(code);
            } else if val == 0 {
                self.ps.on_vol_up();
            }
            return;
        }
        if val == 1 && (is_next(code) || is_prev(code)) {
            if is_next(code) {
                self.sig.next.store(true, Ordering::SeqCst);
            } else {
                self.sig.prev.store(true, Ordering::SeqCst);
            }
            return;
        }
        if is_play_pause(code) {
            if val == 1 {
                self.ps.on_pp_down();
            } else if val == 0 {
                self.ps.on_pp_up(&self.sig);
            }
        }
    }
}

/// Spawn the media-key monitor thread. `skip_devs` lists device paths already
/// owned by the app (touch, power) that must not be reopened.
pub fn spawn_media_key_monitor(
    signals: MediaSignals,
    exit: Arc<AtomicBool>,
    skip_devs: Vec<String>,
) {
    std::thread::Builder::new()
        .name("kobo-media".into())
        .spawn(move || {
            let mut mon = Monitor {
                open: Vec::new(),
                ps: PressState::default(),
                sig: signals,
                exit,
                skip_devs,
                since_scan: Instant::now(),
                error_counts: HashMap::new(),
            };
            mon.run();
        })
        .ok();
}

/// Remove vanished devices, open new event nodes not in the skip list. Error
/// counts for paths that no longer exist are cleared so a future device reusing
/// the same eventN path is not permanently blacklisted.
fn refresh_devices(
    open: &mut Vec<MediaDevice>,
    skip_devs: &[String],
    error_counts: &mut HashMap<String, u32>,
) {
    open.retain(|d| std::path::Path::new(&d.path).exists());
    error_counts.retain(|k, _| std::path::Path::new(k).exists());

    let mut current: HashSet<String> = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(paths::INPUT_DEV_DIR) {
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("event") {
                continue;
            }
            if let Ok(path) = entry.path().into_os_string().into_string() {
                current.insert(path);
            }
        }
    }

    let already: HashSet<String> = open.iter().map(|d| d.path.clone()).collect();
    for path in &current {
        if already.contains(path) || skip_devs.iter().any(|s| s == path) {
            continue;
        }
        if let Ok(f) = std::fs::OpenOptions::new().read(true).open(path) {
            let fd = f.as_raw_fd();
            // SAFETY: fd is the raw fd of the just-opened owned File f; F_GETFL/F_SETFL read/write an int flags word on this descriptor only.
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            info!("media-keys: opened {}", path);
            open.push(MediaDevice {
                path: path.clone(),
                file: f,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_pause_code_set_covers_dialects() {
        assert!(is_play_pause(KEY_PLAYPAUSE));
        assert!(is_play_pause(KEY_PLAYCD));
        assert!(is_play_pause(KEY_PAUSECD));
        assert!(is_play_pause(KEY_PLAY));
        assert!(!is_play_pause(KEY_NEXTSONG));
    }

    #[test]
    fn press_fires_play_on_release_with_no_delay() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        assert!(!sig.play.load(Ordering::SeqCst));
        ps.on_pp_up(&sig);
        assert!(sig.play.load(Ordering::SeqCst));
    }

    #[test]
    fn two_presses_are_two_toggles() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        ps.on_pp_up(&sig);
        assert!(sig.play.swap(false, Ordering::SeqCst));
        ps.on_pp_down();
        ps.on_pp_up(&sig);
        assert!(sig.play.load(Ordering::SeqCst));
    }

    #[test]
    fn release_without_press_does_nothing() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_up(&sig);
        assert!(!sig.play.load(Ordering::SeqCst));
    }

    #[test]
    fn volume_long_press_fires_next() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_vol_down(KEY_VOLUMEUP);
        ps.vol_down_at = Some(Instant::now() - Duration::from_millis(600));
        ps.tick(&sig);
        assert!(sig.next.load(Ordering::SeqCst));
        assert!(!sig.prev.load(Ordering::SeqCst));
    }

    #[test]
    fn volume_long_press_fires_prev() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_vol_down(KEY_VOLUMEDOWN);
        ps.vol_down_at = Some(Instant::now() - Duration::from_millis(600));
        ps.tick(&sig);
        assert!(sig.prev.load(Ordering::SeqCst));
    }

    #[test]
    fn volume_short_press_does_not_fire() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_vol_down(KEY_VOLUMEDOWN);
        ps.tick(&sig);
        assert!(!sig.prev.load(Ordering::SeqCst));
        ps.on_vol_up();
        ps.tick(&sig);
        assert!(!sig.prev.load(Ordering::SeqCst));
    }

    #[test]
    fn autorepeat_yields_a_single_toggle() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        ps.on_pp_down();
        ps.on_pp_down();
        assert!(!sig.play.load(Ordering::SeqCst));
        ps.on_pp_up(&sig);
        assert!(sig.play.swap(false, Ordering::SeqCst));
        ps.on_pp_up(&sig);
        assert!(!sig.play.load(Ordering::SeqCst));
    }

    #[test]
    fn decode_input_event_extracts_type_code_value() {
        let mut buf = [0u8; 16];
        buf[8] = EV_KEY as u8;
        buf[10] = 200;
        buf[11] = 0;
        buf[12] = 1;
        let (typ, code, val) = decode_input_event(&buf);
        assert_eq!(typ, EV_KEY);
        assert_eq!(code, 200);
        assert_eq!(val, 1);
    }
}
