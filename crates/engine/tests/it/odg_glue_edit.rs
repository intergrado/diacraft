//! A drawing opened from an `.odg` is saved back to it after the glue of a connector changed (`connector.reconnect`, `connector.reverse_ends`, an end taken off its
//! shape, a glued shape moved or deleted). The splice writer used to refuse every change of glue, and `file.save` said so in words; it writes the glue now (
//! the rows for `draw:start-shape`, `draw:end-shape`, `draw:start-glue-point` and `draw:end-glue-point`).
//!
//! Each case opens a hand-written drawing, runs real commands, saves in place and holds the file to four things: the attributes of the connector's tag, the glue and the ends the file
//! reads back as (against the session's), a full recalculation of the reopened drawing that changes no cell, and a second save that is no edit.

use diacraft_calc::{Budget, recalc_all};
use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, save};
use crate::run;

/// A rectangle called `name` with `xml:id` and `draw:id` `id`, `w` by `h` cm at (`x`, `y`) cm from the page's top left.
fn rectangle(name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    format!(
        r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" xml:id="{id}" draw:id="{id}" svg:width="{w}cm" svg:height="{h}cm" svg:x="{x}cm" svg:y="{y}cm"><text:p/></draw:rect>"#
    )
}

/// A connector called `C` whose ends are as `glue` says (`draw:start-shape` and the rest, verbatim), from (`x1`, `y1`) to (`x2`, `y2`) cm from the page's top left, with the elbow
/// path LibreOffice writes for a standard connector between them.
fn connector(glue: &str, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> String {
    let (mx, w, h) = ((x2 - x1) / 2.0, (x2 - x1).abs() * 1000.0 + 1.0, (y2 - y1).abs() * 1000.0 + 1.0);
    format!(
        r#"<draw:connector draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="C" {glue} svg:x1="{x1}cm" svg:y1="{y1}cm" svg:x2="{x2}cm" svg:y2="{y2}cm" svg:d="M {} {} h {} v {} h {}" svg:viewBox="0 0 {w} {h}"><text:p/></draw:connector>"#,
        x1 * 1000.0,
        y1 * 1000.0,
        mx * 1000.0,
        (y2 - y1) * 1000.0,
        mx * 1000.0,
    )
}

/// A (4 cm by 2 cm at (3, 10)), B (3 by 2 at (8, 14)), D (3 by 2 at (14, 10), no connector yet) and connector C from A's right side (glue id 1) to B's left side (glue id 3). A rectangle's
/// glue ids are 0 top, 1 right, 2 bottom and 3 left, and its rows 0 top, 1 left, 2 bottom and 3 right.
fn points() -> Drawing {
    Drawing::new().page(&format!(
        "{}{}{}{}",
        rectangle("A", "a", (3.0, 10.0, 4.0, 2.0)),
        rectangle("B", "b", (8.0, 14.0, 3.0, 2.0)),
        rectangle("D", "d", (14.0, 10.0, 3.0, 2.0)),
        connector(r#"draw:start-shape="a" draw:start-glue-point="1" draw:end-shape="b" draw:end-glue-point="3""#, (7.0, 11.0, 8.0, 15.0)),
    ))
}

/// The same with both ends glued to the whole of a shape: no glue point is named.
fn whole() -> Drawing {
    Drawing::new().page(&format!(
        "{}{}{}{}",
        rectangle("A", "a", (3.0, 10.0, 4.0, 2.0)),
        rectangle("B", "b", (8.0, 14.0, 3.0, 2.0)),
        rectangle("D", "d", (14.0, 10.0, 3.0, 2.0)),
        connector(r#"draw:start-shape="a" draw:end-shape="b""#, (7.0, 11.0, 8.0, 15.0)),
    ))
}

/// The start tag of connector `C` in a saved file.
fn tag(bytes: &[u8]) -> String {
    let content = content_of(bytes);
    let at = content.find("<draw:connector ").expect("a connector");
    content[at..at + content[at..].find('>').expect("its end")].to_owned()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// The glue of the first page as `(connector, end, target, part)` by the shapes' names, sorted.
fn glue_by_name(doc: &Document) -> Vec<(String, &'static str, String, u32)> {
    let page = doc.pages.get(0).unwrap();
    let name = |id: u32| page.shapes.iter().find(|s| s.id == id).and_then(|s| s.name_u.as_deref()).unwrap_or("?").to_owned();
    let mut list: Vec<(String, &'static str, String, u32)> = page
        .connects
        .iter()
        .map(|c| {
            (
                name(c.from_sheet),
                if c.from_cell.as_deref().is_some_and(|f| f.starts_with("Begin")) { "begin" } else { "end" },
                name(c.to_sheet),
                c.to_part.unwrap_or(0),
            )
        })
        .collect();
    list.sort();
    list
}

fn glued(list: &[(&str, &'static str, &str, u32)]) -> Vec<(String, &'static str, String, u32)> {
    let mut out: Vec<(String, &'static str, String, u32)> = list.iter().map(|(c, e, t, p)| ((*c).to_owned(), *e, (*t).to_owned(), *p)).collect();
    out.sort();
    out
}

/// The drawing the file holds, read again by the reader of the `odg` crate: a full recalculation of it changes no cell.
#[track_caller]
fn recalculates_clean(bytes: &[u8], what: &str) {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let report = recalc_all(&doc, &Budget::default());
    assert!(report.differed.is_empty() && report.differed_unlisted == 0, "{what}: a recalculation changed cells of the file: {:?}", report.differed);
}

/// What every case checks of the file after its save: it reopens as the session is, with the glue the session has, a recalculation of it changes no cell, and a second save is no edit.
#[track_caller]
fn holds(dir: &Dir, s: &mut Session, what: &str) {
    assert_reopens_as_saved(dir, s, "a.odg", 0);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    assert_eq!(glue_by_name(s.document().unwrap()), glue_by_name(again.document().unwrap()), "{what}: the glue the file reads back as");
    recalculates_clean(&dir.read("a.odg"), what);
    assert_saves_again_as_no_edit(dir, s, "a.odg");
}

fn reconnect(s: &mut Session, end: &str, target: serde_json::Value) {
    let c = id_of(s.document().unwrap(), 0, "C");
    run(s, "connector.reconnect", json!({ "id": c, "end": end, "target": target }));
}

fn named(s: &Session, name: &str) -> u32 {
    id_of(s.document().unwrap(), 0, name)
}

/// An end dragged onto another shape's connection point is saved glued to it: the shape's name and the glue id of the point (D's left side is row 1, glue id 3), the end's place, and
/// the other end as it was.
#[test]
fn an_end_reconnected_to_another_shape_is_saved_glued_to_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("a"), "{t}");
    assert_eq!(attr(&t, "draw:start-glue-point").as_deref(), Some("1"), "{t}");
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("d"), "{t}");
    assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("3"), "{t}");
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "A", 103), ("C", "end", "D", 101)]));
    holds(&dir, &mut s, "an end reconnected");
}

