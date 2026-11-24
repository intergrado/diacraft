//! Change Shape: what a change made is saved and read back in each format: `.diacraft`, `.vsdx` (the model writer for a new drawing, the splice
//! into the file for one that was opened from it) and `.odg`. A format that cannot hold a master says so in the save's own note, and the shape is what it was drawn as.

use diacraft_doc::{Document, Owner, TextPart};
use diacraft_engine::Session;
use diacraft_engine::draw::shape_corners;
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, num};
use crate::stencil_open::Dir;
use crate::{blank_session, check_session, run};

const TOL: f64 = 1.0e-6;

/// What a change leaves of a shape that must come back from a file: the corners of its box on the page, its text, and where its connector ends are.
#[derive(Debug, PartialEq)]
struct Seen {
    corners: Vec<(i64, i64)>,
    text: String,
}

fn micro(v: f64) -> i64 {
    // The page's places to a millionth of an inch: what a file keeps of a number, so that a respelling is not a difference.
    (v * 1.0e6).round() as i64
}

fn seen(d: &Document, id: u32) -> Seen {
    let corners = shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}")).iter().map(|p| (micro(p.0), micro(p.1))).collect();
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    let text = shape
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect::<String>())
        .unwrap_or_default();
    Seen { corners, text }
}

fn master_name(d: &Document, id: u32) -> Option<String> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    d.master(shape.master?).and_then(|m| m.name.as_deref().map(str::to_owned))
}

fn connects(d: &Document) -> Vec<(u32, String, u32)> {
    d.page(1).unwrap().connects.iter().map(|c| (c.from_sheet, c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_sheet)).collect()
}

/// A process with a text, resized, joined by a connector to a rectangle, and the process changed to a decision and the rectangle to a hexagon.
fn changed() -> Session {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "process", 3.0, 2.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Check"}));
    run(&mut s, "shape.resize", json!({"id": a, "x": 1.9, "y": 1.45, "width": 2.2, "height": 1.1}));
    let b = drop_shape(&mut s, "rectangle", 7.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    assert_eq!((a, b, c), (1, 2, 3));
    run(&mut s, "shape.change", json!({"ids": [a], "master": "decision"}));
    run(&mut s, "shape.change", json!({"ids": [b], "master": "hexagon"}));
    s
}

fn reopened(path: &str) -> Session {
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": path }));
    assert_eq!(check_session(&t), Ok(()));
    t
}

#[test]
fn a_changed_drawing_is_saved_and_read_back_in_each_format() {
    let dir = Dir::new();
    let mut s = changed();
    let was: Vec<Seen> = [1, 2].iter().map(|id| seen(doc(&s), *id)).collect();
    assert_eq!(was[0].text, "Check");
    let glue = connects(doc(&s));
    assert_eq!(glue.len(), 2);
    let end = (num(doc(&s), 3, "BeginX"), num(doc(&s), 3, "BeginY"), num(doc(&s), 3, "EndX"), num(doc(&s), 3, "EndY"));
    for name in ["out.diacraft", "out.vsdx", "out.odg"] {
        let path = dir.path(name);
        run(&mut s, "file.save_as", json!({ "path": path }));
        let t = reopened(&path);
        let d = doc(&t);
        let got: Vec<Seen> = [1, 2].iter().map(|id| seen(d, *id)).collect();
        assert_eq!(got, was, "{name}: the shapes are where and as big as they were, with their text");
        assert_eq!(connects(d), glue, "{name}: the connector is still glued to the same shapes");
        for (cell, want) in [("BeginX", end.0), ("BeginY", end.1), ("EndX", end.2), ("EndY", end.3)] {
            assert!((num(d, 3, cell) - want).abs() < TOL, "{name}: {cell}");
        }
        if name != "out.odg" {
            // An OpenDocument drawing has no masters: the shape is drawn as the master draws it and the file says nothing of the master's name.
            assert_eq!(master_name(d, 1).as_deref(), Some("Decision"), "{name}");
            assert_eq!(master_name(d, 2).as_deref(), Some("Hexagon"), "{name}");
        }
    }
}

#[test]
fn a_shape_changed_in_a_drawing_opened_from_a_vsdx_file_is_spliced_into_it_and_read_back() {
    let dir = Dir::new();
    let mut s = changed();
    let first = dir.path("first.vsdx");
    run(&mut s, "file.save_as", json!({ "path": first }));
    // Open the file, change the decision to an ellipse, and save into the same file.
    let mut t = reopened(&first);
    run(&mut t, "shape.change", json!({"ids": [1], "master": "ellipse"}));
    let was = seen(doc(&t), 1);
    let steps = connects(doc(&t));
    run(&mut t, "file.save", json!({}));
    let u = reopened(&first);
    assert_eq!(master_name(doc(&u), 1).as_deref(), Some("Ellipse"));
    assert_eq!(seen(doc(&u), 1), was);
    assert_eq!(connects(doc(&u)), steps);
    assert_eq!(seen(doc(&u), 1).text, "Check");
}
