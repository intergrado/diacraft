//! The Text tool's box: `text.box` makes a shape of text with no master, through `Session::execute` as the control channel
//! calls it. Lengths are paper inches on pages of the default scale; the boxes are about an inch by a quarter of an inch with 12 point text, as a
//! text box is.

use diacraft_doc::{CellAddr, Document, Links, Lookup, Owner};
use diacraft_engine::draw::{connection_points, shape_corners, text_view};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value as Json, json};

use crate::edit::{doc, geometry, id_of, route};
use crate::edit_foreign::Dir;
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    Renderer::new().render(doc, 1, &view)
}

/// The pixel at a place on the page, in inches from its left and bottom edges: premultiplied RGBA.
fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2], p[3]]
}

fn dark(p: [u8; 4]) -> bool {
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> usize {
    let mut n = 0;
    let (mut y, step) = (bottom, 1.0 / PX_PER_INCH);
    while y < top {
        let mut x = left;
        while x < right {
            n += usize::from(dark(px(r, x, y)));
            x += step;
        }
        y += step;
    }
    n
}

/// What shape `id` holds locally in a cell: its formula (if any) and its value text.
fn local(d: &Document, id: u32, cell: &str) -> Option<(Option<String>, Option<String>)> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    match shape.sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

fn near_p(a: (f64, f64), b: (f64, f64)) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

#[test]
fn a_text_box_is_a_shape_with_no_master_that_holds_its_own_cells() {
    let mut s = blank_session();
    let r = run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "Hello"}));
    assert_eq!(r, json!({"page": 1, "id": 1, "width": 1.0, "height": 0.25}));
    let d = doc(&s);
    let shape = shape_of(d, 1);
    assert_eq!((shape.master, shape.master_shape), (None, None), "No master");
    assert!(d.masters.is_empty(), "and none is added to the drawing");
    assert_eq!(shape.sheet.links, Links { line: Some(0), fill: Some(0), text: Some(0) }, "linked to the root style, as the shapes of files are");
    assert_eq!(text_view(d, 1, 1).unwrap().chars, "Hello");
    // The cells a shape made from a master has, with the formulas written on creation, and the seven of the text block.
    let cells: [(&str, Option<&str>, &str); 18] = [
        ("PinX", None, "3"),
        ("PinY", None, "4"),
        ("Width", None, "1"),
        ("Height", None, "0.25"),
        ("LocPinX", Some("Width*0.5"), "0.5"),
        ("LocPinY", Some("Height*0.5"), "0.125"),
        ("Angle", None, "0"),
        ("FlipX", None, "0"),
        ("FlipY", None, "0"),
        ("TxtPinX", Some("Width*0.5"), "0.5"),
        ("TxtPinY", Some("Height*0.5"), "0.125"),
        ("TxtLocPinX", Some("TxtWidth*0.5"), "0.5"),
        ("TxtLocPinY", Some("TxtHeight*0.5"), "0.125"),
        ("TxtWidth", Some("Width*1"), "1"),
        ("TxtHeight", Some("Height*1"), "0.25"),
        ("TxtAngle", None, "0"),
        // It paints nothing until a fill or a line is chosen.
        ("FillPattern", None, "0"),
        ("LinePattern", None, "0"),
    ];
    for (name, formula, value) in cells {
        assert_eq!(local(d, 1, name), Some((formula.map(str::to_owned), Some(value.to_owned()))), "{name}");
    }
    // One rectangle that fills the box, so that a fill and a line have a path to take, and the four middles of its sides to glue to.
    let rows = geometry(d, 1);
    let want = [("MoveTo", 0.0, 0.0), ("LineTo", 1.0, 0.0), ("LineTo", 1.0, 0.25), ("LineTo", 0.0, 0.25), ("LineTo", 0.0, 0.0)];
    assert_eq!(rows.len(), want.len());
    for ((kind, x, y), (want_kind, wx, wy)) in rows.iter().zip(want) {
        assert!(kind == want_kind && near(*x, wx) && near(*y, wy), "{rows:?}");
    }
    let points = connection_points(d, 1, 1);
    assert_eq!(points.len(), 4);
    let at: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
    assert!(near_p(at[0], (3.0, 4.125)) && near_p(at[1], (2.5, 4.0)) && near_p(at[2], (3.0, 3.875)) && near_p(at[3], (3.5, 4.0)), "{at:?}");
}

