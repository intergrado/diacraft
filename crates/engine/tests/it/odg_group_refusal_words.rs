//! Issue 3: the refusal of a save for a group with no name that holds data of DiaCraft's names the group that needs a name, and offers Save As `.diacraft`.
//! It does not tell a person to undo "the change".
//!
//! An `.odg` finds the data of a group by the group's name, so a group with no name that is given data of its own (or a link to a master) cannot be saved into the file
//! (`diacraft_odg`, "saving does not yet write this change: a group with no name ..."). The words ended with the way out of every other refusal: "Undo the change
//! (edit.undo) to save". A person who saved a second time, or moved another shape after the first refusal, did nothing the undo would take back: the undo went to
//! the shape that was moved, and the save was refused again.

use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::odg_build::{Dir, Drawing, id_of, open_bytes_in, rect};
use crate::run;

/// A page of a group with no name that holds two rectangles, and a rectangle beside it.
fn drawing() -> Drawing {
    let [a, b, c] = [("A", 1.0), ("B", 4.0), ("C", 8.0)].map(|(name, x)| rect(name, (x, 1.0, 2.0, 1.0), ""));
    Drawing::new().page(&format!("<draw:g>{a}{b}</draw:g>{c}"))
}

/// Gives the group data of its own, as a person does in the ShapeSheet: a cell no markup of the file gives a group.
fn give_the_group_data(s: &mut Session) {
    s.edit("Test setup: data on a group", |doc: &mut diacraft_doc::Document| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let group = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let cell = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1"));
        std::sync::Arc::make_mut(&mut group.sheet).cells.insert(diacraft_doc::CellName::new("Cost"), cell);
        Ok(())
    })
    .unwrap();
}

/// What a save of the session says when it is refused.
fn refused(s: &mut Session) -> String {
    match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("the save was not refused: {other:?}"),
    }
}

/// The words, by what they must say: which group needs a name, what a person can do, and what they must not say.
fn assert_words(said: &str, what: &str, kind: &str) {
    assert!(said.starts_with("the drawing could not be saved: "), "{what}: {said}");
    assert!(said.contains("the group with no name numbered 1 on the page \"page1\""), "{what}: the words say which group needs a name: {said}");
    assert!(said.contains("Name it, put its data on a shape beside it"), "{what}: and that it needs a name: {said}");
    assert!(said.contains("use Save As for a .diacraft file, which holds it"), "{what}: and offer Save As .diacraft: {said}");
    assert!(said.contains("Nothing was written"), "{what}: {said}");
    assert!(!said.contains("Undo") && !said.contains("edit.undo"), "{what}: the words do not tell a person to undo what they did not do: {said}");
    assert!(said.contains(kind), "{what}: the words say what the file is: {said}");
    assert!(
        said.chars().count() <= 300,
        "{what}: the window shows the first 300 characters, so the words are short: {} characters",
        said.chars().count()
    );
}

/// **A second save, and a save after another shape was moved, are refused in the same words, and none of them says to undo.** A group of the file with no name is given
/// data; the save is refused. Saving again changes nothing, and nor does moving the rectangle beside the group: the cause is the group's, and the words say so. The undo of the
/// move, which the old words asked for, leaves the save refused.
#[test]
fn a_second_save_and_a_save_after_moving_another_shape_are_refused_by_the_group_and_say_no_undo() {
    // (With a flat file, a `.fodg`, the refusal is the flat writer's own, `WriteError::FlatGroup`: `odg_group_data_flat`.)
    let (name, kind, bytes) = ("a.odg", "an .odg", drawing().bytes());
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, name, &bytes);
    give_the_group_data(&mut s);
    let first = refused(&mut s);
    assert_words(&first, &format!("{name}: the first save"), kind);
    assert_eq!(dir.read(name), bytes, "{name}: nothing was written");
    let second = refused(&mut s);
    assert_eq!(second, first, "{name}: a second save says the same");
    let other = id_of(s.document().unwrap(), 0, "C");
    run(&mut s, "shape.move", json!({"page": 0, "ids": [other], "dx": 0.5, "dy": 0.0}));
    let moved = refused(&mut s);
    assert_eq!(moved, first, "{name}: moving another shape changes nothing about the cause");
    // What the old words asked for takes back the move and not the data: the save is refused still.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(refused(&mut s), first, "{name}: the undo of the move does not save");
    // The way out the words offer: Save As a `.diacraft` file, which holds the group's data.
    let target = dir.path("out.diacraft");
    run(&mut s, "file.save_as", json!({ "path": target }));
    assert!(std::path::Path::new(&target).exists(), "{name}: Save As .diacraft wrote the file");
    assert_eq!(dir.read(name), bytes, "{name}: and the file the drawing came from is as it was");
}