/// The same end sent to each of D's four points: the glue id the file names is the point's.
#[test]
fn an_end_reconnected_to_each_point_of_a_shape_names_that_point() {
    // The rows of a rectangle: 0 top (glue id 0), 1 left (3), 2 bottom (2), 3 right (1).
    for (row, glue_id) in [(0u32, "0"), (1, "3"), (2, "2"), (3, "1")] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &points());
        let d = named(&s, "D");
        reconnect(&mut s, "begin", json!({ "id": d, "point": row }));
        save(&mut s);
        let t = tag(&dir.read("a.odg"));
        assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("d"), "row {row}: {t}");
        assert_eq!(attr(&t, "draw:start-glue-point").as_deref(), Some(glue_id), "row {row}: {t}");
        assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("b"), "row {row}: the end is as it was: {t}");
        assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("3"), "row {row}: {t}");
        holds(&dir, &mut s, &format!("the begin reconnected to row {row}"));
    }
}

/// Reverse Ends: the begin and the end swap with their glue, their places and the path.
#[test]
fn reversed_ends_are_saved_swapped_with_their_glue_and_places() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let c = named(&s, "C");
    run(&mut s, "connector.reverse_ends", json!({ "id": c }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("b"), "{t}");
    assert_eq!(attr(&t, "draw:start-glue-point").as_deref(), Some("3"), "{t}");
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("a"), "{t}");
    assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("1"), "{t}");
    assert_eq!(attr(&t, "svg:x1").as_deref(), Some("8cm"), "the begin is B's left side: {t}");
    assert_eq!(attr(&t, "svg:x2").as_deref(), Some("7cm"), "{t}");
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "B", 101), ("C", "end", "A", 103)]));
    holds(&dir, &mut s, "reversed ends");
}

/// Reverse Ends of a connector glued to the whole of two shapes: the shapes swap and no glue point is named.
#[test]
fn a_connector_glued_to_whole_shapes_is_saved_reversed() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &whole());
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "A", 3), ("C", "end", "B", 3)]));
    let c = named(&s, "C");
    run(&mut s, "connector.reverse_ends", json!({ "id": c }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("b"), "{t}");
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("a"), "{t}");
    assert_eq!((attr(&t, "draw:start-glue-point"), attr(&t, "draw:end-glue-point")), (None, None), "the whole of a shape has no glue point: {t}");
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "B", 3), ("C", "end", "A", 3)]));
    holds(&dir, &mut s, "reversed whole-shape glue");
}

