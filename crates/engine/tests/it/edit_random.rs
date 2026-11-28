//! Random drawing: a few hundred editing commands in a row, with ids that are not there, junk between them and undo and redo, and after
//! each one the drawing is still consistent: every `Connect` row names shapes that exist, every glued end is on its connection point (or, glued to the
//! whole of a shape that has none, on its box), every route runs from the begin to the end in horizontal and vertical legs. At the end undo takes back
//! every step to the blank page.
//!
//! The generator is a fixed-seed xorshift, so a failure repeats.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SectionKey, SheetRef};
use diacraft_engine::draw::{Pose, connection_points, shape_corners};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::{blank_session, check_session};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn coord(&mut self) -> f64 {
        (self.below(1_000) as f64) / 100.0
    }

    /// A shape id: one that is there, most of the time, and a number that may not be, some of it.
    fn id(&mut self, live: &[u32]) -> u64 {
        if live.is_empty() || self.below(7) == 0 { 1 + self.below(30) } else { u64::from(live[self.below(live.len() as u64) as usize]) }
    }
}

const MASTERS: [&str; 12] = [
    "rectangle",
    "square",
    "ellipse",
    "circle",
    "triangle",
    "right triangle",
    "diamond",
    "pentagon",
    "hexagon",
    "octagon",
    "parallelogram",
    "trapezoid",
];

fn end(r: &mut Rng, live: &[u32]) -> Value {
    match r.below(5) {
        0 => json!({"x": r.coord(), "y": r.coord()}),
        1 => json!({"id": r.id(live), "point": r.below(5)}),
        _ => json!({"id": r.id(live)}),
    }
}

/// An arrange command on shapes that may not be there, with parameters that may be wrong now and then.
fn arrange_command(r: &mut Rng, live: &[u32]) -> (&'static str, Value) {
    let ids = json!([r.id(live), r.id(live), r.id(live)]);
    match r.below(8) {
        0 | 1 => ("shape.rotate", json!({"ids": ids, "by": (r.below(721) as f64) - 360.0})),
        2 => ("shape.rotate", json!({"ids": ids, "angle": r.below(360)})),
        3 => ("shape.flip", json!({"ids": ids, "axis": if r.below(2) == 0 { "horizontal" } else { "vertical" }})),
        4 | 5 => {
            let to = ["front", "back", "forward", "backward"][r.below(4) as usize];
            ("shape.order", json!({"ids": ids, "to": to}))
        }
        6 => {
            let how = ["left", "center", "right", "top", "middle", "bottom"][r.below(6) as usize];
            ("shape.align", json!({"ids": ids, "how": how}))
        }
        _ => (
            "shape.resize",
            json!({"id": r.id(live), "x": r.coord(), "y": r.coord(), "width": 0.2 + r.coord() / 3.0, "height": 0.2 + r.coord() / 3.0}),
        ),
    }
}

/// A clipboard, group or ungroup command on shapes that may not be there.
fn clipboard_command(r: &mut Rng, live: &[u32]) -> (&'static str, Value) {
    let ids = if r.below(2) == 0 { json!([r.id(live), r.id(live)]) } else { json!([r.id(live), r.id(live), r.id(live)]) };
    match r.below(10) {
        0 | 1 => ("edit.copy", json!({"ids": ids})),
        2 => ("edit.cut", json!({"ids": ids})),
        3..=5 => ("edit.paste", if r.below(3) == 0 { json!({"x": r.coord(), "y": r.coord()}) } else { json!({}) }),
        6 => ("shape.duplicate", json!({"ids": ids, "dx": r.coord() / 4.0 - 1.0, "dy": r.coord() / 4.0 - 1.0})),
        7 | 8 => ("shape.group", json!({"ids": ids})),
        _ => ("shape.ungroup", json!({"ids": ids})),
    }
}

