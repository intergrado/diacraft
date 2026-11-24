//! What a pass over the whole page gives the instances on it, on every save path.
//!
//! **The cause.** An edit that changes something outside the page's own shapes (a style, the theme, a master's formulas) is one `calc` cannot follow
//! cell by cell: the page is worked out whole. That pass used to give **every instance on the page** each cell it inherits from its master's formulas
//! that came out other than the master's saved value, whether or not the edit had anything to do with it: the cell a master's
//! formula gives another value on an instance of another size (`LocPinX`, a row of `Geometry`), a theme colour this evaluator works out otherwise than
//! the file, and the very cell the edit changed, which every instance then held a copy of because **the master itself was not worked out**: its saved
//! value was the one from before the edit. The pass is now told the document the edit started from (`Changes::since`), works out the cells of the masters
//! the page's instances use that the edit changed, and gives an instance a cell only where the edit changed what the cell works out to there and the
//! result is not the master's (`calc/src/commit.rs`). These tests hold each save path to it: the VSDX file saved straight after the edit (the splice
//! writer), the `.diacraft` file, and the VSDX file the model writer makes from it.

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, FormulaSlot, Lookup, Owner, Resolver, RowKey, ShapeRef, SheetRef, WriteOrigin};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};
use diacraft_testkit::theme::ThemeXml;
use serde_json::json;

use crate::native_common::{Dir, opened};
use crate::run;

/// A point in inches.
const POINT: f64 = 1.0 / 72.0;

/// A line style whose weight is a formula: a point.
const STYLE: &str = r#"<StyleSheet ID="3" NameU="Thick" Name="Thick" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.01388888888888889" F="1 pt"/></StyleSheet>"#;

/// A master whose shape takes its line from that style and holds the weight as files do (`F="Inh"` with the value), a fill under the theme call, its
/// pin, a row of geometry and a user cell as formulas of its size, saved for its own size (1 by 0.5).
const MASTER_SHAPE: &str = concat!(
    r#"<Shape ID="5" Type="Shape" LineStyle="3" FillStyle="3" TextStyle="3"><Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.25"/>"#,
    r#"<Cell N="Width" V="1"/><Cell N="Height" V="0.5"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.25" F="Height*0.5"/>"#,
    r#"<Cell N="LineWeight" V="0.01388888888888889" F="Inh"/>"#,
    r##"<Cell N="FillForegnd" V="#abcdef" F="THEMEVAL(&quot;FillColor&quot;)"/>"##,
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
    r#"<Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#,
    r#"<Section N="User"><Row N="Span"><Cell N="Value" V="1" F="Width"/></Row></Section></Shape>"#,
);

/// Two instances: shape 2 at the master's size, and shape 3 twice its width, which leaves every row to the master (a file another program saved may be
/// so: what the master's formulas give it differs from what the master saved, and the file has no local row for it).
const INSTANCES: &str = concat!(
    r#"<Shape ID="2" Master="2" Type="Shape"><Cell N="PinX" V="4"/><Cell N="PinY" V="4"/></Shape>"#,
    r#"<Shape ID="3" Master="2" Type="Shape"><Cell N="PinX" V="6"/><Cell N="PinY" V="4"/><Cell N="Width" V="2"/></Shape>"#,
);

fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .more_styles(STYLE)
        .theme(ThemeXml::default().xml().as_bytes())
        .master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE))
        .page(PageSpec::new(0, "P").shapes(INSTANCES))
        .build()
}

/// The same drawing with a master whose shape links no style: `theme.apply` makes its instances follow the `Theme` style by links of their own.
fn drawing_of_an_unlinked_master() -> Vec<u8> {
    let master = MASTER_SHAPE.replace(r#" LineStyle="3" FillStyle="3" TextStyle="3""#, "");
    assert_ne!(master, MASTER_SHAPE);
    VsdxBuilder::new()
        .theme(ThemeXml::default().xml().as_bytes())
        .master(MasterSpec::new(2, "Box").shapes(&master))
        .page(PageSpec::new(0, "P").shapes(INSTANCES))
        .build()
}

fn span() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("Span"), "Value")
}

