//! Page setup: a page's size, orientation, drawing scale, printer paper and Auto Size, and deleting and reordering pages.
//!
//! The pages are the ones real files hold: US Letter (8.5 by 11 inches) and A4 (210 by 297 mm) and their landscape forms, with drawing scales like
//! 1:1, 1:100 and 1/4 in = 1 ft. A new drawing's page is Letter landscape, 11 by 8.5.

use diacraft_doc::{Cell, CellAddr, CellEdit, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::draw::{connection_points, shape_frame};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::{blank_session, check_session, page_names, run, snapshot};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn close(value: &Json, expected: f64) {
    let got = value.as_f64().unwrap_or(f64::NAN);
    assert!(near(got, expected), "{value} is not {expected}");
}

/// The text a cell directly under the page's sheet holds.
fn cell(s: &Session, page: u32, name: &str) -> Option<String> {
    s.document()?.page(page)?.sheet.cell(name)?.value.as_ref().map(|v| v.text().to_owned())
}

fn unit(s: &Session, page: u32, name: &str) -> Option<String> {
    s.document()?.page(page)?.sheet.cell(name)?.unit.as_ref().map(|u| u.to_string())
}

/// A page cell as a number.
fn num(s: &Session, page: u32, name: &str) -> f64 {
    cell(s, page, name).unwrap_or_else(|| panic!("page {page} holds no {name}")).parse().unwrap()
}

/// Sets cells of a page's sheet the way a file would hold them (one undo step of the session, which the tests then count from).
fn set_cells(s: &mut Session, page: u32, cells: &[(&str, &str)]) {
    s.edit("test: set page cells", |doc| {
        for (name, value) in cells {
            doc.write_cell(&SheetRef::Page(page), &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Automation)
                .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
        }
        Ok(())
    })
    .unwrap();
}

fn undo_steps(s: &Session) -> usize {
    s.history().unwrap().undo_len()
}

/// A shape's pin on the page.
fn pin(s: &Session, page: u32, id: u32) -> (f64, f64) {
    let f = shape_frame(s.document().unwrap(), page, id).unwrap();
    (f.pin_x, f.pin_y)
}

fn drop_at(s: &mut Session, x: f64, y: f64) -> u32 {
    let r = run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": y}));
    u32::try_from(r["id"].as_u64().unwrap()).unwrap()
}

fn auto_size_on(s: &mut Session) {
    run(s, "page.auto_size", json!({"on": true}));
}

// ------------------------------------------------------------------------------------------------------------------ size

#[test]
fn a_named_size_gives_the_page_that_size_in_the_orientation_it_has() {
    let mut s = blank_session();
    // A new drawing's page is Letter landscape. A4 in landscape is 297 by 210 mm.
    let r = run(&mut s, "page.size", json!({"preset": "a4"}));
    close(&r["width"], 297.0 / 25.4);
    close(&r["height"], 210.0 / 25.4);
    assert_eq!((r["orientation"].as_str(), r["size"]["id"].as_str()), (Some("landscape"), Some("a4")));
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("11.692913386"));
    assert_eq!(cell(&s, 1, "PageHeight").as_deref(), Some("8.267716535"));
    // The paper follows: A4, held the way the page is.
    assert_eq!((cell(&s, 1, "PaperKind").as_deref(), cell(&s, 1, "PrintPageOrientation").as_deref()), (Some("9"), Some("2")));
    assert_eq!((r["paper"]["name"].as_str(), r["paper"]["assumed"].as_bool()), (Some("A4"), Some(false)));
    assert_eq!(cell(&s, 1, "DrawingSizeType").as_deref(), Some("5"), "an ISO size is labelled metric");
    assert_eq!((r["sheets"]["across"].as_u64(), r["sheets"]["down"].as_u64()), (Some(1), Some(1)));
    // The unit the cell shows in is kept.
    assert_eq!(unit(&s, 1, "PageWidth").as_deref(), Some("IN"));

    // Turned to portrait, the same names give the portrait forms.
    let r = run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    close(&r["width"], 210.0 / 25.4);
    close(&r["height"], 297.0 / 25.4);
    let r = run(&mut s, "page.size", json!({"preset": "letter"}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(8.5), Some(11.0)));
    assert_eq!((cell(&s, 1, "PaperKind").as_deref(), cell(&s, 1, "PrintPageOrientation").as_deref()), (Some("1"), Some("1")));
    assert_eq!(cell(&s, 1, "DrawingSizeType").as_deref(), Some("2"));
    assert_eq!(r["orientation"], "portrait");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_custom_size_is_taken_as_given_and_leaves_the_paper_alone() {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    let r = run(&mut s, "page.size", json!({"width": 10, "height": 7}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64(), r["orientation"].as_str()), (Some(10.0), Some(7.0), Some("landscape")));
    assert_eq!(r["size"], Json::Null);
    assert_eq!(cell(&s, 1, "DrawingSizeType").as_deref(), Some("3"), "a custom size at 1:1");
    assert_eq!(cell(&s, 1, "PaperKind").as_deref(), Some("9"), "the paper is the A4 it was");
    // A page 10 by 7 on A4 landscape paper (11.69 by 8.27) is one sheet.
    assert_eq!((r["sheets"]["across"].as_u64(), r["sheets"]["down"].as_u64()), (Some(1), Some(1)));
    // A custom size past the sheet is two.
    let r = run(&mut s, "page.size", json!({"width": 14, "height": 7}));
    assert_eq!((r["sheets"]["across"].as_u64(), r["sheets"]["down"].as_u64()), (Some(2), Some(1)));
}

#[test]
fn a_size_the_page_has_already_is_no_undo_step() {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    let steps = undo_steps(&s);
    let before = snapshot(&s);
    run(&mut s, "page.size", json!({"preset": "a4"}));
    assert_eq!(undo_steps(&s), steps);
    assert_eq!(snapshot(&s).document, before.document);
    // A new size is one step, and undo takes the page, its paper and its labels back together.
    run(&mut s, "page.size", json!({"preset": "legal"}));
    assert_eq!(undo_steps(&s), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn a_size_is_checked_and_a_refusal_changes_nothing() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [
        json!({}),
        json!({"preset": "no_such_size"}),
        json!({"preset": "a4", "width": 8, "height": 8}),
        json!({"width": 8}),
        json!({"height": 8}),
        json!({"width": 0, "height": 5}),
        json!({"width": -4, "height": 5}),
        json!({"width": 5, "height": 0.01}),
        json!({"width": 5, "height": 10_001}),
        json!({"width": 1e300, "height": 5}),
        json!({"width": "wide", "height": 5}),
        json!({"preset": 4}),
        json!({"preset": "a4", "page": 9}),
        json!({"preset": "\u{1b}[2J"}),
    ] {
        let err = s.execute("page.size", &params).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{params}: {err:?}");
        assert!(!err.to_string().contains('\u{1b}'), "{params}: the escape came back raw in `{err}`");
        assert_eq!(snapshot(&s), before, "{params}");
    }
}

#[test]
fn a_page_of_a_drawing_scale_gets_a_size_in_drawing_units_that_is_the_same_on_paper() {
    let mut s = blank_session();
    // 1/4 in = 1 ft is 48 drawing units to a paper inch: Letter landscape on paper is 528 by 408 drawing units.
    let r = run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "PageHeight").as_deref()), (Some("528"), Some("408")));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(11.0), Some(8.5)), "the page is the same size on paper");
    close(&r["drawing"]["units_per_inch"], 48.0);
    run(&mut s, "page.size", json!({"preset": "a4"}));
    close(&json!(num(&s, 1, "PageWidth")), 297.0 / 25.4 * 48.0);
    close(&json!(num(&s, 1, "PageHeight")), 210.0 / 25.4 * 48.0);
    let r = run(&mut s, "page.setup", json!({}));
    close(&r["width"], 297.0 / 25.4);
    assert_eq!(r["size"]["id"], "a4");
}