/// A shape operation on shapes that may not be there, in groups, glued to connectors or not areas at all.
fn shape_ops_command(r: &mut Rng, live: &[u32]) -> (&'static str, Value) {
    let ids = if r.below(3) == 0 { json!([r.id(live), r.id(live), r.id(live)]) } else { json!([r.id(live), r.id(live)]) };
    let id = [
        "shape.union",
        "shape.combine",
        "shape.fragment",
        "shape.intersect",
        "shape.subtract",
        "shape.join",
        "shape.trim",
        "shape.offset",
        "shape.fit_curve",
    ][r.below(9) as usize];
    // Offset takes a distance and Fit Curve a tolerance and the flags (a tolerance of half an inch makes a rectangle's lines two splines).
    let params = match id {
        "shape.offset" => json!({"ids": ids, "distance": 0.25}),
        "shape.fit_curve" => json!({"ids": ids, "tolerance": 0.5, "flags": r.below(8)}),
        _ => json!({"ids": ids}),
    };
    (id, params)
}

fn command(r: &mut Rng, live: &[u32], arrange: bool, clipboard: bool, shape_ops: bool) -> (&'static str, Value) {
    if shape_ops && r.below(3) == 0 {
        return shape_ops_command(r, live);
    }
    if arrange && r.below(3) == 0 {
        return arrange_command(r, live);
    }
    if clipboard && r.below(3) == 0 {
        return clipboard_command(r, live);
    }
    match r.below(18) {
        0..=3 => ("shape.drop", json!({"master": MASTERS[r.below(12) as usize], "page": 1, "x": r.coord(), "y": r.coord()})),
        4 | 5 => ("shape.move", json!({"ids": [r.id(live), r.id(live)], "dx": r.coord() - 5.0, "dy": r.coord() - 5.0})),
        6 => (
            "shape.resize",
            json!({"id": r.id(live), "x": r.coord(), "y": r.coord(), "width": 0.2 + r.coord() / 3.0, "height": 0.2 + r.coord() / 3.0}),
        ),
        7 => ("shape.delete", json!({"ids": [r.id(live)]})),
        8 => ("shape.set_text", json!({"id": r.id(live), "text": if r.below(3) == 0 { "" } else { "label" }})),
        9..=12 => ("connector.add", json!({"from": end(r, live), "to": end(r, live)})),
        13..=15 => ("connector.reconnect", json!({"id": r.id(live), "end": if r.below(2) == 0 { "begin" } else { "end" }, "target": end(r, live)})),
        16 => ("edit.undo", json!({})),
        _ => (if r.below(3) == 0 { "edit.undo" } else { "edit.redo" }, json!({})),
    }
}

