//! The owner's ruling of 2026-11-26: **a connector or a one-dimensional shape DiaCraft makes has No Fill** (`FillPattern` 0 on the shape it makes), so that the hollow end the
//! inside of which takes the shape's fill stays see-through until a person picks a fill colour. Files from other apps keep their own fills and show them as the file says.
//!
//! Every way a connector is made is tried: `connector.add` (which the Connector tool and a drag from a connection point send), AutoConnect, the Organization Chart's connectors, each
//! one-dimensional master of the stencils, and a connector made in each template (a theme applied later changes no fill that the connector holds). The inside of the connector's hollow
//! end is **drawn**, over a coloured box, and Home > Fill's commands then colour it.

use diacraft_engine::Session;
use diacraft_engine::doc::{CellAddr, Owner, Resolver, SheetRef};
use diacraft_engine::draw::{connector_path, shape_corners};
use diacraft_engine::formatting::look_of;
use diacraft_engine::stencil;
use serde_json::json;

use crate::endfill_roundtrip::{is, px, render, reopened};
use crate::odg_build::Dir;
use crate::{blank_session, run};

/// The blue of the boxes a connector is drawn over: a hollow end that is see-through shows it, one with a white fill shows white.
const BLUE: [u8; 3] = [0x33, 0x66, 0xCC];

/// What `FillPattern` resolves to for shape `id` of page 1 (through its master and its styles).
fn pattern(s: &Session, id: u32) -> Option<f64> {
    let doc = s.document().expect("a document");
    let at = doc.find_shape(Owner::Page(1), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct("FillPattern"))?;
    found.value.cell.and_then(|c| c.value.as_ref().and_then(|v| v.number()))
}

/// Two boxes with a blue fill, at (2, 4) and (7, 4): the ids 1 and 2.
fn two_boxes(s: &mut Session) {
    for x in [2.0, 7.0] {
        run(s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": x, "y": 4.0}));
    }
    run(s, "shape.format", json!({"ids": [1, 2], "fill": "#3366CC"}));
}

/// The ids of the connectors the engine's connector command made on page 1 (they are named `Connector.<id>`).
fn made_connectors(s: &Session) -> Vec<u32> {
    let doc = s.document().expect("a document");
    doc.pages
        .get(0)
        .expect("a page")
        .shapes
        .iter()
        .filter(|shape| shape.name.as_deref().is_some_and(|n| n.starts_with("Connector.")))
        .map(|shape| shape.id)
        .collect()
}

/// The end of connector `id` and the point 0.08 inch from it toward the middle of the nearest shape that is not a connector: inside the square of a hollow end and, at a connector's end,
/// inside the shape it ends on.
fn inside_of_the_end(s: &Session, id: u32) -> (f64, f64) {
    let doc = s.document().expect("a document");
    let connectors = made_connectors(s);
    let end = connector_path(doc, 1, id).expect("a connector").points.last().copied().expect("a vertex");
    let page = doc.pages.get(0).expect("a page");
    let centre = |shape: u32| {
        let c = shape_corners(doc, 1, shape).expect("corners");
        (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
    };
    let nearest = page
        .shapes
        .iter()
        .filter(|shape| !connectors.contains(&shape.id))
        .map(|shape| centre(shape.id))
        .min_by(|a, b| {
            let (da, db) = ((a.0 - end.0).hypot(a.1 - end.1), (b.0 - end.0).hypot(b.1 - end.1));
            da.total_cmp(&db)
        })
        .expect("a shape the connector ends on");
    let length = (nearest.0 - end.0).hypot(nearest.1 - end.1).max(1.0e-9);
    (end.0 + 0.08 * (nearest.0 - end.0) / length, end.1 + 0.08 * (nearest.1 - end.1) / length)
}

/// Gives connector `id` a hollow square at its end (number 21, size class 6) and draws page 1.
fn hollow(s: &mut Session, id: u32) {
    run(s, "shape.format", json!({"ids": [id], "end_arrow": 21, "end_arrow_size": 6, "weight": 1.5}));
}

/// Asserts that connector `id` has no fill and that the inside of its hollow end shows the box it ends on.
#[track_caller]
fn assert_see_through(s: &mut Session, id: u32, what: &str) {
    assert_eq!(pattern(s, id), Some(0.0), "{what}: the connector's `FillPattern` is 0");
    let doc = s.document().unwrap();
    assert_eq!(look_of(doc, 1, &[id]).unwrap().remove(0).fill, None, "{what}: a look with no fill");
    hollow(s, id);
    let (x, y) = inside_of_the_end(s, id);
    let r = render(s);
    let boxes: Vec<_> = look_of(s.document().unwrap(), 1, &[1, 2]).unwrap().into_iter().map(|l| l.fill).collect();
    assert!(
        is(px(&r, x, y), BLUE),
        "{what}: see-through inside the hollow end, over the box: {:?} at ({x}, {y}); the boxes' fills {boxes:?}; around: {:?} {:?} {:?}",
        px(&r, x, y),
        px(&r, x + 0.3, y),
        px(&r, x, y + 0.3),
        px(&r, x - 0.1, y)
    );
}

#[test]
fn a_connector_made_by_connector_add_has_no_fill_and_its_hollow_end_is_see_through() {
    // What the Connector tool and a drag from a connection point send is `connector.add`.
    let mut s = blank_session();
    two_boxes(&mut s);
    let made = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    assert_see_through(&mut s, id, "connector.add between two boxes");
    // And with free ends, and to a given connection point.
    let made = run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 0}, "to": {"x": 4.0, "y": 7.0}}));
    assert_eq!(pattern(&s, u32::try_from(made["id"].as_u64().unwrap()).unwrap()), Some(0.0), "a connector with a free end");
}

