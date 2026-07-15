use super::*;

const TEST_W: usize = 100;
const TEST_H: usize = 100;

#[test]
fn measure_text_empty_is_zero() {
    assert_eq!(measure_text("", 24.0), 0);
}

#[test]
fn measure_text_scales_with_length() {
    let short = measure_text("Hi", 24.0);
    let long = measure_text("Hello World Reader", 24.0);
    assert!(short > 0);
    assert!(long > short, "longer text must measure wider");
}

#[test]
fn measure_text_accounts_for_spaces() {
    assert!(
        measure_text("a b", 24.0) > measure_text("ab", 24.0),
        "spaces must contribute to measured width"
    );
}

#[test]
fn measure_text_grows_with_font_size() {
    let small = measure_text("Reader", 16.0);
    let large = measure_text("Reader", 48.0);
    assert!(large > small, "larger font must measure wider");
}

fn screen_buffer() -> Vec<u8> {
    vec![0u8; TEST_W * TEST_H * 2]
}

fn pixel_at(buf: &[u8], x: usize, y: usize) -> u16 {
    let off = (y * TEST_W + x) * 2;
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

const PROGRESS_FILLED: u16 = 0x1082;
const PROGRESS_BG: u16 = 0xD6BA;

#[test]
fn progress_bar_half_filled_splits_at_midpoint() {
    let w = 80usize;
    let mut buf = screen_buffer();
    paint_progress_bar(&mut buf, TEST_W, TEST_H, 0, 0, w, 0.5);
    assert_eq!(
        pixel_at(&buf, 0, 0),
        PROGRESS_FILLED,
        "first pixel must be filled"
    );
    assert_eq!(
        pixel_at(&buf, w / 2 - 1, 0),
        PROGRESS_FILLED,
        "pixel just before midpoint must be filled"
    );
    assert_eq!(
        pixel_at(&buf, w / 2, 0),
        PROGRESS_BG,
        "pixel at/after midpoint must be background"
    );
}

#[test]
fn progress_bar_zero_frac_all_background() {
    let w = 80usize;
    let mut buf = screen_buffer();
    paint_progress_bar(&mut buf, TEST_W, TEST_H, 10, 20, w, 0.0);
    for rx in 0..w {
        assert_eq!(
            pixel_at(&buf, 10 + rx, 20),
            PROGRESS_BG,
            "frac=0 must fill nothing"
        );
    }
}

#[test]
fn progress_bar_full_frac_all_filled() {
    let w = 100usize;
    let mut buf = screen_buffer();
    paint_progress_bar(&mut buf, TEST_W, TEST_H, 0, 0, w, 1.0);
    for rx in 0..w {
        assert_eq!(
            pixel_at(&buf, rx, 0),
            PROGRESS_FILLED,
            "frac=1.0 must fill the whole bar"
        );
    }
}

#[test]
fn progress_bar_is_bounds_safe_off_screen() {
    let mut buf = screen_buffer();
    let before = buf.clone();
    paint_progress_bar(&mut buf, TEST_W, TEST_H, TEST_W, TEST_H, 50, 0.5);
    assert_eq!(buf, before, "fully off-screen bar must write nothing");
}