#[test]
fn a_size_that_would_pass_the_drawing_units_a_page_may_hold_is_refused() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"paper": 1, "drawing": 50_000}));
    let before = snapshot(&s);
    // 10,000 inches at 50,000 to the inch is five hundred million drawing units.
    let err = s.execute("page.size", &json!({"width": 10_000, "height": 10_000})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err:?}");
    assert_eq!(snapshot(&s), before);
}

// ------------------------------------------------------------------------------------------------------------------ orientation

#[test]
fn orientation_turns_the_page_and_the_paper_and_moves_no_shape() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let frame = shape_frame(s.document().unwrap(), 1, a).unwrap();
    let r = run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64(), r["orientation"].as_str()), (Some(8.5), Some(11.0), Some("portrait")));
    assert_eq!(cell(&s, 1, "PrintPageOrientation").as_deref(), Some("1"));
    assert_eq!(shape_frame(s.document().unwrap(), 1, a).unwrap(), frame, "The shapes stay where they were");
    let r = run(&mut s, "page.orientation", json!({"orientation": "Landscape"}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(11.0), Some(8.5)));
    assert_eq!(cell(&s, 1, "PrintPageOrientation").as_deref(), Some("2"));
}

#[test]
fn orientation_that_the_page_and_its_paper_have_is_no_undo_step() {
    let mut s = blank_session();
    run(&mut s, "page.orientation", json!({"orientation": "landscape"}));
    let steps = undo_steps(&s);
    let again = snapshot(&s);
    run(&mut s, "page.orientation", json!({"orientation": "landscape"}));
    assert_eq!(snapshot(&s).document, again.document);
    assert_eq!((undo_steps(&s), s.revision()), (steps, again.revision), "nothing changed, so nothing is unsaved or to undo");
}

#[test]
fn a_landscape_page_that_prints_upright_is_turned_on_its_paper_though_its_size_is_right() {
    // The files hold pages wider than tall whose paper is upright (264 of them hold no orientation and read the root's 1).
    let mut s = blank_session();
    set_cells(&mut s, 1, &[("PrintPageOrientation", "1")]);
    let r = run(&mut s, "page.setup", json!({}));
    assert_eq!((r["orientation"].as_str(), r["paper"]["orientation"].as_str()), (Some("landscape"), Some("portrait")));
    let steps = undo_steps(&s);
    let r = run(&mut s, "page.orientation", json!({"orientation": "landscape"}));
    assert_eq!((r["width"].as_f64(), r["paper"]["orientation"].as_str()), (Some(11.0), Some("landscape")));
    assert_eq!(undo_steps(&s), steps + 1, "the paper changed, so this is a step");
}

#[test]
fn a_square_page_keeps_its_size_when_it_is_turned() {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"width": 6, "height": 6}));
    let r = run(&mut s, "page.orientation", json!({"orientation": "landscape"}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(6.0), Some(6.0)));
    assert_eq!(cell(&s, 1, "PrintPageOrientation").as_deref(), Some("2"));
}

#[test]
fn an_orientation_that_is_neither_is_refused() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [json!({}), json!({"orientation": "sideways"}), json!({"orientation": 1}), json!({"orientation": ""})] {
        assert!(matches!(s.execute("page.orientation", &params), Err(EngineError::BadParams { .. })), "{params}");
    }
    assert_eq!(snapshot(&s), before);
}

// ------------------------------------------------------------------------------------------------------------------ drawing scale

#[test]
fn a_drawing_scale_writes_the_two_scale_cells_the_family_and_the_size_and_leaves_the_shapes_alone() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let frame = shape_frame(s.document().unwrap(), 1, a).unwrap();
    let r = run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    assert_eq!((cell(&s, 1, "PageScale").as_deref(), unit(&s, 1, "PageScale").as_deref()), (Some("0.25"), Some("IN")));
    assert_eq!((cell(&s, 1, "DrawingScale").as_deref(), unit(&s, 1, "DrawingScale").as_deref()), (Some("12"), Some("FT")));
    assert_eq!(cell(&s, 1, "DrawingScaleType").as_deref(), Some("1"));
    assert_eq!(r["scale"]["preset"], "arch_1_4");
    close(&r["scale"]["paper"], 0.25);
    close(&r["scale"]["drawing"], 1.0);
    assert_eq!((r["scale"]["paper_unit"].as_str(), r["scale"]["drawing_unit"].as_str()), (Some("IN"), Some("FT")));
    // The shapes' cells are not rescaled.
    assert_eq!(shape_frame(s.document().unwrap(), 1, a).unwrap(), frame);
    // Back to none: the page is 11 by 8.5 drawing units again.
    run(&mut s, "page.scale", json!({"preset": "none"}));
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "DrawingScaleType").as_deref()), (Some("11"), Some("0")));
}

