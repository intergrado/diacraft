//! What a route goes round, and the Behavior commands (`shape.behavior`, `shape.set_behavior`). The rules are the design's section 1.2,
//! and the owner's rulings of
//! 2026-11-25: **Q2** (gluing a connector writes `ObjType` 1 on a shape that reads none of 1, 2 and 4) and **Q3** (DiaCraft's own masters of the basic, flowchart, organization
//! chart and Gane-Sarson shapes hold `ObjType` 1, and containers, swimlanes, callouts, decorative shapes, arrows and text hold none).
//!
//! Every new cell is **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`), from a new drawing (the model writers) and from a file edited and saved in its
//! own format (the splice), and a connector no edit reached keeps its rows, byte for byte.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::stencil;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::connect_style::{cell, z_pair};
use crate::edit::{doc, drop_shape, id_of};
use crate::native_common::Dir;
use crate::odg_build::part_of;
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const FORMATS: [&str; 3] = ["diacraft", "vsdx", "odg"];

/// What `ObjType` a shape of page 1 reads, through its master and styles: `None` where nothing sets it.
fn objtype(d: &Document, id: u32) -> Option<f64> {
    cell(d, id, "ObjType")
}

/// What a cell holds directly in the shape's own sheet.
fn own(d: &Document, id: u32, name: &str) -> Option<f64> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id)?)?;
    shape.sheet.cell(name)?.value.as_ref()?.number()
}

/// Writes a number into a cell of a shape, as a person's edit of the ShapeSheet would.
fn put(s: &mut Session, id: u32, name: &str, value: f64) {
    s.edit("test cell", |d| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).expect("the shape"));
        d.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(&value.to_string())), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

fn text_box(s: &mut Session, x: f64, y: f64) -> u32 {
    id_of(&run(s, "text.box", json!({"page": 1, "x": x, "y": y, "width": 1.5, "height": 1.0, "text": "Box"})))
}

fn behavior(s: &mut Session, id: u32) -> Json {
    let answer = run(s, "shape.behavior", json!({ "ids": [id] }));
    answer["shapes"][0].clone()
}

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    bad_params_msg(&e).to_owned()
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// The model and what it says, as the session is: a call that changes nothing still has its place in the journal.
fn quiet(s: &Session) -> crate::Snapshot {
    crate::Snapshot { journal: Vec::new(), ..snapshot(s) }
}

// ---- Q3: the masters ----------------------------------------------------------------------------------------------------------------

/// What `ObjType` an instance of each two-dimensional master of the built-in stencil `stencil` reads, by master `id`.
fn dropped(stencil: &str) -> Vec<(&'static str, Option<f64>)> {
    let spec = stencil::builtin_stencil(stencil).unwrap_or_else(|| panic!("no stencil {stencil}"));
    let mut out = Vec::new();
    for master in spec.masters() {
        if master.is_connector() || master.is_part() {
            continue;
        }
        let mut s = blank_session();
        let id = id_of(&run(&mut s, "shape.drop", json!({"master": master.id, "stencil": stencil, "page": 1, "x": 4.0, "y": 4.0})));
        out.push((master.id, objtype(doc(&s), id)));
    }
    out
}

fn all_hold(stencil: &str, want: Option<f64>, at_least: usize) {
    let read = dropped(stencil);
    assert!(read.len() >= at_least, "{stencil}: the loop looked at the masters: {read:?}");
    for (master, got) in read {
        assert_eq!(got, want, "{master} of {stencil}");
    }
}

#[test]
fn the_flowchart_masters_hold_objtype_1() {
    all_hold("flowchart", Some(1.0), 15);
}

#[test]
fn the_basic_org_chart_and_gane_sarson_masters_hold_objtype_1() {
    all_hold("basic", Some(1.0), 10);
    all_hold("orgchart", Some(1.0), 4);
    // Gane-Sarson's data flow is a connector, and holds none: the three shapes people join hold 1.
    all_hold("gane-sarson", Some(1.0), 3);
}

