//! An edit recalculates what it changed and what reads it, and nothing else.
//!
//! The page is built as a file from another application is: saved values that this evaluator would not give (a master whose `TxtPinX` says 77
//! where `Width*0.5` is 1, instance cells with `Inh` and a value of 9, a connector whose end says 99 where its glue formula gives 5, user cells
//! with a formula and a value that do not go together). Moving one shape must leave every cell it did not touch, and every cell nothing that
//! it touched reads, with the text the file had, and a save must write them back as they were. No file from another application is used: the
//! package is built in code from the specifications' XML.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_calc::{Budget, EvalOptions, Scope, commit_in};
use diacraft_doc::{Document, Owner, Sheet};
use diacraft_engine::Session;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::{check_session, run};

/// A master of a 2 by 1 box with a connection point on its right (row 0) and on its left (row 1), as a file writes it. Its `TxtPinX` says 77,
/// which `Width*0.5` is not: the master's own saved value is not what this evaluator gives.
fn master_shape() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Cell N="TxtPinX" V="77" F="Width*0.5"/><Cell N="TxtPinY" V="0.5" F="Height*0.5"/>
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

/// A user cell with a formula and a saved value.
fn user(name: &str, formula: &str, value: &str) -> String {
    format!(r#"<Row N="{name}"><Cell N="Value" V="{value}" F="{formula}"/></Row>"#)
}

/// The glue of a connector end, as a file holds it: both cells of the end hold the formula, with a saved value each.
fn glue(cell: &str, value: &str, target: u32, point: u32) -> String {
    format!(r#"<Cell N="{cell}" V="{value}" F="PAR(PNT(Sheet.{target}!Connections.X{point},Sheet.{target}!Connections.Y{point}))"/>"#)
}

/// Instance 10 at (2, 2) moves. Instance 11, three wide, is at (6, 2), and connector 12 (the file's own, with no route of ours) joins them.
/// Instances 13 and 14 and connector 15 are a second pair far from it, with saved values that are not what the formulas give.
fn page_shapes() -> String {
    let ten = shape_with(
        10,
        r#" Master="7""#,
        &format!(
            r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="TxtPinY" V="9" F="Inh"/>
<Section N="User">{}{}{}{}{}</Section>"#,
            user("Echo", "PinX*2", "1"),
            user("Note", "1+1", "9"),
            user("Far", "Sheet.13!PinX+1", "99"),
            user("Peer", "Sheet.11!Width", "55"),
            user("Twice", "User.Echo+1", "1000"),
        ),
    );
    let eleven = shape_with(
        11,
        r#" Master="7""#,
        &format!(
            r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="2"/><Cell N="Width" V="3"/><Cell N="LocPinX" V="1.5" F="Inh"/>
<Section N="Connection"><Row IX="0"><Cell N="X" V="3" F="Inh"/></Row></Section><Section N="User">{}</Section>"#,
            user("Note", "2+2", "7"),
        ),
    );
    let twelve = shape(
        12,
        &format!(
            r#"{}{}{}{}
<Cell N="PinX" V="3.75" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="1.5" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="0.75" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1.5"/><Cell N="Y" V="0"/></Row></Section>
<Section N="User">{}{}</Section>"#,
            glue("BeginX", "3", 10, 1),
            glue("BeginY", "2", 10, 1),
            glue("EndX", "4.5", 11, 2),
            glue("EndY", "20", 11, 2),
            user("Tag", "2+2", "9"),
            user("Len", "Width*2", "123"),
        ),
    );
    let thirteen = shape_with(
        13,
        r#" Master="7""#,
        r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="6"/><Cell N="LocPinX" V="9" F="Inh"/><Cell N="TxtPinX" V="42" F="Inh"/>"#,
    );
    let fourteen = shape_with(14, r#" Master="7""#, r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="6"/><Cell N="TxtPinY" V="3" F="Inh"/>"#);
    let fifteen = shape(
        15,
        &format!(
            r#"{}{}{}{}<Cell N="PinX" V="3" F="(BeginX+EndX)/2"/><Cell N="Width" V="1" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/>
<Cell N="Height" V="0"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>"#,
            glue("BeginX", "3", 13, 1),
            glue("BeginY", "6", 13, 1),
            glue("EndX", "99", 14, 2),
            glue("EndY", "6", 14, 2),
        ),
    );
    format!("{ten}{eleven}{twelve}{thirteen}{fourteen}{fifteen}")
}

fn connects() -> &'static str {
    r#"<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>
<Connect FromSheet="15" FromCell="BeginX" FromPart="9" ToSheet="13" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="15" FromCell="EndX" FromPart="12" ToSheet="14" ToCell="Connections.X2" ToPart="101"/>"#
}

fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&master_shape()))
        .page(PageSpec::new(1, "Page-1").shapes(&page_shapes()).connects(connects()))
        .build()
}

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-scope-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A session that has opened the drawing. The file is read whole, so it is removed at once.
fn opened(dir: &Dir) -> Session {
    let path = dir.path("in.vsdx");
    std::fs::write(&path, drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

/// Every cell of every top-level shape of page 1 as text: shape, section, row and cell, with `F`, `V`, `U` and `E`.
fn dump(doc: &Document) -> BTreeMap<String, String> {
    fn cell_text(c: &diacraft_doc::Cell) -> String {
        format!("F={:?} V={:?} U={:?} E={:?}", c.formula.text(), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref(), c.error.as_deref())
    }
    fn sheet_cells(out: &mut BTreeMap<String, String>, at: &str, sheet: &Sheet) {
        for (name, c) in &sheet.cells {
            out.insert(format!("{at}.{}", name.as_str()), cell_text(c));
        }
        for section in &sheet.sections {
            for (name, c) in &section.cells {
                out.insert(format!("{at}.{}[{:?}].{}", section.name.as_str(), section.ix, name.as_str()), cell_text(c));
            }
            for (place, row) in section.rows.iter().enumerate() {
                for (name, c) in &row.cells {
                    out.insert(
                        format!("{at}.{}[{:?}].row{place}({:?}{:?}).{}", section.name.as_str(), section.ix, row.name, row.ix, name.as_str()),
                        cell_text(c),
                    );
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for id in [10, 11, 12, 13, 14, 15] {
        let Some(at) = doc.find_shape(Owner::Page(1), id) else { continue };
        sheet_cells(&mut out, &format!("shape {id}"), &doc.shape(&at).unwrap().sheet);
    }
    out
}

/// What differs between two dumps: the cells that changed, were added, or went.
fn differing(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, text) in after {
        if before.get(key) != Some(text) {
            out.push(key.clone());
        }
    }
    out.extend(before.keys().filter(|k| !after.contains_key(*k)).cloned());
    out.sort();
    out
}

/// The cells the move of shape 10 changes, in the order of their names: the pin the command writes, the user cell that reads it and the one
/// that reads that (its `User` rows are Echo, Note, Far, Peer, Twice, in that order in the file), and the cells of connector 12 that follow from
/// its begin: `BeginX`, then the pin, the width and the local pin, and the user cell that reads the width (its rows are Tag, Len). Connector 12 is
/// the file's, glued, with an end that moved, so it is routed again: its two rows of the file become the four of a Z from (4, 2)
/// to the end the file says it is at, (4.5, 20), and nothing else of its `Geometry` section is written. Nothing else changes.
/// (`BeginY` and `Angle` are worked out and come out as they were, so they are not written; `EndY` says 20 where its formula gives 2, and nothing
/// the move touched is read by it, so it is not worked out.)
const EXPECTED: [&str; 16] = [
    "shape 10.PinX",
    "shape 10.User[None].row0(Some(\"Echo\")None).Value",
    "shape 10.User[None].row4(Some(\"Twice\")None).Value",
    "shape 12.BeginX",
    "shape 12.Geometry[Some(0)].row0(NoneSome(1)).X",
    "shape 12.Geometry[Some(0)].row0(NoneSome(1)).Y",
    "shape 12.Geometry[Some(0)].row1(NoneSome(2)).X",
    "shape 12.Geometry[Some(0)].row1(NoneSome(2)).Y",
    "shape 12.Geometry[Some(0)].row2(NoneSome(3)).X",
    "shape 12.Geometry[Some(0)].row2(NoneSome(3)).Y",
    "shape 12.Geometry[Some(0)].row3(NoneSome(4)).X",
    "shape 12.Geometry[Some(0)].row3(NoneSome(4)).Y",
    "shape 12.LocPinX",
    "shape 12.PinX",
    "shape 12.User[None].row1(Some(\"Len\")None).Value",
    "shape 12.Width",
];

#[test]
fn moving_a_shape_changes_its_own_cells_and_those_that_read_them_and_no_other() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = dump(s.document().unwrap());
    let doc_before = s.document().unwrap().clone();
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 1.0, "dy": 0.0}));
    let after = dump(s.document().unwrap());
    assert_eq!(differing(&before, &after), EXPECTED, "the cells the move changed");
    // The values are the right ones: the shape is 1 to the right, so the begin of the connector is 1 to the right and the connector is shorter.
    let moved = after.get("shape 10.PinX").unwrap();
    assert!(moved.contains("V=Some(\"3\")"), "{moved}");
    assert!(after.get("shape 12.BeginX").unwrap().contains("V=Some(\"4\")"), "{}", after["shape 12.BeginX"]);
    assert!(after.get("shape 12.Width").unwrap().contains("V=Some(\"0.5\")"), "{}", after["shape 12.Width"]);
    // The cells this evaluator disagrees with the file about, and nothing the move touched reads them, keep their text.
    for kept in [
        "shape 10.TxtPinY",
        "shape 11.User[None].row0(Some(\"Note\")None).Value",
        "shape 12.User[None].row0(Some(\"Tag\")None).Value",
        "shape 12.EndY",
        "shape 13.LocPinX",
        "shape 13.TxtPinX",
        "shape 14.TxtPinY",
        "shape 15.EndX",
    ] {
        assert_eq!(before.get(kept), after.get(kept), "{kept} was not touched and keeps its text");
    }
    assert_eq!(check_session(&s), Ok(()));
    // The document was not otherwise changed: no cell was added to an instance for a master value this evaluator disagrees with.
    let d = s.document().unwrap();
    assert_eq!(d.masters, doc_before.masters);
    assert_eq!(d.page(1).unwrap().connects, doc_before.page(1).unwrap().connects);
}

#[test]
fn a_page_wide_recalculation_would_have_rewritten_the_cells_the_move_left_alone() {
    // The fixture is only worth something if the old way changed what the new way does not.
    let dir = Dir::new();
    let mut s = opened(&dir);
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 1.0, "dy": 0.0}));
    let moved = s.document().unwrap().clone();
    let mut everything = moved.clone();
    let report = commit_in(&mut everything, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    let rewritten = differing(&dump(&moved), &dump(&everything));
    assert!(rewritten.len() >= 8, "the page-wide commit rewrote only {rewritten:?}");
    assert!(report.written >= 8 && report.added >= 1, "{report:?}");
}

