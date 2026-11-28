//! A connector DiaCraft makes is saved to a VSDX file as a connector the application routes.
//!
//! The application's check of the files DiaCraft writes: a person dragged a shape that a DiaCraft connector was glued to. The glue held and the end followed, but the legs tilted and the
//! arrowhead stopped short of the shape: the application did not route the connector again. That is what the specification says of a one-dimensional shape that is no connector to it:
//! "a shape that isn't one never reroutes; its glued ends follow their targets by their formulas", and a connector is a one-dimensional shape
//! whose `ObjType` holds the routable flag 2, which DiaCraft's connector, with no master and no `ObjType`, did not hold (the root style's 0). Glue to the whole of a
//! shape is dynamic glue, which `GlueType` 0 gives only to the application's own dynamic connector and 2 allows, and the routable connectors of the
//! corpus hold `GlueType` 2 on 17,268 of 17,359. So a connector DiaCraft makes holds `ObjType` 2 and `GlueType` 2, and so does one of its own that it routes again.
//!
//! These tests read the bytes of the file each writer writes: the model writer (a drawing with no VSDX file to splice into) and the splice into the VSDX file a drawing was opened from.

use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, SheetRef, Value};
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::connect_walkglue::{cell_tag, odg_drawing, page_part, page_xml, written};
use crate::connect_whole::{id_of, opened, reopened};
use crate::edit::{doc, drop_shape};
use crate::edit_foreign::Dir;
use crate::odg_build::{Dir as OdgDir, id_of as odg_id_of, open_drawing};
use crate::{blank_session, check_session, run};

/// Asserts that shape `c` of the page `xml` holds the cells of a connector the application routes: `ObjType` 2 and `GlueType` 2, plain values with no formula and no unit.
#[track_caller]
fn assert_routable(xml: &str, c: u32) {
    for cell in ["ObjType", "GlueType"] {
        assert_eq!(written(xml, c, cell), (None, Some("2".to_owned()), None), "{cell} of shape {c}");
    }
}

/// Asserts that shape `c` of the page `xml` holds neither cell.
#[track_caller]
fn assert_no_routable_cells(xml: &str, c: u32) {
    for cell in ["ObjType", "GlueType"] {
        assert!(cell_tag(xml, c, cell).is_none(), "shape {c} holds no {cell}: {:?}", cell_tag(xml, c, cell));
    }
}

/// The value shape `id` of page `page` reads for `cell`, its own or inherited (`None` when nothing gives it one).
fn effective(d: &Document, page: u32, id: u32, cell: &str) -> Option<f64> {
    let at = d.find_shape(Owner::Page(page), id)?;
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(Value::number)
}

/// The cell `cell` of shape `id` of page 1 as the shape holds it: whether it has a formula, and its number.
fn own(d: &Document, id: u32, cell: &str) -> Option<(bool, Option<f64>)> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let c = d.shape(&at)?.sheet.cell(cell)?.clone();
    Some((!matches!(c.formula, FormulaSlot::Absent), c.value.as_ref().and_then(Value::number)))
}

/// A blank drawing with two rectangles and a connector from the first to the second. → the session and the connector.
fn two_boxes_joined() -> (Session, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 5.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": a}, "to": {"id": b}})));
    (s, c)
}

#[test]
fn a_connector_made_here_holds_obj_type_two_and_glue_type_two_of_its_own() {
    let (s, c) = two_boxes_joined();
    let d = doc(&s);
    assert_eq!(own(d, c, "ObjType"), Some((false, Some(2.0))), "a local plain 2: routable");
    assert_eq!(own(d, c, "GlueType"), Some((false, Some(2.0))), "a local plain 2: dynamic glue allowed");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_model_writer_saves_a_new_connector_as_one_the_application_routes() {
    // A new drawing has no VSDX file to splice into: the model writer writes it.
    let (mut s, c) = two_boxes_joined();
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_routable(&page_xml(&path), c);
    // A save into the file it wrote (a splice now) keeps them, and so does a save of an edit.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    assert_routable(&page_xml(&path), c);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_splice_writer_saves_a_new_connector_as_one_the_application_routes() {
    // The drawing of `connect_whole.rs`: a connector from shape 1 (no connection point, so the whole of it) to shape 3 (four points), saved into the file and as another VSDX file.
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}})));
    run(&mut s, "file.save", json!({}));
    assert_routable(&page_xml(&dir.path("in.vsdx")), c);
    let other = dir.path("other.vsdx");
    run(&mut s, "file.save_as", json!({ "path": other }));
    assert_routable(&page_xml(&other), c);
    // The shapes the file held are not given the cells.
    let xml = page_xml(&other);
    for id in [1, 2, 3] {
        assert!(cell_tag(&xml, id, "GlueType").is_none(), "shape {id}");
    }
}

