//! **The VSDX writers write the page names the program that opens the file keeps.** That program renames a page called "page" and digits (the name an `.odg` gives its pages,
//! `page1`) and a repeat of another page's name, and gives the renamed page's `Name` and `NameU` different numbers. So a writer that makes a page row (the writer of a drawing never saved, the
//! model writer, and a save into a file for a page it writes: a page that is new or renamed) gives such a page the default form, `Page-N` for the lowest N that no page has, the same in `Name`
//! and `NameU`, and says so in the save's warnings. **A page row the file already holds is not rewritten**, so a name the file has is kept as it is (the reader keeps whatever names a file has,
//! and a save with no edit writes what it read).
//!
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{model_write, text_of};
use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const SAID: &str = "given a new name in the file";

/// The `Name` and `NameU` of each page as a reader finds them, in order.
fn names_of(bytes: &[u8]) -> Vec<(String, String)> {
    let (doc, _) = read_ok(bytes);
    doc.pages.iter().map(|p| (p.name.to_string(), p.name_u.as_deref().unwrap_or("").to_owned())).collect()
}

fn each(names: &[&str]) -> Vec<(String, String)> {
    names.iter().map(|n| ((*n).to_owned(), (*n).to_owned())).collect()
}

fn said(written: &Written) -> Vec<&String> {
    written.warnings.iter().filter(|w| w.contains(SAID)).collect()
}

/// A drawing never saved whose pages have these names (`Name` and `NameU` alike), the page ids from 1.
fn drawing(names: &[&str]) -> Document {
    let mut builder = DocBuilder::new();
    for (at, name) in names.iter().enumerate() {
        builder = builder.page(u32::try_from(at + 1).unwrap(), PageBuilder::new().name(name).shape(ShapeBuilder::new(1)));
    }
    builder.build()
}

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn new_file(doc: &Document) -> Written {
    write_with(doc, None, &options()).unwrap_or_else(|e| panic!("{e}"))
}

/// The package a reader gives pages with these names: one shape each, the page ids from 1.
fn package_of(names: &[&str]) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    for (at, name) in names.iter().enumerate() {
        builder = builder.page(PageSpec::new(u32::try_from(at + 1).unwrap(), name).shapes(&shape(11, &cell("PinX", "1"))));
    }
    builder.build()
}

fn save_into(doc: &Document, original: &[u8], baseline: Option<&Document>) -> Written {
    let original = OriginalBytes { bytes: original, baseline };
    write_with(doc, Some(&original), &options()).unwrap_or_else(|e| panic!("{e}"))
}

/// No two names alike without regard to case, in `Name` and in `NameU`, and none "page" and digits.
fn assert_names_are_kept(names: &[(String, String)]) {
    let page_and_digits = |n: &str| n.len() > 4 && n[..4].eq_ignore_ascii_case("page") && n[4..].bytes().all(|b| b.is_ascii_digit());
    for (which, pick) in [("Name", 0usize), ("NameU", 1usize)] {
        let mut seen = std::collections::HashSet::new();
        for pair in names {
            let name = if pick == 0 { &pair.0 } else { &pair.1 };
            assert!(!page_and_digits(name), "{which} {name} is page and digits: {names:?}");
            assert!(seen.insert(name.to_lowercase()), "{which} {name} is repeated: {names:?}");
        }
    }
}

// ---- a drawing never saved ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_drawing_from_an_odg_with_pages_page1_and_page2_is_written_with_page_1_and_page_2_and_says_so() {
    let written = new_file(&drawing(&["page1", "page2"]));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    let words = said(&written);
    assert_eq!(words.len(), 1, "one warning: {:?}", written.warnings);
    assert!(words[0].starts_with("2 pages were given a new name in the file"), "{}", words[0]);
    assert!(words[0].contains("page 1 is now \"Page-1\"") && words[0].contains("page 2 is now \"Page-2\""), "{}", words[0]);
    // What the save hands back is what the file reads as, and the list of page names in the properties follows the pages.
    assert_eq!(written.document.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>(), ["Page-1", "Page-2"]);
    let app = text_of(&written.bytes, "/docProps/app.xml");
    assert!(app.contains("Page-1") && app.contains("Page-2") && !app.contains("page1"), "{app}");
}

