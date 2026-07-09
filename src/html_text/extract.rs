//! DOM extraction: chapter XHTML → plain text + per-block-element segments.

use scraper::{Html, Selector};
use std::sync::LazyLock;

/// A run of chapter text belonging to one block element, with its char range.
/// For `<img>`/`<figure>` segments the text range is empty (zero-width marker at
/// the image's flow position) and `src`/`caption` carry the image data.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
}

impl TextSegment {
    /// Does this segment contain char offset `i`?
    pub fn contains(&self, i: usize) -> bool {
        i >= self.start && i < self.end
    }
}

/// Selector for the block elements we extract as flow items (text + images).
/// `image` matches SVG `<image>` (covers, inline SVG art).
const BLOCK_SELECTOR: &str = "p, h1, h2, h3, h4, h5, h6, li, blockquote, div, figure, img, image";
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
                    })
                }
            }
            "image" => extract_svg_image_segment(elem, pos, id),
            _ => extract_text_segment(elem, &child_sel, &mut text, pos, tag, id, &segs),
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
) -> Option<TextSegment> {
    if CONTAINER_TAGS.contains(&tag.as_str()) && elem.select(child_sel).next().is_some() {
        return None;
    }
    let raw: String = elem.text().collect();
    let t = raw.trim();
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
    Some(TextSegment {
        start: content_start,
        end,
        tag,
        id,
        src: None,
        caption: None,
    })
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

/// A standalone image reference extracted from chapter XHTML.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub src: String,
    pub alt: String,
    pub caption: String,
}

/// Extract every `<img>` in document order with its `alt` and (if inside a
/// `<figure>`) the figcaption text. Standalone images get an empty caption.
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
              <figcaption>Fig. 1 — the fox leaps.</figcaption></figure>
            <p>Body.</p>
            <img src="images/inline.jpg" alt="inline pic"/>"#;
        let imgs = images(xhtml);
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].src, "images/fox.png");
        assert_eq!(imgs[0].alt, "A fox");
        assert_eq!(imgs[0].caption, "Fig. 1 — the fox leaps.");
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
}
