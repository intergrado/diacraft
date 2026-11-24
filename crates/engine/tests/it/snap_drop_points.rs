//! Row 10's window half: the places a master brings with it when it is dropped, which the window's drop preview passes as the subject's own
//! places so that the ghost lands where `shape.drop` puts the shape. `MasterSpec::connection_offsets` and `snap::dropped_points`.

use diacraft_engine::snap::{MAX_OWN_POINTS, dropped_points, own_points};
use diacraft_engine::stencil::{all_masters, find_master};
use serde_json::json;

use crate::{blank_session, run};

/// Every master of every built-in stencil, dropped by `shape.drop` centred at (5.3, 4.1) on a page of the scale `k`, has the connection points
/// `dropped_points` said it would have: the same places, in the same order, and no more than the snap pulls.
#[test]
fn the_places_a_master_is_said_to_bring_are_the_places_its_drop_makes() {
    for k in [1.0, 2.0] {
        let mut s = blank_session();
        if k != 1.0 {
            run(&mut s, "page.scale", json!({"page": 1, "paper": 1.0, "drawing": k}));
        }
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let scale = diacraft_engine::draw::units_per_inch(s.document().unwrap(), page);
        assert!((scale - k).abs() < 1e-9, "the page is drawn at {k} to 1: {scale}");
        for spec in all_masters() {
            let r = run(&mut s, "shape.drop", json!({"master": spec.id, "page": page, "x": 5.3, "y": 4.1}));
            let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
            let made = own_points(s.document().unwrap(), page, &[id], (0.0, 0.0));
            let said = dropped_points(spec, (5.3, 4.1), scale);
            assert_eq!(said.len(), made.len().min(MAX_OWN_POINTS), "{} at scale {k}: how many", spec.name);
            for (i, (a, b)) in said.iter().zip(&made).enumerate() {
                assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "{} at scale {k}, point {i}: said {a:?}, made {b:?}", spec.name);
            }
            // A position dropped inside another becomes its report and is laid out: the next one is dropped on bare paper.
            if spec.id.starts_with("orgchart.") {
                run(&mut s, "shape.delete", json!({"ids": [id]}));
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
    assert_eq!(dropped_points(rectangle, (10.0, 10.0), 1.0).len(), 4);
    let flow = find_master("gane-sarson.data-flow").unwrap();
    assert!(flow.is_connector());
    assert!(flow.connection_offsets().is_empty() && dropped_points(flow, (1.0, 1.0), 1.0).is_empty());
    // Not a number the snap can use: nothing.
    assert!(dropped_points(rectangle, (f64::NAN, 1.0), 1.0).is_empty());
    assert!(dropped_points(rectangle, (1.0, 1.0), 0.0).is_empty());
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
    for k in [1.0, 2.0] {
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
        let (w, h) = (2.0 / k, 1.0 / k);
        let want = (5.3 - w / 2.0 + 0.6 / k, 4.1 - h / 2.0 + 0.9 / k);
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
