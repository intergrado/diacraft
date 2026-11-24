//! Paragraphs put beside and into lists, lists the save makes, and the faces it declares (the review):
//! a paragraph that is no list's goes outside the list (the reader skips a `text:p`
//! that is a child of `text:list`), a paragraph with a list's row that no neighbour has is the item of a `text:list` the save makes, with an automatic list style,
//! and a run in a font the file does not declare gets the declaration added in the same file.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Shape, TextPart, Value};
use diacraft_odg::{Limits, WriteError, Written, read};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;
use crate::write_core::{Open, open, open_bytes, part, real, shape_mut};
use crate::write_runs::{chars, id_of, set_parts, set_row, shape};

const STYLES: &str = r#"<style:style style:name="P1" style:family="paragraph"/><text:list-style style:name="N1"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"#;

/// The list styles and the font faces of `content.xml`, for the messages of a test that fails.
fn styled(after: &str) -> String {
    let mut out = String::new();
    for (open, close) in [("<text:list-style", "</text:list-style>"), ("<office:font-face-decls", "</office:font-face-decls>")] {
        let mut rest = after;
        while let Some(at) = rest.find(open) {
            let end = rest[at..].find(close).map_or(rest.len(), |e| at + e + close.len());
            out.push_str(&rest[at..end]);
            rest = &rest[end..];
        }
    }
    out
}

/// A numbered list of two items: its paragraphs read with no bullet and no row.
const NUMBERED: &str = r#"<text:list text:style-name="N1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"#;

fn drawing_of(inner: &str) -> Drawing {
    Drawing::a4().auto(STYLES).page(&shape("x", "1cm", inner))
}

/// The characters and the paragraphs the file reads as, for the one shape `x`.
fn read_text(bytes: &[u8]) -> String {
    let (doc, _) = read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    let s = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("x")).unwrap();
    s.text.iter().flat_map(|t| t.parts.iter()).filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect()
}

/// Saves the model and saves it again twice: the first is an edit and the others are no edit and give its bytes.
#[track_caller]
fn saved_thrice(o: &Open) -> Written {
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    assert!(first.edited);
    let mut last = first.clone();
    for n in 2..=3 {
        let again = real(&o.doc, &last.bytes, &last.index).unwrap_or_else(|e| panic!("save {n} was refused: {e}"));
        assert!(!again.edited && again.bytes == last.bytes, "save {n} is no edit");
        last = again;
    }
    first
}

fn refusal(result: Result<Written, WriteError>) -> String {
    match result {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another error: {other}"),
        Ok(w) => panic!("the save was not refused: {}", part(&w.bytes, "content.xml")),
    }
}

/// A paragraph with a row of its own put after the last item of a numbered list, before its first, and (the shape being the second of two) in a file whose first
/// shape holds a list too: it is written outside the list, where the reader reads it, with every paragraph the model has, and saved again it is no edit.
#[test]
fn a_paragraph_that_is_no_lists_goes_outside_the_list_and_is_read_back() {
    // After the last item, and after both items.
    let mut o = open(&drawing_of(NUMBERED));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("new")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("</text:list><text:p text:style-name=\"P2\">new</text:p>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nnew", "{after}");
    // Before the first item.
    let mut o = open(&drawing_of(NUMBERED));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("one\ntwo")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<text:p text:style-name=\"P2\">new</text:p><text:list"), "{after}");
    assert_eq!(read_text(&saved.bytes), "new\none\ntwo", "{after}");
}

