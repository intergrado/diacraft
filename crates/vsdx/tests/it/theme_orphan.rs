//! A theme part that no relationship of the document part reaches. A file may list the part in its content
//! types and leave out the relationship: the reader finds the part by its content type, and failing that by the conventional path, so the drawing
//! is themed; a strict reading of the package would leave the part unread. The package is built in code: no file from another application is used.
//!
//! A save keeps the file's own way where it changes no theme (the part, its content type and the document's relationships are the original's),
//! and writes a relationship where it must (a model writer, or a theme added beside the orphan), so that a saved file reads themed again.

use diacraft_doc::{Document, SharedVec};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{DIR, PageSpec, Pkg, VsdxBuilder};
use diacraft_testkit::theme::ThemeXml;
use diacraft_vsdx::{Limits, OriginalBytes, WriteOptions, Written, read, work, write, write_with};

use crate::read_ok;

const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const THEME_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

/// A part of the package, by its name inside the format's folder.
fn at(rest: &str) -> String {
    format!("{DIR}/{rest}")
}

/// The first accent of the part below, and the scheme number its five components carry.
const ACCENT1: &str = "5B9BD5";
const SCHEME: u32 = 40;

fn theme_bytes() -> Vec<u8> {
    let colours = ["111111", "FEFEFE", "44546A", "E7E6E6", ACCENT1, "ED7D31", "A5A5A5", "FFC000", "4472C4", "70AD47", "0563C1", "954F72"];
    ThemeXml { colours, ..ThemeXml::default() }.xml().into_bytes()
}

/// A drawing whose theme part has the theme content type and no relationship.
fn orphan() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(Pkg::new().part(&at("theme/theme1.xml"), CT_THEME, theme_bytes())).build()
}

fn accent1_of(doc: &Document) -> Vec<Option<String>> {
    doc.themes.iter().map(|t| t.slots.get(4).copied().flatten().map(|c| c.hex().trim_start_matches('#').to_ascii_uppercase())).collect()
}

fn package(bytes: &[u8]) -> Package {
    Package::read(bytes, &OpcLimits::default()).unwrap()
}

fn text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(package(bytes).part(name).unwrap().to_vec()).unwrap()
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"))
}

/// The theme parts the document part's relationships reach, by target.
fn theme_targets(bytes: &[u8]) -> Vec<String> {
    let rels = text(bytes, &at("_rels/document.xml.rels"));
    rels.split("<Relationship ")
        .filter(|row| row.contains(THEME_REL))
        .filter_map(|row| row.split("Target=\"").nth(1).and_then(|t| t.split('"').next()).map(str::to_owned))
        .collect()
}

// ---- the read ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_theme_part_no_relationship_reaches_is_read_by_its_content_type_and_the_report_says_so() {
    let (doc, report) = read_ok(&orphan());
    assert_eq!(doc.themes.len(), 1, "the part is a theme: {:?}", report.warnings);
    assert_eq!(accent1_of(&doc), [Some(ACCENT1.to_owned())]);
    assert_eq!(doc.themes.get(0).unwrap().theme_scheme_id.as_ref().map(|s| s.scheme_enum), Some(SCHEME));
    assert_eq!(&*doc.themes.get(0).unwrap().part, at("theme/theme1.xml"));
    assert_eq!(report.stats.themes, 1);
    assert_eq!(report.warning_count("theme_without_relationship"), 1, "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains(&at("theme/theme1.xml")) && w.contains("no relationship")), "{:?}", report.warnings);
}

#[test]
fn a_theme_part_with_a_relationship_is_read_as_it_was_and_says_nothing() {
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P")).theme(&theme_bytes()).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 1);
    assert_eq!(report.warning_count("theme_without_relationship"), 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_theme_part_beside_one_a_relationship_reaches_stays_an_unknown_part() {
    // The relationship names the first part; the second is listed in the content types and no relationship reaches it. Only the narrow case finds a theme by
    // its content type: a file that has a theme relationship is read as it always was.
    let other = ThemeXml { scheme: 41, ..ThemeXml::default() }.xml().into_bytes();
    let bytes =
        VsdxBuilder::new().page(PageSpec::new(1, "P")).theme(&theme_bytes()).with(Pkg::new().part(&at("theme/theme9.xml"), CT_THEME, other)).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 1, "{:?}", report.warnings);
    assert_eq!(&*doc.themes.get(0).unwrap().part, at("theme/theme1.xml"));
    assert_eq!(report.warning_count("theme_without_relationship"), 0);
    let held = doc.parts.iter().any(|p| *p.name == at("theme/theme9.xml"));
    assert!(held, "the second part is kept as the bytes it is");
}

#[test]
fn several_orphans_are_read_in_the_order_of_their_names_and_the_budget_of_themes_holds() {
    let second = ThemeXml { scheme: 41, ..ThemeXml::default() }.xml().into_bytes();
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .with(Pkg::new().part(&at("theme/theme2.xml"), CT_THEME, second).part(&at("theme/theme1.xml"), CT_THEME, theme_bytes()))
        .build();
    let (doc, _) = read_ok(&bytes);
    let schemes: Vec<u32> = doc.themes.iter().filter_map(|t| t.theme_scheme_id.as_ref().map(|s| s.scheme_enum)).collect();
    assert_eq!(schemes, [SCHEME, 41], "theme1 before theme2, whatever order the package lists them in");
}

#[test]
fn a_theme_part_with_no_theme_content_type_is_found_by_the_conventional_path() {
    // No `Override` and a generic `Default` for `xml`: the content type says nothing, and the part is where themes are.
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .with(Pkg::new().default_type("xml", "application/xml").part_without_override(&at("theme/theme1.xml"), theme_bytes()))
        .build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 1, "{:?}", report.warnings);
    assert_eq!(accent1_of(&doc), [Some(ACCENT1.to_owned())]);
    assert_eq!(report.warning_count("theme_without_relationship"), 1, "{:?}", report.warnings);
}