#[test]
fn the_metric_scale_of_a_page_in_millimetres_is_a_hundredth() {
    // `DrawingScale` 3.9370078 (100 mm) over `PageScale` 0.0393701 (1 mm) is 0.01.
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    let r = run(&mut s, "page.scale", json!({"preset": "metric_1_100"}));
    assert_eq!((unit(&s, 1, "PageScale").as_deref(), unit(&s, 1, "DrawingScale").as_deref()), (Some("MM"), Some("MM")));
    close(&json!(num(&s, 1, "PageScale") / num(&s, 1, "DrawingScale")), 0.01);
    close(&json!(num(&s, 1, "PageWidth")), 210.0 / 25.4 * 100.0);
    close(&json!(num(&s, 1, "PageHeight")), 297.0 / 25.4 * 100.0);
    assert_eq!(cell(&s, 1, "DrawingScaleType").as_deref(), Some("4"));
    close(&r["width"], 210.0 / 25.4);
    // The values the page reads back in the units it shows in.
    close(&r["scale"]["paper"], 1.0);
    close(&r["scale"]["drawing"], 100.0);
    assert_eq!(r["scale"]["preset"], "metric_1_100");
    assert_eq!(r["size"]["id"], "a4");
}

#[test]
fn a_custom_scale_takes_a_length_on_paper_and_the_length_it_stands_for() {
    let mut s = blank_session();
    // 1 in = 50 ft is a civil engineering scale, so a custom one that is that ratio is read back as it.
    let r = run(&mut s, "page.scale", json!({"paper": 1, "drawing": 50, "drawing_unit": "ft"}));
    assert_eq!((cell(&s, 1, "DrawingScale").as_deref(), cell(&s, 1, "DrawingScaleType").as_deref()), (Some("600"), Some("3")));
    assert_eq!(r["scale"]["preset"], "civil_50");
    close(&json!(num(&s, 1, "PageWidth")), 11.0 * 600.0);
    // Centimetres on paper for metres in the drawing: 2 cm = 1 m is 1:50.
    let r = run(&mut s, "page.scale", json!({"paper": 2, "paper_unit": "cm", "drawing": 1, "drawing_unit": "m"}));
    close(&json!(num(&s, 1, "PageWidth")), 11.0 * 50.0);
    assert_eq!(r["scale"]["family"], "custom");
    // 1:1 is no scale at all.
    run(&mut s, "page.scale", json!({"paper": 3, "drawing": 3}));
    assert_eq!((cell(&s, 1, "DrawingScaleType").as_deref(), cell(&s, 1, "PageWidth").as_deref()), (Some("0"), Some("11")));
}

#[test]
fn a_scale_is_checked_and_a_refusal_changes_nothing() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [
        json!({}),
        json!({"preset": "no_such_scale"}),
        json!({"preset": "none", "paper": 1, "drawing": 2}),
        json!({"paper": 1}),
        json!({"drawing": 1}),
        json!({"paper": 0, "drawing": 5}),
        json!({"paper": -1, "drawing": 5}),
        json!({"paper": 1, "drawing": 5, "paper_unit": "parsecs"}),
        json!({"paper": 1, "drawing": 5, "drawing_unit": "pt"}),
        json!({"paper_unit": "mm", "drawing": 5}),
        json!({"paper": 1, "drawing": 1e300}),
        json!({"paper": 1e-300, "drawing": 1}),
        json!({"paper": 1, "drawing": 10_000_000}),
        json!({"paper": 1e-9, "drawing": 1, "drawing_unit": "mi"}),
    ] {
        let err = s.execute("page.scale", &params).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{params}: {err:?}");
        assert_eq!(snapshot(&s), before, "{params}");
    }
    // A ratio inside the range that makes a page of more than a million drawing units is a failure of the document, not of the call.
    let err = s.execute("page.scale", &json!({"paper": 1, "drawing": 900_000})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err:?}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_cell_that_reads_the_page_is_worked_out_again_when_the_page_changes() {
    // Shapes read `ThePage!PageWidth` and the scale cells (a title block a tenth of the page wide; a text size that follows the scale).
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    s.edit("test: a formula that reads the page", |doc| {
        let at = doc.find_shape(Owner::Page(1), a).unwrap();
        for (name, formula, value) in [("Width", "ThePage!PageWidth*0.1", "1.1"), ("Height", "ThePage!PageScale/ThePage!DrawingScale", "1")] {
            doc.write_cell(
                &SheetRef::Shape(at.clone()),
                &CellAddr::direct(name),
                CellEdit::Cell(Cell::with_formula(formula, Some(Value::from_text(value))).with_unit("IN")),
                WriteOrigin::Automation,
            )
            .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
        }
        Ok(())
    })
    .unwrap();
    let read = |s: &Session, name: &str| {
        let doc = s.document().unwrap();
        let at = doc.find_shape(Owner::Page(1), a).unwrap();
        doc.shape(&at).unwrap().sheet.cell(name).unwrap().value.as_ref().unwrap().text().to_owned()
    };
    run(&mut s, "page.size", json!({"width": 20, "height": 10}));
    assert_eq!(read(&s, "Width"), "2");
    run(&mut s, "page.scale", json!({"preset": "metric_1_10"}));
    assert_eq!(read(&s, "Width"), "20", "the page is 200 drawing units wide at 1:10");
    assert!(near(read(&s, "Height").parse::<f64>().unwrap(), 0.1), "PageScale over DrawingScale: {}", read(&s, "Height"));
}

// ------------------------------------------------------------------------------------------------------------------ paper

