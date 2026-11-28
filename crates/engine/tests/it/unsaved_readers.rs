//! The readers that read a saved value for what is not a place, a size or a flag of a layer (the last of public issue 14's follow-ups): shape data, the text a field
//! shows, hyperlinks, the cells of a container, the cells a stencil's master is sized and dropped from, the rows of a connector's route, and a constructor text. A cell with a formula and
//! **no saved value** is what its formula works out to in each, as the picture draws it, and not a cell that holds nothing.
//!
//! Every drawing is built in code: the cells under test are `Cell::with_formula(.., None)`, as a file that leaves the value out beside a formula holds them.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_engine::draw::links::read_links;
use diacraft_engine::draw::shapedata::{Kind, read_fields};
use diacraft_engine::draw::{ConnectorPath, connector_path};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::hyperlinks::edit_shape;
use crate::stencil_open::{CT_STENCIL, Dir, ids, open};
use crate::{blank_session, run};

fn unsaved(formula: &str) -> Cell {
    Cell::with_formula(formula, None)
}

fn one_shape(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5").shape(shape)).build()
}

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

// ---- shape data -----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_cells_of_a_shape_data_row_that_are_a_formula_and_no_saved_value_are_what_the_formula_works_out_to() {
    let prop = |cell: &str| CellAddr::in_row("Property", None, RowKey::name("Cost"), cell);
    let built = one_shape(
        ShapeBuilder::new(1)
            .set(prop("Label"), unsaved("\"Price\""))
            .set(prop("Type"), unsaved("1+1"))
            .set(prop("Value"), unsaved("1+1"))
            .set(prop("Format"), unsaved("\"0.00\""))
            .set(prop("Prompt"), unsaved("\"Ask\""))
            .set(prop("Invisible"), unsaved("1"))
            .set(prop("Verify"), unsaved("1")),
    );
    let listed = read_fields(&built, 1, 1).unwrap();
    let field = &listed.fields[0];
    assert_eq!((field.label.as_str(), field.prompt.as_str(), field.format.as_str()), ("Price", "Ask", "0.00"));
    assert_eq!((field.kind, field.type_code, field.value.as_str()), (Kind::Number, Some(2), "2"), "the type 2 is a number: {field:?}");
    assert_eq!(field.shown, "2.00", "the value through the picture");
    assert!(field.invisible && field.ask, "the two flags: {field:?}");
}

// ---- the text a field shows -----------------------------------------------------------------------------------------------------------------------

