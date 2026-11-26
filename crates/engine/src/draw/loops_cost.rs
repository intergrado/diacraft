//! The shapes a command names are found in **one walk of the page**, and not one walk each (after the cost class measured): every
//! command below is run on the most ids it takes (4,096, or the cap of its own) spread from the first shape of a page of 30,000 to its last, the selection
//! that costs most to find one by one (each is found after about half the page). Two counts that do not move with the load of the machine hold each to it:
//! the shapes `Document::find_shape` visited (`diacraft_doc::work`: a search of the page from its top for one `ID`), which must be none, and the shapes the
//! walks of `draw/scan.rs` visited (`NODES_WALKED`), which must be a few walks of the page and not one for each shape named. Before the fix each of these
//! commands visited about 58 million shapes in `find_shape` (4,096 searches of half the page), and some twice that.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Document, Shape, ShapeList};
use serde_json::{Value, json};

use super::NODES_WALKED;
use crate::Session;

/// The shapes of the page.
const PAGE: usize = 30_000;

/// The most ids a command takes (`cmd/params.rs`).
const MAX_IDS: usize = 4_096;

/// The walks of the page a command may make, whatever the number of shapes it names: the page its shapes are on, the shapes themselves, and what follows them (the
/// callouts and members of a move, the containers of a delete). The commands below make two to five; a walk for each shape named would be thousands.
const WALKS: usize = 6;

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// What the seed of a page is: the shape each of its copies is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seed {
    Rectangle,
    Connector,
}

/// A drawing whose first page holds `PAGE` shapes, copies of one rectangle or of one connector with free ends (`ID`s 101 and up after the first), made in one
/// edit; with `themed` the page has a theme and an active layer, which a new shape follows and joins. → the session and the `ID`s of the page, in order.
fn page_of(seed: Seed, themed: bool) -> (Session, Vec<u32>) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    match seed {
        Seed::Rectangle => run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0})),
        Seed::Connector => run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 3.0, "y": 2.0}})),
    };
    if themed {
        run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
        run(&mut s, "layer.add", json!({"name": "Live", "active": true}));
    }
    s.edit("build", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<Shape>> = page.shapes.iter().cloned().collect();
        let model = Arc::clone(&shapes[0]);
        for k in 1..PAGE as u32 {
            let mut shape = (*model).clone();
            shape.id = 100 + k;
            shapes.push(Arc::new(shape));
        }
        page.shapes = ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    let ids: Vec<u32> = s.document().unwrap().page(1).unwrap().shapes.iter().map(|shape| shape.id).collect();
    assert_eq!(ids.len(), PAGE);
    (s, ids)
}

/// `count` of the `ID`s spread from the first shape of the page to its last.
fn spread(ids: &[u32], count: usize) -> Vec<u32> {
    let stride = ids.len() / count;
    (0..count).map(|k| ids[k * stride]).collect()
}

/// Starts both counts over.
fn reset() {
    diacraft_doc::work::reset();
    NODES_WALKED.with(|c| c.set(0));
}

/// The shapes `find_shape` visited and the shapes the walks of the page visited, since [`reset`].
fn counts() -> (u64, usize) {
    (diacraft_doc::work::finds_visited(), NODES_WALKED.with(std::cell::Cell::get))
}

/// Runs a command on the session, counted, and takes its edit back (when it made one), so that the next command finds the page as it was.
fn counted(s: &mut Session, id: &str, params: Value) -> (Value, u64, usize) {
    let steps = s.history().map_or(0, |h| h.undo_len());
    reset();
    let answer = run(s, id, params);
    let (finds, walked) = counts();
    if s.history().map_or(0, |h| h.undo_len()) > steps {
        run(s, "edit.undo", json!({}));
    }
    (answer, finds, walked)
}

/// Holds a command to a few walks of the page: no search from the top of the page for a shape, and at most `walks` walks of it in `draw/scan.rs`.
fn assert_one_walk_each(what: &str, finds: u64, walked: usize, walks: usize) {
    assert_eq!(finds, 0, "{what}: no shape was looked for by a walk of the page from its top ({finds} shapes visited)");
    assert!(walked <= walks * PAGE, "{what}: {walked} shapes walked, at most {walks} walks of the page of {PAGE}");
}

