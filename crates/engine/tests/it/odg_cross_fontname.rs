//! GATE1622-CROSS-FONTNAME (regression of the held paragraph and text values): a drawing that crosses `.odg`, `.diacraft`, VSDX, `.diacraft` and `.odg` again (and every other
//! way to an `.odg`) names, by `style:font-name`, `style:font-name-asian` and `style:font-name-complex`, only faces that the part it is written in declares. A held name that no
//! declaration of the written package carries makes LibreOffice draw the text in its fallback face instead of the one the source used.
//!
//! The files are made by code, by hand from the OASIS ODF standard (Part 3, `style:font-face` §16.23 and the text properties §16.29): four faces of two families, a graphic default style that names
//! one, and a paragraph style that names another of the second family, as the gate's vector (rebuilt from its note) describes it. The faces are named as ODF 1.2 and later name them, by
//! `style:name`, and three of them share a family, which is the name a VSDX font table finds a face by.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{DRAWING, Drawing, GR1, GR2, package, part_of};
use crate::run;

/// The four faces: two families, two generics and a pitch or none. `Mono`, `Mono1` and `Mono2` are the same family.
const FACES: &str = concat!(
    r#"<style:font-face style:name="DV" svg:font-family="'DejaVu Sans'" style:font-family-generic="swiss" style:font-pitch="variable"/>"#,
    r#"<style:font-face style:name="Mono" svg:font-family="'Liberation Mono'" style:font-family-generic="modern" style:font-pitch="fixed"/>"#,
    r#"<style:font-face style:name="Mono1" svg:font-family="'Liberation Mono'" style:font-family-generic="swiss" style:font-pitch="fixed"/>"#,
    r#"<style:font-face style:name="Mono2" svg:font-family="'Liberation Mono'" style:font-family-generic="modern"/>"#
);

/// The graphic default style of the vector, which names `DV`.
const DEFAULT_DV: &str = r#"<style:default-style style:family="graphic"><style:text-properties style:font-name="DV"/></style:default-style>"#;

/// A graphic default style that names the faces of the second family, one for each script: the class the gate found on every build, a default style whose face the crossing left undeclared.
const DEFAULT_MONO: &str = concat!(
    r#"<style:default-style style:family="graphic"><style:text-properties style:font-name="Mono1" style:font-name-asian="Mono2" "#,
    r#"style:font-name-complex="Mono"/></style:default-style>"#
);

/// The paragraph style the text names: a face of the second family and a size.
const P1_MONO1: &str =
    r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties style:font-name="Mono1" fo:font-size="18pt"/></style:style>"#;

/// A paragraph style that names a face for each script, each of the second family but `DV`.
const P1_SCRIPTS: &str = concat!(
    r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties style:font-name="Mono2" style:font-name-asian="Mono1" "#,
    r#"style:font-name-complex="DV" fo:font-size="18pt"/></style:style>"#
);

/// A run's style (`T1`), which names `Mono1` for the Western script.
const T1_MONO1: &str =
    r#"<style:style style:name="T1" style:family="text"><style:text-properties style:font-name="Mono1" fo:font-weight="bold"/></style:style>"#;

/// A rectangle of 12 cm by 3 cm whose paragraph names `P1`.
const RECT: &str = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="3cm"><text:p text:style-name="P1">Hello</text:p></draw:rect>"#;

/// A rectangle whose paragraph names `P1` and has a span that names `T1`.
const RECT_SPAN: &str = concat!(
    r#"<draw:rect draw:style-name="gr1" draw:layer="layout" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="3cm">"#,
    r#"<text:p text:style-name="P1">Hello <text:span text:style-name="T1">bold</text:span></text:p></draw:rect>"#
);

/// The drawing of the vector: the common styles `common` and the automatic styles `auto` after the two graphic ones, `shapes` on its page.
fn drawing(common: &str, auto: &str, shapes: &str) -> Drawing {
    let mut d = Drawing::new().page(shapes).common(common);
    d.auto = format!("{GR1}{GR2}{auto}");
    d
}

/// `text` as ODF 1.2, with the faces declared in each `office:font-face-decls` of it.
fn as_odf_1_2(text: String) -> String {
    text.replace("office:version=\"1.3\"", "office:version=\"1.2\"")
        .replace("<office:font-face-decls/>", &format!("<office:font-face-decls>{FACES}</office:font-face-decls>"))
}

