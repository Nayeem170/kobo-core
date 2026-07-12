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

const SPINNER_ARC_COLOR: u16 = 0xF148;

const SPINNER_OFFSET_FROM_BOTTOM: f32 = 130.0;
const SPINNER_BADGE_R: f32 = 52.0;
const SPINNER_ARC_R: f32 = 36.0;
const SPINNER_ARC_THICK: f32 = 9.0;
const SPINNER_ARC_SPAN: u32 = 300;
const SPINNER_PAD: i32 = 56;

/// A screen rectangle (x, y, width, height) in pixels, for partial-refresh
/// bounding regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

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

/// Bounding rectangle of the loading bar for partial-refresh updates, with a
/// small margin so the refresh band covers the full bar including
/// anti-aliasing edges.
pub fn loading_bar_rect(screen_w: i32, screen_h: i32) -> Rect {
    let bar_w = screen_w - 2 * BAR_SIDE_PAD as i32;
    let bar_y = screen_h - screen_h / 10;
    Rect {
        x: BAR_SIDE_PAD as i32,
        y: bar_y - 4,
        w: bar_w,
        h: BAR_HEIGHT as i32 + 8,
    }
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
                    val = SPINNER_ARC_COLOR;
                }
            }
            put_pixel(buf, screen_w, px, py, val);
        }
    }
}

/// Bounding rectangle of the spinner.
pub fn spinner_rect(screen_w: i32, screen_h: i32) -> Rect {
    let cx = screen_w / 2;
    let cy = screen_h - SPINNER_OFFSET_FROM_BOTTOM as i32;
    let r = SPINNER_PAD;
    Rect {
        x: cx - r,
        y: cy - r,
        w: r * 2,
        h: r * 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_pixel_writes_little_endian_at_offset() {
        let mut buf = [0u8; 4];
        put_pixel(&mut buf, 2, 1, 0, 0x1234);
        // off = (0 * 2 + 1) * 2 = 2 -> little-endian u16
        assert_eq!(buf, [0, 0, 0x34, 0x12]);
    }

    #[test]
    fn put_pixel_ignores_out_of_bounds() {
        let mut buf = [0u8; 4];
        put_pixel(&mut buf, 2, 5, 0, 0xFFFF);
        put_pixel(&mut buf, 2, 0, 5, 0xFFFF);
        assert_eq!(buf, [0, 0, 0, 0]);
    }

    #[test]
    fn loading_bar_rect_geometry() {
        let r = loading_bar_rect(800, 600);
        assert_eq!(r.x, BAR_SIDE_PAD as i32);
        assert_eq!(r.w, 800 - 2 * BAR_SIDE_PAD as i32);
        let bar_y = 600 - 600 / 10;
        assert_eq!(r.y, bar_y - 4);
        assert_eq!(r.h, BAR_HEIGHT as i32 + 8);
    }

    #[test]
    fn spinner_rect_is_centred_square() {
        let r = spinner_rect(800, 600);
        let cx = 800 / 2;
        let cy = 600 - SPINNER_OFFSET_FROM_BOTTOM as i32;
        let rad = SPINNER_PAD;
        assert_eq!(
            r,
            Rect {
                x: cx - rad,
                y: cy - rad,
                w: 2 * rad,
                h: 2 * rad,
            }
        );
        assert_eq!(r.w, r.h);
    }

    #[test]
    fn paint_loading_bar_fill_vs_track() {
        let (w, h) = (800usize, 600usize);
        let mut buf = vec![0u8; w * h * 2];
        let bar_y = h - h / 10;
        let off = (bar_y * w + BAR_SIDE_PAD) * 2;

        paint_loading_bar(&mut buf, w, h, 0.0);
        assert_eq!(&buf[off..off + 2], &BAR_TRACK_COLOR.to_le_bytes());

        paint_loading_bar(&mut buf, w, h, 1.0);
        assert_eq!(&buf[off..off + 2], &BAR_FILL_COLOR.to_le_bytes());
    }
}
