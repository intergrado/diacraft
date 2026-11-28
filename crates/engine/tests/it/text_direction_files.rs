//! Vertical text in the bytes of a VSDX file. A text sample that the program wrote with `TextDirection` 1 read as 0 in the reference application, so these
//! tests read the bytes each writer gives for `text.format` with `vertical` and say what the specification says of the cell:
//!
//! * `TextDirection` is a cell of the **Text Block Format** group, which is no section: it stands **directly under the shape**, among the shape's other cells and before its
//!   sections and its text;
//! * it is a `vScalar` holding a whole number with **no unit**, and the shape's own cell is local, so it wins over the style and the master by the local
//!   rule: the cell holds **`V` and no `F`**, which is a local constant, and the root style's own cell (0)
//!   and the master's are left as they are;
//! * the other cells of the group that a command writes (`VerticalAlign`, the only one) are written the same way.
//!
//! Each of the writers is read: a new drawing, a drawing opened from `.diacraft` (the model writer), a drawing opened from an `.odg` and saved as a VSDX file, and a VSDX file
//! saved over itself (the splice writer), for a shape that has no master (a text box) and for an instance of a master. Every input is built in code.

use diacraft_engine::Session;
use diacraft_opc::xml::{self, El, Node};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::odg_build::{Dir, Drawing, open_drawing, rect};
use crate::{blank_session, run};

/// How the part that holds the first page of a VSDX package ends.
const PAGE_PART: &str = "/pages/page1.xml";

/// How the part that holds the styles and the document's own sheet ends.
const DOCUMENT_PART: &str = "/document.xml";

/// How the part that holds the first master ends.
const MASTER_PART: &str = "/masters/master1.xml";

/// The part of the package `bytes` whose name ends in `suffix`, as a tree.
fn part(bytes: &[u8], suffix: &str) -> El {
    let package = Package::read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    let name = package.parts().find(|name| name.ends_with(suffix)).unwrap_or_else(|| panic!("no part ends in {suffix}")).to_owned();
    xml::parse(package.part(&name).unwrap_or_else(|| panic!("no part {name}"))).unwrap_or_else(|e| panic!("{e}"))
}

/// The shape `id` of the first page of the package `bytes`, as a tree (a top-level shape).
fn shape_of(bytes: &[u8], id: u32) -> El {
    let page = part(bytes, PAGE_PART);
    let shapes = page.child("Shapes").expect("a Shapes element");
    shapes.children_named("Shape").find(|s| s.attr("ID") == Some(&id.to_string())).unwrap_or_else(|| panic!("no shape {id} on the page")).clone()
}

/// The cells directly under `shape` called `name`, each with its place among the shape's children.
fn own_cells<'a>(shape: &'a El, name: &str) -> Vec<(usize, &'a El)> {
    shape
        .children
        .iter()
        .enumerate()
        .filter_map(|(at, node)| match node {
            Node::El(cell) if cell.is("Cell") && cell.attr("N") == Some(name) => Some((at, cell)),
            _ => None,
        })
        .collect()
}

/// Asserts that `shape` holds the cell `name` once, directly under it, before its sections and its text, as `V` `value` and nothing else: no formula, no unit, no error.
#[track_caller]
fn assert_own_cell(shape: &El, name: &str, value: &str) {
    let found = own_cells(shape, name);
    assert_eq!(found.len(), 1, "{name}: one cell directly under the shape, {shape:?}");
    let (at, cell) = found[0];
    assert_eq!(cell.attr("V"), Some(value), "{name}: the shape's own value");
    for attr in ["F", "U", "E"] {
        assert_eq!(cell.attr(attr), None, "{name}: no `{attr}`: a whole number held as a plain value");
    }
    assert_eq!(cell.attrs.len(), 2, "{name}: `N` and `V` and nothing else: {cell:?}");
    let late = shape.children.iter().position(|node| matches!(node, Node::El(e) if e.is("Section") || e.is("Text")));
    assert!(
        late.is_none_or(|late| at < late),
        "{name}: the cell comes before the shape's sections and text (cell {at}, first section or text {late:?})"
    );
}

/// The value of the cell `name` directly under style 0 of the package `bytes`, when the root holds it.
fn root_value(bytes: &[u8], name: &str) -> Option<String> {
    let document = part(bytes, DOCUMENT_PART);
    let styles = document.child("StyleSheets")?;
    let root = styles.children_named("StyleSheet").find(|s| s.attr("ID") == Some("0"))?;
    root.children_named("Cell").find(|c| c.attr("N") == Some(name)).and_then(|c| c.attr("V")).map(str::to_owned)
}

/// What the commands set in a shape, as the window shows it.
fn seen(s: &mut Session, id: u32) -> Value {
    let answer = run(s, "shape.formatting", json!({ "ids": [id] }));
    let one = &answer["shapes"][0];
    json!({ "vertical": one["vertical"], "valign": one["valign"] })
}

/// The ids of the shapes of the first page, in order.
fn ids(s: &Session) -> Vec<u32> {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

fn vertical(s: &mut Session, id: u32, on: bool) {
    run(s, "text.format", json!({ "ids": [id], "vertical": on, "valign": "top" }));
}

/// A new drawing of a text box (no master) with text, and an instance of a master, each given vertical text: → (the session, the box, the instance).
fn drawn() -> (Session, u32, u32) {
    let mut s = blank_session();
    let text_box = run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 6, "text": "Hello"}))["id"].as_u64().unwrap() as u32;
    let instance = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({ "id": instance, "text": "hello world" }));
    vertical(&mut s, text_box, true);
    vertical(&mut s, instance, true);
    (s, text_box, instance)
}

