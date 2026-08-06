// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
use super::*;

#[test]
fn chapter_from_xhtml_builds_text_and_segments() {
    let ch = Chapter::from_xhtml(0, None, "<h1>Ch</h1><p>Hello world.</p>");
    assert_eq!(ch.segments.len(), 2);
    assert!(ch.text.contains("Hello world."));
    assert!(ch.text.contains("Ch"));
}

#[test]
fn chapter_has_empty_images_by_default() {
    let ch = Chapter::from_xhtml(0, None, "<p>Text</p>");
    assert!(ch.images.is_empty());
    assert!(ch.epub_path.is_empty());
    assert!(ch.chapter_path.is_empty());
}

#[test]
fn normalize_flat_path() {
    assert_eq!(
        normalize_zip_path(Path::new("images/fox.png")),
        "images/fox.png"
    );
}

#[test]
fn normalize_single_parent_dir() {
    assert_eq!(
        normalize_zip_path(Path::new("OEBPS/Text/../Images/x.png")),
        "OEBPS/Images/x.png"
    );
}

#[test]
fn normalize_double_parent_dir() {
    assert_eq!(normalize_zip_path(Path::new("a/b/../../c.png")), "c.png");
}

#[test]
fn normalize_current_dir() {
    assert_eq!(normalize_zip_path(Path::new("./cover.jpeg")), "cover.jpeg");
}

#[test]
fn normalize_mixed_current_and_parent() {
    assert_eq!(normalize_zip_path(Path::new("./a/./b/../c.png")), "a/c.png");
}

#[test]
fn normalize_root_file() {
    assert_eq!(
        normalize_zip_path(Path::new("index-1_1.png")),
        "index-1_1.png"
    );
}

#[test]
fn normalize_excess_parent_dirs_do_not_panic() {
    let result = normalize_zip_path(Path::new("../../x.png"));
    assert_eq!(result, "x.png");
}

/// Build a minimal valid EPUB at `path` with the given chapters.
///
/// Each chapter is `(href, xhtml)`. The `mimetype` entry is stored
/// uncompressed and first, as the EPUB spec requires for `EpubDoc::new`.
fn write_fixture_epub(path: &Path, title: &str, chapters: &[(&str, &str)]) {
    use std::io::{Seek, Write};
    use zip::write::FileOptions;
    use zip::CompressionMethod;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    let mut zip = zip::ZipWriter::new(file);

    zip.start_file(
        "mimetype",
        FileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    let opts = FileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("META-INF/container.xml", opts).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .unwrap();

    let mut manifest = String::new();
    let mut spine = String::new();
    for (href, _) in chapters {
        let id = href.trim_end_matches(".xhtml");
        manifest.push_str(&format!(
            "<item id=\"{id}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push_str(&format!("<itemref idref=\"{id}\"/>"));
    }
    manifest.push_str(
        "<item id=\"cover-img\" href=\"cover.png\" media-type=\"image/png\" properties=\"cover-image\"/>",
    );
    manifest
        .push_str("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>");

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{title}</dc:title>
    <dc:creator>Fixture Author</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="bookid">fixture-001</dc:identifier>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>{manifest}</manifest>
  <spine toc="ncx">{spine}</spine>
</package>"#
    );
    zip.start_file("OEBPS/content.opf", opts).unwrap();
    zip.write_all(opf.as_bytes()).unwrap();

    let mut navpoints = String::new();
    for (i, (href, _)) in chapters.iter().enumerate() {
        navpoints.push_str(&format!(
            "<navPoint id=\"n{i}\" playOrder=\"{i}\"><navLabel><text>Chapter {i}</text></navLabel><content src=\"OEBPS/{href}\"/></navPoint>"
        ));
    }
    let ncx = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="fixture-001"/></head>
  <docTitle><text>{title}</text></docTitle>
  <navMap>{navpoints}</navMap>
</ncx>"#
    );
    zip.start_file("OEBPS/toc.ncx", opts).unwrap();
    zip.write_all(ncx.as_bytes()).unwrap();

    for (href, xhtml) in chapters {
        zip.start_file(format!("OEBPS/{href}"), opts).unwrap();
        zip.write_all(xhtml.as_bytes()).unwrap();
    }

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    zip.start_file("OEBPS/cover.png", opts).unwrap();
    zip.write_all(PNG_1X1).unwrap();

    let mut file = zip.finish().unwrap();
    file.sync_all().unwrap();
    let _ = file.seek(std::io::SeekFrom::Start(0));
}

#[test]
fn epub_open_extracts_metadata_and_chapters() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epub");
    write_fixture_epub(
        &path,
        "Fixture Book",
        &[
            (
                "c1.xhtml",
                "<html><body><h1>One</h1><p>First chapter text.</p></body></html>",
            ),
            (
                "c2.xhtml",
                "<html><body><p>Second chapter here.</p></body></html>",
            ),
        ],
    );
    let book = EpubBook::open(&path).expect("fixture epub must open");
    assert_eq!(book.title.as_deref(), Some("Fixture Book"));
    assert_eq!(book.author.as_deref(), Some("Fixture Author"));
    assert_eq!(book.language.as_deref(), Some("en"));
    assert_eq!(
        book.chapters.len(),
        2,
        "both non-empty chapters must be kept"
    );
    assert!(book.chapters[0].text.contains("First chapter text."));
    assert!(book.chapters[1].text.contains("Second chapter here."));
}

