//! Found by the gate of 0.2.3, a side effect: after an `.odg` has gone through `.diacraft` and come back, a paragraph whose shape's graphic style already gives every
//! property the paragraph style it names states is written with that style or with none, **and never with a new automatic paragraph style whose properties element has no attribute**.
//!
//! The vector is made by code from the ODF 1.4 Part 3 vocabulary (the paragraph properties §16.30, the graphic styles §16.2, the connector §10.4.13): an ODF 1.2 package and its flat drawing, a
//! common graphic style `Wire` that centres its text, the automatic graphic styles `gr1` (nothing) and `gr2` (the child of `Wire`), the paragraph style `P2` (centred), the rectangles `n1`
//! (a paragraph of `P2`) and `n2`, and a connector on `gr2` from `n1` to `n2` whose paragraph names `P2` too. The same drawing with `end` for `Wire` and `P2` is the control that keeps the value the
//! file stated.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{DRAWING, Drawing, package, part_of};
use crate::run;

/// The automatic graphic styles: `gr1` states no paragraph property, `gr2` is the child of `Wire`.
const GRAPHIC: &str = concat!(
    r#"<style:style style:name="gr1" style:family="graphic"/>"#,
    r#"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="Wire"/>"#
);

/// Rectangle `n1`, with a paragraph of `P2`.
const N1: &str = concat!(
    r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="n1" xml:id="n1" draw:id="n1" svg:x="2cm" svg:y="3cm" svg:width="5cm" svg:height="2cm">"#,
    r#"<text:p text:style-name="P2">Berth one</text:p></draw:rect>"#
);

/// Rectangle `n2`, with no text.
const N2: &str = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="n2" xml:id="n2" draw:id="n2" svg:x="11cm" svg:y="3cm" svg:width="5cm" svg:height="2cm"/>"#;

/// The connector from `n1` to `n2`, with a paragraph of `P2`.
const TOW_LINE: &str = concat!(
    r#"<draw:connector draw:style-name="gr2" draw:layer="layout" draw:name="c" draw:start-shape="n1" draw:start-glue-point="3" draw:end-shape="n2" draw:end-glue-point="1" "#,
    r#"svg:x1="7cm" svg:y1="4cm" svg:x2="11cm" svg:y2="4cm" svg:d="M 7000 4000 L 11000 4000" svg:viewBox="0 0 11001 4001">"#,
    r#"<text:p text:style-name="P2">Tow line</text:p></draw:connector>"#
);