#[test]
fn a_box_made_without_a_size_is_an_inch_by_a_quarter_and_a_drag_gives_the_size() {
    let mut s = blank_session();
    let r = run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1.0), Some(0.25)));
    assert!(shape_of(doc(&s), 1).text.is_none(), "no text when none is given");
    // The place is the centre of the box.
    let c = shape_corners(doc(&s), 1, 1).unwrap();
    assert!(near_p(c[0], (2.5, 3.875)) && near_p(c[1], (3.5, 3.875)) && near_p(c[2], (3.5, 4.125)) && near_p(c[3], (2.5, 4.125)), "{c:?}");
    // A drag gives a centre and a size.
    let r = run(&mut s, "text.box", json!({"page": 1, "x": 5, "y": 2, "width": 2.5, "height": 0.5, "text": ""}));
    assert_eq!(r, json!({"page": 1, "id": 2, "width": 2.5, "height": 0.5}));
    let c = shape_corners(doc(&s), 1, 2).unwrap();
    assert!(near_p(c[0], (3.75, 1.75)) && near_p(c[2], (6.25, 2.25)), "{c:?}");
    assert!(shape_of(doc(&s), 2).text.is_none(), "empty text is no text");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_text_box_is_one_undo_step_and_an_id_is_never_given_twice() {
    let mut s = blank_session();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "one"}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Add text box"));
    let after = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before, "undo takes the whole box back");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    // The ids of shapes that went are not given again.
    let two = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 5})));
    let three = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 6})));
    assert_eq!((two, three), (2, 3));
    run(&mut s, "shape.delete", json!({"ids": [three]}));
    let next = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 7})));
    assert_eq!(next, 4, "3 was a shape's");
    let dropped = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 8, "y": 4})));
    assert_eq!(dropped, 5);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_text_box_that_cannot_be_made_is_refused_in_words_and_changes_nothing() {
    let long = "x".repeat(100_001);
    let cases: Vec<(Json, &str)> = vec![
        (json!({"page": 1, "x": 3, "y": 4, "width": 0}), "more than 0"),
        (json!({"page": 1, "x": 3, "y": 4, "width": -1}), "more than 0"),
        (json!({"page": 1, "x": 3, "y": 4, "height": 0}), "more than 0"),
        (json!({"page": 1, "x": 1.0e9, "y": 4}), "within"),
        (json!({"page": 1, "x": 3, "y": 4, "width": 1.0e9}), "within"),
        (json!({"page": 1, "x": 3, "y": 4, "text": "a\u{1}b"}), "control"),
        (json!({"page": 1, "x": 3, "y": 4, "text": long}), "at most"),
        (json!({"page": 1, "x": 3, "y": 4, "text": 5}), "must be text"),
        (json!({"page": 1, "x": 3, "y": 4, "width": "wide"}), "must be a number"),
        (json!({"page": 99, "x": 3, "y": 4}), "no page 99"),
        (json!({"x": 3, "y": 4}), "missing `page`"),
        (json!({"page": 1, "y": 4}), "missing `x`"),
    ];
    for (params, needle) in cases {
        let mut s = blank_session();
        let before = snapshot(&s);
        let e = s.execute("text.box", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(needle), "{}: {e}", params.to_string().chars().take(80).collect::<String>());
        assert_eq!(snapshot(&s), before, "{needle}: nothing changed");
    }
    // A session with no document.
    let mut empty = Session::new();
    assert!(matches!(empty.execute("text.box", &json!({"page": 1, "x": 3, "y": 4})), Err(EngineError::Disabled { .. })));
}

