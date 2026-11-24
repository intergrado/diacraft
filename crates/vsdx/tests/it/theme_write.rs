//! Writing theme parts: a definition made into a part that reads back as
//! the definition, and a save that adds, rewrites and takes out theme parts while every other part stays the original's, byte for byte.
//!
//! The themes here are composed for these tests, with a value at each position that no neighbour holds.

use std::sync::Arc;

use diacraft_color::{ColorTransform, Rgba, SchemeSlot};
use diacraft_doc::{
    ColourVariant, Document, EffectEntry, FillEntry, FillProps, FontProps, GradientEntry, GradientKind, LineEntry, LineEx, MAX_THEME_ENTRIES,
    SchemeId, ShadowEntry, StyleLists, StyleVariant, Theme, ThemeBase, ThemeColour, ThemeEntries, ThemeStop, VarStyle,
};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use diacraft_vsdx::{
    OriginalBytes, ThemeDef, ThemeWriteError, WriteError, WriteOptions, Written, read, read_theme_part, theme_part_bytes, write, write_with,
};

use crate::read_ok;

const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

fn rgb(hex: &str) -> Rgba {
    Rgba::from_hex(hex).unwrap()
}

fn ph(mods: Vec<ColorTransform>) -> ThemeColour {
    ThemeColour { base: ThemeBase::Slot(SchemeSlot::PhClr), modifiers: mods }
}

fn solid(mods: Vec<ColorTransform>) -> FillEntry {
    FillEntry::Solid(ph(mods))
}

fn line(width: i64, dash: &str, fill: FillEntry) -> LineEntry {
    LineEntry {
        width_emu: Some(width),
        cap: Some("flat".to_owned()),
        compound: Some("sng".to_owned()),
        dash: Some(dash.to_owned()),
        fill: Some(fill),
    }
}

fn shadow(blur: i64, dist: i64, alpha: i32) -> EffectEntry {
    EffectEntry {
        outer_shadow: Some(ShadowEntry {
            blur_emu: blur,
            dist_emu: dist,
            dir: 2_700_000,
            scale_x: 1.0,
            scale_y: 1.0,
            colour: ThemeColour { base: ThemeBase::Rgb(Rgba::BLACK), modifiers: vec![ColorTransform::Alpha(alpha)] },
        }),
        has_inner_shadow: false,
    }
}

fn ex(pattern: u32, end: u32) -> Option<LineEx> {
    Some(LineEx { rounding_emu: Some(0), start: Some(0), start_size: Some(2), end: Some(end), end_size: Some(2), pattern: Some(pattern) })
}

fn font(style: u32, slot: SchemeSlot) -> Option<FontProps> {
    Some(FontProps { style: Some(style), colour: Some(ThemeColour { base: ThemeBase::Slot(slot), modifiers: Vec::new() }) })
}

