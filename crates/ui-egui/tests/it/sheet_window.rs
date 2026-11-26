//! The ShapeSheet window, reading: the sheet area, how it opens, its sections and cells, its tabs, its bar, the contextual tab and the cost of a large sheet.
//!
//! The design is the ShapeSheet window's (5.1 to 5.7 and 5.13 for the window, 6 for the budgets, 10 for the
//! tokens and the names) and the spec of the window is. The window reads through `diacraft_engine::sheetview`, so what a
//! test asks of the sheet's content is what the engine's own tests (`crates/engine/tests/it/sheet_read.rs`) already hold; these tests hold what the window does with it.
//! A cost is held to a count of the work (`diacraft_ui_egui::work`) and never to a time.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value as Held};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, TouchPhase, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::fixture;
use crate::support::{Rest, TempDir, Window, click, drawing_window, elements_named, fast_window, input, move_to, press, run_command, window};

// ---- the windows these tests open -----------------------------------------------------------------------------------------------------

/// The window state's account of the sheets.
pub(crate) fn sheet(w: &Window) -> Value {
    w.state().window_state()["sheet"].clone()
}

/// What the sheets have worked out since the window began: outlines made, rows they visited, cells resolved and lines read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Work {
    outlines: u64,
    rows: u64,
    cells: u64,
    lines: u64,
}

fn work_of(w: &Window) -> Work {
    let s = sheet(w)["work"].clone();
    let n = |k: &str| s[k].as_u64().unwrap_or_else(|| panic!("no count {k} in {s}"));
    Work { outlines: n("outlines"), rows: n("rows"), cells: n("cells"), lines: n("lines") }
}

/// The work done between two readings.
fn since(w: &Window, before: Work) -> Work {
    let now = work_of(w);
    Work { outlines: now.outlines - before.outlines, rows: now.rows - before.rows, cells: now.cells - before.cells, lines: now.lines - before.lines }
}

pub(crate) fn labels(w: &Window) -> Vec<String> {
    sheet(w)["sections"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_owned()).collect()
}

pub(crate) fn tab_titles(w: &Window) -> Vec<String> {
    sheet(w)["tabs"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap().to_owned()).collect()
}

pub(crate) fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

pub(crate) fn is_drawn(w: &Window, name: &str) -> bool {
    w.state().element(name).is_some()
}

/// A new drawing with a Process shape (id returned), a Decision and a connector between them, selecting none.
pub(crate) fn flow_window(size: impl Into<Vec2>) -> (Window, u32, u32, u32) {
    let mut w = drawing_window(size);
    let mut drop = |master: &str, x: f64| -> u32 {
        let answer = w.state_mut().execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": 5.0})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    let (process, decision) = (drop("process", 3.0), drop("decision", 7.0));
    let answer = w.state_mut().execute("connector.add", &json!({"from": {"x": 4.5, "y": 5.0}, "to": {"x": 6.0, "y": 5.0}})).unwrap();
    let connector = u32::try_from(answer["id"].as_u64().unwrap()).unwrap();
    run_command(&mut w, "edit.select", json!({"ids": []}));
    (w, process, decision, connector)
}

/// A window on `doc`, built over a new drawing the way the engine's own tests build one.
pub(crate) fn window_on(doc: Document, size: impl Into<Vec2>) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit("build", |d| {
            *d = doc;
            Ok(())
        })
        .unwrap();
    let mut w = window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), size);
    w.rest();
    w
}

/// A shape with a Geometry of `rows` rows (a MoveTo and LineTo rows with an `X` and a `Y` each), as the engine's reading tests build one.
pub(crate) fn geometry_shape(id: u32, rows: u32) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id).cell("Width", "2").cell("Height", "1").cell("PinX", "4").cell("PinY", "4");
    for k in 1..=rows {
        let t = if k == 1 { "MoveTo" } else { "LineTo" };
        shape = shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), t)
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "X", &(k % 3).to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "Y", &(k % 2).to_string());
    }
    shape
}

pub(crate) fn one_page(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build()
}

pub(crate) fn open_shape(w: &mut Window, id: u32) {
    run_command(w, "window.shapesheet", json!({"of": "shape", "id": id, "page": 1}));
}

pub(crate) fn right_click(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        input(w, Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    w.step();
    w.step();
}

pub(crate) fn developer_on(w: &mut Window) {
    run_command(w, "window.developer", json!({"on": true}));
    run_command(w, "window.tab", json!({"tab": "Developer"}));
}

// ---- opening a sheet (design 5.1) ----------------------------------------------------------------------------------------------------------

/// Developer, Shape Design, Show ShapeSheet: the big part opens the selected shape's sheet, and the page's when nothing is selected; its menu holds Shape (enabled with a shape
/// selected), Page and Document.
#[test]
fn show_shapesheet_on_the_developer_tab_opens_the_shape_selected_or_else_the_page() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    developer_on(&mut w);
    assert_eq!(sheet(&w)["open"], json!(false), "no sheet is open before Show");
    press(&mut w, "ribbon.Developer.ShowShapeSheet.main");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "page", "page": 1}), "nothing is selected: the page's sheet");
    assert_eq!(tab_titles(&w), ["Page-1"]);
    run_command(&mut w, "window.sheetClose", json!({}));
    assert_eq!(sheet(&w)["open"], json!(false), "the last tab closed: the sheet area goes");

    run_command(&mut w, "edit.select", json!({"ids": [process]}));
    press(&mut w, "ribbon.Developer.ShowShapeSheet.main");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "shape", "page": 1, "id": process}), "a shape is selected: its sheet");
    assert!(tab_titles(&w)[0].ends_with(", Page-1"), "the shape's name and its page: {:?}", tab_titles(&w));
}

#[test]
fn the_items_of_the_show_shapesheet_menu_open_the_shape_the_page_and_the_document() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    developer_on(&mut w);
    // With nothing selected, Shape is quiet and Page and Document work.
    press(&mut w, "ribbon.Developer.ShowShapeSheet.menu");
    assert!(!w.state().element("menu.ShowShapeSheetShape").unwrap().enabled, "Shape needs a shape selected");
    assert!(w.state().element("menu.ShowShapeSheetPage").unwrap().enabled);
    assert!(w.state().element("menu.ShowShapeSheetDocument").unwrap().enabled);
    press(&mut w, "menu.ShowShapeSheetDocument");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "document"}));
    assert_eq!(tab_titles(&w), ["Document"]);

    run_command(&mut w, "edit.select", json!({"ids": [process]}));
    press(&mut w, "ribbon.Developer.ShowShapeSheet.menu");
    assert!(w.state().element("menu.ShowShapeSheetShape").unwrap().enabled, "a shape is selected");
    press(&mut w, "menu.ShowShapeSheetShape");
    w.rest();
    press(&mut w, "ribbon.Developer.ShowShapeSheet.menu");
    press(&mut w, "menu.ShowShapeSheetPage");
    w.rest();
    assert_eq!(tab_titles(&w).len(), 3, "each Show is a tab of the one sheet area: {:?}", tab_titles(&w));
    assert_eq!(sheet(&w)["tabs"][1]["sheet"], json!({"kind": "shape", "page": 1, "id": process}));
    assert_eq!(sheet(&w)["tabs"][2]["sheet"], json!({"kind": "page", "page": 1}));
}

