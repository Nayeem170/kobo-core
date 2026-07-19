// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use super::{word_wrap_bytes, word_wrap_char_based_styled};
use crate::rendering::text_render;

#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub text_w: usize,
    pub content_h: i32,
    pub heading_h: i32,
    pub heading_gap: i32,
    pub para_gap: i32,
}

pub fn block_indent_for(indent_em: f32, body_px: f32, text_w: usize) -> usize {
    if indent_em <= 0.0 {
        return 0;
    }
    const MAX_BLOCK_INDENT_PX: usize = 255;
    let max = (text_w / 3).min(MAX_BLOCK_INDENT_PX);
    ((indent_em * body_px) as usize).min(max)
}

pub fn estimate_chapter_offsets(
    chapters: &[crate::formats::epub::Chapter],
    current: (usize, usize),
    line_h: i32,
    layout: &ScreenLayout,
) -> Vec<usize> {
    let chars_per_line = (layout.text_w as f32 / (line_h as f32 * 0.6)) as usize;
    let lines_per_page = (layout.content_h / line_h) as usize;
    let chars_per_page = chars_per_line * lines_per_page;
    let mut offsets = vec![0usize; chapters.len() + 1];
    for (i, ch) in chapters.iter().enumerate() {
        let est = if i == current.0 {
            current.1
        } else {
            ((ch.text.chars().count() as f64 / chars_per_page as f64).ceil() as usize).max(1)
        };
        offsets[i + 1] = offsets[i] + est;
    }
    offsets
}

pub fn count_chapter_pages(
    chapter: &mut crate::formats::epub::Chapter,
    body_px: f32,
    line_h: i32,
    layout: &ScreenLayout,
) -> usize {
    let chapter_images = chapter.load_images().to_vec();
    let full = &chapter.text;
    let segs = &chapter.segments;
    let mut row_heights: Vec<i32> = Vec::new();
    let mut heading_indices: Vec<usize> = Vec::new();
    let mut prev_was_gap = false;
    let mut img_idx = 0usize;
    let is_heading = |t: &str| matches!(t, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    for seg in segs {
        if seg.src.is_some() {
            let cap = seg.caption.as_deref().unwrap_or("");
            let h = if let Some(raw) = chapter_images.get(img_idx).map(|(_, b)| b.as_slice()) {
                text_render::decode_image(raw, layout.text_w, layout.content_h as usize - 20)
                    .map(|img| img.height as i32 + if cap.is_empty() { 4 } else { line_h + 4 })
                    .unwrap_or(line_h + 4)
            } else {
                line_h + 4
            };
            row_heights.push(h);
            prev_was_gap = false;
            img_idx += 1;
            continue;
        }
        let seg_text = full.get(seg.start..seg.end).unwrap_or("");
        if is_heading(seg.tag.as_str()) {
            heading_indices.push(row_heights.len());
            row_heights.push(layout.heading_h);
            row_heights.push(layout.heading_gap);
            prev_was_gap = true;
        } else {
            if !row_heights.is_empty() && !prev_was_gap {
                row_heights.push(layout.para_gap);
            }
            let block_indent = block_indent_for(seg.indent, body_px, layout.text_w);
            let avail = layout.text_w.saturating_sub(block_indent);
            let lines = if block_indent > 0 {
                word_wrap_char_based_styled(
                    seg_text,
                    avail,
                    body_px,
                    text_render::TextStyle {
                        mono: true,
                        ..Default::default()
                    },
                )
            } else {
                word_wrap_bytes(seg_text, avail, body_px)
            };
            for _ in &lines {
                row_heights.push(line_h);
            }
            prev_was_gap = false;
        }
    }
    super::paginate_heights(&row_heights, layout.content_h, &heading_indices).len()
}
