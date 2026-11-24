//! What a theme colour pick writes is **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a
//! drawing read from a file and edited, saved in its own format (the splice). What must come back is the colour (what the picture draws) and the formula (so that the colour still
//! follows the theme: a theme applied after the reopen recolours it). An `.odg` holds the colour as a value, with the formula carried.

use diacraft_doc::{FormulaSlot, Owner};
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

fn look(s: &mut Session, id: u32) -> Value {
    let page = page_id(s);
    run(s, "shape.formatting", json!({"ids": [id], "page": page}))["shapes"][0].clone()
}

/// The formula of a cell directly under shape `id`, if it holds one of its own.
fn formula(s: &Session, id: u32, name: &str) -> Option<String> {
    let d = s.document().unwrap();
    let page = page_id(s);
    let shape = d.shape(&d.find_shape(Owner::Page(page), id)?)?;
    match &shape.sheet.cell(name)?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

fn palette_hex(s: &mut Session, pick: &str) -> Value {
    let page = page_id(s);
    let p = run(s, "theme.colors", json!({"page": page}));
    let columns = p["theme_colors"].as_array().unwrap();
    columns.iter().flat_map(|c| c["swatches"].as_array().unwrap().iter()).find(|sw| sw["pick"] == pick).unwrap()["hex"].clone()
}

/// A rectangle on a themed page, with a theme fill, a theme line and theme text, and a second with the fixed colours.
fn made() -> (Session, [u32; 2]) {
    let mut s = blank_session();
    let ids: Vec<u32> = [3.0, 6.0]
        .iter()
        .map(|x| run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": *x, "y": 3.0}))["id"].as_u64().unwrap() as u32)
        .collect();
    run(&mut s, "shape.set_text", json!({"id": ids[0], "text": "Colour"}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "shape.format", json!({"ids": [ids[0]], "fill": "theme:accent2:40", "line": "theme:dark"}));
    run(&mut s, "text.format", json!({"ids": [ids[0]], "color": "theme:accent3"}));
    run(&mut s, "shape.format", json!({"ids": [ids[1]], "fill": "#336699"}));
    (s, [ids[0], ids[1]])
}

const FILL: &str = r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))"#;

#[test]
fn a_theme_pick_comes_back_from_a_new_drawing_saved_in_each_format_and_still_follows_the_theme() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut s, [a, _]) = made();
        let (fill, line, color) = {
            let l = look(&mut s, a);
            (l["fill"].clone(), l["line"].clone(), l["color"].clone())
        };
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let ids = shape_ids(&back);
        let (a, b) = (ids[0], ids[1]);
        let l = look(&mut back, a);
        assert_eq!((l["fill"].clone(), l["line"].clone(), l["color"].clone()), (fill.clone(), line, color), "{ext}: the colours come back");
        assert_eq!(look(&mut back, b)["fill"], "#336699", "{ext}");
        // The formula is carried, so that the colour is still the theme's.
        assert_eq!(formula(&back, a, "FillForegnd").as_deref(), Some(FILL), "{ext}: the formula comes back");
        // And a theme applied after the reopen recolours the pick and not the fixed colour.
        run(&mut back, "theme.apply", json!({"theme": "cinderglow"}));
        let want = palette_hex(&mut back, "theme:accent2:40");
        let l = look(&mut back, a);
        assert_ne!(l["fill"], fill, "{ext}: the fill followed the theme");
        assert_eq!(l["fill"], want, "{ext}: to the colour the new theme's palette shows");
        assert_eq!(look(&mut back, b)["fill"], "#336699", "{ext}: the fixed colour stayed");
        let _ = (a, b);
    }
}

