//! The editing commands on a drawing that came from a file: instances that inherit from a master, `Inh` cells with cached results, and a
//! connector of the file's own, glued by the formulas a file holds. The package is built in code from the specifications' XML
//! No file from another application is used.

use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, FormulaSlot, Lookup, Owner, Resolver, RowKey, SectionKey, SheetRef};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::{check_session, run};

/// A master of a 2 by 1 box with a connection point on its right (row 0) and on its left (row 1), as a file writes it.
fn master_shape() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row></Section>"#,
    )
}

/// Instance 10 at (2, 2) and instance 11, three wide, at (6, 2), joined by connector 12 with the formulas and `Connect` rows a file holds.
fn page_shapes() -> String {
    let a = shape_with(10, r#" Master="7""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#);
    let b = shape_with(
        11,
        r#" Master="7""#,
        r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="2"/><Cell N="Width" V="3"/><Cell N="LocPinX" V="1.5" F="Inh"/>
<Section N="Connection"><Row IX="0"><Cell N="X" V="3" F="Inh"/></Row></Section>"#,
    );
    let c = shape(
        12,
        r#"<Cell N="BeginX" V="3" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/><Cell N="BeginY" V="2" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/>
<Cell N="EndX" V="4.5" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/><Cell N="EndY" V="2" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/>
<Cell N="PinX" V="3.75" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="1.5" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="0.75" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1.5"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    format!("{a}{b}{c}")
}

fn connects() -> &'static str {
    r#"<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>"#
}

/// A session that has opened the drawing. The file is read whole, so it is removed at once.
fn opened() -> Session {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let bytes = VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&master_shape()))
        .page(PageSpec::new(1, "Page-1").shapes(&page_shapes()).connects(connects()))
        .build();
    let name = format!("engine-edit-file-{}-{}.vsdx", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path.to_str().unwrap()}));
    // Litter in a scratch folder is harmless.
    let _ = std::fs::remove_file(path);
    s
}

fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(d)
        .resolve(&sheet, &CellAddr::direct(cell))
        .and_then(|f| f.value.cell?.value.as_ref()?.number())
        .unwrap_or_else(|| panic!("{id}.{cell}"))
}

fn rows_of_connector(d: &Document) -> usize {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), 12).unwrap());
    Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).map_or(0, |r| r.len())
}

#[test]
fn moving_an_instance_of_a_file_moves_the_connector_the_file_glued_to_it_and_routes_it_again() {
    let mut s = opened();
    let d = s.document().unwrap();
    assert_eq!((num(d, 12, "BeginX"), num(d, 12, "EndX")), (3.0, 4.5), "the file's own saved values");
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 1.0}));
    let d = s.document().unwrap();
    assert_eq!((num(d, 11, "PinX"), num(d, 11, "PinY")), (7.0, 3.0));
    // Instance 11 is three wide, so its left point is 1.5 left of its pin; the connector's formula follows through the `Inh` cells.
    assert!((num(d, 12, "EndX") - 5.5).abs() < 1e-9 && (num(d, 12, "EndY") - 3.0).abs() < 1e-9, "{} {}", num(d, 12, "EndX"), num(d, 12, "EndY"));
    assert!((num(d, 12, "Width") - (2.5f64 * 2.5 + 1.0).sqrt()).abs() < 1e-9);
    assert!((num(d, 12, "Angle") - 1.0f64.atan2(2.5)).abs() < 1e-9);
    // The file's connector has a glued end that moved, so it is routed again: out of shape 10's right side at (3, 2), across
    // at the middle, and into shape 11's left side at (5.5, 3), in place of the file's two rows.
    assert_eq!(rows_of_connector(d), 4);
    let points = crate::edit::route(d, 12);
    let want = [(3.0, 2.0), (4.25, 2.0), (4.25, 3.0), (5.5, 3.0)];
    assert!(points.len() == 4 && points.iter().zip(want).all(|(p, w)| (p.0 - w.0).abs() < 1e-9 && (p.1 - w.1).abs() < 1e-9), "{points:?}");
    assert_eq!(num(d, 12, "BeginX"), 3.0);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn resizing_a_file_instance_keeps_its_inh_cells_and_the_connector_follows() {
    let mut s = opened();
    run(&mut s, "shape.resize", json!({"id": 11, "x": 5.0, "y": 1.0, "width": 4.0, "height": 2.0}));
    let d = s.document().unwrap();
    assert_eq!((num(d, 11, "Width"), num(d, 11, "Height"), num(d, 11, "PinX"), num(d, 11, "PinY")), (4.0, 2.0, 7.0, 2.0));
    // The connection point on its left stays on its left edge, at the middle of its new height.
    assert!((num(d, 12, "EndX") - 5.0).abs() < 1e-9 && (num(d, 12, "EndY") - 2.0).abs() < 1e-9);
    // The row the file held as `Inh` is still `Inh`, with the new result.
    let shape = d.shape(&d.find_shape(Owner::Page(1), 11).unwrap()).unwrap();
    let held = match shape.sheet.lookup(&CellAddr::in_row("Connection", None, RowKey::Ix(0), "X")) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(held.formula, FormulaSlot::Inherit);
    assert_eq!(held.value.as_ref().and_then(|v| v.number()), Some(4.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn our_shapes_and_connectors_join_a_drawing_from_a_file() {
    let mut s = opened();
    // A rectangle of ours beside the file's, joined to the file's instance 11.
    let dropped = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 5.0}));
    assert_eq!(dropped["id"], 13, "the next id after the file's highest");
    assert_eq!(s.document().unwrap().masters.len(), 2, "the file's master and ours");
    let added = run(&mut s, "connector.add", json!({"from": {"id": 11, "point": 0}, "to": {"id": 13}}));
    assert_eq!(added["begin"], json!({"id": 11, "point": 0}));
    assert_eq!(added["id"], 14);
    let d = s.document().unwrap();
    // Instance 11 is three wide with its pin at its middle: its right edge, point 0, is at x 7.5, and the new connector starts there.
    assert!((num(d, 14, "BeginX") - 7.5).abs() < 1e-9 && (num(d, 14, "BeginY") - 2.0).abs() < 1e-9);
    // Deleting instance 10 with `connectors: false` frees the file connector's begin where it was; by default the connector goes with it.
    run(&mut s, "shape.delete", json!({"ids": [10], "connectors": false}));
    let d = s.document().unwrap();
    assert!(d.find_shape(Owner::Page(1), 10).is_none());
    assert_eq!(num(d, 12, "BeginX"), 3.0);
    let shape = d.shape(&d.find_shape(Owner::Page(1), 12).unwrap()).unwrap();
    assert!(matches!(shape.sheet.lookup(&CellAddr::direct("BeginX")), Lookup::Found(c) if c.formula == FormulaSlot::Absent));
    assert!(d.page(1).unwrap().connects.iter().all(|c| c.to_sheet != 10));
    // Everything undoes, back to the file as it was read.
    let read = opened().document().unwrap().clone();
    while s.execute("edit.undo", &json!({})).is_ok() {}
    assert_eq!(s.document().unwrap(), &read);
    assert_eq!(check_session(&s), Ok(()));
}
