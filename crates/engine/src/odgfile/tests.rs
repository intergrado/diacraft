//! What the flattening of the shapes an `.odg` save hands the writer does, and what it costs. The drawings are
//! written by hand from the ODF standard and put together by the container's writer; the shapes are made by the engine's own commands.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Cell, CellName, Document, Master, Row, Section, Shape, Value};
use serde_json::json;

use super::flatten::{
    Flat, Flattened, MAX_ROWS, MAX_STEPS, OBJECT_NOT_IN_FILE, Work, constant_colour, flatten, flatten_counted, is_object, without_ids,
};
use crate::Session;

const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
);

/// A drawing of one A4 page holding `n` rectangles in a row of rows, each on the style `gr1`.
fn drawing(n: usize) -> Vec<u8> {
    drawing_of(&rectangles(n))
}

/// `n` rectangles in rows, each on the style `gr1`.
fn rectangles(n: usize) -> String {
    (0..n)
        .map(|k| {
            format!(
                "<draw:custom-shape draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"S{k}\" svg:width=\"1cm\" svg:height=\"1cm\" svg:x=\"{}cm\" svg:y=\"{}cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>",
                k % 15,
                k / 15 % 25
            )
        })
        .collect()
}

/// A drawing of one A4 page holding `shapes`, which may name the style `gr1` (a blue fill and a black line).
fn drawing_of(shapes: &str) -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"gr1\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#3465a4\" draw:stroke=\"solid\" svg:stroke-color=\"#000000\"/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{shapes}</draw:page></office:drawing></office:body></office:document-content>"
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// A session that has opened a drawing of `n` rectangles from a file of its own, and the folder to take away.
fn opened(n: usize) -> (Session, PathBuf) {
    opened_with(&drawing(n))
}

/// A session that has opened the drawing `bytes` from a file of its own, and the folder to take away.
fn opened_with(bytes: &[u8]) -> (Session, PathBuf) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("diacraft-engine-flatten-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.odg");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap();
    (s, dir)
}

/// The flattening of what the session holds now, with its steps counted, and the result.
fn flat_of(s: &Session, before: &Flattened) -> (Result<Flat, String>, u64) {
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let flat = flatten_counted(doc, state.index(), before, &mut work);
    (flat, work.steps)
}

fn drop_rectangles(s: &mut Session, n: usize) {
    for k in 0..n {
        s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 1.0 + (k % 10) as f64, "y": 1.0 + (k / 10 % 8) as f64 })).unwrap();
    }
}

/// A drawing with no new shape is not flattened, and what the save looks at costs a step for the visit of each shape and one for the look at its colours.
#[test]
fn a_drawing_with_nothing_new_is_given_as_it_is_and_costs_two_steps_for_each_shape() {
    let mut steps = Vec::new();
    for n in [100, 400, 1600] {
        let (s, dir) = opened(n);
        let (flat, taken) = flat_of(&s, &Flattened::default());
        let flat = flat.unwrap();
        assert!(flat.doc.is_none() && flat.made.len() == 0 && flat.notes.is_empty(), "{n}: nothing to give but the model itself");
        assert_eq!((flat.visited, taken), (n as u64, 2 * n as u64), "{n}");
        steps.push(taken);
        let _ = std::fs::remove_dir_all(dir);
    }
    assert_eq!((steps[1], steps[2]), (4 * steps[0], 4 * steps[1]), "linear in the shapes");
}

/// The new shapes each cost the same: the steps follow their number (the cells each resolves to and the rows of its sections), four times as many shapes
/// cost four times the steps, and a model that had thousands of shapes before costs one step more for each and no more.
#[test]
fn new_shapes_cost_steps_that_follow_their_number() {
    let mut taken = Vec::new();
    for n in [50, 200, 800] {
        let (mut s, dir) = opened(2);
        drop_rectangles(&mut s, n);
        let (flat, steps) = flat_of(&s, &Flattened::default());
        let flat = flat.unwrap();
        assert_eq!((flat.flattened, flat.made.len()), (n as u64, n), "{n}");
        taken.push(steps);
        let _ = std::fs::remove_dir_all(dir);
    }
    let (one, two) = (taken[1] - taken[0], taken[2] - taken[1]);
    assert_eq!(one * 4, two, "each added shape costs the same: {taken:?}");
    assert_eq!(
        one,
        150 * 82,
        "a rectangle is 82 steps: its visit, its cells, the sizes its pin is told from, the rows of its sections and their cells, the audit of what it holds (its master's cells are kept by the link and are not audited), and the look at its master for the text and the picture it shows"
    );
    // Many shapes in the file that are not new add a step or two each, and the new ones cost what they cost.
    let (mut s, dir) = opened(1500);
    drop_rectangles(&mut s, 50);
    let (_, steps) = flat_of(&s, &Flattened::default());
    assert_eq!(steps, 2 * 1500 + taken[0] - 2 * 2, "{steps}: the shapes of the file cost two each, and the 50 new ones what they cost alone");
    let _ = std::fs::remove_dir_all(dir);
}

/// A flattening past its cap is refused in words, and what it did before the cap is no more than the cap and the section that crossed it.
#[test]
fn a_flattening_past_the_cap_is_refused_in_words() {
    let (mut s, dir) = opened(2);
    drop_rectangles(&mut s, 400);
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(5_000, u64::MAX);
    let refused = flatten_counted(doc, state.index(), &Flattened::default(), &mut work).expect_err("refused");
    assert!(refused.contains("too many to write") && refused.contains("undo some of them to save") && refused.contains("5000"), "{refused}");
    assert!(work.steps > 5_000 && work.steps < 5_000 + 200, "stopped at the cap and not much after: {}", work.steps);
    let _ = std::fs::remove_dir_all(dir);
}

/// A master with a long outline: the instances of it are flattened with its rows. A section of more rows than a shape may have is refused in words, one of
/// as many is not, and what a flattening of many instances of a long master costs is held to the budget **before** the rows are merged.
#[test]
fn a_long_outline_is_refused_by_its_rows_and_many_of_them_by_the_budget() {
    let make = |rows: usize, instances: usize| {
        let (mut s, dir) = opened(1);
        s.edit("Test edit", |doc: &mut Document| {
            let mut sheet = diacraft_doc::Sheet::default();
            for (name, v) in [("Width", "1"), ("Height", "1"), ("PinX", "0.5"), ("PinY", "0.5"), ("LocPinX", "0.5"), ("LocPinY", "0.5")] {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(v)).with_unit("IN"));
            }
            let mut geometry = Section::new("Geometry", Some(0));
            for i in 0..rows {
                let mut row = Row {
                    row_type: Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" })),
                    ix: Some(u32::try_from(i + 1).unwrap()),
                    ..Row::default()
                };
                row.cells.insert(CellName::new("X"), Cell::with_formula("Width*0.5", Some(Value::from_text("0.5"))));
                row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_text("0")));
                geometry.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(geometry));
            let master = Master {
                id: 70,
                shapes: diacraft_doc::ShapeList::from_vec(vec![Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(1) })]),
                ..Master::default()
            };
            doc.add_master(master).unwrap();
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            for k in 0..instances {
                let mut pin = diacraft_doc::Sheet::default();
                pin.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text("2")).with_unit("IN"));
                pin.cells.insert(CellName::new("PinY"), Cell::constant(Value::from_text("2")).with_unit("IN"));
                page.shapes.push(Arc::new(Shape { master: Some(70), sheet: Arc::new(pin), ..Shape::new(1000 + u32::try_from(k).unwrap()) }));
            }
            Ok(())
        })
        .unwrap();
        (s, dir)
    };
    // As many rows as a shape may have: flattened, each row a step and each cell of it.
    let (s, dir) = make(MAX_ROWS, 1);
    let (flat, steps) = flat_of(&s, &Flattened::default());
    assert_eq!(flat.unwrap().flattened, 1);
    assert!(steps > MAX_ROWS as u64 * 3, "{steps}");
    let _ = std::fs::remove_dir_all(dir);
    // One row more: refused in words.
    let (s, dir) = make(MAX_ROWS + 1, 1);
    let refused = flat_of(&s, &Flattened::default()).0.expect_err("refused");
    assert!(refused.contains("section of 20001 rows") && refused.contains("at most 20000"), "{refused}");
    let _ = std::fs::remove_dir_all(dir);
    // Many instances of a master of 2,000 rows: the budget (4,000,000) holds at about 650 of them, and the work stops there: at most one section's merge
    // past it, and not the 3,000 instances' worth.
    let (s, dir) = make(2_000, 3_000);
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let started = std::time::Instant::now();
    let refused = flatten_counted(doc, state.index(), &Flattened::default(), &mut work).expect_err("refused by the budget");
    assert!(refused.contains("too many to write"), "{refused}");
    assert!(work.steps <= MAX_STEPS + 2_000 + 20, "the rows of a section are paid for before they are merged: {} steps", work.steps);
    assert!(started.elapsed() < std::time::Duration::from_secs(60), "the refusal took {:?}", started.elapsed());
    let _ = std::fs::remove_dir_all(dir);
}

/// Each shape is visited once whatever its depth: a chain of groups of the file 150 deep holding a shape whose colour is `RGB(r,g,b)` is one step for the visit
/// of the group and one for each of the 151 shapes in it.
#[test]
fn a_chain_of_groups_is_walked_once() {
    let depth = 150;
    let xml = format!("{}{}{}", "<draw:g>".repeat(depth), rectangles(1), "</draw:g>".repeat(depth));
    let (mut s, dir) = opened_with(&drawing_of(&xml));
    // The colour of the shape at the bottom is chosen by a person: `RGB(0,255,0)` with the value `#00ff00`.
    s.edit("Test edit", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut leaf: &mut Shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        for _ in 0..depth {
            leaf = Arc::make_mut(leaf.children.get_mut(0).unwrap());
        }
        Arc::make_mut(&mut leaf.sheet)
            .cells
            .insert(CellName::new("FillForegnd"), Cell::with_formula("RGB(0,255,0)", Some(Value::from_text("#00ff00"))));
        Ok(())
    })
    .unwrap();
    let (flat, steps) = flat_of(&s, &Flattened::default());
    let flat = flat.unwrap();
    let copy = flat.doc.expect("the colour is made a constant in a copy");
    assert_eq!((flat.visited, steps), (1, 1 + 151), "one visit of the group and one step for each shape in the chain");
    let leaf_of = |doc: &Document| {
        let mut shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        while let Some(child) = shape.children.get(0).cloned() {
            shape = child;
        }
        shape
    };
    let fill = leaf_of(&copy).sheet.cell("FillForegnd").unwrap().clone();
    assert!(
        fill.formula.text().is_none() && fill.value.as_ref().map(Value::text) == Some("#00ff00"),
        "the formula is gone and the colour is kept: {fill:?}"
    );
    assert_eq!(
        leaf_of(s.document().unwrap()).sheet.cell("FillForegnd").unwrap().formula.text(),
        Some("RGB(0,255,0)"),
        "the model's own shape is as it was"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The colours a person chooses are `RGB(r,g,b)` of the value the cell shows; any other formula, and an `RGB` that is not of its value, is left as it is
/// (the writer refuses it in words).
#[test]
fn only_an_rgb_formula_that_is_its_own_value_is_a_constant() {
    let cell = |f: &str, v: &str| Cell::with_formula(f, Some(Value::from_text(v)));
    let made = constant_colour(&cell("RGB(0,255,0)", "#00ff00")).unwrap();
    assert!(made.formula.text().is_none() && made.value.as_ref().unwrap().text() == "#00ff00");
    assert!(constant_colour(&cell("RGB( 18 , 52 , 86 )", "#123456")).is_some(), "spaces");
    assert!(constant_colour(&cell("RGB(0,255,0)", "#00FF00")).is_some(), "the case of the value");
    assert!(constant_colour(&cell("RGB(0,255,0)", "#00ff01")).is_none(), "another colour");
    assert!(constant_colour(&cell("RGB(0,255,0)", "3")).is_none(), "a number");
    assert!(constant_colour(&cell("RGB(0,255,0,1)", "#00ff00")).is_none(), "four");
    assert!(constant_colour(&cell("RGB(0,255)", "#00ff00")).is_none(), "two");
    assert!(constant_colour(&cell("RGB(0,256,0)", "#00ff00")).is_none(), "past a byte");
    assert!(constant_colour(&cell("RGB(0,-1,0)", "#00ff00")).is_none(), "below");
    assert!(constant_colour(&cell("THEMEVAL(\"FillColor\")", "#00ff00")).is_none(), "a theme's formula");
    assert!(constant_colour(&cell("RGB(0,255,0)+1", "#00ff00")).is_none(), "more than the call");
    assert!(constant_colour(&Cell::constant(Value::from_text("#00ff00"))).is_none(), "a constant already: nothing to do");
    assert!(constant_colour(&Cell::inherited(Some(Value::from_text("#00ff00")))).is_none(), "an inherited formula");
}

/// What a flattened shape is: the cells it resolves to as constants, its sections merged with the master's, the links of the root style and the links to its
/// master; and the model's own shape is as it was.
#[test]
fn a_dropped_shape_is_flattened_to_its_resolved_cells_and_sections() {
    let (mut s, dir) = opened(1);
    let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let (flat, _) = flat_of(&s, &Flattened::default());
    let flat = flat.unwrap();
    assert!(flat.made.contains(0, id) && flat.made.len() == 1);
    let copy = flat.doc.unwrap();
    assert_eq!(copy.masters.len(), s.document().unwrap().masters.len(), "the master a drop made is the copy's too");
    assert!(!copy.masters.is_empty());
    let shape = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    let model_shape = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    assert!(
        shape.master.is_some() && (shape.master, shape.master_shape) == (model_shape.master, model_shape.master_shape) && shape.children.is_empty()
    );
    let links = shape.sheet.links;
    assert_eq!((links.line, links.fill, links.text), (Some(0), Some(0), Some(0)));
    let names: Vec<&str> = shape.sheet.cells.keys().map(CellName::as_str).collect();
    for wanted in ["PinX", "PinY", "LocPinX", "LocPinY", "Width", "Height", "FillPattern", "FillForegnd", "LinePattern", "LineWeight", "LineColor"] {
        assert!(names.contains(&wanted), "{wanted} is a cell of the flattened shape: {names:?}");
    }
    // The frame and the style are constants, but for the pin of a shape that turns about its centre, which is the formula the reader gives it: the writer
    // then finds it what the file gives back, and carries nothing of it in DiaCraft's extension.
    let formulas: Vec<(&str, &str)> = shape.sheet.cells.iter().filter_map(|(n, c)| c.formula.text().map(|f| (n.as_str(), f))).collect();
    assert_eq!(formulas, [("LocPinX", "Width*0.5"), ("LocPinY", "Height*0.5")], "the frame and the style are constants, but for the centred pin");
    assert!(shape.sheet.cell("TxtPinX").is_none() && shape.sheet.cell("ResizeMode").is_none(), "what the writer has no attribute for is not carried");
    let colour = shape.sheet.cell("FillForegnd").unwrap().value.as_ref().unwrap().text().to_owned();
    assert!(colour.len() == 7 && colour.starts_with('#'), "a colour resolved to its RGB: {colour}");
    let section = |name: &str| shape.sheet.sections.iter().find(|x| x.name.as_str() == name).unwrap().clone();
    let (geometry, connection) = (section("Geometry"), section("Connection"));
    assert_eq!((geometry.rows.len(), connection.rows.len()), (5, 4));
    let x = geometry.rows.get(1).unwrap().cells.get("X").unwrap();
    assert_eq!(x.formula.text(), Some("Width"), "the formula it resolves to, which the writer reads the shape's form by");
    assert!(geometry.cells.contains_key("NoFill"), "the flags of the section");
    // A rectangle is no connector: it has no route cell and holds no marker of one.
    assert!(
        shape.sheet.cell("ShapeRouteStyle").is_none() && shape.sheet.cell("ConLineRouteExt").is_none(),
        "no route cell on a 2-D shape: {names:?}"
    );
    assert!(shape.sheet.sections.iter().all(|x| matches!(x.name.as_str(), "Geometry" | "Connection")), "only its outline and its connection points");
    // The units are the cells' own: the frame in inches, and a cell of a row as the master's cell has it.
    for cell in ["Width", "Height", "PinX", "PinY", "LocPinX", "LocPinY"] {
        assert_eq!(shape.sheet.cell(cell).unwrap().unit.as_deref(), Some("IN"), "{cell}");
    }
    let master_x = {
        let master = s.document().unwrap().masters.get(0).unwrap().clone();
        let section = master.shapes.get(0).unwrap().sheet.sections.iter().find(|x| x.name.as_str() == "Geometry").unwrap().clone();
        section.rows.get(1).unwrap().cells.get("X").unwrap().unit.clone()
    };
    assert_eq!(master_x.as_deref(), Some("IN"), "the master's cell has a unit");
    assert_eq!(x.unit, master_x, "and so has the flattened one");
    // The shape in the model is the instance it was.
    let held = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    assert!(held.master.is_some() && held.sheet.cells.len() == 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// A 1-D shape of its own cells, from (1, 1) to (3, 1) inches, as a line drawn by hand has them.
fn one_d(id: u32) -> Shape {
    let mut sheet = diacraft_doc::Sheet::default();
    for (name, value) in [
        ("BeginX", "1"),
        ("BeginY", "1"),
        ("EndX", "3"),
        ("EndY", "1"),
        ("PinX", "2"),
        ("PinY", "1"),
        ("Width", "2"),
        ("Height", "0"),
        ("LocPinX", "1"),
    ] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
    }
    let mut geometry = Section::new("Geometry", Some(0));
    for (at, (kind, x)) in [("MoveTo", "0"), ("LineTo", "2")].into_iter().enumerate() {
        let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(u32::try_from(at + 1).unwrap()), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(x)).with_unit("IN"));
        row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_text("0")).with_unit("IN"));
        geometry.rows.push(Arc::new(row));
    }
    sheet.sections.push(Arc::new(geometry));
    Shape { sheet: Arc::new(sheet), ..Shape::new(id) }
}

