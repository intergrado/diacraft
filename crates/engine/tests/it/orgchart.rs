//! The Organization Chart Shapes, dropping a position on its manager, the layouts of a chart and of one manager's reports, Re-Layout, and the picture on a position. The
//! placement is `diacraft-layout`'s (its tests hold the algorithm); these hold what the engine makes of it: a position is a shape with the marker, the hierarchy is the glue, a drop onto a
//! position links and reflows in one undo step, the choices are written into cells and kept by Re-Layout, a loop is refused in words, and the picture is the shape's own.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::draw::{shape_corners, text_view};
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::native_common::Dir;
use crate::{blank_session, check_session, run, snapshot};

type P = (f64, f64);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn centre(d: &Document, id: u32) -> P {
    let c = shape_corners(d, 1, id).unwrap();
    (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
}

fn bounds(d: &Document, id: u32) -> (f64, f64, f64, f64) {
    let c = shape_corners(d, 1, id).unwrap();
    let xs = c.iter().map(|p| p.0);
    let ys = c.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

/// Drops a position (a master of the Organization Chart Shapes) with its centre at (`x`, `y`) → the answer.
fn put(s: &mut Session, master: &str, at: P) -> Value {
    run(s, "shape.drop", json!({"master": format!("orgchart.{master}"), "page": 1, "x": at.0, "y": at.1}))
}

/// Drops a position onto the position `manager`: where its middle is now.
fn put_under(s: &mut Session, master: &str, manager: u32) -> u32 {
    let at = centre(doc(s), manager);
    id_of(&put(s, master, at))
}

/// A chart: an executive with `n` managers dropped onto it, one after another. → the executive and the managers.
fn chart(s: &mut Session, n: usize) -> (u32, Vec<u32>) {
    let boss = id_of(&put(s, "executive", (4.25, 9.5)));
    let reports = (0..n).map(|_| put_under(s, "manager", boss)).collect();
    (boss, reports)
}

fn info(s: &mut Session, ids: &[u32]) -> Value {
    run(s, "orgchart.info", json!({"ids": ids}))
}

fn pause(s: &Session) -> usize {
    snapshot(s).undo_steps
}

#[test]
fn the_stencil_holds_six_positions_with_name_and_title_and_five_data_fields() {
    // The six kinds the specifications name, each a shape with a name over a title, both fields of its Shape Data.
    let mut s = blank_session();
    let list = run(&mut s, "stencil.list", json!({}));
    let stencil = list["stencils"].as_array().unwrap().iter().find(|t| t["id"] == "orgchart").expect("the stencil is open");
    let names: Vec<&str> = stencil["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Executive", "Manager", "Position", "Assistant", "Consultant", "Vacancy"]);
    let id = id_of(&put(&mut s, "position", (4.0, 5.0)));
    let data = run(&mut s, "shapedata.list", json!({"id": id}));
    let rows: Vec<&str> = data["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap()).collect();
    assert_eq!(rows, ["Name", "Title", "Department", "Telephone", "E_mail"], "{data}");
    let v = text_view(doc(&s), 1, id).unwrap();
    assert_eq!(v.fields.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["Name", "Position"], "the text shows the name over the title");
    // A name typed in the Shape Data window shows in the text.
    run(&mut s, "shapedata.set", json!({"id": id, "row": "Name", "value": "Ann Lee"}));
    run(&mut s, "shapedata.set", json!({"id": id, "row": "Title", "value": "Head of Sales"}));
    let v = text_view(doc(&s), 1, id).unwrap();
    assert_eq!(v.fields.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["Ann Lee", "Head of Sales"], "the text follows the data");
}

#[test]
fn only_a_shape_from_the_stencil_is_a_position() {
    // Catches a marker read as a name or a plain rectangle counted as a position.
    let mut s = blank_session();
    let plain = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})));
    let a = id_of(&put(&mut s, "assistant", (5.0, 5.0)));
    let r = info(&mut s, &[plain, a]);
    assert_eq!((r["positions"].as_u64(), r["document_positions"].as_u64()), (Some(1), Some(1)), "{r}");
    assert_eq!(r["selected"][0]["position"], false);
    // `quick` stops at the first page that holds a position: the window's question.
    let q = run(&mut s, "orgchart.info", json!({"quick": true, "ids": [a]}));
    assert_eq!((q["document_positions"].as_u64(), q["complete"].as_bool()), (Some(1), Some(false)), "{q}");
    assert_eq!((r["selected"][1]["position"].as_bool(), r["selected"][1]["kind"].as_str()), (Some(true), Some("Assistant")), "{r}");
}