#[test]
fn lengths_are_paper_inches_whatever_the_pages_scale() {
    use diacraft_doc::{CellEdit, SheetRef, Value, WriteOrigin};
    let mut s = blank_session();
    // Two drawing units to a paper inch.
    s.edit("scale", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct("DrawingScale"), CellEdit::Value(Value::from_text("2")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let r = run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "scaled"}));
    assert_eq!(r, json!({"page": 1, "id": 1, "width": 1.0, "height": 0.25}), "the answer is in paper inches");
    let d = doc(&s);
    for (cell, value) in [("PinX", "6"), ("PinY", "8"), ("Width", "2"), ("Height", "0.5"), ("TxtWidth", "2"), ("TxtHeight", "0.5")] {
        assert_eq!(local(d, 1, cell).unwrap().1.as_deref(), Some(value), "{cell} is in drawing units");
    }
}

#[test]
fn a_text_box_paints_its_text_and_nothing_else_until_it_is_given_a_fill_and_a_line() {
    let mut s = blank_session();
    let empty = render(&s);
    run(&mut s, "text.box", json!({"page": 1, "x": 4, "y": 3, "text": "Text"}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "everything was drawn: {:?}", r.skipped);
    // The box spans x 3.5 to 4.5 and y 2.875 to 3.125: the word is inked inside it and there is no ink outside it.
    assert!(ink(&r, 3.5, 2.85, 4.5, 3.15) > 15, "the word is drawn");
    for (left, bottom, right, top) in [(0.0, 0.0, 3.45, 8.5), (4.55, 0.0, 11.0, 8.5), (3.4, 3.2, 4.6, 8.5), (3.4, 0.0, 4.6, 2.8)] {
        assert_eq!(ink(&r, left, bottom, right, top), 0, "nothing outside the box: {left} {bottom} {right} {top}");
    }
    // No fill and no line: a corner and the top edge are the page's.
    assert_eq!(px(&r, 3.52, 2.885), px(&empty, 3.52, 2.885), "no fill");
    assert_eq!(ink(&r, 3.6, 3.115, 4.4, 3.135), 0, "no line");
    // A fill: makes the pattern solid, and the rectangle path takes the colour.
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#336699"}));
    let filled = render(&s);
    assert_eq!(px(&filled, 3.52, 2.885), [0x33, 0x66, 0x99, 255]);
    assert_eq!(px(&filled, 4.48, 3.115), [0x33, 0x66, 0x99, 255]);
    assert_eq!(px(&filled, 3.4, 3.0), px(&empty, 3.4, 3.0), "and only inside the box");
    // A line, 3 points wide, on the top edge.
    run(&mut s, "shape.format", json!({"ids": [1], "line": "#CC0000", "weight": 3}));
    let lined = render(&s);
    let top = px(&lined, 4.0, 3.125);
    assert!(top[0] > 150 && top[1] < 60 && top[2] < 60 && top[3] == 255, "{top:?}");
    // Taking the fill away leaves the page.
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "none", "line": "none"}));
    let bare = render(&s);
    assert_eq!(px(&bare, 3.52, 2.885), px(&empty, 3.52, 2.885));
    assert_eq!(ink(&bare, 3.6, 3.115, 4.4, 3.135), 0);
}

