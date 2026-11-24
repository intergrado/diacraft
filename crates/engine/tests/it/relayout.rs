//! Re-Layout Page (`page.relayout`), Auto Space (`shape.autospace`), Spacing Options and Avoid Page Breaks. The
//! placement is `diacraft-layout`'s (its tests hold the algorithms); these hold what the engine makes of it: the shapes move as one edit, the connectors are routed again, the settings
//! are written into the page's own cells, a shape on a locked layer stays, and a layout that changes nothing is no edit.

use diacraft_doc::{CellAddr, Document, Resolver, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::draw::shape_corners;
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, route};
use crate::{blank_session, check_session, run, snapshot};

type P = (f64, f64);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

/// The box of a shape on page 1: left, bottom, right, top.
fn bounds(d: &Document, id: u32) -> (f64, f64, f64, f64) {
    let c = shape_corners(d, 1, id).unwrap();
    let xs = c.iter().map(|p| p.0);
    let ys = c.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

fn centre(d: &Document, id: u32) -> P {
    let b = bounds(d, id);
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

/// What a cell of the first page holds (page 1, or page 0 of a drawing an `.odg` reader numbered), if it holds a number.
pub(crate) fn page_cell(d: &Document, name: &str) -> Option<f64> {
    let first = d.pages.iter().next()?.id;
    let found = Resolver::new(d).resolve(&SheetRef::Page(first), &CellAddr::direct(name))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// Shapes 1.5 by 1, dropped in a row (x = 2, 5, 8, ... on y = 4), joined each to the next from its right point to the next one's left. → the shapes and the connectors.
fn chain(s: &mut Session, n: usize) -> (Vec<u32>, Vec<u32>) {
    let shapes: Vec<u32> = (0..n).map(|i| drop_shape(s, "rectangle", 2.0 + 3.0 * i as f64, 4.0)).collect();
    let wires = shapes
        .windows(2)
        .map(|w| id_of(&run(s, "connector.add", json!({"from": {"id": w[0], "point": 3}, "to": {"id": w[1], "point": 1}}))))
        .collect();
    (shapes, wires)
}

fn overlap(d: &Document, ids: &[u32]) -> Option<(u32, u32)> {
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            let (p, q) = (bounds(d, *a), bounds(d, *b));
            if p.0 < q.2 - 1e-6 && q.0 < p.2 - 1e-6 && p.1 < q.3 - 1e-6 && q.1 < p.3 - 1e-6 {
                return Some((*a, *b));
            }
        }
    }
    None
}

#[test]
fn a_chain_is_laid_out_down_the_page_a_space_apart_and_the_choice_is_written_into_the_page() {
    // A flow top to bottom, shapes 1 high with a space of 1 between: the middles are 2 apart, in one column. The page holds PlaceStyle 1 (the table) and the spaces.
    let mut s = blank_session();
    let (shapes, _) = chain(&mut s, 3);
    let r = run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom", "spacing": 1.0}));
    // The corner stays, so the first shape is where it was and the other two move.
    assert_eq!((r["nodes"].as_u64(), r["joins"].as_u64(), r["moved"].as_u64()), (Some(3), Some(2), Some(2)), "{r}");
    assert_eq!((r["style"].as_str(), r["direction"].as_str(), r["place_style"].as_u64()), (Some("flowchart"), Some("top_to_bottom"), Some(1)), "{r}");
    let d = doc(&s);
    let at: Vec<P> = shapes.iter().map(|id| centre(d, *id)).collect();
    assert!(near(at[0].0, at[1].0) && near(at[1].0, at[2].0), "one column: {at:?}");
    assert!(near(at[0].1 - at[1].1, 2.0) && near(at[1].1 - at[2].1, 2.0), "a layer apart, the first on top: {at:?}");
    assert_eq!(page_cell(d, "PlaceStyle"), Some(1.0));
    assert_eq!((page_cell(d, "AvenueSizeX"), page_cell(d, "AvenueSizeY")), (Some(1.0), Some(1.0)));
    check_session(&s).unwrap();
}

#[test]
fn the_corner_of_the_diagram_stays_where_it_was() {
    // Catches a layout that moves the diagram to the page's origin: the top-left corner of the boxes is where it was before.
    let mut s = blank_session();
    let (shapes, _) = chain(&mut s, 3);
    let corner = |d: &Document| {
        let b: Vec<_> = shapes.iter().map(|id| bounds(d, *id)).collect();
        (b.iter().map(|b| b.0).fold(f64::INFINITY, f64::min), b.iter().map(|b| b.3).fold(f64::NEG_INFINITY, f64::max))
    };
    let before = corner(doc(&s));
    run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom"}));
    let after = corner(doc(&s));
    assert!(near(before.0, after.0) && near(before.1, after.1), "{before:?} {after:?}");
}

#[test]
fn the_connectors_are_routed_again_and_follow_the_shapes() {
    // After the layout each connector still runs from the shape it begins at to the shape it ends at: its first point is on its begin shape's box, its last on its end's.
    let mut s = blank_session();
    let (shapes, wires) = chain(&mut s, 3);
    run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom", "spacing": 1.0}));
    let d = doc(&s);
    // A flow down the page leaves a shape by its bottom and comes to the next by its top: the route is a straight drop from the bottom middle to the top middle.
    for (i, wire) in wires.iter().enumerate() {
        let path = route(d, *wire);
        let (a, b) = (bounds(d, shapes[i]), bounds(d, shapes[i + 1]));
        let (first, last) = (path[0], *path.last().unwrap());
        assert!(near(first.1, a.1) && near(first.0, (a.0 + a.2) / 2.0), "connector {wire} begins at the bottom middle of its shape: {path:?} {a:?}");
        assert!(near(last.1, b.3) && near(last.0, (b.0 + b.2) / 2.0), "and ends at the top middle of the next: {path:?} {b:?}");
    }
    run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "left_to_right", "spacing": 1.0}));
    let d = doc(&s);
    for (i, wire) in wires.iter().enumerate() {
        let path = route(d, *wire);
        let (first, last) = (path[0], *path.last().unwrap());
        let (a, b) = (bounds(d, shapes[i]), bounds(d, shapes[i + 1]));
        let on = |p: P, b: (f64, f64, f64, f64)| p.0 >= b.0 - 1e-6 && p.0 <= b.2 + 1e-6 && p.1 >= b.1 - 1e-6 && p.1 <= b.3 + 1e-6;
        assert!(on(first, a) && on(last, b), "connector {wire}: {path:?}, shapes {a:?} {b:?}");
    }
}

