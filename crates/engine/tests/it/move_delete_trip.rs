//! What a move and a delete change, on every save path (the problem classes `move` and `delete`, "other shapes changed").
//!
//! **The cause.** A page whose list holds two shapes of one `ID` (a file may keep a deleted shape, `Del="1"`, beside a live one of its `ID`) was a page
//! `calc::changes_between` gave up on, and the edit's recalculation then worked out **every** cell of the page: each instance was given, in local rows, the
//! inherited cells its master's formulas give it another value for, though the edit never reached it. It is **the engine's edit**
//! that wrote them, so every save path wrote them: the VSDX file saved straight after the move (the splice writer), the `.diacraft` file, and the VSDX
//! file the model writer makes from it. The comparison now tells such shapes apart by their order (`calc/src/changes.rs`), and these tests hold each path
//! to the source's rows. The calc crate's `scoped_repeats.rs` holds the comparison itself.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::run;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};

/// A master whose shape has a connection point, two rows of geometry and a user cell, each a formula of the shape's `Width` and `Height`, saved for the
/// master's own size (1 by 0.5).
const MASTER_SHAPE: &str = concat!(
    r#"<Shape ID="5" Type="Shape"><Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.25"/><Cell N="Width" V="1"/><Cell N="Height" V="0.5"/>"#,
    r#"<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.25" F="Height*0.5"/>"#,
    r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="0.5" F="Width*0.5"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#,
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
    r#"<Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#,
    r#"<Section N="User"><Row N="Span"><Cell N="Value" V="1" F="Width"/></Row></Section></Shape>"#,
);

/// Shape 1, which the tests move or delete.
const MOVED: &str =
    r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape>"#;

/// An instance of the master twice its width that relies on the master for every row: what its master's formulas give it differs from what the master
/// saved, and the file has no local row for it (a file another program saved may be so).
const INSTANCE: &str = r#"<Shape ID="2" Master="2" Type="Shape"><Cell N="PinX" V="4"/><Cell N="PinY" V="4"/><Cell N="Width" V="2"/></Shape>"#;

/// A deleted shape beside a live one of its `ID`.
const TWINS: &str = r#"<Shape ID="3" Del="1"/><Shape ID="3" Type="Shape"><Cell N="PinX" V="6"/><Cell N="PinY" V="6"/></Shape>"#;

fn drawing(twins: bool) -> Vec<u8> {
    let shapes = format!("{MOVED}{INSTANCE}{}", if twins { TWINS } else { "" });
    VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE)).page(PageSpec::new(0, "P").shapes(&shapes)).build()
}

/// The sections of shape 2 of the first page of a VSDX file, by name and number of rows.
fn instance_sections(bytes: &[u8]) -> Vec<String> {
    let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let page = doc.pages.iter().next().unwrap();
    let shape = page.shapes.iter().find(|s| s.id == 2).unwrap();
    shape.sheet.sections.iter().map(|s| format!("{}:{}", s.name.as_str(), s.rows.len())).collect()
}

/// Makes the edit in a session that opened `source`, and saves it the three ways: straight to VSDX (the splice path), as `.diacraft`, and from that,
/// reopened in a session of its own, to VSDX (the model writer). → the two VSDX files and the sections the instance has in the `.diacraft` session.
fn three_saves(source: &[u8], edit: &serde_json::Value, command: &str) -> (Vec<u8>, Vec<u8>, Vec<String>) {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", source) }));
    run(&mut s, command, edit.clone());
    run(&mut s, "file.save_as", json!({ "path": dir.path("splice.vsdx") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let t = opened(&dir.path("x.diacraft"));
    let in_diacraft = {
        let doc = t.document().unwrap();
        let shape = doc.pages.iter().next().unwrap().shapes.iter().find(|s| s.id == 2).unwrap().clone();
        shape.sheet.sections.iter().map(|s| format!("{}:{}", s.name.as_str(), s.rows.len())).collect()
    };
    let mut t = t;
    run(&mut t, "file.save_as", json!({ "path": dir.path("model.vsdx") }));
    (dir.read("splice.vsdx"), dir.read("model.vsdx"), in_diacraft)
}

#[test]
fn a_move_beside_two_shapes_of_one_id_gives_no_other_shape_a_row_on_any_save_path() {
    let none: Vec<String> = Vec::new();
    for twins in [false, true] {
        let source = drawing(twins);
        assert_eq!(instance_sections(&source), none, "the source's instance relies on its master");
        let (splice, model, in_diacraft) = three_saves(&source, &json!({ "ids": [1], "page": 0, "dx": 0.125, "dy": 0.125 }), "shape.move");
        assert_eq!(instance_sections(&splice), none, "twins {twins}: the splice save after the move wrote the instance's inherited rows");
        assert_eq!(in_diacraft, none, "twins {twins}: the move gave the instance rows in the model (the .diacraft file holds them)");
        assert_eq!(instance_sections(&model), none, "twins {twins}: the trip through .diacraft wrote the instance's inherited rows");
    }
}

#[test]
fn a_delete_beside_two_shapes_of_one_id_gives_no_other_shape_a_row_on_any_save_path() {
    let none: Vec<String> = Vec::new();
    for twins in [false, true] {
        let source = drawing(twins);
        let (splice, model, in_diacraft) = three_saves(&source, &json!({ "ids": [1], "page": 0 }), "shape.delete");
        assert_eq!(instance_sections(&splice), none, "twins {twins}: the splice save after the delete wrote the instance's inherited rows");
        assert_eq!(in_diacraft, none, "twins {twins}: the delete gave the instance rows in the model");
        assert_eq!(instance_sections(&model), none, "twins {twins}: the trip through .diacraft wrote the instance's inherited rows");
    }
}

#[test]
fn the_move_and_the_delete_still_reach_what_reads_the_shape_they_edit() {
    // The control: an instance whose own cell reads shape 1's pin is worked out after the move, beside the twins, and after the delete holds the error of a
    // reference to a shape that is gone. A comparison that saw no change at all would pass the two tests above.
    let reader = r#"<Shape ID="4" Type="Shape"><Section N="User"><Row N="Far"><Cell N="Value" V="11" F="Sheet.1!PinX+10"/></Row></Section></Shape>"#;
    let shapes = format!("{MOVED}{INSTANCE}{TWINS}{reader}");
    let source = VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE)).page(PageSpec::new(0, "P").shapes(&shapes)).build();
    let far = |bytes: &[u8]| {
        let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap();
        let shape = doc.pages.iter().next().unwrap().shapes.iter().find(|s| s.id == 4).unwrap().clone();
        let cell = shape.sheet.lookup(&diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name("Far"), "Value"));
        match cell {
            diacraft_doc::Lookup::Found(c) => (c.value.as_ref().map(|v| v.text().to_owned()), c.error.is_some()),
            other => panic!("{other:?}"),
        }
    };
    let (splice, model, _) = three_saves(&source, &json!({ "ids": [1], "page": 0, "dx": 0.125, "dy": 0.125 }), "shape.move");
    assert_eq!(far(&splice), (Some("11.125".to_owned()), false));
    assert_eq!(far(&model), (Some("11.125".to_owned()), false));
    let (splice, model, _) = three_saves(&source, &json!({ "ids": [1], "page": 0 }), "shape.delete");
    assert_eq!(far(&splice), (Some("11".to_owned()), true), "the reference names a shape that is gone");
    assert_eq!(far(&model), (Some("11".to_owned()), true));
}

