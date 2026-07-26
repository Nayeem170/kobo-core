// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Production (Sys) implementations of the device traits.
//!
//! Each `Sys*` struct wraps the corresponding free functions in the device
//! modules. Construct once at startup and pass `&dyn Trait` to code that needs
//! testable device access.

use std::path::PathBuf;

use crate::device::battery;
use crate::device::bt::{self as bt_mod, set_bt_bus};
use crate::device::clock;
use crate::device::config::SocFamily;
use crate::device::fb::Fb;
use crate::device::power;
use crate::device::traits::{FrontlightPaths, *};
use crate::device::wifi;

// ---------------------------------------------------------------------------
// SysFrontlight
// ---------------------------------------------------------------------------

/// Production frontlight: reads/writes real sysfs brightness files.
pub struct SysFrontlight {
    brightness_path: PathBuf,
}

impl SysFrontlight {
    pub fn new(paths: &FrontlightPaths) -> Self {
        SysFrontlight {
            brightness_path: paths.brightness.clone(),
        }
    }

    pub fn from_path(path: PathBuf) -> Self {
        SysFrontlight {
            brightness_path: path,
        }
    }
}

impl Frontlight for SysFrontlight {
    fn get(&self) -> u32 {
        power::frontlight_get(&self.brightness_path).unwrap_or(0)
    }

    fn set(&self, brightness: u32) {
        power::frontlight_set(&self.brightness_path, brightness);
    }

    fn restore(&self, brightness: u32) {
        power::restore_frontlight(&self.brightness_path, brightness);
    }
}

// ---------------------------------------------------------------------------
// SysBattery
// ---------------------------------------------------------------------------

/// Production battery: reads `/sys/class/power_supply/*/capacity`.
pub struct SysBattery;

impl Battery for SysBattery {
    fn pct(&self) -> i32 {
        battery::battery_pct()
    }
}

// ---------------------------------------------------------------------------
// SysWifi
// ---------------------------------------------------------------------------

/// Production wifi: reads `/sys/class/net/wlan0/operstate`.
pub struct SysWifi;

impl Wifi for SysWifi {
    fn connected(&self) -> bool {
        wifi::wifi_status()
    }

    fn ssid(&self) -> Option<String> {
        wifi::wifi_name()
    }

    fn toggle(&self, on: bool) {
        wifi::wifi_toggle(on);
    }
}

// ---------------------------------------------------------------------------
// SysBluetooth
// ---------------------------------------------------------------------------

/// Production bluetooth: DBus calls to bluez / mtk.bluedroid.
pub struct SysBluetooth;

impl SysBluetooth {
    /// Initialize the DBus bus name for the device's SoC family.
    /// Call once at startup after device detection.
    pub fn init(soc: SocFamily) -> Self {
        set_bt_bus(soc);
        SysBluetooth
    }
}

impl Bluetooth for SysBluetooth {
    fn connected(&self) -> bool {
        bt_mod::bt_status()
    }

    fn name(&self) -> Option<String> {
        bt_mod::bt_name()
    }

    fn toggle(&self, on: bool) {
        bt_mod::bt_toggle(on);
    }
}

// ---------------------------------------------------------------------------
// SysSystemControl
// ---------------------------------------------------------------------------

/// Production system control: kernel suspend via `/sys/power/state`,
/// wall-clock via `date` command.
pub struct SysSystemControl;

impl SystemControl for SysSystemControl {
    fn suspend(&self, state: &str) {
        power::kernel_suspend(state);
    }

    fn clock(&self) -> String {
        clock::current_clock()
    }
}

// ---------------------------------------------------------------------------
// SysFramebuffer
// ---------------------------------------------------------------------------

/// Production framebuffer: wraps [`Fb`] (mmap'd `/dev/fb0` + MXCFB refresh).
pub struct SysFramebuffer<'a> {
    fb: &'a Fb,
}

impl<'a> SysFramebuffer<'a> {
    pub fn new(fb: &'a Fb) -> Self {
        SysFramebuffer { fb }
    }
}

impl<'a> Framebuffer for SysFramebuffer<'a> {
    fn resolution(&self) -> (usize, usize) {
        (self.fb.xres, self.fb.yres)
    }

    fn present(
        &self,
        buf: &[u8],
        w: usize,
        h: usize,
        full: bool,
        top: usize,
        rh: usize,
        waveform: u32,
    ) {
        self.fb.present(buf, w, h, full, top, rh, waveform);
    }
}

/// Resolve frontlight sysfs paths from a [`crate::device::config::FrontlightConfig`].
pub fn resolve_frontlight_paths(
    fl_cfg: &crate::device::config::FrontlightConfig,
) -> FrontlightPaths {
    FrontlightPaths {
        brightness: power::known_brightness_path(fl_cfg),
        mixer: power::frontlight_path(fl_cfg),
    }
}
