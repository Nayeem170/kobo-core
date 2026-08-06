// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! EPUB reading: open the book, walk the spine, and turn each chapter's XHTML
//! into plain text + a char-offset->element map (via [`crate::html_text`]).
//!
//! A `WordMark` from the audio spine maps to chapter text as: the synthesized
//! utterance is a slice of `Chapter::text` at a known char range; the word's
//! position within that slice + the slice start = the chapter char offset,
//! which `segment_at` resolves to a highlightable element.

use crate::html_text::{
    extract_with_indents, extract_with_style, parse_book_style, BookStyle, IndentMap, LinkRun,
    StyleRun, TextSegment,
};
use log::warn;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EpubError {
    #[error("epub: {0}")]
    Other(String),
}

/// One spine chapter: plain text + segments + preloaded image bytes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chapter {
    pub index: usize,
    /// Chapter (file-level) title if declared; not the book title.
    pub title: Option<String>,
    pub text: String,
    pub segments: Vec<TextSegment>,
    /// Preloaded image data: (relative src path, raw file bytes).
    /// Populated lazily by `load_images` - empty after `EpubBook::open`.
    pub images: Vec<(String, Vec<u8>)>,
    /// EPUB file path (for lazy image loading via `load_images`).
    pub epub_path: String,
    /// Chapter XHTML path in the archive (for resolving relative image srcs).
    pub chapter_path: String,
    /// Pre-built display body text with list markers, `\n` separators, heading
    /// text, and figure captions. All byte-offset consumers (rows, bookmarks,
    /// reading position, TTS cursor, link hit-testing) operate on offsets into
    /// this string. Built deterministically from `text` + `segments` at cache
    /// time so `build_state()` does not rebuild it.
    #[serde(default)]
    pub body: String,
    /// Body offsets for each segment in `segments`. `seg_body_start[i]` is the
    /// byte offset into `body` where segment `i` starts (after its leading `\n`
    /// separator if any). Used by `anchor_offset()` to convert segment positions
    /// to body offsets.
    #[serde(default)]
    pub seg_body_start: Vec<usize>,
    /// Style runs (bold/italic/link) rebased onto `body` offsets.
    #[serde(default)]
    pub body_styles: Vec<StyleRun>,
    /// Link runs rebased onto `body` offsets.
    #[serde(default)]
    pub body_links: Vec<LinkRun>,
}

impl Chapter {
    /// Build a chapter from its XHTML (loader path + unit-testable without a
    /// real EPUB file - see the html_text tests).
    pub fn from_xhtml(index: usize, title: Option<String>, xhtml: &str) -> Self {
        Self::from_xhtml_with_indents(index, title, xhtml, &IndentMap::new())
    }

    /// As [`Chapter::from_xhtml`], but resolves block indents against the
    /// book's stylesheet so code listings keep their nesting.
    pub fn from_xhtml_with_indents(
        index: usize,
        title: Option<String>,
        xhtml: &str,
        indents: &IndentMap,
    ) -> Self {
        let (text, segments) = extract_with_indents(xhtml, indents);
        Chapter {
            index,
            title,
            text,
            segments,
            images: Vec::new(),
            epub_path: String::new(),
            chapter_path: String::new(),
            body: String::new(),
            seg_body_start: Vec::new(),
            body_styles: Vec::new(),
            body_links: Vec::new(),
        }
    }

    /// As [`Chapter::from_xhtml_with_indents`], but honours every style the
    /// scan reads -- indents plus `list-style: none`.
    pub fn from_xhtml_with_style(
        index: usize,
        title: Option<String>,
        xhtml: &str,
        style: &BookStyle,
    ) -> Self {
        let (text, segments) = extract_with_style(xhtml, style);
        Chapter {
            index,
            title,
            text,
            segments,
            images: Vec::new(),
            epub_path: String::new(),
            chapter_path: String::new(),
            body: String::new(),
            seg_body_start: Vec::new(),
            body_styles: Vec::new(),
            body_links: Vec::new(),
        }
    }

    /// Every hyperlink in the chapter, in chapter-text offsets.
    pub fn links(&self) -> impl Iterator<Item = &LinkRun> {
        self.segments.iter().flat_map(|s| s.links.iter())
    }