#[test]
fn the_decorative_and_arrow_masters_hold_none() {
    all_hold("decorative", None, 5);
    all_hold("arrows", None, 5);
}

#[test]
fn the_swimlane_container_and_callout_masters_do_not() {
    // A swimlane structure, a container, a callout and a text box (which has no master) are no shape people join in the sense of Q3: no `ObjType`.
    let (mut s, _) = crate::swimlane::made("horizontal");
    let page = doc(&s).page(1).unwrap();
    let lanes: Vec<u32> = page.shapes.iter().map(|shape| shape.id).collect();
    assert!(!lanes.is_empty());
    for id in lanes {
        assert_eq!(objtype(doc(&s), id), None, "swimlane part {id}");
    }
    let container = id_of(&run(&mut s, "container.add", json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 3.0})));
    let callout = id_of(&run(&mut s, "callout.insert", json!({"x": 4.0, "y": 5.0, "style": "speech_box", "text": "Hello"})));
    let text = text_box(&mut s, 6.0, 2.0);
    for (what, id) in [("container", container), ("callout", callout), ("text box", text)] {
        assert_eq!(objtype(doc(&s), id), None, "{what}");
    }
}

#[test]
fn an_instance_reads_the_one_of_its_master_and_holds_no_cell_of_its_own() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    assert_eq!((objtype(doc(&s), a), own(doc(&s), a, "ObjType")), (Some(1.0), None));
    assert_eq!(behavior(&mut s, a)["placement"], "route_around");
}

// ---- Q2: gluing ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn gluing_writes_objtype_1_on_a_shape_that_reads_0() {
    // Given a box with no `ObjType` anywhere, and a connector glued to it, expect a local `ObjType` 1, undone with the glue.
    let mut s = blank_session();
    let (a, b) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0));
    assert_eq!((objtype(doc(&s), a), objtype(doc(&s), b)), (None, None));
    let steps = s.history().unwrap().undo_len();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let d = doc(&s);
    for id in [a, b] {
        assert_eq!((objtype(d, id), own(d, id, "ObjType")), (Some(1.0), Some(1.0)), "shape {id}: local, and 1");
    }
    assert_eq!(own(d, c, "ObjType"), None, "the connector is no shape that is placed");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "in the same undo step as the glue");
    run(&mut s, "edit.undo", json!({}));
    for id in [a, b] {
        assert_eq!((objtype(doc(&s), id), own(doc(&s), id, "ObjType")), (None, None), "undone with the glue");
    }
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(objtype(doc(&s), a), Some(1.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_new_end_glued_by_reconnect_makes_its_shape_placeable_too() {
    let mut s = blank_session();
    let (a, b, c) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0), text_box(&mut s, 6.0, 1.0));
    let k = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    assert_eq!(objtype(doc(&s), c), None);
    run(&mut s, "connector.reconnect", json!({"id": k, "end": "end", "target": {"id": c}}));
    assert_eq!(objtype(doc(&s), c), Some(1.0), "the end moved onto c");
    assert_eq!(own(doc(&s), c, "ObjType"), Some(1.0));
    assert_eq!(objtype(doc(&s), b), Some(1.0), "b was glued once, and keeps what that wrote");
}

#[test]
fn a_free_end_writes_nothing() {
    let mut s = blank_session();
    let a = text_box(&mut s, 3.0, 4.0);
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"x": 9.0, "y": 6.0}}));
    assert_eq!(objtype(doc(&s), a), Some(1.0), "the glued end's shape");
    let b = text_box(&mut s, 9.0, 1.0);
    assert_eq!(objtype(doc(&s), b), None, "a shape no connector is glued to");
}