/// The drawing of the vector with `Wire` and `P2` aligned as `align` says, its shapes in the order `shapes` says.
fn drawing(align: &str, shapes: &str) -> Drawing {
    let mut d = Drawing::new().page(shapes);
    d.common =
        format!(r#"<style:style style:name="Wire" style:family="graphic"><style:paragraph-properties fo:text-align="{align}"/></style:style>"#);
    d.auto = format!(
        r#"{GRAPHIC}<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="{align}"/></style:style>"#
    );
    d
}

/// `text` as ODF 1.2.
fn as_odf_1_2(text: String) -> String {
    text.replace("office:version=\"1.3\"", "office:version=\"1.2\"")
}

/// The package of the vector (ODF 1.2).
fn package_of(align: &str, shapes: &str) -> Vec<u8> {
    let d = drawing(align, shapes);
    let files = vec![
        ("content.xml".to_owned(), as_odf_1_2(d.content_xml()).into_bytes()),
        ("styles.xml".to_owned(), as_odf_1_2(d.styles_xml()).into_bytes()),
        ("meta.xml".to_owned(), as_odf_1_2(d.meta_xml()).into_bytes()),
    ];
    package(DRAWING, files)
}

/// The flat drawing of the vector (ODF 1.2).
fn flat_of(align: &str, shapes: &str) -> Vec<u8> {
    as_odf_1_2(drawing(align, shapes).flat_xml(DRAWING)).into_bytes()
}

/// The sources of the class for the alignment `align`, each with the name it is saved under: a package and a flat drawing, with the connector first and last.
fn sources(align: &str) -> Vec<(String, &'static str, Vec<u8>)> {
    let (last, first) = (format!("{N1}{N2}{TOW_LINE}"), format!("{TOW_LINE}{N1}{N2}"));
    vec![
        ("package, connector last".to_owned(), "v.odg", package_of(align, &last)),
        ("package, connector first".to_owned(), "v.odg", package_of(align, &first)),
        ("flat, connector last".to_owned(), "v.fodg", flat_of(align, &last)),
        ("flat, connector first".to_owned(), "v.fodg", flat_of(align, &first)),
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

/// The value of the attribute `name` in the tag `tag`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
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

/// The automatic paragraph styles of `content` as `(name, element)`: the whole element of each, from its tag to its end.
fn automatic_paragraph_styles(content: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (at, _) in content.match_indices("<style:style ") {
        let rest = &content[at..];
        let tag = &rest[..rest.find('>').unwrap()];
        if attr(tag, "style:family").as_deref() != Some("paragraph") {
            continue;
        }
        let end = if tag.ends_with('/') { tag.len() + 1 } else { rest.find("</style:style>").unwrap() + "</style:style>".len() };
        found.push((attr(tag, "style:name").unwrap(), rest[..end].to_owned()));
    }
    found
}

/// The alignment the automatic paragraph style `name` of `content` states, or what is wrong with the style.
fn text_align(content: &str, name: &str) -> Result<String, String> {
    let styles = automatic_paragraph_styles(content);
    let (_, element) = styles.iter().find(|(n, _)| n == name).ok_or_else(|| format!("no automatic paragraph style {name}"))?;
    attr(element, "fo:text-align").ok_or_else(|| format!("paragraph style {name} states no alignment: {element}"))
}

/// The automatic paragraph styles of `content` that say nothing: none holds an attribute on any of its properties elements.
fn empty_styles(content: &str) -> Vec<String> {
    automatic_paragraph_styles(content)
        .into_iter()
        .filter(|(_, element)| {
            let says = |tag: &str| element.find(tag).is_some_and(|at| element[at + tag.len()..].starts_with(' '));
            !(says("<style:paragraph-properties") || says("<style:text-properties"))
        })
        .map(|(name, element)| format!("{name}: {element}"))
        .collect()
}

/// **The plain visit of the vector**, red on d2605d1c, where the connector's paragraph names a new style (`P3`) whose paragraph properties element has no attribute: no style says nothing, the
/// paragraph of `n1` names `P2`, the connector's names `P2` or none, and no style is added to the one the file holds.
#[test]
fn the_plain_visit_writes_no_paragraph_style_that_says_nothing() {
    let mut found: Vec<String> = Vec::new();
    for (what, name, source) in sources("center") {
        let content = part_of(&visit(&Dir::new(), name, &source), "content.xml");
        found.extend(empty_styles(&content).into_iter().map(|e| format!("{what}: {e}")));
        let styles = paragraph_styles(&content);
        let names: Vec<String> = automatic_paragraph_styles(&content).into_iter().map(|(n, _)| n).collect();
        if names != ["P2"] || styles.len() != 2 || styles.iter().any(|s| !matches!(s.as_deref(), None | Some("P2"))) {
            found.push(format!("{what}: the paragraphs name {styles:?} and the automatic paragraph styles are {names:?}: {content}"));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// **The control that keeps the value the file stated**: with `end` for `Wire` and `P2`, both paragraphs are written with a style that states `end`, and no style says nothing.
#[test]
fn the_plain_visit_keeps_the_alignment_the_file_stated() {
    let mut found: Vec<String> = Vec::new();
    for (what, name, source) in sources("end") {
        let content = part_of(&visit(&Dir::new(), name, &source), "content.xml");
        found.extend(empty_styles(&content).into_iter().map(|e| format!("{what}: {e}")));
        let styles = paragraph_styles(&content);
        if styles.len() != 2 {
            found.push(format!("{what}: {} paragraphs: {content}", styles.len()));
        }
        for (k, style) in styles.iter().enumerate() {
            match style.as_deref().map(|s| text_align(&content, s)) {
                Some(Ok(align)) if align == "end" => {}
                other => found.push(format!("{what}: paragraph {k} names {style:?}, which states {other:?} and not `end`: {content}")),
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// A second visit of what came back is the same bytes, in the part that holds the paragraphs.
#[test]
fn a_second_visit_is_the_same() {
    for (what, name, source) in sources("center") {
        let first = visit(&Dir::new(), name, &source);
        let again = visit(&Dir::new(), "v.odg", &first);
        assert_eq!(part_of(&again, "content.xml"), part_of(&first, "content.xml"), "{what}: a second visit");
    }
}

/// An edit after the visit is written from the cells, and still writes no style that says nothing.
#[test]
fn a_visited_drawing_that_is_edited_writes_no_style_that_says_nothing() {
    let dir = Dir::new();
    let b = visit(&dir, "v.odg", &package_of("center", &format!("{N1}{N2}{TOW_LINE}")));
    let path = dir.write("c.odg", &b);
    let mut s = opened(&path);
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("e.odg") }));
    let content = part_of(&dir.read("e.odg"), "content.xml");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
}
