//! Kept markup whose prefix the page does not bind, written without a word (concern 1). Each test is the plain visit a person can make (`file.open`, `file.save_as` to
//! `.diacraft`, then from it to the file's own format, one leg a session), on a drawing built in code from the OASIS ODF standard and XML Namespaces 1.0, **whose prefix is declared below the
//! root** (on the page, the shape, the paragraph, the span or the element itself), where the root of the part binds none of it. The markup must come back bound to the namespace it was read in,
//! whatever prefix the file writes it with, and the legs must name nothing: a drawing that cannot be written as it was says so in words, and never drops it without one.

use diacraft_container::xml::{El, Node};
use serde_json::Value;

use crate::native_common::Dir;
use crate::odg_build::Drawing;
use crate::save_check_misses::{leg, named, parse};

/// The namespace of the extension vocabulary of the vectors: no root binds it.
const ZZ: &str = "urn:example:keptns:zz";
/// A second one, which a shape rebinds a prefix the root binds to.
const OTHER: &str = "urn:example:keptns:other";
/// The namespace the root of the drawings of `odg_build` binds `ext` to.
const ROOT_EXT: &str = "urn:example:extension:of:somebody:else";
const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const XLINK: &str = "http://www.w3.org/1999/xlink";

/// What the two legs of a plain visit said, and the file it made.
struct Visit {
    first: Value,
    second: Value,
    written: Vec<u8>,
}

impl Visit {
    /// The `content.xml` of the file the visit made.
    fn content(&self) -> String {
        crate::odg_build::part_of(&self.written, "content.xml")
    }

    /// The elements of the file the visit made, read as XML Namespaces 1.0 reads them.
    fn found(&self) -> Vec<Found> {
        found_in(&parse(&self.content()))
    }

    /// What the legs said: the classes the check named and the words of the notes, of both.
    fn said(&self) -> (Vec<String>, String) {
        let mut classes = named(&self.first);
        classes.extend(named(&self.second));
        (classes, format!("{} | {}", words(&self.first), words(&self.second)))
    }

    /// That the visit named nothing: no class of the check, and no word of the save.
    fn assert_silent(&self, what: &str) {
        let (classes, words) = self.said();
        assert!(classes.is_empty(), "{what}: the check named {classes:?}: {}", self.content());
        assert_eq!(words.trim_matches(|c| c == ' ' || c == '|'), "", "{what}: the save said something");
    }
}

/// An element of a file with the namespaces its names are in.
#[derive(Debug)]
struct Found {
    ns: String,
    local: String,
    /// The attributes that are no declarations: the namespace (`""` for none; `?p` for a prefix nothing binds), the local name and the value.
    attrs: Vec<(String, String, String)>,
    text: String,
}

impl Found {
    fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, l, _)| n == ns && l == local).map(|(_, _, v)| v.as_str())
    }
}

/// The characters in an element and in everything under it.
fn all_text(el: &El) -> String {
    el.children
        .iter()
        .map(|node| match node {
            Node::Text(t) | Node::CData(t) => t.clone(),
            Node::El(child) => all_text(child),
            _ => String::new(),
        })
        .collect()
}