    /// Lazily load this chapter's image bytes from the EPUB archive.
    /// Memoized: first call opens the archive and populates `self.images`;
    /// subsequent calls return the cached data immediately.
    pub fn load_images(&mut self) -> &[(String, Vec<u8>)] {
        if !self.images.is_empty() {
            return &self.images;
        }
        let base_dir = Path::new(&self.chapter_path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        // Opened on the first segment that actually needs a file. A chapter
        // whose only figures are drawn inline never touches the archive.
        let mut doc: Option<epub::doc::EpubDoc<std::io::BufReader<std::fs::File>>> = None;
        let mut tried_open = false;
        for seg in &self.segments {
            // A drawing defined in the chapter names no archive entry: its
            // markup came through on the segment, so it is served straight from
            // there under the same `src` key everything else looks up by.
            if let (Some(src), Some(markup)) = (&seg.src, &seg.svg) {
                self.images.push((src.clone(), markup.as_bytes().to_vec()));
                continue;
            }
            let Some(src) = &seg.src else { continue };
            if !tried_open {
                tried_open = true;
                if self.epub_path.is_empty() {
                    warn!("load_images: no archive for {src}");
                } else {
                    match epub::doc::EpubDoc::new(&self.epub_path) {
                        Ok(d) => doc = Some(d),
                        Err(e) => warn!("load_images: epub open error: {e}"),
                    }
                }
            }
            let Some(doc) = doc.as_mut() else { continue };
            // The markup's `src` may carry `%20`-style escapes (any image
            // filename with a space in it), but the zip stores the real bytes
            // -- an unencoded lookup misses and the figure silently drops.
            let joined = base_dir.join(percent_decode(src));
            let full = normalize_zip_path(&joined);
            if let Some(data) = doc.get_resource_by_path(Path::new(&full)) {
                self.images.push((src.clone(), data));
            } else {
                // Consumers look images up by `src`, so a miss costs only
                // this one picture -- but it is worth saying which, since a
                // silent skip here used to shift every later figure onto
                // the wrong bitmap.
                warn!("load_images: {full} not found in archive");
            }
        }
        &self.images
    }

    /// Display title with fallbacks: declared title, first heading element,
    /// first text line, then "Chapter N+1". Never a bare position label.
    pub fn display_title(&self, idx: usize) -> String {
        if let Some(t) = self.title.as_deref() {
            let t = t.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        for seg in &self.segments {
            let is_heading = seg.tag.len() == 2
                && seg.tag.starts_with('h')
                && seg.tag.as_bytes()[1].is_ascii_digit();
            if is_heading {
                if let Some(slice) = self.text.get(seg.start..seg.end) {
                    let t = slice.trim();
                    if !t.is_empty() {
                        return t.to_string();
                    }
                }
            }
        }
        for first in self.text.lines().filter(|l| !l.trim().is_empty()) {
            let t = first.trim();
            if is_useful_title(t) {
                return t.to_string();
            }
        }
        format!("Chapter {}", idx + 1)
    }
}

fn is_useful_title(s: &str) -> bool {
    if s.chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_whitespace())
    {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    !matches!(
        lower.as_str(),
        "copyright"
            | "contents"
            | "table of contents"
            | "all rights reserved"
            | "cover"
            | "title page"
            | "dedication"
            | "about the author"
            | "index"
    )
}

/// An opened EPUB.
#[derive(Debug)]
pub struct EpubBook {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub chapters: Vec<Chapter>,
    /// Table of contents as a depth-aware tree. Empty (length 0) when the EPUB
    /// has no NCX/nav. Consumers that need only a flat list of labels can fall
    /// back to walking the spine directly.
    pub toc_tree: Vec<TocEntry>,
}

/// One entry in the book's table of contents, parsed from the NCX or NAV.
/// Carries its nesting depth and resolved spine chapter index so the consumer
/// can indent and jump without re-resolving paths.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TocEntry {
    pub label: String,
    pub depth: usize,
    /// Index into the spine, or `None` for a divider/grouping entry whose own
    /// path names no real chapter (a nav-only "Part One" heading page, for
    /// instance) but that still has real children worth keeping. Not
    /// navigable on its own -- the consumer shows the label but does not
    /// treat a tap on it as a jump.
    pub chapter: Option<usize>,
    /// The `#fragment` this entry named, if any -- the position within
    /// `chapter` to land on, not just which chapter. `None` means the
    /// chapter's top.
    pub anchor: Option<String>,
    pub children: Vec<TocEntry>,
}

impl EpubBook {
    /// Extract only the cover image bytes from an EPUB - no spine walk.
    pub fn cover_bytes(path: impl AsRef<Path>) -> Option<Vec<u8>> {
        let mut doc = epub::doc::EpubDoc::new(path.as_ref()).ok()?;
        let (data, _mime) = doc.get_cover()?;
        Some(data)
    }