#[test]
fn a_part_with_the_theme_content_type_that_is_not_a_theme_stays_an_unknown_part() {
    let stray = b"stray bytes that are not a theme".to_vec();
    let not_a_theme = b"<a:other xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"/>".to_vec();
    for bytes in [stray, not_a_theme] {
        let package = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(Pkg::new().part(&at("theme/theme1.xml"), CT_THEME, bytes)).build();
        let (doc, report) = read_ok(&package);
        assert!(doc.themes.is_empty(), "nothing that is not a theme is taken for one");
        assert_eq!(report.warning_count("theme_without_relationship"), 0);
        assert_eq!(report.warning_count("theme_unreadable"), 0, "it was never read as a theme: {:?}", report.warnings);
    }
}

#[test]
fn a_dangling_theme_relationship_keeps_the_reading_it_had() {
    // A relationship that names a part the package lacks says the file means a theme: it is not read from another part by content type.
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .with(Pkg::new().rel(&at("document.xml"), "rId77", THEME_REL, "theme/gone.xml").part(&at("theme/theme1.xml"), CT_THEME, theme_bytes()))
        .build();
    let (doc, report) = read_ok(&bytes);
    assert!(doc.themes.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.warning_count("part_missing"), 1);
}

#[test]
fn a_theme_part_reached_by_a_relationship_of_another_type_is_read_by_its_content_type() {
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .with(Pkg::new().part(&at("theme/theme1.xml"), CT_THEME, theme_bytes()).rel(
            &at("document.xml"),
            "rId77",
            "http://example.invalid/relationships/not-a-theme",
            "theme/theme1.xml",
        ))
        .build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 1, "{:?}", report.warnings);
}

// ---- a save -----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_save_that_changes_no_theme_keeps_the_file_s_own_way() {
    let original = orphan();
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let written = save(&doc, &original, &baseline);
    assert!(written.added.is_empty() && written.removed.is_empty(), "{written:?}");
    for name in [at("theme/theme1.xml"), "/[Content_Types].xml".to_owned(), at("_rels/document.xml.rels")] {
        assert_eq!(package(&written.bytes).part(&name), package(&original).part(&name), "{name} is the original's");
    }
    assert!(theme_targets(&written.bytes).is_empty(), "the file's own way: no relationship");
    // The saved file reads themed again.
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(accent1_of(&back), [Some(ACCENT1.to_owned())]);
}

#[test]
fn a_theme_added_beside_the_orphan_takes_a_relationship_and_so_does_the_orphan() {
    // With a relationship to the new part, the reader no longer looks at the content types: the orphan must have one too, or the file would read with
    // one theme where the model holds two.
    let original = orphan();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let added = ThemeXml { scheme: 41, ..ThemeXml::default() }.xml().into_bytes();
    let mut themes: Vec<_> = doc.themes.iter().cloned().collect();
    themes.push(std::sync::Arc::new(diacraft_vsdx::read_theme_part(&at("theme/theme9.xml"), &added)));
    doc.themes = SharedVec::from_vec(themes);
    let written = save(&doc, &original, &baseline);
    assert_eq!(written.added, [at("theme/theme2.xml")], "{written:?}");
    let reached = theme_targets(&written.bytes);
    assert!(reached.iter().any(|t| t.ends_with("theme1.xml")) && reached.iter().any(|t| t.ends_with("theme2.xml")), "{reached:?}");
    assert_eq!(
        package(&written.bytes).part(&at("theme/theme1.xml")),
        package(&original).part(&at("theme/theme1.xml")),
        "the orphan's bytes are the original's"
    );
    let (back, report) = read_ok(&written.bytes);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let schemes: Vec<u32> = back.themes.iter().filter_map(|t| t.theme_scheme_id.as_ref().map(|s| s.scheme_enum)).collect();
    assert_eq!(schemes, [SCHEME, 41]);
}