#[test]
fn a_repeated_name_is_given_the_lowest_number_no_page_has_whatever_its_case() {
    let written = new_file(&drawing(&["Page-1", "Page-1", "Overview", "overview", "page-2"]));
    // The first of a name keeps it; the repeats are the lowest numbers free: Page-2 is taken by the fifth page (in another case), so the second is Page-3 and the fourth Page-4.
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-3", "Overview", "Page-4", "page-2"]));
    assert_eq!(said(&written).len(), 1, "{:?}", written.warnings);
}

#[test]
fn a_page_that_is_renamed_never_takes_the_name_a_later_page_keeps() {
    // The first page is "page" and digits; the second is the default name its number would be given: it keeps it, and the first is Page-2.
    let written = new_file(&drawing(&["page1", "Page-1", "page2"]));
    assert_eq!(names_of(&written.bytes), each(&["Page-2", "Page-1", "Page-3"]));
}

#[test]
fn a_drawing_with_names_the_program_keeps_is_written_with_no_word_about_names() {
    let written = new_file(&drawing(&["Page-1", "Overview", "Page 3", "page4a", "VBackground-1"]));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Overview", "Page 3", "page4a", "VBackground-1"]));
    assert!(said(&written).is_empty(), "{:?}", written.warnings);
}

#[test]
fn many_pages_with_one_name_are_each_given_a_name_of_their_own() {
    let many = vec!["page1"; 40];
    let written = new_file(&drawing(&many));
    let names = names_of(&written.bytes);
    assert_eq!(names.len(), 40);
    assert_names_are_kept(&names);
    assert_eq!(names.first().map(|n| n.0.as_str()), Some("Page-1"));
    assert_eq!(names.last().map(|n| n.0.as_str()), Some("Page-40"));
    // The warning lists the first few and says how many more, and not forty names.
    let words = said(&written);
    assert_eq!(words.len(), 1);
    assert!(words[0].starts_with("40 pages were given a new name in the file"), "{}", words[0]);
    assert!(words[0].len() < 700, "{} characters: {}", words[0].len(), words[0]);
    assert!(words[0].contains("and 35 more"), "{}", words[0]);
}

#[test]
fn the_universal_name_is_held_to_the_same_rules_and_the_name_a_person_gave_is_kept() {
    // A page a person renamed to Overview keeps its universal name Page-2; a page called Page-2 that came after it has that universal name too.
    let mut doc = drawing(&["Overview", "Page-2", "page3"]);
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Page-2"));
    let written = new_file(&doc);
    let names = names_of(&written.bytes);
    assert_names_are_kept(&names);
    assert_eq!(names.first().map(|n| n.0.as_str()), Some("Overview"), "the name a person gave is kept: {names:?}");
    assert_eq!(names.get(1).map(|n| n.0.as_str()), Some("Page-2"), "a name that is free is kept: {names:?}");
    assert_eq!(names.first().map(|n| n.1.as_str()), Some("Page-2"));
    let words = said(&written);
    assert_eq!(words.len(), 1, "{:?}", written.warnings);
    // The second page keeps its name and has another universal name, which the words say apart from a new name.
    assert!(words[0].contains("page 2 has the universal name \"Page-1\"") && words[0].contains("page 3 is now \"Page-3\""), "{}", words[0]);
}

// ---- the model writer --------------------------------------------------------------------------------------------------------------------

#[test]
fn a_model_written_from_a_drawing_that_holds_a_package_is_written_with_names_the_program_keeps() {
    // A drawing read from a file holds its package, so it is written from the model alone; its pages are then renamed as an `.odg`'s are named, and one is added with a name another page has.
    let (mut doc, _) = read_ok(&package_of(&["Page-1", "Overview"]));
    doc.rename_page(2, "page2").unwrap();
    doc.add_page(Some("page-1")).unwrap();
    let written = model_write(&doc, false);
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2", "Page-3"]));
    assert_eq!(said(&written).len(), 1, "{:?}", written.warnings);
    // Even a model that says its content is as it was read is written so: the names of another format's drawing are not a package's own.
    let unchanged = model_write(&doc, true);
    assert_eq!(names_of(&unchanged.bytes), each(&["Page-1", "Page-2", "Page-3"]));
}

