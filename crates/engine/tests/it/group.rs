//! Group and ungroup: `shape.group` and `shape.ungroup`, through `Session::execute` as the control channel calls it. Numbers are paper
//! inches on pages of the default scale; angles are degrees in the parameters and radians in the cells.
//!
//! What every test of a group holds to is that **a shape draws where it did**: the corners of each member on the page, and the pixels of the page,
//! are the same after a group and an ungroup.

use diacraft_doc::{CellAddr, Document, Lookup, Owner, Resolver, ShapeRef, SheetRef};
use diacraft_engine::draw::{Pose, shape_corners};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::edit_foreign::{Dir, opened};
use crate::{Snapshot, bad_params_msg, blank_session, check_session, run, snapshot};

type P = (f64, f64);

/// How near two numbers of a page, in inches, are the same: a cell holds nine places.
const TOL: f64 = 1.0e-8;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn at(d: &Document, id: u32) -> ShapeRef {
    d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&at(d, id)).unwrap()
}

/// A number a cell directly under the shape at `place` holds.
fn cell(d: &Document, place: &ShapeRef, name: &str) -> f64 {
    let found = Resolver::new(d).resolve(&SheetRef::Shape(place.clone()), &CellAddr::direct(name));
    found.and_then(|f| f.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())).unwrap_or(0.0)
}

fn pose(d: &Document, place: &ShapeRef) -> Pose {
    let n = |name: &str| cell(d, place, name);
    Pose { pin: (n("PinX"), n("PinY")), loc: (n("LocPinX"), n("LocPinY")), angle: n("Angle"), flip_x: n("FlipX") != 0.0, flip_y: n("FlipY") != 0.0 }
}

/// A point of the frame of the shape's parent, on the page: through the poses of the groups the shape is in.
fn parent_to_page(d: &Document, id: u32, p: P) -> P {
    let place = at(d, id);
    (1..place.path.len()).rev().fold(p, |p, k| {
        let group = ShapeRef { owner: place.owner, path: place.path[..k].to_vec() };
        pose(d, &group).up(p)
    })
}

/// Where an end of a connector is on the page: `"Begin"` or `"End"`.
fn end_on_page(d: &Document, id: u32, which: &str) -> P {
    let place = at(d, id);
    parent_to_page(d, id, (cell(d, &place, &format!("{which}X")), cell(d, &place, &format!("{which}Y"))))
}

fn corners(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}"))
}

fn assert_corners(what: &str, d: &Document, id: u32, want: [P; 4]) {
    let got = corners(d, id);
    assert!(got.iter().zip(&want).all(|(g, w)| near_p(*g, *w)), "{what}: shape {id} has corners {got:?}, want {want:?}");
}

/// The box that holds some corners: (left, bottom, right, top).
fn bounds(c: &[[P; 4]]) -> (f64, f64, f64, f64) {
    let all: Vec<P> = c.iter().flat_map(|c| c.iter().copied()).collect();
    (
        all.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        all.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        all.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        all.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    )
}

/// The `ID`s of the page's top-level shapes, bottom first.
fn top(d: &Document) -> Vec<u32> {
    d.page(1).unwrap().shapes.iter().map(|s| s.id).collect()
}

/// The `ID`s of a shape's members.
fn members(d: &Document, id: u32) -> Vec<u32> {
    shape_of(d, id).children.iter().map(|s| s.id).collect()
}

fn local(d: &Document, id: u32, name: &str) -> Option<(Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

fn assert_unchanged(s: &Session, before: &Snapshot, what: &str) {
    let now = snapshot(s);
    assert_eq!(now.document, before.document, "{what}: the document");
    assert_eq!((now.undo_steps, now.redo_steps, now.revision), (before.undo_steps, before.redo_steps, before.revision), "{what}: no step");
}

const PX_PER_INCH: f64 = 100.0;

fn render(s: &Session) -> Rendered {
    let d = s.document().unwrap();
    Renderer::new().render(d, 1, &View::fit_page(d, 1, PX_PER_INCH).unwrap())
}

/// The page's pixels are the same as `before`'s.
fn assert_same_picture(s: &Session, before: &Rendered, what: &str) {
    let now = render(s);
    assert!(now.skipped.is_empty(), "{what}: {:?}", now.skipped);
    assert_eq!((now.width, now.height), (before.width, before.height), "{what}");
    let off = now.rgba_premul.iter().zip(&before.rgba_premul).filter(|(a, b)| a != b).count();
    assert_eq!(off, 0, "{what}: {off} bytes of the picture differ");
}

/// Rectangle 1 at (2, 2), ellipse 2 at (6, 4) and rectangle 3 at (9, 6): none joined.
fn three() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "ellipse", 6.0, 4.0), 2);
    assert_eq!(drop_shape(&mut s, "rectangle", 9.0, 6.0), 3);
    s
}

fn group(s: &mut Session, ids: &[u32]) -> u32 {
    id_of(&run(s, "shape.group", json!({"ids": ids})))
}

