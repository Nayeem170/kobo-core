// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Font loading from the on-device font directory.
//!
//! Non-Latin fonts (Bengali/Devanagari/Arabic/Thai/CJK) are NOT downloaded at
//! runtime - the Kobo wget cannot complete HTTPS. Instead they are fetched by
//! the USB installer (`install-usb.ps1`) and placed in `/mnt/onboard/.adds/fonts`
//! at install time. This module just loads them from disk at startup / on demand.
//! The default font (NotoSansLatin, covering Latin) is embedded in the binary.

use crate::device::paths::{FONTS_DIR, SYSTEM_FONTS_DIR, USER_FONTS_DIR};
use crate::rendering::text_render::{
    detect_script, font_covers, has_font_for, install_font, Script,
};
use std::fs;
use std::path::PathBuf;

struct FontSpec {
    script: Script,
    filename: &'static str,
    label: &'static str,
    /// Representative characters for this script; a candidate font must have
    /// glyphs for all of them to be considered a match.
    probe: &'static str,
}

/// One entry per loadable script. Adding a script means adding a line here and
/// a variant to `Script` - nothing else in the font pipeline needs touching.
///
/// Probes must be characters the *target* font is guaranteed to have. Keep them
/// script-specific: a probe shared with another script would let the wrong face
/// satisfy the check.
const FONT_SPECS: &[FontSpec] = &[
    FontSpec {
        script: Script::Bengali,
        filename: "NotoSansBengali.ttf",
        label: "Bengali",
        probe: "অবংশ",
    },
    FontSpec {
        script: Script::Devanagari,
        filename: "NotoSansDevanagari.ttf",
        label: "Hindi",
        probe: "अकमश",
    },
    FontSpec {
        script: Script::Arabic,
        filename: "NotoSansArabic.ttf",
        label: "Arabic",
        probe: "ابتة",
    },
    FontSpec {
        script: Script::Hebrew,
        filename: "NotoSansHebrew.ttf",
        label: "Hebrew",
        probe: "אבגד",
    },
    // Greek and Cyrillic are not separate Noto families - both live in the base
    // NotoSans face alongside Latin, so the two specs share one file.
    FontSpec {
        script: Script::Greek,
        filename: "NotoSans.ttf",
        label: "Greek",
        probe: "αβγδ",
    },
    FontSpec {
        script: Script::Cyrillic,
        filename: "NotoSans.ttf",
        label: "Cyrillic",
        probe: "абвг",
    },
    FontSpec {
        script: Script::Georgian,
        filename: "NotoSansGeorgian.ttf",
        label: "Georgian",
        probe: "აბგდ",
    },
    FontSpec {
        script: Script::Armenian,
        filename: "NotoSansArmenian.ttf",
        label: "Armenian",
        probe: "աբգդ",
    },
    FontSpec {
        script: Script::Ethiopic,
        filename: "NotoSansEthiopic.ttf",
        label: "Amharic",
        probe: "ሀለሐመ",
    },
    FontSpec {
        script: Script::Gujarati,
        filename: "NotoSansGujarati.ttf",
        label: "Gujarati",
        probe: "અકગમ",
    },
    FontSpec {
        script: Script::Gurmukhi,
        filename: "NotoSansGurmukhi.ttf",
        label: "Punjabi",
        probe: "ਅਕਗਮ",
    },
    FontSpec {
        script: Script::Tamil,
        filename: "NotoSansTamil.ttf",
        label: "Tamil",
        probe: "அகசத",
    },
    FontSpec {
        script: Script::Telugu,
        filename: "NotoSansTelugu.ttf",
        label: "Telugu",
        probe: "అకగమ",
    },
    FontSpec {
        script: Script::Kannada,
        filename: "NotoSansKannada.ttf",
        label: "Kannada",
        probe: "ಅಕಗಮ",
    },
    FontSpec {
        script: Script::Malayalam,
        filename: "NotoSansMalayalam.ttf",
        label: "Malayalam",
        probe: "അകഗമ",
    },
    FontSpec {
        script: Script::Sinhala,
        filename: "NotoSansSinhala.ttf",
        label: "Sinhala",
        probe: "අකගම",
    },
    FontSpec {
        script: Script::Thai,
        filename: "NotoSansThai.ttf",
        label: "Thai",
        probe: "กขคง",
    },
    FontSpec {
        script: Script::Lao,
        filename: "NotoSansLao.ttf",
        label: "Lao",
        probe: "ກຂຄງ",
    },
    FontSpec {
        script: Script::Khmer,
        filename: "NotoSansKhmer.ttf",
        label: "Khmer",
        probe: "កខគង",
    },
    FontSpec {
        script: Script::Myanmar,
        filename: "NotoSansMyanmar.ttf",
        label: "Burmese",
        probe: "ကခဂဃ",
    },
    FontSpec {
        script: Script::Japanese,
        filename: "NotoSansJP.ttf",
        label: "Japanese",
        // Kana: present in a Japanese face, absent from a Chinese-only one.
        probe: "あいカキ",
    },
    FontSpec {
        script: Script::Korean,
        filename: "NotoSansKR.ttf",
        label: "Korean",
        probe: "가나다라",
    },
    FontSpec {
        script: Script::Chinese,
        filename: "NotoSansSC.ttf",
        label: "Chinese",
        // Simplified-only forms, so a Japanese face cannot satisfy this.
        probe: "这说门车",
    },
];

