//! The colour palette and the theme preview: `theme.colors`, a theme pick through `shape.format` and `text.format`, and `theme.preview`, through
//! `Session::execute` as the control channel calls them. The rows followed are;
//! what a pick writes is, ours, says no source gives it.

use diacraft_doc::{FormulaSlot, Owner};
use diacraft_engine::Session;
use diacraft_engine::theme;
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

fn session_with_rectangle() -> (Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    (s, id)
}

/// What a cell directly under shape `id` of page 1 holds: its formula's text and its saved value.
fn cell(s: &Session, id: u32, name: &str) -> (Option<String>, String) {
    let d = doc(s);
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    let cell = shape.sheet.cell(name).unwrap_or_else(|| panic!("shape {id} holds no {name}"));
    let formula = match &cell.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    };
    (formula, cell.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())
}

fn look(s: &mut Session, id: u32) -> Value {
    run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0].clone()
}

/// The swatch of the palette of page 1 whose `pick` word is `word`.
fn swatch(palette: &Value, word: &str) -> Value {
    let columns = palette["theme_colors"].as_array().unwrap();
    let mut all = columns.iter().flat_map(|c| c["swatches"].as_array().unwrap().iter()).chain(palette["variant_colors"].as_array().unwrap().iter());
    all.find(|s| s["pick"] == word).cloned().unwrap_or_else(|| panic!("no swatch {word}"))
}

