//! The user's path to the refusal the `odg_write` fuzz target found ("a second save of a saved model", `FlatGroup`). A drawing that has a group with no name, whose data is
//! what its markup gives it (its own graphic style and a glue point of its own), has a shape put last in the window and is saved; the next save, with another change, was refused in
//! words that tell the person to put the group's data on a shape beside it or to save a `.diacraft` file, though the person never gave the group any data. The file the first save wrote
//! numbers its shapes in their new order, so the group has another `ID` when the file is read again, and the writer looked for the group's entry in the file by the `ID` the session had.

use serde_json::json;

use crate::odg_build::{Dir, Drawing, id_of, open_bytes_in};
use crate::odg_file::{save, shift};
use crate::run;

/// A rectangle named R, then a group with no name, no `draw:id`, its own style and its own glue point, holding two rectangles.
fn with_a_group() -> Drawing {
    Drawing::new().page(concat!(
        r#"<draw:rect draw:style-name="gr1" draw:name="R" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:g draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
    ))
}

/// What the file is (a package or a flat drawing), with the bytes that open it.
fn forms() -> [(&'static str, Vec<u8>); 2] {
    [("a.odg", with_a_group().bytes()), ("a.fodg", with_a_group().flat_bytes())]
}

/// R put last in the drawing order: the group is the first shape of the page now, as the file the next save writes has it.
fn put_last(s: &mut diacraft_engine::Session) {
    let id = id_of(s.document().expect("a document"), 0, "R");
    run(s, "shape.order", json!({ "ids": [id], "to": "front" }));
}

/// Open, put the shape before the group last, save, move that shape, save again, and save with nothing changed: every save is written, and the last is no edit.
#[test]
fn a_shape_put_last_and_moved_after_a_save_is_saved_again_with_a_group_with_no_name() {
    for (name, bytes) in forms() {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, name, &bytes);
        put_last(&mut s);
        let first = save(&mut s);
        assert_eq!(first["edited"], true, "{name}: {first}");
        shift(&mut s, "R", 0.5, 0.0);
        match s.execute("file.save", &json!({})) {
            Ok(second) => assert_eq!(second["edited"], true, "{name}: {second}"),
            Err(e) => panic!("{name}: the second save was refused: {e}"),
        }
        let again = save(&mut s);
        assert_eq!(again["edited"], false, "{name}: a save with nothing changed is no edit: {again}");
    }
}

/// The same, with a change that a package keeps in DiaCraft's document file and a flat drawing in its one element (a setting of the document) in place of the move: the save after it
/// is written.
#[test]
fn a_setting_changed_after_a_save_is_written_with_a_group_with_no_name_after_a_shape_was_put_last() {
    for (name, bytes) in forms() {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, name, &bytes);
        put_last(&mut s);
        let first = save(&mut s);
        assert_eq!(first["edited"], true, "{name}: {first}");
        s.edit("Test setup: a setting of the document", |doc: &mut diacraft_doc::Document| {
            doc.set_diacraft_child("snap", Some(&[("flags", "6")]));
            Ok(())
        })
        .expect("the setting is set");
        match s.execute("file.save", &json!({})) {
            Ok(second) => assert_eq!(second["edited"], true, "{name}: {second}"),
            Err(e) => panic!("{name}: the save after the setting changed was refused: {e}"),
        }
        let again = save(&mut s);
        assert_eq!(again["edited"], false, "{name}: a save with nothing changed is no edit: {again}");
    }
}