/// The elements under `root`, with their names resolved by the declarations in force where each stands.
fn found_in(root: &El) -> Vec<Found> {
    fn walk(el: &El, scope: &mut Vec<(String, String)>, out: &mut Vec<Found>) {
        let mark = scope.len();
        for (key, value) in &el.attrs {
            if key == "xmlns" {
                scope.push((String::new(), value.clone()));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                scope.push((prefix.to_owned(), value.clone()));
            }
        }
        let lookup = |scope: &[(String, String)], prefix: &str| scope.iter().rev().find(|(p, _)| p == prefix).map(|(_, uri)| uri.clone());
        let resolve = |scope: &[(String, String)], name: &str, is_element: bool| match name.split_once(':') {
            Some((prefix, local)) => (lookup(scope, prefix).unwrap_or_else(|| format!("?{prefix}")), local.to_owned()),
            None => (if is_element { lookup(scope, "").unwrap_or_default() } else { String::new() }, name.to_owned()),
        };
        let (ns, local) = resolve(scope, &el.name, true);
        let attrs = el
            .attrs
            .iter()
            .filter(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:"))
            .map(|(k, v)| {
                let (n, l) = resolve(scope, k, false);
                (n, l, v.clone())
            })
            .collect();
        out.push(Found { ns, local, attrs, text: all_text(el) });
        for child in el.elements() {
            walk(child, scope, out);
        }
        scope.truncate(mark);
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// The elements of `found` named `local` in the namespace `ns`.
fn named_in<'a>(found: &'a [Found], ns: &str, local: &str) -> Vec<&'a Found> {
    found.iter().filter(|f| f.ns == ns && f.local == local).collect()
}

/// The words of the save's warnings of one leg.
fn words(said: &Value) -> String {
    said["warnings"].as_array().map(|n| n.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ")).unwrap_or_default()
}

/// Visits the package `source`: a leg to `.diacraft`, a leg back to an `.odg`.
fn visit_bytes(source: &[u8]) -> Visit {
    let dir = Dir::new();
    dir.write("a.odg", source);
    let first = leg(&dir, "a.odg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    Visit { first, second, written: dir.read("c.odg") }
}

/// The drawing of the vectors: the standard's automatic styles and a text style `T1` of 11 pt, with `shapes` on the one page.
fn drawing_of(shapes: &str) -> Drawing {
    let mut drawing = Drawing::new();
    drawing.auto.push_str(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"/></style:style>"#);
    drawing.page(shapes)
}

/// Visits the package built from `shapes` on the one page (`own` on the page's start tag).
fn visit_on(own: &str, shapes: &str) -> Visit {
    let mut drawing = drawing_of(shapes);
    drawing.pages = drawing.pages.replacen("<draw:page ", &format!("<draw:page{own} "), 1);
    visit_bytes(&drawing.bytes())
}

/// Visits the package built from `shapes` on the one page.
fn visit(shapes: &str) -> Visit {
    visit_on("", shapes)
}

/// A rectangle holding `inner` (a paragraph), with `own` on its start tag.
fn rect(own: &str, inner: &str) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"{own}>{inner}</draw:rect>"#)
}

fn zz(own: &str) -> String {
    format!(r#" xmlns:zz="{ZZ}"{own}"#)
}

// ---------------------------------------------------------------------------------------------------------------- the text of a shape

/// **A link whose `xlink` prefix is declared below the root comes back** (the plain visit lost it and said nothing; only the check named `hyperlinks`). The prefix is `lk`, an alias of
/// `xlink` that the root does not bind, declared on the paragraph, or on the shape, or on the page, or on the link itself.
#[test]
fn a_link_whose_prefix_is_declared_below_the_root_comes_back() {
    let alias = format!(r#"xmlns:lk="{XLINK}""#);
    let link = r#"<text:a lk:type="simple" lk:href="https://example.org/notes">the notes</text:a>"#;
    let own_link = format!(r#"<text:a {alias} lk:type="simple" lk:href="https://example.org/notes">the notes</text:a>"#);
    let cases = [
        ("the paragraph", visit(&rect("", &format!("<text:p {alias}>See {link} now</text:p>")))),
        ("the shape", visit(&rect(&format!(" {alias}"), &format!("<text:p>See {link} now</text:p>")))),
        ("the page", visit_on(&format!(" {alias}"), &rect("", &format!("<text:p>See {link} now</text:p>")))),
        ("the link", visit(&rect("", &format!("<text:p>See {own_link} now</text:p>")))),
    ];
    for (what, v) in cases {
        let found = v.found();
        let links = named_in(&found, TEXT, "a");
        assert_eq!(links.len(), 1, "{what}: the link comes back: {}", v.content());
        assert_eq!(links[0].attr(XLINK, "href"), Some("https://example.org/notes"), "{what}: its address, in the namespace it was read in");
        assert_eq!(links[0].attr(XLINK, "type"), Some("simple"), "{what}");
        assert_eq!(links[0].text, "the notes", "{what}");
        v.assert_silent(what);
    }
}

/// **An element of another vocabulary inside a paragraph comes back** with its attribute and its words, whichever element above declares the prefix: the paragraph, the span, the shape or the
/// page (the plain visit lost it and said nothing; the check named `text-formatting`).
#[test]
fn an_element_of_another_vocabulary_in_a_paragraph_comes_back() {
    let note = r#"<zz:note zz:k="v">big</zz:note>"#;
    let cases = [
        ("the paragraph", visit(&rect("", &format!("<text:p{}>Hello {note} world</text:p>", zz(""))))),
        ("the shape", visit(&rect(&zz(""), &format!("<text:p>Hello {note} world</text:p>")))),
        ("the page", visit_on(&zz(""), &rect("", &format!("<text:p>Hello {note} world</text:p>")))),
        ("the span", visit(&rect("", &format!(r#"<text:p>Hello <text:span text:style-name="T1"{}>{note}</text:span> world</text:p>"#, zz(""))))),
    ];
    for (what, v) in cases {
        let found = v.found();
        let notes = named_in(&found, ZZ, "note");
        assert_eq!(notes.len(), 1, "{what}: the element comes back: {}", v.content());
        assert_eq!(notes[0].attr(ZZ, "k"), Some("v"), "{what}: its attribute, in the namespace it was read in");
        assert_eq!(notes[0].text, "big", "{what}: {}", v.content());
        v.assert_silent(what);
    }
}

/// **The attributes of a paragraph, a span, a list and a field, and a mark, keep a prefix a shape declares**: each is written with its attribute in the namespace it was read in.
#[test]
fn the_attributes_of_a_paragraph_span_list_field_and_mark_keep_a_prefix_a_shape_declares() {
    let cases: Vec<(&str, String, &str, &str)> = vec![
        ("a paragraph", r#"<text:p zz:k="1">Hello</text:p>"#.to_owned(), TEXT, "p"),
        ("a span", r#"<text:p>Hello <text:span text:style-name="T1" zz:k="1">big</text:span></text:p>"#.to_owned(), TEXT, "span"),
        ("a list", r#"<text:list zz:k="1"><text:list-item><text:p>Hello</text:p></text:list-item></text:list>"#.to_owned(), TEXT, "list"),
        ("a list item", r#"<text:list><text:list-item zz:k="1"><text:p>Hello</text:p></text:list-item></text:list>"#.to_owned(), TEXT, "list-item"),
        ("a field", r#"<text:p>Page <text:page-number zz:k="1">3</text:page-number></text:p>"#.to_owned(), TEXT, "page-number"),
        ("a mark", r#"<text:p>Hello <text:bookmark text:name="m1" zz:k="1"/>world</text:p>"#.to_owned(), TEXT, "bookmark"),
    ];
    for (what, inner, ns, local) in cases {
        let v = visit(&rect(&zz(""), &inner));
        let found = v.found();
        let wanted = named_in(&found, ns, local);
        assert!(
            wanted.iter().any(|f| f.attr(ZZ, "k") == Some("1")),
            "{what}: its attribute comes back in the namespace it was read in: {}",
            v.content()
        );
        v.assert_silent(what);
    }
}

/// **A prefix the root binds that an element below it binds to another namespace is that namespace** where the markup was read: the shape rebinds `ext` (the root's is somebody else's), and
/// the attribute of the shape, its child and the element in its paragraph all come back in the shape's.
#[test]
fn a_prefix_the_root_binds_that_a_shape_rebinds_is_the_shapes_namespace() {
    let own = format!(r#" xmlns:ext="{OTHER}" ext:flag="1""#);
    let inner = r#"<ext:extra ext:k="2"/><text:p>Hello <ext:note ext:k="3">big</ext:note> world</text:p>"#;
    let v = visit(&rect(&own, inner));
    let found = v.found();
    let shape = named_in(&found, "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "rect");
    assert_eq!(shape.len(), 1);
    assert_eq!(shape[0].attr(OTHER, "flag"), Some("1"), "the attribute is in the shape's namespace: {}", v.content());
    assert_eq!(named_in(&found, OTHER, "extra").len(), 1, "{}", v.content());
    assert_eq!(named_in(&found, OTHER, "note").len(), 1, "{}", v.content());
    assert!(named_in(&found, ROOT_EXT, "note").is_empty() && named_in(&found, ROOT_EXT, "extra").is_empty(), "none of it is in the root's namespace");
    v.assert_silent("a rebound prefix");
}

/// **A shape that declares what the root declares again adds no declaration**: the markup is bound already, and the plain visit writes it as it did.
#[test]
fn a_declaration_that_binds_what_the_root_binds_adds_nothing() {
    let own = format!(r#" xmlns:ext="{ROOT_EXT}" ext:flag="1""#);
    let inner = r#"<ext:extra ext:k="2"/><text:p>Hello <ext:note ext:k="3">big</ext:note> world</text:p>"#;
    let v = visit(&rect(&own, inner));
    let found = v.found();
    assert_eq!(named_in(&found, ROOT_EXT, "extra").len(), 1, "{}", v.content());
    assert_eq!(named_in(&found, ROOT_EXT, "note").len(), 1, "{}", v.content());
    let content = v.content();
    assert!(!content.contains("<ext:extra xmlns:ext"), "the child declares nothing the root does not: {content}");
    assert!(!content.contains("<ext:note xmlns:ext"), "nor does the element in the text: {content}");
    v.assert_silent("a declaration of what the root binds");
}

/// **A default namespace an element declares is kept with the element that uses it**: an element with no prefix in a paragraph whose start tag declares the default namespace.
#[test]
fn a_default_namespace_a_paragraph_declares_is_kept_with_the_element_that_uses_it() {
    let v = visit(&rect("", &format!(r#"<text:p xmlns="{ZZ}">Hello <note k="v">big</note> world</text:p>"#)));
    let found = v.found();
    let notes = named_in(&found, ZZ, "note");
    assert_eq!(notes.len(), 1, "{}", v.content());
    assert_eq!(notes[0].attr("", "k"), Some("v"), "an attribute with no prefix is in no namespace");
    v.assert_silent("a default namespace");
}

// ---------------------------------------------------------------------------------------------------------------- the markup of a shape

/// **An attribute and a child of a shape whose prefix an element above declares come back**: declared on the shape itself, or on the page (the save said "1 attribute" and "1 element" that it
/// did not write, and the check named `kept-data`).
#[test]
fn an_attribute_and_a_child_of_a_shape_whose_prefix_is_declared_below_the_root_come_back() {
    let on_shape = visit(&rect(&zz(r#" zz:flag="1""#), r#"<zz:extra zz:k="2"/><text:p>Hello</text:p>"#));
    let on_page = visit_on(&zz(""), &rect(r#" zz:flag="1""#, r#"<zz:extra zz:k="2"/><text:p>Hello</text:p>"#));
    for (what, v) in [("the shape", on_shape), ("the page", on_page)] {
        let found = v.found();
        let shape = named_in(&found, "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "rect");
        assert_eq!(shape.len(), 1, "{what}");
        assert_eq!(shape[0].attr(ZZ, "flag"), Some("1"), "{what}: the attribute is in the namespace it was read in: {}", v.content());
        let extra = named_in(&found, ZZ, "extra");
        assert_eq!(extra.len(), 1, "{what}: {}", v.content());
        assert_eq!(extra[0].attr(ZZ, "k"), Some("2"), "{what}");
        v.assert_silent(what);
    }
}

/// **Three visits in a row keep it all**, and write it the same each time: the declarations the first leg put on the elements are the ones the second reads.
#[test]
fn three_visits_in_a_row_keep_the_markup_and_the_bytes() {
    let source = drawing_of(&rect(&zz(r#" zz:flag="1""#), &format!(r#"<zz:extra zz:k="2"/><text:p xmlns:lk="{XLINK}">See <text:a lk:type="simple" lk:href="https://example.org/a">a</text:a> and <zz:note>b</zz:note></text:p>"#)))
        .bytes();
    let dir = Dir::new();
    dir.write("v0.odg", &source);
    let mut written: Vec<String> = Vec::new();
    for k in 0..3 {
        let first = leg(&dir, &format!("v{k}.odg"), &format!("d{k}.diacraft"));
        let second = leg(&dir, &format!("d{k}.diacraft"), &format!("v{}.odg", k + 1));
        assert!(named(&first).is_empty() && named(&second).is_empty(), "visit {k}: {first} / {second}");
        written.push(crate::odg_build::part_of(&dir.read(&format!("v{}.odg", k + 1)), "content.xml"));
    }
    let found = found_in(&parse(&written[2]));
    assert_eq!(named_in(&found, TEXT, "a").len(), 1, "{}", written[2]);
    assert_eq!(named_in(&found, ZZ, "note").len(), 1);
    assert_eq!(named_in(&found, ZZ, "extra").len(), 1);
    assert_eq!(written[1], written[2], "the second visit and the third write the same bytes");
}

// ---------------------------------------------------------------------------------------------------------------- what is bound above stays as it was

/// The package of `shapes` whose `office:drawing` declares `zz` (a container's declaration, which the model holds as a root).
fn package_declaring_on_the_drawing(shapes: &str) -> Vec<u8> {
    let drawing = Drawing::new().page(shapes);
    let content = drawing.content_xml().replace("<office:drawing>", &format!(r#"<office:drawing xmlns:zz="{ZZ}">"#));
    crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![
            ("content.xml".to_owned(), content.into_bytes()),
            ("styles.xml".to_owned(), drawing.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    )
}

/// **A prefix the `office:drawing` declares was always kept**, with the one declaration on the container and none on the elements: the control of the others.
#[test]
fn a_prefix_the_drawing_element_declares_is_bound_there_and_the_elements_declare_nothing() {
    let v = visit_bytes(&package_declaring_on_the_drawing(&rect(
        r#" zz:flag="1""#,
        r#"<zz:extra zz:k="2"/><text:p>Hello <zz:note>big</zz:note> world</text:p>"#,
    )));
    let found = v.found();
    assert_eq!(named_in(&found, ZZ, "extra").len(), 1, "{}", v.content());
    assert_eq!(named_in(&found, ZZ, "note").len(), 1, "{}", v.content());
    assert_eq!(v.content().matches("xmlns:zz=").count(), 1, "one declaration, on the container: {}", v.content());
    v.assert_silent("a prefix the container declares");
}

/// A flat drawing: the root of the file is both parts' root, and a declaration on `office:body` or `office:drawing` is no root's. What uses it comes back all the same.
#[test]
fn a_flat_drawing_keeps_markup_whose_prefix_a_container_declares() {
    let inner = r#"<zz:extra zz:k="2"/><text:p>See <zz:note zz:k="3">big</zz:note> and <text:a xlink:type="simple" xlink:href="https://example.org/f">f</text:a></text:p>"#;
    let flat = Drawing::new().page(&rect(r#" zz:flag="1""#, inner)).flat_xml(crate::odg_build::DRAWING);
    let flat = flat.replace("<office:drawing>", &format!(r#"<office:drawing xmlns:zz="{ZZ}">"#));
    let dir = Dir::new();
    dir.write("a.fodg", flat.as_bytes());
    let first = leg(&dir, "a.fodg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    let content = crate::odg_build::part_of(&dir.read("c.odg"), "content.xml");
    let found = found_in(&parse(&content));
    let shape = named_in(&found, "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "rect");
    assert_eq!(shape.len(), 1, "{content}");
    assert_eq!(shape[0].attr(ZZ, "flag"), Some("1"), "{content}");
    assert_eq!(named_in(&found, ZZ, "extra").len(), 1, "{content}");
    assert_eq!(named_in(&found, ZZ, "note").len(), 1, "{content}");
    assert_eq!(named_in(&found, TEXT, "a").len(), 1, "{content}");
    assert!(named(&first).is_empty() && named(&second).is_empty(), "{first} / {second}");
}

// ---------------------------------------------------------------------------------------------------------------- what cannot be written is said

/// **A prefix nothing binds anywhere is never written as it was, and the save says so**: `qq` is declared by no element, so a namespace-aware reader cannot read what uses it. The element of
/// the text is left out and the shape's attribute and child are left out, each with a word of the save (the plain visit said nothing of the text), and the check names them.
#[test]
fn markup_whose_prefix_nothing_binds_is_left_out_with_a_word_for_each_place() {
    let v = visit(&rect(
        r#" qq:flag="1""#,
        r#"<qq:extra/><text:p>Hello <qq:note>big</qq:note> and <text:a qq:href="https://example.org/q">link</text:a> world</text:p>"#,
    ));
    let (classes, said) = v.said();
    assert!(said.contains("nothing binds"), "the save says what it left out of the text: {said}");
    assert!(said.contains("1 attribute") && said.contains("1 element"), "and what it left out of the shape: {said}");
    assert!(!classes.is_empty(), "the check names what did not come back: {classes:?}");
    let content = v.content();
    assert!(!content.contains("qq:"), "nothing is written with a prefix nothing binds: {content}");
}

/// The words are the ones of a note the engine gives a program in `warnings`, with the class a harness reads.
#[test]
fn the_words_for_unbound_markup_in_a_text_are_a_save_note() {
    let v = visit(&rect("", r#"<text:p>Hello <qq:note>big</qq:note> world</text:p>"#));
    assert!(words(&v.second).contains("1 element of the text of shape"), "{}", words(&v.second));
    assert!(words(&v.second).contains("uses a namespace prefix that nothing binds where it is written"), "{}", words(&v.second));
}

// ---------------------------------------------------------------------------------------------------------------- the formatting a style kept

const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";

/// The drawing with `styles` among its automatic styles, and `shapes` on its page.
fn visit_styles(styles: &str, shapes: &str) -> Visit {
    let mut drawing = Drawing::new();
    drawing.auto.push_str(styles);
    visit_bytes(&drawing.page(shapes).bytes())
}

/// **The attributes of the properties of a text or paragraph style that are in a namespace of their own come back** (the plain visit dropped them and said nothing, and the check named nothing
/// either): one whose prefix the properties element declares, one the root binds, and one in no namespace, on the properties of a text style and on both properties of a paragraph style.
#[test]
fn the_attributes_of_a_style_in_a_namespace_of_its_own_come_back() {
    let styles = format!(
        concat!(
            r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt" xmlns:zz="{zz}" zz:keep="1" ext:keep2="2" plain="3"/></style:style>"#,
            r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center" xmlns:zz="{zz}" zz:para="4"/>"#,
            r#"<style:text-properties ext:inner="5"/></style:style>"#
        ),
        zz = ZZ
    );
    let shapes = rect("", r#"<text:p text:style-name="P2">Hello <text:span text:style-name="T1">big</text:span> world</text:p>"#);
    let v = visit_styles(&styles, &shapes);
    let found = v.found();
    let text_props = named_in(&found, STYLE, "text-properties");
    let para_props = named_in(&found, STYLE, "paragraph-properties");
    assert!(
        text_props.iter().any(|p| p.attr(ZZ, "keep") == Some("1") && p.attr(ROOT_EXT, "keep2") == Some("2") && p.attr("", "plain") == Some("3")),
        "{}",
        v.content()
    );
    assert!(para_props.iter().any(|p| p.attr(ZZ, "para") == Some("4")), "{}", v.content());
    assert!(text_props.iter().any(|p| p.attr(ROOT_EXT, "inner") == Some("5")), "{}", v.content());
    v.assert_silent("the attributes of a style");
}

/// **What cannot be written is said**: an attribute of a style whose prefix nothing binds, and one whose prefix the part has for another namespace (`fo` bound by the properties element to
/// the vector's own), are not written, and the save counts them. The rest of the style is written.
#[test]
fn an_attribute_of_a_style_that_cannot_be_written_is_said() {
    let styles = format!(
        concat!(
            r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center" qq:lost="1"/>"#,
            r#"<style:text-properties xmlns:fo="{zz}" fo:other="2" keep="3"/></style:style>"#
        ),
        zz = ZZ
    );
    let v = visit_styles(&styles, &rect("", r#"<text:p text:style-name="P2">Hello</text:p>"#));
    let said = words(&v.second);
    assert!(said.contains("2 attributes of the formatting of the text styles"), "{said}");
    let found = v.found();
    assert!(
        named_in(&found, STYLE, "text-properties").iter().any(|p| p.attr("", "keep") == Some("3")),
        "the rest of the style is written: {}",
        v.content()
    );
    assert!(!v.content().contains("qq:"), "{}", v.content());
}

// ---------------------------------------------------------------------------------------------------------------- a custom shape's geometry

/// A custom shape holding `geometry_own` on its `draw:enhanced-geometry` and `shape_own` on its own start tag.
fn custom(shape_own: &str, geometry_own: &str) -> String {
    format!(
        concat!(
            r#"<draw:custom-shape draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"{shape_own}>"#,
            r#"<draw:enhanced-geometry{geometry_own} svg:viewBox="0 0 21600 21600" draw:type="flowchart-process" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#
        ),
        shape_own = shape_own,
        geometry_own = geometry_own
    )
}

/// **The geometry element a custom shape held comes back when its attribute's prefix the shape declares** (the plain visit made the outline again from the rows, so the file's element was not
/// written, and said nothing): the element keeps `zz:note` in the namespace it was read in, and the legs name nothing.
#[test]
fn a_geometry_element_whose_attribute_has_a_prefix_the_shape_declares_is_written_as_it_was() {
    let v = visit(&custom(&zz(""), r#" zz:note="kept""#));
    let found = v.found();
    let geometry = named_in(&found, "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "enhanced-geometry");
    assert_eq!(geometry.len(), 1, "{}", v.content());
    assert_eq!(geometry[0].attr(ZZ, "note"), Some("kept"), "{}", v.content());
    v.assert_silent("a geometry element");
}

/// **A geometry element that cannot be written is said**: its attribute's prefix is declared nowhere, so the outline is made again from the rows and the shape's note says an element was not written.
#[test]
fn a_geometry_element_that_cannot_be_written_is_said() {
    let v = visit(&custom("", r#" qq:note="kept""#));
    let said = words(&v.second);
    assert!(said.contains("1 element that this save does not write"), "{said}");
    assert!(!v.content().contains("qq:"), "{}", v.content());
}

// ---------------------------------------------------------------------------------------------------------------- an earlier version's attribute

const DIA: &str = "urn:app.diacraft:extension";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";

/// The package of `drawing` as ODF 1.0 says it: every part's `office:version` and the manifest's are 1.0, the version an earlier producer's file carries (its schema takes any attribute on a
/// properties element, and a later version's does not).
fn version_one_point_zero(drawing: &Drawing) -> Vec<u8> {
    let old = |text: String| text.replace(r#"version="1.3""#, r#"version="1.0""#).into_bytes();
    crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![
            ("content.xml".to_owned(), old(drawing.content_xml())),
            ("styles.xml".to_owned(), old(drawing.styles_xml())),
            ("meta.xml".to_owned(), old(drawing.meta_xml())),
        ],
    )
}

/// The drawing of an ODF 1.0 file whose automatic paragraph style `P2` holds `paragraph` on its paragraph properties and whose text style `T1` holds `text` on its text properties.
fn visit_old(paragraph: &str, text: &str) -> Visit {
    let mut drawing = Drawing::new();
    drawing.auto.push_str(&format!(
        r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"{text}/></style:style><style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center"{paragraph}/></style:style>"#
    ));
    let shapes = rect("", r#"<text:p text:style-name="P2">Hello <text:span text:style-name="T1">big</text:span></text:p>"#);
    visit_bytes(&version_one_point_zero(&drawing.page(&shapes)))
}

/// **An attribute of ODF 1.0 that the version written has no place for is not written**: `text:enable-numbering` on the paragraph properties of an automatic paragraph style (no attribute of
/// ODF 1.2 or 1.4 there; the schema of 1.0 takes any). It was written as it stood, and the file failed the schema of the version it says. It rides in DiaCraft's own attribute
/// (`diacraft:older-enable-numbering`) on the same element, as the style sheets' forms of an earlier version ride in the document file, and a read gives the attribute back.
#[test]
fn text_enable_numbering_of_odf_1_0_is_left_out_of_the_markup_and_rides_in_the_extension() {
    let v = visit_old(r#" text:enable-numbering="false""#, "");
    let found = v.found();
    let props = named_in(&found, STYLE, "paragraph-properties");
    assert!(!props.is_empty(), "{}", v.content());
    for p in &props {
        assert_eq!(
            p.attr(TEXT, "enable-numbering"),
            None,
            "no attribute the written version's schema lacks, on a paragraph properties element: {}",
            v.content()
        );
    }
    assert!(props.iter().any(|p| p.attr(DIA, "older-enable-numbering") == Some("false")), "it rides in DiaCraft's own attribute: {}", v.content());
    assert!(props.iter().any(|p| p.attr(FO, "text-align") == Some("center")), "the rest of the style is written: {}", v.content());
    v.assert_silent("an attribute of an earlier version that rides");
    // A read of the file gives the attribute back where it was (the model holds the style as it was read), and writes the same file again.
    let (doc, _) = crate::odg_build::read_back(&v.written);
    let style =
        doc.ext.get().and_then(|d| d.odf.styles.iter().find(|s| &*s.name == "P2" && &*s.family == "auto:paragraph")).expect("the style is kept");
    let held = style.element.children.iter().find_map(|c| match c {
        diacraft_doc::RawNode::Element(e) if e.name.ends_with("paragraph-properties") => Some(e),
        _ => None,
    });
    assert_eq!(held.and_then(|e| e.attrs.get("text:enable-numbering")), Some("false"), "{style:?}");
    let again = visit_bytes(&v.written);
    assert_eq!(again.content(), v.content(), "a second visit writes the same");
}

/// **No other attribute of an ODF namespace is written where the schemas of 1.2 and 1.4 have none**: `text:display`, `text:condition`, `text:number-lines` and `text:line-number` are theirs
/// on text and paragraph properties and are written, and one that is not (`draw:fill-color` on a text style, `text:display` on a paragraph's properties) is left out, and the save says it.
#[test]
fn an_attribute_of_the_standards_namespaces_the_written_version_does_not_take_there_is_said_and_one_it_takes_is_written() {
    let v = visit_old(
        r##" text:number-lines="true" text:line-number="3" draw:fill-color="#ff0000""##,
        r#" text:display="none" text:condition="none" text:enable-numbering="true""#,
    );
    let found = v.found();
    let para = named_in(&found, STYLE, "paragraph-properties");
    let text = named_in(&found, STYLE, "text-properties");
    assert!(para.iter().any(|p| p.attr(TEXT, "number-lines") == Some("true") && p.attr(TEXT, "line-number") == Some("3")), "{}", v.content());
    assert!(text.iter().any(|p| p.attr(TEXT, "display") == Some("none") && p.attr(TEXT, "condition") == Some("none")), "{}", v.content());
    for p in para.iter().chain(text.iter()) {
        assert_eq!(p.attr(DRAW, "fill-color"), None, "{}", v.content());
        assert_eq!(p.attr(TEXT, "enable-numbering"), None, "{}", v.content());
    }
    let said = words(&v.second);
    assert!(
        said.contains("2 attributes of the formatting of the text styles"),
        "the draw: attribute and the text-properties one are said, the carried one is not: {said}"
    );
}
