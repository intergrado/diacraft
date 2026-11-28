//! The markup the `.odg` reader keeps (the attributes and the children of an element that no field of the model reads) is in the `odf` slot of the object, the
//! splice writer reads it from there, and the VSDX slots stay empty. What these tests do not cover is the part of the task that is not built (see the task's report): the reader
//! does not yet fill the held values, the named definitions, the style names, the page layouts, the font faces, the document's properties, the package entries and the units.

use std::sync::Arc;

use diacraft_doc::{Document, Kept, Page, Passthrough, RawNode, Shape};
use diacraft_odg::{Limits, ReadReport, read};
use diacraft_sheetxml::model::{Budget, MemBlobs, decode, encode};

use crate::drawing::Drawing;
use crate::write_core::{open, real};

/// A page holding a rectangle with an attribute of another namespace and two children kept whole, and a group with a title, on a page whose own element has an attribute no
/// field reads and a child kept whole.
fn drawing() -> Drawing {
    let mut d = Drawing::a4();
    d.pages = concat!(
        r#"<draw:page draw:name="p" draw:master-page-name="Default" draw:style-name="dp1" xml:id="pg1">"#,
        r#"<draw:rect draw:name="R" xml:id="idR" draw:id="idR" x:cell="2" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><svg:title>T</svg:title><x:more a="1"/><text:p>t</text:p></draw:rect>"#,
        r#"<draw:g draw:name="G"><svg:title>group title</svg:title><draw:rect svg:x="4cm" svg:y="1cm" svg:width="1cm" svg:height="1cm" x:own="m"/></draw:g>"#,
        r#"<x:page-child b="2"/>"#,
        r#"</draw:page>"#
    )
    .to_owned();
    d.ns = d.ns.replace("xmlns:diacraft", "xmlns:x=\"urn:example:extension\" xmlns:diacraft");
    d
}

fn odf_of(ext: &diacraft_doc::Ext<diacraft_doc::OdfShape>) -> Kept {
    ext.get().map(|d| d.odf.kept.clone()).unwrap_or_default()
}

fn the_shape<'a>(doc: &'a Document, name: &str) -> &'a Arc<Shape> {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}"))
}

