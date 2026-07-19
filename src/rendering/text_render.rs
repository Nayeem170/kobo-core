// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
pub mod blit;
pub mod fonts;

pub use blit::*;
pub use fonts::*;

const FONT_LATIN: &[u8] = include_bytes!("../../fonts/NotoSansLatin.ttf");
/// Monospace face for code listings. DejaVu Sans Mono, under the DejaVu Fonts
/// License (see `fonts/LICENSE-DejaVu.txt`), which permits redistribution
/// including inside a binary.
const FONT_MONO: &[u8] = include_bytes!("../../fonts/DejaVuSansMono.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Bengali,
    Devanagari,
    Arabic,
    Hebrew,
    Greek,
    Cyrillic,
    Georgian,
    Armenian,
    Ethiopic,
    Gujarati,
    Gurmukhi,
    Tamil,
    Telugu,
    Kannada,
    Malayalam,
    Sinhala,
    Thai,
    Lao,
    Khmer,
    Myanmar,
    Japanese,
    Korean,
    /// Han with no kana or hangul alongside it. Real Japanese prose always
    /// carries kana, so unaccompanied Han is overwhelmingly Chinese - but the
    /// book's `dc:language` still wins when it is present.
    Chinese,
    Other,
}

/// Every script the renderer can load a face for. `Latin` is excluded: it is
/// the embedded default and never occupies a registry slot.
pub const LOADABLE_SCRIPTS: &[Script] = &[
    Script::Bengali,
    Script::Devanagari,
    Script::Arabic,
    Script::Hebrew,
    Script::Greek,
    Script::Cyrillic,
    Script::Georgian,
    Script::Armenian,
    Script::Ethiopic,
    Script::Gujarati,
    Script::Gurmukhi,
    Script::Tamil,
    Script::Telugu,
    Script::Kannada,
    Script::Malayalam,
    Script::Sinhala,
    Script::Thai,
    Script::Lao,
    Script::Khmer,
    Script::Myanmar,
    Script::Japanese,
    Script::Korean,
    Script::Chinese,
];

impl Script {
    /// Number of registry slots. Sized to the enum so a new variant cannot
    /// silently overflow the font table.
    pub const SLOTS: usize = 24;

    /// Stable index into the font registry.
    pub fn slot(self) -> usize {
        self as usize
    }

    /// Glyph-cache discriminator. 0 is the embedded Latin face, so every script
    /// is offset by one; the mono face takes the last id.
    pub fn font_id(self) -> u8 {
        self as u8 + 1
    }

    pub fn is_rtl(self) -> bool {
        matches!(self, Script::Arabic | Script::Hebrew)
    }

    /// Scripts written without spaces between words, so a line may be broken
    /// between any two glyphs rather than only at spaces.
    pub fn uses_word_spacing(self) -> bool {
        !matches!(
            self,
            Script::Japanese
                | Script::Chinese
                | Script::Korean
                | Script::Thai
                | Script::Lao
                | Script::Khmer
                | Script::Myanmar
        )
    }

    /// Default Edge TTS locale for the script. Only consulted when the book
    /// declares no `dc:language`.
    pub fn lang_tag(self) -> &'static str {
        match self {
            Script::Latin => "en-US",
            Script::Bengali => "bn-BD",
            Script::Devanagari => "hi-IN",
            Script::Arabic => "ar-SA",
            Script::Hebrew => "he-IL",
            Script::Greek => "el-GR",
            Script::Cyrillic => "ru-RU",
            Script::Georgian => "ka-GE",
            Script::Armenian => "hy-AM",
            Script::Ethiopic => "am-ET",
            Script::Gujarati => "gu-IN",
            Script::Gurmukhi => "pa-IN",
            Script::Tamil => "ta-IN",
            Script::Telugu => "te-IN",
            Script::Kannada => "kn-IN",
            Script::Malayalam => "ml-IN",
            Script::Sinhala => "si-LK",
            Script::Thai => "th-TH",
            Script::Lao => "lo-LA",
            Script::Khmer => "km-KH",
            Script::Myanmar => "my-MM",
            Script::Japanese => "ja-JP",
            Script::Korean => "ko-KR",
            Script::Chinese => "zh-CN",
            Script::Other => "en-US",
        }
    }
}

