//! Tier 3, LibreOffice edits win ( the design's "Tier 3"; the proposal of for the names).
//!
//! A drawing DiaCraft saved is edited in LibreOffice (the edits are made here by hand in the bytes, as the probe's rules say LibreOffice writes them, since no
//! file of another program is committed) and read again. An item of tier 2 is stale only when the **native outputs** it determined changed (a path, a text, a
//! name, a line end), and a stale item is left out so that the markup's own reading stands; an edit of what it took as **input** (a move, a resize) leaves it as
//! it was, and so does every rewrite a LibreOffice save makes with no edit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Master, Row, Section, SectionKey, Shape, ShapeList, ShapeType, Text, TextPart, Value};

use diacraft_odg::{Limits, ReadReport, read};

use crate::docfile::rebuild;
use crate::drawing::Drawing;
use crate::write_core::{Open, open, open_bytes, part, real};
use crate::write_new::{push_shape, rectangle};

const PATH: &str = "DiaCraft/document.xml";

/// The id the new shape of the fixtures is given.
fn shape_id() -> u32 {
    open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm"))).index.next_id(0).unwrap()
}

fn text_cell(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

/// A new shape that carries one of each item tier 3 judges, and two that it does not: tier-2 geometry (a section that is not filled, which the writer writes as
/// a placeholder outline), a text with markers, a name that is not its `draw:name`, a link to a master, a colour in capitals, and a user row and a cell, which
/// rest on no native output.
fn full_shape() -> Shape {
    let mut shape = rectangle(shape_id(), "rect.1", (2.0, 3.0, 4.0, 2.0));
    shape.name = Some(Arc::from("Display"));
    shape.master = Some(2);
    shape.text = Some(Arc::new(Text {
        parts: vec![TextPart::Cp(0), TextPart::Chars(Arc::from("Hello")), TextPart::Pp(1), TextPart::Chars(Arc::from("\nWorld"))],
        ..Default::default()
    }));
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("Comment"), text_cell("a note"));
    sheet.cells.insert(CellName::new("FillForegnd"), text_cell("#FF0000"));
    if let Some(at) = sheet.sections.iter().position(|s| s.name.as_str() == "Geometry") {
        let mut geometry = (*sheet.sections[at]).clone();
        geometry.cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_number(1.0).unwrap()));
        sheet.sections[at] = Arc::new(geometry);
    }
    let mut user = Section::new("User", None);
    let mut row = Row { name: Some(Arc::from("Key")), ..Row::default() };
    row.cells.insert(CellName::new("Value"), text_cell("kept"));
    user.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(user));
    shape
}

/// A master whose one shape has shape data and a user row, which an instance inherits.
fn master(id: u32) -> Master {
    let mut shape = Shape::new(id * 10 + 1);
    shape.name = Some(Arc::from("Body"));
    shape.name_u = Some(Arc::from("Body"));
    shape.kind = ShapeType::Shape;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let mut prop = Section::new("Property", None);
    let mut cost = Row { name: Some(Arc::from("Cost")), ..Row::default() };
    cost.cells.insert(CellName::new("Value"), text_cell("42"));
    prop.rows.push(Arc::new(cost));
    sheet.sections.push(Arc::new(prop));
    let mut user = Section::new("User", None);
    let mut owner = Row { name: Some(Arc::from("Owner")), ..Row::default() };
    owner.cells.insert(CellName::new("Value"), text_cell("the master"));
    let mut both = Row { name: Some(Arc::from("Key")), ..Row::default() };
    both.cells.insert(CellName::new("Value"), text_cell("the master's"));
    user.rows.push(Arc::new(owner));
    user.rows.push(Arc::new(both));
    sheet.sections.push(Arc::new(user));
    Master { id, name: Some(Arc::from("Master")), shapes: ShapeList::from_vec(vec![Arc::new(shape)]), ..Master::default() }
}

/// The drawing saved with the full shape (and master 2, which the shape is an instance of).
fn saved() -> Vec<u8> {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    push_shape(&mut o.doc, full_shape());
    o.doc.masters.push(Arc::new(master(2)));
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes
}

/// A package with `content.xml` passed through `edit`, as another program that keeps the document file writes it.
fn edited(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    rebuild(
        bytes,
        |name, b| Some(if name == "content.xml" { edit(String::from_utf8(b.to_vec()).unwrap()).into_bytes() } else { b.to_vec() }),
        |m| m.to_owned(),
    )
}

/// What a LibreOffice save does to a file with no edit, as far as the probe saw: `office:version` becomes 1.4, the automatic styles are renamed
/// (`gr1` to `gr9`, `P1` to `P9`) with the references following, the graphic style gains `fo:min-height` and `fo:min-width`, and every shape gains
/// `draw:text-style-name` (already there). The attributes of the extension stay with the style.
fn like_libreoffice(content: String) -> String {
    content
        .replace("office:version=\"1.3\"", "office:version=\"1.4\"")
        .replace("\"gr1\"", "\"gr9\"")
        .replace("\"P1\"", "\"P9\"")
        .replace("<style:graphic-properties ", "<style:graphic-properties fo:min-height=\"0cm\" fo:min-width=\"0cm\" ")
}

/// The package without DiaCraft's document file and its manifest row, as every LibreOffice save leaves it.
fn without_the_document_file(bytes: &[u8]) -> Vec<u8> {
    rebuild(
        bytes,
        |name, b| (name != PATH).then(|| b.to_vec()),
        |m| {
            let mut out = String::new();
            for chunk in m.split("<manifest:file-entry ") {
                if chunk.contains(PATH) {
                    out.push_str(chunk.split("/>").nth(1).unwrap_or_default());
                } else if out.is_empty() {
                    out.push_str(chunk);
                } else {
                    out.push_str("<manifest:file-entry ");
                    out.push_str(chunk);
                }
            }
            out
        },
    )
}

/// The new shape of the fixtures as the reader gives it, and the report.
fn read_full(bytes: &[u8]) -> (Shape, ReadReport) {
    let (doc, report) = read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {e}"));
    let id = shape_id();
    let found = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone());
    (found.unwrap_or_else(|| panic!("the shape {id} is not read")), report)
}

fn stale_count(report: &ReadReport) -> u64 {
    report.warning_counts.get("diacraft_stale").copied().unwrap_or(0)
}

fn chars_of(shape: &Shape) -> String {
    shape
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect())
        .unwrap_or_default()
}

fn geometry_of(shape: &Shape) -> Option<&Section> {
    shape.sheet.section(&SectionKey::new("Geometry", Some(0))).map(|s| &**s)
}

