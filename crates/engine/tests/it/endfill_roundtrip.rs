//! What the inside of a hollow line end shows is the shape's fill, so a drawing whose connector has a fill and a hollow end must keep that look through every
//! format. No cell is new: `FillForegnd`, `FillBkgnd`, `FillPattern`, the transparency and the gradient cells are the ones a 1-D shape already has. The files are saved by the model
//! writers (a new drawing), opened in a session of their own, and **drawn**: the picture of the file read again is the picture of the session that saved it.

use diacraft_engine::Session;
use diacraft_engine::formatting::{Rgb, look_of};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::json;

use crate::odg_build::Dir;
use crate::{blank_session, run};

const PPI: f64 = 100.0;

pub fn render(s: &Session) -> Rendered {
    let doc = s.document().expect("a document");
    let view = View::fit_page(doc, 1, PPI).expect("the page has a size");
    Renderer::new().render(doc, 1, &view)
}

/// The pixel at a place on the page, in inches from its left and bottom edges (the page is as tall as the picture: a template's page may be portrait).
pub fn px(r: &Rendered, x: f64, y: f64) -> [u8; 3] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PPI) as usize).min(width - 1);
    let row = (((height as f64 / PPI - y) * PPI) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    [r.rgba_premul[at], r.rgba_premul[at + 1], r.rgba_premul[at + 2]]
}

/// A drawing of two boxes joined by a straight connector (its id is 3) that has a hollow square at its end (21, size 6) and the fill `fill` (a colour, or `none`).
fn connected(fill: &str) -> Session {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 2.0, "y": 4.0}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 7.0, "y": 4.0}));
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    run(&mut s, "shape.format", json!({"ids": [3], "fill": fill, "end_arrow": 21, "end_arrow_size": 6, "line": "#000000", "weight": 1.5}));
    s
}

/// The connector's end, from the model: where the line stops. The connector runs between the boxes' sides at y 4.
pub fn end_of(s: &Session) -> (f64, f64) {
    let doc = s.document().expect("a document");
    let page = doc.pages.get(0).expect("a page").id;
    let at = diacraft_engine::draw::connector_path(doc, page, 3).expect("a connector").points;
    let last = at.last().copied().expect("a vertex");
    (last.0, last.1)
}

/// Whether the pixel is this colour within a few steps.
pub fn is(p: [u8; 3], rgb: [u8; 3]) -> bool {
    p.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 12)
}

#[test]
fn a_connector_with_a_hollow_end_shows_its_fill_inside_it() {
    let s = connected("#FF0000");
    let (x, y) = end_of(&s);
    // A point inside the square, 0.08 inch before the end: on the paper beside the box the connector ends on (the box's own edge runs through the end itself).
    let (x, y) = (x - 0.08, y);
    let r = render(&s);
    assert!(is(px(&r, x, y), [255, 0, 0]), "red inside the square at the end ({x}, {y}): {:?}", px(&r, x, y));
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    // No fill: what is behind shows through the square, here the paper.
    let none = connected("none");
    let r = render(&none);
    assert!(is(px(&r, x, y), [255, 255, 255]), "no fill, nothing inside: {:?}", px(&r, x, y));
}

#[test]
fn a_connector_given_its_styles_fill_back_by_use_default_is_white_inside_its_hollow_end() {
    // A connector DiaCraft makes has No Fill (`connector_default_fill.rs`). The style's own fill, solid white (the root style's `FillPattern` 1 and `FillForegnd` 1), is what Use Default
    // gives it back, and then the inside of its hollow end is white, over what lies behind it (the owner's rule: the fill shows). The end 21 is centred on the vertex, which lies on the edge of the box
    // the connector ends on: the white inside covers the box's outline there.
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 2.0, "y": 4.0}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 7.0, "y": 4.0}));
    run(&mut s, "shape.format", json!({"ids": [1, 2], "fill": "#3366CC"}));
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    run(&mut s, "shape.format", json!({"ids": [3], "end_arrow": 21, "end_arrow_size": 6, "weight": 1.5}));
    let (x, y) = end_of(&s);
    assert!(is(px(&render(&s), x + 0.08, y), [0x33, 0x66, 0xCC]), "a new connector is see-through inside: {:?}", px(&render(&s), x + 0.08, y));
    run(&mut s, "shape.format", json!({"ids": [3], "fill": "default"}));
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let look = look_of(doc, page, &[3]).unwrap().remove(0);
    assert_eq!(look.fill, Some(Rgb(255, 255, 255)), "the connector's fill is its style's white");
    assert!(is(px(&render(&s), x + 0.08, y), [255, 255, 255]), "white inside the square, over the blue box: {:?}", px(&render(&s), x + 0.08, y));
}

