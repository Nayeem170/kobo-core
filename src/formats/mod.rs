// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Document formats. EPUB is the MVP format (PDF deferred - plan S8).
pub mod epub;

use epub::Chapter;

/// Detect the dominant non-Latin language from book content. Returns a BCP-47
/// code when a non-Latin script makes up >= 10% of letters, so a stray foreign
/// quote in an English book does not mis-detect.
pub fn detect_language(chapters: &[Chapter]) -> Option<String> {
    const BUDGET: usize = 262_144;
    let mut bn = 0u32;
    let mut ar = 0u32;
    let mut letters = 0u32;
    let mut scanned = 0usize;
    for ch in chapters {
        for c in ch.text.chars() {
            match c {
                '\u{0980}'..='\u{09FF}' => {
                    bn += 1;
                    letters += 1;
                }
                '\u{0600}'..='\u{06FF}' => {
                    ar += 1;
                    letters += 1;
                }
                c if c.is_alphabetic() => letters += 1,
                _ => {}
            }
            scanned += c.len_utf8().max(1);
            if scanned >= BUDGET {
                break;
            }
        }
        if scanned >= BUDGET {
            break;
        }
    }
    if letters == 0 {
        return None;
    }
    let frac_bn = bn as f32 / letters as f32;
    let frac_ar = ar as f32 / letters as f32;
    if frac_bn >= 0.10 {
        Some("bn-BD".to_string())
    } else if frac_ar >= 0.10 {
        Some("ar-SA".to_string())
    } else {
        None
    }
}

/// Fraction-read from per-chapter page offsets.
/// `offsets[c]` = page count before chapter c; `offsets.last()` = total pages.
pub fn progress_from_offsets(offsets: &[usize], chapter: usize, page: usize) -> f32 {
    let total = *offsets.last().unwrap_or(&1).max(&1);
    let overall = offsets
        .get(chapter)
        .copied()
        .unwrap_or(0)
        .saturating_add(page);
    (overall as f32 / total as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::epub::Chapter;
    use super::*;

    #[test]
    fn progress_mid_book() {
        assert_eq!(progress_from_offsets(&[0, 10, 20, 30], 1, 5), 0.5);
    }

    #[test]
    fn progress_start_is_zero() {
        assert_eq!(progress_from_offsets(&[0, 10, 20, 30], 0, 0), 0.0);
    }

    #[test]
    fn progress_end_clamps_to_one() {
        assert_eq!(progress_from_offsets(&[0, 10, 20, 30], 2, 10), 1.0);
    }

    #[test]
    fn progress_empty_offsets_is_zero() {
        assert_eq!(progress_from_offsets(&[], 0, 0), 0.0);
    }

    #[test]
    fn progress_overflow_chapter_uses_page_only() {
        assert_eq!(progress_from_offsets(&[0, 10], 9, 3), 0.3);
    }

    #[test]
    fn detect_bengali() {
        let ch = Chapter::from_xhtml(0, None, "<p>বাংলা ভাষা একটি ইন্দো-আর্য ভাষা</p>");
        assert_eq!(detect_language(&[ch]).as_deref(), Some("bn-BD"));
    }

    #[test]
    fn detect_arabic() {
        let ch = Chapter::from_xhtml(0, None, "<p>اللغة العربية لغة سامية</p>");
        assert_eq!(detect_language(&[ch]).as_deref(), Some("ar-SA"));
    }

    #[test]
    fn detect_latin_returns_none() {
        let ch = Chapter::from_xhtml(
            0,
            None,
            "<p>The quick brown fox jumps over the lazy dog.</p>",
        );
        assert_eq!(detect_language(&[ch]), None);
    }

    #[test]
    fn display_title_from_declared() {
        let ch = Chapter::from_xhtml(0, Some("  Prologue  ".into()), "<p>text</p>");
        assert_eq!(ch.display_title(0), "Prologue");
    }

    #[test]
    fn display_title_fallback_to_chapter_n() {
        let ch = Chapter::from_xhtml(2, None, "");
        assert_eq!(ch.display_title(2), "Chapter 3");
    }

    #[test]
    fn display_title_ignores_empty_title() {
        let ch = Chapter::from_xhtml(0, Some("   ".into()), "");
        assert_eq!(ch.display_title(0), "Chapter 1");
    }

    #[test]
    fn display_title_from_heading() {
        let ch = Chapter::from_xhtml(0, None, "<h2>The Beginning</h2><p>text</p>");
        assert_eq!(ch.display_title(0), "The Beginning");
    }
}
