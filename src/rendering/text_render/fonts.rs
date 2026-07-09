use fontdue::Font;
use rustybuzz::Face;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use super::Script;

pub(crate) const FONT_ID_DEFAULT: u8 = 0;
pub(crate) const FONT_ID_BENGALI: u8 = 1;
pub(crate) const FONT_ID_DEVANAGARI: u8 = 2;
pub(crate) const FONT_ID_ARABIC: u8 = 3;
pub(crate) const FONT_ID_CJK: u8 = 4;
pub(crate) const FONT_ID_THAI: u8 = 5;

pub(crate) struct FontEntry {
    pub face: Face<'static>,
    pub body: Font,
    pub id: u8,
}

static FONT_INSTALL_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn font_install_count() -> usize {
    FONT_INSTALL_COUNTER.load(Ordering::Relaxed)
}

pub(crate) struct FontRegistry {
    pub default: FontEntry,
    pub bengali: RwLock<Option<FontEntry>>,
    pub devanagari: RwLock<Option<FontEntry>>,
    pub arabic: RwLock<Option<FontEntry>>,
    pub cjk: RwLock<Option<FontEntry>>,
    pub thai: RwLock<Option<FontEntry>>,
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
        bengali: RwLock::new(None),
        devanagari: RwLock::new(None),
        arabic: RwLock::new(None),
        cjk: RwLock::new(None),
        thai: RwLock::new(None),
    })
}

pub fn install_font(script: Script, data: Vec<u8>) -> bool {
    let id = match script {
        Script::Bengali => FONT_ID_BENGALI,
        Script::Devanagari => FONT_ID_DEVANAGARI,
        Script::Arabic => FONT_ID_ARABIC,
        Script::Cjk => FONT_ID_CJK,
        Script::Thai => FONT_ID_THAI,
        _ => return false,
    };
    let entry = match load_font_owned(data, id) {
        Some(e) => e,
        None => return false,
    };
    let reg = fonts();
    let slot = match script {
        Script::Bengali => &reg.bengali,
        Script::Devanagari => &reg.devanagari,
        Script::Arabic => &reg.arabic,
        Script::Cjk => &reg.cjk,
        Script::Thai => &reg.thai,
        _ => return false,
    };
    if let Ok(mut guard) = slot.write() {
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

pub(crate) fn font_for_script(script: Script) -> &'static FontEntry {
    let reg = fonts();
    let slot = match script {
        Script::Bengali => &reg.bengali,
        Script::Devanagari => &reg.devanagari,
        Script::Arabic => &reg.arabic,
        Script::Cjk => &reg.cjk,
        Script::Thai => &reg.thai,
        _ => return &reg.default,
    };
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
    let slots: [&RwLock<Option<FontEntry>>; 5] = [
        &reg.cjk,
        &reg.thai,
        &reg.bengali,
        &reg.devanagari,
        &reg.arabic,
    ];
    for slot in slots.iter() {
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
    let reg = fonts();
    let slot = match script {
        Script::Bengali => &reg.bengali,
        Script::Devanagari => &reg.devanagari,
        Script::Arabic => &reg.arabic,
        Script::Cjk => &reg.cjk,
        Script::Thai => &reg.thai,
        _ => return true,
    };
    slot.read().map(|g| g.is_some()).unwrap_or(false)
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
    let key = (word.to_owned(), (px_size * 100.0) as u32);
    let cache = super::width_cache();
    if let Ok(c) = cache.lock() {
        if let Some(&w) = c.get(&key) {
            return w;
        }
    }
    let fd = font_for_script(super::detect_script(word));
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
