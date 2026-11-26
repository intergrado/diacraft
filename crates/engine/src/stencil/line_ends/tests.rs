//! The filled diamond and what a connector that uses it does (DD18).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, FormulaSlot, Master, Owner, Resolver, SheetRef};
use diacraft_render::{Renderer, View};

use super::super::*;
use crate::Session;
use crate::draw;

/// The side of the diamond's alignment box, `h`.
const H: f64 = 0.05;

fn diamond() -> &'static MasterSpec {
    find_any("line-end.filled-diamond").expect("the filled diamond is a built-in master")
}

/// The diamond built into a blank document: the document and the master.
fn built() -> (Document, Master) {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, diamond()).unwrap();
    let master = (**doc.master(id).unwrap()).clone();
    (doc, master)
}

/// What a cell of the master's shape holds: the number, and the formula text when it has one.
fn cell_of(master: &Master, name: &str) -> (Option<f64>, Option<String>) {
    let shape = master.shapes.get(0).unwrap();
    let cell = shape.sheet.cell(name).unwrap_or_else(|| panic!("the master's shape holds no {name}"));
    (cell.value.as_ref().and_then(|v| v.number()), cell.formula.text().map(str::to_owned))
}

#[test]
fn the_filled_diamond_is_a_hidden_line_end_master_of_series_22_that_is_in_no_stencil() {
    let spec = diamond();
    assert_eq!((spec.id, spec.name), ("line-end.filled-diamond", "Filled Diamond"));
    assert!(spec.is_hidden(), "A master that people see only as a line end");
    assert_eq!(spec.guid(), "{D1AC0016-0000-4000-8000-000000000001}");
    assert!(
        all_masters().all(|m| m.id != spec.id) && find_master("Filled Diamond").is_none(),
        "in no stencil, so no listing, search or drop finds it"
    );
    assert!(!spec.is_connector() && spec.point_count() == 0, "a figure drawn at the end of a line has no connection point of its own");
    // The master reaches a drawing as a hidden master.
    let (_, master) = built();
    assert_eq!((master.name.as_deref(), master.name_u.as_deref()), (Some("Filled Diamond"), Some("Filled Diamond")));
    assert_eq!(master.raw.get("Hidden"), Some("1"));
    assert_eq!(master.unique_id.as_deref(), Some("{D1AC0016-0000-4000-8000-000000000001}"));
}

#[test]
fn the_filled_diamond_is_a_line_end_pattern_pointing_right_with_its_pin_on_its_tip() {
    let (doc, master) = built();
    // A master is a line end when its `PatternFlags` holds 2; no other bit says upright or scaled.
    assert_eq!(master.pattern_flags, Some(2));
    // A square alignment box `h`, its pin at the middle of its right edge.
    let ((width, _), (height, _)) = (cell_of(&master, "Width"), cell_of(&master, "Height"));
    assert_eq!((width, height), (Some(H), Some(H)), "Width equals Height");
    assert_eq!(cell_of(&master, "LocPinX"), (Some(H), Some("Width".to_owned())), "the pin is on the right edge");
    assert_eq!(cell_of(&master, "LocPinY"), (Some(H / 2.0), Some("Height*0.5".to_owned())), "and halfway up it");
    // One closed filled section that draws no outline: a diamond pointing right, its tip on the pin, its back 20 h behind the tip, its sides 10 h behind and 5 h across.
    let shape = master.shapes.get(0).unwrap();
    let geometry: Vec<_> = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").collect();
    assert_eq!(geometry.len(), 1, "one section");
    let section = geometry[0];
    let flag = |name: &str| section.cells.get(&diacraft_doc::CellName::new(name)).and_then(|c| c.value.as_ref()).and_then(|v| v.number());
    assert_eq!((flag("NoFill"), flag("NoLine"), flag("NoShow")), (Some(0.0), Some(1.0), Some(0.0)), "filled, and no outline");
    let rows: Vec<(String, f64, f64)> = section
        .rows
        .iter()
        .map(|r| {
            let at = |c: &str| r.cells.get(&diacraft_doc::CellName::new(c)).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
            (r.row_type.as_deref().unwrap().to_owned(), at("X"), at("Y"))
        })
        .collect();
    let want = [
        ("MoveTo", H, H / 2.0),
        ("LineTo", -9.0 * H, 5.5 * H),
        ("LineTo", -19.0 * H, H / 2.0),
        ("LineTo", -9.0 * H, -4.5 * H),
        ("LineTo", H, H / 2.0),
    ];
    assert_eq!(rows.len(), want.len());
    for ((kind, x, y), (want_kind, want_x, want_y)) in rows.iter().zip(want) {
        assert!(
            kind == want_kind && (x - want_x).abs() < 1e-9 && (y - want_y).abs() < 1e-9,
            "{kind} ({x}, {y}) against {want_kind} ({want_x}, {want_y})"
        );
    }
    // The tip is the pin, and the figure is twice as long as it is wide.
    let (tip_x, tip_y) = (rows[0].1, rows[0].2);
    assert!((tip_x - H).abs() < 1e-9 && (tip_y - H / 2.0).abs() < 1e-9);
    let (back, up, down) = (rows[2].1, rows[1].2, rows[3].2);
    assert!(((tip_x - back) / (up - down) - 2.0).abs() < 1e-9, "20 h long and 10 h across");
    // The vertices are formulas of the box, so that the figure follows any size the master is given.
    let formula = |row: usize, name: &str| {
        let cell = section.rows[row].cells.get(&diacraft_doc::CellName::new(name)).unwrap();
        match &cell.formula {
            FormulaSlot::Local(f) => Some(f.source.to_string()),
            _ => None,
        }
    };
    assert_eq!(formula(2, "X").as_deref(), Some("Width-Height*20"));
    assert_eq!(formula(1, "Y").as_deref(), Some("Height*5.5"));
    // The master is in the document it was built into, once.
    assert_eq!(doc.masters.len(), 1);
}

