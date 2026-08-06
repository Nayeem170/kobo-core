// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub src: String,
    pub alt: String,
    pub caption: String,
}

/// Extract every `<img>` in document order with its `alt` and (if inside a
/// `<figure>`) the figcaption text. Standalone images get an empty caption.
pub fn images(xhtml: &str) -> Vec<ImageRef> {
    let html = Html::parse_fragment(xhtml);
    let img_sel = Selector::parse("img").expect("selector");
    let fig_sel = Selector::parse("figure").expect("selector");
    let cap_sel = Selector::parse("figcaption").expect("selector");
    let mut fig_captions: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for fig in html.select(&fig_sel) {
        if let Some(img) = fig.select(&img_sel).next() {
            if let Some(src) = img.value().attr("src") {
                let cap = fig
                    .select(&cap_sel)
                    .next()
                    .map(|c| c.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();
                fig_captions.insert(src.to_string(), cap);
            }
        }
    }
    let mut out = Vec::new();
    for img in html.select(&img_sel) {
        let src = img.value().attr("src").unwrap_or_default().to_string();
        let alt = img.value().attr("alt").unwrap_or_default().to_string();
        let caption = fig_captions.get(&src).cloned().unwrap_or_default();
        out.push(ImageRef { src, alt, caption });
    }
    out
}

#[test]
fn extracts_paragraphs_and_offsets() {
    let xhtml = r#"<html><body>
            <h1>Title</h1>
            <p>First paragraph here.</p>
            <p>Second one.</p>
            </body></html>"#;
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 3);
    assert_eq!(&text[segs[0].start..segs[0].end], "Title");
    assert_eq!(segs[0].tag, "h1");
    assert_eq!(&text[segs[1].start..segs[1].end], "First paragraph here.");
    assert_eq!(segs[1].tag, "p");
    assert_eq!(&text[segs[2].start..segs[2].end], "Second one.");
}

#[test]
fn pre_block_preserves_newlines_and_indentation() {
    let xhtml = "<pre><code>fn main() {\n    println!(\"hi\");\n}</code></pre>";
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].tag, "pre");
    let code = &text[segs[0].start..segs[0].end];
    assert_eq!(code, "fn main() {\n    println!(\"hi\");\n}");
    // The indentation on the middle line must survive.
    assert!(code.contains("\n    println!"));
}

#[test]
fn pre_strips_only_surrounding_blank_lines() {
    let xhtml = "<pre>\n  a\n  b\n</pre>";
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 1);
    assert_eq!(&text[segs[0].start..segs[0].end], "  a\n  b");
}

#[test]
fn dedups_wrapping_divs() {
    let xhtml = "<div><p>Hello world.</p></div>";
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 1, "got: {segs:?}");
    assert_eq!(&text, "Hello world.");
}

#[test]
fn skips_multi_para_div() {
    let xhtml = "<div><p>Alpha.</p><p>Beta.</p></div>";
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 2, "got: {segs:?}");
    assert!(text.contains("Alpha."));
    assert!(text.contains("Beta."));
    assert_eq!(segs[0].tag, "p");
    assert_eq!(segs[1].tag, "p");
}

#[test]
fn keeps_leaf_div() {
    let xhtml = "<div>Plain text with no block children.</div>";
    let (text, segs) = extract(xhtml);
    assert_eq!(segs.len(), 1, "got: {segs:?}");
    assert!(text.contains("Plain text"));
    assert_eq!(segs[0].tag, "div");
}

#[test]
fn segment_at_locates_offset() {
    let xhtml = "<p>Alpha.</p><p>Beta gamma.</p>";
    let (text, segs) = extract(xhtml);
    let beta_off = text.find("Beta").unwrap();
    let seg = segment_at(&segs, beta_off).unwrap();
    assert_eq!(&text[seg.start..seg.end], "Beta gamma.");
}

#[test]
fn word_offset_roundtrip_for_highlight() {
    let xhtml = "<p>Alpha beta.</p><p>Gamma delta.</p>";
    let (text, segs) = extract(xhtml);
    let gamma_off = text.find("Gamma").unwrap();
    let seg = segment_at(&segs, gamma_off).unwrap();
    assert_eq!(&text[seg.start..seg.end], "Gamma delta.");
    assert_eq!(seg.tag, "p");
}