/// A paragraph that would fall between two items of a list, where neither item has its row, is refused in words: the specification does not say how a list is
/// split. One that has the row of the item before it is a new item of that list.
#[test]
fn a_paragraph_between_two_items_is_refused_unless_it_has_the_row_of_one() {
    let mut o = open(&drawing_of(NUMBERED));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("one\n"), TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("two")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("between two items"), "{words}");
    // The row of the neighbour: a new item of the list, between the two, and the reader reads three paragraphs.
    let mut o = open(&drawing_of(NUMBERED));
    set_parts(&mut o, vec![chars("one\nnew\ntwo")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list-item>").count(), 3, "{after}");
    assert_eq!(read_text(&saved.bytes), "one\nnew\ntwo");
}

/// Two shapes that hold lists, a paragraph put into the list of each: each goes beside the items of its own shape's list (the places a kid names another by are
/// of the part's list, and the second shape's kids do not begin at 0).
#[test]
fn a_list_in_the_second_shape_takes_a_new_item_in_its_own_list() {
    let body = format!("{}{}", shape("x", "1cm", NUMBERED), shape("y", "6cm", NUMBERED));
    let mut o = open(&Drawing::a4().auto(STYLES).page(&body));
    for name in ["x", "y"] {
        let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap().id;
        shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(diacraft_doc::Text { parts: vec![chars("one\ntwo\nthree")], ..Default::default() }));
    }
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    for (name, other) in [("x", "y"), ("y", "")] {
        let from = after.find(&format!("draw:name=\"{name}\"")).unwrap();
        let to = if other.is_empty() { after.len() } else { after.find(&format!("draw:name=\"{other}\"")).unwrap() };
        assert_eq!(after[from..to].matches("<text:list-item>").count(), 3, "{name}: a new item in its own list: {after}");
    }
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    for shape in doc.pages.get(0).unwrap().shapes.iter() {
        let text: String = shape
            .text
            .iter()
            .flat_map(|t| t.parts.iter())
            .filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None })
            .collect();
        assert_eq!(text, "one\ntwo\nthree");
    }
}

/// The bullet rows of a paragraph of the model that no neighbour has: the item of a `text:list` with an automatic list style, named `L` and a number.
pub(crate) fn bullet_row(o: &mut Open, ix: u32, cells: &[(&str, f64)], words: &[(&str, &str)]) {
    set_row(o, "Paragraph", ix, cells);
    let id = id_of(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Paragraph").unwrap();
    let section = Arc::make_mut(section);
    let row = section.rows.iter_mut().find(|r| r.ix == Some(ix)).unwrap();
    let row = Arc::make_mut(row);
    for (name, text) in words {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(text)));
    }
}

/// The `Paragraph` cells of the row that holds `Bullet` in the shape the file reads, as a list of (cell, text) pairs, or nothing.
fn bullet_cells(bytes: &[u8]) -> Vec<(String, String)> {
    let (doc, _) = read(bytes, &Limits::default()).unwrap();
    let s: &Shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("x")).unwrap();
    let mut out = Vec::new();
    for section in s.sheet.sections.iter().filter(|s| s.name.as_str() == "Paragraph") {
        for row in section.rows.iter().filter(|r| r.cells.contains_key("Bullet")) {
            for (name, cell) in &row.cells {
                out.push((name.as_str().to_owned(), cell.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()));
            }
        }
    }
    out.sort();
    out
}

/// Two paragraphs with `Bullet` 1 and a third with none, after a paragraph of the file: one `text:list` holding the first two as `text:list-item`s, whose
/// list style's level 1 has `text:bullet-char="•"`, and the third as a plain `text:p` after the list.
#[test]
fn two_bulleted_paragraphs_and_a_plain_one_are_one_list_and_a_paragraph() {
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0)]);
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("b1\nb2\n"), TextPart::Pp(0), chars("plain")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list ").count(), 1, "one list: {after}");
    assert!(
        after.contains("<text:p>zero</text:p><text:list text:style-name=\"L2\"><text:list-item><text:p>b1</text:p></text:list-item><text:list-item><text:p>b2</text:p></text:list-item></text:list><text:p>plain</text:p>"),
        "{after}"
    );
    assert!(
        after.contains(
            "<text:list-style style:name=\"L2\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"\u{2022}\"></text:list-level-style-bullet></text:list-style>"
        ),
        "{}",
        styled(&after)
    );
    assert_eq!(read_text(&saved.bytes), "zero\nb1\nb2\nplain");
    assert!(bullet_cells(&saved.bytes).contains(&("Bullet".to_owned(), "1".to_owned())), "the paragraphs read as a list's");
}

