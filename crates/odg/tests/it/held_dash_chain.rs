//! What the reader holds of a line's stroke and its dash, and what the model writer writes of them.
//!
//! A style that says `draw:stroke="dash"` and names no `draw:stroke-dash` takes its dash from the parent that names one, or has none (a program of another make draws its own for it). The cells
//! say it as a `LinePattern` that is a stand-in (2, or the table's row for lengths another make wrote), and the stroke is held beside it. The writer writes the stroke as it was read and **names
//! no dash of DiaCraft's own beside it** (the owner's ruling "Keep the file's form"), until the pattern is changed. A style whose dash is DiaCraft's own and which the cells say again exactly
//! (the pattern its name says, and the cap form its name says) holds none of it, and the cells write it as they do for a new shape.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, HeldGroup, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const LINE: &str = r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#;

/// Another make's dash whose marks and gaps are none of the table's for a 0.05 cm line (2 and 1.6 weights).
const PEBBLE: &str = r#"<draw:stroke-dash draw:name="Pebble" draw:style="rect" draw:dots1="1" draw:dots1-length="0.10cm" draw:distance="0.08cm"/>"#;

/// One of DiaCraft's own: the pattern 4 as a round cap draws it, and as a flat cap does.
const OWN_4: &str = r#"<draw:stroke-dash draw:name="DiaCraft_20_dash_20_4" draw:display-name="DiaCraft dash 4" draw:style="rect" draw:dots1="1" draw:dots1-length="700%" draw:dots2="1" draw:dots2-length="0%" draw:distance="500%"/>"#;
const OWN_4_FLAT: &str = r#"<draw:stroke-dash draw:name="DiaCraft_20_dash_20_4_20_flat" draw:display-name="DiaCraft dash 4 flat" draw:style="rect" draw:dots1="1" draw:dots1-length="800%" draw:dots2="1" draw:dots2-length="100%" draw:distance="400%"/>"#;

fn style(name: &str, parent: Option<&str>, props: &str) -> String {
    let parent = parent.map(|p| format!(r#" style:parent-style-name="{p}""#)).unwrap_or_default();
    format!(r#"<style:style style:name="{name}" style:family="graphic"{parent}><style:graphic-properties{props}/></style:style>"#)
}

/// The weight and the colour of a 0.05 cm black line.
const LOOK: &str = r##" svg:stroke-width="0.05cm" svg:stroke-color="#000000""##;

/// A drawing with the common style `Kerb` (`kerb` properties) and the dashes `defs`, and a line on the automatic style `gr1` (`own` properties) whose parent is `Kerb`.
fn drawing(defs: &str, kerb: &str, own: &str) -> Drawing {
    Drawing::a4().common(defs).common(&style("Kerb", None, kerb)).auto(&style("gr1", Some("Kerb"), own)).page(LINE)
}

fn model_of(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap().0
}

/// The text of the parts `content.xml` and `styles.xml` the model writer gives.
fn written(doc: &Document) -> (String, String) {
    let bytes = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}")).bytes;
    let package = diacraft_odg::package::Package::read(&bytes, &Limits::default()).unwrap();
    let part = |name: &str| String::from_utf8(package.part(name).unwrap().to_vec()).unwrap();
    (part("content.xml"), part("styles.xml"))
}

/// The start tag of the first style named `name` in `part`, with its properties element.
fn style_text<'a>(part: &'a str, name: &str) -> &'a str {
    let at = part.find(&format!(r#"style:name="{name}""#)).unwrap_or_else(|| panic!("no style {name} in {part}"));
    let from = part[..at].rfind("<style:style ").unwrap();
    &part[from..at + part[at..].find("</style:style>").unwrap()]
}

/// The held values of the group `Line` of the first shape, as `(draw:stroke, draw:stroke-dash)` of each.
fn line_held(doc: &Document) -> Vec<(Option<String>, Option<String>)> {
    let shape = doc.pages.iter().flat_map(|p| p.shapes.iter()).next().expect("a shape");
    let slot = shape.ext.get().expect("the shape's odf data");
    let get = |h: &diacraft_doc::Held, name: &str| h.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_owned());
    slot.odf.held.iter().filter(|h| h.group == HeldGroup::Line).map(|h| (get(h, "draw:stroke"), get(h, "draw:stroke-dash"))).collect()
}

/// The `LinePattern` cell of the first shape, as the reader made it.
fn pattern_cell(doc: &Document) -> Cell {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("LinePattern").cloned().expect("a line pattern")
}

fn set_pattern(doc: &mut Document, cell: Cell) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LinePattern"), cell);
}

