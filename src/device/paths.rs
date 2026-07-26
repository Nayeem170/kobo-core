// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Filesystem path constants for Kobo devices.
//!
//! Every device path lives here. Other modules reference these consts instead
//! of re-typing string literals (CODE_CONVENTIONS section 4).

pub const ADDS_DIR: &str = "/mnt/onboard/.adds";
pub const CONFIG_FILE: &str = "/mnt/onboard/.adds/config";
pub const CRASH_LOG: &str = "/mnt/onboard/.adds/crash.log";
pub const KLOG: &str = "/mnt/onboard/.adds/kothok.log";
pub const PPM_DEBUG: &str = "/tmp/kobo-reader.ppm";
pub const PPM_DEPLOY: &str = "/mnt/onboard/.adds/kobo-reader.ppm";
pub const TOUCH_DEV: &str = "/dev/input/event1";
pub const POWER_DEV: &str = "/dev/input/event2";
pub const BT_CONFIG_FILE: &str = "/mnt/onboard/.kobo/Kobo/Kobo eReader.conf";
pub const VERSION_FILE: &str = "/mnt/onboard/.kobo/version";

pub const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";
pub const BACKLIGHT_DIR: &str = "/sys/class/backlight";
/// Default mxc frontlight brightness node shared by every mxc-based preset.
pub const MXC_BRIGHTNESS_PATH: &str = "/sys/class/backlight/mxc_msp430.0/brightness";
pub const SUSPEND_STATE: &str = "/sys/power/state";
pub const FRAMEBUFFER_DEV: &str = "/dev/fb0";
pub const INPUT_DEV_DIR: &str = "/dev/input";
pub const INPUT_DEVICES_PROC: &str = "/proc/bus/input/devices";
pub const KOBO_CONFIG_SH: &str = "/bin/kobo_config.sh";

pub const FONTS_DIR: &str = "/mnt/onboard/.adds/fonts";
pub const USER_FONTS_DIR: &str = "/mnt/onboard/fonts";
pub const SYSTEM_FONTS_DIR: &str = "/usr/local/Kobo/fonts";

pub const WPA_CONF_KOBO: &str = "/mnt/onboard/.kobo/wpa_supplicant.conf";

#[allow(dead_code)]
pub const EDGE_DEBUG_LOG: &str = "/mnt/onboard/.adds/edge_debug.log";
