//! `text.format` with `bullet`, `indent`, `case`, `direction` and `vertical`, through `Session::execute` as the control channel and the CLI call it
//! The parameters, the refusals in words, the undo step, the picture, and what `shape.formatting` says back.

use diacraft_engine::Session;
use diacraft_render::Rendered;
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::format::{px, render};
use crate::{bad_params_msg, blank_session, check_session, run};

/// Where the ink of a rectangle of the page is, in inches: left, bottom, right and top of the dark pixels.
fn ink_box(r: &Rendered, rect: (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)> {
    let step = 0.01;
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

/// The rectangle `drop_shape` makes at (3, 4): x 2.25 to 3.75, y 3.5 to 4.5, a little inside.
const BOX: (f64, f64, f64, f64) = (2.3, 3.55, 3.7, 4.45);

fn labelled(text: &str) -> (Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({ "id": id, "text": text }));
    run(&mut s, "text.format", json!({ "ids": [id], "align": "left", "size": 10 }));
    (s, id)
}

fn width(b: (f64, f64, f64, f64)) -> f64 {
    b.2 - b.0
}

#[test]
fn a_bullet_is_drawn_at_the_margin_and_the_text_stands_off_it() {
    let (mut s, id) = labelled("Hello");
    let plain = ink_box(&render(&s), BOX).expect("the label is drawn");
    run(&mut s, "text.format", json!({ "ids": [id], "bullet": 1 }));
    let bulleted = ink_box(&render(&s), BOX).expect("drawn");
    // The bullet is at the margin, where the text began, and the text a quarter inch from it: the ink reaches a quarter inch further right.
    assert!((bulleted.0 - plain.0).abs() < 0.06, "the left edge is the margin's: {bulleted:?} against {plain:?}");
    assert!((bulleted.2 - plain.2 - 0.25).abs() < 0.04, "the text stands a quarter inch off: {bulleted:?} against {plain:?}");
    run(&mut s, "text.format", json!({ "ids": [id], "bullet": 0 }));
    assert_eq!(ink_box(&render(&s), BOX), Some(plain), "and takes the bullet away");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_indent_step_moves_the_text_a_quarter_inch_and_a_step_back_returns_it() {
    let (mut s, id) = labelled("Hello");
    let plain = ink_box(&render(&s), BOX).unwrap();
    run(&mut s, "text.format", json!({ "ids": [id], "indent": "increase" }));
    let moved = ink_box(&render(&s), BOX).unwrap();
    assert!((moved.0 - plain.0 - 0.25).abs() < 0.03, "{moved:?} against {plain:?}");
    run(&mut s, "text.format", json!({ "ids": [id], "indent": "decrease" }));
    assert_eq!(ink_box(&render(&s), BOX), Some(plain));
}

#[test]
fn change_case_changes_the_letters_drawn_and_not_the_text() {
    let (mut s, id) = labelled("hello world");
    let plain = ink_box(&render(&s), BOX).unwrap();
    run(&mut s, "text.format", json!({ "ids": [id], "case": "all_caps" }));
    let capitals = ink_box(&render(&s), BOX).unwrap();
    assert!(width(capitals) > width(plain) * 1.15, "capitals are wider: {capitals:?} against {plain:?}");
    let d = doc(&s);
    let text = d.shape(&d.find_shape(diacraft_doc::Owner::Page(1), id).unwrap()).unwrap().text.clone().unwrap();
    assert!(text.parts.iter().any(|p| matches!(p, diacraft_doc::TextPart::Chars(c) if &**c == "hello world")), "the stored text is as typed");
    run(&mut s, "text.format", json!({ "ids": [id], "case": "small_caps" }));
    let small = ink_box(&render(&s), BOX).unwrap();
    assert!(
        width(small) > width(plain) * 0.97 && width(small) < width(capitals) * 0.95,
        "small capitals lie between: {small:?} {plain:?} {capitals:?}"
    );
    run(&mut s, "text.format", json!({ "ids": [id], "case": "normal" }));
    assert_eq!(ink_box(&render(&s), BOX), Some(plain));
}

#[test]
fn vertical_text_runs_down_the_box_and_horizontal_puts_it_back() {
    let (mut s, id) = labelled("Hello");
    let across = ink_box(&render(&s), BOX).unwrap();
    assert!(width(across) > (across.3 - across.1) * 1.5, "{across:?}");
    run(&mut s, "text.format", json!({ "ids": [id], "vertical": true }));
    let down = ink_box(&render(&s), BOX).unwrap();
    assert!(down.3 - down.1 > width(down) * 1.5, "a column: {down:?}");
    run(&mut s, "text.format", json!({ "ids": [id], "vertical": false }));
    assert_eq!(ink_box(&render(&s), BOX), Some(across));
}

#[test]
fn each_is_one_undo_step_and_undo_restores_the_drawing() {
    let (mut s, id) = labelled("one\ntwo");
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    let r = run(
        &mut s,
        "text.format",
        json!({ "ids": [id], "bullet": 3, "indent": "increase", "case": "initial_caps", "direction": "rtl", "vertical": true }),
    );
    assert_eq!(r, json!({ "page": 1, "formatted": 1 }));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Format text"));
    let after = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    // Asking for what is there is no step.
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "text.format", json!({ "ids": [id], "bullet": 3, "case": "initial_caps", "direction": "rtl", "vertical": true }));
    assert_eq!(r, json!({ "page": 1, "formatted": 0 }));
    assert_eq!(s.history().unwrap().undo_len(), steps);
}

#[test]
fn a_range_gets_the_bullet_on_its_paragraph_and_the_case_on_its_letters() {
    let (mut s, id) = labelled("one\ntwo\nthree");
    run(&mut s, "text.format", json!({ "ids": [id], "start": 5, "end": 6, "bullet": 1, "case": "all_caps" }));
    let one = run(&mut s, "shape.formatting", json!({ "ids": [id], "start": 5, "end": 6 }));
    assert_eq!(one["shapes"][0]["bullet"], 1);
    assert_eq!(one["shapes"][0]["case"], "all_caps");
    let all = run(&mut s, "shape.formatting", json!({ "ids": [id], "start": 0, "end": 13 }));
    assert_eq!(all["shapes"][0]["bullet"], "mixed", "{all}");
    assert_eq!(all["shapes"][0]["case"], "mixed");
    assert_eq!(all["shapes"][0]["mixed"], json!(["bullet", "case"]));
    let first = run(&mut s, "shape.formatting", json!({ "ids": [id], "start": 0, "end": 2 }));
    assert_eq!(first["shapes"][0]["bullet"], 0);
    assert_eq!(first["shapes"][0]["case"], "normal");
}

#[test]
fn shape_formatting_says_what_the_cells_say() {
    let (mut s, id) = labelled("one");
    let r = run(&mut s, "shape.formatting", json!({ "ids": [id] }));
    let one = &r["shapes"][0];
    assert_eq!((one["bullet"].clone(), one["ind_left"].clone(), one["case"].clone()), (json!(0), json!(0.0), json!("normal")));
    assert_eq!((one["direction"].clone(), one["vertical"].clone()), (json!("ltr"), json!(false)));
    run(&mut s, "text.format", json!({ "ids": [id], "bullet": 5, "indent": "increase", "case": "all_caps", "direction": "rtl", "vertical": true }));
    let r = run(&mut s, "shape.formatting", json!({ "ids": [id] }));
    let one = &r["shapes"][0];
    assert_eq!((one["bullet"].clone(), one["ind_left"].clone(), one["case"].clone()), (json!(5), json!(0.5), json!("all_caps")));
    assert_eq!((one["direction"].clone(), one["vertical"].clone()), (json!("rtl"), json!(true)));
}

#[test]
fn a_bad_parameter_is_refused_in_words_and_the_drawing_is_left_alone() {
    let (mut s, id) = labelled("one");
    let before = doc(&s).clone();
    let words = |s: &mut Session, params: serde_json::Value| bad_params_msg(&s.execute("text.format", &params).unwrap_err()).to_owned();
    let msg = words(&mut s, json!({ "ids": [id], "bullet": 8 }));
    assert!(msg.contains("`bullet` must be a whole number from 0 to 7"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "bullet": -1 }));
    assert!(msg.contains("`bullet`"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "bullet": "round" }));
    assert!(msg.contains("`bullet`"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "indent": "sideways" }));
    assert!(msg.contains("`indent` must be increase or decrease; not `sideways`"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "case": "title" }));
    assert!(msg.contains("`case` must be normal, all_caps, initial_caps or small_caps; not `title`"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "direction": "up" }));
    assert!(msg.contains("`direction` must be ltr or rtl; not `up`"), "{msg}");
    let msg = words(&mut s, json!({ "ids": [id], "vertical": "yes" }));
    assert!(msg.contains("`vertical` must be true or false"), "{msg}");
    assert_eq!(doc(&s), &before);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_new_parameters_alone_are_enough_to_format() {
    let (mut s, id) = labelled("one");
    for params in [
        json!({ "bullet": 1 }),
        json!({ "indent": "increase" }),
        json!({ "case": "all_caps" }),
        json!({ "direction": "rtl" }),
        json!({ "vertical": true }),
    ] {
        let mut p = params.clone();
        p["ids"] = json!([id]);
        let r = run(&mut s, "text.format", p);
        assert_eq!(r["formatted"], 1, "{params}");
    }
}

#[test]
fn a_paragraph_typed_after_a_bulleted_one_has_the_bullet_too() {
    // The Bullets button, then Enter and the next line ("press Enter to start the next bulleted line"): the new paragraph
    // joins the row of the one before it, so it has its bullet and its indent.
    let (mut s, id) = labelled("one");
    run(&mut s, "text.format", json!({ "ids": [id], "bullet": 3 }));
    run(&mut s, "shape.set_text", json!({ "id": id, "text": "one\ntwo", "start": 0, "end": 3 }));
    run(&mut s, "shape.set_text", json!({ "id": id, "text": "\nthree", "start": 7, "end": 7 }));
    let all = run(&mut s, "shape.formatting", json!({ "ids": [id], "start": 0, "end": 13 }));
    assert_eq!(all["shapes"][0]["bullet"], 3, "{all}");
    assert_eq!(all["shapes"][0]["mixed"], json!([]), "one bullet over all three paragraphs: {all}");
    assert_eq!(all["shapes"][0]["ind_left"], 0.25);
}
