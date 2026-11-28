//! Help catalogs, `translate`, and the packed file (design sections 3.1 to 3.3).

use diacraft_help::catalog::{Catalog, bundle_range, pack, segments, translate, unpack};
use diacraft_help::{Block, Code, MAX_CATALOG_BYTES, MAX_CATALOG_ROWS, MAX_ROW_BYTES, Span, Target, Topic, parse};

const HEADER: &str = "---\nid: sample\ntitle: Snap and glue\nsummary: Make shapes stick.\nkeywords: snap, glue, magnet\nsite: no\n---\n";

fn topic(body: &str) -> Topic {
    let parsed = parse(&format!("{HEADER}{body}")).unwrap();
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    parsed.topic
}

/// A catalog from rows written as (context, source, translation).
fn catalog(rows: &[(&str, &str, &str)]) -> Catalog {
    let text: String = rows.iter().map(|(c, s, t)| format!("{c}\t{s}\t{t}\n")).collect();
    let (catalog, problems) = Catalog::parse(&text, false);
    assert!(problems.is_empty(), "{problems:?}");
    catalog
}

fn codes(problems: &[diacraft_help::Problem]) -> Vec<(usize, Code)> {
    problems.iter().map(|p| (p.line, p.code)).collect()
}

#[test]
fn a_catalog_has_rows_of_context_source_and_translation() {
    let text = "# a header\n\ntitle\tSnap and glue\tEinrasten und Kleben\npara\tLine one\\nLine two\tZeile eins\\nZeile zwei\n";
    let (catalog, problems) = Catalog::parse(text, false);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.get("title", "Snap and glue"), Some("Einrasten und Kleben"));
    assert_eq!(catalog.get("para", "Line one\nLine two"), Some("Zeile eins\nZeile zwei"));
    assert_eq!(catalog.get("heading", "Snap and glue"), None, "a row is for its context");
    assert_eq!(catalog.get("title", "Other"), None);
}

#[test]
fn a_byte_order_mark_and_carriage_returns_are_read_over() {
    let (catalog, problems) = Catalog::parse("\u{feff}title\tA\tB\r\npara\tC\tD\r\n", false);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!((catalog.get("title", "A"), catalog.get("para", "C")), (Some("B"), Some("D")));
}

#[test]
fn a_row_that_breaks_a_rule_is_skipped_with_its_line() {
    let text = "title\tA\tB\nonly two\tcolumns\nbogus\tA\tB\ntitle\tA\tAgain\npara\t\tempty source\npara\tempty translation\t\nalt\tA\u{1}B\tx\nalt\tfine\tx\u{202e}y\nalt\tfine\tx\u{2066}y\nnote\tbad \\q\tx\nnote\tfine\tfine\n";
    let (catalog, problems) = Catalog::parse(text, false);
    assert_eq!(
        codes(&problems),
        [
            (2, Code::RowColumns),
            (3, Code::CatalogContext),
            (4, Code::RowDuplicate),
            (5, Code::CatalogEmpty),
            (6, Code::CatalogEmpty),
            (7, Code::RowControl),
            (8, Code::CatalogBidi),
            (9, Code::CatalogBidi),
            (10, Code::RowEscape),
        ]
    );
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.get("title", "A"), Some("B"), "of two rows with one key the first is kept");
}

#[test]
fn direction_marks_are_for_a_right_to_left_language_only() {
    let row = "title\tA\tx\u{200f}y\n";
    let (ltr, problems) = Catalog::parse(row, false);
    assert_eq!(codes(&problems), [(1, Code::CatalogBidi)]);
    assert!(ltr.is_empty());
    let (rtl, problems) = Catalog::parse(row, true);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(rtl.len(), 1);
    // an embedding or override is refused in any language
    let (_, problems) = Catalog::parse("title\tA\tx\u{202b}y\n", true);
    assert_eq!(codes(&problems), [(1, Code::CatalogBidi)]);
}

