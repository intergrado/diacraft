//! A flat drawing's save and the data of a group. A package keeps a group's own data (a cell beyond its frame, a section) in DiaCraft's document file, in a `group` entry; a flat
//! document has no such file (`specs` gives it one element for DiaCraft's own data and no more), and the flat writer compared the group's data with nothing and wrote none of
//! it: an edit of it was **no edit**, and the save was the file's own bytes. The specification asks the other thing of it (and the project rule of its item: a change to what a package
//! keeps in the document file is refused in words, never dropped), and so a flat save of a group that holds data of its own is refused, with nothing written. What the group's own markup gives it (a
//! graphic style, glue points, a title and a description) is no data of its own, and is saved with the group moved or resized as before.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Value};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::flat_write::save;
use crate::write_core::{Open, num_of, open_bytes, set_num, shape_mut};

/// The automatic style the group and its members name (a solid red fill: the model holds it as cells of the group).
const GR1: &str =
    r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;

/// A group with `name` (an attribute, or none), its own style and a glue point of its own, holding two rectangles at `y` centimetres; `inside` goes before its members.
fn group(name: &str, inside: &str, y: u32) -> String {
    format!(
        concat!(
            r#"<draw:g{name} draw:style-name="gr1">{inside}<draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
        ),
        name = if name.is_empty() { String::new() } else { format!(" draw:name=\"{name}\"") },
        inside = inside,
        y = y
    )
}

/// The flat drawing of `page`, read.
fn flat(page: &str) -> Open {
    open_bytes(Flat::of(&Drawing::a4().auto(GR1).page(page)).bytes())
}

/// The ids of the groups of the first page, in order.
fn groups(doc: &Document) -> Vec<u32> {
    doc.pages.get(0).map(|p| p.shapes.iter().filter(|s| !s.children.is_empty()).map(|s| s.id).collect()).unwrap_or_default()
}

/// What `edit` is, to give the group `id`: a cell no markup gives, a `User` row, a `Property` row.
fn give(doc: &mut Document, id: u32, kind: &str) {
    let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
    match kind {
        "cell" => {
            sheet.cells.insert(CellName::new("Cost"), Cell::constant(Value::from_text("3")));
        }
        section => {
            let mut made = Section::new(section, None);
            made.rows.push(Arc::new(Row { name: Some(Arc::from("Row_1")), ..Row::default() }));
            sheet.sections.push(Arc::new(made));
        }
    }
}

const KINDS: [&str; 3] = ["cell", "User", "Prop"];

/// **The vector**: data given to a group of a flat drawing, a group with a name and one with none, is refused in words naming the group; it was saved as no edit, the file the same bytes.
#[test]
fn data_given_to_a_group_of_a_flat_drawing_is_refused_in_words_and_never_dropped() {
    for (name, who) in [("", "the group with no name numbered 1 on the page \"page1\""), ("Pump", "the group \"Pump\" on the page \"page1\"")] {
        for kind in KINDS {
            let mut o = flat(&group(name, "", 5));
            let id = *groups(&o.doc).first().expect("the group");
            give(&mut o.doc, id, kind);
            let said = save(&o.doc, &o.bytes, &o.index).map(|w| format!("saved, edited: {}", w.edited)).unwrap_err().to_string();
            assert!(said.starts_with(&format!("{who} has data of its own")), "{who}, {kind}: {said}");
            assert!(said.contains("a flat file keeps none of it"), "{who}, {kind}: {said}");
        }
    }
}

/// What the group's markup gives it is no data of its own, and a title and a description it has (`svg:title`, `svg:desc`: the rows of its alt text) are among it; a group moved, resized or
/// retitled by nobody is saved with the move, and saved again as no edit.
#[test]
fn what_the_markup_gives_a_group_of_a_flat_drawing_is_no_data_and_a_moved_group_is_saved() {
    let titled = "<svg:title>Pump group</svg:title><svg:desc>Two boxes</svg:desc>";
    for (name, inside) in [("", ""), ("Pump", ""), ("", titled), ("Pump", titled)] {
        let mut o = flat(&group(name, inside, 5));
        let id = *groups(&o.doc).first().expect("the group");
        let x = num_of(shape_mut(&mut o.doc, 0, id), "PinX");
        set_num(shape_mut(&mut o.doc, 0, id), "PinX", x + 12.0 / 7.0);
        let first = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{name:?} {inside:?}: the first save: {e}"));
        assert!(first.edited, "{name:?} {inside:?}: the move is written");
        let again = save(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("{name:?} {inside:?}: the second save: {e}"));
        assert!(!again.edited && again.bytes == first.bytes, "{name:?} {inside:?}: a save with nothing changed is no edit");
    }
}

/// A title the group has, changed, is data of the group: refused. (The title the markup gave, left as it was, is not.)
#[test]
fn a_title_a_flat_group_has_is_refused_when_it_is_changed() {
    let titled = "<svg:title>Pump group</svg:title><svg:desc>Two boxes</svg:desc>";
    let mut o = flat(&group("Pump", titled, 5));
    let id = *groups(&o.doc).first().expect("the group");
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let user = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").expect("the title's section");
    let row = Arc::make_mut(Arc::make_mut(user).rows.get_mut(0).expect("a row"));
    let cell = row.cells.get_mut("Value").expect("the title's text");
    cell.value = Some(Value::from_text("Another title"));
    let said = save(&o.doc, &o.bytes, &o.index).map(|w| format!("saved, edited: {}", w.edited)).unwrap_err().to_string();
    assert!(said.contains("has data of its own"), "{said}");
}

