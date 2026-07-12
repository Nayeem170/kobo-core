//! Rendering primitives: text engine, UI draw helpers, e-ink constants.
//!
//! - [`text_render`] - HarfBuzz shaping + fontdue rasterization -> RGB565
//! - [`common`] - shared rendering state (`IS_RTL`) and body-text size constant
//! - [`layout`] - `word_wrap_bytes()` and `sentences_with_ranges()` (pure text)
//! - [`draw`] - progress bars, rounded rects, nav bars (RGB565 byte-buffer ops)
//! - [`eink`] - MXCFB ioctl structs, waveform constants, `diff_rows()`

pub mod common;
pub mod draw;
pub mod eink;
pub mod layout;
pub mod loader;
pub mod text_render;
