//! Ruling on F40: **the root style of a new drawing holds `DropOnPageScale` 1**, as the root style of every file of the corpus does
//! ("Default. Root 1 in every file of the 1,906"), so that a shape no drop scaled reads 1 and a master's formula that names the
//! cell is the master's own size. Where the root comes from a file it is the file's: a root that lacks the cell is kept lacking it, one that
//! holds 1 is kept as it is, and one that holds another number is kept with that.
//!
//! * A drawing DiaCraft made (a new one, one from a template, one saved as `.diacraft` and read again) reads the factor as 1 where a master's formula names it: a connection point
//!   that is a length of the shape plus a length times the factor, worked out again when the shape is resized, is not short of the factor.
//! * A plain save of a drawing read from a file does not add the cell to the file's root: no edit, no change of the file's bytes. An edit saved into the file leaves the root alone.
//! * A drawing read from a file that lacks the cell and saved as `.diacraft` is a `.diacraft` drawing that lacks it, and a plain save of that is no edit.

use diacraft_doc::{Document, Owner, Resolver, SectionKey, SheetRef};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::native_common::{Dir, pinned};
use crate::stencil_open::{CT_STENCIL, LOOK, box_cells, ids, open};
use crate::{blank_session, run};

/// The parts of a VSDX file that hold the styles and the first page.
const STYLES_PART: &str = "/visio/document.xml"; // brand-ok: a part name the format defines
const PAGE_PART: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines

/// A root's factor: the formula text and the value text of the cell, `None` for a root with no such cell.
type RootFactor = Option<(Option<String>, Option<String>)>;

/// What the root style (`ID` 0) of the drawing holds of `DropOnPageScale`: the formula text and the value text, `None` for a root with no such cell.
fn root_factor(d: &Document) -> RootFactor {
    let root = d.styles.iter().find(|s| s.id == 0)?;
    let cell = root.sheet.cell("DropOnPageScale")?;
    Some((cell.formula.text().map(str::to_owned), cell.value.as_ref().map(|v| v.text().to_owned())))
}

/// The bytes of one part of a package (an OPC package: a VSDX file, or a `.diacraft` file).
fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    package.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()
}

/// The number a formula works out to on the shape `id` of page 1.
fn evaluate(s: &mut Session, id: u32, formula: &str) -> f64 {
    run(s, "sheet.evaluate", json!({"id": id, "page": 1, "formula": formula}))["value"].as_f64().unwrap_or_else(|| panic!("`{formula}` is no number"))
}

#[track_caller]
fn assert_near(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= 1.0e-9, "{what}: got {got}, want {want}");
}

/// A stencil of one master, "Probe": a box 2 by 1 whose connection point is `Height*0.1` along x plus 0.11 inch times the factor, as the masters of real stencils name it (a
/// fixed length that scales with the shape).
fn probe_stencil() -> Vec<u8> {
    let cells = box_cells(2.0, 1.0);
    let cells = cells.split("<Section N=\"Connection\">").next().unwrap_or_default().to_owned();
    let point = r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="0.21" F="Height*0.1+0.11 in*DropOnPageScale"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row></Section>"#;
    let master = MasterSpec::new(1, "Probe").attrs(&ids(101, 201)).shapes(&shape(5, &format!("{LOOK}{cells}{point}")));
    VsdxBuilder::new().document_type(CT_STENCIL).master(master).page(PageSpec::new(1, "Page-1").without_part()).build()
}

/// The connection point's `X` of the shape `id` of page 1, as the shape has it now.
fn connection_x(d: &Document, id: u32) -> f64 {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let rows = Resolver::new(d).effective_rows(&SheetRef::Shape(at), &SectionKey::new("Connection", None)).unwrap();
    rows[0].cells.iter().find(|(n, _)| n.as_str() == "X").and_then(|(_, c)| c.value.cell?.value.as_ref()?.number()).unwrap()
}

