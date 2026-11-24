//! Groups saved to an `.odg` through the commands. `shape.group` of new shapes is written as a `draw:g` and reads back as the session has it; a group of the file
//! is moved and deleted; and what is not written (a shape of the file put into a group, a group taken apart) is refused in words, with nothing written.

use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Renderer, View};
use serde_json::{Value, json};

use crate::odg_build::{Dir, id_of, open_drawing};
use crate::odg_file::{assert_saves_again_as_no_edit, four, save};
use crate::run;

/// The first page drawn at 50 pixels to the inch, on white paper: its size and its premultiplied pixels.
fn drawn(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

/// The file `a.odg` read into a session of its own.
fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

/// Asserts that the drawing the file holds draws as the session's does, to the hundredths of a millimetre a path is written in.
#[track_caller]
fn assert_draws_alike(made: &Document, read: &Document, what: &str) {
    let (a, b) = (drawn(made), drawn(read));
    assert_eq!((a.0, a.1), (b.0, b.1), "{what}: the picture's size");
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= 8, "{what}: the file draws it differently from the session (the worst channel differs by {worst})");
}

/// The shapes of the first page by name, in drawing order, groups and members in the pre-order of the file: `Name/Name` for a member.
fn tree(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, &diacraft_doc::ShapeList)> = vec![(String::new(), &doc.pages.get(0).unwrap().shapes)];
    // A walk by hand, last first: the members of a group before the shapes after it.
    let mut pending: Vec<(String, &std::sync::Arc<diacraft_doc::Shape>)> = Vec::new();
    while let Some((prefix, list)) = stack.pop() {
        pending.extend(list.iter().rev().map(|s| (prefix.clone(), s)));
        while let Some((prefix, shape)) = pending.pop() {
            let name = shape.name_u.as_deref().unwrap_or("-").to_owned();
            out.push(format!("{prefix}{name}"));
            if !shape.children.is_empty() {
                pending.extend(shape.children.iter().rev().map(|s| (format!("{prefix}{name}/"), s)));
            }
        }
    }
    out
}

fn notes(saved: &Value) -> &[Value] {
    saved["notes"].as_array().unwrap_or_else(|| panic!("a save of an .odg has `notes`: {saved}"))
}

/// `shape.group` of new shapes (dropped from three stencils): saved as a `draw:g`, read back with the same tree, drawn as the session draws it, and saved again as no
/// edit; the members' formulas of the group are constants in the file, and the three links to masters are no trigger of the note.
#[test]
fn a_group_of_dropped_shapes_is_saved_as_a_draw_g_and_reads_back_as_the_session_has_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "arrows.right", "stencil": "arrows", "page": 0, "x": 8.0, "y": 7.0 }));
    let c = run(&mut s, "shape.drop", json!({ "master": "decorative.star-5", "stencil": "decorative", "page": 0, "x": 7.0, "y": 8.5 }));
    let grouped = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"], c["id"]] }));
    assert_eq!(grouped["members"], 3);
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(notes(&saved), &[] as &[Value], "the three members are instances of masters, which is no trigger of the note: {saved}");
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let again = reopened(&dir);
    assert_eq!(tree(again.document().unwrap()).len(), 4 + 1 + 3, "the four of the file, the group and its three");
    assert_eq!(tree(s.document().unwrap()).len(), tree(again.document().unwrap()).len());
    assert_draws_alike(s.document().unwrap(), again.document().unwrap(), "the group");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A group inside a group, with a shape beside it, and a new shape dropped after both: written nested, read back as the tree the session has.
#[test]
fn a_group_in_a_group_is_saved_nested_and_a_shape_dropped_after_it_follows_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let inner = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
    let c = run(&mut s, "shape.drop", json!({ "master": "diamond", "page": 0, "x": 6.0, "y": 9.0 }));
    run(&mut s, "shape.group", json!({ "page": 0, "ids": [inner["id"], c["id"]] }));
    run(&mut s, "shape.drop", json!({ "master": "triangle", "page": 0, "x": 3.0, "y": 9.0 }));
    save(&mut s);
    let again = reopened(&dir);
    assert_eq!(tree(s.document().unwrap()).len(), tree(again.document().unwrap()).len(), "{:?}", tree(again.document().unwrap()));
    assert_draws_alike(s.document().unwrap(), again.document().unwrap(), "the nested groups");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A group of the file (the one just saved, read again) is moved and then deleted: the members follow the move on the page, and a delete takes the group with them.
