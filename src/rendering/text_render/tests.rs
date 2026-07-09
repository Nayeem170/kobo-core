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
fn detect_script_cjk() {
    assert_eq!(detect_script("こんにちは"), Script::Cjk);
}

#[test]
fn detect_script_skips_whitespace_punct() {
    assert_eq!(detect_script("  \"hello"), Script::Latin);
    assert_eq!(detect_script("  \nনমস্কার"), Script::Bengali);
}

#[test]
fn detect_script_skips_unicode_quote_prefix() {
    assert_eq!(detect_script("\u{201c}আমি\u{201d}"), Script::Bengali);
    assert_eq!(detect_script("“こんにちは”"), Script::Cjk);
}

#[test]
fn detect_script_skips_digit_prefix() {
    assert_eq!(detect_script("1. নমস্কার"), Script::Bengali);
    assert_eq!(detect_script("2. 日本語"), Script::Cjk);
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
    assert!(!Script::Cjk.uses_word_spacing());
    assert!(!Script::Thai.uses_word_spacing());
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
