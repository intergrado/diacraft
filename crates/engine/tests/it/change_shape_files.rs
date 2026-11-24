//! Change Shape on drawings and stencils from files: shape data, the cells of the new master that say what a swap keeps, glue to points the new
//! master lacks, a group master, and a one-dimensional instance whose frame the old master supplied. Every package is built in code from the specifications' XML
//! No file from another application is used.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, SectionKey, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::draw::{connection_points, shape_corners};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value as Json, json};

use crate::edit::{doc, num};
use crate::stencil_open::{CT_STENCIL, Dir, LOOK, box_cells, ids, open};
use crate::{check_session, run, snapshot};

const TOL: f64 = 1.0e-6;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

/// The line and fill pattern cells that make a page draw a shape, without a colour of the fill: the masters of this file add their own.
const LOOK_NO_COLOUR: &str =
    r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.01"/><Cell N="FillPattern" V="1"/>"##;

/// The shape of a master, with a `Type`.
fn sh(id: u32, inner: &str) -> String {
    shape_with(id, r#" Type="Shape""#, inner)
}

/// A master of this `ID` and name whose two GUIDs follow from the `ID`, so that the stencil's and the drawing's are the same master.
fn master(id: u32, name: &str, shapes: &str) -> MasterSpec {
    MasterSpec::new(id, name).attrs(&ids(100 + id, 200 + id)).shapes(shapes)
}

/// A box `w` by `h` with its connection points at `points`.
fn box_with_points(w: f64, h: f64, points: &[(f64, f64)]) -> String {
    let full = box_cells(w, h);
    let head = full.split("<Section N=\"Connection\">").next().unwrap_or_default().to_owned();
    if points.is_empty() {
        return head;
    }
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| format!(r#"<Row IX="{i}"><Cell N="X" V="{x}"/><Cell N="Y" V="{y}"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row>"#))
        .collect();
    format!(r#"{head}<Section N="Connection">{rows}</Section>"#)
}

fn six() -> MasterSpec {
    let pts = [(0.75, 1.0), (0.0, 0.5), (0.75, 0.0), (1.5, 0.5), (0.2, 0.9), (1.3, 0.1)];
    master(1, "Six", &sh(5, &format!("{LOOK}{}", box_with_points(1.5, 1.0, &pts))))
}

fn four() -> MasterSpec {
    master(2, "Four", &sh(5, &format!("{LOOK}{}", box_cells(1.5, 1.0))))
}

fn blob() -> MasterSpec {
    master(5, "Blob", &sh(5, &format!("{LOOK}{}", box_with_points(1.5, 1.0, &[]))))
}

/// A one-dimensional master from (0, 0) to (2, 0) with the frame cells a file holds: the pin between the ends.
fn link(id: u32, name: &str, extra: &str) -> MasterSpec {
    let inner = format!(
        r#"<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="2"/><Cell N="EndY" V="0"/>
<Cell N="PinX" V="1" F="(BeginX+EndX)/2"/><Cell N="PinY" V="0" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.01"/>{extra}
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#
    );
    master(id, name, &sh(5, &inner))
}

/// A one-dimensional master whose pin is a formula of its own width, as a master of a line that is not a connector may have it: instanced on a page with its ends moved,
/// its pin would not be in the middle.
fn odd_link() -> MasterSpec {
    let inner = r##"<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="2"/><Cell N="EndY" V="0"/>
<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0" F="Height*0.5"/>
<Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#FF0000"/><Cell N="LineWeight" V="0.03"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row></Section>"##;
    master(4, "Odd Link", &sh(5, inner))
}

fn solo() -> MasterSpec {
    master(6, "Solo", &sh(5, &format!("{LOOK_NO_COLOUR}<Cell N=\"FillForegnd\" V=\"#00AA00\"/>{}", box_cells(2.0, 1.0))))
}

/// A master of two top-level shapes: A, a box 2 by 1 at pin (1, 0.5), and B, a box 2 by 1 at pin (3, 1.5): the joint box is 4 by 2.
fn pair() -> MasterSpec {
    let a = sh(5, &format!("{LOOK}{}", box_cells(2.0, 1.0)));
    let moved = box_cells(2.0, 1.0).replace(
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/>"#,
        r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="1.5"/>"#,
    );
    let b = sh(6, &format!("{LOOK}{moved}"));
    master(7, "Pair", &format!("{a}{b}"))
}

/// A master that has text, a colour of its own, and says that a swap to it takes its formatting, its data values and its text.
fn locked() -> MasterSpec {
    let inner = format!(
        r##"{LOOK_NO_COLOUR}<Cell N="FillForegnd" V="#0000AA"/><Cell N="ReplaceLockFormat" V="1"/><Cell N="ReplaceLockShapeData" V="1"/><Cell N="ReplaceLockText" V="1"/>{}
<Section N="Property"><Row N="Cost"><Cell N="Label" V="Price"/><Cell N="Value" V="0"/></Row></Section><Text>New</Text>"##,
        box_cells(1.5, 1.0)
    );
    master(8, "Locked", &sh(5, &inner))
}

/// Process-like masters that carry shape data.
fn with_data(id: u32, name: &str, rows: &str) -> MasterSpec {
    master(id, name, &sh(5, &format!("{LOOK}{}<Section N=\"Property\">{rows}</Section>", box_cells(1.5, 1.0))))
}

fn all_masters() -> Vec<MasterSpec> {
    vec![
        six(),
        four(),
        link(3, "Link", ""),
        odd_link(),
        blob(),
        solo(),
        pair(),
        locked(),
        with_data(
            9,
            "Priced",
            r#"<Row N="Cost"><Cell N="Label" V="Cost"/><Cell N="Value" V="5"/></Row><Row N="Owner"><Cell N="Label" V="Owner"/><Cell N="Value" V="ops"/></Row>"#,
        ),
        with_data(
            10,
            "Other",
            r#"<Row N="Cost"><Cell N="Label" V="Price"/><Cell N="Value" V="0"/></Row><Row N="Risk"><Cell N="Label" V="Risk"/><Cell N="Value" V="1"/></Row>"#,
        ),
    ]
}

/// An open session whose drawing holds `shapes` and `connects` on page 1, with the masters of the drawing and of the stencil `file-1` being the same.
fn session_with(dir: &Dir, shapes: &str, connects: &str) -> Session {
    let mut drawing = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(shapes).connects(connects));
    let mut stencil = VsdxBuilder::new().document_type(CT_STENCIL);
    for m in all_masters() {
        drawing = drawing.master(m);
    }
    for m in all_masters() {
        stencil = stencil.master(m);
    }
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("plan.vsdx", &drawing.build()) }));
    let opened = open(&mut s, &dir.write("stencil.vssx", &stencil.build()));
    assert_eq!(opened["id"], "file-1");
    s
}

