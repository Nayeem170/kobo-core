//! WiFi: link status (operstate + carrier), on/off toggle (wpa_supplicant +
//! dhcpcd), and the connected SSID read-out for the panel pill.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
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
const WMT_DBG_PATH: &str = "/proc/driver/wmt_dbg";
const WMT_WIFI_DEV: &str = "/dev/wmtWifi";
const WMT_UNLOCK_TOKEN: &str = "0xDB9DB9";
const WMT_FUNC_OFF: &str = "7 9 0";
const WMT_FUNC_ON: &str = "7 9 1";
const WMT_WIFI_POWER_ON: &str = "1";
const WMT_SETTLE_MS: u64 = 1000;
const RFKILL_SOFT_PATH: &str = "/sys/class/rfkill/rfkill0/soft";
const IFACE_UP_SETTLE_MS: u64 = 1000;
const CARRIER_POLL_MS: u64 = 500;
const CARRIER_POLL_MAX: usize = 30;
const WPA_DRV_NL80211: &str = "nl80211";
const WPA_DRV_WEXT: &str = "wext";
const WPA_CTRL_DEFAULT: &str = "/var/run/wpa_supplicant";
const WPA_SUPPLICANT_CANDIDATES: &[&str] = &[
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

static WPA_CTRL: OnceLock<Option<String>> = OnceLock::new();
static WPA_DRV: OnceLock<String> = OnceLock::new();

fn read_wpa_cmdline() -> Vec<String> {
    let out = match Command::new("pgrep").args(["-x", "wpa_supplicant"]).output() {
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
        let ctrl = args.iter().position(|a| a == "-C")
            .and_then(|i| args.get(i + 1))
            .map(|s| {
                info!("wifi: ctrl socket from wpa_supplicant: {s}");
                s.clone()
            });
        ctrl
    });
    WPA_DRV.get_or_init(|| {
        let args = read_wpa_cmdline();
        let drv = args.iter().position(|a| a == "-D")
            .and_then(|i| args.get(i + 1))
            .map(|s| {
                info!("wifi: driver from wpa_supplicant: {s}");
                s.clone()
            })
            .unwrap_or_else(|| {
                if is_mtk_platform() { WPA_DRV_NL80211.into() } else { WPA_DRV_WEXT.into() }
            });
        drv
    });
}

fn wpa_cli(args: &[&str]) -> Option<std::process::Output> {
    init_wpa_detection();
    let detected = WPA_CTRL.get().and_then(|o| o.as_ref());
    let candidates: Vec<Option<&str>> = vec![
        detected.map(|s| s.as_str()),
        Some(WPA_CTRL_DEFAULT),
        None,
    ];
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
                    info!("wpa_cli {:?}: ctrl={label} rc={rc} out={}", args, stdout.trim());
                    continue;
                }
                info!("wpa_cli {:?}: ctrl={label} rc=0 out={}", args, stdout.trim());
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

fn find_wpa_binary() -> Option<&'static str> {
    for p in WPA_SUPPLICANT_CANDIDATES {
        if std::path::Path::new(p).exists() || Command::new("which").arg(p).output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}

fn is_mtk_platform() -> bool {
    std::path::Path::new(WMT_WIFI_DEV).exists() || std::path::Path::new(WMT_DBG_PATH).exists()
}

