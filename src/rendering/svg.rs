// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! SVG rasterisation for book figures.
//!
//! Technical books draw their diagrams in SVG -- it is what every flow-chart
//! and plotting tool exports -- and the `image` crate decodes raster formats
//! only. Without this, `decode_image` returned `None` for every diagram and the
//! figure rendered as its caption alone, which reads as a missing picture
//! rather than an unsupported format.

use std::sync::{Arc, OnceLock};

use log::warn;

use super::text_render::DecodedImage;

/// Ceiling on the rasterised pixel count, ~4x a full Kobo screen.
///
/// An SVG declares its own size, and a page-sized diagram scaled to the text
/// column is cheap, but a document that declares a huge canvas would otherwise
/// allocate the square of it. The reader has no swap.
const MAX_PIXELS: usize = 1264 * 1680 * 4;

/// Does this byte slice look like SVG?
///
/// Sniffed rather than taken from the file extension: an EPUB may serve a
/// diagram from a path with no extension at all, and the manifest media-type is
/// not carried this far down. A prolog, DOCTYPE or comment may precede the root
/// element, so the scan looks past them -- but only through the head of the
/// file, so a raster image that happens to contain the bytes `<svg` in its
/// pixel data is not misread as markup.
pub fn is_svg(raw: &[u8]) -> bool {
    const HEAD: usize = 1024;
    let head = &raw[..raw.len().min(HEAD)];
    // Strip a UTF-8 BOM, then leading whitespace.
    let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(head);
    let head = match head.iter().position(|b| !b.is_ascii_whitespace()) {
        Some(i) => &head[i..],
        None => return false,
    };
    if head.starts_with(b"<svg") {
        return true;
    }
    // A prolog (`<?xml ...?>`), DOCTYPE or comment can come first. Anything
    // else at the root means this is not SVG.
    if !(head.starts_with(b"<?xml") || head.starts_with(b"<!")) {
        return false;
    }
    head.windows(4).any(|w| w == b"<svg")
}

/// Faces available to SVG `<text>` elements.
///
/// `system-fonts` is off, and a Kobo has no font directory resvg would find, so
/// without this every text label in a diagram -- the axis titles, the box
/// captions, often the entire content of a flow chart -- renders as nothing at
/// all. The reader's own embedded faces are registered instead, so a diagram
/// gets the same typeface as the prose around it.
fn fontdb() -> &'static Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        for face in super::text_render::embedded_font_data() {
            db.load_font_data(face.to_vec());
        }
        // Diagrams overwhelmingly ask for a generic family or for a face the
        // book does not ship. Point all three generics at what we do have so
        // the request resolves instead of falling through to nothing.
        let latin = db
            .faces()
            .next()
            .and_then(|f| f.families.first().map(|(name, _)| name.clone()));
        if let Some(name) = latin {
            db.set_sans_serif_family(&name);
            db.set_serif_family(&name);
            db.set_cursive_family(&name);
            db.set_fantasy_family(&name);
        }
        Arc::new(db)
    })
}

// --- foreignObject -> <text> preprocessing -----------------------------------
// Mermaid, Excalidraw and similar exporters put every diagram label inside an
// SVG `<foreignObject>` as real HTML (`<div><span class="nodeLabel"><p>text`).
// usvg 0.42 has no foreignObject support, so each one rasterises as a blank
// box and the diagram loses every label. This pass rewrites each foreignObject
// into a native `<text>` centred in its own declared box before the bytes reach
// usvg. Hand-rolled byte scanning, matching is_svg()/extract.rs's style.

const FO_OPEN: &[u8] = b"<foreignobject";
const FO_CLOSE: &[u8] = b"</foreignobject";

/// Substring scan for a lowercased element name with a name-boundary check, so
/// `<foreignObject` matches but a hypothetical `<foreignobjectx>` does not.
fn find_elem(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    for i in 0..=hay.len() - needle.len() {
        if hay[i..i + needle.len()]
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .eq(needle.iter().copied())
        {
            let after = hay.get(i + needle.len()).copied();
            if matches!(
                after,
                Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
            ) {
                return Some(i);
            }
        }
    }
    None
}

fn find_byte(hay: &[u8], b: u8) -> Option<usize> {
    hay.iter().position(|&x| x == b)
}

