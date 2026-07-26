// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
mod power;
mod scan;
mod wpa;

pub use scan::{wifi_list_networks, wifi_name, wifi_saved_ssids, wifi_select_network, WifiNetwork};
pub use wpa::init_wpa_detection;

#[cfg(test)]
use scan::{parse_conf_ssids, parse_ssid, parse_wpa_networks};

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const WLAN_IFACE: &str = "wlan0";
pub(super) const WLAN_OPERSTATE_PATH: &str = "/sys/class/net/wlan0/operstate";
pub(super) const WLAN_CARRIER_PATH: &str = "/sys/class/net/wlan0/carrier";
pub(super) const WPA_CONF_PATHS: &[&str] = &[
    "/etc/wpa_supplicant/wpa_supplicant.conf",
    crate::device::paths::WPA_CONF_KOBO,
    "/tmp/wpa_supplicant.conf",
    "/etc/wpa_supplicant-wlan0.conf",
    "/data/misc/wifi/wpa_supplicant.conf",
];
pub(super) const WIFI_DAEMONS: &[&str] = &["dhcpcd", "udhcpc"];
pub(super) const WMT_DBG_PATH: &str = "/proc/driver/wmt_dbg";
pub(super) const WMT_WIFI_DEV: &str = "/dev/wmtWifi";
pub(super) const WMT_UNLOCK_TOKEN: &str = "0xDB9DB9";
pub(super) const WMT_FUNC_OFF: &str = "7 9 0";
pub(super) const WMT_FUNC_ON: &str = "7 9 1";
pub(super) const WMT_WIFI_POWER_ON: &str = "1";
pub(super) const WMT_SETTLE_MS: u64 = 1000;
pub(super) const RFKILL_SOFT_PATH: &str = "/sys/class/rfkill/rfkill0/soft";
pub(super) const IFACE_UP_SETTLE_MS: u64 = 1000;
pub(super) const CARRIER_POLL_MS: u64 = 500;
pub(super) const CARRIER_POLL_MAX: usize = 30;
pub(super) const WPA_DRV_NL80211: &str = "nl80211";
pub(super) const WPA_DRV_WEXT: &str = "wext";
pub(super) const WPA_CTRL_DEFAULT: &str = "/var/run/wpa_supplicant";
pub(super) const WPA_SUPPLICANT_CANDIDATES: &[&str] = &[
    "wpa_supplicant",
    "/usr/sbin/wpa_supplicant",
    "/system/bin/wpa_supplicant",
    "/sbin/wpa_supplicant",
];

static LAST_WIFI_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn wifi_toggle_age_ms() -> u64 {
    let t = LAST_WIFI_TOGGLE_MS.load(Ordering::Relaxed);
    if t == 0 {
        u64::MAX
    } else {
        now_ms().saturating_sub(t)
    }
}

pub fn wifi_status() -> bool {
    let operstate = fs::read_to_string(WLAN_OPERSTATE_PATH).unwrap_or_default();
    let carrier = fs::read_to_string(WLAN_CARRIER_PATH).unwrap_or_default();
    wifi_connected(&operstate, &carrier)
}

pub fn wifi_connected(operstate: &str, carrier: &str) -> bool {
    operstate.trim() == "up" && carrier.trim() == "1"
}

pub fn wifi_toggle(on: bool) {
    LAST_WIFI_TOGGLE_MS.store(now_ms(), Ordering::Relaxed);
    std::thread::spawn(move || {
        if on {
            power::wifi_turn_on();
        } else {
            power::wifi_turn_off();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_connected_requires_up_and_carrier() {
        assert!(wifi_connected("up", "1"));
        assert!(
            !wifi_connected("down", "1"),
            "interface down -> not connected"
        );
        assert!(!wifi_connected("up", "0"), "no carrier -> not connected");
        assert!(
            !wifi_connected("up", ""),
            "missing carrier -> not connected"
        );
    }

    #[test]
    fn parse_ssid_extracts_quoted_essid() {
        let out = "wlan0     IEEE 802.11  ESSID:\"MyNetwork\"\n\
                   Mode:Managed  Frequency:2.412 GHz\n";
        assert_eq!(parse_ssid(out).as_deref(), Some("MyNetwork"));
    }

    #[test]
    fn parse_ssid_returns_none_for_off_or_any() {
        assert_eq!(parse_ssid("ESSID:off/any\n"), None);
        assert_eq!(parse_ssid("ESSID:\"\"\n"), None, "empty SSID rejected");
        assert_eq!(parse_ssid("no essid here"), None);
    }

    #[test]
    fn parse_wpa_networks_basic() {
        let out = "network_id / ssid / bssid / flags\n\
                   0\thome-net\tany\t[CURRENT]\n\
                   1\toffice\tany\t\n\
                   2\tphone\tany\t[DISABLED]\n";
        let nets = parse_wpa_networks(out);
        assert_eq!(nets.len(), 2);
        assert_eq!(nets[0].id, 0);
        assert_eq!(nets[0].ssid, "home-net");
        assert!(nets[0].connected);
        assert_eq!(nets[1].id, 1);
        assert_eq!(nets[1].ssid, "office");
        assert!(!nets[1].connected);
    }

    #[test]
    fn parse_wpa_networks_skips_disabled_and_empty() {
        let out = "network_id / ssid / bssid / flags\n\
                   0\t\\x00\tany\t\n\
                   1\toffice\tany\t[DISABLED]\n\
                   2\tgood\tany\t\n";
        let nets = parse_wpa_networks(out);
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].ssid, "good");
    }

    #[test]
    fn parse_wpa_networks_empty() {
        assert!(parse_wpa_networks("").is_empty());
        assert!(parse_wpa_networks("network_id / ssid / bssid / flags\n").is_empty());
    }

    #[test]
    fn parse_conf_ssids_quoted_and_hex() {
        let conf = "\
ctrl_interface=/var/run/wpa_supplicant\n\
network={\n\
    ssid=\"home-net\"\n\
    psk=\"secret\"\n\
}\n\
network={\n\
    ssid=4f6666696365\n\
    psk=\"secret2\"\n\
}\n";
        let ssids = parse_conf_ssids(conf);
        assert_eq!(ssids.len(), 2);
        assert_eq!(ssids[0], "home-net");
        assert_eq!(ssids[1], "4f6666696365");
    }

    #[test]
    fn parse_conf_ssids_skips_empty() {
        let conf = "ssid=\"\"\nssid=\\x00\nssid=\"good\"\n";
        let ssids = parse_conf_ssids(conf);
        assert_eq!(ssids, vec!["good"]);
    }

    #[test]
    fn parse_conf_ssids_empty_input() {
        assert!(parse_conf_ssids("").is_empty());
        assert!(parse_conf_ssids("ctrl_interface=foo\n").is_empty());
    }
}
