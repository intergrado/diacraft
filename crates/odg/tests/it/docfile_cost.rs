//! The review: what a crafted document file costs the reader.
//!
//! The reader puts each `page` and `docsheet` entry of the document file over the sheet it names (`payload::overlay`), and an overlay walks the sheet's sections (it
//! takes the gone ones out in one pass and indexes the keys of the rest). A page named by many entries is walked once for each: with one entry of 8,192 sections and
//! then many tiny entries of the same name, the reader did work in the square of what it counted, and a file inside every cap took minutes. Each overlay is charged
//! by the sections it walks now, before it walks them, so the work is counted and held to the budget of items.
//!
//! The file is built by hand, as a hostile file is: the package of a saved drawing with the document file put by another of `crafted`.

use diacraft_odg::{Limits, ReadReport, read};

use crate::drawing::Drawing;
use crate::write_core::custom;

const PATH: &str = "DiaCraft/document.xml";
const NS: &str = "urn:app.diacraft:extension";
/// The sections of the first entry: the most a sheet holds is `MAX_SECTIONS_PER_SHEET` (8,192).
const SECTIONS: usize = 8_192;

/// A package whose document file is `file`.
pub fn package_with(file: &str) -> Vec<u8> {
    let mut drawing = Drawing::a4().page(&custom("s1", "1cm"));
    drawing.files.push((PATH.to_owned(), file.as_bytes().to_vec()));
    drawing.bytes()
}

/// A document file: the root and the children `body` gives.
pub fn file_of(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><diacraft:document xmlns:diacraft="{NS}" version="2">{body}</diacraft:document>"#
    )
}

/// One entry for the page `page1` (or the document's sheet) holding `sections` sections named `S0` and up.
fn entry(element: &str, sections: usize) -> String {
    let name = if element == "page" { r#" name="page1""# } else { "" };
    let inner: String = (0..sections).map(|k| format!(r#"<diacraft:Section N="S{k}"/>"#)).collect();
    format!("<diacraft:{element}{name}><diacraft:sheet>{inner}</diacraft:sheet></diacraft:{element}>")
}

/// A file of the reviewer's shape: one entry of [`SECTIONS`] sections, then `tiny` entries of one section each (named `S0`, which the first holds).
fn shaped(element: &str, tiny: usize) -> String {
    let mut body = entry(element, SECTIONS);
    for _ in 0..tiny {
        body.push_str(&entry(element, 1));
    }
    file_of(&body)
}

pub fn read_items(file: &str, max_items: usize) -> Result<ReadReport, diacraft_odg::OdgError> {
    read(&package_with(file), &Limits { max_items, ..Limits::default() }).map(|(_, report)| report)
}

pub fn items(file: &str) -> u64 {
    read_items(file, Limits::default().max_items).unwrap_or_else(|e| panic!("the file does not read: {e}")).work.items
}

/// The shape the review found: each tiny entry of a page that holds 8,192 sections is charged what the walk of those sections costs, so that the items grow with
/// the number of entries in proportion to the sections they walk (and not by the few items an entry holds), and are linear in it.
#[test]
fn an_entry_of_a_page_with_many_sections_is_charged_the_sections_it_walks_and_the_cost_is_linear() {
    for element in ["page", "docsheet"] {
        let (a, b, c) = (items(&shaped(element, 25)), items(&shaped(element, 50)), items(&shaped(element, 100)));
        let (step_1, step_2) = (b - a, c - b);
        // Each of the extra entries walks the sections of the sheet at least once.
        assert!(step_1 >= 25 * SECTIONS as u64, "{element}: 25 more entries cost {step_1} items, less than the {} sections they walk", 25 * SECTIONS);
        assert!(step_2 >= 50 * SECTIONS as u64, "{element}: 50 more entries cost {step_2} items, less than the {} sections they walk", 50 * SECTIONS);
        // And linear: twice the entries cost twice, within what an entry holds besides the walk.
        assert!(step_2 <= 2 * step_1 + 64 && 2 * step_1 <= step_2 + 64, "{element}: {step_1} items for 25 entries and {step_2} for 50: not linear");
    }
}

/// The charge holds the file to the budget: the file of the review's shape reads in a budget of a million items with the first entry alone, and is refused as too
/// many items with 300 tiny ones after it (they walk 2.4 million sections, which the charge takes before the walk).
#[test]
fn many_entries_over_a_page_with_many_sections_are_refused_by_the_budget_of_items() {
    const BUDGET: usize = 1_000_000;
    for element in ["page", "docsheet"] {
        assert!(read_items(&shaped(element, 0), BUDGET).is_ok(), "{element}: the first entry alone reads in {BUDGET} items");
        match read_items(&shaped(element, 300), BUDGET) {
            Err(e) => assert_eq!(e.class(), "too_many_items", "{element}: {e}"),
            Ok(_) => panic!("{element}: 300 entries over {SECTIONS} sections went through a budget of {BUDGET} items"),
        }
    }
}

/// The sections that are gone are walked too (the `gone` token of a section takes them out in one pass): an entry that only names one is charged the walk.
#[test]
fn an_entry_that_names_a_gone_section_is_charged_the_sections_it_walks() {
    let gone = |element: &str| {
        let name = if element == "page" { r#" name="page1""# } else { "" };
        format!(r#"<diacraft:{element}{name} gone="s.Zz._"/>"#)
    };
    for element in ["page", "docsheet"] {
        let build = |n: usize| file_of(&format!("{}{}", entry(element, SECTIONS), gone(element).repeat(n)));
        let (a, b) = (items(&build(20)), items(&build(40)));
        assert!(b - a >= 20 * SECTIONS as u64, "{element}: 20 more entries cost {} items, less than the {} sections they walk", b - a, 20 * SECTIONS);
    }
}
