use std::fs;
use std::path::Path;
use std::path::PathBuf;

use log::debug;

const WAKE_LOCK_PATH: &str = "/sys/power/wake_lock";
const WAKE_UNLOCK_PATH: &str = "/sys/power/wake_unlock";
const WAKE_LOCK_NAME: &str = "dev_probe";

pub struct WakeLock {
    held: bool,
}

impl WakeLock {
    pub fn acquire() -> Self {
        // best-effort: sysfs write; wakelock is advisory
        let _ = fs::write(WAKE_LOCK_PATH, WAKE_LOCK_NAME);
        debug!("pwr: wakelock ACQUIRED");
        WakeLock { held: true }
    }
}

impl Drop for WakeLock {
    fn drop(&mut self) {
        if self.held {
            // best-effort: sysfs write on process exit
            let _ = fs::write(WAKE_UNLOCK_PATH, WAKE_LOCK_NAME);
        }
    }
}

/// Pure: the configured brightness-path for a frontlight config.
pub fn known_brightness_path(fl_cfg: &crate::device::config::FrontlightConfig) -> PathBuf {
    PathBuf::from(&fl_cfg.brightness_path)
}

/// Pure: given scanned (path, current_value) candidates, pick the highest-valued
/// entry whose value is > 0, or None. Mirrors the selection in frontlight_path.
pub fn pick_backlight(entries: &[(PathBuf, u32)]) -> Option<PathBuf> {
    entries
        .iter()
        .filter(|(_, v)| *v > 0)
        .max_by_key(|(_, v)| *v)
        .map(|(p, _)| p.clone())
}

pub fn frontlight_path(fl_cfg: &crate::device::config::FrontlightConfig) -> Option<PathBuf> {
    let known = known_brightness_path(fl_cfg);
    if known.exists() {
        return Some(known);
    }
    let mut candidates: Vec<(PathBuf, u32)> = Vec::new();
    for entry in fs::read_dir("/sys/class/backlight").ok()?.flatten() {
        let path = entry.path().join("brightness");
        if path.exists() {
            if let Some(val) = frontlight_get(&path) {
                candidates.push((path, val));
            }
        }
    }
    pick_backlight(&candidates)
}

pub fn frontlight_get(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn frontlight_set(path: &Path, val: u32) {
    // best-effort: sysfs write; backlight driver may be asleep
    let _ = fs::write(path, val.to_string());
}

/// Restore the frontlight to `brightness`, retrying: after a long sleep the I2C
/// bus may need a moment to accept the write. Shared by the book-view and
/// library-view wake paths so both illuminate identically.
pub const FL_RESTORE_RETRIES: u32 = 5;
pub const FL_RESTORE_INTERVAL_MS: u64 = 30;

/// Pure retry-loop: write `brightness`, confirm it took, retry up to `retries`
/// times. Returns the number of `set` calls made (1 on immediate success, up
/// to `retries` on persistent failure). `sleep` is injected so tests run with
/// zero real delay.
pub fn restore_frontlight_with(
    fl: &dyn crate::device::traits::Frontlight,
    brightness: u32,
    retries: u32,
    sleep: impl Fn(),
) -> u32 {
    let mut attempts = 0u32;
    for attempt in 0..retries {
        attempts += 1;
        fl.set(brightness);
        sleep();
        if fl.get() == brightness {
            debug!(
                "pwr: frontlight restored to {} (attempt {})",
                brightness,
                attempt + 1
            );
            break;
        }
        debug!(
            "pwr: frontlight retry {}/{} , got {}",
            attempt + 1,
            retries,
            fl.get()
        );
    }
    attempts
}

struct PathFrontlight<'a> {
    path: &'a Path,
}

impl<'a> crate::device::traits::Frontlight for PathFrontlight<'a> {
    fn set(&self, val: u32) {
        frontlight_set(self.path, val);
    }
    fn get(&self) -> u32 {
        frontlight_get(self.path).unwrap_or(0)
    }
    fn restore(&self, brightness: u32) {
        restore_frontlight(self.path, brightness);
    }
}

pub fn restore_frontlight(path: &Path, brightness: u32) {
    let fl = PathFrontlight { path };
    restore_frontlight_with(&fl, brightness, FL_RESTORE_RETRIES, || {
        std::thread::sleep(std::time::Duration::from_millis(FL_RESTORE_INTERVAL_MS));
    });
}