#[test]
fn a_connector_autoconnect_makes_has_no_fill_in_each_of_its_forms() {
    // A click on an arrow with no neighbour (a copy and a connector), a click that joins the nearest neighbour, a drag onto a shape, and a master dropped from the flyout.
    let mut s = blank_session();
    two_boxes(&mut s);
    run(&mut s, "connector.autoconnect", json!({"from": 1, "direction": "up"}));
    run(&mut s, "connector.autoconnect", json!({"from": 1, "direction": "right", "existing": true}));
    run(&mut s, "connector.autoconnect", json!({"from": 2, "direction": "down", "to": 1}));
    run(&mut s, "connector.autoconnect", json!({"from": 2, "direction": "up", "master": "process", "stencil": "flowchart"}));
    let made = made_connectors(&s);
    assert_eq!(made.len(), 4, "four connectors: {made:?}");
    // The first three end on boxes that are blue (a copy of one, the other, the first); the fourth ends on a flowchart shape that holds a fill of its own, so it is not looked at in blue.
    for id in &made[..3] {
        assert_see_through(&mut s, *id, "an AutoConnect connector");
    }
    assert_eq!(pattern(&s, made[3]), Some(0.0), "the connector of a master dropped from the flyout");
}

#[test]
fn the_connectors_of_an_organization_chart_have_no_fill() {
    let mut s = blank_session();
    let boss = run(&mut s, "shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 4.25, "y": 9.5}))["id"].as_u64().unwrap();
    for _ in 0..3 {
        // A position dropped with its centre inside another is that position's report: a connector is made.
        let boss_at = {
            let c = shape_corners(s.document().unwrap(), 1, u32::try_from(boss).unwrap()).unwrap();
            (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
        };
        run(&mut s, "shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": boss_at.0, "y": boss_at.1}));
    }
    let made = made_connectors(&s);
    assert_eq!(made.len(), 3, "a connector for each report: {made:?}");
    for id in made {
        assert_eq!(pattern(&s, id), Some(0.0), "the connector {id} of the chart");
    }
}

#[test]
fn every_one_dimensional_master_of_the_stencils_drops_with_no_fill() {
    // The connectors and the lines drawn along a line (the Data Flow, the Comm-link, UML's associations and messages, BPMN's flows, the crow's foot's relationships, the self message).
    let mut checked = 0;
    for st in stencil::stencils() {
        for master in st.masters().iter().filter(|m| m.is_connector()) {
            let mut s = blank_session();
            let made = run(&mut s, "shape.drop", json!({"master": master.id, "stencil": st.id, "page": 1, "x": 5.5, "y": 4.25}));
            let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
            assert_eq!(pattern(&s, id), Some(0.0), "{}/{} drops with `FillPattern` 0", st.id, master.id);
            let doc = s.document().unwrap();
            assert_eq!(look_of(doc, 1, &[id]).unwrap().remove(0).fill, None, "{}/{}", st.id, master.id);
            checked += 1;
        }
    }
    assert!(checked >= 10, "the stencils have more than a few one-dimensional masters: {checked}");
}