#[test]
fn the_paper_is_chosen_apart_from_the_page() {
    let mut s = blank_session();
    let r = run(&mut s, "page.paper", json!({"preset": "legal", "orientation": "portrait"}));
    assert_eq!((cell(&s, 1, "PaperKind").as_deref(), cell(&s, 1, "PrintPageOrientation").as_deref()), (Some("5"), Some("1")));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(11.0), Some(8.5)), "the page is not touched");
    assert_eq!((r["paper"]["name"].as_str(), r["paper"]["width"].as_f64(), r["paper"]["height"].as_f64()), (Some("Legal"), Some(8.5), Some(14.0)));
    assert_eq!(
        (r["sheets"]["across"].as_u64(), r["sheets"]["down"].as_u64()),
        (Some(2), Some(1)),
        "an 11 by 8.5 page on 8.5 by 14 sheets is two across"
    );
    // The kind by its number, and the way alone.
    let r = run(&mut s, "page.paper", json!({"kind": 3}));
    assert_eq!(r["paper"]["name"], "Tabloid");
    let r = run(&mut s, "page.paper", json!({"orientation": "landscape"}));
    assert_eq!((r["paper"]["width"].as_f64(), r["paper"]["height"].as_f64()), (Some(17.0), Some(11.0)));
    let before = snapshot(&s);
    for params in [
        json!({}),
        json!({"kind": 32767}),
        json!({"kind": 3, "preset": "a4"}),
        json!({"preset": "a0"}),
        json!({"preset": "nothing"}),
        json!({"orientation": "x"}),
    ] {
        assert!(matches!(s.execute("page.paper", &params), Err(EngineError::BadParams { .. })), "{params}");
    }
    assert_eq!(snapshot(&s), before);
}

// ------------------------------------------------------------------------------------------------------------------ Auto Size

#[test]
fn auto_size_is_off_in_a_new_drawing_and_is_turned_on_and_off() {
    let mut s = blank_session();
    assert_eq!(run(&mut s, "page.setup", json!({}))["auto_size"], false);
    let steps = undo_steps(&s);
    // Off already: no step.
    run(&mut s, "page.auto_size", json!({"on": false}));
    assert_eq!(undo_steps(&s), steps);
    let r = run(&mut s, "page.auto_size", json!({"on": true}));
    assert_eq!((r["auto_size"].as_bool(), cell(&s, 1, "DrawingResizeType").as_deref()), (Some(true), Some("1")));
    run(&mut s, "page.auto_size", json!({"on": true}));
    assert_eq!(undo_steps(&s), steps + 1);
    let r = run(&mut s, "page.auto_size", json!({"on": false}));
    assert_eq!((r["auto_size"].as_bool(), cell(&s, 1, "DrawingResizeType").as_deref()), (Some(false), Some("2")));
    for params in [json!({}), json!({"on": "yes"}), json!({"on": 1}), json!({"on": null})] {
        assert!(matches!(s.execute("page.auto_size", &params), Err(EngineError::BadParams { .. })), "{params}");
    }
}

#[test]
fn a_page_that_defers_to_its_size_type_keeps_its_auto_size_when_the_size_is_set() {
    // 0 defers to DrawingSizeType, which grows only at 0. A size would label the page 2, which would turn Auto Size off.
    let mut s = blank_session();
    set_cells(&mut s, 1, &[("DrawingResizeType", "0"), ("DrawingSizeType", "0")]);
    assert_eq!(run(&mut s, "page.setup", json!({}))["auto_size"], true);
    let r = run(&mut s, "page.size", json!({"preset": "letter"}));
    assert_eq!(r["auto_size"], true, "LABELS");
    assert_eq!(cell(&s, 1, "DrawingResizeType").as_deref(), Some("1"));
    assert_eq!(cell(&s, 1, "DrawingSizeType").as_deref(), Some("2"));
    // The same for a page that had it off.
    set_cells(&mut s, 1, &[("DrawingResizeType", "0"), ("DrawingSizeType", "3")]);
    assert_eq!(run(&mut s, "page.setup", json!({}))["auto_size"], false);
    let r = run(&mut s, "page.size", json!({"width": 9, "height": 9}));
    assert_eq!((r["auto_size"].as_bool(), cell(&s, 1, "DrawingResizeType").as_deref()), (Some(false), Some("2")));
}

#[test]
fn the_page_grows_by_a_sheet_when_a_shape_is_dropped_past_its_right_edge() {
    let mut s = blank_session();
    let kept = drop_at(&mut s, 3.0, 4.0);
    auto_size_on(&mut s);
    let before = snapshot(&s);
    // The box of a rectangle is 1.5 by 1: centred at 10.5 it reaches 11.25, past the edge at 11.
    let b = drop_at(&mut s, 10.5, 4.0);
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "PageHeight").as_deref()), (Some("22"), Some("8.5")), "one sheet, 11 inches");
    assert_eq!(pin(&s, 1, b), (10.5, 4.0), "the shape is where it was put");
    assert_eq!(pin(&s, 1, kept), (3.0, 4.0), "growing on the right moves nothing");
    // One undo step took the drop and the growth together.
    assert_eq!(undo_steps(&s), before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_page_grows_by_as_many_whole_sheets_as_take_the_shape_in() {
    let mut s = blank_session();
    auto_size_on(&mut s);
    // 25 centred: the box reaches 25.75, 14.75 past the edge: two sheets, 33 wide.
    drop_at(&mut s, 25.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("33"));
    // Exactly at the edge: a box that only touches it does not grow the page. 32.25 centred reaches 33.
    let steps = undo_steps(&s);
    drop_at(&mut s, 32.25, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("33"));
    assert_eq!(undo_steps(&s), steps + 1);
    // Above: 8.5 high, a box to 9.4 needs one sheet, 8.5 more.
    drop_at(&mut s, 5.0, 8.9);
    assert_eq!(cell(&s, 1, "PageHeight").as_deref(), Some("17"));
}

#[test]
fn a_shape_dropped_past_the_left_or_bottom_edge_carries_the_drawing_to_the_right_or_up() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let b = drop_at(&mut s, 7.0, 4.0);
    let joiner = run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}))["id"].as_u64().unwrap() as u32;
    auto_size_on(&mut s);
    let ends_before = {
        let d = s.document().unwrap();
        let sheet = d.shape(&d.find_shape(Owner::Page(1), joiner).unwrap()).unwrap().sheet.clone();
        let n = |name: &str| sheet.cell(name).unwrap().value.as_ref().unwrap().number().unwrap();
        (n("BeginX"), n("BeginY"), n("EndX"), n("EndY"))
    };
    let routes_before = {
        let d = s.document().unwrap();
        d.shape(&d.find_shape(Owner::Page(1), joiner).unwrap()).unwrap().sheet.sections.clone()
    };
    let connects = s.document().unwrap().page(1).unwrap().connects.len();
    // Left: the box reaches -0.95, so one sheet, 11, on the left.
    let c = drop_at(&mut s, -0.2, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("22"));
    assert_eq!(pin(&s, 1, a), (14.0, 4.0), "The shapes move with the origin");
    assert_eq!(pin(&s, 1, b), (18.0, 4.0));
    assert_eq!(pin(&s, 1, c), (10.8, 4.0), "the shape that made the page grow is inside it");
    // The connector is carried: ends, glue and route.
    let d = s.document().unwrap();
    let sheet = d.shape(&d.find_shape(Owner::Page(1), joiner).unwrap()).unwrap().sheet.clone();
    let n = |name: &str| sheet.cell(name).unwrap().value.as_ref().unwrap().number().unwrap();
    assert!(near(n("BeginX"), ends_before.0 + 11.0) && near(n("EndX"), ends_before.2 + 11.0), "the ends moved with their shapes");
    assert!(near(n("BeginY"), ends_before.1) && near(n("EndY"), ends_before.3));
    assert_eq!(sheet.sections, routes_before, "the route is the rows it had: nothing is routed again");
    assert_eq!(d.page(1).unwrap().connects.len(), connects, "the glue is as it was");
    let at_a = connection_points(d, 1, a);
    assert!(at_a.iter().any(|p| near(p.x, n("BeginX")) && near(p.y, n("BeginY"))), "the begin is at a connection point of a, {at_a:?}");
    // Bottom: the box reaches y -0.3, one sheet, 8.5.
    let e = drop_at(&mut s, 12.0, 0.2);
    assert_eq!(cell(&s, 1, "PageHeight").as_deref(), Some("17"));
    assert_eq!(pin(&s, 1, e), (12.0, 8.7));
    assert_eq!(pin(&s, 1, a), (14.0, 12.5));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_with_auto_size_off_does_not_grow() {
    let mut s = blank_session();
    let b = drop_at(&mut s, 10.5, 4.0);
    drop_at(&mut s, -3.0, -3.0);
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "PageHeight").as_deref()), (Some("11"), Some("8.5")));
    assert_eq!(pin(&s, 1, b), (10.5, 4.0));
}

