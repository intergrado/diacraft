//! A connector glued to a shape as a whole is saved to a VSDX file the way the application writes it, so that the application keeps the glue when it loads the file.
//!
//! The application rebuilds glue from the formulas of the end cells when it loads a file, and drops a `Connect` row whose end cell holds no glue formula. A connector that DiaCraft glued
//! to the whole of a shape (a shape with no connection point, public issue 16) had plain values in its end cells and nothing in its trigger cells, and the application dropped the glue
//! without a word; a copy whose end cells held the `_WALKGLUE` formulas and whose trigger cell held an `_XFTRIGGER` naming the target stayed glued
//! (`_WALKGLUE`-R1 and `_XFTRIGGER`-R1).
//!
//! These tests read the bytes of the file written. The drawing is the one of `connect_whole.rs`: shapes 1 and 2 have no connection point, shape 3 has four.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::connect_whole::{box_of, end_at, id_of, num_on, on_outline, opened, reopened};
use crate::edit::doc;
use crate::edit_foreign::Dir;
use crate::odg_build::{Dir as OdgDir, Drawing, id_of as odg_id_of, open_drawing};
use crate::{check_session, run};

/// The formula of the begin's cells and of the end's (the end's own trigger first, then the other end's, then `WalkPreference`).
const BEGIN: &str = "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)";
const END: &str = "_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)";

/// The text of the first page of the VSDX file at `path`.
pub(crate) fn page_xml(path: &str) -> String {
    page_part(&std::fs::read(path).unwrap())
}

/// The name of the part that holds the first page of a VSDX package.
const PAGE_PART: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines

/// The text of the first page of the VSDX package `bytes`.
pub(crate) fn page_part(bytes: &[u8]) -> String {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(pkg.part(PAGE_PART).expect("page 1").to_vec()).unwrap()
}

