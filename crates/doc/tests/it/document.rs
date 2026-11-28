//! The stub document: pages with ids and names, and the rules that keep it valid.

use std::sync::Arc;

use diacraft_doc::{DocError, Document, MAX_NAME_CHARS, MAX_PAGE_NAME_CHARS, MAX_PAGES, Page, TextProblem, check_given_page_name, check_text};

#[test]
fn a_blank_document_has_one_named_page_and_is_valid() {
    let doc = Document::new_blank();
    assert_eq!(doc.version, Document::CURRENT_VERSION);
    assert_eq!(&*doc.title, "Untitled");
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.page(1).map(|p| &*p.name), Some("Page-1"));
    assert_eq!(doc.validate(), Ok(()));
}

#[test]
fn page_finds_by_id_and_nothing_else() {
    let mut doc = Document::new_blank();
    let second = doc.add_page(Some("Second")).unwrap();
    assert_eq!(doc.page(second).map(|p| &*p.name), Some("Second"));
    assert_eq!(doc.page(0), None);
    assert_eq!(doc.page(second + 1), None);
    assert_eq!(doc.page(u32::MAX), None);
}

#[test]
fn add_page_numbers_after_the_highest_id_and_names_it_by_that_id() {
    let mut doc = Document::new_blank();
    assert_eq!(doc.add_page(None), Ok(2));
    assert_eq!(doc.page(2).map(|p| &*p.name), Some("Page-2"));
    // The id follows the highest one, not the count: a gap is not refilled.
    doc.pages = vec![Arc::new(Page::new(7, "Seven"))].into_iter().collect();
    assert_eq!(doc.add_page(Some("Eight")), Ok(8));
    assert_eq!(doc.pages.iter().map(|p| p.id).collect::<Vec<_>>(), [7, 8]);
}

#[test]
fn add_page_at_the_last_id_is_an_error_and_changes_nothing() {
    let mut doc = Document::new_blank();
    doc.pages = vec![Arc::new(Page::new(u32::MAX, "Last"))].into_iter().collect();
    let before = doc.clone();
    assert_eq!(doc.add_page(Some("More")), Err(DocError::IdsExhausted));
    assert_eq!(doc, before);
}

#[test]
fn add_page_stops_at_the_page_limit_and_changes_nothing() {
    let mut doc = Document::new_blank();
    while doc.pages.len() < MAX_PAGES {
        doc.add_page(None).unwrap();
    }
    assert_eq!(doc.validate(), Ok(()));
    let before = doc.clone();
    assert_eq!(doc.add_page(None), Err(DocError::TooManyPages));
    assert_eq!(doc, before);
}

#[test]
fn rename_page_changes_only_that_page() {
    let mut doc = Document::new_blank();
    let second = doc.add_page(None).unwrap();
    let first_before = Arc::clone(doc.pages.get(0).unwrap());
    doc.rename_page(second, "Renamed").unwrap();
    assert_eq!(doc.page(second).map(|p| &*p.name), Some("Renamed"));
    assert!(Arc::ptr_eq(doc.pages.get(0).unwrap(), &first_before), "the untouched page is shared, not copied");
    assert_eq!(doc.rename_page(99, "Nope"), Err(DocError::NoSuchPage(99)));
}

#[test]
fn names_that_are_empty_blank_long_or_hold_control_characters_are_refused() {
    let mut doc = Document::new_blank();
    let before = doc.clone();
    let long = "x".repeat(MAX_NAME_CHARS + 1);
    for (name, error) in [
        ("", DocError::EmptyName),
        (" \t ", DocError::EmptyName),
        ("\u{a0}", DocError::EmptyName),
        (long.as_str(), DocError::NameTooLong),
        ("two\nlines", DocError::ControlCharacter),
        ("nul\0", DocError::ControlCharacter),
    ] {
        assert_eq!(doc.add_page(Some(name)), Err(error.clone()), "add {name:?}");
        assert_eq!(doc.rename_page(1, name), Err(error), "rename {name:?}");
    }
    assert_eq!(doc, before, "a refused name changes nothing");
}

#[test]
fn a_name_of_the_longest_allowed_length_is_kept_as_given() {
    let mut doc = Document::new_blank();
    // Counted in characters, not bytes; surrounding spaces are the user's own.
    let longest = format!(" {}é ", "ü".repeat(MAX_NAME_CHARS - 3));
    assert_eq!(longest.chars().count(), MAX_NAME_CHARS);
    doc.rename_page(1, &longest).unwrap();
    assert_eq!(doc.page(1).map(|p| &*p.name), Some(longest.as_str()));
}