#[test]
fn a_model_with_names_the_program_keeps_is_written_with_no_word_about_names() {
    let (doc, _) = read_ok(&package_of(&["Page-1", "Overview"]));
    let written = model_write(&doc, false);
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Overview"]));
    assert!(said(&written).is_empty(), "{:?}", written.warnings);
}

// ---- a save into a file ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_save_into_a_file_leaves_the_names_the_file_holds_as_they_are_edited_or_not() {
    // A file of another program with a repeated name and a name "page" and digits: opened, its names are kept, and a save keeps them, with an edit of a shape and with none.
    let bytes = package_of(&["page1", "page1", "Page-1"]);
    let (doc, _) = read_ok(&bytes);
    assert_eq!(names_of(&bytes), each(&["page1", "page1", "Page-1"]));
    let unedited = save_into(&doc, &bytes, Some(&doc));
    assert_eq!(names_of(&unedited.bytes), each(&["page1", "page1", "Page-1"]));
    assert!(said(&unedited).is_empty(), "{:?}", unedited.warnings);
    let mut edited = doc.clone();
    let page = Arc::make_mut(edited.pages.get_mut(0).unwrap());
    page.shapes.push(Arc::new(diacraft_doc::Shape::new(12)));
    let written = save_into(&edited, &bytes, Some(&doc));
    assert_eq!(names_of(&written.bytes), each(&["page1", "page1", "Page-1"]));
    assert!(said(&written).is_empty(), "{:?}", written.warnings);
}

#[test]
fn a_page_added_to_a_file_with_a_name_the_program_renames_is_written_with_the_lowest_number_free() {
    let bytes = package_of(&["Page-1", "Overview"]);
    let (doc, _) = read_ok(&bytes);
    let mut edited = doc.clone();
    edited.add_page(Some("page5")).unwrap();
    edited.add_page(Some("OVERVIEW")).unwrap();
    edited.add_page(Some("Fresh")).unwrap();
    for baseline in [Some(&doc), None] {
        let written = save_into(&edited, &bytes, baseline);
        assert_eq!(names_of(&written.bytes), each(&["Page-1", "Overview", "Page-2", "Page-3", "Fresh"]), "baseline {}", baseline.is_some());
        let words = said(&written);
        assert_eq!(words.len(), 1, "{:?}", written.warnings);
        assert!(words[0].starts_with("2 pages were given a new name in the file"), "{}", words[0]);
        assert!(words[0].contains("page 3 is now \"Page-2\"") && words[0].contains("page 4 is now \"Page-3\""), "{}", words[0]);
    }
}

#[test]
fn a_page_of_a_file_renamed_to_a_name_the_program_renames_is_written_with_a_default_name() {
    let bytes = package_of(&["Page-1", "Overview"]);
    let (doc, _) = read_ok(&bytes);
    let mut edited = doc.clone();
    edited.rename_page(2, "page2").unwrap();
    let written = save_into(&edited, &bytes, Some(&doc));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    assert_eq!(said(&written).len(), 1, "{:?}", written.warnings);
    // A rename to a name that is free is written as it is.
    let mut fine = doc.clone();
    fine.rename_page(2, "Detail").unwrap();
    let written = save_into(&fine, &bytes, Some(&doc));
    // (A rename leaves the universal name a page has.)
    assert_eq!(names_of(&written.bytes), [("Page-1".to_owned(), "Page-1".to_owned()), ("Detail".to_owned(), "Overview".to_owned())]);
    assert!(said(&written).is_empty(), "{:?}", written.warnings);
}

