//! APPID: the whole-model codec reads the namespace that builds made before the id moved to `diacraft.app` wrote as DiaCraft's own, and writes only the current one.
//! Every input is written by hand; the XML goes through the real reader at the native limits.
use diacraft_container::Limits;
use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{Document, Page, RawAttrs, RawNode};
use diacraft_sheetxml::model::{Budget, DecodeNote, MemBlobs, URN, content_stamp, decode_page, encode_page};

use super::model_util::*;

/// The namespace earlier builds wrote, spelled out here and not through the constant under test.
const OLD: &str = "urn:io.github.one_bit.diacraft:extension";

fn parsed(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

fn decode_one(xml: &str) -> (Page, Vec<DecodeNote>) {
    let mut notes = Vec::new();
    let page = decode_page(&parsed(xml), &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    (page, notes)
}

/// The page as the codec writes it, as text.
fn text_of(page: &Page) -> String {
    String::from_utf8(bytes_of_el(&encode_page(page, &mut MemBlobs::new()).unwrap())).unwrap()
}

const SHAPES: &str = "<diacraft:shape id=\"1\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"1\"/><diacraft:Cell N=\"PinY\" V=\"2\"/></diacraft:sheet></diacraft:shape>\
                      <diacraft:shape id=\"2\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"3\"/></diacraft:sheet></diacraft:shape>";

#[test]
fn the_current_namespace_is_the_domain_s_and_the_earlier_one_is_the_one_before_it() {
    assert_eq!(URN, "urn:app.diacraft:extension");
    assert_ne!(URN, OLD);
}

#[test]
fn a_page_in_the_earlier_namespace_is_the_page_the_current_one_gives_and_is_written_in_the_current_one() {
    let (old, notes) = decode_one(&format!("<diacraft:page xmlns:diacraft=\"{OLD}\">{SHAPES}</diacraft:page>"));
    assert!(notes.is_empty(), "{notes:?}");
    let (new, _) = decode_one(&format!("<diacraft:page xmlns:diacraft=\"{URN}\">{SHAPES}</diacraft:page>"));
    assert_eq!(old.shapes.len(), 2, "the shapes are read");
    let (from_old, from_new) = (text_of(&old), text_of(&new));
    assert_eq!(from_old, from_new, "the same page, whichever namespace it was read in");
    assert!(from_old.contains(URN) && !from_old.contains("one_bit"), "{from_old}");
    // The same under a default namespace, and under a prefix other than the codec's own.
    for xml in [
        format!(
            "<page xmlns=\"{OLD}\"><shape id=\"1\"><sheet><Cell N=\"PinX\" V=\"1\"/><Cell N=\"PinY\" V=\"2\"/></sheet></shape><shape id=\"2\"><sheet><Cell N=\"PinX\" V=\"3\"/></sheet></shape></page>"
        ),
        format!(
            "<q:page xmlns:q=\"{OLD}\"><q:shape id=\"1\"><q:sheet><q:Cell N=\"PinX\" V=\"1\"/><q:Cell N=\"PinY\" V=\"2\"/></q:sheet></q:shape><q:shape id=\"2\"><q:sheet><q:Cell N=\"PinX\" V=\"3\"/></q:sheet></q:shape></q:page>"
        ),
    ] {
        assert_eq!(text_of(&decode_one(&xml).0), from_new, "{xml}");
    }
}

#[test]
fn what_a_page_of_the_earlier_namespace_keeps_whole_is_kept_in_the_current_one_and_other_namespaces_are_left_alone() {
    // An unknown attribute in the codec's prefix, an unknown element that declares the namespace itself, one that declares it under another prefix, and one of another namespace.
    let xml = format!(
        "<diacraft:page xmlns:diacraft=\"{OLD}\"><diacraft:shape id=\"1\" diacraft:later=\"1\" x:other=\"2\" xmlns:x=\"{OLD}\" y:foreign=\"3\" xmlns:y=\"urn:y\">\
         <diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"1\"/></diacraft:sheet>\
         <diacraft:hatch xmlns:diacraft=\"{OLD}\" a=\"b\"/><k:later xmlns:k=\"{OLD}\" v=\"1\"><k:inner/></k:later><z:foreign xmlns:z=\"urn:z\"/></diacraft:shape></diacraft:page>"
    );
    let (page, notes) = decode_one(&xml);
    assert!(notes.is_empty(), "{notes:?}");
    let out = text_of(&page);
    assert!(!out.contains("one_bit"), "the earlier namespace is written nowhere: {out}");
    for kept in ["later=\"1\"", "other=\"2\"", "foreign=\"3\"", "a=\"b\"", "<k:inner", "urn:z", "urn:y"] {
        assert!(out.contains(kept), "{kept} is kept: {out}");
    }
    // What was kept is the same once the page is read again: the namespace it declares is the current one now.
    let (again, _) = decode_one(&out);
    assert_eq!(text_of(&again), out);
    let shape = page.shapes.get(0).unwrap();
    let native = shape.ext.get().map(|d| d.native.clone()).unwrap_or_default();
    assert!(native.raw.iter().any(|(k, v)| k.ends_with("later") && v == "1"), "{:?}", native.raw);
    let kept: Vec<&str> = native.kept.iter().filter_map(|p| if let RawNode::Element(e) = &p.node { Some(&*e.name) } else { None }).collect();
    assert!(kept.contains(&"diacraft:hatch") && kept.contains(&"k:later") && kept.contains(&"z:foreign"), "{kept:?}");
    let declared =
        native.kept.iter().filter_map(|p| if let RawNode::Element(e) = &p.node { Some(e) } else { None }).find(|e| &*e.name == "k:later").unwrap();
    assert_eq!(declared.attrs.get("xmlns:k"), Some(URN), "the declaration the model keeps is the current one");
}

#[test]
fn a_page_that_holds_both_namespaces_reads_as_one_page_each_entry_once() {
    // The page declares the earlier namespace for `diacraft` and the current one for `n`: the shapes of each are the page's shapes, and a shape's sheet, which a shape holds once,
    // is not read twice when both namespaces give one.
    let xml = format!(
        "<diacraft:page xmlns:diacraft=\"{OLD}\" xmlns:n=\"{URN}\">\
         <diacraft:shape id=\"1\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"1\"/></diacraft:sheet><n:sheet><n:Cell N=\"PinX\" V=\"9\"/><n:Cell N=\"PinY\" V=\"9\"/></n:sheet></diacraft:shape>\
         <n:shape id=\"2\"><n:sheet><n:Cell N=\"PinX\" V=\"3\"/></n:sheet></n:shape></diacraft:page>"
    );
    let (page, _) = decode_one(&xml);
    assert_eq!(page.shapes.len(), 2, "one shape for each entry");
    let first = page.shapes.get(0).unwrap();
    assert_eq!(first.id, 1);
    assert_eq!(first.sheet.cells.len(), 1, "the first sheet is read and the second is not: no cell twice, and none of the second's");
    assert_eq!(page.shapes.get(1).unwrap().id, 2);
    let out = text_of(&page);
    assert!(!out.contains("one_bit") && out.matches("<diacraft:shape").count() == 2, "{out}");
}

/// The content stamp of `doc`, as text.
fn stamp_of(doc: &Document) -> String {
    content_stamp(doc, &mut Budget::for_open(&Limits::default())).unwrap().text()
}

#[test]
fn a_stamp_is_the_same_whichever_namespace_the_data_declares_and_is_the_one_a_build_before_the_move_gave() {
    // The stamp is fed the namespace as version 1.0 wrote it (5.5 of the format: a later version keeps every stamp already stored), so a drawing whose data declares the earlier
    // namespace and the same drawing with the current one have one stamp, and it is the one that the blank drawing had.
    let mut current = Document::new_blank();
    assert!(current.set_diacraft_child("snap", Some(&[("strength_grid", "40")])));
    let mut earlier = current.clone();
    let data = earlier.diacraft.take().unwrap();
    let mut changed = (*data).clone();
    let pairs: Vec<(String, String)> =
        changed.attrs.iter().map(|(k, v)| (k.to_owned(), if v == URN { OLD.to_owned() } else { v.to_owned() })).collect();
    assert!(pairs.iter().any(|(_, v)| v == OLD), "the data declares the namespace");
    changed.attrs = RawAttrs::from_pairs(pairs);
    earlier.diacraft = Some(std::sync::Arc::new(changed));
    assert_eq!(stamp_of(&current), stamp_of(&earlier), "the namespace the data declares is no content");
    assert_ne!(stamp_of(&current), stamp_of(&Document::new_blank()), "and the settings are");
    // The blank drawing's stamp is the one pinned before the namespace moved (`model_vectors`): the blank drawing as it was, put a cell in the root of a new
    // one (`model_vectors` pins that stamp too).
    assert_eq!(stamp_of(&diacraft_testkit::fixtures::without_root_factor(Document::new_blank())), "00ebc460d282d097");
}