/// The start tag of cell `name` of shape `id` (a top-level shape without members, as every connector here is), or `None`.
pub(crate) fn cell_tag(xml: &str, id: u32, name: &str) -> Option<String> {
    let start = xml.find(&format!(r#"<Shape ID="{id}""#))?;
    let shape = &xml[start..start + xml[start..].find("</Shape>")?];
    let at = shape.find(&format!(r#"<Cell N="{name}" "#))?;
    Some(shape[at..at + shape[at..].find("/>")? + 2].to_owned())
}

/// An attribute of a start tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(r#" {name}=""#))? + name.len() + 3;
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// The cell `name` of shape `id` in the file: its `F`, its `V` as a number if it is one, and its `U`.
pub(crate) fn written(xml: &str, id: u32, name: &str) -> (Option<String>, Option<String>, Option<String>) {
    let tag = cell_tag(xml, id, name).unwrap_or_else(|| panic!("shape {id} has no cell {name} in the file: {xml}"));
    (attr(&tag, "F"), attr(&tag, "V"), attr(&tag, "U"))
}

/// The `Connect` row of connector `id`'s end `cell` as the file holds it, or `None`.
pub(crate) fn connect_row(xml: &str, id: u32, cell: &str) -> Option<String> {
    let start = xml.find(&format!(r#"<Connect FromSheet="{id}" FromCell="{cell}" "#))?;
    Some(xml[start..start + xml[start..].find("/>")? + 2].to_owned())
}

/// Asserts that the four end cells of connector `c` and the two triggers are as the application writes whole-shape glue to `begin_shape` and `end_shape`, with the end cells' `V` where
/// the session has the ends. A `None` shape is an end that is not glued to the whole of a shape: its cells are not looked at.
#[track_caller]
fn assert_whole_glue(xml: &str, d: &Document, c: u32, begin_shape: Option<u32>, end_shape: Option<u32>) {
    assert_whole_glue_on(xml, d, d.pages.get(0).unwrap().id, c, begin_shape, end_shape);
}

/// [`assert_whole_glue`] for a connector on page `page` of the session's drawing.
#[track_caller]
fn assert_whole_glue_on(xml: &str, d: &Document, page: u32, c: u32, begin_shape: Option<u32>, end_shape: Option<u32>) {
    for (shape, x, y, formula, trigger) in [(begin_shape, "BeginX", "BeginY", BEGIN, "BegTrigger"), (end_shape, "EndX", "EndY", END, "EndTrigger")] {
        let Some(shape) = shape else { continue };
        for cell in [x, y] {
            let (f, v, u) = written(xml, c, cell);
            assert_eq!(f.as_deref(), Some(formula), "{cell} holds the formula of whole-shape glue");
            assert_eq!(u, None, "{cell}: the result of `_WALKGLUE` is a length with no unit");
            let v: f64 = v.unwrap_or_else(|| panic!("{cell} has a value")).parse().unwrap();
            let now = num_on(d, page, c, cell);
            assert!((v - now).abs() < 1.0e-9, "{cell}: the file says {v}, the session {now}");
        }
        let (f, v, u) = written(xml, c, trigger);
        assert_eq!(f, Some(format!("_XFTRIGGER(Sheet.{shape}!EventXFMod)")), "{trigger} names the target's EventXFMod");
        assert_eq!((v.as_deref(), u), (Some("2"), None), "{trigger}: a glued end's trigger holds 2, with no unit");
    }
}

/// Opens the drawing, glues a connector from shape `from` to shape `to` with `connector.add`, and saves it as a VSDX file. → the session, the connector, the path.
fn made(dir: &Dir, from: u32, to: u32) -> (Session, u32, String) {
    let mut s = opened(dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": from}, "to": {"id": to}})));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    (s, c, path)
}

#[test]
fn the_connector_of_issue_16_is_saved_with_walkglue_in_its_end_cells_and_xftrigger_in_its_triggers() {
    // The report's edit: `connector.add` from a shape with no connection point to another, then a save.
    let dir = Dir::new();
    let (s, c, path) = made(&dir, 1, 2);
    let xml = page_xml(&path);
    assert_eq!(
        connect_row(&xml, c, "BeginX").as_deref(),
        Some(format!(r#"<Connect FromSheet="{c}" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/>"#).as_str())
    );
    assert_eq!(
        connect_row(&xml, c, "EndX").as_deref(),
        Some(format!(r#"<Connect FromSheet="{c}" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#).as_str())
    );
    assert_whole_glue(&xml, doc(&s), c, Some(1), Some(2));
    // The values are where the whole-shape rule puts the ends: the shapes are level, so the begin is the middle of the right side of shape 1 and the end the middle of the left side of shape 2.
    let d = doc(&s);
    let (begin, end) = (end_at(d, c, true), end_at(d, c, false));
    assert!((begin.0 - 3.0).abs() < 1.0e-9 && (begin.1 - 2.0).abs() < 1.0e-9, "{begin:?}");
    assert!((end.0 - 7.0).abs() < 1.0e-9 && (end.1 - 2.0).abs() < 1.0e-9, "{end:?}");
    assert!(on_outline(begin, box_of(d, 1)) && on_outline(end, box_of(d, 2)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_session_holds_the_formulas_so_a_save_into_the_opened_file_and_a_save_as_diacraft_keep_them() {
    // The cells of the session are the cells of the file: the model holds the formulas, so that a save into the file the drawing was opened from (a splice) and a save from the model
    // write the same, and a drawing saved as DiaCraft's own format keeps them.
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    let spliced = {
        run(&mut s, "file.save", json!({}));
        page_xml(&dir.path("in.vsdx"))
    };
    assert_whole_glue(&spliced, doc(&s), c, Some(1), Some(2));
    let native = dir.path("out.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let back = reopened(&native);
    let d = doc(&back);
    let formula = |cell: &str| {
        let at = d.find_shape(diacraft_doc::Owner::Page(1), c).unwrap();
        match d.shape(&at).unwrap().sheet.lookup(&diacraft_doc::CellAddr::direct(cell)) {
            diacraft_doc::Lookup::Found(cell) => cell.formula.text().map(str::to_owned),
            _ => None,
        }
    };
    assert_eq!(formula("BeginX").as_deref(), Some(BEGIN));
    assert_eq!(formula("EndY").as_deref(), Some(END));
    assert_eq!(formula("BegTrigger").as_deref(), Some("_XFTRIGGER(Sheet.1!EventXFMod)"));
    assert_eq!(formula("EndTrigger").as_deref(), Some("_XFTRIGGER(Sheet.2!EventXFMod)"));
}

#[test]
fn reopened_the_connector_follows_a_shape_that_moves_and_the_formulas_stay_in_the_file() {
    // The application's own check moved the shape by (1.0, 0.5) and the end went with it. Here the whole-shape rule places the end, from a file that holds the formulas.
    let dir = Dir::new();
    let (_, c, path) = made(&dir, 1, 2);
    let mut back = reopened(&path);
    let before = end_at(doc(&back), c, true);
    run(&mut back, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    let d = doc(&back);
    let after = end_at(d, c, true);
    assert!(on_outline(after, box_of(d, 1)), "the begin {after:?} is on the moved box {:?}", box_of(d, 1));
    assert!(after.0 > before.0 + 0.9, "the end went right with the shape: {before:?} to {after:?}");
    assert!(after.1 > before.1 + 0.4 || on_outline(after, box_of(d, 1)), "and up: {after:?}");
    // The other end is not moved by a move of this shape's.
    assert_eq!(end_at(d, c, false), (7.0, 2.0));
    // The save writes the formulas again, with the values of the moved ends.
    run(&mut back, "file.save", json!({}));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&back), c, Some(1), Some(2));
    assert!(connect_row(&xml, c, "BeginX").is_some_and(|r| r.contains(r#"ToSheet="1""#) && r.contains(r#"ToPart="3""#)));
    assert_eq!(check_session(&back), Ok(()));
    // The other shape moves, and the other end follows it.
    run(&mut back, "shape.move", json!({"page": 1, "ids": [2], "dx": 0.0, "dy": 3.0}));
    let d = doc(&back);
    assert!(on_outline(end_at(d, c, false), box_of(d, 2)), "the end {:?} is on the moved box {:?}", end_at(d, c, false), box_of(d, 2));
    assert!(end_at(d, c, false).1 > 3.0, "it went up with its shape: {:?}", end_at(d, c, false));
}

#[test]
fn an_end_glued_to_a_connection_point_is_written_as_before_and_only_the_whole_shape_end_holds_the_glue_formulas() {
    // From shape 1 (no point) to shape 3 (four points): the begin is whole-shape glue and the end is point glue.
    let dir = Dir::new();
    let (s, c, path) = made(&dir, 1, 3);
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), c, Some(1), None);
    let (f, _, _) = written(&xml, c, "EndX");
    assert!(f.as_deref().is_some_and(|f| f.starts_with("PAR(PNT(Sheet.3!Connections.X")), "the end is glued to a point: {f:?}");
    assert!(!f.unwrap_or_default().contains("_WALKGLUE"));
    assert!(cell_tag(&xml, c, "EndTrigger").is_none(), "a point end has no trigger, as before");
    // A connector glued to points only, or to nothing, holds no `_WALKGLUE` and no trigger.
    let mut s = s;
    let free = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 3, "point": 0}, "to": {"x": 9.0, "y": 4.0}})));
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    for cell in ["BeginX", "BeginY", "EndX", "EndY", "BegTrigger", "EndTrigger"] {
        let tag = cell_tag(&xml, free, cell);
        assert!(tag.as_deref().is_none_or(|t| !t.contains("_WALKGLUE") && !t.contains("_XFTRIGGER")), "{cell}: {tag:?}");
    }
}

#[test]
fn a_group_with_no_points_is_glued_to_as_a_whole_with_a_trigger_that_names_the_group() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2]})));
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": group}, "to": {"id": 3}})));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), c, Some(group), None);
    assert!(connect_row(&xml, c, "BeginX").is_some_and(|r| r.contains(&format!(r#"ToSheet="{group}""#)) && r.contains(r#"ToPart="3""#)));
    // It follows the group when the group moves, in the session and from the file.
    let mut back = reopened(&path);
    run(&mut back, "shape.move", json!({"page": 1, "ids": [group], "dx": 0.0, "dy": 4.0}));
    let d = doc(&back);
    assert!(on_outline(end_at(d, c, true), box_of(d, group)), "{:?} on the moved group's box {:?}", end_at(d, c, true), box_of(d, group));
    assert_eq!(check_session(&back), Ok(()));
}

#[test]
fn reconnecting_an_end_writes_the_glue_formulas_for_a_whole_shape_and_takes_them_off_for_a_point() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    // A connector from shape 3's right point to a free place, whose end is then glued to the whole of shape 2.
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 3, "point": 0}, "to": {"x": 9.0, "y": 4.0}})));
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 2}}));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), c, None, Some(2));
    // To the whole of shape 1: the trigger names it now.
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 1}}));
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), c, None, Some(1));
    // To a point of shape 3: the end cells hold the point's formulas, and no `_WALKGLUE` is left in the cells of the end.
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 3, "point": 1}}));
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    for cell in ["EndX", "EndY"] {
        let (f, _, _) = written(&xml, c, cell);
        assert!(f.as_deref().is_some_and(|f| f.starts_with("PAR(PNT(Sheet.3!Connections.")), "{cell}: {f:?}");
    }
    // And the whole of shape 1 again, after the point.
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 1}}));
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_whole_glue(&page_xml(&path), doc(&s), c, None, Some(1));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn undo_takes_the_formulas_and_the_triggers_back_with_the_connector() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = doc(&s).clone();
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    let made = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*doc(&s), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(*doc(&s), made);
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_whole_glue(&page_xml(&path), doc(&s), c, Some(1), Some(2));
}

