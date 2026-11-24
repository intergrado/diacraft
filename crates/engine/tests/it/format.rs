//! The format commands: `shape.format` sets the fill and line cells of shapes, through `Session::execute` exactly as the control channel's
//! `engine.execute` calls it. A colour is written the way a file holds one (`F="RGB(r,g,b)"`, `V="#rrggbb"` in lower case),
//! a weight is in points on the command and in inches in the cell,
//! and a value written under a theme beats the theme's for that cell.

use diacraft_doc::{Cell, CellAddr, Document, Lookup, Owner};
use diacraft_engine::Session;
use diacraft_render::{Rendered, Renderer, View};
use serde_json::json;

use crate::edit::{doc, drop_shape, num};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

pub(crate) fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    Renderer::new().render(doc, 1, &view)
}

/// The pixel at a place of the page, in inches from its left and bottom edges: premultiplied RGBA.
pub(crate) fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2], p[3]]
}

/// Whether a pixel is within `by` of a colour in each channel.
pub(crate) fn close(p: [u8; 4], rgb: (u8, u8, u8), by: u8) -> bool {
    p[3] == 255 && p[0].abs_diff(rgb.0) <= by && p[1].abs_diff(rgb.1) <= by && p[2].abs_diff(rgb.2) <= by
}

/// How many pixels of a rectangle of the page (left, bottom, right, top, in inches) are within `by` of a colour.
pub(crate) fn count(r: &Rendered, rect: (f64, f64, f64, f64), rgb: (u8, u8, u8), by: u8) -> usize {
    let mut n = 0;
    let mut y = rect.1;
    while y < rect.3 {
        let mut x = rect.0;
        while x < rect.2 {
            n += usize::from(close(px(r, x, y), rgb, by));
            x += 1.0 / PX_PER_INCH;
        }
        y += 1.0 / PX_PER_INCH;
    }
    n
}

/// The cell directly under shape `id` of page 1 as the shape itself holds it (not inherited).
pub(crate) fn local(d: &Document, id: u32, name: &str) -> Option<Cell> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id)?)?;
    match shape.sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => Some(c.clone()),
        _ => None,
    }
}

/// The formula and the value of a cell, as text.
pub(crate) fn fv(c: &Cell) -> (Option<String>, String) {
    (c.formula.text().map(str::to_owned), c.value.as_ref().map_or_else(String::new, |v| v.text().to_owned()))
}

