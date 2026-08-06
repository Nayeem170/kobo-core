// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! DOM extraction: chapter XHTML -> plain text + per-block-element segments.

use super::style::{self, BookStyle, IndentMap, MAX_INDENT_EM};
use scraper::{Html, Selector};
use std::sync::LazyLock;

/// A run of chapter text belonging to one block element, with its char range.
/// For `<img>`/`<figure>` segments the text range is empty (zero-width marker at
/// the image's flow position) and `src`/`caption` carry the image data.
/// List marker kind for `li` segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ListKind {
    Unordered,
    Ordered(u32),
    /// The list sets `list-style: none`. The item still indents to its nesting
    /// depth, but draws no bullet or number -- the shape a table of contents
    /// or a nav list uses.
    Unmarked,
}

/// One `em` of left inset per list nesting level.
pub const LIST_INDENT_EM: f32 = 1.5;

/// Blockquote depth for segments inside `<blockquote>` elements.
/// Serde-defaulted so caches written before this field existed still deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BlockquoteKind {
    #[default]
    None,
    Leaf,
    Children,
}

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
    /// For `li` segments: the parent list type and 1-based index.
    #[serde(default)]
    pub list: Option<ListKind>,
    /// Nesting depth of this list item (0-based).
    #[serde(default)]
    pub list_depth: u32,
    /// Whether this segment is inside a `<blockquote>`.
    #[serde(default)]
    pub blockquote: BlockquoteKind,
    /// Hyperlink destinations within this segment, in chapter-text offsets.
    #[serde(default)]
    pub links: Vec<LinkRun>,
    /// Markup for a figure the book *draws* inline rather than referencing as a
    /// file. `src` names no archive entry in that case, so the bytes have to
    /// travel with the segment. Serialised with the offset cache rather than
    /// skipped: a skipped field would make every inline diagram render on the
    /// first open of a book and vanish on the second.
    #[serde(default)]
    pub svg: Option<String>,
    /// True only for an ordinary paragraph whose indent came from a Calibre
    /// code-listing margin (resolved via the `IndentMap`), i.e. the one case
    /// that should render as code. Table cards and nested lists also set
    /// [`TextSegment::indent`] but through other fields, so they default to
    /// `false` and render as prose. Serde-defaulted so offset caches written
    /// before this field existed decode as `false`.
    #[serde(default)]
    pub code_indent: bool,
}

/// A hyperlink's text range and its raw `href`, in chapter-text byte offsets.
///
/// Kept out of [`StyleRun`] deliberately: `StyleRun` is cloned per row on every
/// repaint, and an owned `String` per run would make that allocation-heavy for
/// a field only the tap handler reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinkRun {
    pub start: usize,
    pub end: usize,
    /// The `href` exactly as the markup wrote it, relative to the chapter file.
    pub href: String,
}

/// A run of emphasised text inside a chapter, in chapter-text byte offsets.
///
/// Held alongside the text rather than inside it. `Row` is a Slint struct
/// carrying flat text, and threading styled spans through it would mean
/// changing the `.slint` type, every construction site, and the justification
/// and TTS-highlight code that keys off row ranges. Rows already carry byte
/// ranges, so emphasis can simply be looked up by offset at draw time.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StyleRun {
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
    #[serde(default)]
    pub link: bool,
}

impl TextSegment {
    pub fn contains(&self, i: usize) -> bool {
        i >= self.start && i < self.end
    }

    /// The bullet or number this item draws, if any.
    ///
    /// Nesting depth is **not** baked in here as leading spaces -- it is
    /// carried by [`TextSegment::indent`] so the renderer insets the whole
    /// block in real pixels. Padding with spaces would only line up in a
    /// monospace face.
    pub fn list_marker(&self) -> Option<String> {
        match self.list.as_ref()? {
            ListKind::Unordered => Some("\u{2022} ".to_string()),
            ListKind::Ordered(n) => Some(format!("{n}. ")),
            ListKind::Unmarked => None,
        }
    }
}

