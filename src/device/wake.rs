use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{debug, info, warn};

use crate::device::input::EV_KEY;

// A single physical power press on the Kobo emits repeated EV_KEY press (val=1)
// events. Without debouncing, those bounce events toggle sleep/wake in rapid
// succession - the multi-frame cover<->book flicker on wake. Ignore presses
// within this window of the last accepted press.
const POWER_DEBOUNCE_MS: u64 = 300;

pub fn spawn_power_monitor(pressed: Arc<AtomicBool>, exit: Arc<AtomicBool>, power_dev: &str) {
    let power_dev = power_dev.to_string();
    std::thread::spawn(move || {
        use std::os::unix::io::AsRawFd;
        info!("power: thread started on {} (blocking)", power_dev);
        'outer: loop {
            if exit.load(Ordering::SeqCst) {
                break;
            }
            let mut dev = match std::fs::OpenOptions::new().read(true).open(&power_dev) {
                Ok(d) => d,
                Err(e) => {
                    warn!("power: open {} failed: {} (retry in 500ms)", power_dev, e);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };
            let fd = dev.as_raw_fd();
            // SAFETY: fd is a valid, owned File descriptor (dev is alive on this stack frame,
            // not closed). F_GETFL/F_SETFL take/return an int flags value; clearing O_NONBLOCK
            // on our own fd is sound and only affects this descriptor's blocking mode.
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
            let mut buf = [0u8; 24];
            let mut last_press: Option<std::time::Instant> = None;
            // Track key down/up state. On most kernels the power key emits val=1
            // (press) then val=0 (release); state-tracking gives a clean single
            // press. Some Kobo GPIO drivers only emit val=1 with autorepeat, so a
            // time-based debounce remains as a backstop.
            let mut key_down = false;
            loop {
                if exit.load(Ordering::SeqCst) {
                    drop(dev);
                    break 'outer;
                }
                match dev.read(&mut buf) {
                    Ok(n) if n >= 16 => {
                        let typ = u16::from_le_bytes([buf[8], buf[9]]);
                        let code = u16::from_le_bytes([buf[10], buf[11]]);
                        let val = i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
                        if typ == EV_KEY {
                            debug!(
                                "power: raw event typ={} code={} val={} n={} down={}",
                                typ, code, val, n, key_down
                            );
                        }
                        if typ == EV_KEY {
                            if val == 0 {
                                key_down = false;
                            } else if val == 1 {
                                let now = std::time::Instant::now();
                                let since_last =
                                    last_press.map(|t| now.duration_since(t).as_millis());
                                let debounce_ok = since_last
                                    .map(|ms| ms >= POWER_DEBOUNCE_MS as u128)
                                    .unwrap_or(true);
                                if !key_down || debounce_ok {
                                    if key_down {
                                        info!(
                                            "power: press accepted via debounce ({}ms since last)",
                                            since_last.unwrap_or(0)
                                        );
                                    } else {
                                        info!("power: press accepted (clean key-down)");
                                    }
                                    pressed.store(true, Ordering::SeqCst);
                                    last_press = Some(now);
                                } else {
                                    debug!(
                                        "power: press suppressed (held, {}ms < {}ms debounce)",
                                        since_last.unwrap_or(0),
                                        POWER_DEBOUNCE_MS
                                    );
                                }
                                key_down = true;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue 'outer;
                    }
                }
            }
        }
    });
}

/// Pure decision: does a press->release pair count as a wake swipe-up?
/// Extracted from poll_touch_for_wake so the threshold is unit-testable (S11).
/// Requires a long, deliberate swipe: it must start in the bottom ~40% of the
/// screen, travel upward at least ~40% of the screen height, and be more
/// vertical than horizontal - so a short accidental flick won't unlock.
fn is_wake_swipe(
    press_x: f32,
    press_y: f32,
    release_x: f32,
    release_y: f32,
    screen_h: f32,
) -> bool {
    const START_BAND: f32 = 0.6;
    const MIN_SWIPE_FRAC: f32 = 0.40;
    let dy = press_y - release_y;
    let dx = (release_x - press_x).abs();
    press_y > screen_h * START_BAND && dy > screen_h * MIN_SWIPE_FRAC && dy > dx
}

pub fn poll_touch_for_wake(
    touch_dev: &mut std::fs::File,
    touch_fd: libc::c_int,
    pressed: Arc<AtomicBool>,
    cfg: &crate::device::touch::TouchConfig,
) {
    use crate::device::input::{
        ABS_MT_POSITION_X, ABS_MT_POSITION_Y, BTN_TOUCH_CODE, EV_ABS, EV_KEY, EV_SYN, SYN_REPORT,
    };
    use crate::device::touch::to_display;

    info!(
        "WAKE: poll_touch_for_wake started (screen {}x{}, switch={}, mirror=({},{}))",
        cfg.screen_w, cfg.screen_h, cfg.switch_xy, cfg.mirrored_x, cfg.mirrored_y
    );
    let screen_h = cfg.screen_h as f32;
    let mut touch_rec: [u8; 16] = [0; 16];
    let mut frame_x: i32 = 0;
    let mut frame_y: i32 = 0;
    let mut frame_down = false;
    let mut prev_down = false;
    let mut press_raw: Option<(i32, i32)> = None;
    loop {
        if pressed.load(Ordering::SeqCst) {
            info!("WAKE (power button)");
            break;
        }
        let mut pfd = libc::pollfd {
            fd: touch_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll takes a single initialized `pollfd` pointing at our stack `pfd` and a
        // valid touch_fd (caller-owned, nonblocking). It only writes `revents` back into pfd
        // (no other aliasing) and returns an int count; 500ms timeout bounds the wait.
        let n = unsafe { libc::poll(&mut pfd, 1, 500) };
        if n > 0 {
            while touch_dev.read(&mut touch_rec).is_ok() {
                let typ = u16::from_le_bytes([touch_rec[8], touch_rec[9]]);
                let code = u16::from_le_bytes([touch_rec[10], touch_rec[11]]);
                let val = i32::from_le_bytes([
                    touch_rec[12],
                    touch_rec[13],
                    touch_rec[14],
                    touch_rec[15],
                ]);
                match (typ, code) {
                    (EV_KEY, BTN_TOUCH_CODE) => frame_down = val == 1,
                    (EV_ABS, ABS_MT_POSITION_X) => frame_x = val,
                    (EV_ABS, ABS_MT_POSITION_Y) => frame_y = val,
                    (EV_SYN, SYN_REPORT) => {
                        if frame_down && !prev_down {
                            press_raw = Some((frame_x, frame_y));
                        }
                        if !frame_down && prev_down {
                            if let Some(pr) = press_raw {
                                let (press_x, press_y) = to_display(pr.0, pr.1, cfg);
                                let (release_x, release_y) = to_display(frame_x, frame_y, cfg);
                                if is_wake_swipe(press_x, press_y, release_x, release_y, screen_h) {
                                    info!(
                                        "WAKE: swipe-up press=({:.0},{:.0}) release=({:.0},{:.0}) dy={:.0}",
                                        press_x, press_y, release_x, release_y, press_y - release_y
                                    );
                                    pressed.store(true, Ordering::SeqCst);
                                    info!("WAKE (swipe-up from bottom)");
                                    break;
                                }
                            }
                            press_raw = None;
                        }
                        prev_down = frame_down;
                    }
                    _ => {}
                }
            }
            if pressed.load(Ordering::SeqCst) {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN_H: f32 = 1448.0;

    #[test]
    fn wake_swipe_long_upward_from_bottom_wakes() {
        assert!(is_wake_swipe(500.0, 1300.0, 500.0, 600.0, SCREEN_H));
    }

    #[test]
    fn wake_swipe_short_flick_does_not_wake() {
        assert!(!is_wake_swipe(500.0, 1300.0, 500.0, 1150.0, SCREEN_H));
    }

    #[test]
    fn wake_swipe_barely_meeting_threshold_wakes() {
        assert!(is_wake_swipe(500.0, 1300.0, 500.0, 700.0, SCREEN_H));
    }

    #[test]
    fn wake_swipe_starting_too_high_does_not_wake() {
        assert!(!is_wake_swipe(500.0, 400.0, 500.0, 100.0, SCREEN_H));
    }

    #[test]
    fn wake_swipe_horizontal_does_not_wake() {
        assert!(!is_wake_swipe(100.0, 1300.0, 1100.0, 1000.0, SCREEN_H));
    }

    #[test]
    fn wake_swipe_downward_does_not_wake() {
        assert!(!is_wake_swipe(500.0, 900.0, 500.0, 1300.0, SCREEN_H));
    }
}