fn colour_of(shape: &Shape) -> Option<String> {
    shape.sheet.cell("FillForegnd").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// Every item the fixture carries is there: the text with its markers, the name, the link to a master, the geometry the writer could not write, the colour in
/// capitals, and what rests on no native output (a cell, a user row).
fn assert_all_carried(shape: &Shape, what: &str) {
    let parts = &shape.text.as_ref().unwrap_or_else(|| panic!("{what}: no text")).parts;
    assert!(matches!(parts.first(), Some(TextPart::Cp(0))), "{what}: the text's markers: {parts:?}");
    assert_eq!(chars_of(shape), "Hello\nWorld", "{what}");
    assert_eq!(shape.name.as_deref(), Some("Display"), "{what}: the name");
    assert_eq!(shape.master, Some(2), "{what}: the link to the master");
    assert_eq!(colour_of(shape).as_deref(), Some("#FF0000"), "{what}: the colour");
    assert!(shape.sheet.cell("Comment").is_some(), "{what}: the cell");
    assert!(geometry_of(shape).is_some_and(|g| g.cells.contains_key("NoFill")), "{what}: the carried geometry");
    assert!(
        shape.sheet.section(&SectionKey::new("User", None)).is_some_and(|u| u.rows.iter().any(|r| r.name.as_deref() == Some("Key"))),
        "{what}: the user row"
    );
}

/// The value of the attribute `diacraft:fp-<key>` in `content`.
fn fp_of(content: &str, key: &str) -> Option<String> {
    content.split(&format!("diacraft:fp-{key}=\"")).nth(1).and_then(|r| r.split('"').next()).map(str::to_owned)
}

/// The style of the new shape carries a fingerprint for each item that rests on native outputs, each 16 lowercase hexadecimal digits, and for no item that does not.
#[test]
fn the_writer_puts_a_fingerprint_beside_each_item_that_rests_on_native_outputs() {
    let content = part(&saved(), "content.xml");
    for key in ["text", "name", "master", "s-Geometry-0", "c-FillForegnd"] {
        let value = fp_of(&content, key).unwrap_or_else(|| panic!("no fp-{key} in {content}"));
        assert!(value.len() == 16 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "fp-{key}: {value}");
    }
    assert!(
        !content.contains("diacraft:fp-c-Comment") && !content.contains("diacraft:fp-s-User"),
        "a user cell and a user row rest on no native output: {content}"
    );
    // The outline's fingerprint is the geometry's and the master link's: one fingerprint of one set of outputs.
    assert_eq!(fp_of(&content, "master"), fp_of(&content, "s-Geometry-0"));
}

/// A drawing as DiaCraft saved it reads every item and judges none stale; a second save is no edit.
#[test]
fn a_drawing_as_it_was_saved_reads_every_item_and_judges_none_stale() {
    let bytes = saved();
    let (shape, report) = read_full(&bytes);
    assert_all_carried(&shape, "as saved");
    assert_eq!(stale_count(&report), 0);
    assert_eq!(report.index.page(0).unwrap().stale_shapes(), 0);
    // Nothing is said of a fingerprint that is none, nor of one that is stale, not even a line of words with a count of 0.
    assert!(
        !report.warning_counts.contains_key("diacraft_fingerprint_unreadable") && !report.warning_counts.contains_key("diacraft_stale"),
        "{:?}",
        report.warning_counts
    );
    assert!(report.warnings.iter().all(|w| !w.contains("fingerprint") && !w.contains("left out")), "{:?}", report.warnings);
    let o = open_bytes(bytes.clone());
    let again = real(&o.doc, &bytes, &o.index).unwrap();
    assert!(!again.edited && again.bytes == bytes, "a second save is no edit");
}

/// Tier 3 for LibreOffice's own rewrites: what a save makes with no edit changes no output, so every item stays.
#[test]
fn what_a_libreoffice_save_rewrites_with_no_edit_leaves_every_item_as_it_was() {
    let (shape, report) = read_full(&edited(&saved(), like_libreoffice));
    assert_all_carried(&shape, "after an untouched LibreOffice save");
    assert_eq!(stale_count(&report), 0);
}

/// `libreoffice_moved_shape_keeps_tier2` (the plan's test; an input change): LibreOffice moves a shape (`svg:x` and `svg:y` change and nothing else), and
/// every item of tier 2 stays: the path it stretches is as it was. The same when it resizes the shape (only the frame changes).
#[test]
fn libreoffice_moved_shape_keeps_tier2() {
    type Edit = fn(String) -> String;
    let edits: [(&str, Edit); 3] = [
        ("a move", |c| c.replace("svg:x=\"2cm\" svg:y=\"3cm\"", "svg:x=\"7.5cm\" svg:y=\"11.25cm\"")),
        ("a resize", |c| c.replace("svg:width=\"4cm\" svg:height=\"2cm\"", "svg:width=\"9cm\" svg:height=\"1cm\"")),
        ("a move and a resize, and LibreOffice's own rewrites", |c| {
            like_libreoffice(c.replace(
                "svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"3cm\"",
                "svg:width=\"6cm\" svg:height=\"3cm\" svg:x=\"1cm\" svg:y=\"1cm\"",
            ))
        }),
    ];
    for (what, edit) in edits {
        let (shape, report) = read_full(&edited(&saved(), edit));
        assert_all_carried(&shape, what);
        assert_eq!(stale_count(&report), 0, "{what}");
        assert_eq!(report.index.page(0).unwrap().stale_shapes(), 0, "{what}");
    }
    // The shape is where LibreOffice put it.
    let (moved, _) = read_full(&edited(&saved(), |c| c.replace("svg:x=\"2cm\" svg:y=\"3cm\"", "svg:x=\"7.5cm\" svg:y=\"11.25cm\"")));
    let (before, _) = read_full(&saved());
    let pin = |s: &Shape| s.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((pin(&moved) - pin(&before) - 5.5 / 2.54).abs() < 1e-9, "{} against {}", pin(&moved), pin(&before));
}

/// `libreoffice_edited_path_drops_stale_geometry` (the plan's test; an output change): LibreOffice edits the path of the shape, so the geometry DiaCraft carried is
/// stale and the path the markup says is read. What the shape's link to its master rested on is the same outline, so that goes too, and what rests on no output
/// (a user row, a cell) and what rests on outputs that did not change (the text, the name, the colour) stays.
#[test]
fn libreoffice_edited_path_drops_stale_geometry() {
    let path = "draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"";
    let edit = |c: String| like_libreoffice(c.replace(path, "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""));
    let bytes = edited(&saved(), edit);
    let (shape, report) = read_full(&bytes);
    let geometry = geometry_of(&shape).expect("the shape has a geometry");
    assert!(!geometry.cells.contains_key("NoFill"), "the carried geometry is gone: {geometry:?}");
    assert_eq!(geometry.rows.len(), 4, "the path the markup says (a triangle) is read: {geometry:?}");
    assert_eq!(shape.master, None, "the link to the master rested on the same outline");
    assert_eq!(stale_count(&report), 2, "the geometry and the link: {:?}", report.warning_counts);
    assert_eq!(report.index.page(0).unwrap().stale_shapes(), 1);
    // The rest is as it was.
    assert_eq!(shape.name.as_deref(), Some("Display"));
    assert_eq!(chars_of(&shape), "Hello\nWorld");
    assert!(matches!(shape.text.as_ref().unwrap().parts.first(), Some(TextPart::Cp(0))));
    assert!(shape.sheet.cell("Comment").is_some());
    assert_eq!(colour_of(&shape).as_deref(), Some("#FF0000"));
    assert!(shape.sheet.section(&SectionKey::new("User", None)).is_some_and(|u| u.rows.iter().any(|r| r.name.as_deref() == Some("Key"))));
}

/// A text LibreOffice's user retyped is the markup's text, and the carried one (with its markers) is stale; the other items stay.
#[test]
fn a_text_retyped_in_libreoffice_drops_the_carried_text() {
    let (shape, report) = read_full(&edited(&saved(), |c| c.replace("<text:p>Hello</text:p>", "<text:p>Hallo</text:p>")));
    assert_eq!(chars_of(&shape), "Hallo\nWorld", "the text the markup says");
    assert!(!matches!(shape.text.as_ref().unwrap().parts.first(), Some(TextPart::Cp(0))), "no marker of the carried text");
    assert_eq!(stale_count(&report), 1);
    assert_eq!(shape.name.as_deref(), Some("Display"));
    assert_eq!(shape.master, Some(2), "the link rests on the outline, which is as it was");
    assert!(geometry_of(&shape).is_some_and(|g| g.cells.contains_key("NoFill")));
}

/// A shape renamed in LibreOffice has the name the markup says, for both names.
#[test]
fn a_shape_renamed_in_libreoffice_has_the_name_it_was_given() {
    let (shape, report) = read_full(&edited(&saved(), |c| c.replace("draw:name=\"rect.1\"", "draw:name=\"Renamed\"")));
    assert_eq!((shape.name.as_deref(), shape.name_u.as_deref()), (Some("Renamed"), Some("Renamed")));
    assert_eq!(stale_count(&report), 1);
    assert_eq!(chars_of(&shape), "Hello\nWorld");
}

/// A colour set in LibreOffice (`.uno:FillColor` writes `draw:fill-color` on the shape's own style) wins over the colour DiaCraft carried as a cell.
#[test]
fn a_colour_set_in_libreoffice_wins_over_the_carried_colour() {
    let (shape, report) = read_full(&edited(&saved(), |c| c.replace("draw:fill=\"solid\" ", "draw:fill=\"solid\" draw:fill-color=\"#00ff00\" ")));
    assert_eq!(colour_of(&shape).map(|c| c.to_ascii_lowercase()).as_deref(), Some("#00ff00"));
    assert_eq!(stale_count(&report), 1);
    assert!(shape.sheet.cell("Comment").is_some());
}

// ------------------------------------------------------------------------------------------------------------------ V44, the third sentence

/// A package with `content.xml` and `styles.xml` passed through `edit` (given the part's name).
fn edited_parts(bytes: &[u8], edit: impl Fn(&str, String) -> String) -> Vec<u8> {
    rebuild(
        bytes,
        |name, b| match name {
            "content.xml" | "styles.xml" => Some(edit(name, String::from_utf8(b.to_vec()).unwrap()).into_bytes()),
            _ => Some(b.to_vec()),
        },
        |m| m.to_owned(),
    )
}

/// A new line whose end is the custom end 254 by the formula `USE("Fancy end")`, size class 2 and 0.75 pt, saved: the file.
fn fancy_end() -> (Vec<u8>, u32) {
    let mut o = open(&crate::write_markers::base());
    let id = o.index.next_id(0).unwrap();
    let mut line = crate::write_markers::arrowed(id, None, None, 0.75);
    Arc::make_mut(&mut line.sheet)
        .cells
        .insert(CellName::new("EndArrow"), Cell::read(Some(r#"USE("Fancy end")"#), Some("254"), None, None, &mut 1_000));
    crate::write_markers::set(&mut line, "EndArrowSize", 2.0);
    push_shape(&mut o.doc, line);
    (real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes, id)
}

/// The cell `name` of the shape `id`, as read.
fn cell_of(bytes: &[u8], id: u32, name: &str) -> Option<Cell> {
    let (doc, _) = read(bytes, &Limits::default()).unwrap();
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).and_then(|s| s.sheet.cell(name).cloned())
}

/// The third sentence: given the file, after another program set the line's end to a marker named `Arrow`, expect `EndArrow` 4 as the
/// stand-in and the 254 dropped. The marker's path is the test's own. Before the edit the 254 reads back with its formula, and after a LibreOffice save with no
/// edit that rewrites the marker's width (a thousandth of a centimetre) it still does.
#[test]
fn odg_styles_a_line_end_another_program_re_marked_drops_the_254() {
    let (bytes, id) = fancy_end();
    let content = part(&bytes, "content.xml");
    assert!(fp_of(&content, "c-EndArrow").is_some(), "the cell of the custom end is fingerprinted: {content}");
    let before = cell_of(&bytes, id, "EndArrow").unwrap();
    assert_eq!((before.value.as_ref().and_then(Value::number), before.formula.text()), (Some(254.0), Some(r#"USE("Fancy end")"#)));
    // A save with no edit that writes the marker's width as LibreOffice does: 6.54 pt is 0.231 cm.
    let same = edited_parts(&bytes, |_, text| text.replace("draw:marker-end-width=\"6.54pt\"", "draw:marker-end-width=\"0.231cm\""));
    assert_ne!(same, bytes);
    assert_eq!(cell_of(&same, id, "EndArrow").and_then(|c| c.value.as_ref().and_then(Value::number)), Some(254.0), "a rounded width is no edit");
    // Another program sets the end to its own marker, named `Arrow`.
    let marker = "<draw:marker draw:name=\"Arrow\" svg:viewBox=\"0 0 10 12\" svg:d=\"M 0 12 L 5 0 L 10 12 Z\"/>";
    let re_marked = edited_parts(&bytes, |name, text| {
        if name == "styles.xml" {
            text.replacen("<office:styles>", &format!("<office:styles>{marker}"), 1)
        } else {
            text.replace("draw:marker-end=\"DiaCraft_20_end_20_4\"", "draw:marker-end=\"Arrow\"")
        }
    });
    let after = cell_of(&re_marked, id, "EndArrow").expect("the end reads");
    assert_eq!(after.value.as_ref().and_then(Value::number), Some(4.0), "the stand-in for a marker DiaCraft does not know");
    assert_eq!(after.formula.text(), None, "the 254 and its formula are dropped");
    let (_, report) = read(&re_marked, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale").copied(), Some(1));
}

/// A line end with no marker at all (another program took the end off) is a change of the output too: the 254 goes.
#[test]
fn a_line_end_another_program_took_off_drops_the_254() {
    let (bytes, id) = fancy_end();
    let off = edited_parts(&bytes, |name, text| {
        if name == "content.xml" {
            let at = text.find("draw:marker-end=\"").unwrap();
            let end = at + text[at..].find("\" ").unwrap() + 2;
            let mut t = text.clone();
            t.replace_range(at..end, "");
            t
        } else {
            text
        }
    });
    let after = cell_of(&off, id, "EndArrow");
    assert_eq!(after.and_then(|c| c.value.as_ref().and_then(Value::number)).unwrap_or(0.0), 0.0, "no 254, and no end");
}

// ------------------------------------------------------------------------------------------------------------------ the stale master link

/// `stale_master_link_keeps_shape_data` (the plan's test): the outline LibreOffice edited was the master's, so the link is stale; the shape keeps its own copies of
/// what it inherited, the shape data (`Property`) and the user rows (`User`) the master gives and the shape does not hold itself, and its own rows are as they are.
/// Another program that keeps DiaCraft's document file (LibreOffice drops it) is the case: the master's body is there.
#[test]
fn stale_master_link_keeps_shape_data() {
    let path = "draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"";
    let bytes = edited(&saved(), |c| c.replace(path, "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""));
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(doc.masters.len(), 1, "the document file is read: the master's body is there");
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == shape_id()).map(|s| (**s).clone()).unwrap();
    assert_eq!((shape.master, shape.master_shape), (None, None), "the link is dropped");
    let prop = shape.sheet.section(&SectionKey::new("Property", None)).expect("the shape data is copied");
    let value = |s: &Section, row: &str| {
        s.rows.iter().find(|r| r.name.as_deref() == Some(row)).and_then(|r| r.cells.get("Value")).map(|c| c.value.as_ref().unwrap().text().to_owned())
    };
    assert_eq!(value(prop, "Cost").as_deref(), Some("42"));
    let user = shape.sheet.section(&SectionKey::new("User", None)).expect("the user rows");
    assert_eq!(value(user, "Owner").as_deref(), Some("the master"), "an inherited user row is copied");
    assert_eq!(value(user, "Key").as_deref(), Some("kept"), "the shape's own row is not replaced by the master's");
    assert_eq!(user.rows.len(), 2);
    assert_eq!(report.warning_counts.get("diacraft_stale_link_without_master"), None);
    // A save by DiaCraft that edits the shape writes the link out and the copies in: they are the shape's own, in its style, and the file no longer says anything stale
    // (an unedited save is the file's own bytes, which still hold the stale items and the master's body that the copies are made from again at each read).
    let mut o = open_bytes(bytes.clone());
    crate::write_core::move_by(&mut o.doc, 0, shape_id(), 1.0, 0.0);
    let again = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(again.edited);
    let content = part(&again.bytes, "content.xml");
    assert!(
        content.contains("diacraft:s-Property=") && content.contains("diacraft:s-User="),
        "the copies are carried by the shape's style: {content}"
    );
    for kept in [again.bytes.clone(), without_the_document_file(&again.bytes)] {
        let (back, report) = read(&kept, &Limits::default()).unwrap();
        let back = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == shape_id()).map(|s| (**s).clone()).unwrap();
        assert_eq!(back.master, None);
        assert_eq!(
            value(back.sheet.section(&SectionKey::new("Property", None)).expect("kept in the file"), "Cost").as_deref(),
            Some("42"),
            "with or without the document file"
        );
        assert_eq!(value(back.sheet.section(&SectionKey::new("User", None)).unwrap(), "Owner").as_deref(), Some("the master"));
        assert_eq!(value(back.sheet.section(&SectionKey::new("User", None)).unwrap(), "Key").as_deref(), Some("kept"));
        assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    }
}

/// After a LibreOffice save the document file is gone, and with it the master's body: the link is stale and there is nothing to copy from. The shape keeps
/// what it holds and the read says so, once.
#[test]
fn a_stale_master_link_after_a_libreoffice_save_has_no_master_to_copy_from() {
    let path = "draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"";
    let bytes = without_the_document_file(&edited(&saved(), |c| {
        like_libreoffice(c.replace(path, "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""))
    }));
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    assert!(doc.masters.is_empty());
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == shape_id()).map(|s| (**s).clone()).unwrap();
    assert_eq!(shape.master, None);
    assert!(shape.sheet.section(&SectionKey::new("Property", None)).is_none());
    assert!(shape.sheet.section(&SectionKey::new("User", None)).is_some_and(|u| u.rows.len() == 1), "its own row stays");
    assert_eq!(report.warning_counts.get("diacraft_stale_link_without_master"), Some(&1));
}

// ------------------------------------------------------------------------------------------------------------------ what a DiaCraft edit does of it

/// The page's shape `id` of the model.
fn model_shape(o: &crate::write_core::Open, id: u32) -> Shape {
    o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone()).unwrap()
}

/// **A stale item does not come back.** LibreOffice edits the path; DiaCraft opens the file (the geometry and the link are left out), the user moves the shape and
/// saves; the style no longer carries the stale geometry, its fingerprint or the link, and the file read again has the path LibreOffice made, with nothing stale.
#[test]
fn an_edit_in_diacraft_of_a_shape_whose_items_went_stale_does_not_write_them_again() {
    let path = "draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"";
    let bytes = edited(&saved(), |c| c.replace(path, "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""));
    let mut o = open_bytes(bytes.clone());
    let id = shape_id();
    crate::write_core::move_by(&mut o.doc, 0, id, 1.0, 0.5);
    let saved_by_us = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved_by_us.edited);
    let content = part(&saved_by_us.bytes, "content.xml");
    assert!(
        !content.contains("diacraft:s-Geometry-0")
            && !content.contains("diacraft:fp-s-Geometry-0")
            && !content.contains("diacraft:fp-master")
            && !content.contains("diacraft:master="),
        "the stale items are out of the style: {content}"
    );
    assert!(content.contains("M 0 21600 L 10800 0 21600 21600 Z N"), "LibreOffice's path is the file's");
    let (shape, report) = read_full(&saved_by_us.bytes);
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    assert!(!geometry_of(&shape).unwrap().cells.contains_key("NoFill"));
    assert_eq!(shape.master, None);
    assert_eq!(chars_of(&shape), "Hello\nWorld");
    // The items that were not stale are still carried and still fingerprinted, and a second save is no edit.
    assert!(content.contains("diacraft:fp-text=") && content.contains("diacraft:fp-name="));
    let o = open_bytes(saved_by_us.bytes.clone());
    let again = real(&o.doc, &saved_by_us.bytes, &o.index).unwrap();
    assert!(!again.edited && again.bytes == saved_by_us.bytes, "a second save is no edit");
    let _ = model_shape(&o, id);
}

/// DiaCraft retypes a carried text: the native characters and the fingerprint are written together, so that the file read again has the new text with its
/// markers and nothing stale.
#[test]
fn a_retext_in_diacraft_of_a_carried_text_keeps_the_fingerprint_current() {
    let bytes = saved();
    let mut o = open_bytes(bytes.clone());
    let id = shape_id();
    {
        let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let at = p.shapes.iter().position(|s| s.id == id).unwrap();
        let shape = Arc::make_mut(p.shapes.get_mut(at).unwrap());
        shape.text = Some(Arc::new(Text { parts: vec![TextPart::Cp(0), TextPart::Chars(Arc::from("Bye\nnow"))], ..Default::default() }));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(done.edited);
    let (shape, report) = read_full(&done.bytes);
    assert_eq!(chars_of(&shape), "Bye\nnow");
    assert!(matches!(shape.text.as_ref().unwrap().parts.first(), Some(TextPart::Cp(0))), "the markers are carried");
    assert_eq!(report.warning_counts.get("diacraft_stale"), None);
    assert_ne!(fp_of(&part(&done.bytes, "content.xml"), "text"), fp_of(&part(&bytes, "content.xml"), "text"), "the fingerprint follows the text");
    assert_eq!(fp_of(&part(&done.bytes, "content.xml"), "master"), fp_of(&part(&bytes, "content.xml"), "master"), "the outline's is as it was");
}

/// DiaCraft renames a shape that carries a name: the name's fingerprint is taken out with the change (the item is then not judged, as it was before tier 3), and
/// the file read again has the new name, with nothing stale.
#[test]
fn a_rename_in_diacraft_of_a_carried_name_is_not_taken_for_stale() {
    let bytes = saved();
    let mut o = open_bytes(bytes.clone());
    let id = shape_id();
    {
        let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let at = p.shapes.iter().position(|s| s.id == id).unwrap();
        let shape = Arc::make_mut(p.shapes.get_mut(at).unwrap());
        shape.name = Some(Arc::from("Shown now"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (shape, report) = read_full(&done.bytes);
    assert_eq!(shape.name.as_deref(), Some("Shown now"));
    assert_eq!(report.warning_counts.get("diacraft_stale"), None);
    assert_eq!(chars_of(&shape), "Hello\nWorld");
}

/// DiaCraft edits the outline of a shape whose only tier 2 is the link to its master (the path is written natively, so the geometry is tier 1): the fingerprint of the
/// outline goes with the edit, so that the file read again is not taken for one that LibreOffice edited, and the link is still there.
#[test]
fn an_outline_edit_in_diacraft_of_a_shape_with_a_link_to_its_master_does_not_make_the_link_stale() {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = shape_id();
    let mut shape = rectangle(id, "rect.1", (2.0, 3.0, 4.0, 2.0));
    shape.master = Some(2);
    push_shape(&mut o.doc, shape);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes;
    assert!(fp_of(&part(&first, "content.xml"), "master").is_some(), "the link is fingerprinted by the outline written");
    let mut o = open_bytes(first.clone());
    {
        let shape = crate::write_core::shape_mut(&mut o.doc, 0, id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let geometry = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap();
        let geometry = Arc::make_mut(geometry);
        let mut rows: Vec<Arc<Row>> = geometry.rows.iter().cloned().collect();
        Arc::make_mut(&mut rows[2]).cells.insert(CellName::new("Y"), Cell::with_formula("Height*0.25", Value::from_number(0.25 * 2.0 / 2.54)));
        geometry.rows.clear();
        for row in rows {
            geometry.rows.push(row);
        }
    }
    let done = real(&o.doc, &first, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(done.edited);
    let (back, report) = read(&done.bytes, &Limits::default()).unwrap();
    let back = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone()).unwrap();
    assert_eq!(back.master, Some(2), "the link stands: the outline was edited by DiaCraft, which writes the fingerprint of what it wrote");
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    assert_ne!(part(&done.bytes, "content.xml"), part(&first, "content.xml"));
}

// ------------------------------------------------------------------------------------------------------------------ the review's probes

/// The shape of the first page named `name` of the drawing in `bytes`, read.
fn shape_named(bytes: &[u8], name: &str) -> (Shape, ReadReport) {
    let (doc, report) = read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {e}"));
    let found =
        doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name) || s.name.as_deref() == Some(name)).map(|s| (**s).clone());
    (found.unwrap_or_else(|| panic!("no shape named {name}")), report)
}

/// **Probe E of the review.** A plain shape of a file (a custom shape LibreOffice made, `draw:name="s1"`) is given the name `Display` in DiaCraft and saved:
/// the name is tier 2 (`draw:name` is not rewritten), and **the save stamps the fingerprint of the file's own `draw:name`**, so that a rename in LibreOffice is told. Before,
/// no fingerprint was written for an item an edit added to a shape of a file, tier 3 never applied in the flow open, edit in DiaCraft, edit in LibreOffice, and the rename
/// was lost (`Name` `Display`, `NameU` `s1`).
#[test]
fn a_name_given_in_diacraft_to_a_shape_of_a_file_is_judged_by_the_files_own_name_so_a_libreoffice_rename_wins() {
    let bytes = Drawing::a4().page(&crate::write_core::custom("s1", "1cm")).bytes();
    let mut o = open_bytes(bytes.clone());
    {
        let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        Arc::make_mut(p.shapes.get_mut(0).unwrap()).name = Some(Arc::from("Display"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&done.bytes, "content.xml");
    assert!(content.contains("diacraft:name=\"Display\""), "the name is carried: {content}");
    assert!(fp_of(&content, "name").is_some(), "the save stamps the fingerprint of the file's own name: {content}");
    let (shape, report) = shape_named(&done.bytes, "Display");
    assert_eq!((shape.name.as_deref(), shape.name_u.as_deref()), (Some("Display"), Some("s1")));
    assert_eq!(stale_count(&report), 0, "the file as DiaCraft saved it: nothing is stale");
    // LibreOffice renames the shape: the carried name is stale, and both names are the one it gave.
    let renamed = edited(&done.bytes, |c| c.replace("draw:name=\"s1\"", "draw:name=\"Renamed\""));
    assert_ne!(renamed, done.bytes);
    let (shape, report) = shape_named(&renamed, "Renamed");
    assert_eq!((shape.name.as_deref(), shape.name_u.as_deref()), (Some("Renamed"), Some("Renamed")), "the rename is not lost");
    assert_eq!(stale_count(&report), 1);
}

/// **Probe C of the review.** A shape that carries a name (`Display`, its `draw:name` `rect.1`) is renamed in DiaCraft, and then in LibreOffice: the fingerprint
/// is the file's own `draw:name`'s, which the DiaCraft rename does not rewrite, so it stays and the LibreOffice rename is told. Before, the DiaCraft rename took the
/// fingerprint out and the file read back with `Name` `Shown now` and `NameU` `Renamed`.
#[test]
fn a_carried_name_renamed_in_diacraft_then_in_libreoffice_has_the_name_libreoffice_gave() {
    let bytes = saved();
    let mut o = open_bytes(bytes.clone());
    let id = shape_id();
    {
        let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let at = p.shapes.iter().position(|s| s.id == id).unwrap();
        Arc::make_mut(p.shapes.get_mut(at).unwrap()).name = Some(Arc::from("Shown now"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(fp_of(&part(&done.bytes, "content.xml"), "name").is_some(), "the fingerprint stays");
    let (shape, report) = read_full(&done.bytes);
    assert_eq!(shape.name.as_deref(), Some("Shown now"));
    assert_eq!(stale_count(&report), 0);
    let renamed = edited(&done.bytes, |c| c.replace("draw:name=\"rect.1\"", "draw:name=\"Renamed\""));
    assert_ne!(renamed, done.bytes);
    let (shape, report) = read_full(&renamed);
    assert_eq!((shape.name.as_deref(), shape.name_u.as_deref()), (Some("Renamed"), Some("Renamed")));
    assert_eq!(stale_count(&report), 1);
    assert_eq!(chars_of(&shape), "Hello\nWorld", "the rest of what the shape carries stays");
}

/// **Probe B of the review.** DiaCraft edits the outline of a shape that carries the link to its master, and then LibreOffice edits the path: the save made the
/// fingerprint of the outline again from the path it wrote (and the file's other parts), so the link is told stale. Before, the outline edit took the fingerprint out, the
/// path edit was not told, and the link stood with no stale count.
#[test]
fn an_outline_edited_in_diacraft_and_then_the_path_in_libreoffice_drops_the_link_to_the_master() {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = shape_id();
    let mut shape = rectangle(id, "rect.1", (2.0, 3.0, 4.0, 2.0));
    shape.master = Some(2);
    push_shape(&mut o.doc, shape);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes;
    let mut o = open_bytes(first.clone());
    {
        let shape = crate::write_core::shape_mut(&mut o.doc, 0, id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let geometry = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap());
        let mut rows: Vec<Arc<Row>> = geometry.rows.iter().cloned().collect();
        Arc::make_mut(&mut rows[2]).cells.insert(CellName::new("Y"), Cell::with_formula("Height*0.25", Value::from_number(0.25 * 2.0 / 2.54)));
        geometry.rows.clear();
        for row in rows {
            geometry.rows.push(row);
        }
    }
    let done = real(&o.doc, &first, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(done.edited);
    let content = part(&done.bytes, "content.xml");
    assert!(fp_of(&content, "master").is_some(), "the fingerprint of the outline written is stamped: {content}");
    let (back, report) = read(&done.bytes, &Limits::default()).unwrap();
    let back = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone()).unwrap();
    assert_eq!(back.master, Some(2), "the file as DiaCraft saved it: the link stands");
    assert_eq!(stale_count(&report), 0, "{:?}", report.warning_counts);
    // LibreOffice moves a vertex: the path is another.
    let path = content.split("draw:enhanced-path=\"").nth(1).and_then(|r| r.split('"').next()).expect("the path").to_owned();
    let edited_path = edited(&done.bytes, |c| c.replace(&path, "M 0 0 L 21600 0 L 21600 10000 L 0 21600 Z N"));
    assert_ne!(edited_path, done.bytes);
    let (back, report) = read(&edited_path, &Limits::default()).unwrap();
    let back = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone()).unwrap();
    assert_eq!(back.master, None, "the path was edited in LibreOffice: the link is stale");
    assert_eq!(stale_count(&report), 1);
}

/// A fingerprint the save did not write is stamped by any later save that writes the shape's extension, from the file's own outputs: a file of (items with no
/// fingerprint at all) given another edit of the shape carries them, and a rename in LibreOffice is then told.
#[test]
fn a_file_with_no_fingerprints_is_stamped_by_the_save_that_edits_the_shape_and_a_later_rename_is_told() {
    // The shape: its style carries the name and the link to a master and no fingerprint.
    let attrs =
        r#"diacraft:version="1" diacraft:id="4" diacraft:name="Process" diacraft:name-u="Process.4" diacraft:master="2" diacraft:master-shape="5""#;
    let style = format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#);
    let shape = r#"<draw:custom-shape draw:style-name="gr1" draw:name="Box" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let bytes = Drawing::a4().auto(&style).page(shape).bytes();
    let mut o = open_bytes(bytes.clone());
    assert!(!part(&bytes, "content.xml").contains("diacraft:fp-"));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    // Any edit of the shape that writes its extension: here, a note in a cell that no markup holds.
    crate::write_core::shape_mut(&mut o.doc, 0, id);
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("Comment"), text_cell("a note"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&done.bytes, "content.xml");
    for key in ["name", "master"] {
        assert!(fp_of(&content, key).is_some(), "fp-{key} is stamped: {content}");
    }
    let (read_back, report) = shape_named(&done.bytes, "Process");
    assert_eq!((read_back.master, stale_count(&report)), (Some(2), 0));
    let renamed = edited(&done.bytes, |c| c.replace("draw:name=\"Box\"", "draw:name=\"Renamed\""));
    let (read_back, report) = shape_named(&renamed, "Renamed");
    assert_eq!((read_back.name.as_deref(), read_back.name_u.as_deref()), (Some("Renamed"), Some("Renamed")));
    assert_eq!(stale_count(&report), 1);
}

/// A file of whose carried text is not the text its markup holds (another program retyped it before the file was opened: with no fingerprint, the carried text
/// wins): a save of the shape stamps the fingerprint of the file's own characters, not of the carried text's, so that the file as saved reads back with the carried text
/// (the check of the bytes made does not refuse the save) and a later retype in LibreOffice is told.
#[test]
fn a_carried_text_that_is_not_the_files_own_is_stamped_from_the_files_own_and_the_save_is_not_refused() {
    let text = "&lt;Text&gt;carried&lt;/Text&gt;";
    let style = format!(
        r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" diacraft:version="1" diacraft:id="4" diacraft:text="{text}"/></style:style>"#
    );
    let shape = r#"<draw:custom-shape draw:style-name="gr1" draw:name="Box" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p>native</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let bytes = Drawing::a4().auto(&style).page(shape).bytes();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    assert_eq!(chars_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap()), "carried", "no fingerprint: the carried text wins");
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("Comment"), text_cell("a note"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&done.bytes, "content.xml");
    assert_eq!(
        fp_of(&content, "text"),
        Some("e57ce40d00b6d02f".to_owned()),
        "the fingerprint is of the file's own characters (FNV-1a of the field `native`, worked out apart)"
    );
    let (doc, report) = read(&done.bytes, &Limits::default()).unwrap();
    assert_eq!(chars_of(doc.pages.get(0).unwrap().shapes.get(0).unwrap()), "carried");
    assert_eq!(stale_count(&report), 0);
    // LibreOffice retypes the text: the carried one is stale.
    let retyped = edited(&done.bytes, |c| c.replace("<text:p>native</text:p>", "<text:p>typed</text:p>"));
    let (doc, report) = read(&retyped, &Limits::default()).unwrap();
    assert_eq!(chars_of(doc.pages.get(0).unwrap().shapes.get(0).unwrap()), "typed");
    assert_eq!(stale_count(&report), 1);
}

/// **Probe D of the review.** A turned shape of a file whose `Angle` is a formula (`30 deg`: any formula turn is carried) and whose pin is at its left: the pin
/// is derived all the same (the carried turn is the markup's), so DiaCraft's own save reads the pin where the session had it, and a move in LibreOffice moves the pin
/// with it, also when LibreOffice writes the angle on its own grid (0.5236 as 0.52364). Before, a carried `Angle` made the reader leave the pin alone and it read at
/// the centre. A carried turn that says another turn than the markup's is left alone.
#[test]
fn a_turned_shape_whose_angle_is_a_formula_keeps_its_pin_and_follows_a_libreoffice_move() {
    let bytes = turned_file();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("t")).map(|s| s.id).unwrap();
    let (cx, cy, _) = pins_of(&bytes);
    let width = 6.0 / 2.54;
    let angle = 0.5235987755982988_f64;
    let (px, py) = (cx - angle.cos() * width / 2.0, cy - angle.sin() * width / 2.0);
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("Angle"), Cell::with_formula("30 deg", Value::from_number(angle)).with_unit("DEG"));
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(px).unwrap()).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinY"), Cell::constant(Value::from_number(py).unwrap()).with_unit("IN"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (rx, ry, place) = pins_of(&done.bytes);
    assert_eq!(place.as_deref(), Some("Width*0"));
    assert!((rx - px).abs() < 1e-6 && (ry - py).abs() < 1e-6, "the pin reads back where the session had it: ({rx}, {ry}) against ({px}, {py})");
    // A move in LibreOffice, and the angle as LibreOffice writes it on its grid: the pin is 5 cm further.
    let moved = edited(&done.bytes, |c| c.replace("10.5cm", "15.5cm").replace("rotate (0.5235987755982988)", "rotate (0.52364)"));
    assert_ne!(moved, done.bytes);
    let (mx, my, place) = pins_of(&moved);
    assert_eq!(place.as_deref(), Some("Width*0"));
    // (LibreOffice's grid for the angle moves the pin by the width times the difference of 4e-5 radian: under 1e-4 inch.)
    assert!((mx - px - 5.0 / 2.54).abs() < 1e-4 && (my - py).abs() < 1e-4, "the pin follows the move: ({mx}, {my}) from ({px}, {py})");
    // A carried turn that is not the markup's (LibreOffice turned the shape to 40 degrees): the pin is left alone, as before.
    let turn = |c: String| c.replace("rotate (0.5235987755982988)", "rotate (0.6981317007977318)");
    let (tx, ty, _) = pins_of(&edited(&done.bytes, turn));
    let (ux, uy, _) = pins_of(&edited(&bytes, turn));
    assert!(
        (tx - ux).abs() < 1e-6 && (ty - uy).abs() < 1e-6,
        "the pin is the markup's centre, not derived by a turn that is not the markup's: ({tx}, {ty}) against ({ux}, {uy})"
    );
}

// ------------------------------------------------------------------------------------------------------------------ hostile input

/// A fingerprint that is not 16 lowercase hexadecimal digits is no fingerprint: the item is not judged and stays, with one warning, whatever the length (0, 15, 17 and
/// a very long one) and whatever the bytes.
#[test]
fn a_fingerprint_of_the_wrong_length_is_read_as_none_and_the_item_stays() {
    let bytes = saved();
    let text = fp_of(&part(&bytes, "content.xml"), "text").unwrap();
    for bad in ["", "0123456789abcde", "0123456789abcdef0", &"f".repeat(100_000), "0123456789ABCDEF", "zzzzzzzzzzzzzzzz", "0123456789abcde&#10;"] {
        let wrong = edited(&bytes, |c| c.replace(&format!("diacraft:fp-text=\"{text}\""), &format!("diacraft:fp-text=\"{bad}\"")));
        let (shape, report) = read_full(&wrong);
        assert!(
            matches!(shape.text.as_ref().unwrap().parts.first(), Some(TextPart::Cp(0))),
            "{:?}: the carried text stays",
            &bad[..bad.len().min(20)]
        );
        assert_eq!(report.warning_counts.get("diacraft_fingerprint_unreadable"), Some(&1), "{:?}", &bad[..bad.len().min(20)]);
        assert_eq!(stale_count(&report), 0);
        // A retyped text with a fingerprint that is none is not judged either: the carried text stays.
        let retyped = edited(&wrong, |c| c.replace("<text:p>Hello</text:p>", "<text:p>Hallo</text:p>"));
        let (shape, _) = read_full(&retyped);
        assert_eq!(chars_of(&shape), "Hello\nWorld", "{:?}", &bad[..bad.len().min(20)]);
    }
}

/// A fingerprint is no integrity check: a file that holds the fingerprint of other outputs than its own is judged by it. Two different texts with one forged
/// fingerprint are one text to tier 3, and nothing breaks; an item with no fingerprint of its own is not judged by another's.
#[test]
fn a_fingerprint_that_collides_with_another_judges_by_it_and_breaks_nothing() {
    let bytes = saved();
    let content = part(&bytes, "content.xml");
    let (text, name) = (fp_of(&content, "text").unwrap(), fp_of(&content, "name").unwrap());
    // The fingerprint of the retyped text, as a file written with that text holds it.
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let mut other = full_shape();
    other.text =
        Some(Arc::new(Text { parts: vec![TextPart::Cp(0), TextPart::Chars(Arc::from("Hallo\nWorld")), TextPart::Pp(1)], ..Default::default() }));
    push_shape(&mut o.doc, other);
    o.doc.masters.push(Arc::new(master(2)));
    let theirs = fp_of(&part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml"), "text").unwrap();
    assert_ne!(theirs, text);
    // LibreOffice retyped the text, and the file's fingerprint is the retyped text's: the carried text is taken for current.
    let forged = edited(&bytes, |c| {
        c.replace("<text:p>Hello</text:p>", "<text:p>Hallo</text:p>")
            .replace(&format!("diacraft:fp-text=\"{text}\""), &format!("diacraft:fp-text=\"{theirs}\""))
    });
    let (shape, report) = read_full(&forged);
    assert_eq!(stale_count(&report), 0);
    assert_eq!(chars_of(&shape), "Hello\nWorld", "the carried text stands: a fingerprint is no integrity check");
    // The name's fingerprint on the text: judged by the text's outputs, which differ.
    let swapped = edited(&bytes, |c| c.replace(&format!("diacraft:fp-text=\"{text}\""), &format!("diacraft:fp-text=\"{name}\"")));
    let (shape, report) = read_full(&swapped);
    assert_eq!(stale_count(&report), 1);
    assert_eq!(chars_of(&shape), "Hello\nWorld", "the text of the markup stands");
    assert!(!matches!(shape.text.as_ref().unwrap().parts.first(), Some(TextPart::Cp(0))));
    assert_eq!(shape.name.as_deref(), Some("Display"), "the name has its own fingerprint");
}

/// A fingerprint of an item the style does not carry judges nothing, one of a key this build does not know is kept as it is, and neither is a crash.
#[test]
fn a_fingerprint_with_no_item_or_of_a_key_this_build_does_not_know_judges_nothing() {
    let bytes = saved();
    let extra = edited(&bytes, |c| {
        c.replace(
            "diacraft:fp-text=",
            "diacraft:fp-c-LinePattern=\"0123456789abcdef\" diacraft:fp-future=\"zz\" diacraft:fp-s-User=\"0123456789abcdef\" diacraft:fp-text=",
        )
    });
    let (shape, report) = read_full(&extra);
    assert_all_carried(&shape, "with fingerprints that judge nothing");
    assert_eq!(stale_count(&report), 0);
    // A save that edits the shape writes the fingerprints it owns again and keeps the one of a newer build's key.
    let mut o = open_bytes(extra.clone());
    crate::write_core::move_by(&mut o.doc, 0, shape_id(), 1.0, 0.0);
    let done = real(&o.doc, &extra, &o.index).unwrap();
    let after = part(&done.bytes, "content.xml");
    assert!(after.contains("diacraft:fp-future=\"zz\""), "a key this build does not know is kept: {after}");
}

/// `n` shapes, each on a style of its own that carries a text and the fingerprint of other outputs than the shape's own: every text is stale.
fn stale_texts(n: usize) -> Vec<u8> {
    let (mut auto, mut shapes) = (String::new(), String::new());
    for k in 0..n {
        let text = "&lt;Text&gt;carried&lt;/Text&gt;";
        auto.push_str(&format!("<style:style style:name=\"g{k}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" diacraft:version=\"1\" diacraft:id=\"{}\" diacraft:text=\"{text}\" diacraft:fp-text=\"0123456789abcdef\"/></style:style>", k + 1));
        shapes.push_str(&format!(
            "<draw:rect draw:style-name=\"g{k}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p>native</text:p></draw:rect>"
        ));
    }
    Drawing::a4().auto(&auto).page(&shapes).bytes()
}

/// Many stale items cost work in proportion to their number, and each is dropped: the work of a read of n shapes whose texts are all stale, counted in the reader's
/// items, is as the work of 4n shapes divided by 4 (a pass over the items for each item would be the square), and the words are one line however many items there are.
#[test]
fn many_stale_items_are_dropped_and_cost_work_in_proportion_to_their_number() {
    let cost = |n: usize| {
        let (doc, report) = read(&stale_texts(n), &Limits::default()).unwrap();
        let page = doc.pages.get(0).unwrap();
        assert_eq!(page.shapes.len(), n);
        assert!(page.shapes.iter().all(|s| chars_of(s) == "native"), "every carried text is stale, and the markup's text stands");
        assert_eq!(report.warning_counts.get("diacraft_stale"), Some(&(n as u64)), "every item is counted");
        assert_eq!(report.warnings.iter().filter(|w| w.contains("left out where what it was written for changed")).count(), 1, "and named once");
        (report.work.items, report.work.name_bytes)
    };
    let (small, big) = (cost(500), cost(2_000));
    let ratio = big.0 as f64 / small.0 as f64;
    assert!((3.6..4.4).contains(&ratio), "items {} for 500 shapes and {} for 2,000: {ratio}", small.0, big.0);
    assert!(big.1 <= 4 * small.1 + 64, "the names looked up: {} and {}", small.1, big.1);
}

/// `n` custom shapes, each on a style of its own that carries a text, a name and a link to a master and **no fingerprint** (a file, or items an edit added): the
/// natives they rest on are made for each, and kept for the save.
fn unfingerprinted(n: usize) -> Vec<u8> {
    let (mut auto, mut shapes) = (String::new(), String::new());
    for k in 0..n {
        let text = "&lt;Text&gt;carried&lt;/Text&gt;";
        auto.push_str(&format!("<style:style style:name=\"g{k}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" diacraft:version=\"1\" diacraft:id=\"{}\" diacraft:text=\"{text}\" diacraft:name=\"N{k}\" diacraft:master=\"2\"/></style:style>", k + 1));
        shapes.push_str(&format!(
            "<draw:custom-shape draw:style-name=\"g{k}\" draw:name=\"s{k}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p>native</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 Z N\"/></draw:custom-shape>"
        ));
    }
    Drawing::a4().auto(&auto).page(&shapes).bytes()
}

/// The natives the items rest on are made for every item that holds one, with or without a fingerprint, and cost work in proportion to the number of shapes (counted in the
/// reader's items: 4n shapes cost four times n); no item is stale for want of a fingerprint, and nothing is said.
#[test]
fn items_with_no_fingerprint_cost_natives_in_proportion_to_their_number_and_are_never_stale() {
    let cost = |n: usize| {
        let (doc, report) = read(&unfingerprinted(n), &Limits::default()).unwrap();
        let page = doc.pages.get(0).unwrap();
        assert_eq!(page.shapes.len(), n);
        assert!(
            page.shapes
                .iter()
                .all(|s| chars_of(s) == "carried" && s.master == Some(2) && s.name.as_deref().is_some_and(|name| name.starts_with('N')))
        );
        assert_eq!(stale_count(&report), 0);
        assert_eq!(report.warning_counts.get("diacraft_fingerprint_unreadable"), None);
        report.work.items
    };
    let (small, big) = (cost(500), cost(2_000));
    let ratio = big as f64 / small as f64;
    assert!((3.6..4.4).contains(&ratio), "items {small} for 500 shapes and {big} for 2,000: {ratio}");
}

/// One shape whose style carries thousands of `Geometry` sections, each with a fingerprint that is stale: they are taken out in one pass, and the work is in
/// proportion to their number.
#[test]
fn thousands_of_stale_geometry_sections_of_one_shape_cost_work_in_proportion_to_their_number() {
    let cost = |n: usize| {
        let mut attrs = String::from("diacraft:version=\"1\" diacraft:id=\"3\"");
        for k in 0..n {
            attrs.push_str(&format!(" diacraft:s-Geometry-{k}=\"&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;{k}&quot;/&gt;&lt;/Sheet&gt;\" diacraft:fp-s-Geometry-{k}=\"0123456789abcdef\""));
        }
        let style =
            format!("<style:style style:name=\"g\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" {attrs}/></style:style>");
        let shape = "<draw:custom-shape draw:style-name=\"g\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 Z N\"/></draw:custom-shape>";
        let (doc, report) = read(&Drawing::a4().auto(&style).page(shape).bytes(), &Limits::default()).unwrap();
        let sections = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count();
        assert_eq!(sections, 1, "the sections the markup says, and none of the {n} carried");
        assert_eq!(report.warning_counts.get("diacraft_stale"), Some(&(n as u64)));
        report.work.items
    };
    let (small, big) = (cost(500), cost(2_000));
    let ratio = big as f64 / small as f64;
    assert!((3.0..4.4).contains(&ratio), "items {small} for 500 sections and {big} for 2,000: {ratio}");
}

// ------------------------------------------------------------------------------------------------------------------ V9, and a flat drawing

/// The second sentence, as far as the code of today goes: a shape whose style no longer carries any attribute of DiaCraft's (LibreOffice cleared
/// the formatting, R42 (a)) is read from its native markup alone, and the document file's data for it is judged by tier 3. The document file holds per-shape data of
/// groups only (writes them), so a shape of this kind has nothing in it to judge: the file's own masters stand, and the shape is the markup's, with no item
/// stale and no warning.
#[test]
fn odg_ownership_a_shape_whose_style_lost_its_attributes_is_read_from_its_markup_alone() {
    let bytes = saved();
    let cleared = edited(&bytes, |c| {
        let at = c.find("diacraft:version=").unwrap();
        let end = c[at..].find("/></style:style>").unwrap();
        let mut t = c.clone();
        t.replace_range(at..at + end, "");
        t
    });
    assert!(!part(&cleared, "content.xml").contains(" diacraft:"), "no attribute of the extension is left");
    let (doc, report) = read(&cleared, &Limits::default()).unwrap();
    let shape =
        doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("rect.1")).map(|s| (**s).clone()).expect("read by its draw:name");
    assert_eq!((shape.master, chars_of(&shape).as_str()), (None, "Hello\nWorld"));
    assert!(shape.sheet.cell("Comment").is_none() && !geometry_of(&shape).is_some_and(|g| g.cells.contains_key("NoFill")));
    assert!(!report.index.page(0).unwrap().has_tier2(shape.id), "it carries no tier 2");
    assert_eq!(stale_count(&report), 0);
    assert_eq!(doc.masters.len(), 1, "the document file's masters stand");
}

/// A flat drawing is judged as a package is: the one document holds the style and the markup, so a path edited, a text retyped or a name changed in another program
/// makes the item stale, and a move or the rewrites of a save with no edit do not.
#[test]
fn a_flat_drawing_judges_tier_3_as_a_package_does() {
    use crate::flat_write::{save, text_of, three};
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    let mut shape = full_shape();
    shape.id = id;
    shape.master = None;
    push_shape(&mut o.doc, shape);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let flat = text_of(&saved.bytes);
    for key in ["text", "name", "s-Geometry-0", "c-FillForegnd"] {
        assert!(fp_of(&flat, key).is_some(), "fp-{key} in a flat drawing: {flat}");
    }
    let read_new = |text: &str| {
        let (doc, report) = read(text.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("not read: {e}"));
        let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| (**s).clone()).unwrap();
        (shape, report)
    };
    let (shape, report) = read_new(&flat);
    assert_eq!((chars_of(&shape).as_str(), shape.name.as_deref()), ("Hello\nWorld", Some("Display")));
    assert_eq!(stale_count(&report), 0);
    let (moved, report) = read_new(&flat.replace("svg:x=\"2cm\"", "svg:x=\"9cm\""));
    assert!(geometry_of(&moved).is_some_and(|g| g.cells.contains_key("NoFill")), "a move keeps the geometry");
    assert_eq!(stale_count(&report), 0);
    let (edited_path, report) = read_new(
        &flat.replace("draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"", "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""),
    );
    assert!(!geometry_of(&edited_path).is_some_and(|g| g.cells.contains_key("NoFill")));
    assert_eq!(stale_count(&report), 1);
    let (retyped, report) = read_new(&flat.replace("<text:p>Hello</text:p>", "<text:p>Hallo</text:p>"));
    assert_eq!(chars_of(&retyped), "Hallo\nWorld");
    assert_eq!(stale_count(&report), 1);
}

// ------------------------------------------------------------------------------------------------------------------ a position computed

/// A new shape whose pin is at its left edge (`LocPinX` is `Width*0`, `PinX` the left edge), so that the cells that carry its frame are tier 2: they compute the
/// place the markup says too.
fn pinned_shape() -> Shape {
    let mut shape = rectangle(shape_id(), "rect.1", (2.0, 3.0, 4.0, 2.0));
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)));
    sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(2.0 / 2.54).unwrap()).with_unit("IN"));
    shape
}

fn saved_pinned() -> Vec<u8> {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    push_shape(&mut o.doc, pinned_shape());
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes
}

fn number_of(bytes: &[u8], cell: &str) -> (Option<f64>, Option<String>) {
    let (doc, _) = read(bytes, &Limits::default()).unwrap();
    let s = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == shape_id()).cloned().unwrap();
    (s.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()), s.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned)))
}

/// **A position is the markup's, and the pin is derived from it.** A shape whose pin is not its centre carries its pin's place (`LocPinX`). When LibreOffice moves it (
/// `svg:x` changes and nothing else) the pin is the markup's edge plus that place, so the shape stays where LibreOffice put it, its pin stays at the left edge, and nothing is
/// stale. Before this was derived (the pin was carried as a number) the move was silently undone. A resize that keeps the left edge keeps the pin, and a move below
/// LibreOffice's grid (0.01 mm) is none.
#[test]
fn libreoffice_moved_shape_with_a_carried_pin_stays_where_it_was_put() {
    let bytes = saved_pinned();
    let content = part(&bytes, "content.xml");
    assert!(content.contains("N=&quot;LocPinX&quot;"), "the pin's place is carried: {content}");
    assert!(!content.contains("N=&quot;PinX&quot;"), "the pin's position is the markup's edge plus the place, and is not carried: {content}");
    let pin = |b: &[u8]| number_of(b, "PinX").0.unwrap() * 2.54;
    assert!((pin(&bytes) - 2.0).abs() < 1e-9, "as saved the pin is at the left edge");
    // Moved 7 cm right: the shape is at 9 cm, its pin at the left edge still.
    let moved = edited(&bytes, |c| c.replace("svg:x=\"2cm\"", "svg:x=\"9cm\""));
    assert!((pin(&moved) - 9.0).abs() < 1e-9, "the pin follows the shape: {}", pin(&moved));
    let (_, place) = number_of(&moved, "LocPinX");
    assert_eq!(place.as_deref(), Some("Width*0"), "the pin's place is carried still");
    let (_, report) = read(&moved, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    // A move below the grid LibreOffice keeps (0.01 mm) is none.
    let nudged = edited(&bytes, |c| c.replace("svg:x=\"2cm\"", "svg:x=\"2.0001cm\""));
    assert!((pin(&nudged) - 2.0001).abs() < 1e-6);
    // A resize that keeps the left edge: the pin stays, and the size is the markup's.
    let resized = edited(&bytes, |c| c.replace("svg:width=\"4cm\"", "svg:width=\"6cm\""));
    assert!((number_of(&resized, "Width").0.unwrap() * 2.54 - 6.0).abs() < 1e-9);
    assert!((pin(&resized) - 2.0).abs() < 1e-9);
    // A move on the other axis puts the bottom edge where the markup has it: the pin's place `LocPinY` is the default, so the pin is the centre.
    let down = edited(&bytes, |c| c.replace("svg:y=\"3cm\"", "svg:y=\"8cm\""));
    assert!((number_of(&down, "PinX").0.unwrap() * 2.54 - 2.0).abs() < 1e-9, "x is as it was");
}

/// A carried size is judged by the markup's: `Width` set by a formula (`Height*2`) is carried; LibreOffice resizes the shape, and the size the markup says wins.
#[test]
fn a_size_carried_as_a_formula_is_stale_when_libreoffice_resizes_the_shape() {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let mut shape = rectangle(shape_id(), "rect.1", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet)
        .cells
        .insert(CellName::new("Width"), Cell::with_formula("Height*2", Value::from_number(4.0 / 2.54)).with_unit("IN"));
    push_shape(&mut o.doc, shape);
    let bytes = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes;
    assert_eq!(number_of(&bytes, "Width").1.as_deref(), Some("Height*2"));
    let still = number_of(&edited(&bytes, |c| c.replace("svg:y=\"3cm\"", "svg:y=\"8cm\"")), "Width");
    assert_eq!(still.1.as_deref(), Some("Height*2"), "a move leaves the size");
    let resized = edited(&bytes, |c| c.replace("svg:width=\"4cm\"", "svg:width=\"7cm\""));
    let (w, f) = number_of(&resized, "Width");
    assert!((w.unwrap() * 2.54 - 7.0).abs() < 1e-9 && f.is_none(), "the markup's width, with no formula: {w:?} {f:?}");
    let (_, report) = read(&resized, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale"), Some(&1));
}

/// A rectangle of the file: 6 cm by 2 cm, **turned** 30 degrees (radians, positive counterclockwise) about its frame's origin and then put at (10.5 cm, 7.5 cm) of the page
/// Its pin is the frame's centre when it is read.
fn turned_file() -> Vec<u8> {
    Drawing::a4()
        .page(r#"<draw:rect draw:name="t" svg:x="0cm" svg:y="0cm" svg:width="6cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (10.5cm 7.5cm)"><text:p/></draw:rect>"#)
        .bytes()
}

/// The pin cells of the shape named `t` of the file, in inches.
fn pins_of(bytes: &[u8]) -> (f64, f64, Option<String>) {
    let (doc, _) = read(bytes, &Limits::default()).unwrap();
    let s = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("t")).cloned().unwrap();
    let n = |c: &str| s.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).unwrap();
    (n("PinX"), n("PinY"), s.sheet.cell("LocPinX").and_then(|c| c.formula.text().map(str::to_owned)))
}

/// **The bug the gate showed, and its fix.** `shape.pin` puts the pin at another place of the shape's box and moves the pin's position so that the shape draws where it did
/// The file had the formulas and the frame, and **read back with the pin at the frame's centre and its place at 0**: in the session that opened the file again the
/// shape sat half a width from where it was drawn, and no test saw it (the engine's checks the formula text and the frame's attribute text). The pin is the markup's edge
/// plus the carried place now, for a shape that is turned as well: here a shape turned 30 degrees whose pin is put at the left end of its middle line. And a move in
/// LibreOffice (the `translate`) is then followed by the pin.
#[test]
fn a_pin_put_at_the_left_of_a_turned_shape_of_the_file_reads_back_where_the_session_had_it_and_follows_a_libreoffice_move() {
    let bytes = turned_file();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("t")).map(|s| s.id).unwrap();
    let (cx, cy, _) = pins_of(&bytes);
    let width = 6.0 / 2.54;
    let angle = 0.5235987755982988_f64;
    // The pin at the left end of the middle line, the shape drawn where it was: the pin is the centre less half the width along the turned x axis.
    let (px, py) = (cx - angle.cos() * width / 2.0, cy - angle.sin() * width / 2.0);
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(px).unwrap()).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinY"), Cell::constant(Value::from_number(py).unwrap()).with_unit("IN"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (rx, ry, place) = pins_of(&done.bytes);
    assert_eq!(place.as_deref(), Some("Width*0"), "the place is carried");
    assert!((rx - px).abs() < 1e-6 && (ry - py).abs() < 1e-6, "the pin reads back where the session had it: ({rx}, {ry}) against ({px}, {py})");
    // LibreOffice moves the shape 5 cm to the right: the pin follows the shape.
    let moved = edited(&done.bytes, |c| c.replace("10.5cm", "15.5cm"));
    assert_ne!(moved, done.bytes, "the transform's translate is in the file as written");
    let (mx, my, place) = pins_of(&moved);
    assert_eq!(place.as_deref(), Some("Width*0"));
    assert!((mx - px - 5.0 / 2.54).abs() < 1e-6 && (my - py).abs() < 1e-6, "the pin is 5 cm further and as high: ({mx}, {my}) from ({px}, {py})");
    let (_, report) = read(&moved, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
}

/// A custom shape of 6 by 2 centimetres with the mirrors `h` and `v`, turned 30 degrees and put at (10.5 cm, 7.5 cm); the name is `t`.
fn flipped_file(h: bool, v: bool) -> Vec<u8> {
    Drawing::a4()
        .page(&format!(
            r#"<draw:custom-shape draw:name="t" svg:width="6cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (10.5cm 7.5cm)"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:mirror-horizontal="{h}" draw:mirror-vertical="{v}" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        ))
        .bytes()
}

/// The same for a shape that is **flipped as well as turned**, with each of the four combinations of mirrors and the pin at the corner of its box: the pin is the centre less the map of the pin's place, the map
/// being the turn after the flips (the writer's own), and a move in LibreOffice is followed.
#[test]
fn a_pin_put_at_the_left_of_a_flipped_and_turned_shape_of_the_file_reads_back_where_the_session_had_it() {
    let file_angle = 0.5235987755982988_f64;
    let width = 6.0 / 2.54;
    for (h, v) in [(false, false), (true, false), (false, true), (true, true)] {
        let what = format!("mirrors {h} and {v}");
        // The turn the model has is the file's, and minus it beside one mirror: LibreOffice draws one mirror on the turned frame.
        let angle = if h != v { -file_angle } else { file_angle };
        let bytes = flipped_file(h, v);
        let mut o = open_bytes(bytes.clone());
        let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("t")).map(|s| s.id).unwrap();
        let (cx, cy, _) = pins_of(&bytes);
        // The pin at the bottom left corner of the box: the centre less the map of (w/2, h/2), the flips first and then the turn.
        let (fx, fy) = (if h { -1.0 } else { 1.0 }, if v { -1.0 } else { 1.0 });
        let height = 2.0 / 2.54;
        let (u, w) = (width / 2.0, height / 2.0);
        let (px, py) = (cx - (angle.cos() * fx * u - angle.sin() * fy * w), cy - (angle.sin() * fx * u + angle.cos() * fy * w));
        {
            let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
            sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)).with_unit("IN"));
            sheet.cells.insert(CellName::new("LocPinY"), Cell::with_formula("Height*0", Value::from_number(0.0)).with_unit("IN"));
            sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(px).unwrap()).with_unit("IN"));
            sheet.cells.insert(CellName::new("PinY"), Cell::constant(Value::from_number(py).unwrap()).with_unit("IN"));
        }
        let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("{what}: not saved: {e}"));
        let (rx, ry, place) = pins_of(&done.bytes);
        assert_eq!(place.as_deref(), Some("Width*0"), "{what}");
        assert!(
            (rx - px).abs() < 1e-6 && (ry - py).abs() < 1e-6,
            "{what}: the pin reads back where the session had it: ({rx}, {ry}) against ({px}, {py})"
        );
        let moved = edited(&done.bytes, |c| c.replace("10.5cm", "15.5cm"));
        assert_ne!(moved, done.bytes, "{what}");
        let (mx, my, place) = pins_of(&moved);
        assert_eq!(place.as_deref(), Some("Width*0"), "{what}");
        assert!(
            (mx - px - 5.0 / 2.54).abs() < 1e-6 && (my - py).abs() < 1e-6,
            "{what}: the pin is 5 cm further and as high: ({mx}, {my}) from ({px}, {py})"
        );
    }
}