/// Selector for the block elements we extract as flow items (text + images).
/// `image` matches SVG `<image>` (covers, inline SVG art).
const BLOCK_SELECTOR: &str =
    "p, h1, h2, h3, h4, h5, h6, li, blockquote, pre, div, figure, img, image, svg, table";
const CONTAINER_TAGS: &[&str] = &["div", "blockquote"];

static BLOCK_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(BLOCK_SELECTOR).expect("valid selector"));
static IMG_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("img").expect("selector"));
static CAP_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("figcaption").expect("selector"));
static SVG_IMAGE_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("image").expect("selector"));
static SVG_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("svg").expect("selector"));

/// Extract plain text + per-block-element segments from chapter XHTML.
/// Images (`<img>`/`<figure>`) appear in document order as zero-width segments
/// carrying `src`/`caption` (they contribute no text to the returned string).
pub fn extract(xhtml: &str) -> (String, Vec<TextSegment>) {
    extract_with_style(xhtml, &BookStyle::new())
}

/// As [`extract`], but resolves each text block's left indent against the
/// book's stylesheet (`class -> em`, built by [`style::parse_indents`]).
///
/// Calibre encodes code nesting as `margin-left` on a per-level class rather
/// than as `<pre>`, so without the map every indent level collapses to zero and
/// Python listings lose the structure that carries their meaning.
pub fn extract_with_indents(xhtml: &str, indents: &IndentMap) -> (String, Vec<TextSegment>) {
    extract_with_style(xhtml, &BookStyle::from_indents(indents.clone()))
}

/// As [`extract_with_indents`], but reads every style the scan understands --
/// currently block indents plus `list-style: none`.
pub fn extract_with_style(xhtml: &str, style: &BookStyle) -> (String, Vec<TextSegment>) {
    let html = Html::parse_fragment(xhtml);
    let sel = BLOCK_SELECTOR_OBJ.clone();
    let child_sel = BLOCK_SELECTOR_OBJ.clone();
    let img_sel = IMG_SELECTOR_OBJ.clone();
    let cap_sel = CAP_SELECTOR_OBJ.clone();

    let mut text = String::new();
    let mut segs: Vec<TextSegment> = Vec::new();
    let mut figure_srcs: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Numbers the synthetic keys inline drawings are looked up by.
    let mut inline_svg_seq = 0usize;

    for elem in html.select(&sel) {
        let tag = elem.value().name().to_string();
        let id = elem.value().attr("id").map(|s| s.to_string());

        // A caption belongs to its figure, and a cell belongs to its table.
        // Both are emitted by the owning element, so the block elements the
        // markup wrapped them in must not be walked a second time.
        if tag != "figure" && has_ancestor(elem, &["figcaption"]) {
            continue;
        }
        if tag != "table" && has_ancestor(elem, &["table"]) {
            continue;
        }

        let pos = text.len();
        let produced: Vec<TextSegment> = match tag.as_str() {
            "figure" => extract_figure_segment(
                elem,
                &img_sel,
                &cap_sel,
                pos,
                id,
                &mut figure_srcs,
                &mut inline_svg_seq,
            )
            .into_iter()
            .collect(),
            "img" => {
                if figure_srcs.contains(elem.value().attr("src").unwrap_or("")) {
                    Vec::new()
                } else {
                    vec![TextSegment {
                        start: pos,
                        end: pos,
                        tag,
                        id,
                        src: elem.value().attr("src").map(|s| s.to_string()),
                        caption: elem
                            .value()
                            .attr("alt")
                            .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
                            .filter(|a| !a.is_empty()),
                        indent: 0.0,
                        styles: Vec::new(),
                        list: None,
                        list_depth: 0,
                        blockquote: BlockquoteKind::None,
                        svg: None,
                        code_indent: false,
                        links: Vec::new(),
                    }]
                }
            }
            "image" => extract_svg_image_segment(elem, pos, id)
                .into_iter()
                .collect(),
            "svg" => extract_inline_svg_segment(elem, pos, id, &mut inline_svg_seq)
                .into_iter()
                .collect(),
            "table" => extract_table_segments(elem, &mut text, id),
            _ => extract_text_segment(elem, &child_sel, &mut text, pos, tag, id, &segs, style)
                .into_iter()
                .collect(),
        };
        segs.extend(produced);
    }

    resolve_blockquote_context(&mut segs, &html, &text);

    let extra = scan_raw_svg_images(xhtml, &segs, &mut text);
    segs.extend(extra);

    collect_orphan_text(&html, &mut text, &mut segs);

    (text, segs)
}