#[test]
fn a_text_box_takes_the_other_commands_a_shape_takes() {
    let mut s = blank_session();
    run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "Hello"}));
    // Text: set, formatted, read back.
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Hello world"}));
    run(&mut s, "text.format", json!({"ids": [1], "bold": true, "size": 18}));
    let look = run(&mut s, "shape.formatting", json!({"ids": [1]}));
    assert_eq!((look["shapes"][0]["size"].as_f64(), look["shapes"][0]["bold"].as_bool()), (Some(18.0), Some(true)), "{look}");
    // Duplicated: a new box with its own id, the text, and no master.
    let dup = run(&mut s, "shape.duplicate", json!({"ids": [1]}));
    let copy = u32::try_from(dup["ids"][0].as_u64().unwrap()).unwrap();
    assert_eq!(copy, 2);
    assert_eq!(text_view(doc(&s), 1, copy).unwrap().chars, "Hello world");
    assert_eq!(shape_of(doc(&s), copy).master, None);
    // Copy and paste.
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    let pasted = run(&mut s, "edit.paste", json!({"x": 8, "y": 2}));
    let third = u32::try_from(pasted["ids"][0].as_u64().unwrap()).unwrap();
    assert_eq!(text_view(doc(&s), 1, third).unwrap().chars, "Hello world");
    let c = shape_corners(doc(&s), 1, third).unwrap();
    assert!(near_p(((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0), (8.0, 2.0)), "{c:?}");
    // Grouped with a rectangle, and the text is still drawn where it was.
    let rect = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 6})));
    let before = render(&s);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, rect]})));
    let after = render(&s);
    assert_eq!(ink(&before, 2.4, 3.8, 3.6, 4.2), ink(&after, 2.4, 3.8, 3.6, 4.2), "the box draws where it did");
    assert!(ink(&after, 2.4, 3.8, 3.6, 4.2) > 20);
    run(&mut s, "shape.ungroup", json!({"ids": [group]}));
    // Turned: the text turns with the box.
    run(&mut s, "shape.rotate", json!({"ids": [copy], "angle": 90}));
    let turned = shape_corners(doc(&s), 1, copy).unwrap();
    assert!(turned[1].0 < turned[0].0 + 1e-9 && turned[1].1 > turned[0].1, "the box stands on end: {turned:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_is_glued_to_a_text_box_and_follows_it() {
    let mut s = blank_session();
    run(&mut s, "text.box", json!({"page": 1, "x": 2, "y": 2, "text": "from"}));
    let target = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 7, "y": 2})));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": target}})));
    let begin = |s: &Session| route(doc(s), connector)[0];
    let at_a_point = |s: &Session| connection_points(doc(s), 1, 1).iter().any(|p| near_p((p.x, p.y), begin(s)));
    assert!(at_a_point(&s), "the connector starts at a connection point of the box");
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1, "dy": 1.5}));
    assert!(at_a_point(&s), "and follows the box: {:?}", begin(&s));
    assert!(begin(&s).1 > 2.5);
    // The label of the connector is placed on its route, as for a connector between stencil shapes.
    run(&mut s, "shape.set_text", json!({"id": connector, "text": "yes"}));
    assert_eq!(text_view(doc(&s), 1, connector).unwrap().chars, "yes");
}

#[test]
fn a_text_box_is_saved_and_opened_again() {
    let dir = Dir::new();
    let path = dir.path("box.vsdx");
    let mut s = blank_session();
    run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "width": 1.75, "height": 0.4, "text": "Round trip"}));
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FFEEAA", "line": "#222222"}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let (a, b) = (doc(&s), doc(&t));
    assert!(b.masters.is_empty(), "still no master");
    assert_eq!(shape_of(b, 1).master, None);
    assert_eq!(text_view(b, 1, 1).unwrap().chars, "Round trip");
    for cell in [
        "PinX",
        "PinY",
        "Width",
        "Height",
        "LocPinX",
        "LocPinY",
        "TxtPinX",
        "TxtPinY",
        "TxtLocPinX",
        "TxtLocPinY",
        "TxtWidth",
        "TxtHeight",
        "TxtAngle",
        "FillPattern",
        "LinePattern",
        "FillForegnd",
        "LineColor",
    ] {
        assert_eq!(local(a, 1, cell), local(b, 1, cell), "{cell}");
    }
    assert_eq!(geometry(a, 1), geometry(b, 1));
    let (ra, rb) = (render(&s), render(&t));
    assert_eq!(ink(&ra, 0.0, 0.0, 11.0, 8.5), ink(&rb, 0.0, 0.0, 11.0, 8.5), "it looks as it did");
    assert!(ink(&rb, 2.0, 3.7, 4.0, 4.3) > 30);
    // A box made after the open does not take the id of one that was there.
    let next = id_of(&run(&mut t, "text.box", json!({"page": 1, "x": 6, "y": 4})));
    assert_eq!(next, 2);
}

/// What a cell of page 1's sheet says.
fn page_cell(s: &Session, name: &str) -> Option<String> {
    doc(s).page(1).unwrap().sheet.cell(name).and_then(|c| c.value.as_ref().map(|v| v.text().to_owned()))
}

