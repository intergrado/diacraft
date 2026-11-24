//! What a save keeps of an org chart. The positions (the marker, the Shape Data, the text that shows two of the fields), the links (the glue), the layout cells (the page's
//! `PlaceStyle`, a manager's `ShapePlaceStyle`) and the picture of a position are the model and ride in every format; each is checked in `.diacraft`, VSDX (the model writer and a save into
//! the file) and `.odg` (the model writer and a save into the file), by reading it back, and a save into another format is checked by the check after a save and loses no class.

use diacraft_engine::Session;
use diacraft_engine::draw::text_view;
use serde_json::{Value, json};

use crate::edit::id_of;
use crate::native_common::{Dir, PINNED};
use crate::{blank_session, run};

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

/// A small PNG.
fn png(dir: &Dir) -> String {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 16, 24);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200u8; 16 * 24 * 4]).unwrap();
    dir.write("face.png", &out)
}

fn put(s: &mut Session, master: &str, at: (f64, f64)) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": format!("orgchart.{master}"), "page": 1, "x": at.0, "y": at.1})))
}

/// A chart of six positions: an executive, two managers, two positions under the first manager (which hangs them in a column), one under the second, a picture on the second manager when
/// `picture` (a picture and the text of a shape are not written together in an `.odg`, which says so in words), and names for two.
/// → the ids in the order made.
fn made(s: &mut Session, dir: &Dir, picture: bool) -> Vec<u32> {
    let centre = |s: &Session, id: u32| {
        let c = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
        (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
    };
    let boss = put(s, "executive", (4.25, 9.5));
    let at = centre(s, boss);
    let m1 = put(s, "manager", at);
    let at = centre(s, boss);
    let m2 = put(s, "manager", at);
    let at = centre(s, m1);
    let p1 = put(s, "position", at);
    let at = centre(s, m1);
    let p2 = put(s, "position", at);
    let at = centre(s, m2);
    let p3 = put(s, "assistant", at);
    run(s, "orgchart.arrange", json!({"id": m1, "arrangement": "vertical_right"}));
    run(s, "shapedata.set", json!({"id": boss, "row": "Name", "value": "Ann Lee"}));
    run(s, "shapedata.set", json!({"id": boss, "row": "Title", "value": "Chief"}));
    run(s, "shapedata.set", json!({"id": p3, "row": "Department", "value": "Sales"}));
    if picture {
        run(s, "orgchart.picture_set", json!({"id": m2, "path": png(dir)}));
    }
    vec![boss, m1, m2, p1, p2, p3]
}

/// What a save must keep: the chart as `orgchart.info` tells it, each position's text and Shape Data, and where the shapes are (to a hundredth of an inch).
fn facts(s: &mut Session) -> Value {
    let page = first_page(s);
    let ids: Vec<u32> = s.document().unwrap().page(page).unwrap().shapes.iter().map(|sh| sh.id).collect();
    let info = run(s, "orgchart.info", json!({"page": page, "ids": ids}));
    let doc = s.document().unwrap();
    let mut shapes = Vec::new();
    for sel in info["selected"].as_array().unwrap() {
        if sel["position"] != true {
            continue;
        }
        let id = sel["id"].as_u64().unwrap() as u32;
        let view = text_view(doc, page, id).unwrap();
        let corners = diacraft_engine::draw::shape_corners(doc, page, id).unwrap();
        let at: Vec<f64> = [corners.iter().map(|p| p.0).sum::<f64>() / 4.0, corners.iter().map(|p| p.1).sum::<f64>() / 4.0]
            .iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect();
        shapes.push(json!({"info": sel, "text": view.fields.iter().map(|f| f.text.clone()).collect::<Vec<_>>(), "at": at}));
    }
    for sh in &mut shapes {
        let id = sh["info"]["id"].as_u64().unwrap();
        let data = run(s, "shapedata.list", json!({"id": id, "page": page}));
        sh["data"] = data["fields"].as_array().unwrap().iter().map(|f| json!([f["row"], f["value"]])).collect();
    }
    json!({"positions": info["positions"], "links": info["links"], "layout": info["layout"], "shapes": shapes})
}

/// What an `.odg` save of the chart loses by the check after a save: **nothing**. A connector between two shapes was named for its connection points, its route, its line
/// end, its label and its outline, by a drawing of plain rectangles as well, and the test held the chart to what those lose; the writer and the check now agree on all of them.
fn assert_nothing_differs(saved: &Value) {
    assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{saved}");
}

#[test]
fn a_chart_is_kept_by_a_diacraft_save() {
    let dir = Dir::new();
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    made(&mut s, &dir, true);
    let want = facts(&mut s);
    assert_eq!(want["positions"], 6, "{want}");
    assert_eq!(want["links"], 5);
    let path = dir.path("chart.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = opened(&path);
    assert_eq!(facts(&mut back), want, ".diacraft");
    // The chart is still one: Re-Layout keeps it as it was.
    run(&mut back, "orgchart.relayout", json!({}));
    assert_eq!(facts(&mut back)["shapes"], want["shapes"], "Re-Layout of the reopened chart changes nothing");
}

#[test]
fn and_by_the_vsdx_writer_and_a_save_into_a_vsdx_file() {
    let dir = Dir::new();
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    made(&mut s, &dir, true);
    let want = facts(&mut s);
    let path = dir.path("chart.vsdx");
    let saved = run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{saved}");
    let mut back = opened(&path);
    assert_eq!(facts(&mut back), want, "VSDX from the model");
    // Change the chart in the file it came from and save into it.
    run(&mut back, "orgchart.layout", json!({"layout": "left_to_right"}));
    let into = facts(&mut back);
    run(&mut back, "file.save", json!({}));
    assert_eq!(facts(&mut opened(&path)), into, "VSDX saved into itself");
}

#[test]
fn and_by_an_odg_written_from_the_model_and_a_save_into_an_odg_file() {
    let dir = Dir::new();
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    made(&mut s, &dir, false);
    let want = facts(&mut s);
    let path = dir.path("chart.odg");
    let saved = run(&mut s, "file.save_as", json!({ "path": path }));
    assert_nothing_differs(&saved);
    let mut back = opened(&path);
    assert_eq!(facts(&mut back), want, ".odg from the model");
    // A layout glues the connectors anew, which a save into an `.odg` does not write yet (a connection is a later task of the `.odg` writer): it is refused in words, nothing is written, and
    // Save As `.diacraft` holds the chart as it is.
    run(&mut back, "orgchart.layout", json!({"layout": "hanging_left"}));
    let into = facts(&mut back);
    let e = back.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(e.contains("glued anew") && e.contains(".diacraft"), "{e}");
    assert_eq!(facts(&mut opened(&path)), want, "the file is as it was");
    let native = dir.path("hung.diacraft");
    run(&mut back, "file.save_as", json!({ "path": native }));
    assert_eq!(facts(&mut opened(&native)), into, "Save As .diacraft");
}

#[test]
fn a_chart_saved_across_formats_is_checked_and_loses_no_class() {
    // The trip .diacraft -> VSDX -> .odg keeps every class the check compares (user cells, shape data, pictures, text).
    let dir = Dir::new();
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    made(&mut s, &dir, false);
    let native = dir.path("chart.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut from_native = opened(&native);
    for ext in ["vsdx", "odg"] {
        let saved = run(&mut from_native, "file.save_as", json!({ "path": dir.path(&format!("across.{ext}")) }));
        if ext == "odg" {
            assert_nothing_differs(&saved);
        } else {
            assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{ext}: {saved}");
        }
        let want = facts(&mut opened(&native));
        assert_eq!(facts(&mut opened(&dir.path(&format!("across.{ext}")))), want, "{ext}");
    }
}

#[test]
fn an_odg_is_not_written_while_a_position_holds_a_picture_and_the_words_say_what_to_do() {
    // A position's picture and its text are one shape, and an `.odg` has no place for the text of a picture frame (the save refuses it, as for any picture with a text on it).
    // `.diacraft` and VSDX hold it; delete the picture and the chart saves as an `.odg`.
    let dir = Dir::new();
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    let ids = made(&mut s, &dir, true);
    let e = s.execute("file.save_as", &json!({ "path": dir.path("pic.odg") })).unwrap_err().to_string();
    assert!(e.contains("a text on a picture") && e.contains("Nothing was written") && e.contains(".diacraft"), "{e}");
    assert!(!dir.files().contains(&"pic.odg".to_owned()), "nothing was written");
    run(&mut s, "orgchart.picture_delete", json!({"id": ids[2]}));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("pic.odg") }));
    assert_nothing_differs(&saved);
}
