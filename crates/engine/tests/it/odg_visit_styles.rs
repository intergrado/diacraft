//! Through the engine (PL5's second leg): an `.odg` is opened, Save As `.diacraft`, a new session opens that file, Save As `.odg`, and the content check judges the file that
//! came back against the file that was opened, in the two parts that say what the drawing is (`content.xml` and `styles.xml`). A second visit of what came back is the same bytes (clock
//! pinned). The crate-level tests of the same names are `crates/odg/tests/it/visit_styles.rs`.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{Drawing, GR1, part_of, rect};
use crate::run;

/// The parts a visit is judged by.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// A drawing as ODF 1.4, the version the model writer writes (the version is no class of this task), whose automatic styles are `auto` and whose common styles are `common`.
fn source(auto: &str, common: &str, shapes: &str) -> Vec<u8> {
    let mut drawing = Drawing::new().page(shapes);
    // The shapes the builder makes name the paragraph style `P1`.
    drawing.auto = format!("{}{auto}", crate::odg_build::P1);
    drawing.common = common.to_owned();
    let (content, styles) = (
        drawing.content_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
        drawing.styles_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
    );
    let mut files = vec![
        ("content.xml".to_owned(), content.into_bytes()),
        ("styles.xml".to_owned(), styles.into_bytes()),
        ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
    ];
    files.extend(drawing.files.iter().cloned());
    crate::odg_build::package(crate::odg_build::DRAWING, files)
}

/// What the content check says of the drawing that went through `.diacraft` and came back, in the drawing's two parts, and the file that came back.
fn visit(dir: &Dir, source: &[u8]) -> (Vec<u8>, Vec<String>) {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("w.odg") }));
    let written = dir.read("w.odg");
    let diff = diacraft_odg::compare_content(source, &written, &diacraft_odg::Limits::default()).unwrap();
    let shown =
        diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    (written, shown)
}

/// The visit is the same content, and a visit of what came back is the same bytes.
fn assert_same(what: &str, source: &[u8]) -> Vec<u8> {
    let dir = Dir::new();
    let (written, found) = visit(&dir, source);
    assert!(found.is_empty(), "{what}: the visit differs from the file: {found:#?}");
    let (again, found) = visit(&Dir::new(), &written);
    assert!(found.is_empty(), "{what}: the second visit differs from the file: {found:#?}");
    assert_eq!(again, written, "{what}: a visit of what came back is the same bytes (clock pinned)");
    written
}

const UNUSED: &str =
    r##"<style:style style:name="gr9" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##;
const UNUSED_PARAGRAPH: &str =
    r#"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;

