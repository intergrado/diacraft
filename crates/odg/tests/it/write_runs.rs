//! Retext across formatted runs, and adding or removing a paragraph ( retext):
//! the edited paragraphs are written from the model's text and its `Character` and `Paragraph` rows, a run of a row
//! is a span of an automatic text style, a paragraph of a row has an automatic paragraph style, a style the part holds with the same attributes is used and a
//! new one is named by R27, a paragraph the edit did not change keeps its bytes, and a list takes a new item.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, Text, TextPart, Value};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{Case, Open, Saver, compare, content_changes, echo_control, open, open_bytes, part, real, shape_mut};

const STYLES: &str = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"##;

/// A custom shape holding `inner` as its text.
pub(crate) fn shape(name: &str, x: &str, inner: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{inner}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    )
}

/// A centred paragraph `ab` then a bold span `cd`, and a second centred paragraph `ef`.
const V15: &str = r#"<text:p text:style-name="P1">ab<text:span text:style-name="T1">cd</text:span></text:p><text:p text:style-name="P1">ef</text:p>"#;

fn drawing_of(inner: &str) -> Drawing {
    Drawing::a4().auto(STYLES).page(&shape("x", "1cm", inner))
}

pub(crate) fn id_of(doc: &Document) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("x")).unwrap().id
}

/// The model's text as characters with the markers named.
pub(crate) fn show(shape: &Shape) -> String {
    shape.text.as_ref().map_or(String::new(), |t| {
        t.parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) => c.to_string(),
                TextPart::Cp(i) => format!("<cp{i}>"),
                TextPart::Pp(i) => format!("<pp{i}>"),
                _ => "?".to_owned(),
            })
            .collect()
    })
}

pub(crate) fn chars_of(shape: &Shape) -> String {
    shape
        .text
        .as_ref()
        .map_or(String::new(), |t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect())
}

pub(crate) fn the_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("x")).unwrap()
}

/// Sets the parts of the shape's text.
pub(crate) fn set_parts(o: &mut Open, parts: Vec<TextPart>) {
    let id = id_of(&o.doc);
    shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
}

pub(crate) fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

/// The number a cell of row `ix` of the shape's `section` holds.
pub(crate) fn row_cell(shape: &Shape, section: &str, ix: u32, cell: &str) -> Option<f64> {
    let s = shape.sheet.sections.iter().find(|s| s.name.as_str() == section)?;
    let r = s.rows.iter().find(|r| r.ix == Some(ix))?;
    r.cells.get(cell)?.value.as_ref()?.number()
}

/// What each character of the shape's text has of a `Character` cell: for each run (by the row in force), the cell's value, as `(characters, value)`.
fn runs_of(shape: &Shape, cell: &str) -> Vec<(String, Option<f64>)> {
    let mut out: Vec<(String, Option<f64>)> = Vec::new();
    let mut row = 0u32;
    for p in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match p {
            TextPart::Cp(ix) => row = *ix,
            TextPart::Chars(c) => out.push((c.to_string(), row_cell(shape, "Character", row, cell))),
            _ => {}
        }
    }
    out
}

/// What the reader gives: the text and its markers, so that the edits below are written against the model as it is.
#[test]
fn reads_as_the_spec_says() {
    let o = open(&drawing_of(V15));
    let s = the_shape(&o.doc);
    assert_eq!(show(s), "<pp0>ab<cp1>cd\n<cp0>ef");
    assert_eq!(row_cell(s, "Paragraph", 0, "HorzAlign"), Some(1.0));
    assert_eq!(row_cell(s, "Character", 1, "Style"), Some(1.0));
}