/// The bullet's own cells are the list style's: the character (`BulletStr`), the face and the size of the bullet (the face declared in the file when it was not),
/// the margin and the indent; the paragraph's own cells stay the paragraph style's.
#[test]
fn the_cells_of_a_bullet_are_the_list_style_and_read_back_as_they_were() {
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    bullet_row(
        &mut o,
        1,
        &[("Bullet", 1.0), ("IndLeft", 0.3), ("IndFirst", -0.3), ("BulletFontSize", 10.0 / 72.0), ("HorzAlign", 2.0)],
        &[("BulletStr", "\u{2013}"), ("BulletFont", "Wingdings")],
    );
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("text:bullet-char=\"\u{2013}\""), "{}", styled(&after));
    assert!(after.contains("style:font-name=\"Wingdings\"") && after.contains("fo:font-size=\"0.35277777778cm\""), "{}", styled(&after));
    assert!(
        after.contains("<style:font-face style:name=\"Wingdings\" svg:font-family=\"'Wingdings'\"/>"),
        "the face is declared in the file: {after}"
    );
    assert!(after.contains("fo:text-indent=\"-0.762cm\"") && after.contains("fo:margin-left=\"0.762cm\""), "{after}");
    assert!(after.contains("fo:text-align=\"right\""), "the paragraph's own cell is the paragraph style's: {after}");
    let cells = bullet_cells(&saved.bytes);
    for (name, value) in [("Bullet", "1"), ("BulletStr", "\u{2013}"), ("BulletFont", "Wingdings"), ("HorzAlign", "2")] {
        assert!(cells.contains(&(name.to_owned(), value.to_owned())), "{name}: {cells:?}");
    }
}

/// A paragraph of the file that is no list's and is given a list's row is written inside a list this save makes.
#[test]
fn a_plain_paragraph_given_a_list_row_is_wrapped_in_a_list() {
    let mut o = open(&drawing_of("<text:p>ab</text:p><text:p>cd</text:p>"));
    set_row(&mut o, "Paragraph", 0, &[("Bullet", 1.0)]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list ").count(), 2, "{after}");
    assert!(after.contains("<text:list text:style-name=\"L2\"><text:list-item><text:p>ab</text:p></text:list-item></text:list>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "ab\ncd");
    assert!(bullet_cells(&saved.bytes).contains(&("Bullet".to_owned(), "1".to_owned())));
}

/// A new paragraph beside a list item of another row goes after the list, and the same two paragraphs in a list of the same row (the engine's own sequence:
/// typing a line feed at the end of a list and formatting the new paragraph) are written whole.
#[test]
fn the_paragraph_typed_after_a_bulleted_list_and_formatted_is_a_list_of_its_own() {
    let mut o = open(&drawing_of(r#"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item></text:list>"#));
    set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0), ("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list ").count(), 2, "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo", "{after}");
}

fn run_with_font(family: &str) -> Open {
    let mut o = open(&drawing_of("<text:p>plain</text:p>"));
    let id = id_of(&o.doc);
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = Section::new("Character", None);
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text(family)));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    }
    set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
    // The engine adds the name to the document's table of faces when it gives a run a font.
    Arc::make_mut(&mut o.doc.fonts).push(diacraft_doc::FaceName { name_u: Arc::from(family), raw: Default::default(), ext: Default::default() });
    o
}