#[test]
fn one_undo_step_takes_the_whole_layout_back_and_a_second_layout_is_no_step() {
    let mut s = blank_session();
    let (shapes, _) = chain(&mut s, 4);
    let before: Vec<P> = shapes.iter().map(|id| centre(doc(&s), *id)).collect();
    let steps = snapshot(&s).undo_steps;
    run(&mut s, "page.relayout", json!({"style": "hierarchy", "direction": "top_to_bottom", "spacing": 0.5}));
    assert_eq!(snapshot(&s).undo_steps, steps + 1, "one step for the shapes, the connectors and the cells");
    // The same layout again changes nothing: no step, and the answer says so.
    let again = run(&mut s, "page.relayout", json!({"style": "hierarchy", "direction": "top_to_bottom", "spacing": 0.5}));
    assert_eq!(again["moved"], 0, "{again}");
    assert_eq!(snapshot(&s).undo_steps, steps + 1, "a layout that is made already is no edit");
    run(&mut s, "edit.undo", json!({}));
    let back: Vec<P> = shapes.iter().map(|id| centre(doc(&s), *id)).collect();
    assert!(back.iter().zip(&before).all(|(a, b)| near(a.0, b.0) && near(a.1, b.1)), "{back:?} {before:?}");
    assert_eq!(page_cell(doc(&s), "PlaceStyle"), None, "the cells go back with the shapes");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(page_cell(doc(&s), "PlaceStyle"), Some(16.0 + 1.0), "Hierarchy, top to bottom, centred: the 17");
}

