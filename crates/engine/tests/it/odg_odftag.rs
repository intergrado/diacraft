//! What the `.odg` reader keeps of a shape's own markup (the attributes and children no field reads) is in the shape's `odf` slot, and the engine's copies carry
//! the slot: a pasted or duplicated shape keeps the values the file held for it, but for the ids of the file's own (an id names one element, and a copy is another), and a save
//! that flattens a new shape carries it the same way.

use diacraft_doc::Document;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, id_of, open_drawing};
use crate::run;

/// A rectangle named `name` with the ids of the file's own, an attribute of another namespace and two children the reader keeps whole.
fn rich(name: &str, x: f64) -> String {
    format!(
        r#"<draw:rect draw:name="{name}" xml:id="id-{name}" draw:id="id-{name}" ext:note="kept" svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" draw:style-name="gr1"><svg:title>a title</svg:title><ext:more a="1"/></draw:rect>"#
    )
}

/// The `odf` data of the shape `id` on page 0: its attributes, as pairs, and how many children it keeps.
fn odf_of(doc: &Document, id: u32) -> (Vec<(String, String)>, usize) {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}"));
    let kept = shape.ext.get().map(|d| d.odf.kept.clone()).unwrap_or_default();
    (kept.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(), kept.kept.len())
}

fn names_on_page(doc: &Document) -> Vec<(u32, String)> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| (s.id, s.name_u.as_deref().unwrap_or("").to_owned())).collect()
}

/// A copy and a paste of a shape the file holds: the pasted shape keeps the attributes of another namespace and the children the file held for it, and holds neither of the
/// ids, which name the original's element. The original holds both, as it did.
#[test]
fn copy_and_paste_of_an_odf_shape_keeps_its_odf_values() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&rich("A", 2.0)));
    let a = id_of(s.document().unwrap(), 0, "A");
    let (before, kept_before) = odf_of(s.document().unwrap(), a);
    assert!(before.iter().any(|(k, v)| k == "xml:id" && v == "id-A") && before.iter().any(|(k, _)| k == "ext:note"), "{before:?}");
    assert_eq!(kept_before, 2, "the title and the foreign element");
    run(&mut s, "edit.copy", json!({ "ids": [a] }));
    run(&mut s, "edit.paste", json!({}));
    let doc = s.document().unwrap();
    let pasted = names_on_page(doc).into_iter().map(|(id, _)| id).find(|id| *id != a).expect("a pasted shape");
    let (raw, kept) = odf_of(doc, pasted);
    assert!(raw.iter().any(|(k, v)| k == "ext:note" && v == "kept"), "the attribute of another namespace goes with the copy: {raw:?}");
    assert_eq!(kept, 2, "and so do the children");
    assert!(!raw.iter().any(|(k, _)| k == "xml:id" || k == "draw:id"), "the ids name the original's element and no other: {raw:?}");
    assert_eq!(odf_of(doc, a), (before, kept_before), "the original is as it was");
    // The values the file held beside the cells go with the copy too: the name of the automatic style it named, and the held values with their stamps, which are
    // still good for the copy's own cells (a paste moves the shape, and no cell of a stamp is a place).
    let on_page = |id: u32| doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).expect("the shape").clone();
    let (original, copy) = (on_page(a), on_page(pasted));
    let held_of = |shape: &diacraft_doc::Shape| shape.ext.get().map(|d| (d.odf.style_name.clone(), d.odf.held.clone())).unwrap_or_default();
    let (name, held) = held_of(&copy);
    assert_eq!(name.as_deref(), Some("gr1"), "the style name the file gave the original");
    assert!(!held.is_empty(), "a plain rectangle holds the element that gives its outline");
    assert_eq!(held, held_of(&original).1, "the same values as the original's");
    let outline = held.iter().find(|h| h.group == diacraft_doc::HeldGroup::Geometry).expect("the outline's value");
    assert_eq!(
        outline.stamp,
        Some(diacraft_doc::stamp_sections(&copy.sheet, &["Geometry"])),
        "stamped by the copy's own Geometry sections, which are as they were read"
    );
}

/// A duplicate is a copy of the same kind.
#[test]
fn a_duplicate_of_an_odf_shape_holds_none_of_the_ids_of_the_original() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&rich("A", 2.0)));
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.duplicate", json!({ "ids": [a] }));
    let doc = s.document().unwrap();
    let copies: Vec<u32> = names_on_page(doc).into_iter().map(|(id, _)| id).filter(|id| *id != a).collect();
    assert_eq!(copies.len(), 1);
    let (raw, kept) = odf_of(doc, copies[0]);
    assert!(raw.iter().any(|(k, _)| k == "ext:note") && kept == 2, "{raw:?} {kept}");
    assert!(!raw.iter().any(|(k, _)| k == "xml:id" || k == "draw:id"), "{raw:?}");
}

/// The shapes of the copy a save hands the writer carry the slot as they carry `raw` and `passthrough`: a pasted shape saved to the file is written, and the save names what
/// it did not write of what the copy holds (the attribute of another namespace and the children).
#[test]
fn a_pasted_odf_shape_is_saved_and_the_save_says_what_of_its_markup_it_did_not_write() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&rich("A", 2.0)));
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "edit.copy", json!({ "ids": [a] }));
    run(&mut s, "edit.paste", json!({}));
    let saved = run(&mut s, "file.save", json!({}));
    let words: Vec<String> = saved["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_owned)).collect();
    assert!(words.iter().any(|w| w.contains("attribute") && w.contains("element")), "{words:?}");
    // The second save is no edit.
    let again = run(&mut s, "file.save", json!({}));
    assert_eq!(again["edited"], json!(false), "{again}");
}
