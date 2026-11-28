//! Themes on a drawing that came from a file: a theme part of its own, shapes that hold their own scheme cells and numbers cached under a
//! theme formula, a lock, and hostile cells and parts. The package is built in code from the specifications' XML;
//! no file from another application is used.

use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{Document, Owner};
use diacraft_engine::{Session, theme};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, cell_f, shape_with};
use diacraft_testkit::theme::ThemeXml;
use serde_json::json;

use crate::{check_session, run};

/// The root style as a file holds it, as far as a theme goes: a white fill, a black line, and the cells a themed marker falls back to.
const ROOT: &str = r##"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#ffffff"/><Cell N="FillBkgnd" V="#000000"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/><Section N="Character"><Row IX="0"><Cell N="Color" V="#000000"/><Cell N="Size" V="0.1666666666666667" U="PT"/></Row></Section></StyleSheet>"##;

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The file's own theme part carries 40 in its five components, and its first accent is `204060`.
const FILE_ACCENT: &str = "#204060";

fn geometry() -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    format!(
        r#"{}{}{}{}{}{}<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
        cell("PinX", "1"),
        cell("PinY", "1"),
        cell("Width", "2"),
        cell("Height", "1"),
        cell("LocPinX", "0"),
        cell("LocPinY", "0"),
        cell("NoFill", "0"),
        row("MoveTo", 1, 0.0, 0.0),
        row("LineTo", 2, 2.0, 0.0),
        row("LineTo", 3, 2.0, 1.0),
        row("LineTo", 4, 0.0, 1.0),
        row("LineTo", 5, 0.0, 0.0)
    )
}

/// A box that holds its own scheme cells (a shape pasted from a drawing with theme 40), its Quick Style cells (accent1 for the fill, fill entry 1),
/// the number the last application of theme 40 left in `FillForegnd` under the call that names the property, and the marker in `FillBkgnd`.
fn own_box(id: u32, extra: &str) -> String {
    let inner = [
        geometry(),
        FIVE.map(|n| cell(n, "40")).concat(),
        [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "1"), ("QuickStyleLineColor", "3"), ("QuickStyleLineMatrix", "1")]
            .map(|(n, v)| cell(n, v))
            .concat(),
        cell_f("FillForegnd", "#abcdef", "THEMEVAL(\"FillColor\")"),
        cell_f("FillBkgnd", "Themed", "THEMEVAL()"),
        cell("FillPattern", "1"),
        extra.to_owned(),
    ]
    .concat();
    shape_with(id, r#" LineStyle="0" FillStyle="0" TextStyle="0""#, &inner)
}

fn page_sheet(extra: &str) -> String {
    [cell("PageWidth", "11"), cell("PageHeight", "8.5"), cell("PageScale", "1"), cell("DrawingScale", "1")].concat() + extra
}

fn file(shapes: &str, sheet_extra: &str) -> Vec<u8> {
    VsdxBuilder::new()
        .styles(ROOT)
        .theme(ThemeXml::default().xml().as_bytes())
        .page(PageSpec::new(1, "File").sheet(&page_sheet(sheet_extra)).shapes(shapes))
        .build()
}

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-theme-file-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opened(dir: &Dir, bytes: &[u8]) -> Session {
    let path = dir.0.join("in.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path.to_str().unwrap()}));
    s
}

/// The value text of a cell directly under shape `id`, as the shape holds it itself.
fn held(d: &Document, id: u32, name: &str) -> Option<String> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id)?)?;
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

// ---- a drawing with a theme of its own ----------------------------------------------------------------------------------------------------