/// With no document open Show ShapeSheet is quiet, and the command says why.
#[test]
fn show_shapesheet_needs_a_drawing() {
    let mut w = window(DiacraftApp::new(Session::new(), None), [1200.0, 800.0]);
    w.rest();
    let e = w.state_mut().execute("window.shapesheet", &json!({"of": "page"})).unwrap_err().to_string();
    assert!(e.contains("no document open"), "{e}");
}

/// Right-click a shape (Developer tab on): Show ShapeSheet is the menu's first item and opens that shape's sheet; right-click an empty part of the page: the page's.
#[test]
fn the_right_click_menus_open_the_sheet_of_the_shape_or_the_page_while_the_developer_tab_is_on() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    // The Developer tab is off: the menu has no such row.
    let at = w.state().screen_point_at((3.0, 5.0)).unwrap();
    right_click(&mut w, at);
    assert!(!is_drawn(&w, "menu.CanvasShowShapeSheet"), "the row is there only while the Developer tab is on");
    input(&mut w, Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    w.rest();

    developer_on(&mut w);
    right_click(&mut w, at);
    let row = w.state().element("menu.CanvasShowShapeSheet").expect("Show ShapeSheet is on the shape's menu").clone();
    assert!(row.enabled);
    let cut = rect_of(&w, "menu.CanvasCut");
    assert!(row.rect.top() < cut.top(), "the menu's first item: {:?} over {:?}", row.rect, cut);
    press(&mut w, "menu.CanvasShowShapeSheet");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "shape", "page": 1, "id": process}));

    // An empty part of the page: the page's sheet. (The sheet area is below the drawing now, which moved the page: ask where the empty place is again.)
    run_command(&mut w, "window.sheetClose", json!({}));
    let empty = w.state().screen_point_at((1.0, 1.0)).unwrap();
    right_click(&mut w, empty);
    assert!(is_drawn(&w, "menu.CanvasShowShapeSheet"));
    press(&mut w, "menu.CanvasShowShapeSheet");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "page", "page": 1}), "nothing under the click: the page's sheet");
}

/// Drawing Explorer: right-click the drawing, a style or a master opens its sheet (a master's first top-level shape).
#[test]
fn drawing_explorer_opens_the_sheet_of_the_drawing_a_style_and_a_master() {
    let dir = TempDir::new("sheet-explorer");
    let path = fixture::write(dir.path(), "demo.vsdx", &fixture::demo_vsdx());
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "window.pane", json!({"pane": "drawing_explorer", "shown": true}));
    run_command(&mut w, "window.explorerBranch", json!({"branch": "styles", "open": true}));
    run_command(&mut w, "window.explorerBranch", json!({"branch": "masters", "open": true}));

    let at = rect_of(&w, "explorer.drawing").center();
    right_click(&mut w, at);
    press(&mut w, "explorer.menu.sheet");
    w.rest();
    assert_eq!(sheet(&w)["tabs"][0]["sheet"], json!({"kind": "document"}), "the drawing's own sheet");

    let style = elements_named(&w, "explorer.style.").first().map(|e| e.0.clone()).expect("the demo drawing has a style");
    let at = rect_of(&w, &style).center();
    right_click(&mut w, at);
    press(&mut w, "explorer.menu.sheet");
    w.rest();
    let id: u64 = style.trim_start_matches("explorer.style.").parse().unwrap();
    assert_eq!(sheet(&w)["tabs"][1]["sheet"], json!({"kind": "style", "style": id}));
    assert!(tab_titles(&w)[1].ends_with("(style)"), "{:?}", tab_titles(&w));

    let master = elements_named(&w, "explorer.master.").first().map(|e| e.0.clone()).expect("the demo drawing has a master");
    let at = rect_of(&w, &master).center();
    right_click(&mut w, at);
    press(&mut w, "explorer.menu.sheet");
    w.rest();
    let master_id: u64 = master.trim_start_matches("explorer.master.").parse().unwrap();
    let opened = sheet(&w)["tabs"][2]["sheet"].clone();
    assert_eq!(opened["master"], json!(master_id), "{opened}");
    assert!(opened.get("id").is_some(), "the master's first top-level shape, where its cells are: {opened}");
    assert!(tab_titles(&w)[2].contains("(master)"), "{:?}", tab_titles(&w));
}

/// The command refuses what it cannot open, in words.
#[test]
fn window_shapesheet_refuses_what_it_cannot_open_in_words() {
    let (mut w, _, _, _) = flow_window([1200.0, 800.0]);
    let mut refused = |params: Value| w.state_mut().execute("window.shapesheet", &params).unwrap_err().to_string();
    assert!(refused(json!({"of": "shape"})).contains("nothing is selected"), "a shape needs one selected or named");
    assert!(refused(json!({"of": "shape", "id": 999, "page": 1})).contains("page 1 has no shape 999"));
    assert!(refused(json!({"of": "style"})).contains("style"), "a style needs its id");
    assert!(refused(json!({"of": "style", "style": 999})).contains("no style 999"));
    assert!(refused(json!({"of": "master", "master": 999})).contains("no master 999"));
    assert!(refused(json!({"of": "page", "page": 9})).contains("no page 9"));
    assert!(refused(json!({"of": "nothing"})).contains("`of` must be"));
    assert!(refused(json!({"banana": 1})).contains("unknown parameter"));
}

// ---- the tabs (design 5.1, 5.3) -----------------------------------------------------------------------------------------

