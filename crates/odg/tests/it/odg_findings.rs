//! The `.odg` findings of the gate on `76278d7e`, at the crate's own level (the engine's tests of the same names are the plain visit a person makes). Every read-back goes through the
//! real reader, and the inputs are written by hand from the OASIS ODF standard (Part 3).

use diacraft_doc::{Document, HeldGroup, OdfRun, Shape};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const STANDARD: &str = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffd966" svg:stroke-color="#3465a4"/></style:style>"##;

const AUTO: &str = concat!(
    r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"/>"#,
    r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"/></style:style>"#,
);

fn drawing(shapes: &str) -> Drawing {
    Drawing::a4().common(STANDARD).auto(AUTO).page(shapes)
}

/// The package of a drawing (ODF 1.4, the version the model writer writes), and the model the reader makes of it.
fn read_back(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    let bytes = package(DRAWING, drawing.files());
    read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the vector does not read: {e}")).0
}

/// The model written from nothing, with what each part says: the bytes of the package.
fn save(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}")).bytes
}

fn part(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap().to_vec()).unwrap()
}

fn first_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap()
}

/// The runs the file's first paragraph of the first shape's text held, and the number of characters the text has.
fn runs_and_count(doc: &Document) -> (Vec<OdfRun>, usize) {
    let text = first_shape(doc).text.as_ref().unwrap();
    let count: usize = text.parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.chars().count()) } else { None }).sum();
    (text.ext.get().unwrap().odf.paragraphs.first().unwrap().runs.clone(), count)
}

// ---------------------------------------------------------------------------------------------------------------- the spans end where the characters do

fn linked_rect(last: &str) -> String {
    format!(
        concat!(
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><text:p>"#,
            r#"<text:span text:style-name="T1">See the notes (</text:span>"#,
            r#"<text:span text:style-name="T1"><text:a xlink:href="https://example.org/notes" xlink:type="simple">here</text:a></text:span>"#,
            r#"<text:span text:style-name="T1">{}</text:span></text:p></draw:rect>"#
        ),
        last
    )
}

/// **The runs of a paragraph end where its characters do**: the white space that ends a paragraph goes, and a run that closed after it ended one character past the text, so the
/// writer passed by the paragraph and wrote it with none of its runs, hyperlinks among them.
#[test]
fn the_runs_of_a_paragraph_whose_last_run_ends_in_a_space_end_with_its_characters() {
    for last in [").", ") ", ")  ", ")\n"] {
        let doc = read_back(&drawing(&linked_rect(last)));
        let (runs, count) = runs_and_count(&doc);
        assert!(runs.len() >= 4, "{last:?}: the three spans and the link: {runs:?}");
        assert!(runs.iter().all(|r| r.from <= r.to && r.to as usize <= count), "{last:?}: every run ends within the {count} characters: {runs:?}");
        assert_eq!(runs.iter().map(|r| r.to).max(), Some(count as u32), "{last:?}: the last run ends with the paragraph");
    }
}