/// An end glued to the whole of a shape, sent to a connection point of another: the shape's name changes and a glue point is named.
#[test]
fn an_end_glued_to_a_whole_shape_is_saved_glued_to_a_point_of_another() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &whole());
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("a"), "{t}");
    assert_eq!(attr(&t, "draw:start-glue-point"), None, "the begin is still glued to the whole of A: {t}");
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("d"), "{t}");
    assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("3"), "{t}");
    holds(&dir, &mut s, "a whole-shape end reconnected to a point");
}

/// An end dropped on the page, free: both attributes go and the end is where it was dropped.
#[test]
fn an_end_taken_off_its_shape_is_saved_free() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    reconnect(&mut s, "end", json!({ "x": 6.0, "y": 8.0 }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!((attr(&t, "draw:end-shape"), attr(&t, "draw:end-glue-point")), (None, None), "{t}");
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("a"), "{t}");
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "A", 103)]));
    holds(&dir, &mut s, "a free end");
}

/// A glued shape moved, after the connector was reconnected, in one save: the connector goes where its new glue puts it.
#[test]
fn a_reconnected_connector_follows_the_shape_it_is_glued_to_when_that_moves() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    run(&mut s, "shape.move", json!({ "ids": [d], "dx": 1.0, "dy": -0.5 }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("d"), "{t}");
    assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("3"), "{t}");
    holds(&dir, &mut s, "a reconnected connector and its target moved");
}

/// The target of a reconnected end deleted in the same session: the connector ends free, as it does when the shape is deleted after a save.
#[test]
fn the_shape_an_end_was_reconnected_to_deleted_leaves_the_end_free() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    run(&mut s, "shape.delete", json!({ "ids": [d], "connectors": false }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!((attr(&t, "draw:end-shape"), attr(&t, "draw:end-glue-point")), (None, None), "{t}");
    assert!(!content_of(&dir.read("a.odg")).contains("draw:name=\"D\""));
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "A", 103)]));
    holds(&dir, &mut s, "the target of a new glue deleted");
}

/// Two saves: the first reconnects, the second reverses. Each is held to the file as the one before left it.
#[test]
fn glue_changed_across_two_saves_is_written_against_the_file_each_save_left() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let (d, c) = (named(&s, "D"), named(&s, "C"));
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    save(&mut s);
    holds(&dir, &mut s, "after the first save");
    run(&mut s, "connector.reverse_ends", json!({ "id": c }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("d"), "{t}");
    assert_eq!(attr(&t, "draw:start-glue-point").as_deref(), Some("3"), "{t}");
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("a"), "{t}");
    holds(&dir, &mut s, "after the second save");
}

/// An undo after a save: the next save writes the glue the file had before.
#[test]
fn an_undo_of_a_saved_reconnection_is_saved_as_the_glue_the_file_had() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let original = dir.read("a.odg");
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    save(&mut s);
    run(&mut s, "edit.undo", json!({}));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:end-shape").as_deref(), Some("b"), "{t}");
    assert_eq!(attr(&t, "draw:end-glue-point").as_deref(), Some("3"), "{t}");
    assert_eq!(glue_by_name(s.document().unwrap()), glued(&[("C", "begin", "A", 103), ("C", "end", "B", 101)]));
    assert_eq!(tag(&original).split(" svg:x1").next(), t.split(" svg:x1").next(), "the tag is the file's again up to its ends");
    holds(&dir, &mut s, "an undone reconnection");
}

/// A save that changes no glue still writes none: moving B leaves the connector's glue attributes with their bytes.
#[test]
fn a_move_of_a_glued_shape_leaves_the_glue_attributes_alone() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let b = named(&s, "B");
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 1.0, "dy": 0.0 }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    for (name, want) in [("draw:start-shape", "a"), ("draw:start-glue-point", "1"), ("draw:end-shape", "b"), ("draw:end-glue-point", "3")] {
        assert_eq!(attr(&t, name).as_deref(), Some(want), "{t}");
    }
    holds(&dir, &mut s, "a glued shape moved");
}

/// An end dragged onto a shape dropped in the same session (a shape the file does not hold yet) is saved with the shape and the glue: the shape is written, given an id, and the
/// connector names it and the point.
#[test]
fn an_end_reconnected_to_a_shape_dropped_in_the_session_is_saved_with_the_shape() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let dropped = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    reconnect(&mut s, "end", json!({ "id": dropped["id"], "point": 2 }));
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!(attr(&t, "draw:start-shape").as_deref(), Some("a"), "{t}");
    let named = attr(&t, "draw:end-shape").expect("the end names the new shape");
    assert!(content_of(&dir.read("a.odg")).contains(&format!("draw:id=\"{named}\"")), "the new shape is written with that id: {named}");
    assert!(attr(&t, "draw:end-glue-point").is_some(), "{t}");
    holds(&dir, &mut s, "an end reconnected to a new shape");
}

