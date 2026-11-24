//! The words: `snap.at` takes the design's subjects and words (review M5), a command takes only the words of `snap` it has (review L8), and a
//! kept side resize that snaps the other axis by size tells its hit (review L6).

use diacraft_doc::Document;
use diacraft_engine::snap::Axis;
use diacraft_engine::snap::guides::add_guide;
use diacraft_engine::{Session, find_command, param_keys};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value as Json, json};

use super::snap_review_fixes::{box_in, near, page_of, session_of};
use crate::run;

/// A page of rectangles with a fixed grid of a quarter inch, each `(id, x0, y0, x1, y1)`.
fn page_with_a_quarter_grid(shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let mut page = PageBuilder::new()
        .cell("PageWidth", "20")
        .cell("PageHeight", "20")
        .cell("XGridDensity", "0")
        .cell("YGridDensity", "0")
        .cell("XGridSpacing", "0.25")
        .cell("YGridSpacing", "0.25");
    for (id, x0, y0, x1, y1) in shapes {
        let (w, h) = (x1 - x0, y1 - y0);
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &((x0 + x1) / 2.0).to_string())
                .cell("PinY", &((y0 + y1) / 2.0).to_string())
                .cell("Width", &w.to_string())
                .cell("Height", &h.to_string())
                .cell("LocPinX", &(w / 2.0).to_string())
                .cell("LocPinY", &(h / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

/// The box of shape `id` as the parameters of `snap.at`.
fn box_params(s: &Session, id: u32) -> serde_json::Map<String, Json> {
    let b = box_in(s, id);
    json!({"page": 1, "left": b.x0, "bottom": b.y0, "right": b.x1, "top": b.y1, "ids": [id], "zoom": 96.0}).as_object().unwrap().clone()
}

fn with(mut base: serde_json::Map<String, Json>, extra: Json) -> Json {
    base.extend(extra.as_object().unwrap().clone());
    Json::Object(base)
}

/// `snap.at` on the box of shape `id` with the words of `extra`.
fn snap_at(s: &mut Session, id: u32, extra: Json) -> Json {
    let p = with(box_params(s, id), extra);
    run(s, "snap.at", p)
}

fn refusal(s: &mut Session, id: &str, params: &Json) -> String {
    s.execute(id, params).unwrap_err().to_string()
}

// ---------------------------------------------------------------------------------------------------------------- M5, `snap.at` and its subjects

#[test]
fn snap_at_with_the_nudge_subject_says_where_each_press_of_the_command_lands() {
    // The fixed grid of a quarter inch, the ruler off, a shape (1, 1) to (2, 2) and an upright guide at x = 2.1: the presses of `shape.move` go to 1.1 (onto the guide), 1.25
    // and 1.5. Before each press `snap.at` with the `nudge` subject gives the same hits and marks, and the box the press lands in.
    let mut doc = page_with_a_quarter_grid(&[(1, 1.0, 1.0, 2.0, 2.0)]);
    add_guide(&mut doc, 1, Axis::X, 2.1, 0).unwrap();
    let mut s = session_of(doc);
    run(&mut s, "snap.set", json!({"grid": true, "ruler": false}));
    for want in [1.1, 1.25, 1.5] {
        let before = box_in(&s, 1);
        let at = snap_at(&mut s, 1, json!({"nudge": {"direction": "right", "tick": 0.125}}));
        let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}));
        assert!(near(box_in(&s, 1).x0, want), "{:?}", box_in(&s, 1));
        assert!(near(at["left"].as_f64().unwrap(), want) && near(at["dx"].as_f64().unwrap(), want - before.x0), "{at}");
        assert_eq!((&at["hits"], &at["marks"]), (&moved["snap"]["hits"], &moved["snap"]["marks"]), "{at} against {moved}");
    }
}