/// An edit across the boundary of a run (`bc` over `ab`'s b and the span's c), which the writer used to refuse: the characters join the run of the first one
/// replaced, the span's marker stays, and the paragraph is written again whole with its two runs. The other paragraph keeps its bytes.
fn across_a_run_case(save: &Saver) {
    let mut o = open(&drawing_of(V15));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("aX"), TextPart::Cp(1), chars("d\n"), TextPart::Cp(0), chars("ef")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    // The second paragraph is as it was, byte for byte; the first has its style and a span of the bold style for `d`.
    assert!(after.contains("<text:p text:style-name=\"P1\">ef</text:p>"), "{after}");
    // The span names the style the file's own span named for that row (T1), and no other text style was made.
    assert!(after.contains("<text:p text:style-name=\"P1\">aX<text:span text:style-name=\"T1\">d</text:span></text:p>"), "{after}");
    assert!(!after.contains("style:name=\"T2\"") && after.contains("style:name=\"T1\""), "{after}");
    // Read back: the text is the model's, and `d` is bold, `aX` and `ef` are not.
    let back = open_bytes(saved.bytes.clone());
    let read = the_shape(&back.doc);
    assert_eq!(chars_of(read), "aXd\nef");
    let bold = runs_of(read, "Style");
    let bold_chars: String = bold.iter().filter(|(_, v)| v.is_some_and(|v| (v as u32) & 1 != 0)).map(|(c, _)| c.as_str()).collect();
    assert_eq!(bold_chars.trim_end_matches('\n'), "d", "{bold:?}: {}", show(read));
    // Only the shape (its paragraph and the style that holds its frame) changed: no style but that one was made or taken out.
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert!(changed.iter().all(|(path, _)| path.contains("draw:custom-shape")), "{diff:?}");
    assert_eq!((added, removed), (1, 0), "{diff:?}");
    // Saved again, it is no edit.
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A character typed at the start of a run (the boundary), which the writer used to refuse.
fn at_a_boundary_case(save: &Saver) {
    let mut o = open(&drawing_of(V15));
    // `X` typed at the start of the bold run: it joins the run before the marker, as the engine's rule puts a point after the character before it.
    set_parts(&mut o, vec![TextPart::Pp(0), chars("abX"), TextPart::Cp(1), chars("cd\n"), TextPart::Cp(0), chars("ef")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), "abXcd\nef");
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("abX<text:span"), "{after}");
    // And at the start of the bold run, inside it: `Xcd`.
    let mut o = open(&drawing_of(V15));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(1), chars("Xcd\n"), TextPart::Cp(0), chars("ef")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("\">Xcd</text:span></text:p>"), "{after}");
}

/// A paragraph added (a line feed typed in the middle of a paragraph) and removed.
fn paragraphs_case(save: &Saver) {
    // `ab` | `cd`: the line feed typed between them: the paragraph splits in two, the first keeps the element, the second is new and has the first's style.
    let mut o = open(&drawing_of(V15));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("ab\n"), TextPart::Cp(1), chars("cd\n"), TextPart::Cp(0), chars("ef")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), "ab\ncd\nef", "{after}");
    assert_eq!(after.matches("<text:p ").count(), 3, "three paragraphs: {after}");
    assert!(after.contains("<text:p text:style-name=\"P1\">ef</text:p>"), "the last paragraph is as it was: {after}");
    // The new paragraph has the centred paragraph style, as every paragraph of the model has row 0.
    assert_eq!(after.matches("text:style-name=\"P1\"").count(), 3, "{after}");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // A paragraph removed: the second paragraph's characters and the line feed before it taken out.
    let mut o = open(&drawing_of(V15));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(1), chars("cd")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:p ").count(), 1, "{after}");
    assert_eq!(chars_of(the_shape(&open_bytes(saved.bytes).doc)), "abcd");
}