/// The same for a shape of the file that is not turned: the pin at the left of the middle line, the shape drawn where it was (the shape of the gate's failure in its
/// plainest form: `LocPinX` set to `Width*0` and `PinX` put at the left edge).
#[test]
fn a_pin_put_at_the_left_of_a_shape_of_the_file_reads_back_where_the_session_had_it() {
    let bytes = Drawing::a4().page(&crate::write_core::custom("s1", "1cm")).bytes();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).map(|s| s.id).unwrap();
    let before = {
        let s = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
        let n = |c: &str| s.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).unwrap();
        (n("PinX"), n("Width"))
    };
    let left = before.0 - before.1 / 2.0;
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(left).unwrap()).with_unit("IN"));
    }
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (back, _) = read(&done.bytes, &Limits::default()).unwrap();
    let s = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    let n = |c: &str| s.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!(
        (n("PinX") - left).abs() < 1e-9,
        "the pin is the left edge ({}), as the session had it ({left}), and not the centre ({})",
        n("PinX"),
        before.0
    );
    assert!(n("LocPinX").abs() < 1e-12);
    assert!(
        !real(&open_bytes(done.bytes.clone()).doc, &done.bytes, &open_bytes(done.bytes.clone()).index).unwrap().edited,
        "a second save is no edit"
    );
}

