// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Pure device configuration types and lookup - no IO.
//!
//! These types describe per-model hardware capabilities. IO-bound detection
//! (`detect_device`, `scan_input_devices`, `automagic_*`) lives in the app
//! crate's `device::hw` module.

use crate::device::registry::DEVICES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocFamily {
    Mtk,
    Nxp,
    Sunxi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchProtocol {
    Snow,
    Phoenix,
    Legacy,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FrontlightConfig {
    pub brightness_path: String,
    pub mixer_path: Option<String>,
    pub nl_min: u32,
    pub nl_max: u32,
    pub nl_inverted: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeviceConfig {
    pub codename: &'static str,
    pub model: &'static str,
    pub soc: SocFamily,
    pub display_dpi: u32,
    pub has_color_screen: bool,
    pub has_natural_light: bool,
    pub has_bt: bool,
    pub has_keys: bool,
    pub has_gsensor: bool,
    pub has_eclipse_wfm: bool,
    pub is_smp: bool,
    pub frontlight: FrontlightConfig,
    pub battery_sysfs: String,
    pub touch_protocol: TouchProtocol,
    pub touch_switch_xy: bool,
    pub touch_mirrored_x: bool,
    pub touch_mirrored_y: bool,
    pub standby_state: &'static str,
    pub automagic_sysfs: bool,
}

impl Default for FrontlightConfig {
    fn default() -> Self {
        FrontlightConfig {
            brightness_path: "/sys/class/backlight/mxc_msp430.0/brightness".into(),
            mixer_path: None,
            nl_min: 0,
            nl_max: 10,
            nl_inverted: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InputDevices {
    pub touch_dev: String,
    pub power_dev: String,
}

pub fn lookup_device(codename: &str) -> Option<&'static DeviceConfig> {
    DEVICES.iter().find(|d| d.codename == codename)
}

/// Pure device resolution: the matching entry for `codename`, or the Libra
/// Colour (`monza`) fallback used when the codename is unrecognised. No IO.
pub fn detect_from_codename(codename: &str) -> &'static DeviceConfig {
    lookup_device(codename).unwrap_or_else(|| {
        DEVICES
            .iter()
            .find(|d| d.codename == "monza")
            .expect("monza entry must exist in DEVICES")
    })
}

/// Pure parser for `/proc/bus/input/devices` text. Returns (touch_path,
/// power_path) extracted from the Handlers + Phys lines, before any fallback
/// to /dev/input/event{1,2}. No IO.
pub fn parse_input_devices(content: &str) -> (Option<String>, Option<String>) {
    let mut touch_path: Option<String> = None;
    let mut power_path: Option<String> = None;
    let mut current_handlers = Vec::new();
    for line in content.lines() {
        if let Some(handlers) = line.strip_prefix("H: Handlers=") {
            current_handlers.clear();
            current_handlers = handlers.split_whitespace().map(String::from).collect();
        }
        if let Some(dev) = line.strip_prefix("P: Phys=") {
            let is_touch = current_handlers
                .iter()
                .any(|h| h.starts_with("event") && (dev.contains("touch") || dev.contains("ts")));
            let is_power = current_handlers.iter().any(|h| {
                h.starts_with("event")
                    && (dev.contains("gpio") || dev.contains("power") || dev.contains("keys"))
            });
            if is_touch && touch_path.is_none() {
                for h in &current_handlers {
                    if h.starts_with("event") {
                        touch_path = Some(format!("/dev/input/{}", h));
                        break;
                    }
                }
            }
            if is_power && power_path.is_none() {
                for h in &current_handlers {
                    if h.starts_with("event") {
                        power_path = Some(format!("/dev/input/{}", h));
                        break;
                    }
                }
            }
        }
    }
    (touch_path, power_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_from_codename_known() {
        let d = detect_from_codename("monza");
        assert_eq!(d.model, "Kobo Libra Colour");
        assert_eq!(d.soc, SocFamily::Mtk);
    }

    #[test]
    fn detect_from_codename_unknown_falls_back() {
        let fallback = detect_from_codename("does-not-exist");
        assert_eq!(
            fallback.codename, "monza",
            "unknown codename must fall back to monza"
        );
        assert!(lookup_device("does-not-exist").is_none());
    }

    #[test]
    fn parse_input_devices_extracts_event_nodes() {
        let sample = "\
I: Bus=0018 Vendor=0000 Product=0000 Version=0000
N: Name=\"cyttsp5_mt\"
H: Handlers=event1 mouse0
P: Phys=cyttsp5_mt/input0 touch\n\
I: Bus=0019 Vendor=0001 Product=0001 Version=0100
N: Name=\"gpio-keys\"
H: Handlers=event2
P: Phys=gpio-keys/power0 power\n";
        let (touch, power) = parse_input_devices(sample);
        assert_eq!(touch.as_deref(), Some("/dev/input/event1"));
        assert_eq!(power.as_deref(), Some("/dev/input/event2"));
    }
}
