//! What the transparency, pattern, gradient, shadow, cap, compound and Quick Style commands write is **saved and opened again in the three formats**
//! (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a file and edited, saved in its own format (the splice). What must
//! come back is what `shape.formatting` says of each shape, which is what the picture draws; a Quick Style's shapes keep following the theme.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::{blank_session, run};

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

fn shape_ids(s: &Session) -> Vec<u32> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().map(|shape| shape.id).collect()
}

/// What the shapes look like, as `shape.formatting` says: the fill, line, transparency, pattern, gradient and shadow of each, to compare after a save.
fn looks(s: &mut Session) -> Vec<Value> {
    let page = page_id(s);
    let ids = shape_ids(s);
    let mut out = run(s, "shape.formatting", json!({"ids": ids, "page": page}))["shapes"].as_array().cloned().unwrap();
    for look in &mut out {
        // The text's own face and size are not what this task writes.
        if let Some(map) = look.as_object_mut() {
            for key in [
                "font",
                "size",
                "bold",
                "italic",
                "underline",
                "strike",
                "color",
                "align",
                "valign",
                "bullet",
                "ind_left",
                "case",
                "direction",
                "vertical",
            ] {
                map.remove(key);
            }
        }
    }
    out
}

/// Four shapes, each formatted one way: a transparent fill and line with a cap, a pattern, a gradient with three stops, and a shadow.
fn made() -> Session {
    let mut s = blank_session();
    let ids: Vec<u32> = [2.0, 4.5, 7.0, 9.5]
        .iter()
        .map(|x| run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": *x, "y": 3.0}))["id"].as_u64().unwrap() as u32)
        .collect();
    run(
        &mut s,
        "shape.format",
        json!({"ids": [ids[0]], "fill": "#336699", "fill_transparency": 35, "line": "#CC0000", "weight": 3, "line_transparency": 20, "line_cap": "square", "compound": "double"}),
    );
    run(&mut s, "shape.format", json!({"ids": [ids[1]], "fill": "#202020", "fill_pattern": 4, "fill_background": "#FFFF00"}));
    run(
        &mut s,
        "shape.gradient",
        json!({"ids": [ids[2]], "angle": 60, "stops": [
            {"position": 0, "color": "#FF0000"}, {"position": 0.4, "color": "#00FF00", "transparency": 25}, {"position": 1, "color": "#0000FF"}]}),
    );
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [ids[3]], "preset": "bottom_right", "color": "#202020", "transparency": 45, "blur": 6, "distance": 9, "angle": 70}),
    );
    s
}

/// The values that must be the same after a save and a read: what the controls show, which is what is drawn.
fn same(ext: &str, got: &[Value], want: &[Value]) {
    assert_eq!(got.len(), want.len(), "{ext}: the shapes");
    for (g, w) in got.iter().zip(want) {
        for key in [
            "fill",
            "line",
            "weight",
            "fill_pattern",
            "fill_transparency",
            "fill_background",
            "line_transparency",
            "line_cap",
            "compound",
            "gradient",
            "line_gradient",
            "shadow",
        ] {
            assert_eq!(g[key], w[key], "{ext}: shape {}: {key}", w["id"]);
        }
    }
}

#[test]
fn the_formatting_comes_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let want = looks(&mut s);
        assert!(
            want[2]["gradient"].is_object() && want[3]["shadow"].is_object() && want[1]["fill_pattern"] == json!(4),
            "the shapes have what they were given: {want:?}"
        );
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        same(ext, &looks(&mut back), &want);
    }
}

#[test]
fn the_formatting_comes_back_from_a_drawing_read_from_a_file_and_edited_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut plain = blank_session();
        for x in [2.0, 4.5, 7.0, 9.5] {
            run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": 3.0}));
        }
        let dir = Dir::new();
        let source = dir.path(&format!("source.{ext}"));
        run(&mut plain, "file.save_as", json!({ "path": source }));
        let mut s = reopened(&source);
        let page = page_id(&s);
        let ids = shape_ids(&s);
        run(
            &mut s,
            "shape.format",
            json!({"ids": [ids[0]], "page": page, "fill": "#336699", "fill_transparency": 35, "line": "#CC0000", "weight": 3, "line_transparency": 20, "line_cap": "square", "compound": "double"}),
        );
        run(&mut s, "shape.format", json!({"ids": [ids[1]], "page": page, "fill": "#202020", "fill_pattern": 4, "fill_background": "#FFFF00"}));
        run(
            &mut s,
            "shape.gradient",
            json!({"ids": [ids[2]], "page": page, "angle": 60, "stops": [
                {"position": 0, "color": "#FF0000"}, {"position": 0.4, "color": "#00FF00", "transparency": 25}, {"position": 1, "color": "#0000FF"}]}),
        );
        run(
            &mut s,
            "shape.shadow",
            json!({"ids": [ids[3]], "page": page, "preset": "bottom_right", "color": "#202020", "transparency": 45, "blur": 6, "distance": 9, "angle": 70}),
        );
        let want = looks(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&source);
        same(ext, &looks(&mut back), &want);
    }
}

#[test]
fn a_quick_style_keeps_following_the_theme_after_a_save_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = blank_session();
        let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
        run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
        let b = run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
        run(&mut s, "shape.quick_style", json!({"ids": [a, b], "row": 3, "column": 4}));
        let want = looks(&mut s);
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        same(ext, &looks(&mut back), &want);
        // They still follow the theme: a variant moves them in the file that was read, as in the session that made them.
        run(&mut s, "theme.variant", json!({"variant": 2}));
        run(&mut back, "theme.variant", json!({"variant": 2}));
        same(&format!("{ext} after a variant"), &looks(&mut back), &looks(&mut s));
    }
}