/// A drop of the probe within the range (the page and the master are at 1:1, so nothing is written but the pin), then the height typed: **the formula that names the factor is
/// worked out again, and the factor it reads is 1**. In a drawing whose root has no such cell it read 0, and the point lost its 0.11.
fn probe_in(mut s: Session, dir: &Dir) -> (Session, u32) {
    open(&mut s, &dir.write("probe.vssx", &probe_stencil()));
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "Probe", "stencil": "file-1", "page": 1, "x": 5.0, "y": 4.0})));
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Height", "formula": "2 in"}));
    (s, id)
}

#[test]
fn a_new_drawings_master_formula_that_names_the_factor_works_out_to_1_and_not_0() {
    let dir = Dir::new();
    let (mut s, id) = probe_in(blank_session(), &dir);
    assert_near("the connection point: a fifth of the height and 0.11 inch times the factor", connection_x(doc(&s), id), 0.2 + 0.11);
    assert_near("a formula that names the factor", evaluate(&mut s, id, "DropOnPageScale*3"), 3.0);
    assert_near("and a length of it", evaluate(&mut s, id, "0.11 in*DropOnPageScale"), 0.11);
    assert_eq!(root_factor(doc(&s)), Some((None, Some("1".to_owned()))), "the root of a new drawing holds the factor 1, as a plain value");
}

/// A drawing from each built-in template has the root a new drawing has (`file.new` makes them all from the same blank), and a `.diacraft` file keeps it: saved and read again
/// the root holds the factor and a formula reads it.
#[test]
fn a_drawing_from_a_template_and_a_diacraft_file_read_the_factor_as_1() {
    let dir = Dir::new();
    for template in [None, Some("basic_diagram"), Some("basic_flowchart")] {
        let mut s = Session::new();
        match template {
            None => run(&mut s, "file.new", json!({})),
            Some(t) => run(&mut s, "file.new", json!({ "template": t })),
        };
        assert_eq!(root_factor(doc(&s)), Some((None, Some("1".to_owned()))), "{template:?}");
        let name = format!("{}.diacraft", template.unwrap_or("blank"));
        run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": dir.path(&name) }));
        assert_eq!(root_factor(doc(&t)), Some((None, Some("1".to_owned()))), "{template:?}: read again");
        let id = id_of(&run(&mut t, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0})));
        assert_near(&format!("{template:?}: a formula that names the factor"), evaluate(&mut t, id, "DropOnPageScale*3"), 3.0);
    }
}

// ---- a file's root is the file's ------------------------------------------------------------------------------------------------------

/// The root style of a drawing's file: the three flags and, for `factor`, the cell as it is written.
fn root(factor: Option<&str>) -> String {
    format!(
        r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/>{}</StyleSheet>"#,
        factor.unwrap_or_default()
    )
}

/// A VSDX drawing of one box on a page at 1:1, with this root.
fn drawing(root: &str) -> Vec<u8> {
    let page = r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let rect = r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    VsdxBuilder::new().styles(root).page(PageSpec::new(1, "Page-1").sheet(page).shapes(&shape(1, rect))).build()
}