/// A `User` section of rows with these names.
fn user_rows(names: &[&str]) -> Arc<Section> {
    let mut user = Section::new("User", None);
    for name in names {
        let mut row = Row { name: Some(Arc::from(*name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("1")));
        user.rows.push(Arc::new(row));
    }
    Arc::new(user)
}

/// Puts `shapes` on the first page of the session, in the model as a command would have, and `glued` of them (by `ID`) glued by their begin to the first shape.
fn put_shapes(s: &mut Session, shapes: Vec<Shape>, glued: &[u32]) {
    s.edit("Test edit", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let target = page.shapes.get(0).unwrap().id;
        let mut rows: Vec<diacraft_doc::Connect> = page.connects.iter().cloned().collect();
        for id in glued {
            rows.push(diacraft_doc::Connect {
                from_sheet: *id,
                from_cell: Some(Arc::from("BeginX")),
                from_part: None,
                to_sheet: target,
                to_cell: None,
                to_part: Some(100),
                raw: diacraft_doc::RawAttrs::new(),
                ext: Default::default(),
            });
        }
        page.connects = diacraft_doc::Connects::from_vec(rows);
        for shape in shapes {
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
}

/// What makes a 1-D shape a connector (and so gives it the route cells the writer reads one by): a `Connect` row from it, the marker row `DiaCraftRoute`, or a
/// route cell of its own. A line with a `User` section of other rows, and a 2-D shape with a route cell, are not.
#[test]
fn a_one_d_shape_is_a_connector_only_when_it_is_glued_marked_or_routed() {
    let (mut s, dir) = opened(1);
    let mut plain = one_d(901);
    let mut other_user = one_d(902);
    Arc::make_mut(&mut other_user.sheet).sections.push(user_rows(&["Other"]));
    let mut marked = one_d(903);
    Arc::make_mut(&mut marked.sheet).sections.push(user_rows(&["Other", "DiaCraftRoute", "More"]));
    let mut routed = one_d(904);
    Arc::make_mut(&mut routed.sheet).cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(Value::from_text("2")));
    let glued = one_d(905);
    let mut two_d = Shape { sheet: Arc::new(diacraft_doc::Sheet::default()), ..Shape::new(906) };
    for (name, value) in [("PinX", "5"), ("PinY", "5"), ("Width", "1"), ("Height", "1"), ("LocPinX", "0.5"), ("LocPinY", "0.5")] {
        Arc::make_mut(&mut two_d.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
    }
    Arc::make_mut(&mut two_d.sheet).cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(Value::from_text("2")));
    plain.raw = diacraft_doc::RawAttrs::new();
    put_shapes(&mut s, vec![plain, other_user, marked, routed, glued, two_d], &[905]);
    let copy = flat_of(&s, &Flattened::default()).0.unwrap().doc.expect("new shapes are flattened");
    let of = |id: u32| copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    let route = |shape: &Shape| {
        let number = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
        (number("ShapeRouteStyle"), number("ConLineRouteExt"))
    };
    let sections = |shape: &Shape| shape.sheet.sections.iter().map(|x| x.name.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(route(&of(901)), (None, None), "a line");
    assert_eq!(route(&of(902)), (None, None), "a line with a user row that is no marker is a line");
    assert_eq!(sections(&of(902)), ["Geometry", "User"], "and the writer is given its outline and its data (carries the data)");
    assert_eq!(route(&of(903)), (Some(1.0), Some(1.0)), "the marker makes a connector, drawn as a right-angle one");
    assert_eq!(sections(&of(903)), ["Geometry", "User"]);
    let user = of(903).sheet.sections.iter().find(|x| x.name.as_str() == "User").unwrap().clone();
    assert_eq!(
        user.rows.iter().map(|r| r.name.as_deref().unwrap().to_owned()).collect::<Vec<_>>(),
        ["Other", "DiaCraftRoute", "More"],
        "the marker and the rows beside it are carried"
    );
    assert_eq!(route(&of(904)), (Some(2.0), Some(1.0)), "a route cell of its own is kept, and the other is the right-angle style's");
    assert_eq!(route(&of(905)), (Some(1.0), Some(1.0)), "a glued one is a connector");
    assert_eq!(sections(&of(905)), ["Geometry"], "with no marker to carry");
    assert_eq!(route(&of(906)), (None, None), "a 2-D shape with a route cell is no connector");
    let _ = std::fs::remove_dir_all(dir);
}

/// A flag of an outline that the shape holds as an inherited cell with a value (`F="Inh"` and a `V`) is the value the shape reads, and is kept.
#[test]
fn a_section_flag_that_is_an_inherited_cell_with_a_value_is_kept() {
    let (mut s, dir) = opened(1);
    let mut line = one_d(901);
    {
        let sheet = Arc::make_mut(&mut line.sheet);
        let geometry = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
        geometry.cells.insert(CellName::new("NoFill"), Cell::inherited(Some(Value::from_text("1"))));
        geometry.cells.insert(CellName::new("NoLine"), Cell::constant(Value::from_text("0")));
    }
    put_shapes(&mut s, vec![line], &[]);
    let copy = flat_of(&s, &Flattened::default()).0.unwrap().doc.expect("flattened");
    let shape = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == 901).unwrap().clone();
    let geometry = shape.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry").unwrap().clone();
    let value = |name: &str| geometry.cells.get(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert_eq!((value("NoFill"), value("NoLine"), value("NoShow")), (Some("1".to_owned()), Some("0".to_owned()), None));
    let _ = std::fs::remove_dir_all(dir);
}

/// The walk of a chain of groups is held to the model's own depth: a chain of 250 is walked to 200 and no further, a shape at the bottom is left as it is,
/// and the work is that many steps.
#[test]
fn a_chain_of_groups_deeper_than_the_model_allows_is_walked_to_its_bound_and_no_further() {
    // A group of the file, whose one member the model then replaces by a chain of 250 groups with a shape whose colour is `RGB(r,g,b)` at the bottom.
    let (mut s, dir) = opened_with(&drawing_of(&format!("<draw:g>{}</draw:g>", rectangles(1))));
    s.edit("Test edit", |doc: &mut Document| {
        let mut leaf = Shape::new(500);
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.cells.insert(CellName::new("FillForegnd"), Cell::with_formula("RGB(0,255,0)", Some(Value::from_text("#00ff00"))));
        leaf.sheet = Arc::new(sheet);
        let mut inner = Arc::new(leaf);
        for level in 0..250u32 {
            let mut group = Shape::new(400 + level);
            group.kind = diacraft_doc::ShapeType::Group;
            group.children = diacraft_doc::ShapeList::from_vec(vec![inner]);
            inner = Arc::new(group);
        }
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let top = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        top.children = diacraft_doc::ShapeList::from_vec(vec![inner]);
        Ok(())
    })
    .unwrap();
    let (flat, steps) = flat_of(&s, &Flattened::default());
    assert!(flat.unwrap().doc.is_none(), "nothing within the bound changes, and nothing beyond it is looked at");
    assert_eq!(
        steps,
        1 + diacraft_doc::MAX_SHAPE_DEPTH as u64,
        "one visit of the group at the top and one step for each of the levels the model allows"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A connector is told from a line: glued, or the marker `connector.add` writes, or a route cell it holds, and it is given the route style the engine draws
/// it in; a shape with free ends and none of these is a line and has no route cell.
#[test]
fn a_connector_is_given_the_route_cells_and_a_line_is_not() {
    let (mut s, dir) = opened(2);
    let a = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 3.0, "y": 2.0 })).unwrap();
    let c = s.execute("connector.add", &json!({ "from": { "id": a["id"] }, "to": { "x": 6.0, "y": 2.0 } })).unwrap();
    let cid = u32::try_from(c["id"].as_u64().unwrap()).unwrap();
    s.execute("connector.style", &json!({ "style": "straight", "ids": [cid] })).unwrap();
    let route = |s: &Session| -> (Option<f64>, Option<f64>) {
        let (flat, _) = flat_of(s, &Flattened::default());
        let copy = flat.unwrap().doc.unwrap();
        let shape = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == cid).unwrap().clone();
        let number = |n: &str| shape.sheet.cell(n).and_then(|c| c.value.as_ref()?.number());
        (number("ShapeRouteStyle"), number("ConLineRouteExt"))
    };
    assert_eq!(route(&s), (Some(2.0), Some(1.0)), "straight");
    s.execute("connector.style", &json!({ "style": "right_angle", "ids": [cid] })).unwrap();
    assert_eq!(route(&s), (Some(1.0), Some(1.0)), "right angle");
    // The model's own connector has neither.
    let held = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == cid).unwrap().clone();
    assert!(held.sheet.cell("ShapeRouteStyle").is_none() || held.sheet.cell("ShapeRouteStyle").and_then(|c| c.value.as_ref()?.number()) == Some(1.0));
    let _ = std::fs::remove_dir_all(dir);
}

/// What a new shape resolves to that the file has no attribute for is carried, and said of no one: rounded corners are a cell of the shape's own (the writer
/// carries it in DiaCraft's own data on the shape's style), and a shape that has them from a style sheet is given them as a constant.
#[test]
fn what_a_new_shape_has_that_is_not_written_is_carried_and_not_said() {
    let (mut s, dir) = opened(1);
    let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    assert!(flat_of(&s, &Flattened::default()).0.unwrap().notes.is_empty(), "nothing yet");
    s.execute("shape.format", &json!({ "ids": [id], "rounding": 0.25 })).unwrap();
    let first = flat_of(&s, &Flattened::default()).0.unwrap();
    assert!(first.notes.is_empty(), "{:?}: rounded corners are carried", first.notes);
    assert_eq!(first.made.lost(0, id), Some(0), "nothing is lost");
    let copy = first.doc.as_ref().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    let rounding = copy.sheet.cell("Rounding").and_then(|c| c.value.as_ref()?.number());
    assert!(rounding.is_some_and(|r| (r - 0.25).abs() < 1e-9), "the copy holds the rounding: {rounding:?}");
    // A connector that comes says nothing either, its arrowhead is written.
    let known = first.made;
    let a = s.execute("connector.add", &json!({ "from": { "id": id }, "to": { "x": 8.0, "y": 2.0 } })).unwrap();
    let again = flat_of(&s, &known).0.unwrap();
    assert!(again.notes.is_empty(), "{:?}: the connector's arrowhead is carried, and not a loss", again.notes);
    // The flat copy of the connector holds the end the connector resolves to (and its size class where one resolves: the writer's is 2 otherwise), and no begin.
    let cid = u32::try_from(a["id"].as_u64().unwrap()).unwrap();
    let copy = again.doc.unwrap();
    let shape = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == cid).unwrap().clone();
    let number = |n: &str| shape.sheet.cell(n).and_then(|c| c.value.as_ref()?.number());
    assert!(number("EndArrow").is_some_and(|n| n > 0.0), "the end: {:?}", shape.sheet.cells);
    assert!(number("BeginArrow").is_none(), "no begin: {:?}", shape.sheet.cells);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_ids_of_the_file_are_not_carried_and_every_other_attribute_is() {
    let raw = diacraft_doc::RawAttrs::from_pairs([("xml:id", "id1"), ("ext:note", "x"), ("draw:id", "id1")]);
    assert_eq!(without_ids(&raw).iter().collect::<Vec<_>>(), [("ext:note", "x")]);
    let plain = diacraft_doc::RawAttrs::from_pairs([("ext:note", "x")]);
    assert_eq!(without_ids(&plain), plain);
    // Each of the two is taken out when it is the only one.
    for one in ["xml:id", "draw:id"] {
        let alone = diacraft_doc::RawAttrs::from_pairs([("ext:note", "x"), (one, "id1")]);
        assert_eq!(without_ids(&alone).iter().collect::<Vec<_>>(), [("ext:note", "x")], "{one}");
    }
}

/// The plain flattening is cheap enough to say a time: 3,000 new rectangles in a drawing of 20,000 take a fraction of the second the save's read of
/// the file takes (here, a few seconds on a loaded machine bounds it, and the counted tests above say it is linear).
#[test]
fn a_flattening_of_thousands_of_new_shapes_takes_seconds_at_most() {
    let (mut s, dir) = opened(20_000);
    drop_rectangles(&mut s, 3_000);
    let started = std::time::Instant::now();
    let flat = flatten(s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap().index(), &Flattened::default(), u64::MAX, false).unwrap();
    assert_eq!(flat.flattened, 3_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
    let _ = std::fs::remove_dir_all(dir);
}

/// The session clones its state for every command, and the file it holds with it: what an `.odg` keeps beside the bytes (the index of 350,000 shapes, the shapes
/// flattened) is shared by a clone and not copied.
#[test]
fn what_a_session_keeps_of_an_odg_is_shared_by_a_clone_of_the_file() {
    let (mut s, dir) = opened(200);
    drop_rectangles(&mut s, 5);
    s.execute("file.save", &json!({})).unwrap();
    let file = s.source().unwrap().clone();
    let (a, b) = (file.odg().unwrap(), file.clone().odg().unwrap().clone());
    assert!(
        std::ptr::eq(a.index(), b.index()) && std::ptr::eq(a.flattened(), b.flattened()),
        "the index and the shapes flattened are behind pointers"
    );
    assert!(Arc::ptr_eq(&file.shared(), &s.source().unwrap().shared()), "and so are the bytes");
    assert_eq!(a.flattened().len(), 5, "the five that the save flattened");
    let _ = std::fs::remove_dir_all(dir);
}

/// Every shape of the page written, as a command that touches the whole drawing does: the shapes and their sheets are new.
fn edit_every_shape(s: &mut Session) {
    s.edit("Test edit", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text("1")).with_unit("IN"));
        }
        Ok(())
    })
    .unwrap();
}

/// The engine puts the place of a label on a connector it routes, and the writer has no attribute for it: the copy of a connector that holds no text has none of
/// those cells, and one that holds a text keeps them, so that a change of one is the writer's refusal in words.
#[test]
fn a_routed_connector_without_a_text_loses_the_cells_of_its_label_in_the_copy_and_one_with_a_text_keeps_them() {
    let rect = |name: &str, x: u32, id: &str| {
        format!(
            "<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    let connector = "<draw:connector draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"K\" draw:start-shape=\"id1\" draw:start-glue-point=\"3\" draw:end-shape=\"id2\" draw:end-glue-point=\"1\" svg:x1=\"6cm\" svg:y1=\"4cm\" svg:x2=\"12cm\" svg:y2=\"4cm\" svg:d=\"M 6000 4000 L 12000 4000\" svg:viewBox=\"0 0 12001 4001\"><text:p/></draw:connector>";
    let (mut s, dir) = opened_with(&drawing_of(&format!("{}{}{connector}", rect("A", 2, "id1"), rect("B", 12, "id2"))));
    let a = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    s.execute("shape.move", &json!({ "ids": [a], "dx": 0.0, "dy": -1.0 })).unwrap();
    let labelled = |doc: &Document| doc.pages.get(0).unwrap().shapes.get(2).unwrap().sheet.cell("TxtPinX").is_some();
    assert!(labelled(s.document().unwrap()), "the engine wrote the place of the label when it routed the connector");
    let (flat, _) = flat_of(&s, &Flattened::default());
    let copy = flat.unwrap().doc.expect("a copy without the cells");
    assert!(!labelled(&copy), "the copy has none");
    assert!(labelled(s.document().unwrap()), "and the model keeps them");
    // The same connector with a text is not touched: its label is something.
    s.edit("Test edit", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(2).unwrap()).text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("label"))], ..Default::default() }));
        Ok(())
    })
    .unwrap();
    let (flat, _) = flat_of(&s, &Flattened::default());
    assert!(flat.unwrap().doc.is_none_or(|copy| labelled(&copy)), "a connector with a text keeps the cells of its label");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the engine holds beside the drawing, **measured** by the shapes it holds and not priced by a share of the model's cost (`hold.rs`): the versions the