fn font_path(filename: &str) -> PathBuf {
    PathBuf::from(FONTS_DIR).join(filename)
}

fn spec_for_script(script: Script) -> Option<&'static FontSpec> {
    FONT_SPECS.iter().find(|s| s.script == script)
}

/// Heavy CJK faces (4-9 MB each, ~1.5 s parse time per file) are NOT loaded at
/// boot - they would add ~3 s to every cold launch for scripts most books never
/// use. They load on demand via `ensure_font_for_script` (called at book-open in
/// `open_book.rs`) the first time a book in that script is opened.
fn is_lazy_script(script: Script) -> bool {
    matches!(script, Script::Japanese | Script::Korean | Script::Chinese)
}

/// Try to load a font from disk. Returns true on success.
fn try_load_from_disk(spec: &FontSpec) -> bool {
    let path = font_path(spec.filename);
    match fs::read(&path) {
        Ok(data) => {
            log::info!(
                "font: loaded {} from disk ({} bytes)",
                spec.filename,
                data.len()
            );
            install_font(spec.script, data)
        }
        Err(_) => false,
    }
}

/// Map a BCP-47 language tag to a Script.
fn lang_to_script(lang: &str) -> Script {
    let lower = lang.to_lowercase();
    let prefix = lower.split('-').next().unwrap_or(&lower);
    match prefix {
        "bn" => Script::Bengali,
        "hi" | "mr" | "ne" => Script::Devanagari,
        "ar" | "ur" | "fa" | "ps" => Script::Arabic,
        "he" | "iw" => Script::Hebrew,
        "el" => Script::Greek,
        // Kazakh and Mongolian are written in Cyrillic in the locales Edge
        // offers voices for (kk-KZ, mn-MN).
        "ru" | "uk" | "bg" | "mk" | "sr" | "kk" | "mn" | "be" => Script::Cyrillic,
        "ka" => Script::Georgian,
        "hy" => Script::Armenian,
        "am" | "ti" => Script::Ethiopic,
        "gu" => Script::Gujarati,
        "pa" => Script::Gurmukhi,
        "ta" => Script::Tamil,
        "te" => Script::Telugu,
        "kn" => Script::Kannada,
        "ml" => Script::Malayalam,
        "si" => Script::Sinhala,
        "th" => Script::Thai,
        "lo" => Script::Lao,
        "km" => Script::Khmer,
        "my" => Script::Myanmar,
        "ja" => Script::Japanese,
        "ko" => Script::Korean,
        // Covers zh-CN, zh-TW and zh-HK. NotoSansSC carries the traditional
        // forms too, so Taiwan and Hong Kong render - only the preferred glyph
        // shapes differ from a dedicated NotoSansTC.
        "zh" => Script::Chinese,
        _ => Script::Latin,
    }
}