#[test]
fn gluing_leaves_a_4_and_a_1_alone() {
    let mut s = blank_session();
    let (a, b, c) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0), text_box(&mut s, 6.0, 1.0));
    run(&mut s, "shape.set_behavior", json!({"ids": [a], "placement": "none"}));
    run(&mut s, "shape.set_behavior", json!({"ids": [b], "placement": "route_around"}));
    let (before_a, before_b) = (doc(&s).clone(), doc(&s).clone());
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    run(&mut s, "connector.add", json!({"from": {"id": c}, "to": {"id": b}}));
    let d = doc(&s);
    assert_eq!(own(d, a, "ObjType"), Some(4.0), "neither stays neither: it outranks the glue");
    assert_eq!(own(d, b, "ObjType"), Some(1.0));
    // Not rewritten: the shape's sheet is the one it was.
    for (id, before) in [(a, &before_a), (b, &before_b)] {
        let (was, now) = (before.find_shape(Owner::Page(1), id).unwrap(), d.find_shape(Owner::Page(1), id).unwrap());
        assert!(Arc::ptr_eq(&before.shape(&was).unwrap().sheet, &d.shape(&now).unwrap().sheet), "shape {id} was not written");
    }
}

#[test]
fn gluing_keeps_the_flags_outside_1_2_and_4_a_group_flag_among_them() {
    let mut s = blank_session();
    let (a, b) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0));
    put(&mut s, a, "ObjType", 8.0);
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    assert_eq!(own(doc(&s), a, "ObjType"), Some(9.0), "values add up: 8 and the 1 the glue writes");
}

#[test]
fn a_glue_writes_no_route_of_another_connector() {
    // An edit that glues a shape another connector is glued to writes that shape's cell and the new connector, and nothing else: no other connector is routed.
    let (mut s, a, _, c) = z_pair();
    run(&mut s, "shape.set_behavior", json!({"ids": [a], "placement": "auto"}));
    let before = doc(&s).clone();
    let third = text_box(&mut s, 3.0, 1.0);
    let k = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 2}, "to": {"id": third}})));
    let d = doc(&s);
    assert_eq!(own(d, a, "ObjType"), Some(1.0), "a read 0 (auto) and a connector was glued to it already: this glue writes the cell");
    let (was, now) = (before.find_shape(Owner::Page(1), c).unwrap(), d.find_shape(Owner::Page(1), c).unwrap());
    assert!(Arc::ptr_eq(&before.shape(&was).unwrap().sheet, &d.shape(&now).unwrap().sheet), "connector {c} has the rows it had: not routed again");
    assert_ne!(k, c);
}

// ---- shape.behavior and shape.set_behavior ---------------------------------------------------------------------------------------------

#[test]
fn set_behavior_writes_objtype_and_the_pass_cells_in_one_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let steps = s.history().unwrap().undo_len();
    let params = json!({"ids": [a], "placement": "none", "route_through_x": true, "route_through_y": true, "do_not_move": true, "allow_on_top": true, "plow": "never"});
    let r = run(&mut s, "shape.set_behavior", params.clone());
    assert_eq!(r, json!({"page": 1, "changed": 1}));
    let d = doc(&s);
    for (name, want) in [
        ("ObjType", 4.0),
        ("ShapePermeableX", 1.0),
        ("ShapePermeableY", 1.0),
        ("ShapeFixedCode", 1.0),
        ("ShapePermeablePlace", 1.0),
        ("ShapePlowCode", 1.0),
    ] {
        assert_eq!(own(d, a, name), Some(want), "{name}: local, written once");
    }
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    // The same call again: it writes nothing, and is no step (`changed` 0).
    let before = quiet(&s);
    let r = run(&mut s, "shape.set_behavior", params);
    assert_eq!(r, json!({"page": 1, "changed": 0}));
    let after = quiet(&s);
    assert_eq!((before.undo_steps, before.revision), (after.undo_steps, after.revision), "no undo step and no revision");
    assert_eq!(before.document, after.document);
    // One undo takes every cell back.
    run(&mut s, "edit.undo", json!({}));
    for name in ["ObjType", "ShapePermeableX", "ShapePermeableY", "ShapeFixedCode", "ShapePermeablePlace", "ShapePlowCode"] {
        assert_eq!(own(doc(&s), a, name), None, "{name} undone");
    }
}