/// The `Connect` rows of the first page of the session's drawing, as (from, to).
fn rows_of(s: &diacraft_engine::Session) -> Vec<(u32, u32)> {
    s.document().unwrap().pages.iter().next().unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect()
}

#[test]
fn a_delete_takes_out_the_rows_of_the_shapes_it_takes_out_and_not_of_another_shape_of_their_id() {
    // Connector 4 is glued to shape 9. Group 1 holds a member that is also `ID` 9. A `Connect` row finds the first shape of its `ID`: with the box first, the
    // row is the box's and stays when the group goes; with the group first, it is the member's and goes with it.
    let connector = concat!(
        r#"<Shape ID="4" Type="Shape"><Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="3"/><Cell N="EndY" V="1"/>"#,
        r#"<Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="PinX" V="2"/><Cell N="PinY" V="1"/><Cell N="ObjType" V="2"/></Shape>"#,
    );
    let boxed = r#"<Shape ID="9" Type="Shape"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape>"#;
    let group = concat!(
        r#"<Shape ID="1" Type="Group"><Cell N="PinX" V="6"/><Cell N="PinY" V="6"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Shapes>"#,
        r#"<Shape ID="9" Type="Shape"><Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape></Shapes></Shape>"#,
    );
    let row = r#"<Connect FromSheet="4" FromCell="BeginX" FromPart="9" ToSheet="9" ToCell="PinX" ToPart="3"/>"#;
    for (box_first, kept) in [(true, vec![(4, 9)]), (false, vec![])] {
        let shapes = if box_first { format!("{boxed}{group}{connector}") } else { format!("{group}{boxed}{connector}") };
        let source = VsdxBuilder::new().page(PageSpec::new(0, "P").shapes(&shapes).connects(row)).build();
        let dir = Dir::new();
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &source) }));
        assert_eq!(rows_of(&s), vec![(4, 9)]);
        run(&mut s, "shape.delete", json!({ "ids": [1], "page": 0 }));
        assert_eq!(rows_of(&s), kept, "the box first: {box_first}");
    }
}

#[test]
fn a_visit_through_diacraft_of_what_the_rule_makes_no_content_is_the_same_content() {
    // A shape whose text comes before its sections and whose foreign data comes before its text, an empty `Connects`, an empty `Colors` and an empty relationships
    // part of the page: the trip writes each the way the writer does (sections, text, foreign data; no empty container; no empty part) and the check counts them as
    // the same (the classes F1 and F2).
    let root = concat!(r#"<PageContents xmlns="http://schemas.microsoft.com/office/vi"#, r#"sio/2012/main">"#);
    let part = format!(
        concat!(
            r#"<?xml version="1.0"?>{root}<Shapes><Shape ID="1" Type="Shape"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/>"#,
            r#"<ForeignData ForeignType="Object"/><Text>Hello</Text><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
            r#"</Shape></Shapes><Connects/></PageContents>"#,
        ),
        root = root
    );
    let rels = concat!("/vi", "sio", "/pages/_rels/page1.xml.rels");
    let source = VsdxBuilder::new()
        .colors("")
        .page(PageSpec::new(0, "P").part(&part))
        .with(
            diacraft_testkit::package::Pkg::new()
                .part_without_override(rels, r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#),
        )
        .build();
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let mut t = opened(&dir.path("x.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let out = dir.read("b.vsdx");
    let d = diacraft_vsdx::compare_content(&source, &out, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(d.same(), "{:?}", d.shown);
    // The writer did write them its own way: the control that the check had something to forgive.
    assert_ne!(source, out);
}
