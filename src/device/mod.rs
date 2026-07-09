//! Device module: hardware database, device I/O, and rendering primitives.
//!
//! - [`config`] — `DeviceConfig`, `SocFamily`, `TouchProtocol`, device lookup
//! - [`registry`] — static `DEVICES` table with all known Kobo models
//! - [`touch`] — evdev→display transform, swipe/double-tap predicates
//! - [`paths`] — filesystem path constants (`/mnt/onboard/.adds`, etc.)
//! - [`input`] — evdev constants, `query_abs_max()`
//! - [`battery`] — `battery_pct()`, capacity parsing
//! - [`wifi`] — link status, toggle, SSID readout
//! - [`bt`] — Bluetooth adapter power, A2DP connect, paired-device discovery
//! - [`clock`] — wall-clock time (shells out to `date`)
//! - [`power`] — `WakeLock`, frontlight get/set, kernel suspend
//! - [`wake`] — power-button monitor, touch-wake polling
//! - [`detect`] — `detect_device()`, `scan_input_devices()`, automagic sysfs
//! - [`fonts`] — font directory scanning, `install_font()` bridge
//! - [`fb`] — framebuffer mmap + MXCFB e-ink refresh
//! - [`traits`] — testable trait surface (`Frontlight`, `Battery`, `Wifi`, etc.)
//! - [`impls`] — production `Sys*` wrappers that delegate to free functions

pub mod battery;
pub mod bt;
pub mod clock;
pub mod config;
pub mod detect;
pub mod fb;
pub mod fonts;
pub mod impls;
pub mod input;
pub mod paths;
pub mod power;
pub mod registry;
pub mod touch;
pub mod traits;
pub mod wake;
pub mod wifi;
