//! Item 1: the links of a shape to a style sheet: a shape whose three links name one style
//! sheet has that style as the parent of its automatic style (`style:parent-style-name`), the root's ID taking the parent out. A change of the links from one style sheet to
//! another is written as that; any other change of the links is refused in words (it was refused whole: "a change of a shape's style links").

use std::sync::Arc;

use diacraft_doc::Links;
use diacraft_odg::{Limits, WriteError, read};

use crate::drawing::Drawing;
use crate::write_core::{Open, open, open_bytes, part, real};
use crate::write_styles_new::style_sheet;

const GR1: &str =
    r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;

fn rect(name: &str, style: &str, x: u32) -> String {
    format!(
        r#"<draw:custom-shape draw:style-name="{style}" draw:name="{name}" svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#
    )
}

/// Two shapes, `S1` and `S2`, on the one automatic style `gr1`.
fn two_on_one_style() -> Open {
    open(&Drawing::a4().auto(GR1).page(&[rect("S1", "gr1", 1), rect("S2", "gr1", 8)].concat()))
}

fn link(o: &mut Open, shape: usize, to: Links) {
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let s = Arc::make_mut(page.shapes.get_mut(shape).unwrap());
    Arc::make_mut(&mut s.sheet).links = to;
}

fn all(id: u32) -> Links {
    Links { line: Some(id), fill: Some(id), text: Some(id) }
}

fn links_of(doc: &diacraft_doc::Document, shape: usize) -> (Option<u32>, Option<u32>, Option<u32>) {
    let l = doc.pages.get(0).unwrap().shapes.get(shape).unwrap().sheet.links;
    (l.line, l.fill, l.text)
}

fn refusal(o: &Open) -> String {
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped what the model holds: {} bytes", saved.bytes.len()),
    }
}

/// **A shape whose only change is its links is written** (the trap: an edit with no style cell changed was no edit, so the save wrote nothing and every save after it
/// was an edit again). One of two shapes on a style is given a new style sheet: its style is cloned with the sheet as its parent, the other shape keeps the style it had,
/// and the file reads back with the links the model has; a second save is no edit.
#[test]
fn a_shape_given_a_style_sheet_and_nothing_else_has_it_as_the_parent_of_a_copy_of_its_style() {
    let mut o = two_on_one_style();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, all(n));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited, "the links alone are an edit");
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("style:parent-style-name=\"Theme\"").count(), 1, "one style names it: {content}");
    assert!(content.contains(r#"draw:style-name="gr1" draw:name="S2""#), "the other shape keeps the style it had: {content}");
    assert!(content.contains(r#"<style:style style:name="gr1" style:family="graphic">"#), "and the style is as it was: {content}");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!((links_of(&back, 0), links_of(&back, 1)), ((Some(n), Some(n), Some(n)), (Some(0), Some(0), Some(0))));
    assert_eq!(
        back.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("FillForegnd").map(|c| c.value.clone()),
        o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("FillForegnd").map(|c| c.value.clone()),
        "the fill the shape's style gives is the shape's still"
    );
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
    let reread = open_bytes(saved.bytes.clone());
    assert!(!real(&reread.doc, &reread.bytes, &reread.index).unwrap().edited, "nor is a save of the model the file reads as");
}

/// Both shapes of a shared style: the first is given a copy, and the second, the only one left on the style, has it edited where it stands; no style is left that nothing names.
#[test]
fn every_shape_of_a_shared_style_given_the_sheet_ends_with_the_sheet_as_the_parent_and_no_style_nothing_names() {
    let mut o = two_on_one_style();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, all(n));
    link(&mut o, 1, all(n));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("style:parent-style-name=\"Theme\"").count(), 2, "{content}");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!((links_of(&back, 0), links_of(&back, 1)), ((Some(n), Some(n), Some(n)), (Some(n), Some(n), Some(n))));
    assert!(!real(&o.doc, &saved.bytes, &saved.index).unwrap().edited);
}

/// A shape on a common style: its automatic style is new and has the sheet as its parent; the common style is not touched; and a shape whose links go back to the root has
/// no parent again.
#[test]
fn a_shape_on_a_common_style_gets_an_automatic_style_with_the_new_parent_and_the_root_takes_the_parent_out() {
    let common = r##"<style:style style:name="Base" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let mut o = open(&Drawing::a4().common(common).page(&rect("S1", "Base", 1)));
    let base = o.doc.styles.iter().position(|s| s.name_u.as_deref() == Some("Base")).unwrap();
    assert_eq!(links_of(&o.doc, 0), (Some(base as u32), Some(base as u32), Some(base as u32)), "the shape's links are the common style's");
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, all(n));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert!(
        content.contains(r#"style:parent-style-name="Theme""#) && !content.contains("draw:style-name=\"Base\""),
        "an automatic style of its own: {content}"
    );
    assert!(part(&saved.bytes, "styles.xml").contains(r#"style:name="Base""#), "the common style stays");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(links_of(&back, 0), (Some(n), Some(n), Some(n)));
    // The links go back to the root: the parent is taken out of the style.
    let mut second = open_bytes(saved.bytes.clone());
    link(&mut second, 0, all(0));
    let unlinked = real(&second.doc, &second.bytes, &second.index).unwrap();
    assert!(unlinked.edited);
    assert!(!part(&unlinked.bytes, "content.xml").contains("style:parent-style-name"), "{}", part(&unlinked.bytes, "content.xml"));
    let (back, _) = read(&unlinked.bytes, &Limits::default()).unwrap();
    assert_eq!(links_of(&back, 0), (Some(0), Some(0), Some(0)));
    assert!(!real(&second.doc, &unlinked.bytes, &unlinked.index).unwrap().edited);
}

/// A move beside a change of the links is both written: the frame and the parent.
#[test]
fn a_move_and_a_change_of_the_links_in_one_save_are_both_written() {
    let mut o = two_on_one_style();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, all(n));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let x = shape.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    Arc::make_mut(&mut shape.sheet).cells.insert(
        diacraft_doc::CellName::new("PinX"),
        diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&(x + 1.0).to_string())).with_unit("IN"),
    );
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let pin = |d: &diacraft_doc::Document| {
        d.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap()
    };
    assert!((pin(&back) - (x + 1.0)).abs() < 1e-6);
    assert_eq!(links_of(&back, 0), (Some(n), Some(n), Some(n)));
}

