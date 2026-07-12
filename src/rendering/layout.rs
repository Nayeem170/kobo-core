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

fn word_wrap_char_based(text: &str, max_w: usize, px: f32) -> Vec<html_text::Line> {
    let mut out = Vec::new();
    let max_wf = (max_w.saturating_sub(12)) as f32;
    let mut line_text = String::new();
    let mut line_w: f32 = 0.0;
    let mut byte_start = 0usize;

    for (byte_off, ch) in text.char_indices() {
        let ch_w = text_render::word_width(&ch.to_string(), px);
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
    let mut out = Vec::new();
    let n = text.len();
    let max_wf = (max_w - 12) as f32;
    let mut line_start = 0usize;
    let mut line_text = String::new();
    let mut line_w: f32 = 0.0;
    let mut i = 0usize;
    while i < n {
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

fn is_sentence_terminator(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!' | '?' | '\u{0964}' | '\u{0965}' | '\u{3002}' | '\u{FF01}' | '\u{FF1F}'
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

    if matches!(ch, '\u{3002}' | '\u{FF01}' | '\u{FF1F}') {
        return true;
    }

    if matches!(ch, '!' | '?' | '\u{0964}' | '\u{0965}') {
        return next.is_none()
            | matches!(next, Some(c) if c.is_whitespace())
            | matches!(next, Some('"') | Some(')') | Some(']') | Some('\''));
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
        Some(c) if c.is_ascii_uppercase() => true,
        Some(')') | Some('"') | Some(']') | Some('\'') => true,
        _ => false,
    }
}
