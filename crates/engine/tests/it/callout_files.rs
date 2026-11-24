//! Callouts **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a file and edited, saved in its
//! own format (the splice). A callout is a shape whose ties are its `Relationships` cell and the target's, and whose leader is formulas of its `Scratch` row, so what must come back is the ties, the end of
//! the leader, the figure's formulas, and the behaviour: a target moved after the reopen takes its callout along and the end of the leader with it.

use std::sync::Arc;

use diacraft_doc::{SectionKey, Shape};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::odg_build::Dir;
use crate::{blank_session, run};

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn shapes_of(s: &Session) -> Vec<Arc<Shape>> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().cloned().collect()
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

/// A rectangle and an ellipse, a callout of each style that has a wedge or a leader tied to the rectangle, and a thought cloud tied to nothing.
fn made() -> Session {
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap();
    run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6.0, "y": 3.0}));
    let c = run(&mut s, "callout.insert", json!({"ids": [a], "style": "speech_round_box", "text": "Look here"}))["id"].as_u64().unwrap();
    run(&mut s, "callout.insert", json!({"ids": [a], "style": "box_line"}));
    run(&mut s, "callout.insert", json!({"x": 6.0, "y": 6.0, "style": "thought_cloud", "text": "Free"}));
    assert!(c > a);
    s
}

/// What a drawing says of its callouts, as one value to compare: for each shape that is a callout or has callouts, its ties, the gallery tile, the end of the leader, its text, and the formulas of the
/// cells of its `Scratch` row and of its figure.
fn state(s: &mut Session) -> Json {
    let page = page_id(s);
    let mut out = Vec::new();
    for shape in shapes_of(s) {
        let mut info = run(s, "callout.list", json!({"id": shape.id, "page": page}));
        // A recalculation after a reopen may differ in the last place of a float: the end of the leader is compared to a billionth of an inch.
        if let Some(tip) = info["tip"].as_array().cloned() {
            info["tip"] = json!(tip.iter().map(|n| (n.as_f64().unwrap() * 1.0e9).round() / 1.0e9).collect::<Vec<_>>());
        }
        if info["callouts"].as_array().is_none_or(|c| c.is_empty()) && !info["is_callout"].as_bool().unwrap_or(false) {
            continue;
        }
        let formulas = |name: &str| -> Vec<Json> {
            shape
                .sheet
                .sections
                .iter()
                .filter(|sec| sec.name.as_str() == name)
                .flat_map(|sec| sec.rows.iter().cloned().collect::<Vec<_>>())
                .flat_map(|row| row.cells.iter().map(|(n, c)| json!([n.as_str(), c.formula.text()])).collect::<Vec<_>>())
                .collect()
        };
        let user = shape.sheet.section(&SectionKey::new("User", None)).map(|sec| sec.rows.len());
        // The figure and the working of the end are the callout's own; a target's rectangle is its master's, which a file may hold as its own rows.
        let is_callout = info["is_callout"].as_bool().unwrap_or(false);
        out.push(json!({
            "id": shape.id,
            "info": info,
            "text": diacraft_engine::draw::text_view(s.document().unwrap(), page, shape.id).map(|t| t.chars),
            "relationships": shape.sheet.cell("Relationships").and_then(|c| c.formula.text().map(str::to_owned)),
            "scratch": if is_callout { formulas("Scratch") } else { Vec::new() },
            "geometry": if is_callout { formulas("Geometry") } else { Vec::new() },
            "user_rows": user,
        }));
    }
    Json::Array(out)
}

/// The id of the first shape that has callouts, and the first callout of it.
fn target_and_callout(s: &mut Session) -> (u32, u32) {
    let page = page_id(s);
    for shape in shapes_of(s) {
        let info = run(s, "callout.list", json!({"id": shape.id, "page": page}));
        if let Some(c) = info["callouts"].as_array().and_then(|c| c.first()) {
            return (shape.id, c.as_u64().unwrap() as u32);
        }
    }
    panic!("no shape has a callout");
}

fn pin(s: &Session, id: u32) -> (f64, f64) {
    let d = s.document().unwrap();
    let found = |name: &str| {
        let res = diacraft_doc::Resolver::new(d);
        let at = d.find_shape(diacraft_doc::Owner::Page(page_id(s)), id).unwrap();
        res.resolve(&diacraft_doc::SheetRef::Shape(at), &diacraft_doc::CellAddr::direct(name))
            .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
            .unwrap()
    };
    (found("PinX"), found("PinY"))
}