/// A whole theme: twelve colours, four colour variants (the second `monotone`), four style variants of four styles, six entries in each list of
/// the effect scheme and three in the connector scheme's, one more `vt:lineEx` than lines.
pub fn composed(name: &str, scheme: u32) -> ThemeDef {
    let gradient = FillEntry::Gradient(GradientEntry {
        stops: vec![
            ThemeStop { pos: 0.0, colour: ph(vec![ColorTransform::Tint(70000)]) },
            ThemeStop { pos: 1.0, colour: ph(vec![ColorTransform::Shade(85000)]) },
        ],
        kind: GradientKind::Linear { degrees: 90.0 },
        rotate_with_shape: Some(true),
    });
    let fills = vec![
        solid(vec![]),
        solid(vec![ColorTransform::Tint(50000)]),
        gradient.clone(),
        FillEntry::NoFill,
        solid(vec![ColorTransform::Shade(70000)]),
        FillEntry::Solid(ThemeColour { base: ThemeBase::Slot(SchemeSlot::Lt1), modifiers: Vec::new() }),
    ];
    let lines = vec![
        line(12700, "solid", solid(vec![ColorTransform::Shade(80000)])),
        line(19050, "solid", solid(vec![ColorTransform::Shade(70000)])),
        line(28575, "solid", solid(vec![])),
        line(12700, "dash", solid(vec![])),
        line(6350, "solid", solid(vec![])),
        line(9525, "solid", FillEntry::NoFill),
    ];
    let effects = vec![
        EffectEntry::default(),
        EffectEntry::default(),
        shadow(50800, 25400, 35000),
        shadow(76200, 38100, 45000),
        shadow(12700, 12700, 30000),
        EffectEntry::default(),
    ];
    let accents = ["204060", "608020", "a04020", "2080c0", "c0a020", "806080"];
    let variant = |first: usize, monotone: bool| {
        let mut colours = [None; 7];
        for (i, slot) in colours.iter_mut().enumerate() {
            *slot = Some(rgb(accents[(first + i) % 6]));
        }
        Some(ColourVariant { monotone, colours })
    };
    let style = |fill: u32, line: u32, effect: u32, font: u32| VarStyle {
        fill_idx: Some(fill),
        line_idx: Some(line),
        effect_idx: Some(effect),
        font_idx: Some(font),
    };
    let style_variant = |embellishment: u32, n: u32| {
        Some(StyleVariant {
            embellishment: Some(embellishment),
            styles: vec![style(1 + n, 1, 1, 1), style(5, 2, 3, 4), style(2, 1, 1, 1), style(6, 3 + n % 2, 1, 1)],
        })
    };
    let entries = ThemeEntries {
        background: Some(rgb("f4f4f0")),
        colour_variants: vec![variant(0, false), variant(1, true), variant(2, false), variant(3, false)],
        style_variants: vec![style_variant(0, 0), style_variant(1, 1), style_variant(2, 2), style_variant(3, 3)],
        effect: StyleLists {
            fills: fills.clone().into_iter().map(Some).collect(),
            lines: lines.into_iter().map(Some).collect(),
            effects: effects.into_iter().map(Some).collect(),
        },
        connector: StyleLists {
            fills: vec![Some(solid(vec![])); 3],
            lines: vec![
                Some(line(12700, "solid", solid(vec![]))),
                Some(line(19050, "solid", solid(vec![]))),
                Some(line(28575, "solid", solid(vec![]))),
            ],
            effects: vec![Some(EffectEntry::default()); 3],
        },
        line_ex: (0..7).map(|i| ex(1 + i % 3, 0)).collect(),
        connector_line_ex: (0..4).map(|i| ex(1, if i == 2 { 4 } else { 0 })).collect(),
        fill_props: vec![Some(FillProps { pattern: Some(1) }); 6],
        fonts: vec![
            font(0, SchemeSlot::PhClr),
            font(0, SchemeSlot::PhClr),
            font(0, SchemeSlot::PhClr),
            font(0, SchemeSlot::Lt1),
            font(0, SchemeSlot::Lt1),
            font(1, SchemeSlot::Lt1),
        ],
        connector_fonts: vec![font(0, SchemeSlot::PhClr); 3],
    };
    ThemeDef {
        name: name.to_owned(),
        scheme: SchemeId { scheme_enum: scheme, scheme_guid: Some(Arc::from("{11111111-2222-3333-4444-555555555555}")) },
        colours: [
            rgb("101820"),
            rgb("ffffff"),
            rgb("304050"),
            rgb("e8eef4"),
            rgb("204060"),
            rgb("608020"),
            rgb("a04020"),
            rgb("2080c0"),
            rgb("c0a020"),
            rgb("806080"),
            rgb("0a60b0"),
            rgb("704090"),
        ],
        major_font: "Heading Face".to_owned(),
        minor_font: "Body Face".to_owned(),
        entries,
        effect_background_fills: fills.iter().take(3).cloned().collect(),
        connector_background_fills: vec![solid(vec![]); 3],
    }
}

fn theme(def: &ThemeDef, part: &str) -> Arc<Theme> {
    Arc::new(read_theme_part(part, &theme_part_bytes(def).unwrap()))
}

// ---- the part ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn write_then_read_gives_the_definition() {
    let def = composed("Composed", 1234);
    let bytes = theme_part_bytes(&def).unwrap();
    let t = read_theme_part("/visio/theme/theme1.xml", &bytes);
    assert_eq!(t.entries, def.entries, "every list, variant and extension");
    assert_eq!(t.slots.map(|c| c.unwrap()), def.colours);
    assert!(t.colors.is_some(), "all twelve colours are read");
    assert_eq!(t.colors.as_ref().unwrap().name, "Composed");
    assert_eq!((t.major_font.as_deref(), t.minor_font.as_deref()), (Some("Heading Face"), Some("Body Face")));
    // All five components carry the number, and the GUID is kept as it was given.
    for id in [&t.color_scheme_id, &t.font_scheme_id, &t.effect_scheme_id, &t.connector_scheme_id, &t.theme_scheme_id] {
        let id = id.as_ref().unwrap();
        assert_eq!(id.scheme_enum, 1234);
        assert_eq!(id.scheme_guid.as_deref(), Some("{11111111-2222-3333-4444-555555555555}"));
    }
}