/// Directories searched for usable fonts, in priority order:
///   1. installer-shipped NotoSans fonts (always present after a USB install)
///   2. user side-loaded fonts (/mnt/onboard/fonts = Kobo's "fonts" folder)
///   3. Kobo's bundled system fonts (/usr/local/Kobo/fonts, on the rootfs)
const FONT_SEARCH_DIRS: &[&str] = &[FONTS_DIR, USER_FONTS_DIR, SYSTEM_FONTS_DIR];

/// List every font file in the search dirs (diagnostic: shows what the device
/// has available so we can reuse Kobo's own fonts instead of shipping them).
pub fn log_available_fonts() {
    for dir in FONT_SEARCH_DIRS {
        match fs::read_dir(dir) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| {
                        let p = e.path();
                        let ext = p
                            .extension()
                            .and_then(|x| x.to_str())
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        if matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                names.sort();
                log::debug!("font dir {}: {} file(s) {:?}", dir, names.len(), names);
            }
            Err(_) => log::debug!("font dir {}: (not present)", dir),
        }
    }
}

/// Search the non-shipped font dirs for the first file whose glyphs cover the
/// script's probe characters. Lets the app reuse the Kobo's bundled fonts (or
/// user side-loaded ones) instead of requiring a shipped NotoSans file.
fn find_covering_font(spec: &FontSpec) -> Option<PathBuf> {
    for dir in FONT_SEARCH_DIRS {
        if *dir == FONTS_DIR {
            continue; // shipped fonts handled by try_load_from_disk
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e.flatten().collect::<Vec<_>>(),
            Err(_) => continue,
        };
        for entry in entries {
            let p = entry.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            if let Ok(data) = fs::read(&p) {
                if font_covers(&data, spec.probe) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Load fonts at startup. For each script: prefer the shipped NotoSans file
/// (installer-provided), then fall back to any device font that covers the
/// script. No network needed.
pub fn load_cached_fonts() {
    let mut loaded = 0;
    for spec in FONT_SPECS {
        if is_lazy_script(spec.script) {
            continue;
        }
        if try_load_from_disk(spec) {
            loaded += 1;
            continue;
        }
        if let Some(path) = find_covering_font(spec) {
            log::info!(
                "font: {} -> using {} (covers {})",
                spec.label,
                path.display(),
                spec.label
            );
            if let Ok(data) = fs::read(&path) {
                if install_font(spec.script, data) {
                    loaded += 1;
                    continue;
                }
            }
        }
        log::warn!("font: no {} font found on device", spec.label);
    }
    if loaded > 0 {
        log::info!("font: {loaded} loaded");
    }
}

/// Ensure a font for a book's script is loaded. Returns a status string for the
/// tips display when the font is missing (install wasn't run / font deleted):
/// - Font already installed or on disk -> None (loaded if needed)
/// - Font not on disk -> a short message telling the user to reinstall
pub fn ensure_font_for_script(lang: Option<&str>, sample_text: &str) -> Option<String> {
    // Prefer the declared language, but only when it actually points at a
    // loadable script. Many EPUBs declare `dc:language = en` while their body is
    // CJK (or another non-Latin script): trusting that would detect Latin and
    // skip loading the real face, leaving the page blank. Fall back to content
    // detection whenever the tag resolves to Latin/Other.
    let script = lang
        .map(lang_to_script)
        .filter(|s| !matches!(s, Script::Latin | Script::Other))
        .unwrap_or_else(|| detect_script(sample_text));

    if has_font_for(script) {
        return None;
    }

    let spec = spec_for_script(script)?;

    if try_load_from_disk(spec) {
        return None;
    }

    // Fall back to any device font that covers this script.
    if let Some(path) = find_covering_font(spec) {
        log::info!(
            "font: {} -> using {} (covers {})",
            spec.label,
            path.display(),
            spec.label
        );
        if let Ok(data) = fs::read(&path) {
            if install_font(spec.script, data) {
                return None;
            }
        }
    }

    Some(format!("Reinstall to add {} font", spec.label))
}
