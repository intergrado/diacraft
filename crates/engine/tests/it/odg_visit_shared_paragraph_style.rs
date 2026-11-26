//! GATEDE7D-HELD-PSTYLE, a regression of the guard for held paragraph face names: after an `.odg` has gone through `.diacraft` and come back, every paragraph of an automatic
//! paragraph style is written with a style that carries the text properties the file's style stated (`style:font-name`, `fo:font-size`), **whether the paragraph's text is in a span or bare**.
//! A paragraph that shares its style with another and lost them is drawn in the default style's face and size by every other reader.
//!
//! The vector is made by code from the ODF 1.4 Part 3 vocabulary (`style:font-face` §16.23, the default style §16.2, the text properties §16.29): an ODF 1.2 package whose two parts declare `Ark`
//! (`Ark Sans`) and `Cove` (`Cove Serif`), a graphic default style that names `Cove` at 24 pt, the paragraph style `P3` (`Ark` at 10 pt), the text style `T2` (the same), and two rectangles, `A`
//! with a paragraph of `P3` whose whole text is in a span of `T2` and `B` with a paragraph of `P3` and bare text. Each of the ways a file goes through `.diacraft` is tried with the rectangles in
//! both orders, as an `.odg` package and as a flat drawing.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{DRAWING, Drawing, package, part_of};
use crate::run;

/// Two faces of two families.
const FACES: &str = concat!(
    r#"<style:font-face style:name="Ark" svg:font-family="'Ark Sans'" style:font-family-generic="swiss" style:font-pitch="variable"/>"#,
    r#"<style:font-face style:name="Cove" svg:font-family="'Cove Serif'" style:font-family-generic="roman" style:font-pitch="variable"/>"#
);

/// The graphic default style: `Cove` at 24 pt.
const DEFAULT_COVE: &str =
    r#"<style:default-style style:family="graphic"><style:text-properties style:font-name="Cove" fo:font-size="24pt"/></style:default-style>"#;

/// The paragraph style of both paragraphs: centred, `Ark` at 10 pt.
const P3: &str = concat!(
    r#"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/>"#,
    r#"<style:text-properties style:font-name="Ark" fo:font-size="10pt"/></style:style>"#
);

/// The text style of the span.
const T2: &str =
    r#"<style:style style:name="T2" style:family="text"><style:text-properties style:font-name="Ark" fo:font-size="10pt"/></style:style>"#;

/// Rectangle A: the whole text of its paragraph is in one span.
const A: &str = concat!(
    r#"<draw:rect draw:layer="layout" svg:x="2cm" svg:y="3cm" svg:width="6cm" svg:height="2cm">"#,
    r#"<text:p text:style-name="P3"><text:span text:style-name="T2">Alpha beta gamma</text:span></text:p></draw:rect>"#
);

/// Rectangle B: bare text.
const B: &str = concat!(
    r#"<draw:rect draw:layer="layout" svg:x="2cm" svg:y="7cm" svg:width="6cm" svg:height="2cm">"#,
    r#"<text:p text:style-name="P3">Delta epsilon zeta</text:p></draw:rect>"#
);

/// The drawing of the vector, its rectangles in the order `shapes` says.
fn drawing(shapes: &str) -> Drawing {
    let mut d = Drawing::new().page(shapes).common(DEFAULT_COVE);
    d.auto = format!("{P3}{T2}");
    d
}

/// `text` as ODF 1.2, with the faces declared in each `office:font-face-decls` of it.
fn as_odf_1_2(text: String) -> String {
    text.replace("office:version=\"1.3\"", "office:version=\"1.2\"")
        .replace("<office:font-face-decls/>", &format!("<office:font-face-decls>{FACES}</office:font-face-decls>"))
}

/// The package of the vector (ODF 1.2).
fn package_of(shapes: &str) -> Vec<u8> {
    let d = drawing(shapes);
    let files = vec![
        ("content.xml".to_owned(), as_odf_1_2(d.content_xml()).into_bytes()),
        ("styles.xml".to_owned(), as_odf_1_2(d.styles_xml()).into_bytes()),
        ("meta.xml".to_owned(), d.meta_xml().replace("office:version=\"1.3\"", "office:version=\"1.2\"").into_bytes()),
    ];
    package(DRAWING, files)
}

/// The flat drawing of the vector (ODF 1.2).
fn flat_of(shapes: &str) -> Vec<u8> {
    as_odf_1_2(drawing(shapes).flat_xml(DRAWING)).into_bytes()
}

/// The sources of the class, each with the name it is saved under: a package and a flat drawing, each with the rectangles in both orders.
fn sources() -> Vec<(String, &'static str, Vec<u8>)> {
    let (ab, ba) = (format!("{A}{B}"), format!("{B}{A}"));
    vec![
        ("package, A then B".to_owned(), "v.odg", package_of(&ab)),
        ("package, B then A".to_owned(), "v.odg", package_of(&ba)),
        ("flat, A then B".to_owned(), "v.fodg", flat_of(&ab)),
        ("flat, B then A".to_owned(), "v.fodg", flat_of(&ba)),
    ]
}

/// The plain visit: `source` as `name`, saved as `a.diacraft`, which a session of its own opens and saves as `b.odg`. Gives `b.odg`.
fn visit(dir: &Dir, name: &str, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = opened(&dir.path("a.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.odg") }));
    dir.read("b.odg")
}

/// Save As `.diacraft` and then as `.odg` in one session: `source` as `name`, saved as `a.diacraft`, and the same session saves as `b.odg`. Gives `b.odg`.
fn save_as_both(dir: &Dir, name: &str, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    dir.read("b.odg")
}