#[test]
fn a_page_of_a4_grows_by_sheets_of_a4_held_the_way_the_paper_is() {
    let mut s = blank_session();
    run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    run(&mut s, "page.size", json!({"preset": "a4"}));
    auto_size_on(&mut s);
    // A4 portrait is 8.2677 wide: a box to 9.75 is 1.48 past it, one sheet of 8.2677.
    drop_at(&mut s, 9.0, 5.0);
    close(&json!(num(&s, 1, "PageWidth")), 2.0 * 210.0 / 25.4);
    close(&json!(num(&s, 1, "PageHeight")), 297.0 / 25.4);
    // Paper turned sideways: a sheet is 11.69 wide and 8.27 high, whatever the page is.
    run(&mut s, "page.paper", json!({"orientation": "landscape"}));
    drop_at(&mut s, 5.0, 12.0);
    close(&json!(num(&s, 1, "PageHeight")), 297.0 / 25.4 + 210.0 / 25.4);
}

#[test]
fn a_page_of_a_drawing_scale_grows_by_the_sheet_in_drawing_units() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    auto_size_on(&mut s);
    // The page is 528 by 408 drawing units, a sheet of Letter landscape. x is in paper inches: 11.5 is 552 drawing units, and the box (1.5 wide in
    // drawing units, since a drop does not adapt to the scale) reaches 552.75.
    let b = drop_at(&mut s, 11.5, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("1056"));
    assert_eq!(cell(&s, 1, "PageHeight").as_deref(), Some("408"));
    let r = run(&mut s, "page.setup", json!({}));
    assert_eq!((r["width"].as_f64(), r["sheets"]["across"].as_u64()), (Some(22.0), Some(2)));
    assert_eq!(pin(&s, 1, b), (552.0, 192.0));
}

#[test]
fn every_command_that_puts_or_moves_a_shape_grows_the_page() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let wide = drop_at(&mut s, 5.0, 7.9);
    run(&mut s, "shape.resize", json!({"id": wide, "x": 3.0, "y": 7.4, "width": 4.0, "height": 1.0}));
    auto_size_on(&mut s);
    let width = |s: &Session| cell(s, 1, "PageWidth");
    let height = |s: &Session| cell(s, 1, "PageHeight");

    // shape.move
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 8.5, "dy": 0}));
    assert_eq!(width(&s).as_deref(), Some("22"), "move: 11.5 centred reaches 12.25");
    run(&mut s, "shape.move", json!({"ids": [a], "dx": -8.5, "dy": 0}));
    // shape.resize
    run(&mut s, "shape.resize", json!({"id": a, "x": 2.0, "y": 3.0, "width": 21.0, "height": 1.0}));
    assert_eq!(width(&s).as_deref(), Some("33"), "resize: a box to 23 needs a second sheet");
    run(&mut s, "shape.resize", json!({"id": a, "x": 2.0, "y": 3.0, "width": 1.5, "height": 1.0}));
    // shape.rotate: the wide shape (x 3 to 7, y 7.4 to 8.4) turned a quarter reaches y 9.9, past the top at 8.5.
    run(&mut s, "shape.rotate", json!({"ids": [wide], "by": 90}));
    assert_eq!(height(&s).as_deref(), Some("17"), "rotate");
    // edit.paste and shape.duplicate
    run(&mut s, "edit.copy", json!({"ids": [a]}));
    run(&mut s, "edit.paste", json!({"x": 40.0, "y": 4.0}));
    assert_eq!(width(&s).as_deref(), Some("44"), "paste: 40 centred reaches 40.75, 7.75 past 33: one more sheet");
    let before_duplicate = width(&s);
    run(&mut s, "shape.duplicate", json!({"ids": [a], "dx": 50.0, "dy": 0.0}));
    assert_ne!(width(&s), before_duplicate, "duplicate: a copy at 52.75 reaches 53.5");
    assert_eq!(width(&s).as_deref(), Some("55"));
    // shape.align: a shape that is past the edge (put there while Auto Size was off) is what the others are aligned to. The page grows for the
    // shapes the command names, and that one is among them.
    run(&mut s, "page.auto_size", json!({"on": false}));
    let far = drop_at(&mut s, 70.0, 2.0);
    assert_eq!(width(&s).as_deref(), Some("55"));
    auto_size_on(&mut s);
    run(&mut s, "shape.align", json!({"ids": [far, a], "how": "right"}));
    assert_eq!(width(&s).as_deref(), Some("77"), "align: the box of the far shape reaches 70.75, 15.75 past 55: two sheets");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_off_the_page_is_not_the_reason_the_page_grows_for_another() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let off = drop_at(&mut s, 30.0, 4.0);
    auto_size_on(&mut s);
    // Only the shapes a command names or makes are looked at, so moving `a` inside the page leaves the page as it is.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.0, "dy": 0.0}));
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("11"));
    assert_eq!(pin(&s, 1, off), (30.0, 4.0));
}