#[test]
fn every_style_places_a_tree_with_no_two_shapes_overlapping() {
    // A root, two children and a grandchild each: the styles of the gallery, with the page's spaces. Catches a style that puts two shapes on one place.
    let styles = [
        ("flowchart", "top_to_bottom"),
        ("flowchart", "right_to_left"),
        ("hierarchy", "left_to_right"),
        ("hierarchy", "bottom_to_top"),
        ("compact_tree", "down_right"),
        ("compact_tree", "up_left"),
        ("radial", ""),
        ("circular", ""),
    ];
    for (style, direction) in styles {
        let mut s = blank_session();
        let shapes: Vec<u32> = (0..7).map(|i| drop_shape(&mut s, "rectangle", 1.5 + f64::from(i), 4.0)).collect();
        for (from, to) in [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5), (2, 6)] {
            run(&mut s, "connector.add", json!({"from": {"id": shapes[from], "point": 3}, "to": {"id": shapes[to], "point": 1}}));
        }
        let mut params = json!({"style": style, "spacing": 0.5});
        if !direction.is_empty() {
            params["direction"] = json!(direction);
        }
        run(&mut s, "page.relayout", params);
        assert_eq!(overlap(doc(&s), &shapes), None, "{style} {direction}");
        check_session(&s).unwrap();
    }
}

#[test]
fn a_selection_lays_out_only_the_shapes_named() {
    // The first two shapes of four, named: they are laid out, and the other two stay where they were.
    let mut s = blank_session();
    let (shapes, _) = chain(&mut s, 4);
    let before: Vec<P> = shapes.iter().map(|id| centre(doc(&s), *id)).collect();
    let r = run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom", "ids": [shapes[0], shapes[1]]}));
    assert_eq!(r["nodes"], 2, "{r}");
    let d = doc(&s);
    for id in &shapes[2..] {
        let now = centre(d, *id);
        let was = before[shapes.iter().position(|s| s == id).unwrap()];
        assert!(near(now.0, was.0) && near(now.1, was.1), "shape {id} was not named and moved: {was:?} -> {now:?}");
    }
    assert!(centre(d, shapes[0]).1 > centre(d, shapes[1]).1, "the two named are one above the other");
}

#[test]
fn a_shape_on_a_locked_layer_is_left_where_it_is_and_said() {
    // For the shapes a layout does not name: a layer's Lock "stops shapes on the layer from being moved".
    let mut s = blank_session();
    let (shapes, _) = chain(&mut s, 3);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [shapes[1]], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let held = centre(doc(&s), shapes[1]);
    let r = run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom"}));
    assert_eq!(r["locked"], json!([shapes[1]]), "{r}");
    assert_eq!(r["nodes"], 2, "{r}");
    let now = centre(doc(&s), shapes[1]);
    assert!(near(now.0, held.0) && near(now.1, held.1), "the locked shape did not move: {held:?} {now:?}");
    // Naming it is refused by the registry's lock rule.
    let e = s.execute("page.relayout", &json!({"ids": [shapes[1]]})).unwrap_err().to_string();
    assert!(e.contains("locked"), "{e}");
}

#[test]
fn a_container_goes_with_what_it_holds_and_its_members_are_not_laid_out_on_their_own() {
    // The followers: the members ride with the container, so the layout places the container and the shape outside it, and never the members apart from it.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 8.0);
    let b = drop_shape(&mut s, "rectangle", 4.0, 8.0);
    let outside = drop_shape(&mut s, "rectangle", 9.0, 2.0);
    let made = run(&mut s, "container.add", json!({"ids": [a, b], "heading": "Stage"}));
    let container = id_of(&made);
    let rel = |d: &Document| (centre(d, b).0 - centre(d, a).0, centre(d, b).1 - centre(d, a).1);
    let inside = rel(doc(&s));
    let r = run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "left_to_right", "spacing": 1.0}));
    assert_eq!(r["nodes"], 2, "the container and the shape outside it: {r}");
    assert_eq!(r["followers"], 2, "{r}");
    let after = rel(doc(&s));
    assert!(near(after.0, inside.0) && near(after.1, inside.1), "the members kept their places in the container: {inside:?} {after:?}");
    assert_eq!(overlap(doc(&s), &[container, outside]), None);
}

