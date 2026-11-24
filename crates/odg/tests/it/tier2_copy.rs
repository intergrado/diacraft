//! The review: an edit of a shape that was read as a copy.
//!
//! A LibreOffice copy and paste makes a second shape whose style repeats the first's `diacraft:id` (and the first's data). The reader gives the later shape its own
//! number and none of the data, and the index names it a copy. A save that **edits** the copy and adds a cell used to decode the style it names (the first shape's
//! data, its `ID` among it), keep the `ID`, and write it again: the duplicate was written anew, and when the file was read the copy was a copy again, and the edit
//! was gone. An edit of a copy gives it a tier 2 of its own (what the edit adds, its own `ID`) in a style that does not repeat the other shape's, and what is written
//! is read back and compared, so that a duplicate written by an edit is a refused save and not a lost edit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Value};
use diacraft_odg::SaveNote;

use crate::docfile::rebuild;
use crate::drawing::Drawing;
use crate::write_core::{custom, move_by, open, open_bytes, part, real, shape_mut};
use crate::write_new::{push_shape, rectangle};

/// A saved drawing whose new shape carries tier 2 (a user cell `Comment`), and the shape's id.
fn saved_with_tier2() -> (Vec<u8>, u32) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("mine")));
    push_shape(&mut o.doc, shape);
    (real(&o.doc, &o.bytes, &o.index).unwrap().bytes, id)
}

/// The shape's element put again after itself under another name, as LibreOffice's paste does: with a style of its own that repeats the original's attributes
/// (`shared` false), or on the original's own style (`shared` true).
fn with_a_copy(bytes: &[u8], shared: bool) -> Vec<u8> {
    let content = part(bytes, "content.xml");
    let style_start = content.find("<style:style style:name=\"gr1\"").expect("the shape's style");
    let style_end = style_start + content[style_start..].find("</style:style>").unwrap() + "</style:style>".len();
    let copy_style = content[style_start..style_end].replace("style:name=\"gr1\"", "style:name=\"gr9\"");
    let shape_start = content.find("<draw:custom-shape draw:style-name=\"gr1\"").expect("the shape");
    let shape_end = shape_start + content[shape_start..].find("</draw:custom-shape>").unwrap() + "</draw:custom-shape>".len();
    let style_of_copy = if shared { "gr1" } else { "gr9" };
    let copy_shape = content[shape_start..shape_end]
        .replace("draw:style-name=\"gr1\"", &format!("draw:style-name=\"{style_of_copy}\""))
        .replace("draw:name=\"R\"", "draw:name=\"R_copy\"");
    let mut out = String::new();
    out.push_str(&content[..style_end]);
    if !shared {
        out.push_str(&copy_style);
    }
    out.push_str(&content[style_end..shape_end]);
    out.push_str(&copy_shape);
    out.push_str(&content[shape_end..]);
    rebuild(bytes, |name, b| Some(if name == "content.xml" { out.clone().into_bytes() } else { b.to_vec() }), |m| m.to_owned())
}

