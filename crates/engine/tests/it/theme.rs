//! Themes: the catalogue, `theme.apply` and `theme.variant` through `Session::execute` as the control channel calls them, what a shape then
//! draws, and what a save writes. The colours are worked out here by the own words (tint and shade in linear light), not by the code under test.
//! The drawings are made in code; no theme and no file from another application is used.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::Owner;
use diacraft_engine::theme::{self, THEME_STYLE_NAME};
use diacraft_engine::{EngineError, Session};
use diacraft_opc::{Limits, Package};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape};
use crate::format::{close, px, render};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

// ---- colours, from the words of the spec ------------------------------------------------------------------------------------------------

fn lin(c: u8) -> f64 {
    let x = f64::from(c) / 255.0;
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn enc(l: f64) -> u8 {
    let x = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (x * 255.0).round().clamp(0.0, 255.0) as u8
}

/// `tint` of `k` (0 to 1): each channel in linear light, `lin(c) * k + (1 - k)`.
fn tint(rgb: (u8, u8, u8), k: f64) -> (u8, u8, u8) {
    let t = |c: u8| enc(lin(c) * k + (1.0 - k));
    (t(rgb.0), t(rgb.1), t(rgb.2))
}

/// `shade` of `k`: `lin(c) * k`.
fn shade(rgb: (u8, u8, u8), k: f64) -> (u8, u8, u8) {
    let t = |c: u8| enc(lin(c) * k);
    (t(rgb.0), t(rgb.1), t(rgb.2))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

// Tidepool's accents, as the catalogue lists them: the first four of its palette are 1F8CA8, 2B5FA8, 4FB39A and E5B04B.
const TIDEPOOL_1: &str = "1F8CA8";
const TIDEPOOL_2: &str = "2B5FA8";

// ---- helpers ----------------------------------------------------------------------------------------------------------------------------

fn session_with_rectangle() -> (Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    (s, id)
}

/// What shape `id` resolves to: `shape.formatting`'s entry.
fn look(s: &mut Session, id: u32) -> Value {
    run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0].clone()
}

fn page_theme(s: &mut Session, page: u32) -> Value {
    run(s, "theme.list", json!({"page": page}))["page"].clone()
}

fn failed_msg(e: &EngineError) -> &str {
    match e {
        EngineError::Failed { msg, .. } => msg,
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-theme-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- the catalogue ----------------------------------------------------------------------------------------------------------------------

#[test]
fn the_catalogue_is_five_original_themes_with_four_variants_each() {
    let themes = theme::catalogue();
    assert_eq!(themes.iter().map(|t| t.id).collect::<Vec<_>>(), ["tidepool", "fernwood", "cinderglow", "inkstone", "plumline"]);
    assert_eq!(themes.iter().map(|t| t.scheme()).collect::<Vec<_>>(), [1001, 1002, 1003, 1004, 1005], "far from the 33 to 60");
    for t in themes {
        // The part reads back as the definition (the writer's own test, here for the shipped ones), with the five components all carrying the number.
        let read = t.theme("/visio/theme/theme1.xml");
        assert_eq!(read.entries, t.def.entries, "{}", t.id);
        for id in [&read.color_scheme_id, &read.font_scheme_id, &read.effect_scheme_id, &read.connector_scheme_id, &read.theme_scheme_id] {
            assert_eq!(id.as_ref().map(|i| i.scheme_enum), Some(t.scheme()), "{}", t.id);
        }
        // Four variants of each kind, seven colours in each colour variant, four styles in each style variant, one variant monotone.
        let variants = &read.entries.colour_variants;
        assert_eq!(variants.len(), 4, "{}", t.id);
        assert!(variants.iter().all(|v| v.as_ref().unwrap().colours.iter().all(Option::is_some)), "{}", t.id);
        assert_eq!(variants.iter().filter(|v| v.as_ref().unwrap().monotone).count(), 1, "{}", t.id);
        assert_eq!(read.entries.style_variants.len(), 4);
        assert!(read.entries.style_variants.iter().all(|v| v.as_ref().unwrap().styles.len() == 4));
        // The lengths real parts have, and one more line extension than lines.
        let e = &read.entries;
        assert_eq!((e.effect.fills.len(), e.effect.lines.len(), e.effect.effects.len()), (6, 6, 6), "{}", t.id);
        assert_eq!((e.connector.fills.len(), e.connector.lines.len(), e.connector.effects.len()), (3, 3, 3), "{}", t.id);
        assert_eq!((e.line_ex.len(), e.connector_line_ex.len(), e.fonts.len(), e.connector_fonts.len()), (7, 4, 6, 3), "{}", t.id);
        // The variants use lines 1 to 3 only, since a connector's list holds three and an entry past a list reads the no-theme table.
        for v in e.style_variants.iter().flatten() {
            assert!(v.styles.iter().all(|s| s.line_idx.is_some_and(|l| (1..=3).contains(&l))), "{}: a variant style picks a line past three", t.id);
        }
        assert!(t.bytes.len() < 64 * 1024, "{}: {} bytes", t.id, t.bytes.len());
    }
}

#[test]
fn the_themes_are_not_the_ones_of_another_application() {
    // A sanity check, not a proof: no name of a theme of Microsoft's Office, and none of the colours its default themes are best known for. (The
    // palettes were chosen by hand; this keeps a future edit from sliding one of those in by accident.)
    const NAMES: [&str; 24] = [
        "office",
        "integral",
        "ion",
        "slice",
        "retrospect",
        "facet",
        "wisp",
        "organic",
        "basis",
        "depth",
        "droplet",
        "parallax",
        "frame",
        "savon",
        "gallery",
        "dividend",
        "berlin",
        "celestial",
        "circuit",
        "crop",
        "madison",
        "view",
        "mesh",
        "vapor trail",
    ];
    const COLOURS: [&str; 14] =
        ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47", "4F81BD", "C0504D", "9BBB59", "8064A2", "4BACC6", "F79646", "1F497D", "44546A"];
    for t in theme::catalogue() {
        assert!(!NAMES.contains(&t.name.to_ascii_lowercase().as_str()), "{}", t.name);
        for c in t.colours() {
            let h = format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
            assert!(!COLOURS.contains(&h.as_str()), "{}: {h}", t.name);
        }
    }
}

#[test]
fn theme_list_tells_the_themes_and_the_theme_a_page_has() {
    let mut s = Session::new();
    // With no drawing open the themes are listed and no page is told.
    let r = s.execute("theme.list", &json!({})).unwrap();
    assert_eq!(r["themes"].as_array().unwrap().len(), 5);
    assert_eq!(r["page"], Value::Null);
    assert_eq!(r["variants"], json!(["Original", "Shifted", "Single hue", "Opposite"]));
    let t = &r["themes"][0];
    assert_eq!((t["id"].as_str(), t["name"].as_str(), t["scheme"].as_u64()), (Some("tidepool"), Some("Tidepool"), Some(1001)));
    assert_eq!(t["colours"]["accents"][0], hex(rgb(TIDEPOOL_1)));
    assert_eq!(t["colours"]["accents"][1], hex(rgb(TIDEPOOL_2)));
    assert_eq!(t["variants"].as_array().unwrap().len(), 4);
    assert_eq!(t["variants"][0]["colours"].as_array().unwrap().len(), 7);
    assert_eq!(t["variants"][2]["monotone"], true);
    assert_eq!(t["fonts"]["body"], "Liberation Sans");
    assert_eq!(t["in_drawing"], false);
    // With one open, the page has no theme, and the command did not change it.
    let mut s = blank_session();
    let before = snapshot(&s);
    let r = run(&mut s, "theme.list", json!({}));
    assert_eq!(r["page"]["theme"], "none");
    assert_eq!(r["page"]["page"], 1);
    assert_eq!(snapshot(&s), before, "a query changes and records nothing");
}

// ---- applying ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_theme_recolours_a_shape_and_no_theme_gives_it_back() {
    let (mut s, id) = session_with_rectangle();
    assert_eq!(px(&render(&s), 3.0, 4.0), [255, 255, 255, 255], "white before");
    let before = look(&mut s, id);
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(r["theme"], "tidepool");
    assert_eq!((r["variant"].as_u64(), r["scope"].as_str()), (Some(0), Some("page")));
    assert_eq!((r["pages"].clone(), r["changed_pages"].clone()), (json!([1]), json!([1])));
    assert_eq!((r["theme_added"].clone(), r["style_added"].clone()), (json!(true), json!(true)));
    assert_eq!(r["shapes_linked"], 3, "the line, fill and text families of the one shape");
    // The drawing holds the theme's part, the Theme style, and a page that selects it in all seven cells.
    let d = doc(&s);
    assert_eq!(d.themes.len(), 1);
    assert_eq!(d.themes.get(0).unwrap().theme_scheme_id.as_ref().unwrap().scheme_enum, 1001);
    let style = theme::theme_style_id(d).expect("the Theme style");
    assert_eq!(d.style(style).unwrap().name_u.as_deref(), Some(THEME_STYLE_NAME));
    let sel = theme::selection_of(d, 1).unwrap();
    assert_eq!(sel.numbers(), [1001, 1001, 1001, 1001, 1001, 0, 0]);
    let links = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert_eq!((links.line, links.fill, links.text), (Some(style), Some(style), Some(style)));
    // What the shape draws: a pale tint of the first accent inside (40 percent of it, in linear light), the accent's shade round it, the dark colour for text.
    let accent = rgb(TIDEPOOL_1);
    let fill = tint(accent, 0.4);
    let l = look(&mut s, id);
    assert_eq!(l["fill"], hex(fill), "{l}");
    assert_eq!(l["line"], hex(shade(accent, 0.75)), "{l}");
    assert_eq!(l["weight"], 0.75, "9525 EMU: {l}");
    assert_eq!(l["color"], "#14232b", "dk1 (QuickStyleFontColor 0): {l}");
    assert_eq!(l["font"], "Liberation Sans", "{l}");
    assert!(close(px(&render(&s), 3.0, 4.0), fill, 1), "the page draws it: {:?}", px(&render(&s), 3.0, 4.0));
    assert_eq!(page_theme(&mut s, 1)["theme"], "tidepool");
    // No theme: every cell of the page is 0, and the shape draws as one with no style does.
    let r = run(&mut s, "theme.apply", json!({"theme": "none"}));
    assert_eq!((r["theme"].as_str(), r["changed_pages"].clone()), (Some("none"), json!([1])));
    assert_eq!(theme::selection_of(doc(&s), 1).unwrap().numbers(), [0; 7]);
    assert_eq!(page_theme(&mut s, 1)["theme"], "none");
    let back = look(&mut s, id);
    assert_eq!(
        (back["fill"].clone(), back["line"].clone(), back["color"].clone()),
        (before["fill"].clone(), before["line"].clone(), before["color"].clone()),
        "{back}"
    );
    assert_eq!(px(&render(&s), 3.0, 4.0), [255, 255, 255, 255]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_variant_of_the_colours_alone_and_a_variant_of_both_each_change_what_is_drawn() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let first = look(&mut s, id);
    // The colours of variant 1 start at the second accent.
    let r = run(&mut s, "theme.variant", json!({"colors": 1}));
    assert_eq!((r["theme"].as_str(), r["variant"].as_u64(), r["changed_pages"].clone()), (Some("tidepool"), Some(1), json!([1])));
    assert_eq!(theme::selection_of(doc(&s), 1).unwrap().numbers(), [1001, 1001, 1001, 1001, 1001, 1, 0]);
    let l = look(&mut s, id);
    assert_eq!(l["fill"], hex(tint(rgb(TIDEPOOL_2), 0.4)), "{l}");
    assert_ne!(l["fill"], first["fill"]);
    assert_eq!(l["weight"], first["weight"], "the styles did not change");
    assert!(close(px(&render(&s), 3.0, 4.0), tint(rgb(TIDEPOOL_2), 0.4), 1));
    // Variant 2 is the single-hue one: its first colour is the theme's first accent again, and its first style is a white box with a heavy outline.
    run(&mut s, "theme.variant", json!({"variant": 2}));
    assert_eq!(theme::selection_of(doc(&s), 1).unwrap().numbers(), [1001, 1001, 1001, 1001, 1001, 2, 2]);
    let l = look(&mut s, id);
    assert_eq!(l["fill"], "#ffffff", "style 6 of the fills is white: {l}");
    assert_eq!(l["line"], hex(shade(rgb(TIDEPOOL_1), 0.75)), "{l}");
    assert_eq!(l["weight"], 25400.0 / 12700.0, "line 3 is 2 points wide: {l}");
    // The styles alone: style variant 0 again, with the colours of variant 2.
    run(&mut s, "theme.variant", json!({"styles": 0}));
    assert_eq!(theme::selection_of(doc(&s), 1).unwrap().numbers(), [1001, 1001, 1001, 1001, 1001, 2, 0]);
    assert_eq!(look(&mut s, id)["fill"], first["fill"]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_style_variant_changes_the_line_the_shadow_and_the_gradient() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    let flat = render(&s);
    // Variant 1: gradient fills with a shadow. A gradient runs from light at the top to the colour at the bottom, so two places differ.
    run(&mut s, "theme.apply", json!({"theme": "cinderglow", "variant": 1}));
    let r = render(&s);
    let (top, bottom) = (px(&r, 3.0, 4.45), px(&r, 3.0, 3.55));
    assert!(top[2].abs_diff(bottom[2]) > 8 || top[0].abs_diff(bottom[0]) > 8, "a gradient: {top:?} against {bottom:?}");
    assert_eq!(px(&flat, 3.0, 4.45), px(&flat, 3.0, 3.55), "and variant 0 is flat");
    // The shadow falls down and to the right of the box (the box spans x 2.25 to 3.75 and y 3.5 to 4.5).
    let below_right = px(&r, 3.8, 3.46);
    assert!(below_right[..3].iter().any(|c| *c < 250), "a shadow where paper was: {below_right:?}");
    assert_eq!(px(&flat, 3.8, 3.46), [255, 255, 255, 255], "none in variant 0");
    assert_eq!(look(&mut s, id)["pattern"], 1);
}

#[test]
fn a_cell_a_person_set_stays_set_when_the_theme_changes() {
    let (mut s, a) = session_with_rectangle();
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#ff0000"}));
    run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    // The red is a standard colour: it stays. The line and the text, and the other shape's fill, are the theme's.
    assert_eq!(look(&mut s, a)["fill"], "#ff0000");
    assert_ne!(look(&mut s, b)["fill"], "#ffffff");
    assert_ne!(look(&mut s, a)["line"], "#000000");
    run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    assert_eq!(look(&mut s, a)["fill"], "#ff0000");
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    assert_eq!(look(&mut s, a)["fill"], "#ff0000");
    assert_eq!(look(&mut s, b)["fill"], "#ffffff");
}

#[test]
fn a_shape_with_a_style_of_its_own_keeps_it() {
    // A shape whose fill family is linked to a style that is not the root's (a style the drawing holds) is not given the Theme style for that family.
    let (mut s, id) = session_with_rectangle();
    s.edit("Test setup", |d| {
        let mut style = d.style(0).unwrap().as_ref().clone();
        style.id = 7;
        style.name = Some("Mine".into());
        style.name_u = Some("Mine".into());
        d.add_style(style).unwrap();
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).links.fill = Some(7);
        Ok(())
    })
    .unwrap();
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(r["shapes_linked"], 2, "the line and the text; the fill keeps its style");
    let links = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert_eq!(links.fill, Some(7));
    assert_ne!(links.line, Some(7));
}

#[test]
fn applying_the_same_theme_again_changes_nothing() {
    let (mut s, a) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "inkstone", "variant": 3}));
    let steps = s.history().unwrap().undo_len();
    let before = snapshot(&s);
    let r = run(&mut s, "theme.apply", json!({"theme": "inkstone", "variant": 3}));
    assert_eq!(r["changed_pages"], json!([]));
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert_eq!(doc(&s), before.document.as_ref().unwrap(), "the document is as it was");
    // A shape dropped after the theme was applied follows it, so applying the theme again finds nothing to do.
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    assert_ne!(look(&mut s, b)["fill"], "#ffffff");
    let r = run(&mut s, "theme.apply", json!({"theme": "inkstone", "variant": 3}));
    assert_eq!(
        (r["shapes_linked"].as_u64(), r["theme_added"].clone(), r["style_added"].clone(), r["changed_pages"].clone()),
        (Some(0), json!(false), json!(false), json!([]))
    );
    assert_eq!(look(&mut s, a)["fill"], look(&mut s, b)["fill"], "the same colour of the same theme");
    assert_eq!(doc(&s).themes.len(), 1, "the part was added once");
}