#[test]
fn a_page_does_not_grow_past_its_limits_and_the_drop_still_works() {
    let mut s = blank_session();
    auto_size_on(&mut s);
    // 100,000 inches is more than the 10,000 a page may be: the shape is off the page.
    let b = drop_at(&mut s, 100_000.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("11"));
    assert_eq!(pin(&s, 1, b), (100_000.0, 4.0));
    // 9,000 inches is within it: the box reaches 9,000.75, 8,989.75 past the edge, 818 sheets of 11.
    drop_at(&mut s, 9_000.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("9009"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_whose_size_is_protected_stays_and_the_drop_goes_on() {
    let mut s = blank_session();
    auto_size_on(&mut s);
    s.edit("test: a protected width", |doc| {
        doc.write_cell(
            &SheetRef::Page(1),
            &CellAddr::direct("PageWidth"),
            CellEdit::Cell(Cell::with_formula("GUARD(11)", Some(Value::from_text("11"))).with_unit("IN")),
            WriteOrigin::Automation,
        )
        .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let b = drop_at(&mut s, 12.0, 4.0);
    let after = s.document().unwrap().page(1).unwrap();
    assert_eq!(after.sheet.cell("PageWidth").unwrap().value.as_ref().unwrap().text(), "11");
    assert_eq!(after.sheet.cell("PageHeight").unwrap().value.as_ref().unwrap().text(), "8.5");
    assert_eq!(pin(&s, 1, b), (12.0, 4.0));
    assert_eq!(undo_steps(&s), before.undo_steps + 1);
}

#[test]
fn a_page_with_too_many_shapes_to_carry_grows_only_where_nothing_is_carried() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    // Pad the page with shapes that are nowhere in particular, to one more than a shift carries.
    s.edit("test: a full page", |doc| {
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).unwrap();
        let mut id = 1000;
        while page.shapes.len() <= diacraft_engine::draw::MAX_SHIFT_SHAPES {
            page.shapes.push(std::sync::Arc::new(diacraft_doc::Shape::new(id)));
            id += 1;
        }
        Ok(())
    })
    .unwrap();
    auto_size_on(&mut s);
    // Left: a shift would be 20,000 shapes and more, so the page is left and the shape sits off it.
    let left = drop_at(&mut s, -2.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("11"));
    assert_eq!(pin(&s, 1, left), (-2.0, 4.0));
    assert_eq!(pin(&s, 1, a), (3.0, 4.0));
    // Right: nothing is carried, so it grows.
    drop_at(&mut s, 12.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("22"));
}

// ------------------------------------------------------------------------------------------------------------------ pages

#[test]
fn a_new_page_can_copy_the_setup_of_the_page_on_view() {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    run(&mut s, "page.scale", json!({"preset": "metric_1_20"}));
    let plain = run(&mut s, "page.add", json!({}));
    assert_eq!(cell(&s, plain["id"].as_u64().unwrap() as u32, "PageWidth").as_deref(), Some("11"), "without `like` the page is the default one");
    let r = run(&mut s, "page.add", json!({"name": "Plan", "like": 1}));
    let id = r["id"].as_u64().unwrap() as u32;
    let setup = run(&mut s, "page.setup", json!({"page": id}));
    assert_eq!(
        (setup["orientation"].as_str(), setup["size"]["id"].as_str(), setup["scale"]["preset"].as_str()),
        (Some("portrait"), Some("a4"), Some("metric_1_20"))
    );
    close(&setup["width"], 210.0 / 25.4);
    assert_eq!(setup["paper"]["name"], "A4");
    // A page that is not there.
    let before = snapshot(&s);
    assert!(matches!(s.execute("page.add", &json!({"like": 99})), Err(EngineError::BadParams { .. })));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_page_is_deleted_with_its_shapes_in_one_undo_step() {
    let mut s = blank_session();
    drop_at(&mut s, 3.0, 4.0);
    run(&mut s, "page.add", json!({"name": "Two"}));
    run(&mut s, "page.add", json!({"name": "Three"}));
    let before = snapshot(&s);
    let r = run(&mut s, "page.delete", json!({"id": 1}));
    assert_eq!((r["name"].as_str(), r["pages"].as_u64()), (Some("Page-1"), Some(2)));
    assert_eq!(page_names(&s), ["Two", "Three"]);
    assert_eq!(undo_steps(&s), before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    assert!(s.journal().iter().any(|(id, _)| id == "page.delete"));
}

#[test]
fn the_last_page_stays_and_a_page_that_is_not_there_is_a_bad_call() {
    let mut s = blank_session();
    // One page: there is nothing to delete or to move, and the commands say so to a menu.
    let before = snapshot(&s);
    for (id, params) in [("page.delete", json!({"id": 1})), ("page.move", json!({"id": 1, "to": 0}))] {
        assert!(matches!(s.execute(id, &params), Err(EngineError::Disabled { .. })), "{id}");
        let info = s.commands().into_iter().find(|c| c.id == id).unwrap();
        assert!(!info.enabled && info.disabled_reason.as_deref() == Some("the drawing has one page"), "{info:?}");
    }
    assert_eq!(snapshot(&s), before);
    // A background and one page: the page is the last that is not a background, and the background may go.
    s.edit("test: a background page", |doc| {
        let mut back = diacraft_doc::Page::new(9, "Back");
        back.background = true;
        doc.pages.insert(0, std::sync::Arc::new(back)).map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    assert!(matches!(s.execute("page.delete", &json!({"id": 1})), Err(EngineError::Failed { .. })));
    assert!(matches!(s.execute("page.delete", &json!({"id": 77})), Err(EngineError::BadParams { .. })));
    assert!(matches!(s.execute("page.delete", &json!({})), Err(EngineError::BadParams { .. })));
    assert!(matches!(s.execute("page.delete", &json!({"id": -1})), Err(EngineError::BadParams { .. })));
    assert_eq!(snapshot(&s), before);
    run(&mut s, "page.delete", json!({"id": 9}));
    assert_eq!(page_names(&s), ["Page-1"]);
}

#[test]
fn pages_are_reordered_among_the_foreground_pages_and_a_background_stays_put() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Two"}));
    run(&mut s, "page.add", json!({"name": "Three"}));
    // A background page, as a file lists it: first.
    s.edit("test: a background page", |doc| {
        let mut back = diacraft_doc::Page::new(9, "Back");
        back.background = true;
        doc.pages.insert(0, std::sync::Arc::new(back)).map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
        Ok(())
    })
    .unwrap();
    let steps = undo_steps(&s);
    let r = run(&mut s, "page.move", json!({"id": 3, "to": 0}));
    assert_eq!((r["from"].as_u64(), r["to"].as_u64(), r["renamed"].as_u64()), (Some(2), Some(0), Some(0)));
    assert_eq!(page_names(&s), ["Back", "Three", "Page-1", "Two"]);
    assert_eq!(undo_steps(&s), steps + 1);
    // Where the page already is: no step.
    run(&mut s, "page.move", json!({"id": 3, "to": 0}));
    assert_eq!(undo_steps(&s), steps + 1);
    // A background is not moved, and a place that is not one is refused.
    let before = snapshot(&s);
    assert!(matches!(s.execute("page.move", &json!({"id": 9, "to": 0})), Err(EngineError::Failed { .. })));
    assert!(matches!(s.execute("page.move", &json!({"id": 1, "to": 3})), Err(EngineError::BadParams { .. })));
    assert!(matches!(s.execute("page.move", &json!({"id": 1})), Err(EngineError::BadParams { .. })));
    assert!(matches!(s.execute("page.move", &json!({"id": 77, "to": 0})), Err(EngineError::BadParams { .. })));
    assert!(matches!(s.execute("page.move", &json!({"id": 1, "to": 0, "renumber": "yes"})), Err(EngineError::BadParams { .. })));
    assert_eq!(snapshot(&s), before);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(page_names(&s), ["Back", "Page-1", "Two", "Three"]);
}

#[test]
fn reordering_can_renumber_the_default_names() {
    // Page-1 and Page-2, Page-2 moved first, the update option on: the first tab reads Page-1.
    let mut s = blank_session();
    run(&mut s, "page.add", json!({}));
    assert_eq!(page_names(&s), ["Page-1", "Page-2"]);
    let r = run(&mut s, "page.move", json!({"id": 2, "to": 0, "renumber": true}));
    assert_eq!(r["renamed"], 2);
    assert_eq!(page_names(&s), ["Page-1", "Page-2"]);
    let ids: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    assert_eq!(ids, [2, 1], "the page that was second is first, and now reads Page-1");
    // Without it the names follow their pages.
    let mut t = blank_session();
    run(&mut t, "page.add", json!({}));
    run(&mut t, "page.move", json!({"id": 2, "to": 0}));
    assert_eq!(page_names(&t), ["Page-2", "Page-1"]);
}

#[test]
fn a_saved_drawing_keeps_the_setup_and_the_order_of_its_pages() {
    let dir = std::env::temp_dir().join(format!("diacraft-engine-page-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pages.vsdx");
    let path_text = path.to_str().unwrap().to_owned();
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    run(&mut s, "page.scale", json!({"preset": "metric_1_50"}));
    auto_size_on(&mut s);
    run(&mut s, "page.add", json!({"name": "Two", "like": 1}));
    run(&mut s, "page.add", json!({"name": "Three"}));
    run(&mut s, "page.move", json!({"id": 3, "to": 0}));
    run(&mut s, "page.delete", json!({"id": 2}));
    run(&mut s, "file.save_as", json!({"path": path_text}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path_text}));
    assert_eq!(page_names(&again), ["Three", "Page-1"]);
    let first = run(&mut again, "page.setup", json!({"page": 1}));
    assert_eq!(
        (first["orientation"].as_str(), first["size"]["id"].as_str(), first["scale"]["preset"].as_str()),
        (Some("portrait"), Some("a4"), Some("metric_1_50"))
    );
    assert_eq!((first["auto_size"].as_bool(), first["paper"]["name"].as_str()), (Some(true), Some("A4")));
    close(&first["width"], 210.0 / 25.4);
    // The page `Three` was added after the first, with the default setup.
    let three = run(&mut again, "page.setup", json!({"page": 3}));
    assert_eq!((three["orientation"].as_str(), three["auto_size"].as_bool()), (Some("landscape"), Some(false)));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_presets_are_listed_without_a_document() {
    let mut s = Session::new();
    let r = run(&mut s, "page.presets", json!({}));
    let sizes = r["sizes"].as_array().unwrap();
    assert!(sizes.iter().any(|z| z["id"] == "letter" && z["width"] == 8.5 && z["height"] == 11.0 && z["paper_kind"] == 1));
    assert!(sizes.iter().any(|z| z["id"] == "a4" && z["paper_kind"] == 9));
    assert!(sizes.iter().any(|z| z["id"] == "a0" && z["paper_kind"].is_null()));
    assert!(r["papers"].as_array().unwrap().iter().any(|p| p["kind"] == 5 && p["name"] == "Legal"));
    let scales = r["scales"].as_array().unwrap();
    let quarter = scales.iter().find(|z| z["id"] == "arch_1_4").unwrap();
    assert_eq!(
        (quarter["paper_unit"].as_str(), quarter["drawing_unit"].as_str(), quarter["family"].as_str()),
        (Some("IN"), Some("FT"), Some("architectural"))
    );
    close(&quarter["units_per_inch"], 48.0);
    // A setup needs a document.
    assert!(matches!(s.execute("page.setup", &json!({})), Err(EngineError::Disabled { .. })));
}

#[test]
fn the_setup_of_a_page_that_is_not_there_or_has_no_size_is_an_error() {
    let mut s = blank_session();
    assert!(matches!(s.execute("page.setup", &json!({"page": 9})), Err(EngineError::BadParams { .. })));
    set_cells(&mut s, 1, &[("PageHeight", "0")]);
    let before = snapshot(&s);
    for (id, params) in [
        ("page.setup", json!({})),
        ("page.size", json!({"preset": "a4"})),
        ("page.orientation", json!({"orientation": "portrait"})),
        ("page.scale", json!({"preset": "arch_1_4"})),
        ("page.paper", json!({"kind": 1})),
    ] {
        assert!(matches!(s.execute(id, &params), Err(EngineError::Failed { .. })), "{id}");
    }
    assert_eq!(snapshot(&s), before);
    // Auto Size is a switch on the page's setup, which answers with what the page says, and a page that says nothing of its size cannot.
    assert!(matches!(s.execute("page.auto_size", &json!({"on": true})), Err(EngineError::Failed { .. })));
    assert_eq!(snapshot(&s), before);
}

// ------------------------------------------------------------------------------------------------------------------ more of Auto Size

/// Every corner of the box of shape `id` is on page 1.
fn on_the_page(s: &Session, id: u32) -> bool {
    let doc = s.document().unwrap();
    let (w, h) = (num(s, 1, "PageWidth"), num(s, 1, "PageHeight"));
    diacraft_engine::draw::shape_corners(doc, 1, id).unwrap().iter().all(|&(x, y)| x > -1e-6 && x < w + 1e-6 && y > -1e-6 && y < h + 1e-6)
}

#[test]
fn a_member_of_a_turned_group_grows_the_page_by_where_its_box_is_on_the_page() {
    let mut s = blank_session();
    let a = drop_at(&mut s, 3.0, 4.0);
    let b = drop_at(&mut s, 5.0, 4.0);
    let group = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "shape.rotate", json!({"ids": [group], "by": 90}));
    auto_size_on(&mut s);
    // The group moved 12 inches right: both boxes, which the group's turn carries, pass the edge at 11.
    run(&mut s, "shape.move", json!({"ids": [group], "dx": 12.0, "dy": 0.0}));
    assert!(num(&s, 1, "PageWidth") > 11.0);
    assert!(on_the_page(&s, a) && on_the_page(&s, b));
    // One member moved on its own, along the page's x axis (a quarter turn makes that its group's y): its box is found through the group too.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 30.0, "dy": 0.0}));
    assert!(on_the_page(&s, a), "the page grew for the member");
    assert!(on_the_page(&s, b));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn only_the_page_a_shape_is_put_on_grows() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({}));
    run(&mut s, "page.auto_size", json!({"on": true, "page": 2}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 2, "x": 12.0, "y": 4.0}));
    assert_eq!((cell(&s, 2, "PageWidth").as_deref(), cell(&s, 1, "PageWidth").as_deref()), (Some("22"), Some("11")));
    // Page 1 has Auto Size off, so a drop past its edge leaves it.
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 12.0, "y": 4.0}));
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("11"));
    // `page.setup` without `page` is the first page, with it the page named.
    assert_eq!(run(&mut s, "page.setup", json!({}))["auto_size"], false);
    assert_eq!(run(&mut s, "page.setup", json!({"page": 2}))["auto_size"], true);
}

