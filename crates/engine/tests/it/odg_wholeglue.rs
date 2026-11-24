//! Item 1: an end of an `.odg` connector glued to the whole of a shape (a `draw:start-shape` or `draw:end-shape` with no glue point) goes where LibreOffice puts
//! it, `crates/engine/src/draw/odg_glue.rs`, and not where, the VSDX rule, would: LibreOffice places such an end itself whenever it loads the file,
//! so the session must place it there for the file to draw the same in both programs.
//!
//! **The regression** (the full gate's sample-file check on 02913eef: 9 ends right at eee3748d and 0.27 to 1.42 inch off after, 12 off on both, 19 on another side than LibreOffice's):
//! Had the engine place every whole-shape end of an `.odg` connector by the connection point nearest the other end, or the nearest pair, which is
//! not LibreOffice's rule.
//!
//! **The oracle**: LibreOffice 26.8.2.2 as a black box, measured once on these drawings (a script outside the repository: each drawing below with shape B already
//! moved, written by hand from the ODF standard, re-saved by LibreOffice as a flat document with its own profile, and its `svg:x1` to `svg:y2` read), pinned in
//! [`BOTH`] and [`POINT`].

use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing};
use crate::odg_file::save;
use crate::run;

/// The page's height: the drawings' page is 21 cm by 29.7 cm.
const PAGE_H: f64 = 29.7 / 2.54;

/// How near LibreOffice's place an end must be: 0.01 inch.
const NEAR: f64 = 0.01;

/// The moves of B, page inches with y up (one inch each way), by name.
const MOVES: [(&str, f64, f64); 4] = [("right", 1.0, 0.0), ("left", -1.0, 0.0), ("down", 0.0, -1.0), ("up", 0.0, 1.0)];

/// LibreOffice 26.8.2.2's ends of connector C glued to the whole of A and of B, B moved as named: the type, the move, and the start and the end in centimetres from the page's
/// top left (`lo/pinout/pin.json`, measured 2026-11-22).
const BOTH: [(&str, &str, [f64; 4]); 16] = [
    ("standard", "right", [7.0, 11.0, 10.54, 15.0]),
    ("standard", "left", [5.0, 12.0, 6.96, 14.0]),
    ("standard", "down", [5.0, 12.0, 9.5, 16.54]),
    ("standard", "up", [5.0, 10.0, 9.5, 11.46]),
    ("lines", "right", [7.0, 11.0, 10.54, 15.0]),
    ("lines", "left", [5.0, 12.0, 5.46, 15.0]),
    ("lines", "down", [7.0, 11.0, 8.0, 17.54]),
    ("lines", "up", [7.0, 11.0, 8.0, 12.46]),
    ("line", "right", [7.0, 11.0, 10.54, 15.0]),
    ("line", "left", [7.0, 11.0, 6.96, 14.0]),
    ("line", "down", [7.0, 11.0, 8.0, 17.54]),
    ("line", "up", [7.0, 11.0, 8.0, 12.46]),
    ("curve", "right", [7.0, 11.0, 10.54, 15.0]),
    ("curve", "left", [5.0, 12.0, 6.96, 14.0]),
    ("curve", "down", [5.0, 12.0, 9.5, 16.54]),
    ("curve", "up", [5.0, 10.0, 9.5, 11.46]),
];

/// The same with C's end glued to B's glue point 3 (the middle of its left side) and its start to the whole of A (`lo/pinout/pin.json`, the `point-` rows).
const POINT: [(&str, &str, [f64; 4]); 16] = [
    ("standard", "right", [7.0, 11.0, 10.54, 15.0]),
    ("standard", "left", [3.0, 11.0, 5.46, 15.0]),
    ("standard", "down", [5.0, 12.0, 8.0, 17.54]),
    ("standard", "up", [5.0, 12.0, 8.0, 12.46]),
    ("lines", "right", [7.0, 11.0, 10.54, 15.0]),
    ("lines", "left", [5.0, 12.0, 5.46, 15.0]),
    ("lines", "down", [7.0, 11.0, 8.0, 17.54]),
    ("lines", "up", [7.0, 11.0, 8.0, 12.46]),
    ("line", "right", [7.0, 11.0, 10.54, 15.0]),
    ("line", "left", [5.0, 12.0, 5.46, 15.0]),
    ("line", "down", [7.0, 11.0, 8.0, 17.54]),
    ("line", "up", [7.0, 11.0, 8.0, 12.46]),
    ("curve", "right", [7.0, 11.0, 10.54, 15.0]),
    ("curve", "left", [3.0, 11.0, 5.46, 15.0]),
    ("curve", "down", [5.0, 12.0, 8.0, 17.54]),
    ("curve", "up", [5.0, 12.0, 8.0, 12.46]),
];

/// A rectangle called `name` with `xml:id` and `draw:id` `id`, `w` by `h` cm at (`x`, `y`) cm from the page's top left.
fn rectangle(name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    format!(
        r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" xml:id="{id}" draw:id="{id}" svg:width="{w}cm" svg:height="{h}cm" svg:x="{x}cm" svg:y="{y}cm"><text:p/></draw:rect>"#
    )
}