/// What shape `id` of the first page holds itself: its cells directly under it, by name, and its sections, by name and number of rows.
fn own(doc: &Document, id: u32) -> (Vec<String>, Vec<String>) {
    let page = doc.pages.iter().next().unwrap();
    let shape = page.shapes.iter().find(|s| s.id == id).unwrap();
    let cells = shape.sheet.cells.keys().map(|n| n.as_str().to_owned()).collect();
    let sections = shape.sheet.sections.iter().map(|s| format!("{}:{}", s.name.as_str(), s.rows.len())).collect();
    (cells, sections)
}

/// The value a reader of shape `id`'s cell finds, through inheritance: what the window and the renderer show.
fn shown(doc: &Document, id: u32, addr: &CellAddr) -> f64 {
    let page = doc.pages.iter().next().unwrap().id;
    let at = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
    let found = Resolver::new(doc).resolve(&at, addr).unwrap();
    found.value.cell.and_then(|c| c.value.as_ref()).unwrap().text().parse().unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn read(bytes: &[u8]) -> Document {
    diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap().0
}

/// The edit `f` makes to the document, then the recalculation every editing command makes after its writes (`draw::settle`).
fn edit(s: &mut Session, f: impl FnOnce(&mut Document)) {
    s.edit("an edit outside the page", |doc| {
        let before = doc.clone();
        f(doc);
        diacraft_engine::draw::settle(doc, &before, 0, &[], &[]).map_err(|e| EngineError::Internal { cmd: "test".into(), msg: format!("{e:?}") })
    })
    .unwrap();
}

/// The drawing the session holds after the edit, and the three saves of it: straight to VSDX (the splice path), as `.diacraft` (the model as reopened),
/// and from that, in a session of its own, to VSDX (the model writer).
struct Saved {
    session: Document,
    splice: Document,
    diacraft: Document,
    model: Document,
}

fn saved_after(source: &[u8], make: impl FnOnce(&mut Session)) -> Saved {
    let dir = Dir::new();
    let mut s = opened(&dir.write("a.vsdx", source));
    make(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("splice.vsdx") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let session = s.document().unwrap().clone();
    let mut t = opened(&dir.path("x.diacraft"));
    let diacraft = t.document().unwrap().clone();
    run(&mut t, "file.save_as", json!({ "path": dir.path("model.vsdx") }));
    Saved { session, splice: read(&dir.read("splice.vsdx")), diacraft, model: read(&dir.read("model.vsdx")) }
}

impl Saved {
    fn each(&self) -> [(&'static str, &Document); 4] {
        [("the session", &self.session), ("the splice save", &self.splice), ("the .diacraft file", &self.diacraft), ("the model save", &self.model)]
    }
}

/// What the source's instances hold themselves, read from the source file.
fn as_read(source: &[u8]) -> [(Vec<String>, Vec<String>); 2] {
    let doc = read(source);
    [own(&doc, 2), own(&doc, 3)]
}

#[test]
fn a_style_edited_gives_no_instance_a_cell_or_a_row_it_did_not_have_on_any_save_path() {
    let saved = saved_after(&drawing(), |s| {
        edit(s, |doc| {
            let three = Arc::new(diacraft_formula::parse("3 pt"));
            doc.write_cell(
                &SheetRef::Style(3),
                &CellAddr::direct("LineWeight"),
                CellEdit::Formula { formula: three, value: None },
                WriteOrigin::Drawing,
            )
            .unwrap();
        })
    });
    let [two, three] = as_read(&drawing());
    for (path, doc) in saved.each() {
        assert_eq!(own(doc, 2), two, "{path}: the instance at its master's size was given cells");
        assert_eq!(own(doc, 3), three, "{path}: the instance of another size was given cells the edit did not change");
        // The new weight reaches both through their master, whose cell was worked out.
        for id in [2, 3] {
            let weight = shown(doc, id, &CellAddr::direct("LineWeight"));
            assert!(near(weight, 3.0 * POINT), "{path}: shape {id} shows a weight of {weight}");
        }
    }
}

#[test]
fn a_master_formula_edited_reaches_its_instances_through_the_master_on_any_save_path() {
    let saved = saved_after(&drawing(), |s| {
        edit(s, |doc| {
            let thrice = Arc::new(diacraft_formula::parse("Width*3"));
            let master = SheetRef::Shape(ShapeRef::top(Owner::Master(2), 0));
            doc.write_cell(&master, &span(), CellEdit::Formula { formula: thrice, value: None }, WriteOrigin::Drawing).unwrap();
        })
    });
    let [two, three] = as_read(&drawing());
    for (path, doc) in saved.each() {
        // The instance at the master's size keeps no copy: it shows the master's new value.
        assert_eq!(own(doc, 2), two, "{path}: the instance at its master's size kept a copy of the master's cell");
        assert!(near(shown(doc, 2, &span()), 3.0), "{path}: {}", shown(doc, 2, &span()));
        // The instance twice as wide works the formula out to another value than its master's: only that cell is held, as `Inh` with its
        // result, and none of the cells the edit did not change.
        let (cells, sections) = own(doc, 3);
        assert_eq!((cells, sections), (three.0.clone(), vec!["User:1".to_owned()]), "{path}");
        assert!(near(shown(doc, 3, &span()), 6.0), "{path}: {}", shown(doc, 3, &span()));
        let page = doc.pages.iter().next().unwrap().id;
        let held = doc.shape(&doc.find_shape(Owner::Page(page), 3).unwrap()).unwrap();
        match held.sheet.lookup(&span()) {
            Lookup::Found(cell) => assert_eq!(cell.formula, FormulaSlot::Inherit, "{path}: no formula text is copied down"),
            other => panic!("{path}: {other:?}"),
        }
    }
}

#[test]
fn a_theme_applied_gives_no_instance_a_cell_or_a_row_for_what_it_did_not_change_on_any_save_path() {
    let saved = saved_after(&drawing(), |s| {
        let r = run(s, "theme.apply", json!({ "theme": "fernwood", "page": 0 }));
        assert_eq!(r["changed_pages"], json!([0]), "the theme was applied and the page worked out: {r}");
    });
    let [two, three] = as_read(&drawing());
    for (path, doc) in saved.each() {
        // The instances follow their master's style, which the theme leaves be: it changes nothing they work out.
        assert_eq!(own(doc, 2), two, "{path}: the instance at its master's size was given cells");
        assert_eq!(own(doc, 3), three, "{path}: the instance of another size was given cells the theme did not change");
    }
}

/// The cell `name` directly under shape `id` of the first page, as the shape holds it: its formula slot and its value text.
fn held_cell(doc: &Document, id: u32, name: &str) -> Option<(FormulaSlot, Option<String>)> {
    let page = doc.pages.iter().next().unwrap();
    let shape = page.shapes.iter().find(|s| s.id == id).unwrap();
    shape.sheet.cell(name).map(|c| (c.formula.clone(), c.value.as_ref().map(|v| v.text().to_owned())))
}

#[test]
fn an_instance_the_theme_reaches_by_links_of_its_own_holds_the_one_cell_the_theme_changed_on_it() {
    // An instance of a master that links no style is made to follow the `Theme` style by links of its own. Its master's `THEMEVAL` fill,
    // worked out on it (a theme is read in the shape's own context), is then the theme's and not the master's: the instance
    // holds that one cell, `Inh` with the theme's colour, which is how it shows the theme, and still none of the cells the theme did not change.
    let source = drawing_of_an_unlinked_master();
    let saved = saved_after(&source, |s| {
        let r = run(s, "theme.apply", json!({ "theme": "fernwood", "page": 0 }));
        assert_eq!(r["changed_pages"], json!([0]), "{r}");
    });
    let read = as_read(&source);
    for (path, doc) in saved.each() {
        for (id, (cells, sections)) in [2, 3].into_iter().zip(read.clone()) {
            let mut wanted = cells.clone();
            wanted.push("FillForegnd".to_owned());
            wanted.sort();
            let (mut now, rows) = own(doc, id);
            now.sort();
            assert_eq!((now, rows), (wanted, sections), "{path}: shape {id}");
            let (slot, value) = held_cell(doc, id, "FillForegnd").unwrap();
            assert_eq!(slot, FormulaSlot::Inherit, "{path}: shape {id}: no formula text is copied down");
            assert!(value.as_deref().is_some_and(|v| v.starts_with('#') && v != "#abcdef"), "{path}: shape {id} holds {value:?}");
        }
    }
}