#[test]
fn a_position_dropped_on_its_manager_is_linked_and_the_chart_reflows_in_one_undo_step() {
    // The drop makes the connector (the hierarchy is the glue), the report is under its manager, and one Ctrl+Z takes both away.
    let mut s = blank_session();
    let boss = id_of(&put(&mut s, "executive", (4.25, 9.5)));
    let steps = pause(&s);
    let at = centre(doc(&s), boss);
    let r = put(&mut s, "manager", at);
    let m1 = id_of(&r);
    assert_eq!(r["reports_to"].as_u64(), Some(u64::from(boss)), "{r}");
    assert_eq!(pause(&s), steps + 1, "one undo step for the shape, the connector and the layout");
    let c = info(&mut s, &[m1, boss]);
    assert_eq!((c["links"].as_u64(), c["roots"].clone()), (Some(1), json!([boss])), "{c}");
    assert_eq!(c["selected"][0]["manager"].as_u64(), Some(u64::from(boss)), "{c}");
    assert_eq!(c["selected"][1]["reports"], 1);
    let (b, m) = (centre(doc(&s), boss), centre(doc(&s), m1));
    assert!(near(b.0, m.0) && b.1 > m.1 + 1.0, "the manager is under the executive: {b:?} {m:?}");
    // The connector is glued, begin at the manager and end at the report.
    let connector = r["connector"].as_u64().unwrap() as u32;
    let page = doc(&s).page(1).unwrap();
    let glued: Vec<(Option<&str>, u32)> =
        page.connects.iter().filter(|c| c.from_sheet == connector).map(|c| (c.from_cell.as_deref(), c.to_sheet)).collect();
    assert!(glued.contains(&(Some("BeginX"), boss)) && glued.contains(&(Some("EndX"), m1)), "{glued:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "orgchart.info", json!({}))["positions"], 1, "the drop is undone whole");
    check_session(&s).unwrap();
}

#[test]
fn reports_stand_in_the_order_they_were_dropped_side_by_side_under_their_manager() {
    // Catches a new report that lands among the others by where it was dropped.
    let mut s = blank_session();
    let (boss, kids) = chart(&mut s, 3);
    let d = doc(&s);
    let at: Vec<P> = kids.iter().map(|k| centre(d, *k)).collect();
    assert!(at[0].0 < at[1].0 && at[1].0 < at[2].0, "left to right in the order dropped: {at:?}");
    assert!(near(at[0].1, at[1].1) && near(at[1].1, at[2].1), "one row: {at:?}");
    let b = centre(d, boss);
    assert!(near(b.0, at[1].0), "the executive is over the middle one: {b:?} {at:?}");
    for (i, k) in kids.iter().enumerate() {
        for j in kids.iter().skip(i + 1) {
            let (p, q) = (bounds(d, *k), bounds(d, *j));
            assert!(p.2 < q.0 - 0.3, "a space between them: {p:?} {q:?}");
        }
    }
    assert_eq!(run(&mut s, "page.layout", json!({}))["place_style"], 17, "the page holds the hierarchy it was laid out in");
}

#[test]
fn a_position_dropped_on_empty_page_is_a_new_chart_and_a_plain_shape_dropped_on_a_position_links_nothing() {
    // Catches a link made for a shape that is no position, and one made for a drop on bare paper.
    let mut s = blank_session();
    let boss = id_of(&put(&mut s, "executive", (4.25, 9.5)));
    let r = put(&mut s, "executive", (4.25, 3.0));
    assert!(r.get("reports_to").is_none(), "{r}");
    let at = centre(doc(&s), boss);
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": at.0, "y": at.1}));
    assert!(r.get("reports_to").is_none(), "{r}");
    assert_eq!(run(&mut s, "orgchart.info", json!({}))["links"], 0);
}

#[test]
fn vertical_arrangement_hangs_one_managers_reports_in_a_column_and_re_layout_keeps_it() {
    // Arrange Subordinates writes the manager's ShapePlaceStyle; Re-Layout reads it again ("keeping layout options already applied, such as a vertical layout").
    let mut s = blank_session();
    let (boss, mgr) = chart(&mut s, 2);
    let reports: Vec<u32> = (0..3).map(|_| put_under(&mut s, "position", mgr[0])).collect();
    let r = run(&mut s, "orgchart.arrange", json!({"id": mgr[0], "arrangement": "vertical_right"}));
    assert_eq!(r["arrangement"], "vertical_right", "{r}");
    let d = doc(&s);
    let at: Vec<P> = reports.iter().map(|k| centre(d, *k)).collect();
    assert!(near(at[0].0, at[1].0) && near(at[1].0, at[2].0), "one column: {at:?}");
    assert!(at[0].1 > at[1].1 && at[1].1 > at[2].1, "down the page in order: {at:?}");
    let m = bounds(d, mgr[0]);
    assert!(bounds(d, reports[0]).0 > (m.0 + m.2) / 2.0, "right of the manager's middle line: {m:?}");
    // The second manager, with no reports, stands clear of the first's column.
    assert!(bounds(d, mgr[1]).0 > bounds(d, reports[0]).2 - 0.01 || bounds(d, mgr[1]).1 > bounds(d, reports[2]).3, "no overlap");
    let held = info(&mut s, &[mgr[0], boss]);
    assert_eq!(held["selected"][0]["arrangement"], "vertical_right", "{held}");
    // Pull a report out of line by hand, then Re-Layout: the column is back, and the manager still says vertical.
    run(&mut s, "shape.move", json!({"ids": [reports[1]], "dx": 2.0, "dy": 0.0}));
    run(&mut s, "orgchart.relayout", json!({}));
    let d = doc(&s);
    let again: Vec<P> = reports.iter().map(|k| centre(d, *k)).collect();
    assert!(near(again[0].0, again[1].0) && near(again[1].0, again[2].0), "the column again: {again:?}");
    assert_eq!(info(&mut s, &[mgr[0]])["selected"][0]["arrangement"], "vertical_right");
    check_session(&s).unwrap();
}

#[test]
fn a_layout_for_the_whole_chart_clears_each_managers_own_choice_and_is_written_into_the_page() {
    // Catches a whole-chart layout that leaves a manager hanging, and a page that does not hold the choice.
    let mut s = blank_session();
    let (_, mgr) = chart(&mut s, 2);
    let reports: Vec<u32> = (0..2).map(|_| put_under(&mut s, "position", mgr[0])).collect();
    run(&mut s, "orgchart.arrange", json!({"id": mgr[0], "arrangement": "vertical_left"}));
    let r = run(&mut s, "orgchart.layout", json!({"layout": "left_to_right", "spacing": 0.5}));
    assert_eq!(r["layout"], "left_to_right", "{r}");
    assert_eq!(info(&mut s, &[mgr[0]])["selected"][0]["arrangement"], "inherit");
    let d = doc(&s);
    let (a, b) = (centre(d, reports[0]), centre(d, reports[1]));
    assert!(near(a.0, b.0) && a.1 > b.1, "a flow across the page puts the reports of a manager one above the other: {a:?} {b:?}");
    assert!(centre(d, mgr[0]).0 < a.0, "the manager is to the left of its reports");
    assert_eq!(run(&mut s, "page.layout", json!({}))["place_style"], 23);
    let r = run(&mut s, "orgchart.layout", json!({"layout": "hanging_right"}));
    assert_eq!(r["layout"], "hanging_right");
    assert_eq!(run(&mut s, "page.layout", json!({}))["place_style"], 7);
    check_session(&s).unwrap();
}

#[test]
fn linking_moves_a_position_to_a_new_manager_and_a_reporting_loop_is_refused_in_words() {
    // The old connector goes, and a manager that reports to the position is refused.
    let mut s = blank_session();
    let (boss, mgr) = chart(&mut s, 2);
    let under = put_under(&mut s, "position", mgr[0]);
    let r = run(&mut s, "orgchart.link", json!({"id": under, "manager": mgr[1]}));
    assert_eq!(r["replaced"].as_array().unwrap().len(), 1, "{r}");
    let c = info(&mut s, &[under, mgr[0], mgr[1]]);
    assert_eq!((c["selected"][0]["manager"].as_u64(), c["selected"][1]["reports"].as_u64()), (Some(u64::from(mgr[1])), Some(0)), "{c}");
    assert_eq!(c["links"], 3, "the executive's two and the one that moved");
    // Linking again to the same manager changes nothing and is no undo step.
    let steps = pause(&s);
    let again = run(&mut s, "orgchart.link", json!({"id": under, "manager": mgr[1]}));
    assert_eq!(again["connector"], r["connector"]);
    assert_eq!(pause(&s), steps, "no step");
    // A loop: the executive under its own report.
    let err = s.execute("orgchart.link", &json!({"id": boss, "manager": under})).unwrap_err().to_string();
    assert!(err.contains("loop"), "{err}");
    let err = s.execute("orgchart.link", &json!({"id": under, "manager": under})).unwrap_err().to_string();
    assert!(err.contains("own manager"), "{err}");
    let plain = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})));
    let err = s.execute("orgchart.link", &json!({"id": plain, "manager": boss})).unwrap_err().to_string();
    assert!(err.contains("not a position"), "{err}");
    assert_eq!(run(&mut s, "orgchart.info", json!({}))["in_loops"], 0);
}

