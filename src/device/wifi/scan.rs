// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use std::fs;
use std::process::Command;

use log::info;

use super::wpa::{find_wpa_conf, wpa_cli};
use super::WLAN_IFACE;

pub struct WifiNetwork {
    pub id: u32,
    pub ssid: String,
    pub connected: bool,
}

pub fn wifi_list_networks() -> Vec<WifiNetwork> {
    let out = match wpa_cli(&["list_networks"]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    parse_wpa_networks(&text)
}

pub(super) fn parse_wpa_networks(text: &str) -> Vec<WifiNetwork> {
    let mut result = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let id = match parts[0].parse::<u32>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let ssid = parts[1].trim();
        if ssid.is_empty() || ssid == "\\x00" {
            continue;
        }
        let flags = parts[3].trim();
        if flags.contains("[DISABLED]") {
            continue;
        }
        result.push(WifiNetwork {
            id,
            ssid: ssid.to_string(),
            connected: flags.contains("[CURRENT]"),
        });
    }
    result
}

pub fn wifi_select_network(id: u32) {
    info!("wifi: selecting network {id}");
    let id_str = id.to_string();
    // best-effort: wpa_cli may be unreachable; selection is retried on reconnect
    let _ = wpa_cli(&["select_network", &id_str]);
    // best-effort: same socket risk as above; reconnect is advisory
    let _ = wpa_cli(&["reconnect"]);
}

pub fn wifi_saved_ssids() -> Vec<String> {
    let conf = match find_wpa_conf() {
        Some(p) => match fs::read_to_string(&p) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        },
        None => return Vec::new(),
    };
    parse_conf_ssids(&conf)
}

pub(super) fn parse_conf_ssids(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("ssid=") {
            let ssid = if let Some(s) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                s
            } else {
                rest
            };
            if !ssid.is_empty() && ssid != "\\x00" {
                result.push(ssid.to_string());
            }
        }
    }
    result
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

pub fn wifi_name() -> Option<String> {
    let out = Command::new("iwconfig").args([WLAN_IFACE]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_ssid(&text)
}