/// The text a shape shows: its characters and the results of its fields, in order.
fn shown(s: &Session, id: u32) -> String {
    let at = doc(s).find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
    let shape = doc(s).shape(&at).unwrap();
    shape
        .text
        .as_ref()
        .map(|t| {
            t.parts
                .iter()
                .map(|p| match p {
                    diacraft_doc::TextPart::Chars(c) => c.to_string(),
                    diacraft_doc::TextPart::Fld { text, .. } => text.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_field_whose_format_is_a_formula_and_no_saved_value_shows_through_the_picture_the_formula_gives() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Ratio "}));
    let made = run(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": "1/3", "format": "0.00"}));
    assert_eq!(made["shown"], "0.33", "{made}");
    // A file that leaves the value out beside the formula of the picture: the `Format` cell is a formula and no value.
    edit_shape(&mut s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.set_cell(&CellAddr::in_row("Field", None, RowKey::Ix(0), "Format"), unsaved("\"0.0\"")).unwrap();
    });
    run(&mut s, "text.update_fields", json!({"id": id}));
    assert_eq!(shown(&s, id), "Ratio 0.3", "the picture is the one the formula gives");
}

// ---- hyperlinks -----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_cells_of_a_hyperlink_that_are_a_formula_and_no_saved_value_are_what_the_formula_works_out_to() {
    let link = |cell: &str| CellAddr::in_row("Hyperlink", None, RowKey::name("Row_1"), cell);
    let built = one_shape(
        ShapeBuilder::new(1)
            .set(link("Address"), unsaved("\"http://example.org/\""))
            .set(link("Description"), unsaved("\"The \"&\"site\""))
            .set(link("NewWindow"), unsaved("1"))
            .set(link("Default"), unsaved("1+0"))
            .set(link("Invisible"), unsaved("0")),
    );
    let links = read_links(&built, 1, 1).unwrap();
    let got = &links[0];
    assert_eq!((got.address.as_str(), got.description.as_str()), ("http://example.org/", "The site"), "{got:?}");
    assert_eq!((got.new_window, got.default, got.invisible), (true, true, false), "{got:?}");
}

// ---- the cells of a container -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_whose_structure_type_and_style_are_a_formula_and_no_saved_value_is_that_container() {
    let user = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    let built = one_shape(
        ShapeBuilder::new(1)
            .cell("PinX", "4")
            .cell("PinY", "4")
            .cell("Width", "3")
            .cell("Height", "2")
            .cell("LocPinX", "1.5")
            .cell("LocPinY", "1")
            .set(user("msvStructureType"), unsaved("\"Con\"&\"tainer\""))
            .set(user("DiaCraftContainerStyle"), unsaved("\"tint\"")),
    );
    let mut s = session_with(built);
    let info = run(&mut s, "container.info", json!({"ids": [1], "page": 1}));
    assert_eq!(info["containers"][0]["id"], json!(1), "the shape is a container: {info}");
    assert_eq!(info["containers"][0]["style"], json!("tint"), "{info}");
}

// ---- the sizes a stencil's master is listed and dropped by -----------------------------------------------------------------------------------------

/// A stencil of one master, "Sized", a box whose `Width` and `Height` are formulas of one number each and no saved value.
fn sized_stencil() -> Vec<u8> {
    let inner = r#"<Cell N="Width" F="1+1"/><Cell N="Height" F="0.5+0.5"/><Cell N="PinX" F="Width*0.5"/><Cell N="PinY" F="Height*0.5"/>
<Cell N="LocPinX" F="Width*0.5"/><Cell N="LocPinY" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" F="Width"/><Cell N="Y" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" F="Height"/></Row></Section>"#;
    VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "Sized").attrs(&ids(101, 201)).shapes(&shape(5, inner)))
        .page(PageSpec::new(1, "Page-1").without_part())
        .build()
}