#[test]
fn set_behavior_writes_only_what_it_is_given_and_what_differs() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // Placement `route_around` is what the master gives already, so no local cell is written; the pass flag is.
    let r = run(&mut s, "shape.set_behavior", json!({"ids": [a], "placement": "route_around", "route_through_x": true}));
    assert_eq!(r["changed"], 1);
    let d = doc(&s);
    assert_eq!(own(d, a, "ObjType"), None, "the master's 1 is what the shape reads: nothing to write");
    assert_eq!(own(d, a, "ShapePermeableX"), Some(1.0));
    for name in ["ShapePermeableY", "ShapeFixedCode", "ShapePermeablePlace", "ShapePlowCode"] {
        assert_eq!(own(d, a, name), None, "{name} was not given");
    }
    // Leaving a flag false that reads 0 writes nothing; turning off what the master holds writes a local 0.
    let r = run(&mut s, "shape.set_behavior", json!({"ids": [a], "route_through_y": false}));
    assert_eq!(r["changed"], 0);
    run(&mut s, "shape.set_behavior", json!({"ids": [a], "placement": "auto"}));
    assert_eq!(own(doc(&s), a, "ObjType"), Some(0.0), "Let the application decide, against the master's 1: a local 0");
}

#[test]
fn set_behavior_keeps_the_flags_outside_1_2_and_4() {
    let mut s = blank_session();
    let a = text_box(&mut s, 3.0, 4.0);
    // A group flag (8) and a flag no list names (16) are kept; the placement flags are what the choice says.
    put(&mut s, a, "ObjType", 8.0 + 16.0 + 4.0);
    for (word, want) in [("route_around", 25.0), ("auto", 24.0), ("none", 28.0)] {
        run(&mut s, "shape.set_behavior", json!({"ids": [a], "placement": word}));
        assert_eq!(own(doc(&s), a, "ObjType"), Some(want), "{word}");
    }
    put(&mut s, a, "ShapeFixedCode", 128.0 + 2.0);
    run(&mut s, "shape.set_behavior", json!({"ids": [a], "do_not_move": true}));
    assert_eq!(own(doc(&s), a, "ShapeFixedCode"), Some(131.0), "flag 1 is turned on, and 128 and 2 stay");
    run(&mut s, "shape.set_behavior", json!({"ids": [a], "do_not_move": false}));
    assert_eq!(own(doc(&s), a, "ShapeFixedCode"), Some(130.0), "and off");
}

#[test]
fn set_behavior_takes_the_words_of_each_list_and_refuses_the_others_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    for (word, want) in [("as_page", 0.0), ("never", 1.0), ("always", 2.0)] {
        run(&mut s, "shape.set_behavior", json!({"ids": [a], "plow": word}));
        assert_eq!(behavior(&mut s, a)["plow"], word);
        assert_eq!(own(doc(&s), a, "ShapePlowCode").unwrap_or(0.0), want);
    }
    let before = quiet(&s);
    for (params, words) in [
        (json!({"ids": [a], "placement": "front"}), "`placement` must be auto, route_around or none, not `front`"),
        (json!({"ids": [a], "plow": "some"}), "`plow` must be as_page, never or always, not `some`"),
        (json!({"ids": [a], "route_through_x": 1}), "`route_through_x` must be true or false"),
        (json!({"ids": [a]}), "give at least one of placement, route_through_x, route_through_y, do_not_move, allow_on_top and plow"),
        (json!({"ids": [], "placement": "auto"}), "`ids` must hold between 1 and"),
        (json!({"placement": "auto"}), "`ids` must be an array of shape ids"),
        (json!({"ids": [999], "placement": "auto"}), "page 1 has no shape 999"),
    ] {
        let e = refused(&mut s, "shape.set_behavior", params.clone());
        assert!(e.contains(words), "{params}: {e}");
    }
    assert_eq!(quiet(&s), before, "refused calls change nothing");
}