#[test]
fn epub_open_skips_empty_spine_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epub");
    write_fixture_epub(
        &path,
        "With Blanks",
        &[
            ("blank.xhtml", "<html><body></body></html>"),
            (
                "real.xhtml",
                "<html><body><p>Real content.</p></body></html>",
            ),
        ],
    );
    let book = EpubBook::open(&path).unwrap();
    assert_eq!(
        book.chapters.len(),
        1,
        "empty spine item must be skipped, leaving one chapter"
    );
    assert!(book.chapters[0].text.contains("Real content."));
}

#[test]
fn epub_cover_bytes_returns_image() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epub");
    write_fixture_epub(
        &path,
        "Covered",
        &[("c.xhtml", "<html><body><p>Hi</p></body></html>")],
    );
    let cover = EpubBook::cover_bytes(&path).expect("fixture declares a cover image");
    assert!(!cover.is_empty(), "cover bytes must be non-empty");
    assert_eq!(
        &cover[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "cover is a PNG"
    );
}

#[test]
fn build_toc_map_resolves_by_stripped_path() {
    let toc = vec![epub::doc::NavPoint {
        label: "Intro".into(),
        content: std::path::PathBuf::from("OEBPS/intro.xhtml#sec1"),
        children: vec![],
        play_order: None,
    }];
    let map = build_toc_map(&toc);
    assert_eq!(
        map.get("OEBPS/intro.xhtml").map(|s| s.as_str()),
        Some("Intro")
    );
    assert!(
        !map.contains_key("OEBPS/intro.xhtml#sec1"),
        "fragment must be stripped from key"
    );
}

#[test]
fn build_toc_map_flattens_nested_navpoints() {
    let toc = vec![epub::doc::NavPoint {
        label: "Part One".into(),
        content: std::path::PathBuf::from("part1.xhtml"),
        children: vec![epub::doc::NavPoint {
            label: "Chapter A".into(),
            content: std::path::PathBuf::from("chap_a.xhtml"),
            children: vec![],
            play_order: None,
        }],
        play_order: None,
    }];
    let map = build_toc_map(&toc);
    assert_eq!(map.len(), 2, "nested navpoints must be flattened");
    assert!(map.contains_key("part1.xhtml"));
    assert!(map.contains_key("chap_a.xhtml"));
}

#[test]
fn strip_fragment_removes_hash_suffix() {
    assert_eq!(strip_fragment("a/b.xhtml#x"), "a/b.xhtml");
    assert_eq!(strip_fragment("a/b.xhtml"), "a/b.xhtml");
    assert_eq!(strip_fragment("#only"), "");
}

// --- link resolution -------------------------------------------------------