#[test]
fn a_drawing_made_from_the_model_of_another_save_is_written_the_same_names_again() {
    // The names a save chose are the ones the next save of the same model chooses (nothing changes, so nothing is rewritten): the session's page1 against the file's Page-1.
    let doc = drawing(&["page1", "page2"]);
    let first = new_file(&doc);
    let again = new_file(&doc);
    assert_eq!(first.bytes, again.bytes, "one model, one file");
}

// ---- the function the writers ask -----------------------------------------------------------------------------------------------------------

#[test]
fn keep_page_names_gives_a_drawing_of_a_thousand_pages_one_name_each_and_finds_nothing_more_to_rename_in_what_it_gave() {
    // The pages of a drawing at the limit, all called "page1": the work is a pass over the pages and one number for each, and what it gives is what it keeps.
    let many = vec!["page1"; diacraft_doc::MAX_PAGES];
    let doc = drawing(&many);
    let kept = diacraft_vsdx::keep_page_names(&doc, None).expect("every page is renamed");
    assert_eq!(kept.renamed.len(), diacraft_doc::MAX_PAGES);
    let names: Vec<(String, String)> = kept.doc.pages.iter().map(|p| (p.name.to_string(), p.name_u.as_deref().unwrap_or("").to_owned())).collect();
    assert_names_are_kept(&names);
    assert_eq!(names.last().map(|n| n.0.as_str()), Some("Page-1000"));
    assert!(diacraft_vsdx::keep_page_names(&kept.doc, None).is_none(), "the names it gave stand");
    // A drawing whose names stand is not copied, and says nothing.
    assert!(diacraft_vsdx::keep_page_names(&drawing(&["Page-1", "Overview"]), None).is_none());
    // The words of a thousand renamed pages are cut: the first five by place and the count of the rest.
    let words = kept.words();
    assert!(words.starts_with("1000 pages were given a new name in the file"), "{words}");
    assert!(words.contains("and 995 more") && words.len() < 800, "{} characters: {words}", words.len());
}

#[test]
fn a_page_that_stands_as_the_file_holds_it_is_not_renamed_and_a_session_that_still_has_the_old_names_says_nothing_again() {
    // The file holds Page-1 and Page-2 (a save wrote them for page1 and page2). The session still calls them page1 and page2: nothing the file does not hold is written, and nothing is said.
    let bytes = package_of(&["Page-1", "Page-2"]);
    let (file, _) = read_ok(&bytes);
    let mut session = file.clone();
    session.rename_page(1, "page1").unwrap();
    Arc::make_mut(session.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("page1"));
    session.rename_page(2, "page2").unwrap();
    Arc::make_mut(session.pages.get_mut(1).unwrap()).name_u = Some(Arc::from("page2"));
    let kept = diacraft_vsdx::keep_page_names(&session, Some(&file)).expect("the session's names are not the file's");
    assert!(kept.renamed.is_empty(), "{:?}", kept.renamed);
    assert_eq!(kept.doc.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>(), ["Page-1", "Page-2"]);
    let written = save_into(&session, &bytes, Some(&file));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    assert!(said(&written).is_empty(), "{:?}", written.warnings);
}