/// The number a local cell holds.
pub(crate) fn number(c: &Cell) -> f64 {
    c.value.as_ref().and_then(|v| v.number()).unwrap_or_else(|| panic!("{c:?} holds no number"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-12
}

/// 0.75 point in inches: what a file holds for the stroke most files use, `0.01041666666666667`.
const POINT_75: f64 = 0.75 / 72.0;

fn session_with_rectangle() -> (Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    (s, id)
}

// ---- fill and line -------------------------------------------------------------------------------------------------------------

#[test]
fn a_fill_colour_is_written_as_a_file_holds_one_and_the_shape_is_drawn_in_it() {
    let (mut s, id) = session_with_rectangle();
    assert_eq!(px(&render(&s), 3.0, 4.0), [255, 255, 255, 255], "white before");
    let r = run(&mut s, "shape.format", json!({"ids": [id], "fill": "#3C78D8"}));
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    let c = local(doc(&s), id, "FillForegnd").expect("a local FillForegnd");
    assert_eq!(fv(&c), (Some("RGB(60,120,216)".to_owned()), "#3c78d8".to_owned()), "RGB() and its lower-case hex");
    assert!(local(doc(&s), id, "FillPattern").is_none(), "the root's solid pattern stands: nothing else is written");
    assert!(local(doc(&s), id, "LineColor").is_none(), "a fill leaves the line alone");
    assert_eq!(px(&render(&s), 3.0, 4.0), [60, 120, 216, 255]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_line_colour_a_weight_and_a_pattern_are_written_in_the_units_of_the_cells() {
    let (mut s, id) = session_with_rectangle();
    // 6 points wide and red: the left edge of the box at x 2.25 is red across 0.083 inch.
    run(&mut s, "shape.format", json!({"ids": [id], "line": "#CC0000", "weight": 6}));
    let d = doc(&s);
    assert_eq!(fv(&local(d, id, "LineColor").unwrap()), (Some("RGB(204,0,0)".to_owned()), "#cc0000".to_owned()));
    let w = local(d, id, "LineWeight").unwrap();
    assert!(w.formula.text().is_none(), "a weight is a plain value");
    assert!(near(number(&w), 6.0 / 72.0), "6 points are {} inch", number(&w));
    assert!(close(px(&render(&s), 2.25, 4.0), (204, 0, 0), 4), "{:?}", px(&render(&s), 2.25, 4.0));

    // 0.75 point, the weight most files use: 0.01041666666666667 inch.
    run(&mut s, "shape.format", json!({"ids": [id], "weight": 0.75}));
    assert!(near(number(&local(doc(&s), id, "LineWeight").unwrap()), POINT_75));
    assert!(near(num(doc(&s), id, "LineWeight"), POINT_75), "resolved: the local value wins over the root's");

    // A dashed pattern leaves gaps along the edge that a solid one has none of.
    run(&mut s, "shape.format", json!({"ids": [id], "weight": 3}));
    let solid = count(&render(&s), (2.2, 3.55, 2.3, 4.45), (204, 0, 0), 40);
    run(&mut s, "shape.format", json!({"ids": [id], "pattern": 2}));
    assert_eq!(number(&local(doc(&s), id, "LinePattern").unwrap()), 2.0);
    let dashed = count(&render(&s), (2.2, 3.55, 2.3, 4.45), (204, 0, 0), 40);
    assert!(dashed > 0 && dashed < solid * 9 / 10, "dashed {dashed} against solid {solid}");
}

#[test]
fn none_takes_the_fill_or_the_line_away_and_a_colour_after_it_brings_it_back() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "none", "line": "none"}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "FillPattern").unwrap()), 0.0, "no fill is pattern 0");
    assert_eq!(number(&local(d, id, "LinePattern").unwrap()), 0.0, "no line is pattern 0");
    assert!(local(d, id, "FillForegnd").is_none() && local(d, id, "LineColor").is_none(), "the colours are not touched");
    let r = render(&s);
    assert_eq!(px(&r, 3.0, 4.0), [255, 255, 255, 255]);
    assert!(close(px(&r, 3.0, 4.0), (255, 255, 255), 0), "paper shows through");
    assert_eq!(count(&r, (2.2, 3.5, 2.3, 4.5), (0, 0, 0), 60), 0, "and there is no edge");

    // A colour on a shape that has none puts the pattern back to solid, so that it shows; the line the same.
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#FF9900", "line": "#009900"}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "FillPattern").unwrap()), 1.0);
    assert_eq!(number(&local(d, id, "LinePattern").unwrap()), 1.0);
    assert_eq!(px(&render(&s), 3.0, 4.0), [255, 153, 0, 255]);
}

