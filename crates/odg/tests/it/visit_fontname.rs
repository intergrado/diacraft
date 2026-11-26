//! GATE1622-CROSS-FONTNAME at the crate level (regression of the held paragraph values): the face a paragraph's style names by `style:font-name` is written as a face the part declares,
//! whatever came of the model's table of fonts in between. The paragraph's text properties are held as the file stated them while the cells they rest on are as they were, and the name in them is the
//! declaration's own (`Mono1`), which the table must still hold: a table that lost the declaration (a file another program rewrote, a text brought from another drawing) wrote the name undeclared, and
//! LibreOffice drew the text in its fallback face. Now the name is the one of the declared face of **the family the paragraph's own text is in**, as the cells say it, and a name that is declared, or
//! that no cell of the paragraph can stand for, is written as the file had it.
//!
//! The drawing is built in code from the ODF 1.4 Part 3 vocabulary (`style:font-face` §16.23, the text properties §16.29); the model is read from it, goes through the `.diacraft` codec as a visit
//! does, and has faces taken out of its table before it is written.

use std::sync::Arc;

use diacraft_doc::FaceName;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{part, source_of, through_codec};

/// Four faces: two families, the second declared three times under three names.
const FACES: &str = concat!(
    r#"<style:font-face style:name="DV" svg:font-family="'DejaVu Sans'" style:font-family-generic="swiss" style:font-pitch="variable"/>"#,
    r#"<style:font-face style:name="Mono" svg:font-family="'Liberation Mono'" style:font-family-generic="modern" style:font-pitch="fixed"/>"#,
    r#"<style:font-face style:name="Mono1" svg:font-family="'Liberation Mono'" style:font-family-generic="swiss" style:font-pitch="fixed"/>"#,
    r#"<style:font-face style:name="Mono2" svg:font-family="'Liberation Mono'" style:font-family-generic="modern"/>"#
);

/// The paragraph style the text names: a face for each script and a size.
const P1: &str = concat!(
    r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties style:font-name="Mono1" style:font-name-asian="Mono2" "#,
    r#"style:font-name-complex="DV" fo:font-size="18pt"/></style:style>"#
);

const FRAME: &str = r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="12cm" svg:height="3cm"><draw:text-box><text:p text:style-name="P1">Hello</text:p></draw:text-box></draw:frame>"#;

/// The drawing of the vector: both parts declare the four faces.
fn drawing() -> Drawing {
    let mut d = Drawing::a4().auto(P1).page(FRAME);
    d.fonts = FACES.to_owned();
    d
}

/// The file the model of `source` is written as when the faces of its table that `keep` says no to are gone (a model read, taken through the codec as a visit does, and written from the model alone).
fn written_without(source: &[u8], keep: impl Fn(&FaceName) -> bool) -> Vec<u8> {
    let (doc, _) = read(source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    Arc::make_mut(&mut doc.fonts).retain(keep);
    write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap().bytes
}

/// The declared names of `part`, with the family each stands for.
fn declared(part: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for tag in part.split("<style:font-face ").skip(1) {
        let tag = &tag[..tag.find('>').unwrap()];
        let value = |name: &str| {
            let key = format!("{name}=\"");
            tag.find(&key)
                .map(|at| &tag[at + key.len()..])
                .map(|rest| rest[..rest.find('"').unwrap()].replace("&apos;", "'").trim_matches('\'').to_owned())
        };
        out.push((value("style:name").unwrap_or_default(), value("svg:font-family").unwrap_or_default()));
    }
    out
}

/// What `style:font-name`, `-asian` and `-complex` of the style `P1` of `content` name, in that order (`-` for a script it names no face for).
fn p1_names(content: &str) -> [String; 3] {
    let at = content.find(r#"style:name="P1""#).unwrap_or_else(|| panic!("no P1: {content}"));
    let style = &content[at..at + content[at..].find("</style:style>").unwrap()];
    let name = |attr: &str| {
        let key = format!(" {attr}=\"");
        style.find(&key).map(|i| &style[i + key.len()..]).map_or_else(|| "-".to_owned(), |rest| rest[..rest.find('"').unwrap()].to_owned())
    };
    [name("style:font-name"), name("style:font-name-asian"), name("style:font-name-complex")]
}

/// The family the face `name` of `content` stands for.
fn family(content: &str, name: &str) -> Option<String> {
    declared(content).into_iter().find(|(n, _)| n == name).map(|(_, f)| f)
}

/// A name the table still holds is written as the file named it: the control of the class.
#[test]
fn a_face_the_table_holds_is_named_as_the_file_named_it() {
    let source = source_of(&drawing());
    let written = written_without(&source, |_| true);
    let content = part(&written, "content.xml");
    assert_eq!(p1_names(&content), ["Mono1", "Mono2", "DV"], "{content}");
}

/// A face of the table that is gone while others of its family stay: the paragraph's style names the declared face of the family its text is in, for each script, and keeps its size.
#[test]
fn a_name_the_table_lost_is_the_declared_face_of_the_family_of_the_text() {
    let source = source_of(&drawing());
    // `Mono1` and `Mono2` are gone, `Mono` and `DV` stay.
    let written = written_without(&source, |f| f.ext.get().and_then(|d| d.odf.raw.get("style:name")).is_none_or(|n| n == "Mono" || n == "DV"));
    let content = part(&written, "content.xml");
    let names = p1_names(&content);
    assert_eq!(names, ["Mono", "Mono", "DV"], "red on 1622f7c2: [\"Mono1\", \"Mono2\", \"DV\"], two names no declaration carries\n{content}");
    for name in &names {
        assert!(family(&content, name).is_some(), "{name} is declared: {content}");
    }
    assert_eq!(family(&content, &names[0]).as_deref(), Some("Liberation Mono"));
    assert!(content.contains(r#"fo:font-size="18pt""#), "the size the style held is written: {content}");
}

/// The whole family gone: the face is declared again by the family, which the cells name, and the style names it.
#[test]
fn a_family_the_table_lost_whole_is_declared_again_by_its_name() {
    let source = source_of(&drawing());
    let written = written_without(&source, |f| &*f.name_u != "Liberation Mono");
    let content = part(&written, "content.xml");
    let names = p1_names(&content);
    for name in &names {
        assert!(family(&content, name).is_some(), "{name} is declared: {content}");
    }
    assert_eq!(family(&content, &names[0]).as_deref(), Some("Liberation Mono"), "{content}");
    assert_eq!(family(&content, &names[1]).as_deref(), Some("Liberation Mono"), "{content}");
    assert_eq!(family(&content, &names[2]).as_deref(), Some("DejaVu Sans"), "{content}");
}

/// A name that no declaration of the file carries, and that no cell of the text stands for, is written as the file had it: the file's own, and no face is made up for it.
#[test]
fn a_name_the_file_never_declared_is_written_as_the_file_had_it() {
    let mut d = drawing();
    d.auto = d.auto.replace(r#"style:font-name="Mono1""#, r#"style:font-name="Ghost""#);
    let source = source_of(&d);
    let written = written_without(&source, |_| true);
    let content = part(&written, "content.xml");
    assert_eq!(p1_names(&content)[0], "Ghost", "{content}");
}
