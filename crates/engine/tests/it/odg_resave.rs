//! A drawing saved under another name, then saved again with no edit in between, in the same session.
//!
//! Found that a VSDX drawing saved as an `.odg` was refused on the next `file.save` ("a change of the document's own fields (a property)"), plain boxes too. A save
//! with no edit is never refused and never counts as a change: whatever the first save wrote, the second takes as the file it is. These tests run that chain from every
//! kind of drawing (a VSDX file, a new drawing, one made from a built-in template, an `.odg`) into every format a Save As writes (`.odg`, VSDX, `.diacraft`), and show that
//! an edit made after the Save As is still saved.
//!
//! **Every file is made by code**: the VSDX files by the testkit's builder (the shapes' XML written by hand from the specifications), the `.odg` files by `odg_build`.

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, glued, rect};
use crate::run;

/// A rectangle of the VSDX file: 2 by 1 inches with its pin at (`x`, 2), its outline's rows, and a text.
fn vsdx_box(id: u32, x: f64) -> String {
    format!(
        concat!(
            r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#,
            r#"<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section><Text>box {id}</Text></Shape>"#
        ),
        id = id,
        x = x
    )
}

/// A connector from the right side of the box at `PinX` 2 to the left side of the one at 6, a line of two rows.
const VSDX_CONNECTOR: &str = concat!(
    r#"<Shape ID="3" Type="Shape"><Cell N="BeginX" V="3"/><Cell N="BeginY" V="2"/><Cell N="EndX" V="5"/><Cell N="EndY" V="2"/>"#,
    r#"<Cell N="PinX" V="4" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/>"#,
    r#"<Cell N="Height" V="0"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0"/>"#,
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row></Section></Shape>"#
);

/// The connector glued to the two boxes, each end to the shape as a whole.
const VSDX_CONNECTS: &str = concat!(
    r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/>"#,
    r#"<Connect FromSheet="3" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#
);

const PAGE_SHEET: &str = r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#;

/// A VSDX drawing of two plain boxes, with the builder's own document properties.
fn vsdx_boxes() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE_SHEET).shapes(&format!("{}{}", vsdx_box(1, 2.0), vsdx_box(2, 6.0)))).build()
}

/// The same with a connector glued to both.
fn vsdx_connected() -> Vec<u8> {
    VsdxBuilder::new()
        .page(
            PageSpec::new(1, "Page-1")
                .sheet(PAGE_SHEET)
                .shapes(&format!("{}{}{VSDX_CONNECTOR}", vsdx_box(1, 2.0), vsdx_box(2, 6.0)))
                .connects(VSDX_CONNECTS),
        )
        .build()
}

/// An `.odg` of two rectangles and a connector glued to both.
fn odg_connected() -> Vec<u8> {
    let ided = |text: String, id: &str| text.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1);
    let shapes = [
        ided(rect("A", (2.0, 3.0, 4.0, 2.0), "a"), "id1"),
        ided(rect("B", (12.0, 3.0, 4.0, 2.0), ""), "id2"),
        glued("K", "id1", "id2", (6.0, 4.0, 12.0, 4.0)),
    ]
    .concat();
    Drawing::new().page(&shapes).bytes()
}

/// A session with the clock pinned that has opened `bytes` as `name` in `dir`.
fn opened(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, bytes) }));
    s
}

/// A new drawing of two rectangles, and `connected` a connector glued from one to the other.
fn new_drawing(connected: bool) -> Session {
    let mut s = pinned();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    if connected {
        run(&mut s, "connector.add", json!({ "from": { "id": a, "point": 3 }, "to": { "id": b, "point": 1 } }));
    }
    s
}

/// A drawing made from the built-in template `template`, with two rectangles dropped on it.
fn from_template(template: &str) -> Session {
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": template }));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    for x in [2.0, 6.0] {
        run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": x, "y": 3.0 }));
    }
    s
}

/// A new drawing of two rectangles whose page was given a size: a preset (`DrawingSizeType` 2) or a size of its own (3), a cell no attribute of an `.odg` holds.
fn new_sized(size: Value) -> Session {
    let mut s = new_drawing(false);
    run(&mut s, "page.size", size);
    s
}

/// A way to make the drawing a chain starts from: a fresh session each time, so that every target starts from the drawing as it was opened or made.
type Start<'a> = Box<dyn Fn() -> Session + 'a>;