/// What `FillPattern` resolves to on the one shape of master `master` of the document (its own cell, else its styles).
fn master_pattern(s: &Session, master: u32) -> Option<f64> {
    let doc = s.document().expect("a document");
    let at = SheetRef::Shape(diacraft_engine::doc::ShapeRef::top(Owner::Master(master), 0));
    let found = Resolver::new(doc).resolve(&at, &CellAddr::direct("FillPattern"))?;
    found.value.cell.and_then(|c| c.value.as_ref().and_then(|v| v.number()))
}

#[test]
fn every_connector_master_has_no_fill_and_a_dropped_instances_hollow_end_draws_empty() {
    // The owner's ruling of 2026-11-27: a connector's fill is fully transparent by default, so that a hollow line end draws empty, as the reference application draws it; a fill a person
    // sets still shows inside it. Each one-dimensional master is dropped across a blue box (the box is drawn first, the connector over it), given a hollow square at its end, and drawn:
    // inside the square the box shows through.
    let mut checked = 0;
    for st in stencil::stencils() {
        for master in st.masters().iter().filter(|m| m.is_connector()) {
            let what = format!("{}/{}", st.id, master.id);
            let mut s = blank_session();
            let made = run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 5.5, "y": 4.25}));
            let box_id = made["id"].as_u64().unwrap();
            run(&mut s, "shape.resize", json!({"id": box_id, "x": 3.5, "y": 3.25, "width": 4.0, "height": 2.0}));
            run(&mut s, "shape.format", json!({"ids": [box_id], "fill": "#3366CC"}));
            let made = run(&mut s, "shape.drop", json!({"master": master.id, "stencil": st.id, "page": 1, "x": 5.5, "y": 4.25}));
            let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
            let in_doc = u32::try_from(made["master"].as_u64().unwrap()).unwrap();
            assert_eq!(master_pattern(&s, in_doc), Some(0.0), "{what}: the master's own `FillPattern` is 0");
            assert_eq!(pattern(&s, id), Some(0.0), "{what}: the instance reads the master's");
            let doc = s.document().unwrap();
            assert!(
                doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.cell("FillPattern").is_none(),
                "{what}: and holds none of its own"
            );
            hollow(&mut s, id);
            let end = connector_path(s.document().unwrap(), 1, id).expect("a line").points.last().copied().expect("a vertex");
            // 0.08 inch back along the axis of a line that runs left to right: inside the square (0.4 inch across at size 6 and 1.5 points), clear of the shaft (it stops at the square's edge).
            let (x, y) = (end.0 - 0.08, end.1);
            let r = render(&s);
            assert!(is(px(&r, x, y), BLUE), "{what}: the box shows inside the hollow end at ({x}, {y}): {:?}", px(&r, x, y));
            checked += 1;
        }
    }
    assert!(checked >= 28, "every one-dimensional master of the stencils: {checked}");
}

#[test]
fn a_connector_made_in_each_template_has_no_fill_and_keeps_it_when_a_theme_is_applied_later() {
    let listed = blank_session().execute("template.list", &json!({})).expect("the templates");
    let ids: Vec<String> = listed["templates"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str().map(str::to_owned)).collect();
    assert!(ids.len() >= 6, "{ids:?}");
    for template in ids {
        let mut s = blank_session();
        run(&mut s, "file.new", json!({"template": template}));
        two_boxes(&mut s);
        let made = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        assert_see_through(&mut s, id, &format!("a connector made in the {template} template"));
        // A theme given to the page afterwards follows by the shapes' links and selectors and changes no cell the shape holds.
        run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
        assert_eq!(pattern(&s, id), Some(0.0), "{template}: a theme applied later");
    }
}

