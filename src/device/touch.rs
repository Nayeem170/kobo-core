//! Touch→display transform + gesture predicates.
//!
//! The transform uses per-device flags from `DeviceConfig`:
//! - `touch_switch_xy`: swap X and Y axes
//! - `touch_mirrored_x`: mirror the horizontal axis
//! - `touch_mirrored_y`: mirror the vertical axis

pub const SWIPE_MIN_DX: f32 = 80.0;
pub const SWIPE_MAX_MS: u128 = 500;
pub const DOUBLE_TAP_MAX_PRESS_MS: u128 = 300;
pub const DOUBLE_TAP_WINDOW_MS: u128 = 400;

/// Per-device touch configuration.
#[derive(Clone, Copy)]
pub struct TouchConfig {
    pub switch_xy: bool,
    pub mirrored_x: bool,
    pub mirrored_y: bool,
    pub raw_x_max: i32,
    pub raw_y_max: i32,
    pub screen_w: i32,
    pub screen_h: i32,
}

/// Raw evdev → display-space transform.
pub fn to_display(rx: i32, ry: i32, cfg: &TouchConfig) -> (f32, f32) {
    let (mut x, mut y) = if cfg.switch_xy { (ry, rx) } else { (rx, ry) };

    if cfg.mirrored_x {
        let max = if cfg.switch_xy {
            cfg.raw_y_max
        } else {
            cfg.raw_x_max
        };
        x = max - x;
    }
    if cfg.mirrored_y {
        let max = if cfg.switch_xy {
            cfg.raw_x_max
        } else {
            cfg.raw_y_max
        };
        y = max - y;
    }

    (x as f32, y as f32)
}

#[allow(dead_code)]
pub fn is_swipe(swipe_dx: f32, swipe_dy: f32, dt_ms: u128) -> bool {
    swipe_dx.abs() > SWIPE_MIN_DX && dt_ms < SWIPE_MAX_MS && swipe_dx.abs() > swipe_dy.abs()
}

pub fn is_double_tap(dt_ms: u128, since_prev_tap_ms: u128) -> bool {
    dt_ms < DOUBLE_TAP_MAX_PRESS_MS && since_prev_tap_ms < DOUBLE_TAP_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    // Libra Colour cyttsp5_mt: switch_xy=true, mirrored_x=true
    fn libra_cfg() -> TouchConfig {
        TouchConfig {
            switch_xy: true,
            mirrored_x: true,
            mirrored_y: false,
            raw_x_max: 1447,
            raw_y_max: 1071,
            screen_w: 1072,
            screen_h: 1448,
        }
    }

    // Standard touch panel: no swap, no mirror
    fn plain_cfg() -> TouchConfig {
        TouchConfig {
            switch_xy: false,
            mirrored_x: false,
            mirrored_y: false,
            raw_x_max: 1071,
            raw_y_max: 1447,
            screen_w: 1072,
            screen_h: 1448,
        }
    }

    #[test]
    fn libra_left_edge_maps_small_x() {
        let cfg = libra_cfg();
        let (dx, _) = to_display(736, 1035, &cfg);
        assert!(
            dx < 100.0,
            "left-edge tap must map to small display_x, got {dx}"
        );
    }

    #[test]
    fn libra_right_edge_maps_large_x() {
        let cfg = libra_cfg();
        let (dx, _) = to_display(693, 36, &cfg);
        assert!(
            dx > 970.0,
            "right-edge tap must map to large display_x, got {dx}"
        );
    }

    #[test]
    fn plain_passes_through() {
        let cfg = plain_cfg();
        let (dx, dy) = to_display(100, 200, &cfg);
        assert_eq!(dx, 100.0);
        assert_eq!(dy, 200.0);
    }

    #[test]
    fn plain_mirrored_x() {
        let mut cfg = plain_cfg();
        cfg.mirrored_x = true;
        let (dx, _) = to_display(0, 0, &cfg);
        assert_eq!(dx, cfg.raw_x_max as f32);
    }

    #[test]
    fn is_swipe_detects_fast_horizontal_flick() {
        assert!(is_swipe(200.0, 5.0, 120));
        assert!(is_swipe(-250.0, 40.0, 200));
    }

    #[test]
    fn is_swipe_rejects_small_or_slow_or_vertical() {
        assert!(!is_swipe(20.0, 5.0, 300));
        assert!(!is_swipe(200.0, 5.0, 600));
        assert!(!is_swipe(100.0, 300.0, 100));
    }

    #[test]
    fn is_double_tap_within_window() {
        assert!(is_double_tap(150, 200));
        assert!(!is_double_tap(400, 200));
        assert!(!is_double_tap(150, 500));
    }
}
