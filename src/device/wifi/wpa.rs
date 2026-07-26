// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use std::fs;
use std::process::Command;
use std::sync::OnceLock;

use log::{info, warn};

use super::{
    WLAN_IFACE, WPA_CONF_PATHS, WPA_CTRL_DEFAULT, WPA_DRV_NL80211, WPA_DRV_WEXT,
    WPA_SUPPLICANT_CANDIDATES,
};

pub(super) static WPA_CTRL: OnceLock<Option<String>> = OnceLock::new();
pub(super) static WPA_DRV: OnceLock<String> = OnceLock::new();

pub(super) fn read_wpa_cmdline() -> Vec<String> {
    let out = match Command::new("pgrep")
        .args(["-x", "wpa_supplicant"])
        .output()
    {
        Ok(o) if !o.stdout.is_empty() => o,
        _ => return Vec::new(),
    };
    let pid_str = String::from_utf8_lossy(&out.stdout);
    let pid = match pid_str.trim().lines().next() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let raw = match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let args: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|s| std::str::from_utf8(s).ok().map(String::from))
        .collect();
    if !args.is_empty() {
        info!("wifi: wpa_supplicant pid={pid} cmdline: {:?}", args);
    }
    args
}

pub fn init_wpa_detection() {
    WPA_CTRL.get_or_init(|| {
        let args = read_wpa_cmdline();
        let ctrl = args
            .iter()
            .position(|a| a == "-C")
            .and_then(|i| args.get(i + 1))
            .map(|s| {
                info!("wifi: ctrl socket from wpa_supplicant: {s}");
                s.clone()
            });
        ctrl
    });
    WPA_DRV.get_or_init(|| {
        let args = read_wpa_cmdline();
        let drv = args
            .iter()
            .position(|a| a == "-D")
            .and_then(|i| args.get(i + 1))
            .map(|s| {
                info!("wifi: driver from wpa_supplicant: {s}");
                s.clone()
            })
            .unwrap_or_else(|| {
                if super::power::is_mtk_platform() {
                    WPA_DRV_NL80211.into()
                } else {
                    WPA_DRV_WEXT.into()
                }
            });
        drv
    });
}

pub(super) fn wpa_cli(args: &[&str]) -> Option<std::process::Output> {
    init_wpa_detection();
    let detected = WPA_CTRL.get().and_then(|o| o.as_ref());
    let candidates: Vec<Option<&str>> =
        vec![detected.map(|s| s.as_str()), Some(WPA_CTRL_DEFAULT), None];
    for ctrl in &candidates {
        let mut cmd = Command::new("wpa_cli");
        cmd.args(["-i", WLAN_IFACE]);
        if let Some(c) = ctrl {
            cmd.args(["-p", c]);
        }
        cmd.args(args);
        match cmd.output() {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let rc = o.status.code().unwrap_or(-1);
                let label = ctrl.unwrap_or("default");
                if stdout.contains("Failed to connect") || stdout.contains("Could not connect") {
                    info!("wpa_cli {:?}: ctrl={label} no socket", args);
                    continue;
                }
                if !o.status.success() {
                    info!(
                        "wpa_cli {:?}: ctrl={label} rc={rc} out={}",
                        args,
                        stdout.trim()
                    );
                    continue;
                }
                info!(
                    "wpa_cli {:?}: ctrl={label} rc=0 out={}",
                    args,
                    stdout.trim()
                );
                return Some(o);
            }
            Err(e) => {
                warn!("wpa_cli {:?}: exec error: {e}", args);
            }
        }
    }
    warn!("wpa_cli {:?}: all paths failed", args);
    None
}

pub(super) fn find_wpa_conf() -> Option<String> {
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

pub(super) fn find_wpa_binary() -> Option<&'static str> {
    for p in WPA_SUPPLICANT_CANDIDATES {
        if std::path::Path::new(p).exists()
            || Command::new("which")
                .arg(p)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        {
            return Some(p);
        }
    }
    None
}