/// The three roots a file may have, with what the drawing read from it holds of the factor.
fn roots() -> Vec<(&'static str, String, RootFactor)> {
    let value = |v: &str| Some((None, Some(v.to_owned())));
    vec![
        ("a root with no factor", root(None), None),
        ("a root that holds 1", root(Some(r#"<Cell N="DropOnPageScale" V="1"/>"#)), value("1")),
        ("a root that holds 12", root(Some(r#"<Cell N="DropOnPageScale" V="12"/>"#)), value("12")),
    ]
}

/// A plain save of a VSDX drawing, read from a file, leaves the part that holds the styles as it was: the cell is not added to a root that lacks it, and a root that holds it is kept
/// whatever it holds. An edit saved into the file changes the page and leaves the styles alone.
#[test]
fn a_plain_save_and_an_edit_of_a_vsdx_file_leave_its_root_as_it_is() {
    let dir = Dir::new();
    for (what, root, want) in roots() {
        let path = dir.write("drawing.vsdx", &drawing(&root));
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": path }));
        assert_eq!(root_factor(doc(&s)), want, "{what}: as the file has it");
        let styles = part(&dir.read("drawing.vsdx"), STYLES_PART);
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(saved["dirty"], Value::Bool(false), "{what}: {saved}");
        assert_eq!(part(&dir.read("drawing.vsdx"), STYLES_PART), styles, "{what}: the part that holds the styles is as it was");
        assert_eq!(root_factor(doc(&s)), want, "{what}: after the save");

        let page = part(&dir.read("drawing.vsdx"), PAGE_PART);
        run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        run(&mut s, "file.save", json!({}));
        assert_ne!(part(&dir.read("drawing.vsdx"), PAGE_PART), page, "{what}: a move is saved into the page");
        assert_eq!(part(&dir.read("drawing.vsdx"), STYLES_PART), styles, "{what}: and the styles are still as they were");
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": path }));
        assert_eq!(root_factor(doc(&t)), want, "{what}: the root of the file the edit was saved into");
    }
}

/// A VSDX drawing saved as `.diacraft` is a `.diacraft` drawing with the root it had: no factor where it had none, and what it held where it held a number; read again it is the
/// same, and a plain save of it leaves the file's bytes.
#[test]
fn a_vsdx_drawing_saved_as_diacraft_keeps_its_root_and_a_plain_save_of_that_leaves_it() {
    let dir = Dir::new();
    for (what, root, want) in roots() {
        // The clock is pinned: a `.diacraft` file holds the time it was saved.
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("drawing.vsdx", &drawing(&root)) }));
        let out = dir.path("out.diacraft");
        run(&mut s, "file.save_as", json!({ "path": out }));
        let mut t = pinned();
        run(&mut t, "file.open", json!({ "path": out }));
        assert_eq!(root_factor(doc(&t)), want, "{what}: the .diacraft file's root");
        let before = dir.read("out.diacraft");
        let saved = run(&mut t, "file.save", json!({}));
        assert_eq!(saved["dirty"], Value::Bool(false), "{what}: {saved}");
        assert_eq!(dir.read("out.diacraft"), before, "{what}: the file's bytes are as they were");
        assert_eq!(root_factor(doc(&t)), want, "{what}: after the save");
    }
}

/// An `.odg` file has no root style: the reader makes the drawing's root from a new drawing's, so a drawing read from one reads the factor as 1 too, as the rest of that root is. A
/// plain save of it is no edit and leaves the file's bytes; an edit saved into it writes the shape and not the factor, and the file read again has the same root.
#[test]
fn an_odg_drawing_has_the_root_a_new_drawing_has_and_neither_a_save_nor_an_edit_writes_the_factor_into_the_file() {
    use crate::odg_build::{Drawing, rect};
    let dir = Dir::new();
    let bytes = Drawing::new().page(&rect("R", (2.0, 8.0, 4.0, 2.0), "")).bytes();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("drawing.odg", &bytes) }));
    assert_eq!(root_factor(doc(&s)), Some((None, Some("1".to_owned()))), "the root a reader makes of an `.odg` is a new drawing's");
    let before = dir.read("drawing.odg");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], Value::Bool(false), "a plain save is no edit: {saved}");
    assert_eq!(dir.read("drawing.odg"), before, "and leaves the file's bytes");
    let id = doc(&s).page(0).or_else(|| doc(&s).page(1)).unwrap().shapes.iter().next().unwrap().id;
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], Value::Bool(true), "a move is an edit: {saved}");
    assert!(!odg_names_the_factor(&before) && !odg_names_the_factor(&dir.read("drawing.odg")), "the factor is not written into the file");
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": path_of(&dir, "drawing.odg") }));
    assert_eq!(root_factor(doc(&t)), Some((None, Some("1".to_owned()))), "the file read again");
}

fn path_of(dir: &Dir, name: &str) -> String {
    dir.path(name)
}

/// Whether any part of the `.odg` package says the name of the factor, in any of the parts the format and DiaCraft's own data have.
fn odg_names_the_factor(bytes: &[u8]) -> bool {
    let package = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    package.parts().any(|name| package.part(name).is_some_and(|part| part.windows(14).any(|w| w == b"DropOnPageScale")))
}