/// history keeps count what an edit wrote (an edit of one shape is a chunk of 64 shapes, an edit of every shape is the drawing again), and the redo steps count the
/// same way from the other side.
#[test]
fn the_history_holds_what_the_drawing_does_not_share_with_it_by_what_it_holds() {
    use super::hold::{Sizer, walked_steps};
    let (mut s, dir) = opened(640);
    let held = |s: &Session| walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]).0;
    let per_shape = {
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap();
        // The formulas the shapes share are priced once, as the walk prices them.
        let mut sizer = Sizer::default();
        page.shapes.iter().map(|x| sizer.shape(x)).sum::<u64>() / 640
    };
    assert!(per_shape > 100, "a shape holds a sheet with cells: {per_shape}");
    assert_eq!(held(&s), 0, "no history, nothing held");
    let first = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    s.execute("shape.move", &json!({ "ids": [first], "dx": 1.0, "dy": 0.0 })).unwrap();
    let one = held(&s);
    assert!(
        (per_shape..2 * per_shape).contains(&one),
        "one shape written: that shape and the pointers of its chunk, not its 63 neighbours, which it shares: {one} bytes at {per_shape} a shape"
    );
    edit_every_shape(&mut s);
    let two = held(&s);
    assert!(two > one + 600 * per_shape && two < one + 700 * per_shape, "and then every shape: the drawing before it, whole: {one} then {two}");
    edit_every_shape(&mut s);
    let three = held(&s);
    assert!(three > two + 600 * per_shape && three < two + 700 * per_shape, "again: {three}");
    // Undo puts the drawing before the last edit, and the one it went back from is a redo step: counted from the other side, the same.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!((three - 2 * per_shape..three + 2 * per_shape).contains(&held(&s)), "an undo step less and a redo step more");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(held(&s), three);
    // An edit that writes no shape holds none: a page added shares the first page's list.
    s.execute("page.add", &json!({})).unwrap();
    assert_eq!(held(&s), three, "the page before it shares every shape");
    let _ = std::fs::remove_dir_all(dir);
}

/// **A group is the members it holds** (the review: the first count took a group for one shape, and a drawing of 175,000 members in groups, the
/// groups edited four times, aborted the save): a group of 1,000 members whose members were all written is a drawing's worth of shapes held, and a group written
/// without its members is not.
#[test]
fn the_members_of_a_group_are_counted_and_not_the_group_alone() {
    use super::hold::{Sizer, walked_steps};
    let (mut s, dir) = opened(2);
    s.edit("Test group", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let members: Vec<Arc<Shape>> = (0..1_000)
            .map(|k| {
                let mut shape = Shape::new(1_000 + k);
                Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text("1")).with_unit("IN"));
                Arc::new(shape)
            })
            .collect();
        let mut group = Shape::new(999);
        group.children = diacraft_doc::ShapeList::from_vec(members);
        page.shapes.push(Arc::new(group));
        Ok(())
    })
    .unwrap();
    let held = |s: &Session| walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]);
    let member = {
        let page = s.document().unwrap().pages.get(0).unwrap().clone();
        let group = page.shapes.iter().find(|x| x.id == 999).unwrap().clone();
        let mut sizer = Sizer::default();
        sizer.shape(group.children.get(0).unwrap());
        sizer.shape(group.children.get(1).unwrap())
    };
    // Every member written: the group and a whole list of members are new.
    s.edit("Test members", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == 999).unwrap();
        let group = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        for k in 0..group.children.len() {
            let member = Arc::make_mut(group.children.get_mut(k).unwrap());
            Arc::make_mut(&mut member.sheet).cells.insert(CellName::new("PinY"), Cell::constant(Value::from_text("2")).with_unit("IN"));
        }
        Ok(())
    })
    .unwrap();
    let (bytes, _) = held(&s);
    assert!(bytes > 1_000 * member, "the 1,000 members the version before holds are counted: {bytes} bytes, a member {member}");
    // The group written alone (its name changed) holds its members as the version before did: the members are shared, so none is new.
    s.edit("Test group name", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == 999).unwrap();
        Arc::make_mut(page.shapes.get_mut(at).unwrap()).name = Some(Arc::from("renamed"));
        Ok(())
    })
    .unwrap();
    let (again, _) = held(&s);
    assert!(again < bytes + 200 * member, "the members the version shares with the drawing are not counted twice: {again} against {bytes}");
    let _ = std::fs::remove_dir_all(dir);
}

/// **A mass delete does not make the save impossible** (the review: 3,990 of 4,000 shapes deleted priced the 4,000 the history holds at the
/// model's cost over the 10 that were left, and a save was refused for 4,292 MiB): the history holds 4,000 shapes and is priced at what they hold, which is the
/// size of the file's model and no more.
#[test]
fn a_mass_delete_is_priced_by_what_the_shapes_held_and_not_by_the_shapes_left() {
    use super::hold::walked_steps;
    let (mut s, dir) = opened(4_000);
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().skip(10).map(|x| x.id).collect();
    s.execute("shape.delete", &json!({ "ids": ids })).unwrap();
    assert_eq!(s.document().unwrap().pages.get(0).unwrap().shapes.len(), 10);
    let (bytes, _) = walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]);
    let model = s.source().and_then(|f| f.odg()).unwrap().index().read_cost().model_bytes();
    assert!(bytes > model / 4 && bytes < model * 4, "{bytes} bytes held beside a drawing of a model of {model}");
    assert!(bytes < 64 * 1024 * 1024, "megabytes, not gigabytes: {bytes}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The measure of a shape is the writer's own figure for a model within a factor: the estimate of what the shapes of a file read hold is not far from what the
/// reader says the model costs (`ReadCost::model_bytes`, which the writer's gate is made of), so that the two gates agree about what a drawing weighs.
#[test]
fn the_measure_of_a_shape_agrees_with_what_the_reader_says_the_model_costs() {
    use super::hold::Sizer;
    let (s, dir) = opened(2_000);
    let model = s.source().and_then(|f| f.odg()).unwrap().index().read_cost().model_bytes();
    let mut sizer = Sizer::default();
    let mine: u64 = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|x| sizer.shape(x)).sum();
    assert!(mine * 3 > model && mine < model * 3, "{mine} bytes by the shapes against {model} by the reader");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the walk costs is held to what is new to it: the shapes of the drawing once, then the chunks of each version (a pointer each) and the shapes in the chunks
/// that are not shared. A history of 100 one-shape edits of a drawing four times as big costs the chunks four times over and the 64 new shapes of each edit the
/// same, never the drawing again for each step.
#[test]
fn the_walk_of_the_history_costs_the_chunks_of_each_version_and_the_new_shapes_and_no_more() {
    use super::hold::walked_steps;
    let cost = |n: usize| {
        let (mut s, dir) = opened(n);
        let (_, base) = walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]);
        let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().take(100).map(|x| x.id).collect();
        for id in ids {
            s.execute("shape.move", &json!({ "ids": [id], "dx": 0.01, "dy": 0.0 })).unwrap();
        }
        let (_, with) = walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]);
        let _ = std::fs::remove_dir_all(dir);
        (base, with - base)
    };
    let (small, large) = (cost(640), cost(2_560));
    assert!(large.0 > 3 * small.0 && large.0 < 5 * small.0, "the drawing itself once: {small:?} {large:?}");
    // 100 steps, each a version of every chunk (a pointer look) and the 64 shapes of the chunk written.
    assert!(small.1 < 100 * (640 / 64 + 64 + 16), "{small:?}");
    assert!(large.1 < 100 * (2_560 / 64 + 64 + 16), "{large:?}");
}

/// The flattening of what the session holds now against what its last save flattened, with its steps: (the shapes flattened, the copies given again, the steps).
fn after_save(s: &Session) -> (u64, u64, u64) {
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let flat = flatten_counted(doc, state.index(), state.flattened(), &mut work).unwrap();
    (flat.flattened, flat.reused, work.steps)
}

/// **A shape an earlier save flattened is given again, not flattened again** (the review), until something it was made from changes: the
/// shape itself, the styles, the themes, the colours or the document's sheet, its master, whether a connector is glued and how a connector is drawn. The copies are
/// given again at a step each, whatever they cost to make.
#[test]
fn a_shape_flattened_by_an_earlier_save_is_given_again_until_what_it_was_made_from_changes() {
    let (mut s, dir) = opened(2);
    drop_rectangles(&mut s, 5);
    let a = s.document().unwrap().pages.get(0).unwrap().shapes.iter().next_back().unwrap().id;
    let b = s.document().unwrap().pages.get(0).unwrap().shapes.iter().rev().nth(1).unwrap().id;
    let connector = s.execute("connector.add", &json!({ "from": { "id": a }, "to": { "id": b } })).unwrap();
    let cid = u32::try_from(connector["id"].as_u64().unwrap()).unwrap();
    let shapes = s.document().unwrap().pages.get(0).unwrap().shapes.len() as u64;
    s.execute("file.save", &json!({})).unwrap();
    let (flattened, reused, steps) = after_save(&s);
    assert_eq!((flattened, reused), (0, 6), "nothing is flattened again: the five and the connector are given again");
    // A step for the visit of each shape, one for each `Connect` row, one for the look at the colours of each of the two shapes of the file (the copies given again
    // are not looked at) and one for the way the connector is drawn.
    assert_eq!(steps, shapes + 2 + 2 + 1, "{steps}: a copy given again costs the visit of it and nothing of what it cost to make");
    // One shape written: that shape is flattened again and the others are given again.
    s.execute("shape.move", &json!({ "ids": [a], "dx": 0.5, "dy": 0.0 })).unwrap();
    let (flattened, reused, _) = after_save(&s);
    assert_eq!(
        (flattened, reused),
        (2, 4),
        "the shape moved, and the connector glued to it that is routed again, are flattened; the others are given again"
    );
    s.execute("file.save", &json!({})).unwrap();
    // A page's way of drawing connectors: the connector that follows the page's style is flattened again, the shapes are not.
    s.execute("connector.style", &json!({ "style": "straight" })).unwrap();
    let (flattened, reused, _) = after_save(&s);
    assert_eq!((flattened, reused), (1, 5), "the connector is drawn another way: {flattened} {reused}");
    // (A page's own cells are not written by a save yet, so that change is taken back and not saved.)
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(after_save(&s).0, 0, "and drawn as it was, nothing is flattened");
    // The shape's master replaced: not the very master the copy was made from.
    s.edit("Test master", |doc: &mut Document| {
        let master = Arc::make_mut(doc.masters.get_mut(0).unwrap());
        master.name = Some(Arc::from("renamed"));
        Ok(())
    })
    .unwrap();
    let (flattened, _, _) = after_save(&s);
    assert!(flattened >= 5, "the rectangles' master is another master: {flattened}");
    s.execute("file.save", &json!({})).unwrap();
    assert_eq!(after_save(&s).0, 0);
    // The document's styles, themes, colours and sheet: any of them is not what the copies were made under, and every copy is made again.
    s.edit("Test style", |doc: &mut Document| {
        let style = Arc::make_mut(doc.styles.get_mut(0).unwrap());
        style.name = Some(Arc::from("renamed"));
        Ok(())
    })
    .unwrap();
    assert_eq!(after_save(&s).0, 6, "the styles changed: the six are flattened again");
    // (A change of the styles or of the document's sheet is one a save does not write yet: each is taken back and not saved.)
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(after_save(&s).0, 0, "taken back, they are what the copies were made under");
    s.edit("Test sheet", |doc: &mut Document| {
        Arc::make_mut(&mut doc.sheet).cells.insert(CellName::new("Probe"), Cell::constant(Value::from_text("1")));
        Ok(())
    })
    .unwrap();
    assert_eq!(after_save(&s).0, 6, "the document's sheet changed: the six are flattened again");
    let _ = cid;
    let _ = std::fs::remove_dir_all(dir);
}

/// **Every cell and section a new shape holds is carried or inert** (which closed the kinds of loss the cells had): the shape's own, one of each
/// name at a time, flattened. The copy holds the cell or the section as the shape has it (the writer carries it in DiaCraft's own data on the shape's style), nothing is said to
/// be lost, and what the engine writes by itself (`ResizeMode`) is left out. An empty section holds nothing, and the text is carried whole, the markers of a run among it.
#[test]
fn every_cell_and_section_a_new_shape_holds_is_carried_or_inert() {
    let row = |name: &str| {
        let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("1")));
        Arc::new(row)
    };
    let section = |name: &str, rows: &[&str]| {
        let mut section = Section::new(name, None);
        for r in rows {
            section.rows.push(row(r));
        }
        Arc::new(section)
    };
    // The cells, and whether the copy holds each.
    let cells: [(&str, bool); 12] = [
        ("ResizeMode", false),
        ("TextBkgnd", true),
        ("TextBkgndTrans", true),
        ("TextDirection", true),
        ("LayerMember", true),
        ("LockMoveX", true),
        ("Comment", true),
        ("FillBkgnd", true),
        ("ShdwForegnd", true),
        ("BeginArrowSize", false),
        ("Rounding", true),
        ("ShdwPattern", true),
    ];
    let sections: [(&str, &[&str]); 11] = [
        ("Character", &["Row_1"]),
        ("Paragraph", &["Row_1"]),
        ("Tabs", &["Row_1"]),
        ("Property", &["Row_1"]),
        ("Prop", &["Row_1"]),
        ("User", &["Row_1"]),
        ("User", &["DiaCraftRoute"]),
        ("Hyperlink", &["Row_1"]),
        ("Actions", &["Row_1"]),
        ("Scratch", &["Row_1"]),
        ("Controls", &["Row_1"]),
    ];
    let made = |edit: &dyn Fn(&mut Shape)| -> (u16, Vec<String>, Shape) {
        let (mut s, dir) = opened(1);
        let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.edit("Test edit", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.id == id).unwrap();
            edit(Arc::make_mut(page.shapes.get_mut(at).unwrap()));
            Ok(())
        })
        .unwrap();
        let flat = flat_of(&s, &Flattened::default()).0.unwrap();
        let _ = std::fs::remove_dir_all(dir);
        let copy = flat.doc.as_ref().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
        (flat.made.lost(0, id).unwrap(), flat.notes, Shape::clone(&copy))
    };
    for (name, held) in cells {
        let value = if name == "Rounding" || name == "ShdwPattern" { "2" } else { "1" };
        let (lost, notes, copy) = made(&|shape| {
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
        });
        assert_eq!(lost, 0, "the cell {name} is no loss: {notes:?}");
        assert!(notes.is_empty(), "the cell {name} is not said: {notes:?}");
        assert_eq!(
            copy.sheet.cell(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()),
            held.then(|| value.to_owned()),
            "the cell {name} in the copy"
        );
    }
    for (name, rows) in sections {
        let (lost, notes, copy) = made(&|shape| Arc::make_mut(&mut shape.sheet).sections.push(section(name, rows)));
        assert_eq!(lost, 0, "the section {name} {rows:?}: {notes:?}");
        assert!(notes.is_empty(), "{notes:?}");
        let held: Vec<Option<String>> = copy
            .sheet
            .sections
            .iter()
            .filter(|g| g.name.as_str() == name)
            .flat_map(|g| g.rows.iter().map(|r| r.name.as_deref().map(str::to_owned)))
            .collect();
        assert_eq!(held, rows.iter().map(|r| Some((*r).to_owned())).collect::<Vec<_>>(), "the section {name} in the copy");
    }
    // The text is carried whole: the markers of a run and its fields are no loss.
    let text = diacraft_doc::Text {
        parts: vec![diacraft_doc::TextPart::Cp(0), diacraft_doc::TextPart::Chars(Arc::from("hello")), diacraft_doc::TextPart::Cp(1)],
        ..Default::default()
    };
    let (lost, notes, copy) = made(&|shape| shape.text = Some(Arc::new(text.clone())));
    assert_eq!((lost, notes.is_empty()), (0, true), "the markers of a run");
    assert_eq!(copy.text.as_deref(), Some(&text), "the text, whole");
}

