//! An edit of a shape of the file whose tier 2 is, or becomes, DiaCraft's (the last row of "Styles").
//!
//! What the extension carries of a shape is refreshed from the model by every save that edits the shape, so that a pose, a text or a name it carries never
//! comes back as it was, and what the file's markup cannot say of an edit (a user cell, a section, a formula) is added to it.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, SectionKey, Text, TextPart, Value};

use crate::drawing::Drawing;
use crate::write_core::{custom, move_by, open, open_bytes, part, real, shape_mut};
use crate::write_new::{push_shape, rectangle};

fn cell(formula: Option<&str>, value: &str) -> Cell {
    Cell::read(formula, Some(value), None, None, &mut 1_000)
}

/// A saved drawing whose new shape carries tier 2, and the shape's id.
fn saved_with_tier2() -> (Vec<u8>, u32) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("mine")));
    push_shape(&mut o.doc, shape);
    (real(&o.doc, &o.bytes, &o.index).unwrap().bytes, id)
}

/// The text of the style the shape named `name` uses, in `content.xml`.
fn style_of(content: &str, name: &str) -> String {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name}"));
    let open = content[..at].rfind('<').unwrap_or_default();
    let tag = &content[open..open + content[open..].find('>').unwrap_or_default()];
    let style =
        tag.split("draw:style-name=\"").nth(1).unwrap_or_else(|| panic!("no style on {tag}")).split('"').next().unwrap_or_default().to_owned();
    let start = content.find(&format!("<style:style style:name=\"{style}\"")).unwrap_or_else(|| panic!("no style {style}"));
    content[start..].split("</style:style>").next().unwrap().to_owned()
}

#[test]
fn an_edit_of_a_cell_the_file_says_through_tier_2_is_written_into_it_and_the_rest_is_as_it_was() {
    let (bytes, id) = saved_with_tier2();
    let mut o = open_bytes(bytes.clone());
    {
        let shape = shape_mut(&mut o.doc, 0, id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("edited")));
        sheet.cells.insert(CellName::new("Added"), cell(Some("Width*3"), "9"));
    }
    let saved = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (before, after) = (part(&bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // Only the shape's style changed: the shape's element is as it was.
    let element =
        |c: &str| c.split("<draw:custom-shape draw:style-name=\"gr1\"").nth(1).unwrap().split("</draw:custom-shape>").next().unwrap().to_owned();
    assert_eq!(element(&before), element(&after));
    assert!(after.contains("edited") && !after.contains("mine"), "{after}");
    let back = open_bytes(saved.bytes.clone());
    let shape = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert_eq!(shape.sheet.cell("Comment").and_then(|c| c.value.as_ref()).map(Value::text), Some("edited"));
    assert_eq!(shape.sheet.cell("Added").and_then(|c| c.formula.text()), Some("Width*3"));
    // Saved again, it is no edit.
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited);
}

#[test]
fn a_section_added_to_a_shape_that_carries_no_tier_2_makes_its_own_style_carry_it() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    {
        let shape = shape_mut(&mut o.doc, 0, id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut prop = Section::new("Property", None);
        let mut row = Row { name: Some(Arc::from("Cost")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), cell(None, "12.5"));
        prop.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(prop));
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("n")));
        shape.name = Some(Arc::from("Renamed"));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let after = part(&saved.bytes, "content.xml");
    let style = style_of(&after, "s1");
    assert!(
        style.contains("diacraft:version=\"1\"") && style.contains(&format!("diacraft:id=\"{id}\"")) && style.contains("diacraft:s-Property="),
        "{style}"
    );
    let back = open_bytes(saved.bytes.clone());
    let shape = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!(shape.id, id);
    assert!(shape.sheet.section(&SectionKey::new("Property", None)).is_some());
    assert_eq!((shape.name.as_deref(), shape.name_u.as_deref()), (Some("Renamed"), Some("s1")));
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited);
}

#[test]
fn a_carried_pin_follows_a_move_of_the_shape_and_a_carried_text_follows_a_retext() {
    // A shape whose pin is not its centre carries `LocPinX`, `LocPinY`, `PinX` and `PinY`; a move writes the frame and the carried pin follows.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let h = 2.0 / 2.54;
        // The pin at the frame's lower left.
        sheet.cells.insert(CellName::new("LocPinX"), cell(None, "0"));
        sheet.cells.insert(CellName::new("LocPinY"), cell(None, "0"));
        sheet.cells.insert(CellName::new("PinX"), cell(None, &(2.0 / 2.54).to_string()));
        sheet.cells.insert(CellName::new("PinY"), cell(None, &((29.7 - 3.0) / 2.54 - h).to_string()));
    }
    shape.text = Some(Arc::new(Text { parts: vec![TextPart::Cp(1), TextPart::Chars(Arc::from("one"))], ..Default::default() }));
    push_shape(&mut o.doc, shape);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let mut again = open_bytes(first.bytes.clone());
    let (pin_was, y_was) = {
        let s = again.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
        (
            s.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap(),
            s.sheet.cell("PinY").and_then(|c| c.value.as_ref()?.number()).unwrap(),
        )
    };
    move_by(&mut again.doc, 0, id, 1.0, 0.5);
    shape_mut(&mut again.doc, 0, id).text =
        Some(Arc::new(Text { parts: vec![TextPart::Cp(1), TextPart::Chars(Arc::from("two words"))], ..Default::default() }));
    let saved = real(&again.doc, &first.bytes, &again.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = open_bytes(saved.bytes.clone());
    let s = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    let num = |c: &str| s.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((num("PinX") - pin_was - 1.0 / 2.54).abs() < 1e-6, "the carried pin moved: {pin_was} -> {}", num("PinX"));
    assert!((num("PinY") - y_was - 0.5 / 2.54).abs() < 1e-6);
    assert_eq!(num("LocPinX"), 0.0);
    let chars: String = s.text.as_ref().unwrap().parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect();
    assert_eq!(chars, "two words", "the carried text follows the retext");
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited);
}

