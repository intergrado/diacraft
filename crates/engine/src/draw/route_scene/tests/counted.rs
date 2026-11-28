//! What a scene costs, by count and never by time: a page of 10,000 shapes and 10,000 connectors (the design's section 3.2 and 3.3). The counts are the scene's own
//! ([`Reads`]), held beside the ones the readers keep (poses read, rows the model merged), which no code of the scene writes.

use diacraft_doc::{Cell, CellName, Document, Value};
use diacraft_route::Work;

use super::build::{boxed, glue_both, line, ours, page, placeable, shift};
use super::{assert_same, plenty};
use crate::draw::route_scene::{Reads, Scene};

const SHAPES: u32 = 10_000;
const FIRST_CONNECTOR: u32 = 20_001;

/// 10,000 boxes on a grid of 100 by 100, one every 3 inches, and 10,000 connectors, each glued from a box to the next and holding a route of three rows (a Z of two legs).
pub(super) fn big() -> Document {
    let mut shapes = Vec::new();
    for k in 1..=SHAPES {
        shapes.push(boxed(k, (f64::from((k - 1) % 100) * 3.0, f64::from((k - 1) / 100) * 3.0, 1.0, 1.0)).cell("ObjType", "1"));
    }
    let mut connects = Vec::new();
    for k in 1..=SHAPES {
        let (x, y) = (f64::from((k - 1) % 100) * 3.0 + 1.0, f64::from((k - 1) / 100) * 3.0 + 0.5);
        shapes.push(ours(line(FIRST_CONNECTOR - 1 + k, &[(x, y), (x + 0.75, y), (x + 0.75, y + 1.0)])));
        connects.extend(glue_both(FIRST_CONNECTOR - 1 + k, k, k % SHAPES + 1));
    }
    page(shapes, connects)
}

fn delta(now: Reads, before: Reads) -> Reads {
    Reads {
        walked: now.walked - before.walked,
        shapes: now.shapes - before.shapes,
        peeks: now.peeks - before.peeks,
        lines: now.lines - before.lines,
        connect_rows: now.connect_rows - before.connect_rows,
        route_rows: now.route_rows - before.route_rows,
        whole: now.whole - before.whole,
    }
}

/// The first put right reads the page, and the work it is given is the steps it took; each put right after it walks the page's pointers and reads what changed, and nothing else: a move reads
/// one shape (none of the connectors, none of the rows of any route, no `Connect` row), a change of a connector's own cell reads that connector and its route, a new connector is read with one
/// pass over the `Connect` rows, and a change of the page's sheet reads the page whole.
#[test]
fn a_second_put_right_reads_no_shape_it_did_not_change() {
    let mut doc = big();
    let tops = u64::from(2 * SHAPES);
    let mut work = plenty();
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut work);
    let first = scene.reads();
    assert_eq!(
        first,
        Reads {
            walked: tops,
            shapes: tops,
            lines: u64::from(SHAPES),
            connect_rows: u64::from(4 * SHAPES),
            route_rows: u64::from(3 * SHAPES),
            whole: 1,
            ..Reads::default()
        },
        "the first edit reads every shape, every route and every row of glue, once"
    );
    assert_eq!(
        work.spent(),
        first.walked + first.shapes + first.connect_rows + first.route_rows,
        "the work is the steps: pointers walked, shapes looked at, rows passed and rows read"
    );
    assert!(!scene.cut().any(), "a page of 10,000 shapes and 10,000 connectors is within every cap");
    assert_eq!((scene.obstacles().len(), scene.wires().len(), scene.wires().leg_count()), (SHAPES as usize, SHAPES as usize, 2 * SHAPES as usize));
    // A move of one shape.
    diacraft_doc::work::reset();
    let poses = crate::draw::read::POSES_READ.with(std::cell::Cell::get);
    shift(&mut doc, 4_321, 0.0, 0.25);
    let before = scene.reads();
    let mut work = plenty();
    let changed = scene.put_right(&doc, 1, &mut work);
    assert_eq!(delta(scene.reads(), before), Reads { walked: tops, shapes: 1, ..Reads::default() }, "one shape read, and every pointer walked once");
    assert_eq!(changed.obstacles.len(), 1);
    assert_eq!(work.spent(), tops + 1, "the work of the walk and the read");
    assert_eq!(diacraft_doc::work::rows_merged(), 0, "no row of any route was merged: the model's own count");
    assert!(
        crate::draw::read::POSES_READ.with(std::cell::Cell::get) - poses <= 8,
        "the poses of one shape's four corners were read, and no other shape's"
    );
    // The same page again: nothing.
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads(), before, "a second put right of the same document walks and reads nothing");
    // A change of one connector's cell: that connector, its route and no other.
    {
        let at = u32::try_from(super::build::index_of(&doc, FIRST_CONNECTOR + 77)).unwrap();
        let shape = crate::draw::shape_mut(&mut doc, 1, &[at]).unwrap();
        std::sync::Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("ConFixedCode"), Cell::constant(Value::from_text("2")));
    }
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        delta(scene.reads(), before),
        Reads { walked: tops, shapes: 1, lines: 1, route_rows: 3, ..Reads::default() },
        "one connector and its three rows"
    );
    // A new connector: its glue is read in one pass over the rows, and it is the one shape read.
    super::build::add(&mut doc, ours(line(40_000, &[(0.0, 100.0), (4.0, 100.0)])));
    let mut rows: Vec<_> = doc.page(1).unwrap().connects.iter().cloned().collect();
    rows.extend(glue_both(40_000, 1, 2));
    super::build::reglue(&mut doc, rows);
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        delta(scene.reads(), before),
        Reads { walked: tops + 1, shapes: 1, lines: 1, connect_rows: u64::from(4 * SHAPES) + 4, route_rows: 2, ..Reads::default() },
        "the new connector read, the rows of glue passed once, and no other shape read"
    );
    // The page's sheet: every shape and every route again.
    let page = crate::draw::page_mut(&mut doc, 1).unwrap();
    std::sync::Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("LineToNodeX"), Cell::constant(Value::from_text("0.3")));
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    let d = delta(scene.reads(), before);
    assert_eq!((d.shapes, d.lines, d.whole), (tops + 1, u64::from(SHAPES) + 1, 1), "the page read whole");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "after every kind of change");
}