static BODY_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("body, html").expect("selector"));

/// Tags directly matched by BLOCK_SELECTOR. Every other element at the root
/// level is a candidate for orphan text collection.
static BLOCK_SELECTOR_TAGS: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
    std::sync::LazyLock::new(|| {
        [
            "p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "blockquote", "pre", "div",
            "figure", "img", "image", "svg", "table",
        ]
        .into_iter()
        .collect()
    });

/// Elements whose text content is never visible page prose: `<script>` carries
/// JavaScript, `<style>` carries CSS, and the rest are document metadata. The
/// orphan collector must skip these the way the main BLOCK_SELECTOR pass does
/// implicitly (they are not in the selector), or the raw code at a chapter's
/// end leaks into the rendered text.
static NON_CONTENT_TAGS: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
    std::sync::LazyLock::new(|| {
        [
            "script", "style", "link", "meta", "head", "title", "noscript", "template",
            "base",
        ]
        .into_iter()
        .collect()
    });

/// Walk root children for text nodes or elements not in BLOCK_SELECTOR whose
/// text content was silently dropped.
///
/// Custom tags (`<examples>`, `<input>`), bare text directly in `<body>`, and
/// any unknown container are otherwise discarded with no warning during the
/// main pass. This function collects only the gaps -- no threshold, no
/// duplicate dump -- and injects each gap as a `div` segment at its natural
/// position.
fn collect_orphan_text(html: &Html, text: &mut String, segs: &mut Vec<TextSegment>) {
    let body_sel = BODY_SELECTOR_OBJ.clone();
    let Some(root) = html.select(&body_sel).next() else {
        return;
    };

    let root_ref = scraper::ElementRef::wrap(*root).unwrap_or(root);

    fn walk_orphans(node: ego_tree::NodeRef<scraper::Node>) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        for child in node.children() {
            match child.value() {
                scraper::Node::Text(t) => {
                    let trimmed = t.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
                scraper::Node::Element(e) => {
                    if BLOCK_SELECTOR_TAGS.contains(e.name())
                        || NON_CONTENT_TAGS.contains(e.name())
                    {
                        continue;
                    }
                    let owned = own_text_inner(child);
                    let trimmed = owned.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }
        parts
    }

    let gap_texts = walk_orphans(*root_ref);
    if gap_texts.is_empty() {
        return;
    }

    let total_gap: usize = gap_texts.iter().map(|s| s.len()).sum();
    log::warn!(
        "extract: orphan text collected (captured {} chars, injected {} chars in {} gaps)",
        text.len(),
        total_gap,
        gap_texts.len(),
    );

    for gap in gap_texts {
        let pos = text.len();
        text.push_str(&gap);
        segs.push(TextSegment {
            start: pos,
            end: pos + gap.len(),
            tag: "div".to_string(),
            id: None,
            src: None,
            caption: None,
            indent: 0.0,
            styles: Vec::new(),
            list: None,
            list_depth: 0,
            blockquote: BlockquoteKind::None,
            svg: None,
            code_indent: false,
            links: Vec::new(),
        });
    }
}

fn own_text_inner(node: ego_tree::NodeRef<scraper::Node>) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.value() {
            scraper::Node::Text(t) => out.push_str(t),
            scraper::Node::Element(e)
                if !is_block_tag(e.name()) && !NON_CONTENT_TAGS.contains(e.name()) =>
            {
                out.push_str(&own_text_inner(child));
            }
            _ => {}
        }
    }
    out
}

/// Does `elem` sit inside an element with one of these tag names?
fn has_ancestor(elem: scraper::ElementRef, names: &[&str]) -> bool {
    elem.ancestors()
        .any(|a| matches!(a.value(), scraper::Node::Element(e) if names.contains(&e.name())))
}

