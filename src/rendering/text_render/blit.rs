// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use rustybuzz::UnicodeBuffer;

use super::detect_script;
use super::fonts::{fallback_for_char, font_for, glyph_cache, TextStyle};

pub fn blit_rgb565(
    buf: &mut [u8],
    buf_stride: usize,
    text: &str,
    px_size: f32,
    ox: usize,
    oy: usize,
    max_w: usize,
    max_h: usize,
) {
    blit_rgb565_color(buf, buf_stride, text, px_size, ox, oy, 0x0000, max_w, max_h);
}

/// Emphasis is synthesised from the regular face (see [`TextStyle`]).
///
/// Bold smears each glyph's coverage horizontally, italic shears it about the
/// baseline. Both act on the rasterised bitmap rather than the outline, which
/// means they compose with every script and with the glyph cache: the cache is
/// keyed on the unstyled glyph, so a bold run reuses the same rasterisation as
/// plain text.
///
/// Neither changes the pen advance. That is what keeps emphasis from reflowing
/// a paragraph -- the same reason `word_width_styled` ignores them.
const OBLIQUE_SHEAR: f32 = 0.21; // ~12 degrees

/// Horizontal smear for synthetic bold, in px. Scaled with type size so it
/// reads as the same weight at any font setting, and never less than 1.
fn bold_smear(px_size: f32) -> usize {
    ((px_size / 22.0).round() as usize).max(1)
}

pub fn blit_rgb565_color(
    buf: &mut [u8],
    buf_stride: usize,
    text: &str,
    px_size: f32,
    ox: usize,
    oy: usize,
    color: u16,
    max_w: usize,
    max_h: usize,
) {
    blit_rgb565_styled(
        buf,
        buf_stride,
        text,
        px_size,
        ox,
        oy,
        color,
        TextStyle::PLAIN,
        max_w,
        max_h,
    );
}

/// As [`blit_rgb565_color`], but sets the run in `style`.
#[allow(clippy::too_many_arguments)]
pub fn blit_rgb565_styled(
    buf: &mut [u8],
    buf_stride: usize,
    text: &str,
    px_size: f32,
    ox: usize,
    oy: usize,
    color: u16,
    style: TextStyle,
    max_w: usize,
    max_h: usize,
) {
    let cr = ((color >> 11) & 0x1f) as u32;
    let cg = ((color >> 5) & 0x3f) as u32;
    let cb = (color & 0x1f) as u32;
    let script = detect_script(text);
    let fd = font_for(script, style);
    let smear = if style.bold { bold_smear(px_size) } else { 0 };
    let scale = px_size / fd.face.units_per_em() as f32;
    let lm = fd.body.horizontal_line_metrics(px_size);
    let baseline = match lm {
        Some(m) => m.ascent as i32,
        None => px_size as i32,
    };

    let mut ub = UnicodeBuffer::new();
    ub.push_str(text);
    let dir = if script.is_rtl() {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    };
    ub.set_direction(dir);

    let gb = rustybuzz::shape(&fd.face, &[], ub);
    let glyphs = gb.glyph_infos();
    let positions = gb.glyph_positions();

    let mut cursor_x = 0.0;
    let gc = glyph_cache();
    for (info, pos) in glyphs.iter().zip(positions) {
        let advance = pos.x_advance as f32 * scale;
        let gid = info.glyph_id as u16;
        let px_key = (px_size * 100.0) as u32;
        let (rfont, rgid) = if gid != 0 {
            (fd, gid)
        } else {
            let ch = text
                .get(info.cluster as usize..)
                .and_then(|s| s.chars().next());
            match ch.and_then(fallback_for_char) {
                Some(fb) => fb,
                None => (fd, gid),
            }
        };
        let key = (rfont.id, rgid, px_key);
        let (metrics, bitmap) = if let Ok(cache) = gc.lock() {
            cache.get(&key).cloned().unwrap_or_else(|| {
                drop(cache);
                let entry = rfont.body.rasterize_indexed(rgid, px_size);
                // best-effort: a poisoned/contended cache lock just skips caching
                let _ = gc.lock().map(|mut c| {
                    c.insert(key, entry.clone());
                });
                entry
            })
        } else {
            rfont.body.rasterize_indexed(rgid, px_size)
        };
        let gw = metrics.width;
        let gh = metrics.height;
        if gw > 0 && gh > 0 {
            let pen_x = cursor_x + pos.x_offset as f32 * scale;
            let gx0 = ox as i32 + pen_x.round() as i32 + metrics.xmin;
            let gy0 = oy as i32 + baseline
                - (pos.y_offset as f32 * scale).round() as i32
                - metrics.ymin
                - gh as i32;
            for ry in 0..gh {
                // Shear about the baseline so the glyph leans without drifting
                // off the line: rows above the baseline move right, descenders
                // move left, and the baseline itself does not move.
                let shear_dx = if style.italic {
                    let above_baseline = (gh as i32 - ry as i32) + metrics.ymin;
                    (above_baseline as f32 * OBLIQUE_SHEAR).round() as i32
                } else {
                    0
                };
                for rx in 0..gw + smear {
                    // Synthetic bold: a pixel is inked if any of the `smear`
                    // columns to its left were, which thickens every stem by the
                    // same amount regardless of direction.
                    let lo = rx.saturating_sub(smear);
                    let hi = rx.min(gw - 1);
                    let cov = (lo..=hi)
                        .map(|sx| bitmap[ry * gw + sx] as u32)
                        .max()
                        .unwrap_or(0);
                    if cov == 0 {
                        continue;
                    }
                    let px = gx0 + rx as i32 + shear_dx;
                    let py = gy0 + ry as i32;
                    if px < 0 || (px as usize) >= max_w || py < 0 || (py as usize) >= max_h {
                        continue;
                    }
                    let off = (py as usize * buf_stride + px as usize) * 2;
                    if off + 2 > buf.len() {
                        continue;
                    }
                    let old = (buf[off] as u16) | ((buf[off + 1] as u16) << 8);
                    let inv = 255 - cov;
                    let nr = (cr * cov + (((old >> 11) & 0x1f) as u32) * inv) / 255;
                    let ng = (cg * cov + (((old >> 5) & 0x3f) as u32) * inv) / 255;
                    let nb = (cb * cov + ((old & 0x1f) as u32) * inv) / 255;
                    let v = ((nr << 11) | (ng << 5) | nb) as u16;
                    buf[off] = (v & 0xff) as u8;
                    buf[off + 1] = (v >> 8) as u8;
                }
            }
        }
        cursor_x += advance;
    }
}
