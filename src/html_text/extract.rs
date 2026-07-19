// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! DOM extraction: chapter XHTML -> plain text + per-block-element segments.

use super::style::{self, IndentMap, MAX_INDENT_EM};
use scraper::{Html, Selector};
use std::sync::LazyLock;

/// A run of chapter text belonging to one block element, with its char range.
/// For `<img>`/`<figure>` segments the text range is empty (zero-width marker at
/// the image's flow position) and `src`/`caption` carry the image data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextSegment {
    /// Inclusive start byte/char offset into the chapter text.
    pub start: usize,
    /// Exclusive end offset.
    pub end: usize,
    /// Element tag (e.g. `p`, `h1`, `li`, `img`, `figure`).
    pub tag: String,
    /// The element's `id` attribute if any (useful for anchoring).
    pub id: Option<String>,
    /// `src` for `img`/`figure` (None for text blocks).
    pub src: Option<String>,
    /// `figcaption` text for a `figure` (None for non-figures).
    pub caption: Option<String>,
    /// Left indent of the block, in `em`, resolved from the book's stylesheet
    /// (see [`crate::html_text::style`]) plus any leading spaces the markup
    /// kept. 0 for ordinary prose. Serde-defaulted so offset caches written
    /// before indents existed still deserialize.
    #[serde(default)]
    pub indent: f32,
    /// Bold/italic spans within this segment, in chapter-text offsets.
    #[serde(default)]
    pub styles: Vec<StyleRun>,
}

/// A run of emphasised text inside a chapter, in chapter-text byte offsets.
///
/// Held alongside the text rather than inside it. `Row` is a Slint struct
/// carrying flat text, and threading styled spans through it would mean
/// changing the `.slint` type, every construction site, and the justification
/// and TTS-highlight code that keys off row ranges. Rows already carry byte
/// ranges, so emphasis can simply be looked up by offset at draw time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StyleRun {
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
}

impl TextSegment {
    /// Does this segment contain char offset `i`?
    pub fn contains(&self, i: usize) -> bool {
        i >= self.start && i < self.end
    }
}

/// Selector for the block elements we extract as flow items (text + images).
/// `image` matches SVG `<image>` (covers, inline SVG art).
const BLOCK_SELECTOR: &str =
    "p, h1, h2, h3, h4, h5, h6, li, blockquote, pre, div, figure, img, image";
const CONTAINER_TAGS: &[&str] = &["div", "blockquote"];

static BLOCK_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(BLOCK_SELECTOR).expect("valid selector"));
static IMG_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("img").expect("selector"));
static CAP_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("figcaption").expect("selector"));

/// Extract plain text + per-block-element segments from chapter XHTML.
/// Images (`<img>`/`<figure>`) appear in document order as zero-width segments
/// carrying `src`/`caption` (they contribute no text to the returned string).
pub fn extract(xhtml: &str) -> (String, Vec<TextSegment>) {
    extract_with_indents(xhtml, &IndentMap::new())
}

/// As [`extract`], but resolves each text block's left indent against the
/// book's stylesheet (`class -> em`, built by [`style::parse_indents`]).
///
/// Calibre encodes code nesting as `margin-left` on a per-level class rather
/// than as `<pre>`, so without the map every indent level collapses to zero and
/// Python listings lose the structure that carries their meaning.
pub fn extract_with_indents(xhtml: &str, indents: &IndentMap) -> (String, Vec<TextSegment>) {
    let html = Html::parse_fragment(xhtml);
    let sel = BLOCK_SELECTOR_OBJ.clone();
    let child_sel = BLOCK_SELECTOR_OBJ.clone();
    let img_sel = IMG_SELECTOR_OBJ.clone();
    let cap_sel = CAP_SELECTOR_OBJ.clone();

    let mut text = String::new();
    let mut segs: Vec<TextSegment> = Vec::new();
    let mut figure_srcs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for elem in html.select(&sel) {
        let tag = elem.value().name().to_string();
        let pos = text.len();
        let id = elem.value().attr("id").map(|s| s.to_string());

        let seg = match tag.as_str() {
            "figure" => extract_figure_segment(elem, &img_sel, &cap_sel, pos, id, &mut figure_srcs),
            "img" => {
                if figure_srcs.contains(elem.value().attr("src").unwrap_or("")) {
                    None
                } else {
                    Some(TextSegment {
                        start: pos,
                        end: pos,
                        tag,
                        id,
                        src: elem.value().attr("src").map(|s| s.to_string()),
                        caption: None,
                        indent: 0.0,
                        styles: Vec::new(),
                    })
                }
            }
            "image" => extract_svg_image_segment(elem, pos, id),
            _ => extract_text_segment(elem, &child_sel, &mut text, pos, tag, id, &segs, indents),
        };
        if let Some(s) = seg {
            segs.push(s);
        }
    }

    let extra = scan_raw_svg_images(xhtml, &segs, &mut text);
    segs.extend(extra);

    (text, segs)
}