#[test]
fn extracts_images_with_captions() {
    let xhtml = r#"<p>Intro.</p>
            <figure><img src="images/fox.png" alt="A fox"/>
              <figcaption>Fig. 1 - the fox leaps.</figcaption></figure>
            <p>Body.</p>
            <img src="images/inline.jpg" alt="inline pic"/>"#;
    let imgs = images(xhtml);
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].src, "images/fox.png");
    assert_eq!(imgs[0].alt, "A fox");
    assert_eq!(imgs[0].caption, "Fig. 1 - the fox leaps.");
    assert_eq!(imgs[1].src, "images/inline.jpg");
    assert!(imgs[1].caption.is_empty());
}

#[test]
fn extract_carries_images_in_flow_order() {
    let xhtml = r#"<p>First paragraph.</p>
            <figure><img src="a.png" alt="A"/><figcaption>Cap A</figcaption></figure>
            <p>Second paragraph.</p>"#;
    let (text, segs) = extract(xhtml);
    assert!(text.contains("First paragraph."));
    assert!(text.contains("Second paragraph."));
    assert!(!text.contains("Cap A"));
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].tag, "p");
    assert_eq!(segs[1].tag, "figure");
    assert_eq!(segs[1].src.as_deref(), Some("a.png"));
    assert_eq!(segs[1].caption.as_deref(), Some("Cap A"));
    assert_eq!(segs[1].start, segs[1].end);
    assert_eq!(segs[2].tag, "p");
}

#[test]
fn extracts_svg_image_xlink_href() {
    let xhtml = r#"<html><body>
            <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
                <image width="1275" height="1650" xlink:href="cover.jpeg"/>
            </svg>
            </body></html>"#;
    let (_text, segs) = extract(xhtml);
    let img_segs: Vec<_> = segs.iter().filter(|s| s.src.is_some()).collect();
    assert_eq!(img_segs.len(), 1, "expected 1 SVG image segment");
    assert_eq!(img_segs[0].src.as_deref(), Some("cover.jpeg"));
    assert_eq!(img_segs[0].start, img_segs[0].end);
}

#[test]
fn extracts_svg_image_href_fallback() {
    let xhtml = r#"<svg><image width="100" height="100" href="diagram.png"/></svg>"#;
    let (_text, segs) = extract(xhtml);
    let img_segs: Vec<_> = segs.iter().filter(|s| s.src.is_some()).collect();
    assert_eq!(img_segs.len(), 1);
    assert_eq!(img_segs[0].src.as_deref(), Some("diagram.png"));
}

#[test]
fn does_not_duplicate_already_captured_svg_image() {
    let xhtml = r#"<figure><img src="a.png" alt="A"/></figure>"#;
    let (_text, segs) = extract(xhtml);
    let img_count = segs.iter().filter(|s| s.src.is_some()).count();
    assert_eq!(img_count, 1, "image should appear exactly once");
}

#[test]
fn svg_image_with_no_href_is_ignored() {
    let xhtml = r#"<svg><image width="100" height="100"/></svg>"#;
    let (_text, segs) = extract(xhtml);
    let img_count = segs.iter().filter(|s| s.src.is_some()).count();
    assert_eq!(img_count, 0);
}

/// Calibre-converted technical books emit one `<p class="calibreN">` per source
/// line and put the nesting depth in the class's `margin-left`. Losing it
/// flattens Python listings, where indentation carries the control flow.
#[test]
fn resolves_block_indent_from_the_stylesheet() {
    let css = ".lvl1 { margin-left: 2em } .lvl2 { margin-left: 3em }";
    let indents = crate::html_text::parse_indents(css);
    let xhtml = r#"<p>prose</p><p class="lvl1">if x:</p><p class="lvl2">return x</p>"#;
    let (_text, segs) = extract_with_indents(xhtml, &indents);
    let ind: Vec<f32> = segs.iter().map(|s| s.indent).collect();
    assert_eq!(ind, vec![0.0, 2.0, 3.0]);
}

#[test]
fn leading_spaces_add_to_the_class_indent() {
    let indents = crate::html_text::parse_indents(".lvl { margin-left: 2em }");
    let xhtml = "<p class=\"lvl\">\u{00A0}\u{00A0}'key': 1,</p>";
    let (text, segs) = extract_with_indents(xhtml, &indents);
    assert_eq!(segs[0].indent, 3.0, "2em class + 2 leading nbsp");
    assert_eq!(text, "'key': 1,", "the spaces become indent, not text");
}