#[test]
fn snap_at_nudges_each_way_as_the_command_does_and_a_press_with_nothing_ahead_is_the_tick() {
    for (direction, dx, dy) in [("right", 1.0, 0.0), ("left", -1.0, 0.0), ("up", 0.0, 1.0), ("down", 0.0, -1.0)] {
        // A grid line a quarter inch off the box on each side: the press goes there, as the command's does.
        let mut s = session_of(page_with_a_quarter_grid(&[(1, 5.0, 5.0, 6.0, 6.0), (2, 9.0, 9.0, 10.0, 10.0)]));
        run(&mut s, "snap.set", json!({"grid": true, "ruler": false, "dynamic_grid": false, "alignment_box": false}));
        let at = snap_at(&mut s, 1, json!({"nudge": {"direction": direction, "tick": 0.125}}));
        let before = box_in(&s, 1);
        run(&mut s, "shape.move", json!({"ids": [1], "dx": dx, "dy": dy, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}));
        let after = box_in(&s, 1);
        assert!(
            near(at["dx"].as_f64().unwrap(), after.x0 - before.x0) && near(at["dy"].as_f64().unwrap(), after.y0 - before.y0),
            "{direction}: {at}"
        );
        assert!(near(at["left"].as_f64().unwrap(), after.x0) && near(at["top"].as_f64().unwrap(), after.y1), "{direction}: {at} {after:?}");
        assert!(at["dx"].as_f64().unwrap().abs() + at["dy"].as_f64().unwrap().abs() > 0.0, "the press goes somewhere: {at}");
    }
    // Nothing ahead (the grid, the dynamic grid and the alignment off): one tick, the one told and, without, the ruler's at the zoom (an eighth of an inch at 96).
    let mut s = session_of(page_of(&[(1, 5.0, 5.0, 6.0, 6.0)]));
    run(&mut s, "snap.set", json!({"dynamic_grid": false, "alignment_box": false}));
    for (tick, want) in [(json!({"tick": 0.5}), 0.5), (json!({}), 0.125)] {
        let at = snap_at(&mut s, 1, json!({"nudge": with_direction("up", tick)}));
        assert!(near(at["dy"].as_f64().unwrap(), want) && near(at["dx"].as_f64().unwrap(), 0.0), "{at}");
        assert!(near(at["bottom"].as_f64().unwrap(), 5.0 + want), "{at}");
    }
    // The Snap switch off: a press is one tick, as the command's is.
    run(&mut s, "snap.set", json!({"snap": false}));
    let at = snap_at(&mut s, 1, json!({"nudge": {"direction": "left", "tick": 0.25}}));
    assert!(near(at["dx"].as_f64().unwrap(), -0.25), "{at}");
}

fn with_direction(direction: &str, mut nudge: Json) -> Json {
    nudge["direction"] = json!(direction);
    nudge
}

#[test]
fn a_nudge_subject_is_refused_in_words_where_it_has_no_box_or_two_directions_or_another_subject() {
    let mut s = session_of(page_of(&[(1, 5.0, 5.0, 6.0, 6.0)]));
    let base = box_params(&s, 1);
    let no_box = json!({"page": 1, "x": 5.0, "y": 5.0, "nudge": {"direction": "right"}});
    assert!(refusal(&mut s, "snap.at", &no_box).contains("nudge"), "a place has no press");
    for bad in [
        json!({"direction": "sideways"}),
        json!({}),
        json!({"direction": "right", "tick": 0.0}),
        json!({"direction": "right", "step": 1.0}),
        json!("right"),
    ] {
        let err = refusal(&mut s, "snap.at", &with(base.clone(), json!({"nudge": bad.clone()})));
        assert!(err.contains("nudge"), "{bad}: {err}");
    }
    let err = refusal(&mut s, "snap.at", &with(base.clone(), json!({"nudge": {"direction": "right"}, "lock": "x"})));
    assert!(err.contains("nudge") && err.contains("lock"), "{err}");
    let err = refusal(&mut s, "snap.at", &with(base, json!({"nudge": {"direction": "right"}, "edges": ["right"]})));
    assert!(err.contains("nudge") && err.contains("edges"), "{err}");
}

#[test]
fn snap_at_with_lock_snaps_along_the_free_axis_as_the_locked_move_does() {
    // The page: a 1 by 1 shape at (2, 2) and an upright guide at x = 4.03. The move (1.0, 0.3) locked to x lands on the guide with its y unchanged; `snap.at` is given the
    // box the move keeps to (the y it has) and says the same.
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.03, 0).unwrap();
    let mut s = session_of(doc);
    let at = run(&mut s, "snap.at", json!({"page": 1, "left": 3.0, "bottom": 2.0, "right": 4.0, "top": 3.0, "ids": [1], "lock": "x", "zoom": 96.0}));
    let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.3, "snap": {"lock": "x"}}));
    assert!(near(at["right"].as_f64().unwrap(), 4.03) && near(at["bottom"].as_f64().unwrap(), 2.0), "{at}");
    assert_eq!((at["dy"].as_f64(), &at["hits"]), (Some(0.0), &moved["snap"]["hits"]), "{at} against {moved}");
    // A place locked to y does not take the guide on x, and one locked to x does and stays on its y.
    let place = |lock: &str| json!({"page": 1, "x": 4.0, "y": 2.2, "lock": lock});
    let free = run(&mut s, "snap.at", place("y"));
    assert!(free["dx"].as_f64() == Some(0.0) && free["hits"] == json!([]), "{free}");
    let locked = run(&mut s, "snap.at", place("x"));
    assert!(near(locked["x"].as_f64().unwrap(), 4.03) && locked["dy"].as_f64() == Some(0.0), "{locked}");
    // Refused in words: a lock that is no axis, and a lock on a resize.
    assert!(refusal(&mut s, "snap.at", &place("diagonal")).contains("lock"));
    let resize = with(box_params(&s, 1), json!({"edges": ["right"], "lock": "x"}));
    let err = refusal(&mut s, "snap.at", &resize);
    assert!(err.contains("lock") && err.contains("edges"), "{err}");
}