fn extract_figure_segment(
    elem: scraper::ElementRef,
    img_sel: &Selector,
    cap_sel: &Selector,
    pos: usize,
    id: Option<String>,
    figure_srcs: &mut std::collections::HashSet<String>,
    seq: &mut usize,
) -> Option<TextSegment> {
    // A figure may wrap a plain `<img>` or an SVG `<image>`; the SVG form is
    // what most diagram exporters emit. Reading only `<img src>` dropped the
    // whole figure -- caption included -- for the SVG shape.
    let mut src = elem
        .select(img_sel)
        .next()
        .and_then(|i| i.value().attr("src"))
        .or_else(|| {
            elem.select(&SVG_IMAGE_SELECTOR_OBJ)
                .next()
                .and_then(image_href)
        })
        .map(|s| {
            figure_srcs.insert(s.to_string());
            s.to_string()
        });
    // Neither shape referenced a file, so the drawing is defined in the chapter
    // itself. It travels with the segment under a synthetic key.
    let mut svg = None;
    if src.is_none() {
        if let Some(markup) = elem
            .select(&SVG_SELECTOR_OBJ)
            .next()
            .and_then(inline_svg_markup)
        {
            src = Some(inline_svg_key(seq));
            svg = Some(markup);
        }
    }
    let cap = elem
        .select(cap_sel)
        .next()
        .map(|c| {
            c.text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|c| !c.is_empty())
        .or_else(|| {
            elem.select(img_sel)
                .next()
                .and_then(|i| i.value().attr("alt"))
                .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|a| !a.is_empty())
        });
    // A figure with neither a picture nor a caption has nothing to show.
    if src.is_none() && cap.is_none() {
        return None;
    }
    Some(TextSegment {
        start: pos,
        end: pos,
        tag: "figure".to_string(),
        id,
        src,
        caption: cap,
        indent: 0.0,
        styles: Vec::new(),
        list: None,
        list_depth: 0,
        blockquote: BlockquoteKind::None,
        svg,
        code_indent: false,
        links: Vec::new(),
    })
}

/// The `href` of an SVG `<image>`, whichever spelling the markup used.
///
/// `xlink:href` is a namespaced attribute, so a qualified-name lookup for
/// either `"xlink:href"` or `"href"` misses it -- the parser keeps the prefix
/// separately from the local name. Matching on the local name finds both the
/// namespaced form and the plain SVG2 `href`.
fn image_href<'a>(elem: scraper::ElementRef<'a>) -> Option<&'a str> {
    elem.value()
        .attrs()
        .find(|(name, value)| *name == "href" && !value.trim().is_empty())
        .map(|(_, value)| value)
}

/// A drawing the book defines in the chapter itself, as `<svg>` shape elements
/// rather than a reference to a file.
///
/// This is what every diagramming tool that "embeds" its output produces, and
/// until now it extracted to nothing: the element holds no text, names no
/// `src`, and so contributed neither a picture nor a caption.
///
/// The markup is re-serialised from the parsed tree. SVG is case-sensitive
/// where HTML is not (`viewBox`, `preserveAspectRatio`, `clipPath`), but the
/// parser puts these elements in the SVG namespace and restores their casing on
/// the way in, so the round-trip preserves them -- see the `viewBox` test.
fn inline_svg_markup(elem: scraper::ElementRef) -> Option<String> {
    // An `<svg>` that only wraps `<image href=...>` is a pointer to an archive
    // entry, which the `figure`/`image` paths already resolve. Rasterising the
    // wrapper here would draw an empty box over the real picture.
    if elem.select(&SVG_IMAGE_SELECTOR_OBJ).next().is_some() {
        return None;
    }
    // Nothing to draw: no shapes, no text.
    if !elem.children().any(|c| c.value().is_element()) {
        return None;
    }
    Some(with_svg_namespace(&elem.html()))
}