#[test]
fn a_file_with_a_reporting_loop_is_laid_out_and_the_loop_is_reported() {
    // A manages B manages A, made by hand with connectors: the layout finishes, and info says how many positions are in the loop.
    let mut s = blank_session();
    let a = id_of(&put(&mut s, "manager", (3.0, 8.0)));
    let b = id_of(&put(&mut s, "manager", (7.0, 4.0)));
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    run(&mut s, "connector.add", json!({"from": {"id": b}, "to": {"id": a}}));
    assert_eq!(run(&mut s, "orgchart.info", json!({}))["in_loops"], 2);
    let r = run(&mut s, "orgchart.relayout", json!({}));
    assert_eq!(r["positions"], 2, "{r}");
    check_session(&s).unwrap();
}

#[test]
fn a_page_with_no_chart_refuses_the_commands_in_words() {
    let mut s = blank_session();
    for (command, params) in [("orgchart.layout", json!({"layout": "top_to_bottom"})), ("orgchart.relayout", json!({}))] {
        let err = s.execute(command, &params).unwrap_err().to_string();
        assert!(err.contains("no org chart"), "{command}: {err}");
    }
    let id = id_of(&put(&mut s, "manager", (4.0, 4.0)));
    let err = s.execute("orgchart.layout", &json!({"layout": "sideways"})).unwrap_err().to_string();
    assert!(err.contains("top_to_bottom"), "{err}");
    let err = s.execute("orgchart.arrange", &json!({"id": id, "arrangement": "diagonal"})).unwrap_err().to_string();
    assert!(err.contains("horizontal"), "{err}");
    let err = s.execute("orgchart.layout", &json!({"layout": "top_to_bottom", "spacing": -1})).unwrap_err().to_string();
    assert!(err.contains("spacing"), "{err}");
}