#[test]
fn snap_at_with_edges_and_keep_says_the_box_the_kept_resize_lands_in() {
    // A 2 by 1 shape at (1, 1) and a shape of height 1.5 elsewhere: the right side dragged to 3.97 with the proportions kept comes to a height of 1.5 and so to width 3,
    // (1, 0.75) to (4, 2.25). The command and `snap.at` give the same box, hits and marks.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0), (2, 10.0, 10.0, 15.0, 11.5)]));
    let at = run(
        &mut s,
        "snap.at",
        json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 3.97, "top": 2.0, "ids": [1], "edges": ["right"], "keep": "proportions", "zoom": 96.0}),
    );
    let moved = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.97, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    let b = box_in(&s, 1);
    assert!(near(b.x1, 4.0) && near(b.y0, 0.75) && near(b.y1, 2.25), "{b:?}");
    let landed = [at["left"].as_f64().unwrap(), at["bottom"].as_f64().unwrap(), at["right"].as_f64().unwrap(), at["top"].as_f64().unwrap()];
    assert!(near(landed[0], b.x0) && near(landed[1], b.y0) && near(landed[2], b.x1) && near(landed[3], b.y1), "{at} against {b:?}");
    assert_eq!((&at["landed"], &at["hits"], &at["marks"]), (&moved["snap"]["landed"], &moved["snap"]["hits"], &moved["snap"]["marks"]));
    // Without `keep` the same words snap the side alone, and `landed` is not told.
    let free = run(
        &mut s,
        "snap.at",
        json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 3.97, "top": 2.0, "ids": [1], "edges": ["right"], "zoom": 96.0}),
    );
    assert!(free["landed"].is_null(), "{free}");
    // Refused in words: keep without edges, a keep that is no proportions, keep without the one shape whose proportions they are, and edges that are no sides.
    let b = box_params(&s, 1);
    let err = refusal(&mut s, "snap.at", &with(b.clone(), json!({"keep": "proportions"})));
    assert!(err.contains("keep") && err.contains("edges"), "{err}");
    assert!(refusal(&mut s, "snap.at", &with(b.clone(), json!({"edges": ["right"], "keep": "centre"}))).contains("keep"));
    let two = with(b.clone(), json!({"edges": ["right"], "keep": "proportions", "ids": [1, 2]}));
    assert!(refusal(&mut s, "snap.at", &two).contains("keep"));
    for bad in [json!(["middle"]), json!([]), json!("right"), json!([1])] {
        let err = refusal(&mut s, "snap.at", &with(b.clone(), json!({"edges": bad.clone()})));
        assert!(err.contains("edges"), "{bad}: {err}");
    }
    let err = refusal(&mut s, "snap.at", &json!({"page": 1, "x": 1.0, "y": 1.0, "edges": ["right"]}));
    assert!(err.contains("edges"), "a place has no sides to drag: {err}");
}

#[test]
fn snap_at_with_edges_and_a_view_matches_the_sizes_of_the_shapes_in_it() {
    // B (id 2) from x 6 to 7.5 and A (id 1) 2 inches wide: B's right side dragged to 7.97 stops at 8 when A is in the view or the view is left out, and stays when A is not.
    let mut s = session_of(page_of(&[(1, 3.4, 2.0, 5.4, 3.5), (2, 6.0, 2.0, 7.5, 3.0)]));
    let right = |s: &mut Session, view: Option<Json>| {
        let mut p = json!({"page": 1, "left": 6.0, "bottom": 2.0, "right": 7.97, "top": 3.0, "ids": [2], "edges": ["right"], "zoom": 96.0});
        if let Some(v) = view {
            p["view"] = v;
        }
        run(s, "snap.at", p)
    };
    let seen = right(&mut s, Some(json!([0.0, 0.0, 10.0, 10.0])));
    assert!(near(seen["right"].as_f64().unwrap(), 8.0), "{seen}");
    assert!(seen["marks"].as_array().unwrap().iter().any(|m| m["mark"] == json!("size") && m["hit"] == json!(true)), "{seen}");
    let unseen = right(&mut s, Some(json!([5.5, 0.0, 10.0, 10.0])));
    assert!(near(unseen["right"].as_f64().unwrap(), 7.97) && unseen["hits"] == json!([]), "{unseen}");
    assert!(near(right(&mut s, None)["right"].as_f64().unwrap(), 8.0));
    // A view that is not four numbers is refused in words, with every subject.
    for view in [json!([0.0, 0.0, 1.0]), json!("all"), json!([0.0, 0.0, 1.0, null]), json!([0.0, 0.0, 1.0, 2.0, "x"])] {
        let err = refusal(&mut s, "snap.at", &json!({"page": 1, "x": 1.0, "y": 1.0, "view": view.clone()}));
        assert!(err.contains("view"), "{view}: {err}");
    }
}