fn pin_of(s: &Session, id: u32) -> (f64, f64) {
    let n = |name: &str| shape_of(doc(s), id).sheet.cell(name).unwrap().value.as_ref().unwrap().number().unwrap();
    (n("PinX"), n("PinY"))
}

#[test]
fn a_text_box_put_past_the_edge_of_a_page_with_auto_size_grows_it_by_whole_sheets_in_one_undo_step() {
    // `draw/page_fit.rs`: the page is 11 by 8.5 and its paper is Letter, so a sheet is 11 inches wide.
    let mut s = blank_session();
    let kept = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "kept"})));
    run(&mut s, "page.auto_size", json!({"on": true}));
    let before = snapshot(&s);
    // The box is 1 by 0.25: centred at 10.8 it reaches 11.3, past the edge at 11.
    let right = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 10.8, "y": 4, "text": "right"})));
    assert_eq!((page_cell(&s, "PageWidth").as_deref(), page_cell(&s, "PageHeight").as_deref()), (Some("22"), Some("8.5")), "one sheet");
    assert_eq!(pin_of(&s, right), (10.8, 4.0), "the box is where it was put");
    assert_eq!(pin_of(&s, kept), (3.0, 4.0), "growing on the right moves nothing");
    // The page and the box are one undo step, and undo takes both back.
    assert_eq!(s.history().unwrap().undo_len(), before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(page_cell(&s, "PageWidth").as_deref(), Some("22"));
    // A box 2 wide centred at 25 reaches 26, 4 past the edge at 22: one more sheet, and a box a quarter of an inch above the top of the page needs a
    // sheet of height.
    run(&mut s, "text.box", json!({"page": 1, "x": 25, "y": 4, "width": 2}));
    assert_eq!(page_cell(&s, "PageWidth").as_deref(), Some("33"), "4 past the edge is one sheet, not two");
    let top = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 5, "y": 8.45})));
    assert_eq!(page_cell(&s, "PageHeight").as_deref(), Some("17"));
    assert_eq!(pin_of(&s, top), (5.0, 8.45));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_text_box_put_past_the_left_edge_carries_the_drawing_with_it() {
    let mut s = blank_session();
    let kept = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "kept"})));
    let rect = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 7, "y": 4})));
    run(&mut s, "page.auto_size", json!({"on": true}));
    // Centred at -0.2 the box reaches -0.7: one sheet on the left.
    let left = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": -0.2, "y": 4, "text": "left"})));
    assert_eq!(page_cell(&s, "PageWidth").as_deref(), Some("22"));
    assert_eq!(pin_of(&s, kept), (14.0, 4.0), "the drawing moved with the origin");
    assert_eq!(pin_of(&s, rect), (18.0, 4.0));
    assert_eq!(pin_of(&s, left), (10.8, 4.0), "and the new box is inside the page");
    assert_eq!(text_view(doc(&s), 1, left).unwrap().chars, "left");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_with_auto_size_off_does_not_grow_for_a_text_box_and_the_block_commands_never_grow_it() {
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 10.8, "y": 4, "text": "off"})));
    assert_eq!((page_cell(&s, "PageWidth").as_deref(), page_cell(&s, "PageHeight").as_deref()), (Some("11"), Some("8.5")));
    // With Auto Size on, the text block commands move no shape's box, and a block moved past the edge leaves the page as it is: the specifications
    // give Auto Size for a shape put or moved past the edge, and say nothing of a text block.
    run(&mut s, "page.auto_size", json!({"on": true}));
    run(&mut s, "text.block_move", json!({"id": id, "dx": 9, "dy": 0}));
    run(&mut s, "text.block_resize", json!({"id": id, "x": 20, "y": 4, "width": 3, "height": 1}));
    assert_eq!((page_cell(&s, "PageWidth").as_deref(), page_cell(&s, "PageHeight").as_deref()), (Some("11"), Some("8.5")));
    assert_eq!(pin_of(&s, id), (10.8, 4.0));
}
