//! A connector's path edited by hand and its reroute setting, **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): the route the
//! legs make and the setting a person chose come back as they were saved, from a new drawing (the model writers) and from a drawing read from a file and saved in its own
//! format (the splice).

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::draw::{Reroute, connector_path, connector_reroute};
use serde_json::json;

use crate::connect_style::z_pair;
use crate::odg_build::Dir;
use crate::run;

type P = (f64, f64);

/// The one connector of the first page, by its page and `ID`.
fn connector_of(doc: &Document) -> (u32, u32) {
    let page = doc.pages.get(0).expect("a page");
    let found: Vec<u32> = page.shapes.iter().map(|s| s.id).filter(|id| connector_path(doc, page.id, *id).is_some()).collect();
    assert_eq!(found.len(), 1, "one connector on the page: {found:?}");
    (page.id, found[0])
}

fn route_of(doc: &Document) -> Vec<P> {
    let (page, id) = connector_of(doc);
    connector_path(doc, page, id).expect("a connector with a path of lines").points
}

fn mode_of(doc: &Document) -> Option<Reroute> {
    let (page, id) = connector_of(doc);
    connector_reroute(doc, page, id)
}

/// The two routes are the same corners, to a ten-thousandth of an inch (the `.odg` writes centimetres to a few decimals).
fn same_route(got: &[P], want: &[P], what: &str) {
    assert!(
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1.0e-3 && (g.1 - w.1).abs() < 1.0e-3),
        "{what}: got {got:?}, want {want:?}"
    );
}

/// Opens the file `path` in a session of its own.
fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// A Z whose middle leg was moved by hand to x = 7 (the path of five corners is four, three legs), then `mode` set.
fn edited(mode: &str) -> (Session, Vec<P>) {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": mode}));
    let route = route_of(s.document().unwrap());
    assert_eq!(route.len(), 6, "a jog at the begin: {route:?}");
    (s, route)
}

#[test]
fn a_hand_edited_path_and_its_reroute_setting_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        for (mode, want) in [("on_crossover", Reroute::OnCrossover), ("never", Reroute::Never), ("as_needed", Reroute::AsNeeded)] {
            let (mut s, route) = edited(mode);
            let dir = Dir::new();
            let path = dir.path(&format!("drawing.{ext}"));
            run(&mut s, "file.save_as", json!({ "path": path }));
            let back = reopened(&path);
            let doc = back.document().unwrap();
            same_route(&route_of(doc), &route, &format!("{ext} {mode}"));
            assert_eq!(mode_of(doc), Some(want), "{ext}: the setting {mode} is kept");
        }
    }
}

#[test]
fn a_drawing_read_from_a_file_and_edited_keeps_the_path_and_the_setting_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        // A file of the format, from a drawing that holds a connector as it is routed.
        let (mut first, _, _, _) = z_pair();
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        // Read from the file, a leg is moved by hand and the setting chosen, and the drawing is saved in place.
        let mut s = reopened(&path);
        let (page, id) = connector_of(s.document().unwrap());
        let before = route_of(s.document().unwrap());
        assert_eq!(before.len(), 4, "{ext}: the Z as the file holds it: {before:?}");
        run(&mut s, "connector.edit_path", json!({"id": id, "leg": 1, "to": 7.0, "page": page}));
        run(&mut s, "connector.reroute", json!({"ids": [id], "mode": "never", "page": page}));
        let route = route_of(s.document().unwrap());
        run(&mut s, "file.save", json!({}));
        let back = reopened(&path);
        same_route(&route_of(back.document().unwrap()), &route, &format!("{ext}: saved in place"));
        assert_eq!(mode_of(back.document().unwrap()), Some(Reroute::Never), "{ext}: the setting is kept");
    }
}

#[test]
fn a_leg_dragged_after_a_reopen_is_one_more_edit_and_the_next_save_keeps_it() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut first, _, _, _) = z_pair();
        let dir = Dir::new();
        let path = dir.path(&format!("again.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let mut s = reopened(&path);
        let (page, id) = connector_of(s.document().unwrap());
        run(&mut s, "connector.edit_path", json!({"id": id, "leg": 1, "to": 7.0, "page": page}));
        run(&mut s, "file.save", json!({}));
        // And once more, a later edit of the same drawing.
        let mut s = reopened(&path);
        let (page, id) = connector_of(s.document().unwrap());
        run(&mut s, "connector.edit_path", json!({"id": id, "leg": 1, "to": 6.5, "page": page}));
        let route = route_of(s.document().unwrap());
        run(&mut s, "file.save", json!({}));
        let back = reopened(&path);
        same_route(&route_of(back.document().unwrap()), &route, &format!("{ext}: second edit"));
        assert!(route.iter().any(|p| (p.0 - 6.5).abs() < 1.0e-6), "{ext}: the leg is at 6.5: {route:?}");
    }
}