#[test]
fn the_page_cells_are_the_ones_the_specifications_name_and_a_second_call_keeps_what_it_is_not_given() {
    let mut s = blank_session();
    chain(&mut s, 2);
    run(
        &mut s,
        "page.relayout",
        json!({"style": "hierarchy", "direction": "right_to_left", "alignment": "end", "spacing_x": 0.5, "spacing_y": 0.75, "block_x": 1.5, "block_y": 2.0, "depth": 2, "flip": 9, "connector_style": "straight", "appearance": "curved", "enlarge_page": true}),
    );
    let d = doc(&s);
    for (name, want) in [
        ("PlaceStyle", 27.0),
        ("AvenueSizeX", 0.5),
        ("AvenueSizeY", 0.75),
        ("BlockSizeX", 1.5),
        ("BlockSizeY", 2.0),
        ("PlaceDepth", 2.0),
        ("PlaceFlip", 9.0),
        ("RouteStyle", 2.0),
        ("LineRouteExt", 2.0),
        ("ResizePage", 1.0),
    ] {
        assert_eq!(page_cell(d, name), Some(want), "{name}");
    }
    // The query says the same; a layout that names nothing keeps the page's own choice.
    let q = run(&mut s, "page.layout", json!({}));
    assert_eq!(
        (q["style"].as_str(), q["direction"].as_str(), q["alignment"].as_str()),
        (Some("hierarchy"), Some("right_to_left"), Some("end")),
        "{q}"
    );
    assert_eq!(
        (q["connector_style"].as_str(), q["appearance"].as_str(), q["enlarge_page"].as_bool()),
        (Some("straight"), Some("curved"), Some(true)),
        "{q}"
    );
    let again = run(&mut s, "page.relayout", json!({}));
    assert_eq!(again["place_style"], 27, "{again}");
    assert_eq!(again["moved"], 0, "{again}");
}

#[test]
fn apply_routing_writes_the_style_to_every_connector_and_a_hand_edited_route_is_made_again() {
    let mut s = blank_session();
    let (_, wires) = chain(&mut s, 3);
    run(&mut s, "page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom", "connector_style": "straight", "apply_routing": true}));
    let d = doc(&s);
    for w in &wires {
        let found = Resolver::new(d)
            .resolve(&SheetRef::Shape(d.find_shape(diacraft_doc::Owner::Page(1), *w).unwrap()), &CellAddr::direct("ShapeRouteStyle"))
            .unwrap();
        assert_eq!(found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()), Some(2.0), "connector {w}");
    }
    // Straight: a connector with two glued ends is one leg from one to the other.
    assert_eq!(route(d, wires[0]).len(), 2, "{:?}", route(d, wires[0]));
}

#[test]
fn a_hostile_call_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    chain(&mut s, 3);
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"style": "tree"}), "`style` \"tree\""),
        (json!({"style": "radial", "direction": "top_to_bottom"}), "a radial layout has none"),
        (json!({"style": "flowchart", "alignment": "start"}), "`alignment` is for a hierarchy"),
        (json!({"spacing": -1.0}), "`spacing` must be a number of inches more than 0"),
        (json!({"spacing": 1.0e300}), "`spacing` must be a number of inches more than 0"),
        (json!({"depth": 4}), "`depth` must be a whole number from 0 to 3"),
        (json!({"flip": 16}), "`flip` must be a whole number from 0 to 15"),
        (json!({"connector_style": "wavy"}), "`connector_style`"),
        (json!({"appearance": "wavy"}), "`appearance`"),
        (json!({"apply_routing": "yes"}), "`apply_routing` must be true or false"),
        (json!({"ids": []}), "`ids`"),
        (json!({"ids": [999]}), "no shape 999"),
        (json!({"page": 7}), "there is no page 7"),
    ] {
        let e = s.execute("page.relayout", &params).unwrap_err().to_string();
        assert!(e.contains(words), "{params}: {e}");
        assert_eq!(snapshot(&s), before, "{params}: a refusal changes nothing");
    }
}

