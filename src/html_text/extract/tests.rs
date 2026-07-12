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