fn number(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> f64 {
    res.resolve(sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
}

/// The route of connector `sheet`, on the page.
fn route(res: &Resolver<'_>, sheet: &SheetRef) -> Vec<(f64, f64)> {
    let n = |name: &str| number(res, sheet, name);
    let pose = Pose { pin: (n("PinX"), n("PinY")), loc: (n("LocPinX"), n("LocPinY")), angle: n("Angle"), flip_x: false, flip_y: false };
    let rows = res.effective_rows(sheet, &SectionKey::new("Geometry", Some(0))).unwrap_or_default();
    rows.iter()
        .map(|row| {
            let at = |name: &str| {
                row.cells.iter().find(|(c, _)| c.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
            };
            pose.up((at("X"), at("Y")))
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

/// A point of the frame of the parent of the shape `sheet`, carried through the groups it is in to the page.
fn to_page(doc: &Document, res: &Resolver<'_>, sheet: &SheetRef, p: (f64, f64)) -> (f64, f64) {
    let SheetRef::Shape(at) = sheet else { return p };
    let _ = doc;
    (1..at.path.len()).rev().fold(p, |p, k| {
        let group = SheetRef::Shape(diacraft_doc::ShapeRef { owner: at.owner, path: at.path[..k].to_vec() });
        let n = |name: &str| number(res, &group, name);
        Pose {
            pin: (n("PinX"), n("PinY")),
            loc: (n("LocPinX"), n("LocPinY")),
            angle: n("Angle"),
            flip_x: n("FlipX") != 0.0,
            flip_y: n("FlipY") != 0.0,
        }
        .up(p)
    })
}

fn consistent(doc: &Document) -> Result<(), String> {
    let page = doc.page(1).ok_or("no page")?;
    let exists = |id: u32| doc.find_shape(Owner::Page(1), id).is_some();
    let res = Resolver::new(doc);
    for c in &page.connects {
        if !exists(c.from_sheet) || !exists(c.to_sheet) {
            return Err(format!("a Connect row names a shape that is not there: {c:?}"));
        }
        let part = c.to_part.ok_or("a Connect row with no ToPart")?;
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), c.from_sheet).ok_or("gone")?);
        let (x_cell, y_cell) = if c.from_cell.as_deref() == Some("BeginX") { ("BeginX", "BeginY") } else { ("EndX", "EndY") };
        // The end cells are in the frame of the connector's parent: a group's, for a connector in one.
        let (x, y) = to_page(doc, &res, &sheet, (number(&res, &sheet, x_cell), number(&res, &sheet, y_cell)));
        // Glued to the whole of a shape (a shape with no connection point): the end is on the shape's box.
        if part == 3 {
            let corners = shape_corners(doc, 1, c.to_sheet).ok_or("a whole-shape glue to a shape with no box")?;
            if !crate::connect_whole::on_box(corners, (x, y)) {
                return Err(format!(
                    "{x_cell} of {} is ({x}, {y}), which is not on the box {corners:?} of shape {} it is glued to the whole of",
                    c.from_sheet, c.to_sheet
                ));
            }
            continue;
        }
        // The glued end is on its connection point.
        let index = part.checked_sub(100).ok_or("a Connect row with no ToPart")?;
        let point =
            connection_points(doc, 1, c.to_sheet).into_iter().find(|p| p.index == index).ok_or("a Connect row names a point that is not there")?;
        if !near(x, point.x) || !near(y, point.y) {
            return Err(format!("{x_cell} of {} is ({x}, {y}) and its point is ({}, {})", c.from_sheet, point.x, point.y));
        }
    }
    // Every connector DiaCraft routes runs from its begin to its end in legs along the axes.
    let mut stack: Vec<_> = page.shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), shape.id).ok_or("gone")?);
        let marker = CellAddr::in_row("User", None, diacraft_doc::RowKey::name("DiaCraftRoute"), "Value");
        if res.resolve(&sheet, &marker).is_some_and(|f| f.value.cell.is_some()) {
            let points = route(&res, &sheet);
            let (first, last) = (points.first().ok_or("a route with no rows")?, points.last().ok_or("a route with no rows")?);
            let n = |name: &str| number(&res, &sheet, name);
            if !near(first.0, n("BeginX")) || !near(first.1, n("BeginY")) || !near(last.0, n("EndX")) || !near(last.1, n("EndY")) {
                return Err(format!(
                    "the route of {} is {points:?}, but it runs ({}, {}) to ({}, {})",
                    shape.id,
                    n("BeginX"),
                    n("BeginY"),
                    n("EndX"),
                    n("EndY")
                ));
            }
            if points.windows(2).any(|w| !(near(w[0].0, w[1].0) || near(w[0].1, w[1].1))) {
                return Err(format!("the route of {} has a slanted leg: {points:?}", shape.id));
            }
        }
        stack.extend(shape.children.iter());
    }
    Ok(())
}

fn run_sequence(seed: u64, steps: usize, arrange: bool, clipboard: bool) {
    run_sequence_with(seed, steps, arrange, clipboard, false, false);
}

/// The size of page 1, in inches.
fn page_size(s: &Session) -> (f64, f64) {
    let sheet = &s.document().unwrap().page(1).unwrap().sheet;
    let n = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
    (n("PageWidth"), n("PageHeight"))
}

