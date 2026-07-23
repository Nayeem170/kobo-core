// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Pure text-layout functions: word wrapping and sentence segmentation.
//!
//! Both functions have zero IO and work on any platform.
//! `word_wrap_bytes` depends on [`crate::rendering::text_render`] for script
//! detection and width measurement, and [`crate::html_text::Line`] for output.

use crate::html_text;
use crate::rendering::text_render;

pub fn word_wrap_bytes(text: &str, max_w: usize, px: f32) -> Vec<html_text::Line> {
    let script = text_render::detect_script(text);
    if !script.uses_word_spacing() {
        return word_wrap_char_based(text, max_w, px);
    }
    word_wrap_word_based(text, max_w, px)
}

/// Like `word_wrap_bytes` but reserves `first_indent` px on the first line for a
/// paragraph indent. Only word-spacing scripts indent; the rest wrap unchanged.
pub fn word_wrap_indent(
    text: &str,
    max_w: usize,
    first_indent: usize,
    px: f32,
) -> Vec<html_text::Line> {
    let script = text_render::detect_script(text);
    if !script.uses_word_spacing() {
        return word_wrap_char_based(text, max_w, px);
    }
    word_wrap_word_based_indent(text, max_w, first_indent, px)
}

/// Character-granular wrap that preserves every space and tab. Used for `<pre>`
/// code, where indentation is meaningful and words must not be reflowed, and
/// internally for scripts that do not separate words with spaces.
pub fn word_wrap_char_based(text: &str, max_w: usize, px: f32) -> Vec<html_text::Line> {
    word_wrap_char_based_styled(text, max_w, px, text_render::TextStyle::PLAIN)
}

/// As [`word_wrap_char_based`], measured in `style`. Code wraps in the
/// monospace face, whose advances are wider than the proportional body face --
/// measuring with the wrong one overfills every line.
pub fn word_wrap_char_based_styled(
    text: &str,
    max_w: usize,
    px: f32,
    style: text_render::TextStyle,
) -> Vec<html_text::Line> {
    let mut out = Vec::new();
    let max_wf = (max_w.saturating_sub(12)) as f32;
    let mut line_text = String::new();
    let mut line_w: f32 = 0.0;
    let mut byte_start = 0usize;

    for (byte_off, ch) in text.char_indices() {
        let ch_w = text_render::word_width_styled(&ch.to_string(), px, style);
        if line_w + ch_w > max_wf && !line_text.is_empty() {
            out.push(html_text::Line {
                text: std::mem::take(&mut line_text),
                start: byte_start,
                end: byte_off,
                width: line_w,
            });
            byte_start = byte_off;
            line_w = 0.0;
        }
        line_text.push(ch);
        line_w += ch_w;
    }
    if !line_text.is_empty() {
        out.push(html_text::Line {
            text: line_text,
            start: byte_start,
            end: text.len(),
            width: line_w,
        });
    }
    out
}

fn word_wrap_word_based(text: &str, max_w: usize, px: f32) -> Vec<html_text::Line> {
    word_wrap_word_based_indent(text, max_w, 0, px)
}

/// Word-wrap where the first line is narrower by `first_indent` px (for a
/// first-line paragraph indent), so the indented line still fits the column.
fn word_wrap_word_based_indent(
    text: &str,
    max_w: usize,
    first_indent: usize,
    px: f32,
) -> Vec<html_text::Line> {
    let mut out = Vec::new();
    let n = text.len();
    let full_wf = (max_w - 12) as f32;
    let mut line_start = 0usize;
    let mut line_text = String::new();
    let mut line_w: f32 = 0.0;
    let mut i = 0usize;
    while i < n {
        // First line (nothing pushed yet) reserves the indent.
        let max_wf = if out.is_empty() {
            full_wf - first_indent as f32
        } else {
            full_wf
        };
        while i < n && text[i..].chars().next().is_some_and(|c| c.is_whitespace()) {
            i += text[i..].chars().next().map_or(0, |c| c.len_utf8());
        }
        if i >= n {
            break;
        }
        let word_start = i;
        while i < n && !text[i..].chars().next().is_some_and(|c| c.is_whitespace()) {
            i += text[i..].chars().next().map_or(0, |c| c.len_utf8());
        }
        let word = &text[word_start..i];
        let ww = text_render::word_width(word, px);
        if !line_text.is_empty() && line_w + 8.0 + ww > max_wf {
            out.push(html_text::Line {
                text: line_text,
                start: line_start,
                end: word_start,
                width: line_w,
            });
            line_start = word_start;
            line_text = String::new();
            line_w = 0.0;
        }
        if ww > max_wf && line_text.is_empty() {
            let mut ci = 0usize;
            while ci < word.len() {
                let Some(ch) = word[ci..].chars().next() else {
                    break;
                };
                let ch_w = text_render::word_width(&ch.to_string(), px);
                if line_w + ch_w > max_wf && !line_text.is_empty() {
                    out.push(html_text::Line {
                        text: line_text.clone(),
                        start: line_start,
                        end: word_start + ci,
                        width: line_w,
                    });
                    line_start = word_start + ci;
                    line_text = String::new();
                    line_w = 0.0;
                }
                line_text.push(ch);
                line_w += ch_w;
                ci += ch.len_utf8();
            }
        } else if line_text.is_empty() {
            line_text.push_str(word);
            line_w = ww;
        } else {
            line_text.push(' ');
            line_text.push_str(word);
            line_w += 8.0 + ww;
        }
    }
    if !line_text.is_empty() {
        out.push(html_text::Line {
            text: line_text,
            start: line_start,
            end: n,
            width: line_w,
        });
    }
    out
}

