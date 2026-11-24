//! Of the review: the commands that snap find the page the window's reader made ready, and build no index of the shapes and no
//! set of targets on the thread that runs them. Work is **counted** (`snap::counts`, per thread), never timed; the yardstick that times a release beside a plain move is ignored.
//! The settings are the spec's "Settings for vectors": the grid and the ruler off, the rest of a new drawing's; paper inches, y up, 96 screen points to an inch.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::snap::{Counts, Kind, SnapPage, SnapSettings, Use, counts, make_ready, reset_counts};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::{blank_session, run};

/// A page of `n` 1 by 1 inch rectangles on a grid 1.5 inches apart, 100 to a row, ids from 1; shape 1 is at (1, 1), the one that moves.
pub(crate) fn rects(n: u32) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &(f64::from(i % 100) * 1.5 + 1.0).to_string())
                .cell("PinY", &(f64::from(i / 100) * 1.5 + 1.0).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

/// A session holding `doc`, the grid and the ruler off.
pub(crate) fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
    s
}

/// The page the window asks the session for, made ready by `moving` and `kind` as the window's reader does it.
pub(crate) fn made_ready(s: &Session, moving: &[u32], kind: Kind) -> Arc<SnapPage> {
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), moving, kind);
    page
}

/// The snap word a drag sends: the zoom and the view.
pub(crate) fn snap() -> Value {
    json!({"zoom": 96.0, "view": [0.0, 0.0, 30.0, 30.0]})
}

/// Every command that snaps, with what the window makes the page ready for before it sends it: (name, command, params, the shapes that move, the kind of query).
pub(crate) fn commands() -> Vec<(&'static str, &'static str, Value, Vec<u32>, Kind)> {
    vec![
        ("a release", "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": snap()}), vec![1], Kind::Box { framed: false }),
        (
            "an arrow press",
            "shape.move",
            json!({"ids": [1], "dx": 1.0, "dy": 0.0, "page": 1, "snap": {"zoom": 96.0, "view": [0.0, 0.0, 30.0, 30.0], "nudge": {"tick": 0.125}}}),
            vec![1],
            Kind::Box { framed: false },
        ),
        (
            "an arrow press by the ruler's tick at the zoom",
            "shape.move",
            json!({"ids": [1], "dx": 0.0, "dy": 1.0, "page": 1, "snap": {"zoom": 96.0, "nudge": {}}}),
            vec![1],
            Kind::Box { framed: false },
        ),
        ("a turn", "shape.rotate", json!({"ids": [1], "angle": 31.0, "page": 1, "snap": {"zoom": 96.0, "radius": 1.5}}), vec![1], Kind::Turn),
        (
            "a resize",
            "shape.resize",
            json!({"id": 1, "x": 0.5, "y": 0.5, "width": 1.31, "height": 1.02, "page": 1, "snap": snap()}),
            vec![1],
            Kind::Edges { framed: false },
        ),
        (
            "a drop",
            "shape.drop",
            json!({"master": "rectangle", "page": 1, "x": 7.31, "y": 7.02, "snap": snap()}),
            vec![],
            Kind::Box { framed: false },
        ),
        (
            "a connector's free end",
            "connector.add",
            json!({"from": {"x": 7.31, "y": 7.02}, "to": {"x": 9.31, "y": 7.02}, "page": 1, "snap": snap()}),
            vec![],
            Kind::Point(Use::Connect),
        ),
    ]
}

fn spent(c: Counts) -> (usize, usize, usize) {
    (c.page_reads, c.index_builds, c.target_builds)
}