/// A connection point added in the session (the doubling gives a rectangle a point in the middle of each portion of an edge) that a connector is glued to: the file does not
/// declare the point, so the glue is saved naming its number (the rows the shape has beyond the four standard ones are numbered as the reader numbers them) and the shape's changed
/// section rides in DiaCraft's data; the file reads back with the connector glued to the same row of the same shape.
#[test]
fn an_end_glued_to_a_point_the_session_added_is_saved_naming_its_number_and_reads_back_glued() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &points());
    let d = named(&s, "D");
    run(&mut s, "shape.connection_points", json!({ "id": d, "action": "double" }));
    reconnect(&mut s, "end", json!({ "id": d, "point": 4 }));
    let after = glue_by_name(s.document().unwrap());
    assert_eq!(after, glued(&[("C", "begin", "A", 103), ("C", "end", "D", 104)]), "the end is glued to the new row 4");
    save(&mut s);
    let t = tag(&dir.read("a.odg"));
    assert_eq!((attr(&t, "draw:end-shape").as_deref(), attr(&t, "draw:end-glue-point").as_deref()), (Some("d"), Some("4")), "{t}");
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    assert_eq!(glue_by_name(again.document().unwrap()), after, "the file reads back with the connector on the same row");
    recalculates_clean(&dir.read("a.odg"), "a point the session added");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The same in a flat drawing, which the splice writes as it does a package.
#[test]
fn a_flat_drawing_is_saved_with_the_glue_of_a_reconnected_end() {
    let dir = Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.fodg", &points().flat_bytes());
    let d = named(&s, "D");
    reconnect(&mut s, "end", json!({ "id": d, "point": 1 }));
    assert_eq!(save(&mut s)["edited"], true);
    let flat = String::from_utf8(dir.read("a.fodg")).unwrap();
    let at = flat.find("<draw:connector ").expect("a connector");
    let t = &flat[at..at + flat[at..].find('>').unwrap()];
    assert_eq!((attr(t, "draw:end-shape").as_deref(), attr(t, "draw:end-glue-point").as_deref()), (Some("d"), Some("3")), "{t}");
    assert_reopens_as_saved(&dir, &mut s, "a.fodg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.fodg");
}

/// Keeps the files of each case for a check by another program: with `GLUEWRITE_KEEP` set to a folder, every case below saves into it, as `<case>-before.odg` (the drawing as it was
/// opened) and `<case>.odg` (as saved). Ignored: it writes where the environment says, and checks nothing a case above does not.
#[test]
#[ignore = "writes the files of the cases to the folder GLUEWRITE_KEEP names, for a check by another program"]
fn keep_the_files_of_the_cases() {
    let Some(keep) = std::env::var_os("GLUEWRITE_KEEP") else { return };
    let keep = std::path::PathBuf::from(keep);
    std::fs::create_dir_all(&keep).unwrap();
    type Case = (&'static str, fn() -> Drawing, fn(&mut Session));
    let cases: [Case; 9] = [
        ("reconnect-end-to-d", points, |s| {
            let d = named(s, "D");
            reconnect(s, "end", json!({ "id": d, "point": 1 }));
        }),
        ("reconnect-begin-to-d-top", points, |s| {
            let d = named(s, "D");
            reconnect(s, "begin", json!({ "id": d, "point": 0 }));
        }),
        ("reverse-points", points, |s| {
            let c = named(s, "C");
            run(s, "connector.reverse_ends", json!({ "id": c }));
        }),
        ("reverse-whole", whole, |s| {
            let c = named(s, "C");
            run(s, "connector.reverse_ends", json!({ "id": c }));
        }),
        ("whole-end-to-d-point", whole, |s| {
            let d = named(s, "D");
            reconnect(s, "end", json!({ "id": d, "point": 1 }));
        }),
        ("free-end", points, |s| reconnect(s, "end", json!({ "x": 6.0, "y": 8.0 }))),
        ("reconnect-then-move-d", points, |s| {
            let d = named(s, "D");
            reconnect(s, "end", json!({ "id": d, "point": 1 }));
            run(s, "shape.move", json!({ "ids": [d], "dx": 1.0, "dy": -0.5 }));
        }),
        ("reconnect-then-delete-d", points, |s| {
            let d = named(s, "D");
            reconnect(s, "end", json!({ "id": d, "point": 1 }));
            run(s, "shape.delete", json!({ "ids": [d] }));
        }),
        ("move-b", points, |s| {
            let b = named(s, "B");
            run(s, "shape.move", json!({ "ids": [b], "dx": 1.0, "dy": 0.0 }));
        }),
    ];
    for (name, drawing, edit) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing());
        std::fs::write(keep.join(format!("{name}-before.odg")), dir.read("a.odg")).unwrap();
        edit(&mut s);
        save(&mut s);
        std::fs::write(keep.join(format!("{name}.odg")), dir.read("a.odg")).unwrap();
    }
}
