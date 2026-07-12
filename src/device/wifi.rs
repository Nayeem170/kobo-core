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
    "/etc/wpa_supplicant-wlan0.conf",
    "/data/misc/wifi/wpa_supplicant.conf",
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
            wifi_turn_on();
        } else {
            wifi_turn_off();
        }
    });
}

fn find_wpa_conf() -> Option<String> {
    for p in WPA_CONF_PATHS {
        if std::path::Path::new(p).exists() {
            info!("wifi: config found at {p}");
            return Some((*p).to_string());
        }
    }
    info!("wifi: config not at known paths, searching filesystem...");
    for dir in &["/etc", "/mnt/onboard/.kobo", "/var", "/data"] {
        if let Ok(out) = Command::new("find")
            .args([dir, "-name", "wpa_supplicant*.conf", "-type", "f"])
            .output()
        {
            let found = String::from_utf8_lossy(&out.stdout);
            for line in found.lines() {
                let path = line.trim();
                if !path.is_empty() && std::path::Path::new(path).exists() {
                    info!("wifi: config found via search: {path}");
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

fn wifi_turn_on() {
    let _ = fs::write("/sys/class/rfkill/rfkill0/soft", "0");
    let _ = Command::new("rfkill").args(["unblock", "wifi"]).status();
    std::thread::sleep(std::time::Duration::from_millis(500));

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
        info!("wifi: wpa_supplicant already running, reconfiguring");
        let _ = Command::new("wpa_cli")
            .args(["-i", WLAN_IFACE, "reconfigure"])
            .status();
    } else {
        let conf = find_wpa_conf();
        match conf {
            Some(ref conf_path) => {
                match Command::new("wpa_supplicant")
                    .args(["-B", "-i", WLAN_IFACE, "-c", conf_path])
                    .output()
                {
                    Ok(out) if out.status.success() => {
                        info!("wifi: wpa_supplicant started with {conf_path}");
                    }
                    Ok(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        warn!("wifi: wpa_supplicant failed: {}", stderr.trim());
                    }
                    Err(e) => {
                        warn!("wifi: wpa_supplicant binary not found: {e}");
                    }
                }
            }
            None => {
                warn!("wifi: no wpa_supplicant.conf found anywhere on device");
            }
        }
    }

    let _ = Command::new("wpa_cli")
        .args(["-i", WLAN_IFACE, "reconnect"])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let mut got_carrier = false;
    for i in 0..30 {
        let carrier = fs::read_to_string(WLAN_CARRIER_PATH).unwrap_or_default();
        if carrier.trim() == "1" {
            info!("wifi: carrier acquired after {} poll(s)", i + 1);
            got_carrier = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if !got_carrier {
        warn!("wifi: no carrier after 15s");
        if let Ok(out) = Command::new("wpa_cli")
            .args(["-i", WLAN_IFACE, "status"])
            .output()
        {
            let status = String::from_utf8_lossy(&out.stdout);
            warn!("wifi: wpa status: {}", status.trim());
        }
    }

    if got_carrier {
        if let Err(e) = Command::new("dhcpcd").args([WLAN_IFACE]).status() {
            let _ = Command::new("udhcpc").args(["-i", WLAN_IFACE, "-q"]).status();
            warn!("wifi: dhcpcd failed, tried udhcpc: {e}");
        }
    }
    info!("wifi: turned ON (carrier={got_carrier})");
}

fn wifi_turn_off() {
    let _ = Command::new("killall").args(["-q"]).args(WIFI_DAEMONS).status();
    let _ = Command::new("wpa_cli")
        .args(["-i", WLAN_IFACE, "disconnect"])
        .status();
    if let Err(e) = Command::new("ifconfig").args([WLAN_IFACE, "down"]).status() {
        warn!("wifi: ifconfig down failed: {e}");
    }
    info!("wifi: turned OFF");
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