fn palette(s: &mut Session) -> Value {
    run(s, "theme.colors", json!({}))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn lightness((r, g, b): (u8, u8, u8)) -> f64 {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    (r.max(g).max(b) + r.min(g).min(b)) / 2.0
}

// ---- theme.colors ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_palette_of_a_themed_page_is_ten_columns_of_six_seven_variant_colours_and_ten_standard_ones() {
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let before = snapshot(&s);
    let p = palette(&mut s);
    assert_eq!(snapshot(&s), before, "a query changes and records nothing");
    assert_eq!((p["page"].as_u64(), p["themed"].as_bool()), (Some(1), Some(true)));
    let columns = p["theme_colors"].as_array().unwrap();
    // Ten columns, the first four white, black, white and black whatever the theme, and the last six the accents.
    let slots: Vec<&str> = columns.iter().map(|c| c["slot"].as_str().unwrap()).collect();
    assert_eq!(slots, ["light", "dark", "light", "dark", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"]);
    let places: Vec<u64> = columns.iter().map(|c| c["column"].as_u64().unwrap()).collect();
    assert_eq!(places, (1..=10).collect::<Vec<u64>>(), "each column says where it stands, since two of them name the same colour");
    assert!(columns.iter().all(|c| c["swatches"].as_array().unwrap().len() == 6), "the colour and its five shades");
    assert_eq!(p["variant_colors"].as_array().unwrap().len(), 7);
    assert_eq!(p["standard_colors"].as_array().unwrap().len(), 10);
    // The top row holds the theme's own colours: Tidepool's dark, light and first two accents. Background is no column of the palette.
    assert_eq!(swatch(&p, "theme:light")["hex"], "#ffffff");
    assert_eq!(swatch(&p, "theme:dark")["hex"], "#14232b");
    let every_pick = columns.iter().flat_map(|c| c["swatches"].as_array().unwrap().iter()).map(|sw| sw["pick"].as_str().unwrap());
    assert!(every_pick.clone().all(|pick| !pick.contains("background")), "no swatch picks Background: the third column repeats Light");
    assert_eq!(swatch(&p, "theme:accent1")["hex"], "#1f8ca8");
    assert_eq!(swatch(&p, "theme:accent2")["hex"], "#2b5fa8");
    // A swatch says what picking it writes, and what it is called.
    let s2 = swatch(&p, "theme:accent2:40");
    assert_eq!(s2["formula"], r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))"#);
    assert_eq!((s2["label"].as_str(), s2["tint"].as_i64()), (Some("Accent 2, Lighter 40%"), Some(40)));
    assert!(s2["tooltip"].as_str().unwrap().ends_with(", Accent 2, Lighter 40%"), "{s2}");
}

#[test]
fn the_shade_rows_lighten_and_darken_and_agree_with_the_msotint_vector_of_the_spec() {
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let p = palette(&mut s);
    // White by -35 is #a5a5a5. Tidepool's light colour is white.
    assert_eq!(swatch(&p, "theme:light:-35")["hex"], "#a5a5a5");
    for word in ["accent3", "accent5"] {
        let base = lightness(rgb(swatch(&p, &format!("theme:{word}"))["hex"].as_str().unwrap()));
        for tint in [80, 60, 40] {
            let l = lightness(rgb(swatch(&p, &format!("theme:{word}:{tint}"))["hex"].as_str().unwrap()));
            assert!(l > base, "{word} lighter {tint}%: {l} against {base}");
        }
        for tint in [-25, -50] {
            let l = lightness(rgb(swatch(&p, &format!("theme:{word}:{tint}"))["hex"].as_str().unwrap()));
            assert!(l < base, "{word} darker {}%: {l} against {base}", -tint);
        }
    }
    // Each shade row is further than the one before it from the colour.
    let hexes: Vec<f64> = [80, 60, 40].iter().map(|t| lightness(rgb(swatch(&p, &format!("theme:accent1:{t}"))["hex"].as_str().unwrap()))).collect();
    assert!(hexes[0] > hexes[1] && hexes[1] > hexes[2], "{hexes:?}");
}

#[test]
fn the_variant_colours_are_those_of_the_pages_colour_variant() {
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let list = run(&mut s, "theme.list", json!({}));
    for place in 0..4 {
        run(&mut s, "theme.variant", json!({"colors": place}));
        let want: Vec<String> =
            list["themes"][0]["variants"][place]["colours"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_owned()).collect();
        let p = palette(&mut s);
        let got: Vec<String> = p["variant_colors"].as_array().unwrap().iter().map(|c| c["hex"].as_str().unwrap().to_owned()).collect();
        // A monotone variant gives its first colour to the ones it picks by number 100 to 106 only: the variant colours by name are the ones at 200 to 206, as listed.
        assert_eq!(got, want, "variant {place}");
    }
}

#[test]
fn a_page_with_no_theme_has_the_table_that_themeval_gives_with_no_theme() {
    let (mut s, _) = session_with_rectangle();
    let p = palette(&mut s);
    assert_eq!(p["themed"], false);
    // The table: the second accent is #9DBB61 with no theme.
    assert_eq!(swatch(&p, "theme:accent2")["hex"], "#9dbb61");
    assert_eq!(swatch(&p, "theme:dark")["hex"], "#000000");
    assert_eq!(swatch(&p, "theme:light")["hex"], "#ffffff");
    assert_eq!(p["theme_colors"].as_array().unwrap().len(), 10);
}

#[test]
fn the_third_and_fourth_columns_repeat_light_and_dark_and_the_ladders_are_those_the_sample_files_show() {
    // Until a source settles what the third and fourth columns are, they repeat the first two, and a pick of one writes the same form.
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let p = palette(&mut s);
    let columns = p["theme_colors"].as_array().unwrap();
    assert_eq!((&columns[2]["slot"], &columns[2]["label"]), (&columns[0]["slot"], &columns[0]["label"]), "the third column is the first again");
    assert_eq!(columns[2]["swatches"], columns[0]["swatches"]);
    assert_eq!(columns[3]["swatches"], columns[1]["swatches"], "the fourth column is the second again");
    assert_eq!(columns[2]["swatches"][4]["formula"], r#"THEMEGUARD(MSOTINT(THEMEVAL("Light"),-35))"#);
    assert_eq!(columns[3]["swatches"][3]["formula"], r#"THEMEGUARD(MSOTINT(THEMEVAL("Dark"),25))"#);
    // The tints of each column: 80, 60, 40, -25 and -50 on an accent, 5 to 50 on Dark and -5 to -50 on Light.
    let tints = |c: &Value| -> Vec<i64> { c["swatches"].as_array().unwrap().iter().skip(1).map(|sw| sw["tint"].as_i64().unwrap()).collect() };
    for (n, want) in [
        (0, [-5, -15, -25, -35, -50]),
        (1, [50, 35, 25, 15, 5]),
        (2, [-5, -15, -25, -35, -50]),
        (3, [50, 35, 25, 15, 5]),
        (4, [80, 60, 40, -25, -50]),
        (9, [80, 60, 40, -25, -50]),
    ] {
        assert_eq!(tints(&columns[n]), want, "column {}", n + 1);
    }
    // The greys under the first and third columns darken, and those under the second and fourth lighten, from the colour at the top.
    for n in [0, 2] {
        let top = lightness(rgb(columns[n]["swatches"][0]["hex"].as_str().unwrap()));
        let down: Vec<f64> = (1..6).map(|r| lightness(rgb(columns[n]["swatches"][r]["hex"].as_str().unwrap()))).collect();
        assert!(down.iter().all(|l| *l < top) && down.windows(2).all(|w| w[0] > w[1]), "column {} darkens: {top} then {down:?}", n + 1);
    }
    for n in [1, 3] {
        let top = lightness(rgb(columns[n]["swatches"][0]["hex"].as_str().unwrap()));
        let down: Vec<f64> = (1..6).map(|r| lightness(rgb(columns[n]["swatches"][r]["hex"].as_str().unwrap()))).collect();
        assert!(down.iter().all(|l| *l > top), "column {} lightens from {top}: {down:?}", n + 1);
    }
}

#[test]
fn a_pick_of_the_third_or_fourth_column_writes_the_form_of_the_first_two() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let p = palette(&mut s);
    let third = p["theme_colors"][2]["swatches"][2].clone();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": third["pick"]}));
    assert_eq!(cell(&s, id, "FillForegnd").0.as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("Light"),-15))"#));
    let fourth = p["theme_colors"][3]["swatches"][4].clone();
    run(&mut s, "shape.format", json!({"ids": [id], "line": fourth["pick"]}));
    assert_eq!(cell(&s, id, "LineColor").0.as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("Dark"),15))"#));
    // Background is still a colour a command can name, though no column shows it.
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:background"}));
    assert_eq!(cell(&s, id, "FillForegnd").0.as_deref(), Some(r#"THEMEGUARD(THEMEVAL("BackgroundColor"))"#));
}

// ---- a pick is a formula that follows the theme -----------------------------------------------------------------------------------------------

#[test]
fn a_theme_pick_fills_the_shape_with_a_formula_and_one_undo_takes_it_back() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let before = look(&mut s, id);
    let p = palette(&mut s);
    let want = swatch(&p, "theme:accent2:40")["hex"].clone();
    let revision = s.revision();
    let r = run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:accent2:40"}));
    assert_eq!(r["formatted"], 1);
    assert_eq!(s.revision(), revision + 1, "one command");
    let (formula, value) = cell(&s, id, "FillForegnd");
    assert_eq!(formula.as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))"#));
    assert_eq!(Value::String(value), want, "the cell holds the colour the palette showed");
    assert_eq!(look(&mut s, id)["fill"], want);
    assert_eq!(look(&mut s, id)["pattern"], before["pattern"]);
    // One undo step puts the fill back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(look(&mut s, id)["fill"], before["fill"]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_theme_colour_follows_the_theme_when_it_changes_and_a_standard_colour_does_not() {
    let (mut s, a) = session_with_rectangle();
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "theme:accent3:60", "line": "theme:dark"}));
    run(&mut s, "shape.format", json!({"ids": [b], "fill": "#ff0000"}));
    let first = look(&mut s, a);
    // Another theme: the pick follows, the fixed colour stays.
    run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    let p = palette(&mut s);
    let l = look(&mut s, a);
    assert_ne!(l["fill"], first["fill"], "the fill followed the theme");
    assert_eq!(l["fill"], swatch(&p, "theme:accent3:60")["hex"], "to the colour the new theme's palette shows");
    assert_eq!(l["line"], swatch(&p, "theme:dark")["hex"]);
    assert_eq!(look(&mut s, b)["fill"], "#ff0000");
    // A variant of the colours too: a variant colour and an accent each move with it.
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "theme:variant2"}));
    let before = look(&mut s, a)["fill"].clone();
    run(&mut s, "theme.variant", json!({"colors": 3}));
    let p = palette(&mut s);
    let after = look(&mut s, a)["fill"].clone();
    assert_ne!(after, before, "the variant colour changed with the variant");
    assert_eq!(after, swatch(&p, "theme:variant2")["hex"]);
    // And no theme at all: the table of THEMEVAL with no theme.
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    assert_eq!(look(&mut s, a)["fill"], "#9dbb61", "With no theme the second variant colour is the table's second accent");
    assert_eq!(look(&mut s, b)["fill"], "#ff0000");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_pick_made_before_any_theme_follows_the_theme_applied_after_it() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:accent2"}));
    assert_eq!(look(&mut s, id)["fill"], "#9dbb61", "the table's accent with no theme");
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(look(&mut s, id)["fill"], "#2b5fa8", "Tidepool's second accent");
    run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    let p = palette(&mut s);
    assert_eq!(look(&mut s, id)["fill"], swatch(&p, "theme:accent2")["hex"]);
}