#[test]
fn the_arrange_commands_find_the_shapes_they_name_in_one_walk_of_the_page() {
    // `draw/arrange.rs::targets`, which `shape.rotate`, `shape.flip`, `shape.order`, `shape.align` and a resize of several shapes as one go through.
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let chosen = spread(&all, MAX_IDS);
    for (id, params) in [
        ("shape.rotate", json!({"ids": chosen, "by": 15.0})),
        ("shape.flip", json!({"ids": chosen, "axis": "horizontal"})),
        ("shape.order", json!({"ids": chosen, "to": "front"})),
        ("shape.align", json!({"ids": chosen, "how": "left"})),
        ("shape.resize", json!({"ids": chosen, "x": 1.0, "y": 1.0, "width": 30.0, "height": 30.0})),
    ] {
        let (_, finds, walked) = counted(&mut s, id, params);
        assert_one_walk_each(id, finds, walked, WALKS);
    }
    // A shape that is not on the page is refused in the words it was, the first one named that is missing.
    let missing = [chosen[0], 99_999, 99_998];
    for (id, params) in [
        ("shape.rotate", json!({"ids": missing, "by": 15.0, "page": 1})),
        ("shape.flip", json!({"ids": missing, "axis": "vertical", "page": 1})),
        ("shape.order", json!({"ids": missing, "to": "back", "page": 1})),
        ("shape.align", json!({"ids": missing, "how": "top", "page": 1})),
    ] {
        let e = s.execute(id, &params).unwrap_err().to_string();
        assert!(e.contains("page 1 has no shape 99999"), "{id}: {e}");
    }
}

#[test]
fn a_move_and_a_move_of_each_shape_by_its_own_distance_find_the_shapes_in_one_walk_of_the_page() {
    // `draw/shapes.rs::move_shapes_leaving` (`shape.move`) and `move_each` (Align, the fit of a container, a move by the engine).
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let chosen = spread(&all, MAX_IDS);
    let (answer, finds, walked) = counted(&mut s, "shape.move", json!({"ids": chosen, "dx": 0.5, "dy": 0.25}));
    assert_eq!(answer["moved"], MAX_IDS, "{answer}");
    assert_one_walk_each("shape.move", finds, walked, WALKS);
    let mut doc: Document = s.document().unwrap().clone();
    reset();
    assert_eq!(super::move_shapes(&mut doc, 1, &chosen, 1.0, 0.0).unwrap(), MAX_IDS);
    let (finds, walked) = counts();
    assert_one_walk_each("move_each", finds, walked, 2);
    let e = s.execute("shape.move", &json!({"ids": [chosen[0], 99_999], "dx": 1.0, "dy": 0.0, "page": 1})).unwrap_err().to_string();
    assert!(e.contains("page 1 has no shape 99999"), "{e}");
}

#[test]
fn a_delete_a_copy_a_cut_and_a_duplicate_find_the_shapes_they_name_in_one_walk_of_the_page() {
    // `draw/shapes.rs::delete_shapes`, and `draw/container/mod.rs::with_contents` (what a container among the shapes holds), which the four go through.
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let chosen = spread(&all, MAX_IDS);
    let (answer, finds, walked) = counted(&mut s, "shape.delete", json!({"ids": chosen}));
    assert_eq!(answer["deleted"], MAX_IDS, "{answer}");
    assert_one_walk_each("shape.delete", finds, walked, WALKS);
    for id in ["edit.copy", "edit.cut", "shape.duplicate"] {
        let (_, finds, walked) = counted(&mut s, id, json!({"ids": chosen}));
        assert_one_walk_each(id, finds, walked, WALKS);
    }
    let e = s.execute("shape.delete", &json!({"ids": [chosen[0], 99_999], "page": 1})).unwrap_err().to_string();
    assert!(e.contains("page 1 has no shape 99999"), "{e}");
}

#[test]
fn the_formatting_of_the_shapes_named_is_read_with_one_walk_of_the_page() {
    // `formatting/look.rs::look_of_in`, which took the first shape inside each shape named by a search of the page for each.
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let chosen = spread(&all, MAX_IDS);
    let (answer, finds, walked) = counted(&mut s, "shape.formatting", json!({"ids": chosen}));
    assert_eq!(answer["shapes"].as_array().map(Vec::len), Some(MAX_IDS), "one look for each shape named");
    assert_one_walk_each("shape.formatting", finds, walked, WALKS);
    let e = s.execute("shape.formatting", &json!({"ids": [chosen[0], 99_999], "page": 1})).unwrap_err().to_string();
    assert!(e.contains("page 1 has no shape 99999"), "{e}");
}