/// Puts row `ix` of `section` of the shape's sheet, with `cells` (numbers), replacing the row that has the `IX`.
pub(crate) fn set_row(o: &mut Open, section: &str, ix: u32, cells: &[(&str, f64)]) {
    let id = id_of(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let at = sheet.sections.iter().position(|s| s.name.as_str() == section);
    let at = match at {
        Some(at) => at,
        None => {
            sheet.sections.push(Arc::new(Section::new(section, None)));
            sheet.sections.len() - 1
        }
    };
    let sec = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
    let mut row = Row { ix: Some(ix), ..Row::default() };
    for (name, v) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(*v).unwrap()));
    }
    match sec.rows.iter().position(|r| r.ix == Some(ix)) {
        Some(k) => sec.rows[k] = Arc::new(row),
        None => sec.rows.push(Arc::new(row)),
    }
}

/// The whole label replaced (`shape.set_text`: one run of the text and no marker): the formatted run goes with its style, the paragraph that had no place
/// goes, and the paragraph that stays has the style its row (row 0) gives it. Nothing is left that names the span's style.
fn whole_label_case(save: &Saver) {
    let mut o = open(&drawing_of(V15));
    set_parts(&mut o, vec![chars("Q1234567")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains("<text:p text:style-name=\"P1\">Q1234567</text:p><draw:enhanced-geometry"),
        "one paragraph, in the centred style: {after}"
    );
    assert!(!after.contains("text:span") && !after.contains(">ef<"), "{after}");
    assert!(!after.contains("style:name=\"T1\""), "the style only the span named is taken out: {after}");
    assert!(after.contains("style:name=\"P1\""), "{after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), "Q1234567");
    assert_eq!(row_cell(the_shape(&back.doc), "Paragraph", 0, "HorzAlign"), Some(1.0));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    let third = save(&o.doc, &again.bytes, &again.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes);
}

/// A row of paragraph cells edited: every paragraph that has the row is written with the style the new row asks for, one new style for both, and the style
/// nothing names now is taken out. The same for a row of a run.
fn row_cell_case(save: &Saver) {
    let mut o = open(&drawing_of(V15));
    set_row(&mut o, "Paragraph", 0, &[("HorzAlign", 2.0)]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("text:style-name=\"P2\"").count(), 2, "both paragraphs have the one new style: {after}");
    assert!(
        after.contains(
            "<style:style style:name=\"P2\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"right\"/></style:style>"
        ),
        "{after}"
    );
    assert!(!after.contains("style:name=\"P1\""), "P1 is named by nothing now: {after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(row_cell(the_shape(&back.doc), "Paragraph", 0, "HorzAlign"), Some(2.0));
    assert_eq!(chars_of(the_shape(&back.doc)), "abcd\nef");
    // A run's row: the bold run's size changed. Its span has a style of the new attributes.
    let mut o = open(&drawing_of(V15));
    set_row(&mut o, "Character", 1, &[("Style", 1.0), ("Size", 20.0 / 72.0)]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("fo:font-size=\"20pt\""), "{after}");
    let back = open_bytes(saved.bytes.clone());
    let sizes = runs_of(the_shape(&back.doc), "Size");
    assert!(sizes.iter().any(|(c, v)| c.starts_with("cd") && v.is_some_and(|v| (v * 72.0 - 20.0).abs() < 1e-6)), "{sizes:?}");
}

/// A row with `Style` 3 and `Size` 14 pt is `fo:font-weight="bold"`, `fo:font-style="italic"` and `fo:font-size="14pt"` (and no underline: bit 4 is the
/// underline, and the italic is bit 2), a paragraph row with `HorzAlign` 1 and `SpLine` -1.2 is `fo:text-align="center"` and `fo:line-height="120%"`; read back
/// they are the cells.
fn case_19(save: &Saver) {
    let mut o = open(&drawing_of("<text:p>plain</text:p>"));
    set_row(&mut o, "Character", 1, &[("Style", 3.0), ("Size", 14.0 / 72.0)]);
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 1.0), ("SpLine", -1.2)]);
    set_parts(&mut o, vec![TextPart::Pp(1), TextPart::Cp(1), chars("styled")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    for attr in [
        "fo:font-weight=\"bold\"",
        "fo:font-style=\"italic\"",
        "fo:font-size=\"14pt\"",
        "style:text-underline-style=\"none\"",
        "fo:text-align=\"center\"",
        "fo:line-height=\"120%\"",
    ] {
        assert!(after.contains(attr), "{attr} in {after}");
    }
    let back = open_bytes(saved.bytes.clone());
    let read = the_shape(&back.doc);
    assert_eq!(chars_of(read), "styled");
    assert_eq!(runs_of(read, "Style").iter().map(|(_, v)| *v).collect::<Vec<_>>(), [Some(3.0)]);
    let sizes = runs_of(read, "Size");
    assert!(sizes[0].1.is_some_and(|v| (v * 72.0 - 14.0).abs() < 1e-6), "{sizes:?}");
    let pp = |cell: &str| {
        let mut row = None;
        for p in read.text.iter().flat_map(|t| t.parts.iter()) {
            if let TextPart::Pp(ix) = p {
                row = Some(*ix);
            }
        }
        row.and_then(|ix| row_cell(read, "Paragraph", ix, cell))
    };
    assert_eq!((pp("HorzAlign"), pp("SpLine")), (Some(1.0), Some(-1.2)));
}

/// A style the part already holds with exactly the attributes a row asks for is used, and none is made; one with other attributes, or a parent, is not.
fn existing_style_case(save: &Saver) {
    let exact = r#"<style:style style:name="T9" style:family="text"><style:text-properties fo:font-size="14pt" fo:font-weight="bold" fo:font-style="italic" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal" style:text-underline-style="none"/></style:style>"#;
    let close = r#"<style:style style:name="T8" style:family="text" style:parent-style-name="T1"><style:text-properties fo:font-size="14pt" fo:font-weight="bold" fo:font-style="italic" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal" style:text-underline-style="none"/></style:style>"#;
    let mut d = Drawing::a4().auto(STYLES).auto(close).auto(exact).page(&shape("x", "1cm", "<text:p>plain</text:p>"));
    d.fonts = String::new();
    let mut o = open(&d);
    set_row(&mut o, "Character", 1, &[("Style", 3.0), ("Size", 14.0 / 72.0)]);
    set_parts(&mut o, vec![TextPart::Cp(1), chars("styled")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains("<text:span text:style-name=\"T9\">styled</text:span>"),
        "the style that holds exactly them, and not the one with a parent: {after}"
    );
    assert!(!after.contains("style:name=\"T10\""), "no style was made: {after}");
}

/// The new style's name is the prefix most of the file's text styles use and one more than the largest number either part has.
fn naming_case(save: &Saver) {
    let styles = r#"<style:style style:name="T1" style:family="text"/><style:style style:name="T2" style:family="text"/><style:style style:name="T3" style:family="text"/><style:style style:name="T4" style:family="text"/><style:style style:name="T9" style:family="text"/>"#;
    let mut d = Drawing::a4().auto(styles).page(&shape("x", "1cm", "<text:p>plain</text:p>"));
    d.styles_auto.push_str(r#"<style:style style:name="T12" style:family="text"/>"#);
    let mut o = open(&d);
    set_row(&mut o, "Character", 1, &[("Style", 1.0)]);
    set_parts(&mut o, vec![TextPart::Cp(1), chars("bold")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<style:style style:name=\"T13\" style:family=\"text\">"), "T13, and not T5: {after}");
    assert!(after.contains("<text:span text:style-name=\"T13\">bold</text:span>"), "{after}");
    // A paragraph style the same way: the file's paragraph styles are P2 and P7.
    let styles = r#"<style:style style:name="P2" style:family="paragraph"/><style:style style:name="P7" style:family="paragraph"/>"#;
    let mut o = open(&Drawing::a4().auto(styles).page(&shape("x", "1cm", "<text:p>plain</text:p>")));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![TextPart::Pp(1), chars("right")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(part(&saved.bytes, "content.xml").contains("<text:p text:style-name=\"P8\">right</text:p>"));
}

/// A font a run names is the face the part declares for it; a face no declaration names is declared.
#[test]
fn a_font_is_written_as_the_declared_face_and_an_undeclared_one_is_declared() {
    let mut d = Drawing::a4().page(&shape("x", "1cm", "<text:p>plain</text:p>"));
    d.fonts = r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/><style:font-face style:name="Mono" svg:font-family="DejaVu Sans Mono"/>"#.to_owned();
    for (face, declared) in [("Liberation Sans", "Liberation Sans"), ("DejaVu Sans Mono", "Mono")] {
        let mut o = open(&d);
        let id = id_of(&o.doc);
        {
            let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
            let mut section = Section::new("Character", None);
            let mut row = Row { ix: Some(1), ..Row::default() };
            row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text(face)));
            row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_number(12.0 / 72.0).unwrap()));
            section.rows.push(Arc::new(row));
            sheet.sections.push(Arc::new(section));
        }
        set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{face}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains(&format!("style:font-name=\"{declared}\"")), "{face}: {after}");
        let back = open_bytes(saved.bytes);
        let read = the_shape(&back.doc);
        let fonts = runs_of(read, "Font");
        let _ = fonts;
        let got = read
            .sheet
            .sections
            .iter()
            .find(|s| s.name.as_str() == "Character")
            .and_then(|s| s.rows.iter().find_map(|r| r.cells.get("Font")?.value.as_ref().map(|v| v.text().to_owned())));
        assert_eq!(got.as_deref(), Some(face), "read back");
    }
    let mut o = open(&d);
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
    // A face no declaration names is declared now: the style names the new face, and the declaration is in the same file.
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("style:font-name=\"Nowhere\""), "{after}");
    assert!(after.contains("<style:font-face style:name=\"Nowhere\" svg:font-family=\"'Nowhere'\"/>"), "{after}");
}