#[test]
fn only_the_hidden_line_ends_hold_a_pattern_flag_and_every_other_master_holds_none() {
    // A master is a line end only when its flags say so; a drawing that holds a master of the catalogue must not find a stencil figure used as a line end.
    for spec in all_masters() {
        let mut doc = Document::new_blank();
        let id = ensure_master(&mut doc, spec).unwrap();
        assert_eq!(doc.master(id).unwrap().pattern_flags, Some(0), "{}: no flag", spec.id);
    }
    for spec in line_ends::MASTERS.iter() {
        let mut doc = Document::new_blank();
        let id = ensure_master(&mut doc, spec).unwrap();
        assert_eq!(doc.master(id).unwrap().pattern_flags, Some(2), "{}: a line end", spec.id);
    }
}

/// A connector master of the tests: a solid line whose begin is the filled diamond, as UML composition's will be.
static COMPOSITION_MORE: More = More { dress: None, hidden: false, brings: &["line-end.filled-diamond"] };
static COMPOSITION: MasterSpec = MasterSpec {
    id: "t.composition",
    name: "Test Composition",
    width: 2.0,
    height: 0.0,
    identity: (200, 30),
    outline: Outline::Connector(ConnectorLook::plain(LineEnd::Master { id: "line-end.filled-diamond", name: "Filled Diamond" }, LineEnd::None)),
    points: &[],
    more: &COMPOSITION_MORE,
};

#[test]
fn a_connector_whose_begin_names_the_filled_diamond_brings_the_master_into_the_document_and_saves_it_as_a_line_end_pattern() {
    // Dropping the connector copies the diamond in first (a master it brings), and the connector's begin names it by `USE`.
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, &COMPOSITION).unwrap();
    let names: Vec<&str> = doc.masters.iter().filter_map(|m| m.name.as_deref()).collect();
    assert_eq!(names, ["Filled Diamond", "Test Composition"], "the line end comes first");
    assert_eq!(doc.master(id).unwrap().pattern_flags, Some(0));
    let begin = doc.master(id).unwrap().shapes.get(0).unwrap().sheet.cell("BeginArrow").unwrap().clone();
    assert_eq!(begin.formula.text(), Some("USE(\"Filled Diamond\")"));
    assert_eq!(begin.value.as_ref().and_then(|v| v.number()), Some(254.0), "the cell holds the custom end's number");
    assert_eq!(ensure_master(&mut doc, &COMPOSITION).unwrap(), id, "found again, and the diamond is not brought twice");
    assert_eq!(doc.masters.len(), 2);
    // Saved as a drawing file, the diamond's master row says it is a line end (`PatternFlags` 2) and is hidden, and a file read again holds the same.
    let bytes = diacraft_vsdx::write(&doc, None).unwrap();
    let (back, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let find = |name: &str| back.masters.iter().find(|m| m.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("the file holds {name}"));
    assert_eq!(find("Filled Diamond").pattern_flags, Some(2), "a line end pattern");
    assert_eq!(find("Filled Diamond").raw.get("Hidden"), Some("1"));
    assert_eq!(find("Test Composition").pattern_flags, Some(0));
    let again = find("Test Composition").shapes.get(0).unwrap().sheet.cell("BeginArrow").unwrap().formula.text().map(str::to_owned);
    assert_eq!(again.as_deref(), Some("USE(\"Filled Diamond\")"), "the connector still names the line end");
}