/// usvg parses XML, and rejects a document whose root is not in the SVG
/// namespace. An inline `<svg>` in XHTML frequently declares no `xmlns` of its
/// own -- it inherits the document's -- and the serialiser does not write one
/// back, so it has to be restored here or every inline diagram fails to parse.
fn with_svg_namespace(block: &str) -> String {
    if block.contains("xmlns") {
        return block.to_string();
    }
    match block.find(|c: char| c.is_ascii_whitespace() || c == '>') {
        Some(i) => format!(
            "{} xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"{}",
            &block[..i],
            &block[i..]
        ),
        None => block.to_string(),
    }
}

/// Key an inline drawing is looked up by. Consumers resolve pictures through
/// `src`, and this one names no archive entry, so it gets a synthetic name that
/// cannot collide with a real path (no book resource starts with `#`).
fn inline_svg_key(seq: &mut usize) -> String {
    let key = format!("#inline-svg-{seq}");
    *seq += 1;
    key
}

fn extract_inline_svg_segment(
    elem: scraper::ElementRef,
    pos: usize,
    id: Option<String>,
    seq: &mut usize,
) -> Option<TextSegment> {
    // A figure owns its own drawing, caption included; emitting a second
    // segment here would render the diagram twice.
    if has_ancestor(elem, &["figure"]) {
        return None;
    }
    let markup = inline_svg_markup(elem)?;
    Some(TextSegment {
        start: pos,
        end: pos,
        tag: "image".to_string(),
        id,
        src: Some(inline_svg_key(seq)),
        caption: None,
        indent: 0.0,
        styles: Vec::new(),
        list: None,
        list_depth: 0,
        blockquote: BlockquoteKind::None,
        svg: Some(markup),
        code_indent: false,
        links: Vec::new(),
    })
}

fn extract_svg_image_segment(
    elem: scraper::ElementRef,
    pos: usize,
    id: Option<String>,
) -> Option<TextSegment> {
    let src = image_href(elem).unwrap_or("");
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
        list: None,
        list_depth: 0,
        blockquote: BlockquoteKind::None,
        svg: None,
        code_indent: false,
        links: Vec::new(),
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
    style_sheet: &BookStyle,
) -> Option<TextSegment> {
    if CONTAINER_TAGS.contains(&tag.as_str()) && elem.select(child_sel).next().is_some() {
        return None;
    }
    let indents = &style_sheet.indents;
    // A list item that contains a nested list must contribute only its own
    // label. `elem.text()` would swallow every descendant item's text, which
    // both duplicates it and starves the nested items of their own segments.
    let raw: String = if tag == "li" {
        own_text(elem)
    } else {
        elem.text().collect()
    };
    // `pre` keeps its indentation inside the text itself, so adding a block
    // indent on top would double it.
    let indent_from_block = if tag == "pre" {
        0.0
    } else {
        block_indent_em(elem, &raw, indents)
    };
    // Calibre encodes code-listing nesting as a per-class `margin-left` rather
    // than a real `<pre>`; that resolved margin is the only static signal an
    // ordinary paragraph is code. List items and table cards also indent
    // (nesting, card layout) but through other fields, so they stay `false`
    // here and render as prose -- not faint monospace.
    let code_indent = tag != "pre" && tag != "li" && indent_from_block > 0.0;
    let indent = indent_from_block;
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
    let rebase = |a: usize, b: usize| -> Option<(usize, usize)> {
        let a = content_start + a.saturating_sub(lead);
        let b = content_start + b.saturating_sub(lead);
        let (a, b) = (a.max(content_start).min(end), b.max(content_start).min(end));
        (a < b).then_some((a, b))
    };
    let (raw_styles, raw_links) = if tag == "pre" {
        (Vec::new(), Vec::new())
    } else {
        collect_style_runs(elem)
    };
    let styles: Vec<StyleRun> = raw_styles
        .into_iter()
        .filter_map(|(a, b, bold, italic, link)| {
            rebase(a, b).map(|(start, end)| StyleRun {
                start,
                end,
                bold,
                italic,
                link,
            })
        })
        .collect();
    let links: Vec<LinkRun> = raw_links
        .into_iter()
        .filter_map(|(a, b, href)| rebase(a, b).map(|(start, end)| LinkRun { start, end, href }))
        .collect();
    let (list, list_depth) = list_context(elem, &tag, style_sheet);
    // Nesting is drawn as a real pixel inset, so it stacks with whatever the
    // stylesheet already asked for rather than replacing it.
    let indent = if list.is_some() {
        (indent + list_depth as f32 * LIST_INDENT_EM).min(MAX_INDENT_EM)
    } else {
        indent
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
        list,
        list_depth,
        blockquote: BlockquoteKind::None,
        svg: None,
        code_indent,
        links,
    })
}

