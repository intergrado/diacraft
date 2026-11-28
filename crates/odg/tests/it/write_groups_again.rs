//! A group of the file whose data is what its markup gives it (a graphic style and a glue point of its own) is moved and saved, and the model saved again from the
//! saved bytes and the index the first save gave is no edit. The `odg_write` fuzz target found it with the six bytes `0a 0a 50 50 54 54`: a template of ODF 1.0 whose page body is the
//! seed shapes (among them a group with no name, one automatic style and one glue point) and the seed text, and one edit, the first page's shape 80 modulo
//! its count (the group) moved by 12/7 inch in `PinX`.
//!
//! The cause: the group's entry for DiaCraft's document file holds the four side-middle `Connection` rows beside the glue point of its own, and the cached value of each (`Width*0.5`) is
//! a number that a box makes. A move writes the members again, the box of the file read again is a few units in the last place from the box the session kept, and the entry of the model
//! was no longer the entry of the file read again, so the writer took the group for one that was given data, and refused to key it by its place. A number of a cell is the same up to
//! the rounding the lengths of a save are held to (`write::lengths::same`).

use std::sync::Arc;

use diacraft_doc::{Document, FormulaSlot, Value};
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, num_of, open, real, set_num, shape_mut};

/// The automatic style the group and its members name (a solid red fill: the model holds it as cells of the group).
const GR1: &str =
    r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;

/// A group with `name` (an attribute, or none) with its own style and a glue point of its own, holding two rectangles at `y` centimetres (the seed shapes' group).
fn group(name: &str, y: u32) -> String {
    format!(
        concat!(
            r#"<draw:g{name} draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
        ),
        name = name,
        y = y
    )
}

