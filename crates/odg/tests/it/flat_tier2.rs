//! Met: a flat drawing has no package, so it has no document file
//! (`DiaCraft/document.xml` is a package entry). What a shape's own automatic style carries (tier 2) is in the one document and works as in a package; what the
//! document file carries (the masters, themes, colours, the document's cells, a page's `ID`, `NameU` and cells) has no place in a flat drawing, so a change of it is
//! refused in words and never dropped, and only DiaCraft's own data is written (as the flat extension).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, ColorEntry, Value};
use diacraft_odg::{Form, Limits, OriginalPackage, WriteError, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::flat_write::{picture_shape, save, second_save_is_no_edit, small_png, text_of, three};
use crate::write_core::{open, open_bytes, real};
use crate::write_new::{push_shape, rectangle};

/// What a flat save is refused with: the words, or a panic that says what was written instead.
fn refusal(o: &crate::write_core::Open) -> String {
    match save(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped what the model holds: {} bytes", saved.bytes.len()),
    }
}

/// A new shape of a flat drawing that holds a user cell, a section the writer does not write and a formula the markup cannot say is written with them on its own
/// automatic style, as a package's is, and the file reads back as the model has it, and a second save is no edit.
#[test]
fn a_new_shape_of_a_flat_drawing_carries_what_its_markup_cannot_say_and_reads_back_the_same() {
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 8.0, 4.0, 2.0));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
        sheet.cells.insert(CellName::new("Cost"), Cell::read(Some("Width*3"), Some("12"), None, None, &mut 1_000));
    }
    push_shape(&mut o.doc, shape);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a flat drawing saves tier 2: {e}"));
    assert!(saved.edited);
    let text = text_of(&saved.bytes);
    assert!(text.contains(":cells=\"&lt;Sheet>"), "the cells are an attribute of the extension on the shape's own style: {text}");
    assert!(text.starts_with("<?xml") && !text.contains("DiaCraft/document.xml"), "one document, and no document file: {text}");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_tier2_unreadable"), 0);
    let read_back = back.pages.get(0).unwrap().shapes.get(3).expect("the new shape reads back");
    assert_eq!(read_back.sheet.cell("Comment").and_then(|c| c.value.as_ref()).map(Value::text), Some("note"), "the user cell");
    assert_eq!(read_back.sheet.cell("Cost").and_then(|c| c.formula.text()), Some("Width*3"), "the formula, verbatim");
    second_save_is_no_edit(&o.doc, &saved);
}

/// A flat drawing has no document file: a change of the document's colours, its cells, or a page's `NameU` is refused in words (the package writer carries each in
/// the document file), and the drawing the writer was given is not written.
#[test]
fn a_change_of_what_the_document_file_carries_is_refused_in_a_flat_drawing_and_never_dropped() {
    let flat = three().bytes();
    let mut o = open_bytes(flat.clone());
    Arc::make_mut(&mut o.doc.colors).push(ColorEntry { ix: 24, rgb: Arc::from("#112233") });
    assert!(refusal(&o).contains("document's own fields"), "a colour of the document's table");
    let mut o = open_bytes(flat.clone());
    Arc::make_mut(&mut o.doc.sheet).cells.insert(CellName::new("Tag"), Cell::constant(Value::from_text("doc cell")));
    assert!(refusal(&o).contains("document's own fields"), "a cell of the document's own sheet");
    let mut o = open_bytes(flat.clone());
    o.doc.masters.push(Arc::new(diacraft_doc::Master { id: 7, name: Some(Arc::from("Master 7")), ..diacraft_doc::Master::default() }));
    assert!(refusal(&o).contains("document's own fields"), "a master");
    let mut o = open_bytes(flat.clone());
    o.doc.themes.push(Arc::new(diacraft_doc::Theme {
        part: Arc::from("theme/theme1.xml"),
        bytes: Arc::from(b"<a:theme/>".to_vec()),
        ..diacraft_doc::Theme::default()
    }));
    assert!(refusal(&o).contains("document's own fields"), "a theme");
    let mut o = open_bytes(flat.clone());
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Original.1"));
    assert!(refusal(&o).contains("page's own fields"), "the `NameU` of a page");
    let mut o = open_bytes(flat.clone());
    let id = o.doc.pages.get(0).unwrap().id;
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).id = id + 1;
    assert!(refusal(&o).contains("page's own fields"), "the `ID` of a page");
    let mut o = open_bytes(flat.clone());
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("DrawingScaleType"), Cell::constant(Value::from_text("0")));
    assert!(refusal(&o).contains("page's own fields"), "a cell of a page that no page attribute stands for");
    // The same changes in a package are carried by the document file (the contrast): the save is made and reads back.
    let mut p = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    Arc::make_mut(&mut p.doc.colors).push(ColorEntry { ix: 24, rgb: Arc::from("#112233") });
    Arc::make_mut(p.doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Original.1"));
    let saved = real(&p.doc, &p.bytes, &p.index).expect("a package carries them");
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.colors.len(), 1);
    // And DiaCraft's own data is no refusal in a flat drawing: it is the flat extension's.
    let mut o = open_bytes(flat);
    o.doc.set_diacraft_child("snap", Some(&[("flags", "6")]));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("DiaCraft's own data is written as the flat extension");
    assert_eq!(open_bytes(saved.bytes).doc.diacraft_child("snap").and_then(|e| e.attrs.get("flags").map(str::to_owned)), Some("6".to_owned()));
}