// ---- Auto Space -----------------------------------------------------------------------------------------------------------------

#[test]
fn auto_space_makes_the_gaps_between_columns_equal_to_the_spacing_and_keeps_the_rows() {
    // Shapes 1.5 wide at x = 1, 3, 8 (gaps 0.5 and 3.5) on one row; the space asked is 1: the first stays, the others go to a gap of 1. Their y is kept (a row, so one group along y).
    let mut s = blank_session();
    let shapes: Vec<u32> = [1.0, 3.0, 8.0].iter().map(|x| drop_shape(&mut s, "rectangle", *x, 4.0)).collect();
    let r = run(&mut s, "shape.autospace", json!({"horizontal": 1.0, "vertical": 1.0}));
    assert_eq!((r["columns"].as_u64(), r["rows"].as_u64(), r["moved"].as_u64()), (Some(3), Some(1), Some(2)), "{r}");
    let d = doc(&s);
    let at: Vec<P> = shapes.iter().map(|id| centre(d, *id)).collect();
    assert!(near(at[0].0, 1.0) && near(at[1].0 - at[0].0, 2.5) && near(at[2].0 - at[1].0, 2.5), "gaps of 1 between 1.5 wide: {at:?}");
    assert!(at.iter().all(|p| near(p.1, 4.0)), "the row keeps its y: {at:?}");
    // The same again is no edit.
    let steps = snapshot(&s).undo_steps;
    let again = run(&mut s, "shape.autospace", json!({"horizontal": 1.0, "vertical": 1.0}));
    assert_eq!(again["moved"], 0, "{again}");
    assert_eq!(snapshot(&s).undo_steps, steps);
}

#[test]
fn auto_space_takes_the_pages_spacing_when_none_is_given_and_spacing_options_sets_it() {
    let mut s = blank_session();
    let shapes: Vec<u32> = [1.0, 6.0].iter().map(|x| drop_shape(&mut s, "rectangle", *x, 4.0)).collect();
    let q = run(&mut s, "page.spacing", json!({"horizontal": 2.0, "vertical": 0.5}));
    assert_eq!((q["horizontal"].as_f64(), q["vertical"].as_f64()), (Some(2.0), Some(0.5)), "{q}");
    assert_eq!((page_cell(doc(&s), "AvenueSizeX"), page_cell(doc(&s), "AvenueSizeY")), (Some(2.0), Some(0.5)));
    run(&mut s, "shape.autospace", json!({}));
    let d = doc(&s);
    assert!(
        near(centre(d, shapes[1]).0 - centre(d, shapes[0]).0, 3.5),
        "1.5 wide and a gap of 2: {:?}",
        (centre(d, shapes[0]), centre(d, shapes[1]))
    );
    // A value the page holds is no edit; nothing given, or 0, is refused.
    let steps = snapshot(&s).undo_steps;
    run(&mut s, "page.spacing", json!({"horizontal": 2.0}));
    assert_eq!(snapshot(&s).undo_steps, steps);
    for params in [json!({}), json!({"horizontal": 0.0}), json!({"vertical": -1.0})] {
        assert!(s.execute("page.spacing", &params).is_err(), "{params}");
    }
}

