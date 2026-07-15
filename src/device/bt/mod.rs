//! Bluetooth: adapter power, A2DP device connect/disconnect, paired-device
//! discovery, and the friendly name read-out for the panel pill. All bluez /
//! mtk.bluedroid DBus interaction lives here.

use std::fs;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{debug, info, warn};

use crate::device::config::SocFamily;

mod discover;

pub use discover::bt_target_device;
use discover::{
    clear_cached_bt_device, discover_connected_paired_device, discover_paired_devices,
    set_cached_bt_device,
};
pub use discover::PairedDevice;

/// Wall-clock millis of the last user-initiated BT toggle. The UI status refresh
/// uses `bt_toggle_age_ms` to avoid reverting the pill to "off" while an async
/// connect (which can take several seconds + retries) is still in flight.
static LAST_BT_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Millis since the last BT toggle (u64::MAX if never toggled).
pub fn bt_toggle_age_ms() -> u64 {
    let t = LAST_BT_TOGGLE_MS.load(Ordering::Relaxed);
    if t == 0 {
        u64::MAX
    } else {
        now_ms().saturating_sub(t)
    }
}

/// The Bluetooth DBus bus name, fixed once at startup from the device's SoC.
/// MTK Kobos run `com.kobo.mtk.bluedroid`; NXP/sunxi run the standard `org.bluez`.
/// This is set explicitly (not probed) because a runtime probe that calls
/// `dbus_cmd` would recurse `bt_bus -> dbus_cmd -> bt_bus` and deadlock, and a
/// probe against the wrong object path returned the wrong bus on MTK (breaking
/// every BT operation). Matching develop's hard-coded per-platform value is both
/// correct and proven.
static BT_BUS: OnceLock<&'static str> = OnceLock::new();
const BT_BUS_MTK: &str = "com.kobo.mtk.bluedroid";
const BT_BUS_BLUEZ: &str = "org.bluez";

/// Set the BT bus once from the detected SoC family. Called at startup.
pub fn set_bt_bus(soc: SocFamily) {
    let bus = match soc {
        SocFamily::Mtk => BT_BUS_MTK,
        SocFamily::Nxp | SocFamily::Sunxi => BT_BUS_BLUEZ,
    };
    let _ = BT_BUS.set(bus);
    info!("bt: bus = {} ({:?})", bus, soc);
}

/// Returns the active BT bus name. Falls back to the MTK bus if startup hasn't
/// set it yet (all currently-shipping colour/MTK devices).
pub fn bt_bus() -> &'static str {
    BT_BUS.get().copied().unwrap_or(BT_BUS_MTK)
}

/// One-time BT diagnostic at startup: log the bus, whether nickel's BT config
/// exists, and the paired default audio device. This pins down whether a failed
/// toggle is "no paired device configured" vs "connect call failing".
pub fn log_bt_diagnostics() {
    let cfg = fs::read_to_string(crate::device::paths::BT_CONFIG_FILE).unwrap_or_default();
    let cfg_dev = default_bt_device(&cfg);
    let devices = discover_paired_devices();
    debug!(
        "bt diag: bus={}, config_default={:?}, paired={:?}, target={:?}",
        bt_bus(),
        cfg_dev,
        devices
            .iter()
            .map(|d| format!("{}(connected={})", d.path, d.connected))
            .collect::<Vec<_>>(),
        bt_target_device()
    );
}

const DBUS_DEVICE1_IFACE: &str = "string:org.bluez.Device1";
const DBUS_ADAPTER1_IFACE: &str = "string:org.bluez.Adapter1";
const DBUS_PROPS_GET: &str = "org.freedesktop.DBus.Properties.Get";
const DBUS_PROPS_SET: &str = "org.freedesktop.DBus.Properties.Set";
const DBUS_DEVICE1_PATH: &str = "/org/bluez/hci0";
pub(super) const DBUS_OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager.GetManagedObjects";
const DBUS_DEVICE1_CONNECT: &str = "org.bluez.Device1.Connect";
const DBUS_DEVICE1_DISCONNECT: &str = "org.bluez.Device1.Disconnect";

/// Returns a `dbus-send` Command pre-configured with the detected BT bus name.
pub(super) fn dbus_cmd() -> Command {
    let dest = format!("--dest={}", bt_bus());
    let mut cmd = Command::new("dbus-send");
    cmd.args(["--system", "--print-reply", "--type=method_call"]);
    cmd.arg(dest);
    cmd
}

