// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use std::fs;
use std::process::Command;

use log::info;

use super::wpa::{find_wpa_conf, init_wpa_detection, wpa_cli, WPA_DRV};
use super::WLAN_IFACE;
use super::{
    CARRIER_POLL_MAX, CARRIER_POLL_MS, IFACE_UP_SETTLE_MS, RFKILL_SOFT_PATH, WIFI_DAEMONS,
    WLAN_CARRIER_PATH, WLAN_OPERSTATE_PATH, WMT_DBG_PATH, WMT_WIFI_DEV, WPA_DRV_NL80211,
};

pub(super) fn is_mtk_platform() -> bool {
    std::path::Path::new(WMT_WIFI_DEV).exists() || std::path::Path::new(WMT_DBG_PATH).exists()
}

pub(super) fn power_on_mtk_wmt() -> bool {
    if !std::path::Path::new(WMT_DBG_PATH).exists() {
        log::warn!("wifi: wmt_dbg missing - radio never initialized this boot");
        log::warn!("wifi: cold-boot WMT power-on not yet supported, open nickel WiFi once first");
        return false;
    }
    let w = |val: &str| match fs::write(WMT_DBG_PATH, val) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("wifi: wmt_dbg write '{val}' failed: {e}");
            false
        }
    };
    info!("wifi: MTK WMT power-on sequence starting");
    if !w(super::WMT_UNLOCK_TOKEN) {
        return false;
    }
    if !w(super::WMT_FUNC_OFF) {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(super::WMT_SETTLE_MS));
    if !w(super::WMT_UNLOCK_TOKEN) {
        return false;
    }
    if !w(super::WMT_FUNC_ON) {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    match fs::write(WMT_WIFI_DEV, super::WMT_WIFI_POWER_ON) {
        Ok(_) => {
            info!("wifi: MTK WMT radio powered on");
            true
        }
        Err(e) => {
            log::warn!("wifi: wmtWifi power-on failed: {e}");
            false
        }
    }
}

pub(super) fn wifi_turn_on() {
    if is_mtk_platform() {
        info!("wifi: MTK platform detected");
        power_on_mtk_wmt();
        std::thread::sleep(std::time::Duration::from_millis(500));
    } else {
        info!("wifi: NXP platform, using rfkill");
        // best-effort: rfkill may not exist on all models
        if let Err(e) = fs::write(RFKILL_SOFT_PATH, "0") {
            log::warn!("wifi: rfkill write failed: {e}");
        }
        // best-effort: rfkill unblock may be absent on some models
        let _ = Command::new("rfkill").args(["unblock", "wifi"]).status();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let ifconfig_ok = Command::new("ifconfig")
        .args([WLAN_IFACE, "up"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ifconfig_ok {
        info!("wifi: ifconfig failed, trying ip link");
        // best-effort: ip may not be installed
        let _ = Command::new("ip")
            .args(["link", "set", WLAN_IFACE, "up"])
            .status();
    }
    std::thread::sleep(std::time::Duration::from_millis(IFACE_UP_SETTLE_MS));

    let wpa_running = Command::new("pgrep")
        .args(["-x", "wpa_supplicant"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    info!("wifi: wpa_supplicant running={wpa_running}");

    if wpa_running {
        info!("wifi: wpa_supplicant already running, reconfiguring");
        // best-effort: reconfigure fails silently if the ctrl socket is unavailable
        let _ = wpa_cli(&["reconfigure"]);
    } else {
        let conf = find_wpa_conf();
        let bin = super::wpa::find_wpa_binary();
        info!("wifi: wpa_supplicant conf={conf:?} bin={bin:?}");
        match (conf, bin) {
            (Some(conf_path), Some(wpa_bin)) => {
                init_wpa_detection();
                let driver = WPA_DRV.get().map(|s| s.as_str()).unwrap_or(WPA_DRV_NL80211);
                let out = Command::new(wpa_bin)
                    .args(["-B", "-i", WLAN_IFACE, "-c", &conf_path, "-D", driver])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        info!("wifi: wpa_supplicant started (-D {driver}) with {conf_path}");
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        log::warn!(
                            "wifi: wpa_supplicant (-D {driver}) failed: {}",
                            stderr.trim()
                        );
                        info!("wifi: retrying without -D flag");
                        // best-effort: fallback without driver flag
                        let _ = Command::new(wpa_bin)
                            .args(["-B", "-i", WLAN_IFACE, "-c", &conf_path])
                            .status();
                    }
                    Err(e) => {
                        log::warn!("wifi: wpa_supplicant exec error: {e}");
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(IFACE_UP_SETTLE_MS));
            }
            (None, _) => {
                log::warn!("wifi: no wpa_supplicant.conf found");
            }
            (_, None) => {
                log::warn!("wifi: wpa_supplicant binary not found");
            }
        }
    }

    // best-effort: reconnect may fail if wpa_cli socket unavailable
    let _ = wpa_cli(&["reconnect"]);
    std::thread::sleep(std::time::Duration::from_millis(CARRIER_POLL_MS));

    let mut got_carrier = false;
    for i in 0..CARRIER_POLL_MAX {
        let carrier = fs::read_to_string(WLAN_CARRIER_PATH).unwrap_or_default();
        if carrier.trim() == "1" {
            info!("wifi: carrier acquired after {} poll(s)", i + 1);
            got_carrier = true;
            break;
        }
        if i == 0 || i == 10 || i == 20 {
            let opers = fs::read_to_string(WLAN_OPERSTATE_PATH).unwrap_or_default();
            info!(
                "wifi: poll {i} carrier={} operstate={}",
                carrier.trim(),
                opers.trim()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(CARRIER_POLL_MS));
    }
    if !got_carrier {
        log::warn!("wifi: no carrier after 15s");
        if let Some(o) = wpa_cli(&["status"]) {
            let status = String::from_utf8_lossy(&o.stdout);
            log::warn!("wifi: wpa status: {}", status.trim());
        }
    }

    if got_carrier {
        let dhcp_ok = Command::new("dhcpcd")
            .args([WLAN_IFACE])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !dhcp_ok {
            info!("wifi: dhcpcd failed, trying udhcpc");
            // best-effort: udhcpc may not exist
            let _ = Command::new("udhcpc")
                .args(["-i", WLAN_IFACE, "-q"])
                .status();
        }
    }
    info!("wifi: turned ON (carrier={got_carrier})");
}

pub(super) fn wifi_turn_off() {
    // best-effort: daemons may already be gone
    let _ = Command::new("killall")
        .args(["-q"])
        .args(WIFI_DAEMONS)
        .status();
    // best-effort: disconnect is advisory; the interface goes down regardless
    let _ = wpa_cli(&["disconnect"]);
    if let Err(e) = Command::new("ifconfig").args([WLAN_IFACE, "down"]).status() {
        log::warn!("wifi: ifconfig down failed: {e}");
    }
    info!("wifi: turned OFF");
}