#[test]
fn the_part_has_the_root_the_declaration_and_every_extension_of() {
    let text = String::from_utf8(theme_part_bytes(&composed("T & <U>", 7)).unwrap()).unwrap();
    // The declaration the samples' theme parts start with.
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"), "{}", &text[..80]);
    // The root and its three children in order.
    assert!(text.contains("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"T &amp; &lt;U"), "{}", &text[..200]);
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("no {needle}"));
    assert!(at("<a:themeElements>") < at("<a:objectDefaults") && at("<a:objectDefaults") < at("<a:extraClrSchemeLst"));
    // The twelve URIs.
    for uri in [
        "{093E89EA-6996-430E-BFF9-83A9FAAAAB73}",
        "{2703A3B3-D2E1-43D9-8057-6E9D74E0F44A}",
        "{DDD2D869-C2EF-471E-B8FA-914AFA308C9F}",
        "{B3C0788F-89CC-4CDB-A57D-0664CFDA821B}",
        "{1342405F-259F-4C95-8CDF-A9DCE2D418A2}",
        "{27CD58D4-7086-4B73-B2AB-7AEBE2148A8C}",
        "{C6430689-8E98-42EC-ADBF-087148533A3F}",
        "{D75FF423-9257-4291-A4FE-1B2448832E17}",
        "{6CAB99AB-0A78-4BAB-B597-526D62367CB4}",
        "{56243398-1771-4C39-BF73-A5702A9C147F}",
        "{EBE24D50-EC5C-4D6F-A1A3-C5F0A18B936A}",
        "{494CE47F-D151-47DC-95E8-85652EA8A67E}",
    ] {
        assert!(text.contains(&format!("uri=\"{uri}\"")), "the extension {uri}");
    }
    // The vt: namespace is declared by each extension's child, as the samples' are.
    assert!(text.contains("<vt:schemeID xmlns:vt=\"http://schemas.microsoft.com/office/visio/2012/theme\" schemeEnum=\"7\""));
    // The lists have the lengths real parts have, and R11 the lineStyles one more than the lines.
    assert_eq!(text.matches("<a:ln ").count(), 9, "six lines and three connector lines");
    assert_eq!(text.matches("<vt:lineStyle>").count(), 11, "seven and four");
    assert_eq!(text.matches("<vt:varColor1>").count(), 4, "Four colour variants");
    assert_eq!(text.matches("<vt:varStyle ").count(), 16, "Four variants of four");
    // `monotone` is written only where it is true, so an absent one reads as false.
    assert_eq!(text.matches("monotone=\"1\"").count(), 1);
}

#[test]
fn a_definition_the_reader_could_not_hold_is_refused_and_not_invented() {
    let refuse = |def: ThemeDef| match theme_part_bytes(&def) {
        Err(ThemeWriteError::Unwritable(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    };
    let mut def = composed("A", 1);
    def.entries.effect.fills[0] = None;
    assert!(refuse(def).contains("fill"));
    let mut def = composed("A", 1);
    def.entries.effect.fills[0] = Some(FillEntry::Other);
    assert!(refuse(def).contains("not solid"));
    let mut def = composed("A", 1);
    def.entries.colour_variants[1] = None;
    assert!(refuse(def).contains("colour variant"));
    let mut def = composed("A", 1);
    def.entries.line_ex[2] = None;
    assert!(refuse(def).contains("line extension"));
    // A list past what the reader keeps would be cut on the way back.
    let mut def = composed("A", 1);
    def.entries.fill_props = vec![Some(FillProps { pattern: Some(1) }); MAX_THEME_ENTRIES + 1];
    assert!(refuse(def).contains("longer than the reader keeps"));
    // A modifier the theme functions do not apply has nothing to read it back as.
    let mut def = composed("A", 1);
    def.entries.effect.fills[0] =
        Some(FillEntry::Solid(ThemeColour { base: ThemeBase::Slot(SchemeSlot::PhClr), modifiers: vec![ColorTransform::Comp] }));
    assert!(refuse(def).contains("modifier"));
    // A name XML cannot hold is an error, not a bad part.
    let mut def = composed("A", 1);
    def.name = "bad\u{1}name".to_owned();
    assert!(matches!(theme_part_bytes(&def), Err(ThemeWriteError::Xml(_))));
}

#[test]
fn a_hostile_definition_costs_a_pass_over_its_lists_and_no_more() {
    // The longest lists the reader keeps, every colour with the most modifiers: the part is a pass over the lists, so its size is bounded by them (the
    // 256 fills of 16 modifiers and five copies of the name), and it is read back whole.
    let mut def = composed("Big", 9);
    let many = ThemeColour { base: ThemeBase::Slot(SchemeSlot::PhClr), modifiers: vec![ColorTransform::Tint(1); diacraft_doc::MAX_THEME_MODIFIERS] };
    def.entries.effect.fills = vec![Some(FillEntry::Solid(many)); MAX_THEME_ENTRIES];
    def.name = "x".repeat(100_000);
    let bytes = theme_part_bytes(&def).unwrap();
    let t = read_theme_part("/visio/theme/theme1.xml", &bytes);
    let bound = MAX_THEME_ENTRIES * (diacraft_doc::MAX_THEME_MODIFIERS * 40 + 100) + 5 * 100_000 + 64 * 1024;
    assert!(bytes.len() <= bound, "{} bytes against {bound}", bytes.len());
    assert_eq!(t.entries.effect.fills.len(), MAX_THEME_ENTRIES);
}

// ---- a save -----------------------------------------------------------------------------------------------------------------------------

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    Package::read(bytes, &OpcLimits::default()).unwrap().part(name).map(<[u8]>::to_vec)
}

fn names(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &OpcLimits::default()).unwrap().parts().map(str::to_owned).collect()
}

fn content_type(bytes: &[u8], name: &str) -> Option<String> {
    Package::read(bytes, &OpcLimits::default()).unwrap().content_type(name).map(str::to_owned)
}

fn text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(part(bytes, name).unwrap_or_else(|| panic!("no part {name}"))).unwrap()
}