#[test]
fn a_connector_master_dropped_is_routable_and_a_fixed_one_dimensional_figure_is_not() {
    let mut s = blank_session();
    let flow = id_of(&run(&mut s, "shape.drop", json!({"master": "gane-sarson.data-flow", "page": 1, "x": 3.0, "y": 4.0})));
    let zigzag = id_of(&run(&mut s, "shape.drop", json!({"master": "network.comm-link", "page": 1, "x": 3.0, "y": 7.0})));
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    // A connector master is routed as any connector is (`drop_connector`), so the application routes it too.
    assert_routable(&xml, flow);
    // A fixed figure keeps the geometry of its master and is never routed: the application must not route it either.
    assert_no_routable_cells(&xml, zigzag);
    let d = doc(&s);
    assert_eq!(effective(d, 1, zigzag, "ObjType").unwrap_or(0.0), 0.0, "the figure reads no routable flag");
    assert_eq!(check_session(&s), Ok(()));
}

/// The cells of a box of 2 by 1 with its pin at (`x`, 2) and two connection points: its right side's middle (`IX` 0) and its left side's (`IX` 1).
fn box_cells(x: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Section N="Connection"><Row IX="0"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row><Row IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row></Section><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
    )
}

/// The cells of a straight connector from (`bx`, `by`) to (`ex`, `ey`), its begin glued by `begin` (a formula, or `None` for a free end) and its end by `end`, with `more` cells after
/// them and a route of one leg. `marked` gives it DiaCraft's route marker, as a connector an earlier build made holds it.
fn line_cells(b: (f64, f64), e: (f64, f64), begin: Option<&str>, end: Option<&str>, more: &str, marked: bool) -> String {
    let cell = |name: &str, v: f64, f: Option<&str>| match f {
        Some(f) => format!(r#"<Cell N="{name}" V="{v}" F="{f}"/>"#),
        None => format!(r#"<Cell N="{name}" V="{v}"/>"#),
    };
    let width = ((e.0 - b.0).powi(2) + (e.1 - b.1).powi(2)).sqrt();
    let angle = (e.1 - b.1).atan2(e.0 - b.0);
    let marker = if marked { r#"<Section N="User"><Row N="DiaCraftRoute"><Cell N="Value" V="1"/></Row></Section>"# } else { "" };
    format!(
        r#"{}{}{}{}<Cell N="PinX" V="{}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{}" F="(BeginY+EndY)/2"/><Cell N="Width" V="{width}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="{}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>{more}{marker}<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="{width}" F="Width*1"/><Cell N="Y" V="0"/></Row></Section>"#,
        cell("BeginX", b.0, begin),
        cell("BeginY", b.1, begin),
        cell("EndX", e.0, end),
        cell("EndY", e.1, end),
        (b.0 + e.0) / 2.0,
        (b.1 + e.1) / 2.0,
        width / 2.0,
    )
}

/// The glue formula of an end on connection point `n` (counting from 1) of shape `s`.
fn on_point(s: u32, n: u32) -> String {
    format!("PAR(PNT(Sheet.{s}!Connections.X{n},Sheet.{s}!Connections.Y{n}))")
}

/// What an earlier build wrote, and what another program's plain line is: boxes 1 (pin (2, 2)) and 2 (pin (8, 2)); connector 3 of DiaCraft's (the route marker, no `ObjType`) from box 1's
/// right point to box 2's left point; line 4, no connector of DiaCraft's (no marker) and no routable one, from box 1's left point to a free place; connector 5 of DiaCraft's that holds
/// `ObjType` 4 (neither, as a person may set it) from box 2's right point to a free place.
fn earlier_drawing() -> Vec<u8> {
    let p11 = on_point(1, 1);
    let p22 = on_point(2, 2);
    let p12 = on_point(1, 2);
    let p21 = on_point(2, 1);
    let shapes = format!(
        "{}{}{}{}{}",
        shape(1, &box_cells(2.0)),
        shape(2, &box_cells(8.0)),
        shape(3, &line_cells((3.0, 2.0), (7.0, 2.0), Some(&p11), Some(&p22), r#"<Cell N="EndArrow" V="4"/>"#, true)),
        shape(4, &line_cells((1.0, 2.0), (1.0, 5.0), Some(&p12), None, "", false)),
        shape(5, &line_cells((9.0, 2.0), (11.0, 2.0), Some(&p21), None, r#"<Cell N="ObjType" V="4"/>"#, true)),
    );
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="Connections.X1" ToPart="100"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="Connections.X2" ToPart="101"/><Connect FromSheet="4" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="Connections.X2" ToPart="101"/><Connect FromSheet="5" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="Connections.X1" ToPart="100"/>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="12"/><Cell N="PageHeight" V="12"/>"#).shapes(&shapes).connects(connects))
        .build()
}

/// A session that has opened [`earlier_drawing`], written at `in.vsdx` in `dir`.
fn opened_earlier(dir: &Dir) -> (Session, Vec<u8>) {
    let bytes = earlier_drawing();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    (reopened(&path), bytes)
}

/// The text of shape `id` of the page `xml`, from its start tag to its end tag.
fn shape_xml(xml: &str, id: u32) -> String {
    let start = xml.find(&format!(r#"<Shape ID="{id}""#)).unwrap_or_else(|| panic!("no shape {id}"));
    let end = start + xml[start..].find("</Shape>").unwrap() + "</Shape>".len();
    xml[start..end].to_owned()
}

#[test]
fn a_files_own_connectors_are_left_as_they_are_by_a_save_with_no_edit_and_by_an_edit_that_does_not_touch_them() {
    let dir = Dir::new();
    let (mut s, bytes) = opened_earlier(&dir);
    let path = dir.path("in.vsdx");
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_xml(&path), page_part(&bytes), "a save with no edit writes the page as it was read");
    // A new shape away from them: the connectors are written as the file had them.
    drop_shape(&mut s, "rectangle", 5.0, 9.0);
    run(&mut s, "file.save", json!({}));
    let (was, now) = (page_part(&bytes), page_xml(&path));
    for id in [3, 4, 5] {
        assert_eq!(shape_xml(&now, id), shape_xml(&was, id), "shape {id} is as the file had it");
    }
}

#[test]
fn a_connector_of_ours_that_a_move_routes_again_is_given_the_cells_and_a_files_plain_line_is_not() {
    let dir = Dir::new();
    let (mut s, bytes) = opened_earlier(&dir);
    let path = dir.path("in.vsdx");
    let before = doc(&s).clone();
    // Box 1 moves: connector 3 (ours) and line 4 (a file's) are routed again.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    let xml = page_xml(&path);
    assert_routable(&xml, 3);
    assert_ne!(shape_xml(&xml, 4), shape_xml(&page_part(&bytes), 4), "the line followed box 1 and was routed again");
    assert_no_routable_cells(&xml, 4);
    // Connector 5 did not move, and is as it was.
    assert_eq!(written(&xml, 5, "ObjType").1.as_deref(), Some("4"));
    assert!(cell_tag(&xml, 5, "GlueType").is_none());
    // The cells are written with the route, in its undo step.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*doc(&s), before, "undo takes the route and the cells back");
    // A connector of ours that holds 4 (neither) keeps it when it is routed again: a person's choice.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [2], "dx": 0.0, "dy": 1.0}));
    let d = doc(&s);
    assert_eq!(own(d, 5, "ObjType"), Some((false, Some(4.0))));
    assert_eq!(own(d, 5, "GlueType"), None);
    assert_eq!(own(d, 3, "ObjType"), Some((false, Some(2.0))), "connector 3 followed box 2 and was routed again");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_drawing_an_earlier_build_saved_as_diacraft_is_given_the_cells_when_saved_as_vsdx_and_keeps_them() {
    let dir = Dir::new();
    let (mut s, _) = opened_earlier(&dir);
    let native = dir.path("old.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut s = reopened(&native);
    let out = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": out }));
    let xml = page_xml(&out);
    // The connectors of ours are given them; a file's plain line is not, and a 4 is a person's choice.
    assert_routable(&xml, 3);
    assert_no_routable_cells(&xml, 4);
    assert_eq!(written(&xml, 5, "ObjType").1.as_deref(), Some("4"));
    assert!(cell_tag(&xml, 5, "GlueType").is_none());
    // The drawing is the file's now: a save with no edit writes the same, and an edit and a save keep them.
    let saved = std::fs::read(&out).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_xml(&out), page_part(&saved), "a save with no edit writes what the file holds");
    run(&mut s, "shape.move", json!({"page": 1, "ids": [2], "dx": 0.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    assert_routable(&page_xml(&out), 3);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_drawing_read_from_an_odg_file_is_saved_to_vsdx_with_its_connector_routable() {
    let dir = OdgDir::new();
    let mut s = open_drawing(&dir, &odg_drawing());
    let (a, c) = (odg_id_of(doc(&s), 0, "A"), odg_id_of(doc(&s), 0, "C"));
    let page = doc(&s).pages.get(0).unwrap().id;
    // The `.odg`'s own connector is routed again by a move and is left as the file has it: the cells say nothing to LibreOffice.
    run(&mut s, "shape.move", json!({"page": page, "ids": [a], "dx": 0.0, "dy": -1.0}));
    let at = doc(&s).find_shape(Owner::Page(page), c).unwrap();
    let sheet = &doc(&s).shape(&at).unwrap().sheet;
    assert!(sheet.cell("ObjType").is_none() && sheet.cell("GlueType").is_none(), "the .odg's connector is not given them");
    run(&mut s, "file.save", json!({}));
    let content = crate::odg_build::content_of(&dir.read("a.odg"));
    assert!(!content.contains("ObjType") && !content.contains("GlueType"), "the .odg holds neither: {content}");
    // A save across to VSDX gives them.
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_routable(&page_xml(&path), c);
    assert_eq!(check_session(&s), Ok(()));
}

/// [`earlier_drawing`]'s boxes and connector 3 with a route of five legs, a detour above the boxes: (3, 2), (4, 2), (4, 3), (6, 3), (6, 2), (7, 2) on the page.
fn bent_drawing() -> Vec<u8> {
    let (p11, p22) = (on_point(1, 1), on_point(2, 2));
    let bent = r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="3"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="3"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="6"><Cell N="X" V="4" F="Width*1"/><Cell N="Y" V="0"/></Row></Section>"#;
    let one_leg = line_cells((3.0, 2.0), (7.0, 2.0), Some(&p11), Some(&p22), "", true);
    let route_at = one_leg.find(r#"<Section N="Geometry""#).unwrap();
    let connector = format!("{}{bent}", &one_leg[..route_at]);
    let shapes = format!("{}{}{}", shape(1, &box_cells(2.0)), shape(2, &box_cells(8.0)), shape(3, &connector));
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="Connections.X1" ToPart="100"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="Connections.X2" ToPart="101"/>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="12"/><Cell N="PageHeight" V="12"/>"#).shapes(&shapes).connects(connects))
        .build()
}

#[test]
fn a_leg_of_a_connector_of_ours_moved_by_hand_gives_it_the_cells_with_the_hand_edit() {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, bent_drawing()).unwrap();
    let mut s = reopened(&path);
    let before = doc(&s).clone();
    assert_eq!(own(&before, 3, "ObjType"), None, "as an earlier build made it");
    let moved = run(&mut s, "connector.edit_path", json!({"id": 3, "leg": 2, "to": 3.5}));
    assert_eq!(moved["changed"], true, "{moved}");
    let d = doc(&s);
    assert_eq!(own(d, 3, "ConFixedCode"), Some((false, Some(3.0))), "the hand edit's mark");
    assert_eq!(own(d, 3, "ObjType"), Some((false, Some(2.0))));
    assert_eq!(own(d, 3, "GlueType"), Some((false, Some(2.0))));
    run(&mut s, "file.save", json!({}));
    assert_routable(&page_xml(&path), 3);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*doc(&s), before, "one undo step takes the leg and the cells back");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_reopened_file_routes_the_same_in_diacraft() {
    // The cells change nothing in DiaCraft's own routing: the same move in the session that made the connector and in one that read it from the file gives the same route.
    let (mut made, c) = two_boxes_joined();
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut made, "file.save_as", json!({ "path": path }));
    let mut back = reopened(&path);
    for s in [&mut made, &mut back] {
        run(s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": -1.5}));
    }
    let rows = |s: &Session| diacraft_engine::draw::connector_path(doc(s), 1, c).expect("a route").points;
    let (a, b) = (rows(&made), rows(&back));
    assert_eq!(a.len(), b.len(), "{a:?} against {b:?}");
    for (p, q) in a.iter().zip(&b) {
        assert!((p.0 - q.0).abs() < 1.0e-9 && (p.1 - q.1).abs() < 1.0e-9, "{a:?} against {b:?}");
    }
}