    /// Open and fully extract an EPUB at `path`.
    ///
    /// Chapter titles are resolved from the EPUB TOC (`toc.ncx` / nav) when
    /// available; the reader UI provides additional fallbacks (heading tags,
    /// first text line) for chapters with no TOC entry.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EpubError> {
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let mut doc =
            epub::doc::EpubDoc::new(path.as_ref()).map_err(|e| EpubError::Other(e.to_string()))?;
        let style = collect_book_style(&mut doc);
        let mut chapters = Vec::new();
        let mut idx = 0usize;
        let mut skipped = 0usize;
        loop {
            if let Some((xhtml, _mime)) = doc.get_current_str() {
                let mut ch = Chapter::from_xhtml_with_style(idx, None, &xhtml, &style);
                ch.epub_path = path_str.clone();
                ch.chapter_path = doc
                    .get_current_path()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                idx += 1;
                let has_images = ch.segments.iter().any(|s| s.src.is_some());
                if ch.text.trim().is_empty() && !has_images {
                    skipped += 1;
                    if !doc.go_next() {
                        break;
                    }
                    continue;
                }
                chapters.push(ch);
            }
            if !doc.go_next() {
                break;
            }
        }

        let toc_tree = build_toc_tree(&doc.toc, &chapters);
        let toc_map = build_toc_map(&doc.toc);
        for ch in &mut chapters {
            if ch.title.is_none() {
                let cp = strip_fragment(&ch.chapter_path);
                if let Some(label) = toc_map.get(&cp) {
                    ch.title = Some(label.clone());
                }
            }
        }

        let title = doc.get_title();
        let author = doc.mdata("creator").map(|m| m.value.clone());
        let language = doc.mdata("language").map(|m| m.value.clone());
        log::info!(
            "epub: {} spine items, {} skipped, {} chapters",
            idx,
            skipped,
            chapters.len()
        );
        Ok(EpubBook {
            title,
            author,
            language,
            chapters,
            toc_tree,
        })
    }
}

/// Build the book-wide `class -> indent-em` map from every stylesheet in the
/// archive.
///
/// One map for the whole book, not per chapter: Calibre emits a single shared
/// stylesheet, and merging is what lets a chapter that only links the shared
/// sheet still resolve its indent classes. Later sheets win on collision, which
/// is the same order the cascade would apply them in.
fn collect_book_style(
    doc: &mut epub::doc::EpubDoc<std::io::BufReader<std::fs::File>>,
) -> BookStyle {
    let css_ids: Vec<String> = doc
        .resources
        .iter()
        .filter(|(_, item)| item.mime.contains("css"))
        .map(|(id, _)| id.clone())
        .collect();
    let mut style = BookStyle::new();
    for id in css_ids {
        if let Some((bytes, _mime)) = doc.get_resource(&id) {
            style.extend(parse_book_style(&String::from_utf8_lossy(&bytes)));
        }
    }
    log::info!(
        "epub: {} indent classes, {} unmarked-list classes from stylesheets",
        style.indents.len(),
        style.no_marker.len()
    );
    style
}

/// Where an internal hyperlink points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    /// Index into [`EpubBook::chapters`].
    pub chapter: usize,
    /// The `#fragment`, if the href carried one. Matched against
    /// [`TextSegment::id`] to find the offset within the chapter.
    pub anchor: Option<String>,
}

/// Resolve an `href` written in `from_chapter` to a chapter and anchor.
///
/// Returns `None` for anything that does not land inside this book: external
/// schemes (`http:`, `mailto:`), and hrefs naming a file that is not in the
/// spine (an image, or a nav document the spine skipped).
///
/// Free function rather than a method because the reader holds a
/// `Vec<Chapter>` after open, not the `EpubBook` it came from.
pub fn resolve_link(chapters: &[Chapter], from_chapter: usize, href: &str) -> Option<LinkTarget> {
    let href = href.trim();
    if href.is_empty() || is_external_href(href) {
        return None;
    }
    // A bare `#anchor` stays in the chapter it was written in.
    if let Some(frag) = href.strip_prefix('#') {
        return Some(LinkTarget {
            chapter: from_chapter,
            anchor: Some(percent_decode(frag)),
        });
    }
    let (path_part, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, Some(percent_decode(a))),
        None => (href, None),
    };
    // Hrefs are relative to the file that contains them, not to the archive
    // root -- `../Text/ch02.xhtml` from `OEBPS/Text/ch01.xhtml`.
    let base = chapters
        .get(from_chapter)
        .map(|c| c.chapter_path.as_str())
        .unwrap_or("");
    let base_dir = Path::new(base).parent().unwrap_or(Path::new(""));
    let target = normalize_zip_path(&base_dir.join(percent_decode(path_part)));
    let chapter = chapters
        .iter()
        .position(|c| normalize_zip_path(Path::new(&c.chapter_path)) == target)?;
    Some(LinkTarget { chapter, anchor })
}