#[test]
fn auto_space_spaces_rows_from_the_top_and_leaves_a_shape_on_a_locked_layer() {
    let mut s = blank_session();
    let shapes: Vec<u32> = [8.0, 6.0, 2.0].iter().map(|y| drop_shape(&mut s, "rectangle", 3.0, *y)).collect();
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [shapes[1]], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let r = run(&mut s, "shape.autospace", json!({"vertical": 1.0}));
    assert_eq!(r["locked"], json!([shapes[1]]), "{r}");
    let d = doc(&s);
    // The top shape stays at 8 (1 high); the locked one is not part of it; the bottom one goes up to a gap of 1 under the top: its middle at 8 - 1 - 1 = 6.
    assert!(
        near(centre(d, shapes[0]).1, 8.0) && near(centre(d, shapes[1]).1, 6.0) && near(centre(d, shapes[2]).1, 6.0),
        "{:?}",
        shapes.iter().map(|id| centre(d, *id)).collect::<Vec<_>>()
    );
}

#[test]
fn avoid_page_breaks_is_a_page_cell_and_auto_space_leaves_no_shape_across_a_break() {
    // The page is two sheets across (20 wide, 7.5 high: one sheet down) with one break at `at`. The second shape is 0.5 from the first; asked for a gap of 2 auto space puts it
    // from `at - 0.25` to `at + 1.25`, across the break, and with Avoid Page Breaks on it goes off it, to the right, the nearer side.
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"width": 20.0, "height": 7.5}));
    let breaks = run(&mut s, "page.breaks", json!({}));
    let x = breaks["x"].as_array().unwrap();
    assert_eq!((x.len(), breaks["y"].as_array().unwrap().len()), (1, 0), "two sheets across and one down, one break: {breaks}");
    let at = x[0].as_f64().unwrap();
    let shapes: Vec<u32> = [at - 3.0, at - 1.0].iter().map(|x| drop_shape(&mut s, "rectangle", *x, 4.0)).collect();
    run(&mut s, "shape.autospace", json!({"horizontal": 2.0}));
    let across = |d: &Document, id: u32| {
        let b = bounds(d, id);
        b.0 < at - 1e-6 && b.2 > at + 1e-6
    };
    assert!(across(doc(&s), shapes[1]), "with no box ticked the second shape is across the break: {:?}", bounds(doc(&s), shapes[1]));
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "page.avoid_page_breaks", json!({"on": true}));
    assert_eq!(r["on"], true);
    assert_eq!(page_cell(doc(&s), "AvoidPageBreaks"), Some(1.0));
    run(&mut s, "shape.autospace", json!({"horizontal": 2.0}));
    let b = bounds(doc(&s), shapes[1]);
    assert!(!across(doc(&s), shapes[1]) && !across(doc(&s), shapes[0]), "{:?}", shapes.iter().map(|id| bounds(doc(&s), *id)).collect::<Vec<_>>());
    assert!(near(b.0, at + 0.05), "the nearer side is the right one, and the break is a twentieth of an inch from the shape: {b:?} at {at}");
}

#[test]
fn move_off_page_breaks_moves_a_shape_across_a_break_to_the_nearer_side_and_leaves_the_rest() {
    let mut s = blank_session();
    run(&mut s, "page.size", json!({"width": 20.0, "height": 7.5}));
    let at = run(&mut s, "page.breaks", json!({}))["x"][0].as_f64().unwrap();
    let across = drop_shape(&mut s, "rectangle", at + 0.2, 4.0);
    let clear = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let r = run(&mut s, "shape.move_off_page_breaks", json!({}));
    assert_eq!((r["shapes"].as_u64(), r["moved"].as_u64(), r["too_large"].as_u64()), (Some(2), Some(1), Some(0)), "{r}");
    let d = doc(&s);
    let b = bounds(d, across);
    assert!(b.0 >= at + 0.04 && b.0 < at + 0.06, "the left edge is a twentieth of an inch right of the break: {b:?} at {at}");
    assert!(near(centre(d, clear).0, 2.0), "the other shape is not touched");
    let steps = snapshot(&s).undo_steps;
    assert_eq!(run(&mut s, "shape.move_off_page_breaks", json!({}))["moved"], 0);
    assert_eq!(snapshot(&s).undo_steps, steps, "nothing to move is no edit");
}

