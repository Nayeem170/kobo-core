//! Loading indicators for splash and loading screens.
//!
//! Pure functions operating on raw RGB565 byte buffers. All take explicit
//! screen dimensions so they are reusable by any crate without global state.
//!
//! - [`paint_loading_bar`] - horizontal progress bar at the bottom of the screen
//! - [`paint_spinner`] - rotating arc spinner centred near the bottom

const BAR_SIDE_PAD: usize = 80;
const BAR_HEIGHT: usize = 8;
const BAR_FILL_COLOR: u16 = 0x06A4;
const BAR_TRACK_COLOR: u16 = 0xD6BA;

const SPINNER_OFFSET_FROM_BOTTOM: f32 = 130.0;
const SPINNER_BADGE_R: f32 = 52.0;
const SPINNER_ARC_R: f32 = 36.0;
const SPINNER_ARC_THICK: f32 = 9.0;
const SPINNER_ARC_SPAN: u32 = 300;
const SPINNER_PAD: i32 = 56;

fn put_pixel(buf: &mut [u8], screen_w: usize, px: usize, py: usize, val: u16) {
    let off = (py * screen_w + px) * 2;
    if off + 2 > buf.len() {
        return;
    }
    buf[off] = (val & 0xff) as u8;
    buf[off + 1] = (val >> 8) as u8;
}

/// Draw a horizontal progress bar at the bottom of the screen.
///
/// `frac` ranges from 0.0 (empty) to 1.0 (full). The filled portion uses
/// [`BAR_FILL_COLOR`]; the remaining track uses [`BAR_TRACK_COLOR`].
pub fn paint_loading_bar(buf: &mut [u8], screen_w: usize, screen_h: usize, frac: f32) {
    let bar_w = screen_w.saturating_sub(2 * BAR_SIDE_PAD);
    let bar_y = screen_h - screen_h / 10;
    let fill_w = ((bar_w as f32) * frac.clamp(0.0, 1.0)).round() as usize;
    for ry in 0..BAR_HEIGHT {
        let py = bar_y + ry;
        if py >= screen_h {
            break;
        }
        for rx in 0..bar_w {
            let px = BAR_SIDE_PAD + rx;
            if px >= screen_w {
                break;
            }
            let v = if rx < fill_w {
                BAR_FILL_COLOR
            } else {
                BAR_TRACK_COLOR
            };
            put_pixel(buf, screen_w, px, py, v);
        }
    }
}

/// Bounding rectangle of the loading bar for partial-refresh updates.
///
/// Returns `(x, y, width, height)` with a small margin so the refresh band
/// covers the full bar including anti-aliasing edges.
pub fn loading_bar_rect(screen_w: i32, screen_h: i32) -> (i32, i32, i32, i32) {
    let bar_w = screen_w - 2 * BAR_SIDE_PAD as i32;
    let bar_y = screen_h - screen_h / 10;
    (
        BAR_SIDE_PAD as i32,
        bar_y - 4,
        bar_w,
        BAR_HEIGHT as i32 + 8,
    )
}

/// Draw a rotating-arc spinner centred horizontally near the bottom.
///
/// `angle_deg` is the clockwise start angle of the arc (0 = right). The arc
/// spans [`SPINNER_ARC_SPAN`] degrees. Pixels inside the badge circle but
/// outside the arc ring are set to white (0xFFFF).
pub fn paint_spinner(buf: &mut [u8], screen_w: usize, screen_h: usize, angle_deg: u32) {
    let cx = screen_w as f32 / 2.0;
    let cy = screen_h as f32 - SPINNER_OFFSET_FROM_BOTTOM;
    let badge_r = SPINNER_BADGE_R;
    let arc_r = SPINNER_ARC_R;
    let arc_thick = SPINNER_ARC_THICK;
    let span = SPINNER_ARC_SPAN;
    for py in 0..screen_h {
        let dy = py as f32 - cy;
        if dy.abs() > badge_r + 2.0 {
            continue;
        }
        for px in 0..screen_w {
            let dx = px as f32 - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > badge_r + 2.0 {
                continue;
            }
            let mut val: u16 = 0xFFFF;
            if (dist - arc_r).abs() <= arc_thick {
                let a = (-dy).atan2(dx).to_degrees().rem_euclid(360.0);
                let start = angle_deg as f32;
                let end = start + span as f32;
                let in_arc = if end <= 360.0 {
                    a >= start && a < end
                } else {
                    a >= start || a < end - 360.0
                };
                if in_arc {
                    val = 0x0000;
                }
            }
            put_pixel(buf, screen_w, px, py, val);
        }
    }
}

/// Bounding rectangle of the spinner: `(x, y, width, height)`.
pub fn spinner_rect(screen_w: i32, screen_h: i32) -> (i32, i32, i32, i32) {
    let cx = screen_w / 2;
    let cy = screen_h - SPINNER_OFFSET_FROM_BOTTOM as i32;
    let r = SPINNER_PAD;
    (cx - r, cy - r, r * 2, r * 2)
}