/// True when the given bluez Device1 path reports `Connected` (the ACL link is
/// up). Shared by [`bt_status`] and [`bt_name`] so both agree on connection.
fn device_connected(dev: &str) -> bool {
    let out = dbus_cmd()
        .args([dev, DBUS_PROPS_GET, DBUS_DEVICE1_IFACE, "string:Connected"])
        .output();
    match out {
        Ok(o) if o.status.success() => dbus_connected(&String::from_utf8_lossy(&o.stdout)),
        _ => false,
    }
}

pub fn bt_status() -> bool {
    let dev = match bt_target_device() {
        Some(d) => d,
        None => return false,
    };
    if device_connected(&dev) {
        return true;
    }
    if let Some(connected) = discover_connected_paired_device() {
        if connected != dev {
            info!(
                "bt: switching target {} -> {} (already connected by btservice)",
                dev, connected
            );
            set_cached_bt_device(&connected);
            return true;
        }
    }
    false
}

pub fn default_bt_device(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|l| {
            l.strip_prefix("DefaultAudioDevice=")
                .map(|s| s.trim().to_string())
        })
        .filter(|d| !d.is_empty())
}

pub fn dbus_connected(text: &str) -> bool {
    text.contains("boolean true")
}

pub fn bt_toggle(on: bool) {
    // Stamp the toggle so the UI status refresh doesn't revert the pill while
    // the (async, multi-retry) connect is still settling.
    LAST_BT_TOGGLE_MS.store(now_ms(), Ordering::Relaxed);
    if on {
        // Power the adapter up SYNCHRONOUSLY (matches develop exactly). The Set
        // Powered call is fast and never hangs, and doing it on the caller's
        // thread means the adapter is already on before reconnect runs - which is
        // what made develop's single ON tap connect reliably.
        if let Err(e) = dbus_cmd()
            .args([
                DBUS_DEVICE1_PATH,
                DBUS_PROPS_SET,
                DBUS_ADAPTER1_IFACE,
                "string:Powered",
                "variant:boolean:true",
            ])
            .status()
        {
            warn!("bt: adapter power-on failed: {e}");
        }
        info!("bt: adapter powered on (bus={})", bt_bus());
        // Reconnect (Connect can retry for several seconds) off the main loop.
        let _ = std::thread::spawn(reconnect_bt);
        info!("bt: turned ON + reconnecting");
    } else {
        // Power-down path: a Device1.Disconnect can block indefinitely when the
        // configured speaker isn't actually linked, and callers (sleep entry,
        // panel toggle) run on the main loop - so run the whole thing off-thread
        // to never freeze the UI on any device.
        std::thread::spawn(move || {
            if let Some(dev) = bt_target_device() {
                if let Err(e) = dbus_cmd()
                    .args([&dev, DBUS_DEVICE1_DISCONNECT])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                {
                    warn!("bt: disconnect {dev} failed: {e}");
                }
                debug!("bt: disconnecting {dev}");
            }
            clear_cached_bt_device();
            if let Err(e) = dbus_cmd()
                .args([
                    DBUS_DEVICE1_PATH,
                    DBUS_PROPS_SET,
                    DBUS_ADAPTER1_IFACE,
                    "string:Powered",
                    "variant:boolean:false",
                ])
                .status()
            {
                warn!("bt: adapter power-off failed: {e}");
            }
            info!("bt: turned OFF (adapter powered down)");
        });
    }
}

