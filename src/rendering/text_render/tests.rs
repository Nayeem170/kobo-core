// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use super::*;

fn white_buffer(w: usize, h: usize) -> Vec<u8> {
    vec![0xFF; w * h * 2]
}

#[test]
fn line_height_positive_for_body_size() {
    let lh = line_height(36.0);
    assert!(lh > 0);
    assert!(lh < 100);
}

#[test]
fn line_height_scales_with_size() {
    let small = line_height(20.0);
    let large = line_height(60.0);
    assert!(large > small);
}

#[test]
fn detect_script_latin() {
    assert_eq!(detect_script("Hello world"), Script::Latin);
}

#[test]
fn detect_script_bengali() {
    assert_eq!(detect_script("নমস্কার"), Script::Bengali);
}

#[test]
fn detect_script_arabic() {
    assert_eq!(detect_script("مرحبا"), Script::Arabic);
}

#[test]
fn detect_script_devanagari() {
    assert_eq!(detect_script("नमस्ते"), Script::Devanagari);
}

#[test]
fn detect_script_japanese_from_kana() {
    assert_eq!(detect_script("こんにちは"), Script::Japanese);
    // Kana mixed with Han is still Japanese - the Han belongs to the kana.
    assert_eq!(detect_script("日本語を読む"), Script::Japanese);
}

#[test]
fn detect_script_korean_from_hangul() {
    assert_eq!(detect_script("안녕하세요"), Script::Korean);
}

#[test]
fn detect_script_chinese_from_bare_han() {
    // Han with no kana anywhere. Japanese prose never runs this long without
    // kana, so unaccompanied Han resolves to Chinese.
    assert_eq!(detect_script("我们在读一本书"), Script::Chinese);
}

#[test]
fn detect_script_covers_the_newly_added_scripts() {
    assert_eq!(detect_script("Привет мир"), Script::Cyrillic);
    assert_eq!(detect_script("Καλημέρα"), Script::Greek);
    assert_eq!(detect_script("שלום עולם"), Script::Hebrew);
    assert_eq!(detect_script("வணக்கம்"), Script::Tamil);
    assert_eq!(detect_script("ជំរាបសួរ"), Script::Khmer);
    assert_eq!(detect_script("မင်္ဂလာပါ"), Script::Myanmar);
    assert_eq!(detect_script("ສະບາຍດີ"), Script::Lao);
    assert_eq!(detect_script("ආයුබෝවන්"), Script::Sinhala);
    assert_eq!(detect_script("გამარჯობა"), Script::Georgian);
    assert_eq!(detect_script("ሰላም"), Script::Ethiopic);
}

#[test]
fn detect_script_takes_the_majority_not_the_first_letter() {
    // A Bengali chapter that opens with an English word is still Bengali.
    assert_eq!(
        detect_script("Chapter এক নতুন দিন শুরু হলো আজ"),
        Script::Bengali
    );
    // An English sentence quoting one Greek word is still English.
    assert_eq!(detect_script("the word λόγος means reason"), Script::Latin);
}

#[test]
fn detect_script_latin_covers_vietnamese_diacritics() {
    // Latin Extended Additional. Without it these fall to `Other` and get read
    // aloud in English.
    assert_eq!(detect_script("Xin chào các bạn"), Script::Latin);
}

#[test]
fn detect_script_skips_whitespace_punct() {
    assert_eq!(detect_script("  \"hello"), Script::Latin);
    assert_eq!(detect_script("  \nনমস্কার"), Script::Bengali);
}

#[test]
fn detect_script_skips_unicode_quote_prefix() {
    assert_eq!(detect_script("\u{201c}আমি\u{201d}"), Script::Bengali);
    assert_eq!(
        detect_script("\u{201c}\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{201d}"),
        Script::Japanese
    );
}

#[test]
fn detect_script_skips_digit_prefix() {
    assert_eq!(detect_script("1. নমস্কার"), Script::Bengali);
    // Bare Han, so this resolves to Chinese. A Japanese book reaches Japanese
    // via its kana, or via `dc:language` when the run is this short.
    assert_eq!(detect_script("2. 日本語"), Script::Chinese);
}

#[test]
fn detect_script_empty_defaults_latin() {
    assert_eq!(detect_script(""), Script::Latin);
}

#[test]
fn rtl_detection() {
    assert!(Script::Arabic.is_rtl());
    assert!(Script::Hebrew.is_rtl());
    assert!(!Script::Latin.is_rtl());
    assert!(!Script::Bengali.is_rtl());
}