#[test]
fn what_home_fill_sets_on_a_connector_with_no_fill_shows_inside_its_hollow_end() {
    // The colour, the transparency of a colour, a pattern and a gradient: each shows inside the end, where the connector had nothing. (A theme colour goes the same way as a colour.)
    for (what, params, gradient) in [
        ("a colour", json!({"fill": "#E03030"}), false),
        ("a pattern", json!({"fill_pattern": 13, "fill": "#E03030", "fill_background": "#E0E030"}), false),
        ("a gradient", json!({}), true),
    ] {
        let mut s = blank_session();
        two_boxes(&mut s);
        let made = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        assert_see_through(&mut s, id, what);
        let mut p = params;
        p["ids"] = json!([id]);
        if gradient {
            run(
                &mut s,
                "shape.gradient",
                json!({"ids": [id], "kind": "linear", "angle": 0, "stops": [{"position": 0, "color": "#E03030"}, {"position": 1, "color": "#30E030"}]}),
            );
        } else {
            run(&mut s, "shape.format", p);
        }
        let (x, y) = inside_of_the_end(&s, id);
        let r = render(&s);
        let inside = px(&r, x, y);
        assert!(!is(inside, BLUE) && inside != [255, 255, 255], "{what}: the fill shows inside the end: {inside:?}");
        assert_ne!(pattern(&s, id), Some(0.0), "{what}: the connector has a fill now");
    }
    // A colour picked and then No Fill again is see-through again.
    let mut s = blank_session();
    two_boxes(&mut s);
    let made = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    hollow(&mut s, id);
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#E03030"}));
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "none"}));
    let (x, y) = inside_of_the_end(&s, id);
    assert!(is(px(&render(&s), x, y), BLUE), "No Fill takes it away again");
}

#[test]
fn a_connector_keeps_no_fill_and_a_later_fill_through_diacraft_vsdx_and_odg_files() {
    for name in ["a.diacraft", "a.vsdx", "a.odg"] {
        let dir = Dir::new();
        let mut s = blank_session();
        two_boxes(&mut s);
        let made = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        hollow(&mut s, id);
        run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
        // The file read again: no fill, and the picture shows the box inside the end as the session did.
        let mut again = reopened(&dir, name);
        assert_eq!(pattern(&again, id), Some(0.0), "{name}: `FillPattern` 0 comes back");
        let (x, y) = inside_of_the_end(&again, id);
        assert!(is(px(&render(&again), x, y), BLUE), "{name}: see-through after the round trip: {:?}", px(&render(&again), x, y));
        // A fill set afterwards is saved in place and comes back.
        run(&mut again, "shape.format", json!({"ids": [id], "fill": "#00AA00"}));
        run(&mut again, "file.save", json!({}));
        let last = reopened(&dir, name);
        assert_ne!(pattern(&last, id), Some(0.0), "{name}: the fill a person picked is there");
        let (x, y) = inside_of_the_end(&last, id);
        assert!(is(px(&render(&last), x, y), [0, 170, 0]), "{name}: green inside the hollow end: {:?}", px(&render(&last), x, y));
    }
}

/// A one-dimensional shape as a file from another app holds it: a line from (1.5, `y`) to (4.5, `y`) with a hollow square at its end, its own `FillPattern` and `FillForegnd`.
fn line_of_a_file(id: u32, y: f64, pattern: &str) -> String {
    diacraft_testkit::package::shape(
        id,
        &format!(
            r##"<Cell N="PinX" V="3"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="3"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="1.5"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="0"/>
<Cell N="BeginX" V="1.5"/><Cell N="BeginY" V="{y}"/><Cell N="EndX" V="4.5"/><Cell N="EndY" V="{y}"/>
<Cell N="FillPattern" V="{pattern}"/><Cell N="FillForegnd" V="#FF0000"/><Cell N="LinePattern" V="1"/><Cell N="LineWeight" V="0.02"/><Cell N="LineColor" V="#000000"/>
<Cell N="EndArrow" V="21"/><Cell N="EndArrowSize" V="6"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="3"/><Cell N="Y" V="0"/></Row></Section>"##
        ),
    )
}

#[test]
fn a_connector_of_a_file_from_another_app_keeps_the_fill_the_file_gives_it_and_shows_it_inside_its_hollow_end() {
    // The default belongs to the connectors DiaCraft makes. A line of a VSDX file that holds a red fill shows it inside its hollow end, as the file says; one that holds none shows nothing there.
    use diacraft_testkit::package::PageSpec;
    use diacraft_testkit::package::VsdxBuilder;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&format!(
            "{}{}",
            line_of_a_file(1, 4.0, "1"),
            line_of_a_file(2, 7.0, "0")
        )))
        .build();
    let dir = Dir::new();
    let s = crate::odg_build::open_bytes_in(&dir, "other.vsdx", &bytes);
    let r = render(&s);
    // The end of each line is (4.5, y); 0.08 inch before it along the line is inside the square, which is centred on it.
    assert!(is(px(&r, 4.42, 4.0), [255, 0, 0]), "red inside the hollow end of the line the file filled: {:?}", px(&r, 4.42, 4.0));
    assert!(is(px(&r, 4.42, 7.0), [255, 255, 255]), "nothing inside the hollow end of the one it did not: {:?}", px(&r, 4.42, 7.0));
}