/// DiaCraft moves a shape with a carried pin place: the place and the frame are written together, so the file read again has the pin at the left edge where DiaCraft put
/// the shape, and nothing is stale. The loop that follows: LibreOffice then moves it, and the pin follows the move.
#[test]
fn a_move_in_diacraft_of_a_shape_with_a_carried_pin_keeps_the_pin_and_a_later_libreoffice_move_still_wins() {
    let bytes = saved_pinned();
    let mut o = open_bytes(bytes.clone());
    crate::write_core::move_by(&mut o.doc, 0, shape_id(), 3.0, 0.0);
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (x, f) = number_of(&done.bytes, "LocPinX");
    assert_eq!((x, f.as_deref()), (Some(0.0), Some("Width*0")), "the carried pin place stands after DiaCraft's own move");
    assert!((number_of(&done.bytes, "PinX").0.unwrap() * 2.54 - 5.0).abs() < 1e-9, "at 5 cm");
    let (_, report) = read(&done.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    // The second save is no edit, and a LibreOffice move of what DiaCraft saved is the markup's, with the pin at the left edge.
    let again = open_bytes(done.bytes.clone());
    assert!(!real(&again.doc, &done.bytes, &again.index).unwrap().edited);
    let moved = edited(&done.bytes, |c| c.replace("svg:x=\"5cm\"", "svg:x=\"12cm\""));
    assert!((number_of(&moved, "PinX").0.unwrap() * 2.54 - 12.0).abs() < 1e-9, "the shape is at 12 cm and its pin is at its left edge");
    let (_, report) = read(&moved, &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_stale"), None);
}

/// LibreOffice moved a shape that carries its pin's place; DiaCraft then moves it: the file read again has the shape where DiaCraft put it with its pin at the left edge,
/// the pin's position is not carried (it is the markup's edge plus the place), and a second save is no edit.
#[test]
fn a_move_in_diacraft_after_a_libreoffice_move_keeps_the_pin_at_the_edge() {
    let bytes = edited(&saved_pinned(), |c| c.replace("svg:x=\"2cm\"", "svg:x=\"9cm\""));
    let mut o = open_bytes(bytes.clone());
    crate::write_core::move_by(&mut o.doc, 0, shape_id(), 1.0, 0.0);
    let done = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(done.edited);
    let content = part(&done.bytes, "content.xml");
    assert!(
        content.contains("N=&quot;LocPinX&quot;") && !content.contains("N=&quot;PinX&quot;"),
        "the place is carried and the pin's position is not: {content}"
    );
    let (doc, report) = read(&done.bytes, &Limits::default()).unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == shape_id()).cloned().unwrap();
    let num = |c: &str| shape.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!(((num("PinX") - num("LocPinX")) * 2.54 - 10.0).abs() < 1e-9, "the left edge is at 9 + 1 cm");
    assert!((num("LocPinX")).abs() < 1e-12, "the pin is at the left edge");
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
    let again = open_bytes(done.bytes.clone());
    assert!(!real(&again.doc, &done.bytes, &again.index).unwrap().edited);
}

/// The fuzz of the writer (`odg_write`) found a new connector whose `Width` is not the distance of its ends (the model disagrees with itself): the carried
/// `Width` was judged stale by the check of the file made, which refused the save as a failure of the writer. A line's frame is its ends; its `Width` is not compared.
#[test]
fn a_new_connector_whose_width_is_not_the_distance_of_its_ends_is_saved_and_reads_back() {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = shape_id();
    let mut connector = crate::write_new::connector(id, (1.0, 1.0, 5.0, 3.0));
    let width = connector.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()).unwrap();
    Arc::make_mut(&mut connector.sheet)
        .cells
        .insert(CellName::new("Width"), Cell::constant(Value::from_number(width + 50.0 / 7.0).unwrap()).with_unit("IN"));
    push_shape(&mut o.doc, connector);
    let done = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a connector with a Width of its own is refused: {e}"));
    let (back, report) = read(&done.bytes, &Limits::default()).unwrap();
    let shape = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).cloned().unwrap();
    assert!(
        (shape.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()).unwrap() - (width + 50.0 / 7.0)).abs() < 1e-9,
        "the carried Width stands"
    );
    assert_eq!(report.warning_counts.get("diacraft_stale"), None, "{:?}", report.warning_counts);
}