/// A second Show of the sheet open brings its tab forward and makes no second tab; another object is another tab; the tab named by place or by title comes on top; at most 32
/// tabs, the 33rd taking the place of the least recently used.
#[test]
fn each_show_is_a_tab_and_a_sheet_that_is_open_is_brought_forward() {
    let (mut w, process, decision, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    open_shape(&mut w, decision);
    assert_eq!(sheet(&w)["tabs"].as_array().unwrap().len(), 2);
    assert_eq!(sheet(&w)["active"], json!(1), "the sheet just opened is the one on view");
    open_shape(&mut w, process);
    assert_eq!(sheet(&w)["tabs"].as_array().unwrap().len(), 2, "no two tabs show one sheet");
    assert_eq!(sheet(&w)["active"], json!(0), "its tab came forward");
    run_command(&mut w, "window.sheetTab", json!({"tab": 1}));
    assert_eq!(sheet(&w)["active"], json!(1));
    let title = tab_titles(&w)[0].clone();
    run_command(&mut w, "window.sheetTab", json!({"tab": title}));
    assert_eq!(sheet(&w)["active"], json!(0), "a tab is found by its title too");
    let e = w.state_mut().execute("window.sheetTab", &json!({"tab": 7})).unwrap_err().to_string();
    assert!(e.contains("no tab 7"), "{e}");
    run_command(&mut w, "window.sheetClose", json!({"tab": 0}));
    assert_eq!(sheet(&w)["tabs"].as_array().unwrap().len(), 1);
    assert_eq!(sheet(&w)["tabs"][0]["sheet"]["id"], json!(decision));
}

#[test]
fn the_thirty_third_sheet_takes_the_place_of_the_one_used_least_recently() {
    let mut page = PageBuilder::new().name("Page-1");
    for id in 1..=33 {
        page = page.shape(ShapeBuilder::new(id).cell("PinX", &id.to_string()).cell("PinY", "2").cell("Width", "1").cell("Height", "1"));
    }
    let mut w = window_on(DocBuilder::new().page(1, page).build(), [1200.0, 800.0]);
    for id in 1..=32 {
        open_shape(&mut w, id);
    }
    assert_eq!(sheet(&w)["tabs"].as_array().unwrap().len(), 32);
    // Shape 1's sheet is the oldest; bring it forward so that shape 2's is the least recently used.
    open_shape(&mut w, 1);
    open_shape(&mut w, 33);
    let ids: Vec<u64> = sheet(&w)["tabs"].as_array().unwrap().iter().map(|t| t["sheet"]["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 32, "at most 32 tabs");
    assert!(ids.contains(&33) && ids.contains(&1) && !ids.contains(&2), "the least recently used (shape 2) was reused: {ids:?}");
}

/// An object that is gone leaves its tab open with the words and a Close button, and an undo that brings it back shows it again (design 5.1).
#[test]
fn a_tab_whose_shape_is_gone_says_so_and_the_undo_that_brings_it_back_shows_it_again() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    assert!(!sheet(&w)["tabs"][0]["gone"].as_bool().unwrap());
    assert!(!labels(&w).is_empty());
    run_command(&mut w, "shape.delete", json!({"ids": [process], "page": 1}));
    assert!(sheet(&w)["tabs"][0]["gone"].as_bool().unwrap(), "the shape was deleted: the tab stays");
    assert!(is_drawn(&w, "sheet.gone"), "and says so");
    assert!(is_drawn(&w, "sheet.gone.close"), "with a Close button");
    assert!(labels(&w).is_empty());
    run_command(&mut w, "edit.undo", json!({}));
    assert!(!sheet(&w)["tabs"][0]["gone"].as_bool().unwrap(), "the undo brought the shape back");
    assert!(labels(&w).len() > 3, "and its sheet is shown again: {:?}", labels(&w));
}

/// A drawing that replaces the open one takes the tabs of the old one away.
#[test]
fn a_new_drawing_closes_the_sheets_of_the_old_one() {
    let (mut w, process, _, _) = flow_window([1200.0, 800.0]);
    open_shape(&mut w, process);
    run_command(&mut w, "file.new", json!({}));
    assert_eq!(sheet(&w)["open"], json!(false));
    assert!(!is_drawn(&w, "sheet.area"));
}

// ---- the dock (design 5.2) ---------------------------------------------------------------------------------------------------

/// The sheet area opens below the drawing, 45% of the middle band, with the drawing keeping 120 points and the sheet 160; Beside puts it at the right of the drawing, and Fill gives it
/// the whole band with a tab for the drawing at the left of the sheet tabs; the narrow band shows Beside as Below.
#[test]
fn the_sheet_area_docks_below_beside_or_filling_the_band() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    let canvas_before = rect_of(&w, "canvas");
    open_shape(&mut w, process);
    assert_eq!(sheet(&w)["dock"], json!("below"));
    let (area, canvas) = (rect_of(&w, "sheet.area"), rect_of(&w, "canvas"));
    assert!(area.top() >= canvas.bottom() - 0.5, "below the drawing: {area:?} under {canvas:?}");
    assert!(area.height() >= 160.0, "the sheet keeps 160 points: {area:?}");
    assert!(canvas.height() >= 120.0, "the drawing keeps 120 points: {canvas:?}");
    // The band holds the drawing, its rulers (22 points) and its page tabs (28) as well as the sheet.
    let band = canvas.height() + 22.0 + 28.0 + area.height();
    assert!((area.height() - 0.45 * band).abs() < 0.05 * band, "about 45% of the middle band ({band}): {}", area.height());
    assert!(canvas.height() < canvas_before.height(), "the drawing gave up the room");

    run_command(&mut w, "window.sheetDock", json!({"at": "beside"}));
    let (area, canvas) = (rect_of(&w, "sheet.area"), rect_of(&w, "canvas"));
    assert!(area.left() >= canvas.right() - 0.5, "beside: {area:?} at the right of {canvas:?}");
    assert_eq!(sheet(&w)["shownDock"], json!("beside"));

    run_command(&mut w, "window.sheetDock", json!({"at": "fill"}));
    assert!(!is_drawn(&w, "canvas"), "the sheet fills the band: the drawing is not drawn");
    let (area, drawing_tab, first_tab) = (rect_of(&w, "sheet.area"), rect_of(&w, "sheet.tab.drawing"), rect_of(&w, "sheet.tab.0"));
    assert!(area.width() > canvas_before.width() * 0.9, "{area:?}");
    assert!(drawing_tab.right() <= first_tab.left() + 0.5, "the drawing's tab is at the left of the sheet tabs");
    press(&mut w, "sheet.tab.drawing");
    w.rest();
    assert!(is_drawn(&w, "canvas"), "a press on the drawing's tab goes back to the drawing");
    assert_eq!(sheet(&w)["dock"], json!("beside"), "back to the dock it had before Fill");

    let e = w.state_mut().execute("window.sheetDock", &json!({"at": "above"})).unwrap_err().to_string();
    assert!(e.contains("below, beside or fill"), "{e}");
}

#[test]
fn a_band_narrower_than_640_points_shows_beside_as_below() {
    let (mut w, process, _, _) = flow_window([900.0, 800.0]);
    run_command(&mut w, "window.pane", json!({"pane": "size_position", "shown": true}));
    open_shape(&mut w, process);
    run_command(&mut w, "window.sheetDock", json!({"at": "beside"}));
    // Shown below, the area spans the band.
    let band = rect_of(&w, "sheet.area").width();
    assert!(band < 640.0, "this window leaves the band {band} points wide");
    assert_eq!(sheet(&w)["dock"], json!("beside"), "the choice is kept");
    assert_eq!(sheet(&w)["shownDock"], json!("below"), "and shown as Below while the band is narrow");
    assert!(rect_of(&w, "sheet.area").top() >= rect_of(&w, "canvas").bottom() - 0.5);
}

// ---- the sections, in order (design 5.4) ------------------------------------------------------------------------

/// The window's one order of R2 for a shape (of the window spec): a section the sheet shows comes where this table has it.
const SHAPE_ORDER: &[&str] = &[
    "1-D Endpoints",
    "Shape Transform",
    "User-defined Cells",
    "Shape Data",
    "Hyperlinks",
    "Connection Points",
    "Actions",
    "Controls",
    "Geometry 1",
    "Geometry 2",
    "Scratch",
    "Protection",
    "Miscellaneous",
    "Group Properties",
    "Line Format",
    "Fill Format",
    "Text Fields",
    "Character",
    "Paragraph",
    "Tabs",
    "Annotation",
    "Text Block Format",
    "Text Transform",
    "Layer Membership",
    "Events",
    "Foreign Image Info",
    "Image Properties",
    "Glue Info",
    "Alignment",
    "Shape Layout",
];

fn position(label: &str) -> usize {
    SHAPE_ORDER.iter().position(|l| *l == label).unwrap_or_else(|| panic!("{label} is not in the table of the order"))
}

#[test]
fn a_shapes_sections_come_in_the_order_of_the_window_and_a_connectors_start_with_its_endpoints() {
    let (mut w, process, _, connector) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    let box_labels = labels(&w);
    assert_eq!(box_labels[0], "Shape Transform", "{box_labels:?}");
    assert!(!box_labels.contains(&"1-D Endpoints".to_owned()), "a box has no end points: {box_labels:?}");
    assert!(!box_labels.contains(&"Group Properties".to_owned()), "and is not a group: {box_labels:?}");
    let places: Vec<usize> = box_labels.iter().filter(|l| SHAPE_ORDER.contains(&l.as_str())).map(|l| position(l)).collect();
    assert!(places.windows(2).all(|p| p[0] < p[1]), "the sections of a box follow the table: {box_labels:?}");

    open_shape(&mut w, connector);
    let line_labels = labels(&w);
    assert_eq!(&line_labels[..2], ["1-D Endpoints", "Shape Transform"], "1-D Endpoints is above Shape Transform: {line_labels:?}");
    let places: Vec<usize> = line_labels.iter().filter(|l| SHAPE_ORDER.contains(&l.as_str())).map(|l| position(l)).collect();
    assert!(places.windows(2).all(|p| p[0] < p[1]), "{line_labels:?}");
    assert!(line_labels.iter().any(|l| l == "Geometry 1"), "the connector's path is a Geometry section: {line_labels:?}");
}

#[test]
fn a_pages_sheet_starts_with_page_properties_and_the_documents_with_document_properties() {
    let (mut w, _, _, _) = flow_window([1400.0, 900.0]);
    run_command(&mut w, "window.shapesheet", json!({"of": "page"}));
    let page = labels(&w);
    assert_eq!(&page[..3], ["Page Properties", "Page Layout", "Ruler & Grid"], "{page:?}");
    run_command(&mut w, "window.shapesheet", json!({"of": "document"}));
    assert_eq!(labels(&w)[0], "Document Properties", "{:?}", labels(&w));
}

// ---- a cell: where it comes from, its errors, its two texts (design 5.6) --------------------------------

pub(crate) fn painted<'a>(state: &'a Value, name: &str) -> &'a Value {
    state["painted"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no cell {name} was painted: {:?}", state["painted"]))
}