#[test]
fn a_theme_pick_made_on_a_drawing_read_from_a_file_is_kept_by_a_save_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut plain = blank_session();
        for x in [3.0, 6.0] {
            let id = run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": 3.0}))["id"].as_u64().unwrap();
            run(&mut plain, "shape.set_text", json!({"id": id, "text": "Colour"}));
        }
        run(&mut plain, "theme.apply", json!({"theme": "plumline"}));
        let dir = Dir::new();
        let source = dir.path(&format!("source.{ext}"));
        run(&mut plain, "file.save_as", json!({ "path": source }));
        let mut s = reopened(&source);
        let page = page_id(&s);
        let ids = shape_ids(&s);
        run(&mut s, "shape.format", json!({"ids": [ids[0]], "page": page, "fill": "theme:accent4:-25", "line": "theme:accent1"}));
        // A text colour from the theme is a formula in a run's row: an `.odg` is not written in place with one (the test below says what Save says).
        if ext != "odg" {
            run(&mut s, "text.format", json!({"ids": [ids[0]], "page": page, "color": "theme:light"}));
        }
        let want = look(&mut s, ids[0]);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&source);
        let ids = shape_ids(&back);
        let got = look(&mut back, ids[0]);
        for key in ["fill", "line", "color"] {
            assert_eq!(got[key], want[key], "{ext}: {key}");
        }
        if ext != "odg" {
            assert_eq!(formula_in_text(&back, ids[0]).as_deref(), Some(r#"THEMEGUARD(THEMEVAL("Light"))"#), "{ext}: the text colour's formula");
        }
        assert_eq!(formula(&back, ids[0], "FillForegnd").as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor4"),-25))"#), "{ext}");
        // Still the theme's: another theme recolours it.
        run(&mut back, "theme.apply", json!({"theme": "fernwood"}));
        assert_ne!(look(&mut back, ids[0])["fill"], want["fill"], "{ext}: followed the theme");
    }
}

#[test]
fn use_default_comes_back_from_each_format_as_the_themes_own_colour() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut s, [a, _]) = made();
        run(&mut s, "shape.format", json!({"ids": [a], "fill": "#00ff00"}));
        run(&mut s, "shape.format", json!({"ids": [a], "fill": "default"}));
        let want = look(&mut s, a)["fill"].clone();
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let ids = shape_ids(&back);
        assert_eq!(look(&mut back, ids[0])["fill"], want, "{ext}");
        run(&mut back, "theme.apply", json!({"theme": "inkstone"}));
        assert_ne!(look(&mut back, ids[0])["fill"], want, "{ext}: it follows the theme");
    }
}

#[test]
fn an_odg_made_from_a_new_drawing_loses_no_formula_of_a_theme_pick() {
    let mut s = blank_session();
    let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Colour"}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:accent2:40", "line": "theme:dark"}));
    run(&mut s, "text.format", json!({"ids": [id], "color": "theme:accent3"}));
    let dir = Dir::new();
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("x.odg") }));
    let classes: Vec<&str> = saved["checked"]["classes"].as_array().unwrap().iter().map(|c| c["class"].as_str().unwrap()).collect();
    assert!(!classes.contains(&"formulas"), "no formula is lost: {classes:?}");
    // The same drawing with a fixed colour does lose the formula `RGB(r,g,b)` as before (that is not what this task writes).
    let mut s = blank_session();
    let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#336699"}));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("y.odg") }));
    let classes: Vec<&str> = saved["checked"]["classes"].as_array().unwrap().iter().map(|c| c["class"].as_str().unwrap()).collect();
    assert!(classes.contains(&"formulas"), "the fixed colour's formula is a value in an odg: {classes:?}");
}

/// The formula of the first run's `Color` of shape `id`'s text rows, if it holds one.
fn formula_in_text(s: &Session, id: u32) -> Option<String> {
    use diacraft_doc::{CellAddr, Resolver, RowKey, SheetRef};
    let d = s.document().unwrap();
    let page = page_id(s);
    let at = d.find_shape(Owner::Page(page), id)?;
    let res = Resolver::new(d);
    let found = res.resolve(&SheetRef::Shape(at), &CellAddr::in_row("Character", None, RowKey::Ix(0), "Color"))?;
    match &found.formula.cell?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

#[test]
fn a_theme_text_colour_in_a_drawing_opened_from_an_odg_is_refused_by_save_in_words_and_save_as_keeps_it() {
    let mut plain = blank_session();
    let id = run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap();
    run(&mut plain, "shape.set_text", json!({"id": id, "text": "Colour"}));
    let dir = Dir::new();
    let source = dir.path("source.odg");
    run(&mut plain, "file.save_as", json!({ "path": source }));
    let mut s = reopened(&source);
    let (page, ids) = (page_id(&s), shape_ids(&s));
    run(&mut s, "text.format", json!({"ids": [ids[0]], "page": page, "color": "theme:accent3"}));
    let e = s.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(e.contains("a run's or a paragraph's cell that holds a formula") && e.contains("Save As"), "{e}");
    // A save as a `.diacraft` file holds it, and the colour still follows a theme there.
    let path = dir.path("kept.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = reopened(&path);
    let ids = shape_ids(&back);
    assert_eq!(formula_in_text(&back, ids[0]).as_deref(), Some(r#"THEMEGUARD(THEMEVAL("AccentColor3"))"#));
    run(&mut back, "theme.apply", json!({"theme": "plumline"}));
    let page = page_id(&back);
    let p = run(&mut back, "theme.colors", json!({"page": page}));
    let swatches = p["theme_colors"].as_array().unwrap().iter().flat_map(|c| c["swatches"].as_array().unwrap().iter());
    let want = swatches.clone().find(|sw| sw["pick"] == "theme:accent3").unwrap()["hex"].clone();
    assert_eq!(look(&mut back, ids[0])["color"], want);
}