#[test]
fn a_saved_group_is_moved_and_deleted() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let g = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
    save(&mut s);
    let mut s = reopened(&dir);
    let group = s.document().unwrap().pages.get(0).unwrap().shapes.get(4).unwrap().id;
    run(&mut s, "shape.move", json!({ "ids": [group], "dx": 1.0, "dy": -0.5 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let moved = reopened(&dir);
    assert_draws_alike(s.document().unwrap(), moved.document().unwrap(), "the moved group");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    run(&mut s, "shape.delete", json!({ "ids": [group], "page": 0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(tree(reopened(&dir).document().unwrap()).len(), 4, "the group and its members are gone");
    let _ = g;
}

/// A shape of the file put into a group, and a group taken apart, are refused in words with nothing written: moving an element of the file into another is not written.
#[test]
fn a_shape_of_the_file_grouped_and_a_group_taken_apart_are_refused_in_words_and_nothing_is_written() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let before = dir.read("a.odg");
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    run(&mut s, "shape.group", json!({ "page": 0, "ids": [a, b] }));
    let words = match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("not refused: {other:?}"),
    };
    assert!(words.contains("a shape of the file put into a new group"), "{words}");
    assert_eq!(dir.read("a.odg"), before, "nothing was written");
    run(&mut s, "edit.undo", json!({}));
    // A group of the file taken apart.
    let x = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let y = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let g = run(&mut s, "shape.group", json!({ "page": 0, "ids": [x["id"], y["id"]] }));
    save(&mut s);
    let saved = dir.read("a.odg");
    run(&mut s, "shape.ungroup", json!({ "page": 0, "ids": [g["id"]] }));
    let words = match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("not refused: {other:?}"),
    };
    assert!(words.contains("a shape moved into or out of a group"), "{words}");
    assert_eq!(dir.read("a.odg"), saved, "nothing was written");
}

/// The review: a group's own text was dropped with no word (a group of new shapes with `shape.set_text` on it saved with no warning and read back with
/// none). R45 writes it as one more member of the group, which is not written yet, so the save is **refused in words** (nothing written, and the way out is the undo),
/// for a group of new shapes and for a group of the file; the same group with no text saves.
#[test]
fn the_text_of_a_group_is_refused_in_words_and_never_dropped() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let before = dir.read("a.odg");
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let g = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
    run(&mut s, "shape.set_text", json!({ "id": g["id"], "text": "the group says" }));
    let words = match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("saved with the text of the group dropped: {other:?}"),
    };
    assert!(words.contains("a new group with text of its own") && words.contains("Undo the change (edit.undo)"), "{words}");
    assert_eq!(dir.read("a.odg"), before, "nothing was written");
    run(&mut s, "edit.undo", json!({}));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "the same group with no text saves: {saved}");
    // A group of the file: a text set on it is refused too, and the undo saves.
    let mut s = reopened(&dir);
    let group = s.document().unwrap().pages.get(0).unwrap().shapes.get(4).unwrap().id;
    let file = dir.read("a.odg");
    run(&mut s, "shape.set_text", json!({ "id": group, "text": "and again" }));
    let words = match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("saved with the text of the group dropped: {other:?}"),
    };
    assert!(words.contains("text") && words.contains("group"), "{words}");
    assert_eq!(dir.read("a.odg"), file, "nothing was written");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(save(&mut s)["edited"], false, "the undo saves, and the file is as it was");
}

/// The review: a group of the file turned 30 degrees saved with no word (the members were written turned and the reread group had an angle of 0, so
/// the turn the session holds as the group's was gone), and a new group that is turned or flipped was refused with words that said "a later task". Keep the
/// group's own angle and flips in DiaCraft's document file and write the members turned, which is not written yet: a turn or a flip of a group, new or of the file, is
/// **refused in words**, with nothing written and the undo as the way out; the members turned or flipped one by one save.
#[test]
fn a_turned_or_flipped_group_is_refused_in_words_new_or_of_the_file() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let g = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
    let before = dir.read("a.odg");
    let refused = |s: &mut Session| match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("saved with the turn of the group dropped: {other:?}"),
    };
    for (what, command, params) in [
        ("a new group turned", "shape.rotate", json!({ "ids": [g["id"]], "angle": 30 })),
        ("a new group flipped", "shape.flip", json!({ "ids": [g["id"]], "axis": "horizontal" })),
    ] {
        run(&mut s, command, params);
        let words = refused(&mut s);
        assert!(words.contains("a group that is turned or flipped") && words.contains("Undo the change (edit.undo)"), "{what}: {words}");
        assert!(!words.contains("later task"), "{what}: the words say what is missing and not a task: {words}");
        assert_eq!(dir.read("a.odg"), before, "{what}: nothing was written");
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(save(&mut s)["edited"], true, "the group with no turn saves");
    // A group of the file: turned or flipped is refused as well, and the undo saves.
    let mut s = reopened(&dir);
    let group = s.document().unwrap().pages.get(0).unwrap().shapes.get(4).unwrap().id;
    let file = dir.read("a.odg");
    for (what, command, params) in [
        ("a group of the file turned", "shape.rotate", json!({ "ids": [group], "angle": 30 })),
        ("a group of the file flipped", "shape.flip", json!({ "ids": [group], "axis": "vertical" })),
    ] {
        run(&mut s, command, params);
        let words = refused(&mut s);
        assert!(words.contains("a group that is turned or flipped"), "{what}: {words}");
        assert_eq!(dir.read("a.odg"), file, "{what}: nothing was written");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(save(&mut s)["edited"], false, "{what}: the undo saves, and the file is as it was");
    }
}