#[test]
fn word_spacing_detection() {
    assert!(Script::Latin.uses_word_spacing());
    assert!(Script::Bengali.uses_word_spacing());
    assert!(!Script::Japanese.uses_word_spacing());
    assert!(!Script::Chinese.uses_word_spacing());
    assert!(!Script::Korean.uses_word_spacing());
    assert!(!Script::Thai.uses_word_spacing());
    assert!(!Script::Khmer.uses_word_spacing());
    assert!(!Script::Lao.uses_word_spacing());
    assert!(!Script::Myanmar.uses_word_spacing());
}

#[test]
fn word_width_positive_for_nonempty() {
    assert!(word_width("hello", 36.0) > 0.0);
}

#[test]
fn word_width_empty_is_zero() {
    assert_eq!(word_width("", 36.0), 0.0);
}

#[test]
fn word_width_longer_word_is_wider() {
    let short = word_width("hi", 36.0);
    let long = word_width("hello world", 36.0);
    assert!(long > short);
}

#[test]
fn decode_image_returns_correct_dimensions() {
    let img = image::DynamicImage::new_rgb8(20, 15);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    let decoded = decode_image(&buf, 100, 100).unwrap();
    assert!(decoded.width > 0 && decoded.height > 0);
    assert_eq!(decoded.rgb.len(), decoded.width * decoded.height * 3);
}

#[test]
fn decode_image_caps_height() {
    let img = image::DynamicImage::new_rgb8(10, 200);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    let h = decode_image(&buf, 100, 50).unwrap().height;
    assert!(h <= 50);
}

#[test]
fn decode_image_returns_none_for_invalid_data() {
    assert!(decode_image(b"not an image", 100, 100).is_none());
}

#[test]
fn decode_image_returns_none_for_empty() {
    assert!(decode_image(&[], 100, 100).is_none());
}

#[test]
fn blit_draws_visible_text() {
    let (w, h) = (200usize, 64usize);
    let mut buf = white_buffer(w, h);
    blit_rgb565(&mut buf, w, "Hello", 32.0, 10, 10, w, h);
    assert!(
        buf.iter().any(|&b| b != 0xFF),
        "blitting visible text must change at least one pixel"
    );
}

#[test]
fn blit_empty_text_changes_nothing() {
    let (w, h) = (100usize, 32usize);
    let mut buf = white_buffer(w, h);
    let before = buf.clone();
    blit_rgb565(&mut buf, w, "", 32.0, 0, 0, w, h);
    assert_eq!(buf, before, "empty text must not modify the buffer");
}

#[test]
fn blit_out_of_bounds_origin_is_safe() {
    let (w, h) = (50usize, 20usize);
    let mut buf = white_buffer(w, h);
    let before = buf.clone();
    blit_rgb565(&mut buf, w, "Wide", 24.0, 1000, 1000, w, h);
    assert_eq!(buf, before, "OOB blit must not modify the buffer");
}

#[test]
fn blit_color_tints_toward_color() {
    let (w, h) = (120usize, 40usize);
    let mut buf = white_buffer(w, h);
    blit_rgb565_color(&mut buf, w, "ABC", 28.0, 5, 5, 0xF800, w, h);
    let changed: Vec<u16> = (0..w * h)
        .map(|i| {
            let o = i * 2;
            (buf[o] as u16) | ((buf[o + 1] as u16) << 8)
        })
        .filter(|&v| v != 0xFFFF)
        .collect();
    assert!(!changed.is_empty(), "text must be drawn");
    assert!(
        changed.iter().all(|&v| (v >> 11) & 0x1f != 0),
        "changed pixels must carry red color"
    );
}

// ---- styled text ---------------------------------------------------------

fn ink(text: &str, px: f32, style: TextStyle) -> usize {
    let (w, h) = (400usize, 120usize);
    let mut buf = vec![0xFFu8; w * h * 2];
    blit_rgb565_styled(&mut buf, w, text, px, 10, 10, 0x0000, style, w, h);
    buf.chunks_exact(2)
        .filter(|p| p[0] != 0xFF || p[1] != 0xFF)
        .count()
}

/// Monospace is a real face, so it must actually change the metrics -- that is
/// the whole reason it is bundled rather than synthesised.
#[test]
fn mono_changes_advances() {
    let plain = word_width("iiiii", 40.0);
    let mono = word_width_styled(
        "iiiii",
        40.0,
        TextStyle {
            mono: true,
            ..Default::default()
        },
    );
    assert!(
        (mono - plain).abs() > 1.0,
        "narrow glyphs should widen in a fixed-advance face: {plain} vs {mono}"
    );
}

/// The defining property of a monospace face: every character takes the same
/// space, which is what makes code columns line up.
#[test]
fn mono_advances_are_uniform() {
    let style = TextStyle {
        mono: true,
        ..Default::default()
    };
    let narrow = word_width_styled("iiii", 40.0, style);
    let wide = word_width_styled("MMMM", 40.0, style);
    assert!(
        (narrow - wide).abs() < 0.5,
        "fixed advances should match: {narrow} vs {wide}"
    );
}