/// A Width the master gives is drawn in the text colour with no dot, a PinX the shape holds in the own-cell colour with the dot, and a cell that reads an error shows it in the
/// error colour (design 5.6: "inventory: local blue, inherited black").
#[test]
fn a_local_cell_is_blue_with_a_dot_an_inherited_one_is_plain_and_an_error_shows_its_word() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    let s = sheet(&w);
    let (pin, width) = (painted(&s, "PinX"), painted(&s, "Width"));
    assert_eq!((pin["origin"].as_str(), pin["ink"].as_str(), pin["dot"].as_bool()), (Some("local"), Some("local"), Some(true)), "{pin}");
    assert_eq!((width["origin"].as_str(), width["ink"].as_str(), width["dot"].as_bool()), (Some("inherited"), Some("text"), Some(false)), "{width}");

    // A shape that holds a formula that failed.
    let mut broken = Cell::with_formula("1/0", Some(Held::from_text("0")));
    broken.error = Some("#DIV/0!".into());
    let doc = one_page(ShapeBuilder::new(10).cell("Width", "2").cell("Height", "1").set(CellAddr::direct("PinX"), broken));
    let mut w = window_on(doc, [1400.0, 900.0]);
    open_shape(&mut w, 10);
    let s = sheet(&w);
    let pin = painted(&s, "PinX");
    assert_eq!(pin["shown"], json!("#DIV/0!"), "an error shows its word: {pin}");
    assert_eq!(pin["valueInk"], json!("error"), "in the error colour: {pin}");
    assert_eq!(pin["text"], json!("1/0"), "beside the formula that made it");
}

/// With both toggles on a box shows the formula and the value; a value that reads as its formula is not shown twice, by number and not by text (`2 in` and `2.0000 in`); with one
/// toggle on a box shows that one (four decimals in the Values view, R36: at most four, zeros cut, in the Formulas view).
#[test]
fn a_box_shows_the_formula_and_the_value_and_not_the_value_twice() {
    let doc = one_page(ShapeBuilder::new(10).cell_u("Width", "2.5", "IN").formula("Height", "Width*0.5", "1.25").cell("PinX", "4").cell("PinY", "4"));
    let mut w = window_on(doc, [1400.0, 900.0]);
    open_shape(&mut w, 10);
    let s = sheet(&w);
    let width = painted(&s, "Width");
    assert_eq!(width["text"], json!("2.5 in"), "a plain value shows as a formula does, with its zeros cut: {width}");
    assert_eq!(width["shown"], json!(""), "and its value is not shown twice (`2.5 in` and `2.5000 in` are one number): {width}");
    let height = painted(&s, "Height");
    assert_eq!((height["text"].as_str(), height["shown"].as_str()), (Some("Width*0.5"), Some("1.2500 in")), "{height}");

    run_command(&mut w, "window.sheetView", json!({"formulas": false}));
    let s = sheet(&w);
    assert_eq!((sheet(&w)["formulas"].as_bool(), sheet(&w)["values"].as_bool()), (Some(false), Some(true)));
    let (width, height) = (painted(&s, "Width"), painted(&s, "Height"));
    assert_eq!((width["text"].as_str(), width["shown"].as_str()), (Some(""), Some("2.5000 in")), "Values alone: four decimals, zeros kept: {width}");
    assert_eq!((height["text"].as_str(), height["shown"].as_str()), (Some(""), Some("1.2500 in")), "{height}");

    run_command(&mut w, "window.sheetView", json!({"formulas": true, "values": false}));
    let s = sheet(&w);
    let (width, height) = (painted(&s, "Width"), painted(&s, "Height"));
    assert_eq!((width["text"].as_str(), width["shown"].as_str()), (Some("2.5 in"), Some("")), "Formulas alone: {width}");
    assert_eq!((height["text"].as_str(), height["shown"].as_str()), (Some("Width*0.5"), Some("")), "{height}");
}