/// A snapped command on a page the reader made ready finds it, and reads and builds nothing on its thread: no page, no index of the shapes, no set of targets. On a page nobody made
/// ready (a cold one) the same command reads and builds as it did before the cache: one read, one index, one set (and nothing is lost: it is the same answer, the next test).
#[test]
fn a_snapped_command_on_a_page_that_is_ready_builds_no_index_and_no_targets_and_a_cold_one_does() {
    for (what, command, params, moving, kind) in commands() {
        let mut warm = session_of(rects(2_000));
        made_ready(&warm, &moving, kind);
        reset_counts();
        run(&mut warm, command, params.clone());
        let c = counts();
        assert_eq!((spent(c), c.kept_found, c.kept_missing), ((0, 0, 0), 1, 0), "{what}: a ready page is found and nothing is made: {c:?}");
        // The same on a cold page: it reads as it did.
        let mut cold = session_of(rects(2_000));
        reset_counts();
        run(&mut cold, command, params);
        let c = counts();
        assert_eq!((spent(c), c.kept_found, c.kept_missing), ((1, 1, 1), 0, 1), "{what}: a cold page is read by the command, as before: {c:?}");
    }
}

/// The same on the review's page of 10,000 shapes: a release and an arrow press on a ready page build no index and no targets, and a plain move (no snap) asks nothing of the
/// snap at all.
#[test]
fn a_release_and_an_arrow_press_on_a_ready_page_of_ten_thousand_shapes_build_nothing() {
    let mut s = session_of(rects(10_000));
    made_ready(&s, &[1], Kind::Box { framed: false });
    reset_counts();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": snap()}));
    let release = counts();
    assert_eq!((spent(release), release.kept_found), ((0, 0, 0), 1), "{release:?}");
    // The release is an edit: the page is the window's to make ready again before the next press.
    made_ready(&s, &[1], Kind::Box { framed: false });
    reset_counts();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0, "page": 1, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}));
    let press = counts();
    assert_eq!((spent(press), press.kept_found), ((0, 0, 0), 1), "{press:?}");
    reset_counts();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0, "page": 1}));
    assert_eq!(counts(), Counts::default(), "a plain move reads nothing of the snap");
}

/// A command on a ready page answers what it answers on a cold one, to the bit: the same snap, the same boxes. Each of 40 random scenes of a dozen rectangles (some turned,
/// some alike, so that their own sides meet) has the shape that moves dragged, pressed, turned, resized and dropped, by the same numbers on a session whose page was made ready
/// and on one whose page was not.
#[test]
fn a_command_on_a_ready_page_answers_what_it_answers_on_a_cold_one_to_the_bit() {
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
        fn between(&mut self, a: f64, b: f64) -> f64 {
            a + (b - a) * self.next()
        }
    }
    let mut r = Rng(0x5eed);
    let mut snapped = 0usize;
    for scene in 0..40 {
        let mut page = PageBuilder::new().cell("PageWidth", "30").cell("PageHeight", "30");
        for id in 1..=12u32 {
            let degrees: f64 = [0.0, 30.0, 30.0, 75.0][((r.next() * 4.0) as usize).min(3)];
            page = page.shape(
                ShapeBuilder::new(id)
                    .cell("PinX", &r.between(2.0, 20.0).to_string())
                    .cell("PinY", &r.between(2.0, 20.0).to_string())
                    .cell("Width", &r.between(0.5, 2.5).to_string())
                    .cell("Height", &r.between(0.5, 2.0).to_string())
                    .cell("LocPinX", "0.5")
                    .cell("LocPinY", "0.5")
                    .cell("Angle", &degrees.to_radians().to_string()),
            );
        }
        let doc = DocBuilder::new().page(1, page).build();
        let (dx, dy) = (r.between(-1.5, 1.5), r.between(-1.5, 1.5));
        let steps = [
            ("shape.move", json!({"ids": [1], "dx": dx, "dy": dy, "page": 1, "snap": snap()}), vec![1], Kind::Box { framed: true }),
            (
                "shape.move",
                json!({"ids": [2], "dx": dx, "dy": 0.0, "page": 1, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}),
                vec![2],
                Kind::Box { framed: true },
            ),
            (
                "shape.rotate",
                json!({"ids": [3], "angle": r.between(0.0, 90.0), "page": 1, "snap": {"zoom": 96.0, "radius": 1.5}}),
                vec![3],
                Kind::Turn,
            ),
            (
                "shape.resize",
                json!({"id": 4, "x": 5.0, "y": 5.0, "width": r.between(0.5, 3.0), "height": r.between(0.5, 3.0), "page": 1, "snap": snap()}),
                vec![4],
                Kind::Edges { framed: true },
            ),
            (
                "shape.drop",
                json!({"master": "rectangle", "page": 1, "x": r.between(2.0, 20.0), "y": r.between(2.0, 20.0), "snap": snap()}),
                vec![],
                Kind::Box { framed: false },
            ),
        ];
        for (command, params, moving, kind) in steps {
            let (mut cold, mut warm) = (session_of(doc.clone()), session_of(doc.clone()));
            // A turned subject is asked with its own frame, an upright one without: the page is made ready for the kind that matches, as the window does it.
            let turned = diacraft_engine::snap::frame_of_shapes(warm.document().unwrap(), 1, &moving).is_some();
            let kind = match kind {
                Kind::Box { .. } => Kind::Box { framed: turned },
                Kind::Edges { .. } => Kind::Edges { framed: diacraft_engine::snap::snaps_in_its_own_axes(warm.document().unwrap(), 1, 4) },
                other => other,
            };
            made_ready(&warm, &moving, kind);
            reset_counts();
            let a = run(&mut warm, command, params.clone());
            let built = spent(counts());
            let b = run(&mut cold, command, params);
            assert_eq!(a, b, "scene {scene}, {command}: the answer is the same on a ready page and a cold one");
            assert_eq!(warm.document(), cold.document(), "scene {scene}, {command}: the document is the same, to the bit");
            assert_eq!(built.1 + built.2, 0, "scene {scene}, {command}: a ready page builds nothing ({built:?})");
            snapped += usize::from(a["snap"]["hits"].as_array().is_some_and(|h| !h.is_empty()));
        }
    }
    assert!(snapped > 30, "the scenes are not vacuous: {snapped} of 200 commands snapped to something");
}