/// An `.odg` drawing: rectangles A and B, and a connector C glued to the whole of A and of B (`draw:start-shape` and `draw:end-shape` with no glue point), as LibreOffice writes it.
pub(crate) fn odg_drawing() -> Drawing {
    let rectangle = |name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64)| {
        format!(
            r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" xml:id="{id}" draw:id="{id}" svg:width="{w}cm" svg:height="{h}cm" svg:x="{x}cm" svg:y="{y}cm"><text:p/></draw:rect>"#
        )
    };
    let connector = r#"<draw:connector draw:style-name="gr2" draw:text-style-name="P1" draw:layer="layout" draw:name="C" draw:type="standard" draw:start-shape="a" draw:end-shape="b" svg:x1="5cm" svg:y1="11cm" svg:x2="9.5cm" svg:y2="15cm"><text:p/></draw:connector>"#;
    Drawing::new().page(&format!("{}{}{connector}", rectangle("A", "a", (3.0, 10.0, 4.0, 2.0)), rectangle("B", "b", (8.0, 14.0, 3.0, 2.0))))
}

#[test]
fn a_drawing_read_from_an_odg_file_is_saved_to_vsdx_with_the_glue_formulas_and_the_triggers() {
    // The `.odg` reader gives a whole-shape end plain values, as LibreOffice reads none from it; a VSDX file needs the formulas.
    let dir = OdgDir::new();
    let mut s = open_drawing(&dir, &odg_drawing());
    let (a, b, c) = {
        let d = doc(&s);
        (odg_id_of(d, 0, "A"), odg_id_of(d, 0, "B"), odg_id_of(d, 0, "C"))
    };
    // Where the session has the four end cells before the save: the file's results are those.
    let before: Vec<f64> = {
        let d = doc(&s);
        let page = d.pages.get(0).unwrap().id;
        ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|cell| num_on(d, page, c, cell)).collect()
    };
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    for (cell, want) in ["BeginX", "BeginY", "EndX", "EndY"].iter().zip(&before) {
        let v: f64 = written(&xml, c, cell).1.unwrap().parse().unwrap();
        assert!((v - want).abs() < 1.0e-9, "{cell}: the file says {v}, the drawing had {want}");
    }
    assert!(connect_row(&xml, c, "BeginX").is_some_and(|r| r.contains(&format!(r#"ToSheet="{a}""#)) && r.contains(r#"ToPart="3""#)), "{xml}");
    assert!(connect_row(&xml, c, "EndX").is_some_and(|r| r.contains(&format!(r#"ToSheet="{b}""#)) && r.contains(r#"ToPart="3""#)));
    assert_whole_glue(&xml, doc(&s), c, Some(a), Some(b));
    // The file read again: the glue is there, and the end follows its shape.
    let mut back = reopened(&path);
    let d = doc(&back);
    let page = d.pages.get(0).unwrap();
    let page_id = page.id;
    assert_eq!(page.connects.iter().filter(|r| r.from_sheet == c && r.to_part == Some(3)).count(), 2);
    run(&mut back, "shape.move", json!({"page": page_id, "ids": [a], "dx": 0.0, "dy": -1.0}));
    let d = doc(&back);
    let corners = diacraft_engine::draw::shape_corners(d, page_id, a).unwrap();
    let begin = (num_on(d, page_id, c, "BeginX"), num_on(d, page_id, c, "BeginY"));
    assert!(crate::connect_whole::on_box(corners, begin), "the begin {begin:?} is on the moved box {corners:?}");
    assert_eq!(check_session(&back), Ok(()));
}

#[test]
fn a_connector_of_a_drawing_that_holds_the_formulas_already_is_written_as_it_was() {
    // A file's own whole-shape glue (`_WALKGLUE` in the end cells, the triggers naming the targets) is kept verbatim, as the specification says, and a save without an edit is its bytes.
    let corners = |x: f64| {
        format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
        )
    };
    let connector = r#"<Cell N="BeginX" V="3" F="_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"/><Cell N="BeginY" V="2" F="_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"/><Cell N="EndX" V="7" F="_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"/><Cell N="EndY" V="2" F="_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"/><Cell N="PinX" V="5" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="BegTrigger" V="2" F="_XFTRIGGER(Sheet.1!EventXFMod)"/><Cell N="EndTrigger" V="2" F="_XFTRIGGER(Sheet.2!EventXFMod)"/>"#;
    let shapes = format!("{}{}{}", shape(1, &corners(2.0)), shape(2, &corners(8.0)), shape(3, connector));
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes).connects(connects)).build();
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = reopened(&path);
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_xml(&path), page_part(&bytes), "a save with no edit writes the page as it was read");
    // Moving the shape and saving keeps the formulas the file had.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), 3, Some(1), Some(2));
}

#[test]
fn a_file_written_before_this_change_with_plain_end_values_is_not_changed_by_a_save_with_no_edit() {
    // What an earlier build wrote: a `Connect` row to the whole of a shape and plain values in the end cells. An unedited save writes what it read.
    let corners = |x: f64| {
        format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
        )
    };
    let connector = r#"<Cell N="BeginX" V="3" U="IN"/><Cell N="BeginY" V="2" U="IN"/><Cell N="EndX" V="7" U="IN"/><Cell N="EndY" V="2" U="IN"/><Cell N="PinX" V="5" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>"#;
    let shapes = format!("{}{}{}", shape(1, &corners(2.0)), shape(2, &corners(8.0)), shape(3, connector));
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes).connects(connects)).build();
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = reopened(&path);
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_xml(&path), page_part(&bytes), "a save with no edit writes the page as it was read");
    // An edit that touches neither the connector nor its shapes leaves the connector as it was too.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [2], "dx": 0.0, "dy": 0.0}));
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_xml(&path), page_part(&bytes));
}