/// A group with no name that has only the one thing its markup can give it: its own style (`style`), or a glue point of its own (`glue`), or nothing.
fn bare_group(style: bool, glue: bool, y: u32) -> String {
    let style = if style { r#" draw:style-name="gr1""# } else { "" };
    let glue = if glue { r#"<draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"# } else { "" };
    format!(
        concat!(
            r#"<draw:g{style}>{glue}<draw:rect svg:x="1cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/>"#,
            r#"<draw:rect svg:x="4cm" svg:y="{y}cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
        ),
        style = style,
        glue = glue,
        y = y
    )
}

/// The ids of the groups of the first page, in order.
fn groups(doc: &Document) -> Vec<u32> {
    doc.pages.get(0).map(|p| p.shapes.iter().filter(|s| !s.children.is_empty()).map(|s| s.id).collect()).unwrap_or_default()
}

/// Moves the group `id` by 12/7 inch in `PinX`, as the fuzz target's op did (a move of the cell, which a move in the window makes too).
fn nudge(doc: &mut Document, id: u32) {
    let shape = shape_mut(doc, 0, id);
    let x = num_of(shape, "PinX");
    set_num(shape, "PinX", x + 12.0 / 7.0);
}

/// Saves `o`, and again from the saved bytes and the index: each must be saved, and the second is no edit and the same bytes, and so is the third.
fn saved_twice(o: &Open) -> diacraft_odg::Written {
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the first save: {e}"));
    let again = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("the second save of a saved model: {e}"));
    assert!(!again.edited, "a second save with nothing changed is an edit");
    assert_eq!(again.bytes, first.bytes);
    let third = real(&o.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("the third save of a saved model: {e}"));
    assert!(!third.edited && third.bytes == again.bytes, "a third save with nothing changed is an edit");
    first
}

/// The fuzz target's vector: a group with no name, moved, saved, and saved again.
#[test]
fn a_moved_group_with_no_name_saves_and_saves_again_with_no_edit() {
    let mut o = open(&Drawing::a4().auto(GR1).page(&group("", 5)));
    let id = *groups(&o.doc).first().expect("the group");
    nudge(&mut o.doc, id);
    saved_twice(&o);
}

/// The same group with a name of its own that is unique (its entry has a key, and is written to the document file), and a group in a group with no name each.
#[test]
fn a_moved_group_with_a_name_or_one_in_a_group_saves_and_saves_again_with_no_edit() {
    let mut o = open(&Drawing::a4().auto(GR1).page(&group(r#" draw:name="Named""#, 5)));
    let id = *groups(&o.doc).first().expect("the group");
    nudge(&mut o.doc, id);
    saved_twice(&o);
    let nested = format!("<draw:g>{}{}</draw:g>", group("", 5), group("", 9));
    let mut o = open(&Drawing::a4().auto(GR1).page(&nested));
    for id in groups(&o.doc) {
        nudge(&mut o.doc, id);
    }
    saved_twice(&o);
}

/// A group with its own style alone, one with a glue point alone and one with neither (which holds no data and was never refused): each is moved and saved twice.
#[test]
fn a_moved_group_with_only_a_style_or_only_a_glue_point_or_neither_saves_and_saves_again() {
    for (style, glue) in [(true, false), (false, true), (false, false)] {
        let mut o = open(&Drawing::a4().auto(GR1).page(&bare_group(style, glue, 5)));
        let id = *groups(&o.doc).first().expect("the group");
        nudge(&mut o.doc, id);
        let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("style {style}, glue {glue}: the first save: {e}"));
        let again = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("style {style}, glue {glue}: the second save: {e}"));
        assert!(!again.edited && again.bytes == first.bytes, "style {style}, glue {glue}: a second save with nothing changed is an edit");
    }
}

/// Several groups with no name, all moved, and one of them again after the first save: the second save writes that move and is no refusal, and a save after it is no edit.
#[test]
fn groups_with_no_name_moved_in_one_save_and_another_after_it_are_saved() {
    let page: String = (0..4).map(|k| group("", 2 + 4 * k)).collect();
    let mut o = open(&Drawing::a4().auto(GR1).page(&page));
    let ids = groups(&o.doc);
    assert_eq!(ids.len(), 4, "{ids:?}");
    nudge(&mut o.doc, ids[0]);
    nudge(&mut o.doc, ids[2]);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the first save: {e}"));
    nudge(&mut o.doc, ids[3]);
    let second = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("the second save, after another move: {e}"));
    assert!(second.edited, "the move after the first save is an edit");
    let third = real(&o.doc, &second.bytes, &second.index).unwrap_or_else(|e| panic!("the third save: {e}"));
    assert!(!third.edited && third.bytes == second.bytes, "a save with nothing changed is an edit");
}

/// **What is refused stays refused**: a cell given to the group, and a place of a glue point that is not rounding, make the entry another, so the unnamed group is refused in words;
/// a number that differs by the rounding alone is the same, and so is the value of a cell of a formula, which the box makes. (The words are those of the writer's own refusal of data given to a group
/// that has no key.)
#[test]
fn data_given_to_a_group_with_no_name_is_still_refused_and_rounding_is_not_data() {
    let refused = |change: &dyn Fn(&mut Document, u32)| -> Result<(), String> {
        let mut o = open(&Drawing::a4().auto(GR1).page(&group("", 5)));
        let id = *groups(&o.doc).first().expect("the group");
        change(&mut o.doc, id);
        real(&o.doc, &o.bytes, &o.index).map(|_| ()).map_err(|e| e.to_string())
    };
    // A cell no markup gives.
    let said = refused(&|doc, id| {
        let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
        sheet.cells.insert(diacraft_doc::CellName::new("Cost"), diacraft_doc::Cell::constant(Value::from_text("3")));
    })
    .unwrap_err();
    assert!(said.contains("a group with no name that is unique among the page's groups"), "{said}");
    // The `X` of the `Connection` rows that have a formula (`Width*0.5`: a value the box makes) or none (a place the markup gives), each moved by `by` inches.
    let moved = |by: f64, formula: bool| {
        move |doc: &mut Document, id: u32| {
            let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
            for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "Connection") {
                let section = Arc::make_mut(section);
                for at in 0..section.rows.len() {
                    let Some(cell) = section.rows.get_mut(at).map(Arc::make_mut).and_then(|row| row.cells.get_mut("X")) else { continue };
                    if matches!(cell.formula, FormulaSlot::Absent) == formula {
                        continue;
                    }
                    let was = cell.value.as_ref().and_then(Value::number).expect("a number");
                    cell.value = Value::from_number(was + by);
                }
            }
        }
    };
    // A place the markup gives, moved by a hundredth of an inch, is data of the group's own; moved by less than any rounding of a save it is the same place.
    let said = refused(&moved(0.01, false)).unwrap_err();
    assert!(said.contains("a group with no name that is unique among the page's groups"), "{said}");
    assert_eq!(refused(&moved(1e-15, false)), Ok(()), "a number that differs in its last places is the same number");
    // A value of a cell that holds its formula is what the box makes: the formula is the cell (a resize of the group, which the engine's test makes, changes it by more than any rounding).
    assert_eq!(refused(&moved(0.01, true)), Ok(()), "the value of a cell of a formula follows the cells the formula names");
}

/// The page of `n` groups with no name, each with its own style and glue point, every one moved: the work of the second save, which finds each entry the same as the file's, follows
/// the number of groups (counted, never timed), and each group is a step or a few.
fn second_save_work(n: u32) -> WriteWork {
    let page: String = (0..n).map(|k| group("", 2 + k % 20)).collect();
    let mut o = open(&Drawing::a4().auto(GR1).page(&page));
    for id in groups(&o.doc) {
        nudge(&mut o.doc, id);
    }
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{n} groups, the first save: {e}"));
    let mut work = WriteWork::default();
    let options = WriteOptions::default();
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &first.bytes, index: &first.index }), &options, &mut work)
        .unwrap_or_else(|e| panic!("{n} groups, the second save of a saved model: {e}"));
    work
}