fn original_with_themes(defs: &[ThemeDef]) -> Vec<u8> {
    let mut b = VsdxBuilder::new().page(PageSpec::new(1, "P"));
    for d in defs {
        b = b.theme(&theme_part_bytes(d).unwrap());
    }
    b.build()
}

#[test]
fn a_theme_the_model_gained_is_added_with_its_relationship_and_its_type() {
    let original = original_with_themes(&[composed("One", 41), composed("Two", 42)]);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert_eq!(doc.themes.len(), 2);
    doc.themes.push(theme(&composed("Three", 43), "/visio/theme/whatever.xml"));
    let written = save(&doc, &original, &baseline);
    // The new part is named by the writer, in the folder the others are in, and the model's name is not trusted.
    assert_eq!(written.added, ["/visio/theme/theme3.xml"], "{written:?}");
    assert!(names(&written.bytes).contains(&"/visio/theme/theme3.xml".to_owned()));
    let rels = text(&written.bytes, "/visio/_rels/document.xml.rels");
    assert!(
        rels.contains("Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme3.xml\""),
        "{rels}"
    );
    assert_eq!(content_type(&written.bytes, "/visio/theme/theme3.xml").as_deref(), Some(CT_THEME));
    // It reads back as three themes, selected by their numbers and not by their places.
    let (back, report) = read(&written.bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.themes.len(), 3);
    assert_eq!(back.theme_by_index(43).unwrap().part.as_ref(), "/visio/theme/theme3.xml");
    assert_eq!(back.theme_by_index(42).unwrap().part.as_ref(), "/visio/theme/theme2.xml");
    assert_eq!(&back.themes.get(2).unwrap().entries, &theme(&composed("Three", 43), "x").entries);
    // Every part the edit did not touch is the original's, byte for byte (the two themes, the page, the document).
    for kept in ["/visio/theme/theme1.xml", "/visio/theme/theme2.xml", "/visio/document.xml", "/visio/pages/page1.xml", "/visio/pages/pages.xml"] {
        assert_eq!(part(&written.bytes, kept), part(&original, kept), "{kept}");
    }
    // The old relationships stay as they were, with their ids.
    assert!(rels.contains("Target=\"theme/theme1.xml\"") && rels.contains("Target=\"theme/theme2.xml\""), "{rels}");
}

#[test]
fn a_save_that_adds_a_theme_twice_names_each_part_once() {
    // The second save compares the read-back baseline with the model, which holds the engine's name for the part: the same bytes under another
    // name are the same part and are written no more than once.
    let original = original_with_themes(&[composed("One", 41)]);
    let (mut doc, _) = read_ok(&original);
    doc.themes.push(theme(&composed("Two", 42), "/visio/theme/theme2.xml"));
    let first = save(&doc, &original, &read_ok(&original).0);
    assert_eq!(first.added, ["/visio/theme/theme2.xml"]);
    // The model names the part `/visio/theme/zzz.xml`; the file calls it theme2.xml.
    doc.themes.set(1, theme(&composed("Two", 42), "/visio/theme/zzz.xml")).unwrap();
    let second = save(&doc, &first.bytes, &first.document);
    assert!(second.added.is_empty() && second.removed.is_empty(), "{second:?}");
    assert!(!second.rewritten.iter().any(|p| p.contains("theme")), "{second:?}");
    assert_eq!(part(&second.bytes, "/visio/theme/theme2.xml"), part(&first.bytes, "/visio/theme/theme2.xml"));
}

#[test]
fn a_theme_whose_bytes_changed_is_rewritten_in_its_own_part_and_the_rest_are_not() {
    let original = original_with_themes(&[composed("One", 41), composed("Two", 42)]);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let renamed = theme(&composed("Two, renamed", 42), "/visio/theme/theme2.xml");
    doc.themes.set(1, renamed.clone()).unwrap();
    let written = save(&doc, &original, &baseline);
    // (The application's name in `docProps/app.xml` is the writer's on every save with an edit.)
    let themes: Vec<&String> = written.rewritten.iter().filter(|p| p.contains("/theme/")).collect();
    assert_eq!(themes, ["/visio/theme/theme2.xml"], "{written:?}");
    assert!(written.added.is_empty() && written.removed.is_empty(), "{written:?}");
    assert_eq!(part(&written.bytes, "/visio/theme/theme2.xml").unwrap(), renamed.bytes.to_vec());
    assert_eq!(part(&written.bytes, "/visio/theme/theme1.xml"), part(&original, "/visio/theme/theme1.xml"));
    // No relationship and no type is touched: the document's relationships part is not rewritten.
    assert_eq!(part(&written.bytes, "/visio/_rels/document.xml.rels"), part(&original, "/visio/_rels/document.xml.rels"));
}