// ---- shape.group -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_group_is_a_shape_whose_frame_is_the_box_of_its_members_and_they_draw_where_they_did() {
    let mut s = three();
    let before = render(&s);
    let (c1, c2, c3) = (corners(doc(&s), 1), corners(doc(&s), 2), corners(doc(&s), 3));
    let r = run(&mut s, "shape.group", json!({"ids": [1, 2]}));
    assert_eq!(r, json!({"page": 1, "id": 4, "members": 2}));
    let d = doc(&s);
    let (x0, y0, x1, y1) = bounds(&[c1, c2]);
    // The group: a Group shape holding 1 and 2, in the place of the topmost of them, with the box for its frame and its pin in the middle.
    assert_eq!(shape_of(d, 4).kind, diacraft_doc::ShapeType::Group);
    assert_eq!(top(d), [4, 3], "the group stands where the topmost member stood");
    assert_eq!(members(d, 4), [1, 2]);
    let g = at(d, 4);
    assert!(near(cell(d, &g, "Width"), x1 - x0) && near(cell(d, &g, "Height"), y1 - y0));
    assert!(near(cell(d, &g, "PinX"), (x0 + x1) / 2.0) && near(cell(d, &g, "PinY"), (y0 + y1) / 2.0));
    assert!(near(cell(d, &g, "LocPinX"), (x1 - x0) / 2.0) && near(cell(d, &g, "LocPinY"), (y1 - y0) / 2.0));
    assert_eq!(local(d, 4, "LocPinX").unwrap().0.as_deref(), Some("Width*0.5"));
    assert_eq!(local(d, 4, "LocPinY").unwrap().0.as_deref(), Some("Height*0.5"));
    assert!(local(d, 4, "Angle").is_none(), "a group holds no angle of its own until it is turned");
    // The members' pins are in the group's frame: the page's less the box's lower-left corner.
    assert!(near(num(d, 1, "PinX"), 2.0 - x0) && near(num(d, 1, "PinY"), 2.0 - y0));
    assert!(near(num(d, 2, "PinX"), 6.0 - x0) && near(num(d, 2, "PinY"), 4.0 - y0));
    // And they draw where they did; the shape left out is as it was.
    assert_corners("grouped", d, 1, c1);
    assert_corners("grouped", d, 2, c2);
    assert_corners("left out", d, 3, c3);
    assert_same_picture(&s, &before, "a group draws what its members drew");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_is_one_undo_step_and_undo_takes_back_the_very_document() {
    let mut s = three();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    group(&mut s, &[1, 2]);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let after = doc(&s).clone();
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Group shapes"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
}

#[test]
fn the_group_takes_the_place_of_the_topmost_member_and_the_members_keep_their_order() {
    let mut s = blank_session();
    let ids: Vec<u32> = (0..5).map(|i| drop_shape(&mut s, "rectangle", 1.5 + 2.0 * f64::from(i), 2.0)).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    // Named in any order: 4, 2 and 5 are grouped, 1 and 3 are between them.
    let g = group(&mut s, &[5, 2, 4]);
    let d = doc(&s);
    assert_eq!(members(d, g), [2, 4, 5], "the order of the drawing, not the order named");
    assert_eq!(top(d), [1, 3, g], "the group is where 5, the topmost, was");
    let g2 = group(&mut s, &[1, 3]);
    assert_eq!(top(doc(&s)), [g2, g], "{:?}", top(doc(&s)));
}

#[test]
fn a_group_sends_the_count_it_made_and_a_shape_named_with_its_group_counts_once() {
    let mut s = three();
    let g = group(&mut s, &[1, 2]);
    // The group and a member of it, and another shape: two shapes, since the member goes with the group.
    let r = run(&mut s, "shape.group", json!({"ids": [g, 1, 3]}));
    assert_eq!(r["members"], 2);
    let outer = id_of(&r);
    assert_eq!(members(doc(&s), outer), [g, 3]);
    assert_eq!(members(doc(&s), g), [1, 2], "the inner group is whole");
    // Three shapes named twice over are three.
    let mut s = three();
    let r = run(&mut s, "shape.group", json!({"ids": [3, 1, 2, 3, 1]}));
    assert_eq!(r["members"], 3);
}

#[test]
fn groups_nest_and_a_member_of_a_group_can_be_grouped_with_another_member() {
    let mut s = three();
    let inner = group(&mut s, &[1, 2]);
    let d = doc(&s);
    let (c1, c2) = (corners(d, 1), corners(d, 2));
    // Group the two members of the group with... only themselves is not enough, so add a third rectangle to the group first.
    let outer = group(&mut s, &[inner, 3]);
    assert_eq!(members(doc(&s), outer), [inner, 3]);
    assert_corners("nested", doc(&s), 1, c1);
    assert_corners("nested", doc(&s), 2, c2);
    // Two members of the inner group, grouped inside it: the new group is in the inner group's frame, and the members draw where they did.
    let before = render(&s);
    let deep = group(&mut s, &[1, 2]);
    let d = doc(&s);
    assert_eq!(members(d, inner), [deep], "the new group takes the members' place inside the group");
    assert_eq!(members(d, deep), [1, 2]);
    assert_corners("grouped inside a group", d, 1, c1);
    assert_corners("grouped inside a group", d, 2, c2);
    assert_same_picture(&s, &before, "grouped inside a group");
}

