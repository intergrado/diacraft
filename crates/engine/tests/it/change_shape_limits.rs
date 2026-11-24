//! Change Shape on hostile shapes: groups nested to the depth a file allows, and sheets that are full.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, MAX_CELLS_PER_CONTAINER, Owner, Value};
use diacraft_engine::Session;
use diacraft_engine::draw::MAX_GROUP_DEPTH;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::VsdxBuilder;
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::stencil_open::{CT_STENCIL, Dir, cluster_members, group_master, open};
use crate::{blank_session, check_session, run, snapshot};

fn boxed(id: u32) -> ShapeBuilder {
    ShapeBuilder::new(id).cell("PinX", "4").cell("PinY", "4").cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5")
}

/// A page whose one top-level shape is a chain of `depth` shapes, each a group of the next: shape `depth` is the plain box at the bottom, `depth` levels down.
fn chain(depth: u32) -> Document {
    let mut shape = boxed(depth);
    for id in (1..depth).rev() {
        shape = boxed(id).group().child(shape);
    }
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// A session that holds a chain `depth` deep and has opened a stencil with the master "Cluster": a group of three levels (itself, a member, and its members).
fn nested_session(depth: u32) -> (Session, Dir) {
    let dir = Dir::new();
    let mut s = session_with(chain(depth));
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&cluster_members())).build();
    open(&mut s, &dir.write("cluster.vssx", &stencil));
    (s, dir)
}

fn level_of(d: &Document, id: u32) -> usize {
    d.find_shape(Owner::Page(1), id).expect("the shape").path.len()
}

#[test]
fn a_shape_near_the_deepest_a_file_nests_is_not_changed_to_a_master_whose_members_would_go_past_it() {
    // The bottom shape is `MAX_GROUP_DEPTH - 1` levels down, and the master adds two levels below it: the file would be nested past the deepest it may.
    let depth = u32::try_from(MAX_GROUP_DEPTH).unwrap() - 1;
    let (mut s, _dir) = nested_session(depth);
    assert_eq!(level_of(doc(&s), depth), usize::try_from(depth).unwrap());
    let before = snapshot(&s);
    let e = s.execute("shape.change", &json!({"ids": [depth], "master": "Cluster", "stencil": "file-1"})).unwrap_err();
    assert!(e.to_string().contains("nested too deeply") && e.to_string().contains(&depth.to_string()), "{e}");
    assert_eq!(snapshot(&s), before, "nothing is changed");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_that_would_reach_the_deepest_a_file_nests_is_changed_and_one_level_past_it_is_not() {
    // The master has three levels: its instance, a member and the member's members, so the deepest shape may be `MAX_GROUP_DEPTH - 2` levels down.
    let depth = u32::try_from(MAX_GROUP_DEPTH).unwrap() - 2;
    let (mut s, _dir) = nested_session(depth);
    let r = run(&mut s, "shape.change", json!({"ids": [depth], "master": "Cluster", "stencil": "file-1"}));
    assert_eq!(r["changed"], json!([depth]), "{r}");
    assert_eq!(check_session(&s), Ok(()));
    // A mixed selection: the shape that can be changed is, and the one that cannot is listed with the reason.
    let (mut s, _dir) = nested_session(depth + 1);
    let near = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 9.0}));
    let near = u32::try_from(near["id"].as_u64().unwrap()).unwrap();
    let r = run(&mut s, "shape.change", json!({"ids": [depth + 1, near], "master": "Cluster", "stencil": "file-1"}));
    assert_eq!(r["changed"], json!([near]), "{r}");
    assert_eq!(r["skipped"][0]["id"], depth + 1, "{r}");
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("nested too deeply"), "{r}");
}

/// A stencil whose master "Big" is a group of `n` boxes, which an instance lists as `n` members.
fn big_group_session(n: u32, shapes: u32) -> (Session, Dir) {
    let dir = Dir::new();
    let mut s = blank_session();
    let leaves: String = (0..n).map(|k| format!("<Shape ID=\"{}\"/>", 10 + k)).collect();
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&leaves)).build();
    open(&mut s, &dir.write("big.vssx", &stencil));
    for k in 0..shapes {
        drop_shape(&mut s, "rectangle", 1.0 + f64::from(k), 2.0);
    }
    (s, dir)
}

