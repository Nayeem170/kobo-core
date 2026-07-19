// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use fontdue::Font;
use rustybuzz::Face;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use super::Script;

pub(crate) const FONT_ID_DEFAULT: u8 = 0;
/// Sits above every script id (`Script::font_id` = variant index + 1).
pub(crate) const FONT_ID_MONO: u8 = Script::SLOTS as u8 + 1;

pub(crate) struct FontEntry {
    pub face: Face<'static>,
    pub body: Font,
    pub id: u8,
}

static FONT_INSTALL_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn font_install_count() -> usize {
    FONT_INSTALL_COUNTER.load(Ordering::Relaxed)
}

/// How a run of text should be set.
///
/// `bold` and `italic` are synthesised from the regular face rather than loaded
/// as separate files. That is a deliberate choice, not a shortcut: this is a
/// Bangla-first reader, so a bundled Latin bold would leave Bengali, Devanagari,
/// Arabic, CJK and Thai with no emphasis at all, and mixing a second family's
/// letterforms into a Noto line reads as two fonts colliding mid-sentence.
/// Synthesis keeps every script consistent with its own body face and costs no
/// binary size.
///
/// `mono` is the opposite case and *is* a real font: code is meant to look
/// different from prose, and no amount of emboldening gives a proportional face
/// the fixed advances that make columns line up.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
}

impl TextStyle {
    pub const PLAIN: TextStyle = TextStyle {
        bold: false,
        italic: false,
        mono: false,
    };

    pub fn is_plain(self) -> bool {
        self == Self::PLAIN
    }

    /// Compact key for the width cache.
    pub(crate) fn bits(self) -> u8 {
        (self.bold as u8) | ((self.italic as u8) << 1) | ((self.mono as u8) << 2)
    }
}

pub(crate) struct FontRegistry {
    pub default: FontEntry,
    pub mono: FontEntry,
    /// One slot per `Script`, indexed by `Script::slot()`. A table rather than
    /// named fields so adding a script touches only the enum and the font spec
    /// list - there is no per-script wiring to forget here.
    pub scripts: [RwLock<Option<FontEntry>>; Script::SLOTS],
}

static FONTS: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();

type GlyphMap = std::collections::HashMap<(u8, u16, u32), (fontdue::Metrics, Vec<u8>)>;

pub(crate) static GLYPH_CACHE: std::sync::OnceLock<std::sync::Mutex<GlyphMap>> =
    std::sync::OnceLock::new();

pub(crate) fn glyph_cache() -> &'static std::sync::Mutex<GlyphMap> {
    GLYPH_CACHE.get_or_init(std::sync::Mutex::default)
}

fn load_font(data: &'static [u8], id: u8) -> FontEntry {
    let face = Face::from_slice(data, 0).expect("font parse");
    let body = Font::from_bytes(data, fontdue::FontSettings::default()).expect("fontdue load");
    FontEntry { face, body, id }
}

pub(crate) fn fonts() -> &'static FontRegistry {
    FONTS.get_or_init(|| FontRegistry {
        default: load_font(super::FONT_LATIN, FONT_ID_DEFAULT),
        mono: load_font(super::FONT_MONO, FONT_ID_MONO),
        scripts: std::array::from_fn(|_| RwLock::new(None)),
    })
}

pub fn install_font(script: Script, data: Vec<u8>) -> bool {
    // Latin is the embedded default and Other has no face of its own.
    if matches!(script, Script::Latin | Script::Other) {
        return false;
    }
    let entry = match load_font_owned(data, script.font_id()) {
        Some(e) => e,
        None => return false,
    };
    let reg = fonts();
    if let Ok(mut guard) = reg.scripts[script.slot()].write() {
        *guard = Some(entry);
    }
    super::clear_width_cache();
    FONT_INSTALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    true
}

fn load_font_owned(data: Vec<u8>, id: u8) -> Option<FontEntry> {
    let data_boxed: Box<[u8]> = data.into_boxed_slice();
    let leaked: &'static [u8] = Box::leak(data_boxed);
    let face = Face::from_slice(leaked, 0)?;
    let body = Font::from_bytes(leaked, fontdue::FontSettings::default()).ok()?;
    Some(FontEntry { face, body, id })
}

