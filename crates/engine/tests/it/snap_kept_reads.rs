//! The fixes of the narrow re-review: what a kept read holds is never made of a draft that is thrown away, a closed drawing keeps no read
//! And the survivors of the re-review's mutation runs. The settings are the spec's "Settings for vectors": the grid and the ruler off, the Snap switch and the
//! alignment box on; paper inches, y up, 96 screen points to an inch.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::snap::{Kind, SnapPage, SnapSettings, Use, counts, make_ready, pin_of, reset_counts};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use super::snap_reads::{commands, made_ready, rects, snap};
use super::stencil_open::{Dir, stencil_bytes};
use crate::{blank_session, run};

/// A 1 by 1 shape with its pin at its middle, at (`x`, `y`).
fn shape(id: u32, x: f64, y: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &x.to_string())
        .cell("PinY", &y.to_string())
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0")
}

/// Shape 1 (the one that moves) at (3, 3), shape 2 at (7, `y2`), and thirty shapes far away. A move of shape 1 by (0, 1.97) lines its middle up with shape 2's when `y2` is 5.
fn scene(y2: f64) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "40").cell("PageHeight", "40").shape(shape(1, 3.0, 3.0)).shape(shape(2, 7.0, y2));
    for i in 0..30u32 {
        page = page.shape(shape(10 + i, 20.0 + f64::from(i % 6) * 2.5, 20.0 + f64::from(i / 6) * 2.5));
    }
    DocBuilder::new().page(1, page).build()
}

fn session_with(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", move |target| {
        *target = doc;
        Ok(())
    })
    .unwrap();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false, "alignment_box": true}));
    s
}

fn zoomed() -> Value {
    json!({"zoom": 96.0, "view": [0.0, 0.0, 40.0, 40.0]})
}

/// The probe: shape 1 moved up by 1.97 inch, snapped. With a shape at (7, 5) it comes to its middle line (a hit); with none it does not.
fn probe() -> Value {
    json!({"ids": [1], "dx": 0.0, "dy": 1.97, "page": 1, "snap": zoomed()})
}

fn made_ready_here(s: &Session) {
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), &[1], Kind::Box { framed: false });
}

fn hits(answer: &Value) -> bool {
    answer["snap"]["hits"].as_array().is_some_and(|h| !h.is_empty())
}

/// What a session gives for the probe: the answer, and the document it leaves.
fn probed(s: &mut Session) -> (Value, Document) {
    let answer = run(s, "shape.move", probe());
    (answer, s.document().unwrap().clone())
}

