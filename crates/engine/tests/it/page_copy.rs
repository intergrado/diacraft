//! A page copied (`page.duplicate`), and background pages (`page.add {background}`, `page.type`, `page.background`): the page tab's Duplicate and the Insert tab's
//! Duplicate This Page and Background Page, and Page Setup's Page Properties (
//! and "Background pages").

use std::sync::Arc;

use diacraft_doc::{Document, ForeignData, Media, RawAttrs, Shape, ShapeType};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::{blank_session, check_session, page_names, run, snapshot};

fn id_of(v: &Value) -> u32 {
    u32::try_from(v["id"].as_u64().unwrap()).unwrap()
}

fn undo_steps(s: &Session) -> usize {
    s.history().unwrap().undo_len()
}

fn doc(s: &Session) -> &Document {
    s.document().unwrap()
}

fn drop_rect(s: &mut Session, page: u32, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y})))
}

/// A drawing of one page with two shapes joined by a connector, the page set to A4 portrait at 1:50.
fn drawn() -> Session {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"preset": "a4"}));
    run(&mut s, "page.orientation", json!({"orientation": "portrait"}));
    run(&mut s, "page.scale", json!({"preset": "metric_1_50"}));
    let a = drop_rect(&mut s, 1, 2.0, 3.0);
    let b = drop_rect(&mut s, 1, 5.0, 6.0);
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    s
}

fn err(s: &mut Session, id: &str, params: Value) -> EngineError {
    s.execute(id, &params).expect_err(id)
}

fn msg(e: &EngineError) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------------------------------------- Duplicate

#[test]
fn a_page_is_copied_with_its_shapes_connections_and_cells_in_one_undo_step() {
    let mut s = drawn();
    let before = snapshot(&s);
    let source_shapes = doc(&s).page(1).unwrap().shapes.len();
    assert_eq!(source_shapes, 3, "two rectangles and a connector");
    let made = run(&mut s, "page.duplicate", json!({"id": 1}));
    let copy = id_of(&made);
    assert_ne!(copy, 1);
    assert_eq!(made["from"], 1);
    assert_eq!(made["name"], "Page-1-2");
    // One more page, placed right after the page it was made from, holding what that page holds.
    assert_eq!(page_names(&s), ["Page-1", "Page-1-2"]);
    let (one, two) = (doc(&s).page(1).unwrap(), doc(&s).page(copy).unwrap());
    assert_eq!(two.shapes.len(), one.shapes.len());
    assert_eq!(two.shapes.iter().map(|x| x.id).collect::<Vec<_>>(), one.shapes.iter().map(|x| x.id).collect::<Vec<_>>(), "the shapes keep their ids");
    assert_eq!(two.connects.iter().count(), one.connects.iter().count());
    assert!(one.connects.iter().count() >= 2, "the connector is glued at both ends");
    assert_eq!(two.sheet, one.sheet, "the page cells: size, orientation, scale");
    assert_eq!(&*two.name, "Page-1-2");
    assert_eq!(two.name_u.as_deref(), Some("Page-1-2"), "its universal name follows");
    // The page setup it answers is the source's.
    let setup = run(&mut s, "page.setup", json!({"page": copy}));
    assert_eq!(
        (setup["size"]["id"].as_str(), setup["orientation"].as_str(), setup["scale"]["preset"].as_str()),
        (Some("a4"), Some("portrait"), Some("metric_1_50"))
    );
    // One undo step, and undo takes the page away whole.
    assert_eq!(undo_steps(&s), before.undo_steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Duplicate page"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(page_names(&s), ["Page-1", "Page-1-2"]);
    check_session(&s).unwrap();
}

#[test]
fn the_copy_is_a_page_of_its_own() {
    let mut s = drawn();
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": 1})));
    let first = doc(&s).page(1).unwrap().shapes.iter().next().unwrap().id;
    // Moving a shape of the copy leaves the source where it was, and the other way round.
    let source_before = doc(&s).page(1).unwrap().clone();
    run(&mut s, "shape.move", json!({"ids": [first], "dx": 1.0, "dy": 0.5, "page": copy}));
    assert_eq!(doc(&s).page(1).unwrap(), &source_before, "the source is as it was");
    assert_ne!(doc(&s).page(copy).unwrap().shapes, source_before.shapes);
    run(&mut s, "shape.delete", json!({"ids": [first], "page": 1}));
    assert!(doc(&s).page(copy).unwrap().shapes.iter().any(|x| x.id == first), "the copy still has the shape the source lost");
    // A shape dropped on the copy gets a number the copy has not used.
    let dropped = drop_rect(&mut s, copy, 3.0, 3.0);
    assert!(doc(&s).page(copy).unwrap().shapes.iter().filter(|x| x.id == dropped).count() == 1);
    check_session(&s).unwrap();
}

#[test]
fn the_copy_takes_the_next_name_that_is_free_and_sits_after_its_source() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Overview"}));
    run(&mut s, "page.add", json!({"name": "End"}));
    let a = id_of(&run(&mut s, "page.duplicate", json!({"id": 2})));
    assert_eq!(page_names(&s), ["Page-1", "Overview", "Overview-2", "End"], "right after the page it was made from, not at the far right");
    let b = id_of(&run(&mut s, "page.duplicate", json!({"id": 2})));
    assert_eq!(page_names(&s), ["Page-1", "Overview", "Overview-3", "Overview-2", "End"]);
    // A copy of a copy: the name of the copy plus a number.
    let c = run(&mut s, "page.duplicate", json!({"id": a}));
    assert_eq!(c["name"], "Overview-2-2");
    assert!(a != b && b != id_of(&c));
    // A name as long as a name can be is cut short to leave room for the number.
    let long = "n".repeat(255);
    run(&mut s, "page.rename", json!({"id": 1, "name": long}));
    let d = run(&mut s, "page.duplicate", json!({"id": 1}));
    let name = d["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 255);
    assert!(name.ends_with("-2"), "{name}");
    check_session(&s).unwrap();
}

#[test]
fn a_page_left_out_is_the_first_page_that_is_not_a_background() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"background": true}));
    assert_eq!(page_names(&s), ["VBackground-1", "Page-1"], "a file lists its backgrounds first");
    let made = run(&mut s, "page.duplicate", json!({}));
    assert_eq!(made["from"], 1);
    assert_eq!(made["name"], "Page-1-2");
}

