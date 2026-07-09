//! Device detection IO: codename discovery, input scanning, sysfs automagic.
//!
//! Pure types and lookup live in [`crate::device::config`].

use std::fs;
use std::path::PathBuf;

use log::{info, warn};

use crate::device::config::{
    detect_from_codename, lookup_device, parse_input_devices, DeviceConfig, InputDevices, SocFamily,
};

fn product_from_env() -> Option<String> {
    std::env::var("PRODUCT").ok()
}

fn product_from_kobo_config() -> Option<String> {
    let output = std::process::Command::new("/bin/kobo_config.sh")
        .output()
        .ok()?;
    if output.status.success() {
        let codename = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !codename.is_empty() {
            return Some(codename);
        }
    }
    None
}

fn product_from_version() -> Option<String> {
    let content = fs::read_to_string("/mnt/onboard/.kobo/version").ok()?;
    let last_line = content.lines().last()?.trim();
    let parts: Vec<&str> = last_line.split(',').collect();
    let version = parts.last()?.trim();
    let pid = version.len().checked_sub(3)?;
    let code = &version[pid..];
    match code {
        "379" => Some("snow".into()),
        "378" => Some("snow".into()),
        "310" => Some("trilogy".into()),
        "320" => Some("trilogy".into()),
        _ => None,
    }
}

pub fn detect_device() -> Option<DeviceConfig> {
    let codename = product_from_env()
        .or_else(product_from_kobo_config)
        .or_else(product_from_version);
    let codename = match codename {
        Some(c) => c,
        None => {
            warn!("hw: could not detect Kobo model ($PRODUCT unset, kobo_config.sh failed, version parse failed)");
            return None;
        }
    };
    info!("hw: $PRODUCT = {}", codename);
    match lookup_device(&codename) {
        Some(d) => {
            info!(
                "hw: detected {} ({}) — {} SoC, {}x{} config, touch={:?}, bt={}",
                d.model,
                d.codename,
                match d.soc {
                    SocFamily::Mtk => "MTK",
                    SocFamily::Nxp => "NXP",
                    SocFamily::Sunxi => "sunxi",
                },
                if d.has_color_screen { "color" } else { "B&W" },
                d.display_dpi,
                d.touch_protocol,
                d.has_bt,
            );
            Some(d.clone())
        }
        None => {
            warn!(
                "hw: unknown codename '{}', falling back to Libra Colour defaults",
                codename
            );
            Some(detect_from_codename(&codename).clone())
        }
    }
}

pub fn scan_input_devices() -> Option<InputDevices> {
    let input_dir = PathBuf::from("/dev/input");
    if !input_dir.exists() {
        warn!("hw: /dev/input does not exist");
        return None;
    }
    let content = fs::read_to_string("/proc/bus/input/devices").ok()?;
    let (mut touch_path, mut power_path) = parse_input_devices(&content);
    if touch_path.is_none() {
        let ev1 = PathBuf::from("/dev/input/event1");
        if ev1.exists() {
            touch_path = Some("/dev/input/event1".into());
        }
    }
    if power_path.is_none() {
        let ev2 = PathBuf::from("/dev/input/event2");
        if ev2.exists() {
            power_path = Some("/dev/input/event2".into());
        }
    }
    match (touch_path, power_path) {
        (Some(t), Some(p)) => {
            info!("hw: input scan — touch={}, power={}", t, p);
            Some(InputDevices {
                touch_dev: t,
                power_dev: p,
            })
        }
        (Some(t), None) => {
            info!(
                "hw: input scan — touch={}, power=not found (touch-to-wake only)",
                t
            );
            Some(InputDevices {
                touch_dev: t,
                power_dev: String::new(),
            })
        }
        _ => None,
    }
}

pub fn automagic_battery(cfg: &mut DeviceConfig) {
    if !cfg.automagic_sysfs {
        return;
    }
    let candidates = [
        "/sys/class/power_supply/battery",
        "/sys/class/power_supply/bd71827_bat",
        "/sys/class/power_supply/mc13892_bat",
    ];
    for path in &candidates {
        if fs::metadata(path).is_ok() {
            cfg.battery_sysfs = path.to_string();
            info!("hw: automagic battery → {}", path);
            return;
        }
    }
}

pub fn automagic_frontlight(cfg: &mut DeviceConfig) {
    if !cfg.automagic_sysfs {
        return;
    }
    let mixers = [
        "/sys/class/leds/aw99703-bl_FL1/color",
        "/sys/class/backlight/lm3630a_led/color",
        "/sys/class/backlight/tlc5947_bl/color",
    ];
    for mixer in &mixers {
        if fs::metadata(mixer).is_ok() {
            cfg.frontlight.mixer_path = Some(mixer.to_string());
            info!("hw: automagic FL mixer → {}", mixer);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automagic_battery_noop_when_disabled() {
        let mut cfg = detect_from_codename("monza").clone();
        cfg.automagic_sysfs = false;
        let before = cfg.battery_sysfs.clone();
        automagic_battery(&mut cfg);
        assert_eq!(
            cfg.battery_sysfs, before,
            "automagic must not run when disabled"
        );
    }
}