/// The two toggles are both on at first, and the last one on cannot go off.
#[test]
fn the_two_toggles_start_on_and_the_last_one_on_cannot_go_off() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    assert_eq!((sheet(&w)["formulas"].as_bool(), sheet(&w)["values"].as_bool()), (Some(true), Some(true)));
    run_command(&mut w, "window.sheetView", json!({"values": false}));
    assert_eq!((sheet(&w)["formulas"].as_bool(), sheet(&w)["values"].as_bool()), (Some(true), Some(false)));
    let e = w.state_mut().execute("window.sheetView", &json!({"formulas": false})).unwrap_err().to_string();
    assert!(e.contains("at least one of Formulas and Values stays on"), "{e}");
    assert_eq!(sheet(&w)["formulas"], json!(true), "nothing changed");
    let e = w.state_mut().execute("window.sheetView", &json!({"formulas": false, "values": false})).unwrap_err().to_string();
    assert!(e.contains("at least one"), "{e}");
    run_command(&mut w, "window.sheetView", json!({"formulas": false, "values": true}));
    assert_eq!((sheet(&w)["formulas"].as_bool(), sheet(&w)["values"].as_bool()), (Some(false), Some(true)));
}

/// The View group's Formulas and Values boxes are the toggles: they show the state, and a press changes it.
#[test]
fn the_formulas_and_values_boxes_of_the_shapesheet_design_tab_turn_the_toggles() {
    let (mut w, process, _, _) = flow_window([1600.0, 900.0]);
    open_shape(&mut w, process);
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    assert_eq!(w.get_by_label("Formulas").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    press(&mut w, "ribbon.ShapeSheet Design.ShapeSheetShowValues");
    w.rest();
    assert_eq!(sheet(&w)["values"], json!(false), "a press on Values turned it off");
    assert_eq!(w.get_by_label("Values").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    press(&mut w, "ribbon.ShapeSheet Design.ShapeSheetShowFormulas");
    w.rest();
    assert_eq!(sheet(&w)["formulas"], json!(true), "the last one on stays on: a press on it changes nothing");
    assert!(w.state().window_state()["tab"] == "ShapeSheet Design");
}

// ---- collapse and Show Sections (design 5.4) -------------------------------------------------------------------------------------------

/// A press on a section's header folds it away or opens it; the lines of the sheet follow, and the header shows its state to assistive technology.
#[test]
fn a_press_on_a_header_folds_a_section_and_another_opens_it() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    let total = sheet(&w)["lines"].as_u64().unwrap();
    let header = "sheet.section.Shape Transform";
    assert!(is_drawn(&w, "sheet.cell.PinX"), "Shape Transform is open at first");
    press(&mut w, header);
    w.rest();
    let shown = sheet(&w);
    let section = shown["sections"].as_array().unwrap().iter().find(|s| s["name"] == "Shape Transform").unwrap();
    assert_eq!(section["open"], json!(false));
    assert!(shown["lines"].as_u64().unwrap() < total, "the folded section gave its lines up");
    assert!(!is_drawn(&w, "sheet.cell.PinX"), "its cells are not drawn");
    assert!(is_drawn(&w, header), "its header is");
    assert_eq!(w.get_by_label("Shape Transform").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    press(&mut w, header);
    w.rest();
    assert_eq!(sheet(&w)["lines"].as_u64().unwrap(), total, "opened again");
    assert!(is_drawn(&w, "sheet.cell.PinX"));
    run_command(&mut w, "window.sheetCollapse", json!({"section": "shape transform", "open": false}));
    assert!(!is_drawn(&w, "sheet.cell.PinX"), "the command names the section in any case");
    let e = w.state_mut().execute("window.sheetCollapse", &json!({"section": "Nothing"})).unwrap_err().to_string();
    assert!(e.contains("no section Nothing"), "{e}");
}

/// Show Sections: a card of check boxes, one a section the sheet shows, hides any; all ticked at first.
#[test]
fn show_sections_hides_the_sections_that_are_not_ticked() {
    let (mut w, process, _, _) = flow_window([1600.0, 900.0]);
    open_shape(&mut w, process);
    let all = labels(&w);
    let lines_before = sheet(&w)["lines"].as_u64().unwrap();
    assert!(sheet(&w)["sections"].as_array().unwrap().iter().all(|s| s["shown"] == json!(true)), "all ticked at first");
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    press(&mut w, "ribbon.ShapeSheet Design.ShapeSheetShowSections");
    let boxes = elements_named(&w, "sheet.sections.");
    assert_eq!(boxes.len(), all.len(), "one box a section: {boxes:?}");
    press(&mut w, "sheet.sections.Protection");
    w.rest();
    let after = sheet(&w);
    let protection = after["sections"].as_array().unwrap().iter().find(|s| s["name"] == "Protection").unwrap();
    assert_eq!(protection["shown"], json!(false), "a press on a box hides the section");
    assert!(!is_drawn(&w, "sheet.section.Protection"));
    assert!(is_drawn(&w, "sheet.section.Shape Transform"));
    assert!(after["lines"].as_u64().unwrap() < lines_before, "the hidden section gave its lines up: {} then {}", lines_before, after["lines"]);

    run_command(&mut w, "window.sheetSections", json!({"shown": ["Shape Transform"]}));
    let only: Vec<String> = sheet(&w)["sections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["shown"] == json!(true))
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(only, ["Shape Transform"]);
    assert_eq!(sheet(&w)["lines"].as_u64().unwrap(), 11, "a header and the ten cells of Shape Transform");
    run_command(&mut w, "window.sheetSections", json!({}));
    assert_eq!(labels(&w), all);
    assert!(sheet(&w)["sections"].as_array().unwrap().iter().all(|s| s["shown"] == json!(true)), "without `shown` every section shows again");
}

// ---- the tab, the bar and the contextual tab (design 5.2, 5.13) -------------------------------------------------------------

/// The ShapeSheet Design tab shows while the sheet has the focus, at the right end of the row, with View, Sections, Rows, Editing and Formula Tracing; the focus is
/// had from a press in the sheet or its bar until a press in the drawing, and the ribbon goes back to its first tab when the tab goes.
#[test]
fn the_shapesheet_design_tab_shows_while_the_sheet_has_the_focus() {
    let (mut w, process, _, _) = flow_window([1600.0, 900.0]);
    assert!(!is_drawn(&w, "tab.ShapeSheet Design"), "no sheet, no tab");
    open_shape(&mut w, process);
    assert_eq!(sheet(&w)["focus"], json!(true), "the sheet that was opened has the focus");
    let tab = rect_of(&w, "tab.ShapeSheet Design");
    let help = rect_of(&w, "tab.Help");
    assert!(tab.left() > help.right(), "at the right end of the row, after Help: {tab:?} {help:?}");
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    let groups: Vec<String> =
        w.state().elements().iter().filter_map(|e| e.name.strip_prefix("ribbon.ShapeSheet Design.group.")).map(str::to_owned).collect();
    assert_eq!(groups, ["View", "Sections", "Rows", "Editing", "Formula Tracing"], "V16");

    // A press in the drawing takes the focus away: the tab goes and the ribbon goes back to its first tab.
    let canvas = rect_of(&w, "canvas");
    click(&mut w, canvas.center());
    w.rest();
    assert_eq!(sheet(&w)["focus"], json!(false));
    assert!(!is_drawn(&w, "tab.ShapeSheet Design"));
    assert_eq!(w.state().window_state()["tab"], json!("Home"), "the ribbon is back on its first tab");
    assert!(is_drawn(&w, "sheet.area"), "the sheet stays open");
    assert!(is_drawn(&w, "sheet.bar"), "and so does its bar");
    // A press in the sheet brings the focus and the tab back.
    let list = rect_of(&w, "sheet.list");
    click(&mut w, list.center());
    w.rest();
    assert_eq!(sheet(&w)["focus"], json!(true));
    assert!(is_drawn(&w, "tab.ShapeSheet Design"));
    // A press on a control of the ShapeSheet Design tab is no press in the drawing.
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    press(&mut w, "ribbon.ShapeSheet Design.ShapeSheetShowFormulas");
    assert_eq!(sheet(&w)["focus"], json!(true), "the ribbon does not take the focus from the sheet");
}

/// The formula bar sits directly under the ribbon body and above the sheet, with the cancel and accept buttons before the text, and stays while a sheet is open.
#[test]
fn the_formula_bar_sits_directly_under_the_ribbon_body_above_the_sheet() {
    let (mut w, process, _, _) = flow_window([1600.0, 900.0]);
    assert!(!is_drawn(&w, "sheet.bar"));
    open_shape(&mut w, process);
    let bar = rect_of(&w, "sheet.bar");
    let body_bottom = w
        .state()
        .elements()
        .iter()
        .filter(|e| e.name.starts_with("ribbon.") && e.name.contains(".group."))
        .map(|e| e.rect.bottom())
        .fold(f32::MIN, f32::max);
    assert!(bar.top() >= body_bottom - 0.5 && bar.top() - body_bottom < 8.0, "directly under the ribbon body ({body_bottom}): {bar:?}");
    assert!(bar.bottom() <= rect_of(&w, "sheet.area").top(), "above the sheet");
    assert!(bar.bottom() <= rect_of(&w, "canvas").top() + 0.5, "and above the drawing: {bar:?}");
    let (name, cancel, accept, formula) =
        (rect_of(&w, "sheet.bar.name"), rect_of(&w, "sheet.bar.cancel"), rect_of(&w, "sheet.bar.accept"), rect_of(&w, "sheet.bar.formula"));
    assert!(name.right() <= cancel.left() && cancel.right() <= accept.left() && accept.right() <= formula.left(), "the buttons come before the text");
    assert!(
        !w.state().element("sheet.bar.cancel").unwrap().enabled && !w.state().element("sheet.bar.accept").unwrap().enabled,
        "nothing to edit yet: greyed"
    );
    run_command(&mut w, "window.sheetClose", json!({}));
    assert!(!is_drawn(&w, "sheet.bar"), "the bar goes when the last tab closes");
}

/// The bar shows the selected cell's name and its formula and value.
#[test]
fn the_bar_shows_the_selected_cell() {
    let doc = one_page(ShapeBuilder::new(10).cell("Width", "2").formula("Height", "Width*0.5", "1").cell("PinX", "4").cell("PinY", "4"));
    let mut w = window_on(doc, [1400.0, 900.0]);
    open_shape(&mut w, 10);
    assert_eq!(sheet(&w)["selected"], Value::Null);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "Height"}));
    assert_eq!(sheet(&w)["selected"]["cell"], json!("Height"));
    let bar = sheet(&w)["bar"].clone();
    assert_eq!((bar["reference"].as_str(), bar["formula"].as_str()), (Some("Height"), Some("Width*0.5")), "{bar}");
    assert_eq!(bar["value"], json!("1.0000 in"), "the value the formula works out to, beside it");
    assert_eq!(w.get_all_by_label_contains("Reference: Height").count(), 1);
    assert_eq!(w.get_all_by_label_contains("Formula: Width*0.5").count(), 1);
    // A press on a cell selects it.
    press(&mut w, "sheet.cell.PinX");
    assert_eq!(sheet(&w)["selected"]["cell"], json!("PinX"));
    let e = w.state_mut().execute("window.sheetSelect", &json!({"cell": "Nothing"})).unwrap_err().to_string();
    assert!(e.contains("no cell Nothing"), "{e}");
}