#[test]
fn a_master_sized_by_formulas_and_no_saved_values_is_listed_and_dropped_at_the_size_they_work_out_to() {
    let dir = Dir::new();
    let path = dir.write("sized.vssx", &sized_stencil());
    let mut s = blank_session();
    let opened = open(&mut s, &path);
    let master = &opened["masters"][0];
    assert_eq!((master["width"].as_f64(), master["height"].as_f64()), (Some(2.0), Some(1.0)), "the listing: {master}");
    let dropped = run(&mut s, "shape.drop", json!({"master": "Sized", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!((dropped["width"].as_f64(), dropped["height"].as_f64()), (Some(2.0), Some(1.0)), "the drop: {dropped}");
}

// ---- the rows of a connector's route ----------------------------------------------------------------------------------------------------------------

/// A row of a route: its `IX`, its type and its `X` and `Y` as text.
type RouteRow<'a> = (u32, &'a str, [(&'a str, &'a str); 2]);

/// A connector from (1, 1) to (4, 3) in right angles: four rows, of which `unsaved` names the cells that are a formula and no saved value (`(row, cell, formula)`), and the rest are numbers.
fn connector_with(unsaved_cells: &[(u32, &str, &str)]) -> Document {
    let rows: [RouteRow; 4] = [
        (1, "MoveTo", [("X", "0"), ("Y", "0")]),
        (2, "LineTo", [("X", "1.5"), ("Y", "0")]),
        (3, "LineTo", [("X", "1.5"), ("Y", "2")]),
        (4, "LineTo", [("X", "3"), ("Y", "2")]),
    ];
    let mut shape = ShapeBuilder::new(1)
        .cell("BeginX", "1")
        .cell("BeginY", "1")
        .cell("EndX", "4")
        .cell("EndY", "3")
        .cell("PinX", "2.5")
        .cell("PinY", "2")
        .cell("Width", "3")
        .cell("Height", "2")
        .cell("LocPinX", "1.5")
        .cell("LocPinY", "1")
        .cell("Angle", "0");
    for (ix, kind, cells) in rows {
        let row = RowKey::Ix(ix);
        shape = shape.row_type("Geometry", Some(0), row.clone(), kind);
        for (name, value) in cells {
            let cell = match unsaved_cells.iter().find(|(r, n, _)| *r == ix && *n == name) {
                Some((_, _, formula)) => unsaved(formula),
                None => Cell::constant(diacraft_doc::Value::from_text(value)),
            };
            shape = shape.set(CellAddr::in_row("Geometry", Some(0), row.clone(), name), cell);
        }
    }
    one_shape(shape)
}

fn route(built: &Document) -> Option<ConnectorPath> {
    connector_path(built, 1, 1)
}

#[test]
fn the_route_of_a_connector_whose_rows_are_a_formula_and_no_saved_value_is_the_route_the_formulas_draw() {
    let control = route(&connector_with(&[])).expect("the route of the control");
    assert_eq!(control.points.len(), 4);
    let formulas = connector_with(&[(2, "X", "1+0.5"), (3, "X", "Width*0.5"), (3, "Y", "Height"), (4, "Y", "1+1")]);
    let got = route(&formulas).expect("a route whose rows are formulas is a route");
    assert_eq!(got.points.len(), control.points.len());
    for (a, b) in got.points.iter().zip(&control.points) {
        assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "{:?} against {:?}", got.points, control.points);
    }
}

// ---- the roles a selection and a shape operation read -----------------------------------------------------------------------------------------------

/// Two boxes on a page; shape 1's `User.msvStructureType` is the formula `"Container"` and no saved value.
fn page_with_a_container_by_formula() -> Document {
    let user = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    let boxed = |id: u32, x: &str| {
        ShapeBuilder::new(id).cell("PinX", x).cell("PinY", "4").cell("Width", "2").cell("Height", "1").cell("LocPinX", "1").cell("LocPinY", "0.5")
    };
    DocBuilder::new()
        .page(
            1,
            PageBuilder::new()
                .cell("PageWidth", "11")
                .cell("PageHeight", "8.5")
                .shape(boxed(1, "3").set(user("msvStructureType"), unsaved("\"Con\"&\"tainer\"")))
                .shape(boxed(2, "7")),
        )
        .build()
}

#[test]
fn select_by_role_finds_a_container_whose_type_is_a_formula_and_no_saved_value() {
    let mut s = session_with(page_with_a_container_by_formula());
    let picked = run(&mut s, "select.by_type", json!({"roles": ["container"], "page": 1}));
    assert_eq!(picked["ids"], json!([1]), "{picked}");
}

#[test]
fn a_shape_operation_refuses_a_container_whose_type_is_a_formula_and_no_saved_value() {
    let mut s = session_with(page_with_a_container_by_formula());
    let words = s.execute("shape.union", &json!({"ids": [1, 2], "page": 1})).unwrap_err().to_string();
    assert!(words.contains("container or a list"), "{words}");
}

// ---- the accessibility check's reading of a hyperlink -----------------------------------------------------------------------------------------------

#[test]
fn the_accessibility_check_reads_the_cells_of_a_hyperlink_that_are_a_formula_and_no_saved_value() {
    // A link whose description is its address says nothing of where it leads, and the check says so; the address and the description are the formulas' and no value is saved.
    let link = |cell: &str| CellAddr::in_row("Hyperlink", None, RowKey::name("Row_1"), cell);
    let built = one_shape(
        ShapeBuilder::new(1)
            .cell("PinX", "3")
            .cell("PinY", "3")
            .cell("Width", "2")
            .cell("Height", "1")
            .cell("LocPinX", "1")
            .cell("LocPinY", "0.5")
            .set(link("Address"), unsaved("\"http://example.org/\""))
            .set(link("Description"), unsaved("\"http://example.org/\"")),
    );
    let mut s = session_with(built);
    let checked = run(&mut s, "review.check_accessibility", json!({}));
    let found = checked["findings"].as_array().unwrap().iter().any(|f| f["message"].as_str().unwrap_or("").contains("has no description of its own"));
    assert!(found, "the link says nothing of where it leads: {checked}");
}

// ---- the connection points of an outline's edges ----------------------------------------------------------------------------------------------------

#[test]
fn a_connection_point_whose_cells_are_a_formula_and_no_saved_value_is_on_the_edge_where_the_formula_puts_it() {
    // A box 2 by 1 at the origin, drawn counterclockwise from its lower-left corner; its one connection point is at (`1+0`, 0), the middle of the bottom edge, and the formula alone is saved.
    let mut shape =
        ShapeBuilder::new(1).cell("PinX", "0").cell("PinY", "0").cell("Width", "2").cell("Height", "1").cell("LocPinX", "0").cell("LocPinY", "0");
    for (ix, (kind, x, y)) in
        [("MoveTo", "0", "0"), ("LineTo", "2", "0"), ("LineTo", "2", "1"), ("LineTo", "0", "1"), ("LineTo", "0", "0")].into_iter().enumerate()
    {
        let row = RowKey::Ix(u32::try_from(ix + 1).unwrap());
        shape = shape.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", x).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            y,
        );
    }
    let at = |name: &str| CellAddr::in_row("Connection", None, RowKey::Ix(0), name);
    shape = shape
        .set(at("X"), unsaved("1+0"))
        .set(at("Y"), unsaved("0+0"))
        .set(at("DirX"), Cell::constant(diacraft_doc::Value::from_text("0")))
        .set(at("DirY"), Cell::constant(diacraft_doc::Value::from_text("0")))
        .set(at("Type"), unsaved("0+0"));
    let mut s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build());
    let answer = run(&mut s, "shape.connection_edges", json!({"id": 1}))["shapes"][0].clone();
    let on: Vec<Vec<u64>> =
        answer["edges"].as_array().unwrap().iter().map(|e| e["points"].as_array().unwrap().iter().filter_map(|p| p.as_u64()).collect()).collect();
    assert_eq!(on, vec![vec![0], vec![], vec![], vec![]], "the point is on the bottom edge and no other: {answer}");
}