/// An undo and a redo are other documents whose pointers the scene may hold: the walk, and the shapes the two differ in.
#[test]
fn an_undone_move_is_put_right_by_the_one_shape_it_moved() {
    let before_edit = big();
    let mut after_edit = before_edit.clone();
    shift(&mut after_edit, 777, 0.5, 0.0);
    let mut scene = Scene::read(&before_edit, 1, &mut plenty());
    scene.put_right(&after_edit, 1, &mut plenty());
    let reads = scene.reads();
    scene.put_right(&before_edit, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - reads.shapes, 1, "the undo reads the one shape that differs");
    scene.put_right(&after_edit, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - reads.shapes, 2, "and the redo");
    assert_same(&scene, &Scene::read(&after_edit, 1, &mut plenty()), "undo and redo");
}

/// 30,000 two-dimensional shapes: the first put right reads 10,000 and tells the kind of the other 20,000 by their cells; a put right after an edit walks the pointers and reads only what changed
/// (the shapes past the cap that did not change cost a compare).
#[test]
fn a_page_of_30000_shapes_is_read_to_the_cap_and_the_rest_costs_a_compare() {
    let mut doc = page(
        (1..=30_000u32).map(|k| boxed(k, (f64::from(k % 200) * 3.0, f64::from(k / 200) * 3.0, 1.0, 1.0)).cell("ObjType", "1")).collect(),
        Vec::new(),
    );
    let mut work = plenty();
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut work);
    let first = scene.reads();
    assert_eq!(
        (first.walked, first.shapes, first.peeks),
        (30_000, 10_000, 20_000),
        "ten thousand read, twenty thousand told apart by four cells each"
    );
    assert_eq!(work.spent(), first.walked + first.shapes + first.peeks, "the work is the steps");
    assert_eq!(scene.obstacles().len(), 10_000);
    assert!(scene.cut().shapes && scene.take_notice().is_some());
    // An edit inside the cap, and one past it.
    shift(&mut doc, 77, 0.0, 0.25);
    shift(&mut doc, 25_000, 0.0, 0.25);
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        delta(scene.reads(), before),
        Reads { walked: 30_000, shapes: 1, peeks: 1, ..Reads::default() },
        "the one inside the cap read; the one past it told apart again; the 19,998 others past it cost a compare"
    );
    assert_eq!(scene.take_notice(), None, "and nothing said again");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "30,000 shapes");
}

/// An edit that replaces every shape of the page reads each once, and reports each once, in the order of the `ID`s, **however many times it is made**: the counts are the same at each round
/// and do not grow; the edit after it reads only what it changed.
#[test]
fn an_edit_that_moves_every_shape_reads_each_once() {
    let mut doc = big();
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    let mut rounds = Vec::new();
    for _ in 0..3 {
        for at in 0..2 * SHAPES {
            super::build::nudge_at(&mut doc, at, 0.25);
        }
        let before = scene.reads();
        let mut work = plenty();
        let changed = scene.put_right(&doc, 1, &mut work);
        rounds.push((delta(scene.reads(), before), work.spent(), changed.obstacles.len()));
        assert!(changed.obstacles.windows(2).all(|w| w[0].id < w[1].id), "in the order of the IDs");
    }
    let each = |d: &Reads| (d.shapes, d.lines, d.walked, d.whole);
    for (d, spent, reported) in &rounds {
        assert_eq!(
            each(d),
            (u64::from(2 * SHAPES), u64::from(SHAPES), u64::from(2 * SHAPES), 0),
            "every shape and every route read once, and not the page whole"
        );
        assert_eq!(*spent, d.walked + d.shapes + d.route_rows, "the work is the steps");
        assert_eq!(*reported, SHAPES as usize, "every box moved");
    }
    assert!(rounds.windows(2).all(|w| w[0] == w[1]), "the cost of the third round is the cost of the first: it does not grow with the edits");
    shift(&mut doc, 10, 1.0, 0.0);
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(delta(scene.reads(), before).shapes, 1, "and the edit after it reads one shape");
}