/// Text belonging to this element itself: descendant text nodes, but not those
/// inside a nested block element (which gets its own segment).
fn own_text(elem: scraper::ElementRef) -> String {
    fn walk_own(node: ego_tree::NodeRef<scraper::Node>, out: &mut String) {
        for child in node.children() {
            match child.value() {
                scraper::Node::Text(t) => out.push_str(t),
                scraper::Node::Element(e) if !is_block_tag(e.name()) => walk_own(child, out),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk_own(*elem, &mut out);
    out
}

/// Tags that own a segment of their own, so an ancestor must not absorb them.
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "ul"
            | "ol"
            | "blockquote"
            | "pre"
            | "div"
            | "figure"
            | "img"
            | "image"
            | "table"
    )
}

/// The list marker kind and nesting depth for an `li`, read from its ancestors.
///
/// Resolved here rather than in a post-pass over the segment list: a post-pass
/// has to guess which `li` element produced which segment, and any item the
/// walk skipped (empty, or de-duplicated) silently shifts every marker after
/// it. Walking up from the element itself cannot drift.
fn list_context(
    elem: scraper::ElementRef,
    tag: &str,
    style_sheet: &BookStyle,
) -> (Option<ListKind>, u32) {
    if tag != "li" {
        return (None, 0);
    }
    let mut depth = 0u32;
    let mut owner: Option<scraper::ElementRef> = None;
    for anc in elem.ancestors() {
        let Some(e) = scraper::ElementRef::wrap(anc) else {
            continue;
        };
        if matches!(e.value().name(), "ul" | "ol") {
            if owner.is_none() {
                owner = Some(e);
            } else {
                depth += 1;
            }
        }
    }
    let Some(list_elem) = owner else {
        return (None, 0);
    };
    if list_marker_suppressed(list_elem, style_sheet)
        && has_ancestor(list_elem, &["nav"])
    {
        return (Some(ListKind::Unmarked), depth);
    }
    if list_elem.value().name() == "ol" {
        let start = list_elem
            .value()
            .attr("start")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);
        // Position among *sibling* items only -- a nested list's items restart
        // their own numbering rather than continuing the parent's.
        let preceding = elem
            .prev_siblings()
            .filter(|n| matches!(n.value(), scraper::Node::Element(e) if e.name() == "li"))
            .count() as u32;
        (Some(ListKind::Ordered(start + preceding)), depth)
    } else {
        (Some(ListKind::Unordered), depth)
    }
}

/// Does this `ul`/`ol` (or an ancestor list) set `list-style: none`?
///
/// Only asked of lists inside a `<nav>` -- a real EPUB table of contents, page
/// list or landmarks section, which is nearly always styled this way and whose
/// entries usually carry their own numbering already. `list-style: none` shows
/// up just as often on an ordinary numbered or bulleted list in the body text,
/// there purely to override the browser's default indent; suppressing those
/// markers too was the wrong read -- an in-chapter "eight things an agent does"
/// list lost its numbers entirely, which is a worse loss than a moot bullet on
/// a nav link. `list_context` gates the class/inline-style check on
/// `has_ancestor(list_elem, &["nav"])` for exactly this reason.
fn list_marker_suppressed(list_elem: scraper::ElementRef, style_sheet: &BookStyle) -> bool {
    let by_class = list_elem
        .value()
        .attr("class")
        .map(|classes| {
            classes
                .split_whitespace()
                .any(|c| style_sheet.no_marker.contains(c))
        })
        .unwrap_or(false);
    let by_inline = list_elem
        .value()
        .attr("style")
        .map(style::inline_list_style_none)
        .unwrap_or(false);
    by_class || by_inline
}