// ---- a large sheet (design 6: the budgets, as counts) ----------------------------------------------------------------------------------

/// The most rows the sample files hold in one section: a Geometry of 31,273 rows opens with one visit of each row and no cell resolved to list it, scrolls to its last row, and a
/// frame that changes nothing resolves nothing. (The engine's own test of the outline is `sheet_read.rs`; this holds the window's use of it.)
#[test]
fn a_geometry_of_31273_rows_scrolls_to_its_last_row_and_a_still_frame_resolves_no_cell() {
    const ROWS: u32 = 31_273;
    let mut w = window_on(one_page(geometry_shape(10, ROWS)), [1400.0, 900.0]);
    let before = work_of(&w);
    open_shape(&mut w, 10);
    let opened = since(&w, before);
    assert_eq!(opened.outlines, 1, "the outline is made once");
    assert_eq!(opened.rows, u64::from(ROWS), "by one visit of each row");
    let on_view = sheet(&w)["visible"][1].as_u64().unwrap() - sheet(&w)["visible"][0].as_u64().unwrap();
    assert!(on_view > 5 && on_view < 80, "only the lines on view are laid out: {on_view}");
    assert!(opened.cells <= (on_view + 2) * 10, "a window of lines resolves a few cells: {} for {on_view} lines", opened.cells);
    assert!(opened.cells >= on_view / 2, "and does resolve those: {}", opened.cells);

    // The last row, brought on view by the command, then drawn.
    let before = work_of(&w);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Geometry1", "row": ROWS}));
    assert!(is_drawn(&w, &format!("sheet.row.Geometry1.{ROWS}")), "the last row is on view");
    assert!(sheet(&w)["first"].as_u64().unwrap() > 31_000, "{}", sheet(&w)["first"]);
    let scrolled = since(&w, before);
    assert_eq!((scrolled.outlines, scrolled.rows), (0, 0), "a scroll makes no outline again: {scrolled:?}");
    assert!(scrolled.cells <= (on_view + 4) * 10, "{}", scrolled.cells);

    // A frame in which nothing changed resolves no cell, reads no line and makes no outline.
    w.rest();
    let before = work_of(&w);
    for _ in 0..3 {
        w.step();
    }
    assert_eq!(since(&w, before), Work::default(), "a still frame does no work");
}

