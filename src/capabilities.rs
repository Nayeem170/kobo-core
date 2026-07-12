//! Capability detection (plan S5): drives the read-aloud vs plain-reader state
//! machine. `MockCapabilities` is for the desktop simulator; the real impl
//! (`KoboCapabilities`) lives in the `kobo` backend crate (checks network reach
//! + A2DP sink presence).

/// Device-state query surface. Implemented concretely by `KoboCapabilities`
/// (reads sysfs/dbus) and by `MockCapabilities` (scripted for desktop). Routing
/// device queries through this trait lets the reader's status-refresh and
/// play-enabled decisions be unit-tested without hardware.
pub trait Capabilities {
    fn network_available(&self) -> bool;
    fn audio_sink_available(&self) -> bool;

    fn read_aloud_available(&self) -> bool {
        self.network_available() && self.audio_sink_available()
    }

    fn battery_pct(&self) -> i32 {
        0
    }
    fn wifi_name(&self) -> Option<String> {
        None
    }
    fn bt_name(&self) -> Option<String> {
        None
    }
    fn current_clock(&self) -> String {
        "--:--".to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockCapabilities {
    pub network: bool,
    pub audio_sink: bool,
    pub battery: i32,
    pub wifi_ssid: Option<String>,
    pub bt_device: Option<String>,
    pub clock: String,
}

impl Capabilities for MockCapabilities {
    fn network_available(&self) -> bool {
        self.network
    }
    fn audio_sink_available(&self) -> bool {
        self.audio_sink
    }
    fn battery_pct(&self) -> i32 {
        self.battery
    }
    fn wifi_name(&self) -> Option<String> {
        self.wifi_ssid.clone()
    }
    fn bt_name(&self) -> Option<String> {
        self.bt_device.clone()
    }
    fn current_clock(&self) -> String {
        if self.clock.is_empty() {
            "--:--".to_string()
        } else {
            self.clock.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine() {
        assert!(MockCapabilities {
            network: true,
            audio_sink: true,
            ..Default::default()
        }
        .read_aloud_available());
        assert!(!MockCapabilities {
            network: false,
            audio_sink: true,
            ..Default::default()
        }
        .read_aloud_available());
        assert!(!MockCapabilities {
            network: true,
            audio_sink: false,
            ..Default::default()
        }
        .read_aloud_available());
        assert!(!MockCapabilities {
            network: false,
            audio_sink: false,
            ..Default::default()
        }
        .read_aloud_available());
    }

    #[test]
    fn mock_battery_and_names_round_trip() {
        let mut m = MockCapabilities::default();
        m.battery = 73;
        m.wifi_ssid = Some("Net".into());
        m.bt_device = Some("Speaker".into());
        m.clock = "9:41 PM".into();
        assert_eq!(m.battery_pct(), 73);
        assert_eq!(m.wifi_name().as_deref(), Some("Net"));
        assert_eq!(m.bt_name().as_deref(), Some("Speaker"));
        assert_eq!(m.current_clock(), "9:41 PM");
    }

    #[test]
    fn mock_defaults_are_benign() {
        let m = MockCapabilities::default();
        assert_eq!(m.battery_pct(), 0);
        assert!(m.wifi_name().is_none());
        assert!(m.bt_name().is_none());
        assert_eq!(m.current_clock(), "--:--");
    }
}