/// **Of the re-review.** A `command.batch` of a snapped `shape.drop` and a step that fails puts the revision back, and the read the session kept for that revision stays. The
/// drop took the read before its edit and asked it with the draft of the document; the read's index of the shapes, made at the first query, was made of the draft, and held the new
/// shape. The next snapped move then lined up with a shape that is not there. The page is read as the document is before the edit, whether or not the reader had made it ready.
#[test]
fn a_failed_batch_with_a_snapped_drop_leaves_no_phantom_shape_in_the_kept_read() {
    for ready in [false, true] {
        let (mut kept, mut control) = (session_with(scene(15.0)), session_with(scene(15.0)));
        let page = kept.snap_page(1).unwrap();
        if ready {
            made_ready_here(&kept);
        }
        let batch = json!({"commands": [
            {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 7.0, "y": 5.0, "snap": zoomed()}},
            {"command": "shape.move", "params": {"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1}},
        ]});
        let revision = kept.revision();
        assert!(kept.execute("command.batch", &batch).is_err() && control.execute("command.batch", &batch).is_err());
        assert_eq!(kept.revision(), revision, "ready {ready}: the failure put the revision back");
        assert_eq!(kept.document(), control.document());
        assert!(
            page.shapes(kept.document().unwrap()).find(40).is_none(),
            "ready {ready}: the new shape, whose id would be 40, is not in the index the session kept"
        );
        let (a, document) = probed(&mut kept);
        let (b, _) = probed(&mut control);
        assert_eq!(a, b, "ready {ready}: the kept session and the control answer the same");
        assert!(!hits(&a), "ready {ready}: shape 2 is at y = 15, so nothing is at (7, 5): {a}");
        assert!(document.page(1).is_some());
    }
}

/// The same for a master of a stencil file (`drop_from_file`, the second place a drop snaps on its draft).
#[test]
fn a_failed_batch_with_a_snapped_drop_of_a_stencil_file_master_leaves_no_phantom_shape_in_the_kept_read() {
    let dir = Dir::new();
    let path = dir.write("Flow.vssx", &stencil_bytes());
    for ready in [false, true] {
        let (mut kept, mut control) = (session_with(scene(15.0)), session_with(scene(15.0)));
        for s in [&mut kept, &mut control] {
            run(s, "stencil.open", json!({"path": path}));
        }
        let page = kept.snap_page(1).unwrap();
        if ready {
            made_ready_here(&kept);
        }
        let batch = json!({"commands": [
            {"command": "shape.drop", "params": {"master": "Process", "stencil": "file-1", "page": 1, "x": 7.0, "y": 5.0, "snap": zoomed()}},
            {"command": "shape.move", "params": {"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1}},
        ]});
        assert!(kept.execute("command.batch", &batch).is_err() && control.execute("command.batch", &batch).is_err());
        assert!(
            page.shapes(kept.document().unwrap()).find(40).is_none(),
            "ready {ready}: no shape of the dropped master in the index the session kept"
        );
        let (a, _) = probed(&mut kept);
        let (b, _) = probed(&mut control);
        assert_eq!(a, b, "ready {ready}");
        assert!(!hits(&a), "ready {ready}: {a}");
    }
}

/// **Every command that snaps**, run first in a batch whose next step fails, leaves nothing of what it edited in the read the session kept: the kept read, if it is still found, indexes
/// the shapes of the document as it is and no other. (A command that asks a kept read with a draft of the document would put its draft's shapes in the lazy index.)
#[test]
fn no_command_that_snaps_leaves_the_shapes_of_its_draft_in_the_kept_read_when_its_batch_fails() {
    let mut table = commands();
    // The one command that snaps and is not in the table of the window's: a place by the pin.
    table.push((
        "a place",
        "shape.place",
        json!({"ids": [1], "x": 7.31, "y": 7.02, "page": 1, "snap": snap()}),
        vec![1],
        Kind::Box { framed: false },
    ));
    for (what, command, params, moving, kind) in table {
        for ready in [false, true] {
            let mut s = super::snap_reads::session_of(rects(50));
            let page = if ready { made_ready(&s, &moving, kind) } else { s.snap_page(1).unwrap() };
            let batch = json!({"commands": [
                {"command": command, "params": params},
                {"command": "shape.move", "params": {"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1}},
            ]});
            let revision = s.revision();
            assert!(s.execute("command.batch", &batch).is_err(), "{what}: the second step fails");
            assert_eq!(s.revision(), revision);
            let doc = s.document().unwrap();
            let mut reads = vec![Arc::clone(&page)];
            reads.extend(s.kept_snap_page(1));
            for read in reads {
                for id in 1..=60u32 {
                    let in_index = read.shapes(doc).find(id).is_some();
                    let in_document = doc.page(1).is_some_and(|p| p.shapes.iter().any(|sh| sh.id == id));
                    assert_eq!(in_index, in_document, "{what} (ready {ready}): shape {id}: in the index {in_index}, in the document {in_document}");
                }
            }
        }
    }
}

/// **Of the re-review.** A new drawing and an opened one replace the document: the reads the session kept were of the drawing that is gone, and each holds a page's index and
/// targets. None is kept, and none is found, after either (the revision moving had made them unfound, but nothing dropped them).
#[test]
fn a_new_drawing_and_an_opened_one_leave_no_read_of_the_one_that_was_closed() {
    let dir = Dir::new();
    let path = dir.path("other.vsdx");
    let mut other = session_with(scene(15.0));
    run(&mut other, "file.save_as", json!({"path": path}));
    for open in [false, true] {
        let mut s = session_with(scene(5.0));
        made_ready_here(&s);
        assert!(s.kept_snap_page(1).is_some() && !s.snap_reads().is_empty() && s.snap_reads().held_bytes() > 0);
        if open {
            run(&mut s, "file.open", json!({"path": path}));
        } else {
            run(&mut s, "file.new", json!({}));
        }
        assert!(s.snap_reads().is_empty(), "open {open}: nothing is kept of the drawing that was closed");
        assert_eq!(s.snap_reads().held_bytes(), 0, "open {open}");
        assert!(s.kept_snap_page(1).is_none(), "open {open}: the new drawing is not the old one's page 1");
    }
}

/// The opened drawing answers what a session that never had the other drawing open answers (the reads of the closed drawing are not used, whatever the revision numbers say).
#[test]
fn a_snap_after_an_opened_file_is_what_a_session_that_never_had_another_open_says() {
    let dir = Dir::new();
    let path = dir.path("other.vsdx");
    let mut other = session_with(scene(15.0));
    run(&mut other, "file.save_as", json!({"path": path}));
    let (mut kept, mut control) = (session_with(scene(5.0)), blank_session());
    made_ready_here(&kept);
    run(&mut kept, "file.open", json!({"path": path}));
    run(&mut control, "file.open", json!({"path": path}));
    let (a, b) = (run(&mut kept, "shape.move", probe()), run(&mut control, "shape.move", probe()));
    assert_eq!(a, b);
    assert!(!hits(&a), "the opened drawing's shape 2 is at y = 15: {a}");
}

/// M4 of the re-review: a new drawing and an opened one **move the revision** (a frontend redraws when it differs, and a read kept for the old one is not found by it), whatever
/// the cache does with its reads.
#[test]
fn a_new_drawing_and_an_opened_one_move_the_revision_the_snap_reads_are_kept_by() {
    let dir = Dir::new();
    let path = dir.path("other.vsdx");
    let mut other = session_with(scene(15.0));
    run(&mut other, "file.save_as", json!({"path": path}));
    let mut s = session_with(scene(5.0));
    let before = s.revision();
    run(&mut s, "file.new", json!({}));
    let new = s.revision();
    assert!(new > before, "file.new moved the revision from {before} to {new}");
    run(&mut s, "file.open", json!({"path": path}));
    assert!(s.revision() > new, "file.open moved it from {new} to {}", s.revision());
}

/// The compare harness of the re-review: two sessions of one drawing, `kept` with the page made ready before `edit` and `control` with no read at all; after the edit a probe of a
/// snapped move answers the same in both (a stale read would line up with what was there), the documents are equal, and `kept_found` says whether the command found a kept read.
fn compare(what: &str, before: impl Fn(&mut Session), edit: impl Fn(&mut Session), y2: f64, expect_found: bool) -> Value {
    let (mut kept, mut control) = (session_with(scene(y2)), session_with(scene(y2)));
    before(&mut kept);
    before(&mut control);
    made_ready_here(&kept);
    edit(&mut kept);
    edit(&mut control);
    reset_counts();
    let a = run(&mut kept, "shape.move", probe());
    let c = counts();
    let b = run(&mut control, "shape.move", probe());
    assert_eq!(a, b, "{what}: the answer from the kept session differs from the control's");
    assert_eq!(kept.document(), control.document(), "{what}: documents differ");
    assert_eq!((c.kept_found, c.kept_missing), if expect_found { (1, 0) } else { (0, 1) }, "{what}: kept_found and kept_missing {c:?}");
    a
}

fn move_by(s: &mut Session, id: u32, dy: f64) {
    run(s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": dy, "page": 1}));
}

/// The re-review's soundness cases (held in the re-review, and kept): after every kind of edit a snap on a read kept before it answers what a session with no read answers, and finds
/// no kept read where the revision moved.
#[test]
fn every_kind_of_edit_leaves_no_stale_read() {
    let a = compare("move the target away", |_| {}, |s| move_by(s, 2, 10.0), 5.0, false);
    assert!(!hits(&a), "no hit once the target is gone: {a}");
    let a = compare("move the target in", |_| {}, |s| move_by(s, 2, -10.0), 15.0, false);
    assert!(hits(&a), "a hit now: {a}");
    compare(
        "delete",
        |_| {},
        |s| {
            run(s, "shape.delete", json!({"ids": [2], "page": 1}));
        },
        5.0,
        false,
    );
    let a = compare(
        "undo",
        |s| move_by(s, 2, 10.0),
        |s| {
            run(s, "edit.undo", json!({}));
        },
        5.0,
        false,
    );
    assert!(hits(&a), "the target is back after undo: {a}");
    let redo = |s: &mut Session| {
        move_by(s, 2, 10.0);
        run(s, "edit.undo", json!({}));
    };
    let a = compare(
        "redo",
        redo,
        |s| {
            run(s, "edit.redo", json!({}));
        },
        5.0,
        false,
    );
    assert!(!hits(&a), "the target moved away again by the redo: {a}");
    let a = compare(
        "settings",
        |_| {},
        |s| {
            run(s, "snap.set", json!({"snap": false}));
        },
        5.0,
        false,
    );
    assert!(!hits(&a), "snap off: {a}");
    let drop = |s: &mut Session| {
        run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 7.0, "y": 5.0}));
    };
    let a = compare("a drop adds a target", |_| {}, drop, 15.0, false);
    assert!(hits(&a), "a hit now: {a}");
    compare(
        "a page is added",
        |_| {},
        |s| {
            run(s, "page.add", json!({"name": "Second"}));
        },
        5.0,
        false,
    );
    compare("nothing happened", |_| {}, |_| {}, 5.0, true);
    let fails = |s: &mut Session| assert!(s.execute("shape.move", &json!({"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1})).is_err());
    compare("a command that failed", |_| {}, fails, 5.0, true);
}

/// The reader's thread and a command at once (the window's reader makes the page ready for any kind while a command snaps on it): the command answers what it answers alone, 300
/// times, and nothing deadlocks.
#[test]
fn the_reader_and_a_command_at_once_agree() {
    use diacraft_engine::snap::Use;
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut s = session_with(scene(5.0));
    let mut control = session_with(scene(5.0));
    let params = json!({"page": 1, "ids": [1], "left": 2.5, "bottom": 4.47, "right": 3.5, "top": 5.47, "zoom": 96.0});
    let want = run(&mut control, "snap.at", params.clone());
    assert!(want["hits"].as_array().is_some_and(|h| !h.is_empty()), "{want}");
    let page = s.snap_page(1).unwrap();
    let doc = Arc::new(s.document().unwrap().clone());
    let settings = SnapSettings::of(&doc);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for t in 0..3u32 {
            let (page, doc, stop) = (Arc::clone(&page), Arc::clone(&doc), &stop);
            scope.spawn(move || {
                let kinds = [
                    Kind::Box { framed: false },
                    Kind::Box { framed: true },
                    Kind::Edges { framed: false },
                    Kind::Point(Use::Place),
                    Kind::Point(Use::Connect),
                    Kind::Turn,
                ];
                let mut n = t;
                while !stop.load(Ordering::SeqCst) {
                    let moving: Vec<u32> = match n % 4 {
                        0 => vec![1],
                        1 => vec![2],
                        2 => vec![],
                        _ => vec![1, 2],
                    };
                    make_ready(&doc, &page, &settings, &moving, kinds[(n as usize) % kinds.len()]);
                    n = n.wrapping_add(1);
                }
            });
        }
        for i in 0..300 {
            assert_eq!(run(&mut s, "snap.at", params.clone()), want, "query {i}");
        }
        stop.store(true, Ordering::SeqCst);
    });
}

/// **M5 of the re-review** (`shape.rs`, the cap of a snapped move): 512 shapes are snapped, 513 are refused in words, by a release, by a press of an arrow key and by a place.
#[test]
fn a_snapped_move_of_more_than_512_shapes_is_refused_in_words_and_512_is_not() {
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for i in 0..600u32 {
        page = page.shape(shape(i + 1, f64::from(i % 100) * 1.5 + 1.0, f64::from(i / 100) * 1.5 + 1.0));
    }
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    let ids = |n: u32| (1..=n).collect::<Vec<u32>>();
    let words = "takes at most 512 shapes";
    let e = s.execute("shape.move", &json!({"ids": ids(513), "dx": 0.31, "dy": 0.02, "page": 1, "snap": zoomed()})).unwrap_err().to_string();
    assert!(e.contains(words), "{e}");
    run(&mut s, "shape.move", json!({"ids": ids(512), "dx": 0.31, "dy": 0.02, "page": 1, "snap": zoomed()}));
    let press = json!({"zoom": 96.0, "nudge": {"tick": 0.125}});
    let e = s.execute("shape.move", &json!({"ids": ids(513), "dx": 1.0, "dy": 0.0, "page": 1, "snap": press})).unwrap_err().to_string();
    assert!(e.contains(words), "{e}");
    let e = s.execute("shape.place", &json!({"ids": ids(513), "x": 5.0, "page": 1, "snap": zoomed()})).unwrap_err().to_string();
    assert!(e.contains(words), "{e}");
}

/// **M6 of the re-review** (`pin_of`, which divides by the page's scale to say paper inches): a shape at (10, 10) drawing units with its pin off the middle of its box is at (10, 10)
/// paper inches on a page drawn at 1 to 1, at (5, 5) at 2 to 1 and at (20, 20) at 1 to 2; a turn's angle mark is drawn there.
#[test]
fn the_pin_of_a_shape_is_in_paper_inches_on_a_page_at_any_scale() {
    for (scale, want) in [(1.0, (10.0, 10.0)), (2.0, (5.0, 5.0)), (0.5, (20.0, 20.0))] {
        let page = PageBuilder::new()
            .cell("PageWidth", "40")
            .cell("PageHeight", "40")
            .cell("PageScale", "1")
            .cell("DrawingScale", &scale.to_string())
            .shape(
                ShapeBuilder::new(1)
                    .cell("PinX", "10")
                    .cell("PinY", "10")
                    .cell("Width", "4")
                    .cell("Height", "2")
                    .cell("LocPinX", "1")
                    .cell("LocPinY", "0.5")
                    .cell("Angle", "0.4"),
            );
        let doc = DocBuilder::new().page(1, page).build();
        let got = pin_of(&doc, 1, 1).unwrap();
        assert!((got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9, "at a scale of {scale}: {got:?}, wanted {want:?}");
    }
}

/// A snap whose settings snap to no shape (the Snap switch on, the grid on, the dynamic grid and every option that looks at shapes off) **reads no shape**, for a drop as for a
/// move: the page is made ready for the drop before its edit, and what that makes is what the drop's own query makes, which is nothing of the shapes here.
#[test]
fn a_grid_only_snap_reads_no_shape_for_a_drop_either() {
    let off = json!({
        "grid": true, "ruler": false, "alignment_box": false, "dynamic_grid": false, "snap_connection_points": false, "snap_vertices": false,
        "snap_geometry": false, "snap_handles": false, "intersections": false
    });
    for (what, command, params) in [
        ("a drop", "shape.drop", json!({"master": "rectangle", "page": 1, "x": 7.31, "y": 7.02, "snap": zoomed()})),
        ("a move", "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": zoomed()})),
    ] {
        let mut s = session_with(scene(15.0));
        run(&mut s, "snap.set", off.clone());
        reset_counts();
        run(&mut s, command, params);
        let c = counts();
        assert_eq!((c.page_reads, c.index_builds), (1, 0), "{what}: the page is read for its grid and guides, and no shape: {c:?}");
    }
}

/// A drop on a cold page costs what it did: one page read, one index, one set of targets, all made before the edit (where the draft is asked, it finds them made); on a page the reader
/// made ready for a drop's preview it costs none.
#[test]
fn a_drop_makes_the_page_once_before_its_edit_and_finds_it_made_when_the_reader_did() {
    let drop = json!({"master": "rectangle", "page": 1, "x": 7.31, "y": 7.02, "snap": zoomed()});
    let mut cold = session_with(scene(15.0));
    reset_counts();
    run(&mut cold, "shape.drop", drop.clone());
    let c = counts();
    assert_eq!((c.page_reads, c.index_builds, c.target_builds, c.kept_found, c.kept_missing), (1, 1, 1, 0, 1), "{c:?}");
    let mut warm = session_with(scene(15.0));
    let page = warm.snap_page(1).unwrap();
    let doc = warm.document().unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), &[], Kind::Box { framed: false });
    reset_counts();
    run(&mut warm, "shape.drop", drop);
    let c = counts();
    assert_eq!((c.page_reads, c.index_builds, c.target_builds, c.kept_found, c.kept_missing), (0, 0, 0, 1, 0), "{c:?}");
}

/// A shape whose pin comes to a place that is not a number on one axis only has no pin (`pin_of` says `None`): a member of a group whose pin, added to the group's, overflows to
/// infinity in x and not in y. The angle mark of a turn is not drawn at a place that is not on the page.
#[test]
fn the_pin_of_a_shape_whose_place_overflows_on_one_axis_is_none() {
    let group = ShapeBuilder::new(1)
        .group()
        .cell("PinX", "1.7e308")
        .cell("PinY", "5")
        .cell("Width", "4")
        .cell("Height", "2")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .child(
            ShapeBuilder::new(2)
                .cell("PinX", "1.7e308")
                .cell("PinY", "1")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0")
                .cell("LocPinY", "0"),
        );
    let doc = DocBuilder::new().page(1, PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(group)).build();
    let page_shape = pin_of(&doc, 1, 2);
    assert_eq!(page_shape, None, "the member's pin is infinite in x and finite in y: {page_shape:?}");
    let (x, y) = pin_of(&doc, 1, 1).expect("the group's own pin is a place");
    assert!(x > 1.0e308 && x.is_finite() && (y - 5.0).abs() < 1e-9, "the group's own pin is where it is: {x}, {y}");
}

// ---------------------------------------------------------------------------------------------------------------- the re-review of F3's failed-batch cases, taken in

/// The page made ready by `kind` for the shapes `moving`, as the window's reader does it: the page the session keeps.
fn ready_for(s: &Session, moving: &[u32], kind: Kind) -> Arc<SnapPage> {
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), moving, kind);
    page
}