#[test]
fn a_theme_is_one_undo_step_and_undo_and_redo_restore_the_drawing_exactly() {
    let (mut s, _) = session_with_rectangle();
    let plain = snapshot(&s);
    run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Apply theme"));
    let themed = snapshot(&s);
    run(&mut s, "theme.variant", json!({"variant": 3}));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Theme variant"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, themed.document);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, plain.document, "the part, the style, the cells and the links are all gone");
    assert!(doc(&s).themes.is_empty());
    assert!(theme::theme_style_id(doc(&s)).is_none());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(snapshot(&s).document, themed.document);
    // A batch with other commands is one step too.
    let mut s = blank_session();
    run(
        &mut s,
        "command.batch",
        json!({"commands": [
            {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 3, "y": 4}},
            {"command": "theme.apply", "params": {"theme": "tidepool"}},
            {"command": "theme.variant", "params": {"variant": 1}},
        ]}),
    );
    assert_eq!(s.history().unwrap().undo_len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(doc(&s).themes.is_empty());
}

#[test]
fn a_theme_goes_to_the_page_named_or_to_every_page_and_pages_may_differ() {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = doc(&s).pages.iter().nth(1).unwrap().id;
    run(&mut s, "shape.drop", json!({"master": "ellipse", "page": second, "x": 3, "y": 4}));
    // One page: the first, then the second, each its own theme.
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(page_theme(&mut s, 1)["theme"], "tidepool");
    assert_eq!(page_theme(&mut s, second)["theme"], "none");
    let r = run(&mut s, "theme.apply", json!({"theme": "cinderglow", "page": second}));
    assert_eq!((r["pages"].clone(), r["scope"].clone()), (json!([second]), json!("page")));
    assert_eq!((page_theme(&mut s, 1)["theme"].clone(), page_theme(&mut s, second)["theme"].clone()), (json!("tidepool"), json!("cinderglow")));
    assert_eq!(doc(&s).themes.len(), 2, "two parts, found by their numbers and not by their places");
    // Every page: both take one theme, and the part is not added again.
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool", "scope": "all", "variant": 2}));
    assert_eq!((r["pages"].clone(), r["scope"].clone(), r["theme_added"].clone()), (json!([1, second]), json!("all"), json!(false)));
    for page in [1, second] {
        assert_eq!(theme::selection_of(doc(&s), page).unwrap().numbers(), [1001, 1001, 1001, 1001, 1001, 2, 2], "page {page}");
    }
    // Variants too.
    let r = run(&mut s, "theme.variant", json!({"variant": 1, "scope": "all"}));
    assert_eq!(r["changed_pages"], json!([1, second]));
    run(&mut s, "theme.apply", json!({"theme": "none", "scope": "all"}));
    assert_eq!(theme::selection_of(doc(&s), second).unwrap().numbers(), [0; 7]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn bad_parameters_are_refused_and_change_nothing() {
    let (mut s, _) = session_with_rectangle();
    let before = snapshot(&s);
    for (id, params, want) in [
        ("theme.apply", json!({}), "missing `theme`"),
        ("theme.apply", json!({"theme": "nonesuch"}), "no theme `nonesuch`"),
        ("theme.apply", json!({"theme": 7}), "must be text"),
        ("theme.apply", json!({"theme": "tidepool", "variant": 4}), "0 to 3"),
        ("theme.apply", json!({"theme": "tidepool", "variant": -1}), "whole number"),
        ("theme.apply", json!({"theme": "none", "variant": 1}), "no variant"),
        ("theme.apply", json!({"theme": "tidepool", "scope": "some"}), "`page` or `all`"),
        ("theme.apply", json!({"theme": "tidepool", "scope": "all", "page": 1}), "not both"),
        ("theme.apply", json!({"theme": "tidepool", "page": 99}), "no page 99"),
        ("theme.apply", json!({"theme": "tidepool", "extra": 1}), "unknown parameter"),
        ("theme.variant", json!({}), "give `variant`"),
        ("theme.variant", json!({"variant": 1, "colors": 2}), "goes alone"),
        ("theme.variant", json!({"colors": 9}), "0 to 3"),
        ("theme.list", json!({"page": 99}), "no page 99"),
    ] {
        let e = s.execute(id, &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(want), "{id} {params}: {e}");
        assert_eq!(snapshot(&s), before, "{id} {params}");
    }
    // A variant of no theme is a failure of the request, not of its parameters.
    let e = s.execute("theme.variant", &json!({"variant": 1})).unwrap_err();
    assert!(failed_msg(&e).contains("no theme"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn the_commands_are_in_the_registry_and_need_a_document_but_the_list() {
    let mut s = Session::new();
    for id in ["theme.apply", "theme.variant"] {
        let info = s.commands().into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id}"));
        assert!(!info.enabled, "{id} needs a drawing");
    }
    assert!(s.commands().into_iter().find(|c| c.id == "theme.list").unwrap().enabled);
    let e = s.execute("theme.apply", &json!({"theme": "tidepool"})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled { .. }), "{e:?}");
    let info: Vec<_> = diacraft_engine::command_specs().iter().filter(|c| c.id.starts_with("theme.")).map(|c| (c.id, c.menu.to_vec())).collect();
    assert_eq!(
        info,
        [
            ("theme.list", vec!["Design", "Themes"]),
            ("theme.apply", vec!["Design", "Themes"]),
            ("theme.variant", vec!["Design", "Variants"]),
            ("theme.colors", vec!["Home", "Shape Styles"]),
            ("theme.preview", vec!["Design", "Themes"]),
        ]
    );
}

// ---- saving -----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_themed_drawing_saves_the_part_and_opens_again_as_it_was() {
    let dir = Dir::new();
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "fernwood", "variant": 1}));
    let l = look(&mut s, id);
    let drawn = px(&render(&s), 3.0, 4.0);
    let path = dir.path("themed.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    // The package holds the part, a relationship to it from the document part, and its content type.
    let bytes = std::fs::read(&path).unwrap();
    let pkg = Package::read(&bytes, &Limits::default()).unwrap();
    assert!(pkg.parts().any(|p| p == "/visio/theme/theme1.xml"), "{:?}", pkg.parts().collect::<Vec<_>>());
    assert_eq!(pkg.content_type("/visio/theme/theme1.xml"), Some("application/vnd.openxmlformats-officedocument.theme+xml"));
    let rels = String::from_utf8(pkg.part("/visio/_rels/document.xml.rels").unwrap().to_vec()).unwrap();
    assert!(rels.contains("relationships/theme\" Target=\"theme/theme1.xml\""), "{rels}");
    // It reads again with the same theme, the same Theme style, the same selections and links, and draws and tells the same.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let d = doc(&t);
    assert_eq!(d.themes.len(), 1);
    assert_eq!(d.themes.get(0).unwrap().entries, theme::find("fernwood").unwrap().def.entries);
    assert_eq!(theme::selection_of(d, 1).unwrap().numbers(), [1002, 1002, 1002, 1002, 1002, 1, 1]);
    let style = theme::theme_style_id(d).expect("the Theme style is in the file");
    let links = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert_eq!((links.line, links.fill, links.text), (Some(style), Some(style), Some(style)));
    assert_eq!(look(&mut t, id), l);
    assert_eq!(px(&render(&t), 3.0, 4.0), drawn);
    assert_eq!(page_theme(&mut t, 1)["theme"], "fernwood");
    // A change of variant touches the page's sheet (it is in the pages part) and nothing of the theme or the shapes.
    let again = dir.path("again.vsdx");
    run(&mut t, "theme.variant", json!({"variant": 3}));
    run(&mut t, "file.save_as", json!({"path": again}));
    let second = Package::read(&std::fs::read(&again).unwrap(), &Limits::default()).unwrap();
    assert!(second.part("/visio/theme/theme1.xml") == pkg.part("/visio/theme/theme1.xml"), "the theme part is the file's, byte for byte");
    assert!(second.part("/visio/pages/page1.xml") == pkg.part("/visio/pages/page1.xml"), "the shapes are as they were");
    assert!(second.part("/visio/pages/pages.xml") != pkg.part("/visio/pages/pages.xml"), "the page's cells are not");
    let sheet = String::from_utf8(second.part("/visio/pages/pages.xml").unwrap().to_vec()).unwrap();
    assert!(sheet.contains("N=\"VariationColorIndex\" V=\"3\"") && sheet.contains("N=\"VariationStyleIndex\" V=\"3\""), "{sheet}");
}