#[test]
fn snap_at_with_the_rotate_subject_says_where_the_turn_of_the_command_comes_to() {
    // A shape at 33 degrees and the one that turns: the pointer 1.5625 inch from the pin at 35 degrees comes to 33. The words and the result are the command's.
    let doc = {
        let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
        for (id, x, angle) in [(1u32, 3.0, 0.0), (2, 12.0, 33.0_f64.to_radians())] {
            page = page.shape(
                ShapeBuilder::new(id)
                    .cell("PinX", &x.to_string())
                    .cell("PinY", "5")
                    .cell("Width", "1")
                    .cell("Height", "1")
                    .cell("LocPinX", "0.5")
                    .cell("LocPinY", "0.5")
                    .cell("Angle", &angle.to_string()),
            );
        }
        DocBuilder::new().page(1, page).build()
    };
    let mut s = session_of(doc);
    let at = run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "rotate": {"angle": 35.0, "radius": 1.5625}, "zoom": 96.0}));
    let turned = run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 35.0, "snap": {"zoom": 96.0, "radius": 1.5625}}));
    assert_eq!(at["angle"], json!(33.0), "{at}");
    for key in ["angle", "hits", "marks", "truncated"] {
        assert_eq!(at[key], turned["snap"][key], "{key}: {at} against {turned}");
    }
    // Nothing was written by the query, and it is refused in words with a box, with no radius or one shape fewer or more, and with an angle that is not a number.
    // The most degrees a turn may be given as is a million, either way.
    for angle in [1.0e6, -1.0e6] {
        run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "rotate": {"angle": angle, "radius": 1.0}}));
    }
    let still = run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "rotate": {"angle": 35.0, "radius": 1.5625}}));
    assert_eq!(still["angle"], json!(33.0), "{still}");
    for bad in [
        json!({"page": 1, "ids": [1], "rotate": {"angle": 35.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"radius": 1.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"angle": 35.0, "radius": -1.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"angle": "x", "radius": 1.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"angle": 2.0e6, "radius": 1.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"angle": -2.0e6, "radius": 1.0}}),
        json!({"page": 1, "ids": [1], "rotate": {"angle": 35.0, "radius": 1.0, "step": 5}}),
        json!({"page": 1, "ids": [1, 2], "rotate": {"angle": 35.0, "radius": 1.0}}),
        json!({"page": 1, "rotate": {"angle": 35.0, "radius": 1.0}}),
        json!({"page": 1, "ids": [1], "x": 1.0, "y": 1.0, "rotate": {"angle": 35.0, "radius": 1.0}}),
    ] {
        let err = refusal(&mut s, "snap.at", &bad);
        assert!(err.contains("rotate"), "{bad}: {err}");
    }
}

#[test]
fn m5_the_params_doc_of_snap_at_names_every_word_it_takes() {
    let doc = find_command("snap.at").unwrap().params;
    let keys = param_keys(doc).unwrap();
    for key in ["page", "x", "y", "left", "bottom", "right", "top", "ids", "connect", "zoom", "edges", "keep", "lock", "nudge", "view", "rotate"] {
        assert!(keys.contains(&key), "{key} is named in {keys:?}");
    }
    assert!(!doc.contains("true or {zoom"), "{doc}");
}

// ---------------------------------------------------------------------------------------------------------------- L8, a command takes the words it has

/// Each command with the parameters that make it valid, a word of `snap` it does not take, and the words it does.
#[test]
fn l8_a_command_takes_the_words_of_snap_it_has_and_refuses_the_others_in_words() {
    let lock = ("lock", json!("y"));
    let keep = ("keep", json!("proportions"));
    let nudge = ("nudge", json!({}));
    let radius = ("radius", json!(1.0));
    let view = ("view", json!([0.0, 0.0, 10.0, 10.0]));
    let page = || page_of(&[(1, 1.0, 1.0, 3.0, 2.0), (2, 6.0, 6.0, 7.0, 7.0)]);
    // (command, parameters, words it refuses, words it takes)
    type Words<'a> = Vec<(&'a str, Json)>;
    let table: Vec<(&str, Json, Words, Words)> = vec![
        (
            "shape.drop",
            json!({"master": "rectangle", "page": 1, "x": 12.0, "y": 12.0}),
            vec![lock.clone(), keep.clone(), nudge.clone(), radius.clone()],
            vec![view.clone()],
        ),
        (
            "shape.move",
            json!({"ids": [1], "dx": 1.0, "dy": 0.0}),
            vec![keep.clone(), radius.clone()],
            vec![lock.clone(), nudge.clone(), view.clone()],
        ),
        (
            "shape.resize",
            json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.0, "height": 1.0}),
            vec![lock.clone(), nudge.clone(), radius.clone()],
            vec![keep.clone(), view.clone()],
        ),
        ("shape.rotate", json!({"ids": [1], "angle": 10.0}), vec![lock.clone(), keep.clone(), nudge.clone()], vec![radius.clone(), view.clone()]),
        ("shape.place", json!({"ids": [1], "x": 5.0}), vec![lock.clone(), keep.clone(), nudge.clone(), radius.clone()], vec![view.clone()]),
        ("connector.add", json!({"from": {"x": 12.0, "y": 12.0}, "to": {"x": 14.0, "y": 14.0}}), vec![lock, keep, nudge, radius], vec![view]),
    ];
    for (command, base, refused, taken) in table {
        for (word, value) in refused {
            let mut s = session_of(page());
            let before = s.document().cloned();
            let mut p = base.clone();
            p["snap"] = json!({ word: value });
            let err = refusal(&mut s, command, &p);
            assert!(err.contains(&format!("snap.{word}")) && err.contains(command), "{command} {word}: {err}");
            assert_eq!(s.document().cloned(), before, "{command} {word}: a refused call changes nothing");
        }
        for (word, value) in taken {
            let mut s = session_of(page());
            let mut p = base.clone();
            // A turn needs the pointer's distance, and a drop a master, which `base` has.
            p["snap"] = if command == "shape.rotate" { json!({ word: value, "radius": 1.0 }) } else { json!({ word: value }) };
            if let Err(e) = s.execute(command, &p) {
                panic!("{command} takes {word}: {e}");
            }
        }
    }
}