/// A small PNG, `w` by `h` pixels, written to `dir` → its path.
fn png_file(dir: &Dir, name: &str, w: u32, h: u32) -> String {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&vec![180u8; (w * h * 4) as usize]).unwrap();
    dir.write(name, &out)
}

fn cell(d: &Document, id: u32, name: &str) -> Option<f64> {
    use diacraft_doc::{CellAddr, Resolver, SheetRef};
    let at = d.find_shape(diacraft_doc::Owner::Page(1), id)?;
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(name))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

#[test]
fn a_picture_goes_in_a_square_at_the_left_of_the_position_and_hide_and_delete_work() {
    // The shape's own ForeignData, the box and the text block in cells that follow the height; hiding keeps the picture and gives the text the whole shape.
    let dir = Dir::new();
    let mut s = blank_session();
    let id = id_of(&put(&mut s, "manager", (4.0, 5.0)));
    let file = png_file(&dir, "face.png", 40, 60);
    let r = run(&mut s, "orgchart.picture_set", json!({"id": id, "path": file}));
    assert_eq!((r["format"].as_str(), r["drawn"].as_bool(), r["replaced"].as_bool()), (Some("png"), Some(true), Some(false)), "{r}");
    let d = doc(&s);
    let (w, h) = (cell(d, id, "Width").unwrap(), cell(d, id, "Height").unwrap());
    let [iw, ih, ox, oy] = ["ImgWidth", "ImgHeight", "ImgOffsetX", "ImgOffsetY"].map(|n| cell(d, id, n).unwrap());
    assert!(ih > iw && near(ih, 0.84 * h), "portrait picture fills the square's height: {iw} {ih}");
    assert!(near(iw / ih, 40.0 / 60.0), "proportions kept: {iw} {ih}");
    assert!(ox >= 0.0 && ox + iw <= h + 1e-9 && oy + ih <= h + 1e-9, "inside the square at the left: {ox} {oy}");
    assert!(
        near(cell(d, id, "TxtWidth").unwrap(), w - h) && near(cell(d, id, "TxtPinX").unwrap(), h + (w - h) / 2.0),
        "text to the right of the square"
    );
    let pic = info(&mut s, &[id]);
    assert_eq!((pic["selected"][0]["picture"]["has"].as_bool(), pic["selected"][0]["picture"]["hidden"].as_bool()), (Some(true), Some(false)));
    // Hide: the picture is kept, its box is 0 and the text has the whole shape.
    run(&mut s, "orgchart.picture_hide", json!({"id": id}));
    let d = doc(&s);
    assert!(near(cell(d, id, "ImgWidth").unwrap(), 0.0) && near(cell(d, id, "TxtWidth").unwrap(), w), "hidden");
    assert_eq!(info(&mut s, &[id])["selected"][0]["picture"], json!({"has": true, "hidden": true}));
    run(&mut s, "orgchart.picture_hide", json!({"id": id, "hidden": false}));
    assert!(near(cell(doc(&s), id, "ImgWidth").unwrap(), iw), "shown again");
    // A second picture replaces the first and is shown.
    run(&mut s, "orgchart.picture_hide", json!({"id": id}));
    let second = png_file(&dir, "wide.png", 60, 30);
    let r = run(&mut s, "orgchart.picture_set", json!({"id": id, "path": second}));
    assert_eq!(r["replaced"], true);
    let d = doc(&s);
    assert!(cell(d, id, "ImgWidth").unwrap() > cell(d, id, "ImgHeight").unwrap(), "a wide picture is wider than tall");
    // Delete: the shape is as it was.
    let steps = pause(&s);
    assert_eq!(run(&mut s, "orgchart.picture_delete", json!({"id": id}))["deleted"], true);
    assert_eq!(pause(&s), steps + 1);
    let d = doc(&s);
    assert!(cell(d, id, "ImgWidth").is_none() && cell(d, id, "TxtWidth").unwrap() > 0.0, "the master's own text block again");
    assert_eq!(info(&mut s, &[id])["selected"][0]["picture"], json!({"has": false, "hidden": false}));
    assert_eq!(run(&mut s, "orgchart.picture_delete", json!({"id": id}))["deleted"], false, "nothing to delete is no step");
    check_session(&s).unwrap();
}