/// The package of the vector (ODF 1.2), whose two parts declare the faces.
fn package_of(common: &str, auto: &str, shapes: &str) -> Vec<u8> {
    let d = drawing(common, auto, shapes);
    let files = vec![
        ("content.xml".to_owned(), as_odf_1_2(d.content_xml()).into_bytes()),
        ("styles.xml".to_owned(), as_odf_1_2(d.styles_xml()).into_bytes()),
        ("meta.xml".to_owned(), d.meta_xml().replace("office:version=\"1.3\"", "office:version=\"1.2\"").into_bytes()),
    ];
    package(DRAWING, files)
}

/// The flat document of the vector (ODF 1.2): its one `office:font-face-decls` holds the faces.
fn flat_of(common: &str, auto: &str, shapes: &str) -> Vec<u8> {
    as_odf_1_2(drawing(common, auto, shapes).flat_xml(DRAWING)).into_bytes()
}

/// The vector the gate gave: the four faces, the default style naming `DV`, and a rectangle of 12 cm by 3 cm whose paragraph names `P1`, which names `Mono1` at 18 pt.
fn vector() -> Vec<u8> {
    package_of(DEFAULT_DV, P1_MONO1, RECT)
}

/// The same with a default style and a paragraph style that name faces for every script, and a span that names one.
fn scripts() -> Vec<u8> {
    package_of(DEFAULT_MONO, &format!("{P1_SCRIPTS}{T1_MONO1}"), RECT_SPAN)
}

/// The same as a flat drawing.
fn scripts_flat() -> Vec<u8> {
    flat_of(DEFAULT_MONO, &format!("{P1_SCRIPTS}{T1_MONO1}"), RECT_SPAN)
}

/// The legs of a crossing, each a session of its own: `source` as `name`, then `a.diacraft`, `b.vsdx`, `c.diacraft` and `d.odg`. Gives `d.odg`.
fn cross(dir: &Dir, name: &str, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = opened(&dir.path("a.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let mut u = opened(&dir.path("b.vsdx"));
    run(&mut u, "file.save_as", json!({ "path": dir.path("c.diacraft") }));
    let mut v = opened(&dir.path("c.diacraft"));
    run(&mut v, "file.save_as", json!({ "path": dir.path("d.odg") }));
    dir.read("d.odg")
}

/// The plain visit: `source` as `name`, `a.diacraft`, `b.odg`. Gives `b.odg`.
fn visit(dir: &Dir, name: &str, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = opened(&dir.path("a.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.odg") }));
    dir.read("b.odg")
}

/// The values of the attribute `name` wherever `text` gives it, in order.
fn attributes_of<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let key = format!(" {name}=\"");
    let mut from = 0;
    while let Some(at) = text[from..].find(&key).map(|a| a + from) {
        let start = at + key.len();
        let end = start + text[start..].find('"').unwrap();
        out.push(&text[start..end]);
        from = end;
    }
    out
}

/// The `style:name`s and families of the `style:font-face`s of a part, the quotes taken off the family.
fn declared(part: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = part[from..].find("<style:font-face ").map(|a| a + from) {
        let end = at + part[at..].find('>').unwrap();
        let tag = &part[at..end];
        let name = attributes_of(tag, "style:name").first().map(|s| (*s).to_owned()).unwrap_or_default();
        let family =
            attributes_of(tag, "svg:font-family").first().map_or_else(String::new, |s| s.replace("&apos;", "'").trim_matches('\'').to_owned());
        out.push((name, family));
        from = end;
    }
    out
}

/// The three attributes that name a face.
const NAMES: [&str; 3] = ["style:font-name", "style:font-name-asian", "style:font-name-complex"];

/// The faces the part's styles name by `style:font-name`, `-asian` and `-complex` that none of the part's `style:font-face` declares.
fn dangling(part: &str) -> Vec<String> {
    let faces: Vec<String> = declared(part).into_iter().map(|(n, _)| n).collect();
    let mut out = Vec::new();
    for attribute in NAMES {
        for name in attributes_of(part, attribute) {
            if !faces.iter().any(|f| f == name) {
                out.push(format!("{attribute}=\"{name}\""));
            }
        }
    }
    out
}

/// Every name of a face in either part of `file` is declared by the part that names it. Gives the two parts.
#[track_caller]
fn assert_no_dangling_name(what: &str, file: &[u8]) -> (String, String) {
    let (content, styles) = (part_of(file, "content.xml"), part_of(file, "styles.xml"));
    assert_eq!(dangling(&content), Vec::<String>::new(), "{what}: content.xml names faces it does not declare: {content}");
    assert_eq!(dangling(&styles), Vec::<String>::new(), "{what}: styles.xml names faces it does not declare: {styles}");
    (content, styles)
}