/// A group's own data is the document file's, keyed by the page and the group's name and number (writes it): a cell added to a group
/// is carried in a `group` entry and read back with the group, a move of the group is written, the same file saved again is no edit, and a cell taken away takes the
/// entry (and the file) away. A change to a group's **name** is not written: the entry is found by the name, so a rename is refused in words and never dropped.
#[test]
fn a_change_to_the_cells_of_a_group_is_carried_by_the_document_file_and_a_rename_is_refused_in_words() {
    let group = concat!(
        r#"<draw:g draw:name="Grp">"#,
        r#"<draw:rect draw:name="in1" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#,
        r#"<draw:rect draw:name="in2" svg:x="4cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#,
        r#"</draw:g>"#
    );
    let o = open(&Drawing::a4().page(group));
    let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.kind == diacraft_doc::ShapeType::Group).map(|s| s.id).expect("a group");
    let mut doc = o.doc.clone();
    Arc::make_mut(&mut shape_mut(&mut doc, 0, id).sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("on a group")));
    let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a cell on a group was refused: {e}"));
    assert!(saved.edited);
    let file = part(&saved.bytes, "DiaCraft/document.xml");
    assert!(file.contains(r#"<diacraft:group page="page1" name="Grp">"#) && file.contains("Comment"), "{file}");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.kind == diacraft_doc::ShapeType::Group).unwrap().clone();
    assert_eq!(read.sheet.cell("Comment").and_then(|c| c.value.as_ref().map(|v| v.text().to_owned())).as_deref(), Some("on a group"));
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited, "what was read is saved as no edit");
    // A rename is refused, in words.
    let mut renamed = o.doc.clone();
    shape_mut(&mut renamed, 0, id).name = Some(Arc::from("Renamed"));
    match real(&renamed, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(words.contains("group") || words.contains("name"), "{words}"),
        other => panic!("expected a refusal in words: {other:?}"),
    }
    // A move of the group is no change of its own data: it is written.
    let mut moved = o.doc.clone();
    move_by(&mut moved, 0, id, 1.0, 0.0);
    assert!(real(&moved, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a moved group was refused: {e}")).edited);
}

/// A drawing of one custom shape `s1` on the automatic graphic style `gr1`, whose graphic properties hold `attrs`.
fn on_style(attrs: &str) -> Drawing {
    Drawing::a4()
        .auto(&format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#))
        .page(
            r#"<draw:custom-shape draw:style-name="gr1" draw:name="s1" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
        )
}

/// The attributes of the extension a style holds for one cell `Note` of value 1, with the shape's id 5 and `more`.
fn note_attrs(version: bool, more: &str) -> String {
    let cells = "&lt;Sheet&gt;&lt;Cell N=&quot;Note&quot; V=&quot;1&quot;/&gt;&lt;/Sheet&gt;";
    format!(r#"{} diacraft:id="5" diacraft:cells="{cells}" {more}"#, if version { r#"diacraft:version="1""# } else { "" })
}

/// The graphic style `gr1` of the saved drawing, as written.
fn gr1_of(saved: &[u8]) -> String {
    let content = part(saved, "content.xml");
    let start = content.find(r#"<style:style style:name="gr1""#).unwrap_or_else(|| panic!("no style gr1: {content}"));
    content[start..].split("</style:style>").next().unwrap_or_default().to_owned()
}

/// Saves the drawing with the shape's `Note` cell set by `edit` (`None` takes it out).
fn saved_with_note(drawing: &Drawing, edit: Option<&str>) -> diacraft_odg::Written {
    let mut o = open(drawing);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    assert_eq!(id, 5, "the shape takes the id its style carries");
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    match edit {
        Some(v) => drop(sheet.cells.insert(CellName::new("Note"), Cell::constant(Value::from_text(v)))),
        None => drop(sheet.cells.remove("Note")),
    }
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"))
}

/// The version attribute is `1` while any attribute of the extension is on the style: a style that holds the extension's attributes and no version gets it when
/// it is edited, and a cell taken out of what the style carries leaves the version for the name of what is gone.
#[test]
fn a_style_that_holds_attributes_and_no_version_gets_it_when_edited_and_a_cell_taken_out_is_gone() {
    let saved = saved_with_note(&on_style(&note_attrs(false, "")), Some("2"));
    let style = gr1_of(&saved.bytes);
    assert!(style.contains(r#"diacraft:version="1""#) && style.contains("V=&quot;2&quot;"), "{style}");
    let style = gr1_of(&saved_with_note(&on_style(&note_attrs(true, "")), None).bytes);
    assert!(style.contains(r#"diacraft:version="1""#) && style.contains(r#"diacraft:gone="c.Note""#) && !style.contains("diacraft:cells"), "{style}");
}

/// The attributes of the extension a style holds that the writer owns are decoded for an edit, and only those: a style that holds the version and a name the
/// writer does not own has nothing to decode, and one that holds a cell has one decode.
#[test]
fn a_style_is_decoded_for_an_edit_only_when_it_holds_an_attribute_the_writer_owns() {
    let decodes = |drawing: &Drawing| {
        let mut o = open(drawing);
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("c")));
        real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).work.tier2_decodes
    };
    assert_eq!(decodes(&on_style(r#"diacraft:version="1" diacraft:filled="1""#)), 0);
    assert_eq!(decodes(&on_style(&note_attrs(true, ""))), 1);
}
