// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Physical-size-stable sizing across the Kobo fleet.
//!
//! Kobo's `fb_var_screeninfo` reports 0mm for the panel's physical size on most
//! models, so ppi cannot be derived from millimetres reliably. Instead it is
//! looked up from the panel resolution (the same approach KOReader takes).
//!
//! `dp(n)` returns `n` design units expressed at the reference density, scaled
//! to the current panel so a control keeps the same real-world size on a 212ppi
//! Nia and a 300ppi Sage alike. The existing UI was tuned on a 300ppi panel, so
//! wrapping a former raw-pixel literal in `dp()` leaves 300ppi devices untouched
//! and only rescales the rest.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Reference density: `dp(n) == n` at 300ppi, the most common modern Kobo panel.
pub const REF_PPI: usize = 300;

static PPI: AtomicUsize = AtomicUsize::new(REF_PPI);

/// Resolve ppi from a panel resolution. Orientation-agnostic (sorts the pair),
/// so it works whether the framebuffer reports portrait or landscape.
///
/// The 1404x1872 row resolves to the Elipsa / Elipsa 2E (227ppi). It collides
/// with the long-discontinued Aura ONE (300ppi), which would get a slightly
/// oversized UI; the trade favours the device people still own.
pub fn ppi_for(w: usize, h: usize) -> usize {
    let (lo, hi) = if w <= h { (w, h) } else { (h, w) };
    match (lo, hi) {
        (600, 800) => 167,   // Touch, Mini
        (758, 1024) => 212,  // Glo, Aura, Nia
        (1072, 1448) => 300, // Glo HD, Clara HD, Clara 2E, Clara Colour
        (1080, 1430) => 265, // Aura H2O
        (1264, 1680) => 300, // Libra H2O, Libra 2, Libra Colour
        (1404, 1872) => 227, // Elipsa, Elipsa 2E
        (1440, 1920) => 300, // Forma, Sage
        _ => REF_PPI,
    }
}

/// Latch the panel density once at startup. Call from `setup` after the
/// framebuffer resolution is known.
pub fn init_ppi(w: usize, h: usize) {
    PPI.store(ppi_for(w, h), Ordering::Relaxed);
}

pub fn ppi() -> usize {
    PPI.load(Ordering::Relaxed)
}

/// Physical-size-stable pixels: `n` design units at the reference 300ppi,
/// scaled to the current panel.
pub fn dp(n: i32) -> i32 {
    dp_at(n, ppi())
}

/// Explicit-ppi variant of [`dp`]: the pure, testable core that does not read
/// the global PPI latch. Call this when the panel density is known locally
/// (e.g. inside a layout computation that already received `ppi` as a param).
pub fn dp_at(n: i32, ppi: usize) -> i32 {
    (n * ppi as i32) / REF_PPI as i32
}

/// Float variant for font sizes and sub-pixel geometry.
pub fn dpf(n: f32) -> f32 {
    n * ppi() as f32 / REF_PPI as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppi_table_hits_known_panels() {
        assert_eq!(ppi_for(758, 1024), 212); // Nia
        assert_eq!(ppi_for(1072, 1448), 300); // Clara
        assert_eq!(ppi_for(1264, 1680), 300); // Libra 2
        assert_eq!(ppi_for(1404, 1872), 227); // Elipsa (not Aura ONE)
        assert_eq!(ppi_for(1440, 1920), 300); // Sage
    }

    #[test]
    fn ppi_is_orientation_agnostic() {
        assert_eq!(ppi_for(1024, 758), ppi_for(758, 1024));
    }

    #[test]
    fn ppi_unknown_panel_falls_back_to_reference() {
        assert_eq!(ppi_for(999, 999), REF_PPI);
    }

    #[test]
    fn dp_is_identity_at_reference_density() {
        PPI.store(REF_PPI, Ordering::Relaxed);
        assert_eq!(dp(76), 76);
        assert_eq!(dp(300), 300);
    }

    #[test]
    fn dp_shrinks_on_low_density_panel() {
        PPI.store(212, Ordering::Relaxed);
        // 76dp button stays the same physical size: ~54px on a 212ppi panel.
        assert_eq!(dp(76), 76 * 212 / 300);
        PPI.store(REF_PPI, Ordering::Relaxed); // restore for other tests
    }

    #[test]
    fn dp_at_matches_dp_at_same_ppi() {
        PPI.store(265, Ordering::Relaxed);
        assert_eq!(dp_at(100, 265), dp(100));
        assert_eq!(dp_at(0, 265), 0);
        PPI.store(REF_PPI, Ordering::Relaxed);
    }

    #[test]
    fn dp_at_is_identity_at_reference() {
        assert_eq!(dp_at(300, REF_PPI), 300);
    }
}