#[test]
fn moving_turning_and_deleting_a_group_carry_its_members() {
    let mut s = three();
    let g = group(&mut s, &[1, 2]);
    let c1 = corners(doc(&s), 1);
    // Moved.
    run(&mut s, "shape.move", json!({"ids": [g], "dx": 1.5, "dy": -0.5}));
    let want: [P; 4] = c1.map(|p| (p.0 + 1.5, p.1 - 0.5));
    assert_corners("moved with the group", doc(&s), 1, want);
    assert!(near(num(doc(&s), 1, "PinX"), num(doc(&s), 1, "PinX")), "the member's own cells are in the group's frame and do not change");
    // Turned a quarter about the group's pin (its middle): the member turns with it.
    let pin = (num(doc(&s), g, "PinX"), num(doc(&s), g, "PinY"));
    let was = corners(doc(&s), 1);
    run(&mut s, "shape.rotate", json!({"ids": [g], "by": 90}));
    let quarter = |p: P| (pin.0 - (p.1 - pin.1), pin.1 + (p.0 - pin.0));
    assert_corners("turned with the group", doc(&s), 1, was.map(quarter));
    // Deleted: the members and the group are gone, and a connector glued to a member keeps a plain end.
    run(&mut s, "shape.delete", json!({"ids": [g]}));
    let d = doc(&s);
    assert_eq!(top(d), [3]);
    assert!(d.find_shape(Owner::Page(1), 1).is_none() && d.find_shape(Owner::Page(1), 2).is_none());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn glue_to_a_member_survives_grouping_and_the_connector_follows_the_group() {
    let mut s = three();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 3}})));
    let before = render(&s);
    let (b0, e0) = (end_on_page(doc(&s), c, "Begin"), end_on_page(doc(&s), c, "End"));
    let g = group(&mut s, &[1, 2]);
    let d = doc(&s);
    // The connector is not in the group; its ends are where they were, still glued, and it still follows its shapes.
    assert!(near_p(end_on_page(d, c, "Begin"), b0) && near_p(end_on_page(d, c, "End"), e0), "ends stay where they were");
    let rows: Vec<(u32, u32)> = d.page(1).unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect();
    assert_eq!(rows, [(c, 1), (c, 3)], "the Connect rows are as they were");
    assert_same_picture(&s, &before, "grouping a glued shape");
    run(&mut s, "shape.move", json!({"ids": [g], "dx": 1.0, "dy": 1.0}));
    let d = doc(&s);
    // The begin end is on the connection point of the member, which moved with the group.
    let corners1 = corners(d, 1);
    let begin = end_on_page(d, c, "Begin");
    assert!(begin.0 >= corners1[0].0 - TOL && begin.0 <= corners1[2].0 + TOL && begin.1 >= corners1[0].1 - TOL && begin.1 <= corners1[2].1 + TOL);
    assert!(near_p(begin, (b0.0 + 1.0, b0.1 + 1.0)), "{begin:?} against {b0:?}");
    assert!(near_p(end_on_page(d, c, "End"), e0), "the other end is where it was: that shape did not move");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_grouped_with_its_shapes_keeps_its_ends_and_follows_the_group() {
    let mut s = three();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 3}})));
    let before = render(&s);
    let (b0, e0) = (end_on_page(doc(&s), c, "Begin"), end_on_page(doc(&s), c, "End"));
    let g = group(&mut s, &[1, 3, c]);
    let d = doc(&s);
    assert_eq!(members(d, g), [1, 3, c]);
    assert!(near_p(end_on_page(d, c, "Begin"), b0) && near_p(end_on_page(d, c, "End"), e0));
    assert_same_picture(&s, &before, "a connector in a group");
    // The glue is by formula and carries the points between frames: still glued, still to the shapes.
    assert_eq!(d.page(1).unwrap().connects.len(), 2);
    assert!(local(d, c, "BeginX").unwrap().0.unwrap().starts_with("PAR(PNT(Sheet.1!"));
    run(&mut s, "shape.move", json!({"ids": [g], "dx": 2.0, "dy": -1.0}));
    let d = doc(&s);
    assert!(near_p(end_on_page(d, c, "Begin"), (b0.0 + 2.0, b0.1 - 1.0)) && near_p(end_on_page(d, c, "End"), (e0.0 + 2.0, e0.1 - 1.0)));
    // Turned with the group: the ends turn with their shapes.
    run(&mut s, "shape.rotate", json!({"ids": [g], "by": 90}));
    let d = doc(&s);
    let corners1 = corners(d, 1);
    let begin = end_on_page(d, c, "Begin");
    assert!(
        begin.0 >= corners1.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) - TOL
            && begin.0 <= corners1.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) + TOL
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_is_given_a_new_id_each_time_and_never_one_that_was_a_shapes() {
    let mut s = blank_session();
    let ids: Vec<u32> = (0..4).map(|i| drop_shape(&mut s, "rectangle", 1.5 + 2.0 * f64::from(i), 2.0)).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
    let a = group(&mut s, &[1, 2]);
    assert_eq!(a, 5);
    run(&mut s, "shape.ungroup", json!({"ids": [a]}));
    let b = group(&mut s, &[3, 4]);
    assert_eq!(b, 6, "5 was a shape's, and is not given again");
    run(&mut s, "shape.delete", json!({"ids": [b]}));
    assert_eq!(drop_shape(&mut s, "rectangle", 1.0, 5.0), 7);
    // Undoing the group and doing it again does not give the same `ID` back either.
    assert_eq!(group(&mut s, &[1, 2]), 8);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(group(&mut s, &[1, 2]), 9);
}

#[test]
fn a_group_is_refused_for_fewer_than_two_shapes_for_shapes_in_different_lists_and_for_a_shape_that_is_not_there() {
    let mut s = three();
    let g = group(&mut s, &[1, 2]);
    let before = snapshot(&s);
    // One shape; the same shape twice; a group and its own member.
    for ids in [json!([3]), json!([3, 3]), json!([g, 1])] {
        let e = error_of(&mut s, "shape.group", &json!({"ids": ids}));
        assert!(bad_params_msg(&e).contains("two shapes"), "{e}");
    }
    // A member of the group and a shape of the page are in different lists.
    let e = error_of(&mut s, "shape.group", &json!({"ids": [1, 3]}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("different groups"), "{e}");
    // A shape that is not on the page; a page that is not there.
    let e = error_of(&mut s, "shape.group", &json!({"ids": [3, 99]}));
    assert!(bad_params_msg(&e).contains("no shape 99"), "{e}");
    let e = error_of(&mut s, "shape.group", &json!({"ids": [3, g], "page": 7}));
    assert!(bad_params_msg(&e).contains("no page 7"), "{e}");
    assert_unchanged(&s, &before, "a refused group");
}

/// The rows of the first `Geometry` section of shape `id`, carried to the frame of its parent.
fn route_in_parent(d: &Document, id: u32) -> Vec<P> {
    let place = at(d, id);
    let sheet = SheetRef::Shape(place.clone());
    let rows = Resolver::new(d).effective_rows(&sheet, &diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap_or_default();
    let own = pose(d, &place);
    rows.iter()
        .map(|row| {
            let value = |name: &str| {
                row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, f)| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
            };
            own.up((value("X"), value("Y")))
        })
        .collect()
}

/// The rows of the first `Geometry` section of shape `id`, carried to the page: through the shape's own pose and its groups'.
fn route_on_page(d: &Document, id: u32) -> Vec<P> {
    let place = at(d, id);
    let sheet = SheetRef::Shape(place.clone());
    let rows = Resolver::new(d).effective_rows(&sheet, &diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap_or_default();
    let own = pose(d, &place);
    rows.iter()
        .map(|row| {
            let value = |name: &str| {
                row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, f)| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
            };
            parent_to_page(d, id, own.up((value("X"), value("Y"))))
        })
        .collect()
}

#[test]
fn a_connector_inside_a_group_that_is_turned_or_flipped_stays_on_the_shapes_outside_it_that_it_is_glued_to() {
    for (by, axis) in [(90.0, None), (-30.0, None), (0.0, Some("vertical")), (0.0, Some("horizontal")), (45.0, Some("horizontal"))] {
        let mut s = three();
        let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 3}})));
        // The connector is in the group and its two shapes are not.
        let g = group(&mut s, &[2, c]);
        if by != 0.0 {
            run(&mut s, "shape.rotate", json!({"ids": [g], "by": by}));
        }
        if let Some(axis) = axis {
            run(&mut s, "shape.flip", json!({"ids": [g], "axis": axis}));
        }
        let d = doc(&s);
        let on_a_point = |shape: u32, p: P| diacraft_engine::draw::connection_points(d, 1, shape).iter().any(|c| near_p((c.x, c.y), p));
        let (begin, finish) = (end_on_page(d, c, "Begin"), end_on_page(d, c, "End"));
        assert!(on_a_point(1, begin), "{by} {axis:?}: the begin {begin:?} is on a point of shape 1");
        assert!(on_a_point(3, finish), "{by} {axis:?}: the end {finish:?} is on a point of shape 3");
        // And its route runs from one end to the other, as it was made for the ends it has.
        let route = route_on_page(d, c);
        assert!(
            near_p(route[0], begin) && near_p(route[route.len() - 1], finish),
            "{by} {axis:?}: the route {route:?} runs from {begin:?} to {finish:?}"
        );
        // In the group's frame, where the connector's cells are, its legs are along the axes, as a route of ours has them.
        let in_group = route_in_parent(d, c);
        assert!(
            in_group.windows(2).all(|w| near(w[0].0, w[1].0) || near(w[0].1, w[1].1)),
            "{by} {axis:?}: the legs {in_group:?} are slanted in the group's frame"
        );
        assert_eq!(check_session(&s), Ok(()));
    }
}