/// **The text a new shape reaches through a style sheet is audited** (the review, parked item 2): a stencil file's master whose text
/// links to a text style that holds the rows of a run (the size, the bold of the style) drops as a shape whose text properties the copy, which has no link, does
/// not carry. The save says so, once, as text formatting; a style that holds none, the root style, a link that names no style and a chain that ends in a style
/// with rows are told apart, and a chain of links that goes round is followed no further than a cap.
#[test]
fn the_text_a_new_shape_reaches_through_a_style_sheet_is_audited() {
    use super::flatten::TEXT_FORMAT;
    let style = |id: u32, text_link: Option<u32>, rows: bool, cells: &[&str]| {
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.links.text = text_link;
        if rows {
            let mut section = Section::new("Character", None);
            let mut row = Row { ix: Some(0), ..Row::default() };
            row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_text("0.25")));
            section.rows.push(Arc::new(row));
            sheet.sections.push(Arc::new(section));
        }
        for c in cells {
            sheet.cells.insert(CellName::new(c), Cell::constant(Value::from_text("1")));
        }
        Arc::new(diacraft_doc::Style { id, sheet: Arc::new(sheet), ..diacraft_doc::Style::default() })
    };
    let lost = |styles: Vec<Arc<diacraft_doc::Style>>, link: Option<u32>, master: bool| -> (u16, Vec<String>) {
        let (mut s, dir) = opened(1);
        let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.edit("Test edit", |doc: &mut Document| {
            for st in styles {
                doc.styles.push(st);
            }
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.id == id).unwrap();
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            if master {
                // The shape's master's shape holds the link, as a stencil master's does.
                let m = shape.master.unwrap();
                let masters: Vec<Arc<Master>> = doc.masters.iter().cloned().collect();
                let mut changed = Vec::new();
                for mut one in masters {
                    if one.id == m {
                        let inner = Arc::make_mut(&mut one);
                        if let Some(top) = inner.shapes.get(0).cloned() {
                            let mut top = Shape::clone(&top);
                            Arc::make_mut(&mut top.sheet).links.text = link;
                            let mut list: Vec<Arc<Shape>> = vec![Arc::new(top)];
                            list.extend(inner.shapes.iter().skip(1).cloned());
                            inner.shapes = diacraft_doc::ShapeList::from_vec(list);
                        }
                    }
                    changed.push(one);
                }
                doc.masters = diacraft_doc::SharedVec::from_vec(changed);
            } else {
                Arc::make_mut(&mut shape.sheet).links.text = link;
            }
            Ok(())
        })
        .unwrap();
        let flat = flat_of(&s, &Flattened::default()).0.unwrap();
        let _ = std::fs::remove_dir_all(dir);
        (flat.made.lost(0, id).unwrap(), flat.notes)
    };
    // A text style that holds the rows of a run: the loss is said, once, as text formatting.
    let (got, notes) = lost(vec![style(7, None, true, &[])], Some(7), false);
    assert_eq!(got, TEXT_FORMAT, "{notes:?}");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("text formatting"), "{notes:?}");
    // The same through the link of the master's shape.
    let (got, notes) = lost(vec![style(7, None, true, &[])], Some(7), true);
    assert_eq!(got, TEXT_FORMAT, "{notes:?}");
    // A cell that qualifies the text, with no rows.
    for cell in ["TextBkgnd", "TextDirection"] {
        let (got, _) = lost(vec![style(7, None, false, &[cell])], Some(7), false);
        assert_eq!(got, TEXT_FORMAT, "{cell}");
    }
    // The rows are in a style the chain reaches: the style the shape links to is based on it.
    let (got, _) = lost(vec![style(7, Some(8), false, &[]), style(8, None, true, &[])], Some(7), false);
    assert_eq!(got, TEXT_FORMAT, "a chain");
    // Nothing is lost: a style with no rows, a link that names no style, no link, and the root style whose rows are the defaults of every shape.
    for (styles, link) in [(vec![style(7, None, false, &[])], Some(7)), (Vec::new(), Some(99)), (Vec::new(), None), (Vec::new(), Some(0))] {
        let (got, notes) = lost(styles, link, false);
        assert_eq!(got, 0, "{link:?}: {notes:?}");
        assert!(notes.is_empty(), "{notes:?}");
    }
    // A section of another name that holds rows, and a text section that holds none, are no formatting; the paragraph and tab sections that hold rows are.
    let section_style = |id: u32, name: &str, rows: bool| {
        let mut sheet = diacraft_doc::Sheet::default();
        let mut section = Section::new(name, None);
        if rows {
            let mut row = Row { ix: Some(0), ..Row::default() };
            row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("1")));
            section.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(section));
        Arc::new(diacraft_doc::Style { id, sheet: Arc::new(sheet), ..diacraft_doc::Style::default() })
    };
    for (name, rows) in [("User", true), ("Geometry", true), ("Character", false), ("Paragraph", false), ("Tabs", false)] {
        let (got, notes) = lost(vec![section_style(7, name, rows)], Some(7), false);
        assert_eq!(got, 0, "{name}, rows {rows}: {notes:?}");
    }
    for name in ["Character", "Paragraph", "Tabs"] {
        let (got, _) = lost(vec![section_style(7, name, true)], Some(7), false);
        assert_eq!(got, TEXT_FORMAT, "{name} with a row");
    }
    // A chain of links that goes round is followed to the cap and no further, and says nothing when no style on it holds a row.
    let (got, _) = lost(vec![style(7, Some(8), false, &[]), style(8, Some(7), false, &[])], Some(7), false);
    assert_eq!(got, 0, "a cycle");
}

/// **What the last save was given and flattened is held beside the drawing, and counted** (the review: the pages the index holds alone were
/// not): after a save the state holds the pages the writer was given (the copy, whose dropped shapes are the flattened ones), and the walk counts them beside the model
/// (which holds the shapes as instances of a master) and counts the model's shape that each flattened copy was made from once the shape is written.
#[test]
fn the_pages_the_last_save_was_given_and_the_shapes_it_flattened_are_counted_beside_the_drawing() {
    use super::hold::{held_beside, walked_steps};
    let (mut s, dir) = opened(2);
    drop_rectangles(&mut s, 20);
    s.execute("file.save", &json!({})).unwrap();
    let state = s.source().and_then(|f| f.odg()).unwrap().clone();
    let saved = state.saved_pages();
    assert_eq!(saved.len(), 1, "the one page the writer was given");
    let held = |s: &Session, saved: &[Arc<diacraft_doc::Page>], kept: &[Arc<Shape>]| {
        let doc = s.save_inputs().unwrap().doc;
        let mut steps = 0;
        held_beside(&doc, &doc, s.history(), saved, kept, u64::MAX, &mut steps)
    };
    // The copy's dropped shapes are flattened (their cells are their own, and the model's are the master's) and are not the model's: counted beside the model.
    let model = s.document().unwrap().pages.get(0).unwrap().clone();
    let flat = saved[0].shapes.iter().filter(|x| model.shapes.iter().any(|m| m.id == x.id && !Arc::ptr_eq(m, x))).count();
    assert!(flat >= 20, "the copy holds the 20 dropped shapes flattened: {flat}");
    let (without, with) = (held(&s, &[], &[]), held(&s, saved, &[]));
    assert!(with > without + 20 * 5_000, "the pages of the last save are counted: {without} then {with}");
    assert_eq!(walked_steps(&s.save_inputs().unwrap().doc, s.history(), saved).0, with, "as the walk the tests use counts them");
    // The model's shapes the copies were made from are the model's own while no shape is written: nothing more is held.
    let sources = state.flattened().sources();
    assert_eq!(sources.len(), 20);
    assert_eq!(held(&s, &[], &sources), without, "the shapes of the model are held once");
    // Written, each is another shape of the model, and the shape it was made from is held beside it.
    let ids: Vec<u32> = model.shapes.iter().skip(2).map(|x| x.id).collect();
    for id in &ids {
        s.execute("shape.move", &json!({ "ids": [id], "dx": 0.25, "dy": 0.0 })).unwrap();
    }
    // (The undo steps hold those shapes too; they are left out here so that the shapes the state keeps are what is counted.)
    let alone = |kept: &[Arc<Shape>]| {
        let doc = s.save_inputs().unwrap().doc;
        let mut steps = 0;
        held_beside(&doc, &doc, None, &[], kept, u64::MAX, &mut steps)
    };
    let (before, after) = (alone(&[]), alone(&sources));
    assert!(after > before + 20 * 300, "the 20 shapes the copies were made from, written since, are held beside the drawing: {before} then {after}");
    let _ = std::fs::remove_dir_all(dir);
}

/// What a flattening may take of the memory is held at the byte: a charge that comes to the room is made and one byte more is refused in words; the shapes it
/// replaces and the copies it makes are priced by what they hold (never nothing, never a byte).
#[test]
fn the_copies_a_flattening_makes_are_charged_by_what_they_hold_and_held_to_the_room() {
    let mut work = Work::new(MAX_STEPS, 100);
    assert!(work.charge(30, 70).is_ok(), "30 and 70 are the room");
    assert_eq!((work.charged, work.replaced), (100, 30));
    let refused = work.charge(0, 1).expect_err("one byte more");
    assert!(refused.contains("more memory than a save may") && refused.contains("undo some of the changes"), "{refused}");
    assert!(Work::new(MAX_STEPS, 100).charge(0, 100).is_ok(), "the copy alone may take the room");
    assert!(Work::new(MAX_STEPS, 100).charge(101, 0).is_err(), "so may not the shapes it replaces");
    // By what the shapes hold: ten dropped rectangles replace what a dropped rectangle holds (a pin) and are copied as the shapes they are made into, which hold the
    // cells and rows of a rectangle: kilobytes each, not a byte or nothing.
    let (mut s, dir) = opened(1);
    drop_rectangles(&mut s, 10);
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut counted = Work::new(MAX_STEPS, u64::MAX);
    flatten_counted(doc, state.index(), &Flattened::default(), &mut counted).unwrap();
    assert!((10 * 300..10 * 2_000).contains(&counted.replaced), "the ten shapes replaced hold their pins: {}", counted.replaced);
    assert!(counted.charged > counted.replaced + 10 * 3_000 && counted.charged < counted.replaced + 10 * 30_000, "the copies: {}", counted.charged);
    // The room that is the sum is enough, and one byte less is not.
    let mut exact = Work::new(MAX_STEPS, counted.charged);
    flatten_counted(doc, state.index(), &Flattened::default(), &mut exact).expect("the room that is the sum");
    let mut short = Work::new(MAX_STEPS, counted.charged - 1);
    let refused = flatten_counted(doc, state.index(), &Flattened::default(), &mut short).expect_err("a byte short");
    assert!(refused.contains("more memory than a save may"), "{refused}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The text a flattened shape has is the text the shape has, whole: the text of a field, the markers of a run and an empty text are carried as they are, and
/// the writer carries what a text holds beyond its characters in DiaCraft's own data on the shape's style, so nothing of it is a loss.
#[test]
fn the_text_of_a_flattened_shape_is_the_text_the_shape_has_whole() {
    use diacraft_doc::TextPart;
    let flattened = |parts: Vec<TextPart>| -> (Option<Vec<TextPart>>, u16) {
        let (mut s, dir) = opened(1);
        let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.edit("Test edit", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.id == id).unwrap();
            Arc::make_mut(page.shapes.get_mut(at).unwrap()).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
            Ok(())
        })
        .unwrap();
        let flat = flat_of(&s, &Flattened::default()).0.unwrap();
        let copy = flat.doc.as_ref().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
        let _ = std::fs::remove_dir_all(dir);
        (copy.text.as_ref().map(|t| t.parts.clone()), flat.made.lost(0, id).unwrap())
    };
    let chars = |t: &str| TextPart::Chars(Arc::from(t));
    for parts in [
        vec![chars("a"), TextPart::Fld { ix: 0, text: Arc::from("FIELD") }, chars("b")],
        vec![TextPart::Fld { ix: 0, text: Arc::from("X") }],
        vec![TextPart::Cp(0)],
        vec![TextPart::Cp(0), chars("hi"), TextPart::Pp(0), TextPart::Tp(0)],
        vec![chars("")],
        vec![chars("one\ntwo")],
    ] {
        assert_eq!(flattened(parts.clone()), (Some(parts.clone()), 0), "{parts:?}");
    }
}

/// A group whose members hold a colour a person chose (`RGB(r,g,b)`) is copied member by member and its list of members with it, and **each copy is charged to the memory
/// before it is made**, at what the shape holds and as many times as it is held (the shape of the model that stays, and the copy): the sum is the sum of those, to the
/// byte (the first version charged none of it, and a drawing of 175,000 members whose colours were chosen aborted the save).
#[test]
fn the_copies_of_the_members_of_a_group_that_hold_a_chosen_colour_are_charged_to_the_byte() {
    use super::hold::Sizer;
    let members = 640usize;
    let inner: String =
        (0..members).map(|k| format!("<draw:rect draw:name=\"m{k}\" svg:x=\"{k}mm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>")).collect();
    let (mut s, dir) = opened_with(&drawing_of(&format!("<draw:g draw:name=\"G\">{inner}</draw:g>")));
    let group = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    s.execute("shape.format", &json!({ "ids": [group], "fill": "#336699" })).unwrap();
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let flat = flatten_counted(doc, state.index(), &Flattened::default(), &mut work).unwrap();
    assert!(flat.doc.is_some(), "the members' colours are made constants in a copy");
    // The sum, from the shapes themselves: every member twice (the one that stays and its copy, which holds the same) and the size of a shape, in the order the
    // members are met (a formula many share is priced once), then the copy of the group (a shape) and the list of its members and the chunks above it.
    let page = doc.pages.get(0).unwrap();
    let group_shape = page.shapes.iter().find(|x| x.id == group).unwrap();
    let mut sizer = Sizer::default();
    let mut expected = 0u64;
    for member in group_shape.children.iter() {
        let own = sizer.shape(member);
        expected += own + own + std::mem::size_of::<diacraft_doc::Shape>() as u64;
    }
    expected += std::mem::size_of::<diacraft_doc::Shape>() as u64;
    expected += members as u64 * 8 + (members as u64 / diacraft_doc::CHUNK as u64) * 8;
    assert_eq!(work.charged, expected, "every copy is charged, to the byte");
    // And the replaced shapes are those of the members.
    let replaced: u64 = {
        let mut sizer = Sizer::default();
        group_shape.children.iter().map(|m| sizer.shape(m)).sum()
    };
    assert_eq!(work.replaced, replaced);
    let _ = std::fs::remove_dir_all(dir);
}

/// A shape whose only change is the colour of a run a person chose (an `RGB(r,g,b)` in a `Character` row, no colour of the shape's own) is copied with a constant
/// there, and the copy is charged as any other: the shape it replaces at what it holds, and the copy at that and the size of a shape (the review,
/// and the mutation of `constants`).
#[test]
fn the_copy_of_a_shape_whose_run_colour_was_chosen_is_charged_to_the_byte() {
    use super::hold::Sizer;
    let (mut s, dir) = opened(1);
    let id = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    s.execute("shape.set_text", &json!({ "id": id, "text": "abcde" })).unwrap();
    s.execute("text.format", &json!({ "ids": [id], "start": 1, "end": 3, "color": "#cc0000" })).unwrap();
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let own = Sizer::default().shape(doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap());
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let flat = flatten_counted(doc, state.index(), &Flattened::default(), &mut work).unwrap();
    assert!(flat.doc.is_some(), "the run's colour is made a constant in a copy");
    assert_eq!(work.replaced, own, "the shape the copy replaces is charged at what it holds");
    assert_eq!(work.charged, own + own + std::mem::size_of::<diacraft_doc::Shape>() as u64, "and the copy at that and the size of a shape");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the walk of the history counts is the pointers' and the shapes', to the byte: a drawing of one chunk of shapes, every shape written, holds the version before it
/// beside it: the page (an `Arc` and a page), a chunk (its 64 pointers, the count and its place in the list above), and each shape at what it holds.
#[test]
fn a_whole_chunk_written_holds_the_page_the_chunk_and_the_shapes_of_the_version_before_to_the_byte() {
    use super::hold::{Sizer, walked_steps};
    let (mut s, dir) = opened(64);
    let before: Vec<Arc<Shape>> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().cloned().collect();
    edit_every_shape(&mut s);
    let (bytes, _) = walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]);
    let mut sizer = Sizer::default();
    let shapes: u64 = before.iter().map(|x| sizer.shape(x)).sum();
    let page = 16 + std::mem::size_of::<diacraft_doc::Page>() as u64;
    let chunk = 16 + 8 * 64 + 8;
    assert_eq!(bytes, page + chunk + shapes, "the page, the chunk and the 64 shapes");
    // A formula that many cells share is priced once, and one that nobody shares at its tree.
    let mut sizer = Sizer::default();
    let formula_shape = |text: &str| {
        let mut shape = Shape::new(1);
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Width"), Cell::read(Some(text), Some("1"), Some("IN"), None, &mut 1_000));
        shape
    };
    let one = formula_shape("Width*0.5");
    let (first, second) = (sizer.shape(&one), sizer.shape(&one));
    assert!(
        first > second + 9 * 25 && first - second == 9 * 25 + 96,
        "the second look at the same formula leaves its tree out: {first} then {second}"
    );
    let other = formula_shape("Width*0.7");
    assert_eq!(sizer.shape(&other), first, "another formula is priced in full");
    let _ = std::fs::remove_dir_all(dir);
}