#[test]
fn a_colour_leaves_a_hatch_pattern_as_it_is_and_turns_a_gradient_off() {
    let (mut s, id) = session_with_rectangle();
    // Pattern 2 is a hatch of the foreground colour: the colour is the hatch's, so the pattern stays. A gradient would hide the colour.
    s.edit("test", |d| {
        for (name, value) in [("FillPattern", "2"), ("FillGradientEnabled", "1")] {
            let at = d.find_shape(Owner::Page(1), id).unwrap();
            d.write_cell(
                &diacraft_doc::SheetRef::Shape(at),
                &CellAddr::direct(name),
                diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text(value)),
                diacraft_doc::WriteOrigin::Drawing,
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#336699"}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "FillPattern").unwrap()), 2.0, "a hatch stays");
    assert_eq!(number(&local(d, id, "FillGradientEnabled").unwrap()), 0.0, "the gradient is off");
}

#[test]
fn arrowheads_and_rounding_go_to_a_connector_and_a_shape_and_draw() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let head = |s: &Session, x: f64| count(&render(s), (x, 1.8, x + 0.01, 2.2), (0, 0, 0), 120);
    let shaft = head(&s, 4.0);
    let tip = head(&s, 5.16);
    assert!(tip >= shaft + 3, "the default end arrow 4 is a triangle: {tip} against {shaft}");
    // End arrow 0 and a begin arrow 13.
    run(&mut s, "shape.format", json!({"ids": [c], "end_arrow": 0, "begin_arrow": 13}));
    let d = doc(&s);
    assert_eq!(number(&local(d, c, "EndArrow").unwrap()), 0.0);
    assert_eq!(number(&local(d, c, "BeginArrow").unwrap()), 13.0);
    assert!(head(&s, 5.16) <= head(&s, 4.0) + 1, "no head at the end now");
    assert!(head(&s, 2.8) >= head(&s, 4.0) + 3, "a long triangle at the begin: {} against {}", head(&s, 2.8), head(&s, 4.0));
    // Rounding takes inches on the paper; the page is drawn one to one, so the cell holds the same.
    run(&mut s, "shape.format", json!({"ids": [a], "rounding": 0.25}));
    assert_eq!(number(&local(doc(&s), a, "Rounding").unwrap()), 0.25);
    // The sharp corner of the box at (1.25, 1.5) is cut: its pixel is paper now.
    let r = render(&s);
    assert_eq!(px(&r, 1.255, 1.505)[3], 255);
    assert!(close(px(&r, 1.255, 1.505), (255, 255, 255), 0), "a rounded corner shows the paper, not the edge: {:?}", px(&r, 1.255, 1.505));
}

/// (the Format Shape pane's Line section has **Begin Arrow size** and **End Arrow size**, a combo each, Medium by default; the size classes 0 to 6 of `BeginArrowSize` and `EndArrowSize`).
/// `shape.format` takes `begin_arrow_size` and `end_arrow_size`, 0 to 6, and writes
/// those cells; `shape.formatting` reads them back (2, Medium, where a shape holds none); a bigger class draws a bigger head.
#[test]
fn the_size_of_an_arrowhead_is_a_cell_and_a_bigger_class_draws_a_bigger_head() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let formatting = |s: &mut Session| run(s, "shape.formatting", json!({"ids": [c]}))["shapes"][0].clone();
    let both = formatting(&mut s);
    assert_eq!((both["begin_arrow_size"].as_u64(), both["end_arrow_size"].as_u64()), (Some(2), Some(2)), "Medium until one is chosen: {both}");
    // The widest column of dark pixels along the last half inch of the connector: the head's base.
    let widest = |s: &Session| {
        (0..60).map(|i| count(&render(s), (4.7 + f64::from(i) * 0.01, 1.5, 4.71 + f64::from(i) * 0.01, 2.5), (0, 0, 0), 120)).max().unwrap_or(0)
    };
    run(&mut s, "shape.format", json!({"ids": [c], "end_arrow_size": 0}));
    let small = widest(&s);
    assert_eq!(number(&local(doc(&s), c, "EndArrowSize").unwrap()), 0.0);
    run(&mut s, "shape.format", json!({"ids": [c], "end_arrow_size": 5}));
    let big = widest(&s);
    assert_eq!(number(&local(doc(&s), c, "EndArrowSize").unwrap()), 5.0);
    assert!(big >= small + 6, "a jumbo head is wider than a very small one: {big} against {small}");
    let now = formatting(&mut s);
    assert_eq!((now["begin_arrow_size"].as_u64(), now["end_arrow_size"].as_u64()), (Some(2), Some(5)), "the begin end is as it was: {now}");
    // The begin end has its own cell, and a size alone is a format (nothing else is written).
    run(&mut s, "shape.format", json!({"ids": [c], "begin_arrow_size": 6}));
    assert_eq!(number(&local(doc(&s), c, "BeginArrowSize").unwrap()), 6.0);
    assert_eq!(number(&local(doc(&s), c, "EndArrowSize").unwrap()), 5.0);
    // One undo step each.
    let steps = snapshot(&s).undo_steps;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).undo_steps, steps - 1);
    assert_eq!(formatting(&mut s)["begin_arrow_size"].as_u64(), Some(2));
    // Refused in words: a class past 6, and one that is not a whole number.
    let e = s.execute("shape.format", &json!({"ids": [c], "end_arrow_size": 7})).unwrap_err();
    assert!(bad_params_msg_or_failed(&e).contains("`end_arrow_size` must be a whole number from 0 to 6"), "{e}");
    for params in [json!({"ids": [c], "begin_arrow_size": -1}), json!({"ids": [c], "begin_arrow_size": 1.5})] {
        let e = s.execute("shape.format", &params).unwrap_err();
        assert!(bad_params_msg_or_failed(&e).contains("`begin_arrow_size` must be a whole number"), "{params}: {e}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_flowchart_shape_takes_the_same_cells() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "decision", 4.0, 4.0);
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#FFE699", "line": "#7F6000", "weight": 2.25, "pattern": 1}));
    let d = doc(&s);
    assert_eq!(fv(&local(d, id, "FillForegnd").unwrap()).0.as_deref(), Some("RGB(255,230,153)"));
    assert_eq!(fv(&local(d, id, "LineColor").unwrap()).0.as_deref(), Some("RGB(127,96,0)"));
    assert!(near(number(&local(d, id, "LineWeight").unwrap()), 2.25 / 72.0));
    assert_eq!(px(&render(&s), 4.0, 4.0), [255, 230, 153, 255], "the diamond is filled in the middle");
}

// ---- one undo step, no empty step, and the registry -------------------------------------------------------------------------------

#[test]
fn a_format_is_one_undo_step_that_undo_and_redo_restore_exactly() {
    let (mut s, id) = session_with_rectangle();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#3C78D8", "line": "#CC0000", "weight": 1.5, "pattern": 4, "rounding": 0.1}));
    let after = doc(&s).clone();
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Format shapes"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_format_that_changes_nothing_is_no_undo_step() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#3C78D8", "weight": 1.5}));
    let steps = s.history().unwrap().undo_len();
    let revision = s.revision();
    let r = run(&mut s, "shape.format", json!({"ids": [id], "fill": "#3c78d8", "weight": 1.5}));
    assert_eq!(r, json!({"page": 1, "formatted": 0}));
    assert_eq!((s.history().unwrap().undo_len(), s.revision()), (steps, revision), "the same colour in either case, the same weight");
}