#[test]
fn prose_without_an_indent_class_stays_flush() {
    let indents = crate::html_text::parse_indents(".lvl { margin-left: 2em }");
    let xhtml = r#"<p class="body">An ordinary paragraph.</p>"#;
    let (_text, segs) = extract_with_indents(xhtml, &indents);
    assert_eq!(segs[0].indent, 0.0);
}

/// `pre` already carries its indentation inside the text, so a block indent on
/// top of it would double every level.
#[test]
fn pre_takes_no_block_indent() {
    let indents = crate::html_text::parse_indents(".lvl { margin-left: 2em }");
    let xhtml = "<pre class=\"lvl\">    indented</pre>";
    let (_text, segs) = extract_with_indents(xhtml, &indents);
    assert_eq!(segs[0].indent, 0.0);
}

/// The Calibre code-margin is the one indent that should render as code. A
/// paragraph whose class resolves through the `IndentMap` carries
/// `code_indent`; flush prose, nested list items, and table cards also indent
/// but through other fields, so they stay `false` and render as prose -- not
/// faint monospace.
#[test]
fn only_calibre_code_margin_marks_a_segment_as_code() {
    let indents = crate::html_text::parse_indents(".lvl { margin-left: 2em }");
    let xhtml = r#"<p>flush</p><p class="lvl">if x:</p>"#;
    let (_t, segs) = extract_with_indents(xhtml, &indents);
    assert!(!segs[0].code_indent, "flush prose is not code");
    assert!(segs[1].code_indent, "Calibre-margin paragraph is code");

    let (_t, segs) = extract("<ul><li>top<ul><li>inner</li></ul></li></ul>");
    assert!(
        segs.iter().all(|s| !s.code_indent),
        "nested list items are not code even though they indent"
    );

    let (_t, segs) = extract("<table><tr><td>cell</td></tr></table>");
    assert!(
        segs.iter().all(|s| !s.code_indent),
        "table-card lines are not code even though they indent"
    );
}

// ---- emphasis ------------------------------------------------------------

fn styled(xhtml: &str) -> (String, Vec<StyleRun>) {
    let (text, segs) = extract(xhtml);
    (text, segs.into_iter().flat_map(|s| s.styles).collect())
}

#[test]
fn captures_bold_and_italic_spans() {
    let (text, runs) = styled("<p>plain <b>bold</b> and <i>italic</i> end</p>");
    assert_eq!(text, "plain bold and italic end");
    assert_eq!(runs.len(), 2);
    assert_eq!(&text[runs[0].start..runs[0].end], "bold");
    assert!(runs[0].bold && !runs[0].italic);
    assert_eq!(&text[runs[1].start..runs[1].end], "italic");
    assert!(runs[1].italic && !runs[1].bold);
}

#[test]
fn strong_and_em_count_too() {
    let (text, runs) = styled("<p>a <strong>S</strong> b <em>E</em></p>");
    assert_eq!(runs.len(), 2);
    assert!(runs[0].bold, "strong is bold");
    assert!(runs[1].italic, "em is italic");
    assert_eq!(&text[runs[1].start..runs[1].end], "E");
}

#[test]
fn nested_emphasis_is_both() {
    let (text, runs) = styled("<p>x <b>bold <i>both</i></b></p>");
    let both = runs
        .iter()
        .find(|r| r.bold && r.italic)
        .expect("nested run");
    assert_eq!(&text[both.start..both.end], "both");
}

/// Offsets are measured in the untrimmed text, so leading whitespace must be
/// discounted or every run points a few bytes too far right.
#[test]
fn offsets_survive_the_leading_trim() {
    let (text, runs) = styled("<p>   lead <b>bold</b></p>");
    assert_eq!(text, "lead bold");
    assert_eq!(runs.len(), 1);
    assert_eq!(&text[runs[0].start..runs[0].end], "bold");
}

#[test]
fn plain_paragraphs_have_no_runs() {
    let (_, runs) = styled("<p>nothing special here</p>");
    assert!(runs.is_empty());
}