/// What a VSDX file must hold for the two shapes: the text box has no master, the instance has one, each holds `TextDirection` 1 and `VerticalAlign` 0 of its own.
#[track_caller]
fn assert_vertical_shapes(bytes: &[u8], text_box: u32, instance: u32) {
    let boxed = shape_of(bytes, text_box);
    assert_eq!(boxed.attr("Master"), None, "the text box has no master");
    assert_own_cell(&boxed, "TextDirection", "1");
    assert_own_cell(&boxed, "VerticalAlign", "0");
    let made = shape_of(bytes, instance);
    assert!(made.attr("Master").is_some(), "the shape is an instance of a master");
    assert_own_cell(&made, "TextDirection", "1");
    assert_own_cell(&made, "VerticalAlign", "0");
    // The root style, which both reach, says across, and the master the instance has says nothing of it: the shapes' own cells are what makes them vertical.
    if let Some(root) = root_value(bytes, "TextDirection") {
        assert_eq!(root, "0", "style 0 is left as it was");
    }
    let master = part(bytes, MASTER_PART);
    let master_shape = master.child("Shapes").and_then(|s| s.child("Shape")).expect("the master's shape");
    assert!(own_cells(master_shape, "TextDirection").is_empty(), "the master holds no TextDirection");
}

#[test]
fn a_new_drawing_is_written_with_the_shapes_own_text_direction_directly_under_each_shape() {
    let dir = Dir::new();
    let (mut s, text_box, instance) = drawn();
    run(&mut s, "file.save_as", json!({ "path": dir.path("new.vsdx") }));
    assert_vertical_shapes(&dir.read("new.vsdx"), text_box, instance);
}

#[test]
fn a_drawing_opened_from_diacraft_and_saved_as_a_vsdx_file_is_written_the_same_way() {
    // The model writer: the drawing came from no VSDX file.
    let dir = Dir::new();
    let (mut s, text_box, instance) = drawn();
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.diacraft") }));
    assert_eq!(ids(&again), [text_box, instance]);
    assert_eq!(seen(&mut again, text_box), json!({ "vertical": true, "valign": "top" }));
    run(&mut again, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_vertical_shapes(&dir.read("b.vsdx"), text_box, instance);
}

#[test]
fn a_drawing_opened_from_an_odg_and_given_vertical_text_is_written_as_a_vsdx_file_the_same_way() {
    let dir = Dir::new();
    let drawing = Drawing::new().page(&format!("{}{}", rect("A", (2.0, 3.0, 4.0, 2.0), "one"), rect("B", (8.0, 3.0, 4.0, 2.0), "two")));
    let mut s = open_drawing(&dir, &drawing);
    let shapes = ids(&s);
    assert_eq!(shapes.len(), 2);
    for id in &shapes {
        vertical(&mut s, *id, true);
    }
    run(&mut s, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    let bytes = dir.read("c.vsdx");
    for id in shapes {
        let one = shape_of(&bytes, id);
        assert_own_cell(&one, "TextDirection", "1");
        assert_own_cell(&one, "VerticalAlign", "0");
    }
}

#[test]
fn a_vsdx_file_saved_over_itself_puts_the_cell_among_the_shapes_cells_and_not_after_a_section_or_the_text() {
    // The splice writer: a file with a shape of its own cells (shape 1) and an instance of a master (shape 2), both with sections and text.
    let dir = Dir::new();
    let path = dir.write("in.vsdx", &foreign());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    for id in [1, 2] {
        vertical(&mut s, id, true);
    }
    run(&mut s, "file.save", json!({}));
    let bytes = dir.read("in.vsdx");
    for id in [1, 2] {
        let one = shape_of(&bytes, id);
        assert_own_cell(&one, "TextDirection", "1");
        assert_own_cell(&one, "VerticalAlign", "0");
        // The shape's other cells are as they were.
        assert_own_cell_value(&one, "PinX", if id == 1 { "2" } else { "8" });
    }
    // A shape with the cell already, edited back to across and again to down, holds one cell of its own each time.
    for on in [false, true] {
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": path }));
        vertical(&mut s, 1, on);
        run(&mut s, "file.save", json!({}));
        assert_own_cell(&shape_of(&dir.read("in.vsdx"), 1), "TextDirection", if on { "1" } else { "0" });
    }
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(seen(&mut s, 1), json!({ "vertical": true, "valign": "top" }));
}

#[track_caller]
fn assert_own_cell_value(shape: &El, name: &str, value: &str) {
    let found = own_cells(shape, name);
    assert_eq!(found.len(), 1, "{name}");
    assert_eq!(found[0].1.attr("V"), Some(value), "{name}");
}

/// A package a program other than this one could have made: a shape of its own with sections and text, and an instance of a master that has the same.
fn foreign() -> Vec<u8> {
    let outline = r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#;
    let master = shape(
        4,
        &format!(
            r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>{outline}"#
        ),
    );
    let own =
        shape(1, &format!(r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>{outline}<Text>box</Text>"#));
    let instance = shape_with(2, r#" Master="4""#, r#"<Cell N="PinX" V="8"/><Cell N="PinY" V="2"/><Text>instance</Text>"#);
    VsdxBuilder::new().master(MasterSpec::new(4, "Blob").shapes(&master)).page(PageSpec::new(1, "Page-1").shapes(&[own, instance].concat())).build()
}

#[test]
fn the_text_direction_of_every_writer_reads_back_from_the_formats_the_program_saves() {
    let dir = Dir::new();
    for name in ["out.diacraft", "out.vsdx", "out.odg"] {
        let (mut s, text_box, instance) = drawn();
        run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": dir.path(name) }));
        assert_eq!(ids(&again), [text_box, instance], "{name}");
        for id in [text_box, instance] {
            assert_eq!(seen(&mut again, id), json!({ "vertical": true, "valign": "top" }), "{name}: shape {id}");
        }
    }
}