/// An edit moves the revision, and the kept page is then not the document's: the next command reads for itself. So does undo (it moves the revision forward, to the document
/// before), and the page the session gives the window next is a new read.
#[test]
fn an_edit_and_an_undo_make_the_kept_page_not_found() {
    let mut s = session_of(rects(50));
    let first = made_ready(&s, &[1], Kind::Box { framed: false });
    assert!(Arc::ptr_eq(&s.kept_snap_page(1).unwrap(), &first), "found for the revision it was made at");
    assert!(Arc::ptr_eq(&s.snap_page(1).unwrap(), &first), "and the session gives the window the same one while the drawing is as it was");
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 1.0, "dy": 0.0, "page": 1}));
    reset_counts();
    assert!(s.kept_snap_page(1).is_none(), "an edit moved the revision");
    assert_eq!(counts().kept_missing, 1);
    let second = s.snap_page(1).unwrap();
    assert!(!Arc::ptr_eq(&second, &first), "a new read for the new revision");
    assert!(!second.shapes_read(), "and it is not made ready yet");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.kept_snap_page(1).is_none(), "undo moved the revision too");
    let third = s.snap_page(1).unwrap();
    assert!(
        !Arc::ptr_eq(&third, &second) && !Arc::ptr_eq(&third, &first),
        "the document before the edit is read again: the old read was dropped when the edit made it stale"
    );
    // A snapped command after the edit does not use the page made ready before it: it finds none, and reads (a stale page would have the shape where it was).
    let mut s = session_of(rects(50));
    made_ready(&s, &[1], Kind::Box { framed: false });
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 3.0, "page": 1}));
    reset_counts();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": snap()}));
    let c = counts();
    assert_eq!((spent(c), c.kept_found, c.kept_missing), ((1, 1, 1), 0, 1), "{c:?}");
}