#[test]
fn a_theme_the_model_lost_goes_with_its_relationship_and_its_type() {
    let original = original_with_themes(&[composed("One", 41), composed("Two", 42), composed("Three", 43)]);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    // The model drops the middle theme: a list without it.
    let kept: Vec<Arc<Theme>> = doc.themes.iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, t)| t.clone()).collect();
    doc.themes = diacraft_doc::SharedVec::from_vec(kept);
    let written = save(&doc, &original, &baseline);
    assert_eq!(written.removed, ["/visio/theme/theme2.xml"], "{written:?}");
    assert!(!names(&written.bytes).contains(&"/visio/theme/theme2.xml".to_owned()));
    let rels = text(&written.bytes, "/visio/_rels/document.xml.rels");
    assert!(!rels.contains("theme2.xml") && rels.contains("theme1.xml") && rels.contains("theme3.xml"), "{rels}");
    assert_ne!(content_type(&written.bytes, "/visio/theme/theme2.xml").as_deref(), Some(CT_THEME), "its Override went with it");
    assert_eq!(content_type(&written.bytes, "/visio/theme/theme3.xml").as_deref(), Some(CT_THEME));
    let (back, report) = read(&written.bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.themes.iter().map(|t| t.theme_scheme_id.as_ref().unwrap().scheme_enum).collect::<Vec<_>>(), [41, 43]);
    for kept in ["/visio/theme/theme1.xml", "/visio/theme/theme3.xml"] {
        assert_eq!(part(&written.bytes, kept), part(&original, kept), "{kept}");
    }
}

#[test]
fn a_theme_part_something_else_reaches_stays_when_the_document_lets_go_of_it() {
    // A second relationship (from the pages index here) reaches the part: the document's own goes, and the part stays for the one that is left.
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .theme(&theme_part_bytes(&composed("One", 41)).unwrap())
        .with(diacraft_testkit::package::Pkg::new().rel(
            "/visio/pages/pages.xml",
            "rId77",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
            "../theme/theme1.xml",
        ))
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.themes = diacraft_doc::SharedVec::new();
    let written = save(&doc, &original, &baseline);
    assert!(written.removed.is_empty(), "{written:?}");
    assert!(names(&written.bytes).contains(&"/visio/theme/theme1.xml".to_owned()));
    assert!(!text(&written.bytes, "/visio/_rels/document.xml.rels").contains("theme1.xml"));
}

#[test]
fn a_drawing_that_was_never_saved_gets_its_themes() {
    let mut doc = Document::new_blank();
    doc.themes.push(theme(&composed("One", 41), "/visio/theme/theme1.xml"));
    doc.themes.push(theme(&composed("Two", 42), "/visio/theme/theme2.xml"));
    let bytes = write(&doc, None).unwrap();
    let (back, report) = read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.themes.len(), 2);
    assert_eq!(back.theme_by_index(42).unwrap().entries, doc.themes.get(1).unwrap().entries);
    assert_eq!(content_type(&bytes, "/visio/theme/theme2.xml").as_deref(), Some(CT_THEME));
    // The eleven parts of a new drawing, and the themes.
    assert_eq!(names(&bytes).iter().filter(|n| n.contains("/theme/")).count(), 2);
}

#[test]
fn a_part_with_the_name_a_new_theme_would_take_is_not_overwritten() {
    // A theme part no relationship reaches is an unknown part. A new theme must not take its name.
    let stray = b"stray bytes that are not a theme".to_vec();
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .with(diacraft_testkit::package::Pkg::new().part("/visio/theme/theme1.xml", CT_THEME, &stray))
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.themes.push(theme(&composed("New", 50), "/visio/theme/theme1.xml"));
    let written = save(&doc, &original, &baseline);
    assert_eq!(written.added, ["/visio/theme/theme2.xml"], "{written:?}");
    assert_eq!(part(&written.bytes, "/visio/theme/theme1.xml").unwrap(), stray, "the stray part is the original's");
}

#[test]
fn a_theme_that_is_not_a_theme_part_is_refused_and_nothing_is_written() {
    let original = original_with_themes(&[composed("One", 41)]);
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    for bytes in [&b"not xml at all"[..], &b"<a:other xmlns:a=\"x\"/>"[..], &b"<a:theme xmlns:a=\"x\">"[..]] {
        let mut bad = doc.clone();
        bad.themes.push(Arc::new(Theme { part: Arc::from("/visio/theme/bad.xml"), bytes: Arc::from(bytes), ..Theme::default() }));
        let err = write_with(&bad, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &WriteOptions::default()).unwrap_err();
        assert!(matches!(err, WriteError::Unsupported(why) if why.contains("a:theme")), "{err:?}");
    }
    // A rewrite to bytes that are not a theme is refused too.
    let mut bad = doc.clone();
    bad.themes.set(0, Arc::new(Theme { part: Arc::from("/visio/theme/theme1.xml"), bytes: Arc::from(&b"<x/>"[..]), ..Theme::default() })).unwrap();
    assert!(write_with(&bad, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &WriteOptions::default()).is_err());
}