#[test]
fn a_dropped_connector_draws_the_filled_diamond_at_its_begin_pointing_out_of_the_line() {
    // The composition and V18 together, as the engine puts them: the connector is dropped, and its begin shows a diamond 20 box heights long (0.2 inch at the blank
    // document's weight of 0.01 inch), 0.1 inch across, whose tip is the begin point. A plain line there is 0.01 inch wide.
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, &COMPOSITION, 3.0, 4.0).unwrap();
    let at = SheetRef::Shape(doc.find_shape(Owner::Page(page), info.id).unwrap());
    let res = Resolver::new(&doc);
    let cell = |name: &str| {
        res.resolve(&at, &CellAddr::direct(name)).and_then(|r| r.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())).unwrap()
    };
    let (bx, by, ex) = (cell("BeginX"), cell("BeginY"), cell("EndX"));
    let weight = cell("LineWeight");
    drop(res);
    let along = if ex > bx { 1.0 } else { -1.0 };
    let (ppi, height) = (200.0, doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap());
    let view = View::fit_page(&doc, page, ppi).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(&doc, page, &view);
    assert!(frame.skipped.is_empty(), "{:?}", frame.skipped);
    let dark = |x: f64, y: f64| {
        let (col, row) = ((x * ppi) as usize, ((height - y) * ppi) as usize);
        let i = (row * frame.width as usize + col) * 4;
        let p = &frame.rgba_premul[i..i + 4];
        p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300
    };
    // The box is as tall as the weight: 20 weights (0.2 inch) long, 10 across; the half height 0.1 inch from the tip is 0.05.
    assert!((weight - 0.01).abs() < 1e-9, "the connector takes the blank document's weight of 0.01 inch: {weight}");
    let near = 0.1;
    assert!(
        dark(bx + along * near, by + 0.04) && dark(bx + along * near, by - 0.04),
        "the diamond is 0.08 inch across 0.1 inch from its tip: more than the line is"
    );
    assert!(!dark(bx + along * near, by + 0.07), "and not 0.14");
    assert!(!dark(bx - along * 0.03, by), "nothing before the begin point: the tip is on it");
}

#[test]
fn a_dropped_connector_with_the_filled_diamond_is_saved_as_an_odg_with_the_diamonds_marker_and_reads_back_with_its_use() {
    // The engine's copy of a new connector for an `.odg` keeps the `USE` of its custom end (it holds the end as a constant of the number otherwise), so that the writer knows the master
    // by its name and writes the diamond's marker, and a file read again still draws the line from its master and not as number 4.
    use super::super::flowchart_tests::{Dir, render as render_session, run};
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    draw::drop_shape(&mut doc, page, &COMPOSITION, 3.0, 4.0).unwrap();
    let mut session = Session::new();
    session.replace_document("test", doc).unwrap();
    let before = render_session(&session);
    assert!(before.skipped.is_empty(), "{:?}", before.skipped);
    let dir = Dir::new();
    let path = dir.path("composition.odg");
    run(&mut session, "file.save_as", serde_json::json!({ "path": path }));
    let bytes = std::fs::read(&path).unwrap();
    let package = diacraft_odg::package::Package::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
    let part = |name: &str| String::from_utf8(package.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap();
    let (content, styles) = (part("content.xml"), part("styles.xml"));
    assert!(content.contains("draw:marker-start=\"DiaCraft_20_end_20_254\""), "the begin names the diamond's marker: {content}");
    assert!(!content.contains("draw:marker-end="), "and the end has none: {content}");
    assert!(styles.contains("svg:d=\"M 500 0 L 1000 1000 L 500 2000 L 0 1000 Z\""), "the diamond is in office:styles: {styles}");
    let mut again = Session::new();
    run(&mut again, "file.open", serde_json::json!({ "path": path }));
    let reopened = again.document().unwrap();
    let own = reopened
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find_map(|s| s.sheet.cell("BeginArrow").cloned())
        .expect("the connector holds its own BeginArrow after a save and a reopen");
    assert_eq!(own.formula.text(), Some("USE(\"Filled Diamond\")"));
    assert_eq!(own.value.as_ref().and_then(|v| v.number()), Some(254.0));
    assert!(
        reopened.masters.iter().any(|m| m.name.as_deref() == Some("Filled Diamond") && m.pattern_flags == Some(2)),
        "the master is in the file as a line end"
    );
    let after = render_session(&again);
    assert!(after.skipped.iter().all(|s| s.kind != diacraft_render::SkipKind::CustomLineEnd), "drawn from its master: {:?}", after.skipped);
}

/// `DIACRAFT_CUSTOM_ENDS_ODG=/absolute/path.odg cargo nextest run ... the_odg_of_a_dropped_composition` writes the `.odg` of a connector with the diamond at its begin, 3 points heavy, to look at in
/// another program (what the marker draws there is the thing to see).
#[test]
fn the_odg_of_a_dropped_composition_can_be_written_to_look_at() {
    use super::super::flowchart_tests::{Dir, run};
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, &COMPOSITION, 3.0, 4.0).unwrap();
    let mut session = Session::new();
    session.replace_document("test", doc).unwrap();
    run(&mut session, "shape.format", serde_json::json!({ "ids": [info.id], "weight": 3.0 }));
    let dir = Dir::new();
    let path = dir.path("composition.odg");
    run(&mut session, "file.save_as", serde_json::json!({ "path": path }));
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
    if let Ok(out) = std::env::var("DIACRAFT_CUSTOM_ENDS_ODG") {
        std::fs::copy(&path, out).unwrap();
    }
}