/// [`run_sequence`], with Auto Size on from the first command when `auto_size`: the page grows as shapes are dropped, moved and pasted past its
/// edge, and carries what is on it when it grows on the left or the bottom, so every glued end must still be on its connection point.
fn run_sequence_with(
    seed: u64,
    steps: usize,
    arrange: bool,
    clipboard: bool,
    auto_size: bool,
    shape_ops: bool,
) -> std::collections::BTreeMap<&'static str, usize> {
    let mut r = Rng(seed);
    let mut s: Session = blank_session();
    let blank = s.document().unwrap().clone();
    if auto_size {
        s.execute("page.auto_size", &json!({"on": true})).unwrap();
    }
    let mut biggest = page_size(&s);
    let mut done = 0;
    let mut by_command: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut most_connects = 0;
    for step in 0..steps {
        let live: Vec<u32> = if clipboard {
            all_ids(s.document().unwrap())
        } else {
            s.document().unwrap().page(1).unwrap().shapes.iter().map(|shape| shape.id).collect()
        };
        let (id, params) = command(&mut r, &live, arrange, clipboard, shape_ops);
        // A connector of ours has legs along the axes of the page, which holds until a group it is in is turned: its legs are then turned with the
        // group, and stay so when it is ungrouped (it draws where it did). So no group is turned or flipped here; the tests of `group.rs` turn them.
        if clipboard && matches!(id, "shape.rotate" | "shape.flip") && names_a_group(s.document().unwrap(), &params) {
            continue;
        }
        let before = s.document().cloned();
        let outcome = s.execute(id, &params);
        match &outcome {
            Err(EngineError::Internal { .. }) => panic!("seed {seed} step {step}: {id} {params} hit a bug: {outcome:?}"),
            Err(_) => assert_eq!(s.document().cloned(), before, "seed {seed} step {step}: {id} {params} failed but changed the document"),
            Ok(_) => {
                done += 1;
                *by_command.entry(id).or_default() += 1;
            }
        }
        most_connects = most_connects.max(s.document().unwrap().page(1).unwrap().connects.len());
        let size = page_size(&s);
        biggest = (biggest.0.max(size.0), biggest.1.max(size.1));
        assert_eq!(s.panics_caught(), 0, "seed {seed} step {step}: {id} {params}");
        check_session(&s).unwrap_or_else(|e| panic!("seed {seed} step {step}: {id} {params}: {e}"));
        consistent(s.document().unwrap()).unwrap_or_else(|e| panic!("seed {seed} step {step}: after {id} {params}: {e}"));
    }
    assert!(done > steps / 4, "seed {seed}: only {done} of {steps} commands ran, so the sequence proved little");
    // Each kind of edit really ran, and the page held glued connectors at some point (the check above is not vacuous).
    // (With the shape operations among them, a third of the commands are theirs, and which of these runs depends on what the operations left of the ids: the
    // walk through all the seeds is asked for each of theirs instead.)
    let least = if shape_ops { 0 } else { 3 };
    for kind in ["shape.drop", "shape.move", "shape.resize", "shape.delete", "shape.set_text", "connector.add", "connector.reconnect"] {
        assert!(by_command.get(kind).copied().unwrap_or(0) >= least, "seed {seed}: {kind} ran {:?} times: {by_command:?}", by_command.get(kind));
    }
    assert!(most_connects >= 6, "seed {seed}: at most {most_connects} Connect rows at once");
    if arrange {
        for kind in ["shape.rotate", "shape.flip", "shape.order", "shape.align"] {
            assert!(by_command.get(kind).copied().unwrap_or(0) >= 3, "seed {seed}: {kind} ran {:?} times: {by_command:?}", by_command.get(kind));
        }
    }
    if clipboard {
        for kind in ["edit.copy", "edit.cut", "edit.paste", "shape.duplicate", "shape.group", "shape.ungroup"] {
            assert!(by_command.get(kind).copied().unwrap_or(0) >= 3, "seed {seed}: {kind} ran {:?} times: {by_command:?}", by_command.get(kind));
        }
    }
    if auto_size {
        // The page grew, by whole sheets of its Letter landscape paper, and it grew on both axes at some point.
        assert!(biggest.0 > 11.0 && biggest.1 > 8.5, "seed {seed}: the page was never bigger than {biggest:?}");
        let whole = |size: f64, sheet: f64| ((size / sheet).round() * sheet - size).abs() < 1e-6;
        assert!(whole(biggest.0, 11.0) && whole(biggest.1, 8.5), "seed {seed}: the page is {biggest:?}, not whole sheets");
    } else {
        assert_eq!(biggest, (11.0, 8.5), "seed {seed}: with Auto Size off the page is never bigger");
    }
    eprintln!("seed {seed:#x}: {by_command:?}, up to {most_connects} Connect rows, the page up to {biggest:?}");
    while s.execute("edit.undo", &json!({})).is_ok() {}
    // Undo takes back everything to the blank page: the same pages, no masters.
    assert_eq!(s.document().unwrap(), &blank, "seed {seed}: undoing every step does not give the blank page back");
    by_command
}

#[test]
fn a_few_hundred_random_drawing_commands_leave_a_consistent_drawing_and_undo_back_to_blank() {
    for seed in [0x9E37_79B9_7F4A_7C15, 0xD1B5_4A32_D192_ED03, 0x2545_F491_4F6C_DD1D] {
        run_sequence(seed, 400, false, false);
    }
}