/// What a shape holds is priced by each of its parts, to the byte: the shape and its names and data, its sheet, each cell (its entry, its name, its formula's tree, its value
/// and its unit), each section with its cells and rows, each row with its names and cells, the text with its parts, and the attributes and nodes each of them keeps (an
/// allocation of 96 each). The test spells out what each part is worth and puts the shape together part by part.
#[test]
fn a_shape_is_priced_by_each_part_it_holds_to_the_byte() {
    use super::hold::Sizer;
    use diacraft_doc::{RawAttrs, RawNode, Text, TextPart};
    use std::mem::size_of;
    let (arc, entry, node) = (16u64, 24u64, 96u64);
    let cell_of = |name: &str, formula: Option<&str>, value: &str, unit: Option<&str>| -> (CellName, Cell, u64) {
        let cell = Cell::read(formula, Some(value), unit, None, &mut 1_000);
        let tree = formula.map_or(0, |f| f.len() as u64 * 25 + node);
        let bytes = size_of::<CellName>() as u64
            + size_of::<Cell>() as u64
            + entry
            + name.len() as u64
            + tree
            + value.len() as u64
            + unit.map_or(0, |u| u.len() as u64);
        (CellName::new(name), cell, bytes)
    };
    let kept = |attrs: usize, nodes: usize| -> (RawAttrs, diacraft_doc::Passthrough, u64) {
        let raw = RawAttrs::from_pairs((0..attrs).map(|k| (format!("a{k}"), "v")));
        let mut pass = diacraft_doc::Passthrough::new();
        for _ in 0..nodes {
            pass.push(0, RawNode::Text(Arc::from("x")));
        }
        (raw, pass, (attrs + nodes) as u64 * node)
    };
    // A row: its names, a cell, one attribute and two nodes kept.
    let (cell_name, cell, cell_bytes) = cell_of("Value", Some("Width*0.5"), "3", Some("IN"));
    let (raw, pass, kept_bytes) = kept(1, 2);
    let mut row = Row {
        name: Some(Arc::from("R1")),
        local_name: Some(Arc::from("Local")),
        row_type: Some(Arc::from("T")),
        raw,
        passthrough: pass,
        ..Row::default()
    };
    row.cells.insert(cell_name, cell);
    let row_bytes = arc + size_of::<Row>() as u64 + 2 + 5 + 1 + cell_bytes + kept_bytes;
    // A section: its name, a cell of its own, the row, one attribute and three nodes kept.
    let (flag_name, flag, flag_bytes) = cell_of("NoFill", None, "1", None);
    let (raw, pass, kept_bytes) = kept(1, 3);
    let mut section = Section::new("Prop", None);
    section.cells.insert(flag_name, flag);
    section.rows.push(Arc::new(row));
    section.raw = raw;
    section.passthrough = pass;
    let section_bytes = arc + size_of::<Section>() as u64 + 4 + flag_bytes + row_bytes + kept_bytes;
    // The sheet: two cells (one formula of another text than the row's), the section, two attributes and four nodes kept.
    let (a_name, a, a_bytes) = cell_of("PinX", None, "1.5", Some("IN"));
    let (b_name, b, b_bytes) = cell_of("Height", Some("Width*0.7"), "2", None);
    let (raw, pass, kept_bytes) = kept(2, 4);
    let mut sheet = diacraft_doc::Sheet { raw, passthrough: pass, ..diacraft_doc::Sheet::default() };
    sheet.cells.insert(a_name, a);
    sheet.cells.insert(b_name, b);
    sheet.sections.push(Arc::new(section));
    let sheet_bytes = arc + size_of::<diacraft_doc::Sheet>() as u64 + a_bytes + b_bytes + section_bytes + kept_bytes;
    // The shape: names, data, the sheet, a text of four parts, and one attribute and five nodes kept.
    let text = Text {
        parts: vec![
            TextPart::Chars(Arc::from("abc")),
            TextPart::Fld { ix: 0, text: Arc::from("field") },
            TextPart::Raw(RawNode::Text(Arc::from("r"))),
            TextPart::Cp(0),
        ],
        ..Default::default()
    };
    // Each part is a `TextPart` of its own; a run and a field hold their text behind an `Arc`, a kept node is an allocation of its own, a marker is the part.
    let part = size_of::<TextPart>() as u64;
    let text_bytes = arc + 4 * part + (arc + 3) + (arc + 5) + node;
    let (raw, pass, kept_bytes) = kept(1, 5);
    let shape = Shape {
        name: Some(Arc::from("Shape")),
        name_u: Some(Arc::from("S")),
        data1: Some(Arc::from("dd")),
        data3: Some(Arc::from("e")),
        sheet: Arc::new(sheet),
        text: Some(Arc::new(text)),
        raw,
        passthrough: pass,
        ..Shape::new(1)
    };
    let expected = arc + size_of::<Shape>() as u64 + 5 + 1 + 2 + 1 + sheet_bytes + text_bytes + kept_bytes;
    assert_eq!(Sizer::default().shape(&shape), expected);
}

/// The walk stops as soon as the sum is past the limit it is given: it is already over, and the cost of the walk is bounded by what a save may hold and not by what a
/// history holds. A drawing whose history holds three whole drawings, walked with a limit of one kilobyte, goes through a few steps and not the thousands of the whole.
#[test]
fn the_walk_stops_when_the_sum_is_past_the_limit() {
    use super::hold::held_beside;
    let (mut s, dir) = opened(640);
    for _ in 0..3 {
        edit_every_shape(&mut s);
    }
    let doc = s.save_inputs().unwrap().doc;
    let (mut whole, mut cut, mut at_the_limit) = (0, 0, 0);
    let full = held_beside(&doc, &doc, s.history(), &[], &[], u64::MAX, &mut whole);
    let stopped = held_beside(&doc, &doc, s.history(), &[], &[], 1_000, &mut cut);
    assert!(stopped > 1_000 && stopped < full, "the sum so far, past the limit: {stopped} of {full}");
    assert!(
        cut + 1_000 < whole,
        "{cut} steps against {whole}: the walk ended (the drawing itself is walked whole, and no version after the first is)"
    );
    // A limit that the whole sum is under is the whole sum, and the walk went all the way.
    let all = held_beside(&doc, &doc, s.history(), &[], &[], full, &mut at_the_limit);
    assert_eq!((all, at_the_limit), (full, whole), "a sum that is the limit is not past it");
    let _ = std::fs::remove_dir_all(dir);
}

/// The connects of a page are held once by the versions that share them: a version of a page whose shapes were written holds the page (an allocation) and nothing of
/// the connects the page now has; one before the connects were written holds the 50 rows they were, each at what a row and its two ends keep.
#[test]
fn the_connects_of_a_page_are_counted_once_by_the_versions_that_share_them() {
    use super::hold::walked_steps;
    use std::mem::size_of;
    let (mut s, dir) = opened(64);
    let row = |k: u32| diacraft_doc::Connect {
        from_sheet: k,
        from_cell: Some(Arc::from("BeginX")),
        from_part: None,
        to_sheet: k,
        to_cell: None,
        to_part: Some(100),
        raw: diacraft_doc::RawAttrs::new(),
        ext: Default::default(),
    };
    s.edit("Test connects", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.connects = diacraft_doc::Connects::from_vec((0..50).map(row).collect());
        Ok(())
    })
    .unwrap();
    let page = (16 + size_of::<diacraft_doc::Page>()) as u64;
    let held = |s: &Session| walked_steps(&s.save_inputs().unwrap().doc, s.history(), &[]).0;
    // The version before this one has no connects and shares every shape: it holds a page.
    assert_eq!(held(&s), page, "the page before the connects were written");
    // A shape written: the version before holds that shape and its chunk, and the page; its connects are the page's now (the same list), and are not counted again.
    let old_first = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    s.execute("shape.move", &json!({ "ids": [old_first.id], "dx": 1.0, "dy": 0.0 })).unwrap();
    let after_move = held(&s);
    // Two pages, the chunk of the shape that was written and that shape as it was (the connects the pages hold are the drawing's own, counted by none).
    assert_eq!(
        after_move,
        2 * page + (16 + 8 * 64 + 8) + super::hold::Sizer::default().shape(&old_first),
        "the pages before the move share the connects of the page now"
    );
    s.edit("Test more connects", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.connects.push(row(50));
        Ok(())
    })
    .unwrap();
    // Now the version before holds 50 rows that the page no longer has the list of, and a page; its shapes are those of the drawing.
    let rows = 50 * (size_of::<diacraft_doc::Connect>() as u64 + 96);
    assert_eq!(held(&s), after_move + page + rows, "the page and the 50 rows the drawing has a longer list of");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the writer is given and what the model is are counted together and each once: the shapes of the model that the copy does not hold are priced beside it; a group
/// the state keeps holds its members, which are priced with it.
#[test]
fn the_model_beside_a_copy_and_the_members_of_a_kept_group_are_priced() {
    use super::hold::{Sizer, held_beside};
    let (s, dir) = opened(64);
    let doc = s.save_inputs().unwrap().doc;
    // A copy that holds none of the model's shapes (every one is another shape): the whole model is held beside it.
    let mut copy = (*doc).clone();
    {
        let page = Arc::make_mut(copy.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text("9")).with_unit("IN"));
        }
    }
    let mut steps = 0;
    let beside = held_beside(&copy, &doc, None, &[], &[], u64::MAX, &mut steps);
    let mut sizer = Sizer::default();
    let shapes: u64 = doc.pages.get(0).unwrap().shapes.iter().map(|x| sizer.shape(x)).sum();
    assert_eq!(beside, 16 + std::mem::size_of::<diacraft_doc::Page>() as u64 + 16 + 8 * 64 + 8 + shapes, "the model, which the copy shares none of");
    assert_eq!(held_beside(&doc, &doc, None, &[], &[], u64::MAX, &mut steps), 0, "the model given is held by the writer and by nothing else");
    // A shape the state keeps that is a group: its members are held with it.
    let members: Vec<Arc<Shape>> = (0..100).map(|k| Arc::new(Shape::new(5_000 + k))).collect();
    let group = Arc::new(Shape { children: diacraft_doc::ShapeList::from_vec(members.clone()), ..Shape::new(4_999) });
    let mut sizer = Sizer::default();
    let expected = sizer.shape(&group) + members.iter().map(|m| sizer.shape(m)).sum::<u64>() + 16 + 8 * 64 + 8 + 16 + 8 * 64 + 8;
    let kept = held_beside(&doc, &doc, None, &[], &[group], u64::MAX, &mut steps);
    assert_eq!(kept, expected, "the group, its two chunks of members, and the members");
    let _ = std::fs::remove_dir_all(dir);
}

/// **What the audit of text through a style sheet costs is counted** (the sweep of the new loops): the table of the document's style sheets is made once, only
/// when a shape is flattened, and is charged a step for each sheet before it is made; a chain of links that goes round costs the cap and no more, however many
/// styles the document has beside it.
#[test]
fn the_style_table_of_the_text_audit_is_charged_for_each_sheet_and_only_when_a_shape_is_flattened() {
    let style = |id: u32, text_link: Option<u32>| {
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.links.text = text_link;
        Arc::new(diacraft_doc::Style { id, sheet: Arc::new(sheet), ..diacraft_doc::Style::default() })
    };
    // The steps of flattening the drawing with `styles` added to it and, when `drop` is set, a new rectangle linked to the text style `link`.
    let steps_of = |styles: Vec<Arc<diacraft_doc::Style>>, drop: bool, link: Option<u32>| -> u64 {
        let (mut s, dir) = opened(1);
        if drop {
            let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
            let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
            s.edit("Test edit", |doc: &mut Document| {
                let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                let at = page.shapes.iter().position(|x| x.id == id).unwrap();
                let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
                Arc::make_mut(&mut shape.sheet).links.text = link;
                Ok(())
            })
            .unwrap();
        }
        s.edit("Test styles", |doc: &mut Document| {
            for st in &styles {
                doc.styles.push(Arc::clone(st));
            }
            Ok(())
        })
        .unwrap();
        let steps = flat_of(&s, &Flattened::default()).1;
        let _ = std::fs::remove_dir_all(dir);
        steps
    };
    let many = |n: u32| -> Vec<Arc<diacraft_doc::Style>> { (0..n).map(|k| style(1000 + k, None)).collect() };
    // A drawing that flattens nothing makes no table: the styles it holds cost it nothing.
    assert_eq!(steps_of(Vec::new(), false, None), steps_of(many(300), false, None));
    // One that flattens a shape pays a step for each style sheet, once: 300 more sheets are 300 more steps, and 1,200 are 1,200.
    let (none, some, more) = (steps_of(Vec::new(), true, None), steps_of(many(300), true, None), steps_of(many(1200), true, None));
    assert_eq!(some - none, 300, "{none} {some}");
    assert_eq!(more - none, 1200, "{none} {more}");
    // A chain of links that goes round costs the cap, each way along it taken, and no more: a hundred sheets beside it add only the table.
    let ring = || vec![style(7, Some(8)), style(8, Some(7))];
    let plain = steps_of(ring(), true, None);
    let round = steps_of(ring(), true, Some(7));
    assert_eq!(round - plain, super::flatten::MOST_STYLE_STEPS as u64, "the walk stops at the cap: {plain} {round}");
    let mut crowded = ring();
    crowded.extend(many(100));
    assert_eq!(steps_of(crowded, true, Some(7)) - steps_of(many(100), true, None), 2 + super::flatten::MOST_STYLE_STEPS as u64);
}

// ---- the first save of a drawing with no file yet writes its thumbnail ------------------------

/// The drawing `bytes` as a drawing with no file yet holds it: the package it was made from, with no path (`OpenFile::from_template`), and the model its read gave.
fn unsaved(bytes: &[u8]) -> (Arc<Document>, crate::file::OpenFile) {
    let (doc, report) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    (Arc::new(doc), crate::file::OpenFile::from_template(bytes.to_vec()).with_odg(super::OdgState::new(report.index)))
}

/// The same drawing as one saved to a file at `path`.
fn saved_as(bytes: &[u8], path: &str) -> (Arc<Document>, crate::file::OpenFile) {
    let (doc, report) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    (Arc::new(doc), crate::file::OpenFile::new(std::path::Path::new(path), bytes.to_vec()).with_odg(super::OdgState::new(report.index)))
}

/// The thumbnail of a package, when it has one.
fn thumbnail_of(bytes: &[u8]) -> Option<Vec<u8>> {
    diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap().part("Thumbnails/thumbnail.png").map(<[u8]>::to_vec)
}

/// The paths of the rows of a package's manifest.
fn manifest_rows(bytes: &[u8]) -> Vec<String> {
    diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default())
        .unwrap()
        .manifest()
        .entries
        .iter()
        .map(|e| e.full_path.clone())
        .collect()
}

/// The width and the height of a PNG, from its header.
fn png_size(png: &[u8]) -> (u32, u32) {
    (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]))
}