/// Every drawing the chain starts from, with what it is: VSDX files (plain boxes, a connector, the vectors of a file an application wrote, style sheets), new drawings (plain, with a
/// connector, with a page size), drawings made from each built-in template, and an `.odg`.
fn starts(dir: &Dir) -> Vec<(String, Start<'_>)> {
    let mut out: Vec<(String, Start<'_>)> = vec![
        ("a VSDX file of two boxes".to_owned(), Box::new(|| opened(dir, "boxes.vsdx", &vsdx_boxes()))),
        ("a VSDX file of two boxes and a connector".to_owned(), Box::new(|| opened(dir, "connected.vsdx", &vsdx_connected()))),
        ("a VSDX file with style sheets".to_owned(), Box::new(|| opened(dir, "styles.vsdx", &crate::across::vsdx_with_styles()))),
    ];
    for (at, (what, bytes)) in crate::odgout_masters::vectors().into_iter().enumerate() {
        let name = format!("vector{at}.vsdx");
        out.push((format!("a VSDX file: {what}"), Box::new(move || opened(dir, &name, &bytes))));
    }
    out.push(("a new drawing of two rectangles".to_owned(), Box::new(|| new_drawing(false))));
    out.push(("a new drawing of two rectangles and a connector".to_owned(), Box::new(|| new_drawing(true))));
    out.push(("a new drawing with a preset page size".to_owned(), Box::new(|| new_sized(json!({ "preset": "letter" })))));
    out.push(("a new drawing with a page size of its own".to_owned(), Box::new(|| new_sized(json!({ "width": 10.0, "height": 7.0 })))));
    for template in ["basic_diagram", "basic_flowchart", "cross_functional_flowchart", "organization_chart"] {
        out.push((format!("a drawing made from the built-in template {template}"), Box::new(move || from_template(template))));
    }
    out.push(("an .odg of two rectangles and a connector".to_owned(), Box::new(|| opened(dir, "connected.odg", &odg_connected()))));
    out
}

/// The chain: `file.save_as` to `name`, then `file.save` with no edit in between. `Err` says what went wrong: a refusal of either save (in its words), a second save that counts as
/// an edit or leaves the drawing dirty, or one that wrote other bytes than the first.
fn save_as_then_save(s: &mut Session, dir: &Dir, name: &str) -> Result<(), String> {
    s.execute("file.save_as", &json!({ "path": dir.path(name) })).map_err(|e| format!("the save as was refused: {e}"))?;
    let first = dir.read(name);
    let again: Value = s.execute("file.save", &json!({})).map_err(|e| format!("the save after it was refused: {e}"))?;
    if again.get("edited").is_some_and(|edited| *edited != json!(false)) {
        return Err(format!("the save after it counts as an edit: {again}"));
    }
    if again["dirty"] != json!(false) || s.is_dirty() {
        return Err(format!("the drawing is dirty after the save: {again}"));
    }
    if dir.read(name) != first {
        return Err("the save after it wrote other bytes than the save as".to_owned());
    }
    Ok(())
}

/// **The bug, and its neighbours.** From each drawing, a Save As to `.odg`, then a Save: no refusal, no edit, the same bytes. The same chain into VSDX and `.diacraft`. Every
/// pair is run before anything is asserted, so that one failure does not hide another.
#[test]
fn a_save_after_a_save_as_with_no_edit_between_is_no_edit_in_every_format() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut chains = 0;
    for (at, (what, start)) in starts(&dir).into_iter().enumerate() {
        for target in ["odg", "vsdx", "diacraft"] {
            chains += 1;
            let mut s = start();
            if let Err(why) = save_as_then_save(&mut s, &dir, &format!("out{at}.{target}")) {
                failures.push(format!("{what} saved as .{target}: {why}"));
            }
        }
    }
    assert!(chains >= 3 * 25, "every drawing in every format: {chains} chains");
    assert!(failures.is_empty(), "{} of {chains} chains failed:\n{}", failures.len(), failures.join("\n"));
}

/// **A real edit after the Save As is still saved.** From a VSDX file and from a new drawing, saved as `.odg`: one shape moved, then a Save writes it into the file (an edit), and the
/// file read again has the shape where the drawing has it; a Save after that is no edit.
#[test]
fn an_edit_after_a_save_as_odg_is_saved_and_the_save_after_it_is_no_edit() {
    let dir = Dir::new();
    for (what, mut s, id) in [("a VSDX file", opened(&dir, "boxes.vsdx", &vsdx_boxes()), 1u32), ("a new drawing", new_drawing(false), 0)] {
        let id = if id == 0 { s.document().unwrap().pages.get(0).unwrap().shapes.iter().next().unwrap().id } else { id };
        let name = format!("{}.odg", what.replace(' ', "-"));
        run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
        let first = dir.read(&name);
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
        assert!(s.is_dirty(), "{what}");
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the edit was refused: {e}"));
        assert_eq!(saved["edited"], true, "{what}: {saved}");
        assert_ne!(dir.read(&name), first, "{what}: the edit is in the file");
        let pin = |s: &Session| {
            let doc = s.document().unwrap();
            let page = doc.pages.get(0).unwrap().id;
            let f = diacraft_engine::draw::shape_frame(doc, page, id).unwrap();
            (f.pin_x, f.pin_y)
        };
        let mut back = pinned();
        run(&mut back, "file.open", json!({ "path": dir.path(&name) }));
        let (want, got) = (pin(&s), pin(&back));
        assert!((want.0 - got.0).abs() < 1e-6 && (want.1 - got.1).abs() < 1e-6, "{what}: the file has the shape at {got:?}, the drawing at {want:?}");
        let edited = dir.read(&name);
        let again = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save after the edit's save was refused: {e}"));
        assert_eq!(again["edited"], false, "{what}: {again}");
        assert_eq!(dir.read(&name), edited, "{what}: the save after it is the file the edit's save wrote");
    }
}