/// A batch that edits, snaps and then fails puts the revision back. The snap inside it asked for the page at the revision its edit reached, which is not the kept read's, and
/// dropped the kept read: the next command reads (cold, and right). And the next edit, which reaches the revision the failed batch reached on its way, finds no read of what the
/// batch made.
#[test]
fn a_failed_batch_leaves_no_read_of_what_it_edited() {
    let mut s = session_of(rects(50));
    let first = made_ready(&s, &[1], Kind::Box { framed: false });
    let batch = json!({"commands": [
        {"command": "shape.move", "params": {"ids": [2], "dx": 1.0, "dy": 0.0, "page": 1}},
        {"command": "shape.move", "params": {"ids": [1], "dx": 0.3, "dy": 0.0, "page": 1, "snap": snap()}},
        {"command": "shape.move", "params": {"ids": [999], "dx": 1.0, "dy": 0.0, "page": 1}},
    ]});
    let revision = s.revision();
    assert!(s.execute("command.batch", &batch).is_err(), "the third step names a shape that is not there");
    assert_eq!(s.revision(), revision, "the revision is back");
    assert!(
        s.kept_snap_page(1).is_none(),
        "the snap inside the batch looked at another revision and dropped the read: the page is read again, never found stale"
    );
    assert!(s.snap_reads().is_empty());
    let second = s.snap_page(1).unwrap();
    assert!(!Arc::ptr_eq(&second, &first));
    // The next edit reaches the revision the failed batch reached on its way, and finds no read made there.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 2.0, "page": 1}));
    assert!(s.kept_snap_page(1).is_none());
}

/// The set of targets is keyed by the shapes that move **that the index holds**: a shape that is not there (the new shape of a drop, which the page was read without) leaves nothing
/// out of the set, so the set made ready for a drop's preview, with nothing moving, is the one the command finds.
#[test]
fn a_moving_shape_the_index_does_not_hold_makes_no_other_set_of_targets() {
    let s = session_of(rects(30));
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    let settings = SnapSettings { alignment_box: true, ..SnapSettings::of(doc) };
    let ask = |moving: &[u32]| {
        let query = diacraft_engine::snap::Query {
            subject: diacraft_engine::snap::Subject::Box(diacraft_engine::snap::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }),
            using: Use::Place,
            moving,
            zoom: 96.0,
            own_points: &[],
        };
        diacraft_engine::snap::snap(doc, &page, &settings, &query)
    };
    ask(&[]);
    assert_eq!(page.target_builds(), 1);
    ask(&[900_001]);
    assert_eq!(page.target_builds(), 1, "a shape the page does not hold leaves nothing out");
    ask(&[900_001, 900_002]);
    assert_eq!(page.target_builds(), 1);
    ask(&[5]);
    assert_eq!(page.target_builds(), 2, "a shape the index holds is left out, and the set differs");
    ask(&[5, 900_001]);
    assert_eq!(page.target_builds(), 2, "and the shape it does not hold changes nothing");
    ask(&[6]);
    assert_eq!(page.target_builds(), 3);
}

/// The angle mark of a turn is drawn at the shape's pin, the point `shape.rotate` turns it about, and not at the middle of its box: with the pin moved to the top left of
/// the box, `shape.rotate`'s answer and `snap.at`'s both say the pin, and `pin_of` is where the shape's pin is (the middle of the box for a shape not pinned elsewhere).
#[test]
fn a_turns_angle_mark_is_at_the_pin_of_the_shape_and_not_the_middle_of_its_box() {
    let mut s = session_of(rects(5));
    // Shape 1 is the 1 by 1 box from (0.5, 0.5) to (1.5, 1.5), its pin at its middle.
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
    assert!(near(diacraft_engine::snap::pin_of(s.document().unwrap(), 1, 1).unwrap(), (1.0, 1.0)));
    assert_eq!(diacraft_engine::snap::pin_of(s.document().unwrap(), 1, 999), None, "a shape that is not there has no pin");
    run(&mut s, "shape.pin", json!({"ids": [1], "at": "top-left", "page": 1}));
    assert!(near(diacraft_engine::snap::pin_of(s.document().unwrap(), 1, 1).unwrap(), (0.5, 1.5)), "the pin is the top left now");
    let pivot = |marks: &Value| {
        let m = marks.as_array().unwrap().iter().find(|m| m["mark"] == "angle").expect("an angle mark").clone();
        (m["pivot"][0].as_f64().unwrap(), m["pivot"][1].as_f64().unwrap())
    };
    let at = run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "rotate": {"angle": 33.0, "radius": 1.5}}));
    assert!(near(pivot(&at["marks"]), (0.5, 1.5)), "snap.at: {at}");
    let turned = run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 33.0, "page": 1, "snap": {"zoom": 96.0, "radius": 1.5}}));
    assert!(near(pivot(&turned["snap"]["marks"]), (0.5, 1.5)), "shape.rotate: {turned}");
    // The shape turned about its pin: the pin has not moved.
    assert!(near(diacraft_engine::snap::pin_of(s.document().unwrap(), 1, 1).unwrap(), (0.5, 1.5)), "the pin stayed");
}

