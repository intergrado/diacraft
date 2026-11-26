//! GATEDE7D-HELD-PSTYLE, a regression of the guard for held paragraph face names: two paragraphs of one automatic paragraph style come back from a visit through `.diacraft` each
//! with a paragraph style that carries the text properties the file's style stated (`style:font-name`, `fo:font-size`), **whether the paragraph's text is in a span or bare**. The text properties
//! of a paragraph style are the style's, and a paragraph that names it is drawn with them: a paragraph whose style lost them is drawn in the default style's face and size.
//!
//! The drawing is built in code from the ODF 1.4 Part 3 vocabulary (`style:font-face` §16.23, the default style §16.2, the text properties §16.29). It is an ODF 1.2 package and a flat drawing, each with
//! the two rectangles in both orders: the paragraph with the span first and the bare one second, and the other way about (the first paragraph of a style is the one that makes it).

use std::sync::Arc;

use diacraft_doc::FaceName;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::visit_styles::{part, source_of, through_codec, visit};

/// Two faces of two families: `Ark` (sans) and `Cove` (serif).
const FACES: &str = concat!(
    r#"<style:font-face style:name="Ark" svg:font-family="'Ark Sans'" style:font-family-generic="swiss" style:font-pitch="variable"/>"#,
    r#"<style:font-face style:name="Cove" svg:font-family="'Cove Serif'" style:font-family-generic="roman" style:font-pitch="variable"/>"#
);

/// The graphic default style: `Cove` at 24 pt, what a text of a shape reads where nothing says otherwise.
const DEFAULT_COVE: &str =
    r#"<style:default-style style:family="graphic"><style:text-properties style:font-name="Cove" fo:font-size="24pt"/></style:default-style>"#;

/// The paragraph style the two paragraphs name: centred, in `Ark` at 10 pt.
const P3: &str = concat!(
    r#"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/>"#,
    r#"<style:text-properties style:font-name="Ark" fo:font-size="10pt"/></style:style>"#
);

/// The text style of the span: the same face and size.
const T2: &str =
    r#"<style:style style:name="T2" style:family="text"><style:text-properties style:font-name="Ark" fo:font-size="10pt"/></style:style>"#;

/// Rectangle A: a paragraph of `P3` whose whole text is in one span of `T2`.
const A: &str = concat!(
    r#"<draw:rect draw:layer="layout" svg:x="2cm" svg:y="3cm" svg:width="6cm" svg:height="2cm">"#,
    r#"<text:p text:style-name="P3"><text:span text:style-name="T2">Alpha beta gamma</text:span></text:p></draw:rect>"#
);

/// Rectangle B: a paragraph of `P3` with bare text.
const B: &str = concat!(
    r#"<draw:rect draw:layer="layout" svg:x="2cm" svg:y="7cm" svg:width="6cm" svg:height="2cm">"#,
    r#"<text:p text:style-name="P3">Delta epsilon zeta</text:p></draw:rect>"#
);