#[test]
fn the_copy_of_a_page_that_names_a_background_names_it_too_and_a_background_is_copied_as_a_background() {
    let mut s = blank_session();
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    run(&mut s, "page.background", json!({"background": back, "page": 1}));
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": 1})));
    assert_eq!(doc(&s).page(copy).unwrap().back_page, Some(back));
    // The copy of the background page is a background page: it sits with the backgrounds, and no page names it.
    let back_copy = run(&mut s, "page.duplicate", json!({"id": back}));
    assert_eq!(back_copy["name"], "VBackground-1-2");
    let p = doc(&s).page(id_of(&back_copy)).unwrap();
    assert!(p.background);
    assert!(doc(&s).pages.iter().filter(|p| p.back_page == Some(id_of(&back_copy))).count() == 0);
    check_session(&s).unwrap();
}

#[test]
fn a_page_that_is_not_there_is_refused_and_so_is_a_drawing_that_has_all_the_pages_it_may() {
    let mut s = drawn();
    let before = snapshot(&s);
    let e = err(&mut s, "page.duplicate", json!({"id": 77}));
    assert!(matches!(e, EngineError::BadParams { .. }) && msg(&e).contains("no page 77"), "{e}");
    for junk in [json!({"id": -1}), json!({"id": "one"}), json!({"id": 1.5}), json!({"id": null, "x": 1})] {
        assert!(matches!(s.execute("page.duplicate", &junk), Err(EngineError::BadParams { .. })), "{junk}");
    }
    assert_eq!(snapshot(&s), before, "a refusal changes nothing");
    // A drawing of 1,000 pages cannot have another, and the refusal says so.
    s.edit("test: a thousand pages", |d| {
        while d.pages.len() < diacraft_doc::MAX_PAGES {
            let id = d.add_page(None).map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })?;
            assert!(id > 0);
        }
        Ok(())
    })
    .unwrap();
    let full = snapshot(&s);
    let e = err(&mut s, "page.duplicate", json!({"id": 1}));
    assert!(msg(&e).contains("1000 pages"), "the refusal names the limit: {e}");
    assert_eq!(snapshot(&s), full);
}

fn picture_shape(id: u32, rel: Option<&str>, picture: bool, kind: &str) -> Shape {
    let mut attrs = RawAttrs::new();
    attrs.push("ForeignType", kind);
    let foreign = ForeignData {
        attrs,
        rel_id: rel.map(Arc::from),
        picture: picture.then(|| {
            Arc::new(Media { part: Arc::from("/visio/media/image1.png"), content_type: Arc::from("image/png"), bytes: Arc::from(&b"\x89PNG"[..]) })
        }),
        ..ForeignData::default()
    };
    Shape { id, kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() }
}