#[test]
fn a_picture_moves_with_its_position_and_the_chart_lays_it_out_as_one_shape() {
    // Catches a picture left behind when the chart is laid out: it is the shape's own.
    let dir = Dir::new();
    let mut s = blank_session();
    let (boss, kids) = chart(&mut s, 2);
    let file = png_file(&dir, "face.png", 30, 30);
    run(&mut s, "orgchart.picture_set", json!({"id": kids[0], "path": file}));
    run(&mut s, "orgchart.layout", json!({"layout": "bottom_to_top"}));
    let d = doc(&s);
    assert!(centre(d, boss).1 < centre(d, kids[0]).1, "the executive is at the bottom of a chart that flows up");
    assert_eq!(info(&mut s, &[kids[0]])["selected"][0]["picture"]["has"], true);
}

#[test]
fn a_position_that_is_not_one_and_a_bad_file_are_refused_in_words() {
    let dir = Dir::new();
    let mut s = blank_session();
    let plain = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})));
    let manager = id_of(&put(&mut s, "manager", (4.0, 4.0)));
    let file = png_file(&dir, "face.png", 8, 8);
    let err = s.execute("orgchart.picture_set", &json!({"id": plain, "path": file})).unwrap_err().to_string();
    assert!(err.contains("not a position"), "{err}");
    let junk = dir.write("junk.png", b"not a picture at all");
    let err = s.execute("orgchart.picture_set", &json!({"id": manager, "path": junk})).unwrap_err().to_string();
    assert!(!err.is_empty(), "{err}");
    let err = s.execute("orgchart.picture_set", &json!({"id": manager, "path": dir.path("missing.png")})).unwrap_err().to_string();
    assert!(err.contains("no such file"), "{err}");
    let err = s.execute("orgchart.picture_hide", &json!({"id": manager})).unwrap_err().to_string();
    assert!(err.contains("no picture"), "{err}");
}