#[test]
fn a_shape_that_follows_no_style_reads_the_pages_theme_for_its_pick() {
    // Pasted or built shapes may read the root style (scheme 0): the pick sends it to the page's theme as a Quick Style does (the scheme cells 65534).
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    // A shape on the themed page that has been taken off the Theme style: its families read the root.
    s.edit("Test setup", |d| {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        sheet.links.fill = None;
        sheet.links.line = None;
        sheet.links.text = None;
        Ok(())
    })
    .unwrap();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:accent1"}));
    let p = palette(&mut s);
    assert_eq!(look(&mut s, id)["fill"], swatch(&p, "theme:accent1")["hex"], "the page's theme, not the table with no theme");
    run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    let p = palette(&mut s);
    assert_eq!(look(&mut s, id)["fill"], swatch(&p, "theme:accent1")["hex"]);
}

#[test]
fn use_default_hands_the_fill_back_to_the_theme() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let themed = look(&mut s, id)["fill"].clone();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#00ff00"}));
    assert_eq!(look(&mut s, id)["fill"], "#00ff00");
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "default"}));
    assert_eq!(look(&mut s, id)["fill"], themed, "the fill the theme gives the shape");
    let (formula, _) = cell(&s, id, "FillForegnd");
    assert_eq!(formula.as_deref(), Some("THEMEVAL()"));
    // And it still follows a new theme.
    run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    assert_ne!(look(&mut s, id)["fill"], themed);
    // No Fill and then Use Default brings the fill back.
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "none"}));
    assert_eq!(look(&mut s, id)["fill"], "none");
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "default"}));
    assert_ne!(look(&mut s, id)["fill"], "none");
}