fn extract_figure_segment(
    elem: scraper::ElementRef,
    img_sel: &Selector,
    cap_sel: &Selector,
    pos: usize,
    id: Option<String>,
    figure_srcs: &mut std::collections::HashSet<String>,
) -> Option<TextSegment> {
    let src = elem
        .select(img_sel)
        .next()
        .and_then(|i| i.value().attr("src"))
        .map(|s| {
            figure_srcs.insert(s.to_string());
            s.to_string()
        });
    let cap = elem
        .select(cap_sel)
        .next()
        .map(|c| c.text().collect::<String>().trim().to_string());
    Some(TextSegment {
        start: pos,
        end: pos,
        tag: "figure".to_string(),
        id,
        src,
        caption: cap,
        indent: 0.0,
        styles: Vec::new(),
    })
}

fn extract_svg_image_segment(
    elem: scraper::ElementRef,
    pos: usize,
    id: Option<String>,
) -> Option<TextSegment> {
    let src = elem
        .value()
        .attr("xlink:href")
        .or_else(|| elem.value().attr("href"))
        .unwrap_or("");
    if src.is_empty() {
        return None;
    }
    Some(TextSegment {
        start: pos,
        end: pos,
        tag: "image".to_string(),
        id,
        src: Some(src.to_string()),
        caption: None,
        indent: 0.0,
        styles: Vec::new(),
    })
}

fn extract_text_segment(
    elem: scraper::ElementRef,
    child_sel: &Selector,
    text: &mut String,
    pos: usize,
    tag: String,
    id: Option<String>,
    segs: &[TextSegment],
    indents: &IndentMap,
) -> Option<TextSegment> {
    if CONTAINER_TAGS.contains(&tag.as_str()) && elem.select(child_sel).next().is_some() {
        return None;
    }
    let raw: String = elem.text().collect();
    // `pre` keeps its indentation inside the text itself, so adding a block
    // indent on top would double it.
    let indent = if tag == "pre" {
        0.0
    } else {
        block_indent_em(elem, &raw, indents)
    };
    // `pre` is verbatim: keep its own line breaks and leading indentation, only
    // shedding the blank lines the markup wraps around the block. Every other
    // block collapses to a trimmed run that the layout re-wraps.
    let t: &str = if tag == "pre" {
        raw.trim_matches('\n').trim_end()
    } else {
        raw.trim()
    };
    if t.is_empty() {
        return None;
    }
    if !text.is_empty() {
        text.push('\n');
    }
    let content_start = text.len();
    text.push_str(t);
    let end = text.len();
    let dup = segs
        .last()
        .map(|s| text[s.start..s.end] == text[content_start..end])
        .unwrap_or(false);
    if dup {
        text.truncate(pos);
        return None;
    }
    // Emphasis offsets are measured in `raw`; `t` dropped the leading
    // whitespace, so shift by however much went and clamp to the stored range.
    let lead = raw.len() - raw.trim_start().len();
    let styles = if tag == "pre" {
        Vec::new()
    } else {
        collect_style_runs(elem)
            .into_iter()
            .filter_map(|(a, b, bold, italic)| {
                let a = content_start + a.saturating_sub(lead);
                let b = content_start + b.saturating_sub(lead);
                let (a, b) = (a.max(content_start).min(end), b.max(content_start).min(end));
                (a < b).then_some(StyleRun {
                    start: a,
                    end: b,
                    bold,
                    italic,
                })
            })
            .collect()
    };
    Some(TextSegment {
        start: content_start,
        end,
        tag,
        id,
        src: None,
        caption: None,
        indent,
        styles,
    })
}

fn is_bold_tag(name: &str) -> bool {
    matches!(name, "b" | "strong")
}

fn is_italic_tag(name: &str) -> bool {
    matches!(name, "i" | "em" | "cite" | "var")
}

