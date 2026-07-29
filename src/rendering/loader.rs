// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Loading indicators for splash and loading screens.
//!
//! Pure functions operating on raw RGB565 byte buffers. All take explicit
//! screen dimensions so they are reusable by any crate without global state.
//!
//! - [`paint_loading_bar`] - horizontal progress bar at the bottom of the screen
//!
//! There used to be a rotating-arc spinner here. It was removed: a rotating arc
//! drives every pixel in its ring from black to white and back, continuously,
//! for as long as loading takes. Non-monotone and unbounded is the worst case
//! this panel can be given, and no amount of tuning the rate fixed it. The
//! splash now reveals its wordmark one piece at a time instead, so each pixel
//! changes state exactly once (see `rendering::splash` in the app crate).
//!
//! A progress bar only ever fills in one direction, so it stays valid.

const BAR_SIDE_PAD: usize = 80;
const BAR_HEIGHT: usize = 8;
const BAR_FILL_COLOR: u16 = 0x06A4;
const BAR_TRACK_COLOR: u16 = 0xD6BA;

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

/// Area-average downscale of an RGB888 buffer.
///
/// Averaging every source pixel that lands in a destination cell preserves
/// thin strokes as continuous grey rather than flickering fragments.
pub fn box_downscale(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 3];
    for dy in 0..dh {
        let y0 = dy * sh / dh;
        let y1 = (((dy + 1) * sh).div_ceil(dh)).min(sh).max(y0 + 1);
        for dx in 0..dw {
            let x0 = dx * sw / dw;
            let x1 = (((dx + 1) * sw).div_ceil(dw)).min(sw).max(x0 + 1);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1 {
                let row = sy * sw;
                for sx in x0..x1 {
                    let o = (row + sx) * 3;
                    r += src[o] as u32;
                    g += src[o + 1] as u32;
                    b += src[o + 2] as u32;
                    n += 1;
                }
            }
            let o = (dy * dw + dx) * 3;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
        }
    }
    out
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

    #[test]
    fn box_downscale_averages_block() {
        let src = [100u8, 200, 50, 100, 200, 50, 100, 200, 50, 100, 200, 50];
        let out = box_downscale(&src, 2, 2, 1, 1);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], 100);
        assert_eq!(out[1], 200);
        assert_eq!(out[2], 50);
    }

    #[test]
    fn box_downscale_identity_passes_through() {
        let src = [10u8, 20, 30, 40, 50, 60];
        let out = box_downscale(&src, 1, 2, 1, 2);
        assert_eq!(out, src);
    }
}