#[test]
fn a_reset_of_connectors_tells_what_changed_with_one_walk_of_the_page() {
    // `draw/hand.rs::reset_connectors`, which compared each connector before and after by a search of both drawings for it.
    let (mut s, all) = page_of(Seed::Connector, false);
    let chosen = spread(&all, MAX_IDS);
    let (answer, finds, walked) = counted(&mut s, "connector.reset", json!({"ids": chosen}));
    assert_eq!(answer["reset"], MAX_IDS, "{answer}");
    assert_one_walk_each("connector.reset", finds, walked, WALKS);
}

#[test]
fn a_snapped_move_resize_and_place_find_the_box_of_their_shapes_in_one_walk_of_the_page() {
    // `snap/subject.rs::bounds_each`, `box_of` and `own_points`, on the most shapes a snap takes (`MAX_SNAPPED_SHAPES`).
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let few = spread(&all, crate::snap::MAX_SNAPPED_SHAPES);
    for (id, params) in [
        ("shape.move", json!({"ids": few, "dx": 0.5, "dy": 0.25, "snap": true})),
        ("shape.resize", json!({"ids": few, "x": 1.0, "y": 1.0, "width": 30.0, "height": 30.0, "snap": true})),
        ("shape.place", json!({"ids": few, "x": 2.0, "y": 2.0, "snap": true})),
        ("snap.at", json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 2.0, "top": 2.0, "ids": few})),
    ] {
        let (_, finds, walked) = counted(&mut s, id, params);
        assert_one_walk_each(id, finds, walked, WALKS);
    }
    // The box of shapes one of which is not there is none, as it was.
    let doc = s.document().unwrap();
    assert!(crate::snap::box_of(doc, 1, &[few[0], 99_999]).is_none());
    assert!(crate::snap::bounds_each(doc, 1, &[few[0], 99_999]).is_none());
    assert!(crate::snap::box_of(doc, 7, &[few[0]]).is_none(), "a page that is not there");
    // The points a place pulls (each shape's, moved by its own distance) are the points a call for each shape gives, in order, a shape not there giving none.
    let moves: Vec<(u32, crate::snap::P)> = vec![(few[3], (1.0, 0.0)), (99_999, (5.0, 5.0)), (few[200], (0.0, -2.5)), (few[3], (0.25, 0.25))];
    let one_by_one: Vec<crate::snap::P> = moves.iter().flat_map(|(id, by)| crate::snap::own_points(doc, 1, &[*id], *by)).collect();
    assert!(!one_by_one.is_empty(), "a rectangle has connection points");
    reset();
    assert_eq!(crate::snap::own_points_each(doc, 1, &moves), one_by_one);
    assert_one_walk_each("own_points_each", counts().0, counts().1, 1);
    assert!(crate::snap::own_points_each(doc, 7, &moves).is_empty(), "a page that is not there");
}

#[test]
fn a_change_of_shape_finds_the_shapes_it_names_in_one_walk_and_each_new_instance_where_it_was_put() {
    // `draw/change.rs::change_shapes` (the shapes named) and `drop_instance` (the instance of the new master each is given), on the most it changes.
    let (mut s, all) = page_of(Seed::Rectangle, false);
    let few = spread(&all, super::MAX_CHANGED_SHAPES);
    let (answer, finds, walked) = counted(&mut s, "shape.change", json!({"ids": few, "master": "ellipse"}));
    assert_eq!(answer["changed"].as_array().map(Vec::len), Some(few.len()), "{answer}");
    assert_one_walk_each("shape.change", finds, walked, WALKS);
    let e = s.execute("shape.change", &json!({"ids": [few[0], 99_999], "master": "ellipse", "page": 1})).unwrap_err().to_string();
    assert!(e.contains("page 1 has no shape 99999"), "{e}");
}