#[test]
fn a_catalog_over_2_mib_is_refused_and_a_row_over_16_kib_is_skipped_and_rows_past_20000_are_dropped() {
    let big = format!("title\tA\t{}\n", "x".repeat(MAX_CATALOG_BYTES));
    let (catalog, problems) = Catalog::parse(&big, false);
    assert!(catalog.is_empty());
    assert_eq!(codes(&problems), [(0, Code::CatalogTooLarge)]);

    let long = format!("title\tA\t{}\npara\tB\tC\n", "x".repeat(MAX_ROW_BYTES));
    let (catalog, problems) = Catalog::parse(&long, false);
    assert_eq!(codes(&problems), [(1, Code::RowTooLong)]);
    assert_eq!(catalog.len(), 1);

    let mut many = String::new();
    for n in 0..MAX_CATALOG_ROWS + 5 {
        many.push_str(&format!("para\tsource {n}\ttranslation {n}\n"));
    }
    let (catalog, problems) = Catalog::parse(&many, false);
    assert_eq!(catalog.len(), MAX_CATALOG_ROWS);
    assert_eq!(codes(&problems), [(MAX_CATALOG_ROWS + 1, Code::TooManyRows)]);
}

#[test]
fn the_segments_of_a_topic_are_its_header_and_the_text_of_each_block_but_not_its_code() {
    let t = topic(
        "## Using snap {#using}\n\nPlain **text**.\n\n- item one\n  - nested\n\n> a note\n\n| head | other |\n|---|---|\n| cell | x |\n\n```sh\nnot translated\n```\n\n![Alt text](site:p)\n",
    );
    let got: Vec<(&str, String, usize)> = segments(&t).into_iter().map(|s| (s.context, s.source, s.line)).collect();
    assert_eq!(
        got,
        [
            ("title", "Snap and glue".to_owned(), 3),
            ("summary", "Make shapes stick.".to_owned(), 4),
            ("keywords", "snap, glue, magnet".to_owned(), 5),
            ("heading", "Using snap".to_owned(), 8),
            ("para", "Plain **text**.".to_owned(), 10),
            ("item", "item one".to_owned(), 12),
            ("item", "nested".to_owned(), 13),
            ("note", "a note".to_owned(), 15),
            ("cell", "head".to_owned(), 17),
            ("cell", "other".to_owned(), 17),
            ("cell", "cell".to_owned(), 19),
            ("cell", "x".to_owned(), 19),
            ("alt", "Alt text".to_owned(), 25),
        ]
    );
}

#[test]
fn a_translated_topic_has_the_blocks_of_the_english_and_the_text_of_the_catalog() {
    let t = topic("## Using snap\n\nTurn on [Snap](ctl:SnapAndGlueDialog) with **care**, see `Alt+F9`.\n\n- first\n- second\n");
    let de = catalog(&[
        ("title", "Snap and glue", "Einrasten und Kleben"),
        ("keywords", "snap, glue, magnet", "einrasten, kleben, magnet"),
        ("heading", "Using snap", "Einrasten benutzen"),
        (
            "para",
            "Turn on [Snap](ctl:SnapAndGlueDialog) with **care**, see `Alt+F9`.",
            "Schalte [Einrasten](ctl:SnapAndGlueDialog) mit **Sorgfalt** ein, siehe `Alt+F9`.",
        ),
        ("item", "first", "erstens"),
    ]);
    let done = translate(&t, &de);
    assert!(done.skipped.is_empty(), "{:?}", done.skipped);
    let tt = &done.topic;
    assert_eq!(tt.header.title, "Einrasten und Kleben");
    assert_eq!(tt.header.keywords, ["einrasten", "kleben", "magnet"]);
    assert_eq!(tt.header.summary, "Make shapes stick.", "a segment with no row stays English");
    assert_eq!(tt.header.id, "sample");
    match &tt.doc.blocks[0] {
        Block::Heading { text, anchor, .. } => {
            assert_eq!((text.source.as_str(), anchor.as_str()), ("Einrasten benutzen", "using-snap"), "the anchor is the English one")
        }
        other => panic!("{other:?}"),
    }
    match &tt.doc.blocks[1] {
        Block::Para(text) => {
            assert!(text.source.starts_with("Schalte "));
            assert!(matches!(&text.spans[1], Span::Link { target: Target::Control(id), .. } if id == "SnapAndGlueDialog"));
        }
        other => panic!("{other:?}"),
    }
    match &tt.doc.blocks[2] {
        Block::List(list) => assert_eq!((list.items[0].text.source.as_str(), list.items[1].text.source.as_str()), ("erstens", "second")),
        other => panic!("{other:?}"),
    }
    assert_eq!((done.translated, done.missing), (5, 2));
}