fn instance(id: u32, master: u32, x: f64, y: f64, more: &str) -> String {
    shape_with(id, &format!(r#" Master="{master}""#), &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/>{more}"#))
}

fn change(s: &mut Session, ids: &[u32], master: &str) -> Json {
    run(s, "shape.change", json!({"ids": ids, "master": master, "stencil": "file-1"}))
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap()
}

fn master_name(d: &Document, id: u32) -> Option<String> {
    d.master(shape_of(d, id).master?).and_then(|m| m.name.as_deref().map(str::to_owned))
}

/// The resolved shape data rows of a shape: (row name, [(cell, value text)]).
fn data_rows(d: &Document, id: u32) -> Vec<(String, Vec<(String, String)>)> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    let rows = Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Property", None)).unwrap_or_default();
    rows.iter()
        .map(|row| {
            let name = match &row.key {
                RowKey::Name(n) => n.to_string(),
                RowKey::Ix(i) => i.to_string(),
            };
            let cells = row
                .cells
                .iter()
                .map(|(n, found)| {
                    (n.as_str().to_owned(), found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default())
                })
                .collect();
            (name, cells)
        })
        .collect()
}

fn value_of(rows: &[(String, Vec<(String, String)>)], row: &str, cell: &str) -> Option<String> {
    rows.iter().find(|r| r.0 == row)?.1.iter().find(|c| c.0 == cell).map(|c| c.1.clone())
}

fn resolved_text(d: &Document, id: u32, cell: &str) -> Option<String> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(d).resolve(&sheet, &CellAddr::direct(cell)).and_then(|r| r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
}

fn connects(d: &Document) -> Vec<(u32, String, u32, u32)> {
    d.page(1)
        .unwrap()
        .connects
        .iter()
        .map(|c| (c.from_sheet, c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_sheet, c.to_part.unwrap_or(0)))
        .collect()
}

// ---- shape data and the cells of the new master -----------------------------------------------------------------------------------

#[test]
fn the_shape_data_a_shape_had_is_kept_cell_by_cell_and_the_new_masters_rows_it_lacked_are_added() {
    let dir = Dir::new();
    // Shape 10 is an instance of "Priced" (rows Cost and Owner) with a value of its own for Cost.
    let local = r#"<Section N="Property"><Row N="Cost"><Cell N="Value" V="12"/></Row></Section>"#;
    let mut s = session_with(&dir, &instance(10, 9, 3.0, 2.0, local), "");
    let was = data_rows(doc(&s), 10);
    assert_eq!(value_of(&was, "Cost", "Value").as_deref(), Some("12"));
    assert_eq!(value_of(&was, "Owner", "Value").as_deref(), Some("ops"));
    let steps = s.history().unwrap().undo_len();
    let r = change(&mut s, &[10], "Other");
    assert_eq!(r["changed"], json!([10]), "{r}");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let d = doc(&s);
    assert_eq!(master_name(d, 10).as_deref(), Some("Other"));
    let now = data_rows(d, 10);
    // The shape's own row, with its own label and the value it was given; a row only the old master had, kept; a row only the new master has, the master's.
    assert_eq!(value_of(&now, "Cost", "Value").as_deref(), Some("12"), "{now:?}");
    assert_eq!(value_of(&now, "Cost", "Label").as_deref(), Some("Cost"), "the label the shape had, not the new master's `Price`: {now:?}");
    assert_eq!(value_of(&now, "Owner", "Value").as_deref(), Some("ops"), "{now:?}");
    assert_eq!(value_of(&now, "Risk", "Value").as_deref(), Some("1"), "{now:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn shape_data_that_the_new_master_gives_the_same_is_not_copied_into_the_shape() {
    let dir = Dir::new();
    let mut s = session_with(&dir, &instance(10, 9, 3.0, 2.0, ""), "");
    // "Priced" to "Priced" is no change; to "Other", Cost's value 5 and label differ from the new master's, so they are held, and Owner is held as the master lacks it.
    change(&mut s, &[10], "Other");
    let held: Vec<String> = shape_of(doc(&s), 10)
        .sheet
        .sections
        .iter()
        .filter(|sec| sec.name.as_str() == "Property")
        .flat_map(|sec| sec.rows.iter().filter_map(|r| r.name.as_ref().map(|n| n.to_string())))
        .collect();
    assert_eq!(held, ["Cost", "Owner"], "Risk is the new master's and is not copied: {held:?}");
}

#[test]
fn the_new_master_can_say_that_its_formatting_data_and_text_beat_the_shapes_own() {
    let dir = Dir::new();
    let own = r##"<Cell N="FillForegnd" V="#FF0000"/><Section N="Property"><Row N="Cost"><Cell N="Value" V="12"/></Row><Row N="Mine"><Cell N="Value" V="7"/></Row></Section><Text>Mine</Text>"##;
    let mut s = session_with(&dir, &instance(10, 6, 3.0, 2.0, own), "");
    assert_eq!(resolved_text(doc(&s), 10, "FillForegnd").as_deref(), Some("#FF0000"));
    change(&mut s, &[10], "Locked");
    let d = doc(&s);
    assert_eq!(resolved_text(d, 10, "FillForegnd").as_deref(), Some("#0000AA"), "the new master's colour, not the shape's own");
    assert!(shape_of(d, 10).text.is_none(), "the new master's text, not the shape's own");
    let rows = data_rows(d, 10);
    assert_eq!(value_of(&rows, "Cost", "Value").as_deref(), Some("0"), "the new master's value: {rows:?}");
    assert_eq!(value_of(&rows, "Mine", "Value").as_deref(), Some("7"), "a row only the shape has is kept: {rows:?}");
    // The same shape changed to a master that does not say so keeps everything of its own.
    let mut s = session_with(&dir, &instance(10, 6, 3.0, 2.0, own), "");
    change(&mut s, &[10], "Four");
    let d = doc(&s);
    assert_eq!(resolved_text(d, 10, "FillForegnd").as_deref(), Some("#FF0000"));
    assert!(shape_of(d, 10).text.is_some());
}

#[test]
fn what_the_old_master_supplied_does_not_follow_the_shape() {
    let dir = Dir::new();
    let mut s = session_with(&dir, &instance(10, 6, 3.0, 2.0, ""), "");
    assert_eq!(resolved_text(doc(&s), 10, "FillForegnd").as_deref(), Some("#00AA00"), "Solo's own colour");
    change(&mut s, &[10], "Four");
    let probe = run(&mut s, "shape.drop", json!({"master": "Four", "stencil": "file-1", "page": 1, "x": 9.0, "y": 9.0}));
    let probe = u32::try_from(probe["id"].as_u64().unwrap()).unwrap();
    let d = doc(&s);
    assert_eq!(resolved_text(d, 10, "FillForegnd"), resolved_text(d, probe, "FillForegnd"), "the look is Four's");
    assert_ne!(resolved_text(d, 10, "FillForegnd").as_deref(), Some("#00AA00"));
}

#[test]
fn a_master_that_locks_replacing_cannot_be_changed_to() {
    let dir = Dir::new();
    let locked_master = master(11, "Sealed", &shape(5, &format!("{LOOK}<Cell N=\"LockReplace\" V=\"1\"/>{}", box_cells(1.5, 1.0))));
    let drawing = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(10, &format!("{LOOK}{}", box_cells(1.5, 1.0))))).build();
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).master(locked_master).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing) }));
    open(&mut s, &dir.write("s.vssx", &stencil));
    let before = snapshot(&s);
    let e = s.execute("shape.change", &json!({"ids": [10], "master": "Sealed", "stencil": "file-1"})).unwrap_err();
    assert!(e.to_string().contains("LockReplace"), "{e}");
    assert_eq!(snapshot(&s), before);
}