/// A scroll that brings k new lines on view resolves at most k times the columns of cells (design 6).
#[test]
fn a_scroll_of_k_lines_resolves_the_cells_of_k_lines_and_no_more() {
    let mut w = window_on(one_page(geometry_shape(10, 5_000)), [1400.0, 900.0]);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Geometry1", "row": 100}));
    w.rest();
    let first = sheet(&w)["first"].as_u64().unwrap();
    // The wheel over the list, three lines' worth of points.
    let list = rect_of(&w, "sheet.list");
    let before = work_of(&w);
    move_to(&mut w, list.center());
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -66.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    for _ in 0..12 {
        w.step();
    }
    w.rest();
    let after = sheet(&w)["first"].as_u64().unwrap();
    let moved = after - first;
    assert!((1..=6).contains(&moved), "a wheel of 66 points moved the list a few lines: {moved}");
    let worked = since(&w, before);
    assert!(worked.lines <= moved + 2, "{} lines read for {moved} new lines", worked.lines);
    assert!(worked.cells <= (moved + 2) * 10, "{} cells for {moved} lines", worked.cells);
    assert_eq!(worked.outlines, 0);
}

/// A tab that is not on view costs nothing: the outline is made for the tab on view only, kept while the drawing is as it was, and made again for a tab brought forward after
/// the drawing has changed.
#[test]
fn only_the_tab_on_view_reads_anything() {
    let (mut w, process, decision, _) = flow_window([1400.0, 900.0]);
    let start = work_of(&w);
    open_shape(&mut w, process);
    open_shape(&mut w, decision);
    open_shape(&mut w, process);
    assert_eq!(since(&w, start).outlines, 2, "one outline for each sheet that came on view, and none again for the one that was already read");
    let before = work_of(&w);
    for _ in 0..3 {
        w.step();
    }
    assert_eq!(since(&w, before).outlines, 0);
    // An edit of the drawing makes the outline of the tab on view again, and the other tab's when it is brought forward.
    let before = work_of(&w);
    run_command(&mut w, "shape.move", json!({"ids": [decision], "dx": 1.0, "dy": 0.0}));
    assert_eq!(since(&w, before).outlines, 1, "one for the tab on view");
    run_command(&mut w, "window.sheetTab", json!({"tab": 1}));
    assert_eq!(since(&w, before).outlines, 2, "and one for the tab brought forward");
}

// ---- narrow and wide, and the names ----------------------------------------------------------------------------------------------------

/// A shape with a Shape Data section of three rows, so that the sheet has a table with column titles.
pub(crate) fn data_shape(id: u32) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id).cell("Width", "2").cell("Height", "1").cell("PinX", "4").cell("PinY", "4");
    for (name, label) in [("Cost", "Unit cost"), ("Owner", "Who owns it"), ("Phase", "Delivery phase")] {
        let key = RowKey::name(name);
        shape = shape
            .row_cell("Property", None, key.clone(), "Label", label)
            .row_cell("Property", None, key.clone(), "Prompt", "A long prompt for the field")
            .row_cell("Property", None, key.clone(), "Value", "12")
            .row_cell("Property", None, key, "Format", "@");
    }
    shape
}

pub(crate) fn overlaps(a: Rect, b: Rect) -> bool {
    a.intersects(b) && a.intersect(b).width() > 0.5 && a.intersect(b).height() > 0.5
}