#[test]
fn a_stroke_that_names_no_dash_is_held_beside_a_stand_in_and_written_with_no_dash_of_ours() {
    // IS: the line says dash and names none, its parent names Pebble and says solid.
    let d = drawing(PEBBLE, &format!(r#" draw:stroke="solid" draw:stroke-dash="Pebble"{LOOK}"#), r#" draw:stroke="dash""#);
    let doc = model_of(&d);
    assert_eq!(line_held(&doc), [(Some("dash".to_owned()), None)], "the stroke is held, and no dash");
    let (content, styles) = written(&doc);
    let own = style_text(&content, "gr1");
    assert!(own.contains(r#"draw:stroke="dash""#) && !own.contains("stroke-dash"), "the line says dash and names none: {own}");
    assert!(style_text(&styles, "Kerb").contains(r#"draw:stroke-dash="Pebble""#), "its parent still names Pebble: {styles}");
    assert!(
        !content.contains("DiaCraft_20_dash_20_2") && !styles.contains("DiaCraft_20_dash_20_2"),
        "DiaCraft's own dash 2 is nowhere: {content}\n{styles}"
    );
}

#[test]
fn a_chain_that_names_no_dash_names_none_when_written() {
    // U: the line and its parent say dash, and nothing names a dash.
    let d = drawing("", &format!(r#" draw:stroke="dash"{LOOK}"#), r#" draw:stroke="dash""#);
    let doc = model_of(&d);
    let (content, styles) = written(&doc);
    assert!(!content.contains("stroke-dash") && !styles.contains("stroke-dash"), "no style names a dash: {content}\n{styles}");
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke="dash""#) && style_text(&styles, "Kerb").contains(r#"draw:stroke="dash""#));
    // And what was written reads and writes the same again.
    let again = written(&model_of_bytes(&write_with(&doc, None, &WriteOptions::default()).unwrap().bytes));
    assert_eq!(again.0.matches("stroke-dash").count() + again.1.matches("stroke-dash").count(), 0, "{again:?}");
}

fn model_of_bytes(bytes: &[u8]) -> Document {
    read(bytes, &Limits::default()).unwrap().0
}

#[test]
fn a_style_that_names_its_own_dash_beside_the_stroke_still_has_the_dash_written_as_held() {
    // O: the line's own style names Pebble: the dash is held, with the stroke.
    let d = drawing(PEBBLE, LOOK, r#" draw:stroke="dash" draw:stroke-dash="Pebble""#);
    let doc = model_of(&d);
    let held = line_held(&doc);
    assert!(held.contains(&(None, Some("Pebble".to_owned()))) && held.contains(&(Some("dash".to_owned()), None)), "{held:?}");
    let (content, _) = written(&doc);
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke-dash="Pebble""#), "{content}");
}

#[test]
fn a_style_whose_dash_of_diacraft_the_cells_say_again_holds_neither_the_dash_nor_the_stroke() {
    // The line's own style names DiaCraft's dash 4 (a round cap): the cells say it (`LinePattern` 4 and no flat cap), so nothing is held and the cells write it.
    let d = drawing(OWN_4, LOOK, r#" draw:stroke="dash" draw:stroke-dash="DiaCraft_20_dash_20_4""#);
    let doc = model_of(&d);
    assert_eq!(line_held(&doc), [], "nothing of the line is held");
    let (content, _) = written(&doc);
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke-dash="DiaCraft_20_dash_20_4""#), "{content}");
    // The same, named for a flat cap that the style states itself.
    let d = drawing(OWN_4_FLAT, LOOK, r#" draw:stroke="dash" draw:stroke-dash="DiaCraft_20_dash_20_4_20_flat" svg:stroke-linecap="butt""#);
    let doc = model_of(&d);
    assert_eq!(line_held(&doc), [], "nothing of the line is held under a flat cap either");
    let (content, _) = written(&doc);
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke-dash="DiaCraft_20_dash_20_4_20_flat""#), "{content}");
}

#[test]
fn a_dash_of_diacraft_the_cells_cannot_say_again_is_held_as_the_file_wrote_it() {
    // The form its name says is the flat one, and the style states no cap of its own (the cap is its parent's): the cells would write the round form, so the name is held.
    let d = drawing(
        &format!("{OWN_4}{OWN_4_FLAT}"),
        &format!(r#" draw:stroke="dash" svg:stroke-linecap="butt"{LOOK}"#),
        r#" draw:stroke="dash" draw:stroke-dash="DiaCraft_20_dash_20_4_20_flat""#,
    );
    let doc = model_of(&d);
    assert!(line_held(&doc).contains(&(None, Some("DiaCraft_20_dash_20_4_20_flat".to_owned()))), "{:?}", line_held(&doc));
    let (content, _) = written(&doc);
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke-dash="DiaCraft_20_dash_20_4_20_flat""#), "{content}");
    // A style that names a dash of DiaCraft's and says no stroke has no pattern of its own: its name is held, and written.
    let d = drawing(OWN_4, "", r#" draw:stroke-dash="DiaCraft_20_dash_20_4""#);
    let (content, _) = written(&model_of(&d));
    assert!(style_text(&content, "gr1").contains(r#"draw:stroke-dash="DiaCraft_20_dash_20_4""#), "{content}");
}

#[test]
fn a_pattern_set_after_the_read_is_written_as_a_dash_of_ours_over_the_dash_the_style_named() {
    // The stroke's stamp is stale once `LinePattern` is the person's: the dash the style named beside it goes, and the dash of the pattern is written.
    for own in [r#" draw:stroke="dash""#, r#" draw:stroke="dash" draw:stroke-dash="Pebble""#] {
        let mut doc = model_of(&drawing(PEBBLE, &format!(r#" draw:stroke="solid" draw:stroke-dash="Pebble"{LOOK}"#), own));
        let read_as = pattern_cell(&doc);
        set_pattern(&mut doc, Cell::constant(Value::from_number(9.0).unwrap()));
        let (content, styles) = written(&doc);
        let text = style_text(&content, "gr1");
        assert!(text.contains(r#"draw:stroke-dash="DiaCraft_20_dash_20_9""#) && !text.contains("Pebble"), "{own}: {text}");
        assert!(styles.contains(r#"draw:name="DiaCraft_20_dash_20_9""#), "its definition is written: {styles}");
        // The pattern put back to what it was read as (an undo) holds the stroke again, and the dash named is written as it was.
        set_pattern(&mut doc, read_as);
        let (content, _) = written(&doc);
        let text = style_text(&content, "gr1");
        assert!(!text.contains("DiaCraft_20_dash"), "{own}: {text}");
        assert_eq!(text.contains("Pebble"), own.contains("Pebble"), "{own}: {text}");
    }
}
