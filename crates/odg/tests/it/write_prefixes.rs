//! A prefix the writer declares on a start tag is declared once, and a part with a tag that names an attribute twice is never saved (found by the hostile-file sweeps).
//!
//! **The bug.** A name the writer adds in ODF's `svg`, `draw`, `fo`, `style` or `text` namespace, or in DiaCraft's, is written under the prefix the place binds to that namespace, and under a
//! prefix of its own (`svg1`, `draw1`, ...), declared on the tag, where the place binds none (a file that binds `svg` to another namespace, or to the W3C's). The editor made one `TagEdit` for each
//! edit of a shape and each kept the declarations it had made in itself, so a connector whose ends and then whose path were written (two edits) had `xmlns:svg1` written twice on its tag, which
//! is not XML: the engine would not open the file, and `file.save` said it had saved it. The page tag of a page added with comments held the declarations twice too (the page's own and its comments').
//!
//! **What the vectors show**, flat (`.fodg`) and packaged (`.odg`): the connector of the sweep's seed, with `svg` bound elsewhere, moved with the shape it is glued to and saved by the engine; the
//! same inside a group that rebinds `svg` (a prefix bound twice in nested scopes); a shape whose place binds no prefix to ODF's `draw` namespace, turned and put in another place in the page's
//! order in one save; a page added with a comment to a file that binds `svg` to the W3C's. Each file saved has no start tag with a name twice (the container's parser refuses one) and opens again,
//! and each prefix of the writer's own is declared once on each tag it is written on.

use std::sync::Arc;

use diacraft_container::xml;
use diacraft_doc::{Cell, CellName, Shape, ShapeList, Value};
use diacraft_testkit::door::seeds::{self, Seed};
use diacraft_testkit::door::{Outcome, engine_door, engine_move_and_save};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::write_core::{open, open_bytes, part, real, shape_mut};

const ODF_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const OTHER: &str = "urn:x:other";
const DRAWING_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";

/// The text of the seed's flat document.
fn flat_seed(at: usize) -> String {
    String::from_utf8(seeds::fodg().remove(at).bytes).expect("a flat document is text")
}

/// The seed's packaged `content.xml`, changed by `change`, and the package made of it.
fn packaged_seed(change: impl Fn(&str) -> String) -> Vec<u8> {
    let seed: Seed = seeds::odg().remove(0);
    let mut archive = seed.archive.expect("a packaged seed has its archive");
    let at = archive.find("content.xml").expect("a content part");
    let text = String::from_utf8(archive.entries[at].data.clone()).expect("text");
    archive.entries[at].data = change(&text).into_bytes();
    archive.honest()
}

/// Every start tag of the XML `text` names each attribute once: the container's parser refuses a repeat.
#[track_caller]
fn well_formed(what: &str, text: &[u8]) {
    if let Err(e) = xml::parse(text) {
        panic!("{what}: {e}\n{}", String::from_utf8_lossy(text));
    }
}

/// The start tag of the first element called `name` in `text`.
fn start_tag<'a>(text: &'a str, name: &str) -> &'a str {
    let from = text.find(&format!("<{name}")).unwrap_or_else(|| panic!("no <{name}> in {text}"));
    let rest = &text[from..];
    &rest[..rest.find('>').expect("a start tag ends") + 1]
}

/// The file the engine writes after the first shape of the seed is moved: damaged, refused in words or fine.
#[track_caller]
fn moved_and_saved(what: &str, bytes: &[u8], extension: &str) -> Vec<u8> {
    match engine_move_and_save(bytes, extension) {
        Ok(saved) => saved,
        Err(words) => panic!("{what}: {words}"),
    }
}

/// The engine opens the file again (a saved drawing is a drawing).
#[track_caller]
fn opens_again(what: &str, bytes: &[u8], extension: &str) {
    match engine_door(bytes, extension, 0) {
        Outcome::Failed(bug) => panic!("{what}: {bug}"),
        Outcome::Refused(words) => panic!("{what}: the file saved does not open again: {words}"),
        Outcome::Passed(_) => {}
    }
}

