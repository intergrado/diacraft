//! A group with no name, saved twice. The fuzz target of the writer found that a package with a group that has no name and a glue point of its own, a shape of
//! the page put last, and the save written as a flat document, was refused on the second save through the index the first one gave: `FlatGroup`, "has data of its own, and a flat
//! file keeps none of it". A Save after a Save As was refused in words. The second save is of the same model through the file the first one wrote, so it holds nothing the file
//! lacks, and is no edit.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_odg::{Form, Limits, OriginalPackage, WriteError, WriteOptions, Written, read, write_with};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::write_core::{Open, num_of, open_bytes, set_num, shape_mut};

const GR1: &str =
    r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;

/// A group with `name` (an attribute, or none), a style, a glue point of its own and two rectangles.
fn group(name: &str) -> String {
    format!(
        concat!(
            r#"<draw:g{name} draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
        ),
        name = if name.is_empty() { String::new() } else { format!(" draw:name=\"{name}\"") }
    )
}

/// Two rectangles, then the group.
fn page(name: &str) -> String {
    format!(
        concat!(
            r#"<draw:rect draw:style-name="gr1" draw:id="s1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
            r#"<draw:rect draw:style-name="gr1" draw:id="s2" svg:x="8cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>{group}"#
        ),
        group = group(name)
    )
}

/// The drawing of `page`, with the automatic style the group and the rectangles name or without it.
fn drawing(page: &str, styled: bool) -> Drawing {
    let d = Drawing::a4();
    (if styled { d.auto(GR1) } else { d }).page(page)
}

/// The first shape of the first page put last.
fn reorder(doc: &mut Document) {
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    if !page.shapes.is_empty() {
        let first = page.shapes.remove(0);
        page.shapes.push(first.expect("a shape"));
    }
}

/// The group of the first page moved a little to the right.
fn move_group(doc: &mut Document) {
    let id = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|s| !s.children.is_empty()).map(|s| s.id)).expect("the group");
    let x = num_of(shape_mut(doc, 0, id), "PinX");
    set_num(shape_mut(doc, 0, id), "PinX", x + 12.0 / 7.0);
}

fn save(doc: &Document, from: (&[u8], &diacraft_odg::ShapeIndex), form: Option<Form>) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes: from.0, index: from.1 }), &WriteOptions { form, ..WriteOptions::default() })
}

/// How many members each shape of the first page holds.
fn members(doc: &Document) -> Vec<usize> {
    doc.pages.get(0).map(|p| p.shapes.iter().map(|s| s.children.len()).collect()).unwrap_or_default()
}

/// Saves `doc` from `open`, then again and again from what each save gave: each is written, and the second and the third are no edit, the third the second's bytes.
fn saved_three_times(doc: &Document, open: &Open, form: Option<Form>, what: &str) {
    let one = save(doc, (&open.bytes, &open.index), form).unwrap_or_else(|e| panic!("{what}: the first save: {e}"));
    let two = save(doc, (&one.bytes, &one.index), form).unwrap_or_else(|e| panic!("{what}: a second save of a saved model: {e}"));
    assert!(!two.edited, "{what}: a second save with nothing changed is an edit");
    assert_eq!(two.bytes, one.bytes, "{what}: the second save gives the first's bytes");
    let three = save(doc, (&two.bytes, &two.index), form).unwrap_or_else(|e| panic!("{what}: a third save of a saved model: {e}"));
    assert!(!three.edited && three.bytes == two.bytes, "{what}: a third save with nothing changed is an edit");
    let (back, _) = read(&three.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{what}: the saved file does not read: {e}"));
    assert_eq!(members(&back), members(doc), "{what}: the groups read back as the model has them");
}

/// What a person does after a Save As: a setting of the document is changed (DiaCraft's own data of the document, which a package keeps in its document file and a flat drawing in its one
/// element) and the drawing is saved again. The save is written, and the group is as it was.
fn saved_after_a_setting(doc: &Document, open: &Open, form: Option<Form>, what: &str) {
    let one = save(doc, (&open.bytes, &open.index), form).unwrap_or_else(|e| panic!("{what}: the first save: {e}"));
    let mut doc = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("flags", "6")])), "{what}: the data takes the setting");
    let two = save(&doc, (&one.bytes, &one.index), form).unwrap_or_else(|e| panic!("{what}: a save after a setting changed: {e}"));
    assert!(two.edited, "{what}: the setting is written");
    let (back, _) = read(&two.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{what}: the saved file does not read: {e}"));
    assert_eq!(back.diacraft, doc.diacraft, "{what}: the setting reads back");
    assert_eq!(members(&back), members(&doc), "{what}: the groups read back as the model has them");
    let three = save(&doc, (&two.bytes, &two.index), form).unwrap_or_else(|e| panic!("{what}: a save again: {e}"));
    assert!(!three.edited && three.bytes == two.bytes, "{what}: a save with nothing changed is an edit");
}

/// **The vector**: a group with no name (and one with a name), with a style the automatic styles hold and one they lack, in a package, in a flat drawing and in a package saved
/// as a flat document, after the first shape is put last and after the group is moved: every save is written, and each save after the first is no edit.
#[test]
fn a_group_with_no_name_is_saved_again_in_every_form() {
    for styled in [true, false] {
        for name in ["", "Pump"] {
            let d = drawing(&page(name), styled);
            let opens = [
                (open_bytes(d.bytes()), None, "a package"),
                (open_bytes(Flat::of(&d).bytes()), None, "a flat drawing"),
                (open_bytes(d.bytes()), Some(Form::Flat), "a package saved as a flat document"),
            ];
            for (open, form, what) in opens {
                let what = format!("{what}, the group {name:?}, styled {styled}");
                let mut doc = open.doc.clone();
                reorder(&mut doc);
                saved_three_times(&doc, &open, form, &format!("{what}, a shape put last"));
                let mut doc = open.doc.clone();
                move_group(&mut doc);
                saved_three_times(&doc, &open, form, &format!("{what}, the group moved"));
            }
        }
    }
}

/// A setting of the document changed between two saves, after a shape of the page was put last: the second save is written in every form, whether the group has a name or none.
#[test]
fn a_setting_changed_after_a_save_is_written_with_a_group_with_no_name() {
    for styled in [true, false] {
        for name in ["", "Pump"] {
            let d = drawing(&page(name), styled);
            let opens = [
                (open_bytes(d.bytes()), None, "a package"),
                (open_bytes(Flat::of(&d).bytes()), None, "a flat drawing"),
                (open_bytes(d.bytes()), Some(Form::Flat), "a package saved as a flat document"),
            ];
            for (open, form, what) in opens {
                let what = format!("{what}, the group {name:?}, styled {styled}");
                let mut doc = open.doc.clone();
                reorder(&mut doc);
                saved_after_a_setting(&doc, &open, form, &format!("{what}, a shape put last"));
            }
        }
    }
}