#[test]
fn adjacent_identical_runs_merge() {
    let (text, runs) = styled("<p><b>one</b><b>two</b></p>");
    assert_eq!(runs.len(), 1, "touching bold spans are one run: {runs:?}");
    assert_eq!(&text[runs[0].start..runs[0].end], "onetwo");
}

/// `pre` is verbatim; its markup is code, not emphasis.
#[test]
fn pre_carries_no_emphasis() {
    let (_, runs) = styled("<pre>let <b>x</b> = 1;</pre>");
    assert!(runs.is_empty());
}

#[test]
fn runs_stay_inside_their_segment() {
    let (text, segs) = extract("<p>a <b>B</b></p><p>c <i>D</i></p>");
    for seg in &segs {
        for r in &seg.styles {
            assert!(
                r.start >= seg.start && r.end <= seg.end,
                "run {r:?} escapes segment {}..{} in {text:?}",
                seg.start,
                seg.end
            );
        }
    }
}

// --- lists -----------------------------------------------------------------

fn li_segs(xhtml: &str) -> Vec<TextSegment> {
    extract(xhtml)
        .1
        .into_iter()
        .filter(|s| s.tag == "li")
        .collect()
}

#[test]
fn unordered_items_get_a_bullet() {
    let segs = li_segs("<ul><li>one</li><li>two</li></ul>");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].list_marker().as_deref(), Some("\u{2022} "));
    assert_eq!(segs[1].list_marker().as_deref(), Some("\u{2022} "));
}

#[test]
fn ordered_items_number_from_one() {
    let segs = li_segs("<ol><li>one</li><li>two</li><li>three</li></ol>");
    let markers: Vec<_> = segs.iter().filter_map(|s| s.list_marker()).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
}

