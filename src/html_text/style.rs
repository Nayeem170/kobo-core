// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Minimal CSS scan: class name -> block left indent, in `em`.
//!
//! Calibre-converted technical books do not use `<pre>` for code. They emit a
//! plain `<p class="calibreN">` per source line and encode the nesting depth as
//! `margin-left` on that class (1em per level). Dropping it turns Python into a
//! flat wall of text, so the indent has to survive extraction.
//!
//! This is not a CSS engine: it reads top-level rule blocks, keeps the
//! `margin-left` (or the 4-value `margin` shorthand's left component), and
//! ignores everything else. Cascade, specificity, and media queries do not
//! matter here -- Calibre writes one flat class per indent level.

use std::collections::{HashMap, HashSet};

/// class name (without the leading `.`) -> left indent in `em`.
pub type IndentMap = HashMap<String, f32>;

/// Class names whose rule sets `list-style` / `list-style-type` to `none`.
pub type NoMarkerSet = HashSet<String>;

/// Everything the extractor reads out of the book's stylesheets.
///
/// One struct rather than a widening argument list: the extractor already
/// threads the indent map through four call levels, and every future style the
/// scan learns to read would otherwise add another parameter to each of them.
#[derive(Debug, Clone, Default)]
pub struct BookStyle {
    pub indents: IndentMap,
    /// Lists carrying one of these classes render without bullets or numbers.
    pub no_marker: NoMarkerSet,
}

impl BookStyle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an indent map alone (the pre-`BookStyle` call shape).
    pub fn from_indents(indents: IndentMap) -> Self {
        Self {
            indents,
            no_marker: NoMarkerSet::new(),
        }
    }

    /// Merge another sheet's rules in. Later sheets win on collision, matching
    /// the order the cascade would apply them.
    pub fn extend(&mut self, other: BookStyle) {
        self.indents.extend(other.indents);
        self.no_marker.extend(other.no_marker);
    }
}

/// Blocks indented further than this are almost certainly a stylesheet quirk,
/// not real nesting; clamping keeps them from eating the whole text column.
pub const MAX_INDENT_EM: f32 = 12.0;

/// Assumed root font size, for stylesheets that specify indents in px.
const PX_PER_EM: f32 = 16.0;

/// Parse every `.class { ... }` rule in `css` into a class -> indent-em map.
/// Rules with no left indent, or a zero/negative one, are omitted.
pub fn parse_indents(css: &str) -> IndentMap {
    parse_book_style(css).indents
}

/// Parse `css` into every style the extractor understands, in one pass over the
/// rule blocks.
pub fn parse_book_style(css: &str) -> BookStyle {
    let mut out = BookStyle::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let selectors = &rest[..open];
        let after = &rest[open + 1..];
        let close = match after.find('}') {
            Some(c) => c,
            None => break,
        };
        let body = &after[..close];
        rest = &after[close + 1..];
        // `@media` / `@font-face` wrappers: skip the block, not the file.
        if selectors.contains('@') {
            continue;
        }
        let em = match left_indent_em(body) {
            Some(v) if v > 0.0 => Some(v.min(MAX_INDENT_EM)),
            _ => None,
        };
        let no_marker = list_style_is_none(body);
        if em.is_none() && !no_marker {
            continue;
        }
        for name in class_names(selectors) {
            if let Some(v) = em {
                out.indents.insert(name.clone(), v);
            }
            if no_marker {
                out.no_marker.insert(name);
            }
        }
    }
    out
}