/// **The second save of a session does not lose what the first knew.** LibreOffice edited the path of a shape; DiaCraft opens it, edits another shape and saves; with
/// the bytes and the index that save gave, the stale shape is moved and saved again: its style no longer carries the stale geometry, its fingerprints or its link.
#[test]
fn a_second_save_in_one_session_does_not_write_the_stale_items_back() {
    let path = "draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"";
    let bytes = edited(&saved(), |c| c.replace(path, "draw:enhanced-path=\"M 0 21600 L 10800 0 21600 21600 Z N\""));
    let mut o = open_bytes(bytes.clone());
    // The first save edits the other shape (`s1`, the first of the page).
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut first.sheet);
        let x = sheet.cells.get("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
        sheet.cells.get_mut("PinX").unwrap().value = Value::from_number(x + 0.5);
    }
    let first_save = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(first_save.edited);
    assert!(part(&first_save.bytes, "content.xml").contains("diacraft:s-Geometry-0"), "the stale shape was not touched by that save");
    // The second save, with what the first gave.
    let mut again = Open { bytes: first_save.bytes.clone(), doc: o.doc.clone(), index: first_save.index.clone() };
    crate::write_core::move_by(&mut again.doc, 0, shape_id(), 1.0, 0.0);
    let second = real(&again.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("not saved the second time: {e}"));
    let content = part(&second.bytes, "content.xml");
    assert!(
        !content.contains("diacraft:s-Geometry-0") && !content.contains("diacraft:fp-s-Geometry-0") && !content.contains("diacraft:master="),
        "{content}"
    );
    assert!(content.contains("diacraft:fp-text="), "the items that were not stale stay");
    let (_, report) = read_full(&second.bytes);
    assert_eq!(report.warning_counts.get("diacraft_stale"), None);
}