/// Four times the groups is at most about five times the work of the second save, and a hundred groups are saved a second time (the comparison of an entry with the file's is charged
/// to the save's steps, so that a page of thousands of groups is refused in words and never walked without end).
#[test]
fn the_second_save_of_many_moved_groups_costs_work_in_proportion_to_their_number() {
    let (small, big) = (second_save_work(100), second_save_work(400));
    assert!(small.cells_compared > 0, "the groups' entries are compared");
    assert!(
        big.cells_compared <= small.cells_compared * 5,
        "the steps of 400 groups ({}) are more than five times those of 100 ({})",
        big.cells_compared,
        small.cells_compared
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------- GATEBAE6-GROUP-ALTTEXT

/// A group with no name: its title and description first (as the schema places them), then two rectangles. With `name`, a group of a name of its own.
fn titled_group(name: &str, title: &str) -> String {
    format!(
        concat!(
            r#"<draw:g{name}><svg:title>{title}</svg:title><svg:desc>Two boxes</svg:desc>"#,
            r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/><draw:rect svg:x="6cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/></draw:g>"#
        ),
        name = name,
        title = title
    )
}

/// The texts of the rows `visAltText` and `visAltDescription` the group `id` holds.
fn alt_texts(doc: &Document, id: u32) -> Vec<String> {
    let shape = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|s| s.id == id)).expect("the group");
    let rows = shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).map(|s| s.rows.clone());
    rows.iter().flat_map(|r| r.iter()).filter_map(|r| diacraft_doc::alt_text::standard_text(r).map(str::to_owned)).collect()
}

/// Gives the group `id` the title `title` and the description "Two boxes", as rows of its `User` section (what `shape.alt_text` does).
fn retitle(doc: &mut Document, id: u32, title: &str) {
    let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
    for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "User") {
        let rows = [("visAltText", title), ("visAltDescription", "Two boxes")];
        Arc::make_mut(section).rows = rows.iter().map(|(n, t)| Arc::new(diacraft_doc::alt_text::standard_row(n, t))).collect();
    }
}

/// **The gate's vector, written from the model alone** (a visit through `.diacraft` writes an `.odg` so): a group of the file with no name, a title and a description, is written, and the
/// file read again holds both. Its entry is none: `svg:title` and `svg:desc` say what its rows say (they were refused: the rows were taken for data given to a group with no key).
/// What stays refused: a title with a cell of the group's own beside it, since the entry would hold the whole section.
#[test]
fn an_unnamed_group_with_a_title_and_a_description_is_written_from_the_model_and_keeps_them() {
    let o = open(&Drawing::a4().page(&titled_group("", "Pump group")));
    let id = *groups(&o.doc).first().expect("the group");
    assert_eq!(alt_texts(&o.doc, id), ["Pump group", "Two boxes"]);
    let saved = diacraft_odg::write_with(&o.doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the model was not written: {e}"));
    let content = crate::write_core::part(&saved.bytes, "content.xml");
    assert!(content.contains("<svg:title>Pump group</svg:title>") && content.contains("<svg:desc>Two boxes</svg:desc>"), "{content}");
    let back = crate::write_core::open_bytes(saved.bytes.clone());
    assert_eq!(alt_texts(&back.doc, *groups(&back.doc).first().expect("the group")), ["Pump group", "Two boxes"]);
    // The title and a cell of the group's own: the data has no key.
    let mut with_data = open(&Drawing::a4().page(&titled_group("", "Pump group")));
    let id = *groups(&with_data.doc).first().expect("the group");
    Arc::make_mut(&mut shape_mut(&mut with_data.doc, 0, id).sheet)
        .cells
        .insert(diacraft_doc::CellName::new("Cost"), diacraft_doc::Cell::constant(Value::from_text("3")));
    let said = diacraft_odg::write_with(&with_data.doc, None, &WriteOptions::default()).map(|_| ()).unwrap_err().to_string();
    assert!(said.contains("a group with no name that is unique among the page's groups"), "{said}");
}

/// The controls of the vector: a group with a name of its own saves with its title, and the title changed in the session (`shape.alt_text`) of a group of the file that has no name is never
/// left out of the file without a word.
#[test]
fn a_title_changed_on_a_group_with_no_name_is_never_left_out_of_the_file_and_the_control_saves() {
    // A group with a name saves with its title (its entry carries the rows beside the markup).
    let o = open(&Drawing::a4().page(&titled_group(r#" draw:name="Pump""#, "Pump group")));
    let saved = diacraft_odg::write_with(&o.doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the group with a name: {e}"));
    assert!(crate::write_core::part(&saved.bytes, "content.xml").contains("<svg:title>Pump group</svg:title>"));
    // A group with no name whose title is changed, saved into the file it came from: the splice leaves the elements of a group as they are, so the new title is never left out without a word:
    // the save is refused in words, or writes it.
    let mut o = open(&Drawing::a4().page(&titled_group("", "Pump group")));
    let id = *groups(&o.doc).first().expect("the group");
    retitle(&mut o.doc, id, "Other pump");
    match real(&o.doc, &o.bytes, &o.index) {
        Ok(saved) => assert!(
            crate::write_core::part(&saved.bytes, "content.xml").contains("<svg:title>Other pump</svg:title>"),
            "the new title was left out of the file"
        ),
        Err(e) => assert!(e.to_string().contains("a group with no name that is unique among the page's groups"), "{e}"),
    }
}