#[test]
fn a_model_whose_pages_are_renamed_is_an_edited_save_even_when_it_says_its_content_is_as_it_was_read() {
    // The names of the pages are content: a model written with content that is as it was read keeps the program's name in the properties, and one whose pages the writer renamed does not.
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application></Properties>";
    let bytes = VsdxBuilder::new()
        .app(app)
        .page(PageSpec::new(1, "Page-1").shapes(&shape(11, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Overview").shapes(&shape(12, &cell("PinX", "2"))))
        .build();
    let (doc, _) = read_ok(&bytes);
    let kept = model_write(&doc, true);
    assert!(
        text_of(&kept.bytes, "/docProps/app.xml").contains("<Application>Some Writer</Application>"),
        "the control: nothing renamed, nothing edited"
    );
    let mut renamed = doc.clone();
    renamed.rename_page(2, "page2").unwrap();
    let written = model_write(&renamed, true);
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    let app = text_of(&written.bytes, "/docProps/app.xml");
    assert!(app.contains("<Application>DiaCraft</Application>"), "{app}");
}

// ---- the formulas that name a page the writer renamed ---------------------------------------------------------------------------------------

/// The text of the formula of a cell and the value it holds, in a sheet.
fn cell_of(sheet: &diacraft_doc::Sheet, name: &str) -> (String, String) {
    let cell = sheet.cell(name).unwrap_or_else(|| panic!("no cell {name}"));
    let formula = match &cell.formula {
        diacraft_doc::FormulaSlot::Local(f) => f.source.to_string(),
        other => format!("{other:?}"),
    };
    (formula, cell.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())
}

fn shape_cell(doc: &Document, page: u32, id: u32, name: &str) -> (String, String) {
    let at = doc.find_shape(diacraft_doc::Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id} on page {page}"));
    let shape = doc.shape(&at).unwrap();
    assert_eq!(shape.sheet.cell(name).and_then(|c| c.error.clone()), None, "the cell {name} of shape {id} holds no error");
    cell_of(&shape.sheet, name)
}

fn formula_edit(text: &str, value: &str) -> diacraft_doc::CellEdit {
    diacraft_doc::CellEdit::Formula { formula: Arc::new(diacraft_formula::parse(text)), value: Some(diacraft_doc::Value::from_text(value)) }
}

/// A drawing whose pages are `page1` and `page2` (the names an `.odg` gives) and a repeated name, with a formula that names a page in every place a sheet is held: a shape, a member of a
/// group, a page's sheet, a master and its shape, a style and the document's sheet.
fn drawing_with_formulas() -> Document {
    use diacraft_testkit::build::{MasterBuilder, StyleBuilder};
    let group = ShapeBuilder::new(20).group().child(ShapeBuilder::new(21).formula("Height", "Pages['page2']!Sheet.1!Height+1", "3"));
    let mut doc = DocBuilder::new()
        .style(1, StyleBuilder::new().name("Linked").formula("LineWeight", "Pages[page1]!Sheet.10!Width*0.01", "0.04"))
        .master(
            1,
            MasterBuilder::new().name("Box").formula("PageWidth", "Pages[page2]!PageWidth", "11").shape(ShapeBuilder::new(5).formula(
                "Width",
                "Pages[page1]!Sheet.10!Width",
                "4",
            )),
        )
        .page(
            1,
            PageBuilder::new()
                .name("page1")
                .shape(ShapeBuilder::new(10).cell("Width", "4").formula("Height", "IF(1,\"Pages[page1]!Sheet.1!Width\",0)", "1").formula(
                    "PinX",
                    "Pages[Other]!Sheet.1!Width",
                    "6",
                ))
                .shape(group),
        )
        .page(
            2,
            PageBuilder::new()
                .name("page2")
                .formula("PageHeight", "Pages[ page1 ]!PageHeight", "8.5")
                .shape(ShapeBuilder::new(1).formula("Width", "Pages[page1]!Sheet.10!Width+1", "5").cell("Height", "2")),
        )
        .page(3, PageBuilder::new().name("Dup").shape(ShapeBuilder::new(1).formula("Width", "Pages[Dup]!Sheet.1!Height", "2").cell("Height", "2")))
        .page(4, PageBuilder::new().name("Dup").shape(ShapeBuilder::new(1).formula("Width", "Pages[8]!Sheet.1!Height", "2").cell("Height", "2")))
        .build();
    // The universal names of the last two pages are numbers, so that a formula can name the second by its number (`Pages[8]`).
    Arc::make_mut(doc.pages.get_mut(2).unwrap()).name_u = Some(Arc::from("7"));
    Arc::make_mut(doc.pages.get_mut(3).unwrap()).name_u = Some(Arc::from("8"));
    doc.write_cell(
        &diacraft_doc::SheetRef::Document,
        &diacraft_doc::CellAddr::direct("Probe"),
        formula_edit("Pages[page2]!PageWidth", "11"),
        diacraft_doc::WriteOrigin::AutomationForce,
    )
    .unwrap();
    doc
}

#[test]
fn the_formulas_that_name_a_page_the_writer_renamed_name_its_new_name_everywhere_a_sheet_is_held_and_keep_their_values() {
    let written = new_file(&drawing_with_formulas());
    assert_eq!(
        names_of(&written.bytes),
        [("Page-1".into(), "Page-1".into()), ("Page-2".into(), "Page-2".into()), ("Dup".into(), "7".into()), ("Page-3".into(), "Page-3".into())]
    );
    let (doc, _) = read_ok(&written.bytes);
    // Each formula names the page by its new name, the blanks and the quotes of the old index gone with it, and holds the value it held.
    assert_eq!(shape_cell(&doc, 2, 1, "Width"), ("Pages[Page-1]!Sheet.10!Width+1".into(), "5".into()));
    assert_eq!(shape_cell(&doc, 1, 21, "Height"), ("Pages[Page-2]!Sheet.1!Height+1".into(), "3".into()));
    assert_eq!(cell_of(&doc.page(2).unwrap().sheet, "PageHeight"), ("Pages[Page-1]!PageHeight".into(), "8.5".into()));
    assert_eq!(cell_of(&doc.master(1).unwrap().sheet, "PageWidth"), ("Pages[Page-2]!PageWidth".into(), "11".into()));
    let master_shape = doc.shape(&doc.find_shape(diacraft_doc::Owner::Master(1), 5).unwrap()).unwrap();
    assert_eq!(cell_of(&master_shape.sheet, "Width"), ("Pages[Page-1]!Sheet.10!Width".into(), "4".into()));
    assert_eq!(cell_of(&doc.style(1).unwrap().sheet, "LineWeight"), ("Pages[Page-1]!Sheet.10!Width*0.01".into(), "0.04".into()));
    assert_eq!(cell_of(&doc.sheet, "Probe"), ("Pages[Page-2]!PageWidth".into(), "11".into()));
    // A page named by its number is named by its new name.
    assert_eq!(shape_cell(&doc, 4, 1, "Width"), ("Pages[Page-3]!Sheet.1!Height".into(), "2".into()));
    // What names no renamed page is as it was: a string, a page that is no page, and a page that kept its name.
    assert_eq!(shape_cell(&doc, 1, 10, "Height"), ("IF(1,\"Pages[page1]!Sheet.1!Width\",0)".into(), "1".into()));
    assert_eq!(shape_cell(&doc, 1, 10, "PinX"), ("Pages[Other]!Sheet.1!Width".into(), "6".into()));
    assert_eq!(shape_cell(&doc, 3, 1, "Width"), ("Pages[Dup]!Sheet.1!Height".into(), "2".into()));
    // The warning says how many formulas follow their page.
    let words = said(&written);
    assert_eq!(words.len(), 1, "{:?}", written.warnings);
    assert!(
        words[0].contains("8 formulas that named such a page by their old name now name it by its new name and keep their values"),
        "{}",
        words[0]
    );
}

#[test]
fn a_page_a_formula_names_by_the_universal_name_a_rename_left_is_followed_by_the_model_writer() {
    // A page a person renamed keeps its universal name (`Overview`), which a formula names it by: the page is given a default name in both, and the formula follows.
    let formula = r#"<Cell N="Width" V="2" F="Pages[Overview]!Sheet.12!PinX"/>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").shapes(&format!(r#"<Shape ID="11" Type="Shape">{formula}</Shape>"#)))
        .page(PageSpec::new(2, "Overview").shapes(&shape(12, &cell("PinX", "1"))))
        .build();
    let (mut doc, _) = read_ok(&bytes);
    doc.rename_page(2, "page2").unwrap();
    let written = model_write(&doc, false);
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(shape_cell(&back, 1, 11, "Width"), ("Pages[Page-2]!Sheet.12!PinX".into(), "2".into()));
    let words = said(&written);
    assert!(words[0].contains("1 formula that named such a page by its old name now names it by its new name and keeps its value"), "{}", words[0]);
}

#[test]
fn a_save_into_a_file_follows_the_page_it_renames_in_the_formulas_of_the_pages_it_leaves() {
    let formula = r#"<Cell N="Width" V="2" F="Pages[Overview]!Sheet.12!PinX"/>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").shapes(&format!(r#"<Shape ID="11" Type="Shape">{formula}</Shape>"#)))
        .page(PageSpec::new(2, "Overview").shapes(&shape(12, &cell("PinX", "1"))))
        .build();
    let (file, _) = read_ok(&bytes);
    // Nothing renamed: the formula is the file's own, byte for byte.
    let same = save_into(&file, &bytes, Some(&file));
    assert_eq!(shape_cell(&read_ok(&same.bytes).0, 1, 11, "Width").0, "Pages[Overview]!Sheet.12!PinX");
    // The page is renamed to a name the program renames: the page row is given the default name and the formula of the page that was not touched follows.
    let mut edited = file.clone();
    edited.rename_page(2, "page2").unwrap();
    let written = save_into(&edited, &bytes, Some(&file));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2"]));
    assert_eq!(shape_cell(&read_ok(&written.bytes).0, 1, 11, "Width"), ("Pages[Page-2]!Sheet.12!PinX".into(), "2".into()));
    assert!(
        said(&written)[0].contains("1 formula that named such a page by its old name now names it by its new name and keeps its value"),
        "{:?}",
        written.warnings
    );
    // A page added with a name the program renames, which a formula of the model names, is followed the same way, and the file is read as the model.
    let mut added = file.clone();
    added.add_page(Some("page5")).unwrap();
    added
        .write_cell(
            &diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0)),
            &diacraft_doc::CellAddr::direct("Height"),
            formula_edit("Pages[page5]!PageWidth+1", "9"),
            diacraft_doc::WriteOrigin::Drawing,
        )
        .unwrap();
    let written = save_into(&added, &bytes, Some(&file));
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Overview", "Page-2"]));
    assert_eq!(shape_cell(&read_ok(&written.bytes).0, 1, 11, "Height"), ("Pages[Page-2]!PageWidth+1".into(), "9".into()));
}

// ---- the hyperlinks that name a page the writer renamed -------------------------------------------------------------------------------------

/// A row of a shape's `Hyperlink` section, with the `SubAddress` and, for a link outside the drawing, the `Address`, as plain text.
fn link_row(row: &str, address: Option<&str>, sub_address: &str) -> Vec<(diacraft_doc::CellAddr, diacraft_doc::Cell)> {
    let at = |cell: &str| diacraft_doc::CellAddr::in_row("Hyperlink", None, diacraft_doc::RowKey::Name(Arc::from(row)), cell);
    let text = |t: &str| diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(t)).with_unit("STR");
    let mut cells = vec![(at("SubAddress"), text(sub_address))];
    if let Some(address) = address {
        cells.push((at("Address"), text(address)));
    }
    cells
}