// ---- the rows a field is chosen from and the page a background's field shows ------------------------------------------------------------------------

#[test]
fn a_shape_data_field_whose_type_and_picture_are_a_formula_and_no_saved_value_shows_through_them() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    edit_shape(&mut s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let cost = |cell: &str| CellAddr::in_row("Property", None, RowKey::name("Cost"), cell);
        sheet.set_cell(&cost("Value"), Cell::constant(diacraft_doc::Value::from_text("12.5"))).unwrap();
        sheet.set_cell(&cost("Type"), unsaved("1+1")).unwrap();
        sheet.set_cell(&cost("Format"), unsaved("\"0.00\"")).unwrap();
    });
    let made = run(&mut s, "text.insert_field", json!({"id": id, "category": "shape_data", "field": "Cost"}));
    assert_eq!(made["shown"], "12.50", "the picture and the type the row's formulas give: {made}");
}

#[test]
fn a_field_on_a_background_page_whose_value_is_a_formula_and_no_saved_value_keeps_the_text_it_was_saved_with() {
    // A background page's field that reads the page shows the number of the page that uses it, which the background's own answer is not: the refresh leaves its text, whether the
    // formula is saved with a value or not.
    let field = |cell: &str| CellAddr::in_row("Field", None, RowKey::Ix(0), cell);
    let shape = ShapeBuilder::new(1)
        .cell("PinX", "3")
        .cell("PinY", "3")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .text_parts(vec![diacraft_doc::TextPart::Fld { ix: 0, text: Arc::from("7") }])
        .set(field("Value"), unsaved("PAGENUMBER()"))
        .set(field("Format"), Cell::constant(diacraft_doc::Value::from_text("")));
    let mut s = session_with(DocBuilder::new().page(1, PageBuilder::new().background().shape(shape)).build());
    let updated = run(&mut s, "text.update_fields", json!({}));
    assert_eq!(updated["updated"], 0, "the text of a background page's field is left: {updated}");
}