/// A package that holds a document file written as a flat document: the file has no place there (it is a package entry no part of the flat document names), and
/// the conversion is refused in words and not written without what it carried (Q8 gives no rule for it; Q10 asks where a flat drawing keeps what the file holds).
#[test]
fn a_package_that_carries_a_document_file_is_not_written_as_a_flat_document_without_it() {
    let mut p = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    Arc::make_mut(&mut p.doc.colors).push(ColorEntry { ix: 24, rgb: Arc::from("#112233") });
    let saved = real(&p.doc, &p.bytes, &p.index).expect("a package carries the colour");
    let o = open_bytes(saved.bytes);
    assert_eq!(o.doc.colors.len(), 1, "the file is read");
    let options = WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() };
    match write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("a package part a flat document has no place for"), "{words}"),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(flat) => panic!("a flat document that dropped the document file: {} bytes", flat.bytes.len()),
    }
}

/// A new picture of a flat drawing carries tier 2 as every new shape does (the arm is one for a package and a flat drawing): a cell no style attribute says (a user
/// cell), a formula in a style cell and a custom line end are written and read back, and the picture with the cells a style says is written without any.
#[test]
fn a_new_picture_with_a_cell_no_style_attribute_says_carries_it_as_tier_2() {
    let saved_with = |edit: &dyn Fn(&mut diacraft_doc::Shape)| {
        let mut o = open_bytes(three().bytes());
        let id = o.index.next_id(0).unwrap();
        let mut shape = picture_shape(id, "Picture.1", (2.0, 8.0, 4.0, 3.0), &small_png());
        edit(&mut shape);
        push_shape(&mut o.doc, shape);
        let saved = save(&o.doc, &o.bytes, &o.index).expect("tier 2 carries what the picture's style cannot say");
        let (back, _) = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap();
        let read =
            back.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("Picture.1")).cloned().expect("the picture reads back");
        (saved, read)
    };
    // A cell no style attribute says.
    let (_, read) = saved_with(&|s| {
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
    });
    assert_eq!(read.sheet.cell("Comment").map(|c| c.value.as_ref().map(Value::text).unwrap_or_default().to_owned()), Some("note".to_owned()));
    // A formula in a style cell.
    let (_, read) = saved_with(&|s| {
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LineWeight"), Cell::read(Some("THEMEVAL()"), Some("0.01"), None, None, &mut 1_000));
    });
    assert_eq!(read.sheet.cell("LineWeight").and_then(|c| c.formula.text()), Some("THEMEVAL()"));
    // A custom line end (254): number 4's marker is drawn and the cell is tier 2, as for any new shape.
    let (_, read) = saved_with(&|s| {
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("EndArrow"), Cell::constant(Value::from_text("254")));
    });
    assert_eq!(read.sheet.cell("EndArrow").map(|c| c.value.as_ref().map(Value::text).unwrap_or_default().to_owned()), Some("254".to_owned()));
    // A colour in capitals is a style attribute, written as it was (the file reads it back in lower case, which is the same colour).
    let (saved, _) = saved_with(&|s| {
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LineColor"), Cell::constant(Value::from_text("#FF0000")));
    });
    assert!(text_of(&saved.bytes).contains("#ff0000") || text_of(&saved.bytes).contains("#FF0000"));
    // And the plain picture is written (the control), with no tier 2.
    let (saved, _) = saved_with(&|_| {});
    assert!(text_of(&saved.bytes).contains("<draw:image draw:mime-type=\"image/png\">"));
}