#[test]
fn a_translation_may_move_its_links_but_not_change_where_they_go() {
    let t = topic("See [one](topic:a) and [two](topic:b).\n");
    let swapped = catalog(&[("para", "See [one](topic:a) and [two](topic:b).", "Siehe [zwei](topic:b) und [eins](topic:a).")]);
    let done = translate(&t, &swapped);
    assert!(done.skipped.is_empty(), "{:?}", done.skipped);
    assert_eq!(done.translated, 1);

    let changed = catalog(&[("para", "See [one](topic:a) and [two](topic:b).", "Siehe [eins](topic:a) und [zwei](topic:c).")]);
    let done = translate(&t, &changed);
    assert_eq!(done.skipped.len(), 1);
    assert_eq!(done.skipped[0].1.code, Code::TranslationTargets);
    assert_eq!(done.translated, 0);
    match &done.topic.doc.blocks[0] {
        Block::Para(text) => assert_eq!(text.source, "See [one](topic:a) and [two](topic:b).", "the English is shown"),
        other => panic!("{other:?}"),
    }

    let lost = catalog(&[("para", "See [one](topic:a) and [two](topic:b).", "Siehe eins und [zwei](topic:b).")]);
    assert_eq!(translate(&t, &lost).skipped[0].1.code, Code::TranslationTargets);
    let key_t = topic("Press [x](key:Cmd+Z).\n");
    let key_changed = catalog(&[("para", "Press [x](key:Cmd+Z).", "Drücke [x](key:Cmd+Y).")]);
    assert_eq!(translate(&key_t, &key_changed).skipped[0].1.code, Code::TranslationTargets);
}

#[test]
fn a_translation_keeps_its_code_spans_verbatim_and_is_valid_help_markdown() {
    let t = topic("Run `diacraft-cli run` now, in **bold**.\n");
    let src = "Run `diacraft-cli run` now, in **bold**.";
    let code = catalog(&[("para", src, "Starte `diacraft-cli starten` jetzt, in **fett**.")]);
    assert_eq!(translate(&t, &code).skipped[0].1.code, Code::TranslationCode);
    let unbalanced = catalog(&[("para", src, "Starte `diacraft-cli run` jetzt, in **fett.")]);
    assert_eq!(translate(&t, &unbalanced).skipped[0].1.code, Code::TranslationMarkup);
    let html = catalog(&[("para", src, "Starte `diacraft-cli run` <b>jetzt</b>, in **fett**.")]);
    assert_eq!(translate(&t, &html).skipped[0].1.code, Code::TranslationMarkup);
    let fine = catalog(&[("para", src, "Starte `diacraft-cli run` jetzt, in **fett**.")]);
    assert!(translate(&t, &fine).skipped.is_empty());
}

#[test]
fn keywords_are_translated_as_words_and_the_other_header_fields_as_text() {
    let t = topic("Text.\n");
    let marks = catalog(&[("keywords", "snap, glue, magnet", "*einrasten*, [kleben]")]);
    let done = translate(&t, &marks);
    assert_eq!(done.skipped[0].1.code, Code::TranslationWords);
    assert_eq!(done.topic.header.keywords, ["snap", "glue", "magnet"]);
    let empty = catalog(&[("keywords", "snap, glue, magnet", " , ")]);
    assert_eq!(translate(&t, &empty).skipped[0].1.code, Code::TranslationWords);
}

