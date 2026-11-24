//! The review: the caps on what a document file holds, and what it may not say.
//!
//! * The masters: the reader refuses the master past the most a document holds (`MAX_MASTERS`) **before it reads it**; the model's own check at the end of a read
//!   (`Document::check_limits`) is the backstop that says the same in the same words, so no result tells the two apart, and a mutation that moves the early check
//!   by one master survived. The work the read had done when it refused tells them apart: a file of one master more than the most and a file of two more are
//!   refused at the same point.
//! * The themes: a theme is pushed with no count cap (only the file's size), where the VSDX reader keeps 256 (`vsdx::MAX_THEMES`) and the engine adds no more. A file
//!   of more is refused, before the theme is read.
//! * The page cells (`PageWidth` and the rest the page's layout says): the file's markup says them, and an entry of the document file that names one is ignored, so
//!   that an untouched save is no edit and the page size is the layout's.

use diacraft_odg::{Limits, read, read_partial};

use crate::docfile_cost::{file_of, package_with};

const MAX_MASTERS: usize = diacraft_doc::MAX_MASTERS;
/// The most themes a document file holds (the VSDX reader keeps the same number).
const MAX_THEMES: usize = 256;

fn masters(n: usize) -> String {
    file_of(&r#"<diacraft:master id="1"/>"#.repeat(n))
}

fn themes(n: usize) -> String {
    file_of(&r#"<diacraft:theme part="t">QUJD</diacraft:theme>"#.repeat(n))
}

/// The items a read had spent when it said no, **not counting the file's size** (charged once, at 64 bytes an item, before anything is read), and the class it said it in.
fn refused(file: &str) -> (u64, &'static str) {
    let (result, report) = read_partial(&package_with(file), &Limits::default());
    match result {
        Err(e) => (report.work.items - file.len().div_ceil(64) as u64, e.class()),
        Ok(_) => panic!("the file was read"),
    }
}

#[test]
fn the_most_masters_a_file_holds_are_read_and_the_next_is_refused_before_it_is_read() {
    let ok = read(&package_with(&masters(MAX_MASTERS)), &Limits::default());
    assert_eq!(ok.map(|(doc, _)| doc.masters.len()).ok(), Some(MAX_MASTERS), "the most are read");
    let (one_more, class) = refused(&masters(MAX_MASTERS + 1));
    assert_eq!(class, "xml_too_complex");
    // The master past the most is refused where it stands, so a file of two past it is refused at the same point: the work done is the same.
    let (two_more, _) = refused(&masters(MAX_MASTERS + 2));
    assert_eq!(one_more, two_more, "the read of a master past the most was done before it was refused");
}

#[test]
fn the_most_themes_a_file_holds_are_read_and_the_next_is_refused_before_it_is_read() {
    let ok = read(&package_with(&themes(MAX_THEMES)), &Limits::default());
    assert_eq!(ok.map(|(doc, _)| doc.themes.len()).ok(), Some(MAX_THEMES), "the most are read");
    let (one_more, class) = refused(&themes(MAX_THEMES + 1));
    assert_eq!(class, "xml_too_complex");
    let (two_more, _) = refused(&themes(MAX_THEMES + 2));
    assert_eq!(one_more, two_more, "a theme past the most is refused where it stands");
}

/// A page cell an entry of the document file names (the file's markup says it: `PageWidth` and the rest of the page's layout) is ignored: the page is the layout's, and an
/// untouched save is no edit (it reported an edit, dropped the file and reverted the cell). The cell said gone, and the same cell in the document's own sheet, too.
#[test]
fn a_page_cell_in_a_document_file_is_ignored_and_an_untouched_save_is_no_edit() {
    let without = read(&package_with(&file_of("")), &Limits::default()).unwrap().0;
    let layout_width = without.pages.get(0).and_then(|p| p.sheet.cell("PageWidth")).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
    for (what, body) in [
        ("a page cell", r#"<diacraft:page name="page1"><diacraft:sheet><diacraft:Cell N="PageWidth" V="9"/></diacraft:sheet></diacraft:page>"#),
        ("a page cell said gone", r#"<diacraft:page name="page1" gone="c.PageWidth c.PageHeight"/>"#),
        ("the document's cell", r#"<diacraft:docsheet><diacraft:sheet><diacraft:Cell N="PageWidth" V="9"/></diacraft:sheet></diacraft:docsheet>"#),
    ] {
        let bytes = package_with(&file_of(body));
        let (doc, report) = read(&bytes, &Limits::default()).unwrap();
        let width = doc.pages.get(0).and_then(|p| p.sheet.cell("PageWidth")).and_then(|c| c.value.as_ref()).and_then(|v| v.number());
        assert_eq!(width, Some(layout_width), "{what}: the page is the layout's");
        assert!(doc.sheet.cell("PageWidth").is_none(), "{what}: and the document's sheet has none");
        let saved = crate::write_core::real(&doc, &bytes, &report.index).unwrap();
        assert!(!saved.edited && saved.bytes == bytes, "{what}: an untouched save is no edit");
    }
    // A cell that is not a page cell is read, as it was.
    let bytes =
        package_with(&file_of(r#"<diacraft:page name="page1"><diacraft:sheet><diacraft:Cell N="Tag" V="t"/></diacraft:sheet></diacraft:page>"#));
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    assert!(doc.pages.get(0).and_then(|p| p.sheet.cell("Tag")).is_some(), "another cell of a page entry is read");
    let saved = crate::write_core::real(&doc, &bytes, &report.index).unwrap();
    assert!(!saved.edited && saved.bytes == bytes, "and an untouched save of it is no edit either");
}
