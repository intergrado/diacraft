//! A regression found by the 0.2.4 gate: after an `.odg` has gone through `.diacraft` and come back, a span of an automatic text style that a second shape shares is written with the
//! style the file gave it, **with every property the file's style stated** (the sizes and weights of the Asian and complex scripts among them), and not with a new style made from the cells that lost
//! them and says the defaults. The crate-level visit and the cause are in the odg crate's `visit_run_style.rs`; here are the ways the engine takes a file through `.diacraft`.
//!
//! The vector is made by code from the ODF 1.4 Part 3 vocabulary (the default style §16.2, the text properties §16.29): an ODF 1.2 package whose graphic default style states the face of the complex
//! script, the text styles `T1` (a size and a weight in the three scripts) and `T2` (the same, and a language and a country), shape A with a span of `T1` and then one of `T2` in its paragraph and shape
//! B with a span of `T2` alone. Each way is tried as an `.odg` package and as a flat drawing, with the shapes as `draw:rect` and as `draw:line`.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{DRAWING, Drawing, package, part_of};
use crate::run;

/// The graphic default style: the face of the complex script, and nothing else.
const DEFAULT_COMPLEX_FACE: &str =
    r#"<style:default-style style:family="graphic"><style:text-properties style:font-family-complex="Skiff Naskh"/></style:default-style>"#;

/// What `T1` states: a size in the three scripts, and bold in the three.
const S1: [(&str, &str); 6] = [
    ("fo:font-size", "14pt"),
    ("style:font-size-asian", "14pt"),
    ("style:font-size-complex", "14pt"),
    ("fo:font-weight", "bold"),
    ("style:font-weight-asian", "bold"),
    ("style:font-weight-complex", "bold"),
];

/// What `T2` states beside it: a language and a country.
const LANGUAGE: [(&str, &str); 2] = [("fo:language", "en"), ("fo:country", "US")];