// ---- shape.ungroup ---------------------------------------------------------------------------------------------------------------

#[test]
fn ungrouping_gives_the_members_back_to_the_page_where_they_draw_and_in_the_place_of_the_group() {
    let mut s = three();
    let g = group(&mut s, &[1, 2]);
    let (c1, c2, c3) = (corners(doc(&s), 1), corners(doc(&s), 2), corners(doc(&s), 3));
    let before = render(&s);
    let r = run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    assert_eq!(r, json!({"page": 1, "ungrouped": 1, "ids": [1, 2]}));
    let d = doc(&s);
    assert_eq!(top(d), [1, 2, 3], "the members stand where the group stood, in their order");
    assert!(d.find_shape(Owner::Page(1), g).is_none());
    for (id, c) in [(1, c1), (2, c2), (3, c3)] {
        assert_corners("ungrouped", d, id, c);
    }
    // The pins are the page's again.
    assert!(near(num(d, 1, "PinX"), 2.0) && near(num(d, 1, "PinY"), 2.0) && near(num(d, 2, "PinX"), 6.0));
    assert_same_picture(&s, &before, "ungroup");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Ungroup shapes"));
    // Group and ungroup again: the round trip leaves the cells as they were.
    let g2 = group(&mut s, &[1, 2]);
    run(&mut s, "shape.ungroup", json!({"ids": [g2]}));
    assert!(near(num(doc(&s), 1, "PinX"), 2.0) && near(num(doc(&s), 2, "PinY"), 4.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn ungrouping_a_turned_and_flipped_group_carries_the_turn_and_the_flips_to_the_members() {
    for (turn, flip) in [(30.0, None), (90.0, Some("horizontal")), (-50.0, Some("vertical")), (0.0, Some("horizontal")), (135.0, Some("vertical"))] {
        let mut s = three();
        let g = group(&mut s, &[1, 2]);
        if turn != 0.0 {
            run(&mut s, "shape.rotate", json!({"ids": [g], "by": turn}));
        }
        if let Some(axis) = flip {
            run(&mut s, "shape.flip", json!({"ids": [g], "axis": axis}));
        }
        let (c1, c2) = (corners(doc(&s), 1), corners(doc(&s), 2));
        let before = render(&s);
        run(&mut s, "shape.ungroup", json!({"ids": [g]}));
        let what = format!("turned {turn}, flipped {flip:?}");
        for (id, c) in [(1, c1), (2, c2)] {
            assert_corners(&what, doc(&s), id, c);
        }
        assert_same_picture(&s, &before, &what);
        // The members are shapes of the page now: they turn and flip like any.
        run(&mut s, "shape.rotate", json!({"ids": [1], "by": 10}));
        run(&mut s, "shape.flip", json!({"ids": [2], "axis": "horizontal"}));
        assert_eq!(check_session(&s), Ok(()), "{what}");
    }
}

#[test]
fn a_connector_in_a_turned_group_comes_out_where_it_was_drawn_and_one_in_a_flipped_group_is_refused() {
    for by in [35.0, 90.0, -120.0] {
        let mut s = three();
        let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
        let g = group(&mut s, &[1, 2, c]);
        run(&mut s, "shape.rotate", json!({"ids": [g], "by": by}));
        let (c1, c2) = (corners(doc(&s), 1), corners(doc(&s), 2));
        let (b, e) = (end_on_page(doc(&s), c, "Begin"), end_on_page(doc(&s), c, "End"));
        let route = route_on_page(doc(&s), c);
        let before = render(&s);
        run(&mut s, "shape.ungroup", json!({"ids": [g]}));
        let d = doc(&s);
        assert_corners("turned", d, 1, c1);
        assert_corners("turned", d, 2, c2);
        assert!(near_p(end_on_page(d, c, "Begin"), b) && near_p(end_on_page(d, c, "End"), e), "{by}: the ends");
        let now = route_on_page(d, c);
        assert!(now.len() == route.len() && now.iter().zip(&route).all(|(a, b)| near_p(*a, *b)), "{by}: the route {now:?} against {route:?}");
        assert_same_picture(&s, &before, &format!("a connector out of a group turned {by}"));
        assert_eq!(check_session(&s), Ok(()));
    }
    // Flipped: no connector is carried out of it yet, and nothing changes.
    let mut s = three();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
    let g = group(&mut s, &[1, 2, c]);
    run(&mut s, "shape.flip", json!({"ids": [g], "axis": "horizontal"}));
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.ungroup", &json!({"ids": [g]}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("flipped group"), "{e}");
    assert_unchanged(&s, &before, "a refused ungroup");
}

#[test]
fn a_member_of_a_turned_group_cannot_be_turned_or_flipped_yet_and_can_once_it_is_out() {
    let mut s = three();
    let g = group(&mut s, &[1, 2]);
    run(&mut s, "shape.rotate", json!({"ids": [g], "by": 90}));
    let before = snapshot(&s);
    for (cmd, params) in [("shape.rotate", json!({"ids": [1], "by": 10})), ("shape.flip", json!({"ids": [1], "axis": "vertical"}))] {
        let e = error_of(&mut s, cmd, &params);
        assert!(e.to_string().contains("turned or flipped group"), "{e}");
    }
    assert_unchanged(&s, &before, "refused");
    // A new group inside the turned group does not lift the rule: its members are members of the turned group still.
    let inner = group(&mut s, &[1, 2]);
    assert_eq!(members(doc(&s), g), [inner]);
    let e = error_of(&mut s, "shape.rotate", &json!({"ids": [1], "by": 10}));
    assert!(e.to_string().contains("turned or flipped group"), "{e}");
    // Taking the groups apart brings the shapes to the page with the turn in their own cells, and then they turn.
    run(&mut s, "shape.ungroup", json!({"ids": [inner]}));
    let e = error_of(&mut s, "shape.flip", &json!({"ids": [2], "axis": "horizontal"}));
    assert!(e.to_string().contains("turned or flipped group"), "{e}");
    run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    run(&mut s, "shape.rotate", json!({"ids": [1], "by": 10}));
    run(&mut s, "shape.flip", json!({"ids": [2], "axis": "horizontal"}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn ungrouping_one_level_leaves_a_group_inside_whole_and_a_group_named_with_its_group_is_left_to_it() {
    let mut s = three();
    let inner = group(&mut s, &[1, 2]);
    let outer = group(&mut s, &[inner, 3]);
    let (c1, c2, c3) = (corners(doc(&s), 1), corners(doc(&s), 2), corners(doc(&s), 3));
    let r = run(&mut s, "shape.ungroup", json!({"ids": [outer, inner]}));
    assert_eq!(r["ungrouped"], 1, "the inner group is left to the outer one: one level");
    let d = doc(&s);
    assert_eq!(top(d), [inner, 3]);
    assert_eq!(members(d, inner), [1, 2]);
    for (id, c) in [(1, c1), (2, c2), (3, c3)] {
        assert_corners("one level", d, id, c);
    }
    let r = run(&mut s, "shape.ungroup", json!({"ids": [inner]}));
    assert_eq!(r["ids"], json!([1, 2]));
    assert_eq!(top(doc(&s)), [1, 2, 3]);
}

#[test]
fn several_groups_are_ungrouped_in_one_step_in_any_list() {
    let mut s = blank_session();
    let ids: Vec<u32> = (0..8).map(|i| drop_shape(&mut s, "rectangle", 1.5 + 1.2 * f64::from(i), 1.0 + f64::from(i % 3))).collect();
    let pictures: Vec<[P; 4]> = ids.iter().map(|id| corners(doc(&s), *id)).collect();
    let a = group(&mut s, &ids[0..2]);
    let b = group(&mut s, &ids[3..5]);
    let c = group(&mut s, &ids[6..8]);
    assert_eq!(top(doc(&s)), [a, 3, b, 6, c]);
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.ungroup", json!({"ids": [c, a, b, 3]}));
    assert_eq!(r, json!({"page": 1, "ungrouped": 3, "ids": [1, 2, 4, 5, 7, 8]}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one step for all three");
    assert_eq!(top(doc(&s)), [1, 2, 3, 4, 5, 6, 7, 8]);
    for (id, c) in ids.iter().zip(&pictures) {
        assert_corners("ungrouped", doc(&s), *id, *c);
    }
    // Groups in different lists: one inside a group, one on the page.
    let mut s = blank_session();
    let ids: Vec<u32> = (0..6).map(|i| drop_shape(&mut s, "rectangle", 1.5 + 1.2 * f64::from(i), 2.0)).collect();
    let pictures: Vec<[P; 4]> = ids.iter().map(|id| corners(doc(&s), *id)).collect();
    let a = group(&mut s, &[1, 2]);
    let b = group(&mut s, &[3, 4]);
    let o = group(&mut s, &[a, 5]);
    let before = render(&s);
    let r = run(&mut s, "shape.ungroup", json!({"ids": [a, b]}));
    assert_eq!(r["ungrouped"], 2);
    assert_eq!(members(doc(&s), o), [1, 2, 5]);
    assert_eq!(top(doc(&s)), [3, 4, o, 6]);
    for (id, c) in ids.iter().zip(&pictures) {
        assert_corners("ungrouped in two lists", doc(&s), *id, *c);
    }
    assert_same_picture(&s, &before, "ungrouped in two lists");
}

#[test]
fn ungrouping_a_shape_that_is_not_a_group_changes_nothing_and_is_no_undo_step() {
    let mut s = three();
    let before = snapshot(&s);
    let r = run(&mut s, "shape.ungroup", json!({"ids": [1, 2]}));
    assert_eq!(r, json!({"page": 1, "ungrouped": 0, "ids": []}));
    assert_unchanged(&s, &before, "nothing to ungroup");
    assert!(matches!(error_of(&mut s, "shape.ungroup", &json!({"ids": [42]})), EngineError::BadParams { .. }));
}

/// A group (10, with members 11 and 12), a connector (20) glued at its end to the whole of the group, and a free shape (13), as a file holds them.
fn drawing_with_a_group() -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
    let member = |id: u32, x: f64| {
        shape(
            id,
            &format!(
                r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
            ),
        )
    };
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/><Shapes>{}{}</Shapes>"#,
            member(11, 1.0),
            member(12, 3.0)
        ),
    );
    let connector = shape(
        20,
        r#"<Cell N="BeginX" V="1"/><Cell N="BeginY" V="8"/><Cell N="EndX" V="5"/><Cell N="EndY" V="6"/><Cell N="PinX" V="3" F="(BeginX+EndX)/2"/><Cell N="PinY" V="7" F="(BeginY+EndY)/2"/><Cell N="Width" V="4.4721359" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2.2360679" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="-0.4636476" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>"#,
    );
    let other = member(13, 9.0);
    let connects = r#"<Connect FromSheet="20" FromCell="EndX" FromPart="12" ToSheet="10" ToCell="PinX" ToPart="3"/>"#;
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{group}{connector}{other}")).connects(connects)).build()
}

#[test]
fn a_connector_glued_to_the_group_itself_is_left_with_a_plain_end_when_it_is_ungrouped() {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, drawing_with_a_group()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let d = doc(&s);
    assert_eq!(d.page(1).unwrap().connects.len(), 1);
    let (c11, c12) = (corners(d, 11), corners(d, 12));
    // A member of a group in a file has its pin in the group's frame: the group's pin (5, 5) less its local pin (2, 1) puts the origin at (3, 4).
    assert_corners("a member of a group in a file", d, 11, [(3.5, 4.5), (4.5, 4.5), (4.5, 5.5), (3.5, 5.5)]);
    assert_corners("a member of a group in a file", d, 12, [(5.5, 4.5), (6.5, 4.5), (6.5, 5.5), (5.5, 5.5)]);
    let r = run(&mut s, "shape.ungroup", json!({"ids": [10]}));
    assert_eq!(r, json!({"page": 1, "ungrouped": 1, "ids": [11, 12]}));
    let d = doc(&s);
    assert!(d.page(1).unwrap().connects.is_empty(), "no row names the group that is gone");
    assert!(near(num(d, 20, "EndX"), 5.0) && near(num(d, 20, "EndY"), 6.0), "the end is a point where it was");
    assert_eq!(local(d, 20, "EndX").unwrap().0, None, "and no longer has a formula");
    assert_eq!(local(d, 20, "BeginX").unwrap().0, None);
    assert_corners("ungrouped", d, 11, c11);
    assert_corners("ungrouped", d, 12, c12);
    // The members came out in the place of the group: before the connector and the other shape, which are as they were.
    assert_eq!(top(d), [11, 12, 20, 13]);
    // Grouping the connector and the free shape: the ends and the pose of each are carried, and the connector draws between the same points.
    let (b0, e0) = (end_on_page(d, 20, "Begin"), end_on_page(d, 20, "End"));
    let g = group(&mut s, &[20, 13]);
    let d = doc(&s);
    assert!(near_p(end_on_page(d, 20, "Begin"), b0) && near_p(end_on_page(d, 20, "End"), e0));
    assert_eq!(members(d, g), [20, 13]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_of_shapes_joined_by_a_connector_is_saved_and_read_back_where_it_was() {
    let mut s = three();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
    let g = group(&mut s, &[1, 2, c]);
    let dir = Dir::new();
    let path = dir.path("grouped.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let (a, b) = (doc(&s), doc(&t));
    assert_eq!(members(b, g), [1, 2, c]);
    for id in [1, 2, c] {
        let (x, y) = (corners(a, id), corners(b, id));
        assert!(x.iter().zip(&y).all(|(p, q)| near_p(*p, *q)), "shape {id} was saved and read where it was");
    }
    assert!(near_p(end_on_page(b, c, "Begin"), end_on_page(a, c, "Begin")) && near_p(end_on_page(b, c, "End"), end_on_page(a, c, "End")));
    // The reopened drawing ungroups to the same picture.
    let picture = render(&t);
    run(&mut t, "shape.ungroup", json!({"ids": [g]}));
    assert_same_picture(&t, &picture, "ungroup after a reopen");
    // And a drawing saved again after it holds the members on the page.
    let again = dir.path("again.vsdx");
    run(&mut t, "file.save_as", json!({"path": again}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": again}));
    assert_eq!(top(doc(&u)), [3, 1, 2, c], "the members stand where the group stood, after the shape that was before it");
}

// ---- a drawing from a file -------------------------------------------------------------------------------------------------------

#[test]
fn a_file_connector_glued_by_formulas_and_one_glued_to_the_whole_of_a_shape_keep_their_ends_when_grouped_and_follow_the_group() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let d = doc(&s);
    let pictures: Vec<(u32, [P; 4])> = [10, 11, 12, 13].iter().map(|id| (*id, corners(d, *id))).collect();
    let ends = |d: &Document, id: u32| (end_on_page(d, id, "Begin"), end_on_page(d, id, "End"));
    let (e20, e21, e22, e23) = (ends(d, 20), ends(d, 21), ends(d, 22), ends(d, 23));
    let (begin_formula, trigger) = (local(d, 21, "BeginX").unwrap().0, local(d, 21, "BegTrigger").unwrap().0);
    assert_eq!(begin_formula.as_deref(), Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"));
    // 20 joins 10 and 11 by points (PAR); 21 joins 12 and 13 by the whole of each (_WALKGLUE).
    let g1 = group(&mut s, &[10, 11, 20]);
    let g2 = group(&mut s, &[12, 13, 21]);
    let d = doc(&s);
    for (id, c) in &pictures {
        assert_corners("grouped", d, *id, *c);
    }
    let (n20, n21) = (ends(d, 20), ends(d, 21));
    assert!(near_p(n20.0, e20.0) && near_p(n20.1, e20.1), "20 {n20:?} against {e20:?}");
    assert!(near_p(n21.0, e21.0) && near_p(n21.1, e21.1), "21 {n21:?} against {e21:?}: the end the file put is kept");
    // The formulas are as the file had them, and so are the Connect rows.
    assert_eq!(local(d, 21, "BeginX").unwrap().0, begin_formula);
    assert_eq!(local(d, 21, "BegTrigger").unwrap().0, trigger);
    assert!(local(d, 20, "BeginX").unwrap().0.unwrap().starts_with("PAR(PNT(Sheet.10!"));
    assert_eq!(d.page(1).unwrap().connects.len(), 8);
    // A group moves with everything in it, and the connectors that stay on the page follow the shapes in the groups.
    run(&mut s, "shape.move", json!({"ids": [g1, g2], "dx": 1.0, "dy": -1.0}));
    let d = doc(&s);
    for (id, was) in [(20, e20), (21, e21)] {
        let now = ends(d, id);
        assert!(near_p(now.0, (was.0.0 + 1.0, was.0.1 - 1.0)) && near_p(now.1, (was.1.0 + 1.0, was.1.1 - 1.0)), "{id}: {now:?} against {was:?}");
    }
    // 22 is fixed and glued to points of 10 and 12; 23 is glued to a point of 11 and to the whole of 13.
    for (id, was) in [(22, e22), (23, e23)] {
        let now = ends(d, id);
        assert!(near_p(now.0, (was.0.0 + 1.0, was.0.1 - 1.0)) && near_p(now.1, (was.1.0 + 1.0, was.1.1 - 1.0)), "{id}: {now:?} against {was:?}");
    }
    let moved = (ends(d, 20), ends(d, 21));
    run(&mut s, "shape.ungroup", json!({"ids": [g1, g2]}));
    let d = doc(&s);
    assert_eq!(top(d), [10, 11, 20, 12, 13, 21, 22, 23]);
    assert_eq!((ends(d, 20), ends(d, 21)), moved, "ungrouping leaves the ends where they are");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_file_with_a_group_is_saved_and_read_back_with_the_groups_it_was_given() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let g = group(&mut s, &[10, 11, 20]);
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    assert_eq!(members(doc(&t), g), [10, 11, 20]);
    for id in [10, 11, 12, 13] {
        let (a, b) = (corners(doc(&s), id), corners(doc(&t), id));
        assert!(a.iter().zip(&b).all(|(p, q)| near_p(*p, *q)), "{id}");
    }
}

/// What a cell of shape `id` on page 1 holds: its formula, its cached value and its error, if it has the cell.
fn cell_of(d: &Document, id: u32, name: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => {
            Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()), c.error.as_deref().map(str::to_owned)))
        }
        _ => None,
    }
}

/// A group (10) holding a box (11) whose `Width` is a quarter of the group's, a group (12) with a box (13) whose `Height` is half the group 10's and a box
/// (14) that names group 12, and a box (15) on the page that names group 10.
fn drawing_whose_members_name_their_group() -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
    let boxed = |id: u32, x: f64, extra: &str| {
        shape(id, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>{extra}"#))
    };
    let named = |id: u32, x: f64, width: &str, height: &str| shape(id, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="1"/>{width}{height}"#));
    let inner = shape_with(
        12,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/><Shapes>{}{}</Shapes>"#,
            named(13, 0.5, r#"<Cell N="Width" V="1"/>"#, r#"<Cell N="Height" V="1" F="Sheet.10!Height*0.5"/>"#),
            named(14, 0.5, r#"<Cell N="Width" V="1" F="Sheet.12!Width"/>"#, r#"<Cell N="Height" V="1"/>"#),
        ),
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Shapes>{}{inner}</Shapes>"#,
            named(11, 1.0, r#"<Cell N="Width" V="1" F="Sheet.10!Width*0.25"/>"#, r#"<Cell N="Height" V="1"/>"#),
        ),
    );
    let outside = boxed(15, 9.0, r#"<Cell N="LocPinX" V="2" F="Sheet.10!Width*0.5"/>"#);
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{group}{outside}"))).build()
}

#[test]
fn a_member_whose_formula_names_the_group_that_is_taken_apart_keeps_the_value_and_loses_the_formula() {
    let dir = Dir::new();
    let path = dir.path("names.vsdx");
    std::fs::write(&path, drawing_whose_members_name_their_group()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(cell_of(doc(&s), 11, "Width").unwrap().0.as_deref(), Some("Sheet.10!Width*0.25"));
    run(&mut s, "shape.ungroup", json!({"ids": [10]}));
    let d = doc(&s);
    // The member's cell, and one of a shape inside a group that was a member: the constants of their values, with no error from a shape that is gone.
    assert_eq!(cell_of(d, 11, "Width"), Some((None, Some("1".to_owned()), None)));
    assert_eq!(cell_of(d, 13, "Height"), Some((None, Some("1".to_owned()), None)));
    // A name of a group that stays is a name still, and the cells that name nothing are as they were.
    assert_eq!(cell_of(d, 14, "Width"), Some((Some("Sheet.12!Width".to_owned()), Some("1".to_owned()), None)));
    assert_eq!(cell_of(d, 11, "Height").unwrap().0, None);
    // A shape outside the group that names it is another matter (as after a delete): it reads a shape that is not there.
    assert!(cell_of(d, 15, "LocPinX").unwrap().2.is_some(), "{:?}", cell_of(d, 15, "LocPinX"));
    // Saved and read again, no member names a group that is not there.
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    let d = doc(&t);
    assert_eq!(cell_of(d, 11, "Width"), Some((None, Some("1".to_owned()), None)));
    assert_eq!(cell_of(d, 13, "Height"), Some((None, Some("1".to_owned()), None)));
    assert_eq!(top(d), [11, 12, 15]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A master (7) of a group (4) holding a group (5) holding a box (6), and an instance (10) of it: 11 stands for 5 and 12 for 6.
fn drawing_with_a_master_group_of_groups() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
    let leaf = shape(
        6,
        r#"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/>"#,
    );
    let middle = shape_with(
        5,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/><Shapes>{leaf}</Shapes>"#
        ),
    );
    let top = shape_with(
        4,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="1"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Shapes>{middle}</Shapes>"#
        ),
    );
    let instance = shape_with(
        10,
        r#" Master="7" Type="Group""#,
        r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Shapes><Shape ID="11" MasterShape="5" Type="Group"><Shapes><Shape ID="12" MasterShape="6" Type="Shape"/></Shapes></Shape></Shapes>"#,
    );
    VsdxBuilder::new().master(MasterSpec::new(7, "Pair").shapes(&top)).page(PageSpec::new(1, "Page-1").shapes(&instance)).build()
}

#[test]
fn a_group_inside_an_instance_of_a_master_is_not_taken_apart_either() {
    let dir = Dir::new();
    let path = dir.path("master.vsdx");
    std::fs::write(&path, drawing_with_a_master_group_of_groups()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let before = snapshot(&s);
    for (id, words) in [(10, "instance of a master"), (11, "made from a master")] {
        let e = error_of(&mut s, "shape.ungroup", &json!({"ids": [id]}));
        assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains(words), "{id}: {e}");
        assert_unchanged(&s, &before, "a refused ungroup");
    }
    // Named with others that are not groups of a master, it is still refused, and nothing of the others is done.
    let e = error_of(&mut s, "shape.ungroup", &json!({"ids": [11, 12]}));
    assert!(e.to_string().contains("made from a master"), "{e}");
    assert_unchanged(&s, &before, "a refused ungroup");
}

#[test]
fn a_page_that_holds_an_id_twice_has_the_members_found_by_where_they_went_and_not_by_the_id() {
    // Shape 3 at the top of the page and a member of group 2 with the same `ID`. The shape on the page holds a cell with
    // a stale cached value that this evaluator would change if it were worked out; taking the group apart works out the member's cells and no other's.
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
    let member = |id: u32| shape(id, r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let other = shape(3, r#"<Cell N="PinX" V="7" F="1+1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1" F="Sheet.9!Width"/><Cell N="Height" V="1"/>"#);
    let group = shape_with(
        2,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Shapes>{}{}</Shapes>"#,
            member(3),
            member(4)
        ),
    );
    let dir = Dir::new();
    let path = dir.path("twice.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{other}{group}"))).build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let r = run(&mut s, "shape.ungroup", json!({"ids": [2]}));
    assert_eq!(r["ids"], json!([3, 4]));
    let page = doc(&s).page(1).unwrap();
    let ids: Vec<u32> = page.shapes.iter().map(|shape| shape.id).collect();
    assert_eq!(ids, [3, 3, 4]);
    // The shape that was on the page is as it was, its stale cached value included: it was not worked out.
    let first = &page.shapes.iter().next().unwrap().sheet;
    let pin_x = |sheet: &diacraft_doc::Sheet| match sheet.lookup(&CellAddr::direct("PinX")) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    };
    assert_eq!(pin_x(first).as_deref(), Some("7"));
    // The member that came out was carried by the group's pose (its pin was (1, 1) in a frame at (5, 5)).
    let second = &page.shapes.get(1).unwrap().sheet;
    assert_eq!(pin_x(second).as_deref(), Some("6"));
    assert_eq!(check_session(&s), Ok(()));
}

/// A group (10) holding a box (11) with three cells that name the group and are not plain (`Width` with a cached error, `Height` with no cached value,
/// `LocPinX` guarded) and a connector (12) whose begin cells name the group and hold an error.
fn drawing_whose_members_freeze_badly() -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
    let boxed = shape(
        11,
        r##"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1" F="Sheet.10!Width*0.25" E="#REF!"/><Cell N="Height" F="Sheet.10!Height"/>
<Cell N="LocPinX" V="0.5" F="GUARD(Sheet.10!Width*0.125)"/>"##,
    );
    let connector = shape(
        12,
        r##"<Cell N="BeginX" V="1" F="PAR(PNT(Sheet.10!PinX,Sheet.10!PinY))" E="#REF!"/><Cell N="BeginY" V="1" F="PAR(PNT(Sheet.10!PinX,Sheet.10!PinY))" E="#REF!"/>
<Cell N="EndX" V="3"/><Cell N="EndY" V="1"/><Cell N="PinX" V="2" F="(BeginX+EndX)/2"/><Cell N="PinY" V="1" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>"##,
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Shapes>{boxed}{connector}</Shapes>"#
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&group)).build()
}

#[test]
fn a_member_cell_made_its_value_by_an_ungroup_has_no_error_keeps_its_formula_with_no_value_and_loses_a_guard() {
    let dir = Dir::new();
    let path = dir.path("freeze.vsdx");
    std::fs::write(&path, drawing_whose_members_freeze_badly()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(cell_of(doc(&s), 11, "Width"), Some((Some("Sheet.10!Width*0.25".to_owned()), Some("1".to_owned()), Some("#REF!".to_owned()))));
    run(&mut s, "shape.ungroup", json!({"ids": [10]}));
    let d = doc(&s);
    // The cached error goes with the formula: a constant reports none, and the group that went is not read for a new one.
    assert_eq!(cell_of(d, 11, "Width"), Some((None, Some("1".to_owned()), None)));
    // No cached value, nothing to be a constant of: the formula stays, and no empty cell (which would read as 0) is made.
    assert_eq!(cell_of(d, 11, "Height").map(|c| c.0), Some(Some("Sheet.10!Height".to_owned())));
    // The guard goes with the formula.
    assert_eq!(cell_of(d, 11, "LocPinX"), Some((None, Some("0.5".to_owned()), None)));
    assert!(!shape_of(d, 11).sheet.cell("LocPinX").unwrap().is_guarded());
    // The ends of the connector: plain points without errors, carried out of the group like any, so a later reroute is not skipped for it.
    for cell in ["BeginX", "BeginY"] {
        let (formula, _, error) = cell_of(d, 12, cell).unwrap();
        assert_eq!((formula, error), (None, None), "{cell}");
    }
    assert!(
        (num(d, 12, "BeginX") - 4.0).abs() < 1e-9 && (num(d, 12, "BeginY") - 5.0).abs() < 1e-9,
        "carried by the group's pose: {} {}",
        num(d, 12, "BeginX"),
        num(d, 12, "BeginY")
    );
    assert_eq!(check_session(&s), Ok(()));
}
