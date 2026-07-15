use crate::rendering::common::BODY_PX;
use crate::rendering::layout::word_wrap_bytes;
use crate::rendering::text_render;

pub fn paint_placeholder_box(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    for ry in 0..h {
        for rx in 0..w {
            let px = x + rx;
            let py = y + ry;
            if px >= screen_w || py >= screen_h {
                continue;
            }
            let off = (py * screen_w + px) * 2;
            if off + 2 > buf_bytes.len() {
                continue;
            }
            let border = rx == 0 || ry == 0 || rx == w - 1 || ry == h - 1;
            let v = if border { 0x6B4D } else { 0xEF5D };
            buf_bytes[off] = (v & 0xff) as u8;
            buf_bytes[off + 1] = (v >> 8) as u8;
        }
    }
}

pub fn fill_rounded_rect(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    fill: u16,
    border: u16,
    radius: usize,
) {
    let r = radius.min(w / 2).min(h / 2);
    let put = |buf: &mut [u8], px: usize, py: usize, v: u16| {
        if px >= screen_w || py >= screen_h {
            return;
        }
        let off = (py * screen_w + px) * 2;
        if off + 2 > buf.len() {
            return;
        }
        buf[off] = (v & 0xff) as u8;
        buf[off + 1] = (v >> 8) as u8;
    };
    for ry in 0..h {
        for rx in 0..w {
            let cx_edge = if rx < r {
                r - rx
            } else if rx >= w - r {
                rx - (w - 1 - r)
            } else {
                0
            };
            let cy_edge = if ry < r {
                r - ry
            } else if ry >= h - r {
                ry - (h - 1 - r)
            } else {
                0
            };
            let in_corner = (rx < r || rx >= w - r) && (ry < r || ry >= h - r);
            let v = if in_corner {
                let dist2 =
                    (cx_edge as i32) * (cx_edge as i32) + (cy_edge as i32) * (cy_edge as i32);
                if dist2 > (r as i32) * (r as i32) {
                    continue;
                } else if dist2 >= ((r - 1) as i32) * ((r - 1) as i32) {
                    border
                } else {
                    fill
                }
            } else {
                let on_edge = rx == 0 || ry == 0 || rx == w - 1 || ry == h - 1;
                if on_edge {
                    border
                } else {
                    fill
                }
            };
            put(buf_bytes, x + rx, y + ry, v);
        }
    }
}

pub fn paint_progress_bar(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    x: usize,
    y: usize,
    w: usize,
    frac: f32,
) {
    let bar_h = 8usize;
    for ry in 0..bar_h {
        let py = y + ry;
        if py >= screen_h {
            break;
        }
        for rx in 0..w {
            let px = x + rx;
            if px >= screen_w {
                break;
            }
            let off = (py * screen_w + px) * 2;
            if off + 2 > buf_bytes.len() {
                continue;
            }
            let filled = (rx as f32 / w as f32) < frac;
            let v = if filled { 0x1082 } else { 0xD6BA };
            buf_bytes[off] = (v & 0xff) as u8;
            buf_bytes[off + 1] = (v >> 8) as u8;
        }
    }
}

pub fn measure_text(text: &str, px: f32) -> usize {
    let space_w = text_render::word_width(" ", px);
    let mut total = 0.0f32;
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            total += space_w;
        }
        if !word.is_empty() {
            total += text_render::word_width(word, px);
        }
    }
    total as usize
}

pub const ACTION_BTN_FILL: u16 = 0x0349;
pub const ACTION_BTN_FG: u16 = 0xFFFF;
pub const ACTION_BTN_RADIUS: usize = 10;

pub fn paint_action_button(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    label: &str,
    px: f32,
) {
    fill_rounded_rect(
        buf_bytes,
        screen_w,
        screen_h,
        x,
        y,
        w,
        h,
        ACTION_BTN_FILL,
        ACTION_BTN_FILL,
        ACTION_BTN_RADIUS,
    );
    let lw = measure_text(label, px);
    let lh = text_render::line_height(px);
    let tx = x + (w.saturating_sub(lw)) / 2;
    let ty = y + (h.saturating_sub(lh)) / 2;
    text_render::blit_rgb565_color(
        buf_bytes,
        screen_w,
        label,
        px,
        tx,
        ty,
        ACTION_BTN_FG,
        screen_w,
        screen_h,
    );
}

pub fn paint_nav_bar(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    nav_bar_h: usize,
    y: usize,
    center: &str,
    clock: &str,
    battery: i32,
) {
    let h = nav_bar_h;
    let top = y;
    for ry in 0..h {
        let py = top + ry;
        if py >= screen_h {
            break;
        }
        let is_edge = ry == 0;
        for rx in 0..screen_w {
            let off = (py * screen_w + rx) * 2;
            if off + 2 > buf_bytes.len() {
                break;
            }
            let v = if is_edge { 0x1082 } else { 0xFFFF };
            buf_bytes[off] = (v & 0xff) as u8;
            buf_bytes[off + 1] = (v >> 8) as u8;
        }
    }
    let label_px = BODY_PX * 0.8;
    let small_px = BODY_PX * 0.66;
    let lh = text_render::line_height(label_px);
    let sh = text_render::line_height(small_px);
    let cy = top + (h.saturating_sub(lh)) / 2;
    let sy = top + (h.saturating_sub(sh)) / 2;

    let exit_w = 150usize;
    let exit_h = 56usize.min(h - 8);
    let exit_x = 12usize;
    let exit_y = top + (h.saturating_sub(exit_h)) / 2;
    paint_action_button(
        buf_bytes, screen_w, screen_h, exit_x, exit_y, exit_w, exit_h, "Exit", label_px,
    );

    let status = if battery > 0 {
        format!("{}  {}%", clock, battery)
    } else {
        clock.to_string()
    };
    if !status.is_empty() {
        let sw = measure_text(&status, small_px);
        let sx = screen_w.saturating_sub(sw + 24);
        text_render::blit_rgb565(
            buf_bytes, screen_w, &status, small_px, sx, sy, screen_w, screen_h,
        );
    }
    if !center.is_empty() {
        let label_w = measure_text(center, label_px);
        let cx = (screen_w / 2).saturating_sub(label_w / 2);
        text_render::blit_rgb565(
            buf_bytes, screen_w, center, label_px, cx, cy, screen_w, screen_h,
        );
    }
}

pub fn paint_wrapped_text(
    buf_bytes: &mut [u8],
    screen_w: usize,
    screen_h: usize,
    text: &str,
    x: usize,
    y: usize,
    max_w: usize,
    px: f32,
    max_lines: usize,
) -> usize {
    let lines = word_wrap_bytes(text, max_w, px);
    let lh = text_render::line_height(px);
    let mut cy = y;
    for (i, line) in lines.iter().enumerate() {
        if i >= max_lines {
            break;
        }
        text_render::blit_rgb565(
            buf_bytes, screen_w, &line.text, px, x, cy, screen_w, screen_h,
        );
        cy += lh;
    }
    cy - y
}

#[cfg(test)]
mod tests;