fn shape_with_links(id: u32, rows: &[(&str, Option<&str>, &str)]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id);
    for (row, address, sub_address) in rows {
        for (addr, cell) in link_row(row, *address, sub_address) {
            shape = shape.set(addr, cell);
        }
    }
    shape
}

fn link_text(doc: &Document, page: u32, id: u32, row: &str, cell: &str) -> String {
    let at = doc.find_shape(diacraft_doc::Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id} on page {page}"));
    let shape = doc.shape(&at).unwrap();
    let addr = diacraft_doc::CellAddr::in_row("Hyperlink", None, diacraft_doc::RowKey::Name(Arc::from(row)), cell);
    match shape.sheet.lookup(&addr) {
        diacraft_doc::Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
        _ => panic!("no cell {cell} in the row {row}"),
    }
}

#[test]
fn a_hyperlink_that_names_a_page_the_writer_renamed_as_plain_text_names_its_new_name() {
    // `page1` and `page2` are what an `.odg` names its pages, and are renamed; `Overview` is a name the program keeps. A link of this drawing (no `Address`) names a page, or a shape of a page after a slash.
    let rows = [
        ("Row_1", None, "page2"),
        ("Row_2", None, "page2/Box"),
        ("Row_3", None, "  page1  "),
        ("Row_4", Some("https://example.org/a"), "page2"),
        ("Row_5", None, "Overview"),
        ("Row_6", None, "page22"),
        ("Row_7", None, "page1/Sheet.4"),
        ("Row_8", None, "Overview/page2"),
        ("Row_9", None, "PAGE2"),
    ];
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("page1").shape(shape_with_links(1, &rows)))
        .page(2, PageBuilder::new().name("page2").shape(ShapeBuilder::new(1)))
        .page(3, PageBuilder::new().name("Overview").shape(ShapeBuilder::new(1)))
        .build();
    let written = new_file(&doc);
    assert_eq!(names_of(&written.bytes), each(&["Page-1", "Page-2", "Overview"]));
    let (back, _) = read_ok(&written.bytes);
    let sub = |row: &str| link_text(&back, 1, 1, row, "SubAddress");
    assert_eq!(sub("Row_1"), "Page-2");
    assert_eq!(sub("Row_2"), "Page-2/Box");
    assert_eq!(sub("Row_3"), "  Page-1  ", "the blanks round the name stay");
    assert_eq!(sub("Row_7"), "Page-1/Sheet.4");
    // What does not name a renamed page as the link of this drawing is as it was: the fragment of a web address, a page that kept its name, a text that names no page, another case of the name.
    assert_eq!(sub("Row_4"), "page2");
    assert_eq!(sub("Row_5"), "Overview");
    assert_eq!(sub("Row_6"), "page22");
    assert_eq!(sub("Row_8"), "Overview/page2");
    assert_eq!(sub("Row_9"), "PAGE2");
    assert_eq!(link_text(&back, 1, 1, "Row_4", "Address"), "https://example.org/a");
    let words = said(&written);
    assert_eq!(words.len(), 1, "{:?}", written.warnings);
    assert!(words[0].contains("4 hyperlinks that named such a page now name it by its new name"), "{}", words[0]);
}