#[test]
fn set_behavior_refuses_a_connector_in_words() {
    let (mut s, a, _, c) = z_pair();
    let before = quiet(&s);
    let e = refused(&mut s, "shape.set_behavior", json!({"ids": [a, c], "placement": "none"}));
    assert_eq!(
        e,
        format!("shape {c} is a connector or a line: it has no placement behavior (connector.reroute and connector.style set how it is routed)")
    );
    assert_eq!(quiet(&s), before, "shape {a} was not written either: all or nothing");
}

#[test]
fn set_behavior_refuses_a_guide_in_words() {
    let (mut s, ..) = z_pair();
    let guide = id_of(&run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 5.0})));
    let before = quiet(&s);
    let e = refused(&mut s, "shape.set_behavior", json!({"ids": [guide], "route_through_x": true}));
    assert_eq!(e, format!("shape {guide} is a guide: it is an aid and no shape a route goes round"));
    assert_eq!(quiet(&s), before);
}

#[test]
fn set_behavior_takes_a_member_of_a_group_as_it_takes_any_shape() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 3.0, 4.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let r = run(&mut s, "shape.set_behavior", json!({"ids": [a], "route_through_y": true}));
    assert_eq!(r["changed"], 1);
    assert_eq!(own(doc(&s), a, "ShapePermeableY"), Some(1.0), "the member's own cell");
    assert_eq!(own(doc(&s), b, "ShapePermeableY"), None);
    // A group made by DiaCraft holds no `ObjType` of its own, so no route goes round it until one is glued to it or it is told to be placed.
    assert_eq!(behavior(&mut s, group)["obstacle"], false);
    run(&mut s, "shape.set_behavior", json!({"ids": [group], "placement": "route_around"}));
    assert_eq!(behavior(&mut s, group)["obstacle"], true);
}

#[test]
fn set_behavior_routes_nothing() {
    // A setting, not a command: the route of a connector that crosses the shape is as it was (the next change of the shape asks).
    let (mut s, a, b, c) = z_pair();
    let m = drop_shape(&mut s, "rectangle", 6.0, 5.0);
    let before = doc(&s).clone();
    for params in [
        json!({"ids": [m], "placement": "none"}),
        json!({"ids": [a, b], "route_through_x": true, "route_through_y": true}),
        json!({"ids": [m], "placement": "route_around"}),
    ] {
        run(&mut s, "shape.set_behavior", params);
    }
    let (was, now) = (before.find_shape(Owner::Page(1), c).unwrap(), doc(&s).find_shape(Owner::Page(1), c).unwrap());
    assert!(Arc::ptr_eq(&before.shape(&was).unwrap().sheet, &doc(&s).shape(&now).unwrap().sheet), "the connector's rows are the ones it had");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn shape_behavior_says_what_a_shape_holds_and_why_it_is_no_obstacle() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let t = text_box(&mut s, 7.0, 4.0);
    let r = run(&mut s, "shape.behavior", json!({"ids": [a, t]}));
    assert_eq!(r["page"], 1);
    let listed: BTreeMap<u64, Json> = r["shapes"].as_array().unwrap().iter().map(|x| (x["id"].as_u64().unwrap(), x.clone())).collect();
    assert_eq!(listed.len(), 2);
    let (ra, rt) = (&listed[&u64::from(a)], &listed[&u64::from(t)]);
    assert_eq!((ra["placement"].as_str(), ra["obstacle"].as_bool(), ra["why_not"].is_null()), (Some("route_around"), Some(true), true), "{ra}");
    assert_eq!(
        (rt["placement"].as_str(), rt["obstacle"].as_bool(), rt["why_not"].as_str()),
        (Some("auto"), Some(false), Some("its placement behavior is Let the application decide and no connector is glued to it")),
        "{rt}"
    );
    for key in ["route_through_x", "route_through_y", "do_not_move", "allow_on_top"] {
        assert_eq!(ra[key], false, "{key}");
    }
    assert_eq!((ra["plow"].as_str(), ra["obj_type"].as_f64()), (Some("as_page"), Some(1.0)));
    // Glued, the text box is one; set to neither, it is none whatever is glued to it.
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": t}}));
    assert_eq!(behavior(&mut s, t)["obstacle"], true);
    run(&mut s, "shape.set_behavior", json!({"ids": [t], "placement": "none"}));
    let none = behavior(&mut s, t);
    assert_eq!(
        (none["placement"].as_str(), none["obstacle"].as_bool(), none["why_not"].as_str()),
        (Some("none"), Some(false), Some("its placement behavior is Do not lay out and route around"))
    );
}