/// A flat or packaged seed with `svg` bound to `OTHER` on the root: the connector glued to the first shape is written by the engine after the move under a prefix of the writer's own.
#[test]
fn a_connector_of_a_file_that_binds_svg_to_another_namespace_is_saved_with_the_prefix_declared_once_flat_and_packaged() {
    let flat = flat_seed(0).replace(ODF_SVG, OTHER);
    let saved = moved_and_saved("flat", flat.as_bytes(), "fodg");
    well_formed("flat", &saved);
    let text = String::from_utf8(saved.clone()).expect("text");
    let tag = start_tag(&text, "draw:connector");
    assert_eq!(tag.matches("xmlns:svg1=").count(), 1, "the declaration is made once on the connector: {tag}");
    assert!(
        tag.contains("svg1:x1=") && tag.contains("svg1:d=") && tag.contains("svg1:viewBox="),
        "the ends and the path are written under it: {tag}"
    );
    opens_again("flat", &saved, "fodg");

    let package = packaged_seed(|content| content.replace(ODF_SVG, OTHER));
    let saved = moved_and_saved("packaged", &package, "odg");
    let content = part(&saved, "content.xml");
    well_formed("packaged", content.as_bytes());
    let tag = start_tag(&content, "draw:connector");
    assert_eq!(tag.matches("xmlns:svg1=").count(), 1, "{tag}");
    assert!(tag.contains("svg1:x1=") && tag.contains("svg1:d=") && tag.contains("svg1:viewBox="), "{tag}");
    opens_again("packaged", &saved, "odg");
}

/// The same bound to another namespace by an element inside the page, around the shapes and the connector: the group's own tag and the connector's both declare the prefix of the writer's
/// own (a nested scope binds it again), each once. And the same where the root binds `svg` right and the group binds it to another namespace.
#[test]
fn a_prefix_the_writer_declares_in_nested_scopes_is_declared_once_on_each_tag_that_needs_it() {
    let wrap = |text: &str, rebind: &str| {
        let open_at = text.find("<draw:page ").expect("a page");
        let after = open_at + text[open_at..].find('>').expect("the page's tag ends") + 1;
        let close = text.find("</draw:page>").expect("the page ends");
        format!("{}<draw:g {rebind}>{}</draw:g>{}", &text[..after], &text[after..close], &text[close..])
    };
    for (what, root_binds_other) in [("the root binds svg elsewhere too", true), ("the root binds svg right", false)] {
        let rebind = format!("xmlns:svg=\"{OTHER}\"");
        let flat = {
            let text = flat_seed(0);
            let text = if root_binds_other { text.replace(ODF_SVG, OTHER) } else { text };
            wrap(&text, &rebind)
        };
        let saved = moved_and_saved(&format!("flat, {what}"), flat.as_bytes(), "fodg");
        well_formed(&format!("flat, {what}"), &saved);
        let text = String::from_utf8(saved.clone()).expect("text");
        for name in ["draw:g", "draw:connector"] {
            let tag = start_tag(&text, name);
            assert!(tag.matches("xmlns:svg1=").count() <= 1, "flat, {what}: {tag}");
        }
        opens_again(&format!("flat, {what}"), &saved, "fodg");
        let package = packaged_seed(|content| {
            let content = if root_binds_other { content.replace(ODF_SVG, OTHER) } else { content.to_owned() };
            wrap(&content, &rebind)
        });
        let saved = moved_and_saved(&format!("packaged, {what}"), &package, "odg");
        let content = part(&saved, "content.xml");
        well_formed(&format!("packaged, {what}"), content.as_bytes());
        for name in ["draw:g", "draw:connector"] {
            assert!(start_tag(&content, name).matches("xmlns:svg1=").count() <= 1, "packaged, {what}: {content}");
        }
        opens_again(&format!("packaged, {what}"), &saved, "odg");
    }
}

/// A page of two shapes: the first carries `draw:z-index` (so the page's order is written as z-indexes), the second is in the drawing namespace by the default namespace and binds the prefix
/// `draw` to another namespace on itself, so that no prefix in force binds ODF's `draw` there.
fn draw_elsewhere() -> Drawing {
    Drawing::a4().page(&format!(
        "<draw:custom-shape draw:name=\"A\" draw:z-index=\"5\" svg:x=\"2cm\" svg:y=\"3cm\" svg:width=\"2cm\" svg:height=\"2cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>\
         <rect xmlns=\"{DRAWING_NS}\" xmlns:draw=\"{OTHER}\" svg:x=\"8cm\" svg:y=\"3cm\" svg:width=\"2cm\" svg:height=\"2cm\"/>"
    ))
}

