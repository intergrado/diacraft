//! The content check sets the ordered children of an element side by side by what they are, and not by their place, on `.odg` packages and flat `.fodg` files built
//! from the ODF standard. The old reading was measured on sample files: where the written file adds an element the source lacked (an `office:font-face-decls`, in 100 package
//! files) or drops one the source held (an `office:scripts`, in 291 of 320 flat files), every later sibling was set beside the wrong one, the root's children counted as different
//! and nothing inside them was compared; in 51 files none of 638 planted differences in `content.xml` was seen. A manifest with a DOCTYPE (32 files) was compared as bytes.
//!
//! Each test has a **masking control**: a difference planted after the insertion or the drop, deep inside a later sibling, is counted and found at its own place.

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_odg::{Limits, compare_content};

use crate::build::{DRAWING, replace, with_manifest, write};
use crate::canon::{at, diff, graphic, shape_with};
use crate::drawing::Drawing;
use crate::flat::Flat;

/// The differences as `(kind, path)`, sorted.
fn seen(d: &ContentDiff) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = d.shown.iter().map(|x| (x.found.word(), format!("{}:{}", x.part, x.path))).collect();
    out.sort();
    out
}

const FONTS: &str = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#;

/// A drawing as a writer of the model writes it: faces declared in both parts, an automatic style, one page with one shape holding `text`.
fn written(text: &str) -> Drawing {
    let mut d = Drawing::a4().auto(&graphic("gr1", "#ff0000")).page(&shape_with("s", &at("1cm"), text));
    d.fonts = FONTS.to_owned();
    d
}

/// `d` with `from` put as `to` in its automatic styles of `content.xml`.
fn auto_changed(mut d: Drawing, from: &str, to: &str) -> Drawing {
    d.auto = d.auto.replace(from, to);
    d
}

/// `d` with `from` put as `to` in its pages.
fn pages_changed(mut d: Drawing, from: &str, to: &str) -> Drawing {
    d.pages = d.pages.replace(from, to);
    d
}

/// The same drawing as its source has it: `content.xml` with no `office:font-face-decls` at all (the faces are declared in `styles.xml` alone).
fn without_decls(d: &Drawing) -> Vec<u8> {
    let files = d.files();
    let content = files.iter().find(|(n, _)| n == "content.xml").map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap();
    let without = content.replace(&format!("<office:font-face-decls>{}</office:font-face-decls>", d.fonts), "");
    assert_ne!(without, content, "the declarations were there to leave out");
    crate::build::package(DRAWING, replace(files, "content.xml", without.into_bytes()))
}

// ------------------------------------------------------------------------------------------------------------------- an element the written file adds

#[test]
fn an_element_the_written_file_adds_is_one_difference_in_a_package() {
    let clean = diff(&without_decls(&written("a")), &written("a").bytes());
    assert_eq!(seen(&clean), [("element", "content.xml:/document-content[0]/font-face-decls[0]".to_owned())], "{:?}", clean.shown);
}

#[test]
fn a_difference_planted_after_the_added_element_is_counted_and_found_where_it_is() {
    let source = without_decls(&written("a"));
    // The text of the shape, an automatic style's colour, the shape's place and the page's name: each after the container the written file adds.
    let plants: [(&str, Drawing, &str); 4] = [
        ("a text", written("b"), "content.xml:/document-content[0]/body[1]/drawing[0]/page[0]/custom-shape[0]/p[0]/#text[0]"),
        (
            "an automatic style's colour",
            auto_changed(written("a"), "#ff0000", "#ff0001"),
            "content.xml:/document-content[0]/automatic-styles[0]/style#0/graphic-properties[0]/@fill-color",
        ),
        (
            "a place",
            pages_changed(written("a"), "svg:x=\"1cm\"", "svg:x=\"2cm\""),
            "content.xml:/document-content[0]/body[1]/drawing[0]/page[0]/custom-shape[0]/@x",
        ),
        (
            "a page's name",
            pages_changed(written("a"), "draw:name=\"page1\"", "draw:name=\"other\""),
            "content.xml:/document-content[0]/body[1]/drawing[0]/page[0]/@name",
        ),
    ];
    for (what, planted, path) in plants {
        let d = diff(&source, &planted.bytes());
        let found = seen(&d);
        assert_eq!(d.count, 2, "{what}: {:?}", d.shown);
        assert!(found.iter().any(|(kind, p)| *kind == "element" && p == "content.xml:/document-content[0]/font-face-decls[0]"), "{what}: {found:?}");
        assert!(found.iter().any(|(_, p)| p == path), "{what} is found at {path}: {found:?}");
    }
}