#[test]
fn a_batch_that_grows_the_page_twice_is_one_undo_step() {
    let mut s = blank_session();
    let first = drop_at(&mut s, 3.0, 4.0);
    auto_size_on(&mut s);
    let before = snapshot(&s);
    run(
        &mut s,
        "command.batch",
        json!({"commands": [
            {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 12.0, "y": 4.0}},
            {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": -1.0, "y": 4.0}},
        ], "label": "Two drops"}),
    );
    // 12 grows it to 22; -1 takes a sheet on the left: 33, with everything carried by 11.
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("33"));
    assert_eq!(pin(&s, 1, first), (14.0, 4.0));
    assert_eq!(undo_steps(&s), before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn a_shape_with_an_endless_size_does_not_grow_the_page_or_stop_the_command() {
    let mut s = blank_session();
    s.edit("test: a shape of no size", |doc| {
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).unwrap();
        let mut sheet = diacraft_doc::Sheet::default();
        for (name, value) in [("PinX", "1"), ("PinY", "1"), ("Width", "1e400"), ("Height", "1"), ("LocPinX", "0"), ("LocPinY", "0")] {
            sheet.cells.insert(diacraft_doc::CellName::new(name), Cell::constant(Value::from_text(value)));
        }
        let mut shape = diacraft_doc::Shape::new(77);
        shape.sheet = std::sync::Arc::new(sheet);
        page.shapes.push(std::sync::Arc::new(shape));
        Ok(())
    })
    .unwrap();
    auto_size_on(&mut s);
    run(&mut s, "shape.move", json!({"ids": [77], "dx": 1.0, "dy": 0.0}));
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "PageHeight").as_deref()), (Some("11"), Some("8.5")));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_sheets_do_not_turn_when_the_page_grows_taller_than_wide() {
    // A new drawing's page holds no orientation for its paper, so the paper is held the way the page is (landscape, 11 by 8.5). The page grows upward
    // to 11 by 17, which is taller than wide: the sheet must stay 11 by 8.5, and the next growth to the right is 11 inches, not 8.5.
    let mut s = blank_session();
    auto_size_on(&mut s);
    drop_at(&mut s, 5.0, 8.9);
    assert_eq!((cell(&s, 1, "PageWidth").as_deref(), cell(&s, 1, "PageHeight").as_deref()), (Some("11"), Some("17")));
    assert_eq!(cell(&s, 1, "PrintPageOrientation").as_deref(), Some("2"), "the first growth fixes the way the sheet is held");
    drop_at(&mut s, 12.0, 4.0);
    assert_eq!(cell(&s, 1, "PageWidth").as_deref(), Some("22"));
    drop_at(&mut s, 5.0, 17.5);
    assert_eq!(cell(&s, 1, "PageHeight").as_deref(), Some("25.5"));
    let r = run(&mut s, "page.setup", json!({}));
    // The page is 22 by 25.5, taller than wide, and the paper is still held sideways.
    assert_eq!((r["orientation"].as_str(), r["paper"]["orientation"].as_str()), (Some("portrait"), Some("landscape")));
    assert_eq!((r["sheets"]["across"].as_u64(), r["sheets"]["down"].as_u64()), (Some(2), Some(3)));
}