#[test]
fn a_pick_refused_is_refused_in_words_and_changes_nothing() {
    let (mut s, id) = session_with_rectangle();
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"ids": [id], "fill": "theme:accent9"}), "names no theme colour"),
        (json!({"ids": [id], "fill": "theme:accent2:101"}), "from -100 (darker) to 100 (lighter)"),
        (json!({"ids": [id], "line": "theme:"}), "names no theme colour"),
        (json!({"ids": [id], "fill_background": "theme:accent1"}), "must be a colour like #3C78D8"),
    ] {
        let e = s.execute("shape.format", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(words), "{params}: {e}");
    }
    let e = s.execute("text.format", &json!({"ids": [id], "color": "theme:nope"})).unwrap_err();
    assert!(bad_params_msg(&e).contains("names no theme colour"), "{e}");
    let e = s.execute("text.format", &json!({"ids": [id], "color": "none"})).unwrap_err();
    assert!(bad_params_msg(&e).contains("must be a colour like #3C78D8, default or theme:"), "{e}");
    assert_eq!(snapshot(&s), before);
}

// ---- text colour ------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_theme_pick_colours_the_text_and_follows_the_theme() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Hello"}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    run(&mut s, "text.format", json!({"ids": [id], "color": "theme:accent4"}));
    let p = palette(&mut s);
    assert_eq!(look(&mut s, id)["color"], swatch(&p, "theme:accent4")["hex"]);
    run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    let p = palette(&mut s);
    assert_eq!(look(&mut s, id)["color"], swatch(&p, "theme:accent4")["hex"], "the text followed the theme");
    // A range of the characters: the pick is on those runs only.
    run(&mut s, "text.format", json!({"ids": [id], "start": 1, "end": 3, "color": "theme:dark:50"}));
    let range = run(&mut s, "shape.formatting", json!({"ids": [id], "start": 1, "end": 3}))["shapes"][0].clone();
    assert_eq!(range["color"], swatch(&palette(&mut s), "theme:dark:50")["hex"]);
    let whole = run(&mut s, "shape.formatting", json!({"ids": [id], "start": 0, "end": 5}))["shapes"][0].clone();
    assert_eq!(whole["color"], "mixed");
    // Use Default hands the text colour to the theme again.
    run(&mut s, "text.format", json!({"ids": [id], "color": "default"}));
    assert_ne!(look(&mut s, id)["color"], swatch(&palette(&mut s), "theme:accent4")["hex"]);
    assert_eq!(check_session(&s), Ok(()));
}