/// The last sentence: a face the file does not declare is declared, in the same file's `office:font-face-decls`, and the text style names it. Whether the
/// element is empty, holds other faces, or is not there.
#[test]
fn a_font_the_file_does_not_declare_is_declared_beside_the_style_that_names_it() {
    // The element is there, with no face.
    let o = run_with_font("Nowhere Sans");
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(
            "<office:font-face-decls><style:font-face style:name=\"Nowhere Sans\" svg:font-family=\"'Nowhere Sans'\"/></office:font-face-decls>"
        ),
        "{after}"
    );
    assert!(after.contains("style:font-name=\"Nowhere Sans\""), "{after}");
    // The element is an empty element.
    let d = drawing_of("<text:p>plain</text:p>");
    let bytes = d.bytes();
    let content = part(&bytes, "content.xml").replace("<office:font-face-decls></office:font-face-decls>", "<office:font-face-decls/>");
    let mut files = d.files();
    for (name, data) in &mut files {
        if name == "content.xml" {
            *data = content.clone().into_bytes();
        }
    }
    let mut o = open_bytes(package(DRAWING, files));
    let id = id_of(&o.doc);
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = Section::new("Character", None);
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text("Nowhere")));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    }
    set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains("<office:font-face-decls><style:font-face style:name=\"Nowhere\" svg:font-family=\"'Nowhere'\"/></office:font-face-decls>"),
        "{after}"
    );
    // There is no such element: it is made before `office:automatic-styles`.
    let content = part(&bytes, "content.xml").replace("<office:font-face-decls></office:font-face-decls>", "");
    let mut files = d.files();
    for (name, data) in &mut files {
        if name == "content.xml" {
            *data = content.clone().into_bytes();
        }
    }
    let mut o = open_bytes(package(DRAWING, files));
    let id = id_of(&o.doc);
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = Section::new("Character", None);
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text("Nowhere")));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    }
    set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains("<office:font-face-decls><style:font-face style:name=\"Nowhere\" svg:font-family=\"'Nowhere'\"/></office:font-face-decls><office:automatic-styles>"),
        "{after}"
    );
    // A family that cannot be a name is refused in words.
    let o = run_with_font("bad\u{1}family");
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("font face"), "{words}");
}

/// Two families in one save are two declarations in the order they were made; a family twice is declared once; a name that another family has in the file is
/// not taken (the declaration gets a number).
#[test]
fn the_faces_a_save_declares_are_once_each_and_named_apart_from_the_file_s() {
    let mut d = drawing_of("<text:p>plain</text:p>");
    d.fonts = r#"<style:font-face style:name="Arial" svg:font-family="Helvetica"/>"#.to_owned();
    let mut o = open(&d);
    let id = id_of(&o.doc);
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = Section::new("Character", None);
        for (ix, family) in [(1u32, "Arial"), (2, "Zapf"), (3, "Zapf")] {
            let mut row = Row { ix: Some(ix), ..Row::default() };
            row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text(family)));
            row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_number(f64::from(ix) / 10.0).unwrap()));
            section.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(section));
    }
    set_parts(&mut o, vec![TextPart::Cp(1), chars("a"), TextPart::Cp(2), chars("b"), TextPart::Cp(3), chars("c")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(
            "<office:font-face-decls><style:font-face style:name=\"Arial\" svg:font-family=\"Helvetica\"/><style:font-face style:name=\"Arial 2\" svg:font-family=\"'Arial'\"/><style:font-face style:name=\"Zapf\" svg:font-family=\"'Zapf'\"/></office:font-face-decls>"
        ),
        "{after}"
    );
    assert_eq!(after.matches("<style:font-face ").count(), 3, "Zapf is declared once");
}

/// A family with a single quote in it is declared between double quotes (as references in the attribute), and reads back as the family (the fuzz target's finding:
/// the quotes were written raw and the part made did not parse).
#[test]
fn a_family_with_a_single_quote_is_declared_and_reads_back() {
    let o = run_with_font("O'Neil Sans");
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<style:font-face style:name=\"O'Neil Sans\" svg:font-family=\"&quot;O'Neil Sans&quot;\"/>"), "{}", styled(&after));
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("x")).unwrap();
    let fonts: Vec<String> = shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == "Character")
        .flat_map(|s| s.rows.iter())
        .filter_map(|r| r.cells.get("Font").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
        .collect();
    assert!(fonts.iter().any(|f| f == "O'Neil Sans"), "{fonts:?}");
}