#[test]
fn a_page_that_is_one_sheet_has_no_breaks_and_the_box_is_a_setting_that_is_no_undo_step() {
    let mut s = blank_session();
    let breaks = run(&mut s, "page.breaks", json!({}));
    assert_eq!((breaks["x"].as_array().map(Vec::len), breaks["y"].as_array().map(Vec::len)), (Some(0), Some(0)), "{breaks}");
    assert_eq!(run(&mut s, "page_breaks.settings", json!({}))["on"], false, "a drawing that holds nothing has them off");
    let steps = snapshot(&s).undo_steps;
    let r = run(&mut s, "page_breaks.set", json!({"on": true}));
    assert_eq!((r["on"].as_bool(), r["held"].as_bool()), (Some(true), Some(true)), "{r}");
    assert_eq!(snapshot(&s).undo_steps, steps, "no undo step");
    assert!(s.is_dirty(), "but the drawing is changed");
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "page_breaks.settings", json!({}))["on"], true, "undo leaves the box where it was turned");
    run(&mut s, "page_breaks.set", json!({"on": false}));
    assert!(s.document().unwrap().diacraft.is_none(), "off takes the data out again");
    let e = s.execute("page_breaks.set", &json!({})).unwrap_err().to_string();
    assert!(e.contains("`on`"), "{e}");
}

// ---- Auto Size and the page breaks agree on the sheets -----------------------------------------------------------------------

/// "with Auto Size on, working past the edge extends the page, **page breaks show the sheet divisions**, and **paper size and
/// orientation set those divisions**". A shape put past the edge grows the page by whole sheets of its printer paper (`page.auto_size`), the breaks of the grown page
/// (`page.breaks`) are the borders of those sheets, and the printer paper and the way it is held (`page.paper`) set where they fall.
#[test]
fn auto_size_grows_the_page_by_whole_sheets_and_the_page_breaks_show_their_borders() {
    let mut s = blank_session();
    let one_sheet = run(&mut s, "page.breaks", json!({}));
    assert_eq!(
        (one_sheet["x"].as_array().map(Vec::len), one_sheet["y"].as_array().map(Vec::len)),
        (Some(0), Some(0)),
        "a page that is one sheet has no breaks: {one_sheet}"
    );
    run(&mut s, "page.auto_size", json!({"on": true}));
    // A shape whose box reaches 13 inches across a page 11 wide: the page takes a whole sheet more, and nothing else.
    drop_shape(&mut s, "rectangle", 12.25, 4.0);
    assert_eq!((page_cell(doc(&s), "PageWidth"), page_cell(doc(&s), "PageHeight")), (Some(22.0), Some(8.5)), "a sheet of 11 inches more, whole");
    let landscape = run(&mut s, "page.breaks", json!({}));
    let (x, y) = (landscape["x"].as_array().unwrap(), landscape["y"].as_array().unwrap());
    assert_eq!((x.len(), y.len()), (1, 0), "two sheets across and one down, so one break: {landscape}");
    let at = x[0].as_f64().unwrap();
    assert!(at > 9.0 && at < 11.0 + 1.0e-6, "the break is the border of the first sheet, inside the first 11 inches: {at}");
    // The paper held the other way: the sheets are narrower and taller, so more of them run across the same page.
    run(&mut s, "page.paper", json!({"orientation": "portrait"}));
    let portrait = run(&mut s, "page.breaks", json!({}));
    let across = portrait["x"].as_array().unwrap().len();
    assert!(across > x.len(), "portrait sheets are narrower, so more breaks across the 22 inches: {portrait} against {landscape}");
    assert!(portrait["x"][0].as_f64().unwrap() < at, "and the first falls sooner: {portrait}");
    // The page itself did not change: the paper decides the divisions, not the size.
    assert_eq!(page_cell(doc(&s), "PageWidth"), Some(22.0));
    assert_eq!(check_session(&s), Ok(()));
}