fn read_it() -> (Document, ReadReport) {
    read(&drawing().bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{} ({e})", e.class()))
}

/// What the `.odg` reader keeps is in the `odf` slot of the shape, the group and the page, and the VSDX `raw` and `passthrough` of each are empty, as are the slots' VSDX and
/// native sides: the model tags each format's markup, so that no writer of the other format meets it (the design's 3.2).
#[test]
fn odf_markup_goes_to_the_odf_slot_and_the_vsdx_slots_stay_empty() {
    let (doc, _) = read_it();
    let r = the_shape(&doc, "R");
    let kept = odf_of(&r.ext);
    assert_eq!(kept.raw.get("x:cell"), Some("2"));
    assert_eq!(kept.raw.get("xml:id"), Some("idR"));
    assert_eq!(kept.raw.get("draw:id"), Some("idR"));
    assert_eq!(kept.kept.len(), 1, "the foreign element, and not the text or the title (a row of the User section)");
    assert!(r.raw.is_empty() && r.passthrough.is_empty(), "the VSDX `raw` and `passthrough` of the shape are empty");
    let data = r.ext.get().expect("the slot");
    assert!(data.vsdx.0.is_empty() && data.native == Kept::default(), "the VSDX and native sides of the slot are empty");
    // The group: its own children are kept in its slot, its member's attribute in the member's.
    let g = the_shape(&doc, "G");
    assert_eq!(odf_of(&g.ext).kept.len(), 1, "the group's title (which its sheet's rows hold too)");
    assert!(g.passthrough.is_empty() && g.raw.is_empty());
    let member = g.children.get(0).unwrap();
    assert_eq!(odf_of(&member.ext).raw.get("x:own"), Some("m"));
    assert!(member.raw.is_empty());
    // The page's element: its attributes and its kept child.
    let page: &Page = doc.pages.get(0).unwrap();
    let p = page.ext.get().expect("the page's slot").odf.kept.clone();
    assert_eq!(p.raw.get("draw:style-name"), Some("dp1"));
    assert_eq!(p.raw.get("xml:id"), Some("pg1"));
    assert!(p.raw.get("draw:name").is_none(), "the page's name is the page's own field");
    assert_eq!(p.raw.get("draw:master-page-name"), Some("Default"), "kept beside the field that reads it, as the reader kept it in `raw` before");
    assert_eq!(p.kept.len(), 1);
    assert!(matches!(p.kept.iter().next().map(|k| &k.node), Some(RawNode::Element(e)) if &*e.name == "x:page-child"));
    assert!(page.raw.is_empty() && page.passthrough.is_empty(), "the VSDX fields of the page are empty");
    assert!(page.ext.get().is_some_and(|d| d.vsdx.0.is_empty() && d.native == Kept::default()));
    // A document that is not an `.odg`'s: no slot at all, so a model with no ODF data compares as it did.
    assert!(diacraft_doc::Document::new_blank().pages.iter().all(|p| p.ext.is_empty()));
}

/// An unedited save of the drawing is the file, byte for byte, and says nothing; a save after an edit of a shape that holds markup keeps that markup as the file had it, and
/// the file the save made reads back to the same slots and is no edit when saved again. (The plan's whole test, every existing write test, is the suite's own.)
#[test]
fn an_unedited_odg_splice_save_is_byte_identical_as_before() {
    let o = open(&drawing());
    let same = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!same.edited && same.bytes == o.bytes, "an unedited save is the file");
    assert!(same.notes.is_empty(), "{:?}", same.notes);
    // A move of the shape that holds markup: the markup is as the file had it, and the slots read back the same.
    let mut doc = o.doc.clone();
    let id = the_shape(&doc, "R").id;
    crate::write_core::move_by(&mut doc, 0, id, 0.5, 0.0);
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let content = crate::write_core::part(&saved.bytes, "content.xml");
    for kept in [
        r#"x:cell="2""#,
        r#"xml:id="idR""#,
        "<svg:title>T</svg:title>",
        r#"<x:more a="1"/>"#,
        r#"<x:page-child b="2"/>"#,
        "<svg:title>group title</svg:title>",
    ] {
        assert!(content.contains(kept), "{kept} is as the file had it: {content}");
    }
    let again = open_after(&saved.bytes);
    let (before, after) = (odf_of(&the_shape(&o.doc, "R").ext), odf_of(&the_shape(&again.doc, "R").ext));
    assert_eq!(before, after, "the slot reads back as it was");
    let second = real(&again.doc, &again.bytes, &again.index).unwrap();
    assert!(!second.edited, "a second save is no edit");
}

fn open_after(bytes: &[u8]) -> crate::write_core::Open {
    crate::write_core::open_bytes(bytes.to_vec())
}

/// The writer compares what the `odf` slot keeps: a kept node the model lost, or an attribute it changed, on a shape or on a page, is refused, in words, as the same loss of a
/// kept node was when it was in `passthrough`; and a slot that holds a value beside the kept markup (a held value, a style name) makes no edit of its own.
#[test]
fn a_kept_node_or_attribute_that_the_slot_lost_or_changed_is_refused() {
    let o = open(&drawing());
    let refused = |doc: &Document, what: &str| match real(doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(!words.is_empty(), "{what}"),
        other => panic!("{what}: {:?}", other.map(|w| w.edited)),
    };
    // The shape's kept children gone.
    let mut doc = o.doc.clone();
    let at = doc.pages.get(0).unwrap().shapes.iter().position(|s| s.name_u.as_deref() == Some("R")).unwrap();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(page.shapes.get_mut(at).unwrap()).ext.make_mut().odf.kept.kept = Passthrough::new();
    refused(&doc, "the shape's kept children are gone");
    // The shape's kept attribute changed.
    let mut doc = o.doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(page.shapes.get_mut(at).unwrap()).ext.make_mut().odf.kept.raw = diacraft_doc::RawAttrs::from_pairs([("x:cell", "3")]);
    refused(&doc, "the shape's kept attribute changed");
    // The page's kept child gone, and its kept attribute changed.
    let mut doc = o.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).ext.make_mut().odf.kept.kept = Passthrough::new();
    refused(&doc, "the page's kept child is gone");
    let mut doc = o.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).ext.make_mut().odf.kept.raw = diacraft_doc::RawAttrs::new();
    refused(&doc, "the page's kept attributes are gone");
    // A value in the slot beside the kept markup is no edit: the writer writes what it writes from the cells and the kept markup.
    let mut doc = o.doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
    shape.ext.make_mut().odf.style_name = Some(Arc::from("gr9"));
    shape.ext.make_mut().odf.units.push("svg:x", "in");
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).ext.make_mut().odf.layout = Some(Arc::from("PM9"));
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    assert!(!saved.edited && saved.bytes == o.bytes, "a name or a unit in the slot says nothing the file says");
}