#[test]
fn l8_a_drop_that_is_sent_lock_does_not_snap_by_it_and_a_resize_that_is_sent_nudge_is_refused() {
    // The review's cases: `shape.drop {snap: {lock: "y"}}` moved the shape by dx -0.03 (to the guide) and not by 0, and the resize took `nudge` and `radius` in silence.
    let mut doc = page_of(&[(1, 1.0, 1.0, 3.0, 2.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.97, 0).unwrap();
    let mut s = session_of(doc);
    let err = refusal(&mut s, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 5.0, "snap": {"lock": "y"}}));
    assert!(err.contains("snap.lock") && err.contains("shape.drop") && err.contains("zoom"), "the words it takes are named: {err}");
    let err =
        refusal(&mut s, "shape.resize", &json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.0, "height": 1.0, "snap": {"radius": 1.0, "nudge": {}}}));
    assert!(err.contains("snap.") && err.contains("shape.resize"), "{err}");
}

#[test]
fn l8_the_params_doc_of_shape_rotate_says_what_it_takes_and_the_other_commands_name_their_words() {
    let doc = find_command("shape.rotate").unwrap().params;
    assert!(!doc.contains("true or {zoom, radius}"), "snap: true is refused by the command, so the doc does not offer it: {doc}");
    assert!(doc.contains("snap: {zoom, radius"), "{doc}");
    for (command, words) in [
        ("shape.drop", vec!["zoom", "view"]),
        ("shape.move", vec!["zoom", "lock", "nudge", "view"]),
        ("shape.resize", vec!["zoom", "keep", "view"]),
        ("shape.place", vec!["zoom", "view"]),
        ("connector.add", vec!["zoom", "view"]),
        ("connector.reconnect", vec!["zoom", "view"]),
    ] {
        let doc = find_command(command).unwrap().params;
        for word in words {
            assert!(doc.contains(word), "{command}'s params doc names `{word}`: {doc}");
        }
    }
    // `snap: true` on a turn is refused in words, as the doc says.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0)]));
    let err = refusal(&mut s, "shape.rotate", &json!({"ids": [1], "angle": 10.0, "snap": true}));
    assert!(err.contains("radius"), "{err}");
}

#[test]
fn l8_a_kept_resize_of_a_shape_with_no_width_to_keep_is_refused_in_words_and_changes_nothing() {
    // The review's case: a shape 1e-12 inch wide, resized with `keep`, was refused as "`y` must be within 1000000": its proportions are a ratio of 1e12, and the box
    // they ask for is 2e12 inches high. The refusal names the proportions.
    for width in [1.0e-12, 1.0e-9, 1.0e-7] {
        let mut s = session_of(page_of(&[(1, 5.0, 5.0, 5.0 + width, 6.0)]));
        let before = s.document().cloned();
        let err = refusal(
            &mut s,
            "shape.resize",
            &json!({"id": 1, "x": 5.0, "y": 5.0, "width": 2.0, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
        );
        assert!(err.contains("snap.keep") && err.contains("proportions"), "{width}: {err}");
        assert!(!err.contains("must be a number within"), "{width}: {err}");
        assert_eq!(s.document().cloned(), before, "{width}");
    }
    // A shape whose proportions can be kept is resized as before.
    let mut s = session_of(page_of(&[(1, 5.0, 5.0, 5.01, 6.0)]));
    run(&mut s, "shape.resize", json!({"id": 1, "x": 5.0, "y": 5.0, "width": 0.02, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}));
}

// ---------------------------------------------------------------------------------------------------------------- L6, a kept side resize snaps the other axis by size

#[test]
fn a_kept_side_resize_that_snaps_the_other_axis_by_size_tells_its_hit() {
    // The review's case: (1, 1) to (3, 2) kept in proportion (2 to 1), its right side dragged to 3.97, and another shape 1.5 high. The box grows to height 1.5 about its
    // middle, (1, 0.75) to (4, 2.25): a size match on y that no side of y was named for, so `hits` was empty.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0), (2, 10.0, 10.0, 15.0, 11.5)]));
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.97, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(r["snap"]["landed"] == json!([1.0, 0.75, 4.0, 2.25]), "{r}");
    let hits: Vec<&Json> = r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["target"] == json!("size")).collect();
    assert_eq!(hits.len(), 1, "one size hit: {r}");
    let h = hits[0];
    assert_eq!((h["axis"].clone(), h["anchor"].clone(), h["shape"].clone()), (json!("y"), json!("middle"), json!(2)), "{h}");
    assert!(near(h["gap"].as_f64().unwrap(), 1.5) && near(h["line"].as_f64().unwrap(), 1.5), "the length matched, and where the middle is: {h}");
    assert!(r["snap"]["marks"].as_array().unwrap().iter().any(|m| m["mark"] == json!("size") && m["hit"] == json!(true)), "{r}");
    // The same on the other axis: the top side dragged, the width grows about its middle onto another shape's width.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0), (2, 10.0, 10.0, 13.0, 11.0)]));
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.0, "height": 1.48, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    let hit = r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("{r}")).clone();
    assert_eq!((hit["axis"].clone(), hit["anchor"].clone()), (json!("x"), json!("middle")), "{r}");
}