#[test]
fn a_format_in_a_batch_is_one_step_with_the_other_commands() {
    let (mut s, id) = session_with_rectangle();
    let steps = s.history().unwrap().undo_len();
    run(
        &mut s,
        "command.batch",
        json!({"commands": [
            {"command": "shape.format", "params": {"ids": [id], "fill": "#3C78D8"}},
            {"command": "shape.format", "params": {"ids": [id], "line": "#CC0000"}},
        ]}),
    );
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(local(doc(&s), id, "FillForegnd").is_none() && local(doc(&s), id, "LineColor").is_none());
}

#[test]
fn bad_parameters_are_refused_and_change_nothing() {
    let (mut s, id) = session_with_rectangle();
    let before = snapshot(&s);
    let cases = [
        json!({"ids": [id]}),
        json!({"ids": [id], "fill": null}),
        json!({"ids": [], "fill": "#000000"}),
        json!({"ids": [id], "fill": "red"}),
        json!({"ids": [id], "fill": "#12345"}),
        json!({"ids": [id], "fill": "#GG0000"}),
        json!({"ids": [id], "fill": "#1234567"}),
        json!({"ids": [id], "fill": 5}),
        json!({"ids": [id], "line": "#00000"}),
        json!({"ids": [id], "weight": 0}),
        json!({"ids": [id], "weight": -1}),
        json!({"ids": [id], "weight": 1.0e6}),
        json!({"ids": [id], "weight": "thick"}),
        json!({"ids": [id], "pattern": 24}),
        json!({"ids": [id], "pattern": -1}),
        json!({"ids": [id], "pattern": 1.5}),
        json!({"ids": [id], "begin_arrow": 46}),
        json!({"ids": [id], "end_arrow": 254}),
        json!({"ids": [id], "rounding": -0.1}),
        json!({"ids": [id], "rounding": 1.0e9}),
        json!({"ids": [99], "fill": "#000000"}),
        json!({"ids": [id], "page": 9, "fill": "#000000"}),
        json!({"ids": [id, 99], "fill": "#000000"}),
    ];
    for params in cases {
        let e = s.execute("shape.format", &params).unwrap_err();
        assert!(!bad_params_msg_or_failed(&e).is_empty(), "{params}: {e}");
        assert_eq!(snapshot(&s), before, "{params}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

fn bad_params_msg_or_failed(e: &diacraft_engine::EngineError) -> String {
    match e {
        diacraft_engine::EngineError::BadParams { .. } => bad_params_msg(e).to_owned(),
        diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_commands_are_in_the_registry_with_their_parameters_and_need_a_document() {
    let info = Session::new().commands();
    let find = |id: &str| info.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id} is not registered"));
    let keys = |id: &str| diacraft_engine::param_keys(diacraft_engine::find_command(id).unwrap().params).unwrap();
    assert_eq!(
        keys("shape.format"),
        [
            "ids",
            "page",
            "fill",
            "line",
            "weight",
            "pattern",
            "begin_arrow",
            "end_arrow",
            "begin_arrow_size",
            "end_arrow_size",
            "rounding",
            "fill_transparency",
            "line_transparency",
            "fill_pattern",
            "fill_background",
            "line_cap",
            "compound"
        ]
    );
    assert_eq!(
        keys("text.format"),
        [
            "ids",
            "page",
            "start",
            "end",
            "font",
            "size",
            "bold",
            "italic",
            "underline",
            "strike",
            "color",
            "align",
            "valign",
            "bullet",
            "indent",
            "case",
            "direction",
            "vertical"
        ]
    );
    assert_eq!(keys("shape.formatting"), ["ids", "page", "start", "end"]);
    for id in ["shape.format", "text.format", "shape.formatting"] {
        assert!(!find(id).enabled, "{id} needs a document");
        assert_eq!(find(id).disabled_reason.as_deref(), Some("no document open"));
    }
    // A query is not journaled; the other two are.
    assert!(!diacraft_engine::find_command("shape.formatting").unwrap().journal);
    assert!(diacraft_engine::find_command("shape.format").unwrap().journal && diacraft_engine::find_command("text.format").unwrap().journal);
}

// ---- text.format -------------------------------------------------------------------------------------------------------------------

/// Where the ink of a rectangle of the page is, in inches: the leftmost and rightmost column and the top and bottom row with a dark pixel.
fn ink_box(r: &Rendered, rect: (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)> {
    let step = 1.0 / PX_PER_INCH;
    let mut found: Option<(f64, f64, f64, f64)> = None;
    let mut y = rect.1;
    while y < rect.3 {
        let mut x = rect.0;
        while x < rect.2 {
            let p = px(r, x, y);
            if p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300 {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((l, b, rr, t)) => (l.min(x), b.min(y), rr.max(x), t.max(y)),
                });
            }
            x += step;
        }
        y += step;
    }
    found
}

fn labelled() -> (Session, u32) {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Hello world"}));
    (s, id)
}

/// The box the rectangle of `session_with_rectangle` covers on the page: x 2.25 to 3.75, y 3.5 to 4.5, a little inside.
const BOX: (f64, f64, f64, f64) = (2.3, 3.55, 3.7, 4.45);

#[test]
fn text_size_alignment_and_colour_change_the_picture() {
    let (mut s, id) = labelled();
    let plain = ink_box(&render(&s), BOX).expect("the label is drawn");
    // Centred, in the middle of the box at 12 points: the ink's middle is the box's.
    assert!(((plain.0 + plain.2) / 2.0 - 3.0).abs() < 0.1, "{plain:?}");

    run(&mut s, "text.format", json!({"ids": [id], "size": 8}));
    let small = ink_box(&render(&s), BOX).unwrap();
    assert!(small.2 - small.0 < (plain.2 - plain.0) * 0.8, "8 points is narrower than 12: {small:?} against {plain:?}");

    run(&mut s, "text.format", json!({"ids": [id], "size": 8, "align": "left"}));
    let left = ink_box(&render(&s), BOX).unwrap();
    assert!(left.0 < small.0 - 0.1, "left-aligned text starts at the left of the box: {left:?} against {small:?}");
    run(&mut s, "text.format", json!({"ids": [id], "align": "right"}));
    let right = ink_box(&render(&s), BOX).unwrap();
    assert!(right.2 > small.2 + 0.1, "right-aligned text ends at the right of the box: {right:?} against {small:?}");

    run(&mut s, "text.format", json!({"ids": [id], "valign": "top"}));
    let top = ink_box(&render(&s), BOX).unwrap();
    run(&mut s, "text.format", json!({"ids": [id], "valign": "bottom"}));
    let bottom = ink_box(&render(&s), BOX).unwrap();
    assert!(top.3 > bottom.3 + 0.1, "top-aligned text is above bottom-aligned text: {top:?} {bottom:?}");

    run(&mut s, "text.format", json!({"ids": [id], "color": "#CC0000", "size": 24}));
    assert!(count(&render(&s), BOX, (204, 0, 0), 60) > 20, "red text, large enough to hold whole pixels of the colour");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn bold_draws_more_ink_and_underline_and_strike_add_lines() {
    let (mut s, id) = labelled();
    let dark = |s: &Session| count(&render(s), BOX, (0, 0, 0), 120);
    let plain = dark(&s);
    run(&mut s, "text.format", json!({"ids": [id], "bold": true}));
    let bold = dark(&s);
    assert!(bold > plain, "bold {bold} against plain {plain}");
    run(&mut s, "text.format", json!({"ids": [id], "bold": false, "underline": true}));
    let underlined = ink_box(&render(&s), BOX).unwrap();
    run(&mut s, "text.format", json!({"ids": [id], "underline": false}));
    let not = ink_box(&render(&s), BOX).unwrap();
    assert!(underlined.1 < not.1 - 0.005, "an underline is below the letters: {underlined:?} against {not:?}");
}

#[test]
fn text_format_is_one_undo_step_and_a_call_that_changes_nothing_is_none() {
    let (mut s, id) = labelled();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    let r = run(
        &mut s,
        "text.format",
        json!({"ids": [id], "font": "Liberation Sans", "size": 11, "bold": true, "color": "#202020", "align": "left", "valign": "top"}),
    );
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Format text"));
    let after = doc(&s).clone();
    assert!(after.fonts.iter().any(|f| &*f.name_u == "Liberation Sans"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    assert!(doc(&s).fonts.is_empty(), "the face went with the undo");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    let steps = s.history().unwrap().undo_len();
    let r = run(
        &mut s,
        "text.format",
        json!({"ids": [id], "font": "liberation sans", "size": 11, "bold": true, "color": "#202020", "align": "left", "valign": "top"}),
    );
    assert_eq!(r, json!({"page": 1, "formatted": 0}));
    assert_eq!(s.history().unwrap().undo_len(), steps);
}

#[test]
fn a_range_formats_part_of_the_label_and_a_later_edit_of_the_text_keeps_the_runs() {
    let (mut s, id) = labelled();
    run(&mut s, "text.format", json!({"ids": [id], "start": 6, "end": 11, "bold": true, "color": "#CC0000"}));
    let text = |s: &Session| doc(s).shape(&doc(s).find_shape(Owner::Page(1), id).unwrap()).unwrap().text.as_ref().unwrap().parts.clone();
    use diacraft_doc::TextPart;
    assert!(
        matches!(text(&s).as_slice(), [TextPart::Cp(0), TextPart::Chars(a), TextPart::Cp(1), TextPart::Chars(b)] if &**a == "Hello " && &**b == "world"),
        "{:?}",
        text(&s)
    );
    // `world` is red and `Hello ` is not.
    let r = render(&s);
    assert!(count(&r, (2.3, 3.55, 3.0, 4.45), (204, 0, 0), 60) == 0, "the first word is not red");
    assert!(count(&r, (3.0, 3.55, 3.7, 4.45), (204, 0, 0), 60) > 10, "the last word is red");
    // Typing over the first letter of `world` (shape.set_text with a range) joins its run, so the new letter is red and bold too.
    run(&mut s, "shape.set_text", json!({"id": id, "text": "W", "start": 6, "end": 7}));
    assert!(
        matches!(text(&s).as_slice(), [TextPart::Cp(0), TextPart::Chars(a), TextPart::Cp(1), TextPart::Chars(b)] if &**a == "Hello " && &**b == "World"),
        "{:?}",
        text(&s)
    );
    // A range that does not fit is refused.
    let before = snapshot(&s);
    let e = s.execute("text.format", &json!({"ids": [id], "start": 3, "end": 40, "bold": true})).unwrap_err();
    assert!(bad_params_msg_or_failed(&e).contains("not within the text"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn bad_text_parameters_are_refused_and_change_nothing() {
    let (mut s, id) = labelled();
    run(&mut s, "text.format", json!({"ids": [id], "start": 2, "end": 4, "bold": true}));
    let group = {
        let a = crate::edit::drop_shape(&mut s, "rectangle", 6.0, 4.0);
        let b = crate::edit::drop_shape(&mut s, "ellipse", 9.0, 4.0);
        (a, b)
    };
    let before = snapshot(&s);
    let cases = [
        json!({"ids": [id]}),
        json!({"ids": [id], "bold": null}),
        json!({"ids": [id], "font": ""}),
        json!({"ids": [id], "font": "   "}),
        json!({"ids": [id], "font": "a\nb"}),
        json!({"ids": [id], "font": "nul\u{0}"}),
        json!({"ids": [id], "font": "bad\u{fffe}"}),
        json!({"ids": [id], "font": "x".repeat(300)}),
        json!({"ids": [id], "font": 5}),
        json!({"ids": [id], "size": 0}),
        json!({"ids": [id], "size": -3}),
        json!({"ids": [id], "size": 1.0e9}),
        json!({"ids": [id], "size": "12"}),
        json!({"ids": [id], "bold": 1}),
        json!({"ids": [id], "italic": "true"}),
        json!({"ids": [id], "strike": []}),
        json!({"ids": [id], "color": "red"}),
        json!({"ids": [id], "color": "#12345"}),
        json!({"ids": [id], "color": "none"}),
        json!({"ids": [id], "align": "diagonal"}),
        json!({"ids": [id], "align": 1}),
        json!({"ids": [id], "valign": "centre"}),
        json!({"ids": [id], "bold": true, "start": 1}),
        json!({"ids": [id], "bold": true, "end": 1}),
        json!({"ids": [id], "bold": true, "start": 5, "end": 2}),
        json!({"ids": [id], "bold": true, "start": 0, "end": 4_294_967_295_u64}),
        json!({"ids": [id, group.0], "bold": true, "start": 0, "end": 1}),
        json!({"ids": [99], "bold": true}),
        json!({"ids": [id], "page": 9, "bold": true}),
    ];
    for params in cases {
        let e = s.execute("text.format", &params).unwrap_err();
        let msg = bad_params_msg_or_failed(&e);
        assert!(!msg.is_empty() && !msg.chars().any(char::is_control), "{params}: {e}");
        assert_eq!(snapshot(&s), before, "{params}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_guarded_cell_in_a_row_that_a_split_writes_over_is_a_refusal_and_changes_nothing() {
    // A split before row 1 moves the cells of the runs down, so the row 1 that holds a `GUARD` cell is given other cells: a write of a guarded
    // cell, refused. A guarded cell that is moved with its run, to another row, is not changed and is let through.
    use diacraft_doc::TextPart;
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
    let text = vec![TextPart::Cp(0), TextPart::Chars("ab".into()), TextPart::Cp(1), TextPart::Chars("cd".into())];
    let shape = ShapeBuilder::new(1)
        .text_parts(text)
        .row_cell("Character", None, diacraft_doc::RowKey::Ix(0), "Size", "0.1666666666666667")
        .row_formula("Character", None, diacraft_doc::RowKey::Ix(1), "Size", "GUARD(0.1111111111111111)", "0.1111111111111111");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let e = s.execute("text.format", &json!({"ids": [1], "start": 1, "end": 2, "bold": true})).unwrap_err();
    assert!(e.to_string().contains("GUARD"), "{e}");
    assert_eq!(snapshot(&s), before);
    // A format that does not cut a run writes only the cells it names: `bold` on the second run alone changes `Style` of row 1, and the guard on
    // `Size` is not touched.
    run(&mut s, "text.format", json!({"ids": [1], "start": 2, "end": 4, "bold": true}));
    let d = doc(&s);
    let shape = d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let Lookup::Found(size) = shape.sheet.lookup(&CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(1), "Size")) else {
        panic!("row 1 holds a Size")
    };
    assert_eq!(fv(size).0.as_deref(), Some("GUARD(0.1111111111111111)"));
}

#[test]
fn a_cell_that_reads_a_formatted_cell_is_worked_out_and_one_that_does_not_keeps_its_saved_value() {
    // The line colour is a formula of the fill, as a themed shape's is; `LineWeight` is a formula that reads neither, and its saved value
    // (a number the formula does not give) is the file's own and stays.
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
    let shape = ShapeBuilder::new(1)
        .cell("Width", "2")
        .cell("FillPattern", "1")
        .formula("FillForegnd", "RGB(1,2,3)", "#010203")
        .formula("LineColor", "FillForegnd", "#010203")
        .formula("LineWeight", "Width*0", "9");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        Ok(())
    })
    .unwrap();
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#336699"}));
    let d = doc(&s);
    assert_eq!(fv(&local(d, 1, "FillForegnd").unwrap()), (Some("RGB(51,102,153)".to_owned()), "#336699".to_owned()));
    assert_eq!(
        fv(&local(d, 1, "LineColor").unwrap()),
        (Some("FillForegnd".to_owned()), "#336699".to_owned()),
        "the cell that reads the fill follows it"
    );
    assert_eq!(
        fv(&local(d, 1, "LineWeight").unwrap()),
        (Some("Width*0".to_owned()), "9".to_owned()),
        "a cell nothing formatted is read by keeps its value"
    );
}

#[test]
fn a_face_whose_name_is_made_of_hex_digits_is_a_face_and_not_a_colour() {
    // `Decade` is six letters that are all hex digits: read as a colour it would show as `DECADE`, and a face is only a face.
    let (mut s, id) = labelled();
    run(&mut s, "text.format", json!({"ids": [id], "font": "Decade"}));
    let r = run(&mut s, "shape.formatting", json!({"ids": [id]}));
    assert_eq!(r["shapes"][0]["font"], "Decade");
}

#[test]
fn a_group_is_formatted_through_its_members_and_a_range_names_a_shape_with_characters() {
    // A group built from two shapes: both members take the format, the group's own cells are left.
    use diacraft_doc::{Cell, Value};
    let mut doc = diacraft_testkit::build::DocBuilder::new()
        .page(
            1,
            diacraft_testkit::build::PageBuilder::new().shape(
                diacraft_testkit::build::ShapeBuilder::new(1)
                    .group()
                    .child(diacraft_testkit::build::ShapeBuilder::new(2).text("a"))
                    .child(diacraft_testkit::build::ShapeBuilder::new(3).text("b")),
            ),
        )
        .build();
    let r = diacraft_engine::formatting::format_shapes(
        &mut doc,
        1,
        &[1],
        &diacraft_engine::formatting::ShapeFormat {
            fill: Some(diacraft_engine::formatting::Paint::Colour(diacraft_engine::formatting::Rgb(1, 2, 3))),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r, 2);
    let group = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    assert!(group.sheet.cell("FillForegnd").is_none(), "the group's own cells are left alone");
    for member in group.children.iter() {
        let c: &Cell = member.sheet.cell("FillForegnd").unwrap();
        assert_eq!(c.value.as_ref().map(Value::text), Some("#010203"));
    }
    // A range of characters of the group itself is refused: it has none.
    let problem = diacraft_engine::formatting::format_text(
        &mut doc,
        1,
        &[1],
        Some((0, 1)),
        &diacraft_engine::formatting::TextFormat { bold: Some(true), ..Default::default() },
    )
    .unwrap_err();
    assert!(problem.bad_params, "{}", problem.message);
}