/// The vector (ODF 1.2): the faces declared in both parts, the default style, `P3`, `T2` and the two rectangles in the order `shapes` says.
fn vector(shapes: &str) -> Drawing {
    let mut d = Drawing::a4().common(DEFAULT_COVE).auto(P3).auto(T2).page(shapes);
    d.version = "1.2".to_owned();
    d.fonts = FACES.to_owned();
    d
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

/// What is wrong with the way `written` writes the paragraphs, as one line each (none when nothing is): both paragraphs are to be written with a style that names a face of the family `Ark Sans` that
/// the part declares, at 10 pt.
fn problems(what: &str, written: &[u8]) -> Vec<String> {
    let content = part(written, "content.xml");
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

/// The visit of a package, the span first and the bare paragraph second: **red on de7d53ce**, where the second paragraph names a style of its own (`P4`) that holds the paragraph properties alone.
#[test]
fn a_package_visit_gives_both_paragraphs_the_text_properties_of_their_style_span_first() {
    let (written, _) = visit(&vector(&format!("{A}{B}")).bytes()).unwrap();
    assert_eq!(problems("package, A then B", &written), Vec::<String>::new());
}

/// The other order: the bare paragraph makes the style, the one with the span is the second to name it.
#[test]
fn a_package_visit_gives_both_paragraphs_the_text_properties_of_their_style_bare_first() {
    let (written, _) = visit(&vector(&format!("{B}{A}")).bytes()).unwrap();
    assert_eq!(problems("package, B then A", &written), Vec::<String>::new());
}

/// A flat drawing is read as the package that holds the same elements, so its visit is the same.
#[test]
fn a_flat_visit_gives_both_paragraphs_the_text_properties_of_their_style_in_both_orders() {
    let mut found = Vec::new();
    for (order, shapes) in [("A then B", format!("{A}{B}")), ("B then A", format!("{B}{A}"))] {
        let (written, _) = visit(&Flat::of(&vector(&shapes)).bytes()).unwrap();
        found.extend(problems(&format!("flat, {order}"), &written));
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// One style and not two: the paragraphs share the style the file gave them, as they did, so the visit adds no style a drawing program would list.
#[test]
fn the_two_paragraphs_still_share_the_one_style_the_file_gave_them() {
    for (order, shapes) in [("A then B", format!("{A}{B}")), ("B then A", format!("{B}{A}"))] {
        let (written, _) = visit(&vector(&shapes).bytes()).unwrap();
        let content = part(&written, "content.xml");
        assert_eq!(paragraph_styles(&content), [Some("P3".to_owned()), Some("P3".to_owned())], "{order}: {content}");
        assert_eq!(content.matches(r#"style:family="paragraph""#).count(), 1, "{order}: one paragraph style: {content}");
    }
}

/// A second visit, of what the first wrote, is the same bytes.
#[test]
fn a_second_visit_of_the_shared_style_is_the_same_bytes() {
    for shapes in [format!("{A}{B}"), format!("{B}{A}")] {
        let (first, _) = visit(&vector(&shapes).bytes()).unwrap();
        let (second, _) = visit(&first).unwrap();
        assert_eq!(part(&first, "content.xml"), part(&second, "content.xml"));
    }
}

/// The file the model of `source` is written as when the faces of its table that `keep` says no to are gone (a model read, taken through the codec as a visit does, and written from the model alone).
fn written_without(source: &[u8], keep: impl Fn(&FaceName) -> bool) -> Vec<u8> {
    let (doc, _) = read(source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    Arc::make_mut(&mut doc.fonts).retain(keep);
    write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap().bytes
}

/// **A face the table lost** (the part declares no face of the name the style names, GATE1622-CROSS-FONTNAME): the paragraph whose text is bare is written with the declared face of the family its text
/// is in, and the paragraph whose text is all in a span, which has no text of its own on the style's row, still has the text properties of the style, in either order. A paragraph that asks second
/// and cannot take the style as it was made is given a style of its own that carries them, and not one that lost them.
#[test]
fn a_face_the_table_lost_leaves_both_paragraphs_the_text_properties_of_their_style() {
    for (order, shapes) in [("A then B", format!("{A}{B}")), ("B then A", format!("{B}{A}"))] {
        // The family `Ark Sans` is gone from the table, and `Cove Serif` stays.
        let source = source_of(&vector(&shapes));
        let written = written_without(&source, |f| &*f.name_u != "Ark Sans");
        let content = part(&written, "content.xml");
        let styles = paragraph_styles(&content);
        assert_eq!(styles.len(), 2, "{order}: {content}");
        for (k, name) in styles.iter().enumerate() {
            let name = name.as_deref().unwrap_or_else(|| panic!("{order}: paragraph {k} names no style: {content}"));
            let props = text_properties(&content, name).unwrap_or_else(|e| panic!("{order}: paragraph {k} (style {name}): {e}\n{content}"));
            assert_eq!(attr(&props, "fo:font-size").as_deref(), Some("10pt"), "{order}: paragraph {k} (style {name}): {props}");
        }
        // The paragraph of bare text (the second of the file for `A then B`, the first for the other) names a face the part declares, of the family its text is in.
        let bare = styles.get(usize::from(order == "A then B")).and_then(Option::as_deref).unwrap();
        let face = attr(&text_properties(&content, bare).unwrap(), "style:font-name").unwrap();
        assert_eq!(family_of(&content, &face).as_deref(), Some("Ark Sans"), "{order}: the bare paragraph's style {bare} names {face}: {content}");
    }
}
