// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Battery: capacity read from `/sys/class/power_supply`, with type checking
//! so a USB/Mains supply isn't mistaken for the battery.

use std::fs;

pub fn battery_pct() -> i32 {
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let dir = entry.path();
            let is_battery = fs::read_to_string(dir.join("type"))
                .map(|t| is_battery_type(&t))
                .unwrap_or(true);
            if !is_battery {
                continue;
            }
            if let Ok(s) = fs::read_to_string(dir.join("capacity")) {
                if let Some(v) = parse_capacity(&s) {
                    return v;
                }
            }
        }
    }
    0
}

pub fn is_battery_type(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("Battery")
}

pub fn parse_capacity(s: &str) -> Option<i32> {
    let v = s.trim().parse::<i32>().ok()?;
    (0..=100).contains(&v).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_clamps_to_valid_range() {
        assert_eq!(parse_capacity("75\n"), Some(75));
        assert_eq!(parse_capacity("0"), Some(0));
        assert_eq!(parse_capacity("100"), Some(100));
        assert_eq!(parse_capacity("150"), None, "over 100 rejected");
        assert_eq!(parse_capacity("-5"), None, "negative rejected");
        assert_eq!(parse_capacity("abc"), None, "non-numeric rejected");
    }

    #[test]
    fn battery_type_matches_case_insensitive() {
        assert!(is_battery_type("Battery"));
        assert!(is_battery_type("battery\n"));
        assert!(is_battery_type("  BATTERY  "));
        assert!(!is_battery_type("Mains"));
    }
}