pub fn reconnect_bt() {
    // Resolve the target device: configured default, else a paired device
    // discovered on the adapter (the config key is absent on newer firmware /
    // after a factory reset). Without this, KoThok had nothing to connect to.
    let dev = match bt_target_device() {
        Some(d) => d,
        None => {
            info!("reconnect_bt: no paired device found on adapter, nothing to connect");
            return;
        }
    };
    info!("reconnect_bt: connecting {dev} on bus={}", bt_bus());
    // The adapter was just powered on; the first Connect issued immediately
    // after power-up frequently fails (the stack needs a moment to settle). A
    // short lead delay plus a bounded retry loop makes a single ON tap reliably
    // connect, instead of forcing the user to tap repeatedly.
    std::thread::sleep(std::time::Duration::from_millis(800));
    for attempt in 1..=6 {
        let rc = dbus_cmd()
            .args([&dev, DBUS_DEVICE1_CONNECT])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let connected = bt_status();
        debug!(
            "reconnect_bt: attempt {attempt} rc={:?} connected={}",
            rc.as_ref().ok().and_then(|s| s.code()),
            connected
        );
        if connected {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    info!("reconnect_bt: gave up after 6 attempts ({dev})");
}

pub fn bt_name() -> Option<String> {
    if !bt_status() {
        return None;
    }
    let dev = bt_target_device()?;
    let out = dbus_cmd()
        .args([&dev, DBUS_PROPS_GET, DBUS_DEVICE1_IFACE, "string:Alias"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_dbus_string(&text).or_else(|| {
        // Some stacks expose "Name" rather than "Alias".
        let out2 = dbus_cmd()
            .args([&dev, DBUS_PROPS_GET, DBUS_DEVICE1_IFACE, "string:Name"])
            .output()
            .ok()?;
        parse_dbus_string(&String::from_utf8_lossy(&out2.stdout))
    })
}

pub fn parse_dbus_string(text: &str) -> Option<String> {
    let idx = text.find("string \"")?;
    let rest = &text[idx + 8..];
    let end = rest.find('"')?;
    let name = &rest[..end];
    (!name.is_empty()).then(|| name.to_string())
}

pub struct BtDeviceInfo {
    pub path: String,
    pub name: String,
    pub connected: bool,
}

fn device_alias(dev: &str) -> Option<String> {
    let out = dbus_cmd()
        .args([dev, DBUS_PROPS_GET, DBUS_DEVICE1_IFACE, "string:Alias"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_dbus_string(&text).or_else(|| {
        let out2 = dbus_cmd()
            .args([dev, DBUS_PROPS_GET, DBUS_DEVICE1_IFACE, "string:Name"])
            .output()
            .ok()?;
        parse_dbus_string(&String::from_utf8_lossy(&out2.stdout))
    })
}

pub fn bt_list_devices() -> Vec<BtDeviceInfo> {
    let paired = discover_paired_devices();
    paired
        .into_iter()
        .map(|d| {
            let name = device_alias(&d.path).unwrap_or_else(|| {
                d.path
                    .rsplit('/')
                    .next()
                    .unwrap_or("Unknown")
                    .replace('_', ":")
            });
            BtDeviceInfo {
                name,
                connected: d.connected,
                path: d.path,
            }
        })
        .collect()
}

pub fn bt_connect_device(path: &str) {
    info!("bt: switching to device {path}");
    if let Some(current) = bt_target_device() {
        if current != path {
            let _ = dbus_cmd()
                .args([&current, DBUS_DEVICE1_DISCONNECT])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            debug!("bt: disconnected {current}");
        }
    }
    set_cached_bt_device(path);
    let path = path.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        for attempt in 1..=4 {
            let rc = dbus_cmd()
                .args([&path, DBUS_DEVICE1_CONNECT])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let connected = device_connected(&path);
            debug!("bt_connect_device: attempt {attempt} rc={:?} connected={}", rc.as_ref().ok().and_then(|s| s.code()), connected);
            if connected {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        info!("bt_connect_device: gave up after 4 attempts ({path})");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bt_device_extracts_first_entry() {
        let cfg = "[Audio]\nDefaultAudioDevice=/org/bluez/hci0/dev_AA_BB\nFoo=bar\n";
        assert_eq!(
            default_bt_device(cfg).as_deref(),
            Some("/org/bluez/hci0/dev_AA_BB")
        );
    }

    #[test]
    fn default_bt_device_ignores_empty_value() {
        assert_eq!(default_bt_device("DefaultAudioDevice=\n"), None);
        assert_eq!(default_bt_device("DefaultAudioDevice=   \n"), None);
        assert_eq!(default_bt_device("[Section]\n"), None);
    }

    #[test]
    fn dbus_connected_detects_true() {
        assert!(dbus_connected("   variant   boolean true\n"));
        assert!(!dbus_connected("   variant   boolean false\n"));
        assert!(!dbus_connected(""));
    }

    #[test]
    fn parse_dbus_string_extracts_value() {
        let out = "   variant       string \"JBL Flip\"\n";
        assert_eq!(parse_dbus_string(out).as_deref(), Some("JBL Flip"));
    }

    #[test]
    fn parse_dbus_string_none_when_empty_or_absent() {
        assert_eq!(parse_dbus_string("string \"\"\n"), None);
        assert_eq!(parse_dbus_string("no string field"), None);
    }
}