#[test]
fn ordered_list_honours_the_start_attribute() {
    let segs = li_segs(r#"<ol start="5"><li>five</li><li>six</li></ol>"#);
    let markers: Vec<_> = segs.iter().filter_map(|s| s.list_marker()).collect();
    assert_eq!(markers, vec!["5. ", "6. "]);
}

/// A real table of contents (`<nav>`) turns the marker off in CSS; drawing
/// numbers there is worse than drawing nothing, because the entries usually
/// carry their own numbering already.
#[test]
fn list_style_none_suppresses_the_marker_inside_nav() {
    let style = crate::html_text::parse_book_style(".toc { list-style-type: none }");
    let (_, segs) = extract_with_style(
        r#"<nav><ul class="toc"><li>Chapter One</li></ul></nav>"#,
        &style,
    );
    let li: Vec<_> = segs.iter().filter(|s| s.tag == "li").collect();
    assert_eq!(li.len(), 1);
    assert_eq!(li[0].list, Some(ListKind::Unmarked));
    assert_eq!(li[0].list_marker(), None, "unmarked lists draw no bullet");
}

#[test]
fn inline_list_style_none_suppresses_the_marker_inside_nav() {
    let (_, segs) = extract(r#"<nav><ol style="list-style: none"><li>Contents</li></ol></nav>"#);
    let li: Vec<_> = segs.iter().filter(|s| s.tag == "li").collect();
    assert_eq!(li[0].list_marker(), None);
}

/// An unmarked list still indents to its depth -- only the bullet is gone.
#[test]
fn unmarked_nested_list_keeps_its_indent() {
    let (_, segs) = extract(
        r#"<nav><ul style="list-style: none"><li>top<ul style="list-style: none"><li>inner</li></ul></li></ul></nav>"#,
    );
    let li: Vec<_> = segs.iter().filter(|s| s.tag == "li").collect();
    assert_eq!(li.len(), 2);
    assert_eq!(li[0].list_depth, 0);
    assert_eq!(li[1].list_depth, 1);
    assert!(li[1].indent > li[0].indent, "nested item indents further");
}

/// The device regression this gate exists for: an in-chapter numbered list
/// ("A Simple Example First", "What Is an Agent?", ...) styled with
/// `list-style: none` purely to kill the browser's default indent. The book
/// never wrote its own numerals into the text, so suppressing the marker left
/// the list looking like unordered plain paragraphs -- readers could no longer
/// tell it was an ordered list at all.
#[test]
fn list_style_none_outside_nav_keeps_its_numbers() {
    let style = crate::html_text::parse_book_style(".contents { list-style-type: none }");
    let (_, segs) = extract_with_style(
        r#"<ol class="contents"><li>A Simple Example First</li><li>What Is an Agent?</li></ol>"#,
        &style,
    );
    let markers: Vec<_> = segs.iter().filter_map(|s| s.list_marker()).collect();
    assert_eq!(
        markers,
        vec!["1. ", "2. "],
        "a body-text ordered list must keep its numbers even when CSS hides the bullet"
    );
}

#[test]
fn list_style_none_outside_nav_keeps_bullets_too() {
    let (_, segs) = extract(r#"<ul style="list-style: none"><li>First</li><li>Second</li></ul>"#);
    let markers: Vec<_> = segs.iter().filter_map(|s| s.list_marker()).collect();
    assert_eq!(markers, vec!["\u{2022} ", "\u{2022} "]);
}

#[test]
fn nested_list_items_get_their_own_depth() {
    let (_, segs) = extract("<ul><li>outer<ul><li>inner</li></ul></li></ul>");
    let li: Vec<_> = segs.iter().filter(|s| s.tag == "li").collect();
    assert_eq!(li.len(), 2, "both items produce a segment");
    assert_eq!(li[0].list_depth, 0);
    assert_eq!(li[1].list_depth, 1);
}

/// A parent item must contribute only its own label. Collecting descendant
/// text duplicated the nested items and starved them of their own segments.
#[test]
fn parent_item_does_not_absorb_nested_item_text() {
    let (text, segs) = extract("<ul><li>outer<ul><li>inner</li></ul></li></ul>");
    let li: Vec<_> = segs.iter().filter(|s| s.tag == "li").collect();
    assert_eq!(&text[li[0].start..li[0].end], "outer");
    assert_eq!(&text[li[1].start..li[1].end], "inner");
}

/// Nested ordered lists restart, they do not continue the outer count.
#[test]
fn nested_ordered_list_restarts_numbering() {
    let (_, segs) = extract("<ol><li>a</li><li>b<ol><li>b1</li><li>b2</li></ol></li></ol>");
    let markers: Vec<_> = segs
        .iter()
        .filter(|s| s.tag == "li")
        .filter_map(|s| s.list_marker())
        .collect();
    assert_eq!(markers, vec!["1. ", "2. ", "1. ", "2. "]);
}

// --- links -----------------------------------------------------------------

#[test]
fn captures_the_href_of_a_link() {
    let (text, segs) = extract(r#"<p>see <a href="ch02.xhtml#top">chapter two</a></p>"#);
    let links: Vec<_> = segs.iter().flat_map(|s| s.links.iter()).collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].href, "ch02.xhtml#top");
    assert_eq!(&text[links[0].start..links[0].end], "chapter two");
}

#[test]
fn link_text_is_also_marked_for_underlining() {
    let (_, segs) = extract(r#"<p><a href="x.xhtml">go</a></p>"#);
    let runs: Vec<_> = segs.iter().flat_map(|s| s.styles.iter()).collect();
    assert!(runs.iter().any(|r| r.link), "link run drives the underline");
}

/// A bare `<a id=...>` is an anchor target, not a link. Underlining it would
/// promise a tap that goes nowhere.
#[test]
fn anchor_without_href_is_not_a_link() {
    let (_, segs) = extract(r#"<p><a id="top">Title</a></p>"#);
    assert!(segs.iter().all(|s| s.links.is_empty()));
    assert!(segs.iter().flat_map(|s| s.styles.iter()).all(|r| !r.link));
}

#[test]
fn links_inside_list_items_are_captured() {
    let (text, segs) = extract(r#"<ol><li><a href="c1.xhtml">One</a></li></ol>"#);
    let links: Vec<_> = segs.iter().flat_map(|s| s.links.iter()).collect();
    assert_eq!(links.len(), 1, "a TOC entry is a link inside an li");
    assert_eq!(&text[links[0].start..links[0].end], "One");
}

// --- tables ----------------------------------------------------------------

#[test]
fn table_linearises_to_one_card_per_row() {
    let (text, _segs) = extract(
        "<table><tr><th>Type</th><th>Analogy</th><th>Lasts</th></tr><tr><td>Sensory</td><td>A glimpse</td><td>A flash</td></tr></table>",
    );
    assert!(text.contains("Sensory"), "row title present: {text:?}");
    assert!(
        text.contains("Analogy: A glimpse"),
        "labelled cell: {text:?}"
    );
    assert!(text.contains("Lasts: A flash"), "labelled cell: {text:?}");
    assert!(!text.contains('|'), "no ascii grid pipes: {text:?}");
}

/// The header row labels the cells; it is not emitted as a card of its own.
#[test]
fn table_header_row_is_not_a_card() {
    let (text, _) = extract(
        "<table><tr><th>Name</th><th>Age</th></tr><tr><td>Ada</td><td>36</td></tr></table>",
    );
    assert!(
        text.starts_with("Ada"),
        "first card is the first body row: {text:?}"
    );
}

/// A table between two paragraphs must render between them, not after the
/// chapter's last paragraph.
#[test]
fn table_lands_in_document_order() {
    let (text, _) = extract("<p>before</p><table><tr><td>cell</td></tr></table><p>after</p>");
    let b = text.find("before").expect("before");
    let c = text.find("cell").expect("cell");
    let a = text.find("after").expect("after");
    assert!(
        b < c && c < a,
        "table sits between the paragraphs: {text:?}"
    );
}

/// Table text reaches the chapter string, so read-aloud picks it up. The old
/// ASCII grid was appended outside it and was silent.
#[test]
fn table_text_is_part_of_the_chapter_text() {
    let (text, segs) = extract("<table><tr><td>only</td></tr></table>");
    assert!(text.contains("only"));
    assert!(
        segs.iter().any(|s| text[s.start..s.end].contains("only")),
        "a segment covers the cell text"
    );
}

#[test]
fn table_cells_are_not_extracted_twice() {
    let (text, _) = extract("<table><tr><td><p>cell</p></td></tr></table>");
    assert_eq!(text.matches("cell").count(), 1, "{text:?}");
}

#[test]
fn table_survives_multibyte_cells() {
    let (text, _) = extract("<table><tr><th>名前</th></tr><tr><td>日本語のセル</td></tr></table>");
    assert!(text.contains("日本語のセル"), "{text:?}");
}

// --- figures ---------------------------------------------------------------

/// A figure wrapping an SVG `<image>` is the shape most diagram exporters
/// emit. Reading only `<img src>` dropped the picture and its caption.
#[test]
fn figure_falls_back_to_the_svg_image_href() {
    let (_, segs) = extract(
        r#"<figure><svg><image xlink:href="graph.png"/></svg><figcaption>Fig 1</figcaption></figure>"#,
    );
    let fig = segs.iter().find(|s| s.tag == "figure").expect("figure");
    assert_eq!(fig.src.as_deref(), Some("graph.png"));
    assert_eq!(fig.caption.as_deref(), Some("Fig 1"));
}

/// A diagram drawn in the chapter rather than referenced as a file. It has no
/// `src` and no text, so it used to extract to nothing at all -- the figure
/// rendered as its caption over blank space.
#[test]
fn figure_carries_inline_svg_markup() {
    let (_, segs) = extract(
        r#"<figure><svg width="100" height="50"><rect width="100" height="50"/></svg><figcaption>Fig 1</figcaption></figure>"#,
    );
    let fig = segs.iter().find(|s| s.tag == "figure").expect("figure");
    assert_eq!(fig.caption.as_deref(), Some("Fig 1"));
    let svg = fig.svg.as_deref().expect("inline markup captured");
    assert!(svg.contains("<rect"), "{svg}");
    assert!(svg.contains("xmlns"), "namespace restored: {svg}");
    assert!(
        fig.src.as_deref().is_some_and(|s| s.starts_with('#')),
        "needs a lookup key no archive path can collide with: {:?}",
        fig.src
    );
}

/// SVG is case-sensitive where HTML is not. `viewBox` decides how the whole
/// drawing scales, so losing its casing on the parse/serialise round-trip would
/// render every diagram at the wrong size.
#[test]
fn inline_svg_keeps_case_sensitive_attributes() {
    let (_, segs) = extract(
        r#"<figure><svg viewBox="0 0 10 20" preserveAspectRatio="xMidYMid"><clipPath id="c"><rect/></clipPath></svg></figure>"#,
    );
    let svg = segs
        .iter()
        .find_map(|s| s.svg.as_deref())
        .expect("inline markup");
    assert!(svg.contains("viewBox"), "{svg}");
    assert!(svg.contains("preserveAspectRatio"), "{svg}");
    assert!(svg.contains("clipPath"), "{svg}");
}

/// An `<svg>` that only wraps `<image href=...>` points at an archive entry.
/// Capturing the wrapper too would draw an empty box over the real picture.
#[test]
fn svg_wrapping_a_referenced_image_is_not_captured_as_markup() {
    let (_, segs) =
        extract(r#"<figure><svg><image xlink:href="graph.png"/></svg><figcaption>F</figcaption></figure>"#);
    let fig = segs.iter().find(|s| s.tag == "figure").expect("figure");
    assert_eq!(fig.src.as_deref(), Some("graph.png"));
    assert!(fig.svg.is_none(), "{:?}", fig.svg);
}

/// A drawing outside any `<figure>` still has to reach the page.
#[test]
fn standalone_inline_svg_becomes_an_image_segment() {
    let (_, segs) = extract(r#"<p>Before</p><svg width="10" height="10"><circle r="5"/></svg><p>After</p>"#);
    let img = segs
        .iter()
        .find(|s| s.svg.is_some())
        .expect("inline drawing");
    assert_eq!(img.tag, "image");
    assert!(img.svg.as_deref().unwrap().contains("<circle"));
}

/// The figure owns its drawing. Without the ancestor guard the `svg` block
/// selector emitted a second segment and the diagram rendered twice.
#[test]
fn inline_svg_inside_a_figure_is_emitted_once() {
    let (_, segs) =
        extract(r#"<figure><svg width="10" height="10"><circle r="5"/></svg><figcaption>F</figcaption></figure>"#);
    assert_eq!(
        segs.iter().filter(|s| s.svg.is_some()).count(),
        1,
        "{segs:#?}"
    );
}

/// An empty `<svg>` is a placeholder, not a picture. Emitting a segment for it
/// would insert a blank gap in the flow.
#[test]
fn empty_inline_svg_is_ignored() {
    let (_, segs) = extract(r#"<p>Text</p><svg width="10" height="10"></svg>"#);
    assert!(segs.iter().all(|s| s.svg.is_none()), "{segs:#?}");
}

/// Two drawings in one chapter must not share a lookup key, or both figures
/// resolve to the same picture.
#[test]
fn inline_svg_keys_are_unique_within_a_chapter() {
    let (_, segs) = extract(
        r#"<svg width="10" height="10"><circle r="5"/></svg><svg width="10" height="10"><rect width="4" height="4"/></svg>"#,
    );
    let keys: Vec<_> = segs
        .iter()
        .filter(|s| s.svg.is_some())
        .filter_map(|s| s.src.clone())
        .collect();
    assert_eq!(keys.len(), 2, "{segs:#?}");
    assert_ne!(keys[0], keys[1]);
}

#[test]
fn figure_caption_is_not_also_extracted_as_prose() {
    let (_, segs) =
        extract(r#"<figure><img src="a.png"/><figcaption><p>Caption</p></figcaption></figure>"#);
    let fig = segs.iter().find(|s| s.tag == "figure").expect("figure");
    assert_eq!(fig.caption.as_deref(), Some("Caption"));
    assert!(
        !segs.iter().any(|s| s.tag == "p"),
        "the caption's <p> must not become a second segment"
    );
}

#[test]
fn caption_only_figure_still_produces_a_segment() {
    let (_, segs) = extract("<figure><figcaption>Just words</figcaption></figure>");
    let fig = segs.iter().find(|s| s.tag == "figure").expect("figure");
    assert_eq!(fig.caption.as_deref(), Some("Just words"));
}

// --- orphan text (issue 18) -------------------------------------------------

/// An unescaped custom container (`<examples>`, `<input>` — a void element in
/// HTML) matches nothing in `BLOCK_SELECTOR`, so its text used to vanish with
/// no warning: the real paragraphs on either side extracted fine, and the
/// example block in between silently disappeared.
#[test]
fn unescaped_custom_tag_no_longer_vanishes() {
    let (text, segs) = extract(
        r#"<p>Real paragraph one.</p><examples><example><input>The cart total is wrong.</input></example></examples><p>Real paragraph two.</p>"#,
    );
    let ps: Vec<_> = segs.iter().filter(|s| s.tag == "p").collect();
    assert_eq!(ps.len(), 2, "both real paragraphs must still extract: {segs:#?}");
    assert!(
        text.contains("The cart total is wrong."),
        "the orphaned custom-tag text must reach the chapter text: {text:?}"
    );
    let orphan = segs
        .iter()
        .find(|s| text[s.start..s.end].contains("The cart total is wrong."))
        .expect("orphan text must have its own segment");
    assert!(
        orphan.start < orphan.end,
        "orphan segment must carry a real range: {orphan:?}"
    );
}

/// Bare text placed directly in `<body>`, wrapped in no element at all, hits
/// the same hole.
#[test]
fn bare_body_text_is_captured() {
    let (text, segs) = extract("<p>Intro.</p>Some stray text with no wrapper at all.");
    assert!(
        text.contains("Some stray text with no wrapper at all."),
        "{text:?}"
    );
    assert!(
        segs.iter()
            .any(|s| text[s.start..s.end].contains("Some stray text")),
        "the stray text must have its own segment: {segs:#?}"
    );
}

/// The extractor must not duplicate content it already captured properly --
/// the crude version of this fix used to re-dump the whole body once
/// coverage fell under a threshold, doubling up everything captured so far.
#[test]
fn orphan_collection_does_not_duplicate_already_captured_text() {
    let (text, _) = extract(
        r#"<p>First real paragraph with enough words to matter.</p><weird>Stray orphan content.</weird>"#,
    );
    let occurrences = text.matches("First real paragraph").count();
    assert_eq!(
        occurrences, 1,
        "already-captured content must not also appear in the orphan dump: {text:?}"
    );
}

/// A normal chapter with no orphans at all must be unaffected -- the fallback
/// must not fire, or duplicate, when there is nothing missing.
#[test]
fn no_orphan_pass_when_nothing_is_missing() {
    let (text, segs) = extract("<p>Only a normal paragraph.</p>");
    assert_eq!(segs.len(), 1, "{segs:#?}");
    assert_eq!(text, "Only a normal paragraph.");
}

/// `<script>` and `<style>` at the end of a chapter's body carry JavaScript
/// and CSS, not page prose. Without a non-content skip, the orphan collector
/// treated them as unknown containers and injected their code as visible text
/// -- the "raw .xhtml at chapter end" symptom.
#[test]
fn script_and_style_at_body_end_do_not_leak() {
    let (text, segs) = extract(
        r#"<p>Real content here.</p>
<script>if (window.foo) { bar(); }</script>
<style>.cls { color: red; }</style>"#,
    );
    assert!(
        !text.contains("window.foo"),
        "JavaScript must not reach chapter text: {text:?}"
    );
    assert!(
        !text.contains("color: red"),
        "CSS must not reach chapter text: {text:?}"
    );
    assert!(
        segs.iter().all(|s| s.tag != "div" || !text[s.start..s.end].contains("bar()")),
        "no orphan segment should carry script text: {segs:#?}"
    );
}

/// A blockquote whose scraped text diverges from the chapter text (here: a
/// leading `<figure>` caption, which the scraper includes but the chapter
/// text excludes) and carries multi-byte characters. The blockquote-context
/// resolver used to slice the blockquote's scraped text with chapter-text
/// offsets; with the caption pushing the multi-byte run to the front, a later
/// paragraph's offset landed inside a 3-byte smart quote and panicked. The
/// scan thread died and the whole library came back empty (one bad book
/// blanks every book). The fix slices the chapter text with those offsets.
#[test]
fn blockquote_context_does_not_panic_on_multibyte_scrape_divergence() {
    let rq = "\u{2019}"; // right single quote, 3 bytes in UTF-8
    let xhtml = format!(
        "<blockquote>\
         <figure><img src=\"x.png\"/><figcaption>{rq}{rq}{rq}</figcaption></figure>\
         <p>XX</p>\
         </blockquote>"
    );
    let (_text, segs) = extract(&xhtml);
    // Reaching here means no panic. The blockquote still flags its paragraph.
    let flagged = segs
        .iter()
        .filter(|s| s.tag == "p")
        .filter(|s| s.blockquote != BlockquoteKind::None)
        .count();
    assert!(
        flagged >= 1,
        "the paragraph inside the blockquote must carry blockquote context: {segs:#?}"
    );
}