#[test]
fn an_element_the_source_has_and_the_written_file_lacks_is_one_difference_as_well() {
    // The first document is the written one here and the second the source: the count is the same either way round, and so is what is found.
    let (source, planted) = (without_decls(&written("a")), written("b").bytes());
    let d = diff(&planted, &source);
    assert_eq!(d.count, 2, "{:?}", d.shown);
}

// ---------------------------------------------------------------------------------------------------------------------------- an element dropped

fn scripted(d: &Drawing) -> Flat {
    let mut f = Flat::of(d);
    f.scripts = r#"<office:scripts><office:script script:language="ooo:script"/></office:scripts>"#.to_owned();
    f
}

#[test]
fn a_container_the_written_flat_file_drops_is_one_difference_and_the_content_after_it_is_compared() {
    let source = scripted(&written("a")).bytes();
    // The scripts are all the written file lacks.
    let clean = diff(&source, &Flat::of(&written("a")).bytes());
    assert_eq!(seen(&clean), [("element", "office:document:/document[0]/scripts[0]".to_owned())], "{:?}", clean.shown);
    // The masking control: a text planted in the shape, after the scripts, deep in the body.
    let planted = diff(&source, &Flat::of(&written("b")).bytes());
    assert_eq!(planted.count, 2, "{:?}", planted.shown);
    let found = seen(&planted);
    assert!(found.iter().any(|(kind, p)| *kind == "element" && p == "office:document:/document[0]/scripts[0]"), "{found:?}");
    assert!(found.iter().any(|(kind, p)| *kind == "text" && p.ends_with("/custom-shape[0]/p[0]/#text[0]")), "{found:?}");
    // Against the package of the written file, which has no scripts: the same.
    let against_package = diff(&source, &written("b").bytes());
    assert_eq!(against_package.count, 2, "{:?}", against_package.shown);
}

// ------------------------------------------------------------------------------------------------------------------ siblings with a name of their own

fn shapes_named(names: &[&str], text: &str) -> String {
    names.iter().enumerate().map(|(k, n)| shape_with(n, &at(&format!("{}cm", k + 1)), text)).collect()
}

#[test]
fn a_shape_put_in_before_the_others_is_one_difference_and_each_of_the_others_is_compared_with_its_own() {
    let one = Drawing::a4().page(&shapes_named(&["s1", "s2", "s3", "s4"], "x")).bytes();
    // The shape `n` put in first, and each of the others with another text: one element and four texts.
    let two = Drawing::a4().page(&format!("{}{}", shape_with("n", &at("9cm"), "x"), shapes_named(&["s1", "s2", "s3", "s4"], "y"))).bytes();
    let d = diff(&one, &two);
    assert_eq!(d.count, 5, "{:?}", d.shown);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Text).count(), 4, "{:?}", d.shown);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Element).count(), 1, "{:?}", d.shown);
    // One of them taken out of the middle of the second.
    let three = Drawing::a4().page(&format!("{}{}", shapes_named(&["s1"], "y"), shapes_named(&["s3", "s4"], "y"))).bytes();
    let d = diff(&one, &three);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Element).count(), 1, "{:?}", d.shown);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Text).count(), 3, "{:?}", d.shown);
}