#[test]
fn the_same_with_turns_flips_orders_and_aligns_among_them() {
    // Every glued end stays on its connection point, whatever pose its shape is in, and undo takes every step back to the blank page.
    for seed in [0x1F83_D9AB_FB41_BD6B, 0x5BE0_CD19_137E_2179, 0xCBBB_9D5D_C105_9ED8] {
        run_sequence(seed, 400, true, false);
    }
}

/// Whether any `ID` the parameters name is a group's.
fn names_a_group(doc: &Document, params: &Value) -> bool {
    let ids = params["ids"].as_array().cloned().unwrap_or_default();
    ids.iter()
        .filter_map(|v| v.as_u64())
        .filter_map(|id| u32::try_from(id).ok())
        .any(|id| doc.find_shape(Owner::Page(1), id).and_then(|at| doc.shape(&at)).is_some_and(|shape| !shape.children.is_empty()))
}

/// The `ID` of every shape of page 1, members of groups too.
fn all_ids(doc: &Document) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = doc.page(1).unwrap().shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        out.push(shape.id);
        stack.extend(shape.children.iter());
    }
    out.sort_unstable();
    out
}

#[test]
fn the_same_with_copies_pastes_groups_and_ungroups_among_them() {
    // Every glued end stays on its connection point, whether its connector is on the page or in a group, and undo takes every step back to
    // the blank page: a copy is not a step, and a paste, a cut, a group and an ungroup are one each.
    for seed in [0x0123_4567_89AB_CDEF, 0xFEDC_BA98_7654_3210, 0x9E37_79B9_7F4A_7C15, 0xB5AD_4ECE_DA1C_E2A9] {
        run_sequence(seed, 500, true, true);
    }
}

#[test]
fn the_same_with_auto_size_growing_and_carrying_the_page_among_them() {
    // With Auto Size on, drops, moves, pastes and the rest that pass the page's edge grow it by whole sheets, and a growth on the left or the
    // bottom carries every shape: every glued end stays on its connection point and every route is as it was drawn, and undo takes every step
    // (the growth with its command) back to the blank page.
    for seed in [0x6A09_E667_F3BC_C908, 0xBB67_AE85_84CA_A73B, 0x3C6E_F372_FE94_F82B, 0xA54F_F53A_5F1D_36F1] {
        run_sequence_with(seed, 500, true, true, true, false);
    }
}

#[test]
fn the_same_with_the_shape_operations_among_them() {
    // Union, Combine, Fragment, Intersect and Subtract on shapes that may be in groups, glued to connectors or not there, among drops, moves, turns, groups
    // and pastes: no command hits a bug, a refused one changes nothing, every glued end stays on its connection point, and undo takes every step (an operation
    // is one) back to the blank page.
    let mut ran: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    // More seeds: a connector is glued to the whole of a shape that has no connection point so more of the random connectors are made, the random drawings are
    // others, and the five seeds that reached Intersect twice before reached it once.
    // (The first seed of the walk was 0x0123_4567_89AB_CDEF, and with Offset and Fit Curve among the draws it leaves only two Connect rows at its most, under the six the
    // sequence asks for to be of any use: its walk is no longer the walk it was, since each new command moves every draw after it. The seed that took its place holds ten.)
    let seeds = [
        0x1234_5678_9ABC_DEF0,
        0xFEDC_BA98_7654_3210,
        0x9E37_79B9_7F4A_7C15,
        0xB5AD_4ECE_DA1C_E2A9,
        0x1F83_D9AB_FB41_BD6B,
        0x6A09_E667_F3BC_C908,
        0xBB67_AE85_84CA_A73B,
        0x3C6E_F372_FE94_F82B,
    ];
    for seed in seeds {
        for (kind, n) in run_sequence_with(seed, 1_200, true, true, false, true) {
            *ran.entry(kind).or_default() += n;
        }
    }
    // Each operation really made or changed shapes, over the seeds, whatever else it was refused for.
    for kind in [
        "shape.union",
        "shape.combine",
        "shape.fragment",
        "shape.intersect",
        "shape.subtract",
        "shape.join",
        "shape.trim",
        "shape.offset",
        "shape.fit_curve",
    ] {
        assert!(ran.get(kind).copied().unwrap_or(0) >= 2, "{kind} ran {:?} times: {ran:?}", ran.get(kind));
    }
}
