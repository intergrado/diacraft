//! Row 10's window half: the places a master brings with it when it is dropped, which the window's drop preview passes as the subject's own
//! places so that the ghost lands where `shape.drop` puts the shape. `MasterSpec::connection_offsets`, `snap::dropped_points` and `snap::drop_preview`: the
//! shape a drop would make, found by dropping on a copy of the document (U49).

use diacraft_engine::snap::{DropMaster, MAX_OWN_POINTS, counts, drop_preview, dropped_points, own_points, reset_counts};
use diacraft_engine::stencil::{all_masters, find_master};
use serde_json::json;

use crate::{blank_session, run};

/// Every master of every built-in stencil, dropped by `shape.drop` centred at (5.3, 4.1) on a page of the scale `k`, has the connection points
/// `dropped_points` said it would have: the same places, in the same order, and no more than the snap pulls, **at every scale and for every master** (U49: a group of compartments that
/// follow their text, beyond the range of eight of the page's scale, was once let off the height, because the places were worked out from the master and no document: the drop on a copy of
/// the document is the shape the command makes). The preview says the box of the drop too, in paper inches, as the command answers it, and moved to another place it is the preview made there.
#[test]
fn the_places_a_master_is_said_to_bring_are_the_places_its_drop_makes() {
    // 100 is beyond the range of eight of the masters' own scale: the shape keeps the master's size on paper, and its places with it.
    for k in [1.0, 2.0, 100.0] {
        let mut s = blank_session();
        if k != 1.0 {
            run(&mut s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": k}));
        }
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let scale = diacraft_engine::draw::units_per_inch(s.document().unwrap(), page);
        assert!((scale - k).abs() < 1e-9, "the page is drawn at {k} to 1: {scale}");
        for spec in all_masters() {
            // What the drop would make, asked before it is made: the preview drops on a copy and changes nothing.
            let before = s.document().cloned();
            let preview = drop_preview(s.document().unwrap(), page, DropMaster::Stock(spec), (5.3, 4.1)).expect("a preview");
            let said = dropped_points(s.document().unwrap(), page, spec, (5.3, 4.1));
            assert_eq!(s.document().cloned(), before, "{} at scale {k}: the preview changes nothing", spec.name);
            let r = run(&mut s, "shape.drop", json!({"master": spec.id, "page": page, "x": 5.3, "y": 4.1}));
            let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
            let made = own_points(s.document().unwrap(), page, &[id], (0.0, 0.0));
            assert_eq!(said.len(), made.len().min(MAX_OWN_POINTS), "{} at scale {k}: how many", spec.name);
            // The box the drop answers is the box the preview has: a Pool's drop makes a structure and answers its own, so it is left out.
            if !spec.makes_structure() {
                let (w, h) = (r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap());
                assert!(
                    (preview.width() - w).abs() < 1e-9 && (preview.height() - h).abs() < 1e-9,
                    "{} at scale {k}: the preview is {} by {} inches, the drop {w} by {h}",
                    spec.name,
                    preview.width(),
                    preview.height()
                );
            }
            // Moved to another place the preview is the one made there: the box and the places, which a window works out once for a drag and moves with the pointer.
            let there = (9.1, 2.2);
            let again = drop_preview(&before.clone().unwrap(), page, DropMaster::Stock(spec), there).expect("a preview there");
            let (a, b) = (preview.rect_at(there), again.rect_at(there));
            assert!(
                (a.x0 - b.x0).abs() < 1e-9 && (a.y0 - b.y0).abs() < 1e-9 && (a.x1 - b.x1).abs() < 1e-9 && (a.y1 - b.y1).abs() < 1e-9,
                "{} at scale {k}: the box moved is the box made there",
                spec.name
            );
            for (p, q) in preview.points_at(there).iter().zip(again.points_at(there)) {
                assert!(
                    (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9,
                    "{} at scale {k}: the places moved are the places made there",
                    spec.name
                );
            }
            for (i, (a, b)) in said.iter().zip(&made).enumerate() {
                assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "{} at scale {k}, point {i}: said {a:?}, made {b:?}", spec.name);
            }
            // A position dropped inside another becomes its report and is laid out: the next one is dropped on bare paper.
            if spec.id.starts_with("orgchart.") {
                run(&mut s, "shape.delete", json!({"ids": [id]}));
            }
            // A master whose drop makes a structure of swimlanes (BPMN's Pool) is undone, so that the next lands on bare paper: a Lane dropped over a pool is put into it and is no shape of its own.
            if spec.makes_structure() {
                run(&mut s, "edit.undo", json!({}));
            }
        }
    }
}

/// A rectangle has the middles of its four sides, from the corner of its box at its size (1.5 by 1); a one-dimensional master has none; a master's points
/// never come out as more than the snap pulls.
#[test]
fn a_masters_places_are_the_middles_of_a_rectangles_sides_and_none_for_a_connector() {
    let rectangle = find_master("rectangle").unwrap();
    let offsets = rectangle.connection_offsets();
    assert_eq!(offsets.len(), 4);
    for want in [(0.75, 1.0), (0.0, 0.5), (0.75, 0.0), (1.5, 0.5)] {
        assert!(offsets.iter().any(|o| (o.0 - want.0).abs() < 1e-12 && (o.1 - want.1).abs() < 1e-12), "{want:?} in {offsets:?}");
    }
    let s = blank_session();
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    assert_eq!(dropped_points(doc, page, rectangle, (10.0, 10.0)).len(), 4);
    let flow = find_master("gane-sarson.data-flow").unwrap();
    assert!(flow.is_connector());
    assert!(flow.connection_offsets().is_empty() && dropped_points(doc, page, flow, (1.0, 1.0)).is_empty());
    // Not a number the snap can use, and a page that is not there: nothing.
    assert!(dropped_points(doc, page, rectangle, (f64::NAN, 1.0)).is_empty());
    assert!(dropped_points(doc, 99, rectangle, (1.0, 1.0)).is_empty());
}

/// What a preview costs, counted and not timed (U49): a window asks for one **for a drag**, so what it costs is one drop on a copy of the document. Each is counted
/// (`snap::counts().drops`); the places and the box moved to another place make none; a master whose drop is a structure made by code (BPMN's Pool) and a page that is not there make none;
/// and the calculation it makes (the cells the drop writes and what reads them) is the same on a page of 800 shapes as on a blank one: the copy of the document costs a few reference counts and
/// the drop's own recalculation does not grow with the page.
#[test]
fn a_preview_is_one_drop_on_a_copy_and_costs_no_more_on_a_crowded_page() {
    let s = blank_session();
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let class = find_master("uml-class.class").unwrap();
    reset_counts();
    let preview = drop_preview(doc, page, DropMaster::Stock(class), (5.3, 4.1)).expect("a preview");
    assert_eq!(counts().drops, 1);
    let _ = (preview.points_at((1.0, 1.0)), preview.rect_at((1.0, 1.0)), preview.width(), preview.height());
    assert_eq!(counts().drops, 1, "moving a preview drops nothing");
    assert!(
        drop_preview(doc, 99, DropMaster::Stock(class), (5.3, 4.1)).is_none()
            && drop_preview(doc, page, DropMaster::Stock(class), (f64::NAN, 4.1)).is_none()
    );
    assert_eq!(counts().drops, 1, "a place or a page that is not there makes no drop");
    let pool = all_masters().find(|m| m.makes_structure()).expect("a master that makes a structure");
    assert!(drop_preview(doc, page, DropMaster::Stock(pool), (5.3, 4.1)).is_some());
    assert_eq!(counts().drops, 1, "a structure made by code has no shape to drop on a copy");

    // The recalculation of a preview of a group at 100 to 1 (it writes `DropOnPageScale`, `Width` and `Height`): the same on a blank page and a page of 500 shapes and 300 connectors.
    let steps = |s: &mut diacraft_engine::Session| {
        run(s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": 100.0}));
        diacraft_calc::work::reset();
        drop_preview(s.document().unwrap(), 1, DropMaster::Stock(class), (5.3, 4.1)).expect("a preview");
        (diacraft_calc::work::commits(), diacraft_calc::work::steps())
    };
    let blank = steps(&mut blank_session());
    let crowded = steps(&mut crate::edit_cost::big_page());
    assert!(blank.0 >= 1 && blank.1 > 0, "the drop beyond the range works its cells out: {blank:?}");
    // The same commits, and the steps differ by a few (3 when the two were merged), which a cost that grew with the page's 800 shapes could not be: the calculation of a commit looks
    // at what the document's other pages and masters name (which came after this test), a constant of what the document holds beside the drop's own cells.
    assert_eq!(crowded.0, blank.0, "commits of a preview on a crowded page and a blank one");
    assert!(crowded.1.abs_diff(blank.1) <= 16, "steps of a preview on a crowded page and a blank one: {crowded:?} and {blank:?}");
}

// ---------------------------------------------------------------------------------------------------------------- the master of a stencil file

/// A stencil of the masters "Offset" (2 by 1, one connection point 0.3 of the width and 0.9 of the height from its lower left corner: where no corner and no middle of its
/// box is), "Process" and "Decision" (the stencil tests' own: points at the middles of the sides).
fn stencil_with_an_off_place_point() -> Vec<u8> {
    use crate::stencil_open::{CT_STENCIL, LOOK, box_cells, decision_master, ids, process_master};
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
    let point = r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="0.6" F="Width*0.3"/><Cell N="Y" V="0.9" F="Height*0.9"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row></Section>"#;
    // The shape's box cells carry four points of their own; the master here keeps the geometry and the sizes, and has the one point.
    let cells = box_cells(2.0, 1.0);
    let cells = cells.split("<Section N=\"Connection\">").next().unwrap_or_default().to_owned();
    let offset = MasterSpec::new(7, "Offset").attrs(&ids(107, 207)).shapes(&shape(5, &format!("{LOOK}{cells}{point}")));
    VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(offset)
        .master(process_master())
        .master(decision_master())
        .page(PageSpec::new(1, "Page-1").without_part())
        .build()
}

/// The places a master of a stencil file is said to bring (`dropped_points_of_file`) are the places its drop makes, in the same order and to the bit, at page scales 1 and 2, for
/// a master whose point lies off every place of its box and for masters with the four middles; the accessor changes nothing (it reads a document, and drops on a copy of it).
#[test]
fn the_places_a_file_master_is_said_to_bring_are_the_places_its_drop_makes() {
    use crate::stencil_open::Dir;
    use diacraft_engine::snap::dropped_points_of_file;
    let dir = Dir::new();
    let path = dir.write("Offset.vssx", &stencil_with_an_off_place_point());
    // At 100 the masters' own scale is beyond the range of eight of the page's: the shape is the master's times the ratio, which is `k`.
    for k in [1.0, 2.0, 100.0] {
        let ratio = if k > 8.0 { k } else { 1.0 };
        let mut s = blank_session();
        if k != 1.0 {
            run(&mut s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": k}));
        }
        let opened = run(&mut s, "stencil.open", json!({"path": path}));
        let id = opened["id"].as_str().unwrap().to_owned();
        let stencil = s.stencils().get(&id).unwrap().clone();
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        for key in ["Offset", "Process", "Decision"] {
            let before = s.document().cloned();
            let said = dropped_points_of_file(s.document().unwrap(), page, &stencil, key, (5.3, 4.1));
            assert_eq!(s.document().cloned(), before, "{key}: the accessor changes nothing");
            let r = run(&mut s, "shape.drop", json!({"master": key, "stencil": id, "page": page, "x": 5.3, "y": 4.1}));
            let made = own_points(s.document().unwrap(), page, &[u32::try_from(r["id"].as_u64().unwrap()).unwrap()], (0.0, 0.0));
            assert!(!said.is_empty(), "{key} at scale {k}: it has points");
            assert_eq!(said, made, "{key} at scale {k}: the same places in the same order, to the bit");
        }
        // The first master's one point is where its formulas put it, on the page, and is no corner and no middle of its box.
        let said = dropped_points_of_file(s.document().unwrap(), page, &stencil, "Offset", (5.3, 4.1));
        assert_eq!(said.len(), 1);
        let (w, h) = (2.0 * ratio / k, 1.0 * ratio / k);
        let want = (5.3 - w / 2.0 + 0.6 * ratio / k, 4.1 - h / 2.0 + 0.9 * ratio / k);
        assert!((said[0].0 - want.0).abs() < 1e-9 && (said[0].1 - want.1).abs() < 1e-9, "scale {k}: {:?} against {want:?}", said[0]);
        // A master that is not there, a page that is not, a place that is not a number: none.
        assert!(dropped_points_of_file(s.document().unwrap(), page, &stencil, "Nothing like it", (5.3, 4.1)).is_empty());
        assert!(dropped_points_of_file(s.document().unwrap(), 99, &stencil, "Offset", (5.3, 4.1)).is_empty());
        assert!(dropped_points_of_file(s.document().unwrap(), page, &stencil, "Offset", (f64::NAN, 4.1)).is_empty());
    }
}

/// A file master dropped near a target's connection point lands where the preview the window makes puts it: its box at the master's size, its own place from the accessor, and
/// the snap's pull; the point that pulls is the one off every place of the master's box, which a preview with no accessor would not have pulled.
#[test]
fn a_file_master_previews_where_it_drops_by_a_point_off_the_places_of_its_box() {
    use crate::stencil_open::Dir;
    use diacraft_engine::snap::{Extras, Query, Rect, SnapPage, SnapSettings, Subject, Use, box_of, dropped_points_of_file, snap_ext};
    let dir = Dir::new();
    let path = dir.write("Offset.vssx", &stencil_with_an_off_place_point());
    for k in [1.0, 2.0, 0.5] {
        let mut s = blank_session();
        if k != 1.0 {
            run(&mut s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": k}));
        }
        run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
        let id = run(&mut s, "stencil.open", json!({"path": path}))["id"].as_str().unwrap().to_owned();
        let stencil = s.stencils().get(&id).unwrap().clone();
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        // The target: a stock rectangle, whose leftmost connection point is the middle of its left side.
        let t = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 8.0, "y": 4.0}));
        let tid = u32::try_from(t["id"].as_u64().unwrap()).unwrap();
        let target_point =
            own_points(s.document().unwrap(), page, &[tid], (0.0, 0.0)).into_iter().fold((f64::INFINITY, 0.0), |a, p| if p.0 < a.0 { p } else { a });
        // The master's point is 0.6 right of and 0.9 above its lower left corner (drawing inches), and its box's middle is 1.0 and 0.5 from that corner.
        let (w, h) = (2.0 / k, 1.0 / k);
        let off = ((0.6 - 1.0) / k, (0.9 - 0.5) / k);
        let centre = (target_point.0 - off.0 + 0.03, target_point.1 - off.1 - 0.02);
        let centre = ((centre.0 * 1e6).round() / 1e6, (centre.1 * 1e6).round() / 1e6);
        let doc = s.document().unwrap();
        let read = SnapPage::read(doc, page).unwrap();
        let rect = Rect { x0: centre.0 - w / 2.0, y0: centre.1 - h / 2.0, x1: centre.0 + w / 2.0, y1: centre.1 + h / 2.0 };
        let own = dropped_points_of_file(doc, page, &stencil, "Offset", centre);
        let query = Query { subject: Subject::Box(rect), using: Use::Place, moving: &[], zoom: 96.0, own_points: &own };
        let view = Some([0.0, 0.0, 20.0, 20.0]);
        let with = snap_ext(doc, &read, &SnapSettings::of(doc), &query, Extras { view, ..Extras::default() });
        let query = Query { own_points: &[], ..query };
        let without = snap_ext(doc, &read, &SnapSettings::of(doc), &query, Extras { view, ..Extras::default() });
        assert!(
            (with.dx.hypot(with.dy) - 0.036).abs() < 0.01,
            "scale {k}: the point is pulled to the target's, by about the 0.036 it is off: {:?} {:?}",
            (with.dx, with.dy),
            with.hits
        );
        assert!((without.dx, without.dy) != (with.dx, with.dy), "scale {k}: a preview with no places of its own would show another place");
        let ghost = (centre.0 + with.dx, centre.1 + with.dy);
        let snap = json!({"zoom": 96.0, "view": [0.0, 0.0, 20.0, 20.0]});
        let r = run(&mut s, "shape.drop", json!({"master": "Offset", "stencil": id, "page": page, "x": centre.0, "y": centre.1, "snap": snap}));
        let made = box_of(s.document().unwrap(), page, &[u32::try_from(r["id"].as_u64().unwrap()).unwrap()]).unwrap();
        let middle = ((made.x0 + made.x1) / 2.0, (made.y0 + made.y1) / 2.0);
        assert!((middle.0 - ghost.0).abs() < 1e-9 && (middle.1 - ghost.1).abs() < 1e-9, "scale {k}: the drop is the ghost: {middle:?} {ghost:?}");
    }
}

/// A timing for the report and not a check (a time moves with the load on the machine: the counted test above is the check): what one preview takes (the least of five) for each master of the built-in stencils,
/// on a page at 100 to 1 that is blank, holds 1,000 shapes, holds 5,000, and (with the page blank) in a drawing that holds every master of the built-in stencils. Run with `cargo test -p diacraft-engine --test it what_a_preview_takes -- --ignored --nocapture`.
#[test]
#[ignore = "a timing for the report, not a check"]
fn what_a_preview_takes_on_pages_of_different_sizes() {
    use std::time::Instant;
    let rectangle = find_master("rectangle").unwrap();
    for (shapes, every_master) in [(0_u32, false), (1_000, false), (5_000, false), (0, true)] {
        let mut s = blank_session();
        run(&mut s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": 100.0}));
        s.edit("build", |doc| {
            for i in 0..shapes {
                let (x, y) = (1.0 + f64::from(i % 100) * 0.8, 1.0 + f64::from(i / 100) * 0.5);
                diacraft_engine::draw::drop_shape(doc, 1, rectangle, x, y)
                    .map_err(|p| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: p.message })?;
            }
            // Every master of the built-in stencils is in the drawing once, and the shapes are taken off the page again: what is left is a drawing that holds them all.
            if every_master {
                for spec in all_masters().filter(|m| !m.makes_structure()) {
                    diacraft_engine::draw::drop_shape(doc, 1, spec, 5.0, 5.0)
                        .map_err(|p| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: p.message })?;
                }
                let ids: Vec<u32> = doc.page(1).map(|p| p.shapes.iter().map(|shape| shape.id).collect()).unwrap_or_default();
                diacraft_engine::draw::delete_shapes(doc, 1, &ids)
                    .map_err(|p| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: p.message })?;
            }
            Ok(())
        })
        .unwrap();
        let doc = s.document().unwrap();
        let mut times: Vec<(std::time::Duration, &str)> = Vec::new();
        for spec in all_masters().filter(|m| !m.makes_structure()) {
            // The least of five, so that another process's burst on a loaded machine is not what is reported.
            let took = (0..5)
                .map(|_| {
                    let from = Instant::now();
                    let preview = drop_preview(doc, 1, DropMaster::Stock(spec), (5.3, 4.1));
                    assert!(preview.is_some(), "{}", spec.name);
                    from.elapsed()
                })
                .min()
                .unwrap_or_default();
            times.push((took, spec.id));
        }
        times.sort();
        let (median, worst) = (times[times.len() / 2], times[times.len() - 1]);
        let class = times.iter().find(|t| t.1 == "uml-class.class").unwrap();
        eprintln!(
            "{shapes} shapes, {} masters in the drawing: {} previews, median {:?}, slowest {:?} ({}), uml-class.class {:?}",
            doc.masters.len(),
            times.len(),
            median.0,
            worst.0,
            worst.1,
            class.0
        );
    }
}