/// The families the faces named by `attribute` in `part` stand for, in order, as the part's own declarations say them (`None` for a name it declares no face by).
fn families(part: &str, attribute: &str) -> Vec<Option<String>> {
    let faces = declared(part);
    attributes_of(part, attribute).into_iter().map(|n| faces.iter().find(|(name, _)| name == n).map(|(_, f)| f.clone())).collect()
}

/// Every family a part's styles name for a script in `source` is named for that script in `written`: the text is drawn in the faces it was drawn in, by whatever name the file now gives them.
#[track_caller]
fn assert_same_families(what: &str, source: &str, written: &str) {
    for attribute in NAMES {
        let named = families(written, attribute);
        for family in families(source, attribute) {
            assert!(named.contains(&family), "{what}: {attribute} names {family:?} in the source and {named:?} in the file written:\n{written}");
        }
    }
}

/// **The plain crossing keeps the face of the text**: the vector goes `.odg`, `.diacraft`, VSDX, `.diacraft`, `.odg`, and every `style:font-name` of the file that comes back names a
/// face its own part declares, so the text of the rectangle is drawn in Liberation Mono as the source's is. Red on main (1622f7c2): `d.odg` declares only `DV` and `Mono2`, and `P1` still
/// names `Mono1`, so LibreOffice falls back to Liberation Sans.
#[test]
fn the_crossing_through_vsdx_names_only_faces_the_file_declares() {
    let dir = Dir::new();
    let source = vector();
    let d = cross(&dir, "v.odg", &source);
    let (content, styles) = assert_no_dangling_name("crossing", &d);
    assert_same_families("crossing, content.xml", &part_of(&source, "content.xml"), &content);
    assert_same_families("crossing, styles.xml", &part_of(&source, "styles.xml"), &styles);
    // The faces are the file's own, all of them: a crossing keeps the declarations as it keeps the text.
    for (part, text) in [("content.xml", &content), ("styles.xml", &styles)] {
        let names: Vec<String> = declared(text).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["DV", "Mono", "Mono1", "Mono2"], "{part} declares the four faces: {text}");
    }
}

/// **The class of an older build too**: a default style and a paragraph style that name a face for each script, one of the faces three of one family, and a span that names another: after the
/// crossing every name is declared, in `styles.xml` (the default style's) as in `content.xml`.
#[test]
fn a_default_style_and_a_paragraph_that_name_faces_of_one_family_keep_them_through_vsdx() {
    let dir = Dir::new();
    let source = scripts();
    let d = cross(&dir, "v.odg", &source);
    let (content, styles) = assert_no_dangling_name("crossing", &d);
    assert_same_families("scripts, content.xml", &part_of(&source, "content.xml"), &content);
    assert_same_families("scripts, styles.xml", &part_of(&source, "styles.xml"), &styles);
    assert!(styles.contains(r#"style:font-name-asian="Mono2""#), "the default style names the faces it named: {styles}");
}

/// A flat drawing crosses as a package does: `.fodg`, `.diacraft`, VSDX, `.diacraft`, `.odg`.
#[test]
fn a_flat_drawing_crossing_through_vsdx_names_only_faces_the_file_declares() {
    let dir = Dir::new();
    let source = scripts_flat();
    let d = cross(&dir, "v.fodg", &source);
    let (content, styles) = assert_no_dangling_name("flat crossing", &d);
    let text = String::from_utf8(source).unwrap();
    // The flat file's one tree says what both parts of the package written say.
    assert_same_families("flat", &text, &format!("{content}{styles}"));
}

/// The plain visit, of a package and of a flat drawing, and a visit of what came back, name only faces they declare, and a flat drawing saved into its own file does the same after an edit.
#[test]
fn the_plain_visit_names_only_faces_it_declares() {
    for (name, source) in [("v.odg", scripts()), ("v.fodg", scripts_flat())] {
        let dir = Dir::new();
        let b = visit(&dir, name, &source);
        assert_no_dangling_name(name, &b);
        let again = visit(&Dir::new(), "v.odg", &b);
        assert_no_dangling_name(&format!("{name}, a second visit"), &again);
    }
    let dir = Dir::new();
    let path = dir.write("w.fodg", &scripts_flat());
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": path }));
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut s, "file.save", json!({}));
    let text = String::from_utf8(dir.read("w.fodg")).unwrap();
    assert_eq!(dangling(&text), Vec::<String>::new(), "the flat file saved: {text}");
}

/// **An edited drawing**: the crossing's last file opened, a shape moved, and saved as an `.odg` (a new file, the model writer) and into the `.odg` opened (a splice).
#[test]
fn an_edited_drawing_after_the_crossing_names_only_faces_it_declares() {
    let dir = Dir::new();
    let source = scripts();
    cross(&dir, "v.odg", &source);
    let mut s = opened(&dir.path("c.diacraft"));
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("e.odg") }));
    let (content, styles) = assert_no_dangling_name("edited", &dir.read("e.odg"));
    assert_same_families("edited, content.xml", &part_of(&source, "content.xml"), &content);
    assert_same_families("edited, styles.xml", &part_of(&source, "styles.xml"), &styles);
    // The `.odg` that came back, opened, edited and saved into itself.
    let mut t = opened(&dir.path("d.odg"));
    run(&mut t, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut t, "file.save", json!({}));
    assert_no_dangling_name("edited and saved into itself", &dir.read("d.odg"));
}