const LIST_BODY: &str = r#"<text:list text:style-name="L1"><text:list-item><text:p text:style-name="P1">one</text:p></text:list-item><text:list-item><text:p text:style-name="P1">two</text:p></text:list-item></text:list>"#;

/// A list takes a new item and loses one: the new paragraph has the row of its neighbour, so it is a new `text:list-item` of the same list with the
/// neighbour's style; a paragraph taken out takes its item with it.
fn list_case(save: &Saver) {
    let o = open(&drawing_of(LIST_BODY));
    let s = the_shape(&o.doc);
    assert_eq!(chars_of(s), "one\ntwo", "{}", show(s));
    // A paragraph added between the two.
    let mut added = o.doc.clone();
    {
        let id = id_of(&added);
        let parts: Vec<TextPart> = the_shape(&added)
            .text
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) if c.contains("one") => TextPart::Chars(Arc::from(c.replacen("one\n", "one\nbetween\n", 1))),
                other => other.clone(),
            })
            .collect();
        shape_mut(&mut added, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
        assert_eq!(chars_of(the_shape(&added)), "one\nbetween\ntwo");
    }
    let saved = save(&added, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(r#"<text:list text:style-name="L1"><text:list-item><text:p text:style-name="P1">one</text:p></text:list-item><text:list-item><text:p text:style-name="P1">between</text:p></text:list-item><text:list-item><text:p text:style-name="P1">two</text:p></text:list-item></text:list>"#),
        "{after}"
    );
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), "one\nbetween\ntwo");
    let again = save(&added, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // A paragraph added at the end, and one before the first.
    for (from, to, expect) in [("two", "two\nlast", "last"), ("one", "first\none", "first")] {
        let mut doc = o.doc.clone();
        let id = id_of(&doc);
        let parts: Vec<TextPart> = the_shape(&doc)
            .text
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) if c.contains(from) => TextPart::Chars(Arc::from(c.replacen(from, to, 1))),
                other => other.clone(),
            })
            .collect();
        shape_mut(&mut doc, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
        let saved = save(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{to}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert_eq!(after.matches("<text:list-item>").count(), 3, "{to}: {after}");
        assert!(after.contains(&format!("<text:list-item><text:p text:style-name=\"P1\">{expect}</text:p></text:list-item>")), "{to}: {after}");
        assert_eq!(chars_of(the_shape(&open_bytes(saved.bytes).doc)), chars_of(the_shape(&doc)));
    }
    // A paragraph taken out takes its item.
    let mut gone = o.doc.clone();
    {
        let id = id_of(&gone);
        let parts: Vec<TextPart> = the_shape(&gone)
            .text
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) => TextPart::Chars(Arc::from(c.replace("\ntwo", ""))),
                other => other.clone(),
            })
            .collect();
        shape_mut(&mut gone, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
    }
    let saved = save(&gone, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(r#"<text:list text:style-name="L1"><text:list-item><text:p text:style-name="P1">one</text:p></text:list-item></text:list>"#),
        "{after}"
    );
    assert_eq!(after.matches("<text:list-item>").count(), 1, "{after}");
}