/// Raw value bytes of an `attr="..."` / `attr='...'` attribute on an opening
/// tag, matched case-insensitively and bounded so `width` does not hit
/// `stroke-width`.
fn attr_bytes<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let n = name.len();
    if tag.len() <= n {
        return None;
    }
    let lower_name: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut i = 0;
    while i + n <= tag.len() {
        let win_matches = tag[i..i + n]
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .eq(lower_name.iter().copied());
        if win_matches {
            let prev_ok =
                i == 0 || matches!(tag[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b'<' | b'/');
            let next = tag.get(i + n).copied();
            let after_ok = matches!(
                next,
                Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'=')
            );
            if prev_ok && after_ok {
                let mut j = i + n;
                while j < tag.len() && tag[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < tag.len() && tag[j] == b'=' {
                    j += 1;
                    while j < tag.len() && tag[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < tag.len() && (tag[j] == b'"' || tag[j] == b'\'') {
                        let quote = tag[j];
                        let start = j + 1;
                        let end = tag[start..].iter().position(|&b| b == quote)?;
                        return Some(&tag[start..start + end]);
                    }
                    let start = j;
                    let end = tag[start..]
                        .iter()
                        .position(|&b| b.is_ascii_whitespace() || b == b'>' || b == b'/')
                        .unwrap_or(tag.len() - start);
                    return Some(&tag[start..start + end]);
                }
            }
        }
        i += 1;
    }
    None
}

/// Leading numeric run of an attribute value, so "106.9px" / "100%" parse.
fn parse_dim(v: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(v).ok()?;
    let num: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect();
    num.parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Decode the handful of entities a label can contain, including `&#39;`. The
/// window is bounded so an unescaped trailing `&` cannot scan the whole doc.
fn decode_entity(bytes: &[u8]) -> Option<(char, usize)> {
    let semi = bytes.iter().take(16).position(|&b| b == b';')?;
    let body = &bytes[1..semi];
    let len = semi + 1;
    match body {
        b"amp" => Some(('&', len)),
        b"lt" => Some(('<', len)),
        b"gt" => Some(('>', len)),
        b"quot" => Some(('"', len)),
        b"apos" => Some(('\'', len)),
        b"nbsp" => Some((' ', len)),
        _ => {
            if body.first() == Some(&b'#') {
                let num = &body[1..];
                let cp =
                    if let Some(rest) = num.strip_prefix(b"x").or_else(|| num.strip_prefix(b"X")) {
                        u32::from_str_radix(std::str::from_utf8(rest).ok()?, 16).ok()?
                    } else {
                        std::str::from_utf8(num).ok()?.parse::<u32>().ok()?
                    };
                Some((char::from_u32(cp)?, len))
            } else {
                None
            }
        }
    }
}

/// Strip HTML tags from inside a foreignObject, decode entities, and collapse
/// whitespace. Each tag becomes a single separator so `</p><p>` does not
/// word-glue. Non-ASCII bytes are copied verbatim so multibyte labels survive.
fn extract_text(inner: &[u8]) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let b = inner[i];
        if b == b'<' {
            if let Some(off) = inner[i..].iter().position(|&x| x == b'>') {
                bytes.push(b' ');
                i += off + 1;
            } else {
                break;
            }
        } else if b == b'&' {
            if let Some((ch, len)) = decode_entity(&inner[i..]) {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                i += len;
            } else {
                bytes.push(b'&');
                i += 1;
            }
        } else {
            bytes.push(b);
            i += 1;
        }
    }
    let s = String::from_utf8_lossy(&bytes);
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn xml_text_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Format an f32 for an SVG attribute: two decimals, trailing zeros stripped,
/// never scientific.
fn fmt_attr(f: f32) -> String {
    if !f.is_finite() {
        return "0".to_string();
    }
    let mut s = format!("{:.2}", f);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Greedy word-wrap of a label against `width` at font size `px`, measuring
/// each candidate line with the same engine that rasterises it. A single word
/// wider than the box takes its own line rather than being hyphenated --
/// consistent with the rest of the codebase, which never hyphenates.
fn wrap_label(text: &str, width: f32, px: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        let fits = super::text_render::word_width(&candidate, px) <= width;
        if fits || cur.is_empty() {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Rewrite every `<foreignObject>` label to a native `<text>` so usvg can
/// rasterise it. The common case (no foreignObject at all) allocates once and
/// copies straight through.
pub(crate) fn inline_foreignobject_text(raw: &[u8]) -> Vec<u8> {
    if find_elem(raw, FO_OPEN).is_none() {
        return raw.to_vec();
    }
    let mut out: Vec<u8> = Vec::with_capacity(raw.len() + 64);
    let mut rest = raw;
    loop {
        match find_elem(rest, FO_OPEN) {
            None => {
                out.extend_from_slice(rest);
                break;
            }
            Some(start) => {
                out.extend_from_slice(&rest[..start]);
                let tag = &rest[start..];
                // Opening tag ends at its first '>'.
                let Some(gt) = find_byte(tag, b'>') else {
                    out.extend_from_slice(tag);
                    break;
                };
                let open_tag = &tag[..=gt];
                let after_open = &tag[gt + 1..];

                // Self-closing: no content to recover.
                if open_tag.ends_with(b"/>") {
                    rest = after_open;
                    continue;
                }

                // foreignObject never nests in practice, so the first close
                // after the open is the matching one.
                let Some(close_rel) = find_elem(after_open, FO_CLOSE) else {
                    out.extend_from_slice(open_tag);
                    out.extend_from_slice(after_open);
                    break;
                };
                let close_tail = &after_open[close_rel..];
                let Some(cgt) = find_byte(close_tail, b'>') else {
                    out.extend_from_slice(open_tag);
                    out.extend_from_slice(after_open);
                    break;
                };
                let inner = &after_open[..close_rel];
                let after_close = &close_tail[cgt + 1..];

                let text = extract_text(inner);
                let w = attr_bytes(open_tag, b"width").and_then(parse_dim);
                let h = attr_bytes(open_tag, b"height").and_then(parse_dim);

                if text.is_empty() {
                    // Edge labels are empty spans; drop cleanly with nothing.
                } else if let (Some(w), Some(h)) = (w, h) {
                    // Wrap to the box and emit one `<tspan>` per line, each with
                    // an explicit x/y (no dy accumulation). Short labels that fit
                    // produce a single tspan, so this is a superset of the old
                    // single-line behaviour.
                    const PX: f32 = 14.0;
                    let lh = super::text_render::line_height(PX) as f32;
                    let lines = wrap_label(&text, w as f32, PX);
                    let total_h = lines.len() as f32 * lh;
                    let start_y = (h as f32 - total_h) / 2.0 + lh * 0.72;
                    let cx = fmt_attr((w / 2.0) as f32);
                    let mut s =
                        format!("<text x=\"{cx}\" text-anchor=\"middle\" font-size=\"14\">");
                    for (i, line) in lines.iter().enumerate() {
                        let y = fmt_attr(start_y + i as f32 * lh);
                        let esc = xml_text_escape(line);
                        s.push_str(&format!("<tspan x=\"{cx}\" y=\"{y}\">{esc}</tspan>"));
                    }
                    s.push_str("</text>");
                    out.extend_from_slice(s.as_bytes());
                } else {
                    // Text present but no box to place it in: leave the span so
                    // behaviour matches today (usvg drops it) rather than risk a
                    // misplaced glyph.
                    out.extend_from_slice(open_tag);
                    out.extend_from_slice(inner);
                    out.extend_from_slice(&close_tail[..=cgt]);
                }
                rest = after_close;
            }
        }
    }
    out
}

/// Rasterise `raw` SVG markup to fit within `max_w` x `max_h`.
///
/// The result is composited onto white. An SVG's background is transparent by
/// default, and a diagram drawn in black strokes on transparency would come out
/// as black-on-black once the RGBA is flattened -- the picture would be present
/// and unreadable, which is harder to diagnose than a missing one.
pub fn rasterize_svg(raw: &[u8], max_w: usize, max_h: usize) -> Option<DecodedImage> {
    if max_w == 0 || max_h == 0 {
        return None;
    }
    let raw = inline_foreignobject_text(raw);
    let mut opt = resvg::usvg::Options {
        fontdb: fontdb().clone(),
        ..Default::default()
    };
    // Relative `font-size`/`width` units resolve against this. The text column
    // is the box the figure is being fitted into, so it is the honest viewport.
    opt.default_size = resvg::usvg::Size::from_wh(max_w as f32, max_h as f32)?;

    let tree = match resvg::usvg::Tree::from_data(&raw, &opt) {
        Ok(t) => t,
        Err(e) => {
            warn!("svg parse failed: {e}");
            return None;
        }
    };

    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite()) || sw <= 0.0 || sh <= 0.0 {
        warn!("svg has no usable size ({sw}x{sh})");
        return None;
    }

    // Content box: the declared canvas unioned with the actual drawn geometry.
    // Strokes and arrowheads can spill past the viewBox; without this a diagram
    // that bleeds is clipped to nothing. Diagrams that already fit are
    // unaffected -- the union collapses to the declared box. The origin shift
    // pulls negative-coordinate content into view.
    let bbox = tree.root().abs_stroke_bounding_box();
    let mut minx = 0.0_f32;
    let mut miny = 0.0_f32;
    let mut maxx = sw;
    let mut maxy = sh;
    if bbox.width() > 0.0 && bbox.height() > 0.0 {
        minx = minx.min(bbox.x());
        miny = miny.min(bbox.y());
        maxx = maxx.max(bbox.x() + bbox.width());
        maxy = maxy.max(bbox.y() + bbox.height());
    }
    let cw = maxx - minx;
    let ch = maxy - miny;
    if !(cw.is_finite() && ch.is_finite()) || cw <= 0.0 || ch <= 0.0 {
        warn!("svg content box is empty ({cw}x{ch})");
        return None;
    }

    // Fit to the column width, then to the height if that overflowed. Vector
    // art is scaled at render time rather than resampled afterwards, so a small
    // diagram enlarged to the column stays sharp.
    let scale = (max_w as f32 / cw).min(max_h as f32 / ch);
    let w = ((cw * scale).round() as usize).clamp(1, max_w);
    let h = ((ch * scale).round() as usize).clamp(1, max_h);
    if w * h > MAX_PIXELS {
        warn!("svg too large to rasterise ({w}x{h})");
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w as u32, h as u32)?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale).pre_translate(-minx, -miny),
        &mut pixmap.as_mut(),
    );

    // Pixmap data is premultiplied RGBA; the fill above made every pixel
    // opaque, so dropping alpha is exact rather than an approximation.
    let mut rgb = Vec::with_capacity(w * h * 3);
    for px in pixmap.pixels() {
        rgb.extend_from_slice(&[px.red(), px.green(), px.blue()]);
    }
    Some(DecodedImage {
        rgb,
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="black"/></svg>"#;

    #[test]
    fn sniffs_plain_and_prologued_svg() {
        assert!(is_svg(SQUARE.as_bytes()));
        assert!(is_svg(
            br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>"#
        ));
        assert!(is_svg(b"\xef\xbb\xbf  \n<svg/>"));
        assert!(is_svg(
            b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x.dtd\">\n<svg/>"
        ));
    }

    #[test]
    fn does_not_sniff_raster_or_html() {
        assert!(!is_svg(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a]));
        assert!(!is_svg(b"\xff\xd8\xff\xe0"), "jpeg");
        assert!(!is_svg(b"<html><body><svg/></body></html>"));
        assert!(!is_svg(b""));
        assert!(!is_svg(b"   "));
    }

    /// The bytes `<svg` can appear anywhere in compressed pixel data. Only the
    /// head of the file is scanned, so that cannot be mistaken for markup.
    #[test]
    fn svg_bytes_deep_inside_a_blob_are_not_markup() {
        let mut blob = vec![0x89, b'P', b'N', b'G'];
        blob.extend(std::iter::repeat(0u8).take(4096));
        blob.extend_from_slice(b"<svg");
        assert!(!is_svg(&blob));
    }

    #[test]
    fn rasterizes_to_the_requested_box() {
        let img = rasterize_svg(SQUARE.as_bytes(), 200, 400).expect("renders");
        assert_eq!(img.width, 200, "scaled to the column width");
        assert_eq!(img.height, 100, "aspect ratio kept");
        assert_eq!(img.rgb.len(), 200 * 100 * 3);
    }

    /// A tall diagram must fit the page height, not just the column width.
    #[test]
    fn height_bound_applies() {
        let tall = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="500"><rect width="50" height="500" fill="black"/></svg>"#;
        let img = rasterize_svg(tall.as_bytes(), 400, 100).expect("renders");
        assert!(img.height <= 100, "height {} exceeds the box", img.height);
        assert!(img.width <= 400);
    }

    /// The regression this compositing exists for: strokes on a transparent
    /// background must not flatten to black-on-black.
    #[test]
    fn transparent_background_becomes_white() {
        let stroke = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><line x1="0" y1="0" x2="10" y2="10" stroke="black"/></svg>"#;
        let img = rasterize_svg(stroke.as_bytes(), 10, 10).expect("renders");
        assert!(
            img.rgb.chunks(3).any(|p| p == [255, 255, 255]),
            "untouched pixels must be white, not transparent-black"
        );
        assert!(
            img.rgb.chunks(3).any(|p| p[0] < 128),
            "the stroke must actually be drawn"
        );
    }

    #[test]
    fn malformed_markup_is_rejected_not_panicked_on() {
        assert!(rasterize_svg(b"<svg", 100, 100).is_none());
        assert!(rasterize_svg(b"not markup at all", 100, 100).is_none());
        assert!(rasterize_svg(SQUARE.as_bytes(), 0, 100).is_none());
    }

    /// Text labels are most of what a flow chart says. With no font database
    /// they render as nothing, and the diagram comes out as empty boxes.
    #[test]
    fn text_labels_render() {
        let labelled = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><text x="4" y="40" font-size="30">Agent</text></svg>"#;
        let img = rasterize_svg(labelled.as_bytes(), 200, 60).expect("renders");
        let inked = img.rgb.chunks(3).filter(|p| p[0] < 200).count();
        assert!(inked > 20, "expected glyph pixels, got {inked}");
    }

    /// The dominant case for Mermaid/Excalidraw diagrams: every label lives in a
    /// `<foreignObject>` as real HTML. usvg drops those, so the label vanishes.
    /// The preprocessing pass must lift the text out into a native `<text>`.
    #[test]
    fn foreignobject_label_is_lifted_to_text() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="60">
  <g class="label" transform="translate(20,18)">
    <rect width="120" height="24" fill="#eee"/>
    <foreignObject width="120" height="24">
      <div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel"><p>You write rules</p></span></div>
    </foreignObject>
  </g>
</svg>"##;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            !s.to_ascii_lowercase().contains("<foreignobject"),
            "foreignObject must be removed, got: {s}"
        );
        // Short label in a 120px box -> a single wrapped line, inside one
        // <tspan> under a centred <text>.
        assert!(
            s.contains(r#"<text x="60" text-anchor="middle" font-size="14">"#),
            "expected a centred <text>, got: {s}"
        );
        assert!(
            s.contains(r#"<tspan x="60""#),
            "expected a <tspan>, got: {s}"
        );
        assert!(
            s.contains("You write rules"),
            "label text missing, got: {s}"
        );
        assert_eq!(
            s.matches("<tspan").count(),
            1,
            "short label should be one line, got: {s}"
        );
    }

    /// That rewrite must actually produce visible ink at render time, not just
    /// well-formed markup. Today (no foreignObject handling) it renders blank.
    #[test]
    fn foreignobject_label_rasterises_as_ink() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="60"><g transform="translate(20,18)"><rect width="120" height="24" fill="#eeeeee"/><foreignObject width="120" height="24"><div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel"><p>You write rules</p></span></div></foreignObject></g></svg>"##;
        let img = rasterize_svg(svg, 160, 60).expect("renders");
        let ink = img.rgb.chunks(3).filter(|p| p[0] < 200).count();
        assert!(ink > 15, "label text should produce glyph ink, got {ink}");
    }

    /// Edge labels in Mermaid are empty `<span class="edgeLabel"></span>` inside
    /// a foreignObject. They must be dropped cleanly: no panic, no leftover
    /// foreignObject, no stray empty `<text>`.
    #[test]
    fn empty_foreignobject_is_dropped() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><foreignObject width="20" height="20"><span class="edgeLabel"></span></foreignObject></svg>"#;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            !s.to_ascii_lowercase().contains("<foreignobject"),
            "empty foreignObject must be removed, got: {s}"
        );
        assert!(!s.contains("<text"), "no replacement text for empty label");
        // Still parses and renders without panicking.
        let img = rasterize_svg(svg, 40, 40);
        assert!(
            img.is_some(),
            "should still render after dropping the label"
        );
    }

    /// The common case -- a plain SVG with no foreignObject -- must pass through
    /// the preprocessing step byte-for-byte (the early-exit path).
    #[test]
    fn no_foreignobject_is_byte_identical() {
        let plain = SQUARE.as_bytes();
        assert_eq!(inline_foreignobject_text(plain), plain);
        let with_image = br#"<svg xmlns="http://www.w3.org/2000/svg" width="9" height="9"><circle cx="4" cy="4" r="3"/></svg>"#;
        assert_eq!(inline_foreignobject_text(with_image), with_image);
    }

    /// Entities decode so a label like "a &amp; b" reads as "a & b", not the raw
    /// markup, and the emitted `<text>` re-escapes it correctly.
    #[test]
    fn label_entities_are_decoded_and_re_escaped() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><foreignObject width="100" height="30"><p>a &amp; b &lt;c&gt; &#39;d&#39;</p></foreignObject></svg>"#;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("a &amp; b &lt;c&gt; 'd'"), "got: {s}");
    }

    /// `raster-images` is on: an SVG `<image>` pointing at an inline base64 PNG
    /// decodes instead of leaving a blank region.
    fn tiny_red_png() -> Vec<u8> {
        let mut img = image::RgbaImage::new(4, 4);
        for px in img.pixels_mut() {
            *px = image::Rgba([200, 30, 30, 255]);
        }
        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("encode png");
        buf.into_inner()
    }

    fn b64(input: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            out.push(T[(b[0] >> 2) as usize] as char);
            out.push(T[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                T[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                T[(b[2] & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn raster_image_inside_svg_decodes() {
        let uri = format!("data:image/png;base64,{}", b64(&tiny_red_png()));
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><image href="{uri}" width="40" height="40"/></svg>"#
        );
        let img = rasterize_svg(svg.as_bytes(), 40, 40).expect("renders");
        let reddish = img
            .rgb
            .chunks(3)
            .filter(|p| p[0] > 120 && p[1] < 90 && p[2] < 90)
            .count();
        assert!(
            reddish > 0,
            "embedded raster should decode, not be blank; reddish={reddish}"
        );
    }

    /// Content drawn outside the declared viewBox (a diagram exporter that
    /// undershoots the box) is pulled into view instead of clipped to nothing.
    #[test]
    fn content_outside_the_viewbox_is_not_clipped() {
        let bleed = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><rect x="-100" y="0" width="30" height="30" fill="black"/></svg>"#;
        let img = rasterize_svg(bleed.as_bytes(), 300, 100).expect("renders");
        let ink = img.rgb.chunks(3).filter(|p| p[0] < 100).count();
        assert!(
            ink > 0,
            "off-canvas content should be pulled into view, not clipped away"
        );
    }

    /// The actual complaint this exists for: a label wider than its box must
    /// wrap onto multiple lines, and the rendered ink must stay inside the box
    /// rather than spilling past the next shape. Box is 60px wide at x=0, so no
    /// dark pixel may land past ~x=60 in the rasterised image.
    #[test]
    fn wide_label_wraps_and_stays_inside_its_box() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="80"><g transform="translate(0,10)"><foreignObject width="60" height="60"><div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel"><p>You write rules now please</p></span></div></foreignObject></g></svg>"##;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.matches("<tspan").count() >= 2,
            "a label wider than its box must wrap to multiple lines, got: {s}"
        );

        let img = rasterize_svg(svg, 200, 80).expect("renders");
        let mut max_x = 0usize;
        for y in 0..img.height {
            for x in 0..img.width {
                let i = (y * img.width + x) * 3;
                if img.rgb[i] < 120 {
                    max_x = max_x.max(x);
                }
            }
        }
        // Box is 60px wide starting at x=0; allow a couple of px of anti-alias.
        assert!(
            max_x <= 64,
            "wrapped label ink should stay inside its 60px box, reached x={max_x}"
        );
    }

    /// A short label that already fits stays on a single line -- regression
    /// guard that wrapping did not fragment trivial labels.
    #[test]
    fn short_label_stays_on_one_line() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40"><foreignObject width="200" height="24"><p>Hi</p></foreignObject></svg>"##;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(
            s.matches("<tspan").count(),
            1,
            "short label should be one line, got: {s}"
        );
        assert!(s.contains("Hi"));
    }

    /// A single word too wide for the box takes its own line and renders
    /// without panicking or looping -- the accepted degradation case.
    #[test]
    fn single_overwide_word_renders_without_panicking() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40"><foreignObject width="30" height="24"><p>supercalifragilisticexpialidocious</p></foreignObject></svg>"##;
        let out = inline_foreignobject_text(svg);
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(
            s.matches("<tspan").count(),
            1,
            "one word is one line even if it overflows, got: {s}"
        );
        assert!(
            s.contains("supercalifragilisticexpialidocious"),
            "the word must survive intact, got: {s}"
        );
        // And it still rasterises rather than erroring out.
        let img = rasterize_svg(svg, 200, 40);
        assert!(img.is_some(), "overwide single word must still render");
    }
}