/// The error says where the group is: the page, the group's own name and its number among the groups of that name (the number is what a group with no name has to be found by).
#[test]
fn the_error_names_the_page_the_group_and_its_number() {
    let fields = |page: &str, doc_edit: &dyn Fn(&mut Document, &[u32])| {
        let mut o = flat(page);
        let ids = groups(&o.doc);
        doc_edit(&mut o.doc, &ids);
        match save(&o.doc, &o.bytes, &o.index).map(|_| ()).unwrap_err() {
            WriteError::FlatGroup { page, page_nth, name, nth } => {
                let e = WriteError::FlatGroup { page: page.clone(), page_nth, name: name.clone(), nth };
                assert_eq!(e.class(), "write_unsupported");
                (page, page_nth, name, nth, e.to_string())
            }
            other => panic!("another error: {other:?}"),
        }
    };
    // Two groups with no name, the second given data: it is the second of the groups with no name.
    let two = format!("{}{}", group("", "", 3), group("", "", 8));
    let (page, page_nth, name, nth, said) = fields(&two, &|doc, ids| give(doc, ids[1], "User"));
    assert_eq!((page.as_str(), page_nth, name.as_str(), nth), ("page1", 1, "", 2), "{said}");
    assert!(said.starts_with("the group with no name numbered 2 on the page \"page1\" has data of its own"), "{said}");
    // Two groups of one name (neither has a key of its own), the second given data.
    let same = format!("{}{}", group("Pump", "", 3), group("Pump", "", 8));
    let (_, _, name, nth, said) = fields(&same, &|doc, ids| give(doc, ids[1], "Prop"));
    assert_eq!((name.as_str(), nth), ("Pump", 2), "{said}");
    assert!(said.starts_with("the group \"Pump\" numbered 2 of that name on the page \"page1\" has data of its own"), "{said}");
    // The first of two groups of a name is not numbered in the words.
    let (_, _, name, nth, said) = fields(&same, &|doc, ids| give(doc, ids[0], "cell"));
    assert_eq!((name.as_str(), nth), ("Pump", 1), "{said}");
    assert!(said.starts_with("the group \"Pump\" on the page \"page1\" has data of its own"), "{said}");
}

/// A page's name that is as long as a file can make it, and holds a line feed, is cut and escaped, and the sentence is whole.
#[test]
fn the_names_in_the_words_are_cut_and_escaped() {
    let long = "p".repeat(5_000);
    let flat = Flat::of(&Drawing::a4().auto(GR1).page(&group("Pump", "", 5)))
        .text()
        .replace("draw:name=\"page1\"", &format!("draw:name=\"{long}&#10;[2J\""));
    let mut o = open_bytes(flat.into_bytes());
    let id = *groups(&o.doc).first().expect("the group");
    give(&mut o.doc, id, "User");
    let said = save(&o.doc, &o.bytes, &o.index).map(|w| format!("saved, edited: {}", w.edited)).unwrap_err().to_string();
    assert!(said.chars().count() < 300 && !said.contains('\n') && !said.contains(&long), "{} characters: {said}", said.chars().count());
    assert!(said.ends_with("which holds it"), "{said}");
}

/// The steps a flat save takes to find a group's data are counted in the save's own, in proportion to the groups, and a save with a cap too low for them is refused in words: it never
/// walks without end (counted, never timed).
fn work_of(groups_on_page: u32, cap: Option<u64>) -> (WriteWork, Result<(), WriteError>) {
    let page: String = (0..groups_on_page).map(|k| group("", "", 2 + k % 20)).collect();
    let o = flat(&page);
    let mut options = WriteOptions::default();
    if let Some(cap) = cap {
        options = WriteOptions { limits: Limits { max_write_steps: cap, ..Limits::default() }, ..options };
    }
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work).map(|_| ());
    (work, saved)
}

#[test]
fn the_search_for_the_data_of_the_groups_of_a_flat_save_is_counted_and_held_to_the_cap() {
    let (small, big) = (work_of(100, None), work_of(400, None));
    assert!(small.1.is_ok() && big.1.is_ok(), "{:?} {:?}", small.1, big.1);
    assert!(small.0.cells_compared > 100, "the groups' entries are compared: {}", small.0.cells_compared);
    assert!(
        big.0.cells_compared <= small.0.cells_compared * 5,
        "the steps of 400 groups ({}) are more than five times those of 100 ({})",
        big.0.cells_compared,
        small.0.cells_compared
    );
    // A cap below what the 400 groups need: refused in words, and the steps counted stop within a step of the cap.
    let cap = small.0.steps_used();
    let (work, refused) = work_of(400, Some(cap));
    let said = refused.unwrap_err().to_string();
    assert!(said.contains("steps"), "{said}");
    assert!(work.steps_used() <= cap.saturating_mul(2), "the walk went on to {} steps against a cap of {cap}", work.steps_used());
}
