// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
pub mod legacy;
pub mod mtk;
pub mod nxp;

use super::config::{DeviceConfig, FrontlightConfig};
use std::sync::LazyLock;

pub(super) fn fl_mxc() -> FrontlightConfig {
    FrontlightConfig {
        brightness_path: "/sys/class/backlight/mxc_msp430.0/brightness".into(),
        mixer_path: None,
        nl_min: 0,
        nl_max: 10,
        nl_inverted: true,
    }
}

pub(super) fn fl_mxc_lm3630a() -> FrontlightConfig {
    FrontlightConfig {
        brightness_path: "/sys/class/backlight/lm3630a_led/brightness".into(),
        mixer_path: Some("/sys/class/backlight/lm3630a_led/color".into()),
        nl_min: 0,
        nl_max: 10,
        nl_inverted: true,
    }
}

pub(super) fn fl_mxc_aw99703(inverted: bool) -> FrontlightConfig {
    FrontlightConfig {
        brightness_path: "/sys/class/backlight/mxc_msp430.0/brightness".into(),
        mixer_path: Some("/sys/class/leds/aw99703-bl_FL1/color".into()),
        nl_min: 0,
        nl_max: 10,
        nl_inverted: inverted,
    }
}

pub(super) fn fl_mxc_tlc5947() -> FrontlightConfig {
    FrontlightConfig {
        brightness_path: "/sys/class/backlight/mxc_msp430.0/brightness".into(),
        mixer_path: Some("/sys/class/backlight/tlc5947_bl/color".into()),
        nl_min: 0,
        nl_max: 10,
        nl_inverted: true,
    }
}

pub static DEVICES: LazyLock<Vec<DeviceConfig>> = LazyLock::new(|| {
    let mut v = mtk::mtk_devices();
    v.extend(nxp::nxp_devices());
    v.extend(legacy::legacy_devices());
    v
});

#[cfg(test)]
mod tests {
    use super::super::config::*;
    use super::DEVICES;

    #[test]
    fn device_table_has_all_codenames() {
        for d in DEVICES.iter() {
            assert!(!d.codename.is_empty(), "entry with empty codename");
            assert!(!d.model.is_empty(), "{} has empty model", d.codename);
        }
    }

    #[test]
    fn device_table_unique_codenames() {
        let mut names: Vec<&str> = DEVICES.iter().map(|d| d.codename).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate codenames in DEVICES");
    }

    #[test]
    fn standby_state_matches_soc() {
        for d in DEVICES.iter() {
            match d.soc {
                SocFamily::Mtk => assert_eq!(
                    d.standby_state, "mem",
                    "{} (MTK) should use mem standby",
                    d.codename
                ),
                SocFamily::Nxp | SocFamily::Sunxi => assert_eq!(
                    d.standby_state, "standby",
                    "{} ({:?}) should use standby",
                    d.codename, d.soc
                ),
            }
        }
    }
}
