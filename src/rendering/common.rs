// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Shared rendering state and constants.

use std::sync::atomic::{AtomicBool, Ordering};

static IS_RTL: AtomicBool = AtomicBool::new(false);

pub fn set_rtl(rtl: bool) {
    IS_RTL.store(rtl, Ordering::Relaxed);
}

pub fn is_rtl() -> bool {
    IS_RTL.load(Ordering::Relaxed)
}

/// Whether a BCP-47 language code (e.g. "ar-SA", "en-US") resolves to an RTL
/// script. Complements [`crate::rendering::text_render::Script::is_rtl`], which
/// works on text content; this works on a declared language tag.
pub fn lang_is_rtl(lang: Option<&str>) -> bool {
    let prefix = lang
        .and_then(|l| l.split('-').next())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        prefix.as_str(),
        "ar" | "ur" | "fa" | "he" | "yi" | "ps" | "sd"
    )
}

pub const BODY_PX: f32 = 36.0;

/// Reinterpret a slice of any type as a shared `&[u8]`.
///
/// SAFETY: the slice covers exactly `size_of_val(buf)` bytes starting at the
/// same address. `u8` has no alignment requirement, so any `T` is valid.
pub fn slice_as_bytes<T>(buf: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, std::mem::size_of_val(buf)) }
}

/// Reinterpret a mutable slice of any type as a `&mut [u8]`.
///
/// SAFETY: same as [`slice_as_bytes`], but with an exclusive `&mut` borrow.
pub fn slice_as_bytes_mut<T>(buf: &mut [T]) -> &mut [u8] {
    unsafe {
        std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, std::mem::size_of_val(buf))
    }
}