// ---- the other ways an end comes to be glued to the whole of a shape, and what happens to such an end afterwards ----------------------------------

#[test]
fn autoconnect_from_a_group_writes_the_formulas_and_the_triggers_of_both_ends() {
    let mut s = crate::blank_session();
    let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = crate::edit::drop_shape(&mut s, "ellipse", 3.5, 4.0);
    let group = u32::try_from(run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap()).unwrap();
    let made = run(&mut s, "connector.autoconnect", json!({"from": group, "direction": "right"}));
    let (connector, copy) = (u32::try_from(made["connector"].as_u64().unwrap()).unwrap(), u32::try_from(made["id"].as_u64().unwrap()).unwrap());
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_whole_glue(&page_xml(&path), doc(&s), connector, Some(group), Some(copy));
}

#[test]
fn halving_the_points_of_a_shape_leaves_an_end_glued_to_the_whole_with_the_formulas_and_the_trigger() {
    use crate::conn_points::act;
    use crate::conn_points_edges::{page_of, rectangle, session_with};
    let mut s = session_with(page_of(vec![rectangle(1, 2.0, 1.0)]));
    act(&mut s, 1, "double", json!({"edge": 2}));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 0}, "to": {"x": 5.0, "y": 1.5}})));
    act(&mut s, 1, "halve", json!({"edge": 2}));
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert!(connect_row(&xml, c, "BeginX").is_some_and(|r| r.contains(r#"ToSheet="1""#) && r.contains(r#"ToPart="3""#)), "{xml}");
    assert_whole_glue(&xml, doc(&s), c, Some(1), None);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn reversing_a_connector_turns_the_formulas_and_the_trigger_with_the_end() {
    // From the whole of a group to a free place: reversed, the free place is the begin and the whole of the group the end, and the end's cells hold the end's form of the formula.
    let mut s = crate::blank_session();
    let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = crate::edit::drop_shape(&mut s, "ellipse", 3.5, 4.0);
    let group = u32::try_from(run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap()).unwrap();
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": group}, "to": {"x": 9.0, "y": 6.0}})));
    run(&mut s, "connector.reverse_ends", json!({"id": c}));
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert_whole_glue(&xml, doc(&s), c, None, Some(group));
    let (f, _, _) = written(&xml, c, "BeginX");
    assert!(f.as_deref().is_none_or(|f| !f.contains("_WALKGLUE")), "the free end holds a place: {f:?}");
    assert!(connect_row(&xml, c, "EndX").is_some_and(|r| r.contains(&format!(r#"ToSheet="{group}""#)) && r.contains(r#"ToPart="3""#)));
    // And reversed again, the begin is the group's.
    run(&mut s, "connector.reverse_ends", json!({"id": c}));
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_whole_glue(&page_xml(&path), doc(&s), c, Some(group), None);
}

#[test]
fn a_connector_whose_shape_is_deleted_keeps_no_glue_formula_at_the_end_it_lost() {
    // The end that lost its shape is a place, as the delete rule leaves it; the other end is glued as it was.
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    run(&mut s, "shape.delete", json!({"ids": [1], "connectors": false}));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    assert!(connect_row(&xml, c, "BeginX").is_none(), "no row for the end that lost its shape");
    for cell in ["BeginX", "BeginY"] {
        let (f, _, _) = written(&xml, c, cell);
        assert!(f.as_deref().is_none_or(|f| !f.contains("_WALKGLUE")), "{cell} is a place now: {f:?}");
    }
    // The trigger that named the shape is the call the application writes when the shape it named is deleted (`_XFTRIGGER`-R1: `REF()`), with the value it had.
    let (f, v, _) = written(&xml, c, "BegTrigger");
    assert_eq!((f.as_deref(), v.as_deref()), (Some("_XFTRIGGER(REF())"), Some("2")), "no trigger names the shape that is gone");
    assert_whole_glue(&xml, doc(&s), c, None, Some(2));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_and_its_shapes_copied_and_pasted_keep_the_glue_formulas_and_name_the_copies_in_the_triggers() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    run(&mut s, "edit.copy", json!({"ids": [1, 2, c]}));
    let pasted = run(&mut s, "edit.paste", json!({"x": 6.0, "y": 8.0}));
    let ids: Vec<u32> = pasted["ids"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect();
    assert_eq!(ids.len(), 3, "{pasted}");
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let xml = page_xml(&path);
    // The copy of the connector is the one whose trigger names a shape other than 1 and 2.
    let copy = ids.iter().copied().find(|id| cell_tag(&xml, *id, "BeginX").is_some()).expect("the connector's copy");
    let (begin_target, end_target) = {
        let d = doc(&s);
        let rows: Vec<(String, u32)> = d
            .page(1)
            .unwrap()
            .connects
            .iter()
            .filter(|r| r.from_sheet == copy)
            .map(|r| (r.from_cell.as_deref().unwrap_or("").to_owned(), r.to_sheet))
            .collect();
        (rows.iter().find(|r| r.0 == "BeginX").map(|r| r.1), rows.iter().find(|r| r.0 == "EndX").map(|r| r.1))
    };
    assert!(
        begin_target.is_some_and(|t| t != 1) && end_target.is_some_and(|t| t != 2),
        "the copy is glued to the copies: {begin_target:?} {end_target:?}"
    );
    assert_whole_glue(&xml, doc(&s), copy, begin_target, end_target);
}

#[test]
fn a_drawing_an_earlier_build_saved_as_diacraft_is_given_the_formulas_when_it_is_saved_as_vsdx_and_keeps_them_in_the_next_save() {
    // The plain places of a file an earlier build wrote (and so of its `.diacraft` copy) are given the formulas by a save as VSDX; the drawing then is the file's, and a Save into that file keeps them.
    let corners = |x: f64| {
        format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
        )
    };
    let connector = r#"<Cell N="BeginX" V="3" U="IN"/><Cell N="BeginY" V="2" U="IN"/><Cell N="EndX" V="7" U="IN"/><Cell N="EndY" V="2" U="IN"/><Cell N="PinX" V="5" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>"#;
    // An earlier build's connector holds the route marker (a file's own connector, with none, keeps its plain places).
    let connector = format!("{connector}{ROUTE_MARKER}");
    let shapes = format!("{}{}{}", shape(1, &corners(2.0)), shape(2, &corners(8.0)), shape(3, &connector));
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#;
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes).connects(connects)).build()).unwrap();
    let mut s = reopened(&path);
    let native = dir.path("old.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut s = reopened(&native);
    let out = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": out }));
    assert_whole_glue(&page_xml(&out), doc(&s), 3, Some(1), Some(2));
    // The drawing is the file's now: an edit and a Save into the file keep the formulas.
    run(&mut s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    assert_whole_glue(&page_xml(&out), doc(&s), 3, Some(1), Some(2));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a file's own whole-shape ends are what the other program wrote ("Only DiaCraft's ends") --------------------------------------

/// The marker row a connector of DiaCraft's holds: one an earlier build made, and one the `.odg` reader read.
const ROUTE_MARKER: &str = r#"<Section N="User"><Row N="DiaCraftRoute"><Cell N="Value" V="1"/></Row></Section>"#;

/// The six end cells of a connector that is glued to the whole of two shapes and holds plain places: the begin and the end, and the trigger of the end alone, which names shape 2 (the begin's
/// has none).
const PLAIN_ENDS: [&str; 6] = ["BeginX", "BeginY", "EndX", "EndY", "BegTrigger", "EndTrigger"];

/// A VSDX file of another program: boxes 1 and 2, and connector 3 glued to the whole of both (`Connect` rows with `ToPart` 3), its end cells plain places and its end holding a trigger that
/// names shape 2. `marked` gives the connector the route marker of DiaCraft's.
fn plain_glue_file(marked: bool) -> Vec<u8> {
    let corners = |x: f64| {
        format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
        )
    };
    let connector = format!(
        r#"<Cell N="BeginX" V="3" U="IN"/><Cell N="BeginY" V="2" U="IN"/><Cell N="EndX" V="7" U="IN"/><Cell N="EndY" V="2" U="IN"/><Cell N="PinX" V="5" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="EndTrigger" V="2" F="_XFTRIGGER(Sheet.2!EventXFMod)"/>{}"#,
        if marked { ROUTE_MARKER } else { "" }
    );
    let shapes = format!("{}{}{}", shape(1, &corners(2.0)), shape(2, &corners(8.0)), shape(3, &connector));
    let connects = r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/><Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#;
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes).connects(connects)).build()
}

/// The six cells of connector 3 as page `xml` holds them: the formula, the value and the unit of each, or `None` for a cell it does not hold.
type EndCells = Vec<(&'static str, Option<(Option<String>, Option<String>, Option<String>)>)>;

fn end_cells(xml: &str) -> EndCells {
    PLAIN_ENDS.iter().map(|name| (*name, cell_tag(xml, 3, name).map(|t| (attr(&t, "F"), attr(&t, "V"), attr(&t, "U"))))).collect()
}

#[test]
fn a_files_own_whole_shape_ends_are_what_the_other_program_wrote_on_every_save_route() {
    let bytes = plain_glue_file(false);
    let before = end_cells(&page_part(&bytes));
    let cell = |f: Option<&str>, v: &str, u: Option<&str>| Some((f.map(str::to_owned), Some(v.to_owned()), u.map(str::to_owned)));
    let expected: EndCells = vec![
        ("BeginX", cell(None, "3", Some("IN"))),
        ("BeginY", cell(None, "2", Some("IN"))),
        ("EndX", cell(None, "7", Some("IN"))),
        ("EndY", cell(None, "2", Some("IN"))),
        ("BegTrigger", None),
        ("EndTrigger", cell(Some("_XFTRIGGER(Sheet.2!EventXFMod)"), "2", None)),
    ];
    assert_eq!(before, expected, "the file holds plain places, a trigger on the end and none on the begin");
    // Save As another file of the same type: the splice.
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = reopened(&path);
    let spliced = dir.path("spliced.vsdx");
    run(&mut s, "file.save_as", json!({ "path": spliced }));
    assert_eq!(end_cells(&page_xml(&spliced)), before, "Save As VSDX: the ends are the file's");
    // Save As `.diacraft`, which is then saved as VSDX: the model writer, which gave every plain whole-shape end the formulas.
    let mut s = reopened(&path);
    let native = dir.path("own.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut s = reopened(&native);
    let across = dir.path("across.vsdx");
    run(&mut s, "file.save_as", json!({ "path": across }));
    let xml = page_xml(&across);
    assert_eq!(end_cells(&xml), before, "Save As .diacraft and then as VSDX: the ends are the file's too");
    assert!(!xml.contains("_WALKGLUE"), "no glue formula was put in a file's own end: {xml}");
    assert!(cell_tag(&xml, 3, "BegTrigger").is_none(), "and no trigger on the end that had none");
    // The glue is the file's too: the `Connect` rows are there as they were.
    assert!(connect_row(&xml, 3, "BeginX").is_some_and(|r| r.contains(r#"ToSheet="1""#) && r.contains(r#"ToPart="3""#)), "{xml}");
    assert!(connect_row(&xml, 3, "EndX").is_some_and(|r| r.contains(r#"ToSheet="2""#) && r.contains(r#"ToPart="3""#)), "{xml}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_of_ours_with_plain_ends_is_given_the_formulas_and_the_triggers_when_it_is_saved_across() {
    // The same drawing with the route marker: an earlier build's connector (and an `.odg`'s) is DiaCraft's, and a VSDX type needs the formulas of it.
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, plain_glue_file(true)).unwrap();
    let mut s = reopened(&path);
    let native = dir.path("ours.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut s = reopened(&native);
    let across = dir.path("across.vsdx");
    run(&mut s, "file.save_as", json!({ "path": across }));
    assert_whole_glue(&page_xml(&across), doc(&s), 3, Some(1), Some(2));
}

#[test]
fn an_end_the_person_glues_again_holds_the_formulas_and_the_end_they_left_stays_as_the_file_had_it() {
    // A file's connector the person edits: the end `connector.reconnect` glues to the whole of a shape is written with the glue, and the begin, which they did not touch, is the file's.
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, plain_glue_file(false)).unwrap();
    let mut s = reopened(&path);
    run(&mut s, "connector.reconnect", json!({"id": 3, "end": "end", "target": {"id": 1}}));
    let native = dir.path("edited.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut s = reopened(&native);
    let across = dir.path("across.vsdx");
    run(&mut s, "file.save_as", json!({ "path": across }));
    let xml = page_xml(&across);
    assert_whole_glue(&xml, doc(&s), 3, None, Some(1));
    for cell in ["BeginX", "BeginY"] {
        assert_eq!(written(&xml, 3, cell).0, None, "{cell}: the begin the person left is the file's, a place");
    }
    assert!(cell_tag(&xml, 3, "BegTrigger").is_none(), "and has no trigger, as the file had it");
}

#[test]
fn a_move_of_the_shape_a_files_own_end_is_glued_to_places_the_end_and_leaves_its_cell_a_plain_place() {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, plain_glue_file(false)).unwrap();
    let mut s = reopened(&path);
    let before = num_on(doc(&s), 1, 3, "BeginX");
    run(&mut s, "shape.move", json!({"page": 1, "ids": [1], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    let xml = page_xml(&path);
    let (f, v, _) = written(&xml, 3, "BeginX");
    assert_eq!(f, None, "the end is placed with the shape, and its cell is no formula");
    assert!(v.unwrap().parse::<f64>().unwrap() > before + 0.9, "the end went with the shape");
    assert!(!xml.contains("_WALKGLUE") && cell_tag(&xml, 3, "BegTrigger").is_none(), "nothing of the glue formulas was written: {xml}");
}