#[test]
fn the_organization_chart_template_starts_a_landscape_page_with_an_executive_and_the_position_stencil_open() {
    // The tile is in File > New's list, the stencil of positions is open first, and the page holds an Executive to drop positions on.
    let mut s = Session::new();
    let listed = run(&mut s, "template.list", json!({}));
    let tile = listed["templates"].as_array().unwrap().iter().find(|t| t["id"] == "organization_chart").expect("a tile");
    assert_eq!((tile["name"].as_str(), tile["category"].as_str()), (Some("Organization Chart"), Some("Business")), "{tile}");
    assert_eq!((tile["page"]["orientation"].as_str(), tile["stencils"][0]["id"].as_str()), (Some("landscape"), Some("orgchart")), "{tile}");
    let r = run(&mut s, "file.new", json!({"template": "organization_chart"}));
    assert_eq!(r["stencils"]["open"], json!(["orgchart", "basic"]), "{r}");
    let c = run(&mut s, "orgchart.info", json!({}));
    assert_eq!((c["positions"].as_u64(), c["links"].as_u64()), (Some(1), Some(0)), "{c}");
    let boss = c["roots"][0].as_u64().unwrap() as u32;
    assert_eq!(info(&mut s, &[boss])["selected"][0]["kind"], "Executive");
    // A position dropped on it is its report.
    let at = centre(doc(&s), boss);
    let r = put(&mut s, "manager", at);
    assert_eq!(r["reports_to"].as_u64(), Some(u64::from(boss)), "{r}");
    check_session(&s).unwrap();
}

/// The connection points (their `IX`) a connector's two ends are glued to, as (begin, end), read from the page's `Connect` rows: `ToPart` is 100 plus the point's `IX`.
fn glued_points(d: &Document, connector: u32) -> (Option<u32>, Option<u32>) {
    let page = d.page(1).unwrap();
    let at = |cell: &str| {
        page.connects.iter().find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(cell)).and_then(|c| c.to_part).map(|p| p - 100)
    };
    (at("BeginX"), at("EndX"))
}

#[test]
fn the_connectors_leave_a_manager_along_the_flow_and_come_into_a_report_from_the_side_it_is_set_off() {
    // The Rectangle's points are 0 top, 1 left, 2 bottom, 3 right. A flow down the page glues bottom to top; a column hanging to the right glues the manager's bottom to the left of
    // each report; a flow to the right glues right to left. (Catches connectors that are glued at the nearest points whatever the layout: a side of a shape that is not where the flow goes.)
    let mut s = blank_session();
    let (boss, kids) = chart(&mut s, 2);
    let connectors: Vec<u32> = (0..2)
        .map(|k| {
            doc(&s).page(1).unwrap().connects.iter().find(|c| c.to_sheet == kids[k] && c.from_cell.as_deref() == Some("EndX")).unwrap().from_sheet
        })
        .collect();
    for c in &connectors {
        assert_eq!(glued_points(doc(&s), *c), (Some(2), Some(0)), "down the page: out of the bottom, into the top");
    }
    run(&mut s, "orgchart.layout", json!({"layout": "hanging_right"}));
    for c in &connectors {
        assert_eq!(glued_points(doc(&s), *c), (Some(2), Some(1)), "hanging to the right: out of the bottom, into the left side");
    }
    run(&mut s, "orgchart.layout", json!({"layout": "hanging_left"}));
    for c in &connectors {
        assert_eq!(glued_points(doc(&s), *c), (Some(2), Some(3)), "hanging to the left: into the right side");
    }
    run(&mut s, "orgchart.layout", json!({"layout": "left_to_right"}));
    for c in &connectors {
        assert_eq!(glued_points(doc(&s), *c), (Some(3), Some(1)), "to the right: out of the right side, into the left");
    }
    run(&mut s, "orgchart.layout", json!({"layout": "bottom_to_top"}));
    for c in &connectors {
        assert_eq!(glued_points(doc(&s), *c), (Some(0), Some(2)), "up the page: out of the top, into the bottom");
    }
    let _ = boss;
    check_session(&s).unwrap();
}