/// Script of a single character, ignoring the Han ambiguity.
fn script_of(code: u32) -> Script {
    match code {
        0x0980..=0x09FF => Script::Bengali,
        0x0900..=0x097F => Script::Devanagari,
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0400..=0x052F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => Script::Cyrillic,
        0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => Script::Georgian,
        0x0530..=0x058F => Script::Armenian,
        0x1200..=0x137F | 0x1380..=0x139F | 0x2D80..=0x2DDF => Script::Ethiopic,
        0x0A80..=0x0AFF => Script::Gujarati,
        0x0A00..=0x0A7F => Script::Gurmukhi,
        0x0B80..=0x0BFF => Script::Tamil,
        0x0C00..=0x0C7F => Script::Telugu,
        0x0C80..=0x0CFF => Script::Kannada,
        0x0D00..=0x0D7F => Script::Malayalam,
        0x0D80..=0x0DFF => Script::Sinhala,
        0x0E00..=0x0E7F => Script::Thai,
        0x0E80..=0x0EFF => Script::Lao,
        0x1780..=0x17FF | 0x19E0..=0x19FF => Script::Khmer,
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => Script::Myanmar,
        // Latin, including Extended Additional so Vietnamese diacritics do not
        // fall through to `Other` and get read out in English.
        0x0000..=0x024F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F => Script::Latin,
        _ => Script::Other,
    }
}

/// Detect the dominant script of a run.
///
/// Counts every letter and takes the majority rather than trusting the first
/// one. Both simpler rules break on real books: "first letter wins" calls a
/// Bengali chapter Latin because it opens with "Chapter", and "first non-Latin
/// wins" calls an English paragraph Greek because it quotes one Greek word.
///
/// Kana and hangul are decisive regardless of count - they appear in exactly
/// one language each, and Japanese prose interleaves kana with Han, so the Han
/// belongs to whichever of the two is present.
pub fn detect_script(text: &str) -> Script {
    let mut counts = [0usize; Script::SLOTS];
    let mut kana = 0usize;
    let mut hangul = 0usize;
    let mut han = 0usize;

    for c in text.chars() {
        if !c.is_alphabetic() {
            continue;
        }
        match c as u32 {
            0x3040..=0x30FF | 0x31F0..=0x31FF => kana += 1,
            0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => hangul += 1,
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x20000..=0x2A6DF => han += 1,
            code => counts[script_of(code).slot()] += 1,
        }
    }

    // Any kana at all means Japanese; any hangul means Korean. Han on its own
    // is Chinese, since Japanese never runs long without kana.
    if kana > 0 {
        return Script::Japanese;
    }
    if hangul > 0 {
        return Script::Korean;
    }
    counts[Script::Chinese.slot()] += han;

    let (best, best_count) = counts
        .iter()
        .enumerate()
        .max_by_key(|&(_, n)| *n)
        .map(|(i, n)| (i, *n))
        .unwrap_or((Script::Latin.slot(), 0));

    if best_count == 0 {
        return Script::Latin;
    }
    LOADABLE_SCRIPTS
        .iter()
        .copied()
        .chain(std::iter::once(Script::Latin))
        .find(|s| s.slot() == best)
        .unwrap_or(Script::Latin)
}

static WIDTH_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<(String, u32), f32>>,
> = std::sync::OnceLock::new();

pub(crate) fn width_cache(
) -> &'static std::sync::Mutex<std::collections::HashMap<(String, u32), f32>> {
    WIDTH_CACHE.get_or_init(std::sync::Mutex::default)
}

#[allow(dead_code)]
pub fn clear_width_cache() {
    if let Ok(mut cache) = width_cache().lock() {
        cache.clear();
    }
}