#[test]
fn more_themes_than_the_reader_keeps_are_refused() {
    let original = original_with_themes(&[composed("One", 41)]);
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let tiny = Arc::new(Theme { part: Arc::from("/visio/theme/t.xml"), bytes: Arc::from(&b"<a:theme xmlns:a=\"x\"/>"[..]), ..Theme::default() });
    let mut many = doc.clone();
    for _ in 0..diacraft_vsdx::MAX_THEMES {
        many.themes.push(tiny.clone());
    }
    let err = write_with(&many, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &WriteOptions::default()).unwrap_err();
    assert!(matches!(err, WriteError::Limit(what) if what.contains("theme parts")), "{err:?}");
    // And bytes: one part over the budget.
    let mut heavy = doc.clone();
    let big = vec![b' '; usize::try_from(diacraft_vsdx::MAX_THEME_BYTES).unwrap() + 1];
    let mut bytes = b"<a:theme xmlns:a=\"x\"/>".to_vec();
    bytes.extend_from_slice(&big);
    heavy.themes.push(Arc::new(Theme { part: Arc::from("/visio/theme/h.xml"), bytes: Arc::from(bytes), ..Theme::default() }));
    let err = write_with(&heavy, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &WriteOptions::default()).unwrap_err();
    assert!(matches!(err, WriteError::Limit(what) if what.contains("bytes of theme")), "{err:?}");
}

#[test]
fn pairing_themes_is_linear_in_what_the_model_holds() {
    // A hundred themes of about 20 KB each, near what the reader keeps in bytes: pairing hashes each once, and the save of a changed one is quick.
    let defs: Vec<ThemeDef> = (0..100_u32).map(|i| composed(&format!("T{i}"), 100 + i)).collect();
    let original = original_with_themes(&defs);
    let (mut doc, _) = read_ok(&original);
    assert_eq!(doc.themes.len(), 100);
    let baseline = doc.clone();
    // Reverse the list (every theme is under a place that is not its own) and change one.
    let mut reversed: Vec<Arc<Theme>> = doc.themes.iter().cloned().collect();
    reversed.reverse();
    reversed[10] = theme(&composed("Changed", 190), "/visio/theme/other.xml");
    doc.themes = diacraft_doc::SharedVec::from_vec(reversed);
    let total: u64 = doc.themes.iter().map(|t| t.bytes.len() as u64).sum();
    diacraft_vsdx::work::reset();
    let written = save(&doc, &original, &baseline);
    // Only the changed theme and the one it replaced are paired by their bytes, and each is hashed once to be found and once to look for it: the work
    // is what those two hold and not a comparison of every pair (counted, `diacraft_vsdx::work`).
    let hashed = diacraft_vsdx::work::theme_bytes_hashed();
    assert!(hashed <= 2 * total / 50, "{hashed} bytes hashed for {total} bytes of themes");
    // The reversed ones are the same parts under the same names, the changed one is a new part (its name matched none), and the one it replaced
    // is gone.
    assert_eq!(written.added.len(), 1, "{:?}", written.added);
    assert_eq!(written.removed.len(), 1, "{:?}", written.removed);
}

// ---- generated definitions --------------------------------------------------------------------------------------------------------------

mod generated {
    use proptest::prelude::*;

    use super::*;

    fn arb_rgba() -> impl Strategy<Value = Rgba> {
        (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Rgba::rgb(r, g, b))
    }

    fn arb_modifier() -> impl Strategy<Value = ColorTransform> {
        (0_u8..7, -200_000_i32..400_000).prop_map(|(kind, v)| match kind {
            0 => ColorTransform::Tint(v),
            1 => ColorTransform::Shade(v),
            2 => ColorTransform::LumMod(v),
            3 => ColorTransform::LumOff(v),
            4 => ColorTransform::SatMod(v),
            5 => ColorTransform::SatOff(v),
            _ => ColorTransform::Alpha(v),
        })
    }

    fn arb_colour() -> impl Strategy<Value = ThemeColour> {
        let base = prop_oneof![
            arb_rgba().prop_map(ThemeBase::Rgb),
            prop::sample::select(vec![SchemeSlot::PhClr, SchemeSlot::Dk1, SchemeSlot::Lt1, SchemeSlot::Accent3, SchemeSlot::Tx2])
                .prop_map(ThemeBase::Slot),
        ];
        (base, prop::collection::vec(arb_modifier(), 0..4)).prop_map(|(base, modifiers)| ThemeColour { base, modifiers })
    }

    fn arb_fill() -> impl Strategy<Value = FillEntry> {
        let stops = prop::collection::vec((0_u32..=100_000, arb_colour()), 1..4)
            .prop_map(|v| v.into_iter().map(|(pos, colour)| ThemeStop { pos: f64::from(pos) / 100_000.0, colour }).collect::<Vec<_>>());
        let kind = prop_oneof![
            (-21_600_000_i32..21_600_000).prop_map(|a| GradientKind::Linear { degrees: f64::from(a) / 60_000.0 }),
            Just(GradientKind::Circle),
            Just(GradientKind::Rect),
            Just(GradientKind::Shape),
            Just(GradientKind::Unstated),
        ];
        prop_oneof![
            arb_colour().prop_map(FillEntry::Solid),
            Just(FillEntry::NoFill),
            (stops, kind, prop::option::of(any::<bool>())).prop_map(|(stops, kind, rotate_with_shape)| FillEntry::Gradient(GradientEntry {
                stops,
                kind,
                rotate_with_shape
            })),
        ]
    }

