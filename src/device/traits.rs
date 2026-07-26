// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Device trait surface for testability.
//!
//! Each device subsystem gets a narrow trait so that consumer apps can inject
//! Mock impls for desktop unit tests instead of hitting real sysfs/ioctl.
//!
//! - [`Frontlight`] - brightness get/set/restore
//! - [`Battery`] - capacity percentage
//! - [`Wifi`] - link status, SSID, toggle
//! - [`Bluetooth`] - connection status, name, toggle
//! - [`SystemControl`] - suspend, wall-clock
//! - [`Framebuffer`] - resolution, present
//!
//! Sys impls (production wrappers around the free functions) live in
//! [`crate::device::impls`].

use std::cell::RefCell;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Frontlight
// ---------------------------------------------------------------------------

pub trait Frontlight {
    fn get(&self) -> u32;
    fn set(&self, brightness: u32);
    fn restore(&self, brightness: u32);
}

/// Scripted frontlight mock: records every `set` call and returns scripted
/// `get` values (repeating the last once exhausted). Use for deterministic
/// retry-loop testing.
pub struct MockFrontlight {
    pub sets: RefCell<Vec<u32>>,
    pub scripted_gets: Vec<u32>,
    get_idx: std::cell::Cell<usize>,
}

impl MockFrontlight {
    pub fn new(gets: Vec<u32>) -> Self {
        MockFrontlight {
            sets: RefCell::new(Vec::new()),
            scripted_gets: gets,
            get_idx: std::cell::Cell::new(0),
        }
    }

    pub fn last_set(&self) -> Option<u32> {
        self.sets.borrow().last().copied()
    }

    pub fn set_count(&self) -> usize {
        self.sets.borrow().len()
    }
}

impl Frontlight for MockFrontlight {
    fn get(&self) -> u32 {
        let i = self.get_idx.get();
        let v = self
            .scripted_gets
            .get(i)
            .copied()
            .unwrap_or_else(|| self.scripted_gets.last().copied().unwrap_or(0));
        self.get_idx.set(i + 1);
        v
    }

    fn set(&self, brightness: u32) {
        self.sets.borrow_mut().push(brightness);
    }

    fn restore(&self, brightness: u32) {
        self.set(brightness);
    }
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

pub trait Battery {
    fn pct(&self) -> i32;
    fn is_charging(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockBattery {
    pub pct: i32,
    pub charging: bool,
}

impl Battery for MockBattery {
    fn pct(&self) -> i32 {
        self.pct
    }
    fn is_charging(&self) -> bool {
        self.charging
    }
}

// ---------------------------------------------------------------------------
// Wifi
// ---------------------------------------------------------------------------

pub trait Wifi {
    fn connected(&self) -> bool;
    fn ssid(&self) -> Option<String>;
    fn toggle(&self, on: bool);
}

#[derive(Debug, Clone, Default)]
pub struct MockWifi {
    pub connected: bool,
    pub ssid: Option<String>,
}

impl Wifi for MockWifi {
    fn connected(&self) -> bool {
        self.connected
    }
    fn ssid(&self) -> Option<String> {
        self.ssid.clone()
    }
    fn toggle(&self, _on: bool) {}
}

// ---------------------------------------------------------------------------
// Bluetooth
// ---------------------------------------------------------------------------

pub trait Bluetooth {
    fn connected(&self) -> bool;
    fn name(&self) -> Option<String>;
    fn toggle(&self, on: bool);
}

#[derive(Debug, Clone, Default)]
pub struct MockBluetooth {
    pub connected: bool,
    pub name: Option<String>,
}

impl Bluetooth for MockBluetooth {
    fn connected(&self) -> bool {
        self.connected
    }
    fn name(&self) -> Option<String> {
        self.name.clone()
    }
    fn toggle(&self, _on: bool) {}
}

// ---------------------------------------------------------------------------
// SystemControl
// ---------------------------------------------------------------------------

pub trait SystemControl {
    fn suspend(&self, state: &str);
    fn clock(&self) -> String;
}

#[derive(Debug, Clone, Default)]
pub struct MockSystemControl {
    pub clock: String,
}

impl SystemControl for MockSystemControl {
    fn suspend(&self, _state: &str) {}
    fn clock(&self) -> String {
        if self.clock.is_empty() {
            "--:--".to_string()
        } else {
            self.clock.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

pub trait Framebuffer {
    fn resolution(&self) -> (usize, usize);
    fn present(
        &self,
        buf: &[u8],
        w: usize,
        h: usize,
        full: bool,
        top: usize,
        rh: usize,
        waveform: u32,
    );
}

pub struct MockFramebuffer {
    pub width: usize,
    pub height: usize,
    pub present_count: std::cell::Cell<usize>,
}

impl MockFramebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        MockFramebuffer {
            width,
            height,
            present_count: std::cell::Cell::new(0),
        }
    }
}

impl Framebuffer for MockFramebuffer {
    fn resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn present(
        &self,
        _buf: &[u8],
        _w: usize,
        _h: usize,
        _full: bool,
        _top: usize,
        _rh: usize,
        _waveform: u32,
    ) {
        self.present_count.set(self.present_count.get() + 1);
    }
}

// ---------------------------------------------------------------------------
// Re-export the brightness path type for Sys impl constructors
// ---------------------------------------------------------------------------

/// Frontlight sysfs paths resolved from a [`crate::device::config::FrontlightConfig`].
pub struct FrontlightPaths {
    pub brightness: PathBuf,
    pub mixer: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_battery_round_trip() {
        let b = MockBattery {
            pct: 73,
            charging: true,
        };
        assert_eq!(b.pct(), 73);
        assert!(b.is_charging());
    }

    #[test]
    fn mock_frontlight_scripts_gets() {
        let fl = MockFrontlight::new(vec![0, 0, 60]);
        assert_eq!(fl.get(), 0);
        assert_eq!(fl.get(), 0);
        fl.set(60);
        assert_eq!(fl.get(), 60);
        assert_eq!(fl.set_count(), 1);
        assert_eq!(fl.last_set(), Some(60));
    }

    #[test]
    fn mock_frontlight_repeats_last_get() {
        let fl = MockFrontlight::new(vec![42]);
        assert_eq!(fl.get(), 42);
        assert_eq!(fl.get(), 42);
    }

    #[test]
    fn mock_wifi_and_bt() {
        let w = MockWifi {
            connected: true,
            ssid: Some("HomeNet".into()),
        };
        assert!(w.connected());
        assert_eq!(w.ssid().as_deref(), Some("HomeNet"));

        let bt = MockBluetooth {
            connected: false,
            name: None,
        };
        assert!(!bt.connected());
        assert!(bt.name().is_none());
    }

    #[test]
    fn mock_system_clock() {
        let sc = MockSystemControl {
            clock: "9:41 PM".into(),
        };
        assert_eq!(sc.clock(), "9:41 PM");
        assert_eq!(MockSystemControl::default().clock(), "--:--");
    }

    #[test]
    fn mock_framebuffer_counts_presents() {
        let fb = MockFramebuffer::new(100, 200);
        assert_eq!(fb.resolution(), (100, 200));
        fb.present(&[], 100, 200, true, 0, 200, 2);
        fb.present(&[], 100, 200, false, 0, 100, 2);
        assert_eq!(fb.present_count.get(), 2);
    }
}