/// The second shape turned (`draw:transform` added) and put first in the page's order (`draw:z-index` added): two edits of one tag, which binds no prefix to `draw`.
fn turned_and_put_first(o: &mut crate::write_core::Open) {
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, 2).sheet)
        .cells
        .insert(CellName::new("Angle"), Cell::constant(Value::from_number(0.5).expect("a number")));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).expect("a page"));
    let mut shapes: Vec<Arc<Shape>> = page.shapes.iter().cloned().collect();
    shapes.reverse();
    page.shapes = ShapeList::from_vec(shapes);
}

#[test]
fn a_shape_whose_place_binds_no_prefix_to_the_drawing_namespace_gets_the_prefix_of_the_writer_s_own_declared_once_packaged() {
    let mut o = open(&draw_elsewhere());
    assert_eq!(o.doc.pages.get(0).expect("a page").shapes.len(), 2, "the shape in the default namespace is read");
    turned_and_put_first(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&saved.bytes, "content.xml");
    well_formed("packaged", content.as_bytes());
    let tag = start_tag(&content[content.find("<rect").expect("the second shape")..], "rect");
    assert_eq!(tag.matches("xmlns:draw1=").count(), 1, "{tag}");
    assert!(tag.contains("draw1:transform=") && tag.contains("draw1:z-index="), "both names are written under it: {tag}");
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.pages.get(0).expect("a page").shapes.len(), 2);
}

#[test]
fn a_shape_whose_place_binds_no_prefix_to_the_drawing_namespace_gets_the_prefix_of_the_writer_s_own_declared_once_flat() {
    let d = draw_elsewhere();
    let flat = Flat::of(&d);
    let mut o = open_bytes(flat.bytes());
    assert_eq!(o.doc.pages.get(0).expect("a page").shapes.len(), 2);
    turned_and_put_first(&mut o);
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    well_formed("flat", &saved.bytes);
    let text = String::from_utf8(saved.bytes.clone()).expect("text");
    let tag = start_tag(&text[text.find("<rect").expect("the second shape")..], "rect");
    assert_eq!(tag.matches("xmlns:draw1=").count(), 1, "{tag}");
    assert!(tag.contains("draw1:transform=") && tag.contains("draw1:z-index="), "{tag}");
    assert_eq!(open_bytes(saved.bytes).doc.pages.get(0).expect("a page").shapes.len(), 2);
}

/// A page the model adds, with a comment on it, to a package that binds `svg` to the W3C's namespace: the page's own tag declares `svg1`, and so did the comments' block, so the tag held it
/// twice. (A flat drawing is saved with no page added: `a page added to the drawing ... in a flat drawing`.)
fn page_with_a_comment(d: &Drawing) -> Vec<u8> {
    let mut d = d.clone();
    d.ns = d.ns.replace(ODF_SVG, "http://www.w3.org/2000/svg");
    let mut o = open_bytes(d.bytes());
    let page = crate::write_page_ops::new_page(&o.doc, 1, "Added", Vec::new());
    o.doc.pages.push(Arc::new(page));
    let author = o.doc.comments.author_for("Ada", "Ad").expect("an author");
    o.doc
        .comments
        .add(diacraft_doc::NewComment {
            parent: None,
            page: 1,
            shape: None,
            author,
            text: Arc::from("a note"),
            date: Arc::from("2031-03-07T20:05:09Z"),
        })
        .expect("a comment");
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}")).bytes
}

#[test]
fn a_page_added_with_a_comment_to_a_package_that_binds_svg_to_the_w3c_namespace_declares_each_prefix_once() {
    let saved = page_with_a_comment(&Drawing::a4().page(&crate::write_core::custom("a", "1cm")));
    let content = part(&saved, "content.xml");
    well_formed("packaged", content.as_bytes());
    let page = &content[content.find("<draw:page draw:name=\"Added\"").expect("the page added")..];
    assert_eq!(start_tag(page, "draw:page").matches("xmlns:svg1=").count(), 1, "{}", start_tag(page, "draw:page"));
    assert!(page.contains("<office:annotation"), "the comment is written: {page}");
    assert_eq!(open_bytes(saved).doc.pages.len(), 2);
}