/// Each fingerprint a style carries is charged to the read's items twice: once when the style's attributes are decoded and once when the shape's items are judged. A style
/// of n sections with a fingerprint for each costs at least 2n items more than the same style with none (the sections' own cost is the same in both).
#[test]
fn the_fingerprints_of_a_style_are_charged_to_the_items_of_the_read_when_they_are_decoded_and_when_they_are_judged() {
    let items = |n: usize, with: bool| {
        let mut attrs = String::from("diacraft:version=\"1\" diacraft:id=\"3\"");
        for k in 0..n {
            attrs.push_str(&format!(
                " diacraft:s-Geometry-{k}=\"&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;{k}&quot;/&gt;&lt;/Sheet&gt;\""
            ));
            if with {
                attrs.push_str(&format!(" diacraft:fp-s-Geometry-{k}=\"{:016x}\"", k + 1));
            }
        }
        let style =
            format!("<style:style style:name=\"g\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" {attrs}/></style:style>");
        let shape = "<draw:custom-shape draw:style-name=\"g\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 Z N\"/></draw:custom-shape>";
        read(&Drawing::a4().auto(&style).page(shape).bytes(), &Limits::default()).unwrap().1.work.items
    };
    for n in [200usize, 800] {
        let (with, without) = (items(n, true), items(n, false));
        assert!(
            with >= without + 2 * n as u64,
            "{n} fingerprints cost {} items, fewer than the {} of a decode and a judgement each",
            with - without,
            2 * n
        );
        // Each attribute is also in the style kept whole: another pair and the bytes of its name and value.
        assert!(with <= without + 6 * n as u64, "{n} fingerprints cost {} items, more than six each", with - without);
    }
}

/// By hand, not part of the suite (a measure, not a test: **tests never assert time**): 100,000 shapes whose carried texts are all stale, read beside a yardstick file
/// of the same shapes with no tier 2 on their styles, and the same for one shape that carries 8,192 stale sections. The times and the items are printed.
#[test]
#[ignore = "a timed measure run by hand: cargo test -p diacraft-odg --test it timing_of_a_hundred_thousand_stale_items -- --ignored --nocapture"]
fn timing_of_a_hundred_thousand_stale_items() {
    let n = 100_000usize;
    let stale = stale_texts(n);
    // The yardstick: the same file with the attributes of the extension taken out of every style.
    let yard = {
        let (mut auto, mut shapes) = (String::new(), String::new());
        for k in 0..n {
            auto.push_str(&format!(
                "<style:style style:name=\"g{k}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\"/></style:style>"
            ));
            shapes.push_str(&format!("<draw:rect draw:style-name=\"g{k}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p>native</text:p></draw:rect>"));
        }
        Drawing::a4().auto(&auto).page(&shapes).bytes()
    };
    let time = |bytes: &[u8]| {
        let mut best = std::time::Duration::MAX;
        let mut items = 0;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let (_, report) = read(bytes, &Limits::default()).unwrap();
            best = best.min(start.elapsed());
            items = report.work.items;
        }
        (best, items)
    };
    let (t_stale, i_stale) = time(&stale);
    let (t_yard, i_yard) = time(&yard);
    println!(
        "100,000 stale texts: {} bytes, best of 3 {t_stale:?}, {i_stale} items; yardstick: {} bytes, {t_yard:?}, {i_yard} items",
        stale.len(),
        yard.len()
    );
    // One shape that carries 8,192 sections with stale fingerprints (the style's own cap).
    let sections = {
        let mut attrs = String::from("diacraft:version=\"1\" diacraft:id=\"3\"");
        for k in 0..8_192usize {
            attrs.push_str(&format!(" diacraft:s-Geometry-{k}=\"&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;{k}&quot;/&gt;&lt;/Sheet&gt;\" diacraft:fp-s-Geometry-{k}=\"0123456789abcdef\""));
        }
        let style =
            format!("<style:style style:name=\"g\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" {attrs}/></style:style>");
        let shape = "<draw:custom-shape draw:style-name=\"g\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 Z N\"/></draw:custom-shape>";
        Drawing::a4().auto(&style).page(shape).bytes()
    };
    let (t_sections, i_sections) = time(&sections);
    println!("8,192 stale sections of one shape: {} bytes, {t_sections:?}, {i_sections} items", sections.len());
}

// ------------------------------------------------------------------------------------------------------------------ LibreOffice as a black box

/// By hand, not part of the suite: the files LibreOffice is given, written to `ODG10_LO_DIR/in` (`lo_tier3_write`), and the files it saved, read and counted
/// (`lo_tier3_read`). The edits are made by `lotests.py` of the run folder through UNO, in a profile of its own, and only a missing output is a failure.
fn lo_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ODG10_LO_DIR").expect("set ODG10_LO_DIR to an empty folder"))
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG10_LO_DIR set"]
fn lo_tier3_write() {
    let dir = lo_dir().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("full.odg"), saved()).unwrap();
    std::fs::write(dir.join("fancy.odg"), fancy_end().0).unwrap();
    std::fs::write(dir.join("pin.odg"), saved_pinned()).unwrap();
    // A shape whose pin is at its left edge, at coordinates that LibreOffice's grid of 0.01 mm rounds: the carried pin must still be taken for the markup's.
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let mut shape = rectangle(shape_id(), "rect.1", (2.34567, 3.15823, 4.98765, 2.72413));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)));
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(2.34567 / 2.54).unwrap()).with_unit("IN"));
    }
    push_shape(&mut o.doc, shape);
    std::fs::write(dir.join("pin_awkward.odg"), real(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    // A carried text with a double space, a tab and a line break: what LibreOffice does to white space.
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let mut shape = full_shape();
    shape.text = Some(Arc::new(Text {
        parts: vec![TextPart::Cp(0), TextPart::Chars(Arc::from("He  llo\tx  ")), TextPart::Pp(1), TextPart::Chars(Arc::from("\nWor ld \u{2028}end"))],
        ..Default::default()
    }));
    push_shape(&mut o.doc, shape);
    o.doc.masters.push(Arc::new(master(2)));
    std::fs::write(dir.join("spacey.odg"), real(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    // The review's probes, as the files DiaCraft saves after an edit of a shape of a file: a name given to a plain shape (E), a turned shape whose pin is at its left and
    // whose angle is a formula (D), and a linked shape whose outline DiaCraft edited (B).
    let bytes = Drawing::a4().page(&crate::write_core::custom("s1", "1cm")).bytes();
    let mut o = open_bytes(bytes.clone());
    {
        let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        Arc::make_mut(p.shapes.get_mut(0).unwrap()).name = Some(Arc::from("Display"));
    }
    std::fs::write(dir.join("named.odg"), real(&o.doc, &bytes, &o.index).unwrap().bytes).unwrap();
    let bytes = turned_file();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("t")).map(|s| s.id).unwrap();
    let (cx, cy, _) = pins_of(&bytes);
    let angle = 0.5235987755982988_f64;
    let width = 6.0 / 2.54;
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        sheet.cells.insert(CellName::new("Angle"), Cell::with_formula("30 deg", Value::from_number(angle)).with_unit("DEG"));
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0", Value::from_number(0.0)).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(cx - angle.cos() * width / 2.0).unwrap()).with_unit("IN"));
        sheet.cells.insert(CellName::new("PinY"), Cell::constant(Value::from_number(cy - angle.sin() * width / 2.0).unwrap()).with_unit("IN"));
    }
    std::fs::write(dir.join("turned.odg"), real(&o.doc, &bytes, &o.index).unwrap().bytes).unwrap();
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = shape_id();
    let mut shape = rectangle(id, "rect.1", (2.0, 3.0, 4.0, 2.0));
    shape.master = Some(2);
    push_shape(&mut o.doc, shape);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap().bytes;
    let mut o = open_bytes(first.clone());
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        let geometry = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap());
        let mut rows: Vec<Arc<Row>> = geometry.rows.iter().cloned().collect();
        Arc::make_mut(&mut rows[2]).cells.insert(CellName::new("Y"), Cell::with_formula("Height*0.25", Value::from_number(0.25 * 2.0 / 2.54)));
        geometry.rows.clear();
        for row in rows {
            geometry.rows.push(row);
        }
    }
    std::fs::write(dir.join("outline.odg"), real(&o.doc, &first, &o.index).unwrap().bytes).unwrap();
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG10_LO_DIR set"]
fn lo_tier3_read() {
    let out = lo_dir().join("out");
    let mut names: Vec<_> =
        std::fs::read_dir(&out).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "odg")).collect();
    names.sort();
    for path in names {
        let bytes = std::fs::read(&path).unwrap();
        let (doc, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let page = doc.pages.get(0).unwrap();
        let mut line =
            format!("{}: stale {} ", path.file_name().unwrap().to_string_lossy(), report.warning_counts.get("diacraft_stale").copied().unwrap_or(0));
        for (at, shape) in page.shapes.iter().enumerate() {
            let geometry = shape.sheet.section(&SectionKey::new("Geometry", Some(0)));
            line.push_str(&format!(
                "| shape {at}: id {} name {:?} master {:?} text {:?} markers {} nofill {} comment {} user {} colour {:?} endarrow {:?} pinx_cm {:?} locpinx {:?} width_cm {:?} ",
                shape.id,
                shape.name.as_deref(),
                shape.master,
                chars_of(shape),
                shape.text.as_ref().is_some_and(|t| t.parts.iter().any(|p| matches!(p, TextPart::Cp(_) | TextPart::Pp(_)))),
                geometry.is_some_and(|g| g.cells.contains_key("NoFill")),
                shape.sheet.cell("Comment").is_some(),
                shape.sheet.section(&SectionKey::new("User", None)).is_some_and(|u| u.rows.iter().any(|r| r.name.as_deref() == Some("Key"))),
                colour_of(shape),
                shape.sheet.cell("EndArrow").map(|c| (c.value.as_ref().and_then(Value::number), c.formula.text().map(str::to_owned))),
                shape.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).map(|v| (v * 2.54 * 1000.0).round() / 1000.0),
                shape.sheet.cell("LocPinX").and_then(|c| c.formula.text().map(str::to_owned)),
                shape.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()).map(|v| (v * 2.54 * 1000.0).round() / 1000.0),
            ));
        }
        println!("{line}");
        // What each edit must make of the items (the table of the report): the number of items found stale.
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let want = match stem.as_str() {
            "noop" | "move" | "resize" | "cleared" | "pin_noop" | "pin_move" | "pin_resize" | "pin_awkward_noop" | "spacey_noop" | "fancy_noop" => 0,
            "named_noop" | "turned_noop" | "turned_move" | "outline_noop" => 0,
            "retext" | "rename" | "fill" | "fancy_end_arrow" | "named_rename" | "outline_path" => 1,
            "path" => 2,
            other => panic!("a file this check does not know: {other}"),
        };
        assert_eq!(report.warning_counts.get("diacraft_stale").copied().unwrap_or(0), want, "{stem}");
        // The pin follows the shape a move puts elsewhere: its left edge is where LibreOffice put it (9 cm) and its place in the shape is the carried one.
        if stem == "pin_move" {
            let shape = page.shapes.get(1).unwrap();
            let pin = shape.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap() * 2.54;
            assert!((pin - 9.0).abs() < 0.001, "the pin is at {pin} cm, the left edge of the moved shape");
            assert_eq!(shape.sheet.cell("LocPinX").and_then(|c| c.formula.text()), Some("Width*0"));
        }
        // Probe E on LibreOffice's own output: a name DiaCraft gave a plain shape stands until LibreOffice renames the shape, and then both names are the new one.
        let first = page.shapes.get(0);
        let shown = first.and_then(|s| s.name.as_deref()).map(str::to_owned);
        let own = first.and_then(|s| s.name_u.as_deref()).map(str::to_owned);
        match stem.as_str() {
            "named_noop" => assert_eq!((shown.as_deref(), own.as_deref()), (Some("Display"), Some("s1"))),
            "named_rename" => assert_eq!((shown.as_deref(), own.as_deref()), (Some("Renamed"), Some("Renamed"))),
            // Probe B: the link DiaCraft's outline edit saved is told stale by LibreOffice's path edit and stands otherwise.
            "outline_noop" => assert_eq!(page.shapes.get(1).and_then(|s| s.master), Some(2)),
            "outline_path" => assert_eq!(page.shapes.get(1).and_then(|s| s.master), None),
            // Probe D: the pin of the turned shape follows a move of 5 cm.
            "turned_move" | "turned_noop" => {
                let pin =
                    page.shapes.iter().find(|s| s.name.as_deref() == Some("t")).and_then(|s| s.sheet.cell("PinX")?.value.as_ref()?.number()).unwrap()
                        * 2.54;
                println!("{stem}: the pin is at {pin:.4} cm");
            }
            _ => {}
        }
    }
}