fn linked_chapters() -> Vec<Chapter> {
    let mut a = Chapter::from_xhtml(0, None, r#"<p id="top">one</p>"#);
    a.chapter_path = "OEBPS/Text/ch01.xhtml".into();
    let mut b = Chapter::from_xhtml(1, None, r#"<p>lead</p><p id="mid">two</p>"#);
    b.chapter_path = "OEBPS/Text/ch02.xhtml".into();
    vec![a, b]
}

#[test]
fn resolves_a_sibling_chapter_href() {
    let chs = linked_chapters();
    let t = resolve_link(&chs, 0, "ch02.xhtml").expect("resolves");
    assert_eq!(t.chapter, 1);
    assert_eq!(t.anchor, None);
}

#[test]
fn resolves_an_href_with_a_fragment() {
    let chs = linked_chapters();
    let t = resolve_link(&chs, 0, "ch02.xhtml#mid").expect("resolves");
    assert_eq!(t.chapter, 1);
    assert_eq!(t.anchor.as_deref(), Some("mid"));
    assert!(
        anchor_offset(&chs, &t) > 0,
        "the anchor lands past the chapter top"
    );
}

/// Hrefs are relative to the file that contains them, not the archive root.
#[test]
fn resolves_a_dot_dot_href() {
    let mut chs = linked_chapters();
    chs[1].chapter_path = "OEBPS/Other/ch02.xhtml".into();
    let t = resolve_link(&chs, 0, "../Other/ch02.xhtml").expect("resolves");
    assert_eq!(t.chapter, 1);
}

#[test]
fn a_bare_fragment_stays_in_the_current_chapter() {
    let chs = linked_chapters();
    let t = resolve_link(&chs, 1, "#mid").expect("resolves");
    assert_eq!(t.chapter, 1);
    assert_eq!(t.anchor.as_deref(), Some("mid"));
}

#[test]
fn external_hrefs_do_not_resolve() {
    let chs = linked_chapters();
    for href in [
        "https://example.com",
        "http://example.com",
        "mailto:a@b.c",
        "tel:+1",
    ] {
        assert!(resolve_link(&chs, 0, href).is_none(), "{href} is external");
    }
}

/// A nav document or image the spine never included has no chapter to open.
#[test]
fn href_outside_the_spine_does_not_resolve() {
    let chs = linked_chapters();
    assert!(resolve_link(&chs, 0, "nav.xhtml").is_none());
}

#[test]
fn percent_escapes_in_hrefs_are_decoded() {
    let mut chs = linked_chapters();
    chs[1].chapter_path = "OEBPS/Text/my chapter.xhtml".into();
    let t = resolve_link(&chs, 0, "my%20chapter.xhtml").expect("resolves");
    assert_eq!(t.chapter, 1);
}

#[test]
fn an_unknown_anchor_falls_back_to_the_chapter_top() {
    let chs = linked_chapters();
    let t = resolve_link(&chs, 0, "ch02.xhtml#nope").expect("resolves");
    assert_eq!(anchor_offset(&chs, &t), 0);
}

// --- build_toc_tree (issue 11) ---------------------------------------------

fn nav(label: &str, content: &str, children: Vec<epub::doc::NavPoint>) -> epub::doc::NavPoint {
    epub::doc::NavPoint {
        label: label.to_string(),
        content: std::path::PathBuf::from(content),
        children,
        play_order: None,
    }
}

/// Two nav points into the same chapter file (different anchors) used to
/// collapse to one row through a `HashMap` keyed by stripped path. The tree
/// keeps them as separate sibling entries.
#[test]
fn multiple_entries_into_the_same_chapter_stay_separate() {
    let chs = linked_chapters();
    let toc = vec![
        nav("Section One", "OEBPS/Text/ch02.xhtml#s1", vec![]),
        nav("Section Two", "OEBPS/Text/ch02.xhtml#s2", vec![]),
    ];
    let tree = build_toc_tree(&toc, &chs);
    assert_eq!(tree.len(), 2, "{tree:#?}");
    assert_eq!(tree[0].chapter, Some(1));
    assert_eq!(tree[1].chapter, Some(1));
    assert_ne!(tree[0].anchor, tree[1].anchor);
}

#[test]
fn anchor_is_captured_and_stripped_from_the_path() {
    let chs = linked_chapters();
    let toc = vec![nav("Mid", "OEBPS/Text/ch02.xhtml#mid", vec![])];
    let tree = build_toc_tree(&toc, &chs);
    assert_eq!(tree[0].chapter, Some(1));
    assert_eq!(tree[0].anchor.as_deref(), Some("mid"));
}

#[test]
fn entry_with_no_fragment_has_no_anchor() {
    let chs = linked_chapters();
    let toc = vec![nav("Ch1", "OEBPS/Text/ch01.xhtml", vec![])];
    let tree = build_toc_tree(&toc, &chs);
    assert_eq!(tree[0].chapter, Some(0));
    assert_eq!(tree[0].anchor, None);
}

#[test]
fn depth_is_correct_for_a_three_level_nav() {
    let chs = linked_chapters();
    let toc = vec![nav(
        "Part One",
        "OEBPS/Text/ch01.xhtml",
        vec![nav(
            "Chapter Two",
            "OEBPS/Text/ch02.xhtml",
            vec![nav("Section Two-A", "OEBPS/Text/ch02.xhtml#a", vec![])],
        )],
    )];
    let tree = build_toc_tree(&toc, &chs);
    assert_eq!(tree[0].depth, 0);
    assert_eq!(tree[0].children[0].depth, 1);
    assert_eq!(tree[0].children[0].children[0].depth, 2);
}

/// A divider entry (a nav-only "Part" heading that names no spine chapter)
/// used to drop its whole subtree, including children that *do* resolve.
#[test]
fn a_non_resolving_parent_keeps_its_resolving_children() {
    let chs = linked_chapters();
    let toc = vec![nav(
        "Part One",
        "OEBPS/Text/part1.xhtml",
        vec![nav("Chapter One", "OEBPS/Text/ch01.xhtml", vec![])],
    )];
    let tree = build_toc_tree(&toc, &chs);
    assert_eq!(
        tree.len(),
        1,
        "the divider node itself must survive: {tree:#?}"
    );
    assert_eq!(tree[0].chapter, None, "divider names no real chapter");
    assert_eq!(
        tree[0].children.len(),
        1,
        "its resolving child must survive"
    );
    assert_eq!(tree[0].children[0].chapter, Some(0));
}

/// A node with neither a resolved chapter nor any surviving children -- a nav
/// entry pointing entirely outside the spine -- is dropped, not kept as dead
/// weight in the tree.
#[test]
fn a_node_resolving_to_nothing_at_all_is_dropped() {
    let chs = linked_chapters();
    let toc = vec![nav("Ghost", "OEBPS/Text/does-not-exist.xhtml", vec![])];
    let tree = build_toc_tree(&toc, &chs);
    assert!(tree.is_empty(), "{tree:#?}");
}

#[test]
fn empty_nav_produces_an_empty_tree() {
    let chs = linked_chapters();
    let tree = build_toc_tree(&[], &chs);
    assert!(tree.is_empty());
}