fn text_cell(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

/// The ids the styles of `content.xml` carry, in order.
fn ids_of(content: &str) -> Vec<String> {
    content.split("diacraft:id=\"").skip(1).map(|r| r.split('"').next().unwrap_or_default().to_owned()).collect()
}

/// The edit of the review (probe RV1): a cell added to the copy. The copy keeps it, has an `ID` of its own, the first shape keeps its data and its `ID`, nothing is
/// a copy any more, and a second save is no edit. Both shapes of a paste: the copy on a style of its own, and on the original's.
#[test]
fn an_edit_that_adds_a_cell_to_a_copy_gives_it_an_id_and_data_of_its_own() {
    for shared in [false, true] {
        let (bytes, id) = saved_with_tier2();
        let copied = with_a_copy(&bytes, shared);
        let mut o = open_bytes(copied.clone());
        let page = o.doc.pages.get(0).unwrap();
        let copy = page.shapes.get(2).unwrap().id;
        assert_eq!(o.index.page(0).unwrap().copies(), [copy], "shared {shared}: the copy is read as one");
        assert_ne!(copy, id);
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, copy).sheet).cells.insert(CellName::new("Mine"), text_cell("added to the copy"));
        let saved = real(&o.doc, &copied, &o.index).unwrap_or_else(|e| panic!("shared {shared}: not saved: {e}"));
        assert!(saved.edited, "shared {shared}");
        // What the edit wrote for the shape of the file is read back as a new shape's is (the check of the bytes made): the file is read again and counted.
        assert!(
            saved.work.read_back_bytes >= saved.bytes.len() as u64,
            "shared {shared}: the tier 2 an edit wrote is read back: {} bytes",
            saved.work.read_back_bytes
        );
        let content = part(&saved.bytes, "content.xml");
        let ids = ids_of(&content);
        assert_eq!(ids.len(), 2, "shared {shared}: one id for each shape that carries data: {content}");
        assert_ne!(ids[0], ids[1], "shared {shared}: no id is written twice: {content}");
        assert!(
            ids.contains(&id.to_string()) && ids.contains(&copy.to_string()),
            "shared {shared}: the first keeps {id}, the copy has its own number {copy}: {ids:?}"
        );
        let back = open_bytes(saved.bytes.clone());
        let shapes = &back.doc.pages.get(0).unwrap().shapes;
        let (first, second) = (shapes.get(1).unwrap(), shapes.get(2).unwrap());
        assert_eq!((first.id, second.id), (id, copy), "shared {shared}: the ids as the model had them");
        assert!(first.sheet.cell("Comment").is_some() && first.sheet.cell("Mine").is_none(), "shared {shared}: the first shape's data is its own");
        assert!(second.sheet.cell("Mine").is_some(), "shared {shared}: the edit of the copy is in the file");
        assert!(second.sheet.cell("Comment").is_none(), "shared {shared}: and the first shape's data is not the copy's");
        assert!(back.index.page(0).unwrap().copies().is_empty(), "shared {shared}: nothing is read as a copy now");
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "shared {shared}: a second save is no edit");
    }
}

/// An edit of a copy that adds nothing the markup cannot say but restyles it (a fill colour): the copy's style is its own and holds none of the first shape's
/// attributes, so it no longer repeats its `ID`; and the first shape is as it was.
#[test]
fn a_restyle_of_a_copy_leaves_it_a_style_that_repeats_no_id_and_no_data() {
    for shared in [false, true] {
        let (bytes, id) = saved_with_tier2();
        let copied = with_a_copy(&bytes, shared);
        let mut o = open_bytes(copied.clone());
        let copy = o.doc.pages.get(0).unwrap().shapes.get(2).unwrap().id;
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, copy).sheet).cells.insert(CellName::new("FillForegnd"), text_cell("#ff0000"));
        let saved = real(&o.doc, &copied, &o.index).unwrap_or_else(|e| panic!("shared {shared}: not saved: {e}"));
        assert!(saved.edited);
        let content = part(&saved.bytes, "content.xml");
        assert_eq!(ids_of(&content), [id.to_string()], "shared {shared}: only the first shape's style carries the id: {content}");
        let back = open_bytes(saved.bytes.clone());
        let shapes = &back.doc.pages.get(0).unwrap().shapes;
        assert!(shapes.get(1).unwrap().sheet.cell("Comment").is_some(), "shared {shared}: the first shape keeps its data");
        assert!(shapes.get(2).unwrap().sheet.cell("Comment").is_none(), "shared {shared}: the copy has none");
        let fill = shapes.get(2).unwrap().sheet.cell("FillForegnd").and_then(|c| c.value.as_ref()).map(|v| v.text().to_ascii_lowercase());
        assert_eq!(fill.as_deref(), Some("#ff0000"), "shared {shared}: the restyle is written");
        assert!(back.index.page(0).unwrap().copies().is_empty(), "shared {shared}: and it is no copy: the style repeats nothing");
    }
}

/// An edit of the copy that is only a move leaves both styles as the file has them (the copy is still a copy, and says so).
#[test]
fn a_move_of_a_copy_leaves_the_styles_as_the_file_has_them_and_names_the_copy() {
    let (bytes, id) = saved_with_tier2();
    let copied = with_a_copy(&bytes, false);
    let mut o = open_bytes(copied.clone());
    let copy = o.doc.pages.get(0).unwrap().shapes.get(2).unwrap().id;
    move_by(&mut o.doc, 0, copy, 1.0, 0.0);
    let saved = real(&o.doc, &copied, &o.index).unwrap();
    assert!(saved.edited);
    assert_eq!(ids_of(&part(&saved.bytes, "content.xml")), [id.to_string(), id.to_string()], "both styles as the file had them");
    assert!(saved.notes.iter().any(|n| matches!(n, SaveNote::DuplicateIds { shapes, .. } if shapes == &[copy])), "{:?}", saved.notes);
}