#[test]
fn l8_a_kept_resize_whose_box_goes_past_the_page_by_its_place_and_not_by_its_size_is_refused_too() {
    // A shape a hundred thousandth of an inch wide at y = 950,000: its proportions give a box 200,000 inches high about its middle, which is within the limit as a size and
    // goes to 1,050,000 by its top.
    let mut s = session_of(page_of(&[(1, 5.0, 950_000.0, 5.00001, 950_001.0)]));
    let before = s.document().cloned();
    let err = refusal(
        &mut s,
        "shape.resize",
        &json!({"id": 1, "x": 5.0, "y": 950_000.0, "width": 2.0, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(err.contains("snap.keep") && !err.contains("must be a number within"), "{err}");
    assert_eq!(s.document().cloned(), before);
}

#[test]
fn snap_at_with_edges_names_each_of_the_four_sides_and_the_box_it_comes_to() {
    // The resizes of `a_size_match_says_which_side_of_which_axis_it_moved`, one side at a time: B (id 2) from (6, 2) to (7.5, 3) against A, 2 wide and 1.5 high.
    let mut s = session_of(page_of(&[(1, 3.4, 0.5, 5.4, 2.0), (2, 6.0, 2.0, 7.5, 3.0)]));
    for (side, to, landed, axis, anchor) in [
        ("right", (6.0, 2.0, 7.97, 3.0), (6.0, 2.0, 8.0, 3.0), "x", "high"),
        ("left", (5.53, 2.0, 7.5, 3.0), (5.5, 2.0, 7.5, 3.0), "x", "low"),
        ("top", (6.0, 2.0, 7.5, 3.47), (6.0, 2.0, 7.5, 3.5), "y", "high"),
        ("bottom", (6.0, 1.53, 7.5, 3.0), (6.0, 1.5, 7.5, 3.0), "y", "low"),
    ] {
        let r = run(
            &mut s,
            "snap.at",
            json!({"page": 1, "left": to.0, "bottom": to.1, "right": to.2, "top": to.3, "ids": [2], "edges": [side], "view": [0.0, 0.0, 10.0, 10.0], "zoom": 96.0}),
        );
        let got = [r["left"].as_f64().unwrap(), r["bottom"].as_f64().unwrap(), r["right"].as_f64().unwrap(), r["top"].as_f64().unwrap()];
        assert!(near(got[0], landed.0) && near(got[1], landed.1) && near(got[2], landed.2) && near(got[3], landed.3), "{side}: {r}");
        let hit = r["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("{side}: {r}")).clone();
        assert_eq!((hit["axis"].clone(), hit["anchor"].clone()), (json!(axis), json!(anchor)), "{side}: {r}");
    }
    // Two sides at once are both named.
    let r = run(
        &mut s,
        "snap.at",
        json!({"page": 1, "left": 6.0, "bottom": 2.0, "right": 7.97, "top": 3.47, "ids": [2], "edges": ["top", "right", "right"], "view": [0.0, 0.0, 10.0, 10.0], "zoom": 96.0}),
    );
    assert!(near(r["right"].as_f64().unwrap(), 8.0) && near(r["top"].as_f64().unwrap(), 3.5), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- the survivors of cargo mutants

/// A page `k` drawing units to a paper inch (a 20 by 20 inch page, `PageScale` 1 and `DrawingScale` `k`) with shape 1, a 1 inch square with its pin at (3, 3) inches.
fn scaled_page(k: f64) -> Document {
    let page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").cell("PageScale", "1").cell("DrawingScale", &k.to_string()).shape(
        ShapeBuilder::new(1)
            .cell("PinX", &(3.0 * k).to_string())
            .cell("PinY", &(3.0 * k).to_string())
            .cell("Width", &k.to_string())
            .cell("Height", &k.to_string())
            .cell("LocPinX", &(k / 2.0).to_string())
            .cell("LocPinY", &(k / 2.0).to_string())
            .cell("Angle", "0"),
    );
    DocBuilder::new().page(1, page).build()
}

#[test]
fn shape_place_pulls_the_box_to_where_the_pins_go_on_a_page_at_every_scale() {
    // A 1 inch square at (3, 3) with an upright guide at x = 7.55 inches: the pin placed at x = 7.03 puts the box's right side 0.02 inch short of it, and the snap pulls it on,
    // whatever the drawing units to an inch (the pin's place is told in drawing units, and the box and the guide in paper inches).
    for k in [1.0, 2.0, 0.5] {
        let mut s = session_of(scaled_page(k));
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 7.55}));
        let r = run(&mut s, "shape.place", json!({"ids": [1], "x": 7.03, "snap": {"zoom": 96.0}}));
        let b = box_in(&s, 1);
        assert!(near(b.x1, 7.55) && near(b.x1 - b.x0, 1.0), "{k} units to an inch: {b:?} {r}");
        assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{k}: {r}");
        // The same along y, with a level guide at y = 7.55, and with both axes given at once (the pin of a shape that is on its guide already stays where it is).
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "horizontal", "at": 7.55}));
        let r = run(&mut s, "shape.place", json!({"ids": [1], "y": 7.03, "snap": {"zoom": 96.0}}));
        let b = box_in(&s, 1);
        assert!(near(b.y1, 7.55) && near(b.x1, 7.55), "{k} units to an inch: {b:?} {r}");
        let r = run(&mut s, "shape.place", json!({"ids": [1], "x": 7.03, "y": 7.03, "snap": {"zoom": 96.0}}));
        assert!(near(box_in(&s, 1).x1, 7.55) && near(box_in(&s, 1).y1, 7.55), "{k}: {r}");
        // The low sides as well: guides at 6.55 take the left and the bottom (the box is 6.53 to 7.53).
        let mut s = session_of(scaled_page(k));
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 6.55}));
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "horizontal", "at": 6.55}));
        let r = run(&mut s, "shape.place", json!({"ids": [1], "x": 7.03, "y": 7.03, "snap": {"zoom": 96.0}}));
        assert!(near(box_in(&s, 1).x0, 6.55) && near(box_in(&s, 1).y0, 6.55), "{k}: {r}");
    }
}

