// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! AVRCP media-button capture over evdev.
//!
//! A Bluetooth headset surfaces media buttons as EV_KEY events on a dynamic
//! `/dev/input/eventN` device. This monitor opens every event device the app
//! does not already own (touch, power), polls them, and signals the main loop
//! via [`MediaSignals`].
//!
//! Two headset families need different decoding, but they cannot be told apart
//! by capability bits: many single-button earbuds advertise the full AVRCP key
//! set in their EVIOCGBIT bitmap yet only ever emit a play/pause code. So both
//! decode paths run simultaneously on every device:
//!
//! - Distinct next/prev codes fire immediately (multi-key firmware headsets).
//! - Play/pause codes go through a press counter: 1 = play/pause, 2+ = bookmark.
//! - Volume+/- hold (>= 500 ms) fires next/prev.
use std::collections::HashSet;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, info};

use crate::device::input::{decode_input_event, EV_KEY};

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
const MULTI_PRESS_WINDOW_MS: u128 = 450;
const VOL_HOLD_MS: u128 = 500;

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
#[derive(Clone)]
pub struct MediaSignals {
    pub play: Arc<AtomicBool>,
    pub next: Arc<AtomicBool>,
    pub prev: Arc<AtomicBool>,
    pub bookmark: Arc<AtomicBool>,
}

impl MediaSignals {
    pub fn new() -> Self {
        Self {
            play: Arc::new(AtomicBool::new(false)),
            next: Arc::new(AtomicBool::new(false)),
            prev: Arc::new(AtomicBool::new(false)),
            bookmark: Arc::new(AtomicBool::new(false)),
        }
    }
}

struct MediaDevice {
    path: String,
    file: std::fs::File,
}

/// Play/pause press-count state and volume-key long-press tracking. One set
/// covers all devices.
#[derive(Default)]
struct PressState {
    count: u32,
    last_press: Option<Instant>,
    pp_down: bool,
    vol_down_at: Option<Instant>,
    vol_code: Option<u16>,
    vol_fired: bool,
}

impl PressState {
    fn tick(&mut self, sig: &MediaSignals) {
        self.tick_volume(&sig);
        self.tick_play_pause(&sig);
    }

    fn tick_volume(&mut self, sig: &MediaSignals) {
        if let Some(t) = self.vol_down_at {
            if !self.vol_fired && t.elapsed().as_millis() >= VOL_HOLD_MS {
                if let Some(code) = self.vol_code {
                    if is_volume_up(code) {
                        sig.next.store(true, Ordering::SeqCst);
                    } else {
                        sig.prev.store(true, Ordering::SeqCst);
                    }
                    self.vol_fired = true;
                    self.reset_count();
                    info!(
                        "media-keys: volume {} hold",
                        if is_volume_up(code) { "+" } else { "-" }
                    );
                }
            }
        }
    }

    fn tick_play_pause(&mut self, sig: &MediaSignals) {
        if self.count == 0 {
            return;
        }
        if let Some(t) = self.last_press {
            if t.elapsed().as_millis() >= MULTI_PRESS_WINDOW_MS {
                if self.count >= 2 {
                    sig.bookmark.store(true, Ordering::SeqCst);
                    info!("media-keys: {} presses -> bookmark", self.count);
                } else {
                    sig.play.store(true, Ordering::SeqCst);
                    info!("media-keys: play/pause");
                }
                self.reset_count();
            }
        }
    }

    fn on_pp_down(&mut self) {
        self.pp_down = true;
    }

    fn on_pp_up(&mut self) {
        if self.pp_down {
            self.pp_down = false;
            self.count += 1;
            self.last_press = Some(Instant::now());
            info!("media-keys: press count={}", self.count);
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

    fn reset_count(&mut self) {
        self.count = 0;
        self.last_press = None;
    }
}

struct Monitor {
    open: Vec<MediaDevice>,
    ps: PressState,
    sig: MediaSignals,
    exit: Arc<AtomicBool>,
    skip_devs: Vec<String>,
    since_scan: Instant,
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
            refresh_devices(&mut self.open, &self.skip_devs);
            self.since_scan = Instant::now();
        }
    }

    /// Returns false if exit was signalled during poll.
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
        // SAFETY: pfds is a stack Vec of initialized pollfd; each fd is a valid
        // owned descriptor alive on this frame. POLLIN only; bounded timeout.
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
                let path = self.open[i].path.clone();
                self.open.remove(i);
                info!("media-keys: dropped {} (revents={})", path, rv);
                continue;
            }
            if rv & libc::POLLIN != 0 {
                self.drain_device(i);
            }
        }
    }

    fn drain_device(&mut self, i: usize) {
        let mut buf = [0u8; 16];
        loop {
            match self.open[i].file.read(&mut buf) {
                Ok(n) if n >= 16 => {
                    let (typ, code, val) = decode_input_event(&buf);
                    if typ == EV_KEY {
                        self.handle_key(code, val);
                    }
                }
                _ => break,
            }
        }
    }

    fn handle_key(&mut self, code: u16, val: i32) {
        debug!("media-keys: code={} val={}", code, val);
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
            self.ps.reset_count();
            return;
        }
        if is_play_pause(code) {
            if val == 1 {
                self.ps.on_pp_down();
            } else if val == 0 {
                self.ps.on_pp_up();
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
            };
            mon.run();
        })
        // best-effort: thread spawn failure is non-fatal; the app still runs
        .ok();
}

/// Synchronise the open-device set with `/dev/input`. Removes vanished devices,
/// opens new `event*` nodes not in the skip list.
fn refresh_devices(open: &mut Vec<MediaDevice>, skip_devs: &[String]) {
    open.retain(|d| std::path::Path::new(&d.path).exists());

    let mut current: HashSet<String> = HashSet::new();
    if let Ok(entries) = std::fs::read_dir("/dev/input") {
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
        match std::fs::OpenOptions::new().read(true).open(path) {
            Ok(f) => {
                let fd = f.as_raw_fd();
                // SAFETY: fd is a valid, owned descriptor; F_GETFL/F_SETFL only
                // touch this fd's flags.
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
            Err(e) => {
                debug!("media-keys: open {} failed: {}", path, e);
            }
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
    fn single_press_resolves_to_play() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        ps.on_pp_up();
        ps.tick(&sig);
        assert!(!sig.play.load(Ordering::SeqCst));
        ps.last_press = Some(Instant::now() - Duration::from_millis(400));
        ps.tick(&sig);
        assert!(sig.play.load(Ordering::SeqCst));
    }

    #[test]
    fn double_press_resolves_to_bookmark() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        ps.on_pp_up();
        ps.on_pp_down();
        ps.on_pp_up();
        ps.last_press = Some(Instant::now() - Duration::from_millis(400));
        ps.tick(&sig);
        assert!(sig.bookmark.load(Ordering::SeqCst));
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
    fn autorepeat_does_not_inflate_press_count() {
        let sig = MediaSignals::new();
        let mut ps = PressState::default();
        ps.on_pp_down();
        ps.on_pp_down();
        ps.on_pp_up();
        assert_eq!(ps.count, 1);
        ps.last_press = Some(Instant::now() - Duration::from_millis(400));
        ps.tick(&sig);
        assert!(sig.play.load(Ordering::SeqCst));
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