pub fn font_covers(data: &[u8], probe_chars: &str) -> bool {
    let face = match Face::from_slice(data, 0) {
        Some(f) => f,
        None => return false,
    };
    probe_chars.chars().all(|c| face.glyph_index(c).is_some())
}

/// Face for a run: the monospace face when the style asks for it, otherwise the
/// script's own face. Bold and italic do not choose a face -- they are applied
/// to the rasterised glyph, so they compose with every script.
pub(crate) fn font_for(script: Script, style: TextStyle) -> &'static FontEntry {
    if style.mono {
        // Latin-only face. Anything it cannot draw falls through to the normal
        // per-character fallback in the blit loop, so a Bengali comment inside a
        // code block still renders.
        return &fonts().mono;
    }
    font_for_script(script)
}

pub(crate) fn font_for_script(script: Script) -> &'static FontEntry {
    let reg = fonts();
    if matches!(script, Script::Latin | Script::Other) {
        return &reg.default;
    }
    let slot = &reg.scripts[script.slot()];
    if let Ok(guard) = slot.read() {
        if let Some(ref entry) = *guard {
            // SAFETY: FontEntry is stored in a 'static RwLock inside a 'static
            // OnceLock. The entry is never removed once installed, so the
            // reference is valid for the program lifetime.
            unsafe {
                return &*(entry as *const FontEntry);
            }
        }
    }
    &reg.default
}

pub(crate) fn fallback_for_char(c: char) -> Option<(&'static FontEntry, u16)> {
    let reg = fonts();
    // Every installed face is a candidate, so a stray character from any script
    // still renders inside a run set in another.
    for slot in reg.scripts.iter() {
        if let Ok(g) = slot.read() {
            if let Some(entry) = g.as_ref() {
                if let Some(gid) = entry.face.glyph_index(c) {
                    // SAFETY: FontEntry is stored in a 'static RwLock inside a
                    // 'static OnceLock; it is never removed once installed.
                    let e: &'static FontEntry = unsafe { &*(entry as *const FontEntry) };
                    return Some((e, gid.0));
                }
            }
        }
    }
    reg.default
        .face
        .glyph_index(c)
        .map(|gid| (&reg.default, gid.0))
}

pub fn has_font_for(script: Script) -> bool {
    // Latin is embedded; Other has no dedicated face and falls back per glyph.
    if matches!(script, Script::Latin | Script::Other) {
        return true;
    }
    fonts().scripts[script.slot()]
        .read()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

pub fn line_height_for(script: Script, px_size: f32) -> usize {
    let fd = font_for_script(script);
    let lm = fd.body.horizontal_line_metrics(px_size);
    let h = match lm {
        Some(m) => m.ascent - m.descent + m.line_gap,
        None => px_size * 1.2,
    };
    h.max(1.0) as usize
}

pub fn line_height(px_size: f32) -> usize {
    line_height_for(Script::Latin, px_size)
}

pub fn word_width(word: &str, px_size: f32) -> f32 {
    word_width_styled(word, px_size, TextStyle::PLAIN)
}

/// Width of a run as it will actually be drawn.
///
/// Only `mono` changes the answer: it swaps the face, and fixed advances differ
/// from proportional ones. Synthetic bold thickens strokes without moving the
/// pen and synthetic italic shears in place, so both keep the regular advances
/// -- which is what stops emphasis from reflowing a paragraph.
pub fn word_width_styled(word: &str, px_size: f32, style: TextStyle) -> f32 {
    let key = (
        format!("{}\u{1}{}", word, style.bits()),
        (px_size * 100.0) as u32,
    );
    let cache = super::width_cache();
    if let Ok(c) = cache.lock() {
        if let Some(&w) = c.get(&key) {
            return w;
        }
    }
    let fd = font_for(super::detect_script(word), style);
    let scale = px_size / fd.face.units_per_em() as f32;
    let mut ub = rustybuzz::UnicodeBuffer::new();
    ub.push_str(word);
    let dir = if super::detect_script(word).is_rtl() {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    };
    ub.set_direction(dir);
    let gb = rustybuzz::shape(&fd.face, &[], ub);
    let w = gb
        .glyph_positions()
        .iter()
        .map(|p| p.x_advance as f32)
        .sum::<f32>()
        * scale;
    if let Ok(mut c) = cache.lock() {
        c.insert(key, w);
    }
    w
}