const SENTENCE_ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "inc", "ltd", "co", "corp",
    "vol", "fig", "dept", "est", "approx", "e.g", "i.e", "u.s", "u.k", "ph.d", "a.m", "p.m",
];

pub fn sentences_with_ranges(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut chunk_start = 0usize;
    let mut ci = 0usize;
    while ci < n {
        let (byte_idx, ch) = chars[ci];
        // A newline is a hard paragraph boundary: block elements are joined with
        // '\n', so it always ends an utterance even when a paragraph carries no
        // terminal punctuation (Thai/Lao prose, a heading, or a caption). Without
        // this, such paragraphs run together into one mega-utterance spanning
        // several scripts, and detect_script on the blob picks the wrong voice.
        if ch == '\n' {
            push_sentence(&mut out, text, chunk_start, byte_idx);
            chunk_start = byte_idx + 1;
            ci += 1;
            continue;
        }
        if is_sentence_terminator(ch) && is_real_sentence_end(&chars, ci) {
            let end = byte_idx + ch.len_utf8();
            push_sentence(&mut out, text, chunk_start, end);
            chunk_start = end;
        }
        ci += 1;
    }
    push_sentence(&mut out, text, chunk_start, text.len());
    out
}

/// Paginate row heights into pages that fit within `content_h` px.
///
/// Returns `(start, end)` row-index ranges, one per page. A heading that would
/// land at the very bottom of a page is pushed to the next page instead so it
/// is not orphaned from the body it introduces.
pub fn paginate_heights(
    heights: &[i32],
    content_h: i32,
    heading_indices: &[usize],
) -> Vec<(usize, usize)> {
    let heading_set: std::collections::HashSet<usize> = heading_indices.iter().copied().collect();
    let mut pages = Vec::new();
    let n = heights.len();
    let mut i = 0usize;
    while i < n {
        let start = i;
        let mut h = 0i32;
        while i < n {
            let rh = heights[i];
            if h + rh > content_h && i > start {
                break;
            }
            h += rh;
            i += 1;
        }
        let page_end = i;
        if page_end < n {
            let last = page_end - 1;
            if heading_set.contains(&last) && i < n && !heading_set.contains(&i) {
                i = last;
            }
        }
        pages.push((start, i));
    }
    if pages.is_empty() {
        pages.push((0, 0));
    }
    pages
}

/// Clamp a page index into `[0, page_count-1]`. An empty chapter (0 pages)
/// maps any request to 0.
pub fn clamp_page(page: usize, page_count: usize) -> usize {
    if page_count == 0 {
        0
    } else {
        page.min(page_count - 1)
    }
}

/// Map a progress-bar per-mille (0..1000) to a `(chapter, local_offset)` pair
/// using the cumulative chapter-offset table.
pub fn resolve_progress_target(
    per_mille: i32,
    chapter_offsets: &[usize],
    chapter_count: usize,
) -> (usize, usize) {
    let total = (*chapter_offsets.last().unwrap_or(&1)).max(1);
    let global = (per_mille as usize * total) / 1000;
    let mut c = 0usize;
    while c + 1 < chapter_offsets.len() && chapter_offsets[c + 1] <= global {
        c += 1;
    }
    if c >= chapter_count {
        c = chapter_count.saturating_sub(1);
    }
    let local = global.saturating_sub(*chapter_offsets.get(c).unwrap_or(&0));
    (c, local)
}

fn is_sentence_terminator(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!'
            | '?'
            | '\u{0964}'
            | '\u{0965}'
            | '\u{3002}'
            | '\u{FF01}'
            | '\u{FF1F}'
            | '\u{17D4}'
            | '\u{17D5}'
            | '\u{104A}'
            | '\u{104B}'
            | '\u{1362}'
    )
}