#[test]
fn a_page_with_a_theme_of_a_file_is_told_custom_and_a_catalogue_theme_replaces_it() {
    let dir = Dir::new();
    let mut s = opened(&dir, &file(&own_box(1, ""), &FIVE.map(|n| cell(n, "40")).concat()));
    let listed = run(&mut s, "theme.list", json!({}));
    assert_eq!(listed["page"]["theme"], "custom", "40 is no theme of the catalogue");
    assert_eq!(listed["themes"][0]["in_drawing"], false);
    // A variant of the file's theme is allowed: the page has a theme.
    run(&mut s, "theme.variant", json!({"variant": 1}));
    assert_eq!(theme::selection_of(s.document().unwrap(), 1).unwrap().numbers(), [40, 40, 40, 40, 40, 1, 1]);
    // Applying the catalogue's theme adds its part beside the file's; the file's stays.
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!((r["theme_added"].clone(), r["style_added"].clone()), (json!(true), json!(true)));
    assert_eq!(s.document().unwrap().themes.len(), 2);
    assert_eq!(
        s.document().unwrap().themes.get(0).unwrap().theme_scheme_id.as_ref().unwrap().scheme_enum,
        40,
        "the file's part is first and unchanged"
    );
    assert_eq!(run(&mut s, "theme.list", json!({}))["page"]["theme"], "tidepool");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_scheme_cells_of_its_own_takes_the_new_numbers_and_its_cached_colour_is_worked_out_again() {
    let dir = Dir::new();
    let mut s = opened(&dir, &file(&own_box(1, ""), &FIVE.map(|n| cell(n, "40")).concat()));
    // Before: the stale `#abcdef` is what the shape holds, and the marker asks the file's theme (accent1 of part 40, under fill entry 1).
    assert_eq!(held(s.document().unwrap(), 1, "FillForegnd").as_deref(), Some("#abcdef"));
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(r["cells_rewritten"], 5, "the five scheme cells of the shape: {r}");
    let d = s.document().unwrap();
    for name in ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"] {
        assert_eq!(held(d, 1, name).as_deref(), Some("1001"), "{name}");
    }
    // The call that names the property is worked out again, with the new theme: fill entry 1 of Tidepool's list is the plain colour, and the shape's
    // `QuickStyleFillColor` 2 picks the first accent. The marker is not written (a recalculation leaves `Themed` alone) and asks the theme when drawn.
    assert_eq!(held(d, 1, "FillForegnd").as_deref(), Some("#1f8ca8"));
    assert_eq!(held(d, 1, "FillBkgnd").as_deref(), Some("Themed"));
    let shape = d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    assert_eq!(shape.sheet.cell("FillForegnd").unwrap().formula.text(), Some("THEMEVAL(\"FillColor\")"), "the formula stays");
    // No theme: the same shape reads the no-theme table for the call that names the property.
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    let d = s.document().unwrap();
    assert_eq!(held(d, 1, "ThemeIndex").as_deref(), Some("0"));
    assert_eq!(held(d, 1, "FillForegnd").as_deref(), Some("#96afcf"));
    assert_eq!(held(d, 1, "FillBkgnd").as_deref(), Some("Themed"));
    // A theme again: the cells were zeroed by "no theme", so the shape is given the page's numbers once more.
    run(&mut s, "theme.apply", json!({"theme": "fernwood"}));
    assert_eq!(held(s.document().unwrap(), 1, "FillForegnd").as_deref(), Some("#4c9a52"), "fernwood's first accent");
}

#[test]
fn a_lock_keeps_a_new_theme_from_changing_the_number_it_names() {
    let dir = Dir::new();
    // Shape 2 locks its colours; shape 3 locks its theme number (which holds no colour); shape 1 locks nothing.
    let shapes = [own_box(1, ""), own_box(2, &cell("LockThemeColors", "1")), own_box(3, &cell("LockThemeIndex", "1"))].concat();
    let mut s = opened(&dir, &file(&shapes, &FIVE.map(|n| cell(n, "40")).concat()));
    let r = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(r["cells_rewritten"], 5 + 4 + 4, "shape 1 five cells, shapes 2 and 3 four each");
    let d = s.document().unwrap();
    assert_eq!(held(d, 2, "ColorSchemeIndex").as_deref(), Some("40"), "locked");
    assert_eq!(held(d, 2, "EffectSchemeIndex").as_deref(), Some("1001"));
    assert_eq!(held(d, 3, "ThemeIndex").as_deref(), Some("40"), "locked");
    assert_eq!(held(d, 3, "ColorSchemeIndex").as_deref(), Some("1001"));
    // Shape 2 draws with the colours of the file's own theme and the lists of Tidepool: accent1 of part 40, plain, as entry 1 gives it.
    assert_eq!(held(d, 2, "FillForegnd").as_deref(), Some(FILE_ACCENT));
    assert_eq!(held(d, 1, "FillForegnd").as_deref(), Some("#1f8ca8"));
}