/// The text of each shape's element in a saved page, by `ID`.
fn shapes_of_page(bytes: &[u8]) -> BTreeMap<u32, String> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let name = pkg.parts().find(|p| p.ends_with("/pages/page1.xml")).expect("the page part").to_owned();
    let xml = String::from_utf8(pkg.part(&name).unwrap().to_vec()).unwrap();
    let mut out = BTreeMap::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<Shape ID=\"") {
        let tail = &rest[start..];
        let end = tail.find("</Shape>").map_or(tail.len(), |e| e + "</Shape>".len());
        let id: u32 = tail["<Shape ID=\"".len()..].split('"').next().unwrap().parse().unwrap();
        out.insert(id, tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

/// The `<Cell .../>` elements of one shape's text, by their `N` and the place of the row they are in.
fn cells_of(shape: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = shape;
    while let Some(start) = rest.find("<Cell ") {
        let tail = &rest[start..];
        let end = tail.find('>').map_or(tail.len(), |e| e + 1);
        out.push(tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

#[test]
fn a_save_after_the_move_leaves_every_other_cell_of_the_file_byte_for_byte_as_it_was() {
    let dir = Dir::new();
    let mut untouched = opened(&dir);
    let kept = dir.path("kept.vsdx");
    run(&mut untouched, "file.save_as", json!({"path": kept}));
    let mut s = opened(&dir);
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 1.0, "dy": 0.0}));
    let edited = dir.path("edited.vsdx");
    run(&mut s, "file.save_as", json!({"path": edited}));
    let (a, b) = (shapes_of_page(&std::fs::read(&kept).unwrap()), shapes_of_page(&std::fs::read(&edited).unwrap()));
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "the same shapes");
    // Every shape but the two the move reached is the same text.
    for id in [11, 13, 14, 15] {
        assert_eq!(a[&id], b[&id], "shape {id}");
    }
    // In the two it reached, the cells that differ are the ones the dependents say, and the rest of the text is the same.
    let (mut differing_cells, mut kept_cells) = (0, 0);
    // The route of connector 12 is four rows where the file had two: its `Geometry` section is compared on its own.
    let route_of = |text: &str| text.split_once("<Section N=\"Geometry\"").map(|(_, rest)| rest.split("</Section>").next().unwrap_or("").to_owned());
    assert_ne!(route_of(&a[&12]), route_of(&b[&12]), "the file's two rows became a route");
    let without_route = |text: &str| match text.split_once("<Section N=\"Geometry\"") {
        Some((head, rest)) => format!("{head}{}", rest.split_once("</Section>").map_or("", |(_, tail)| tail)),
        None => text.to_owned(),
    };
    for id in [10, 12] {
        let (ca, cb) = (cells_of(&without_route(&a[&id])), cells_of(&without_route(&b[&id])));
        assert_eq!(ca.len(), cb.len(), "shape {id} holds the same cells");
        for (x, y) in ca.iter().zip(&cb) {
            if x == y {
                kept_cells += 1;
            } else {
                differing_cells += 1;
            }
        }
    }
    assert_eq!(differing_cells, 8, "the cells of shapes 10 and 12 that changed");
    assert_eq!(kept_cells, 13, "the rest of their cells (8 + 13 hold 21, without the route) are as they were");
    // And the saved file reads back as the drawing in the window.
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": edited}));
    assert_eq!(dump(again.document().unwrap()), dump(s.document().unwrap()));
}

#[test]
fn taking_a_shape_out_reaches_the_cells_that_named_it_and_the_ends_glued_to_it_and_nothing_else() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = dump(s.document().unwrap());
    run(&mut s, "shape.delete", json!({"ids": [13]}));
    let after = dump(s.document().unwrap());
    let changed = differing(&before, &after);
    // Shape 13 and all its cells go; connector 15's begin, which was glued to it, holds the point it was at, and the pin, width and local pin
    // that read it are worked out again (this file's `EndX` says 99 where its formula gives 5, and they are worked out from the formula); and the
    // cell of shape 10 that read `Sheet.13` holds an error. The cell of shape 10 that reads *that* one is not touched.
    let others: Vec<&String> = changed.iter().filter(|c| !c.starts_with("shape 13.")).collect();
    assert_eq!(
        others,
        [
            "shape 10.User[None].row2(Some(\"Far\")None).Value",
            "shape 15.BeginX",
            "shape 15.BeginY",
            "shape 15.LocPinX",
            "shape 15.PinX",
            "shape 15.Width",
        ],
        "{changed:?}"
    );
    assert!(after["shape 10.User[None].row2(Some(\"Far\")None).Value"].contains("E=Some(\"#REF!\")"), "{after:?}");
    assert!(after["shape 15.BeginX"].contains("F=None"), "an end that is no longer glued is a value: {}", after["shape 15.BeginX"]);
    // Every other cell keeps its text: 15's stale `EndX` of 99 and the cell of shape 10 that reads the one that went to an error.
    assert_eq!(before["shape 15.EndX"], after["shape 15.EndX"]);
    assert_eq!(before["shape 10.User[None].row4(Some(\"Twice\")None).Value"], after["shape 10.User[None].row4(Some(\"Twice\")None).Value"]);
    assert_eq!(check_session(&s), Ok(()));
}