/// Walk a block's descendants in document order, recording which byte ranges of
/// its concatenated text sit inside a bold or italic element.
///
/// Offsets are into the raw (untrimmed) text of the block, which is the same
/// string `elem.text()` produces -- both walk text nodes in the same order.
fn collect_style_runs(elem: scraper::ElementRef) -> Vec<(usize, usize, bool, bool)> {
    fn walk(
        node: ego_tree::NodeRef<scraper::Node>,
        bold: bool,
        italic: bool,
        pos: &mut usize,
        out: &mut Vec<(usize, usize, bool, bool)>,
    ) {
        for child in node.children() {
            match child.value() {
                scraper::Node::Text(t) => {
                    let start = *pos;
                    *pos += t.len();
                    if (bold || italic) && *pos > start {
                        out.push((start, *pos, bold, italic));
                    }
                }
                scraper::Node::Element(e) => {
                    let name = e.name();
                    walk(
                        child,
                        bold || is_bold_tag(name),
                        italic || is_italic_tag(name),
                        pos,
                        out,
                    );
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    walk(*elem, false, false, &mut pos, &mut out);
    // Adjacent runs with the same styling are one run.
    out.dedup_by(|b, a| {
        if a.1 == b.0 && a.2 == b.2 && a.3 == b.3 {
            a.1 = b.1;
            true
        } else {
            false
        }
    });
    out
}

/// Extra indent contributed by each leading space the markup preserved.
///
/// Calibre puts the coarse nesting level on the class (1em per level) but
/// leaves finer, within-level alignment -- a continued call's arguments, a JSON
/// key one step inside its brace -- as leading `&nbsp;`, which `trim` would
/// otherwise discard. Half an em keeps that visible without competing with a
/// real nesting step.
const EM_PER_LEADING_SPACE: f32 = 0.5;

/// Left indent for a text block: its class's stylesheet `margin-left` (or an
/// inline one), plus whatever leading spaces the markup kept.
///
/// The two are additive because they encode different things -- see
/// [`EM_PER_LEADING_SPACE`].
fn block_indent_em(elem: scraper::ElementRef, raw: &str, indents: &IndentMap) -> f32 {
    let from_class = elem
        .value()
        .attr("class")
        .map(|classes| {
            classes
                .split_whitespace()
                .filter_map(|c| indents.get(c).copied())
                .fold(0.0f32, f32::max)
        })
        .unwrap_or(0.0);
    let from_style = elem
        .value()
        .attr("style")
        .and_then(style::inline_indent_em)
        .unwrap_or(0.0);
    let leading = raw
        .chars()
        .take_while(|c| *c == ' ' || *c == '\u{00A0}' || *c == '\t')
        .count() as f32;
    (from_class.max(from_style) + leading * EM_PER_LEADING_SPACE).min(MAX_INDENT_EM)
}

fn scan_raw_svg_images(xhtml: &str, segs: &[TextSegment], text: &mut String) -> Vec<TextSegment> {
    let captured_srcs: std::collections::HashSet<String> =
        segs.iter().filter_map(|s| s.src.clone()).collect();
    let mut extra: Vec<TextSegment> = Vec::new();
    let mut search = 0;
    while let Some(pos) = xhtml[search..].find("<image") {
        let abs = search + pos;
        let tag_end = xhtml[abs..]
            .find('>')
            .map(|p| abs + p)
            .unwrap_or(xhtml.len());
        let tag = &xhtml[abs..tag_end];
        for attr in &["xlink:href", "href"] {
            if let Some(eq) = tag.find(attr) {
                let rest = &tag[eq + attr.len()..];
                let rest = rest.trim_start();
                if let Some(after_eq) = rest.strip_prefix('=') {
                    let v = after_eq.trim_start();
                    let q = v.chars().next().unwrap_or('"');
                    if (q == '"' || q == '\'') && v.len() > 1 {
                        if let Some(end) = v[1..].find(q) {
                            let src = &v[1..1 + end];
                            if !captured_srcs.contains(src) && !src.is_empty() {
                                extra.push(TextSegment {
                                    start: text.len(),
                                    end: text.len(),
                                    tag: "image".to_string(),
                                    id: None,
                                    src: Some(src.to_string()),
                                    caption: None,
                                    indent: 0.0,
                                    styles: Vec::new(),
                                });
                            }
                            break;
                        }
                    }
                }
            }
        }
        search = tag_end;
    }
    extra
}

/// Find the [`TextSegment`] containing char offset `i`, if any.
pub fn segment_at(segs: &[TextSegment], i: usize) -> Option<&TextSegment> {
    segs.iter().find(|s| s.contains(i))
}
#[cfg(test)]
mod tests;