/// The first save of a drawing that has no file yet (a new drawing, whose package is the one it was made from) writes the picture of its first
/// page as the thumbnail **though the model is not edited**, at most 256 pixels on the long side, with its manifest row; the package is otherwise the one it
/// was made from, and the save is no edit of the model.
#[test]
fn the_first_save_of_a_drawing_with_no_file_yet_writes_the_thumbnail_of_its_first_page_though_nothing_changed() {
    let package = drawing(2);
    assert!(thumbnail_of(&package).is_none());
    let (doc, file) = unsaved(&package);
    let saved = super::save::save("file.save", &doc, &file, false, None, None, crate::across::Options::default()).unwrap();
    assert!(!saved.edited && saved.warnings.is_empty(), "{:?}", saved.warnings);
    let png = thumbnail_of(&saved.bytes).expect("a thumbnail");
    let (w, h) = png_size(&png);
    // An A4 page, portrait: the long side is the height, as near 256 as the page's size gives.
    assert!((250..=256).contains(&h) && w < h, "{w} x {h}");
    assert_eq!(manifest_rows(&saved.bytes), ["/", "content.xml", "styles.xml", "Thumbnails/thumbnail.png"]);
    let changes = diacraft_odg::diff_packages(&package, &saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(changes.added, ["Thumbnails/thumbnail.png"]);
    assert!(changes.bytes_only.is_empty() && changes.removed.is_empty(), "{changes:?}");
}

/// A drawing that has a file is not saved for the first time: its save that is no edit is the file's bytes, a thumbnail-less file included, whether or not the
/// session calls the drawing changed (a change taken back is the case: the model is the file's, and the writer sees no edit).
#[test]
fn a_drawing_that_has_a_file_keeps_its_bytes_on_a_save_that_is_no_edit_whatever_the_session_says() {
    let package = drawing(2);
    let (doc, file) = saved_as(&package, "/some/where/a.odg");
    for dirty in [false, true] {
        let saved = super::save::save("file.save", &doc, &file, dirty, None, None, crate::across::Options::default()).unwrap();
        assert_eq!((saved.bytes.as_slice(), saved.edited), (package.as_slice(), false), "dirty {dirty}");
    }
}

/// A package with a thumbnail keeps it on the first save; and the file the first save makes, which has one now, is saved again as the same bytes
/// whether it is saved as a file of its own (the next save of the drawing) or again as a drawing with no file (another save of it as a new one).
#[test]
fn a_thumbnail_the_package_has_is_kept_and_a_second_save_is_the_same_bytes() {
    let package = drawing(2);
    let (doc, file) = unsaved(&package);
    let first = super::save::save("file.save", &doc, &file, false, None, None, crate::across::Options::default()).unwrap();
    let png = thumbnail_of(&first.bytes).unwrap();
    // The next save of the drawing, which has its file now, with the index the first gave.
    let next = crate::file::OpenFile::new(std::path::Path::new("/some/where/a.odg"), first.bytes.clone()).with_odg(first.state.clone());
    let second = super::save::save("file.save", &doc, &next, false, None, None, crate::across::Options::default()).unwrap();
    assert_eq!((second.bytes.as_slice(), second.edited), (first.bytes.as_slice(), false));
    // The package that has a thumbnail, as the source of a drawing with no file yet: kept, not rendered over.
    let (doc_again, again) = unsaved(&first.bytes);
    let kept = super::save::save("file.save", &doc_again, &again, true, None, None, crate::across::Options::default()).unwrap();
    assert_eq!(thumbnail_of(&kept.bytes), Some(png));
    assert_eq!(kept.bytes, first.bytes);
}

/// What `file.open` says was kept (`skipped_count`) counts each thing once: a placeholder or a box is no more than the element it is (the census compares
/// this number).
#[test]
fn what_file_open_says_was_kept_counts_a_placeholder_and_a_box_once() {
    let table = r#"<draw:frame svg:width="1cm" svg:height="1cm"><table:table table:name="T"/></draw:frame>"#;
    let caption = r#"<draw:caption svg:x="3cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" draw:caption-point-x="1cm" draw:caption-point-y="1cm"/>"#;
    let unknown = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#;
    let bytes = drawing_of(&format!("{table}{caption}{unknown}"));
    let dir = std::env::temp_dir().join(format!("diacraft-engine-kept-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.odg");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::new();
    let opened = s.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap();
    // The entries the reader keeps and no more: the table frame (`frame_object`, `shape_child`) is 2, the callout (`caption` and its two point attributes) 3 and the
    // type-only shape (`enhanced_geometry`, `shape_type`) 2; a placeholder or a box was counted a second time by the first version, which made it 10.
    assert_eq!(opened["file"]["skipped_count"], 7, "{opened}");
    let _ = std::fs::remove_dir_all(dir);
}

/// An object with no picture, which the reader gave a `User` row to say what it is, is moved and saved as any shape of the file is: the one
/// element's position changes in the file, and nothing else of the part (the row is the model's, and the file has no place for it and needs none).
#[test]
fn an_object_that_is_a_placeholder_is_moved_and_saved_by_its_frame_alone() {
    let caption = r#"<draw:caption draw:name="C" svg:x="3cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" draw:caption-point-x="1cm" draw:caption-point-y="1cm"/>"#;
    let package = drawing_of(&format!("{}{caption}", rectangles(1)));
    let (mut s, dir) = opened_with(&package);
    let held = s.document().unwrap().pages.get(0).unwrap().shapes.get(1).unwrap().sheet.clone();
    assert!(held.sections.iter().any(|section| section.name.as_str() == "User"), "the object names itself");
    s.execute("shape.move", &json!({ "ids": [2], "dx": 1.0, "dy": 0.5 })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let saved = std::fs::read(dir.join("a.odg")).unwrap();
    let text = |bytes: &[u8]| {
        let p = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
        String::from_utf8(p.part("content.xml").unwrap().to_vec()).unwrap()
    };
    let moved = r#"svg:x="5.54cm" svg:y="1.73cm" svg:width="4cm" svg:height="2cm" draw:caption-point-x"#;
    let was = r#"svg:x="3cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" draw:caption-point-x"#;
    assert_eq!(text(&saved), text(&package).replace(was, moved), "only the frame's position changed");
    // Opened again, it is the same object, still naming itself.
    let (again, again_dir) = opened_with(&saved);
    let shape = again.document().unwrap().pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    assert!(shape.foreign.is_none() && shape.sheet.sections.iter().any(|section| section.name.as_str() == "User"));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(again_dir);
}

/// The caption of a drawing of one rectangle and one callout (an object DiaCraft cannot draw, with no picture, which the reader gives a `User` row that names it).
const CALLOUT: &str =
    r#"<draw:caption draw:name="C" svg:x="3cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" draw:caption-point-x="1cm" draw:caption-point-y="1cm"/>"#;

/// A section called `name` with rows of these names.
fn section_of(name: &str, rows: &[&str]) -> Arc<Section> {
    let mut s = Section::new(name, None);
    for row in rows {
        s.rows.push(Arc::new(Row { name: Some(Arc::from(*row)), ..Row::default() }));
    }
    Arc::new(s)
}

/// A shape of this kind with these sections.
fn shape_of(kind: diacraft_doc::ShapeType, sections: Vec<Arc<Section>>) -> Shape {
    let mut shape = Shape::new(1);
    shape.kind = kind;
    Arc::make_mut(&mut shape.sheet).sections = sections;
    shape
}

/// The shape with these children kept, each given as written: an element by its name, and a text by its characters when the name starts with a `"`.
fn keeping(mut shape: Shape, kept: &[&str]) -> Shape {
    for name in kept {
        let node = match name.strip_prefix('"') {
            Some(text) => diacraft_doc::RawNode::Text(Arc::from(text)),
            None => diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
                name: Arc::from(*name),
                attrs: diacraft_doc::RawAttrs::new(),
                children: Vec::new(),
            }),
        };
        shape.passthrough.push(0, node);
    }
    shape
}

/// `is_object` of a shape with a work of its own, and the steps it took.
fn object_steps(shape: &Shape) -> (bool, u64) {
    let mut work = Work::new(MAX_STEPS, u64::MAX);
    let found = is_object(shape, &mut work).unwrap();
    (found, work.steps)
}

/// What is refused is exactly an object: a **foreign shape** (a picture frame) that **has the row `ODFObject` in a `User` section** (an object with no picture, which
/// the reader names for the box that stands for it), **or keeps the element of an object among the children it keeps** (an object with a replacement picture, which has no
/// row). A foreign shape with neither (a picture), a plain shape that has the row or the element (a user cell or a child another program wrote), a row of that name in another
/// section, and an element that is not a top-level child are not.
#[test]
fn an_object_is_a_foreign_shape_with_the_row_in_a_user_section_or_the_element_of_an_object_kept_and_nothing_else_is() {
    let (foreign, plain) = (diacraft_doc::ShapeType::Foreign, diacraft_doc::ShapeType::Shape);
    let found = |shape: &Shape| object_steps(shape).0;
    // With no picture: the row.
    assert!(found(&shape_of(foreign.clone(), vec![user_rows(&["ODFObject"])])), "the object");
    assert!(found(&shape_of(foreign.clone(), vec![user_rows(&["Other", "ODFObject"])])), "among other user rows");
    assert!(found(&shape_of(foreign.clone(), vec![section_of("Prop", &["ODFObject"]), user_rows(&["ODFObject"])])), "after another section");
    assert!(!found(&shape_of(foreign.clone(), Vec::new())), "a picture: no sections");
    assert!(!found(&shape_of(foreign.clone(), vec![user_rows(&["Other"])])), "a user row of another name");
    assert!(!found(&shape_of(foreign.clone(), vec![user_rows(&[])])), "an empty user section");
    assert!(!found(&shape_of(foreign.clone(), vec![section_of("Prop", &["ODFObject"])])), "the name in another section is no marker");
    assert!(!found(&shape_of(plain.clone(), vec![user_rows(&["ODFObject"])])), "a plain shape with the row is a plain shape");
    assert!(!found(&shape_of(diacraft_doc::ShapeType::Group, vec![user_rows(&["ODFObject"])])));
    // With a picture: the element, each of the kinds R50 lists, at any place among the children kept, whatever prefix binds the namespace.
    for element in
        ["draw:object", "draw:object-ole", "draw:applet", "draw:plugin", "draw:floating-frame", "table:table", "d:object", "x:table", "object"]
    {
        assert!(found(&keeping(shape_of(foreign.clone(), Vec::new()), &[element])), "{element}");
        assert!(
            found(&keeping(shape_of(foreign.clone(), Vec::new()), &["svg:title", "\"text", "draw:image-map", element])),
            "{element} after other children"
        );
    }
    // What is no object, with a picture: the children a picture frame keeps, a name that only starts like an object's, and an element that is not on the top level.
    for kept in [
        vec![],
        vec!["svg:title", "svg:desc", "office:event-listeners", "draw:image-map", "draw:text-box", "draw:glue-point", "draw:contour-polygon"],
        vec!["\"object", "\"draw:object"],
        vec!["draw:objects", "draw:object-", "table:tables", "draw:object:x", "svg:object-ole:", "draw:", "draw:ole"],
    ] {
        assert!(!found(&keeping(shape_of(foreign.clone(), Vec::new()), &kept)), "{kept:?}");
    }
    let mut inside = keeping(shape_of(foreign.clone(), Vec::new()), &["svg:title"]);
    if let Some(diacraft_doc::Placed { node: diacraft_doc::RawNode::Element(title), .. }) = inside.passthrough.nodes_mut().first_mut() {
        title.children.push(diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
            name: Arc::from("draw:object"),
            attrs: diacraft_doc::RawAttrs::new(),
            children: Vec::new(),
        }));
    }
    assert!(!found(&inside), "an element inside a child kept is not the frame's object");
    // A shape that is not a picture frame is no object, whatever it keeps.
    for kind in [plain.clone(), diacraft_doc::ShapeType::Group] {
        assert!(!found(&keeping(shape_of(kind, Vec::new()), &["draw:object", "table:table"])));
    }
    // A shape whose text holds the frame of an object has the row `ODFTextObject`, whatever kind of shape it is, and the row in no other section is no marker.
    for kind in [plain.clone(), foreign.clone(), diacraft_doc::ShapeType::Group] {
        assert!(found(&shape_of(kind.clone(), vec![user_rows(&["ODFTextObject"])])), "{kind:?}: the text's row");
        assert!(
            found(&shape_of(kind.clone(), vec![section_of("Prop", &["ODFTextObject"]), user_rows(&["a", "ODFTextObject"])])),
            "{kind:?}: after a section"
        );
        assert!(!found(&shape_of(kind.clone(), vec![section_of("Prop", &["ODFTextObject"])])), "{kind:?}: the name in another section is no marker");
        assert!(!found(&shape_of(kind, vec![user_rows(&["ODFTextObjects", "odftextobject", "TextObject"])])), "a name that only looks like it");
    }
    // The row of an object with no picture is no marker on a shape that is not a picture frame.
    assert!(!found(&shape_of(plain, vec![user_rows(&["ODFObject"])])));
}

/// What looking for an object costs, counted in the steps of the flattening: **a shape that is not a picture frame costs a step for each of its sections and each row of
/// its `User` section, and none for what it keeps** (a shape with no section, as a shape dropped from a master is, costs none, so the step counts of the tests above do not
/// move), a picture frame costs those and a step for each child it keeps, all charged before they are looked at, and **the cap holds**: a shape that keeps more than the cap
/// allows is refused in words, and so are many copies of one that keeps a little.
#[test]
fn looking_for_an_object_is_charged_by_what_the_shape_holds_and_the_cap_holds() {
    let (foreign, plain) = (diacraft_doc::ShapeType::Foreign, diacraft_doc::ShapeType::Shape);
    let kept = ["svg:title"; 7];
    // A plain shape: its section (1) and the two rows of it (2), and nothing for the seven children it keeps; one with no section, nothing. The row of a text is found by
    // the same look: a section (1) and its rows (3), and found.
    assert_eq!(object_steps(&keeping(shape_of(plain.clone(), vec![user_rows(&["a", "b"])]), &kept)), (false, 1 + 2));
    assert_eq!(object_steps(&keeping(shape_of(plain.clone(), Vec::new()), &kept)), (false, 0));
    assert_eq!(object_steps(&keeping(shape_of(plain, vec![user_rows(&["x", "ODFTextObject", "y"])]), &kept)), (true, 1 + 3));
    // A picture frame with no object: a step for each of its sections (two), the rows of the `User` section only (five) and its seven kept children.
    let sections = vec![section_of("Prop", &["a", "b", "c"]), user_rows(&["a", "b", "c", "d", "e"])];
    assert_eq!(object_steps(&keeping(shape_of(foreign.clone(), sections.clone()), &kept)), (false, 2 + 5 + 7));
    assert_eq!(object_steps(&shape_of(foreign.clone(), Vec::new())), (false, 0));
    // The row is found before the children are charged, and an object kept is found after all of them were.
    assert_eq!(object_steps(&keeping(shape_of(foreign.clone(), vec![user_rows(&["x", "ODFObject", "y"])]), &kept)), (true, 1 + 3));
    assert_eq!(object_steps(&keeping(shape_of(foreign.clone(), sections.clone()), &["svg:title", "draw:object", "svg:desc"])), (true, 2 + 5 + 3));
    // Past the cap: refused in words, and the steps stop at the cap and a section.
    let mut work = Work::new(10, u64::MAX);
    let refused = is_object(&keeping(shape_of(foreign.clone(), Vec::new()), &["svg:title"; 100]), &mut work).expect_err("more than the cap");
    assert!(refused.contains("too many to write") && refused.contains("10"), "{refused}");
    assert_eq!(work.steps, 100, "charged before the children are looked at, and not one by one");
    // Many copies of a shape that keeps a little: the sum is what is capped (each copy is charged, so the walk of a drawing of copies stops at the cap).
    let mut work = Work::new(1_000, u64::MAX);
    let copy = keeping(shape_of(foreign, Vec::new()), &kept);
    let mut seen = 0;
    while is_object(&copy, &mut work).is_ok() {
        seen += 1;
        assert!(seen <= 1_000, "the cap stops it");
    }
    assert_eq!((seen, work.steps), (142, 143 * 7), "142 copies of seven fit in a thousand steps, and the 143rd crosses it");
}

/// The result and the steps of the flattening, under the cap `cap`, of a drawing of one rectangle of the file and these new `shapes`, each given an `ID` of its own. The
/// shapes the flattening is asked about end in a refusal in the loop that finds the new ones (an object, or the cap), so none of them is flattened.
fn new_shapes_flattened(shapes: Vec<Shape>, cap: u64) -> (Result<(), String>, u64) {
    let (mut s, dir) = opened(1);
    let shapes: Vec<Shape> = shapes.into_iter().enumerate().map(|(k, shape)| Shape { id: 1_000 + u32::try_from(k).unwrap(), ..shape }).collect();
    put_shapes(&mut s, shapes, &[]);
    let (doc, state) = (s.document().unwrap(), s.source().and_then(|f| f.odg()).unwrap());
    let mut work = Work::new(cap, u64::MAX);
    let result = flatten_counted(doc, state.index(), &Flattened::default(), &mut work).map(|_| ());
    let _ = std::fs::remove_dir_all(dir);
    (result, work.steps)
}