#[test]
fn a_hyperlink_in_a_master_a_style_and_a_page_sheet_follows_the_page_and_a_link_a_save_into_a_file_leaves_is_left() {
    use diacraft_testkit::build::MasterBuilder;
    let mut master = MasterBuilder::new().name("Box").shape(shape_with_links(5, &[("Row_1", None, "page2")]));
    for (addr, cell) in link_row("Row_1", None, "page1") {
        master = master.set(addr, cell);
    }
    let mut page_one = PageBuilder::new().name("page1").shape(ShapeBuilder::new(1));
    for (addr, cell) in link_row("Row_1", None, "page2") {
        page_one = page_one.set(addr, cell);
    }
    let doc = DocBuilder::new().master(1, master).page(1, page_one).page(2, PageBuilder::new().name("page2").shape(ShapeBuilder::new(1))).build();
    let written = new_file(&doc);
    let (back, _) = read_ok(&written.bytes);
    let row = |sheet: &diacraft_doc::Sheet| match sheet.lookup(&diacraft_doc::CellAddr::in_row(
        "Hyperlink",
        None,
        diacraft_doc::RowKey::Name(Arc::from("Row_1")),
        "SubAddress",
    )) {
        diacraft_doc::Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
        _ => panic!("no row"),
    };
    assert_eq!(row(&back.master(1).unwrap().sheet), "Page-1");
    let master_shape = back.shape(&back.find_shape(diacraft_doc::Owner::Master(1), 5).unwrap()).unwrap();
    assert_eq!(row(&master_shape.sheet), "Page-2");
    assert_eq!(row(&back.page(1).unwrap().sheet), "Page-2");
    assert!(said(&written)[0].contains("3 hyperlinks that named such a page now name it by its new name"), "{:?}", written.warnings);

    // A file that holds the names `page1` and `page2` keeps them, and its links with them: nothing is renamed, so nothing follows.
    let bytes = package_of(&["page1", "page2"]);
    let (file, _) = read_ok(&bytes);
    let same = save_into(&file, &bytes, Some(&file));
    assert_eq!(names_of(&same.bytes), each(&["page1", "page2"]));
    assert!(said(&same).is_empty(), "{:?}", same.warnings);
}