#[test]
fn validate_reports_the_first_broken_rule() {
    let page = |id, name: &str| Arc::new(Page::new(id, name));
    let mut doc = Document::new_blank();
    doc.pages = vec![page(1, "A"), page(2, "B"), page(1, "C")].into_iter().collect();
    assert_eq!(doc.validate(), Err(DocError::DuplicatePageId(1)));
    doc.pages = vec![page(1, "")].into_iter().collect();
    assert_eq!(doc.validate(), Err(DocError::EmptyName));
    doc.pages = (1..=u32::try_from(MAX_PAGES + 1).unwrap()).map(|i| page(i, "p")).collect();
    assert_eq!(doc.validate(), Err(DocError::TooManyPages));
}

#[test]
fn every_error_has_a_message_that_names_what_to_fix() {
    for (error, words) in [
        (DocError::EmptyName, "empty"),
        (DocError::NameTooLong, "255"),
        (DocError::ControlCharacter, "control"),
        (DocError::NoSuchPage(5), "5"),
        (DocError::TooManyPages, "1000"),
        (DocError::IdsExhausted, "ids"),
        (DocError::DuplicatePageId(3), "3"),
    ] {
        assert!(error.to_string().contains(words), "{error:?}: {error}");
    }
}

/// The one rule for text a person reads or types as a name: page names and undo step names both use it.
#[test]
fn check_text_is_the_one_rule_for_names() {
    assert_eq!(check_text("Fine"), Ok(()));
    assert_eq!(check_text(" padded "), Ok(()));
    assert_eq!(check_text(&"x".repeat(MAX_NAME_CHARS)), Ok(()));
    assert_eq!(check_text(""), Err(TextProblem::Empty));
    assert_eq!(check_text(" \t\u{a0}"), Err(TextProblem::Empty));
    assert_eq!(check_text(&"x".repeat(MAX_NAME_CHARS + 1)), Err(TextProblem::TooLong));
    // Every control character, wherever it is: C0, DEL and C1.
    for control in ['\u{0}', '\t', '\n', '\r', '\u{1b}', '\u{7f}', '\u{85}', '\u{9f}'] {
        assert_eq!(check_text(&format!("a{control}b")), Err(TextProblem::ControlCharacter), "{control:?}");
        assert_eq!(check_text(&format!("{control}start")), Err(TextProblem::ControlCharacter), "{control:?}");
        assert_eq!(check_text(&format!("end{control}")), Err(TextProblem::ControlCharacter), "{control:?}");
    }
    // A page name is checked by that rule and nothing else: the two cannot drift apart.
    for text in ["", "  ", "x".repeat(MAX_NAME_CHARS + 1).as_str(), "a\nb", "ok"] {
        let page = diacraft_doc::check_page_name(text);
        let expected = match check_text(text) {
            Ok(()) => Ok(()),
            Err(TextProblem::Empty) => Err(DocError::EmptyName),
            Err(TextProblem::TooLong) => Err(DocError::NameTooLong),
            Err(TextProblem::ControlCharacter) => Err(DocError::ControlCharacter),
        };
        assert_eq!(page, expected, "{text:?}");
    }
}

#[test]
fn text_problems_say_what_to_fix_in_words_that_fit_after_a_name() {
    for (problem, words) in [
        (TextProblem::Empty, "must not be empty"),
        (TextProblem::TooLong, "must be at most 255 characters"),
        (TextProblem::ControlCharacter, "must not contain control characters"),
    ] {
        assert_eq!(problem.to_string(), words);
    }
}

// ---- the model's own additions to the stub ------------------------------------------------------------------------------------

use diacraft_doc::{Cell, CellAddr, CellEdit, ColorEntry, Resolver, SheetRef, Source, Value, WriteOrigin};

#[test]
fn a_blank_document_has_the_root_style_and_a_page_that_can_be_drawn() {
    // A new document has one style 0, `No Style`, holding the three flags; the page's sheet holds a size.
    let doc = Document::new_blank();
    let root = doc.style(0).unwrap();
    assert_eq!(root.name_u.as_deref(), Some("No Style"));
    for flag in ["EnableLineProps", "EnableFillProps", "EnableTextProps"] {
        assert_eq!(root.sheet.cell(flag).and_then(|c| c.value.as_ref()).map(Value::text), Some("1"), "{flag}");
    }
    let page = SheetRef::Page(1);
    let resolver = Resolver::new(&doc);
    for (cell, value) in [("PageWidth", "11"), ("PageHeight", "8.5"), ("PageScale", "1"), ("DrawingScale", "1")] {
        let r = resolver.resolve(&page, &CellAddr::direct(cell)).unwrap();
        assert_eq!((r.formula.source, r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text)), (Source::Local, Some(value)), "{cell}");
    }
    // The group and glue cells of R23 are the root's, and a page sheet with none of its own reads them.
    let r = resolver.resolve(&page, &CellAddr::direct("SelectMode")).unwrap();
    assert_eq!((r.formula.source, r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text)), (Source::Style { id: 0, family: None }, Some("1")));
}