/// Links that name different style sheets after the edit, or a style sheet the file cannot name, are refused in words and nothing is written.
#[test]
fn links_that_name_different_sheets_or_a_sheet_the_file_cannot_name_are_refused_in_words() {
    let mut o = two_on_one_style();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, Links { line: Some(n), fill: Some(0), text: Some(0) });
    assert!(refusal(&o).contains("naming different style sheets"), "{}", refusal(&o));
    link(&mut o, 0, Links { line: Some(0), fill: Some(n), text: Some(n) });
    assert!(refusal(&o).contains("naming different style sheets"), "{}", refusal(&o));
    link(&mut o, 0, all(99));
    assert!(refusal(&o).contains("whose name the file cannot say"), "{}", refusal(&o));
    // Links that were different and still are, in other places, are as before: refused.
    let mut p = two_on_one_style();
    link(&mut p, 1, Links { line: Some(0), fill: Some(1), text: Some(0) });
    assert!(refusal(&p).contains("naming different style sheets"), "{}", refusal(&p));
    // None is the root: a shape whose links are none and are the root's is no change.
    let mut q = two_on_one_style();
    link(&mut q, 0, Links { line: None, fill: Some(0), text: None });
    assert!(real(&q.doc, &q.bytes, &q.index).is_ok());
}

/// A shape whose automatic style already has a parent is given another: the parent of the style is set to the new one where the style stands (the shape alone names it), or
/// in the copy a shape that shares it is given (the others keep the parent they had).
#[test]
fn a_style_that_has_a_parent_gets_the_new_one_in_place_or_in_its_copy() {
    let common = r#"<style:style style:name="Base" style:family="graphic"/><style:style style:name="Other" style:family="graphic"/>"#;
    let auto = r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Base"><style:graphic-properties/></style:style>"#;
    let id_of = |o: &Open, name: &str| u32::try_from(o.doc.styles.iter().position(|s| s.name_u.as_deref() == Some(name)).unwrap()).unwrap();
    // Alone on the style: edited where it stands.
    let mut o = open(&Drawing::a4().common(common).auto(auto).page(&rect("S1", "gr1", 1)));
    let (base, other) = (id_of(&o, "Base"), id_of(&o, "Other"));
    assert_eq!(links_of(&o.doc, 0), (Some(base), Some(base), Some(base)));
    link(&mut o, 0, all(other));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert!(
        content.contains(r#"style:name="gr1" style:family="graphic" style:parent-style-name="Other""#) && !content.contains("\"Base\""),
        "{content}"
    );
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(links_of(&back, 0), (Some(other), Some(other), Some(other)));
    assert!(!real(&o.doc, &saved.bytes, &saved.index).unwrap().edited);
    // Shared by two: the one that changes gets a copy with the new parent, the other keeps `Base`.
    let mut o = open(&Drawing::a4().common(common).auto(auto).page(&[rect("S1", "gr1", 1), rect("S2", "gr1", 8)].concat()));
    link(&mut o, 1, all(other));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!((links_of(&back, 0), links_of(&back, 1)), ((Some(base), Some(base), Some(base)), (Some(other), Some(other), Some(other))));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(
        (content.matches("style:parent-style-name=\"Base\"").count(), content.matches("style:parent-style-name=\"Other\"").count()),
        (1, 1),
        "{content}"
    );
}