fn drop_step(x: f64, y: f64) -> Value {
    json!({"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": x, "y": y, "snap": zoomed()}})
}

/// A step that fails: a shape that is not there.
fn fail_step() -> Value {
    json!({"command": "shape.move", "params": {"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1}})
}

fn edit_step() -> Value {
    json!({"command": "shape.move", "params": {"ids": [2], "dx": 0.0, "dy": 0.5, "page": 1}})
}

/// No read of page 1 that `reads` and the session kept indexes a shape that the document does not hold (ids 1 to 200: a draft's shape has an id above the page's).
fn no_draft_shape_in_the_index(s: &Session, reads: &[Arc<SnapPage>], what: &str) {
    let doc = s.document().unwrap();
    let mut all = reads.to_vec();
    all.extend(s.kept_snap_page(1));
    for read in all {
        for id in 1..=200u32 {
            let in_index = read.shapes(doc).find(id).is_some();
            let in_document = doc.page(1).is_some_and(|p| p.shapes.iter().any(|sh| sh.id == id));
            assert!(!in_index || in_document, "{what}: shape {id}: in the index {in_index}, in the document {in_document}");
        }
    }
}

/// **The matrix of the re-review:** batches of every shape around a snapped drop (the drop first, last and between edits, two drops, a nested batch, a place, an undo and a new drawing
/// after it), each run cold and made ready for four kinds of query, on a session with a kept read and on a control with none: the batch ends as it does on the control, the document
/// and the revision are as they were, no read indexes a shape of the drop, and the probe afterwards answers what the control's does.
#[test]
fn failed_batches_around_a_snapped_drop_leave_no_draft_shape_in_the_kept_read_whatever_the_page_was_made_ready_for() {
    let nested_failing = json!({"command": "command.batch", "params": {"commands": [drop_step(7.0, 5.0), fail_step()]}});
    let nested_then_fail = json!({"command": "command.batch", "params": {"commands": [drop_step(7.0, 5.0)]}});
    let place = json!({"command": "shape.place", "params": {"ids": [1], "x": 7.31, "y": 7.02, "page": 1, "snap": zoomed()}});
    let undo = json!({"command": "edit.undo", "params": {}});
    let new = json!({"command": "file.new", "params": {}});
    let cases: Vec<(&str, Vec<Value>)> = vec![
        ("drop, fail", vec![drop_step(7.0, 5.0), fail_step()]),
        ("fail, drop", vec![fail_step(), drop_step(7.0, 5.0)]),
        ("edit, drop, fail", vec![edit_step(), drop_step(7.0, 5.0), fail_step()]),
        ("drop, drop, fail", vec![drop_step(7.0, 5.0), drop_step(9.0, 5.0), fail_step()]),
        ("drop, edit, drop, fail", vec![drop_step(7.0, 5.0), edit_step(), drop_step(9.0, 5.0), fail_step()]),
        ("drop, undo", vec![drop_step(7.0, 5.0), undo]),
        ("drop, new", vec![drop_step(7.0, 5.0), new]),
        ("a nested batch that fails", vec![nested_failing]),
        ("a nested batch, then a failure", vec![nested_then_fail, fail_step()]),
        ("place, drop, fail", vec![place, drop_step(7.0, 5.0), fail_step()]),
    ];
    let readies: [(Option<Kind>, Vec<u32>); 4] =
        [(None, vec![]), (Some(Kind::Box { framed: false }), vec![]), (Some(Kind::Box { framed: false }), vec![1]), (Some(Kind::Turn), vec![1])];
    for (what, steps) in cases {
        for (kind, moving) in &readies {
            let (mut kept, mut control) = (session_with(scene(15.0)), session_with(scene(15.0)));
            let page = match kind {
                Some(kind) => ready_for(&kept, moving, *kind),
                None => kept.snap_page(1).unwrap(),
            };
            let batch = json!({ "commands": steps });
            let revision = kept.revision();
            let (a, b) = (kept.execute("command.batch", &batch), control.execute("command.batch", &batch));
            assert_eq!(a.is_err(), b.is_err(), "{what}");
            if a.is_err() {
                assert_eq!(kept.revision(), revision, "{what}: the revision is back");
            }
            assert_eq!(kept.document(), control.document(), "{what}");
            if what != "drop, new" {
                no_draft_shape_in_the_index(&kept, &[Arc::clone(&page)], what);
            }
            match (kept.execute("shape.move", &probe()), control.execute("shape.move", &probe())) {
                (Ok(x), Ok(y)) => assert_eq!(x, y, "{what} ({kind:?} {moving:?}): the kept session and the control answer the probe alike"),
                (Err(x), Err(y)) => assert_eq!(x.to_string(), y.to_string(), "{what}"),
                (x, y) => panic!("{what}: {x:?} against {y:?}"),
            }
        }
    }
}

/// **Every stock master**, dropped with a snap in a batch whose next step fails, cold and ready: none leaves a shape of its draft in the read the session kept (the connectors among
/// them drop another way, and the shapes with many points or paths have many boxes and places of their own).
#[test]
fn every_stock_master_dropped_in_a_failing_batch_leaves_no_draft_shape_in_the_kept_read() {
    let ids: Vec<&str> = diacraft_engine::stencil::all_masters().map(|m| m.id).collect();
    assert!(ids.len() > 40, "the stock masters: {}", ids.len());
    for id in ids {
        for ready in [false, true] {
            let mut s = session_with(scene(15.0));
            let page = if ready { ready_for(&s, &[], Kind::Box { framed: false }) } else { s.snap_page(1).unwrap() };
            let drop = json!({"command": "shape.drop", "params": {"master": id, "page": 1, "x": 7.0, "y": 5.0, "snap": zoomed()}});
            let batch = json!({"commands": [drop, fail_step()]});
            assert!(s.execute("command.batch", &batch).is_err(), "{id}: the second step fails");
            no_draft_shape_in_the_index(&s, &[page], id);
        }
    }
}

/// The reader's threads make the page ready for every kind and set of shapes while failing batches of two drops run on the session: the index never holds a shape of a drop, and the
/// probe never lines up with one (150 batches, three threads).
#[test]
fn reader_threads_making_ready_while_failing_batches_with_drops_run_leave_no_draft_shape() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut s = session_with(scene(15.0));
    let page = ready_for(&s, &[], Kind::Box { framed: false });
    let doc = Arc::new(s.document().unwrap().clone());
    let settings = SnapSettings::of(&doc);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for t in 0..3u32 {
            let (page, doc, stop) = (Arc::clone(&page), Arc::clone(&doc), &stop);
            scope.spawn(move || {
                let kinds = [
                    Kind::Box { framed: false },
                    Kind::Box { framed: true },
                    Kind::Edges { framed: false },
                    Kind::Point(Use::Place),
                    Kind::Point(Use::Connect),
                    Kind::Turn,
                ];
                let mut n = t;
                while !stop.load(Ordering::SeqCst) {
                    let moving: Vec<u32> = match n % 4 {
                        0 => vec![1],
                        1 => vec![2],
                        2 => vec![],
                        _ => vec![1, 2],
                    };
                    make_ready(&doc, &page, &settings, &moving, kinds[(n as usize) % kinds.len()]);
                    n = n.wrapping_add(1);
                }
            });
        }
        for i in 0..150 {
            let batch = json!({"commands": [drop_step(7.0, 5.0), drop_step(9.0, 6.0), fail_step()]});
            assert!(s.execute("command.batch", &batch).is_err());
            no_draft_shape_in_the_index(&s, &[Arc::clone(&page)], &format!("batch {i}"));
            let answer = run(&mut s, "shape.move", probe());
            assert!(!hits(&answer), "no hit on a shape that is not there: {answer}");
        }
        stop.store(true, Ordering::SeqCst);
    });
}