/// **The look for an object is charged to the flattening that makes it**, through `flatten_counted` and not in `is_object` alone. What a hand mutant
/// that gave `is_object` a work of its own (one that charges nothing to the cap) or charged a row after finding the marker left standing is pinned here by exact steps: a
/// drawing of one rectangle of the file (2 steps), then new picture frames that are no object (each a step for its visit, a step for each of its sections, one for each row
/// of its `User` section and one for each child it keeps), then **one that is** (found by the row, which is charged its section and rows and then found; or found by the
/// element it keeps, after the children were charged), which refuses the flattening in the loop and before any shape is flattened.
#[test]
fn the_look_for_an_object_is_charged_to_the_flattening_by_what_each_new_picture_frame_holds() {
    let foreign = diacraft_doc::ShapeType::Foreign;
    let (file_shape, visit) = (2u64, 1u64);
    // A picture frame that is no object: a `Prop` section of three rows and a `User` section of five, and nine children kept. Its look: 2 sections, 5 rows, 9 children.
    let picture =
        || keeping(shape_of(foreign.clone(), vec![section_of("Prop", &["a", "b", "c"]), user_rows(&["a", "b", "c", "d", "e"])]), &["svg:title"; 9]);
    let picture_look = 2 + 5 + 9;
    // An object found by its row: the `Prop` section (1), then the `User` section (1) and its three rows (3), and found.
    let by_row =
        || keeping(shape_of(foreign.clone(), vec![section_of("Prop", &["a", "b", "c"]), user_rows(&["x", "ODFObject", "y"])]), &["svg:title"; 9]);
    let row_look = 1 + 1 + 3;
    // An object found by the element it keeps: all the sections (2) and the rows of the `User` section (5) and the nine children are charged, and then it is found.
    let by_element = || {
        let mut kept = ["svg:title"; 9];
        kept[4] = "draw:object";
        keeping(shape_of(foreign.clone(), vec![section_of("Prop", &["a", "b", "c"]), user_rows(&["a", "b", "c", "d", "e"])]), &kept)
    };
    for n in [0u64, 1, 4, 25] {
        let copies = |last: Shape| -> Vec<Shape> { (0..n).map(|_| picture()).chain(std::iter::once(last)).collect() };
        let before = file_shape + n * (visit + picture_look);
        let (refused, steps) = new_shapes_flattened(copies(by_row()), MAX_STEPS);
        assert_eq!((refused, steps), (Err(OBJECT_NOT_IN_FILE.to_owned()), before + visit + row_look), "{n} copies, then an object found by its row");
        let (refused, steps) = new_shapes_flattened(copies(by_element()), MAX_STEPS);
        assert_eq!(
            (refused, steps),
            (Err(OBJECT_NOT_IN_FILE.to_owned()), before + visit + picture_look),
            "{n} copies, then an object found by its element"
        );
        // A text frame whose text holds the frame of an object: a plain shape with the row of the text, found like the row of an object, after the `Prop`
        // section (1) and the `User` section (1) and its three rows (3).
        let by_text = shape_of(diacraft_doc::ShapeType::Shape, vec![section_of("Prop", &["a", "b", "c"]), user_rows(&["x", "ODFTextObject", "y"])]);
        let (refused, steps) = new_shapes_flattened(copies(by_text), MAX_STEPS);
        assert_eq!(
            (refused, steps),
            (Err(OBJECT_NOT_IN_FILE.to_owned()), before + visit + row_look),
            "{n} copies, then a text frame that holds an object"
        );
    }
    // The cap: the fourth copy's look crosses it at the charge of its children (after its 2 sections and 5 rows), by one step, and the flattening stops there in the cap's words.
    let steps_when_the_fourth_is_charged = file_shape + 3 * (visit + picture_look) + visit + 2 + 5 + 9;
    let cap = steps_when_the_fourth_is_charged - 1;
    let (refused, steps) = new_shapes_flattened((0..6).map(|_| picture()).collect(), cap);
    let refused = refused.expect_err("the cap is crossed");
    assert!(refused.contains("too many to write") && refused.contains(&format!("more than {cap} cells")), "{refused}");
    assert_eq!(steps, cap + 1, "stopped at the crossing: the copies before it were charged, and the ones after it never looked at");
}

/// The same at the production cap (`MAX_STEPS`, 6,000,000): nine copies of one picture frame that keeps 700,000 children (the copies share them, as the copies of a shape
/// do) are refused in the cap's words when the ninth copy's children are charged, and the steps stop there: the copies were charged before any was looked at.
#[test]
fn nine_copies_of_a_picture_frame_that_keeps_seven_hundred_thousand_children_cross_the_production_cap_in_the_caps_words() {
    let kept = 700_000u64;
    let shape = keeping(shape_of(diacraft_doc::ShapeType::Foreign, vec![user_rows(&["a", "b"])]), &vec!["svg:title"; usize::try_from(kept).unwrap()]);
    let (refused, steps) = new_shapes_flattened((0..9).map(|_| shape.clone()).collect(), MAX_STEPS);
    let refused = refused.expect_err("the cap is crossed");
    assert!(refused.contains("too many to write") && refused.contains("6000000"), "{refused}");
    // A visit, one section and two rows, and the children, for each copy; the file's rectangle is two steps.
    let per_copy = 1 + (1 + 2) + kept;
    assert_eq!(steps, 2 + 9 * per_copy, "each copy's look was charged in full, the ninth crossing the cap at the charge of its children");
    assert!(2 + 8 * per_copy <= MAX_STEPS && steps > MAX_STEPS);
}

/// Whether the shape at `at` on the first page of `doc` has the row that names an object (`User.ODFObject`).
fn names_an_object(doc: &Document, at: usize) -> bool {
    let shape = doc.pages.get(0).unwrap().shapes.get(at).unwrap();
    shape.sheet.sections.iter().any(|section| section.name.as_str() == "User" && section.rows.iter().any(|r| r.name.as_deref() == Some("ODFObject")))
}

/// **A copy of an object DiaCraft cannot draw is refused in words**, and nothing is written. Its element and the files that go with it
/// (the object's folder, its replacement picture) are not copied yet, so the save could only write a plain shape with the name
/// of the object gone (which draws nothing when it is opened again), and the one note it gave said "shape data" and not what was lost. The way out is the
/// usual one: undo the copy, or delete it. The file is as it was, and so is the drawing.
#[test]
fn a_copy_of_an_object_that_is_a_placeholder_is_refused_in_words_and_nothing_is_written() {
    let (mut s, dir) = opened_with(&drawing_of(&format!("{}{CALLOUT}", rectangles(1))));
    let file = dir.join("a.odg");
    let before = std::fs::read(&file).unwrap();
    s.execute("shape.duplicate", &json!({ "ids": [2] })).unwrap();
    assert!(names_an_object(s.document().unwrap(), 2), "the copy is an object in the model, naming itself");
    // The flattening refuses it, in words that say what it is and why, and name nothing from the file.
    let refused = flat_of(&s, &Flattened::default()).0.map(|_| ()).expect_err("the copy is refused");
    assert_eq!(refused, OBJECT_NOT_IN_FILE);
    assert!(refused.contains("an object (") && refused.contains("not copied") && !refused.contains("Callout"), "{refused}");
    // So does the save, with the way out, and the file is the one it was.
    let said = s.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(said.contains(OBJECT_NOT_IN_FILE) && said.contains("edit.undo"), "{said}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "nothing was written");
    // Undone, the drawing saves (it is the file's own again), and a move of the object itself still saves, by its frame alone.
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("shape.move", &json!({ "ids": [2], "dx": 1.0, "dy": 0.5 })).unwrap();
    let saved = s.execute("file.save", &json!({})).unwrap();
    assert_eq!(saved["edited"], true, "{saved}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The same refusal for **an object an undo brought back** after a save deleted it: it is not in the file any more, so it is as new as a copy is. The save that
/// deleted it was the file's (its element and files went), and the way out here is to redo the delete or delete it again.
#[test]
fn an_object_put_back_by_an_undo_after_a_save_deleted_it_is_refused_in_the_same_words() {
    let (mut s, dir) = opened_with(&drawing_of(&format!("{}{CALLOUT}", rectangles(1))));
    s.execute("shape.delete", &json!({ "ids": [2] })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(names_an_object(s.document().unwrap(), 1), "the object is back in the model");
    let said = s.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(said.contains(OBJECT_NOT_IN_FILE), "{said}");
    // Deleted again, the drawing is the file's.
    s.execute("shape.delete", &json!({ "ids": [2] })).unwrap();
    assert!(s.execute("file.save", &json!({})).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

/// What the refusal does not reach: a copy of a plain shape is flattened and saved as before. (A copy of an object that has a replacement picture is refused too, since
/// `crates/engine/tests/it/odg_objects_kept.rs`.)
#[test]
fn a_copy_of_a_plain_shape_is_saved_as_before() {
    let (mut s, dir) = opened_with(&drawing_of(&format!("{}{CALLOUT}", rectangles(1))));
    s.execute("shape.duplicate", &json!({ "ids": [1] })).unwrap();
    let saved = s.execute("file.save", &json!({})).unwrap();
    assert_eq!(saved["edited"], true, "{saved}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Through a save: a first save of a drawing whose first page has no size has no picture to make.
#[test]
fn a_first_save_whose_picture_cannot_be_made_says_no_thumbnail_was_written() {
    let package = drawing(2);
    let (doc, file) = unsaved(&package);
    let mut broken = (*doc).clone();
    let page = Arc::make_mut(broken.pages.get_mut(0).unwrap());
    Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("PageWidth"), Cell::constant(Value::from_text("0")));
    let saved = super::save::save("file.save", &Arc::new(broken), &file, false, None, None, crate::across::Options::default()).unwrap();
    // It saves (the page's size is written as the model has it), no thumbnail is in the package, and the one word says what is true: none was written, and not
    // that a picture "was not renewed" where there was none to renew.
    assert_eq!(saved.warnings, ["the picture of the first page could not be made, so no thumbnail was written: the page has no size"]);
    assert!(thumbnail_of(&saved.bytes).is_none(), "no thumbnail");
    assert!(!manifest_rows(&saved.bytes).iter().any(|r| r == "Thumbnails/thumbnail.png"), "and no row for one");
}

/// An edited first save writes one thumbnail, as an edited save of a file does.
#[test]
fn an_edited_first_save_writes_the_thumbnail_once() {
    let package = drawing(2);
    let (mut s, dir) = opened_with(&package);
    s.execute("shape.move", &json!({ "ids": [1], "dx": 1.0, "dy": 0.5 })).unwrap();
    let moved = Arc::new(s.document().unwrap().clone());
    let (_, unnamed) = unsaved(&package);
    let saved = super::save::save("file.save", &moved, &unnamed, true, None, None, crate::across::Options::default()).unwrap();
    assert!(saved.edited);
    assert_eq!(manifest_rows(&saved.bytes).iter().filter(|r| *r == "Thumbnails/thumbnail.png").count(), 1);
    assert!(thumbnail_of(&saved.bytes).is_some());
    let _ = std::fs::remove_dir_all(dir);
}

// item 4 and: every master of the model is the copy's, and a copy of a shape keeps the links to its master.

/// A drop brings its master into the model: the copy holds it too (NEWSAVE left it out), and the copy of the shape the drop made links to it. When the shape is
/// deleted again and the master stays in the model, nothing needs a copy, and the model itself is given.
#[test]
fn the_copy_keeps_every_master_of_the_model_and_the_links_of_a_shape_made_from_one() {
    let (mut s, dir) = opened(1);
    s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
    let held: Vec<u32> = s.document().unwrap().masters.iter().map(|m| m.id).collect();
    assert_eq!(held.len(), 1, "the drop brought one master");
    let made = s.document().unwrap().pages.get(0).unwrap().shapes.iter().max_by_key(|x| x.id).unwrap().clone();
    assert_eq!(
        made.master,
        Some(held[0]),
        "the shape the drop made is an instance of it (of a master of one shape: the link to a shape of it is none)"
    );
    let flat = flat_of(&s, &Flattened::default()).0.unwrap();
    let copy = flat.doc.as_ref().unwrap();
    assert_eq!(copy.masters.iter().map(|m| m.id).collect::<Vec<_>>(), held, "the copy holds the master the drop brought");
    assert!(
        Arc::ptr_eq(&copy.masters.get(0).unwrap().clone(), &s.document().unwrap().masters.get(0).unwrap().clone()),
        "and the very master of the model"
    );
    let shape = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == made.id).unwrap();
    assert_eq!((shape.master, shape.master_shape), (made.master, made.master_shape), "the copy of the shape keeps its links");
    // The shape the drop made is deleted again and the master stays in the model: nothing needs a copy.
    s.execute("shape.delete", &json!({ "ids": [made.id], "page": 0 })).unwrap();
    assert_eq!(s.document().unwrap().masters.len(), 1, "the master stays in the model");
    let given = flat_of(&s, &Flattened::default()).0.unwrap();
    assert!(given.doc.is_none(), "a model that holds no new shape needs no copy, whatever masters it holds");
    let _ = std::fs::remove_dir_all(dir);
}

// Item 3: a guide is given to the writer whole, with no fill and no line.

/// The copy of a guide holds every cell the guide has, and no fill and no line when it holds none (so that the file draws nothing: a shape of no size is a dot in
/// another program, drawn in the root's line); a fill or a line the guide holds is its own and stays; the model's guide is as it was; and nothing is said lost.
#[test]
fn a_guide_is_given_whole_with_no_fill_and_no_line_and_nothing_is_said_lost() {
    let (mut s, dir) = opened(1);
    let made = s.execute("guide.add", &json!({ "page": 0, "orientation": "vertical", "at": 2.0 })).unwrap();
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let guide_of = |doc: &Document| doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    let (flat, _) = flat_of(&s, &Flattened::default());
    let flat = flat.unwrap();
    assert!(flat.made.contains(0, id) && flat.made.len() == 1, "the guide is flattened as a new shape is");
    assert!(flat.notes.is_empty(), "nothing a guide holds is said lost: {:?}", flat.notes);
    let (model, copy) = (guide_of(s.document().unwrap()), guide_of(flat.doc.as_ref().unwrap()));
    let value = |g: &Shape, name: &str| g.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
    assert_eq!((value(&copy, "FillPattern"), value(&copy, "LinePattern")), (Some(0.0), Some(0.0)), "no fill and no line");
    assert_eq!((value(&model, "FillPattern"), value(&model, "LinePattern")), (None, None), "and the model's guide is as it was");
    // Every cell and section the guide holds is in the copy, and the copy holds nothing else but those two cells.
    let names = |g: &Shape| g.sheet.cells.keys().map(|k| k.as_str().to_owned()).collect::<Vec<_>>();
    let mut wanted = names(&model);
    wanted.extend(["FillPattern".to_owned(), "LinePattern".to_owned()]);
    wanted.sort();
    assert_eq!(names(&copy), wanted);
    assert_eq!(copy.sheet.sections, model.sheet.sections);
    assert_eq!((copy.kind.clone(), copy.sheet.links), (diacraft_doc::ShapeType::Guide, model.sheet.links));
    // A guide that holds a fill and a line of its own keeps them.
    s.edit("Test edit", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == id).unwrap();
        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("FillPattern"), Cell::constant(Value::from_text("1")));
        sheet.cells.insert(CellName::new("LinePattern"), Cell::constant(Value::from_text("7")));
        Ok(())
    })
    .unwrap();
    let kept = guide_of(flat_of(&s, &Flattened::default()).0.unwrap().doc.as_ref().unwrap());
    assert_eq!((value(&kept, "FillPattern"), value(&kept, "LinePattern")), (Some(1.0), Some(7.0)));
    let _ = std::fs::remove_dir_all(dir);
}

// The themes a document file carries are read when the file is opened, and the read is bounded.

fn unread_theme(part: &str, bytes: Arc<[u8]>) -> Arc<diacraft_doc::Theme> {
    Arc::new(diacraft_doc::Theme { part: Arc::from(part), bytes, ..diacraft_doc::Theme::default() })
}

/// A theme is read as the VSDX reader reads one (its scheme number, colours and fonts), in the order the file lists them; **a theme past the most themes, and one that
/// would take the bytes past the most, is left as the bytes it is** (and the ones after it that fit are read); a part that is no theme is kept as it is; and a document
/// with no theme costs nothing.
#[test]
fn the_themes_of_a_file_are_read_in_order_within_the_most_themes_and_the_most_bytes() {
    let fern = crate::theme::find("fernwood").unwrap();
    let mut doc = Document::empty();
    assert!(super::read_themes(&mut doc).is_empty());
    assert!(doc.themes.is_empty());
    // One theme: read, and its bytes and part name are as they were.
    doc.themes.push(unread_theme("t/one.xml", Arc::clone(&fern.bytes)));
    let notes = super::read_themes(&mut doc);
    assert!(notes.is_empty(), "a theme that reads whole is no note: {notes:?}");
    let one = doc.themes.get(0).unwrap();
    assert!(one.theme_scheme_id.is_some() && one.colors.is_some(), "read");
    assert_eq!((&*one.part, &one.bytes), ("t/one.xml", &fern.bytes));
    // A part that is no theme is kept as bytes, whole, and does not stop the others.
    let mut doc = Document::empty();
    doc.themes.push(unread_theme("t/none.xml", Arc::from(&b"not xml at all"[..])));
    doc.themes.push(unread_theme("t/two.xml", Arc::clone(&fern.bytes)));
    let notes = super::read_themes(&mut doc);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].part == "t/none.xml" && notes[0].problem.is_some() && notes[0].unread == 0 && notes[0].capped == 0, "{notes:?}");
    assert!(doc.themes.get(0).unwrap().theme_scheme_id.is_none() && &*doc.themes.get(0).unwrap().bytes == b"not xml at all");
    assert!(doc.themes.get(1).unwrap().theme_scheme_id.is_some());
    // The most themes: the first 256 are read and the rest are bytes.
    let most = diacraft_vsdx::MAX_THEMES;
    let mut doc = Document::empty();
    for k in 0..most + 2 {
        doc.themes.push(unread_theme(&format!("t/{k}.xml"), Arc::clone(&fern.bytes)));
    }
    let notes = super::read_themes(&mut doc);
    let past: Vec<(&str, Option<&str>)> = notes.iter().map(|n| (n.part.as_str(), n.problem)).collect();
    assert_eq!(past, [("t/256.xml", Some("past_the_most_themes_or_bytes")), ("t/257.xml", Some("past_the_most_themes_or_bytes"))]);
    let read: Vec<bool> = doc.themes.iter().map(|t| t.theme_scheme_id.is_some()).collect();
    assert!(read[..most].iter().all(|r| *r) && read[most..].iter().all(|r| !*r), "256 read and 2 left: {} read", read.iter().filter(|r| **r).count());
    // The most bytes: a theme padded to 3.5 MiB is read; one of a MiB after it is not (it would pass 4 MiB), and a small one after that is.
    let padded = |extra: usize| -> Arc<[u8]> {
        let mut v = fern.bytes.to_vec();
        v.extend(std::iter::repeat_n(b' ', extra));
        Arc::from(v)
    };
    let mut doc = Document::empty();
    doc.themes.push(unread_theme("t/big.xml", padded(3 * 1024 * 1024 + 512 * 1024)));
    doc.themes.push(unread_theme("t/more.xml", padded(1024 * 1024)));
    doc.themes.push(unread_theme("t/small.xml", Arc::clone(&fern.bytes)));
    let notes = super::read_themes(&mut doc);
    assert_eq!(notes.iter().map(|n| n.part.as_str()).collect::<Vec<_>>(), ["t/more.xml"], "only the theme past the room is a note");
    let read: Vec<bool> = doc.themes.iter().map(|t| t.theme_scheme_id.is_some()).collect();
    assert_eq!(read, [true, false, true], "the bytes are held to what the VSDX reader holds themes to");
    // A theme exactly at the room is read, and one a byte over is not.
    let room = usize::try_from(diacraft_vsdx::MAX_THEME_BYTES).unwrap();
    for (extra, expect) in [(room - fern.bytes.len(), true), (room - fern.bytes.len() + 1, false)] {
        let mut doc = Document::empty();
        doc.themes.push(unread_theme("t/edge.xml", padded(extra)));
        super::read_themes(&mut doc);
        assert_eq!(doc.themes.get(0).unwrap().theme_scheme_id.is_some(), expect, "{extra} bytes of padding");
    }
}

// What the copy of a new group is (the survivors of the mutation run of `Flattener::group_at`).

/// A cell holding the number `v` in inches.
fn inches(v: &str) -> Cell {
    Cell::constant(Value::from_text(v)).with_unit("IN")
}

/// A new group on the first page, `900`, holding the members `901` (with a shadow, which the writer does not write) and `902`, with the frame cells, a flip, an angle of
/// zero, a cell of its own, and a width that is a formula.
fn with_a_new_group(s: &mut Session) {
    s.edit("Test group", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let member = |id: u32, shadow: bool| {
            let mut m = Shape::new(id);
            let sheet = Arc::make_mut(&mut m.sheet);
            for (name, v) in [("PinX", "1"), ("PinY", "1"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5")] {
                sheet.cells.insert(CellName::new(name), inches(v));
            }
            if shadow {
                sheet.cells.insert(CellName::new("ShdwPattern"), Cell::constant(Value::from_text("1")));
            }
            Arc::new(m)
        };
        let mut group = Shape::new(900);
        group.kind = diacraft_doc::ShapeType::Group;
        let sheet = Arc::make_mut(&mut group.sheet);
        sheet.cells.insert(CellName::new("Width"), Cell::with_formula("Sheet.901!Width*2", Some(Value::from_text("4"))).with_unit("IN"));
        for (name, v) in [("Height", "2"), ("PinX", "5"), ("PinY", "5"), ("LocPinX", "2"), ("LocPinY", "1")] {
            sheet.cells.insert(CellName::new(name), inches(v));
        }
        sheet.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_text("0")));
        sheet.cells.insert(CellName::new("FlipX"), Cell::constant(Value::from_text("1")));
        sheet.cells.insert(CellName::new("Cost"), Cell::constant(Value::from_text("42")));
        group.children = diacraft_doc::ShapeList::from_vec(vec![member(901, true), member(902, false)]);
        page.shapes.push(Arc::new(group));
        Ok(())
    })
    .unwrap();
}

