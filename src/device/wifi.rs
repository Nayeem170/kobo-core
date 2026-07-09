//! WiFi: link status (operstate + carrier), on/off toggle (wpa_supplicant +
//! dhcpcd), and the connected SSID read-out for the panel pill.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use log::info;

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
    let operstate = fs::read_to_string("/sys/class/net/wlan0/operstate").unwrap_or_default();
    let carrier = fs::read_to_string("/sys/class/net/wlan0/carrier").unwrap_or_default();
    wifi_connected(&operstate, &carrier)
}

pub fn wifi_connected(operstate: &str, carrier: &str) -> bool {
    operstate.trim() == "up" && carrier.trim() == "1"
}

pub fn wifi_toggle(on: bool) {
    LAST_WIFI_TOGGLE_MS.store(now_ms(), Ordering::Relaxed);
    std::thread::spawn(move || {
        if on {
            let _ = Command::new("killall")
                .args(["-q", "wpa_supplicant", "dhcpcd", "udhcpc"])
                .status();
            std::thread::sleep(std::time::Duration::from_millis(500));

            let _ = Command::new("ifconfig").args(["wlan0", "up"]).status();

            let conf = ["/etc/wpa_supplicant/wpa_supplicant.conf"]
                .iter()
                .find_map(|p| {
                    if std::path::Path::new(p).exists() {
                        Some(*p)
                    } else {
                        None
                    }
                });

            if let Some(conf) = conf {
                let _ = Command::new("wpa_supplicant")
                    .args(["-B", "-i", "wlan0", "-c", conf])
                    .status();
                info!("wifi: wpa_supplicant started with {conf}");
            } else {
                info!("wifi: no wpa_supplicant.conf found - connect from Kobo settings first");
            }

            let _ = Command::new("dhcpcd").args(["wlan0"]).status();
            info!("wifi: turned ON");
        } else {
            let _ = Command::new("killall")
                .args(["-q", "wpa_supplicant", "dhcpcd", "udhcpc"])
                .status();
            let _ = Command::new("ifconfig").args(["wlan0", "down"]).status();
            info!("wifi: turned OFF");
        }
    });
}

pub fn wifi_name() -> Option<String> {
    let out = Command::new("iwconfig").args(["wlan0"]).output().ok()?;
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