    fn arb_line() -> impl Strategy<Value = LineEntry> {
        (
            prop::option::of(0_i64..5_000_000),
            prop::option::of(prop::sample::select(vec!["rnd".to_owned(), "flat".to_owned(), "sq".to_owned()])),
            prop::option::of(prop::sample::select(vec!["sng".to_owned(), "dbl".to_owned(), "tri".to_owned()])),
            prop::sample::select(vec!["solid".to_owned(), "dash".to_owned(), "sysDot".to_owned()]),
            prop::option::of(arb_fill()),
        )
            .prop_map(|(width_emu, cap, compound, dash, fill)| LineEntry { width_emu, cap, compound, dash: Some(dash), fill })
    }

    fn arb_effect() -> impl Strategy<Value = EffectEntry> {
        let shadow = (0_i64..500_000, 0_i64..500_000, 0_i64..21_600_000, 0_u32..300_000, 0_u32..300_000, arb_colour()).prop_map(
            |(blur, dist, dir, sx, sy, colour)| ShadowEntry {
                blur_emu: blur,
                dist_emu: dist,
                dir,
                scale_x: f64::from(sx) / 100_000.0,
                scale_y: f64::from(sy) / 100_000.0,
                colour,
            },
        );
        prop::option::of(shadow).prop_map(|outer_shadow| EffectEntry { outer_shadow, has_inner_shadow: false })
    }

    fn arb_lists(len: std::ops::Range<usize>) -> impl Strategy<Value = StyleLists> {
        (prop::collection::vec(arb_fill(), len.clone()), prop::collection::vec(arb_line(), len.clone()), prop::collection::vec(arb_effect(), len))
            .prop_map(|(f, l, e)| StyleLists {
                fills: f.into_iter().map(Some).collect(),
                lines: l.into_iter().map(Some).collect(),
                effects: e.into_iter().map(Some).collect(),
            })
    }

    fn arb_line_ex() -> impl Strategy<Value = Option<LineEx>> {
        (prop::option::of(0_i64..100_000), 0_u32..46, 0_u32..4, 0_u32..46, 0_u32..4, prop::option::of(0_u32..24)).prop_map(
            |(rounding_emu, start, start_size, end, end_size, pattern)| {
                Some(LineEx { rounding_emu, start: Some(start), start_size: Some(start_size), end: Some(end), end_size: Some(end_size), pattern })
            },
        )
    }

    fn arb_entries() -> impl Strategy<Value = ThemeEntries> {
        let variants = prop::collection::vec((any::<bool>(), prop::collection::vec(arb_rgba(), 7)), 0..5).prop_map(|v| {
            v.into_iter()
                .map(|(monotone, c)| {
                    let mut colours = [None; 7];
                    for (slot, colour) in colours.iter_mut().zip(c) {
                        *slot = Some(colour);
                    }
                    Some(ColourVariant { monotone, colours })
                })
                .collect::<Vec<_>>()
        });
        let var_style = (0_u32..9, 0_u32..9, 0_u32..9, 0_u32..9).prop_map(|(a, b, c, d)| VarStyle {
            fill_idx: Some(a),
            line_idx: Some(b),
            effect_idx: Some(c),
            font_idx: Some(d),
        });
        let style_variants = prop::collection::vec((prop::option::of(0_u32..9), prop::collection::vec(var_style, 0..5)), 0..5)
            .prop_map(|v| v.into_iter().map(|(embellishment, styles)| Some(StyleVariant { embellishment, styles })).collect::<Vec<_>>());
        let fonts = || {
            prop::collection::vec((0_u32..256, prop::option::of(arb_colour())), 0..7)
                .prop_map(|v| v.into_iter().map(|(style, colour)| Some(FontProps { style: Some(style), colour })).collect::<Vec<_>>())
        };
        (
            prop::option::of(arb_rgba()),
            variants,
            style_variants,
            (arb_lists(0..7), arb_lists(0..4)),
            (prop::collection::vec(arb_line_ex(), 0..8), prop::collection::vec(arb_line_ex(), 0..5)),
            (fonts(), fonts()),
            prop::collection::vec(prop::option::of(0_u32..40), 0..7),
        )
            .prop_map(
                |(
                    background,
                    colour_variants,
                    style_variants,
                    (effect, connector),
                    (line_ex, connector_line_ex),
                    (fonts, connector_fonts),
                    patterns,
                )| {
                    ThemeEntries {
                        background,
                        colour_variants,
                        style_variants,
                        effect,
                        connector,
                        line_ex,
                        connector_line_ex,
                        fill_props: patterns.into_iter().map(|pattern| Some(FillProps { pattern })).collect(),
                        fonts,
                        connector_fonts,
                    }
                },
            )
    }