/// A group's members are charged to the work **before** each is read: a put right with one step left for them reads one member, however many the group holds (the walk used to read up to the
/// shape cap, 10,000 of them, and charge them after), and work that is refused is no cut of the shape cap.
#[test]
fn a_wide_group_is_not_read_past_the_work_the_put_right_has_left() {
    let members: Vec<_> = (1..=5_000u32).map(|k| placeable(100_000 + k, (f64::from(k % 100), f64::from(k / 100), 0.5, 0.5))).collect();
    let doc = page(vec![super::build::group(1, (0.0, 0.0, 100.0, 60.0), "8", members)], Vec::new());
    crate::draw::obstacle::MEMBER_REFS.with(|c| c.set(0));
    let poses = crate::draw::read::POSES_READ.with(std::cell::Cell::get);
    // One pointer walked, the group looked at, and one step for the members.
    let mut work = Work::new(3);
    let mut scene = Scene::new(1);
    let changed = scene.put_right(&doc, 1, &mut work);
    let refs = crate::draw::obstacle::MEMBER_REFS.with(std::cell::Cell::get);
    let read = crate::draw::read::POSES_READ.with(std::cell::Cell::get) - poses;
    assert!(changed.incomplete && !scene.is_complete(), "three steps are not a group of 5,000 members");
    assert_eq!(work.spent(), 3, "no more work than it was given");
    assert!(refs <= 3, "{refs} references to members were made with one step left for them");
    assert!(read <= 3 * 8, "{read} poses were read: the group's own box and one member's, and no more");
    assert!(!scene.cut().any(), "work that was refused is no cut of the shape cap");
    // The same group with the work it needs is read whole, and the steps charged are the shapes looked at.
    let mut work = plenty();
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut work);
    assert_eq!(scene.reads().shapes, 5_001, "the group's own look and its 5,000 members");
    assert_eq!(work.spent(), scene.reads().walked + scene.reads().shapes + scene.reads().peeks, "the work is the steps");
    // Work enough for 100 members, and a cap of 50: the cap cuts, the work does not run out.
    let caps = crate::draw::route_scene::Caps { shapes: 51, ..crate::draw::route_scene::Caps::default() };
    let mut work = Work::new(1 + 1 + 100);
    let mut scene = Scene::with_caps(1, caps);
    let changed = scene.put_right(&doc, 1, &mut work);
    assert!(!changed.incomplete && scene.is_complete(), "the cap is met before the work runs out");
    assert!(scene.cut().shapes, "a cut of the shape cap");
    assert_eq!(scene.reads().shapes, 51);
}

/// A chain of groups as deep as the model allows, each holding 8 and the next: a read goes down 8 levels and no further, in a number of steps that does not grow with the chain; and a group of
/// 20,000 members is read to the cap and not past it.
#[test]
fn deep_and_wide_groups_are_read_to_their_bounds() {
    use crate::draw::obstacle::MAX_MEMBER_DEPTH;
    let mut inner = placeable(1_000, (0.0, 0.0, 1.0, 1.0));
    for level in 0..150u32 {
        inner = super::build::group(900 - level, (0.0, 0.0, 4.0, 4.0), "8", vec![inner]);
    }
    let doc = page(vec![inner, placeable(2, (30.0, 0.0, 1.0, 1.0))], Vec::new());
    let scene = Scene::read(&doc, 1, &mut plenty());
    assert!(scene.reads().shapes <= MAX_MEMBER_DEPTH as u64 + 3, "{} shapes looked at for 150 levels of groups", scene.reads().shapes);
    assert!(scene.candidate(1_000).is_none(), "the box 150 levels down is not read");
    assert!(scene.candidate(2).is_some());
    // A group of 20,000 members.
    let members: Vec<_> = (1..=20_000u32).map(|k| placeable(100_000 + k, (f64::from(k % 100), f64::from(k / 100), 0.5, 0.5))).collect();
    let doc = page(vec![super::build::group(1, (0.0, 0.0, 100.0, 200.0), "8", members), placeable(2, (500.0, 0.0, 1.0, 1.0))], Vec::new());
    let mut work = plenty();
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut work);
    assert_eq!(scene.reads().shapes, 10_000, "the group's own look and 9,999 of its members");
    assert_eq!(scene.obstacles().len(), 9_999);
    assert!(scene.cut().shapes);
    assert!(scene.candidate(2).is_none(), "and the shape after it is past the cap");
    assert_eq!(work.spent(), scene.reads().walked + scene.reads().shapes + scene.reads().peeks);
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a wide group");
}
