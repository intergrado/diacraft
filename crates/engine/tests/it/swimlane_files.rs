//! A cross-functional flowchart **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a file and
//! saved in its own format (the splice). A lane and a list are cells: `User.msvStructureType` and the `DiaCraftSwimlane` and `DiaCraftListDirection` rows, the `Relationships` cell on both sides of
//! each tie (the order of a list's members is the formula's), and each shape's own geometry; a file holds them in the format's own way (VSDX: the User and Shape Layout sections; `.diacraft`: the
//! model; `.odg`: DiaCraft's markup beside the shapes, with the ties as cross-shape formulas).

use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::container::{boxed, members, ties};
use crate::edit::{doc, drop_shape};
use crate::odg_build::Dir;
use crate::swimlane::{ids, role, structure};
use crate::{blank_session, run};

/// A page with a structure of three lanes and two phases, a shape in the second lane, the first lane's label rotated and its text typed. → the session
fn made() -> Session {
    let mut s = blank_session();
    let r = run(&mut s, "swimlane.create", json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 3, "phases": 2}));
    let lanes = ids(&r["lanes"]);
    drop_shape(&mut s, "rectangle", 4.0, 5.75);
    run(&mut s, "shape.set_text", json!({"id": lanes[0], "text": "Sales"}));
    run(&mut s, "swimlane.rotate_label", json!({"ids": [lanes[0]]}));
    run(&mut s, "swimlane.move", json!({"lane": lanes[2], "to": 0}));
    s
}

/// What the structure says of itself, the boxes and the ties of every shape, as one value to compare.
fn state(s: &mut Session) -> Json {
    let all: Vec<u32> = doc(s).pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|sh| sh.id).collect();
    // To six places: an `.odg` writes a length to six.
    let six = |v: f64| (v * 1.0e6).round() / 1.0e6;
    let boxes: Vec<Json> = all
        .iter()
        .map(|id| {
            let b = boxed(doc(s), *id);
            json!([six(b.0), six(b.1), six(b.2), six(b.3)])
        })
        .collect();
    let formulas: Vec<Json> = all.iter().map(|id| json!(ties(doc(s), *id))).collect();
    let roles: Vec<Json> = all.iter().map(|id| json!(role(doc(s), *id))).collect();
    let mut st = structure(s);
    // The boxes are told to six places in `boxes`; the structure's own are rounded the same way here.
    for part in ["lanes", "phases"] {
        if let Some(list) = st[part].as_array_mut() {
            for p in list {
                p["rect"] = Json::Null;
                if let Some(t) = p["thickness"].as_f64() {
                    p["thickness"] = json!(six(t));
                }
                if let Some(w) = p["width"].as_f64() {
                    p["width"] = json!(six(w));
                }
            }
        }
    }
    st["rect"] = Json::Null;
    st["length"] = json!(six(st["length"].as_f64().unwrap_or(0.0)));
    json!({"structure": st, "boxes": boxes, "ties": formulas, "roles": roles})
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

#[test]
fn a_structure_comes_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let want = state(&mut s);
        let dir = Dir::new();
        let path = dir.path(&format!("lanes.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got = state(&mut back);
        assert_eq!(got["roles"], want["roles"], "{ext}: what each shape is");
        assert_eq!(got["ties"], want["ties"], "{ext}: both sides of each tie, a list's members in their order");
        assert_eq!(got["boxes"], want["boxes"], "{ext}: the boxes");
        assert_eq!(got["structure"], want["structure"], "{ext}: the structure, its orientation, the labels and their sides");
    }
}

#[test]
fn a_structure_read_from_a_file_is_laid_out_again_and_saved_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut first = made();
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        // Read from the file: the lane list is in the order the file says; a lane is made thicker, a separator is dragged, the lanes are moved about and turned: no new shape.
        let mut s = reopened(&path);
        let st = structure(&mut s);
        let lanes: Vec<u32> = st["lanes"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect();
        let phases: Vec<u32> = st["phases"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect();
        run(&mut s, "swimlane.resize", json!({"id": lanes[1], "size": 2.0}));
        run(&mut s, "swimlane.resize", json!({"id": phases[0], "size": 2.0}));
        run(&mut s, "swimlane.move", json!({"lane": lanes[0], "to": 2}));
        run(&mut s, "swimlane.orientation", json!({"orientation": "vertical"}));
        let want = state(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        let got = state(&mut back);
        assert_eq!(got["ties"], want["ties"], "{ext}: the order after an edit in place");
        assert_eq!(got["boxes"], want["boxes"], "{ext}: the boxes after an edit in place");
        assert_eq!(got["structure"], want["structure"], "{ext}: the structure after an edit in place");
        // A lane laid out again after a shape went: no tie names the shape that is gone.
        let mut s = reopened(&path);
        // The shape is in the lane that was the second when the structure was made: the third now, since the last lane was moved to the front.
        let held = members(&mut s, lanes[2])[0];
        run(&mut s, "shape.delete", json!({"ids": [held]}));
        let want = state(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        assert_eq!(state(&mut back), want, "{ext}: a member deleted and saved in place");
    }
}

#[test]
fn a_lane_added_to_a_structure_read_from_a_file_is_saved_in_place_where_the_format_can_and_refused_in_words_where_it_cannot() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut first = made();
        let dir = Dir::new();
        let path = dir.path(&format!("add.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let mut s = reopened(&path);
        let new = u32::try_from(run(&mut s, "swimlane.insert", json!({}))["id"].as_u64().unwrap()).unwrap();
        assert_eq!(structure(&mut s)["lanes"].as_array().unwrap().len(), 4);
        let want = state(&mut s);
        if ext == "odg" {
            // An `.odg` keeps no number for a shape of the file, so a new lane's formula cannot name the list: the save says so, and Save As keeps everything.
            let e = s.execute("file.save", &json!({})).unwrap_err();
            assert!(e.to_string().contains("a formula of a new shape that names a shape of the file"), "{e} (lane {new})");
            let other = dir.path("add-as.diacraft");
            run(&mut s, "file.save_as", json!({ "path": other }));
            let mut back = reopened(&other);
            assert_eq!(state(&mut back), want, "odg: saved as a .diacraft drawing");
            // And undo makes the drawing saveable again.
            run(&mut s, "edit.undo", json!({}));
            run(&mut s, "file.save", json!({}));
        } else {
            run(&mut s, "file.save", json!({}));
            let mut back = reopened(&path);
            assert_eq!(state(&mut back), want, "{ext}: the new lane after an edit in place");
        }
    }
}

#[test]
fn an_odg_save_of_a_drawing_with_lanes_says_what_a_libreoffice_save_would_drop() {
    // OpenDocument has no list: the ties are cross-shape formulas and the settings are data DiaCraft keeps beside the shapes, and the save says so.
    let mut s = made();
    let dir = Dir::new();
    let path = dir.path("note.odg");
    let r = run(&mut s, "file.save_as", json!({ "path": path }));
    let notes = r["notes"].as_array().cloned().unwrap_or_default();
    assert!(
        notes.iter().any(|n| n["class"] == "libreoffice_would_drop" && n["cross_shape_formulas"].as_u64().unwrap_or(0) > 0),
        "the save note names the ties as cross-shape formulas: {r}"
    );
}

#[test]
fn a_template_drawing_saved_and_opened_again_in_each_format_keeps_its_lanes() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({ "template": "cross_functional_flowchart" }));
        let want = state(&mut s);
        let dir = Dir::new();
        let path = dir.path(&format!("template.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        assert_eq!(state(&mut back), want, "{ext}");
    }
}