    fn arb_def() -> impl Strategy<Value = ThemeDef> {
        (
            "[A-Za-z0-9 &<>'\"-]{0,24}",
            0_u32..70_000,
            prop::option::of("\\{[0-9A-F]{8}-[0-9A-F]{4}\\}"),
            prop::collection::vec(arb_rgba(), 12),
            ("[A-Za-z ]{0,16}", "[A-Za-z ]{0,16}"),
            arb_entries(),
        )
            .prop_map(|(name, scheme_enum, guid, colours, (major_font, minor_font), entries)| {
                let mut c = [Rgba::BLACK; 12];
                for (slot, colour) in c.iter_mut().zip(colours) {
                    *slot = colour;
                }
                ThemeDef {
                    name,
                    scheme: SchemeId { scheme_enum, scheme_guid: guid.map(|g| Arc::from(g.as_str())) },
                    colours: c,
                    major_font,
                    minor_font,
                    entries,
                    effect_background_fills: vec![FillEntry::NoFill; 3],
                    connector_background_fills: vec![FillEntry::NoFill; 3],
                }
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        /// Whatever definition is written reads back as it was, and the reader finds no warning in it.
        #[test]
        fn any_definition_reads_back_as_itself(def in arb_def()) {
            let bytes = theme_part_bytes(&def).unwrap();
            let t = read_theme_part("/visio/theme/theme1.xml", &bytes);
            prop_assert_eq!(&t.entries, &def.entries);
            prop_assert_eq!(t.slots.map(|c| c.unwrap()), def.colours);
            prop_assert_eq!(t.theme_scheme_id.as_ref().map(|i| i.scheme_enum), Some(def.scheme.scheme_enum));
            prop_assert_eq!(t.theme_scheme_id.as_ref().and_then(|i| i.scheme_guid.clone()), def.scheme.scheme_guid.clone());
            prop_assert_eq!(t.major_font.as_deref(), Some(def.major_font.as_str()));
            // And it is a part a package takes: written into a drawing and read from the file.
            let mut doc = Document::new_blank();
            doc.themes.push(Arc::new(t));
            let file = write(&doc, None).unwrap();
            let (back, report) = read(&file, &diacraft_vsdx::Limits::default()).unwrap();
            prop_assert_eq!(back.themes.len(), 1);
            prop_assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        }
    }
}

#[test]
fn pairing_by_bytes_is_hashed_whatever_the_themes_share() {
    // The worst case for a comparison of bytes: 200 parts that are the same but for their last bytes (and so equal in length), and a second model
    // that names every one differently, so that none pairs by name. Each part is hashed once; nothing is written, and the time shows it.
    let tail = |i: usize| format!("<!-- {i:08} -->");
    let body = |i: usize| {
        let mut bytes = theme_part_bytes(&composed("Same", 7)).unwrap();
        bytes.extend_from_slice(tail(i).as_bytes());
        bytes
    };
    let mut builder = VsdxBuilder::new().page(PageSpec::new(1, "P"));
    for i in 0..200 {
        builder = builder.theme(&body(i));
    }
    let original = builder.build();
    let (doc, _) = read_ok(&original);
    assert_eq!(doc.themes.len(), 200);
    let baseline = doc.clone();
    let mut renamed = doc.clone();
    let all: Vec<Arc<Theme>> = doc
        .themes
        .iter()
        .rev()
        .enumerate()
        .map(|(i, t)| Arc::new(Theme { part: Arc::from(format!("/visio/theme/other{i}.xml")), ..(**t).clone() }))
        .collect();
    renamed.themes = diacraft_doc::SharedVec::from_vec(all);
    let total: u64 = doc.themes.iter().map(|t| t.bytes.len() as u64).sum();
    diacraft_vsdx::work::reset();
    let written = save(&renamed, &original, &baseline);
    // Every theme is hashed once to be found and once to look for it, whatever the parts share: twice the bytes of the themes at the most, where
    // comparing each with each would read them 200 times over (counted, `diacraft_vsdx::work`).
    let hashed = diacraft_vsdx::work::theme_bytes_hashed();
    assert!(hashed <= 2 * total, "{hashed} bytes hashed for {total} bytes of themes");
    assert!(hashed >= total, "{hashed} bytes hashed: every theme was paired by its bytes");
    assert!(written.added.is_empty() && written.removed.is_empty(), "{:?} {:?}", written.added.len(), written.removed.len());
    assert!(!written.rewritten.iter().any(|p| p.contains("/theme/")), "no theme part is written again");
    // Two parts with the very same bytes pair with the two that have them, one each.
    let twin = composed("Twin", 9);
    let original = original_with_themes(&[twin.clone(), twin.clone()]);
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let mut renamed = doc.clone();
    let both: Vec<Arc<Theme>> =
        doc.themes.iter().enumerate().map(|(i, t)| Arc::new(Theme { part: Arc::from(format!("/visio/theme/x{i}.xml")), ..(**t).clone() })).collect();
    renamed.themes = diacraft_doc::SharedVec::from_vec(both);
    let written = save(&renamed, &original, &baseline);
    assert!(written.added.is_empty() && written.removed.is_empty(), "{written:?}");
}