/// **The two commands that snap and are not in the window's table:** `connector.reconnect` and `snap.at`, each first in a batch of a snapped drop and a failing step, and the others of
/// the table again, cold and ready: the batch ends as it began, and no read indexes the drop's shape.
#[test]
fn connector_reconnect_and_snap_at_first_in_a_failing_batch_leave_no_draft_shape_in_the_kept_read() {
    for ready in [false, true] {
        let mut s = session_with(scene(15.0));
        let connector = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 2.0, "y": 2.0}, "page": 1}));
        let id = connector["id"].as_u64().unwrap();
        let page = if ready { ready_for(&s, &[], Kind::Point(Use::Connect)) } else { s.snap_page(1).unwrap() };
        let steps = [
            (
                "connector.reconnect",
                json!({"command": "connector.reconnect", "params": {"id": id, "end": "end", "target": {"x": 7.31, "y": 7.02}, "page": 1, "snap": zoomed()}}),
            ),
            (
                "connector.add",
                json!({"command": "connector.add", "params": {"from": {"x": 7.31, "y": 7.02}, "to": {"x": 9.31, "y": 7.02}, "page": 1, "snap": zoomed()}}),
            ),
            ("snap.at", json!({"command": "snap.at", "params": {"page": 1, "x": 7.0, "y": 5.0, "zoom": 96.0}})),
            (
                "shape.rotate",
                json!({"command": "shape.rotate", "params": {"ids": [1], "angle": 31.0, "page": 1, "snap": {"zoom": 96.0, "radius": 1.5}}}),
            ),
            (
                "shape.resize",
                json!({"command": "shape.resize", "params": {"id": 1, "x": 0.5, "y": 0.5, "width": 1.31, "height": 1.02, "page": 1, "snap": zoomed()}}),
            ),
        ];
        for (what, step) in steps {
            let batch = json!({"commands": [step, drop_step(7.0, 5.0), fail_step()]});
            let revision = s.revision();
            assert!(s.execute("command.batch", &batch).is_err(), "{what}: the last step fails");
            assert_eq!(s.revision(), revision, "{what}: the revision is back");
            no_draft_shape_in_the_index(&s, &[Arc::clone(&page)], &format!("{what} (ready {ready})"));
        }
    }
}