#[test]
fn l8_a_kept_resize_is_refused_by_the_page_and_not_by_the_inch() {
    // On a page of 2 drawing units to an inch the limit of a million units is 500,000 inches: a shape a hundred thousandth of an inch wide resized to 6 inches wide with its
    // proportions comes to 600,000 inches high, within a million as inches and past it as units.
    let mut s = session_of(scaled_page(2.0));
    run(&mut s, "shape.resize", json!({"id": 1, "x": 2.5, "y": 2.5, "width": 0.00001, "height": 1.0}));
    let err = refusal(
        &mut s,
        "shape.resize",
        &json!({"id": 1, "x": 2.5, "y": 2.5, "width": 6.0, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(err.contains("snap.keep") && !err.contains("must be a number within"), "{err}");
}

#[test]
fn l8_a_kept_resize_is_refused_by_the_far_side_of_its_box_in_x_as_in_y() {
    // The mirror of the case in y: a shape 1 inch high and a hundred thousandth wide... on its side. A tall thin shape at x = 950,000: its proportions give a box 200,000
    // inches wide about its middle, which is within the limit as a size and goes to 1,050,000 by its right side.
    let mut s = session_of(page_of(&[(1, 950_000.0, 5.0, 950_001.0, 5.00001)]));
    let before = s.document().cloned();
    let err = refusal(
        &mut s,
        "shape.resize",
        &json!({"id": 1, "x": 950_000.0, "y": 5.0, "width": 1.0, "height": 2.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(err.contains("snap.keep") && !err.contains("must be a number within"), "{err}");
    assert_eq!(s.document().cloned(), before);
}

#[test]
fn l8_a_kept_resize_of_a_large_shape_far_from_the_origin_is_made() {
    // The refusal is for a box past the page, not for numbers whose products are large: 2,000 inches from the origin and as wide, the proportions kept.
    let mut s = session_of(page_of(&[(1, 2000.0, 2000.0, 4000.0, 3000.0)]));
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 2000.0, "y": 2000.0, "width": 2500.0, "height": 1000.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    let b = box_in(&s, 1);
    assert!(near(b.x1 - b.x0, 2500.0) && near(b.y1 - b.y0, 1250.0), "{b:?} {r}");
}

#[test]
fn l8_the_refusal_of_a_kept_box_is_the_commands_for_keep_with_a_snap_and_nobody_elses() {
    // A box past the page asked for with no `keep`, or with `keep` while the Snap switch is off, is refused by the edit as it was, in the edit's words.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 2.0, 2.0)]));
    let big = |snap: Json| json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.0e6, "height": 1.0, "snap": snap});
    let err = refusal(&mut s, "shape.resize", &big(json!({"zoom": 96.0})));
    assert!(err.contains("must be a number within") && !err.contains("snap.keep"), "{err}");
    run(&mut s, "snap.set", json!({"snap": false}));
    let err = refusal(&mut s, "shape.resize", &big(json!({"zoom": 96.0, "keep": "proportions"})));
    assert!(err.contains("must be a number within") && !err.contains("snap.keep"), "{err}");
}

#[test]
fn a_turn_is_refused_beside_each_word_that_is_not_its_own_one_at_a_time() {
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 2.0, 2.0)]));
    let rotate = json!({"angle": 35.0, "radius": 1.0});
    for (word, value) in [
        ("x", json!(1.0)),
        ("y", json!(1.0)),
        ("left", json!(1.0)),
        ("bottom", json!(1.0)),
        ("right", json!(2.0)),
        ("top", json!(2.0)),
        ("edges", json!(["left"])),
        ("nudge", json!({"direction": "left"})),
        ("lock", json!("x")),
        ("keep", json!("proportions")),
    ] {
        let err = refusal(&mut s, "snap.at", &json!({"page": 1, "ids": [1], "rotate": rotate.clone(), word: value}));
        assert!(err.contains("rotate"), "{word}: {err}");
    }
    // The words it does take: the shape, the zoom and a view that it has no use for.
    run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "rotate": rotate, "zoom": 48.0, "view": [0.0, 0.0, 5.0, 5.0]}));
}