// ---- glue -----------------------------------------------------------------------------------------------------------------------------

fn two_boxes() -> String {
    format!("{}{}", instance(10, 1, 3.0, 3.0, ""), instance(11, 1, 8.0, 3.0, ""))
}

#[test]
fn an_end_glued_to_a_point_the_new_master_lacks_is_glued_to_the_nearest_point_of_the_shape() {
    let dir = Dir::new();
    let mut s = session_with(&dir, &two_boxes(), "");
    // The connector leaves point 5 of shape 10 (a point of the six-point master that the four-point one lacks) for point 3 of shape 11.
    let c = run(&mut s, "connector.add", json!({"from": {"id": 10, "point": 5}, "to": {"id": 11, "point": 3}}));
    let c = u32::try_from(c["id"].as_u64().unwrap()).unwrap();
    let r = change(&mut s, &[10], "Four");
    let d = doc(&s);
    let begin = connects(d).into_iter().find(|k| k.0 == c && k.1 == "BeginX").expect("the begin is glued");
    assert_eq!((begin.2, begin.3 >= 100 && begin.3 <= 103), (10, true), "glued to one of the four points: {begin:?} {r}");
    let point = connection_points(d, 1, 10).into_iter().find(|p| p.index == begin.3 - 100).unwrap();
    assert!(near(num(d, c, "BeginX"), point.x) && near(num(d, c, "BeginY"), point.y), "the begin is at the point it is glued to");
    // The right-hand point, the nearest to the other end.
    assert_eq!(begin.3, 103, "{begin:?}");
    assert_eq!(r["glue"][0]["connector"], c, "{r}");
    assert_eq!((r["glue"][0]["glued"].as_str(), r["glue"][0]["point"].as_u64()), (Some("point"), Some(3)), "{r}");
    // The end on shape 11 is untouched.
    assert_eq!(connects(d).into_iter().find(|k| k.0 == c && k.1 == "EndX").map(|k| (k.2, k.3)), Some((11, 103)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_end_glued_to_a_shape_that_has_no_connection_point_now_is_let_go_where_it_is() {
    let dir = Dir::new();
    let mut s = session_with(&dir, &two_boxes(), "");
    let c = run(&mut s, "connector.add", json!({"from": {"id": 10, "point": 3}, "to": {"id": 11, "point": 1}}));
    let c = u32::try_from(c["id"].as_u64().unwrap()).unwrap();
    let (bx, by) = (num(doc(&s), c, "BeginX"), num(doc(&s), c, "BeginY"));
    let r = change(&mut s, &[10], "Blob");
    let d = doc(&s);
    assert_eq!(r["glue"][0]["glued"], "free", "{r}");
    assert!(connects(d).iter().all(|k| !(k.0 == c && k.1 == "BeginX")), "no Connect row for the begin: {:?}", connects(d));
    assert!(near(num(d, c, "BeginX"), bx) && near(num(d, c, "BeginY"), by), "the end stays where it was");
    assert!(connects(d).iter().any(|k| k.0 == c && k.1 == "EndX" && k.2 == 11), "the other end is still glued");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a group master --------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_changed_to_a_group_master_gets_its_members_and_keeps_its_place_size_and_text() {
    let dir = Dir::new();
    let own = r#"<Cell N="Width" V="3"/><Cell N="Height" V="1.5"/><Cell N="LocPinX" V="1.5" F="Inh"/><Cell N="LocPinY" V="0.75" F="Inh"/><Text>Mine</Text>"#;
    let mut s = session_with(&dir, &instance(10, 6, 4.0, 3.0, own), "");
    let r = change(&mut s, &[10], "Pair");
    assert_eq!(r["changed"], json!([10]), "{r}");
    let d = doc(&s);
    let g = shape_of(d, 10);
    assert_eq!((g.kind.text(), master_name(d, 10).as_deref()), (Some("Group"), Some("Pair")));
    assert_eq!(g.children.iter().map(|m| m.master_shape).collect::<Vec<_>>(), [Some(5), Some(6)]);
    assert!(g.text.is_some(), "the text stays");
    assert!(near(num(d, 10, "Width"), 3.0) && near(num(d, 10, "Height"), 1.5), "the size stays");
    assert!(near(num(d, 10, "PinX"), 4.0) && near(num(d, 10, "PinY"), 3.0), "the place stays");
    assert!(near(num(d, 10, "LocPinX"), 1.5) && near(num(d, 10, "LocPinY"), 0.75), "the pin is the middle of the box it was given");
    // The members are inside the box of the group, which is where the group was.
    let (lo, hi) = ((2.5, 2.25), (5.5, 3.75));
    for m in g.children.iter() {
        let c = shape_corners(d, 1, m.id).unwrap();
        assert!(c.iter().all(|p| p.0 >= lo.0 - TOL && p.0 <= hi.0 + TOL && p.1 >= lo.1 - TOL && p.1 <= hi.1 + TOL), "member {} {c:?}", m.id);
    }
    assert_eq!(check_session(&s), Ok(()));
    // The next shape does not take an ID a member has.
    let next = run(&mut s, "shape.drop", json!({"master": "Four", "stencil": "file-1", "page": 1, "x": 9.0, "y": 9.0}));
    let ids: Vec<u32> = shape_of(doc(&s), 10).children.iter().map(|m| m.id).collect();
    assert!(!ids.contains(&u32::try_from(next["id"].as_u64().unwrap()).unwrap()), "{ids:?} {next}");
}

#[test]
fn a_group_masters_instance_is_changed_back_to_a_single_shape_and_its_members_are_gone() {
    let dir = Dir::new();
    let mut s = session_with(&dir, &instance(10, 6, 4.0, 3.0, ""), "");
    change(&mut s, &[10], "Pair");
    assert_eq!(shape_of(doc(&s), 10).children.len(), 2);
    let kept_id = shape_of(doc(&s), 10).children.get(0).map(|m| m.id).unwrap();
    // A member is part of an instance and is not changed on its own.
    let e = s.execute("shape.change", &json!({"ids": [kept_id], "master": "Four", "stencil": "file-1"})).unwrap_err();
    assert!(e.to_string().contains("part of an instance"), "{e}");
    let r = change(&mut s, &[10], "Solo");
    assert_eq!(r["changed"], json!([10]), "{r}");
    let g = shape_of(doc(&s), 10);
    assert_eq!((g.children.len(), g.kind.text()), (0, Some("Shape")));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a one-dimensional instance --------------------------------------------------------------------------------------------------

#[test]
fn a_connector_instance_whose_frame_the_old_master_supplied_keeps_its_frame_when_the_new_master_would_give_another() {
    let dir = Dir::new();
    // Shape 12 is an instance of the master "Link": its ends are its own (glued to shapes 10 and 11), and its pin, width and angle are the master's formulas.
    let glue_b = "PAR(PNT(Sheet.10!Connections.X4,Sheet.10!Connections.Y4))";
    let glue_e = "PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))";
    let c = shape_with(
        12,
        r#" Master="3""#,
        &format!(
            r#"<Cell N="BeginX" V="3.75" F="{glue_b}"/><Cell N="BeginY" V="3" F="{glue_b}"/><Cell N="EndX" V="7.25" F="{glue_e}"/><Cell N="EndY" V="3" F="{glue_e}"/>
<Cell N="PinX" V="5.5" F="Inh"/><Cell N="PinY" V="3" F="Inh"/><Cell N="Width" V="3.5" F="Inh"/><Cell N="LocPinX" V="1.75" F="Inh"/><Cell N="Angle" V="0" F="Inh"/>"#
        ),
    );
    let page = format!("{}{}{c}", instance(10, 2, 3.0, 3.0, ""), instance(11, 2, 8.0, 3.0, ""));
    let joins = r#"<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X4" ToPart="103"/><Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>"#;
    let mut s = session_with(&dir, &page, joins);
    assert!(near(num(doc(&s), 12, "PinX"), 5.5) && near(num(doc(&s), 12, "Width"), 3.5), "as the old master says");
    let before = connects(doc(&s));
    let r = change(&mut s, &[12], "Odd Link");
    assert_eq!(r["changed"], json!([12]), "{r}");
    let d = doc(&s);
    assert_eq!(master_name(d, 12).as_deref(), Some("Odd Link"));
    assert_eq!(connects(d), before, "still glued to the same points");
    // The odd master's pin is `Width*0.5`, which would put the pin at 1.75: the connector's own frame is between its ends.
    for (cell, want) in [("BeginX", 3.75), ("EndX", 7.25), ("PinX", 5.5), ("PinY", 3.0), ("Width", 3.5), ("Angle", 0.0)] {
        assert!(near(num(d, 12, cell), want), "{cell}: {} wanted {want}", num(d, 12, cell));
    }
    // It still follows the shape it is glued to.
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 0.0, "dy": 2.0}));
    let d = doc(&s);
    assert!(near(num(d, 12, "EndY"), 5.0), "the end follows shape 11: {}", num(d, 12, "EndY"));
    assert!(near(num(d, 12, "PinY"), 4.0), "so does the pin: {}", num(d, 12, "PinY"));
    assert_eq!(check_session(&s), Ok(()));
}