#[test]
fn new_pages_share_one_default_sheet_until_one_is_written() {
    let mut doc = Document::new_blank();
    doc.add_page(None).unwrap();
    doc.add_page(Some("Third")).unwrap();
    assert!(
        Arc::ptr_eq(&doc.pages.get(0).unwrap().sheet, &doc.pages.get(1).unwrap().sheet)
            && Arc::ptr_eq(&doc.pages.get(1).unwrap().sheet, &doc.pages.get(2).unwrap().sheet),
        "adding a page copies no sheet"
    );
    let width = CellEdit::Cell(Cell::constant(Value::from_text("5")));
    doc.write_cell(&SheetRef::Page(2), &CellAddr::direct("PageWidth"), width, WriteOrigin::Automation).unwrap();
    assert!(!Arc::ptr_eq(&doc.pages.get(1).unwrap().sheet, &doc.pages.get(0).unwrap().sheet));
    assert!(Arc::ptr_eq(&doc.pages.get(0).unwrap().sheet, &doc.pages.get(2).unwrap().sheet), "the others still share it");
    assert_eq!(doc.pages.get(0).unwrap().sheet.cell("PageWidth").and_then(|c| c.value.as_ref()).map(Value::text), Some("11"));
}

#[test]
fn a_name_is_checked_where_it_is_set_and_an_imported_one_does_not_stop_other_writes() {
    // F2: a page that came in with a blank name, and the writes that do not touch it.
    let mut doc = Document::new_blank();
    doc.pages.push(Arc::new(Page::new(5, "")));
    assert_eq!(doc.validate(), Err(DocError::EmptyName), "the audit sees it");
    assert_eq!(doc.add_page(Some("Fine")), Ok(6));
    assert_eq!(doc.rename_page(1, "Renamed"), Ok(()));
    // The write that would set a bad name is the one refused, wherever the document is otherwise.
    assert_eq!(doc.rename_page(1, " "), Err(DocError::EmptyName));
    assert_eq!(doc.page(1).map(|p| &*p.name), Some("Renamed"));
}

#[test]
fn renaming_a_page_leaves_its_universal_name_and_a_new_page_gets_both() {
    let mut doc = Document::new_blank();
    assert_eq!(doc.page(1).and_then(|p| p.name_u.as_deref()), Some("Page-1"));
    doc.rename_page(1, "Overview").unwrap();
    assert_eq!(doc.page(1).map(|p| (&*p.name, p.name_u.as_deref())), Some(("Overview", Some("Page-1"))));
}

#[test]
fn a_color_index_is_the_fixed_palette_or_the_color_table() {
    // 2 is #FF0000, 23 is #1A1A1A, 24 with an entry `#123456` is that.
    let mut doc = Document::empty();
    doc.colors = Arc::new(vec![ColorEntry { ix: 24, rgb: Arc::from("#123456") }, ColorEntry { ix: 30, rgb: Arc::from("not a color") }]);
    assert_eq!(doc.color(2).map(|c| c.hex()), Some("FF0000".to_owned()));
    assert_eq!(doc.color(23).map(|c| c.hex()), Some("1A1A1A".to_owned()));
    assert_eq!(doc.color(24).map(|c| c.hex()), Some("123456".to_owned()));
    // No entry, no color; nothing is invented. And the hostile indexes.
    for index in [25, 30, -1, i64::MIN, i64::MAX, 253, 4_294_967_296] {
        assert_eq!(doc.color(index), None, "{index}");
    }
}

#[test]
fn background_pages_are_followed_nearest_first() {
    let mut doc = Document::empty();
    for (id, back) in [(1, Some(2)), (2, Some(3)), (3, None), (4, Some(4))] {
        let mut page = Page::new(id, "p");
        page.back_page = back;
        doc.pages.push(Arc::new(page));
    }
    assert_eq!(doc.background_chain(1).iter().map(|p| p.id).collect::<Vec<_>>(), [2, 3]);
    assert!(doc.background_chain(3).is_empty());
    assert!(doc.background_chain(4).is_empty());
}

