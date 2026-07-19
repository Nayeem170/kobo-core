// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use super::{clamp_page, paginate_heights, resolve_progress_target};

#[test]
fn paginate_basic_split() {
    let heights = [100, 100, 100, 100, 100];
    let pages = paginate_heights(&heights, 250, &[]);
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0], (0, 2));
    assert_eq!(pages[1], (2, 4));
    assert_eq!(pages[2], (4, 5));
}

#[test]
fn paginate_single_page() {
    let heights = [50, 50];
    let pages = paginate_heights(&heights, 200, &[]);
    assert_eq!(pages, vec![(0, 2)]);
}

#[test]
fn paginate_empty() {
    let pages = paginate_heights(&[], 200, &[]);
    assert_eq!(pages, vec![(0, 0)]);
}

#[test]
fn paginate_heading_not_orphaned() {
    let heights = [50, 50, 50, 84, 50, 50];
    let pages = paginate_heights(&heights, 140, &[3]);
    assert_eq!(pages[0], (0, 2));
    assert!(pages.iter().any(|&(s, _)| s == 3));
}

#[test]
fn paginate_tall_row_alone() {
    let heights = [300, 100, 100];
    let pages = paginate_heights(&heights, 200, &[]);
    assert_eq!(pages[0], (0, 1));
}

#[test]
fn clamp_page_within_range() {
    assert_eq!(clamp_page(3, 10), 3);
}

#[test]
fn clamp_page_overflows_to_last() {
    assert_eq!(clamp_page(99, 10), 9);
}

#[test]
fn clamp_page_empty_chapter_is_zero() {
    assert_eq!(clamp_page(5, 0), 0);
}

#[test]
fn resolve_progress_midpoint() {
    // Offsets chosen so 50% lands inside chapter 1, not on a boundary:
    // global = 500 * 500 / 1000 = 250, which falls in [100, 300).
    let offsets = [0, 100, 300, 500];
    let (ch, local) = resolve_progress_target(500, &offsets, 3);
    assert_eq!(ch, 1);
    assert_eq!(local, 150);
}

#[test]
fn resolve_progress_at_chapter_boundary() {
    // 50% lands exactly on chapter 2's start offset (250 == offsets[2]).
    // A boundary belongs to the chapter that starts there, so local = 0.
    let offsets = [0, 100, 250, 500];
    let (ch, local) = resolve_progress_target(500, &offsets, 3);
    assert_eq!(ch, 2);
    assert_eq!(local, 0);
}

#[test]
fn resolve_progress_start() {
    let offsets = [0, 100, 250, 500];
    let (ch, local) = resolve_progress_target(0, &offsets, 3);
    assert_eq!(ch, 0);
    assert_eq!(local, 0);
}

#[test]
fn resolve_progress_end() {
    let offsets = [0, 100, 250, 500];
    let (ch, _local) = resolve_progress_target(1000, &offsets, 3);
    assert_eq!(ch, 2);
}