#[test]
fn an_automatic_style_nothing_names_comes_back() {
    let shapes = rect("A", (2.0, 3.0, 4.0, 2.0), "");
    let written = assert_same("a graphic style", &source(&format!("{GR1}{UNUSED}"), "", &shapes));
    assert!(part_of(&written, "content.xml").contains(r#"style:name="gr9""#));
    assert_same("a graphic and a paragraph style", &source(&format!("{GR1}{UNUSED}{UNUSED_PARAGRAPH}"), "", &shapes));
}

#[test]
fn a_style_with_no_name_comes_back_unnamed() {
    let shapes = rect("A", (2.0, 3.0, 4.0, 2.0), "");
    let written = assert_same(
        "an automatic style",
        &source(
            &format!(
                "{GR1}{}",
                r##"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##
            ),
            "",
            &shapes,
        ),
    );
    assert!(!part_of(&written, "content.xml").contains(r#"style:name="""#));
}

#[test]
fn a_bold_style_keeps_only_the_attribute_it_had() {
    let bold = r#"<style:style style:name="Bold" style:family="graphic"><style:text-properties fo:font-weight="bold"/></style:style>"#;
    let auto = r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Bold"/>"#;
    let written = assert_same("bold", &source(auto, bold, &rect("A", (2.0, 3.0, 4.0, 2.0), "")));
    let styles = part_of(&written, "styles.xml");
    assert!(styles.contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "{styles}");
    assert!(!styles.contains("font-variant") && !styles.contains("-complex"), "{styles}");
}

#[test]
fn two_common_styles_of_one_name_come_back() {
    let two = r##"<style:style style:name="Dup" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style><style:style style:name="Dup" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let auto = r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Dup"/>"#;
    let written = assert_same("two common styles of a name", &source(auto, two, &rect("A", (2.0, 3.0, 4.0, 2.0), "")));
    assert_eq!(part_of(&written, "styles.xml").matches(r#"style:name="Dup""#).count(), 2);
}

/// A drawing with many styles nothing names visits through `.diacraft` and comes back as it was: the check after a save, which compares the kept styles of the two drawings, is charged
/// before it walks them and ends in a save, never in a hang.
#[test]
fn three_thousand_styles_nothing_names_come_back() {
    let styles: String = (0..3_000)
        .map(|k| format!(r##"<style:style style:name="u{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##))
        .collect();
    let written = assert_same("many styles", &source(&format!("{GR1}{styles}"), "", &rect("A", (2.0, 3.0, 4.0, 2.0), "")));
    assert_eq!(part_of(&written, "content.xml").matches(r#"style:name="u"#).count(), 3_000);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------: the forms of an earlier version

/// [`source`] as the version `version` of ODF says it is.
fn source_in(version: &str, auto: &str, common: &str, shapes: &str) -> Vec<u8> {
    let mut drawing = Drawing::new().page(shapes);
    drawing.auto = format!("{}{auto}", crate::odg_build::P1);
    drawing.common = common.to_owned();
    let label = format!("office:version=\"{version}\"");
    let (content, styles) =
        (drawing.content_xml().replace("office:version=\"1.3\"", &label), drawing.styles_xml().replace("office:version=\"1.3\"", &label));
    let files = vec![
        ("content.xml".to_owned(), content.into_bytes()),
        ("styles.xml".to_owned(), styles.into_bytes()),
        ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
    ];
    crate::odg_build::package(crate::odg_build::DRAWING, files)
}

/// Every part of the package `odg` against the ODF 1.4 schema when `ODF_SCHEMA_RNG` names it (the OASIS RelaxNG file), DiaCraft's markup taken out: skipped where it is not set.
fn assert_schema_valid(odg: &[u8], dir: &Dir, what: &str) {
    fn strip(el: &mut diacraft_container::xml::El) {
        let ours = |name: &str| name.starts_with("diacraft:") || name.starts_with("xmlns:diacraft");
        el.attrs.retain(|(k, _)| !ours(k));
        el.children.retain(|n| !matches!(n, diacraft_container::xml::Node::El(e) if ours(&e.name)));
        for n in &mut el.children {
            if let diacraft_container::xml::Node::El(e) = n {
                strip(e);
            }
        }
    }
    let Ok(rng) = std::env::var("ODF_SCHEMA_RNG") else { return };
    for part in ["content.xml", "styles.xml", "meta.xml"] {
        let mut root = diacraft_container::xml::parse(part_of(odg, part).as_bytes()).unwrap();
        strip(&mut root);
        let file = dir.path(&format!("schema-{part}"));
        std::fs::write(&file, diacraft_container::xml::to_bytes(&root, b"", b"\n", true).unwrap()).unwrap();
        let run = std::process::Command::new("xmllint").arg("--noout").arg("--relaxng").arg(&rng).arg(&file).output().unwrap();
        assert!(run.status.success(), "{what}: {part} is not valid ODF 1.4: {}", String::from_utf8_lossy(&run.stderr));
    }
}

/// **The forms of an earlier version of ODF, through the engine**: a drawing of ODF 1.0 whose common style says `fo:font-weight="250"` and whose automatic paragraph style nothing names says
/// `text:enable-numbering="false"` is opened, saved as `.diacraft`, opened again and saved as `.odg`. The file written is valid ODF 1.4 (with `ODF_SCHEMA_RNG` set), the weight is the one ODF 1.4 lists
/// nearest and the attribute is carried in DiaCraft's own, the check after the save names no class and says the values were written in the form the format takes (`as_values`), and a read of the file
/// gives the weight and the attribute back. A visit of what came back is the same bytes (clock pinned).
#[test]
fn the_forms_of_an_earlier_version_are_written_valid_and_come_back_through_the_engine() {
    let light = r#"<style:style style:name="Light" style:family="graphic"><style:text-properties fo:font-weight="250"/></style:style>"#;
    let numbered = r#"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties fo:text-align="center" text:enable-numbering="false"/></style:style>"#;
    let auto = format!(r#"{GR1}{numbered}<style:style style:name="gr3" style:family="graphic" style:parent-style-name="Light"/>"#);
    let source = source_in("1.0", &auto, light, &rect("A", (2.0, 3.0, 4.0, 2.0), ""));
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", &source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    let saved = run(&mut t, "file.save_as", json!({ "path": dir.path("w.odg") }));
    let written = dir.read("w.odg");
    let (styles, content) = (part_of(&written, "styles.xml"), part_of(&written, "content.xml"));
    assert!(
        styles.contains(r#"fo:font-weight="200""#) && !styles.contains(r#"fo:font-weight="250""#),
        "the weight 1.4 lists nearest to 250: {styles}"
    );
    assert!(styles.contains(r#"diacraft:older-font-weight="250""#), "the weight rides in DiaCraft's own attribute: {styles}");
    assert!(!content.contains("text:enable-numbering"), "{content}");
    assert!(content.contains(r#"diacraft:older-enable-numbering="false""#), "{content}");
    assert_schema_valid(&written, &dir, "a visit of a drawing of ODF 1.0");
    assert!(saved["checked"]["classes"].as_array().is_some_and(Vec::is_empty), "the check names no class: {saved}");
    assert_eq!(saved["checked"]["as_values"], json!(true), "the card says the values were written in the form the format takes: {saved}");
    // A DiaCraft read gives both back.
    let (doc, _) = diacraft_odg::read(&written, &diacraft_odg::Limits::default()).unwrap();
    let light = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Light")).cloned().expect("the style sheet");
    assert_eq!(light.ext.get().unwrap().odf.unmapped.get("style:text-properties/fo:font-weight"), Some("250"));
    let kept = doc.ext.get().unwrap().odf.styles.iter().find(|k| &*k.name == "P9").cloned().expect("the style nothing names");
    assert!(format!("{:?}", kept.element).contains(r#"("text:enable-numbering", "false")"#), "{kept:?}");
    // A visit of what came back is the same bytes.
    let (again, _) = visit(&Dir::new(), &written);
    assert_eq!(again, written, "a visit of what came back is the same bytes (clock pinned)");
}
