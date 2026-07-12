//! Font loading from the on-device font directory.
//!
//! Non-Latin fonts (Bengali/Devanagari/Arabic/Thai/CJK) are NOT downloaded at
//! runtime - the Kobo wget cannot complete HTTPS. Instead they are fetched by
//! the USB installer (`install-usb.ps1`) and placed in `/mnt/onboard/.adds/fonts`
//! at install time. This module just loads them from disk at startup / on demand.
//! The default font (NotoSansLatin, covering Latin) is embedded in the binary.

use crate::rendering::text_render::{
    detect_script, font_covers, has_font_for, install_font, Script,
};
use std::fs;
use std::path::PathBuf;

const FONTS_DIR: &str = "/mnt/onboard/.adds/fonts";

struct FontSpec {
    script: Script,
    filename: &'static str,
    label: &'static str,
    /// Representative characters for this script; a candidate font must have
    /// glyphs for all of them to be considered a match.
    probe: &'static str,
}

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
        script: Script::Cjk,
        filename: "NotoSansJP.ttf",
        label: "Japanese",
        probe: "日本語字",
    },
    FontSpec {
        script: Script::Thai,
        filename: "NotoSansThai.ttf",
        label: "Thai",
        probe: "กขคง",
    },
];

fn font_path(filename: &str) -> PathBuf {
    PathBuf::from(FONTS_DIR).join(filename)
}

fn spec_for_script(script: Script) -> Option<&'static FontSpec> {
    FONT_SPECS.iter().find(|s| s.script == script)
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
        "ar" | "ur" | "fa" => Script::Arabic,
        "ja" | "zh" | "ko" => Script::Cjk,
        "th" => Script::Thai,
        _ => Script::Latin,
    }
}

/// Directories searched for usable fonts, in priority order:
///   1. installer-shipped NotoSans fonts (always present after a USB install)
///   2. user side-loaded fonts (/mnt/onboard/fonts = Kobo's "fonts" folder)
///   3. Kobo's bundled system fonts (/usr/local/Kobo/fonts, on the rootfs)
const FONT_SEARCH_DIRS: &[&str] = &[
    "/mnt/onboard/.adds/fonts",
    "/mnt/onboard/fonts",
    "/usr/local/Kobo/fonts",
];

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
    let script = lang
        .map(lang_to_script)
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