fn power_on_mtk_wmt() -> bool {
    if !std::path::Path::new(WMT_DBG_PATH).exists() {
        warn!("wifi: wmt_dbg missing - radio never initialized this boot");
        warn!("wifi: cold-boot WMT power-on not yet supported, open nickel WiFi once first");
        return false;
    }
    let w = |val: &str| match fs::write(WMT_DBG_PATH, val) {
        Ok(_) => true,
        Err(e) => {
            warn!("wifi: wmt_dbg write '{val}' failed: {e}");
            false
        }
    };
    info!("wifi: MTK WMT power-on sequence starting");
    if !w(WMT_UNLOCK_TOKEN) { return false; }
    if !w(WMT_FUNC_OFF) { return false; }
    std::thread::sleep(std::time::Duration::from_millis(WMT_SETTLE_MS));
    if !w(WMT_UNLOCK_TOKEN) { return false; }
    if !w(WMT_FUNC_ON) { return false; }
    std::thread::sleep(std::time::Duration::from_millis(500));
    match fs::write(WMT_WIFI_DEV, WMT_WIFI_POWER_ON) {
        Ok(_) => {
            info!("wifi: MTK WMT radio powered on");
            true
        }
        Err(e) => {
            warn!("wifi: wmtWifi power-on failed: {e}");
            false
        }
    }
}

fn wifi_turn_on() {
    if is_mtk_platform() {
        info!("wifi: MTK platform detected");
        power_on_mtk_wmt();
        std::thread::sleep(std::time::Duration::from_millis(500));
    } else {
        info!("wifi: NXP platform, using rfkill");
        // best-effort: rfkill may not exist on all models
        if let Err(e) = fs::write(RFKILL_SOFT_PATH, "0") {
            warn!("wifi: rfkill write failed: {e}");
        }
        let _ = Command::new("rfkill").args(["unblock", "wifi"]).status();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let ifconfig_ok = Command::new("ifconfig").args([WLAN_IFACE, "up"]).status().map(|s| s.success()).unwrap_or(false);
    if !ifconfig_ok {
        info!("wifi: ifconfig failed, trying ip link");
        // best-effort: ip may not be installed
        let _ = Command::new("ip").args(["link", "set", WLAN_IFACE, "up"]).status();
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
        let _ = wpa_cli(&["reconfigure"]);
    } else {
        let conf = find_wpa_conf();
        let bin = find_wpa_binary();
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
                        warn!("wifi: wpa_supplicant (-D {driver}) failed: {}", stderr.trim());
                        info!("wifi: retrying without -D flag");
                        // best-effort: fallback without driver flag
                        let _ = Command::new(wpa_bin)
                            .args(["-B", "-i", WLAN_IFACE, "-c", &conf_path])
                            .status();
                    }
                    Err(e) => {
                        warn!("wifi: wpa_supplicant exec error: {e}");
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(IFACE_UP_SETTLE_MS));
            }
            (None, _) => {
                warn!("wifi: no wpa_supplicant.conf found");
            }
            (_, None) => {
                warn!("wifi: wpa_supplicant binary not found");
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
            info!("wifi: poll {i} carrier={} operstate={}", carrier.trim(), opers.trim());
        }
        std::thread::sleep(std::time::Duration::from_millis(CARRIER_POLL_MS));
    }
    if !got_carrier {
        warn!("wifi: no carrier after 15s");
        if let Some(o) = wpa_cli(&["status"]) {
            let status = String::from_utf8_lossy(&o.stdout);
            warn!("wifi: wpa status: {}", status.trim());
        }
    }

    if got_carrier {
        let dhcp_ok = Command::new("dhcpcd").args([WLAN_IFACE]).status().map(|s| s.success()).unwrap_or(false);
        if !dhcp_ok {
            info!("wifi: dhcpcd failed, trying udhcpc");
            // best-effort: udhcpc may not exist
            let _ = Command::new("udhcpc").args(["-i", WLAN_IFACE, "-q"]).status();
        }
    }
    info!("wifi: turned ON (carrier={got_carrier})");
}

fn wifi_turn_off() {
    let _ = Command::new("killall").args(["-q"]).args(WIFI_DAEMONS).status();
    let _ = wpa_cli(&["disconnect"]);
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

fn parse_wpa_networks(text: &str) -> Vec<WifiNetwork> {
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
    let _ = wpa_cli(&["select_network", &id_str]);
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

fn parse_conf_ssids(text: &str) -> Vec<String> {
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