/// The attributes of a text style, as markup.
fn text_style(name: &str, attrs: &[(&str, &str)]) -> String {
    let attrs: String = attrs.iter().map(|(k, v)| format!(r#" {k}="{v}""#)).collect();
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties{attrs}/></style:style>"#)
}

fn span(style: &str, text: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{text}</text:span>"#)
}

/// A shape `kind` (`rect` or `line`) at `y` cm, with the paragraph `inner`.
fn shape(kind: &str, y: u32, inner: &str) -> String {
    let geometry = if kind == "line" {
        format!(r#"svg:x1="2cm" svg:y1="{y}cm" svg:x2="8cm" svg:y2="{y}cm""#)
    } else {
        format!(r#"svg:x="2cm" svg:y="{y}cm" svg:width="6cm" svg:height="2cm""#)
    };
    format!(r#"<draw:{kind} draw:layer="layout" {geometry}><text:p>{inner}</text:p></draw:{kind}>"#)
}

/// The vector's shapes of `kind`, the one with two spans first or second.
fn shapes(kind: &str, a_first: bool) -> String {
    let a = shape(kind, if a_first { 3 } else { 7 }, &format!("{}{}", span("T1", "Ferry "), span("T2", "dock")));
    let b = shape(kind, if a_first { 7 } else { 3 }, &span("T2", "pier"));
    if a_first { format!("{a}{b}") } else { format!("{b}{a}") }
}

fn drawing(shapes: &str) -> Drawing {
    let t2: Vec<(&str, &str)> = S1.iter().chain(LANGUAGE.iter()).copied().collect();
    let mut d = Drawing::new().page(shapes).common(DEFAULT_COMPLEX_FACE);
    d.auto = format!("{}{}", text_style("T1", &S1), text_style("T2", &t2));
    d
}

/// `text` as ODF 1.2.
fn as_odf_1_2(text: String) -> String {
    text.replace("office:version=\"1.3\"", "office:version=\"1.2\"")
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

/// The sources of the class, each with the name it is saved under: a package and a flat drawing, with the shapes as lines and as rectangles, and the shape with two spans first and second.
fn sources() -> Vec<(String, &'static str, Vec<u8>)> {
    let mut out = Vec::new();
    for kind in ["line", "rect"] {
        for a_first in [true, false] {
            let order = if a_first { "A then B" } else { "B then A" };
            out.push((format!("package, {kind}, {order}"), "v.odg", package_of(&shapes(kind, a_first))));
            out.push((format!("flat, {kind}, {order}"), "v.fodg", flat_of(&shapes(kind, a_first))));
        }
    }
    out
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

/// Save As `.diacraft` and then as `.odg` in one session. Gives `b.odg`.
fn save_as_both(dir: &Dir, name: &str, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    dir.read("b.odg")
}

/// The value of the attribute `name` in the tag `tag`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// Each span of `content` as (its text, the style it names), in the order of the file.
fn spans_of(content: &str) -> Vec<(String, Option<String>)> {
    content
        .split("<text:span")
        .skip(1)
        .filter(|rest| rest.starts_with(' '))
        .map(|rest| {
            let tag = &rest[..rest.find('>').unwrap()];
            let text = &rest[rest.find('>').unwrap() + 1..];
            (text[..text.find("</text:span>").unwrap()].to_owned(), attr(tag, "text:style-name"))
        })
        .collect()
}

/// The text properties tag of the automatic text style `name` of `content`, or what is wrong with it.
fn text_properties(content: &str, name: &str) -> Result<String, String> {
    let key = format!(r#"<style:style style:name="{name}" style:family="text""#);
    let at = content.find(&key).ok_or_else(|| format!("no text style {name}"))?;
    let style = &content[at..at + content[at..].find("</style:style>").ok_or("a style that does not end")?];
    let from = style.find("<style:text-properties").ok_or_else(|| format!("text style {name} has no text properties: {style}"))?;
    Ok(style[from..from + style[from..].find('>').unwrap()].to_owned())
}

/// What is wrong with the way `file` writes the spans, as one line each (none when nothing is): each of the two spans of `T2` ("dock" and "pier") names a style that states every property of `T2`,
/// and **the same style**.
fn problems(what: &str, file: &[u8]) -> Vec<String> {
    let content = part_of(file, "content.xml");
    let spans = spans_of(&content);
    let mut found = Vec::new();
    let mut names = Vec::new();
    for text in ["dock", "pier"] {
        let Some((_, Some(name))) = spans.iter().find(|(t, _)| t == text) else {
            found.push(format!("{what}: the span of {text} names no style: {spans:?}"));
            continue;
        };
        names.push(name.clone());
        let props = match text_properties(&content, name) {
            Ok(props) => props,
            Err(e) => {
                found.push(format!("{what}: the span of {text} (style {name}): {e}"));
                continue;
            }
        };
        for (k, v) in S1.iter().chain(LANGUAGE.iter()) {
            if attr(&props, k).as_deref() != Some(*v) {
                found.push(format!("{what}: the span of {text} (style {name}) has {k} = {:?}, not {v}: {props}", attr(&props, k)));
            }
        }
    }
    if names.len() == 2 && names[0] != names[1] {
        found.push(format!("{what}: the two spans of one style name two: {names:?}"));
    }
    found
}

/// **The plain visit** (open, Save As `.diacraft`, reopen, Save As `.odg`: the command line's `roundtrip --via diacraft`): red on 898db0ef, where the span of shape B is written with a new style that lost the
/// Asian and complex sizes and the Asian weight.
#[test]
fn the_plain_visit_gives_both_spans_every_property_of_their_style() {
    let found: Vec<String> = sources().into_iter().flat_map(|(what, name, source)| problems(&what, &visit(&Dir::new(), name, &source))).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// **Save As `.diacraft` and then as `.odg` in one session** writes the same file.
#[test]
fn saving_as_diacraft_and_then_as_odg_gives_both_spans_every_property_of_their_style() {
    let found: Vec<String> =
        sources().into_iter().flat_map(|(what, name, source)| problems(&what, &save_as_both(&Dir::new(), name, &source))).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// A second visit of what came back is the same text, and the two spans still share the one style the file gave them.
#[test]
fn a_second_visit_is_the_same_and_the_spans_share_one_style() {
    for (what, name, source) in sources() {
        let b = visit(&Dir::new(), name, &source);
        let again = visit(&Dir::new(), "v.odg", &b);
        assert_eq!(part_of(&again, "content.xml"), part_of(&b, "content.xml"), "{what}: a second visit");
    }
}

/// An edit after the visit is written and still names the styles the file gave: the control of the class.
#[test]
fn a_visited_drawing_that_is_edited_keeps_the_properties_of_both_spans() {
    let dir = Dir::new();
    let b = visit(&dir, "v.odg", &package_of(&shapes("rect", true)));
    let path = dir.write("c.odg", &b);
    let mut s = opened(&path);
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("e.odg") }));
    assert_eq!(problems("edited", &dir.read("e.odg")), Vec::<String>::new());
}