#[test]
fn a_theme_added_to_a_drawing_that_came_from_a_file_is_added_to_the_file_and_the_rest_of_it_stays() {
    let dir = Dir::new();
    let (mut s, _) = session_with_rectangle();
    let first = dir.path("one.vsdx");
    run(&mut s, "file.save_as", json!({"path": first}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": first}));
    let original = Package::read(&std::fs::read(&first).unwrap(), &Limits::default()).unwrap();
    run(&mut t, "theme.apply", json!({"theme": "plumline"}));
    let second = dir.path("two.vsdx");
    run(&mut t, "file.save_as", json!({"path": second}));
    let saved = Package::read(&std::fs::read(&second).unwrap(), &Limits::default()).unwrap();
    assert!(saved.parts().any(|p| p == "/visio/theme/theme1.xml"));
    for kept in ["/visio/windows.xml", "/docProps/custom.xml"] {
        assert!(saved.part(kept) == original.part(kept), "{kept} is the file's, byte for byte");
    }
    // The shape's three links to the Theme style are in the page part, and nothing else of it changed.
    let page = String::from_utf8(saved.part("/visio/pages/page1.xml").unwrap().to_vec()).unwrap();
    let was = String::from_utf8(original.part("/visio/pages/page1.xml").unwrap().to_vec()).unwrap();
    // (The Theme style is the first the drawing holds after the root: `ID` 1.)
    assert!(page.contains("LineStyle=\"1\"") && page.contains("FillStyle=\"1\"") && page.contains("TextStyle=\"1\""), "{page}");
    assert!(!was.contains("LineStyle="), "the shape followed no style before: {was}");
    // The pages part gained the page's cells, and the document part the Theme style (and its relationship to the theme, in its own part).
    assert!(saved.part("/visio/pages/pages.xml") != original.part("/visio/pages/pages.xml"));
    assert!(saved.part("/visio/document.xml") != original.part("/visio/document.xml"));
    let styles = String::from_utf8(saved.part("/visio/document.xml").unwrap().to_vec()).unwrap();
    assert!(styles.contains("NameU=\"Theme\""), "{styles}");
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": second}));
    assert_eq!(page_theme(&mut u, 1)["theme"], "plumline");
}

#[test]
fn a_document_holds_at_most_the_themes_a_reader_keeps() {
    let (mut s, _) = session_with_rectangle();
    let tiny =
        Arc::new(diacraft_doc::Theme { part: "/visio/theme/x.xml".into(), bytes: Arc::from(&b"<a:theme xmlns:a=\"x\"/>"[..]), ..Default::default() });
    s.edit("Test setup", |d| {
        for _ in 0..diacraft_vsdx::MAX_THEMES {
            d.themes.push(tiny.clone());
        }
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let e = s.execute("theme.apply", &json!({"theme": "tidepool"})).unwrap_err();
    assert!(failed_msg(&e).contains("at most"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_variant_for_every_page_leaves_a_page_with_no_theme_alone() {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = doc(&s).pages.iter().nth(1).unwrap().id;
    // Only the first page has a theme: the variant goes to it, and the second is not an error.
    run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    let r = run(&mut s, "theme.variant", json!({"variant": 2, "scope": "all"}));
    assert_eq!((r["pages"].clone(), r["changed_pages"].clone()), (json!([1]), json!([1])));
    assert_eq!(theme::selection_of(doc(&s), second).unwrap().numbers(), [0; 7]);
    // With no theme anywhere it is a failure, and for the one page named it always was.
    run(&mut s, "theme.apply", json!({"theme": "none", "scope": "all"}));
    let before = snapshot(&s);
    let e = s.execute("theme.variant", &json!({"variant": 1, "scope": "all"})).unwrap_err();
    assert!(failed_msg(&e).contains("no page has a theme"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_second_theme_is_added_to_the_file_by_a_second_save_and_the_first_part_is_not_written_again() {
    let dir = Dir::new();
    let (mut s, _) = session_with_rectangle();
    let (one, two) = (dir.path("one.vsdx"), dir.path("two.vsdx"));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "file.save_as", json!({"path": one}));
    run(&mut s, "theme.apply", json!({"theme": "cinderglow", "variant": 3}));
    let saved = run(&mut s, "file.save_as", json!({"path": two}));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let first = Package::read(&std::fs::read(&one).unwrap(), &Limits::default()).unwrap();
    let second = Package::read(&std::fs::read(&two).unwrap(), &Limits::default()).unwrap();
    let names: Vec<&str> = second.parts().filter(|p| p.contains("/theme/")).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    // The first theme's part is the first file's, byte for byte, and the second is the other theme.
    assert!(second.part("/visio/theme/theme1.xml") == first.part("/visio/theme/theme1.xml"));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": two}));
    assert_eq!(doc(&t).themes.len(), 2);
    assert_eq!(page_theme(&mut t, 1)["theme"], "cinderglow");
    assert_eq!(theme::selection_of(doc(&t), 1).unwrap().numbers(), [1003, 1003, 1003, 1003, 1003, 3, 3]);
    assert!(doc(&t).theme_by_index(1001).is_some() && doc(&t).theme_by_index(1003).is_some());
}

#[test]
fn a_random_run_of_theme_commands_among_edits_keeps_the_drawing_sound_and_undo_takes_it_all_back() {
    // A fixed-seed xorshift: a failure repeats. Themes, variants, scopes and pages that may not be there, between drops, formats, page adds and undo and redo.
    for seed in [0x9E37_79B9_7F4A_7C15_u64, 0xC2B2_AE3D_27D4_EB4F, 0x1656_67B1_9E37_79F9] {
        let mut x = seed;
        let mut below = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut s = blank_session();
        let blank = doc(&s).clone();
        const THEMES: [&str; 7] = ["tidepool", "fernwood", "cinderglow", "inkstone", "plumline", "none", "nonesuch"];
        for step in 0..300 {
            let (id, params) = match below(12) {
                0 | 1 => {
                    ("shape.drop", json!({"master": "rectangle", "page": 1 + below(3), "x": below(10) as f64 + 1.0, "y": below(7) as f64 + 1.0}))
                }
                2 => ("page.add", json!({})),
                3..=5 => (
                    "theme.apply",
                    json!({"theme": THEMES[below(7) as usize], "variant": below(5), "scope": if below(2) == 0 { "all" } else { "page" }, "page": below(4)}),
                ),
                6 | 7 => ("theme.variant", json!({"variant": below(5), "scope": if below(2) == 0 { "all" } else { "page" }})),
                8 => ("shape.format", json!({"ids": [1 + below(6)], "fill": "#336699"})),
                9 => ("theme.list", json!({"page": below(4)})),
                10 => ("edit.undo", json!({})),
                _ => ("edit.redo", json!({})),
            };
            // `page` is given with `scope` all now and then: a refusal, like the junk ids, is part of the run.
            let _ = s.execute(id, &params);
            assert_eq!(s.panics_caught(), 0, "seed {seed:#x} step {step}: {id} {params}");
            assert_eq!(check_session(&s), Ok(()), "seed {seed:#x} step {step}: {id} {params}");
            // Every theme number a page holds is one of the catalogue's or 0, and the five agree.
            for page in doc(&s).pages.iter() {
                let n = theme::selection_of(doc(&s), page.id).unwrap().numbers();
                assert!(
                    n[..5].iter().all(|v| *v == n[0]) && (n[0] == 0 || theme::by_scheme(n[0]).is_some()),
                    "seed {seed:#x} step {step}: page {}: {n:?}",
                    page.id
                );
                assert!(n[5] <= 3 && n[6] <= 3, "seed {seed:#x} step {step}: {n:?}");
            }
        }
        while s.execute("edit.undo", &json!({})).is_ok() {}
        assert_eq!(doc(&s), &blank, "seed {seed:#x}: undoing every step does not give the blank drawing back");
    }
}

#[test]
fn every_theme_and_variant_draws_a_shape_a_label_and_a_connector_with_no_cell_left_unanswered() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "ellipse", 7.0, 4.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Start"}));
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    let mut seen = std::collections::BTreeSet::new();
    for t in theme::catalogue() {
        for variant in 0..4 {
            run(&mut s, "theme.apply", json!({"theme": t.id, "variant": variant}));
            let r = render(&s);
            // The theme answered every cell the Theme style asks it: a cell it could not answer is noted. (A face the host lacks is noted too, and is the
            // host's: the notes of the text are not the theme's.)
            let noted = format!("{:?}", r.skipped);
            assert!(!noted.contains("ThemeUnanswered"), "{} variant {variant}: {noted}", t.id);
            let l = look(&mut s, a);
            assert_ne!(l["line"], "#000000", "{} variant {variant}: the line is the theme's: {l}", t.id);
            assert_eq!(l["font"], t.def.minor_font, "{} variant {variant}: {l}", t.id);
            seen.insert((t.id, variant, l["fill"].as_str().unwrap().to_owned(), l["line"].as_str().unwrap().to_owned(), l["weight"].to_string()));
        }
    }
    // Every theme and variant is a look of its own.
    assert_eq!(seen.len(), 20, "{seen:?}");
}

// ---- new shapes on a themed page ------------------------------------------------------------------------------------------

/// The style links of shape `id` of page 1: line, fill, text.
fn links_of(s: &Session, id: u32) -> (Option<u32>, Option<u32>, Option<u32>) {
    let d = doc(s);
    let l = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    (l.line, l.fill, l.text)
}

#[test]
fn a_shape_a_box_and_a_connector_put_on_a_themed_page_follow_its_theme_in_the_same_undo_step() {
    let (mut s, a) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let style = theme::theme_style_id(doc(&s)).unwrap();
    let steps = s.history().unwrap().undo_len();
    // A shape dropped from a built-in stencil: the theme's pale fill, and one undo step that takes shape and links away together.
    let b = drop_shape(&mut s, "ellipse", 7.0, 4.0);
    assert_eq!(links_of(&s, b), (Some(style), Some(style), Some(style)));
    assert_eq!(look(&mut s, b)["fill"], hex(tint(rgb(TIDEPOOL_1), 0.4)));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    // A text box keeps the no fill and no line it is made with, and its text is the theme's.
    let r = run(&mut s, "text.box", json!({"page": 1, "x": 5, "y": 7, "text": "Note"}));
    let t = r["id"].as_u64().unwrap() as u32;
    assert_eq!(links_of(&s, t), (Some(style), Some(style), Some(style)));
    let l = look(&mut s, t);
    assert_eq!(
        (l["fill"].as_str(), l["line"].as_str(), l["font"].as_str(), l["color"].as_str()),
        (Some("none"), Some("none"), Some("Liberation Sans"), Some("#14232b")),
        "{l}"
    );
    // A connector and what AutoConnect makes.
    let c = run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}))["id"].as_u64().unwrap() as u32;
    assert_eq!(links_of(&s, c), (Some(style), Some(style), Some(style)));
    assert_eq!(look(&mut s, c)["line"], hex(rgb(TIDEPOOL_1)));
    let auto = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "down", "master": "square"}));
    for key in ["id", "connector"] {
        let id = auto[key].as_u64().unwrap() as u32;
        assert_eq!(links_of(&s, id), (Some(style), Some(style), Some(style)), "{key}");
    }
    // Applying the theme again finds every shape following it already.
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!((r["shapes_linked"].clone(), r["changed_pages"].clone()), (json!(0), json!([])), "{r}");
    run(&mut s, "edit.undo", json!({}));
    assert!(
        doc(&s).find_shape(Owner::Page(1), auto["id"].as_u64().unwrap() as u32).is_none(),
        "undo takes the autoconnected shape and its links away"
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_with_no_theme_gives_a_new_shape_nothing() {
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    let b = drop_shape(&mut s, "ellipse", 7.0, 4.0);
    assert_eq!(links_of(&s, b), (None, None, None));
    assert_eq!(look(&mut s, b)["fill"], "#ffffff");
}

#[test]
fn a_drop_from_a_stencil_file_follows_the_theme_unless_the_master_links_a_style_of_its_own() {
    let (mut s, _dir) = crate::stencil_open::with_stencil();
    run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    let style = theme::theme_style_id(doc(&s)).unwrap();
    // "Decision" links no style: the instance follows the theme. "Process" links the stencil's "Flow Normal" (3), which the drawing now holds under
    // another ID: the instance has that style through its master, so the theme does not override it.
    let decision =
        run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))["id"].as_u64().unwrap() as u32;
    assert_eq!(links_of(&s, decision), (Some(style), Some(style), Some(style)));
    let process =
        run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 6, "y": 3}))["id"].as_u64().unwrap() as u32;
    assert_eq!(links_of(&s, process), (None, None, None), "its style is its master's");
    // The page's other shapes and a later application agree: nothing is left to link.
    let r = run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    assert_eq!(r["shapes_linked"], 0, "{r}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn applying_a_theme_leaves_an_instance_alone_whose_master_shape_links_a_style() {
    // The instance has no link of its own, as a file's instances have none: the style it has is its master's, and a link of its own would outrank it
    // Only the instance of a master that links nothing is made to follow the theme.
    let (mut s, _dir) = crate::stencil_open::with_stencil();
    let decision =
        run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))["id"].as_u64().unwrap() as u32;
    let process =
        run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 6, "y": 3}))["id"].as_u64().unwrap() as u32;
    let before = look(&mut s, process);
    let r = run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    assert_eq!(r["shapes_linked"], 3, "the three families of the one shape whose master links no style: {r}");
    let style = theme::theme_style_id(doc(&s)).unwrap();
    assert_eq!(links_of(&s, decision), (Some(style), Some(style), Some(style)));
    assert_eq!(links_of(&s, process), (None, None, None));
    assert_eq!(
        (look(&mut s, process)["fill"].clone(), look(&mut s, process)["line"].clone()),
        (before["fill"].clone(), before["line"].clone()),
        "its look is its style's, as it was"
    );
}