#[test]
fn a_picture_alt_and_a_table_are_translated_cell_by_cell() {
    let t = topic("![The dialog](site:dialog)\n\n| Key | Does |\n|---|---|\n| F1 | Help |\n");
    let c = catalog(&[("alt", "The dialog", "Der Dialog"), ("cell", "Key", "Taste"), ("cell", "Help", "Hilfe")]);
    let done = translate(&t, &c);
    assert_eq!(done.topic.doc.blocks[0], Block::Picture { alt: "Der Dialog".to_owned(), name: "dialog".to_owned(), line: 8 });
    match &done.topic.doc.blocks[1] {
        Block::Table { header, rows } => {
            assert_eq!((header[0].source.as_str(), header[1].source.as_str()), ("Taste", "Does"));
            assert_eq!((rows[0][0].source.as_str(), rows[0][1].source.as_str()), ("F1", "Hilfe"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn translating_with_an_empty_catalog_changes_nothing() {
    let t = topic("## H\n\nText [x](topic:a).\n\n- a\n");
    let done = translate(&t, &Catalog::default());
    assert_eq!(done.topic.doc, t.doc);
    assert_eq!(done.topic.header, t.header);
    assert_eq!((done.translated, done.skipped.len()), (0, 0));
}

#[test]
fn bundle_range_is_the_range_when_it_lies_inside_the_total() {
    assert_eq!(bundle_range(0, 0, 0), Some(0..0));
    assert_eq!(bundle_range(5, 5, 10), Some(5..10));
    assert_eq!(bundle_range(0, 10, 10), Some(0..10));
    assert_eq!(bundle_range(5, 6, 10), None);
    assert_eq!(bundle_range(11, 0, 10), None);
    assert_eq!(bundle_range(usize::MAX, 1, 10), None);
    assert_eq!(bundle_range(1, usize::MAX, 10), None);
    assert_eq!(bundle_range(usize::MAX, usize::MAX, usize::MAX), None);
    assert_eq!(bundle_range(10, 0, 10), Some(10..10));
}

#[test]
fn topics_pack_into_one_file_and_come_back_whole() {
    let topics = [
        ("home", "---\nid: home\n---\nHello.\n"),
        ("other", "---\n\n---\nA line of three dashes\n---\ninside a topic.\n"),
        ("empty", ""),
        ("uni", "日本語 é 🙂"),
    ];
    let packed = pack(&topics);
    let back = unpack(&packed).unwrap();
    assert_eq!(back.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), topics);
}

#[test]
fn a_packed_file_that_lies_about_itself_is_refused_in_words() {
    for bad in [
        "",
        "not a bundle",
        "help-bundle 2\n0\n\n",
        "help-bundle 1\n1\nid 99\n\nshort",
        "help-bundle 1\n2\nid 5\n\nabc",
        "help-bundle 1\n1\nid -1\n\n",
        "help-bundle 1\n1\nid 18446744073709551615\n\nx",
        "help-bundle 1\n1\nid\n\nx",
        "help-bundle 1\n99999999\nid 1\n\nx",
        "help-bundle 1\n1\nid 1\nx",
    ] {
        let e = unpack(bad).unwrap_err();
        assert_eq!(e.code, Code::PackFormat, "{bad:?}: {e}");
    }
    // a length that cuts a character in two
    let cut = "help-bundle 1\n1\nid 1\n\n\u{e9}";
    assert_eq!(unpack(cut).unwrap_err().code, Code::PackFormat);
    // text after the last topic
    assert_eq!(unpack("help-bundle 1\n1\nid 1\n\nxy").unwrap_err().code, Code::PackFormat);
}
