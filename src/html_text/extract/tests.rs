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
