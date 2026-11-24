//! Containers **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a
//! file and saved in its own format (the splice). A container is cells: `User.msvStructureType` and the settings rows, the `Relationships` cell on both sides of each tie, and its
//! own geometry; a file holds them in the format's own way (VSDX: the User and Shape Layout sections; `.diacraft`: the model; `.odg`: DiaCraft's markup beside the shapes, since
//! OpenDocument has no container, and the save note says what a LibreOffice save would drop).

use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::container::{add_container, boxed, info, members, ties};
use crate::edit::{doc, drop_shape};
use crate::odg_build::Dir;
use crate::{blank_session, run};

/// A page with two squares round which a container with a heading is put (locked, with a wide margin and its heading on the left), and a second container inside the first.
fn made() -> Session {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = add_container(&mut s, json!({"ids": [a, b], "heading": "Stage", "style": "tint-strip"}));
    run(&mut s, "container.margin", json!({"ids": [c], "margin": "wide"}));
    run(&mut s, "container.heading", json!({"ids": [c], "side": "left"}));
    run(&mut s, "container.auto_resize", json!({"ids": [c], "mode": "fit"}));
    let inner = add_container(&mut s, json!({"x": 4.5, "y": 4.0, "width": 1.0, "height": 1.0, "heading": "Inner"}));
    run(&mut s, "container.lock", json!({"ids": [c], "locked": true}));
    assert_eq!(members(&mut s, c).len(), 3, "the two squares and the inner container: {:?} {inner}", info(&mut s, &[c]));
    s
}

/// What a container says of itself and its box, as one value to compare.
fn state(s: &mut Session) -> Json {
    let ids: Vec<u32> = doc(s).pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|sh| sh.id).collect();
    let infos = info(s, &ids);
    // To six places: an `.odg` writes a length to six, and a fit works one out to the last bit of a double.
    let six = |v: f64| (v * 1.0e6).round() / 1.0e6;
    let boxes: Vec<Json> = ids
        .iter()
        .map(|id| {
            let b = boxed(doc(s), *id);
            json!([six(b.0), six(b.1), six(b.2), six(b.3)])
        })
        .collect();
    let formulas: Vec<Json> = ids.iter().map(|id| json!(ties(doc(s), *id))).collect();
    json!({"containers": infos, "boxes": boxes, "ties": formulas})
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

#[test]
fn containers_and_their_ties_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let want = state(&mut s);
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got = state(&mut back);
        assert_eq!(got["containers"], want["containers"], "{ext}: the containers, their settings and members");
        assert_eq!(got["ties"], want["ties"], "{ext}: both sides of each tie");
        assert_eq!(got["boxes"], want["boxes"], "{ext}: the boxes");
    }
}

#[test]
fn a_container_read_from_a_file_and_edited_keeps_its_settings_and_ties_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut first = made();
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        // Read from the file: the container is unlocked, its margin is narrower and it fits its members, and the drawing is saved in place.
        let mut s = reopened(&path);
        let c = first_container(&mut s);
        run(&mut s, "container.lock", json!({"ids": [c], "locked": false}));
        run(&mut s, "container.margin", json!({"ids": [c], "margin": "narrow"}));
        run(&mut s, "container.fit", json!({"ids": [c]}));
        let want = state(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        assert_eq!(state(&mut back), want, "{ext}: the settings and the box after an edit in place");
        // A member deleted: no tie names it in the file that was saved.
        let mut s = reopened(&path);
        let c = first_container(&mut s);
        let gone = members(&mut s, c)[0];
        run(&mut s, "shape.delete", json!({"ids": [gone]}));
        let want = state(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        assert_eq!(state(&mut back), want, "{ext}: a member deleted and saved in place");
        let at = back.document().unwrap();
        let text: String = at.pages.iter().flat_map(|p| p.shapes.iter()).map(|sh| ties(at, sh.id)).collect::<Vec<_>>().join(" ");
        assert!(!text.contains(&format!("Sheet.{gone}!")), "{ext}: a tie still names the deleted shape: {text}");
    }
}

#[test]
fn a_shape_dropped_into_a_container_read_from_a_file_is_saved_in_place_where_the_format_can_and_refused_in_words_where_it_cannot() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut first = made();
        let dir = Dir::new();
        let path = dir.path(&format!("drop.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let mut s = reopened(&path);
        let c = first_container(&mut s);
        run(&mut s, "container.lock", json!({"ids": [c], "locked": false}));
        let extra = drop_shape(&mut s, "rectangle", 3.5, 4.2);
        assert!(members(&mut s, c).contains(&extra), "{ext}: dropped inside the container read from the file");
        run(&mut s, "container.heading", json!({"ids": [c], "rotate": true}));
        let want = state(&mut s);
        if ext == "odg" {
            // An `.odg` keeps no number for a shape of the file, so a new shape's formula cannot name the container: the save says so, and Save As keeps everything.
            let e = s.execute("file.save", &json!({})).unwrap_err();
            assert!(e.to_string().contains("a formula of a new shape that names a shape of the file"), "{e}");
            let other = dir.path("drop-as.diacraft");
            run(&mut s, "file.save_as", json!({ "path": other }));
            let mut back = reopened(&other);
            assert_eq!(state(&mut back), want, "odg: saved as a .diacraft drawing");
        } else {
            run(&mut s, "file.save", json!({}));
            let mut back = reopened(&path);
            assert_eq!(state(&mut back), want, "{ext}: the new member and the heading after an edit in place");
        }
    }
}

/// The outermost container of the first page: the one no other container holds.
fn first_container(s: &mut Session) -> u32 {
    let ids: Vec<u32> = doc(s).pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|sh| sh.id).collect();
    for c in info(s, &ids) {
        if c["inside"].as_array().is_some_and(Vec::is_empty) && c["members"].as_array().is_some_and(|m| m.len() >= 3) {
            return u32::try_from(c["id"].as_u64().unwrap()).unwrap();
        }
    }
    panic!("no outer container among {ids:?}");
}

#[test]
fn an_odg_save_of_a_drawing_with_a_container_says_what_a_libreoffice_save_would_drop() {
    // OpenDocument has no container: the ties are cross-shape formulas and the settings are data DiaCraft keeps beside the shapes (`odfile/note.rs`), and the save says so.
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