#[test]
fn shape_behavior_is_a_query_that_changes_nothing_and_refuses_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let before = quiet(&s);
    run(&mut s, "shape.behavior", json!({"ids": [a]}));
    assert_eq!(quiet(&s), before, "a query: no step, no journal entry");
    assert_eq!(refused(&mut s, "shape.behavior", json!({"ids": [999]})), "page 1 has no shape 999");
    assert!(refused(&mut s, "shape.behavior", json!({})).contains("`ids` must be an array of shape ids"));
    assert!(refused(&mut s, "shape.behavior", json!({"ids": [a], "page": 7})).contains("no page 7"));
    let ids: Vec<u32> = (0..5_000).collect();
    assert!(refused(&mut s, "shape.behavior", json!({ "ids": ids })).contains("`ids` must hold between 1 and 4096"));
}

#[test]
fn a_shape_on_a_hidden_layer_is_no_obstacle_and_says_so() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    run(&mut s, "layer.add", json!({"name": "Plan"}));
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Notes"]}));
    assert_eq!(behavior(&mut s, a)["obstacle"], true, "a visible layer");
    run(&mut s, "layer.set", json!({"layer": "Notes", "visible": false}));
    let hidden = behavior(&mut s, a);
    assert_eq!((hidden["obstacle"].as_bool(), hidden["why_not"].as_str()), (Some(false), Some("every layer it is on is hidden")));
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Plan"], "mode": "add"}));
    assert_eq!(behavior(&mut s, a)["obstacle"], true, "one of its layers is shown");
}

#[test]
fn a_one_d_shape_and_a_guide_are_no_obstacle_and_say_so() {
    let (mut s, _, _, c) = z_pair();
    let guide = id_of(&run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 5.0})));
    let line = behavior(&mut s, c);
    assert_eq!(
        (line["obstacle"].as_bool(), line["why_not"].as_str()),
        (Some(false), Some("it is a connector or a line, which a route does not go round"))
    );
    let guide = behavior(&mut s, guide);
    assert_eq!((guide["obstacle"].as_bool(), guide["why_not"].as_str()), (Some(false), Some("it is a guide, an aid and no shape")));
}

// ---- the three formats ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_cells_set_behavior_writes_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in FORMATS {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
        let t = text_box(&mut s, 7.0, 4.0);
        run(
            &mut s,
            "shape.set_behavior",
            json!({"ids": [a], "placement": "none", "route_through_x": true, "route_through_y": true, "do_not_move": true, "allow_on_top": true, "plow": "always"}),
        );
        run(&mut s, "shape.set_behavior", json!({"ids": [t], "placement": "route_around", "route_through_y": true}));
        let dir = Dir::new();
        let path = dir.path(&format!("behavior.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let (first, second) = (behavior(&mut back, a), behavior(&mut back, t));
        assert_eq!(first["placement"], "none", "{ext}");
        for key in ["route_through_x", "route_through_y", "do_not_move", "allow_on_top"] {
            assert_eq!(first[key], true, "{ext}: {key}");
        }
        assert_eq!(first["plow"], "always", "{ext}");
        assert_eq!(second["placement"], "route_around", "{ext}");
        assert_eq!((second["route_through_x"].as_bool(), second["route_through_y"].as_bool()), (Some(false), Some(true)), "{ext}");
    }
}