/// The attributes a new picture's frame is written with (`svg:width`, `svg:height`, `svg:x` and `svg:y`) are looked for on the tag the save made, by the save's own
/// check of the bytes made: the attributes read back are counted (`checked_attributes`), and a save of one new picture reads back every attribute of its tag.
/// (The mutation run of found no test that told a picture whose written attributes are not listed for that check.)
#[test]
fn the_attributes_a_new_picture_is_written_with_are_read_back_by_the_check_of_the_bytes_made() {
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture_shape(id, "Picture.1", (2.0, 8.0, 4.0, 3.0), &small_png()));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a plain new picture is written");
    let text = text_of(&saved.bytes);
    let tag = text.split("<draw:frame ").nth(1).and_then(|r| r.split('>').next()).expect("the frame's start tag");
    let on_the_tag = tag.matches("=\"").count() as u64;
    assert!(on_the_tag >= 8, "the tag holds its style, layer, name, id and frame: {tag}");
    assert!(
        saved.work.checked_attributes >= on_the_tag,
        "the check of the bytes made read back the {on_the_tag} attributes of the picture's tag, and counted {}",
        saved.work.checked_attributes
    );
}

/// The save of a flat drawing reads the file it made again for the tier 2 it wrote (F2: the call was deletable with no test failing): the bytes read
/// again are counted (`read_back_bytes`), for a new shape that carries tier 2 and for a shape of the file an edit gives some, and for no edit that carries none.
#[test]
fn a_save_of_a_flat_drawing_reads_back_the_tier_2_it_wrote() {
    // A new shape with a user cell.
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 8.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
    push_shape(&mut o.doc, shape);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(
        saved.work.read_back_bytes >= saved.bytes.len() as u64,
        "a new shape that carries tier 2 is read back: {} bytes",
        saved.work.read_back_bytes
    );
    // A shape of the file that an edit gives a user cell.
    let mut o = open_bytes(three().bytes());
    let first = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, first).sheet)
        .cells
        .insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(
        saved.edited && saved.work.read_back_bytes >= saved.bytes.len() as u64,
        "a shape of the file given tier 2 is read back: {}",
        saved.work.read_back_bytes
    );
    // A move reads nothing back.
    let mut o = open_bytes(three().bytes());
    crate::write_core::move_by(&mut o.doc, 0, first, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited && saved.work.read_back_bytes == 0, "a move carries no tier 2: {}", saved.work.read_back_bytes);
}

/// The shapes a flat drawing holds that repeat an earlier shape's `ID` are read as copies and named in every save, edited or not (F3).
#[test]
fn the_copies_of_a_flat_drawing_are_named_in_its_saves() {
    let style = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" diacraft:version="1" diacraft:id="5" diacraft:cells="&lt;Sheet&gt;&lt;Cell N=&quot;Comment&quot; V=&quot;a&quot;/&gt;&lt;/Sheet&gt;"/></style:style>"#;
    let rect = |name: &str, x: &str| {
        format!(
            r#"<draw:rect draw:name="{name}" draw:style-name="gr1" svg:x="{x}" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#
        )
    };
    let drawing = Drawing::a4().auto(style).page(&format!("{}{}", rect("a", "1cm"), rect("b", "5cm")));
    let flat = crate::flat::Flat::of(&drawing).bytes();
    let mut o = open_bytes(flat.clone());
    let (first, second) = (o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id, o.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id);
    assert_eq!(first, 5, "the first holder keeps the ID");
    assert_eq!(o.index.page(0).unwrap().copies(), [second], "the second is read as a copy");
    let named = |notes: &[diacraft_odg::SaveNote]| -> Vec<Vec<u32>> {
        notes.iter().filter_map(|n| if let diacraft_odg::SaveNote::DuplicateIds { shapes, .. } = n { Some(shapes.clone()) } else { None }).collect()
    };
    let unedited = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!unedited.edited);
    assert_eq!(named(&unedited.notes), [vec![second]], "an unedited save names the copy");
    crate::write_core::move_by(&mut o.doc, 0, first, 1.0, 0.0);
    let edited = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(edited.edited);
    assert_eq!(named(&edited.notes), [vec![second]], "an edited save names it too");
}