fn push_sentence(
    out: &mut Vec<(String, usize, usize)>,
    text: &str,
    start_byte: usize,
    end_byte: usize,
) {
    let end_byte = end_byte.min(text.len());
    if start_byte >= end_byte {
        return;
    }
    let chunk = &text[start_byte..end_byte];
    let trimmed = chunk.trim();
    if trimmed.is_empty() {
        return;
    }
    let off = chunk.find(trimmed).unwrap_or(0);
    let start = start_byte + off;
    out.push((trimmed.to_string(), start, start + trimmed.len()));
}

fn char_at(chars: &[(usize, char)], i: isize) -> Option<char> {
    if i < 0 || (i as usize) >= chars.len() {
        None
    } else {
        Some(chars[i as usize].1)
    }
}

fn next_non_space(chars: &[(usize, char)], from: usize) -> Option<char> {
    let mut j = from;
    while j < chars.len() {
        let c = chars[j].1;
        if !c.is_whitespace() {
            return Some(c);
        }
        j += 1;
    }
    None
}

/// True when a newline separates this position from the next visible text.
///
/// Chapter bodies join block elements with `\n`, so a terminator followed by a
/// newline is always a paragraph break and therefore always a sentence end.
fn newline_before_next(chars: &[(usize, char)], from: usize) -> bool {
    let mut j = from;
    while j < chars.len() {
        let c = chars[j].1;
        if c == '\n' {
            return true;
        }
        if !c.is_whitespace() {
            return false;
        }
        j += 1;
    }
    false
}

/// Characters that may legitimately follow a sentence terminator: ASCII closers
/// plus the typographic quotes ebooks actually use. Accepting only the ASCII
/// forms merged every `... .` + `'Dialogue'` pair into one run-on utterance.
fn is_closer_or_quote(c: char) -> bool {
    matches!(
        c,
        ')' | '"'
            | ']'
            | '\''
            | '\u{2018}'
            | '\u{2019}'
            | '\u{201C}'
            | '\u{201D}'
            | '\u{00AB}'
            | '\u{00BB}'
    )
}

fn abbrev_token_before(chars: &[(usize, char)], i: usize) -> String {
    let mut start = i;
    while start > 0 && chars[start - 1].1.is_ascii_alphanumeric() {
        start -= 1;
    }
    if start > 0 && chars[start - 1].1 == '.' {
        let mut s2 = start - 1;
        while s2 > 0 && chars[s2 - 1].1.is_ascii_alphanumeric() {
            s2 -= 1;
        }
        if s2 < start - 1 {
            start = s2;
        }
    }
    chars[start..i]
        .iter()
        .map(|(_, c)| c.to_ascii_lowercase())
        .collect()
}

fn is_real_sentence_end(chars: &[(usize, char)], i: usize) -> bool {
    let ch = chars[i].1;
    let prev = char_at(chars, (i as isize) - 1);
    let next = char_at(chars, (i as isize) + 1);

    if matches!(
        ch,
        '\u{3002}'
            | '\u{FF01}'
            | '\u{FF1F}'
            | '\u{17D4}'
            | '\u{17D5}'
            | '\u{104A}'
            | '\u{104B}'
            | '\u{1362}'
    ) {
        return true;
    }

    // A paragraph break always ends a sentence, whatever precedes it. Without
    // this, a paragraph closing on `.` before a line that opens with a quote
    // runs on into the following paragraphs, building a single enormous
    // utterance that no TTS timeout can ever synthesize.
    if newline_before_next(chars, i + 1) {
        return true;
    }

    if matches!(ch, '!' | '?' | '\u{0964}' | '\u{0965}') {
        return next.is_none()
            | matches!(next, Some(c) if c.is_whitespace())
            | matches!(next, Some(c) if is_closer_or_quote(c));
    }

    if prev == Some('.') || next == Some('.') {
        return false;
    }
    if matches!(prev, Some(c) if c.is_ascii_digit()) {
        return false;
    }
    if matches!(prev, Some(c) if c.is_ascii_uppercase()) {
        if !matches!(char_at(chars, (i as isize) - 2), Some(c) if c.is_ascii_alphanumeric()) {
            return false;
        }
    }
    if SENTENCE_ABBREVIATIONS.contains(&abbrev_token_before(chars, i).as_str()) {
        return false;
    }
    match next_non_space(chars, i + 1) {
        None => true,
        // `is_uppercase`, not `is_ascii_uppercase`: accented capitals open
        // sentences too.
        Some(c) if c.is_uppercase() => true,
        Some(c) if is_closer_or_quote(c) => true,
        _ => false,
    }
}

mod pagination;
#[cfg(test)]
mod tests;

pub use pagination::{
    block_indent_for, count_chapter_pages, estimate_chapter_offsets, ScreenLayout,
};