#[test]
fn pages_written_in_another_order_are_one_order_and_what_changed_inside_a_page_is_found() {
    let page = |name: &str, text: &str| {
        format!(r#"<draw:page draw:name="{name}" draw:master-page-name="Default">{}</draw:page>"#, shape_with(name, &at("1cm"), text))
    };
    let mut a = Drawing::a4();
    a.pages = format!("{}{}", page("p1", "x"), page("p2", "x"));
    let mut b = Drawing::a4();
    b.pages = format!("{}{}", page("p2", "y"), page("p1", "x"));
    let d = diff(&a.bytes(), &b.bytes());
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Order).count(), 1, "{:?}", d.shown);
    assert_eq!(d.shown.iter().filter(|x| x.found == Found::Text).count(), 1, "{:?}", d.shown);
    // The pages in another order and nothing else is one order, as it was.
    let mut c = Drawing::a4();
    c.pages = format!("{}{}", page("p2", "x"), page("p1", "x"));
    let d = diff(&a.bytes(), &c.bytes());
    assert_eq!((d.count, d.shown.first().map(|x| x.found)), (1, Some(Found::Order)), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------------- a DOCTYPE in the manifest

const DOCTYPE: &str = r#"<!DOCTYPE manifest:manifest PUBLIC "-//OpenOffice.org//DTD Manifest 1.0//EN" "Manifest.dtd">"#;

/// A package of `d` whose manifest is as an old writer writes it: with a DOCTYPE after the XML declaration, and the same rows as the package of `d` has.
fn with_doctype(d: &Drawing, change: impl Fn(String) -> String) -> Vec<u8> {
    let mut files = with_manifest(DRAWING, d.files());
    let at = files.iter().position(|(n, _)| n == "META-INF/manifest.xml").unwrap();
    let text = String::from_utf8(files[at].1.clone()).unwrap();
    let (declaration, rest) = text.split_once('\n').unwrap();
    files[at].1 = change(format!("{declaration}\n{DOCTYPE}\n{rest}")).into_bytes();
    write(Some(DRAWING), &files)
}

#[test]
fn a_manifest_with_a_doctype_is_the_same_content_as_one_without() {
    let d = written("a");
    let plain = d.bytes();
    let old = with_doctype(&d, |m| m);
    assert_ne!(plain, old);
    for (a, b) in [(&plain, &old), (&old, &plain), (&old, &old)] {
        let found = diff(a, b);
        assert!(found.same(), "the DOCTYPE is no content: {:?}", found.shown);
    }
}

#[test]
fn a_difference_in_a_manifest_with_a_doctype_is_a_value_and_not_bytes() {
    let d = written("a");
    let changed = with_doctype(&d, |m| m.replace("manifest:media-type=\"text/xml\"", "manifest:media-type=\"text/plain\""));
    let found = diff(&d.bytes(), &changed);
    assert!(found.count >= 1, "the media type of a row differs");
    assert!(found.shown.iter().all(|x| x.found == Found::Value), "a value of a row, never the bytes of the part: {:?}", found.shown);
    // A row taken out of the manifest of the old writer is a row the other side has.
    let dropped = with_doctype(&d, |m| m.lines().filter(|l| !l.contains("styles.xml")).collect::<Vec<_>>().join("\n"));
    let found = diff(&d.bytes(), &dropped);
    assert_eq!(found.count, 1, "{:?}", found.shown);
    assert_eq!(found.shown.first().map(|x| x.found), Some(Found::Element), "{:?}", found.shown);
}

#[test]
fn a_doctype_that_declares_something_is_refused_as_it_always_was() {
    let d = written("a");
    let declared = with_doctype(&d, |m| m.replace(DOCTYPE, "<!DOCTYPE manifest:manifest [<!ENTITY e \"x\">]>"));
    let err = compare_content(&d.bytes(), &declared, &Limits::default()).unwrap_err();
    assert_eq!(err.class(), "xml_entity_refused", "{err}");
}