/// **A VSDX file saved as an `.odg`**: the crossing's VSDX file opened and saved as an `.odg` at once, and a VSDX drawing whose cells name faces by family.
#[test]
fn a_vsdx_file_saved_as_an_odg_names_only_faces_it_declares() {
    let dir = Dir::new();
    let source = scripts();
    cross(&dir, "v.odg", &source);
    let mut s = opened(&dir.path("b.vsdx"));
    run(&mut s, "file.save_as", json!({ "path": dir.path("f.odg") }));
    let (content, styles) = assert_no_dangling_name("vsdx as odg", &dir.read("f.odg"));
    assert_same_families("vsdx as odg, content.xml", &part_of(&source, "content.xml"), &content);
    assert_same_families("vsdx as odg, styles.xml", &part_of(&source, "styles.xml"), &styles);
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("g.vsdx", &crate::across::vsdx_with_styles()) }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("g.odg") }));
    assert_no_dangling_name("plain vsdx", &dir.read("g.odg"));
}

/// The document part of a VSDX package.
const DOCUMENT_PART: &str = "visio/document.xml";

/// `bytes` as a package with its entry `name` replaced by `data` (the other entries as they were).
fn with_entry(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    use diacraft_container::zip::{Archive, ArchiveWriter, Method};
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    for (k, n) in names.iter().enumerate() {
        let inflated = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        w.add(n, if n == name { data } else { &inflated }, Method::Deflated).unwrap();
    }
    w.finish().unwrap()
}

/// The text of the entry `name` of a package.
fn entry_text(bytes: &[u8], name: &str) -> String {
    let mut archive = diacraft_container::zip::Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let at = archive.position(name).unwrap_or_else(|| panic!("no entry {name}"));
    String::from_utf8(archive.inflate(at, &mut diacraft_container::Work::default()).unwrap()).unwrap()
}

/// **A VSDX font table another program rewrote**: the file the crossing wrote, with its `FaceNames` cut to one face of the family the three declarations share (a table that holds a name once, as a
/// program that makes a name its key writes it). The declarations ride beside the table, and each is a face of the table again when it is read, so the names the text holds are still declared.
#[test]
fn a_font_table_cut_to_one_face_of_a_name_gets_the_declarations_back() {
    let dir = Dir::new();
    let source = scripts();
    cross(&dir, "v.odg", &source);
    let vsdx = dir.read("b.vsdx");
    let document = entry_text(&vsdx, DOCUMENT_PART);
    let face = r#"<FaceName NameU="Liberation Mono"/>"#;
    assert_eq!(document.matches(face).count(), 3, "the table holds the three declarations of the family: {document}");
    let cut = document.replacen(face, "", 2);
    assert_eq!(cut.matches(face).count(), 1);
    dir.write("b2.vsdx", &with_entry(&vsdx, DOCUMENT_PART, cut.as_bytes()));
    let mut s = opened(&dir.path("b2.vsdx"));
    run(&mut s, "file.save_as", json!({ "path": dir.path("c2.diacraft") }));
    let mut t = opened(&dir.path("c2.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("d2.odg") }));
    let (content, styles) = assert_no_dangling_name("a table cut to one face", &dir.read("d2.odg"));
    assert_same_families("cut table, content.xml", &part_of(&source, "content.xml"), &content);
    assert_same_families("cut table, styles.xml", &part_of(&source, "styles.xml"), &styles);
    let names: Vec<String> = declared(&content).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("Mono")).count(), 3, "the three declarations of the family are written: {names:?}");
}