#[test]
fn an_object_type_that_a_glue_wrote_and_one_a_master_holds_come_back_in_each_format() {
    for ext in FORMATS {
        let mut s = blank_session();
        let (a, b) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0));
        let r = drop_shape(&mut s, "rectangle", 6.0, 1.0);
        run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
        let dir = Dir::new();
        let path = dir.path(&format!("glued.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let back = reopened(&path);
        let d = doc(&back);
        assert_eq!((objtype(d, a), objtype(d, b)), (Some(1.0), Some(1.0)), "{ext}: what the glue wrote");
        assert_eq!(objtype(d, r), Some(1.0), "{ext}: what the master holds");
    }
}

/// The element of each shape in the saved page of a VSDX file, by `ID`.
fn vsdx_shapes(bytes: &[u8]) -> BTreeMap<u32, String> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let name = pkg.parts().find(|p| p.ends_with("/pages/page1.xml")).expect("the page part").to_owned();
    let xml = String::from_utf8(pkg.part(&name).unwrap().to_vec()).unwrap();
    let mut out = BTreeMap::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<Shape ID=\"") {
        let tail = &rest[start..];
        let end = tail.find("</Shape>").map_or(tail.len(), |e| e + "</Shape>".len());
        let id: u32 = tail["<Shape ID=\"".len()..].split('"').next().unwrap().parse().unwrap();
        out.insert(id, tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

/// The markup of each `draw:connector` of the saved `content.xml` of an `.odg`, by its `draw:id` or `draw:name`.
fn odg_connectors(bytes: &[u8]) -> Vec<String> {
    let xml = part_of(bytes, "content.xml");
    let mut out = Vec::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<draw:connector") {
        let tail = &rest[start..];
        let end = tail.find("</draw:connector>").map_or(tail.len(), |e| e + "</draw:connector>".len());
        out.push(tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

#[test]
fn a_drawing_read_from_a_file_and_edited_by_a_glue_keeps_the_cell_and_the_connector_nobody_touched_byte_for_byte() {
    for ext in FORMATS {
        // A file of the format, from a drawing with one connector between two boxes and two more boxes no connector is glued to.
        let mut first = blank_session();
        let (a, b) = (text_box(&mut first, 3.0, 4.0), text_box(&mut first, 9.0, 6.0));
        let (c, d) = (text_box(&mut first, 3.0, 1.0), text_box(&mut first, 9.0, 1.0));
        let k1 = id_of(&run(&mut first, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let kept_before = std::fs::read(&path).unwrap();
        // Read from the file, a second connector is glued to the two boxes that read 0, and the drawing is saved in place.
        let mut s = reopened(&path);
        assert_eq!((objtype(doc(&s), c), objtype(doc(&s), d)), (None, None), "{ext}: no connector is glued to them");
        let before = doc(&s).clone();
        let k2 = id_of(&run(&mut s, "connector.add", json!({"from": {"id": c, "point": 3}, "to": {"id": d, "point": 1}})));
        run(&mut s, "file.save", json!({}));
        let kept_after = std::fs::read(&path).unwrap();
        let back = reopened(&path);
        let after = doc(&back);
        assert_eq!((objtype(after, c), objtype(after, d)), (Some(1.0), Some(1.0)), "{ext}: the glue's cell is in the file");
        assert_eq!((objtype(after, a), objtype(after, b)), (Some(1.0), Some(1.0)), "{ext}: and what the first glue wrote is kept");
        // The connector nobody touched: its rows in the file are as they were.
        match ext {
            "vsdx" => {
                let (was, now) = (vsdx_shapes(&kept_before), vsdx_shapes(&kept_after));
                assert_eq!(was[&k1], now[&k1], "vsdx: connector {k1} is byte for byte as it was");
                assert!(now.contains_key(&k2));
            }
            "odg" => {
                let (was, now) = (odg_connectors(&kept_before), odg_connectors(&kept_after));
                assert_eq!(was.len(), 1);
                assert!(now.contains(&was[0]), "odg: the first connector's markup is byte for byte as it was:\n{}\nnot in\n{now:?}", was[0]);
            }
            _ => {
                let at = |d: &Document, id: u32| d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().clone();
                assert_eq!(at(&before, k1), at(after, k1), "diacraft: connector {k1} is the shape it was");
            }
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_relayout_that_glues_the_connectors_again_makes_their_shapes_placeable() {
    // Holds for every way a connector is glued, among them Re-Layout Page, which glues the ends again.
    let mut s = blank_session();
    let (a, b) = (text_box(&mut s, 3.0, 4.0), text_box(&mut s, 9.0, 6.0));
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    for id in [a, b] {
        run(&mut s, "shape.set_behavior", json!({"ids": [id], "placement": "auto"}));
        put(&mut s, id, "ObjType", 0.0);
    }
    assert_eq!((objtype(doc(&s), a), objtype(doc(&s), b)), (Some(0.0), Some(0.0)), "both read 0 and a connector is glued to each");
    run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "left_to_right"}));
    assert_eq!((objtype(doc(&s), a), objtype(doc(&s), b)), (Some(1.0), Some(1.0)), "glued again: placeable");
    assert_eq!(check_session(&s), Ok(()));
}

/// The markup of the shape of an `.odg`'s `content.xml` whose `draw:name` is `name`: from its start tag to its end tag.
fn odg_shape(content: &str, name: &str) -> String {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name}"));
    let start = content[..at].rfind("<draw:custom-shape").expect("a start tag");
    let end = content[start..].find("</draw:custom-shape>").map_or(content.len(), |e| start + e + "</draw:custom-shape>".len());
    content[start..end].to_owned()
}

/// A drawing another program made: four shapes on two automatic styles that several of them share, and no data of DiaCraft's. A connector glued to some of them writes
/// `ObjType` 1 on each into a shape that shares its style, which is written in the file's own bytes by the splice, and **the shapes no connector reached keep their
/// markup byte for byte**; the save then saves again as no edit.
#[test]
fn a_glue_to_the_shapes_of_an_odg_another_program_made_is_saved_in_the_file_and_the_other_shapes_are_as_they_were() {
    let dir = crate::odg_build::Dir::new();
    let mut s = crate::odg_build::open_drawing(&dir, &crate::odg_file::four());
    let (a, b, c) = {
        let d = doc(&s);
        (crate::odg_build::id_of(d, 0, "A"), crate::odg_build::id_of(d, 0, "B"), crate::odg_build::id_of(d, 0, "C"))
    };
    let read = |d: &Document, id: u32| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(0), id).unwrap());
        diacraft_doc::Resolver::new(d).resolve(&sheet, &CellAddr::direct("ObjType")).and_then(|found| found.value.cell?.value.as_ref()?.number())
    };
    assert_eq!((read(doc(&s), a), read(doc(&s), b), read(doc(&s), c)), (None, None, None), "another program's shapes hold no ObjType");
    let before = crate::odg_build::content_of(&dir.read("a.odg"));
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}, "page": 0}));
    run(&mut s, "connector.add", json!({"from": {"id": c}, "to": {"id": b}, "page": 0}));
    let saved = crate::odg_file::save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let bytes = dir.read("a.odg");
    let after = crate::odg_build::content_of(&bytes);
    // The shape no connector was glued to is the markup it was, and the three that were hold the cell in the attribute of their style.
    assert_eq!(odg_shape(&before, "D"), odg_shape(&after, "D"), "shape D was not reached");
    assert!(odg_shape(&before, "D").contains("draw:style-name=\"gr1\""));
    for name in ["A", "B", "C"] {
        assert_ne!(odg_shape(&before, name), odg_shape(&after, name), "shape {name} holds its new cell");
    }
    let (back, _) = crate::odg_build::read_back(&bytes);
    let at = |name: &str| crate::odg_build::id_of(&back, 0, name);
    assert_eq!([read(&back, at("A")), read(&back, at("B")), read(&back, at("C")), read(&back, at("D"))], [Some(1.0), Some(1.0), Some(1.0), None]);
    crate::odg_file::assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    crate::odg_file::assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}