fn is_bold_tag(name: &str) -> bool {
    matches!(name, "b" | "strong")
}

fn is_italic_tag(name: &str) -> bool {
    matches!(name, "i" | "em" | "cite" | "var")
}

fn is_link_tag(name: &str) -> bool {
    name == "a"
}

struct StyleWalk {
    runs: Vec<(usize, usize, bool, bool, bool)>,
    links: Vec<(usize, usize, String)>,
}

fn walk(
    node: ego_tree::NodeRef<scraper::Node>,
    bold: bool,
    italic: bool,
    href: Option<&str>,
    pos: &mut usize,
    out: &mut StyleWalk,
) {
    for child in node.children() {
        match child.value() {
            scraper::Node::Text(t) => {
                let start = *pos;
                *pos += t.len();
                let link = href.is_some();
                if (bold || italic || link) && *pos > start {
                    out.runs.push((start, *pos, bold, italic, link));
                }
                if let Some(h) = href {
                    if *pos > start {
                        out.links.push((start, *pos, h.to_string()));
                    }
                }
            }
            scraper::Node::Element(e) => {
                let name = e.name();
                // An `<a>` without an href is an anchor target, not a link --
                // underlining it would promise a tap that goes nowhere.
                let child_href = if is_link_tag(name) {
                    e.attr("href").filter(|h| !h.trim().is_empty()).or(href)
                } else {
                    href
                };
                walk(
                    child,
                    bold || is_bold_tag(name),
                    italic || is_italic_tag(name),
                    child_href,
                    pos,
                    out,
                );
            }
            _ => {}
        }
    }
}

/// Walk a block's descendants in document order, recording which byte ranges of
/// its concatenated text sit inside a bold, italic, or link element.
///
/// Offsets are into the raw (untrimmed) text of the block, which is the same
/// string `elem.text()` produces -- both walk text nodes in the same order.
#[allow(clippy::type_complexity)]
fn collect_style_runs(
    elem: scraper::ElementRef,
) -> (
    Vec<(usize, usize, bool, bool, bool)>,
    Vec<(usize, usize, String)>,
) {
    let mut out = StyleWalk {
        runs: Vec::new(),
        links: Vec::new(),
    };
    let mut pos = 0usize;
    walk(*elem, false, false, None, &mut pos, &mut out);
    out.runs.dedup_by(|b, a| {
        if a.1 == b.0 && a.2 == b.2 && a.3 == b.3 && a.4 == b.4 {
            a.1 = b.1;
            true
        } else {
            false
        }
    });
    out.links.dedup_by(|b, a| {
        if a.1 == b.0 && a.2 == b.2 {
            a.1 = b.1;
            true
        } else {
            false
        }
    });
    (out.runs, out.links)
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

static BQ_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("blockquote").expect("selector"));
static TR_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tr").expect("selector"));
static TD_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("td, th").expect("selector"));
static TH_SELECTOR_OBJ: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("th").expect("selector"));

fn resolve_blockquote_context(segs: &mut [TextSegment], html: &Html, text: &str) {
    let bq_sel = BQ_SELECTOR_OBJ.clone();
    let block_sel = BLOCK_SELECTOR_OBJ.clone();
    for bq_elem in html.select(&bq_sel) {
        let bq_children: Vec<_> = bq_elem.select(&block_sel).collect();
        if bq_children.is_empty() {
            continue;
        }
        let bq_text: String = bq_elem.text().collect();
        let bq_trimmed = bq_text.trim();
        if bq_trimmed.is_empty() {
            continue;
        }
        let is_leaf = bq_children.len() == 1
            && bq_children[0].value().name() != "div"
            && bq_children[0].value().name() != "blockquote";
        for seg in segs.iter_mut() {
            if seg.tag == "blockquote" || seg.tag == "div" || seg.src.is_some() {
                continue;
            }
            let Some(seg_text) = text.get(seg.start..seg.end) else {
                continue;
            };
            if seg_text.is_empty() {
                continue;
            }
            if !bq_trimmed.contains(seg_text) {
                continue;
            }
            seg.blockquote = if is_leaf {
                BlockquoteKind::Leaf
            } else {
                BlockquoteKind::Children
            };
        }
    }
}