/// Bold and italic must NOT change advances, or emphasis would reflow the
/// paragraph around it.
#[test]
fn synthetic_emphasis_keeps_the_regular_advances() {
    let plain = word_width("Hello world", 40.0);
    for style in [
        TextStyle {
            bold: true,
            ..Default::default()
        },
        TextStyle {
            italic: true,
            ..Default::default()
        },
        TextStyle {
            bold: true,
            italic: true,
            ..Default::default()
        },
    ] {
        let w = word_width_styled("Hello world", 40.0, style);
        assert_eq!(w, plain, "style {style:?} moved the pen");
    }
}

#[test]
fn synthetic_bold_inks_more_than_regular() {
    let plain = ink("Hello", 40.0, TextStyle::PLAIN);
    let bold = ink(
        "Hello",
        40.0,
        TextStyle {
            bold: true,
            ..Default::default()
        },
    );
    assert!(
        bold > plain,
        "bold should thicken strokes: {plain} -> {bold}"
    );
}

/// Shearing must lean the glyphs, not simply redraw them in place.
#[test]
fn synthetic_italic_moves_pixels() {
    let (w, h) = (400usize, 120usize);
    let render = |style| {
        let mut buf = vec![0xFFu8; w * h * 2];
        blit_rgb565_styled(&mut buf, w, "Hello", 40.0, 10, 10, 0x0000, style, w, h);
        buf
    };
    let plain = render(TextStyle::PLAIN);
    let italic = render(TextStyle {
        italic: true,
        ..Default::default()
    });
    assert_ne!(
        plain, italic,
        "italic should not render identically to regular"
    );
}

#[test]
fn plain_style_renders_exactly_as_the_unstyled_path() {
    let (w, h) = (400usize, 120usize);
    let mut a = vec![0xFFu8; w * h * 2];
    let mut b = vec![0xFFu8; w * h * 2];
    blit_rgb565(&mut a, w, "Hello world", 36.0, 10, 10, w, h);
    blit_rgb565_styled(
        &mut b,
        w,
        "Hello world",
        36.0,
        10,
        10,
        0x0000,
        TextStyle::PLAIN,
        w,
        h,
    );
    assert_eq!(a, b, "adding styles must not disturb ordinary text");
}

#[test]
fn style_bits_are_distinct() {
    let mut seen = std::collections::HashSet::new();
    for bold in [false, true] {
        for italic in [false, true] {
            for mono in [false, true] {
                assert!(seen.insert(TextStyle { bold, italic, mono }.bits()));
            }
        }
    }
    assert!(TextStyle::PLAIN.is_plain());
}

/// The CJK faces ship as CFF-outline OTF (noto-cjk publishes no static TTF),
/// while every other face is TrueType. `fontdue` rasterises via
/// `ttf_parser::outline_glyph`, which handles both - but a face that parses and
/// then rasterises to nothing would be a silent blank screen on the device, so
/// this asserts real glyph coverage and a non-empty bitmap.
///
/// Skips when the font set has not been fetched; see `scripts/fetch-fonts.ps1`.
#[test]
fn shipped_faces_parse_and_rasterise() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("package").join("fonts"));
    let Some(dir) = dir else { return };
    if !dir.is_dir() {
        return;
    }

    // (file, a character that face must be able to draw)
    let cases = [
        ("NotoSansSC.ttf", '这'),
        ("NotoSansKR.ttf", '가'),
        ("NotoSansJP.ttf", 'あ'),
        ("NotoSansHebrew.ttf", 'א'),
        ("NotoSansTamil.ttf", 'அ'),
        ("NotoSans.ttf", 'Ж'),
    ];

    let mut checked = 0;
    for (name, ch) in cases {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).expect("read font");

        assert!(
            font_covers(&data, &ch.to_string()),
            "{name}: no glyph for {ch:?}"
        );

        let font = fontdue::Font::from_bytes(data.as_slice(), fontdue::FontSettings::default())
            .unwrap_or_else(|e| panic!("{name}: fontdue rejected the face: {e}"));

        let (metrics, bitmap) = font.rasterize(ch, 32.0);
        assert!(
            metrics.width > 0 && metrics.height > 0,
            "{name}: {ch:?} rasterised to a zero-size bitmap"
        );
        assert!(
            bitmap.iter().any(|&p| p > 0),
            "{name}: {ch:?} rasterised to an entirely blank bitmap"
        );
        checked += 1;
    }

    assert!(checked > 0, "font dir present but no expected faces found");
}
