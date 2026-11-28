//! The languages of a drawing's text and of the drawing itself cross a save as `.vsdx`, `.odg` and `.diacraft`:
//! each run is read back in the language it was in, the drawing's language is the same in all three, and no setting of the person is in any of the files.
//!
//! **Every file is made by code**: the VSDX package by the testkit's builder (the shapes' XML written by hand from the specifications), the readers and writers are the real ones.

use diacraft_doc::{Document, Shape, TextPart};
use diacraft_odg::package::Package;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape_with};
use serde_json::json;

use crate::native_common::{Dir, pinned};
use crate::run;
use crate::stencil_open::box_cells;

/// The core properties of a file whose language is German (Germany).
const CORE: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
    "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
    "<dc:title>Sprachen</dc:title><dc:language>de-DE</dc:language></cp:coreProperties>"
);

/// U+65E5 U+672C U+8A9E: Japanese letters.
const NIHONGO: &str = "\u{65E5}\u{672C}\u{8A9E}";

/// The runs the drawing's one shape holds, with the language each is in.
const RUNS: [(&str, &str); 4] = [("Hallo", "de-CH"), (NIHONGO, "ja-JP"), ("zxx", "x-none"), ("end", "en-US")];

/// A drawing in German whose shape says four runs in four languages, one of them no language, one of Asian letters.
fn drawing() -> Vec<u8> {
    let rows: String = RUNS
        .iter()
        .enumerate()
        .map(|(ix, (_, lang))| format!(r#"<Row IX="{ix}"><Cell N="LangID" V="{lang}"/><Cell N="Size" V="0.2"/></Row>"#))
        .collect();
    let text: String = RUNS.iter().enumerate().map(|(ix, (run, _))| format!(r#"<cp IX="{ix}"/>{run}"#)).collect();
    let shape = shape_with(
        5,
        r#" Type="Shape""#,
        &format!(
            r##"{}<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#003366"/><Cell N="FillPattern" V="1"/><Section N="Character">{rows}</Section><Text>{text}</Text>"##,
            box_cells(2.0, 1.0)
        ),
    );
    VsdxBuilder::new()
        .core(CORE)
        .document_sheet(r#"<Cell N="DocLangID" V="de-DE"/>"#)
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shape))
        .build()
}

/// The first shape of the first page.
fn the_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap()
}

/// What each run of the first shape's text is in: its characters, and the `LangID` of the `Character` row in force.
fn langs(doc: &Document) -> Vec<(String, Option<String>)> {
    let shape = the_shape(doc);
    let lang_of = |ix: u32| {
        let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character")?;
        let row = section.rows.iter().find(|r| r.ix == Some(ix))?;
        row.cells.get("LangID")?.value.as_ref().map(|v| v.text().to_owned())
    };
    let (mut out, mut row) = (Vec::new(), 0u32);
    for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match part {
            TextPart::Cp(ix) => row = *ix,
            TextPart::Chars(c) => out.push((c.to_string(), lang_of(row))),
            _ => {}
        }
    }
    out
}

fn expected() -> Vec<(String, Option<String>)> {
    RUNS.iter().map(|(run, lang)| ((*run).to_owned(), Some((*lang).to_owned()))).collect()
}

/// The drawing's language as the three places say it: `DocLangID`, the core properties' language, and what [`diacraft_doc::lang::document_language`] makes of them.
fn document_language(doc: &Document) -> (Option<String>, Option<String>, String) {
    let doc_lang_id = doc.sheet.cell("DocLangID").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    (doc_lang_id, doc.props.language.as_deref().map(str::to_owned), diacraft_doc::lang::document_language(doc, None).to_string())
}

fn opened(dir: &Dir, name: &str) -> Document {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s.document().unwrap().clone()
}

fn save_as(s: &mut diacraft_engine::Session, dir: &Dir, name: &str) {
    s.execute("file.save_as", &json!({ "path": dir.path(name) })).unwrap_or_else(|e| panic!("save as {name}: {e}"));
}

/// Across the formats: the drawing saved as `.vsdx`, as `.odg` and as `.diacraft`, each read back, holds the same languages for the same runs and the same language
/// for the drawing.
#[test]
fn the_languages_of_the_runs_and_of_the_drawing_cross_every_format() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    assert_eq!(langs(s.document().unwrap()), expected(), "as the VSDX file has them");
    let want_document = (Some("de-DE".to_owned()), Some("de-DE".to_owned()), "de-DE".to_owned());
    assert_eq!(document_language(s.document().unwrap()), want_document);
    for name in ["b.vsdx", "b.odg", "b.diacraft"] {
        save_as(&mut s, &dir, name);
        let back = opened(&dir, name);
        assert_eq!(langs(&back), expected(), "{name}");
        assert_eq!(document_language(&back), want_document, "{name}");
    }
    // And on through the other formats: the `.odg` to `.diacraft` to `.vsdx`.
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path("b.odg") }));
    save_as(&mut s, &dir, "c.diacraft");
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path("c.diacraft") }));
    save_as(&mut s, &dir, "c.vsdx");
    let back = opened(&dir, "c.vsdx");
    assert_eq!(langs(&back), expected(), "the .odg's languages in a VSDX file");
    assert_eq!(document_language(&back).2, "de-DE");
}

/// The `.odg` says the languages in the standard's own attributes, where another program reads them: the Latin triplet for Latin letters, the Asian one for Japanese ones, `zxx` and
/// `none` for no language; and `dc:language` says the drawing's.
#[test]
fn the_odg_says_the_languages_in_odf_s_own_attributes() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    save_as(&mut s, &dir, "b.odg");
    let package = Package::read(&dir.read("b.odg"), &diacraft_odg::Limits::default()).unwrap();
    let part = |name: &str| String::from_utf8(package.part(name).unwrap().to_vec()).unwrap();
    let content = part("content.xml");
    for attr in [
        r#"fo:language="de""#,
        r#"fo:country="CH""#,
        r#"style:language-asian="ja""#,
        r#"style:country-asian="JP""#,
        r#"fo:language="zxx""#,
        r#"fo:country="none""#,
        r#"fo:language="en""#,
        r#"fo:country="US""#,
    ] {
        assert!(content.contains(attr), "{attr} in {content}");
    }
    assert!(part("meta.xml").contains("<dc:language>de-DE</dc:language>"), "{}", part("meta.xml"));
}

/// Nothing of the person's settings is in any part of any of the files (the custom dictionaries, the AutoCorrect lists and the options of the proofing are the person's, never the
/// drawing's).
#[test]
fn no_setting_of_the_person_is_in_any_file() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    for name in ["b.vsdx", "b.odg", "b.diacraft"] {
        save_as(&mut s, &dir, name);
    }
    let package = Package::read(&dir.read("b.odg"), &diacraft_odg::Limits::default()).unwrap();
    for entry in package.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        let text = String::from_utf8_lossy(package.part(&entry.name).unwrap()).to_lowercase();
        for word in ["custom.dic", "autocorrect", "ignore-all", "ignoreall", "proofing-options", "editing-languages"] {
            assert!(!text.contains(word), "{word} in {}", entry.name);
        }
    }
}