/// Applying a theme works the whole page out again, and the `C` and `D` of an elliptical arc are not among the cells it
/// rewrites: no argument of the arc changed (a theme writes the page's selector cells and the style links, and no size), and the rule's pair differs
/// from the file's in 2.6% of the angles and 0.5% of the ratios, the family that holds the reciprocal among them, which would turn
/// the arc into another curve. A circle made 3 by 3 and now 1.5 by 2 gives the rule (0, 0.75); the file holds (0, 4/3).
#[test]
fn applying_a_theme_leaves_the_arcs_of_the_reciprocal_family_as_the_file_saved_them() {
    use diacraft_calc::{Budget, Changes, EvalOptions, commit_changed};
    use diacraft_doc::{CellAddr, Lookup, RowKey};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    let row = RowKey::Ix(2);
    let shape = ShapeBuilder::new(1)
        .cell("Width", "1.5")
        .cell("Height", "2")
        .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
        .row_type("Geometry", Some(0), row.clone(), "EllipticalArcTo")
        .row_cell("Geometry", Some(0), row.clone(), "X", "1")
        .row_cell("Geometry", Some(0), row.clone(), "Y", "0")
        .row_cell("Geometry", Some(0), row.clone(), "A", "0.5")
        .row_cell("Geometry", Some(0), row.clone(), "B", "0.5")
        .row_formula("Geometry", Some(0), row.clone(), "C", "_ELLIPSE_THETA(0.5,1,3,3,Width,Height)", "0")
        .row_formula("Geometry", Some(0), row.clone(), "D", "_ELLIPSE_ECC(0.5,1,3,3,Width,Height,Geometry1.C2)", "1.3333333333333333");
    let built = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let saved = |d: &diacraft_doc::Document, name: &str| {
        let at = d.find_shape(Owner::Page(1), 1).unwrap();
        match d.shape(&at).unwrap().sheet.lookup(&CellAddr::in_row("Geometry", Some(0), row.clone(), name)) {
            Lookup::Found(cell) => cell.value.as_ref().unwrap().text().to_owned(),
            other => panic!("{other:?}"),
        }
    };
    // The control: a pass over the whole page that is not told the arguments are as they were writes the rule's ratio (0.75).
    let mut control = built.clone();
    let report = commit_changed(&mut control, &Budget::default(), EvalOptions::default(), 1, &Changes { whole_page: true, ..Changes::default() });
    assert_eq!(report.written, 1, "{report:?}");
    assert_eq!(saved(&control, "D").parse::<f64>().unwrap(), 0.75);

    let mut s = blank_session();
    s.edit("arcs", |d| {
        *d = built.clone();
        Ok(())
    })
    .unwrap();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    for name in ["C", "D"] {
        assert_eq!(saved(doc(&s), name), saved(&built, name), "{name} is as the file saved it");
    }
}