/// A paragraph with a row of its own that no neighbour has, put between two paragraphs that are not items of one list (two plain ones, an item and a plain one, two
/// lists), is written between them and outside any list; only between two items of one list is it refused (the mutation of `insert_para`).
#[test]
fn a_paragraph_between_paragraphs_that_are_not_of_one_list_is_put_between_them() {
    let rows = |o: &mut Open| set_row(o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    // Two plain paragraphs.
    let mut o = open(&drawing_of("<text:p>ab</text:p><text:p>cd</text:p>"));
    rows(&mut o);
    set_parts(&mut o, vec![chars("ab\n"), TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("cd")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<text:p>ab</text:p><text:p text:style-name=\"P2\">new</text:p><text:p>cd</text:p>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "ab\nnew\ncd");
    // The last item of a list, and a plain paragraph.
    let mut o = open(&drawing_of(&format!("{NUMBERED}<text:p>tail</text:p>")));
    rows(&mut o);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("tail")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("</text:list><text:p text:style-name=\"P2\">new</text:p><text:p>tail</text:p>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nnew\ntail");
    // A plain paragraph, and the first item of a list.
    let mut o = open(&drawing_of(&format!("<text:p>head</text:p>{NUMBERED}")));
    rows(&mut o);
    set_parts(&mut o, vec![chars("head\n"), TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("one\ntwo")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<text:p>head</text:p><text:p text:style-name=\"P2\">new</text:p><text:list"), "{after}");
    assert_eq!(read_text(&saved.bytes), "head\nnew\none\ntwo");
    // The last item of one list and the first of another.
    let mut o = open(&drawing_of(&format!("{NUMBERED}{NUMBERED}")));
    rows(&mut o);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("new\n"), TextPart::Pp(0), chars("one\ntwo")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("</text:list><text:p text:style-name=\"P2\">new</text:p><text:list"), "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nnew\none\ntwo");
}

/// Two paragraphs put in a row, each with a list's row but not the same one, are the items of two lists, each with the list style its own row asks for (V20
/// merges items of one row only; the mutation of `insert_para`).
#[test]
fn bulleted_paragraphs_in_a_row_with_rows_that_differ_are_lists_of_their_own() {
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0)]);
    set_row(&mut o, "Paragraph", 2, &[("Bullet", 2.0)]);
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("b1\n"), TextPart::Pp(2), chars("b2")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list ").count(), 2, "{after}");
    assert!(
        after.contains("<text:list text:style-name=\"L2\"><text:list-item><text:p>b1</text:p></text:list-item></text:list><text:list text:style-name=\"L3\"><text:list-item><text:p>b2</text:p></text:list-item></text:list>"),
        "{after}"
    );
    let styles = styled(&after);
    assert!(styles.contains("text:bullet-char=\"\u{2022}\"") && styles.contains("text:bullet-char=\"\u{25C6}\""), "{styles}");
    let cells = bullet_cells(&saved.bytes);
    assert!(cells.contains(&("Bullet".to_owned(), "1".to_owned())) && cells.contains(&("Bullet".to_owned(), "2".to_owned())), "{cells:?}");
}

/// A bullet's face written with quotes, as a family is in `svg:font-family` (single or double), is declared by the family: the quotes are no part of the name
/// (the mutation of `list_style`).
#[test]
fn a_bullet_font_with_quotes_is_declared_by_its_family() {
    for quoted in ["'Wingdings'", "\"Wingdings\"", " 'Wingdings' ", "Wingdings"] {
        let mut o = open(&drawing_of("<text:p>zero</text:p>"));
        bullet_row(&mut o, 1, &[("Bullet", 1.0)], &[("BulletFont", quoted)]);
        set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
        let saved = saved_thrice(&o);
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains("<style:font-face style:name=\"Wingdings\" svg:font-family=\"'Wingdings'\"/>"), "{quoted}: {after}");
        assert!(after.contains("style:font-name=\"Wingdings\""), "{quoted}: {after}");
    }
}