/// The copy of a new group: its links are the root style's, its frame cells are constants of what they resolve to (a width that is a formula is its value), a flip is
/// carried and an angle of zero is not, a cell of its own is as it is, and what a member lost is the group's.
#[test]
fn a_new_group_is_copied_with_its_frame_as_constants_its_own_cells_as_they_are_and_what_its_members_lost() {
    let (mut s, dir) = opened(1);
    with_a_new_group(&mut s);
    let (flat, _) = flat_of(&s, &Flattened::default());
    let flat = flat.unwrap();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let copy = flat.doc.as_ref().expect("a new group is copied");
    let group = copy.pages.get(0).unwrap().shapes.iter().find(|x| x.id == 900).unwrap().clone();
    let links = group.sheet.links;
    assert_eq!((links.line, links.fill, links.text), (Some(0), Some(0), Some(0)), "the links of the root style");
    let width = group.sheet.cell("Width").expect("the width is carried");
    assert!(width.formula.text().is_none(), "as a constant: {:?}", width.formula.text());
    assert_eq!(width.value.as_ref().and_then(Value::number), Some(4.0), "of what it resolves to");
    assert_eq!(group.sheet.cell("Cost").and_then(|c| c.value.as_ref().and_then(Value::number)), Some(42.0), "a cell of its own is as it is");
    assert!(group.sheet.cell("FlipX").is_some(), "a flip is carried");
    assert!(group.sheet.cell("Angle").is_none(), "an angle of zero is not");
    assert_eq!(group.children.len(), 2, "and its members");
    assert_eq!(
        flat.made.lost(page, 900),
        Some(0),
        "nothing is lost by the group: the shadow of the first member is the member's own cell, which the copy carries"
    );
    let first = group.children.get(0).unwrap();
    assert!(first.sheet.cell("ShdwPattern").is_some(), "the member's shadow is carried");
    assert_eq!(flat.made.lost(page, 902), None, "(a member has no entry of its own)");
    let _ = std::fs::remove_dir_all(dir);
}

/// A chain of `levels` new groups with a shape at the bottom, on the first page.
fn chain_of_new_groups(s: &mut Session, levels: usize) {
    s.edit("Test chain", |doc: &mut Document| {
        let mut inner = Arc::new(Shape::new(5_000));
        for level in 0..levels {
            let mut group = Shape::new(u32::try_from(4_000 + level).unwrap());
            group.kind = diacraft_doc::ShapeType::Group;
            group.children = diacraft_doc::ShapeList::from_vec(vec![inner]);
            inner = Arc::new(group);
        }
        Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.push(inner);
        Ok(())
    })
    .unwrap();
}

/// A new group is copied to the depth the model allows and a chain past it is refused in words, at the group (not walked on).
#[test]
fn a_chain_of_new_groups_is_copied_to_the_depth_of_the_model_and_one_past_it_is_refused() {
    let most = diacraft_doc::MAX_SHAPE_DEPTH;
    for (levels, ok) in [(most - 1, true), (most, true), (most + 1, false)] {
        let (mut s, dir) = opened(1);
        chain_of_new_groups(&mut s, levels);
        let (flat, _) = flat_of(&s, &Flattened::default());
        match (flat, ok) {
            (Ok(flat), true) => assert!(flat.doc.is_some(), "{levels} groups: copied"),
            (Err(words), false) => assert!(words.contains("nested deeper than the model allows"), "{levels} groups: {words}"),
            (Ok(_), false) => panic!("{levels} groups: not refused"),
            (Err(words), true) => panic!("{levels} groups: refused: {words}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A rectangle that is dropped has no gradient, so its copy holds no `FillGradientEnabled` (a cell of 0 is the reader's own default and is no cell of the copy):
/// the cell is put only when the shape resolves it to something and the copy does not leave it to a style it keeps the links to.
#[test]
fn the_copy_of_a_dropped_rectangle_holds_no_gradient_cell() {
    let (mut s, dir) = opened(2);
    drop_rectangles(&mut s, 1);
    let flat = flat_of(&s, &Flattened::default()).0.unwrap();
    let doc = flat.doc.as_ref().expect("a shape was flattened");
    let page = doc.pages.get(0).unwrap();
    let copy = page.shapes.get(page.shapes.len() - 1).unwrap();
    assert!(copy.sheet.cell("FillGradientEnabled").is_none(), "no cell for a gradient the shape does not have");
    assert!(copy.sheet.cell("FillForegnd").is_some() && copy.sheet.cell("FillPattern").is_some(), "and the fill it has");
    // A shape that holds the cell, at 0, is no different: the cell is put only when it is not 0, and a 1 is put.
    for (value, put) in [("0", false), ("1", true)] {
        s.edit("Test setup: a gradient cell", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let last = page.shapes.len() - 1;
            let shape = Arc::make_mut(page.shapes.get_mut(last).unwrap());
            Arc::make_mut(&mut shape.sheet)
                .cells
                .insert(diacraft_doc::CellName::new("FillGradientEnabled"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(value)));
            Ok(())
        })
        .unwrap();
        let flat = flat_of(&s, &Flattened::default()).0.unwrap();
        let doc = flat.doc.as_ref().expect("a shape was flattened");
        let page = doc.pages.get(0).unwrap();
        let copy = page.shapes.get(page.shapes.len() - 1).unwrap();
        assert_eq!(copy.sheet.cell("FillGradientEnabled").is_some(), put, "the cell at {value}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A new group of `id` holding one new shape of `id + 1`, called `name`.
fn named_group(id: u32, name: Option<&str>) -> Arc<Shape> {
    let mut member = Shape::new(id + 1);
    for (cell, v) in [("PinX", "1"), ("PinY", "1"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5")] {
        Arc::make_mut(&mut member.sheet).cells.insert(CellName::new(cell), inches(v));
    }
    let mut group = Shape::new(id);
    group.kind = diacraft_doc::ShapeType::Group;
    group.name_u = name.map(Arc::from);
    group.name = group.name_u.clone();
    group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(member)]);
    Arc::new(group)
}

/// The names of the groups of the copy of the page the session's model has, in the order of the page (a group of the file has its own name).
fn copy_names(s: &Session) -> Vec<Option<String>> {
    let flat = flat_of(s, &Flattened::default()).0.unwrap();
    let copy = flat.doc.as_ref().expect("a copy");
    copy.pages.get(0).unwrap().shapes.iter().filter(|x| !x.children.is_empty()).map(|g| g.name_u.as_deref().map(str::to_owned)).collect()
}

/// The review: a new group that has no name, or one another group of the page has, is named `Group_<ID>` in the copy (`Group_<ID>_2` and so on when a
/// group of the page has that name already), and the name is the key its entry is found by; a new group with a name no other group has keeps it; a group of the file is
/// never renamed; a page with no new group costs nothing.
#[test]
fn a_new_group_with_no_name_or_a_name_another_group_has_is_named_after_its_id_in_the_copy() {
    let put = |groups: Vec<Arc<Shape>>| {
        let (mut s, dir) = opened(1);
        s.edit("Test groups", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            for g in groups {
                page.shapes.push(g);
            }
            Ok(())
        })
        .unwrap();
        (s, dir)
    };
    let some = |n: &str| Some(n.to_owned());
    // No name: named after its `ID`. A name of its own that no other group has: kept.
    let (s, dir) = put(vec![named_group(900, None), named_group(910, Some("Mine"))]);
    assert_eq!(copy_names(&s), [some("Group_900"), some("Mine")]);
    let _ = std::fs::remove_dir_all(dir);
    // Two of one name: both are named after their `ID`s. An empty name is none.
    let (s, dir) = put(vec![named_group(900, Some("Twin")), named_group(910, Some("Twin")), named_group(920, Some(""))]);
    assert_eq!(copy_names(&s), [some("Group_900"), some("Group_910"), some("Group_920")]);
    let _ = std::fs::remove_dir_all(dir);
    // A group of the page already called `Group_900` (it is the first of two of one name here, so that it is not kept): the new group takes the next name.
    let (s, dir) = put(vec![named_group(800, Some("Group_900")), named_group(810, Some("Group_900")), named_group(900, None)]);
    assert_eq!(copy_names(&s)[2], some("Group_900_2"), "a name a group of the page has is not given again");
    let _ = std::fs::remove_dir_all(dir);
    // A new group nested in a new group is named too, once each.
    let (mut s, dir) = put(vec![named_group(900, None)]);
    s.edit("Test nesting", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let top = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        top.children = diacraft_doc::ShapeList::from_vec(vec![named_group(950, None), Arc::clone(top.children.get(0).unwrap())]);
        Ok(())
    })
    .unwrap();
    let flat = flat_of(&s, &Flattened::default()).0.unwrap();
    let group = flat.doc.as_ref().unwrap().pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    assert_eq!(group.name_u.as_deref(), Some("Group_900"));
    assert_eq!(group.children.get(0).unwrap().name_u.as_deref(), Some("Group_950"), "the group inside the group");
    let _ = std::fs::remove_dir_all(dir);
    // A page with no new group: no group is named, and no step is taken for it.
    let (mut s, dir) = opened(2);
    let (_, plain) = flat_of(&s, &Flattened::default());
    drop_rectangles(&mut s, 1);
    let (flat, with_one) = flat_of(&s, &Flattened::default());
    assert!(flat.unwrap().doc.is_some());
    assert!(with_one > plain, "{plain} {with_one}");
    let _ = std::fs::remove_dir_all(dir);
}

/// What naming the groups costs: a step for each shape of a page that has a new group, and nothing for a page that has none; the names are in proportion.
#[test]
fn naming_the_new_groups_of_a_page_costs_a_step_for_each_shape_of_the_page_and_nothing_for_a_page_with_none() {
    let steps_with = |groups: usize| {
        let (mut s, dir) = opened(1);
        s.edit("Test groups", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            for k in 0..groups {
                page.shapes.push(named_group(1_000 + 10 * u32::try_from(k).unwrap(), None));
            }
            Ok(())
        })
        .unwrap();
        let steps = flat_of(&s, &Flattened::default()).1;
        let _ = std::fs::remove_dir_all(dir);
        steps
    };
    let (one, two, four) = (steps_with(1), steps_with(2), steps_with(4));
    assert_eq!((two - one) * 2, four - two, "in proportion to the groups: {one} {two} {four}");
}

/// A cell that only the master holds (here `Rounding`) is kept by the link and is not copied; the same cell on the shape itself is carried as the shape has it.
/// (No master of the built-in stencils holds a `Rounding` or a shadow, so no other test reached it.)
#[test]
fn a_rounding_only_the_master_holds_is_kept_by_the_link_and_one_the_shape_holds_is_carried() {
    let (mut s, dir) = opened(1);
    s.edit("Test setup: a master with a rounding and instances of it", |doc: &mut Document| {
        let mut inner = Shape::new(1);
        for (name, v) in [("PinX", "1"), ("PinY", "1"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5"), ("Rounding", "0.2")] {
            Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new(name), inches(v));
        }
        doc.masters.push(Arc::new(diacraft_doc::Master {
            id: 77,
            shapes: diacraft_doc::ShapeList::from_vec(vec![Arc::new(inner)]),
            ..diacraft_doc::Master::default()
        }));
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        // 900 holds nothing of its own; 910 holds a rounding of 0 of its own (it wins over the master's); 920 holds one of 0.3.
        for (id, own) in [(900u32, None), (910, Some("0")), (920, Some("0.3"))] {
            let mut shape = Shape::new(id);
            shape.master = Some(77);
            for (name, v) in [("PinX", "5"), ("PinY", "5"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5")] {
                Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), inches(v));
            }
            if let Some(v) = own {
                Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Rounding"), inches(v));
            }
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
    let flat = flat_of(&s, &Flattened::default()).0.unwrap();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let copy = flat.doc.as_ref().unwrap().pages.get(0).unwrap();
    let rounding = |id: u32| copy.shapes.iter().find(|x| x.id == id).unwrap().sheet.cell("Rounding").and_then(|c| c.value.as_ref()?.number());
    for id in [900, 910, 920] {
        assert_eq!(flat.made.lost(page, id), Some(0), "nothing is lost: {id}");
    }
    assert_eq!(rounding(900), None, "the master's rounding is kept by the link: the copy holds none");
    assert_eq!(rounding(910), Some(0.0), "the shape's own rounding of 0 wins over the master's, and is carried");
    assert!(rounding(920).is_some_and(|r| (r - 0.3).abs() < 1e-9), "the shape's own rounding is carried");
    let _ = std::fs::remove_dir_all(dir);
}