/// Turn a `<table>` into a stacked card list, in document order.
///
/// A fixed-width grid cannot work here. The text column is a few hundred
/// points wide and the body face is proportional, so even three columns of
/// ordinary prose overflow it, and padding cells to a character count is
/// meaningless for CJK, whose glyphs are twice as wide as the count implies.
///
/// So each `<tr>` becomes a card, the way responsive web tables collapse on a
/// phone: the row's first cell is the card's title, and every other cell is
/// emitted as its own `header: value` line. Nothing is truncated, everything
/// wraps, and -- unlike the grid, which was never added to the chapter text --
/// all of it reaches read-aloud.
fn extract_table_segments(
    table: scraper::ElementRef,
    text: &mut String,
    id: Option<String>,
) -> Vec<TextSegment> {
    let tr_sel = TR_SELECTOR_OBJ.clone();
    let td_sel = TD_SELECTOR_OBJ.clone();
    let rows: Vec<Vec<String>> = table
        .select(&tr_sel)
        .map(|tr| {
            tr.select(&td_sel)
                .map(|td| {
                    td.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        })
        .filter(|cells: &Vec<String>| !cells.is_empty())
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }

    // A header row is what turns a bare cell into `label: value`. Prefer a real
    // `<th>` row; fall back to the first row when every later row has the same
    // shape, which is how most converters emit a header they forgot to mark up.
    let has_th = table
        .select(&tr_sel)
        .next()
        .map(|tr| tr.select(&TH_SELECTOR_OBJ).next().is_some())
        .unwrap_or(false);
    let uniform = rows.len() > 1 && rows[1..].iter().all(|r| r.len() == rows[0].len());
    let (headers, body_rows) = if (has_th || uniform) && rows.len() > 1 {
        (Some(&rows[0]), &rows[1..])
    } else {
        (None, &rows[..])
    };

    let mut out = Vec::new();
    let mut push =
        |content: &str, indent: f32, bold: bool, text: &mut String, id: &mut Option<String>| {
            if content.is_empty() {
                return;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            let start = text.len();
            text.push_str(content);
            let end = text.len();
            let styles = if bold {
                vec![StyleRun {
                    start,
                    end,
                    bold: true,
                    italic: false,
                    link: false,
                }]
            } else {
                Vec::new()
            };
            out.push(TextSegment {
                start,
                end,
                tag: "p".to_string(),
                // The table's own id belongs to its first line, so an internal
                // link that targets the table lands at its top.
                id: id.take(),
                src: None,
                caption: None,
                indent,
                styles,
                list: None,
                list_depth: 0,
                blockquote: BlockquoteKind::None,
                svg: None,
                code_indent: false,
                links: Vec::new(),
            });
        };

    let mut pending_id = id;
    for row in body_rows {
        let mut cells = row.iter().enumerate();
        // Card title: the row's first cell, which is what identifies the row.
        if let Some((_, title)) = cells.next() {
            let title = if title.is_empty() { "\u{2014}" } else { title };
            push(title, 0.0, true, text, &mut pending_id);
        }
        for (ci, cell) in cells {
            if cell.is_empty() {
                continue;
            }
            let line = match headers.and_then(|h| h.get(ci)) {
                Some(label) if !label.is_empty() => format!("{label}: {cell}"),
                _ => cell.clone(),
            };
            push(&line, TABLE_CELL_INDENT_EM, false, text, &mut pending_id);
        }
    }
    out
}

/// Left inset for a table card's `label: value` lines, so they read as
/// belonging to the title above them.
const TABLE_CELL_INDENT_EM: f32 = 1.5;

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
                                    list: None,
                                    list_depth: 0,
                                    blockquote: BlockquoteKind::None,
                                    svg: None,
                                    code_indent: false,
                                    links: Vec::new(),
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