/// Bare `.class` selectors in a selector list. Anything compound is beyond what
/// this scan can resolve without a real cascade, and is skipped.
fn class_names(selectors: &str) -> Vec<String> {
    let mut out = Vec::new();
    for sel in selectors.split(',') {
        let sel = sel.trim();
        if let Some(name) = sel.strip_prefix('.') {
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Does a rule body suppress the list marker?
///
/// Both the `list-style-type` longhand and the `list-style` shorthand are read.
/// The shorthand also carries position and image, so `none` is matched as a
/// whole word rather than a substring -- `list-style: none inside` suppresses
/// the marker, `list-style: square` does not.
fn list_style_is_none(body: &str) -> bool {
    for decl in body.split(';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let is_list_style = prop == "list-style-type" || prop == "list-style";
        if is_list_style
            && value.split_whitespace().any(|w| {
                w.trim_end_matches(&[',', '!'][..])
                    .eq_ignore_ascii_case("none")
            })
        {
            return true;
        }
    }
    false
}

/// `list-style` declared inline on an element, e.g.
/// `style="list-style-type: none"`.
pub fn inline_list_style_none(style_attr: &str) -> bool {
    list_style_is_none(style_attr)
}

/// Left indent declared by a rule body, preferring the longhand.
fn left_indent_em(body: &str) -> Option<f32> {
    for decl in body.split(';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        if prop.trim().eq_ignore_ascii_case("margin-left")
            || prop.trim().eq_ignore_ascii_case("padding-left")
        {
            return parse_len_em(value.trim());
        }
    }
    // 4-value `margin: top right bottom left` shorthand.
    for decl in body.split(';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        if prop.trim().eq_ignore_ascii_case("margin") {
            let parts: Vec<&str> = value.split_whitespace().collect();
            return match parts.len() {
                4 => parse_len_em(parts[3]),
                // `margin: v h` and `margin: t h b` both put the horizontal
                // value second.
                2 | 3 => parse_len_em(parts[1]),
                _ => None,
            };
        }
    }
    None
}

/// A CSS length in `em`, `rem`, `px`, or `pt`. Anything else (`%`, `auto`,
/// keywords) is not convertible without layout context, so it is dropped.
fn parse_len_em(s: &str) -> Option<f32> {
    let s = s.trim();
    let (num, unit) = s.split_at(
        s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
            .unwrap_or(s.len()),
    );
    let v: f32 = num.parse().ok()?;
    match unit.trim().to_ascii_lowercase().as_str() {
        "em" | "rem" => Some(v),
        "px" => Some(v / PX_PER_EM),
        "pt" => Some(v * 4.0 / 3.0 / PX_PER_EM),
        _ => None,
    }
}

/// Indent declared inline on an element, e.g. `style="margin-left: 2em"`.
pub fn inline_indent_em(style_attr: &str) -> Option<f32> {
    left_indent_em(style_attr).map(|v| v.clamp(0.0, MAX_INDENT_EM))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_margin_left_longhand() {
        let m = parse_indents(".calibre7 { display: block; margin-left: 2em; }");
        assert_eq!(m.get("calibre7"), Some(&2.0));
    }

    #[test]
    fn reads_four_value_margin_shorthand() {
        let m = parse_indents(".c { margin: 0 0 0 3em; }");
        assert_eq!(m.get("c"), Some(&3.0));
    }

    #[test]
    fn shares_one_rule_across_a_selector_list() {
        let m = parse_indents(".a, .b { margin-left: 1em }");
        assert_eq!(m.get("a"), Some(&1.0));
        assert_eq!(m.get("b"), Some(&1.0));
    }

    #[test]
    fn skips_zero_and_unitless_and_percent() {
        let m =
            parse_indents(".z { margin-left: 0 } .p { margin-left: 5% } .n { margin-left: -2em }");
        assert!(m.is_empty());
    }

    #[test]
    fn converts_px_to_em() {
        let m = parse_indents(".c { margin-left: 32px; }");
        assert_eq!(m.get("c"), Some(&2.0));
    }

    #[test]
    fn clamps_absurd_indents() {
        let m = parse_indents(".c { margin-left: 99em; }");
        assert_eq!(m.get("c"), Some(&MAX_INDENT_EM));
    }

    #[test]
    fn ignores_at_rules_but_keeps_parsing_after_them() {
        let m = parse_indents("@font-face { src: url(x) } .c { margin-left: 4em }");
        assert_eq!(m.get("c"), Some(&4.0));
    }

    #[test]
    fn ignores_compound_selectors() {
        let m = parse_indents("div.c p { margin-left: 4em }");
        assert!(m.is_empty());
    }

    #[test]
    fn reads_list_style_type_none() {
        let s = parse_book_style(".toc { list-style-type: none; }");
        assert!(s.no_marker.contains("toc"));
    }

    #[test]
    fn reads_list_style_shorthand_none() {
        let s = parse_book_style(".nav { list-style: none inside; }");
        assert!(s.no_marker.contains("nav"));
    }

    #[test]
    fn keeps_marker_for_other_list_styles() {
        let s = parse_book_style(".a { list-style-type: square } .b { list-style: decimal }");
        assert!(s.no_marker.is_empty());
    }

    #[test]
    fn one_rule_can_carry_both_indent_and_no_marker() {
        let s = parse_book_style(".toc { margin-left: 2em; list-style: none }");
        assert_eq!(s.indents.get("toc"), Some(&2.0));
        assert!(s.no_marker.contains("toc"));
    }

    #[test]
    fn inline_list_style_none_detected() {
        assert!(inline_list_style_none("list-style: none"));
        assert!(!inline_list_style_none("list-style: disc"));
    }
}