/// A target moved after a reopen takes its callout along and the end of the leader with it: the formulas that worked it out are alive.
fn assert_follows(ext: &str, s: &mut Session) {
    let (t, c) = target_and_callout(s);
    let page = page_id(s);
    let (t0, c0) = (pin(s, t), pin(s, c));
    let tip0 = run(s, "callout.list", json!({"id": c, "page": page}))["tip"].clone();
    run(s, "shape.move", json!({"ids": [t], "page": page, "dx": 1.25, "dy": -0.5}));
    let (t1, c1) = (pin(s, t), pin(s, c));
    assert!((t1.0 - t0.0 - 1.25).abs() < 1e-6 && (t1.1 - t0.1 + 0.5).abs() < 1e-6, "{ext}: the target moved");
    assert!((c1.0 - c0.0 - 1.25).abs() < 1e-6 && (c1.1 - c0.1 + 0.5).abs() < 1e-6, "{ext}: the callout went with it: {c0:?} -> {c1:?}");
    let tip1 = run(s, "callout.list", json!({"id": c, "page": page}))["tip"].clone();
    assert!(
        (tip1[0].as_f64().unwrap() - tip0[0].as_f64().unwrap() - 1.25).abs() < 1e-6
            && (tip1[1].as_f64().unwrap() - tip0[1].as_f64().unwrap() + 0.5).abs() < 1e-6,
        "{ext}: the end of the leader went with it: {tip0} -> {tip1}"
    );
}

#[test]
fn callouts_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let want = state(&mut s);
        assert_eq!(want.as_array().unwrap().len(), 4, "the rectangle and its two callouts and the free one");
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got = state(&mut back);
        for (g, w) in got.as_array().unwrap().iter().zip(want.as_array().unwrap()) {
            for key in w.as_object().unwrap().keys() {
                assert_eq!(g[key], w[key], "{ext}: shape {}: {key}", w["id"]);
            }
        }
        assert_eq!(got, want, "{ext}: the ties, the tiles, the ends of the leaders, the text and the formulas");
        assert_follows(ext, &mut back);
    }
}

#[test]
fn a_drawing_read_from_a_file_and_given_callouts_keeps_them_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut plain = blank_session();
        run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
        run(&mut plain, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6.0, "y": 3.0}));
        let dir = Dir::new();
        let source = dir.path(&format!("source.{ext}"));
        run(&mut plain, "file.save_as", json!({ "path": source }));
        let mut s = reopened(&source);
        let page = page_id(&s);
        // A shape of the session, a callout for it, a free one, and (but for an .odg, whose splice keeps no ID of a file's shape for a formula to name, and says so: the test below) a callout for a
        // shape the file held.
        let new = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 9.0, "y": 6.0}))["id"].as_u64().unwrap();
        run(&mut s, "callout.insert", json!({"ids": [new], "page": page, "style": "left_bracket"}));
        run(&mut s, "callout.insert", json!({"page": page, "x": 6.0, "y": 6.0, "style": "oval_line"}));
        if ext != "odg" {
            let first = shapes_of(&s)[0].id;
            run(&mut s, "callout.insert", json!({"ids": [first], "page": page, "style": "box_line"}));
        }
        let want = state(&mut s);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&source);
        assert_eq!(state(&mut back), want, "{ext}: saved in its own format");
        assert_follows(ext, &mut back);
    }
}

#[test]
fn a_callout_for_a_shape_an_odg_file_held_is_refused_at_the_save_in_words_and_one_undo_makes_it_saveable() {
    let mut plain = blank_session();
    run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    let dir = Dir::new();
    let source = dir.path("source.odg");
    run(&mut plain, "file.save_as", json!({ "path": source }));
    let mut s = reopened(&source);
    let page = page_id(&s);
    let first = shapes_of(&s)[0].id;
    run(&mut s, "callout.insert", json!({"ids": [first], "page": page}));
    let refusal = s.execute("file.save", &json!({})).expect_err("the splice cannot name a shape of the file").to_string();
    assert!(refusal.contains("names a shape of the file") && refusal.contains("edit.undo"), "{refusal}");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "file.save", json!({}));
}

#[test]
fn a_callout_deleted_or_untied_before_the_save_is_not_named_by_the_file() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let (t, c) = target_and_callout(&mut s);
        run(&mut s, "shape.delete", json!({"ids": [c]}));
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let page = page_id(&back);
        let info = run(&mut back, "callout.list", json!({"id": t, "page": page}));
        assert_eq!(info["callouts"].as_array().unwrap().len(), 1, "{ext}: the second callout is left, the deleted one is not named");
        assert!(!info["callouts"].to_string().contains(&c.to_string()), "{ext}: {info}");
    }
}
