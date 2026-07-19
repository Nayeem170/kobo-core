// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Bluetooth paired-device discovery: parse the bluez ObjectManager dump,
//! cache the active target device, and resolve which device to talk to.

use std::fs;
use std::sync::Mutex;

use log::debug;

use super::{dbus_cmd, default_bt_device, DBUS_OBJECT_MANAGER};

pub struct PairedDevice {
    pub path: String,
    pub connected: bool,
}

static CACHED_BT_DEVICE: Mutex<Option<String>> = Mutex::new(None);

fn get_cached_bt_device() -> Option<String> {
    CACHED_BT_DEVICE.lock().ok().and_then(|g| g.clone())
}

pub(super) fn set_cached_bt_device(dev: &str) {
    if let Ok(mut guard) = CACHED_BT_DEVICE.lock() {
        *guard = Some(dev.to_string());
    }
}

pub(super) fn clear_cached_bt_device() {
    if let Ok(mut guard) = CACHED_BT_DEVICE.lock() {
        *guard = None;
    }
}

fn parse_managed_objects(text: &str) -> Vec<PairedDevice> {
    let mut result: Vec<PairedDevice> = Vec::new();
    let mut current_path: Option<String> = None;
    let mut in_device_iface = false;
    let mut is_paired = false;
    let mut is_connected = false;
    let mut expect_paired_val = false;
    let mut expect_connected_val = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(p) = t
            .strip_prefix("object path \"")
            .and_then(|s| s.strip_suffix('"'))
        {
            if in_device_iface && is_paired {
                if let Some(path) = current_path.take() {
                    result.push(PairedDevice {
                        path,
                        connected: is_connected,
                    });
                }
            }
            current_path = Some(p.to_string());
            in_device_iface = false;
            is_paired = false;
            is_connected = false;
            expect_paired_val = false;
            expect_connected_val = false;
            continue;
        }
        if t == "string \"org.bluez.Device1\"" {
            in_device_iface = true;
            continue;
        }
        if t == "string \"Paired\"" {
            expect_paired_val = true;
            continue;
        }
        if t == "string \"Connected\"" {
            expect_connected_val = true;
            continue;
        }
        if expect_paired_val {
            expect_paired_val = false;
            if t.contains("boolean true") && in_device_iface {
                is_paired = true;
            }
        } else if expect_connected_val {
            expect_connected_val = false;
            if t.contains("boolean true") && in_device_iface {
                is_connected = true;
            }
        }
    }
    if in_device_iface && is_paired {
        if let Some(path) = current_path.take() {
            result.push(PairedDevice {
                path,
                connected: is_connected,
            });
        }
    }
    result
}

pub fn discover_paired_devices() -> Vec<PairedDevice> {
    let out = match dbus_cmd().args(["/", DBUS_OBJECT_MANAGER]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let result = parse_managed_objects(&text);
    debug!(
        "bt discover: paired={:?}",
        result
            .iter()
            .map(|d| format!("{}(connected={})", d.path, d.connected))
            .collect::<Vec<_>>()
    );
    result
}

pub(super) fn discover_connected_paired_device() -> Option<String> {
    discover_paired_devices()
        .into_iter()
        .find(|d| d.connected)
        .map(|d| d.path)
}

/// The BT device to connect/query: a device already connected by btservice
/// (preferred), then the configured default, then the first paired device.
/// Cached for the session; the cache is cleared on BT power-off so the next
/// power-on re-resolves with live connected state.
pub fn bt_target_device() -> Option<String> {
    if let Some(d) = get_cached_bt_device() {
        return Some(d);
    }
    let devices = discover_paired_devices();
    let found = devices
        .iter()
        .find(|d| d.connected)
        .map(|d| d.path.clone())
        .or_else(|| {
            fs::read_to_string(crate::device::paths::BT_CONFIG_FILE)
                .ok()
                .and_then(|c| default_bt_device(&c))
        })
        .or_else(|| devices.first().map(|d| d.path.clone()))?;
    set_cached_bt_device(&found);
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_managed_objects_finds_paired_with_connected() {
        let dbus_output = "\
object path \"/org/bluez/hci0\"\n\
   dict entry(\n\
      string \"org.bluez.Adapter1\"\n\
      dict entry(\n\
         string \"Powered\"\n\
         variant          boolean true\n\
      )\n\
   )\n\
object path \"/org/bluez/hci0/dev_11_22_33_44_55_66\"\n\
   dict entry(\n\
      string \"org.bluez.Device1\"\n\
      dict entry(\n\
         string \"Paired\"\n\
         variant          boolean true\n\
      )\n\
      dict entry(\n\
         string \"Connected\"\n\
         variant          boolean true\n\
      )\n\
   )\n\
object path \"/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF\"\n\
   dict entry(\n\
      string \"org.bluez.Device1\"\n\
      dict entry(\n\
         string \"Paired\"\n\
         variant          boolean true\n\
      )\n\
      dict entry(\n\
         string \"Connected\"\n\
         variant          boolean false\n\
      )\n\
   )\n";
        let devices = parse_managed_objects(dbus_output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].path, "/org/bluez/hci0/dev_11_22_33_44_55_66");
        assert!(devices[0].connected);
        assert_eq!(devices[1].path, "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF");
        assert!(!devices[1].connected);
    }

    #[test]
    fn parse_managed_objects_skips_unpaired() {
        let dbus_output = "\
object path \"/org/bluez/hci0/dev_FF_EE_DD_CC_BB_AA\"\n\
   dict entry(\n\
      string \"org.bluez.Device1\"\n\
      dict entry(\n\
         string \"Paired\"\n\
         variant          boolean false\n\
      )\n\
      dict entry(\n\
         string \"Connected\"\n\
         variant          boolean true\n\
      )\n\
   )\n";
        let devices = parse_managed_objects(dbus_output);
        assert!(devices.is_empty());
    }

    #[test]
    fn parse_managed_objects_empty_input() {
        assert!(parse_managed_objects("").is_empty());
    }
}