#[allow(dead_code)]
pub fn kernel_suspend(state: &str) {
    debug!("pwr: entering kernel suspend ({})", state);
    // best-effort: write may fail if the kernel rejects the requested state
    let _ = fs::write("/sys/power/state", state);
    debug!("pwr: resumed from kernel suspend");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::config::FrontlightConfig;

    #[test]
    fn known_brightness_path_returns_configured() {
        let cfg = FrontlightConfig {
            brightness_path: "/sys/class/backlight/lm3630a_led/brightness".into(),
            mixer_path: None,
            nl_min: 0,
            nl_max: 100,
            nl_inverted: false,
        };
        assert_eq!(
            known_brightness_path(&cfg),
            PathBuf::from(&cfg.brightness_path)
        );
    }

    #[test]
    fn pick_backlight_picks_highest_nonzero() {
        let e = [
            (PathBuf::from("/sys/a"), 0),
            (PathBuf::from("/sys/b"), 42),
            (PathBuf::from("/sys/c"), 99),
            (PathBuf::from("/sys/d"), 7),
        ];
        assert_eq!(pick_backlight(&e), Some(PathBuf::from("/sys/c")));
    }

    #[test]
    fn pick_backlight_none_when_all_zero() {
        let e = [(PathBuf::from("/sys/a"), 0), (PathBuf::from("/sys/b"), 0)];
        assert_eq!(pick_backlight(&e), None);
        assert_eq!(pick_backlight(&[]), None);
    }

    use std::cell::{Cell, RefCell};

    /// Mock frontlight: records every `set` value; `get` returns the value at
    /// the current scripted index (repeating the last once exhausted).
    struct MockFl {
        sets: RefCell<Vec<u32>>,
        gets: Cell<usize>,
        scripted: Vec<u32>,
    }

    impl MockFl {
        fn new(gets: Vec<u32>) -> Self {
            MockFl {
                sets: RefCell::new(Vec::new()),
                gets: Cell::new(0),
                scripted: gets,
            }
        }
    }

    impl crate::device::traits::Frontlight for &MockFl {
        fn set(&self, val: u32) {
            self.sets.borrow_mut().push(val);
        }
        fn get(&self) -> u32 {
            let i = self.gets.get();
            let v = self
                .scripted
                .get(i)
                .copied()
                .unwrap_or_else(|| self.scripted.last().copied().unwrap_or(0));
            self.gets.set(i + 1);
            v
        }
        fn restore(&self, brightness: u32) {
            self.set(brightness);
        }
    }

    #[test]
    fn restore_frontlight_succeeds_first_try() {
        let fl = MockFl::new(vec![60]);
        let attempts = restore_frontlight_with(&(&fl), 60, FL_RESTORE_RETRIES, || {});
        assert_eq!(attempts, 1, "converged on the first attempt");
        assert_eq!(*fl.sets.borrow(), vec![60]);
    }

    #[test]
    fn restore_frontlight_retries_until_convergence() {
        let fl = MockFl::new(vec![0, 0, 60]);
        let attempts = restore_frontlight_with(&(&fl), 60, FL_RESTORE_RETRIES, || {});
        assert_eq!(attempts, 3, "two failures then success");
        assert_eq!(
            *fl.sets.borrow(),
            vec![60, 60, 60],
            "set retried each attempt"
        );
    }

    #[test]
    fn restore_frontlight_gives_up_after_max_retries() {
        let fl = MockFl::new(vec![0]);
        let attempts = restore_frontlight_with(&(&fl), 60, FL_RESTORE_RETRIES, || {});
        assert_eq!(
            attempts, FL_RESTORE_RETRIES,
            "never converges -> retries exhausted"
        );
        assert_eq!(fl.sets.borrow().len(), FL_RESTORE_RETRIES as usize);
    }

    #[test]
    fn restore_frontlight_zero_retries_is_single_attempt() {
        let fl = MockFl::new(vec![60]);
        let attempts = restore_frontlight_with(&(&fl), 60, 0, || {});
        assert_eq!(attempts, 0, "zero retries means the loop never runs");
        assert!(fl.sets.borrow().is_empty());
    }
}