/// At 640, 1024 and 1920 points the tab strip, the bar and the column titles do not clip or overlap, and the window comes to rest at each width.
#[test]
fn the_tab_strip_the_bar_and_the_column_titles_hold_at_640_1024_and_1920_points() {
    for (width, height) in [(640.0, 720.0), (1024.0, 800.0), (1920.0, 1000.0)] {
        let mut w = window_on(one_page(data_shape(10)), [width, height]);
        w.rest();
        open_shape(&mut w, 10);
        run_command(&mut w, "window.shapesheet", json!({"of": "page"}));
        run_command(&mut w, "window.shapesheet", json!({"of": "document"}));
        run_command(&mut w, "window.sheetTab", json!({"tab": 0}));
        w.rest();
        let area = rect_of(&w, "sheet.area");
        assert!(area.width() <= width + 0.5, "{width}: {area:?}");
        // The tabs, in order, inside the strip, never over one another.
        let tabs = elements_named(&w, "sheet.tab.");
        let strip = rect_of(&w, "sheet.tabs");
        let tab_rects: Vec<Rect> =
            tabs.iter().filter(|(n, _, _)| n.matches('.').count() == 2 && !n.ends_with(".close")).map(|(_, r, _)| *r).collect();
        assert!(!tab_rects.is_empty());
        for (i, a) in tab_rects.iter().enumerate() {
            assert!(a.width() >= 40.0, "{width}: a tab is wide enough to hold its close button: {a:?}");
            for b in &tab_rects[i + 1..] {
                assert!(!overlaps(*a, *b), "{width}: tabs overlap: {a:?} {b:?}");
            }
        }
        assert!(
            tab_rects.iter().all(|r| r.top() >= strip.top() - 0.5 && r.bottom() <= strip.bottom() + 0.5),
            "{width}: the tabs are in the strip {strip:?}"
        );
        // The bar's parts, in a row, the text with at least 120 points.
        let parts: Vec<Rect> =
            ["sheet.bar.name", "sheet.bar.cancel", "sheet.bar.accept", "sheet.bar.formula"].iter().map(|n| rect_of(&w, n)).collect();
        for pair in parts.windows(2) {
            assert!(pair[0].right() <= pair[1].left() + 0.5, "{width}: the bar's parts overlap: {pair:?}");
        }
        assert!(parts[3].width() >= 120.0, "{width}: the bar keeps 120 points of text: {:?}", parts[3]);
        assert!(w.state().element("sheet.bar.cancel").is_some() && w.state().element("sheet.bar.accept").is_some(), "{width}: the two buttons stay");
        let bar = rect_of(&w, "sheet.bar");
        assert!(parts.iter().all(|r| bar.contains_rect(*r)), "{width}: the parts are inside the bar {bar:?}");

        // A table's column titles: in a row, none over another, none outside the list.
        run_command(&mut w, "window.sheetTab", json!({"tab": 0}));
        run_command(&mut w, "window.sheetSelect", json!({"section": "Prop", "row": "Cost", "cell": "Label"}));
        let list = rect_of(&w, "sheet.list");
        let titles: Vec<Rect> = elements_named(&w, "sheet.title.Prop.").iter().map(|(_, r, _)| *r).collect();
        assert!(titles.len() >= 3, "{width}: the titles of Shape Data's columns are drawn: {titles:?}");
        for (i, a) in titles.iter().enumerate() {
            assert!(list.contains_rect(*a) || a.intersect(list).width() > 0.0, "{width}: {a:?} outside {list:?}");
            for b in &titles[i + 1..] {
                assert!(!overlaps(*a, *b), "{width}: column titles overlap: {a:?} {b:?}");
            }
        }
        let row = rect_of(&w, "sheet.title.Prop.Row");
        assert!(row.left() <= list.left() + 40.0, "{width}: the Row column is held at the left: {row:?} in {list:?}");
        // Every cell box on view is between the top and the bottom of the list, and none is over another.
        let cells = elements_named(&w, "sheet.cell.");
        for (name, r, _) in &cells {
            assert!(r.top() >= list.top() - 1.0 && r.bottom() <= list.bottom() + 1.0, "{width}: {name} {r:?} leaves {list:?}");
        }
        for (i, (na, a, _)) in cells.iter().enumerate() {
            for (nb, b, _) in &cells[i + 1..] {
                assert!(!overlaps(*a, *b), "{width}: cells overlap: {na} {a:?} and {nb} {b:?}");
            }
        }
        w.rest();
    }
}

/// Every control the sheet draws has a name for assistive technology (design 10), and a cell's name says its place, its formula, its value and where it comes from.
#[test]
fn every_part_of_the_sheet_has_a_name_that_says_what_it_is() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "PinX"}));
    let label = w
        .get_all_by_label_contains("PinX: formula")
        .next()
        .expect("PinX has a name")
        .accesskit_node()
        .label()
        .map(|l| l.to_string())
        .unwrap_or_default();
    assert!(label.contains("Shape Transform") && label.contains("own"), "{label}");
    let width = w
        .get_all_by_label_contains("Width: formula")
        .next()
        .expect("Width has a name")
        .accesskit_node()
        .label()
        .map(|l| l.to_string())
        .unwrap_or_default();
    assert!(width.contains("inherited"), "{width}");
    for name in ["Cancel", "Accept", "Reference", "Formula: "] {
        assert_eq!(w.get_all_by_label_contains(name).count(), 1, "{name}");
    }
    assert_eq!(w.get_all_by_role(egui::accesskit::Role::Grid).count(), 1, "the sheet is one grid");
    let title = tab_titles(&w)[0].clone();
    assert_eq!(w.get_all_by_role_and_label(egui::accesskit::Role::Tab, &title).count(), 1, "a tab of the strip is a tab");
    assert_eq!(w.get_all_by_role_and_label(egui::accesskit::Role::ScrollBar, "Sheet scroll bar").count(), 1);
}

// ---- the commands ----------------------------------------------------------------------------------------------------------------------

/// Each window command of the sheet is in the window's registry with a params doc the registry can read: one `name: description` entry a key (the lesson).
#[test]
fn the_window_commands_of_the_sheet_are_listed_with_readable_params() {
    use diacraft_ui_egui::commands::{UI_COMMANDS, find_ui_command};
    let expected: &[(&str, &[&str])] = &[
        ("window.shapesheet", &["of", "id", "page", "style", "master"]),
        ("window.sheetTab", &["tab"]),
        ("window.sheetClose", &["tab"]),
        ("window.sheetDock", &["at"]),
        ("window.sheetView", &["formulas", "values"]),
        ("window.sheetSections", &["shown"]),
        ("window.sheetCollapse", &["section", "open"]),
        ("window.sheetSelect", &["cell", "section", "row"]),
    ];
    for (id, keys) in expected {
        let spec = find_ui_command(id).unwrap_or_else(|| panic!("{id} is not a window command"));
        assert_eq!(&diacraft_engine::param_keys(spec.params).unwrap_or_else(|e| panic!("{id}: {e}")), keys, "{id}");
        assert!(!spec.label.is_empty());
    }
    assert!(UI_COMMANDS.iter().filter(|c| c.id.starts_with("window.sheet") || c.id == "window.shapesheet").count() >= expected.len());
}

/// A command that acts on an open sheet is refused while none is open, in words.
#[test]
fn the_commands_that_act_on_an_open_sheet_say_so_when_none_is_open() {
    let mut w = drawing_window([1200.0, 800.0]);
    for (id, params) in [
        ("window.sheetTab", json!({"tab": 0})),
        ("window.sheetClose", json!({})),
        ("window.sheetDock", json!({"at": "fill"})),
        ("window.sheetView", json!({"values": true})),
        ("window.sheetSections", json!({})),
        ("window.sheetCollapse", json!({"section": "Shape Transform"})),
        ("window.sheetSelect", json!({"cell": "Width"})),
    ] {
        let e = w.state_mut().execute(id, &params).unwrap_err().to_string();
        assert!(e.contains("no ShapeSheet is open"), "{id}: {e}");
    }
}

// ---- the window keeps one frame the same as the next ----------------------------------------------------------------------------------

/// A sheet open, scrolled and selected comes to rest at once: nothing repaints for ever, and nothing asks for another frame on a timer.
#[test]
fn an_open_sheet_comes_to_rest_and_asks_for_no_timer() {
    let (mut w, process, _, _) = flow_window([1100.0, 800.0]);
    open_shape(&mut w, process);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "PinX"}));
    let frames = w.rest();
    assert!(frames <= 8, "settled in {frames} frames");
    assert!(w.state().pending_input() == 0);
}
