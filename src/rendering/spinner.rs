//! Loading spinner for splash screens — a rotating arc rendered directly to
//! an RGB565 buffer.  Pure function: no state, no allocation.

use crate::rendering::text_render;

/// Draw the spinner arc at `angle_deg` rotation onto `buffer`.
///
/// The spinner is centred horizontally, ~130 px from the bottom of the screen.
/// Pixels outside the badge radius are left untouched; pixels inside but
/// outside the arc are set to white (0xFFFF).
pub fn paint_spinner(buffer: &mut [crate::Rgb565Pixel], angle_deg: u32) {
    let w = crate::w();
    let h = crate::h();
    let buf_bytes = text_render::rgb565_as_bytes(buffer);
    let cx = w as f32 / 2.0;
    let cy = h as f32 - 130.0;
    let badge_r = 52.0f32;
    let arc_r = 36.0f32;
    let arc_thick = 9.0f32;
    let span = 300u32;
    for py in 0..h {
        let dy = py as f32 - cy;
        if dy.abs() > badge_r + 2.0 {
            continue;
        }
        for px in 0..w {
            let dx = px as f32 - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > badge_r + 2.0 {
                continue;
            }
            let off = (py * w + px) * 2;
            if off + 2 > buf_bytes.len() {
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
            buf_bytes[off] = (val & 0xff) as u8;
            buf_bytes[off + 1] = (val >> 8) as u8;
        }
    }
}

/// Bounding rectangle of the spinner: `(x, y, width, height)`.
pub fn spinner_rect() -> (i32, i32, i32, i32) {
    let w = crate::w() as i32;
    let h = crate::h() as i32;
    let cx = w / 2;
    let cy = h - 130;
    let r = 56;
    (cx - r, cy - r, r * 2, r * 2)
}