// ---- theme.preview ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_preview_shows_what_the_theme_would_make_and_changes_nothing() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "theme:accent2"}));
    let before = snapshot(&s);
    let plain = look(&mut s, id);
    let r = run(&mut s, "theme.preview", json!({"theme": "tidepool", "ids": [id]}));
    assert_eq!(r["preview"], true);
    assert_eq!(snapshot(&s), before, "no revision, no history, no journal entry, and the document as it was");
    assert_eq!(look(&mut s, id), plain, "the drawing's own look");
    // The copy shows the theme: the same fill, line and text colour as applying it makes.
    let shown = r["shapes"][0].clone();
    assert_eq!(shown["fill"], "#2b5fa8");
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let applied = look(&mut s, id);
    assert_eq!(
        (shown["fill"].clone(), shown["line"].clone(), shown["color"].clone()),
        (applied["fill"].clone(), applied["line"].clone(), applied["color"].clone())
    );
    assert_eq!(r["changed_pages"], json!([1]));
}

#[test]
fn a_preview_leaves_the_redo_stack_and_the_unsaved_mark_as_they_were() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#00ff00"}));
    run(&mut s, "edit.undo", json!({}));
    let history = s.history().unwrap();
    let (undo, redo) = (history.undo_len(), history.redo_len());
    assert_eq!(redo, 1, "an undone step is waiting");
    let (revision, dirty) = (s.revision(), s.is_dirty());
    run(&mut s, "theme.preview", json!({"theme": "cinderglow", "variant": 2}));
    run(&mut s, "theme.preview", json!({"theme": "none"}));
    let history = s.history().unwrap();
    assert_eq!((history.undo_len(), history.redo_len()), (undo, redo), "a theme applied for real would have cleared the redo stack");
    assert_eq!((s.revision(), s.is_dirty()), (revision, dirty));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(look(&mut s, id)["fill"], "#00ff00", "and the redo is still there");
}

#[test]
fn a_variant_preview_names_the_variant_and_needs_a_themed_page() {
    let (mut s, id) = session_with_rectangle();
    let e = s.execute("theme.preview", &json!({"variant": 1})).unwrap_err();
    assert!(e.to_string().contains("has no theme to give a variant"), "{e}");
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let first = look(&mut s, id);
    let r = run(&mut s, "theme.preview", json!({"colors": 1, "ids": [id]}));
    assert_ne!(r["shapes"][0]["fill"], first["fill"]);
    assert_eq!(look(&mut s, id), first, "unchanged");
    for (params, words) in [
        (json!({}), "give `theme`, `variant`, `colors` or `styles`"),
        (json!({"theme": "none", "variant": 1}), "`none` takes no variant"),
        (json!({"theme": "tidepool", "colors": 1}), "give the variant as `variant`"),
        (json!({"theme": "no-such-theme"}), "no theme `no-such-theme`"),
        (json!({"theme": "tidepool", "variant": 9}), "0 to 3"),
        (json!({"theme": "tidepool", "page": 7}), "there is no page 7"),
    ] {
        let e = s.execute("theme.preview", &params).unwrap_err();
        assert!(e.to_string().contains(words), "{params}: {e}");
    }
}

#[test]
fn the_palette_and_the_preview_are_listed_with_the_engines_commands() {
    let s = Session::new();
    let ids: Vec<String> = s.commands().iter().map(|c| c.id.clone()).collect();
    for id in ["theme.colors", "theme.preview"] {
        assert!(ids.iter().any(|c| c == id), "{id}");
    }
    assert!(theme::STANDARD_COLOURS.len() == 10 && theme::COLUMNS.len() == 10);
}