/// The crossing of the gate: `source` as `name`, then `a.diacraft`, `b.vsdx`, `c.diacraft` and `d.odg`. Gives `d.odg`.
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

/// The value of the attribute `name` in the tag `tag`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].replace("&apos;", "'"))
}

/// The style each `text:p` of `content` names, in the order of the file.
fn paragraph_styles(content: &str) -> Vec<Option<String>> {
    content
        .split("<text:p")
        .skip(1)
        .filter(|rest| rest.starts_with([' ', '>', '/']))
        .map(|rest| attr(&rest[..rest.find('>').unwrap()], "text:style-name"))
        .collect()
}

/// The text properties tag of the automatic paragraph style `name` of `content`, or what is wrong with it.
fn text_properties(content: &str, name: &str) -> Result<String, String> {
    let key = format!(r#"<style:style style:name="{name}" style:family="paragraph""#);
    let at = content.find(&key).ok_or_else(|| format!("no paragraph style {name}"))?;
    let style = &content[at..at + content[at..].find("</style:style>").ok_or("a style that does not end")?];
    let from = style.find("<style:text-properties").ok_or_else(|| format!("paragraph style {name} has no text properties: {style}"))?;
    Ok(style[from..from + style[from..].find('>').unwrap()].to_owned())
}

/// The family of the face `content` declares under `name`.
fn family_of(content: &str, name: &str) -> Option<String> {
    content
        .split("<style:font-face")
        .skip(1)
        .filter(|rest| rest.starts_with(' '))
        .map(|rest| &rest[..rest.find('>').unwrap()])
        .find(|tag| attr(tag, "style:name").as_deref() == Some(name))
        .and_then(|tag| attr(tag, "svg:font-family"))
        .map(|f| f.trim_matches('\'').to_owned())
}

/// What is wrong with the way `file` writes the paragraphs, as one line each (none when nothing is): both paragraphs are to be written with a paragraph style whose text properties name a face of the
/// family `Ark Sans` that `content.xml` declares, at 10 pt, so that another reader draws the words of both in the face and size of the file's style.
fn problems(what: &str, file: &[u8]) -> Vec<String> {
    let content = part_of(file, "content.xml");
    let styles = paragraph_styles(&content);
    if styles.len() != 2 {
        return vec![format!("{what}: {} paragraphs: {content}", styles.len())];
    }
    let mut found = Vec::new();
    for (k, name) in styles.iter().enumerate() {
        let Some(name) = name.as_deref() else {
            found.push(format!("{what}: paragraph {k} names no style"));
            continue;
        };
        let props = match text_properties(&content, name) {
            Ok(props) => props,
            Err(e) => {
                found.push(format!("{what}: paragraph {k} (style {name}): {e}"));
                continue;
            }
        };
        match attr(&props, "style:font-name") {
            Some(face) if family_of(&content, &face).as_deref() == Some("Ark Sans") => {}
            other => {
                found.push(format!("{what}: paragraph {k} (style {name}) names the face {other:?}, which is no declared face of Ark Sans: {props}"))
            }
        }
        if attr(&props, "fo:font-size").as_deref() != Some("10pt") {
            found.push(format!("{what}: paragraph {k} (style {name}) is not at 10pt: {props}"));
        }
    }
    found
}

/// **The plain visit**: red on de7d53ce, where the paragraph of the rectangle that comes second is written with a style of its own (`P4`) that holds the paragraph properties alone.
#[test]
fn the_plain_visit_gives_both_paragraphs_the_text_properties_of_their_style() {
    let found: Vec<String> = sources().into_iter().flat_map(|(what, name, source)| problems(&what, &visit(&Dir::new(), name, &source))).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// **Save As `.diacraft` and then as `.odg` in one session** writes the same file.
#[test]
fn saving_as_diacraft_and_then_as_odg_gives_both_paragraphs_the_text_properties_of_their_style() {
    let found: Vec<String> =
        sources().into_iter().flat_map(|(what, name, source)| problems(&what, &save_as_both(&Dir::new(), name, &source))).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// **The crossing through VSDX** (GATE1622-CROSS-FONTNAME's way) keeps them too: the text properties of the style ride through the drawing of the other format.
#[test]
fn the_crossing_through_vsdx_gives_both_paragraphs_the_text_properties_of_their_style() {
    let found: Vec<String> = sources().into_iter().flat_map(|(what, name, source)| problems(&what, &cross(&Dir::new(), name, &source))).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// The paragraphs still share the one style the file gave them: the visit makes no style a drawing program would list twice, and a second visit of what came back is the same.
#[test]
fn the_paragraphs_still_share_the_one_style_and_a_second_visit_is_the_same() {
    for (what, name, source) in sources() {
        let b = visit(&Dir::new(), name, &source);
        let content = part_of(&b, "content.xml");
        assert_eq!(paragraph_styles(&content), [Some("P3".to_owned()), Some("P3".to_owned())], "{what}: {content}");
        let again = visit(&Dir::new(), "v.odg", &b);
        assert_eq!(part_of(&again, "content.xml"), content, "{what}: a second visit");
    }
}

/// An edit after the visit is written from the cells and still names the style whose text properties the file stated: the control of the class.
#[test]
fn a_visited_drawing_that_is_edited_keeps_the_text_properties_of_the_style() {
    let dir = Dir::new();
    let b = visit(&dir, "v.odg", &package_of(&format!("{A}{B}")));
    let path = dir.write("c.odg", &b);
    let mut s = opened(&path);
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("e.odg") }));
    assert_eq!(problems("edited", &dir.read("e.odg")), Vec::<String>::new());
}