/// Chapter-text offset an anchor resolves to, or 0 for the chapter top.
/// Returns a body offset when the chapter has a pre-built body.
pub fn anchor_offset(chapters: &[Chapter], target: &LinkTarget) -> usize {
    let Some(chapter) = chapters.get(target.chapter) else {
        return 0;
    };
    let Some(anchor) = target.anchor.as_deref() else {
        return 0;
    };
    let seg_idx = chapter
        .segments
        .iter()
        .position(|s| s.id.as_deref() == Some(anchor));
    if !chapter.body.is_empty() {
        if let Some(idx) = seg_idx {
            return chapter.seg_body_start.get(idx).copied().unwrap_or(0);
        }
        return 0;
    }
    seg_idx
        .and_then(|idx| chapter.segments.get(idx))
        .map(|s| s.start)
        .unwrap_or(0)
}

impl EpubBook {
    /// [`resolve_link`] against this book's spine.
    pub fn resolve_link(&self, from_chapter: usize, href: &str) -> Option<LinkTarget> {
        resolve_link(&self.chapters, from_chapter, href)
    }

    /// [`anchor_offset`] against this book's spine.
    pub fn anchor_offset(&self, target: &LinkTarget) -> usize {
        anchor_offset(&self.chapters, target)
    }
}

/// Schemes that leave the book. Checked before the path join so a `mailto:`
/// or `https:` href is never mistaken for a relative filename.
fn is_external_href(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:", "data:", "ftp://"]
        .iter()
        .any(|s| lower.starts_with(s))
}

/// Percent-decode the `%20`-style escapes EPUB filenames carry. Anything that
/// is not a valid escape is passed through unchanged.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Build a depth-aware TOC tree from NAV/NCX nav points. Each entry is resolved
/// against the spine so its `chapter` index is a direct index into `chapters`,
/// and its `#fragment` (if any) is kept as `anchor` rather than discarded --
/// without it every entry could only jump to a chapter's top, not the position
/// within it the nav point actually named.
///
/// A node whose own path resolves to no spine chapter is not simply dropped:
/// nav trees commonly use a non-navigable entry (a "Part One" divider) purely
/// to group real chapters underneath it, and dropping the node used to drop
/// its resolving children with it -- an entire recursing branch lost because
/// its own header line didn't happen to be a spine item. Only a node with
/// neither a resolved chapter nor any surviving children is dropped.
fn build_toc_tree(toc: &[epub::doc::NavPoint], chapters: &[Chapter]) -> Vec<TocEntry> {
    fn walk(np: &epub::doc::NavPoint, depth: usize, chapters: &[Chapter]) -> Option<TocEntry> {
        let raw = np.content.to_string_lossy().into_owned();
        let (path, anchor) = match raw.rsplit_once('#') {
            Some((base, frag)) if !frag.is_empty() => {
                (base.to_string(), Some(percent_decode(frag)))
            }
            _ => (strip_fragment(&raw), None),
        };
        let chapter = chapters
            .iter()
            .position(|c| normalize_zip_path(Path::new(&c.chapter_path)) == path);
        let children: Vec<TocEntry> = np
            .children
            .iter()
            .filter_map(|c| walk(c, depth + 1, chapters))
            .collect();
        if chapter.is_none() && children.is_empty() {
            return None;
        }
        Some(TocEntry {
            label: np.label.clone(),
            depth,
            chapter,
            anchor,
            children,
        })
    }
    toc.iter().filter_map(|np| walk(np, 0, chapters)).collect()
}

/// Flatten the EPUB TOC tree into a label-by-path map.
/// NCX `content` paths may carry `#fragment` suffixes - stripped before lookup.
fn build_toc_map(toc: &[epub::doc::NavPoint]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut flat = Vec::new();
    flatten_toc(toc, &mut flat);
    for (label, path) in flat {
        map.entry(path).or_insert(label);
    }
    map
}

fn flatten_toc(toc: &[epub::doc::NavPoint], out: &mut Vec<(String, String)>) {
    for np in toc {
        let raw = np.content.to_string_lossy().into_owned();
        let path = strip_fragment(&raw);
        out.push((np.label.clone(), path));
        if !np.children.is_empty() {
            flatten_toc(&np.children, out);
        }
    }
}

fn strip_fragment(path: &str) -> String {
    match path.rsplit_once('#') {
        Some((base, _)) => base.to_string(),
        None => path.to_string(),
    }
}

/// Normalize a ZIP archive path by resolving `.` and `..` components.
/// ZIP entries store flat paths (no `..`), so `OEBPS/Text/../Images/x.png`
/// must become `OEBPS/Images/x.png` for `get_resource_by_path` to match.
fn normalize_zip_path(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(s) => {
                parts.push(s.to_string_lossy().into_owned());
            }
            _ => {}
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests;
