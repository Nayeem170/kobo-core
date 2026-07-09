//! Filesystem path constants for Kobo devices.

pub const ADDS_DIR: &str = "/mnt/onboard/.adds";
pub const CONFIG_FILE: &str = "/mnt/onboard/.adds/config";
pub const CRASH_LOG: &str = "/mnt/onboard/.adds/crash.log";
pub const PPM_DEBUG: &str = "/tmp/kobo-reader.ppm";
pub const PPM_DEPLOY: &str = "/mnt/onboard/.adds/kobo-reader.ppm";
pub const TOUCH_DEV: &str = "/dev/input/event1";
pub const POWER_DEV: &str = "/dev/input/event2";
pub const BT_CONFIG_FILE: &str = "/mnt/onboard/.kobo/Kobo/Kobo eReader.conf";
#[allow(dead_code)]
pub const EDGE_DEBUG_LOG: &str = "/mnt/onboard/.adds/edge_debug.log";