/// The drawing: A (4 cm by 2 cm at (3 cm, 10 cm)), B (3 cm by 2 cm at (8 cm, 14 cm)) and connector C of `kind` from A to B, glued to the whole of A and to the whole of B or,
/// with `point`, to B's glue point 3; its stated ends at A's middle and B's (LibreOffice reads neither of an end glued to the whole of a shape).
fn drawing(kind: &str, point: bool) -> Drawing {
    let glue = if point { r#" draw:end-glue-point="3""# } else { "" };
    let (x2, y2) = if point { (8.0, 15.0) } else { (9.5, 15.0) };
    let connector = format!(
        r#"<draw:connector draw:style-name="gr2" draw:text-style-name="P1" draw:layer="layout" draw:name="C" draw:type="{kind}" draw:start-shape="a" draw:end-shape="b"{glue} svg:x1="5cm" svg:y1="11cm" svg:x2="{x2}cm" svg:y2="{y2}cm"><text:p/></draw:connector>"#
    );
    Drawing::new().page(&format!("{}{}{connector}", rectangle("A", "a", (3.0, 10.0, 4.0, 2.0)), rectangle("B", "b", (8.0, 14.0, 3.0, 2.0))))
}

/// Centimetres from the page's top left as page inches, y up.
fn page((x, y): (f64, f64)) -> (f64, f64) {
    (x / 2.54, PAGE_H - y / 2.54)
}

/// C's begin and end in the session's model, page inches with y up.
fn ends(s: &Session) -> [(f64, f64); 2] {
    let doc = s.document().unwrap();
    let id = id_of(doc, 0, "C");
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap();
    let n = |cell: &str| shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    [(n("BeginX"), n("BeginY")), (n("EndX"), n("EndY"))]
}

fn attr(element: &str, name: &str) -> Option<String> {
    let at = element.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(element[at..at + element[at..].find('"')?].to_owned())
}

/// A length the writer wrote (`cm`, `mm`, `in`), in centimetres.
fn centimetres(text: &str) -> f64 {
    let (n, unit) = text.split_at(text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len()));
    let n: f64 = n.trim().parse().unwrap_or_else(|_| panic!("a length: {text}"));
    n * match unit {
        "cm" => 1.0,
        "mm" => 0.1,
        "in" => 2.54,
        other => panic!("a unit the writer writes: {other}"),
    }
}

#[track_caller]
fn near(got: (f64, f64), want: (f64, f64), what: &str) {
    let off = (got.0 - want.0).hypot(got.1 - want.1);
    assert!(off < NEAR, "{what}: at ({:.4}, {:.4}), LibreOffice's at ({:.4}, {:.4}) ({off:.4} inch off)", got.0, got.1, want.0, want.1);
}

/// Opens the drawing, moves B as `mv` says, and checks C's ends in the session and in the saved file against LibreOffice's.
fn check(kind: &str, point: bool, mv: &str, want: [f64; 4]) {
    let (_, dx, dy) = MOVES.iter().find(|m| m.0 == mv).copied().unwrap();
    let what = format!("{kind}, {}, B moved {mv}", if point { "the end glued to B's point 3" } else { "both ends glued to the whole of a shape" });
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing(kind, point));
    let b = id_of(s.document().unwrap(), 0, "B");
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": dx, "dy": dy }));
    let [begin, end] = ends(&s);
    near(begin, page((want[0], want[1])), &format!("{what}: the begin"));
    near(end, page((want[2], want[3])), &format!("{what}: the end"));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    let tag = content.match_indices("<draw:connector ").map(|(at, _)| &content[at..at + content[at..].find('>').unwrap()]).next().unwrap();
    let stated = |x: &str, y: &str| (centimetres(&attr(tag, x).unwrap()) / 2.54, centimetres(&attr(tag, y).unwrap()) / 2.54);
    near(stated("svg:x1", "svg:y1"), (want[0] / 2.54, want[1] / 2.54), &format!("{what}: the saved start"));
    near(stated("svg:x2", "svg:y2"), (want[2] / 2.54, want[3] / 2.54), &format!("{what}: the saved end"));
    assert_eq!(attr(tag, "draw:start-glue-point"), None, "{what}: the start stays glued to the whole of A: {tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").is_some(), point, "{what}: the end's glue as it was: {tag}");
}

/// The brief's test: a connector of each type glued to the whole of two shapes, one of them moved left, right, up and down: each end within 0.01 inch of LibreOffice's.
#[test]
fn gatef1_each_type_glued_to_the_whole_of_both_shapes_ends_where_libreoffice_puts_them_after_a_move() {
    for (kind, mv, want) in BOTH {
        check(kind, false, mv, want);
    }
}

/// One end glued to the whole of a shape, the other to a glue point of the shape that moves: the whole end takes LibreOffice's point beside the glued one.
#[test]
fn gatef1_each_type_glued_to_the_whole_of_one_shape_and_a_point_of_the_other_ends_where_libreoffice_puts_them() {
    for (kind, mv, want) in POINT {
        check(kind, true, mv, want);
    }
}

/// A VSDX connector glued to the whole of a shape is still placed by the connection point nearest the other end: is for an `.odg` connector only,
/// one the `.odg` reader read (`User.DiaCraftRoute`), which a connector of another file does not carry.
#[test]
fn a_connector_without_the_odg_readers_mark_is_placed_by_shape_glue() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing("standard", false));
    let c = id_of(s.document().unwrap(), 0, "C");
    // Take the reader's mark off C: what a connector of a VSDX file holds.
    s.edit("take the mark off", |doc| {
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).unwrap();
        let at = page.shapes.iter().position(|x| x.id == c).unwrap();
        let shape = page.shapes.get_mut(at).map(std::sync::Arc::make_mut).unwrap();
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|x| !(x.name.as_str() == "User" && x.rows.iter().any(|r| r.name.as_deref() == Some("DiaCraftRoute"))));
        Ok(())
    })
    .unwrap();
    let b = id_of(s.document().unwrap(), 0, "B");
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 0.0, "dy": -1.0 }));
    // The nearest pair: A's right side and B's top (LibreOffice's pick is A's bottom and B's top, `BOTH`).
    let [begin, end] = ends(&s);
    near(begin, page((7.0, 11.0)), "The begin on A's right side");
    near(end, page((9.5, 16.54)), "The end on B's top");
}