#[test]
fn callouts_for_many_shapes_follow_the_theme_and_join_the_active_layers_in_one_pass() {
    // `cmd/callout.rs::insert`: each new callout followed the theme (a walk of the page to find it) and joined the active layers (another), one by one.
    let (mut s, all) = page_of(Seed::Rectangle, true);
    let chosen = spread(&all, MAX_IDS);
    diacraft_doc::work::reset();
    NODES_WALKED.with(|c| c.set(0));
    crate::theme::reset_follow_work();
    let answer = run(&mut s, "callout.insert", json!({"ids": chosen}));
    let (finds, walked) = counts();
    let made: Vec<u32> = answer["ids"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect();
    assert_eq!(made.len(), MAX_IDS);
    assert_eq!(crate::theme::follow_work().page_walks, 0, "no walk of the page to find a callout by its ID");
    assert_one_walk_each("callout.insert", finds, walked, WALKS);
    // Every callout follows the theme and is on the active layer, as each one was before.
    let doc = s.document().unwrap();
    let found = super::find_named(doc, 1, &made.iter().copied().collect()).unwrap();
    for id in [made[0], made[MAX_IDS / 2], made[MAX_IDS - 1]] {
        let shape = doc.shape(&found[&id]).unwrap();
        assert!(shape.sheet.links.fill.is_some_and(|style| style != 0), "callout {id} follows the Theme style, not the root style it is made with");
        assert!(shape.sheet.cell("LayerMember").is_some(), "callout {id} joined the active layer");
    }
}

#[test]
fn the_fields_of_the_shapes_named_are_found_in_one_walk_of_the_page() {
    // `draw/fields.rs::refresh` over `Scope::Shapes`, a library call that takes many shapes (the command takes one).
    let (s, all) = page_of(Seed::Rectangle, false);
    let chosen = spread(&all, MAX_IDS);
    let mut doc: Document = s.document().unwrap().clone();
    reset();
    super::fields::refresh(&mut doc, super::fields::Scope::Shapes(1, &chosen), &super::fields::Env::none(), true).unwrap();
    let (finds, walked) = counts();
    assert_one_walk_each("fields::refresh", finds, walked, 1);
    let e = super::fields::refresh(&mut doc, super::fields::Scope::Shapes(1, &[chosen[0], 99_999, 99_998]), &super::fields::Env::none(), true)
        .unwrap_err();
    assert!(e.message.contains("page 1 has no shape 99999"), "{e:?}");
    let e = super::fields::refresh(&mut doc, super::fields::Scope::Shapes(7, &[chosen[0]]), &super::fields::Env::none(), true).unwrap_err();
    assert!(e.message.contains("there is no page 7"), "{e:?}");
}

#[test]
fn a_new_page_style_finds_the_connectors_that_follow_the_page_in_one_walk_of_the_page() {
    // `draw/routing.rs::followers`: every connector of a page of 30,000 follows the page and reroutes freely, so each is asked, and the page is walked for them once, and not
    // once for each. The routing the edit makes after it tries 10,000 of them at the most (`MAX_ROUTED_PER_EDIT`), and the answer says the others.
    let (mut s, _) = page_of(Seed::Connector, false);
    let (answer, finds, walked) = counted(&mut s, "page.set_routing", json!({"style": "straight"}));
    assert_eq!(answer["routed"], PAGE, "every connector of the page was asked: {answer}");
    assert_one_walk_each("page.set_routing", finds, walked, WALKS);
    // The same style again asks nobody, and walks the page not at all.
    run(&mut s, "page.set_routing", json!({"style": "straight"}));
    let (answer, finds, walked) = counted(&mut s, "page.set_routing", json!({"style": "straight"}));
    assert_eq!((answer["routed"].as_u64(), finds, walked), (Some(0), 0, 0), "{answer}");
}

// ---- `sheet.set_cells` -------------------------------------------------------------------------------------------------------------

/// A batch of 4,096 writes to shapes spread over a page of 30,000, named by their `ID` alone and by `page`, finds them in a few walks of the page and not one for each write
/// (the cost is a count). Two counts hold it: no `Document::find_shape` search, and the nodes `draw/scan.rs` walked.
#[test]
fn a_batch_of_sheet_writes_finds_its_shapes_in_a_few_walks_of_the_page() {
    let (mut s, ids) = page_of(Seed::Rectangle, false);
    let named = spread(&ids, MAX_IDS);
    for with_page in [false, true] {
        let writes: Vec<Value> = named
            .iter()
            .map(|id| {
                let mut w = json!({"id": id, "cell": "PinX", "formula": "1"});
                if with_page {
                    w["page"] = json!(1);
                }
                w
            })
            .collect();
        reset();
        run(&mut s, "sheet.set_cells", json!({"writes": writes}));
        let (finds, walked) = counts();
        assert_eq!(finds, 0, "page {with_page}: no search of the page for a shape");
        assert!(walked <= WALKS * PAGE, "page {with_page}: {walked} shapes walked for 4,096 writes");
    }
}
