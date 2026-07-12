//! WiFi: link status (operstate + carrier), on/off toggle (wpa_supplicant +
//! dhcpcd), and the connected SSID read-out for the panel pill.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{info, warn};

const WLAN_IFACE: &str = "wlan0";
const WLAN_OPERSTATE_PATH: &str = "/sys/class/net/wlan0/operstate";
const WLAN_CARRIER_PATH: &str = "/sys/class/net/wlan0/carrier";
const WPA_CONF_PATHS: &[&str] = &[
    "/etc/wpa_supplicant/wpa_supplicant.conf",
    "/mnt/onboard/.kobo/wpa_supplicant.conf",
    "/tmp/wpa_supplicant.conf",
    "/var/run/wpa_supplicant/wlan0",
];
const WIFI_DAEMONS: &[&str] = &["dhcpcd", "udhcpc"];

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
            let _ = fs::write("/sys/class/rfkill/rfkill0/soft", "0");
            let _ = Command::new("rfkill").args(["unblock", "wifi"]).status();

            if let Err(e) = Command::new("ifconfig").args([WLAN_IFACE, "up"]).status() {
                warn!("wifi: ifconfig up failed: {e}");
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));

            let wpa_running = Command::new("pgrep")
                .args(["-x", "wpa_supplicant"])
                .output()
                .map(|o| !o.stdout.is_empty())
                .unwrap_or(false);

            if wpa_running {
                info!("wifi: wpa_supplicant already running, requesting reconnect");
            } else {
                let conf = WPA_CONF_PATHS
                    .iter()
                    .find(|p| std::path::Path::new(p).exists() && !p.ends_with("wlan0"));

                if let Some(conf) = conf {
                    if let Err(e) = Command::new("wpa_supplicant")
                        .args(["-B", "-i", WLAN_IFACE, "-c", conf])
                        .status()
                    {
                        warn!("wifi: wpa_supplicant start failed: {e}");
                    }
                    info!("wifi: wpa_supplicant started with {conf}");
                } else {
                    info!("wifi: no wpa_supplicant.conf found - connect from Kobo settings first");
                }
            }

            let _ = Command::new("wpa_cli")
                .args(["-i", WLAN_IFACE, "reconnect"])
                .status();

            if let Err(e) = Command::new("dhcpcd").args([WLAN_IFACE]).status() {
                let _ = Command::new("udhcpc")
                    .args(["-i", WLAN_IFACE, "-q"])
                    .status();
                warn!("wifi: dhcpcd failed, tried udhcpc: {e}");
            }
            info!("wifi: turned ON");
        } else {
            let _ = Command::new("killall")
                .args(["-q"])
                .args(WIFI_DAEMONS)
                .status();
            let _ = Command::new("wpa_cli")
                .args(["-i", WLAN_IFACE, "disconnect"])
                .status();
            if let Err(e) = Command::new("ifconfig").args([WLAN_IFACE, "down"]).status() {
                warn!("wifi: ifconfig down failed: {e}");
            }
            info!("wifi: turned OFF");
        }
    });
}

pub fn wifi_name() -> Option<String> {
    let out = Command::new("iwconfig").args([WLAN_IFACE]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_ssid(&text)
}

pub fn parse_ssid(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(idx) = line.find("ESSID:") {
            let rest = &line[idx + 6..];
            if let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                if let Some(end) = after.find('"') {
                    let ssid = &after[..end];
                    if !ssid.is_empty() {
                        return Some(ssid.to_string());
                    }
                }
            }
        }
    }
    None
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
}