/// The paragraphs of the model's text, each with the row of the `Paragraph` section in force (`None` before the first marker), as `(row, text)`.
fn paragraphs_of(o: &Open) -> Vec<(Option<u32>, String)> {
    let shape = crate::write_runs::the_shape(&o.doc);
    let mut row: Option<u32> = None;
    let mut out: Vec<(Option<u32>, String)> = vec![(None, String::new())];
    for p in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match p {
            TextPart::Pp(ix) => {
                row = Some(*ix);
                if let Some(last) = out.last_mut() {
                    last.0 = row;
                }
            }
            TextPart::Chars(c) => {
                for (k, piece) in c.split('\n').enumerate() {
                    if k > 0 {
                        out.push((row, String::new()));
                    }
                    if let Some(last) = out.last_mut() {
                        last.1.push_str(piece);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Two paragraphs put in a row after a list (the reviewer's engine sequence: a line feed and two lines typed at the end of a
/// numbered list, the first of them formatted): the first has a row of its own, so it goes after the list; the second has the row of the list's items, so it
/// would be a new item of the list, which is before the first. They are written in the model's order, both after the list, and the same two characters in both
/// (the case that read back as itself with the paragraphs swapped, and said nothing).
#[test]
fn two_paragraphs_put_after_a_list_are_written_in_the_models_order() {
    let mut o = open(&drawing_of(NUMBERED));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("x\n"), TextPart::Pp(0), chars("x")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("</text:list><text:p text:style-name=\"P2\">x</text:p><text:p>x</text:p>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nx\nx");
    // The texts differ, and the order is the same.
    let mut o = open(&drawing_of(NUMBERED));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("A\n"), TextPart::Pp(0), chars("B")]);
    let saved = saved_thrice(&o);
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nA\nB");
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("</text:list><text:p text:style-name=\"P2\">A</text:p><text:p>B</text:p>"), "{after}");
}

/// The mirror: two paragraphs put before a list, the first of the row of the list's items (a new item of the list, at its start), the second of a row of its
/// own: that one would be put before the list, ahead of the first. It would fall between two items of one list, and is refused in words.
#[test]
fn two_paragraphs_put_before_a_list_that_would_swap_are_refused_in_words() {
    let styles = format!(
        "{STYLES}<style:style style:name=\"P3\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"center\"/></style:style>"
    );
    let list = r#"<text:list text:style-name="N1"><text:list-item><text:p text:style-name="P3">one</text:p></text:list-item><text:list-item><text:p text:style-name="P3">two</text:p></text:list-item></text:list>"#;
    let mut o = open(&Drawing::a4().auto(&styles).page(&shape("x", "1cm", &format!("<text:p>head</text:p>{list}"))));
    let paras = paragraphs_of(&o);
    let centred = paras[1].0.expect("the items are centred, by a row");
    set_row(&mut o, "Paragraph", 20, &[("HorzAlign", 2.0)]);
    // head, a paragraph of the items' row, a paragraph of a row of its own, one, two.
    set_parts(
        &mut o,
        vec![chars("head\n"), TextPart::Pp(centred), chars("A\n"), TextPart::Pp(20), chars("B\n"), TextPart::Pp(centred), chars("one\ntwo")],
    );
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("between two items"), "{words}");
    // In the other order the row of its own is first, outside the list, and the item of the list after it is the new item of the list: written in the order.
    let mut o = open(&Drawing::a4().auto(&styles).page(&shape("x", "1cm", &format!("<text:p>head</text:p>{list}"))));
    set_row(&mut o, "Paragraph", 20, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("head\n"), TextPart::Pp(20), chars("B\n"), TextPart::Pp(centred), chars("A\none\ntwo")]);
    let saved = saved_thrice(&o);
    assert_eq!(read_text(&saved.bytes), "head\nB\nA\none\ntwo");
}

/// A bullet's face that is the number 0, or empty, or the text 0 is the document's own face: no face is declared (a face named `0` was), and the list style
/// names none. Another number is a place in a table an `.odg` has not, and is refused in words (the re-review, parked 5).
#[test]
fn a_bullet_font_of_0_or_nothing_declares_no_face_and_another_number_is_refused() {
    for (number, words) in [(Some(0.0), None), (None, Some("0")), (None, Some("")), (None, Some("  "))] {
        let mut o = open(&drawing_of("<text:p>zero</text:p>"));
        let cells: Vec<(&str, f64)> = [("Bullet", 1.0)].into_iter().chain(number.map(|n| ("BulletFont", n))).collect();
        let words: Vec<(&str, &str)> = words.map(|w| ("BulletFont", w)).into_iter().collect();
        bullet_row(&mut o, 1, &cells, &words);
        set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
        let saved = saved_thrice(&o);
        let after = part(&saved.bytes, "content.xml");
        assert!(!after.contains("style:name=\"0\"") && !after.contains("style:font-name"), "{number:?} {words:?}: {after}");
        assert_eq!(after.matches("<style:font-face ").count(), 0, "{after}");
    }
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    bullet_row(&mut o, 1, &[("Bullet", 1.0), ("BulletFont", 3.0)], &[]);
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("number of the document"), "{words}");
}