#[test]
fn the_root_style_of_a_blank_document_holds_a_basic_line_fill_and_text_look() {
    // "basic line, fill and text cells". The review found the comment claimed them and the root had no text cell and
    // no `LineWeight`.
    let doc = Document::new_blank();
    let resolver = Resolver::new(&doc);
    let root = SheetRef::Style(0);
    let text = |cell: &str, row: Option<u32>| {
        let addr = match row {
            Some(ix) => CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(ix), cell),
            None => CellAddr::direct(cell),
        };
        resolver.resolve(&root, &addr).and_then(|r| r.value.cell.and_then(|c| c.value.clone())).map(|v| v.text().to_owned())
    };
    for (cell, value) in
        [("LineWeight", "0.01"), ("LinePattern", "1"), ("LineColor", "0"), ("FillPattern", "1"), ("FillForegnd", "1"), ("FillBkgnd", "0")]
    {
        assert_eq!(text(cell, None).as_deref(), Some(value), "{cell}");
    }
    for (cell, value) in [
        ("VerticalAlign", "1"),
        ("LeftMargin", "0.05555556"),
        ("RightMargin", "0.05555556"),
        ("TopMargin", "0.05555556"),
        ("BottomMargin", "0.05555556"),
    ] {
        assert_eq!(text(cell, None).as_deref(), Some(value), "{cell}");
    }
    assert_eq!(text("Size", Some(0)).as_deref(), Some("0.1666667"));
    assert_eq!(text("Color", Some(0)).as_deref(), Some("0"));
    // A shape linked to the root by the three links reads them, in its family.
    let linked = diacraft_testkit::build::DocBuilder::new()
        .page(1, diacraft_testkit::build::PageBuilder::new().shape(diacraft_testkit::build::ShapeBuilder::new(10).links(Some(0), Some(0), Some(0))))
        .root(|r| r.cell("LineWeight", "0.02"))
        .build();
    let at = SheetRef::Shape(diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(1), path: vec![0] });
    let r = Resolver::new(&linked).resolve(&at, &CellAddr::direct("LineWeight")).unwrap();
    assert_eq!(
        (r.formula.source, r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text)),
        (Source::Style { id: 0, family: Some(diacraft_doc::Family::Line) }, Some("0.02"))
    );
}

/// A name a command gives a page has at most 31 characters (the Page Properties tab, rule E2 of the page setup dialog); the model still holds the names of a file, up to 255.
#[test]
fn a_name_a_command_gives_a_page_has_at_most_31_characters_and_the_model_still_holds_a_longer_one_a_file_has() {
    assert_eq!(MAX_PAGE_NAME_CHARS, 31);
    // Counted in characters, not bytes: 31 of two bytes and 31 of four.
    assert_eq!(check_given_page_name(&"é".repeat(31)), Ok(()));
    assert_eq!(check_given_page_name(&"\u{1f600}".repeat(31)), Ok(()));
    assert_eq!(check_given_page_name(&"x".repeat(32)), Err(DocError::PageNameTooLong));
    assert_eq!(check_given_page_name(&"x".repeat(100_000)), Err(DocError::PageNameTooLong));
    // The rules of any name still hold.
    assert_eq!(check_given_page_name(""), Err(DocError::EmptyName));
    assert_eq!(check_given_page_name("  "), Err(DocError::EmptyName));
    assert_eq!(check_given_page_name("a\nb"), Err(DocError::ControlCharacter));
    assert!(DocError::PageNameTooLong.to_string().contains("at most 31 characters"), "{}", DocError::PageNameTooLong);
    // The document takes the 40 characters a file gave a page, as an add and a rename of it, and the audit passes it.
    let mut doc = Document::new_blank();
    let forty = "n".repeat(40);
    doc.rename_page(1, &forty).unwrap();
    let second = doc.add_page(Some(&forty)).unwrap();
    assert_eq!(doc.page(second).map(|p| &*p.name), Some(forty.as_str()));
    assert_eq!(doc.validate(), Ok(()));
}

/// The page a command works on when it is not told which: the first page that is not a background (the tabs put the background pages after the others, and a file lists them first), and
/// the first page there is when every page is one.
#[test]
fn the_first_foreground_page_is_the_first_that_is_not_a_background_else_the_first() {
    let page = |id: u32, background: bool| {
        let mut page = Page::new(id, &format!("P{id}"));
        page.background = background;
        Arc::new(page)
    };
    let mut doc = Document::new_blank();
    assert_eq!(doc.first_foreground_page().map(|p| p.id), Some(1), "a blank drawing's one page");
    doc.pages = vec![page(7, true), page(8, true), page(3, false), page(4, false)].into_iter().collect();
    assert_eq!(doc.first_foreground_page().map(|p| p.id), Some(3), "backgrounds first, as a file lists them");
    doc.pages = vec![page(3, false), page(7, true), page(4, false)].into_iter().collect();
    assert_eq!(doc.first_foreground_page().map(|p| p.id), Some(3), "a background between the pages");
    doc.pages = vec![page(7, true), page(8, true)].into_iter().collect();
    assert_eq!(doc.first_foreground_page().map(|p| p.id), Some(7), "every page a background: the first there is");
    doc.pages = Default::default();
    assert!(doc.first_foreground_page().is_none(), "no pages, no page");
}
