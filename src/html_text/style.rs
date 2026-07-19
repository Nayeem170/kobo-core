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

use std::collections::HashMap;

/// class name (without the leading `.`) -> left indent in `em`.
pub type IndentMap = HashMap<String, f32>;

/// Blocks indented further than this are almost certainly a stylesheet quirk,
/// not real nesting; clamping keeps them from eating the whole text column.
pub const MAX_INDENT_EM: f32 = 12.0;

/// Assumed root font size, for stylesheets that specify indents in px.
const PX_PER_EM: f32 = 16.0;

/// Parse every `.class { ... }` rule in `css` into a class -> indent-em map.
/// Rules with no left indent, or a zero/negative one, are omitted.
pub fn parse_indents(css: &str) -> IndentMap {
    let mut map = IndentMap::new();
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
            Some(v) if v > 0.0 => v.min(MAX_INDENT_EM),
            _ => continue,
        };
        for sel in selectors.split(',') {
            let sel = sel.trim();
            // Only bare `.class` selectors -- anything compound is beyond what
            // this scan can resolve without a real cascade.
            if let Some(name) = sel.strip_prefix('.') {
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    map.insert(name.to_string(), em);
                }
            }
        }
    }
    map
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
}