#[test]
fn snap_at_keeps_the_proportions_of_the_shape_whatever_they_are_and_refuses_a_shape_with_no_size() {
    // A 3 by 2 shape (a ratio of 1.5) with its right side dragged to 4.97 and a guide at 5: it lands (1, 0.667) to (5, 3.333), as the command's resize does.
    let mut doc = page_of(&[(1, 1.0, 1.0, 4.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 5.0, 0).unwrap();
    let mut s = session_of(doc);
    let at = run(
        &mut s,
        "snap.at",
        json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 4.97, "top": 3.0, "ids": [1], "edges": ["right"], "keep": "proportions", "zoom": 96.0}),
    );
    let moved = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.97, "height": 2.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    let b = box_in(&s, 1);
    assert!(near(b.x1, 5.0) && ((b.x1 - b.x0) / (b.y1 - b.y0) - 1.5).abs() < 1e-9, "{b:?}");
    assert_eq!(at["landed"], moved["snap"]["landed"], "{at} against {moved}");
    // A shape with no width, or no height, has no proportions to keep.
    for to in [(1.0, 1.0, 1.0, 3.0), (1.0, 1.0, 4.0, 1.0)] {
        let mut s = session_of(page_of(&[(1, to.0, to.1, to.2, to.3)]));
        let err = refusal(
            &mut s,
            "snap.at",
            &json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 5.0, "top": 4.0, "ids": [1], "edges": ["right"], "keep": "proportions"}),
        );
        assert!(err.contains("keep") && err.contains("no width or no height"), "{to:?}: {err}");
    }
}

#[test]
fn snap_at_carries_the_own_points_of_up_to_512_shapes_and_no_more() {
    use diacraft_engine::draw::{connection_points, drop_shape};
    use diacraft_engine::snap::bounds_of;
    use diacraft_engine::stencil::find_master;
    // E (the stock rectangle at (5, 5)) and a target T with a connection point at the middle of its left side. The box of the shapes is moved so that E's right middle is 0.02 inch
    // from T's point, and the connection points of the shapes in `ids` are pulled with it: for 512 shapes, and not for 513, which the commands do not snap either.
    let mut doc = page_of(&[]);
    let spec = find_master("rectangle").unwrap();
    let e = drop_shape(&mut doc, 1, spec, 5.0, 5.0).unwrap();
    let t = drop_shape(&mut doc, 1, spec, 11.0, 9.0).unwrap();
    let own = connection_points(&doc, 1, e.id)
        .into_iter()
        .find(|p| (p.x - (5.0 + e.width / 2.0)).abs() < 1e-9 && (p.y - 5.0).abs() < 1e-9)
        .expect("E's right middle");
    let t_left = connection_points(&doc, 1, t.id)
        .into_iter()
        .find(|p| (p.x - (11.0 - t.width / 2.0)).abs() < 1e-9 && (p.y - 9.0).abs() < 1e-9)
        .expect("T's left middle");
    let mut others = Vec::new();
    for i in 0..512u32 {
        others.push(drop_shape(&mut doc, 1, spec, 40.0 + f64::from(i % 32) * 2.0, 40.0 + f64::from(i / 32) * 2.0).unwrap().id);
    }
    let (by_x, by_y) = (t_left.x + 0.02 - own.x, t_left.y + 0.01 - own.y);
    for (n, hits) in [(512usize, true), (513, false)] {
        let mut ids = vec![e.id];
        ids.extend(others.iter().take(n - 1));
        let at = bounds_of(&doc, 1, &ids).unwrap().rect;
        let mut s = session_of(doc.clone());
        run(
            &mut s,
            "snap.set",
            json!({"snap_connection_points": true, "dynamic_grid": false, "alignment_box": false, "snap_geometry": false, "snap_vertices": false, "snap_handles": false, "intersections": false, "guides": false}),
        );
        let r = run(
            &mut s,
            "snap.at",
            json!({"page": 1, "left": at.x0 + by_x, "bottom": at.y0 + by_y, "right": at.x1 + by_x, "top": at.y1 + by_y, "ids": ids, "zoom": 96.0}),
        );
        let pulled = r["hits"].as_array().unwrap().iter().any(|h| h["target"] == json!("connection_point") && h["shape"] == json!(t.id));
        assert_eq!(pulled, hits, "{n} shapes: {r}");
    }
}