pub struct DecodedImage {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub struct DecodedRgba {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub fn decode_image(raw: &[u8], max_w: usize, max_h: usize) -> Option<DecodedImage> {
    let img = image::load_from_memory(raw).ok()?;
    let (ow, oh) = (img.width() as usize, img.height() as usize);
    if ow == 0 || oh == 0 {
        return None;
    }
    let scale = max_w as f32 / ow as f32;
    let mut nw = max_w;
    let mut nh = (oh as f32 * scale).round() as usize;
    if nh == 0 {
        return None;
    }
    if nh > max_h {
        let hscale = max_h as f32 / nh as f32;
        nh = max_h;
        nw = (nw as f32 * hscale).round() as usize;
        if nw == 0 {
            return None;
        }
    }
    let resized = img.resize(nw as u32, nh as u32, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let (rw, rh) = (rgb.width() as usize, rgb.height() as usize);
    Some(DecodedImage {
        rgb: rgb.into_raw(),
        width: rw,
        height: rh,
    })
}

pub fn decode_image_rgba(raw: &[u8], max_w: usize, max_h: usize) -> Option<DecodedRgba> {
    let img = image::load_from_memory(raw).ok()?;
    let (ow, oh) = (img.width() as usize, img.height() as usize);
    if ow == 0 || oh == 0 {
        return None;
    }
    let scale = max_w as f32 / ow as f32;
    let mut nw = max_w;
    let mut nh = (oh as f32 * scale).round() as usize;
    if nh == 0 {
        return None;
    }
    if nh > max_h {
        let hscale = max_h as f32 / nh as f32;
        nh = max_h;
        nw = (nw as f32 * hscale).round() as usize;
        if nw == 0 {
            return None;
        }
    }
    let resized = img.resize(nw as u32, nh as u32, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let (rw, rh) = (rgba.width() as usize, rgba.height() as usize);
    Some(DecodedRgba {
        rgba: rgba.into_raw(),
        width: rw,
        height: rh,
    })
}

pub fn blit_rgb565_image(
    buf: &mut [u8],
    buf_stride: usize,
    rgb: &[u8],
    iw: usize,
    ih: usize,
    ox: usize,
    oy: usize,
    max_w: usize,
    max_h: usize,
) {
    for ry in 0..ih {
        let py = oy + ry;
        if py >= max_h {
            break;
        }
        for rx in 0..iw {
            let px = ox + rx;
            if px >= max_w {
                break;
            }
            let idx = (ry * iw + rx) * 3;
            let r = rgb[idx] as u16;
            let g = rgb[idx + 1] as u16;
            let b = rgb[idx + 2] as u16;
            let r5 = (r >> 3) & 0x1f;
            let g6 = (g >> 2) & 0x3f;
            let b5 = (b >> 3) & 0x1f;
            let v = (r5 << 11) | (g6 << 5) | b5;
            let off = (py * buf_stride + px) * 2;
            if off + 2 > buf.len() {
                continue;
            }
            buf[off] = (v & 0xff) as u8;
            buf[off + 1] = (v >> 8) as u8;
        }
    }
}

pub fn blit_rgb565_image_alpha(
    buf: &mut [u8],
    buf_stride: usize,
    rgba: &[u8],
    iw: usize,
    ih: usize,
    ox: usize,
    oy: usize,
    max_w: usize,
    max_h: usize,
) {
    for ry in 0..ih {
        let py = oy + ry;
        if py >= max_h {
            break;
        }
        for rx in 0..iw {
            let px = ox + rx;
            if px >= max_w {
                break;
            }
            let idx = (ry * iw + rx) * 4;
            let a = rgba[idx + 3];
            if a == 0 {
                continue;
            }
            let r = rgba[idx] as u16;
            let g = rgba[idx + 1] as u16;
            let b = rgba[idx + 2] as u16;
            let r5 = (r >> 3) & 0x1f;
            let g6 = (g >> 2) & 0x3f;
            let b5 = (b >> 3) & 0x1f;
            let v = (r5 << 11) | (g6 << 5) | b5;
            let off = (py * buf_stride + px) * 2;
            if off + 2 > buf.len() {
                continue;
            }
            buf[off] = (v & 0xff) as u8;
            buf[off + 1] = (v >> 8) as u8;
        }
    }
}

pub fn style_at(runs: &[crate::html_text::StyleRun], off: usize) -> TextStyle {
    for r in runs {
        if off >= r.start && off < r.end {
            return TextStyle {
                bold: r.bold,
                italic: r.italic,
                mono: false,
            };
        }
        if r.start > off {
            break;
        }
    }
    TextStyle::PLAIN
}

pub fn style_for(runs: &[crate::html_text::StyleRun], off: usize, base: TextStyle) -> TextStyle {
    let s = style_at(runs, off);
    TextStyle {
        bold: s.bold,
        italic: s.italic,
        mono: base.mono,
    }
}

#[cfg(test)]
mod tests;
