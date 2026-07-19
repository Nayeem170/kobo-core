// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! EPUB reading: open the book, walk the spine, and turn each chapter's XHTML
//! into plain text + a char-offset->element map (via [`crate::html_text`]).
//!
//! A `WordMark` from the audio spine maps to chapter text as: the synthesized
//! utterance is a slice of `Chapter::text` at a known char range; the word's
//! position within that slice + the slice start = the chapter char offset,
//! which `segment_at` resolves to a highlightable element.

use crate::html_text::{extract_with_indents, parse_indents, IndentMap, TextSegment};
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
        }
    }

    /// Lazily load this chapter's image bytes from the EPUB archive.
    /// Memoized: first call opens the archive and populates `self.images`;
    /// subsequent calls return the cached data immediately.
    pub fn load_images(&mut self) -> &[(String, Vec<u8>)] {
        if !self.images.is_empty() || self.epub_path.is_empty() {
            return &self.images;
        }
        let mut doc = match epub::doc::EpubDoc::new(&self.epub_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("load_images: epub open error: {e}");
                return &self.images;
            }
        };
        let base_dir = Path::new(&self.chapter_path)
            .parent()
            .unwrap_or(Path::new(""));
        for seg in &self.segments {
            if let Some(src) = &seg.src {
                let joined = base_dir.join(src);
                let full = normalize_zip_path(&joined);
                if let Some(data) = doc.get_resource_by_path(Path::new(&full)) {
                    log::debug!("loaded image {} ({} bytes)", src, data.len());
                    self.images.push((src.clone(), data));
                } else {
                    log::debug!("image {} not found at {}", src, full);
                }
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
        if let Some(first) = self.text.lines().find(|l| !l.trim().is_empty()) {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        format!("Chapter {}", idx + 1)
    }
}

/// An opened EPUB.
#[derive(Debug)]
pub struct EpubBook {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub chapters: Vec<Chapter>,
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
        let indents = collect_indents(&mut doc);
        let mut chapters = Vec::new();
        let mut idx = 0usize;
        let mut skipped = 0usize;
        loop {
            if let Some((xhtml, _mime)) = doc.get_current_str() {
                let mut ch = Chapter::from_xhtml_with_indents(idx, None, &xhtml, &indents);
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
fn collect_indents(doc: &mut epub::doc::EpubDoc<std::io::BufReader<std::fs::File>>) -> IndentMap {
    let css_ids: Vec<String> = doc
        .resources
        .iter()
        .filter(|(_, item)| item.mime.contains("css"))
        .map(|(id, _)| id.clone())
        .collect();
    let mut map = IndentMap::new();
    for id in css_ids {
        if let Some((bytes, _mime)) = doc.get_resource(&id) {
            map.extend(parse_indents(&String::from_utf8_lossy(&bytes)));
        }
    }
    log::info!("epub: {} indent classes from stylesheets", map.len());
    map
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
