//! Wall-clock time for the on-screen clock (shells out to `date` for the
//! locale-correct 12-hour format).

use std::process::Command;

pub fn current_clock() -> String {
    Command::new("date")
        .args(["+%-I:%M %p"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "--:--".to_string())
}