/// `LockVariation` keeps a variant chosen for the page off the shape's variant cells,
/// whatever its two theme locks say, as the colour lock holds `VariationColorIndex` and the effect lock `VariationStyleIndex`.
#[test]
fn a_variation_lock_keeps_the_variant_cells_of_a_shape_with_the_theme_locks_off() {
    let dir = Dir::new();
    let variants = [cell("VariationColorIndex", "1"), cell("VariationStyleIndex", "1")].concat();
    // Shape 1 locks its variant only; shape 2 locks its colours only; shape 3 locks its effects only; shape 4 locks nothing; shape 5 locks both theme locks and not the variant.
    let shapes = [
        own_box(1, &(variants.clone() + &cell("LockVariation", "1"))),
        own_box(2, &(variants.clone() + &cell("LockThemeColors", "1"))),
        own_box(3, &(variants.clone() + &cell("LockThemeEffects", "1"))),
        own_box(4, &variants),
        own_box(5, &(variants.clone() + &cell("LockThemeColors", "1") + &cell("LockThemeEffects", "1") + &cell("LockVariation", "0"))),
    ]
    .concat();
    let mut s = opened(&dir, &file(&shapes, &FIVE.map(|n| cell(n, "40")).concat()));
    run(&mut s, "theme.variant", json!({"variant": 3}));
    let d = s.document().unwrap();
    let both = |id: u32| (held(d, id, "VariationColorIndex"), held(d, id, "VariationStyleIndex"));
    let text = |a: &str, b: &str| (Some(a.to_owned()), Some(b.to_owned()));
    assert_eq!(both(1), text("1", "1"), "LockVariation holds both variant cells");
    assert_eq!(both(2), text("1", "3"), "the colour lock holds the colour variant alone");
    assert_eq!(both(3), text("3", "1"), "the effect lock holds the style variant alone");
    assert_eq!(both(4), text("3", "3"), "no lock: both follow the page");
    assert_eq!(both(5), text("1", "1"), "both theme locks hold both, with LockVariation off");
    // A new theme moves the variant too, and the lock holds it again: the variant of a theme applied is a variant chosen.
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "variant": 2}));
    let d = s.document().unwrap();
    let both = |id: u32| (held(d, id, "VariationColorIndex"), held(d, id, "VariationStyleIndex"));
    assert_eq!(both(1), text("1", "1"), "still held after a theme");
    assert_eq!(both(4), text("2", "2"));
    // The cell holds its lock as any other flag is read: a number other than 0 is on.
    let mut t = opened(&dir, &file(&own_box(1, &(variants + &cell("LockVariation", "10"))), &FIVE.map(|n| cell(n, "40")).concat()));
    run(&mut t, "theme.variant", json!({"variant": 3}));
    assert_eq!(held(t.document().unwrap(), 1, "VariationColorIndex").as_deref(), Some("1"), "10 is locked");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_scheme_cell_that_is_a_formula_or_65534_or_65535_or_not_a_number_is_left_alone() {
    let dir = Dir::new();
    let odd = [
        cell("ThemeIndex", "65534"),
        cell("ColorSchemeIndex", "65535"),
        cell_f("EffectSchemeIndex", "40", "40+0"),
        cell("ConnectorSchemeIndex", "-3"),
        cell("FontSchemeIndex", "forty"),
    ]
    .concat();
    let shape = shape_with(1, r#" LineStyle="0" FillStyle="0" TextStyle="0""#, &(geometry() + &odd));
    let mut s = opened(&dir, &file(&shape, ""));
    let r = run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    assert_eq!(r["cells_rewritten"], 0, "{r}");
    let d = s.document().unwrap();
    for (name, want) in [
        ("ThemeIndex", "65534"),
        ("ColorSchemeIndex", "65535"),
        ("EffectSchemeIndex", "40"),
        ("ConnectorSchemeIndex", "-3"),
        ("FontSchemeIndex", "forty"),
    ] {
        assert_eq!(held(d, 1, name).as_deref(), Some(want), "{name}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_whose_cells_hold_absurd_selectors_is_themed_all_the_same() {
    let dir = Dir::new();
    let sheet =
        [cell("ThemeIndex", "4294967296"), cell("ColorSchemeIndex", "-1"), cell("EffectSchemeIndex", "1e999"), cell("FontSchemeIndex", "x")].concat();
    let mut s = opened(&dir, &file(&own_box(1, ""), &sheet));
    assert_eq!(run(&mut s, "theme.list", json!({}))["page"]["theme"], "none", "none of them is a whole number: the page has no theme");
    run(&mut s, "theme.apply", json!({"theme": "inkstone"}));
    assert_eq!(theme::selection_of(s.document().unwrap(), 1).unwrap().numbers(), [1004, 1004, 1004, 1004, 1004, 0, 0]);
    assert_eq!(check_session(&s), Ok(()));
}

// ---- hostile sizes ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_page_of_shapes_that_all_hold_scheme_cells_of_their_own_is_themed_by_one_pass_and_one_recalculation() {
    // 10,000 shapes, each with five cells of its own and a lock. The locks are looked up through one resolver, made once for the page.
    let dir = Dir::new();
    let one = |id: u32| own_box(id, &cell("LockThemeFonts", if id.is_multiple_of(2) { "1" } else { "0" }));
    let shapes: String = (1..=10_000).map(one).collect();
    let mut s = opened(&dir, &file(&shapes, &FIVE.map(|n| cell(n, "40")).concat()));
    // Counted, not timed: the page is worked out once (`diacraft_calc::work`), and the resolver that finds the locks is made once for the page (the unit
    // test of `theme/apply.rs` counts those).
    diacraft_calc::work::reset();
    let r = run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    eprintln!(
        "a theme on a page of 10,000 shapes with scheme cells of their own: {} commit of {} steps",
        diacraft_calc::work::commits(),
        diacraft_calc::work::steps()
    );
    assert_eq!(r["cells_rewritten"], 5_000 * 5 + 5_000 * 4);
    assert_eq!(diacraft_calc::work::commits(), 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_most_themes_a_drawing_holds_are_listed_and_applied_with_a_recalculation_each() {
    // 255 parts of the file's own, so that the catalogue's joins as the 256th: every lookup of a number walks them.
    let dir = Dir::new();
    let mut builder = VsdxBuilder::new().styles(ROOT).page(PageSpec::new(1, "File").sheet(&page_sheet("")).shapes(&own_box(1, "")));
    let theme_xml = ThemeXml::default().xml();
    for _ in 0..255 {
        builder = builder.theme(theme_xml.as_bytes());
    }
    let mut s = opened(&dir, &builder.build());
    assert_eq!(s.document().unwrap().themes.len(), 255);
    diacraft_calc::work::reset();
    run(&mut s, "theme.list", json!({}));
    let r = run(&mut s, "theme.apply", json!({"theme": "plumline"}));
    run(&mut s, "theme.variant", json!({"variant": 3, "scope": "all"}));
    // The apply and the variant each work the one page out once, and the list works out nothing: the themes in the drawing cost lookups and no commit.
    assert_eq!(diacraft_calc::work::commits(), 2);
    assert_eq!(r["theme_added"], true);
    assert_eq!(s.document().unwrap().themes.len(), 256);
    // One more is refused: a drawing holds at most what a reader keeps.
    let e = s.execute("theme.apply", &json!({"theme": "tidepool"})).unwrap_err();
    assert!(e.to_string().contains("at most"), "{e}");
}

#[test]
fn a_hostile_theme_part_in_the_file_costs_the_steps_of_the_page_and_no_more() {
    // A part whose lists are as long as the reader keeps them and whose colours carry the most modifiers each: a THEMEVAL that reads it is bounded by
    // `calc`'s steps (`theme_eval.rs`), and the application of a theme to a page that selects it is bounded by the page.
    let dir = Dir::new();
    let mut xml = ThemeXml::default().xml();
    let fill = format!("<a:solidFill><a:schemeClr val=\"phClr\">{}</a:schemeClr></a:solidFill>", "<a:tint val=\"50000\"/>".repeat(16));
    xml = xml.replacen("<a:fillStyleLst>", &format!("<a:fillStyleLst>{}", fill.repeat(200)), 1);
    let bytes = VsdxBuilder::new()
        .styles(ROOT)
        .theme(xml.as_bytes())
        .page(PageSpec::new(1, "File").sheet(&page_sheet(&FIVE.map(|n| cell(n, "40")).concat())).shapes(&own_box(1, "")))
        .build();
    let mut s = opened(&dir, &bytes);
    diacraft_calc::work::reset();
    run(&mut s, "theme.variant", json!({"variant": 2}));
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    // Two commits, and far fewer steps than the budget a commit may take: the part's 16-modifier fills are applied to one colour each time it is read.
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("a variant and a removal on a page that selects a part of 200 sixteen-modifier fills: {commits} commits of {steps} steps");
    assert_eq!(commits, 2);
    assert!(steps < diacraft_calc::Budget::DEFAULT_STEPS / 100, "{steps} steps");
    assert_eq!(check_session(&s), Ok(()));
}