fn put_on_page(s: &mut Session, page: u32, shape: Shape) {
    s.edit("test: a picture", |d| {
        let at = d.pages.iter().position(|p| p.id == page).unwrap();
        Arc::make_mut(d.pages.get_mut(at).unwrap()).shapes.push(Arc::new(shape));
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_picture_from_another_format_is_copied_as_the_source_holds_it() {
    // No relationship: the picture is the shape's own, as an `.odg`'s frame with an image is.
    let mut s = drawn();
    put_on_page(&mut s, 1, picture_shape(40, None, true, "Bitmap"));
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": 1})));
    let held = |page: u32| doc(&s).page(page).unwrap().shapes.iter().find(|x| x.id == 40).cloned().unwrap();
    assert!(Arc::ptr_eq(&held(1), &held(copy)), "the shape is shared, picture and all, until one of them is written to");
}

#[test]
fn a_picture_or_an_object_that_names_a_row_of_its_page_is_copied_with_the_page_in_one_undo_step() {
    // Refused these, as a save lost them; the writers now give the copy's part the rows its shapes name.
    for (what, shape) in [
        ("a picture of a file", picture_shape(41, Some("rId2"), true, "Bitmap")),
        ("an embedded object", picture_shape(41, Some("rId2"), true, "Object")),
        ("a picture that did not load", picture_shape(41, Some("rId2"), false, "Bitmap")),
    ] {
        let mut s = drawn();
        put_on_page(&mut s, 1, shape);
        let steps = undo_steps(&s);
        let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": 1})));
        assert_eq!(undo_steps(&s), steps + 1, "{what}");
        let held = |page: u32| doc(&s).page(page).unwrap().shapes.iter().find(|x| x.id == 41).cloned().unwrap();
        assert!(Arc::ptr_eq(&held(1), &held(copy)), "{what}: the shape is shared, picture and all, until one of them is written to");
        assert_eq!(held(copy).foreign.as_ref().unwrap().rel_id.as_deref(), Some("rId2"), "{what}: the copy names the row its source names");
    }
    // A picture deep in a group is copied too.
    let mut s = drawn();
    let inner = Shape {
        id: 50,
        kind: ShapeType::Foreign,
        foreign: Some(Arc::new(ForeignData { rel_id: Some(Arc::from("rId1")), ..ForeignData::default() })),
        ..Shape::default()
    };
    let mut group = Shape { id: 51, kind: ShapeType::Group, ..Shape::default() };
    group.children.push(Arc::new(inner));
    put_on_page(&mut s, 1, group);
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": 1})));
    let members = doc(&s).page(copy).unwrap().shapes.iter().find(|x| x.id == 51).map(|g| g.children.len());
    assert_eq!(members, Some(1));
}

// ---------------------------------------------------------------------------------------------------------------------- background pages

#[test]
fn a_background_page_is_added_named_and_placed_with_the_backgrounds() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Two"}));
    let steps = undo_steps(&s);
    let made = run(&mut s, "page.add", json!({"background": true}));
    assert_eq!(made["name"].as_str(), Some("VBackground-1"));
    assert_eq!(undo_steps(&s), steps + 1);
    let p = doc(&s).page(id_of(&made)).unwrap();
    assert!(p.background && p.back_page.is_none());
    // A file lists its backgrounds before its foreground pages.
    assert_eq!(page_names(&s), ["VBackground-1", "Page-1", "Two"]);
    // A second one goes after the first, with the next number; a name and a `like` are taken as for any page.
    run(&mut s, "page.size", json!({"preset": "a4", "page": 2}));
    let second = run(&mut s, "page.add", json!({"background": true, "like": 2}));
    assert_eq!(second["name"], "VBackground-2");
    assert_eq!(page_names(&s), ["VBackground-1", "VBackground-2", "Page-1", "Two"]);
    let named = run(&mut s, "page.add", json!({"background": true, "name": "Watermark"}));
    assert_eq!(named["name"], "Watermark");
    let setup = run(&mut s, "page.setup", json!({"page": id_of(&second)}));
    assert_eq!(setup["size"]["id"], "a4", "its setup is the page's it was made like");
    assert_eq!(setup["background"], true);
    // Undo takes it away whole.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(page_names(&s), ["VBackground-1", "VBackground-2", "Page-1", "Two"]);
    check_session(&s).unwrap();
}

#[test]
fn a_foreground_page_is_given_a_background_and_may_be_given_none_again() {
    let mut s = blank_session();
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let steps = undo_steps(&s);
    let r = run(&mut s, "page.background", json!({"background": back, "page": 1}));
    assert_eq!(r["back_page"], back);
    assert_eq!(doc(&s).page(1).unwrap().back_page, Some(back));
    assert_eq!(undo_steps(&s), steps + 1);
    assert_eq!(run(&mut s, "page.setup", json!({"page": 1}))["back_page"], back);
    // The same again changes nothing and is no step.
    run(&mut s, "page.background", json!({"background": back, "page": 1}));
    assert_eq!(undo_steps(&s), steps + 1);
    // None takes it away, by null or by the word.
    run(&mut s, "page.background", json!({"background": null, "page": 1}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, None);
    run(&mut s, "page.background", json!({"background": back, "page": 1}));
    run(&mut s, "page.background", json!({"background": "none", "page": 1}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, None);
    assert_eq!(run(&mut s, "page.setup", json!({"page": 1}))["back_page"], Value::Null);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, Some(back));
    // A page left out is the first page that is not a background.
    run(&mut s, "page.background", json!({"background": null}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, None);
    run(&mut s, "page.background", json!({"background": back}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, Some(back));
}

#[test]
fn only_a_background_page_may_be_named_and_never_the_page_itself_or_one_that_would_name_it_back() {
    let mut s = blank_session();
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    let a = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let b = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let before = snapshot(&s);
    // A foreground page is no background.
    let e = err(&mut s, "page.background", json!({"background": two, "page": 1}));
    assert!(matches!(e, EngineError::BadParams { .. }) && msg(&e).contains("not a background page"), "{e}");
    // A page that is not there.
    let e = err(&mut s, "page.background", json!({"background": 99, "page": 1}));
    assert!(matches!(e, EngineError::BadParams { .. }) && msg(&e).contains("no page 99"), "{e}");
    let e = err(&mut s, "page.background", json!({"background": a, "page": 99}));
    assert!(matches!(e, EngineError::BadParams { .. }) && msg(&e).contains("no page 99"), "{e}");
    // A background cannot be its own background.
    let e = err(&mut s, "page.background", json!({"background": a, "page": a}));
    assert!(msg(&e).contains("its own background"), "{e}");
    // A background may have one of its own, but not one that has it for its own: that is a loop.
    run(&mut s, "page.background", json!({"background": b, "page": a}));
    assert_eq!(doc(&s).page(a).unwrap().back_page, Some(b));
    let e = err(&mut s, "page.background", json!({"background": a, "page": b}));
    assert!(msg(&e).contains("loop") || msg(&e).contains("itself"), "{e}");
    for junk in [
        json!({}),
        json!({"background": "a"}),
        json!({"background": -1, "page": 1}),
        json!({"background": 1.5, "page": 1}),
        json!({"background": true}),
    ] {
        assert!(matches!(s.execute("page.background", &junk), Err(EngineError::BadParams { .. })), "{junk}");
    }
    assert_ne!(snapshot(&s).document, before.document, "the one good call went through");
    check_session(&s).unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document, "and the refusals changed nothing");
}

#[test]
fn a_page_becomes_a_background_and_a_background_becomes_a_page_again() {
    let mut s = blank_session();
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    let steps = undo_steps(&s);
    let r = run(&mut s, "page.type", json!({"type": "background", "page": two}));
    assert_eq!((r["background"].as_bool(), r["page"].as_u64()), (Some(true), Some(u64::from(two))));
    assert!(doc(&s).page(two).unwrap().background);
    assert_eq!(undo_steps(&s), steps + 1);
    // It sits with the backgrounds, before the foreground page.
    assert_eq!(page_names(&s), ["Two", "Page-1"]);
    // The same again: nothing to do, no step.
    run(&mut s, "page.type", json!({"type": "background", "page": two}));
    assert_eq!(undo_steps(&s), steps + 1);
    // A page names it, then it goes back to the foreground: the page no longer names a page that is not a background, and the answer says how many were let go.
    run(&mut s, "page.background", json!({"background": two, "page": 1}));
    let r = run(&mut s, "page.type", json!({"type": "foreground", "page": two}));
    assert_eq!((r["background"].as_bool(), r["unassigned"].as_u64()), (Some(false), Some(1)));
    assert_eq!(doc(&s).page(1).unwrap().back_page, None);
    assert!(!doc(&s).page(two).unwrap().background);
    // One undo step puts both back.
    run(&mut s, "edit.undo", json!({}));
    assert!(doc(&s).page(two).unwrap().background);
    assert_eq!(doc(&s).page(1).unwrap().back_page, Some(two));
    check_session(&s).unwrap();
}

#[test]
fn a_drawing_keeps_a_page_that_is_not_a_background_and_a_type_must_be_one_of_the_two() {
    let mut s = blank_session();
    let before = snapshot(&s);
    let e = err(&mut s, "page.type", json!({"type": "background", "page": 1}));
    assert!(matches!(e, EngineError::Failed { .. }) && msg(&e).contains("at least one page that is not a background"), "{e}");
    for junk in [json!({}), json!({"type": "middle"}), json!({"type": 3}), json!({"type": "background", "page": 9}), json!({"page": 1})] {
        assert!(matches!(s.execute("page.type", &junk), Err(EngineError::BadParams { .. })), "{junk}");
    }
    assert_eq!(snapshot(&s), before);
    // A foreground page that has a background of its own may become one, and keeps what it names.
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    run(&mut s, "page.background", json!({"background": back, "page": two}));
    run(&mut s, "page.type", json!({"type": "background", "page": two}));
    assert_eq!(doc(&s).page(two).unwrap().back_page, Some(back), "a background page may have a background of its own");
}

#[test]
fn a_background_page_that_a_page_names_is_taken_away_and_the_page_names_none() {
    let mut s = blank_session();
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    run(&mut s, "page.background", json!({"background": back, "page": 1}));
    let r = run(&mut s, "page.delete", json!({"id": back}));
    assert_eq!(r["unassigned"], 1, "the answer says how many pages let go of it");
    assert_eq!(doc(&s).page(1).unwrap().back_page, None);
    assert_eq!(page_names(&s), ["Page-1"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).page(1).unwrap().back_page, Some(back), "undo puts the assignment back with the page");
}

// ---------------------------------------------------------------------------------------------------------------------- the pixels

const PX: f64 = 100.0;

fn paint(s: &Session, page: u32) -> (Rendered, f64) {
    let d = doc(s);
    let view = View::fit_page(d, page, PX).unwrap();
    let height = f64::from(view.height) / PX;
    (Renderer::new().render(d, page, &view), height)
}

/// The colour at (`x`, `y`) inches from the page's lower-left corner.
fn at(r: &(Rendered, f64), x: f64, y: f64) -> [u8; 3] {
    let (r, h) = (&r.0, r.1);
    let (w, rows) = (r.width as usize, r.height as usize);
    let col = ((x * PX) as usize).min(w - 1);
    let row = (((h - y) * PX) as usize).min(rows - 1);
    let i = (row * w + col) * 4;
    [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2]]
}

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

/// A drawing whose page 1 has a blue square at (6, 5) and a background page with a red rectangle across (4, 4), the page naming it when `named`.
fn with_background(named: bool) -> Session {
    let mut s = blank_session();
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let red = drop_rect(&mut s, back, 4.0, 4.0);
    run(&mut s, "shape.size", json!({"ids": [red], "width": 4.0, "height": 3.0, "page": back}));
    run(&mut s, "shape.format", json!({"ids": [red], "fill": "#FF0000", "line": "none", "page": back}));
    let blue = drop_rect(&mut s, 1, 5.0, 4.0);
    run(&mut s, "shape.size", json!({"ids": [blue], "width": 2.0, "height": 2.0, "page": 1}));
    run(&mut s, "shape.format", json!({"ids": [blue], "fill": "#0000FF", "line": "none", "page": 1}));
    if named {
        run(&mut s, "page.background", json!({"background": back, "page": 1}));
    }
    s
}

#[test]
fn a_background_page_draws_under_the_page_that_names_it() {
    let s = with_background(true);
    let shown = paint(&s, 1);
    assert_eq!(at(&shown, 3.0, 4.0), RED, "the background's shape shows where the page has none");
    assert_eq!(at(&shown, 5.0, 4.0), BLUE, "the page's own shape is over the background's");
    assert_eq!(at(&shown, 9.5, 1.0), WHITE, "and paper is paper");
    // The background page shown by itself draws its own shape and not the page's.
    let back = doc(&s).pages.iter().find(|p| p.background).unwrap().id;
    let alone = paint(&s, back);
    assert_eq!((at(&alone, 3.0, 4.0), at(&alone, 5.0, 4.0)), (RED, RED));
    // Taking the assignment away takes the shape off the page.
    let mut s = s;
    run(&mut s, "page.background", json!({"background": null, "page": 1}));
    let bare = paint(&s, 1);
    assert_eq!((at(&bare, 3.0, 4.0), at(&bare, 5.0, 4.0)), (WHITE, BLUE));
}

#[test]
fn a_background_that_no_page_names_is_not_drawn_on_any_page() {
    let s = with_background(false);
    assert_eq!(at(&paint(&s, 1), 3.0, 4.0), WHITE);
}

#[test]
fn a_background_of_a_background_draws_farthest_first() {
    let mut s = with_background(true);
    let near = doc(&s).page(1).unwrap().back_page.unwrap();
    let far = id_of(&run(&mut s, "page.add", json!({"background": true, "name": "Far"})));
    let wide = drop_rect(&mut s, far, 4.0, 4.0);
    run(&mut s, "shape.size", json!({"ids": [wide], "width": 8.0, "height": 6.0, "page": far}));
    run(&mut s, "shape.format", json!({"ids": [wide], "fill": "#00FF00", "line": "none", "page": far}));
    run(&mut s, "page.background", json!({"background": far, "page": near}));
    let shown = paint(&s, 1);
    assert_eq!(at(&shown, 3.0, 4.0), RED, "the nearer background is over the farther");
    assert_eq!(at(&shown, 1.0, 1.5), [0, 255, 0], "the farther one shows where the nearer has nothing");
}

// ---------------------------------------------------------------------------------------------------------------------- files

fn assert_backgrounds_survive(extension: &str) {
    let dir = Dir::new();
    let path = dir.path(&format!("backgrounds.{extension}"));
    let mut s = with_background(true);
    run(&mut s, "page.duplicate", json!({"id": 1}));
    let names_before = page_names(&s);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    check_session(&again).unwrap();
    assert_eq!(page_names(&again), names_before, "{extension}: the pages and their order");
    let back = doc(&again).pages.iter().find(|p| p.background).expect("the background page");
    assert_eq!(&*back.name, "VBackground-1");
    assert_eq!(back.shapes.len(), 1, "{extension}: the background's own shape");
    let named: Vec<(String, Option<u32>)> = doc(&again).pages.iter().filter(|p| !p.background).map(|p| (p.name.to_string(), p.back_page)).collect();
    assert_eq!(named, [("Page-1".to_owned(), Some(back.id)), ("Page-1-2".to_owned(), Some(back.id))], "{extension}: both pages name the background");
    // The pages draw as they did.
    let shown = paint(&again, doc(&again).pages.iter().find(|p| !p.background).unwrap().id);
    assert_eq!((at(&shown, 3.0, 4.0), at(&shown, 5.0, 4.0)), (RED, BLUE), "{extension}");
    // The copy has the shapes of the page it was made from.
    let first = doc(&again).pages.iter().find(|p| &*p.name == "Page-1").unwrap();
    let copy = doc(&again).pages.iter().find(|p| &*p.name == "Page-1-2").unwrap();
    assert_eq!(copy.shapes.len(), first.shapes.len(), "{extension}");
}

#[test]
fn background_pages_their_assignment_and_a_copied_page_survive_a_vsdx_save_and_reopen() {
    assert_backgrounds_survive("vsdx");
}

#[test]
fn background_pages_their_assignment_and_a_copied_page_survive_a_diacraft_save_and_reopen() {
    let dir = Dir::new();
    let path = dir.path("b.diacraft");
    let mut s = with_background(true);
    run(&mut s, "page.duplicate", json!({"id": 1}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    assert!(s.document() == again.document(), "the model read back is the model saved");
    assert_backgrounds_survive("diacraft");
}

#[test]
fn a_background_added_to_a_vsdx_file_that_was_opened_is_saved_into_it() {
    let dir = Dir::new();
    let path = dir.path("grown.vsdx");
    let mut s = drawn();
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let back = id_of(&run(&mut s, "page.add", json!({"background": true, "name": "Frame"})));
    run(&mut s, "page.background", json!({"background": back, "page": 1}));
    run(&mut s, "page.duplicate", json!({"id": 1}));
    run(&mut s, "file.save", json!({}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(page_names(&again), page_names(&s));
    let frame = doc(&again).pages.iter().find(|p| p.background).unwrap();
    assert_eq!(&*frame.name, "Frame");
    assert_eq!(doc(&again).page(1).unwrap().back_page, Some(frame.id));
    assert_eq!(doc(&again).pages.iter().filter(|p| p.back_page == Some(frame.id)).count(), 2, "the page and its copy");
    // Changing the assignment of the page and saving again is saved too.
    run(&mut again, "page.background", json!({"background": null, "page": 1}));
    run(&mut again, "file.save", json!({}));
    let mut last = Session::new();
    run(&mut last, "file.open", json!({"path": path}));
    assert_eq!(doc(&last).page(1).unwrap().back_page, None);
    check_session(&last).unwrap();
}

#[test]
fn a_page_made_a_background_and_made_a_page_again_in_a_vsdx_file_that_was_opened_is_saved_into_it() {
    let dir = Dir::new();
    let path = dir.path("retyped.vsdx");
    let mut s = drawn();
    run(&mut s, "page.add", json!({"name": "Two"}));
    run(&mut s, "page.add", json!({"name": "Three"}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    // The middle page becomes a background page, and the first page names it.
    run(&mut s, "page.type", json!({"type": "background", "page": 2}));
    run(&mut s, "page.background", json!({"background": 2, "page": 1}));
    assert_eq!(page_names(&s), ["Two", "Page-1", "Three"]);
    run(&mut s, "file.save", json!({}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(page_names(&again), ["Two", "Page-1", "Three"], "a file lists its backgrounds first");
    assert_eq!(pages_of(&again), [(2, true, None), (1, false, Some(2)), (3, false, None)]);
    // It goes back to the foreground: the page that named it names none, and the page is the last page that is not a background.
    run(&mut again, "page.type", json!({"type": "foreground", "page": 2}));
    assert_eq!(page_names(&again), ["Page-1", "Three", "Two"]);
    run(&mut again, "file.save", json!({}));
    let mut last = Session::new();
    run(&mut last, "file.open", json!({"path": path}));
    assert_eq!(page_names(&last), ["Page-1", "Three", "Two"]);
    assert_eq!(pages_of(&last), [(1, false, None), (3, false, None), (2, false, None)]);
    check_session(&last).unwrap();
}

/// `(id, background, back_page)` of each page of the model, in order.
fn pages_of(s: &Session) -> Vec<(u32, bool, Option<u32>)> {
    doc(s).pages.iter().map(|p| (p.id, p.background, p.back_page)).collect()
}

const PNG: [u8; 7] = [0x89, b'P', b'N', b'G', 1, 2, 3];
const EMF: [u8; 6] = [1, 0, 0, 0, 9, 9];
const OLE: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];

/// A page with a picture (shape 7) and an embedded object (shape 8, whose own part has a row to the EMF it is shown as) beside a box (shape 6), as a file holds them: each names a
/// relationship of the page's part.
fn drawing_with_a_picture() -> Vec<u8> {
    use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape};
    let plain = shape(6, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let picture = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="6"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let object = r#"<Shape ID="8" Type="Foreign"><Cell N="PinX" V="3"/><Cell N="PinY" V="5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{plain}{picture}{object}")).rel("rId1", &format!("{OD}image"), "../media/image1.png").rel(
            "rId2",
            &format!("{OD}oleObject"),
            "../embeddings/oleObject1.bin",
        ))
        .with(
            Pkg::new()
                .part("/visio/media/image1.png", "image/png", PNG)
                .part("/visio/embeddings/oleObject1.bin", "application/vnd.openxmlformats-officedocument.oleObject", OLE)
                .part("/visio/media/image2.emf", "image/x-emf", EMF)
                .rel("/visio/embeddings/oleObject1.bin", "rId1", &format!("{OD}image"), "../media/image2.emf"),
        )
        .build()
}

/// The file at `path` opened in a session of its own: it opens with no warning, and each of `pages` shows the picture and the object of the source page, each through a row of its own part to
/// the one part the file holds for it.
fn assert_both_pages_show_them(path: &str, pages: &[u32], what: &str) {
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(opened["file"]["warning_count"], 0, "{what}: {opened}");
    let bytes = std::fs::read(path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    for page in pages {
        let p = doc(&again).page(*page).unwrap_or_else(|| panic!("{what}: no page {page}"));
        let part = p.part_name.clone().unwrap_or_else(|| panic!("{what}: page {page} has no part"));
        let rels = pkg.rels(&part).unwrap();
        for (shape, shows, ty, target) in
            [(7, &PNG[..], "image", "/visio/media/image1.png"), (8, &EMF[..], "oleObject", "/visio/embeddings/oleObject1.bin")]
        {
            let foreign = p.shapes.iter().find(|x| x.id == shape).and_then(|x| x.foreign.clone()).unwrap();
            assert_eq!(foreign.picture.as_ref().map(|m| m.bytes.to_vec()), Some(shows.to_vec()), "{what}: shape {shape} of page {page}");
            let row = rels.get(foreign.rel_id.as_deref().unwrap()).unwrap_or_else(|| panic!("{what}: shape {shape} of page {page} names no row"));
            assert!(row.ty.ends_with(ty) && row.target.as_deref() == Some(target), "{what}: shape {shape} of page {page}: {row:?}");
        }
    }
    for (bytes, name) in [(&PNG[..], "/visio/media/image1.png"), (&OLE[..], "/visio/embeddings/oleObject1.bin")] {
        let holding: Vec<&str> = pkg.parts().filter(|n| pkg.part(n) == Some(bytes)).collect();
        assert_eq!(holding, [name], "{what}: one part for the two pages, and nothing copied");
    }
}

#[test]
fn a_page_with_a_picture_and_an_object_of_a_vsdx_file_is_copied_and_both_pages_show_them_after_a_save_and_a_reopen() {
    let dir = Dir::new();
    let path = dir.path("pictures.vsdx");
    std::fs::write(&path, drawing_with_a_picture()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let source = doc(&s).pages.get(0).unwrap().id;
    let made = run(&mut s, "page.duplicate", json!({"id": source}));
    let copy = id_of(&made);
    assert_eq!(page_names(&s), ["P", "P-2"]);
    run(&mut s, "file.save", json!({}));
    assert_both_pages_show_them(&path, &[source, copy], "saved into the file");
    // The copy is edited and the drawing saved again: its part is patched, and the rows stay.
    run(&mut s, "shape.move", json!({"page": copy, "ids": [6], "dx": 0.5, "dy": 0.0}));
    run(&mut s, "file.save", json!({}));
    assert_both_pages_show_them(&path, &[source, copy], "edited and saved again");
    // The source is taken out: the copy keeps them.
    run(&mut s, "page.delete", json!({"id": source}));
    run(&mut s, "file.save", json!({}));
    assert_both_pages_show_them(&path, &[copy], "the source taken out");
}

#[test]
fn a_copied_page_with_a_picture_and_an_object_keeps_them_through_a_diacraft_file_and_the_model_writer() {
    let dir = Dir::new();
    let path = dir.path("pictures.vsdx");
    std::fs::write(&path, drawing_with_a_picture()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let source = doc(&s).pages.get(0).unwrap().id;
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": source})));
    let native = dir.path("pictures.diacraft");
    run(&mut s, "file.save_as", json!({"path": native}));
    // A session of its own opens the `.diacraft` file and saves it as a VSDX file: the model writer, with no file to write into.
    let mut v = Session::new();
    run(&mut v, "file.open", json!({"path": native}));
    let out = dir.path("again.vsdx");
    run(&mut v, "file.save_as", json!({"path": out}));
    assert_both_pages_show_them(&out, &[source, copy], "through a .diacraft file");
}

/// A drawing whose pages the `.odg` cannot say (a background page, which is a master page that holds shapes) is refused when it is saved as an `.odg` from the model, in
/// words that say what is missing and the way out.
#[test]
fn a_drawing_made_in_the_window_with_a_background_page_is_not_saved_as_an_odg_and_the_refusal_says_the_way_out() {
    let dir = Dir::new();
    let mut s = with_background(true);
    let e = err(&mut s, "file.save_as", json!({"path": dir.path("b.odg")}));
    let words = msg(&e);
    assert!(words.contains(".odg") && words.contains(".diacraft") && words.contains(".vsdx"), "the refusal names the way out: {words}");
    check_session(&s).unwrap();
}

fn odg_of_two_pages() -> crate::odg_build::Drawing {
    use crate::odg_build::{Drawing, rect};
    Drawing::new().page(&[rect("A", (2.0, 3.0, 4.0, 2.0), ""), rect("B", (12.0, 3.0, 4.0, 2.0), "")].concat()).page(&rect(
        "C",
        (2.0, 8.0, 4.0, 2.0),
        "",
    ))
}

/// A page copied in a drawing opened from an `.odg` is a page the file gets: the copy has the shapes of its source.
#[test]
fn a_page_copied_in_a_drawing_opened_from_an_odg_is_saved_into_the_file() {
    use crate::odg_build::{open_drawing, read_back};
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &odg_of_two_pages());
    let first = doc(&s).pages.get(0).unwrap().id;
    let made = run(&mut s, "page.duplicate", json!({"id": first}));
    assert_eq!(made["name"], "page1-2");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "{saved}");
    let back = read_back(&dir.read("a.odg")).0;
    let names: Vec<String> = back.pages.iter().map(|p| p.name.to_string()).collect();
    assert_eq!(names, ["page1", "page1-2", "page2"], "the copy is right after its source, in the file as in the model");
    assert_eq!(back.pages.get(1).unwrap().shapes.len(), 2, "with the two shapes of the page it was made from");
    check_session(&s).unwrap();
}

/// A background page is an `.odg` master page that holds shapes; a new one is not written into a file that was opened (`odg/src/write/structure.rs`), and the save says so and
/// the way out. The page and the model are as they were, and a later save that does not need it goes through.
#[test]
fn a_background_page_added_to_a_drawing_opened_from_an_odg_is_refused_at_the_save_in_words() {
    use crate::odg_build::open_drawing;
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &odg_of_two_pages());
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let words = crate::odg_file::refused(&mut s);
    assert!(words.contains("a background page") && words.contains("edit.undo"), "the save names what cannot be written and the way out: {words}");
    // Taking the page away again leaves a drawing the file can take.
    run(&mut s, "page.delete", json!({"id": back}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], false, "{saved}");
    // Saved as `.diacraft` the drawing keeps a background page (the way out of a file that cannot take it).
    let back = id_of(&run(&mut s, "page.add", json!({"background": true})));
    let native = dir.path("kept.diacraft");
    run(&mut s, "file.save_as", json!({"path": native}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": native}));
    assert!(doc(&again).pages.iter().any(|p| p.background && p.id == back), "the background page is in the `.diacraft` file");
    check_session(&s).unwrap();
}

// ------------------------------------------------------------------------------------------------- Duplicate: a picture a shape keeps

/// A page with a box (shape 6) and a shape of `Type` `Shape` that holds a picture's `ForeignData` (shape 7): the reader does not read it and keeps it whole,
/// and its `Rel` names a row of the page's part.
fn drawing_with_a_kept_picture() -> Vec<u8> {
    use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape};
    let plain = shape(6, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let kept = r#"<Shape ID="7" Type="Shape"><Cell N="PinX" V="6"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{plain}{kept}")).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        // The picture is typed by a `Default` row, as the model writer types one (which of the two a file chose is not in the model), so that the two writers' files can be held to one content.
        .with(Pkg::new().default_type("png", "image/png").part_without_override("/visio/media/image1.png", PNG))
        .build()
}

/// The file at `path` opened in a session of its own: it opens with no warning, every `r:id` of the part of each of `pages` names a row of that part, and the kept `ForeignData` of shape 7
/// names an `image` row to the one part that holds the picture.
fn assert_kept_rows(path: &str, pages: &[u32], what: &str) {
    use diacraft_doc::RawNode;
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(opened["file"]["warning_count"], 0, "{what}: {opened}");
    let bytes = std::fs::read(path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    for page in pages {
        let p = doc(&again).page(*page).unwrap_or_else(|| panic!("{what}: no page {page}"));
        let part = p.part_name.clone().unwrap_or_else(|| panic!("{what}: page {page} has no part"));
        let rels = pkg.rels(&part).unwrap();
        let text = String::from_utf8(pkg.part(&part).unwrap().to_vec()).unwrap();
        for (at, _) in text.match_indices(r#"r:id=""#) {
            let rest = &text[at + 6..];
            let id = &rest[..rest.find('"').unwrap()];
            assert!(rels.get(id).is_some(), "{what}: the part of page {page} names {id}, which is no row of it");
        }
        let shape = p.shapes.iter().find(|s| s.id == 7).unwrap_or_else(|| panic!("{what}: page {page} has no shape 7"));
        assert!(shape.foreign.is_none(), "{what}: the ForeignData of a shape of Type Shape is kept, not read");
        let id = shape
            .passthrough
            .iter()
            .find_map(|placed| match &placed.node {
                RawNode::Element(e) if &*e.name == "ForeignData" => e.children.iter().find_map(|c| match c {
                    RawNode::Element(rel) if &*rel.name == "Rel" => rel.attrs.get("r:id").map(str::to_owned),
                    _ => None,
                }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{what}: shape 7 of page {page} keeps no ForeignData"));
        let row = rels.get(&id).unwrap_or_else(|| panic!("{what}: the kept Rel of page {page} names no row"));
        assert!(row.ty.ends_with("image") && row.target.as_deref() == Some("/visio/media/image1.png"), "{what}: page {page}: {row:?}");
    }
    let holding: Vec<&str> = pkg.parts().filter(|n| pkg.part(n) == Some(&PNG[..])).collect();
    assert_eq!(holding, ["/visio/media/image1.png"], "{what}: one part for the two pages, and nothing copied");
}

/// The save said nothing but what the builder's package leaves it to say (its `docProps/app.xml` has no list of page names to keep in step).
fn says_nothing_of_rows(saved: &Value, what: &str) {
    let words: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(words.iter().all(|w| w.contains("docProps/app.xml")), "{what}: {saved}");
}

#[test]
fn a_page_whose_shape_of_type_shape_keeps_a_picture_is_copied_and_both_pages_name_its_row_through_either_writer() {
    let dir = Dir::new();
    let path = dir.path("kept.vsdx");
    std::fs::write(&path, drawing_with_a_kept_picture()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let source = doc(&s).pages.get(0).unwrap().id;
    let copy = id_of(&run(&mut s, "page.duplicate", json!({"id": source})));
    let saved = run(&mut s, "file.save", json!({}));
    says_nothing_of_rows(&saved, "saved into the file");
    assert_kept_rows(&path, &[source, copy], "saved into the file");
    let first = std::fs::read(&path).unwrap();
    // The model writer: the file saved as `.diacraft`, opened in a session of its own and saved as a VSDX file. The content check (the design's 4.3) of the two VSDX files says same.
    let native = dir.path("kept.diacraft");
    let mut n = Session::new();
    run(&mut n, "file.open", json!({"path": path}));
    run(&mut n, "file.save_as", json!({"path": native}));
    let mut m = Session::new();
    run(&mut m, "file.open", json!({"path": native}));
    let out = dir.path("again.vsdx");
    let written = run(&mut m, "file.save_as", json!({"path": out}));
    says_nothing_of_rows(&written, "the model writer");
    assert_kept_rows(&out, &[source, copy], "through a .diacraft file and the model writer");
    let same = diacraft_vsdx::compare_content(&first, &std::fs::read(&out).unwrap(), &diacraft_vsdx::Limits::default()).unwrap();
    assert!(same.same(), "the content of the file the model writer made is the file's: {:?}", same.shown);
    // Each file is edited in its session (the copy's box moved) and saved into again: the copy's part is patched, and its rows stay.
    for (session, file, what) in [(&mut s, &path, "the first file, edited"), (&mut m, &out, "the model writer's file, edited")] {
        run(session, "shape.move", json!({"page": copy, "ids": [6], "dx": 0.5, "dy": 0.0}));
        let saved = run(session, "file.save", json!({}));
        says_nothing_of_rows(&saved, what);
        assert_kept_rows(file, &[source, copy], what);
    }
    check_session(&s).unwrap();
}