/// The `.odg` of a frame that shows an embedded object (a compound file, with a PNG for its replacement) and a rectangle beside it.
fn odg_with_object() -> Vec<u8> {
    Drawing::new()
        .page(&format!("{}{}", crate::across::object_frame(), rect("R", (2.0, 8.0, 4.0, 2.0), "")))
        .file("Object 1", &crate::across::compound())
        .file("ObjectReplacements/Object 1", &crate::across::png(10))
        .bytes()
}

/// A VSDX drawing that holds an embedded object: the `.odg` [`odg_with_object`] saved as a `.vsdm`, as a person's file would hold one.
fn vsdx_with_object(dir: &Dir) -> Vec<u8> {
    let mut s = opened(dir, "object.odg", &odg_with_object());
    run(&mut s, "file.save_as", json!({ "path": dir.path("object.vsdm") }));
    dir.read("object.vsdm")
}

/// Whether the package holds an entry of DiaCraft's own folder whose bytes are `bytes`: a part of the other format the `.odg` carries.
fn carries(package: &[u8], bytes: &[u8]) -> bool {
    let read = diacraft_odg::package::Package::read(package, &diacraft_odg::Limits::default()).unwrap();
    let names: Vec<String> = read.entries().filter(|e| e.name.starts_with("DiaCraft/")).map(|e| e.name.clone()).collect();
    names.iter().any(|name| read.part(name).is_some_and(|b| b == bytes))
}

/// **What the Save As was asked to leave out stays out of every save into the file** (`drop_macros`, `drop_objects`): the drawing keeps it, and the save after the Save As, with no edit, is no
/// edit and the same bytes. Before, the save after a Save As that left the macro project out put it back into the file (an edit, with none made), and the one after a Save As that left the
/// objects out compared the drawing's objects with the file's pictures.
#[test]
fn what_a_save_as_odg_left_out_stays_out_of_the_saves_after_it() {
    let dir = Dir::new();
    let vba: &[u8] = b"not code, never run";
    let cases: Vec<(&str, Vec<u8>, &str, Value)> = vec![
        ("a .vsdm whose macro project is left out", crate::across::vsdm_with_vba(), "a.vsdm", json!({ "drop_macros": true })),
        ("a VSDX file whose embedded object is left out", vsdx_with_object(&dir), "b.vsdm", json!({ "drop_objects": true })),
        ("a VSDX file whose object and macros are left out", vsdx_with_object(&dir), "c.vsdm", json!({ "drop_objects": true, "drop_macros": true })),
    ];
    let mut failures = Vec::new();
    for (at, (what, bytes, name, asked)) in cases.into_iter().enumerate() {
        let mut s = opened(&dir, name, &bytes);
        let out = format!("left{at}.odg");
        let mut params = asked.clone();
        params["path"] = json!(dir.path(&out));
        if let Err(e) = s.execute("file.save_as", &params) {
            failures.push(format!("{what}: the save as was refused: {e}"));
            continue;
        }
        let first = dir.read(&out);
        match s.execute("file.save", &json!({})) {
            Ok(again) if again["edited"] == json!(false) && dir.read(&out) == first => {}
            Ok(again) => failures.push(format!("{what}: the save after it counts as an edit or wrote other bytes: {again}")),
            Err(e) => failures.push(format!("{what}: the save after it was refused: {e}")),
        }
        // An edit after it is saved, and what was left out stays out.
        let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().next_back().unwrap().id;
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 0.5, "dy": 0.0 }));
        match s.execute("file.save", &json!({})) {
            Ok(edited) if edited["edited"] == json!(true) && dir.read(&out) != first => {}
            Ok(edited) => failures.push(format!("{what}: the edit was not saved: {edited}")),
            Err(e) => failures.push(format!("{what}: the edit was refused: {e}")),
        }
        if asked["drop_macros"] == json!(true) && carries(&dir.read(&out), vba) {
            failures.push(format!("{what}: the macro project is in the file after the edit"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
