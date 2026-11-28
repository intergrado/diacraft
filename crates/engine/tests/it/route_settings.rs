//! The page's routing settings and the directional styles (`page.routing`, `page.set_routing`, `connector.path`, `connector.style` with a direction).
//!
//! The rules are the styles and their directions, what a connector's own 0 follows (the spacing cells are written as a
//! pair in one step, in drawing units), (a new page style asks a new
//! route of the connectors that follow the page and are set to reroute freely). The window, the command line and MCP reach the same commands.

use diacraft_doc::{CellAddr, CellEdit, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::draw::route::Dir as Way;
use diacraft_engine::draw::{Flow, RouteStyle, connector_style, lean_of, route_style_of};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use serde_json::{Value as Json, json};

use crate::connect_style::{cell, page_cell, z_pair};
use crate::edit::{doc, drop_shape, geometry, id_of};
use crate::native_common::{Dir, PINNED};
use crate::odg_build::{Drawing, rect};
use crate::{Snapshot, bad_params_msg, blank_session, check_session, run, snapshot};

/// A snapshot as the document and its history have it: a call that changes nothing still has its place in the journal.
fn quiet(s: &Session) -> Snapshot {
    Snapshot { journal: Vec::new(), ..snapshot(s) }
}

/// Writes the cell `name` of page 1 straight into the document, as an edit of the ShapeSheet would.
fn put_page(s: &mut Session, name: &str, text: &str) {
    s.edit("test", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct(name), CellEdit::Value(Value::from_text(text)), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

/// Writes the cell `name` of shape `id` on page 1.
fn put_shape(s: &mut Session, id: u32, name: &str, text: &str) {
    s.edit("test", |d| {
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        d.write_cell(&SheetRef::Shape(at), &CellAddr::direct(name), CellEdit::Value(Value::from_text(text)), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn num(v: &Json) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number: {v}"))
}

/// The refusal of a call, in words: the call is refused as bad parameters and changes nothing.
fn refused(s: &mut Session, id: &str, params: Json) -> String {
    let before = quiet(s);
    let e = s.execute(id, &params).unwrap_err();
    assert_eq!(quiet(s), before, "{id} {params}: a refused call changes nothing");
    bad_params_msg(&e).to_owned()
}

/// The table: the code, the style and the direction a page's `RouteStyle` says.
const TABLE: [(u32, &str, Option<&str>); 23] = [
    (0, "right_angle", None),
    (1, "right_angle", None),
    (2, "straight", None),
    (3, "org_chart", Some("top_to_bottom")),
    (4, "org_chart", Some("left_to_right")),
    (5, "flowchart", Some("top_to_bottom")),
    (6, "flowchart", Some("left_to_right")),
    (7, "tree", Some("top_to_bottom")),
    (8, "tree", Some("left_to_right")),
    (9, "network", None),
    (10, "org_chart", Some("bottom_to_top")),
    (11, "org_chart", Some("right_to_left")),
    (12, "flowchart", Some("bottom_to_top")),
    (13, "flowchart", Some("right_to_left")),
    (14, "tree", Some("bottom_to_top")),
    (15, "tree", Some("right_to_left")),
    (16, "centre_to_centre", None),
    (17, "simple", Some("top_to_bottom")),
    (18, "simple", Some("left_to_right")),
    (19, "simple", Some("bottom_to_top")),
    (20, "simple", Some("right_to_left")),
    (21, "simple", Some("level_first")),
    (22, "simple", Some("upright_first")),
];

#[test]
fn a_page_style_code_is_its_style_and_direction_by_the_table_of_route_r1() {
    let mut s = blank_session();
    for (code, style, direction) in TABLE {
        put_page(&mut s, "RouteStyle", &code.to_string());
        let q = run(&mut s, "page.routing", json!({}));
        assert_eq!((q["style"].as_str(), q["direction"].as_str()), (Some(style), direction), "code {code}: {q}");
        // A 0 is the default, which is right angle, and its code is 1 ("1, and 0 as the default").
        assert_eq!(q["style_code"], json!(code.max(1)), "code {code}");
    }
    // A number outside the list is kept as it is and read as right angle, as the connectors' own 0 and 1 are.
    for odd in ["23", "99", "255", "-3", "2.5", "1000000"] {
        put_page(&mut s, "RouteStyle", odd);
        let q = run(&mut s, "page.routing", json!({}));
        assert_eq!((q["style"].as_str(), q["direction"].as_str(), q["style_code"].as_u64()), (Some("right_angle"), None, Some(1)), "{odd}: {q}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn page_set_routing_writes_the_code_of_each_style_and_direction() {
    let mut s = blank_session();
    for (code, style, direction) in TABLE.into_iter().filter(|(code, ..)| *code != 0) {
        let mut params = json!({ "style": style });
        if let Some(direction) = direction {
            params["direction"] = json!(direction);
        }
        run(&mut s, "page.set_routing", params);
        assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(f64::from(code)), "{style} {direction:?}");
    }
    // A style that takes a direction and is given none flows from the top to the bottom, and a direction alone turns the style the page has.
    run(&mut s, "page.set_routing", json!({"style": "org_chart"}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(3.0));
    run(&mut s, "page.set_routing", json!({"direction": "right_to_left"}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(11.0), "the page's own style, turned");
    run(&mut s, "page.set_routing", json!({"style": "simple", "direction": "upright_first"}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(22.0));
    // The words are read in any case, with a hyphen or a space for the underscore, and the spellings of the dialog are taken.
    run(&mut s, "page.set_routing", json!({"style": "Organization Chart", "direction": "Left-To-Right"}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(4.0));
    run(&mut s, "page.set_routing", json!({"style": "center to center"}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(16.0));
}

#[test]
fn a_connectors_own_zero_follows_the_page_and_any_other_style_is_its_own() {
    // `ShapeRouteStyle` 0 on a page with `RouteStyle` 6 is flowchart, left to right; 16 stays 16; 0 on a page with 0 is right angle.
    let (mut s, _, _, c) = z_pair();
    put_page(&mut s, "RouteStyle", "6");
    let got = connector_style(doc(&s), 1, c).unwrap();
    assert_eq!(got.route, RouteStyle::Flowchart(Flow::LeftToRight));
    assert!(!got.own, "the connector holds none of its own");
    let said = run(&mut s, "connector.path", json!({"id": c}));
    assert_eq!(said["route_style"], json!({"style": "flowchart", "direction": "left_to_right", "code": 6, "own": false}), "{said}");
    put_shape(&mut s, c, "ShapeRouteStyle", "16");
    let got = connector_style(doc(&s), 1, c).unwrap();
    assert_eq!((got.route, got.own), (RouteStyle::CentreToCentre, true));
    let said = run(&mut s, "connector.path", json!({"id": c}));
    assert_eq!(said["route_style"], json!({"style": "centre_to_centre", "direction": null, "code": 16, "own": true}), "{said}");
    put_shape(&mut s, c, "ShapeRouteStyle", "0");
    put_page(&mut s, "RouteStyle", "0");
    assert_eq!(connector_style(doc(&s), 1, c).unwrap().route, RouteStyle::RightAngle);
    // A shape that is no connector, a shape that is not there and a page that is not there have no style.
    let a = drop_shape(&mut s, "rectangle", 1.0, 1.0);
    assert!(connector_style(doc(&s), 1, a).is_err() && connector_style(doc(&s), 1, 99).is_err() && connector_style(doc(&s), 9, c).is_err());
}

#[test]
fn the_pages_style_and_appearance_are_what_a_connector_with_zero_draws() {
    // A page with `RouteStyle` 2 and a connector with `ShapeRouteStyle` 0 is straight; with 1, right angle.
    let (mut s, _, _, c) = z_pair();
    put_page(&mut s, "RouteStyle", "2");
    assert_eq!(run(&mut s, "connector.path", json!({"id": c}))["style"], "straight");
    put_shape(&mut s, c, "ShapeRouteStyle", "1");
    assert_eq!(run(&mut s, "connector.path", json!({"id": c}))["style"], "right_angle");
    // `ConLineRouteExt` 0 on a page with `LineRouteExt` 2 is curved: reading 0 as straight fails. A curved route has no corners to give.
    put_shape(&mut s, c, "ShapeRouteStyle", "0");
    put_page(&mut s, "RouteStyle", "0");
    put_page(&mut s, "LineRouteExt", "2");
    let said = run(&mut s, "connector.path", json!({"id": c}));
    assert_eq!(said["style"], "curved");
    // (The rows still say lines until the connector is routed: the style is what it draws at its next route.)
    put_shape(&mut s, c, "ConLineRouteExt", "1");
    assert_eq!(run(&mut s, "connector.path", json!({"id": c}))["style"], "right_angle");
}

#[test]
fn set_routing_writes_each_cell_in_one_step_and_a_second_call_writes_nothing() {
    let mut s = blank_session();
    let before = quiet(&s);
    let all = json!({
        "style": "flowchart", "direction": "left_to_right", "separate": "all", "overlap": "related", "appearance": "curved", "plow": true, "split": true,
        "between_shapes": {"x": 0.5, "y": 0.6}, "shape_size": {"x": 1.0, "y": 1.1},
        "connector_to_connector": {"x": 0.2, "y": 0.25}, "connector_to_shape": {"x": 0.3, "y": 0.35}
    });
    run(&mut s, "page.set_routing", all.clone());
    let after = quiet(&s);
    assert_eq!(after.undo_steps, before.undo_steps + 1, "one undo step");
    assert_eq!(after.next_undo.as_deref(), Some("Page routing"));
    let d = doc(&s);
    let want = [
        ("RouteStyle", 6.0),
        ("LineAdjustFrom", 1.0),
        ("LineAdjustTo", 3.0),
        ("LineRouteExt", 2.0),
        ("PlowCode", 1.0),
        ("PageShapeSplit", 1.0),
        ("AvenueSizeX", 0.5),
        ("AvenueSizeY", 0.6),
        ("BlockSizeX", 1.0),
        ("BlockSizeY", 1.1),
        ("LineToLineX", 0.2),
        ("LineToLineY", 0.25),
        ("LineToNodeX", 0.3),
        ("LineToNodeY", 0.35),
    ];
    for (name, v) in want {
        assert_eq!(page_cell(d, name), Some(v), "{name}");
        // Each is a cell of the page itself, written locally.
        let own = d.page(1).unwrap().sheet.cells.contains_key(&diacraft_doc::CellName::new(name));
        assert!(own, "{name} is the page's own cell");
    }
    // The same call again is no change, no step and no journal entry beyond the call's own.
    let r = run(&mut s, "page.set_routing", all);
    assert_eq!(quiet(&s).undo_steps, after.undo_steps, "a call that writes nothing is no undo step");
    assert_eq!(r["routed"], 0);
    // One undo takes every cell back.
    run(&mut s, "edit.undo", json!({}));
    for (name, _) in want {
        assert_eq!(page_cell(doc(&s), name), None, "{name} is back to what it was");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_call_that_names_some_cells_writes_those_and_no_others() {
    let mut s = blank_session();
    run(&mut s, "page.set_routing", json!({"separate": "none"}));
    let d = doc(&s);
    assert_eq!(page_cell(d, "LineAdjustFrom"), Some(2.0));
    for name in
        ["RouteStyle", "LineAdjustTo", "LineRouteExt", "PlowCode", "PageShapeSplit", "AvenueSizeX", "BlockSizeX", "LineToLineX", "LineToNodeX"]
    {
        assert_eq!(page_cell(d, name), None, "{name} is not written");
    }
    // One of a pair is written alone, and the other is left as it is.
    run(&mut s, "page.set_routing", json!({"connector_to_shape": {"x": 0.4}}));
    assert_eq!((page_cell(doc(&s), "LineToNodeX"), page_cell(doc(&s), "LineToNodeY")), (Some(0.4), None));
    run(&mut s, "page.set_routing", json!({"overlap": "all", "separate": "by_style"}));
    assert_eq!((page_cell(doc(&s), "LineAdjustTo"), page_cell(doc(&s), "LineAdjustFrom")), (Some(1.0), Some(3.0)));
    run(&mut s, "page.set_routing", json!({"separate": "unrelated", "overlap": "by_style"}));
    assert_eq!((page_cell(doc(&s), "LineAdjustTo"), page_cell(doc(&s), "LineAdjustFrom")), (Some(0.0), Some(0.0)));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), None);
}

#[test]
fn the_spacing_pairs_are_written_in_drawing_units_from_paper_inches() {
    // A page at a drawing scale of 10 holds a length of 1.1 as 0.11 inch on paper; a command's inches are paper's, so 0.2 on paper is 2 in the cell.
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"paper": 1, "paper_unit": "in", "drawing": 10, "drawing_unit": "in"}));
    assert_eq!(run(&mut s, "page.setup", json!({}))["drawing"]["units_per_inch"], 10.0);
    let before = quiet(&s);
    run(&mut s, "page.set_routing", json!({"connector_to_connector": {"x": 0.2, "y": 0.3}, "between_shapes": {"x": 0.5, "y": 0.75}}));
    let d = doc(&s);
    assert_eq!((page_cell(d, "LineToLineX"), page_cell(d, "LineToLineY")), (Some(2.0), Some(3.0)), "drawing units: ten to a paper inch");
    assert_eq!((page_cell(d, "AvenueSizeX"), page_cell(d, "AvenueSizeY")), (Some(5.0), Some(7.5)));
    assert_eq!(quiet(&s).undo_steps, before.undo_steps + 1, "both pairs are one step: writing one of them fails");
    // And it reads back as paper inches.
    let q = run(&mut s, "page.routing", json!({}));
    assert_eq!(q["connector_to_connector"], json!({"x": 0.2, "y": 0.3}), "{q}");
    assert_eq!(q["between_shapes"], json!({"x": 0.5, "y": 0.75}), "{q}");
    // A cell of 1.1 on this page is 0.11 inch on paper.
    put_page(&mut s, "LineToNodeX", "1.1");
    assert!(near(num(&run(&mut s, "page.routing", json!({}))["connector_to_shape"]["x"]), 0.11));
}

#[test]
fn a_direction_the_style_does_not_take_is_refused_in_words() {
    let mut s = blank_session();
    let words = refused(&mut s, "page.set_routing", json!({"style": "right_angle", "direction": "top_to_bottom"}));
    assert!(words.contains("right_angle") && words.contains("takes no direction"), "{words}");
    for style in ["straight", "network", "centre_to_centre"] {
        let words = refused(&mut s, "page.set_routing", json!({"style": style, "direction": "left_to_right"}));
        assert!(words.contains(style) && words.contains("takes no direction"), "{style}: {words}");
    }
    // A direction alone turns the page's style, and a page whose style has none cannot be turned.
    let words = refused(&mut s, "page.set_routing", json!({"direction": "left_to_right"}));
    assert!(words.contains("right_angle") && words.contains("takes no direction") && words.contains("`style`"), "{words}");
    // The two ways of the simple style are the simple style's alone.
    let words = refused(&mut s, "page.set_routing", json!({"style": "flowchart", "direction": "level_first"}));
    assert!(words.contains("level_first") && words.contains("simple"), "{words}");
    let words = refused(&mut s, "page.set_routing", json!({"style": "tree", "direction": "upright_first"}));
    assert!(words.contains("upright_first") && words.contains("simple"), "{words}");
    // A word that is no style and no direction names the words there are.
    let words = refused(&mut s, "page.set_routing", json!({"style": "zigzag"}));
    assert!(
        words.contains("`style` must be right_angle, straight, org_chart, flowchart, tree, network, centre_to_centre or simple")
            && words.contains("zigzag"),
        "{words}"
    );
    let words = refused(&mut s, "page.set_routing", json!({"style": "flowchart", "direction": "sideways"}));
    assert!(
        words.contains("`direction` must be top_to_bottom, left_to_right, bottom_to_top or right_to_left") && words.contains("sideways"),
        "{words}"
    );
    let words = refused(&mut s, "page.set_routing", json!({"separate": "some"}));
    assert!(words.contains("`separate` must be unrelated, all, none or by_style"), "{words}");
    let words = refused(&mut s, "page.set_routing", json!({"overlap": "some"}));
    assert!(words.contains("`overlap` must be by_style, all, none or related"), "{words}");
    let words = refused(&mut s, "page.set_routing", json!({"appearance": "wavy"}));
    assert!(words.contains("`appearance` must be straight or curved"), "{words}");
    let words = refused(&mut s, "page.set_routing", json!({}));
    assert!(words.contains("give at least one of"), "{words}");
    // The connectors' own command says the same of its styles.
    let (mut s, _, _, c) = z_pair();
    let words = refused(&mut s, "connector.style", json!({"style": "right_angle", "direction": "top_to_bottom", "ids": [c]}));
    assert!(words.contains("right_angle") && words.contains("takes no direction"), "{words}");
    let words = refused(&mut s, "connector.style", json!({"style": "curved", "direction": "top_to_bottom"}));
    assert!(words.contains("curved") && words.contains("takes no direction"), "{words}");
    let words = refused(&mut s, "connector.style", json!({"style": "zigzag", "ids": [c]}));
    assert!(
        words.contains("`style` must be right_angle, straight, curved, org_chart, flowchart, tree, network, centre_to_centre or simple"),
        "{words}"
    );
}

#[test]
fn a_length_that_is_not_more_than_nothing_is_refused_in_words() {
    let mut s = blank_session();
    for (params, want) in [
        (json!({"between_shapes": {"x": 0.0}}), "`between_shapes.x` must be a number of inches more than 0"),
        (json!({"shape_size": {"y": -1.0}}), "`shape_size.y` must be a number of inches more than 0"),
        (json!({"connector_to_connector": {"x": 1.0e300}}), "`connector_to_connector.x` must be a number of inches more than 0 and within"),
        (json!({"connector_to_shape": {"x": "wide"}}), "`connector_to_shape.x` must be a number"),
        (json!({"connector_to_shape": {"z": 1}}), "`connector_to_shape` has an unknown key `z`"),
        (json!({"connector_to_shape": {}}), "`connector_to_shape` needs `x`, `y` or both"),
        (json!({"connector_to_shape": 0.5}), "`connector_to_shape` must be an object with `x`, `y` or both"),
        (json!({"plow": "yes"}), "`plow` must be true or false"),
        (json!({"split": 1}), "`split` must be true or false"),
        (json!({"page": 9, "plow": true}), "there is no page 9"),
    ] {
        let words = refused(&mut s, "page.set_routing", params.clone());
        assert!(words.contains(want), "{params}: {words}");
    }
}

/// Six connectors from one shape to six: [follower, own style, as needed, never, on crossover, on a locked layer], each a Z of four points.
fn six() -> (Session, Vec<u32>) {
    let mut s = blank_session();
    let from = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let mut wires = Vec::new();
    for i in 0..6 {
        let to = drop_shape(&mut s, "rectangle", 9.0, 2.5 + f64::from(i));
        wires.push(id_of(&run(&mut s, "connector.add", json!({"from": {"id": from, "point": 3}, "to": {"id": to, "point": 1}}))));
    }
    run(&mut s, "connector.style", json!({"style": "right_angle", "ids": [wires[1]]}));
    run(&mut s, "connector.reroute", json!({"ids": [wires[2]], "mode": "as_needed"}));
    run(&mut s, "connector.reroute", json!({"ids": [wires[3]], "mode": "never"}));
    run(&mut s, "connector.reroute", json!({"ids": [wires[4]], "mode": "on_crossover"}));
    run(&mut s, "layer.add", json!({"name": "Held"}));
    run(&mut s, "layer.assign", json!({"ids": [wires[5]], "layers": ["Held"]}));
    run(&mut s, "layer.set", json!({"layer": "Held", "lock": true}));
    (s, wires)
}

fn legs(s: &Session, wires: &[u32]) -> Vec<usize> {
    wires.iter().map(|w| geometry(doc(s), *w).len()).collect()
}

#[test]
fn a_new_page_style_routes_the_followers_set_to_freely_and_no_other() {
    // A connector with a style of its own, one set to 1, 2 or 3 and one on a locked layer are left; the one that follows the page is routed by the new style.
    let (mut s, wires) = six();
    assert_eq!(legs(&s, &wires), [4, 4, 4, 4, 4, 4], "each is a Z");
    let before = quiet(&s);
    let r = run(&mut s, "page.set_routing", json!({"style": "straight"}));
    assert_eq!(r["routed"], 1, "one connector follows the page, is set to reroute freely and is not locked: {r}");
    assert_eq!(legs(&s, &wires), [2, 4, 4, 4, 4, 4], "only the follower is straight now");
    assert_eq!(quiet(&s).undo_steps, before.undo_steps + 1, "the style and the new route are one undo step");
    // Back to right angle: the follower is routed again, and a call that changes the page's style to what it reads is no route at all.
    let r = run(&mut s, "page.set_routing", json!({"style": "right_angle"}));
    assert_eq!(r["routed"], 1);
    assert_eq!(legs(&s, &wires), [4, 4, 4, 4, 4, 4]);
    // A style that draws the same (flowchart is a right angle route) is a new style all the same, and the follower is asked once.
    let r = run(&mut s, "page.set_routing", json!({"style": "flowchart", "direction": "top_to_bottom"}));
    assert_eq!(r["routed"], 1);
    let r = run(&mut s, "page.set_routing", json!({"style": "flowchart", "direction": "top_to_bottom"}));
    assert_eq!(r["routed"], 0, "the same style again asks nothing");
    // 0 and 1 are the same style: a page that reads 0 asked for right angle writes its 1 and routes nothing.
    let mut fresh = blank_session();
    let from = drop_shape(&mut fresh, "rectangle", 2.0, 4.0);
    let to = drop_shape(&mut fresh, "rectangle", 9.0, 6.0);
    run(&mut fresh, "connector.add", json!({"from": {"id": from, "point": 3}, "to": {"id": to, "point": 1}}));
    let r = run(&mut fresh, "page.set_routing", json!({"style": "right_angle"}));
    assert_eq!((r["routed"].as_u64(), page_cell(doc(&fresh), "RouteStyle")), (Some(0), Some(1.0)));
    // Undo takes the style and the route back together.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(legs(&s, &wires), [4, 4, 4, 4, 4, 4]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_connectors_menu_with_nothing_selected_routes_nothing() {
    // Today's rule, kept: `connector.style` with no `ids` sets the default for new connectors, and the connectors there are stay as they are.
    let (mut s, wires) = six();
    let before = geometry(doc(&s), wires[0]);
    let r = run(&mut s, "connector.style", json!({"style": "straight"}));
    assert_eq!(r, json!({"page": 1, "style": "straight", "styled": 0, "default_changed": true}));
    assert_eq!(page_cell(doc(&s), "RouteStyle"), Some(2.0));
    assert_eq!(geometry(doc(&s), wires[0]), before, "no connector was routed");
    // The same for a directional style: the page takes it, and no connector is routed.
    let r = run(&mut s, "connector.style", json!({"style": "tree", "direction": "right_to_left"}));
    assert_eq!(r, json!({"page": 1, "style": "tree", "direction": "right_to_left", "styled": 0, "default_changed": true}));
    assert_eq!((page_cell(doc(&s), "RouteStyle"), page_cell(doc(&s), "LineRouteExt")), (Some(15.0), Some(1.0)));
    assert_eq!(geometry(doc(&s), wires[0]), before);
}

#[test]
fn the_appearance_routes_nothing() {
    // The tab's Appearance concerns new connectors: a connector takes it at its next route.
    let (mut s, wires) = six();
    let rows: Vec<_> = wires.iter().map(|w| geometry(doc(&s), *w)).collect();
    let steps = quiet(&s).undo_steps;
    let r = run(&mut s, "page.set_routing", json!({"appearance": "curved"}));
    assert_eq!(r["routed"], 0);
    assert_eq!(page_cell(doc(&s), "LineRouteExt"), Some(2.0));
    assert_eq!(wires.iter().map(|w| geometry(doc(&s), *w)).collect::<Vec<_>>(), rows, "no connector was routed");
    assert_eq!(quiet(&s).undo_steps, steps + 1);
    // The spacing cells ask nothing either: "the next route keeps the new gap, and paths already saved stay as they are".
    run(&mut s, "page.set_routing", json!({"connector_to_shape": {"x": 0.5, "y": 0.5}, "separate": "all", "plow": true}));
    assert_eq!(wires.iter().map(|w| geometry(doc(&s), *w)).collect::<Vec<_>>(), rows);
}

#[test]
fn page_routing_reports_what_the_page_holds_and_the_defaults() {
    let mut s = blank_session();
    let q = run(&mut s, "page.routing", json!({}));
    assert_eq!(q["page"], 1);
    assert_eq!((q["style"].as_str(), q["direction"].as_str(), q["style_code"].as_u64()), (Some("right_angle"), None, Some(1)), "{q}");
    assert_eq!(
        (q["separate"].as_str(), q["overlap"].as_str(), q["appearance"].as_str()),
        (Some("unrelated"), Some("by_style"), Some("straight")),
        "{q}"
    );
    assert_eq!((q["plow"].as_bool(), q["split"].as_bool()), (Some(false), Some(false)), "{q}");
    // The root's values: 1/8, 1/4 and 3/8 inch.
    assert_eq!(q["connector_to_connector"], json!({"x": 0.125, "y": 0.125}), "{q}");
    assert_eq!(q["connector_to_shape"], json!({"x": 0.125, "y": 0.125}), "{q}");
    assert_eq!(q["between_shapes"], json!({"x": 0.375, "y": 0.375}), "{q}");
    assert_eq!(q["shape_size"], json!({"x": 0.25, "y": 0.25}), "{q}");
    assert_eq!(q["line_jumps"]["code"], 1, "the jump values are the page's, read as the root's where it holds none: {q}");
    for key in [
        "style",
        "separate",
        "overlap",
        "appearance",
        "plow",
        "split",
        "between_shapes",
        "shape_size",
        "connector_to_connector",
        "connector_to_shape",
        "line_jumps",
    ] {
        assert_eq!(q["held"][key], false, "a blank page holds no {key}: {q}");
    }
    // What the page is given it holds, and what it is not given it does not.
    run(&mut s, "page.set_routing", json!({"separate": "all", "connector_to_shape": {"x": 0.3, "y": 0.35}}));
    run(&mut s, "connector.line_jumps", json!({"code": 2}));
    let q = run(&mut s, "page.routing", json!({}));
    assert_eq!(q["separate"], "all");
    assert_eq!(q["connector_to_shape"], json!({"x": 0.3, "y": 0.35}));
    assert_eq!(q["line_jumps"]["code"], 2);
    let held: Vec<(&str, bool)> = ["separate", "connector_to_shape", "line_jumps", "style", "overlap", "between_shapes"]
        .into_iter()
        .map(|k| (k, q["held"][k].as_bool().unwrap()))
        .collect();
    assert_eq!(
        held,
        [("separate", true), ("connector_to_shape", true), ("line_jumps", true), ("style", false), ("overlap", false), ("between_shapes", false)]
    );
    // A cell that is not a number, or is below 0, reads as the default; a cell of 0 is the cell's own.
    put_page(&mut s, "LineToNodeX", "-1");
    put_page(&mut s, "LineToNodeY", "0");
    let q = run(&mut s, "page.routing", json!({}));
    assert_eq!(q["connector_to_shape"], json!({"x": 0.125, "y": 0.0}), "{q}");
    // Out-of-range codes read as the default (they are kept verbatim and read by the cell's range).
    put_page(&mut s, "LineAdjustFrom", "7");
    put_page(&mut s, "LineAdjustTo", "-1");
    put_page(&mut s, "LineRouteExt", "9");
    let q = run(&mut s, "page.routing", json!({}));
    assert_eq!(
        (q["separate"].as_str(), q["overlap"].as_str(), q["appearance"].as_str()),
        (Some("unrelated"), Some("by_style"), Some("straight")),
        "{q}"
    );
    for (code, separate, overlap) in [(0.0, "unrelated", "by_style"), (1.0, "all", "all"), (2.0, "none", "none"), (3.0, "by_style", "related")] {
        put_page(&mut s, "LineAdjustFrom", &code.to_string());
        put_page(&mut s, "LineAdjustTo", &code.to_string());
        let q = run(&mut s, "page.routing", json!({}));
        assert_eq!((q["separate"].as_str(), q["overlap"].as_str()), (Some(separate), Some(overlap)), "code {code}");
    }
    for (code, appearance) in [(0.0, "straight"), (1.0, "straight"), (2.0, "curved")] {
        put_page(&mut s, "LineRouteExt", &code.to_string());
        assert_eq!(run(&mut s, "page.routing", json!({}))["appearance"], appearance, "code {code}");
    }
    assert_eq!(refused(&mut s, "page.routing", json!({"page": 9})), "there is no page 9");
}

#[test]
fn connector_path_gives_the_corners_in_paper_inches() {
    for scaled in [false, true] {
        let (mut s, c) = if scaled {
            // The same drawing on a page of a scale of 4, made after the scale: the corners are paper's, whatever the cells hold. (A scale of 4 is within the range of eight of the
            // masters' own, so the shapes keep their size in the drawing; at 10 a drop keeps the master's size on paper, and the cells would say the same thing.)
            let mut s = blank_session();
            run(&mut s, "page.scale", json!({"paper": 1, "paper_unit": "in", "drawing": 4, "drawing_unit": "in"}));
            let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
            let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
            let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
            (s, c)
        } else {
            let (s, _, _, c) = z_pair();
            (s, c)
        };
        let said = run(&mut s, "connector.path", json!({"id": c}));
        let points: Vec<(f64, f64)> = said["points"].as_array().unwrap().iter().map(|p| (num(&p[0]), num(&p[1]))).collect();
        // (A rectangle is 1.5 inches across in the drawing, so 0.375 on a page of a scale of 4: its right and left points are 0.1875 from its pin.)
        let (out, back) = if scaled { (0.1875, 0.1875) } else { (0.75, 0.75) };
        let want = [(3.0 + out, 4.0), (6.0, 4.0), (6.0, 6.0), (9.0 - back, 6.0)];
        assert!(points.len() == 4 && points.iter().zip(want).all(|(g, w)| near(g.0, w.0) && near(g.1, w.1)), "scaled {scaled}: {points:?}");
        assert_eq!(
            (said["style"].as_str(), said["reroute"].as_str(), said["hand_edited"].as_bool(), said["editable"].as_bool()),
            (Some("right_angle"), Some("freely"), Some(false), Some(true)),
            "{said}"
        );
        assert_eq!((said["page"].as_u64(), said["id"].as_u64()), (Some(1), Some(u64::from(c))));
        // A route edited by hand says so, and what the connector is set to.
        run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
        let said = run(&mut s, "connector.path", json!({"id": c}));
        assert_eq!((said["hand_edited"].as_bool(), said["reroute"].as_str()), (Some(true), Some("on_crossover")), "{said}");
        assert!(near(num(&said["points"][1][0]), 7.0) || near(num(&said["points"][2][0]), 7.0) || near(num(&said["points"][1][1]), 7.0), "{said}");
        run(&mut s, "connector.reset", json!({"ids": [c]}));
        // A curve is no path of lines: the corners are null, and what it draws and how it is set are still told.
        run(&mut s, "connector.style", json!({"style": "curved", "ids": [c]}));
        let said = run(&mut s, "connector.path", json!({"id": c}));
        assert_eq!((said["points"].is_null(), said["style"].as_str(), said["editable"].as_bool()), (true, Some("curved"), Some(false)), "{said}");
        // A straight one has its two ends.
        run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
        let said = run(&mut s, "connector.path", json!({"id": c}));
        assert_eq!(
            (said["points"].as_array().map(Vec::len), said["style"].as_str(), said["editable"].as_bool()),
            (Some(2), Some("straight"), Some(false))
        );
    }
}

#[test]
fn connector_path_refuses_what_is_no_connector_in_words() {
    let (mut s, a, _, _) = z_pair();
    let words = refused(&mut s, "connector.path", json!({"id": a}));
    assert!(words.contains(&format!("shape {a} is not a connector")), "{words}");
    let words = refused(&mut s, "connector.path", json!({"id": 99}));
    assert!(words.contains("no shape 99"), "{words}");
    let words = refused(&mut s, "connector.path", json!({}));
    assert!(words.contains("missing `id`"), "{words}");
    let words = refused(&mut s, "connector.path", json!({"id": a, "page": 9}));
    assert!(words.contains("there is no page 9"), "{words}");
    // It reads: no undo step and no journal entry of an edit.
    let before = snapshot(&s);
    run(&mut s, "connector.path", json!({"id": 3}));
    run(&mut s, "page.routing", json!({}));
    assert_eq!(snapshot(&s), before, "a query changes nothing, and it is not journaled");
}

#[test]
fn a_directional_connector_style_writes_its_code_and_routes_the_connector_again() {
    let (mut s, _, _, c) = z_pair();
    let steps = quiet(&s).undo_steps;
    let r = run(&mut s, "connector.style", json!({"style": "flowchart", "direction": "left_to_right", "ids": [c]}));
    assert_eq!(r, json!({"page": 1, "style": "flowchart", "direction": "left_to_right", "styled": 1, "default_changed": false}));
    assert_eq!((cell(doc(&s), c, "ShapeRouteStyle"), cell(doc(&s), c, "ConLineRouteExt")), (Some(6.0), Some(1.0)));
    assert_eq!(quiet(&s).undo_steps, steps + 1);
    // Its route is a right angle route, the same Z, and what it follows is its own style.
    assert_eq!(geometry(doc(&s), c).len(), 4);
    let said = run(&mut s, "connector.path", json!({"id": c}));
    assert_eq!(said["route_style"], json!({"style": "flowchart", "direction": "left_to_right", "code": 6, "own": true}));
    assert_eq!(said["style"], "right_angle");
    // The same again is no change; a hand edit is undone, as for the others.
    let r = run(&mut s, "connector.style", json!({"style": "flowchart", "direction": "left_to_right", "ids": [c]}));
    assert_eq!(r["styled"], 0);
    // Each of the words writes its code, and a style with no direction given flows down the page.
    for (word, direction, code) in [
        ("org_chart", None, 3.0),
        ("tree", Some("bottom_to_top"), 14.0),
        ("network", None, 9.0),
        ("centre_to_centre", None, 16.0),
        ("simple", Some("level_first"), 21.0),
    ] {
        let mut params = json!({"style": word, "ids": [c]});
        if let Some(direction) = direction {
            params["direction"] = json!(direction);
        }
        run(&mut s, "connector.style", params);
        assert_eq!(cell(doc(&s), c, "ShapeRouteStyle"), Some(code), "{word}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_words_of_a_style_are_the_codes_of_route_r1_and_a_lean_is_the_way_it_says() {
    let down = Way::Down;
    for (code, style, direction) in TABLE {
        let got = route_style_of(f64::from(code));
        let lean = lean_of(got);
        // A lean only for 3 to 8, 10 to 15 and 17 to 20, the way the table says: the flow of the style.
        let want = match (code, direction) {
            (3..=8 | 10..=15 | 17..=20, Some("top_to_bottom")) => Some(down),
            (3..=8 | 10..=15 | 17..=20, Some("left_to_right")) => Some(Way::Right),
            (3..=8 | 10..=15 | 17..=20, Some("bottom_to_top")) => Some(Way::Up),
            (3..=8 | 10..=15 | 17..=20, Some("right_to_left")) => Some(Way::Left),
            _ => None,
        };
        assert_eq!(lean, want, "{code} {style} {direction:?}");
        assert_eq!(u32::from(got.code()), code.max(1), "{code}");
        assert_eq!(got.family(), style, "{code}");
        assert_eq!(got.direction(), direction, "{code}");
    }
    // Total: a code that is no number or no code is right angle.
    for odd in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 22.5, 23.0, 255.0, 1.0e300, -1.0e300, 2.5] {
        assert_eq!(route_style_of(odd), RouteStyle::RightAngle, "{odd}");
    }
    // A number near a code is that code, as every cell of this kind is read.
    assert_eq!(route_style_of(5.4), RouteStyle::Flowchart(Flow::TopToBottom));
    assert_eq!(route_style_of(4.6), RouteStyle::Flowchart(Flow::TopToBottom));
}

// ------------------------------------------------------------------------------------------------------------------ saved and reopened

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// What the page says of its routing, and where the connector stands: what a save must keep.
fn facts(s: &mut Session) -> Json {
    run(s, "page.routing", json!({}))
}

fn routed_drawing() -> Session {
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}}));
    run(
        &mut s,
        "page.set_routing",
        json!({
            "style": "tree", "direction": "right_to_left", "separate": "all", "overlap": "related", "appearance": "curved", "plow": true, "split": true,
            "between_shapes": {"x": 0.5, "y": 0.6}, "shape_size": {"x": 1.0, "y": 1.1},
            "connector_to_connector": {"x": 0.2, "y": 0.25}, "connector_to_shape": {"x": 0.3, "y": 0.35}
        }),
    );
    s
}

#[test]
fn the_settings_are_kept_by_a_diacraft_save() {
    let dir = Dir::new();
    let mut s = routed_drawing();
    let want = facts(&mut s);
    assert_eq!(want["style"], "tree", "{want}");
    let path = dir.path("routed.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(facts(&mut opened(&path)), want, ".diacraft");
}

#[test]
fn and_by_the_vsdx_writer_and_a_save_into_a_vsdx_file() {
    let dir = Dir::new();
    let mut s = routed_drawing();
    let want = facts(&mut s);
    let path = dir.path("routed.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(facts(&mut opened(&path)), want, "VSDX from the model");
    // A person's file: the page takes the settings, and the save splices into the file.
    let other = dir.write(
        "other.vsdx",
        &VsdxBuilder::new().page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#))).with(Pkg::new()).build(),
    );
    let mut s = opened(&other);
    run(
        &mut s,
        "page.set_routing",
        json!({"style": "flowchart", "direction": "bottom_to_top", "separate": "none", "connector_to_connector": {"x": 0.4, "y": 0.45}, "plow": true}),
    );
    let into = facts(&mut s);
    run(&mut s, "file.save", json!({}));
    assert_eq!(facts(&mut opened(&other)), into, "VSDX saved into itself");
}

#[test]
fn and_by_an_odg_written_from_the_model_and_a_save_into_an_odg_file() {
    let dir = Dir::new();
    let mut s = routed_drawing();
    let want = facts(&mut s);
    let path = dir.path("routed.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(facts(&mut opened(&path)), want, ".odg from the model");
    // An `.odg` of another program's: its page takes the settings, and the save splices into the file.
    let other = dir.write("other.odg", &Drawing::new().page(&rect("A", (1.0, 1.0, 2.0, 1.0), "")).bytes());
    let mut s = opened(&other);
    run(
        &mut s,
        "page.set_routing",
        json!({"style": "simple", "direction": "level_first", "overlap": "none", "connector_to_shape": {"x": 0.4, "y": 0.45}, "split": true}),
    );
    let into = facts(&mut s);
    run(&mut s, "file.save", json!({}));
    assert_eq!(facts(&mut opened(&other)), into, ".odg saved into itself");
}

#[test]
fn a_directional_connector_is_kept_by_a_diacraft_save_and_a_vsdx_save() {
    let dir = Dir::new();
    for extension in ["diacraft", "vsdx"] {
        let (mut s, _, _, c) = z_pair();
        s.pin_clock(Some(PINNED));
        run(&mut s, "connector.style", json!({"style": "flowchart", "direction": "left_to_right", "ids": [c]}));
        let before = geometry(doc(&s), c);
        let path = dir.path(&format!("wire.{extension}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let back = opened(&path);
        let got = connector_style(doc(&back), 1, c).unwrap();
        assert_eq!((got.route, got.own), (RouteStyle::Flowchart(Flow::LeftToRight), true), "{extension}");
        assert_eq!(geometry(doc(&back), c).len(), before.len(), "{extension}: the route");
    }
}

#[test]
fn an_odg_save_writes_a_directional_connector_made_here_and_its_style_comes_back() {
    // The `.odg` format has a connector type for a standard, a straight and a curved connector (`draw:type`), and no place for a flowchart's direction: the writer writes the connector
    // with the default type and the cells that say its style ride in DiaCraft's data (flipped this test: the save used to refuse it, "a new connector of a route kind the
    // writer does not write"). A reopened file holds the connector with the style it had; the page's own style is kept too (the page's cells ride in DiaCraft's own document file).
    let dir = Dir::new();
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "connector.style", json!({"style": "flowchart", "direction": "left_to_right", "ids": [c]}));
    let path = dir.path("wire.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let back = opened(&path);
    // A file numbers its shapes as it likes: the connector is the shape of the page that holds the style.
    let page = doc(&back).pages.get(0).unwrap().clone();
    let found = page.shapes.iter().find(|shape| shape.sheet.cell("ShapeRouteStyle").is_some()).map(|shape| shape.id).expect("the connector");
    let got = connector_style(doc(&back), 1, found).unwrap();
    assert_eq!((got.route, got.own), (RouteStyle::Flowchart(Flow::LeftToRight), true), "the style it had");
}