/// A list item whose paragraph row changes is refused in words (its settings are the list's); a second paragraph with a list's row, in a shape that has no
/// list, is the item of a list the save makes.
#[test]
fn a_list_item_whose_row_changes_is_refused_and_a_paragraph_with_a_list_row_is_a_new_list() {
    let mut o = open(&drawing_of("<text:p>one</text:p>"));
    set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0)]);
    set_parts(&mut o, vec![chars("one\n"), TextPart::Pp(1), chars("two")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<text:list text:style-name=\"L2\"><text:list-item><text:p>two</text:p></text:list-item></text:list>"), "{after}");
    // The row of a list item changed.
    let mut o = open(&drawing_of(LIST_BODY));
    set_row(&mut o, "Paragraph", 0, &[("Bullet", 1.0), ("HorzAlign", 2.0)]);
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains("list"), "{what}"),
        other => panic!("expected a refusal of a list: {:?}", other.map(|w| w.bytes.len())),
    }
}

const CASES: [Case; 8] = [
    ("an_edit_across_the_boundary_of_a_run_is_written", across_a_run_case),
    ("an_edit_at_the_boundary_of_a_run_is_written", at_a_boundary_case),
    ("a_paragraph_added_or_removed_is_written", paragraphs_case),
    ("a_whole_label_replaced_drops_the_runs_and_their_styles", whole_label_case),
    ("an_edit_of_a_row_is_written_to_every_paragraph_that_has_it", row_cell_case),
    ("cells_are_written_as_their_attributes_and_read_back", case_19),
    ("a_style_the_part_holds_is_used", existing_style_case),
    ("a_new_style_is_named_by_the_file_s_numbers", naming_case),
];

#[test]
fn an_edit_across_the_boundary_of_a_run_is_written() {
    across_a_run_case(&real);
}

#[test]
fn an_edit_at_the_boundary_of_a_run_is_written() {
    at_a_boundary_case(&real);
}

#[test]
fn a_paragraph_added_or_removed_is_written() {
    paragraphs_case(&real);
}

#[test]
fn a_whole_label_replaced_drops_the_runs_and_their_styles() {
    whole_label_case(&real);
}

#[test]
fn an_edit_of_a_row_is_written_to_every_paragraph_that_has_it() {
    row_cell_case(&real);
}

#[test]
fn cells_are_written_as_their_attributes_and_read_back() {
    case_19(&real);
}

#[test]
fn a_style_the_part_holds_is_used() {
    existing_style_case(&real);
}

#[test]
fn a_new_style_is_named_by_the_file_s_numbers() {
    naming_case(&real);
}

#[test]
fn a_list_takes_a_new_item_and_loses_one() {
    list_case(&real);
}

#[test]
fn echo_control_fails_every_run_case() {
    echo_control(&CASES);
    echo_control(&[("a_list_takes_a_new_item_and_loses_one", list_case)]);
}