#[test]
fn an_edit_of_the_orphan_s_bytes_is_written_into_its_own_part_with_no_relationship_made_up() {
    let original = orphan();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let edited = ThemeXml { scheme: SCHEME, background: "123456", ..ThemeXml::default() }.xml().into_bytes();
    let part = doc.themes.get(0).unwrap().part.clone();
    doc.themes.set(0, std::sync::Arc::new(diacraft_vsdx::read_theme_part(&part, &edited))).unwrap();
    let written = save(&doc, &original, &baseline);
    assert_eq!(package(&written.bytes).part(&at("theme/theme1.xml")).unwrap(), edited.as_slice());
    assert!(theme_targets(&written.bytes).is_empty(), "the file's own way");
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.themes.len(), 1);
}

#[test]
fn a_model_writer_gives_the_theme_a_relationship_and_the_file_reads_themed_without_a_word() {
    let (doc, _) = read_ok(&orphan());
    let bytes = write(&doc, None).unwrap();
    let reached = theme_targets(&bytes);
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert!(package(&bytes).part(&at("theme/theme1.xml")).is_some(), "the part keeps its name");
    assert!(package(&bytes).part(&at("theme/theme2.xml")).is_none(), "no second theme part is made");
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(accent1_of(&back), [Some(ACCENT1.to_owned())]);
    assert_eq!(report.warning_count("theme_without_relationship"), 0, "a relationship leads to it now");
}

// ---- hostile packages ---------------------------------------------------------------------------------------------------------------------

/// A part that says it is a theme and is a tree of `elements` empty elements under another root: well-formed, and no theme.
fn junk_theme(elements: usize) -> Vec<u8> {
    let mut xml = String::from("<x>");
    xml.push_str(&"<y/>".repeat(elements));
    xml.push_str("</x>");
    xml.into_bytes()
}

fn events_of(bytes: &[u8]) -> u64 {
    work::reset();
    let _ = read_ok(bytes);
    work::xml_events_read()
}

#[test]
fn parts_that_only_say_they_are_themes_are_parsed_to_the_budget_of_theme_bytes_and_no_further() {
    // Twelve parts of 500 KB: six megabytes that say they are themes, against a budget of four. Every `<y/>` is one event, so the events the parts add are
    // what was parsed of them. Counted, as time is no measure on a machine at load: a reader that charged only the parts it took for themes parsed all twelve.
    let plain = events_of(&orphan_less());
    let mut pkg = Pkg::new();
    for i in 0..12 {
        pkg = pkg.part(&at(&format!("theme/junk{i:02}.xml")), CT_THEME, junk_theme(125_000));
    }
    let hostile = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build();
    let (doc, report) = read_ok(&hostile);
    assert!(doc.themes.is_empty(), "no part of them is a theme");
    assert_eq!(report.warning_count("themes_over_budget"), 4, "the four parts past the budget are named: {:?}", report.warnings);
    let added = events_of(&hostile) - plain;
    let allowed = diacraft_vsdx::MAX_THEME_BYTES / 4 + 64;
    assert!(added <= allowed, "{added} events were parsed of the parts, a budget of {allowed}");
    assert!(added >= allowed / 2, "{added} events: the budget is spent on the parts and not on nothing");
}

fn orphan_less() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P")).build()
}

#[test]
fn only_so_many_parts_that_may_be_themes_are_looked_at() {
    // More parts than the reader keeps themes, each tiny: the first `max_themes` by name are looked at, and the real theme named last is not reached.
    let limits = Limits::default();
    let mut pkg = Pkg::new();
    for i in 0..limits.max_themes + 20 {
        pkg = pkg.part(&at(&format!("theme/a{i:04}.xml")), CT_THEME, junk_theme(1));
    }
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg.part(&at("theme/zz.xml"), CT_THEME, theme_bytes())).build();
    let (doc, report) = read_ok(&bytes);
    assert!(doc.themes.is_empty(), "the theme sorts after the cap");
    assert_eq!(report.warning_count("themes_over_budget"), 1, "{:?}", report.warnings);
}

#[test]
fn a_hostile_theme_part_found_by_its_content_type_is_read_within_the_pool_or_refused_in_words() {
    // The charge of the part is given back when it is no theme: a read in a small pool ends as a read or a refusal for memory in words.
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(Pkg::new().part(&at("theme/theme1.xml"), CT_THEME, junk_theme(50_000))).build();
    let (_, first) = diacraft_vsdx::read_partial(&bytes, &Limits::default());
    let room = first.stats.file_bytes + first.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    for pool in [0u64, 1_000, 100_000, 600_000] {
        let (result, report) = diacraft_vsdx::read_partial(&bytes, &Limits { max_memory: room + pool, ..Limits::default() });
        assert!(report.stats.memory_peak <= room + pool, "pool {pool}: {:?}", report.stats);
        if let Err(e) = result {
            assert!(!e.to_string().chars().any(char::is_control), "{e:?}");
        }
    }
}