/// The model the reader made comes back from the codec the same, the slot and all: what the `.odg` reader keeps is data the `.diacraft` codec writes and reads (the part of the
/// design's `odg_to_diacraft_and_reopen_is_the_same_model` that does not need the package).
#[test]
fn a_model_read_from_an_odg_comes_back_from_the_codec_the_same() {
    let (doc, _) = read_it();
    let mut blobs = MemBlobs::new();
    let encoded = encode(&doc, &mut blobs).unwrap_or_else(|e| panic!("{e:?}"));
    let pages: Vec<&diacraft_container::xml::El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&diacraft_container::xml::El> = encoded.masters.iter().map(|(_, e)| e).collect();
    let mut budget = Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4);
    let mut notes = Vec::new();
    let back = decode(&encoded.document, &pages, &masters, &blobs, &mut budget, &mut notes).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(notes.is_empty(), "{notes:?}");
    let (a, b) = (doc.pages.get(0).unwrap(), back.pages.get(0).unwrap());
    assert_eq!(a.ext, b.ext, "the page's slot");
    assert_eq!(the_shape(&doc, "R").ext, the_shape(&back, "R").ext, "the shape's slot");
    assert_eq!(the_shape(&doc, "G").ext, the_shape(&back, "G").ext, "the group's slot");
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), back.pages.get(0).unwrap().shapes.len());
    assert!(odf_of(&the_shape(&back, "R").ext).raw.get("xml:id").is_some(), "the ids the file held come back with the shape");
}

/// What the slots cost the reader is counted, before they are made: a page of many shapes that each keep an attribute costs a constant number of items each, linear in their
/// number, and the cost is the slot's allocation and the attribute's own items (the model's bytes are the process ledger's).
#[test]
fn the_slots_are_charged_by_the_items_they_hold_and_linearly() {
    let items = |n: usize| {
        let shapes: String = (0..n).map(|k| format!(r#"<draw:rect svg:x="{k}cm" svg:width="1cm" svg:height="1cm" x:k="v"/>"#)).collect();
        let mut d = Drawing::a4().page(&shapes);
        d.ns = d.ns.replace("xmlns:diacraft", "xmlns:x=\"urn:example:extension\" xmlns:diacraft");
        let report = read(&d.bytes(), &Limits::default()).unwrap().1;
        report.work.items
    };
    let (one, hundred, thousand) = (items(1), items(101), items(1_001));
    let per = (hundred - one) / 100;
    assert_eq!(thousand - hundred, per * 900, "linear: every shape costs the same");
    let slot = (16 + std::mem::size_of::<diacraft_doc::ExtData<diacraft_doc::OdfShape>>()).div_ceil(64) as u64;
    // The same shapes with no attribute to keep cost the slot and the attribute's pair (two items) and its bytes (a name and a value: one item) the less.
    let plain = |n: usize| {
        let shapes: String = (0..n).map(|k| format!(r#"<draw:rect svg:x="{k}cm" svg:width="1cm" svg:height="1cm"/>"#)).collect();
        read(&Drawing::a4().page(&shapes).bytes(), &Limits::default()).unwrap().1.work.items
    };
    let per_plain = (plain(101) - plain(1)) / 100;
    // A rectangle holds the element that gives its outline, so the slot is there with no attribute kept: the attribute costs its pair and its bytes.
    let _ = slot;
    assert_eq!(per - per_plain, 2 + 1, "a kept attribute costs its pair and its bytes beside the slot a rectangle has");
}

/// A shape that keeps nothing of its markup keeps nothing of it in its slot: the slot holds the element it was read from and the ends it was read with, no attribute, no
/// child, no style name, no unit.
#[test]
fn a_shape_that_keeps_nothing_has_no_kept_markup() {
    let mut d = Drawing::a4().page(r#"<draw:line svg:x1="1cm" svg:y1="1cm" svg:x2="2cm" svg:y2="1cm"/>"#);
    d.pages = d.pages.replace(r#" draw:master-page-name="Default""#, "");
    let (doc, _) = read(&d.bytes(), &Limits::default()).unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let odf = &shape.ext.get().expect("the element it was read from").odf;
    assert!(odf.kept.raw.is_empty() && odf.kept.kept.is_empty() && odf.style_name.is_none() && odf.units.is_empty());
    assert_eq!(odf.held.len(), 2, "the ends and the element");
}
