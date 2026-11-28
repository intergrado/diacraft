//! The page a command works on when it is not told which. A file lists its background pages first and a drawing a session makes keeps them first
//! So **the model's first page is a background page** in any drawing that has one, where the page a person works on is the first page that is not a background (the tabs,
//! `Document::first_foreground_page`). A command that was not given a page, through the CLI, MCP or the control channel, takes that page: an export draws it, a picture is put on it, a guide is
//! listed on it. A search for the page that holds a shape's `ID` looks at the pages that are not backgrounds first, since a shape's `ID` is the page's own and a background page and
//! another may both have a shape 1.

use diacraft_engine::Session;
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::native_common::Dir;
use crate::{bad_params_msg, blank_session, page_names, run};

/// A drawing whose model lists the background page `VBackground-1` (id 2) first, then `Page-1` (id 1).
fn with_a_background_page_first() -> Session {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"background": true}));
    assert_eq!(page_names(&s), ["VBackground-1", "Page-1"]);
    assert_eq!(doc(&s).pages.get(0).map(|p| p.id), Some(2), "the model's first page is the background page");
    s
}

#[test]
fn an_export_with_no_page_draws_the_first_page_that_is_not_a_background() {
    let dir = Dir::new();
    let mut s = with_a_background_page_first();
    for ext in ["svg", "png", "pdf"] {
        let out = dir.path(&format!("out.{ext}"));
        let made = run(&mut s, "file.export", json!({ "path": out }));
        assert_eq!(made["page"], json!({"id": 1, "name": "Page-1"}), "{ext}: the page that is not a background");
    }
    // A page that is named is the one exported, a background page too.
    let named = run(&mut s, "file.export", json!({"path": dir.path("named.svg"), "page": 2}));
    assert_eq!(named["page"]["id"], 2);
    // A drawing of background pages only has no other page to draw: the first there is.
    let mut only = with_a_background_page_first();
    only.edit("every page a background", |d| {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(1).expect("the second page"));
        page.background = true;
        Ok(())
    })
    .unwrap();
    assert!(doc(&only).pages.iter().all(|p| p.background));
    let made = run(&mut only, "file.export", json!({"path": dir.path("only.svg")}));
    assert_eq!(made["page"]["id"], 2, "the first page there is");
}

#[test]
fn a_picture_inserted_with_no_page_goes_on_the_first_page_that_is_not_a_background() {
    let dir = Dir::new();
    let mut s = with_a_background_page_first();
    let path = dir.write("logo.png", &crate::pictures::real_png(96, 48));
    let made = run(&mut s, "insert.picture", json!({ "path": path }));
    assert_eq!(made["page"], 1);
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 1, "on Page-1");
    assert_eq!(doc(&s).page(2).unwrap().shapes.len(), 0, "and not on the background page");
}

#[test]
fn the_guides_of_a_page_not_named_are_those_of_the_first_page_that_is_not_a_background() {
    let mut s = with_a_background_page_first();
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.0}));
    let listed = run(&mut s, "guide.list", json!({}));
    assert_eq!(listed["page"], 1);
    assert_eq!(listed["guides"].as_array().map(Vec::len), Some(1), "{listed}");
}

#[test]
fn the_field_choices_of_a_page_not_named_are_those_of_the_first_page_that_is_not_a_background() {
    let mut s = with_a_background_page_first();
    assert_eq!(run(&mut s, "text.field_choices", json!({}))["page"], 1);
}

#[test]
fn a_paste_whose_source_page_is_gone_goes_on_the_first_page_that_is_not_a_background() {
    let mut s = with_a_background_page_first();
    run(&mut s, "page.add", json!({"name": "Second"}));
    assert_eq!(page_names(&s), ["VBackground-1", "Page-1", "Second"]);
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // The shape is copied from page 3, which is then taken out: the paste has no page to go back to.
    let moved = run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 3, "x": 4.0, "y": 4.0}));
    let on_second = crate::edit::id_of(&moved);
    run(&mut s, "edit.copy", json!({"ids": [on_second], "page": 3}));
    run(&mut s, "page.delete", json!({"id": 3}));
    let pasted = run(&mut s, "edit.paste", json!({}));
    assert_eq!(pasted["page"], 1, "{pasted}");
    assert_eq!(doc(&s).page(2).unwrap().shapes.len(), 0, "nothing on the background page");
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 2, "the shape {a} and the paste");
}

#[test]
fn a_command_about_shapes_with_none_found_falls_back_to_the_first_page_that_is_not_a_background() {
    let mut s = with_a_background_page_first();
    // Nothing holds shape 999, so the page is the fallback, and the refusal names it.
    let e = s.execute("shape.behavior", &json!({"ids": [999]})).unwrap_err();
    assert_eq!(bad_params_msg(&e), "page 1 has no shape 999");
}

#[test]
fn a_shape_id_both_a_background_page_and_another_page_have_is_looked_for_on_the_pages_that_are_not_backgrounds_first() {
    let mut s = with_a_background_page_first();
    // Shape 1 of each page: the `ID` is the page's own.
    let on_page = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})));
    let on_background = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 2, "x": 3.0, "y": 3.0})));
    assert_eq!((on_page, on_background), (1, 1));
    let told = run(&mut s, "shape.behavior", json!({"ids": [1]}));
    assert_eq!(told["page"], 1, "the page a person works on, and not the background page the model lists first");
    // A shape only a background page holds is found there all the same.
    let only = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 2, "x": 5.0, "y": 5.0})));
    assert_eq!(only, 2);
    assert_eq!(run(&mut s, "shape.behavior", json!({"ids": [2]}))["page"], 2);
}

#[test]
fn a_variant_given_to_every_page_is_told_for_the_first_page_that_is_not_a_background() {
    // The answer of a command that works on every page says the theme and the variant of one page: the first that is not a background, and not the background the model lists first.
    let mut s = with_a_background_page_first();
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "page": 1}));
    run(&mut s, "theme.apply", json!({"theme": "fernwood", "page": 2}));
    let told = run(&mut s, "theme.variant", json!({"scope": "all", "colors": 1}));
    assert_eq!(told["theme"], "tidepool", "{told}");
    // A call that names a page, or none, is as it was.
    assert_eq!(run(&mut s, "theme.variant", json!({"page": 2, "colors": 2}))["theme"], "fernwood");
    assert_eq!(run(&mut s, "theme.variant", json!({"colors": 3}))["theme"], "tidepool");
}

#[test]
fn the_commands_that_already_took_the_first_page_that_is_not_a_background_still_do() {
    let mut s = with_a_background_page_first();
    assert_eq!(run(&mut s, "page.setup", json!({}))["page"], 1);
    assert_eq!(run(&mut s, "layer.list", json!({}))["page"], 1);
}
