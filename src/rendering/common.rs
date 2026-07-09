//! Shared rendering state and constants.

use std::sync::atomic::{AtomicBool, Ordering};

static IS_RTL: AtomicBool = AtomicBool::new(false);

pub fn set_rtl(rtl: bool) {
    IS_RTL.store(rtl, Ordering::Relaxed);
}

pub fn is_rtl() -> bool {
    IS_RTL.load(Ordering::Relaxed)
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