/// A bullet is one character (`text:bullet-char`): a `BulletStr` of more than one is refused in words when the list is written, where LibreOffice would
/// keep the first. A list of the file whose bullet is longer, left as it is, does not stop an edit elsewhere in the text.
#[test]
fn a_bullet_string_of_more_than_one_character_is_refused_when_the_list_is_written() {
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    bullet_row(&mut o, 1, &[("Bullet", 1.0)], &[("BulletStr", "ab")]);
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("one character"), "{words}");
    // One character, an en dash, is a bullet.
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    bullet_row(&mut o, 1, &[("Bullet", 1.0)], &[("BulletStr", "\u{2013}")]);
    set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
    assert!(saved_thrice(&o).edited);
    // The file's own list with a bullet of two characters: an edit of the text elsewhere in the shape is written and the list is left.
    let list = r#"<text:list text:style-name="LL"><text:list-item><text:p>item</text:p></text:list-item></text:list>"#;
    let styles = format!(
        "{STYLES}<text:list-style style:name=\"LL\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"ab\"/></text:list-style>"
    );
    let mut o = open(&Drawing::a4().auto(&styles).page(&shape("x", "1cm", &format!("<text:p>zero</text:p>{list}"))));
    let paras = paragraphs_of(&o);
    assert_eq!(paras.len(), 2);
    let row = paras[1].0.expect("the list's row");
    set_parts(&mut o, vec![chars("zero!\n"), TextPart::Pp(row), chars("item")]);
    let saved = saved_thrice(&o);
    assert_eq!(read_text(&saved.bytes), "zero!\nitem");
}

/// Two paragraphs put in at one place that both have the row of the list's items are two new items of the list, in the model's order, whether they follow the
/// last item or precede the first (the places they have are the same, which is no paragraph put in before one already put in; the mutation of `insert_para`).
#[test]
fn two_paragraphs_with_the_row_of_the_items_are_two_new_items_in_the_models_order() {
    let mut o = open(&drawing_of(NUMBERED));
    set_parts(&mut o, vec![chars("one\ntwo\nthree\nfour")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list-item>").count(), 4, "{after}");
    assert!(after.contains("<text:p>two</text:p></text:list-item><text:list-item><text:p>three</text:p></text:list-item><text:list-item><text:p>four</text:p></text:list-item></text:list>"), "{after}");
    assert_eq!(read_text(&saved.bytes), "one\ntwo\nthree\nfour");
    // Before the first item.
    let mut o = open(&drawing_of(NUMBERED));
    set_parts(&mut o, vec![chars("up\ndown\none\ntwo")]);
    let saved = saved_thrice(&o);
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list-item>").count(), 4, "{after}");
    assert_eq!(read_text(&saved.bytes), "up\ndown\none\ntwo");
    // In the middle: the plan puts the two between the items.
    let mut o = open(&drawing_of(NUMBERED));
    set_parts(&mut o, vec![chars("one\nbetween\nalso\ntwo")]);
    let saved = saved_thrice(&o);
    assert_eq!(read_text(&saved.bytes), "one\nbetween\nalso\ntwo");
    assert_eq!(part(&saved.bytes, "content.xml").matches("<text:list-item>").count(), 4);
}