/// The same paragraph written from the model keeps its hyperlink, whatever the last run ends in, and the text rides in no tier 2 of DiaCraft's.
#[test]
fn the_model_writer_keeps_a_link_whose_paragraph_ends_in_a_space() {
    for last in [").", ") "] {
        let doc = read_back(&drawing(&linked_rect(last)));
        let written = save(&doc);
        let content = part(&written, "content.xml");
        assert_eq!(content.matches("<text:a ").count(), 1, "{last:?}: the link is written: {content}");
        assert!(!content.contains("diacraft:text="), "{last:?}: the text is the markup's, not carried: {content}");
        let again = read(&written, &Limits::default()).unwrap().0;
        let (runs, _) = runs_and_count(&again);
        assert!(runs.iter().any(|r| r.element.as_ref().is_some_and(|e| &*e.name == "text:a")), "{last:?}: the link is held after the read: {runs:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- the file's runs are written as they were

fn three_spans(middle: &str) -> String {
    format!(
        concat!(
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><text:p>"#,
            r#"<text:span text:style-name="T1">See the notes (</text:span>{}<text:span text:style-name="T1">).</text:span></text:p></draw:rect>"#
        ),
        middle
    )
}

/// **Three runs of one style that the model holds as one stretch are written as the file's three spans**, each under the name the file gave it: no span of the file covered the stretch, so the row
/// was not confirmed and the whole text rode in DiaCraft's data, and the paragraph read back lost the markup's runs.
#[test]
fn the_runs_of_a_file_inside_a_stretch_of_one_row_are_written_as_they_were() {
    let doc = read_back(&drawing(&three_spans(r#"<text:span text:style-name="T1">here</text:span>"#)));
    let written = save(&doc);
    let content = part(&written, "content.xml");
    assert_eq!(content.matches(r#"<text:span text:style-name="T1">"#).count(), 3, "three spans, each named as the file named it: {content}");
    assert!(!content.contains("diacraft:text="), "the text is the markup's: {content}");
    let (runs, count) = runs_and_count(&read(&written, &Limits::default()).unwrap().0);
    assert_eq!(runs.iter().map(|r| (r.from, r.to)).collect::<Vec<_>>(), vec![(0, 15), (15, 19), (19, count as u32)]);
}

/// A span that wraps a link and has no characters of its own is the file's span for the stretch it covers (its stamp is the empty row's): written back as it was, with the link inside.
#[test]
fn a_span_that_holds_only_a_link_is_written_back_with_the_link() {
    let middle = r#"<text:span text:style-name="T1"><text:a xlink:href="https://example.org/notes" xlink:type="simple">here</text:a></text:span>"#;
    let doc = read_back(&drawing(&three_spans(middle)));
    let written = save(&doc);
    let content = part(&written, "content.xml");
    assert_eq!(content.matches("<text:a ").count(), 1, "{content}");
    assert!(!content.contains("diacraft:text="), "the text is the markup's: {content}");
}

// ---------------------------------------------------------------------------------------------------------------- glue points of a group's own, and of a custom shape's geometry beside its own

/// The structure of the vector: an outer group, a group with an id, and in it a group with no attribute that holds two `draw:glue-point`, a path and a group of one polygon.
fn nested_groups(middle_open: &str) -> String {
    format!(
        concat!(
            r#"<draw:g>{}<draw:g>"#,
            r#"<draw:glue-point draw:id="4" svg:x="1.5cm" svg:y="0cm"/><draw:glue-point draw:id="5" svg:x="3cm" svg:y="1cm"/>"#,
            r#"<draw:path draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="2cm" svg:viewBox="0 0 3000 2000" svg:d="M0 0L3000 0 3000 2000 0 2000Z"><text:p/></draw:path>"#,
            r#"<draw:g><draw:polygon draw:style-name="gr1" svg:x="2.5cm" svg:y="2.5cm" svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"><text:p/></draw:polygon></draw:g>"#,
            r#"</draw:g></draw:g></draw:g>"#
        ),
        middle_open
    )
}

/// **A group of the file with `draw:glue-point` children of its own is written from the model without an entry for them**, at any depth and whatever its neighbours hold: the rows are what the markup
/// gives, so no entry in DiaCraft's document file is found by the place of a group with no name (which is refused). Red on `76278d7e` for the group below a group that holds `draw:id`.
#[test]
fn a_group_s_own_glue_points_are_written_from_the_model_at_any_depth() {
    for middle in [r#"<draw:g draw:id="k1" xml:id="k1">"#, "<draw:g>"] {
        let doc = read_back(&drawing(&nested_groups(middle)));
        let written = save(&doc);
        let content = part(&written, "content.xml");
        assert_eq!(content.matches("<draw:glue-point ").count(), 2, "{middle}: both points are written: {content}");
        let again = read(&written, &Limits::default()).unwrap().0;
        let rows = |doc: &Document| -> usize {
            let mut stack: Vec<&std::sync::Arc<Shape>> = doc.pages.get(0).unwrap().shapes.iter().collect();
            let mut most = 0;
            while let Some(shape) = stack.pop() {
                stack.extend(shape.children.iter());
                let rows = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Connection").map(|s| s.rows.len()).max().unwrap_or(0);
                most = most.max(rows);
            }
            most
        };
        assert_eq!(rows(&again), rows(&doc), "{middle}: the group has the rows it had: four and the two points");
        assert_eq!(rows(&doc), 6);
    }
}

fn custom_shape(own: &str) -> String {
    format!(
        concat!(
            r#"<draw:custom-shape draw:id="id1" draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm">{}"#,
            r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="flowchart-process" draw:glue-points="10800 0 0 10800 10800 21600 21600 10800" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        ),
        own
    )
}

/// The element a row holds, by the `draw:id` it was read from.
fn held_of(doc: &Document, ix: u32) -> Option<diacraft_doc::Held> {
    let section = first_shape(doc).sheet.sections.iter().find(|s| s.name.as_str() == "Connection")?.clone();
    let row = section.rows.iter().find(|r| r.ix == Some(ix))?;
    row.ext.get()?.odf.held.clone()
}

/// **A carried `Connection` section keeps the element of each glue point it still is**: a custom shape whose geometry lists four glue points and that has a `draw:glue-point` of
/// its own after them has its section carried in DiaCraft's data whole, which replaced the rows the markup gave and with them the element each row held. The row that is still what its
/// element gives has it again after the read; a row that was moved has none.
#[test]
fn a_carried_connection_row_that_is_still_its_glue_point_holds_the_element_again() {
    let doc = read_back(&drawing(&custom_shape(r#"<draw:glue-point draw:id="8" svg:x="1cm" svg:y="0.5cm"/>"#)));
    let before = held_of(&doc, 8).expect("the point is held on its row");
    assert_eq!(before.group, HeldGroup::Glue);
    let written = save(&doc);
    assert!(part(&written, "content.xml").contains("diacraft:s-Connection="), "the section is carried (the geometry's rows have no element to hold)");
    let again = read(&written, &Limits::default()).unwrap().0;
    assert_eq!(held_of(&again, 8), Some(before), "the row is the glue point it was, and holds it");
    // A point moved in the model is not what its element says, and holds none after the read.
    let mut moved = doc.clone();
    {
        let page = std::sync::Arc::make_mut(moved.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        let key = diacraft_doc::SectionKey::new("Connection", None);
        let row = sheet.row_mut(&key, &diacraft_doc::RowKey::Ix(8)).unwrap();
        row.cells.insert(diacraft_doc::CellName::new("X"), diacraft_doc::Cell::with_formula("Width*0.9", Some(diacraft_doc::Value::from_text("1"))));
    }
    let written = save(&moved);
    let after = read(&written, &Limits::default()).unwrap().0;
    assert!(held_of(&after, 8).is_none(), "a moved point holds no element");
}
