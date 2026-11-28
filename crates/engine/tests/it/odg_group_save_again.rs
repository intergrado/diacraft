//! The user's path to the refusal the `odg_write` fuzz target found with the six bytes `0a 0a 50 50 54 54`. A group of an `.odg` with no name, whose data is what its
//! markup gives it (its own graphic style and a glue point of its own), is moved in the window and saved, and the save after it was refused in words that tell the person to name the
//! group, though the person never gave the group any data: the saved file reads back with cells a few units in the last place from the session's, and the writer took the difference
//! for data given to a group it cannot key.

use serde_json::json;

use crate::odg_build::{Dir, Drawing, open_drawing, read_back};
use crate::odg_file::{save, shift};
use crate::run;

/// The page the fuzz target's seed shapes give: a rectangle, and a group with no name, no `draw:id`, its own style and its own glue point, holding two rectangles.
fn with_a_group() -> Drawing {
    Drawing::new().page(concat!(
        r#"<draw:rect draw:style-name="gr1" draw:name="R" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:g draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
    ))
}

/// The id of the group of the first page.
fn group_id(s: &diacraft_engine::Session) -> u32 {
    let doc = s.document().expect("a document");
    doc.pages.get(0).and_then(|p| p.shapes.iter().find(|x| !x.children.is_empty())).map(|x| x.id).expect("the group")
}

/// Open, move the group, save, move a shape beside it, save, and save again with nothing changed: the saves after the first are no refusal, and the last is no edit.
#[test]
fn a_group_of_the_file_with_no_name_is_moved_and_saved_twice() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &with_a_group());
    let id = group_id(&s);
    run(&mut s, "shape.move", json!({ "ids": [id], "dx": 12.0 / 7.0, "dy": 0.0 }));
    let first = save(&mut s);
    assert_eq!(first["edited"], true, "{first}");
    shift(&mut s, "R", 0.5, 0.0);
    match s.execute("file.save", &json!({})) {
        Ok(second) => assert_eq!(second["edited"], true, "{second}"),
        Err(e) => panic!("the second save was refused: {e}"),
    }
    let again = save(&mut s);
    assert_eq!(again["edited"], false, "a save with nothing changed is no edit: {again}");
}

/// The number a cell of the group holds.
fn number_of(s: &diacraft_engine::Session, id: u32, cell: &str) -> f64 {
    let doc = s.document().expect("a document");
    let shape = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|x| x.id == id)).expect("the group");
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).expect("a number")
}

/// A resize of the group (the box of its members is stretched), then a move, a save after each and a last with nothing changed: every save after the first is no refusal.
#[test]
fn a_group_of_the_file_with_no_name_is_resized_and_moved_and_saved_after_each() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &with_a_group());
    let id = group_id(&s);
    let (w, h) = (number_of(&s, id, "Width"), number_of(&s, id, "Height"));
    let (x, y) = (number_of(&s, id, "PinX") - w / 2.0, number_of(&s, id, "PinY") - h / 2.0);
    run(&mut s, "shape.resize", json!({ "id": id, "x": x + 0.3, "y": y + 0.7, "width": w * 1.37, "height": h * 0.83 }));
    match s.execute("file.save", &json!({})) {
        Ok(first) => assert_eq!(first["edited"], true, "{first}"),
        Err(e) => panic!("the save after the resize was refused: {e}"),
    }
    // The file read again holds the group as the session has it: the box the resize made, and the rows of its box (the four of every shape and the glue point of its own).
    let (back, _) = read_back(&dir.read("a.odg"));
    let again = back.pages.get(0).and_then(|p| p.shapes.iter().find(|x| !x.children.is_empty())).expect("the group in the file");
    for cell in ["Width", "Height", "PinX", "PinY"] {
        let (was, now) = (number_of(&s, id, cell), again.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).expect("a number"));
        assert!((was - now).abs() < 1e-9, "{cell}: the session has {was}, the file read again {now}");
    }
    let rows = |shape: &diacraft_doc::Shape| shape.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).map_or(0, |c| c.rows.len());
    let session = s.document().and_then(|d| d.pages.get(0).and_then(|p| p.shapes.iter().find(|x| x.id == id).map(|x| rows(x)))).expect("the group");
    assert_eq!((rows(again), session), (5, 5), "the standard four rows and the glue point's");
    run(&mut s, "shape.move", json!({ "ids": [id], "dx": -0.123_456_789, "dy": 0.987_654_321 }));
    match s.execute("file.save", &json!({})) {
        Ok(second) => assert_eq!(second["edited"], true, "{second}"),
        Err(e) => panic!("the save after the move was refused: {e}"),
    }
    match s.execute("file.save", &json!({})) {
        Ok(last) => assert_eq!(last["edited"], false, "a save with nothing changed is no edit: {last}"),
        Err(e) => panic!("the save with nothing changed was refused: {e}"),
    }
}