#[test]
fn a_selection_whose_new_members_pass_the_most_a_change_gives_is_refused_and_one_inside_it_is_changed() {
    // Each instance of the master lists 5,000 members. Ten of them are 50,000, the most one change gives out; eleven are refused in words, with nothing changed.
    let (mut s, _dir) = big_group_session(5_000, 11);
    let all: Vec<u32> = (1..=11).collect();
    let before = snapshot(&s);
    let e = s.execute("shape.change", &json!({"ids": all, "master": "Cluster", "stencil": "file-1"})).unwrap_err();
    assert!(e.to_string().contains("50000") || e.to_string().contains("50,000"), "{e}");
    assert_eq!(snapshot(&s), before, "nothing is changed");
    let ten: Vec<u32> = (1..=10).collect();
    let r = run(&mut s, "shape.change", json!({"ids": ten, "master": "Cluster", "stencil": "file-1"}));
    assert_eq!(r["changed"].as_array().map(Vec::len), Some(10), "{r}");
    assert_eq!(check_session(&s), Ok(()));
}

/// A row of `n` rectangles, 1 to `n`, joined by a connector each (`n + 1` to `2n - 1`).
fn row_of_joined(n: u32) -> Session {
    let mut s = blank_session();
    for k in 0..n {
        drop_shape(&mut s, "rectangle", 1.0 + f64::from(k % 50) * 2.0, 1.0 + f64::from(k / 50) * 2.0);
    }
    for k in 1..n {
        run(&mut s, "connector.add", json!({"from": {"id": k}, "to": {"id": k + 1}}));
    }
    s
}

/// What changing the first `m` shapes of a row to hexagons costs, counted in the recalculations the calc crate did: `(commits, steps)`.
fn counted_change(n: u32, m: u32) -> (u64, u64) {
    let mut s = row_of_joined(n);
    let ids: Vec<u32> = (1..=m).collect();
    diacraft_calc::work::reset();
    let r = run(&mut s, "shape.change", json!({"ids": ids, "master": "hexagon"}));
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    assert_eq!(r["changed"].as_array().map(Vec::len), usize::try_from(m).ok(), "{r}");
    assert_eq!(check_session(&s), Ok(()));
    eprintln!("{m} of {n} shapes changed: {commits} commits of {steps} steps");
    (commits, steps)
}

#[test]
fn what_a_change_recalculates_is_a_few_commits_and_grows_with_the_shapes_changed_and_no_faster() {
    // The calc crate's commits and steps are counted, not timed: a few commits whatever the number of shapes, and the steps of ten times as many shapes are about ten times.
    let (small_commits, small_steps) = counted_change(60, 6);
    let (large_commits, large_steps) = counted_change(60, 60);
    assert!(
        small_commits <= 12 && large_commits <= 12,
        "{small_commits} and {large_commits} commits: one for the change, one for the pins, and the routes"
    );
    assert!(large_steps <= small_steps.saturating_mul(14), "ten times the shapes: {small_steps} steps and {large_steps} steps");
}

/// Adds `n` cells of its own to the shape `id`.
fn add_cells(s: &mut Session, id: u32, n: usize) {
    s.edit("junk", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|shape| shape.id == id).unwrap();
        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        for i in 0..n {
            sheet.cells.insert(CellName::new(&format!("Junk{i}")), Cell::constant(Value::from_text("1")));
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_shape_that_the_change_would_leave_with_more_cells_than_a_sheet_holds_is_refused() {
    // A rectangle holds PinX and PinY of its own; the junk brings it to the most cells a sheet may hold. A square is 1 by 1 where it is 1.5 by 1, so the change writes
    // its width and height too, and two cells would be over.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    add_cells(&mut s, a, MAX_CELLS_PER_CONTAINER - 2);
    let held = doc(&s).page(1).unwrap().shapes.iter().next().unwrap().sheet.cells.len();
    assert_eq!(held, MAX_CELLS_PER_CONTAINER, "the shape holds the most a sheet may");
    let before = snapshot(&s);
    let e = s.execute("shape.change", &json!({"ids": [a], "master": "square"})).unwrap_err();
    assert!(e.to_string().contains("cells") && e.to_string().contains(&MAX_CELLS_PER_CONTAINER.to_string()), "{e}");
    assert_eq!(snapshot(&s), before, "nothing is changed");
    // Four cells fewer, and it is changed.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    add_cells(&mut s, a, MAX_CELLS_PER_CONTAINER - 4);
    let r = run(&mut s, "shape.change", json!({"ids": [a], "master": "square"}));
    assert_eq!(r["changed"], json!([a]), "{r}");
}