pub fn reopened(dir: &Dir, name: &str) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path(name) }));
    again
}

#[test]
fn the_look_of_a_filled_hollow_end_is_kept_through_diacraft_vsdx_and_odg_files() {
    for (name, fill) in
        [("a.diacraft", "#FF0000"), ("a.vsdx", "#FF0000"), ("a.odg", "#FF0000"), ("b.diacraft", "none"), ("b.vsdx", "none"), ("b.odg", "none")]
    {
        let dir = Dir::new();
        let mut s = connected(fill);
        run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
        let again = reopened(&dir, name);
        let (a, b) = (s.document().unwrap(), again.document().unwrap());
        let page = |d: &diacraft_engine::doc::Document| d.pages.get(0).unwrap().id;
        let ids = |d: &diacraft_engine::doc::Document| d.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect::<Vec<_>>();
        let connector = *ids(b).iter().find(|id| **id == 3).unwrap_or_else(|| panic!("{name}: the connector is in the file: {:?}", ids(b)));
        let (la, lb) = (look_of(a, page(a), &[3]).unwrap().remove(0), look_of(b, page(b), &[connector]).unwrap().remove(0));
        assert_eq!(la.fill, lb.fill, "{name}: the fill is the same after a round trip");
        assert_eq!(la.line, lb.line, "{name}: and so is the line");
        // The picture of the file read again shows what the session showed inside the mark.
        let (x, y) = end_of(&s);
        let (ra, rb) = (render(&s), render(&again));
        assert!(
            is(px(&ra, x, y), px(&rb, x, y)),
            "{name}: inside the mark the file reads as the session draws: {:?} against {:?}",
            px(&rb, x, y),
            px(&ra, x, y)
        );
        if fill == "#FF0000" {
            assert!(is(px(&rb, x, y), [255, 0, 0]), "{name}: red inside the mark of the file read again: {:?}", px(&rb, x, y));
        }
    }
}

/// The classes a save as `.odg` says differ, as `(class, count)`.
fn classes_of(s: &mut Session, dir: &Dir, name: &str) -> Vec<(String, u64)> {
    let saved = run(s, "file.save_as", json!({ "path": dir.path(name) }));
    saved["checked"]["classes"]
        .as_array()
        .map(|list| list.iter().map(|c| (c["class"].as_str().unwrap_or_default().to_owned(), c["count"].as_u64().unwrap_or(0))).collect())
        .unwrap_or_default()
}

#[test]
fn an_odg_save_of_a_connector_with_a_fill_of_its_own_loses_nothing() {
    // The save is read back and compared with the drawing, class by class. A fill that did not come back would be a class of its own (`style-content`: it was one before the fill of a
    // 1-D shape was written). The colours `shape.format` sets are written as a formula, `RGB(r,g,b)`, which ODF holds as a colour: the formula of the line's and of the fill's came back as a value
    // only, and the check named them (`formulas`: two for a fill and a line, one for a line) until a later change, which has a drawing's own formulas ride in DiaCraft's data beside the colour.
    let dir = Dir::new();
    for fill in ["#FF0000", "none"] {
        let mut s = connected(fill);
        assert_eq!(classes_of(&mut s, &dir, "filled.odg"), Vec::<(String, u64)>::new(), "fill {fill}");
    }
}

#[test]
fn a_fill_set_on_a_connector_of_an_odg_file_is_saved_into_the_file_and_comes_back() {
    // The save in place (a splice into the file's own bytes): a connector the file holds is given a fill, the file is saved and opened in a session of its own, and the fill is there and
    // shows inside the hollow end. A second save with nothing changed is no edit.
    let dir = Dir::new();
    let mut first = connected("none");
    run(&mut first, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let mut s = reopened(&dir, "a.odg");
    run(&mut s, "shape.format", json!({"ids": [3], "fill": "#00AA00"}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "the fill is an edit: {saved}");
    let again = reopened(&dir, "a.odg");
    let doc = again.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    assert_eq!(look_of(doc, page, &[3]).unwrap().remove(0).fill, Some(Rgb(0, 170, 0)), "the file holds the connector's fill");
    let (x, y) = end_of(&again);
    assert!(
        is(px(&render(&again), x - 0.08, y), [0, 170, 0]),
        "green inside the hollow end of the file read again: {:?}",
        px(&render(&again), x - 0.08, y)
    );
    let mut resaved = reopened(&dir, "a.odg");
    assert_eq!(run(&mut resaved, "file.save", json!({}))["edited"], false, "a save with nothing changed is no edit");
}