/// The measurement: the review's yardstick, a snapped release beside the plain move, on the page of 10,000 shapes, with the page ready and cold. Run it with `--ignored`.
#[test]
#[ignore = "a measurement: it times, and a test never asserts a time"]
fn yardstick_a_release_beside_the_plain_move_on_ten_thousand_shapes() {
    use std::time::Instant;
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let mut s = session_of(rects(10_000));
    let t = Instant::now();
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.01, "dy": 0.0, "page": 1}));
    let plain = ms(t);
    let t = Instant::now();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": snap()}));
    let cold = ms(t);
    let t = Instant::now();
    made_ready(&s, &[1], Kind::Box { framed: false });
    let ready_in = ms(t);
    let t = Instant::now();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.31, "dy": 0.02, "page": 1, "snap": snap()}));
    let warm = ms(t);
    made_ready(&s, &[1], Kind::Box { framed: false });
    let t = Instant::now();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0, "page": 1, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}));
    let press = ms(t);
    eprintln!(
        "YARDSTICK 10,000 shapes: plain move {plain:.1} ms; a release on a cold page {cold:.1} ms; the reader's work {ready_in:.1} ms; a release on a ready page {warm:.1} ms; an arrow press on a ready page {press:.1} ms"
    );
}

/// The review's own page: 10,000 stock shapes, every one turned (the budget of 32 times a query's pays for all of them): the commands beside the plain move, a release on a cold
/// page and on a ready one, an arrow press, a turn and a resize on a ready one, and what the reader's thread spends making the page ready. Run it with `--ignored`.
#[test]
#[ignore = "a measurement: it times, and a test never asserts a time"]
fn yardstick_the_commands_on_ten_thousand_turned_stock_shapes_beside_the_plain_move() {
    use std::time::Instant;
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let mut s = session_of(crate::snap_adoption::turned_stock_page(10_000));
    let time = |s: &mut Session, id: &str, p: Value| {
        let t = Instant::now();
        run(s, id, p);
        ms(t)
    };
    let plain = time(&mut s, "shape.move", json!({"ids": [2], "dx": 0.01, "dy": 0.0, "page": 1}));
    let cold = time(&mut s, "shape.move", json!({"ids": [1], "dx": 0.01, "dy": 0.0, "page": 1, "snap": snap()}));
    let t = Instant::now();
    made_ready(&s, &[1], Kind::Box { framed: true });
    let reader = ms(t);
    let release = time(&mut s, "shape.move", json!({"ids": [1], "dx": 0.01, "dy": 0.0, "page": 1, "snap": snap()}));
    made_ready(&s, &[1], Kind::Box { framed: true });
    let press = time(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0, "page": 1, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}));
    made_ready(&s, &[1], Kind::Turn);
    let turn = time(&mut s, "shape.rotate", json!({"ids": [1], "angle": 31.0, "page": 1, "snap": {"zoom": 96.0, "radius": 1.5}}));
    made_ready(&s, &[3], Kind::Edges { framed: true });
    let resize = time(&mut s, "shape.resize", json!({"id": 3, "x": 0.5, "y": 0.5, "width": 1.3, "height": 0.9, "page": 1, "snap": snap()}));
    eprintln!(
        "YARDSTICK 10,000 turned stock shapes: plain move {plain:.1} ms; a release on a cold page {cold:.1} ms; the reader's work {reader:.1} ms; on a ready page: release {release:.1} ms, arrow press {press:.1} ms, turn {turn:.1} ms, resize {resize:.1} ms; the page holds {} bytes by the charge",
        s.snap_reads().held_bytes()
    );
}
