//! The splice writer keeps and drops what says. Each test names the R8 row it follows, and runs
//! the real writer; the echo control at the end fails every case.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape, Value};

use crate::drawing::Drawing;
use crate::drawing::cm;
use crate::write_core::{Saver, echo, echo_says_edited, move_by, num_of, open, open_bytes, part, real, set_num, shape_mut};
use crate::write_more::{CONNECTOR_BODY, set_route_end};

/// A rectangle named `name` on the automatic style `style`.
fn rect(name: &str, style: &str, x: &str) -> String {
    format!(
        "\n    <draw:rect draw:name=\"{name}\" draw:style-name=\"{style}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p/></draw:rect>"
    )
}

pub(crate) fn set_text(shape: &mut Shape, cell: &str, text: &str) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

pub(crate) fn id_of(doc: &Document, name: &str) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).id
}

fn cell_text(doc: &Document, name: &str, cell: &str) -> Option<String> {
    let shape = doc.pages.get(0)?.shapes.iter().find(|s| s.name.as_deref() == Some(name))?;
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// One automatic style element of a part, by name.
pub(crate) fn style_of<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let start = content.find(&format!("<style:style style:name=\"{name}\""))?;
    let end = content[start..].find("</style:style>")? + "</style:style>".len();
    Some(&content[start..start + end])
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Gap 1: the `draw:fill` row removes `draw:fill-image-*` with the picture.

/// The attributes of a bitmap fill that the `draw:fill` row takes out with it.
const BITMAP: [&str; 7] = [
    "draw:fill-image-name",
    "draw:fill-image-width",
    "draw:fill-image-height",
    "draw:fill-image-ref-point",
    "draw:fill-image-ref-point-x",
    "draw:fill-image-ref-point-y",
    "style:repeat",
];

const BITMAP_STYLE: &str = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="img1" draw:fill-image-width="2cm" draw:fill-image-height="1cm" draw:fill-image-ref-point="center" draw:fill-image-ref-point-x="10%" draw:fill-image-ref-point-y="20%" style:repeat="repeat" svg:stroke-width="0.1cm"/></style:style>"#;

fn bitmap_drawing() -> Drawing {
    Drawing::a4().auto(BITMAP_STYLE).page(&rect("a", "gr1", "1cm"))
}

/// R8, `draw:fill`: "remove ... `draw:fill-image-*` when the new kind no longer uses them": a fill picture dropped (the pattern set to solid) takes every
/// attribute of the picture with it, and the second save of the same model is no edit.
fn a_dropped_fill_picture_takes_all_its_attributes_with_it(save: &Saver) {
    let mut o = open(&bitmap_drawing());
    let a = id_of(&o.doc, "a");
    // A bitmap reads as the stand-in pattern 0, so the edit is the pattern 1.
    assert_eq!(cell_text(&o.doc, "a", "FillPattern").as_deref(), Some("0"));
    set_text(shape_mut(&mut o.doc, 0, a), "FillPattern", "1");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let s = style_of(&after, "gr1").unwrap_or_else(|| panic!("no gr1: {after}"));
    assert!(s.contains("draw:fill=\"solid\""), "{s}");
    for attr in BITMAP {
        assert!(!s.contains(attr), "{attr} is still there: {s}");
    }
    // What is not the picture's stays: the line.
    assert!(s.contains("svg:stroke-width=\"0.1cm\""), "{s}");
    // The reader reads back what was written, and saving the same model again is no edit.
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_text(&back.doc, "a", "FillPattern").as_deref(), Some("1"));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The `draw:fill` row, the other half: the picture's attributes are kept while `FillPattern` and `FillGradientEnabled` are unchanged (the form): an edit of
/// the line's weight leaves all seven byte for byte.
fn an_edit_that_leaves_the_fill_keeps_the_picture_attributes(save: &Saver) {
    let mut o = open(&bitmap_drawing());
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "LineWeight", "0.05");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
    let after = part(&saved.bytes, "content.xml");
    let s = style_of(&after, "gr1").unwrap_or_else(|| panic!("no gr1: {after}"));
    for attr in BITMAP {
        assert!(s.contains(attr), "{attr} went with a line weight: {s}");
    }
    assert!(s.contains("draw:fill=\"bitmap\""), "{s}");
    // The edit itself was made: the line is 0.05 in wide.
    assert!(s.contains("svg:stroke-width=\"0.127cm\""), "{s}");
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Gap 3: the Connectors table keeps `draw:line-skew` until DiaCraft routes the connector anew.

/// A shape and a curved connector glued to its right side middle, with a skew and its path.
pub(crate) fn skewed_drawing() -> Drawing {
    Drawing::a4().page(&CONNECTOR_BODY.replace(r#"draw:name="c" "#, r#"draw:name="c" draw:type="curve" draw:line-skew="1cm" "#))
}

pub(crate) fn tag_of<'a>(content: &'a str, name: &str) -> &'a str {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no {name}: {content}"));
    let start = content[..at].rfind('<').unwrap();
    let end = at + content[at..].find('>').unwrap();
    &content[start..=end]
}

/// R8, `draw:line-skew`: "remove when DiaCraft routes the connector anew; kept until then". A move of the glued shape that has the engine route the connector
/// again (its `svg:d` is written) takes the skew with the route; a save that writes no route (the line weight edited, or the other shape moved) keeps it.
fn the_skew_goes_with_a_route_written_anew_and_stays_otherwise(save: &Saver) {
    // Kept: the connector's line weight is edited and no route is written.
    let mut o = open(&skewed_drawing());
    set_text(shape_mut(&mut o.doc, 0, 2), "LineWeight", "0.05");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
    let after = part(&saved.bytes, "content.xml");
    assert!(tag_of(&after, "c").contains("draw:line-skew=\"1cm\""), "{}", tag_of(&after, "c"));
    // Kept: the shape moves and the connector is not routed again (its ends follow by their formulas, and its `svg:d` is the file's).
    let mut o = open(&skewed_drawing());
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(tag_of(&after, "c").contains("draw:line-skew=\"1cm\"") && tag_of(&after, "c").contains("svg:d=\"M 6000 4000 h 4000\""), "{after}");
    // Removed: the shape moves 1 cm right and the engine routes the connector again.
    let mut o = open(&skewed_drawing());
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    {
        let c = shape_mut(&mut o.doc, 0, 2);
        let x = num_of(c, "BeginX");
        set_num(c, "BeginX", x + cm(1.0));
        set_route_end(c, cm(3.0));
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let tag = tag_of(&after, "c");
    assert!(tag.contains(r#"svg:d="M 7000 4000 h 3000" svg:viewBox="0 0 3001 1""#), "the route is written anew: {tag}");
    assert!(!tag.contains("draw:line-skew"), "the skew goes with the route: {tag}");
    assert!(tag.contains("draw:type=\"curve\""), "the type stays: {tag}");
    // The reader reads back what was written, and the same model saved again is no edit.
    let back = open_bytes(saved.bytes.clone());
    assert!(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cell("BeginX").is_some());
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Item 4: the `svg:d`, `svg:viewBox` row of a `draw:path`: regenerate on an edit of the path, keep while it is not edited.

pub(crate) const FRAME: &str = r#"svg:x="1cm" svg:y="2cm" svg:width="4cm" svg:height="4cm""#;

/// A square path whose `svg:viewBox` is spelled with commas, which the reader reads as the box `0 0 1000 1000`.
pub(crate) fn path_drawing() -> Drawing {
    Drawing::a4().page(&format!(
        r#"<draw:path draw:name="p" {FRAME} svg:viewBox="0,0,1000,1000" svg:d="M 0 0 L 1000 0 L 1000 1000 L 0 1000 Z"><text:p/></draw:path>"#
    ))
}

/// A fifth point put into the path's one section, after the third, half way across and below the shape's bottom.
pub(crate) fn with_a_fifth_point(doc: &mut Document, id: u32) {
    let shape = shape_mut(doc, 0, id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap();
    let section = Arc::make_mut(section);
    let mut new = section.rows[3].clone();
    for (cell, v) in [("X", 0.5), ("Y", -0.25)] {
        Arc::make_mut(&mut new).cells.insert(CellName::new(cell), Cell::constant(Value::from_number(v).unwrap()));
    }
    section.rows.insert(3, new);
    for (i, r) in section.rows.iter_mut().enumerate() {
        Arc::make_mut(r).ix = Some(i as u32 + 1);
    }
}

fn a_path_edit_regenerates_its_view_box_and_an_unedited_path_keeps_it(save: &Saver) {
    // Edited: `svg:d` and `svg:viewBox` are both written.
    let mut o = open(&path_drawing());
    let p = id_of(&o.doc, "p");
    with_a_fifth_point(&mut o.doc, p);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"svg:viewBox="0 0 1000 1000""#), "the view box is regenerated: {after}");
    assert!(after.contains(r#"svg:d="M 0 0 L 1000 0 L 1000 1000 L 500 1250 L 0 1000 Z""#), "{after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count(), 1);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
    // Not edited: a restyle of the path leaves the attribute as the file spells it.
    let mut o = open(&path_drawing());
    let p = id_of(&o.doc, "p");
    set_text(shape_mut(&mut o.doc, 0, p), "LineWeight", "0.05");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"svg:viewBox="0,0,1000,1000""#), "the view box is kept while the path is not edited: {after}");
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Item 6: R9 "add a basic shape": `draw:layer` is the layer the shape is on, `layout` only when it is on none (`draw:layer`).

pub(crate) fn layered_drawing() -> Drawing {
    let mut d = Drawing::a4().page(&crate::write_core::custom("s1", "1cm"));
    d.masters = d.masters.replace(
        r#"<draw:layer draw:name="measurelines"/>"#,
        r#"<draw:layer draw:name="measurelines"/><draw:layer draw:name="Notes"/><draw:layer draw:name="Other" draw:display="printer"/>"#,
    );
    d
}

pub(crate) fn layer_attr(content: &str, name: &str) -> Option<String> {
    let tag = tag_of(content, name);
    let at = tag.find("draw:layer=\"")? + "draw:layer=\"".len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

fn a_new_shape_is_written_on_the_layer_it_is_on(save: &Saver) {
    use crate::write_new::{connector, push_shape, rectangle};
    let o = open(&layered_drawing());
    assert_eq!(o.doc.pages.get(0).unwrap().shapes.len(), 1);
    let layers = |member: Option<&str>| -> (String, Document) {
        let mut doc = o.doc.clone();
        let mut shape = rectangle(o.index.next_id(0).unwrap(), "R", (2.0, 3.0, 4.0, 2.0));
        if let Some(text) = member {
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(text)));
        }
        push_shape(&mut doc, shape);
        let saved = save(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{member:?}: {e}"));
        (part(&saved.bytes, "content.xml"), doc)
    };
    // On the first layer of the page's rows: `Notes`, which the reader reads back as row 0, so that nothing is carried as tier 2.
    let (after, doc) = layers(Some("0"));
    assert_eq!(layer_attr(&after, "R").as_deref(), Some("Notes"), "{after}");
    assert!(!after.contains("diacraft:version"), "the layer reads back as the model has it: {after}");
    let saved = save(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    assert_eq!(read.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).as_deref(), Some("0"));
    let again = save(&doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
    // On the second row, and on two rows (the first is the shape's layer; the other membership is carried).
    let (after, _) = layers(Some("1"));
    assert_eq!(layer_attr(&after, "R").as_deref(), Some("Other"), "{after}");
    assert!(!after.contains("diacraft:version"), "{after}");
    let (after, _) = layers(Some("1;0"));
    assert_eq!(layer_attr(&after, "R").as_deref(), Some("Other"), "{after}");
    assert!(after.contains("diacraft:version"), "the second membership is tier 2: {after}");
    // On no layer, and on a row the page does not have: `layout`.
    for member in [None, Some("7"), Some("x"), Some("")] {
        let (after, _) = layers(member);
        assert_eq!(layer_attr(&after, "R").as_deref(), Some("layout"), "{member:?}: {after}");
    }
    // A new connector is written the same way.
    let mut doc = o.doc.clone();
    let mut c = connector(o.index.next_id(0).unwrap(), (2.0, 10.0, 8.0, 10.0));
    Arc::make_mut(&mut c.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text("1")));
    c.name = Some(Arc::from("K"));
    c.name_u = Some(Arc::from("K"));
    push_shape(&mut doc, c);
    let saved = save(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(layer_attr(&part(&saved.bytes, "content.xml"), "K").as_deref(), Some("Other"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Gap 2 and item 5: the text rows. A paragraph's and a run's style keep each value "while its cell is unchanged": an edit of one cell writes that
// cell's attributes into a copy of the style the file has, and every other attribute stays as the file has it.

use diacraft_doc::{Row, TextPart};

use crate::write_runs::{chars, id_of as shape_id, set_parts, the_shape};

/// The row `ix` of the section `section` of the shape, edited: the cell `cell` set to the number `v` (put in when the row has none).
pub(crate) fn edit_cell(o: &mut crate::write_core::Open, section: &str, ix: u32, cell: &str, v: f64) {
    let id = shape_id(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let sec = sheet.sections.iter_mut().find(|s| s.name.as_str() == section).unwrap_or_else(|| panic!("no section {section}"));
    let sec = Arc::make_mut(sec);
    let row = sec.rows.iter_mut().find(|r| r.ix == Some(ix)).unwrap_or_else(|| panic!("no row {ix} in {section}"));
    Arc::make_mut(row).cells.insert(CellName::new(cell), Cell::constant(Value::from_number(v).unwrap()));
}

/// Sets a cell of a row to some text.
pub(crate) fn edit_text(o: &mut crate::write_core::Open, section: &str, ix: u32, cell: &str, text: &str) {
    let id = shape_id(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let sec = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == section).unwrap());
    let row = Arc::make_mut(sec.rows.iter_mut().find(|r| r.ix == Some(ix)).unwrap());
    row.cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

/// Takes the cell out of the row.
pub(crate) fn drop_cell(o: &mut crate::write_core::Open, section: &str, ix: u32, cell: &str) {
    let id = shape_id(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let sec = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == section).unwrap());
    let row = Arc::make_mut(sec.rows.iter_mut().find(|r| r.ix == Some(ix)).unwrap());
    assert!(row.cells.remove(cell).is_some(), "{section} {ix} had no {cell}");
}

/// The number a cell of a row of the shape holds.
pub(crate) fn cell_number(doc: &Document, section: &str, ix: u32, cell: &str) -> Option<f64> {
    crate::write_runs::row_cell(the_shape(doc), section, ix, cell)
}

/// A row of a section by `IX`, for a failure message.
pub(crate) fn row_of(doc: &Document, section: &str, ix: u32) -> String {
    let shape = the_shape(doc);
    let row: Option<&Arc<Row>> =
        shape.sheet.sections.iter().find(|s| s.name.as_str() == section).and_then(|s| s.rows.iter().find(|r| r.ix == Some(ix)));
    let cells = row.map(|r| r.cells.iter().map(|(n, c)| format!("{}={}", n.as_str(), c.value.as_ref().map_or("", |v| v.text()))).collect::<Vec<_>>());
    format!("{cells:?}")
}

pub(crate) fn shape_in(inner: &str) -> String {
    crate::write_runs::shape("x", "1cm", inner)
}

/// A run whose style holds values no cell maps and values whose cells are kept: a wave underline with its colour and width, `lowercase`, a percentage position,
/// and a language.
pub(crate) const T1: &str = r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="12pt" fo:font-weight="bold" style:text-underline-style="wave" style:text-underline-type="single" style:text-underline-color="#ff0000" style:text-underline-width="bold" fo:text-transform="lowercase" style:text-position="33% 58%" fo:language="de" fo:country="DE"/></style:style>"##;

pub(crate) fn run_drawing() -> Drawing {
    Drawing::a4().auto(T1).page(&shape_in(r#"<text:p>ab<text:span text:style-name="T1">cd</text:span></text:p>"#))
}

/// The name and the style of the first span of the saved content.
pub(crate) fn span_style(content: &str) -> (&str, &str) {
    let key = "<text:span text:style-name=\"";
    let at = content.find(key).expect("a span") + key.len();
    let name = &content[at..at + content[at..].find('"').unwrap()];
    (name, style_of(content, name).unwrap_or_else(|| panic!("no style {name}: {content}")))
}

/// R8, "Styles" (the text row): "regenerate the attribute of the edited cell" and keep "`lowercase`, percentage positions": an edit of a run's `Size` writes the size
/// into a copy of the run's style, which keeps the underline's style, colour and width, the lowercase, the position and the language exactly. Read back, the cells are
/// the model's, and the same model saved again is no edit.
fn a_size_edit_keeps_every_other_value_of_the_runs_style(save: &Saver) {
    let mut o = open(&run_drawing());
    assert_eq!(cell_number(&o.doc, "Character", 1, "Style"), Some(5.0), "bold and underlined: {}", row_of(&o.doc, "Character", 1));
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (name, style) = span_style(&after);
    // Nothing else names the style: it is edited where it stands (cloned only when another paragraph or run names it, and no style is renamed), the file's
    // style in its attributes' places with the size written: nothing else.
    assert_eq!(name, "T1", "{after}");
    assert_eq!(style, style_of(&part(&o.bytes, "content.xml"), "T1").unwrap().replace("12pt", "14pt"), "{after}");
    assert!(style_of(&after, "T2").is_none(), "no copy is made: {after}");
    // Read back the cells are the model's, and the saved model is no edit.
    let back = open_bytes(saved.bytes.clone());
    for cell in ["Style", "Case", "Pos", "Size"] {
        let (read, made) = (cell_number(&back.doc, "Character", 1, cell), cell_number(&o.doc, "Character", 1, cell));
        assert_eq!(read, made, "{cell}: {}", row_of(&back.doc, "Character", 1));
    }
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The underline row: the underline's style, colour and width are kept "while the cell is unchanged", and the cell is `Style` bit 4: a bold edit keeps a
/// held wave underline; the underline taken off (bit 4 cleared) writes `none` and takes the colour and width, which then say nothing, with it.
fn a_bold_edit_keeps_a_held_underline_and_the_underline_taken_off_drops_it(save: &Saver) {
    let mut o = open(&run_drawing());
    edit_cell(&mut o, "Character", 1, "Style", 4.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = span_style(&after);
    assert!(style.contains("fo:font-weight=\"normal\""), "the bold is written: {style}");
    for kept in [
        r#"style:text-underline-style="wave""#,
        r##"style:text-underline-color="#ff0000""##,
        r#"style:text-underline-width="bold""#,
        r#"fo:text-transform="lowercase""#,
        r#"style:text-position="33% 58%""#,
        r#"fo:language="de""#,
    ] {
        assert!(style.contains(kept), "{kept} went with a bold edit: {style}");
    }
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_number(&back.doc, "Character", 1, "Style"), Some(4.0));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
    // The underline off: bit 4 is cleared, and the bold stays.
    let mut o = open(&run_drawing());
    edit_cell(&mut o, "Character", 1, "Style", 1.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = span_style(&after);
    assert!(style.contains(r#"style:text-underline-style="none""#), "{style}");
    for gone in ["style:text-underline-type", "style:text-underline-color", "style:text-underline-width", "wave"] {
        assert!(!style.contains(gone), "{gone} stays with the underline taken off: {style}");
    }
    for kept in [r#"fo:text-transform="lowercase""#, r#"fo:language="de""#, r#"fo:font-size="12pt""#] {
        assert!(style.contains(kept), "{kept}: {style}");
    }
}

/// A paragraph style with the spelling `start`, a line spacing beside the line height, and a tab stop with a dotted leader.
pub(crate) const P1: &str = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="start" fo:line-height="150%" style:line-spacing="0.1cm"><style:tab-stops><style:tab-stop style:position="2cm" style:leader-style="dotted" style:leader-text="."/></style:tab-stops></style:paragraph-properties></style:style>"##;

pub(crate) fn paragraph_drawing() -> Drawing {
    Drawing::a4().auto(P1).page(&shape_in(r#"<text:p text:style-name="P1">abcd</text:p>"#))
}

/// The name and the style of the first paragraph of the saved content that names one.
pub(crate) fn paragraph_style(content: &str) -> (&str, &str) {
    let key = "<text:p text:style-name=\"";
    let at = content.find(key).expect("a paragraph that names a style") + key.len();
    let name = &content[at..at + content[at..].find('"').unwrap()];
    (name, style_of(content, name).unwrap_or_else(|| panic!("no style {name}: {content}")))
}

/// R8, "Styles" (the paragraph rows): the spelling `start` is kept while `HorzAlign` is unchanged, `style:line-spacing` while `SpLine` is, and a tab stop's leader
/// while the row is. An edit of the indent writes `fo:margin-left` into a copy of the paragraph's style and nothing else; an edit of the line height takes the
/// line spacing out with it; one of the alignment replaces the spelling.
fn a_paragraph_edit_keeps_the_values_of_the_cells_it_does_not_touch(save: &Saver) {
    let before = part(&open(&paragraph_drawing()).bytes, "content.xml");
    let p1 = style_of(&before, "P1").unwrap();
    let mut o = open(&paragraph_drawing());
    assert_eq!(cell_number(&o.doc, "Paragraph", 0, "HorzAlign"), Some(0.0), "{}", row_of(&o.doc, "Paragraph", 0));
    edit_cell(&mut o, "Paragraph", 0, "IndLeft", 0.5);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (name, style) = paragraph_style(&after);
    // The paragraph alone names the style: it is edited where it stands.
    assert_eq!(name, "P1", "{after}");
    let expected = p1.replace(r#"style:line-spacing="0.1cm">"#, r#"style:line-spacing="0.1cm" fo:margin-left="1.27cm">"#);
    assert_eq!(style, expected, "the style as the file has it, with the one attribute added: {after}");
    assert!(style_of(&after, "P2").is_none(), "no copy is made: {after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_number(&back.doc, "Paragraph", 0, "IndLeft"), Some(0.5));
    assert_eq!(cell_number(&back.doc, "Paragraph", 0, "HorzAlign"), Some(0.0));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
    // The line height: its two companions go with it, and the tab stop stays.
    let mut o = open(&paragraph_drawing());
    edit_cell(&mut o, "Paragraph", 0, "SpLine", -2.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = paragraph_style(&after);
    assert!(style.contains(r#"fo:line-height="200%""#) && !style.contains("style:line-spacing"), "{style}");
    assert!(style.contains(r#"style:leader-style="dotted""#) && style.contains(r#"fo:text-align="start""#), "{style}");
    // The alignment: the new word replaces `start`, and the rest stays.
    let mut o = open(&paragraph_drawing());
    edit_cell(&mut o, "Paragraph", 0, "HorzAlign", 2.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = paragraph_style(&after);
    assert!(style.contains(r#"fo:text-align="right""#) && !style.contains("start"), "{style}");
    assert!(style.contains(r#"style:line-spacing="0.1cm""#) && style.contains(r#"style:leader-text=".""#), "{style}");
    let back = open_bytes(saved.bytes);
    assert_eq!(cell_number(&back.doc, "Paragraph", 0, "HorzAlign"), Some(2.0));
}

/// A run whose characters and row change together takes its style from the run it stands in the place of: the head and the tail the two texts share say which.
fn a_run_edited_in_its_text_and_its_row_is_made_from_the_run_it_stands_in_the_place_of(save: &Saver) {
    let mut o = open(&run_drawing());
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    set_parts(&mut o, vec![TextPart::Pp(0), chars("abX"), TextPart::Cp(1), chars("cYd")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = span_style(&after);
    assert!(style.contains("fo:font-size=\"14pt\"") && style.contains("wave") && style.contains(r#"fo:language="de""#), "{style}");
    assert!(after.contains(">cYd</text:span>") && after.contains("abX"), "{after}");
}

/// A cell that is gone from the row takes its attributes out; a face is written as the part's declaration and takes `fo:font-family` with it.
fn a_cell_gone_and_a_face_changed_are_written_into_the_copy(save: &Saver) {
    let style = r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:color="#00ff00" fo:font-family="Arial" fo:hyphenate="true"/></style:style>"##;
    // The family as the model has it: bare, or in the quotes a font-family has in CSS (the declaration the part holds is found by the family without them).
    for family in ["DejaVu Sans Mono", "'DejaVu Sans Mono'", "\"DejaVu Sans Mono\"", " 'DejaVu Sans Mono' "] {
        let mut d = Drawing::a4().auto(style).page(&shape_in(r#"<text:p>ab<text:span text:style-name="T1">cd</text:span></text:p>"#));
        d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="DejaVu Sans Mono"/>"#.to_owned();
        let mut o = open(&d);
        drop_cell(&mut o, "Character", 1, "Color");
        edit_text(&mut o, "Character", 1, "Font", family);
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{family}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        let (_, style) = span_style(&after);
        assert!(!style.contains("fo:color") && !style.contains("fo:font-family"), "{family}: {style}");
        assert!(style.contains(r#"style:font-name="Mono""#) && style.contains(r#"fo:hyphenate="true""#), "{family}: {style}");
        assert!(
            !part(&saved.bytes, "content.xml").contains("style:font-face style:name=\"DejaVu"),
            "{family}: no face is declared for the quotes: {after}"
        );
    }
}

/// Two paragraphs that share a row and a style are both written with the one copy made for it; an edit the style already says makes no copy; a style with a
/// parent keeps it, and a style with no properties of the kind (an empty element) is given them.
fn a_row_shared_by_two_paragraphs_is_one_copy_and_an_edit_the_style_says_already_is_none(save: &Saver) {
    let two = r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#;
    let mut o = open(&Drawing::a4().auto(T1).page(&shape_in(two)));
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("text:style-name=\"T2\"").count(), 2, "both spans name the one copy: {after}");
    assert_eq!(after.matches("style:name=\"T2\"").count(), 1, "{after}");
    assert!(style_of(&after, "T1").is_none() && style_of(&after, "T3").is_none(), "{after}");
    // A change of a cell that changes no attribute: a double underline has `DblUnderline`, and the underline bit of `Style` set beside it says the same. The group
    // is written again, the style already says it, and no copy is made (and no name is used for one).
    let double = r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="bold" style:text-underline-style="solid" style:text-underline-type="double"/></style:style>"##;
    let mut o = open(&Drawing::a4().auto(double).page(&shape_in(r#"<text:p>ab<text:span text:style-name="T3">cd</text:span></text:p>"#)));
    assert_eq!(cell_number(&o.doc, "Character", 1, "Style"), Some(1.0), "{}", row_of(&o.doc, "Character", 1));
    assert_eq!(cell_number(&o.doc, "Character", 1, "DblUnderline"), Some(1.0), "{}", row_of(&o.doc, "Character", 1));
    edit_cell(&mut o, "Character", 1, "Style", 5.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(saved.edited, "the cell is another cell");
    assert!(after.contains("<text:span text:style-name=\"T3\">cd</text:span>") && style_of(&after, "T4").is_none(), "{after}");
    // The style the span still names is still there: it is named once more by the paragraph written again, so that nothing takes it out as an orphan.
    assert!(style_of(&after, "T3").is_some(), "{after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_number(&back.doc, "Character", 1, "DblUnderline"), Some(1.0), "{}", row_of(&back.doc, "Character", 1));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
    // A parent stays, and an empty style gets the properties.
    let styles = r##"<style:style style:name="T1" style:family="text" style:parent-style-name="Strong"/>"##;
    let d = Drawing::a4()
        .common(r#"<style:style style:name="Strong" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .auto(styles)
        .page(&shape_in(r#"<text:p>ab<text:span text:style-name="T1">cd</text:span></text:p>"#));
    let mut o = open(&d);
    assert_eq!(cell_number(&o.doc, "Character", 1, "Style"), Some(1.0), "bold through the parent: {}", row_of(&o.doc, "Character", 1));
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (name, style) = span_style(&after);
    assert_eq!(name, "T1", "{after}");
    assert_eq!(
        style,
        r#"<style:style style:name="T1" style:family="text" style:parent-style-name="Strong"><style:text-properties fo:font-size="14pt"/></style:style>"#
    );
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_number(&back.doc, "Character", 1, "Style"), Some(1.0));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The alignment row is fed by `HorzAlign` and by `Flags` (`start` and `end` swap with the writing mode): the writing mode edited with the alignment as it was
/// writes the alignment again, in words that do not swap (`left`), so that it reads back as the model has it, and the second save is no edit.
fn a_writing_mode_edit_writes_the_alignment_again_in_words_that_do_not_swap(save: &Saver) {
    let mut o = open(&paragraph_drawing());
    edit_cell(&mut o, "Paragraph", 0, "Flags", 1.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (_, style) = paragraph_style(&after);
    assert!(style.contains(r#"style:writing-mode="rl-tb""#) && style.contains(r#"fo:text-align="left""#) && !style.contains("start"), "{style}");
    // What is no cell's stays: the line spacing and the tab stop.
    assert!(style.contains(r#"style:line-spacing="0.1cm""#) && style.contains(r#"style:leader-style="dotted""#), "{style}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(cell_number(&back.doc, "Paragraph", 0, "HorzAlign"), Some(0.0), "{}", row_of(&back.doc, "Paragraph", 0));
    assert_eq!(cell_number(&back.doc, "Paragraph", 0, "Flags"), Some(1.0));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes);
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 12] = [
    ("a_dropped_fill_picture_takes_all_its_attributes_with_it", a_dropped_fill_picture_takes_all_its_attributes_with_it),
    ("an_edit_that_leaves_the_fill_keeps_the_picture_attributes", an_edit_that_leaves_the_fill_keeps_the_picture_attributes),
    ("the_skew_goes_with_a_route_written_anew_and_stays_otherwise", the_skew_goes_with_a_route_written_anew_and_stays_otherwise),
    ("a_path_edit_regenerates_its_view_box_and_an_unedited_path_keeps_it", a_path_edit_regenerates_its_view_box_and_an_unedited_path_keeps_it),
    ("a_new_shape_is_written_on_the_layer_it_is_on", a_new_shape_is_written_on_the_layer_it_is_on),
    ("a_size_edit_keeps_every_other_value_of_the_runs_style", a_size_edit_keeps_every_other_value_of_the_runs_style),
    (
        "a_bold_edit_keeps_a_held_underline_and_the_underline_taken_off_drops_it",
        a_bold_edit_keeps_a_held_underline_and_the_underline_taken_off_drops_it,
    ),
    ("a_paragraph_edit_keeps_the_values_of_the_cells_it_does_not_touch", a_paragraph_edit_keeps_the_values_of_the_cells_it_does_not_touch),
    (
        "a_run_edited_in_its_text_and_its_row_is_made_from_the_run_it_stands_in_the_place_of",
        a_run_edited_in_its_text_and_its_row_is_made_from_the_run_it_stands_in_the_place_of,
    ),
    ("a_cell_gone_and_a_face_changed_are_written_into_the_copy", a_cell_gone_and_a_face_changed_are_written_into_the_copy),
    (
        "a_row_shared_by_two_paragraphs_is_one_copy_and_an_edit_the_style_says_already_is_none",
        a_row_shared_by_two_paragraphs_is_one_copy_and_an_edit_the_style_says_already_is_none,
    ),
    (
        "a_writing_mode_edit_writes_the_alignment_again_in_words_that_do_not_swap",
        a_writing_mode_edit_writes_the_alignment_again_in_words_that_do_not_swap,
    ),
];

#[test]
fn a_dropped_fill_picture_takes_all_its_attributes_with_it_test() {
    a_dropped_fill_picture_takes_all_its_attributes_with_it(&real);
}

#[test]
fn an_edit_that_leaves_the_fill_keeps_the_picture_attributes_test() {
    an_edit_that_leaves_the_fill_keeps_the_picture_attributes(&real);
}

#[test]
fn the_skew_goes_with_a_route_written_anew_and_stays_otherwise_test() {
    the_skew_goes_with_a_route_written_anew_and_stays_otherwise(&real);
}

#[test]
fn a_path_edit_regenerates_its_view_box_and_an_unedited_path_keeps_it_test() {
    a_path_edit_regenerates_its_view_box_and_an_unedited_path_keeps_it(&real);
}

#[test]
fn a_new_shape_is_written_on_the_layer_it_is_on_test() {
    a_new_shape_is_written_on_the_layer_it_is_on(&real);
}

#[test]
fn a_size_edit_keeps_every_other_value_of_the_runs_style_test() {
    a_size_edit_keeps_every_other_value_of_the_runs_style(&real);
}

#[test]
fn a_bold_edit_keeps_a_held_underline_and_the_underline_taken_off_drops_it_test() {
    a_bold_edit_keeps_a_held_underline_and_the_underline_taken_off_drops_it(&real);
}

#[test]
fn a_paragraph_edit_keeps_the_values_of_the_cells_it_does_not_touch_test() {
    a_paragraph_edit_keeps_the_values_of_the_cells_it_does_not_touch(&real);
}

#[test]
fn a_run_edited_in_its_text_and_its_row_is_made_from_the_run_it_stands_in_the_place_of_test() {
    a_run_edited_in_its_text_and_its_row_is_made_from_the_run_it_stands_in_the_place_of(&real);
}

#[test]
fn a_cell_gone_and_a_face_changed_are_written_into_the_copy_test() {
    a_cell_gone_and_a_face_changed_are_written_into_the_copy(&real);
}

#[test]
fn a_row_shared_by_two_paragraphs_is_one_copy_and_an_edit_the_style_says_already_is_none_test() {
    a_row_shared_by_two_paragraphs_is_one_copy_and_an_edit_the_style_says_already_is_none(&real);
}

#[test]
fn a_writing_mode_edit_writes_the_alignment_again_in_words_that_do_not_swap_test() {
    a_writing_mode_edit_writes_the_alignment_again_in_words_that_do_not_swap(&real);
}

/// The echo control: a writer that gives the original back fails every case, and the real writer passes each.
#[test]
fn echo_control_fails_every_case() {
    for (name, case) in CASES {
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo))).is_err(), "the echo writer passed the case {name}: the case checks nothing");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&real))).is_ok(), "the real writer failed the case {name}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A black-box check against LibreOffice (not part of the suite: both tests are ignored, and run by hand with `ODG_R8FIX_DIR` set to an empty folder).
//
// 1. `lo_kept_dropped_write` writes `in/*.odg`: a run and a paragraph edited per cell, a connector routed anew with its skew taken out, a path edited, new shapes on the
//    page's layers, a bitmap fill dropped and a pin place carried.
// 2. LibreOffice converts them as a black box: `soffice -env:UserInstallation=file://<folder>/profile --headless --convert-to odg --outdir out in/*.odg`, and
//    `--convert-to pdf`; only a missing output file is a failure.
// 3. `lo_kept_dropped_read` reads `out/*.odg` with the reader and says what came back: the layers, the underline, the case, the position, the size.

fn lo_folder() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ODG_R8FIX_DIR").expect("set ODG_R8FIX_DIR to an empty folder"))
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_R8FIX_DIR set"]
fn lo_kept_dropped_write() {
    let dir = lo_folder().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    // A run: the size edited, the bold edited.
    let mut o = open(&run_drawing());
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    put("run_size.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let mut o = open(&run_drawing());
    edit_cell(&mut o, "Character", 1, "Style", 4.0);
    put("run_bold.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // A paragraph: the indent and the writing mode edited.
    let mut o = open(&paragraph_drawing());
    edit_cell(&mut o, "Paragraph", 0, "IndLeft", 0.5);
    put("paragraph_indent.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let mut o = open(&paragraph_drawing());
    edit_cell(&mut o, "Paragraph", 0, "Flags", 1.0);
    put("paragraph_mode.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // A connector routed anew: the skew goes with the old route.
    let mut o = open(&skewed_drawing());
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    {
        let c = shape_mut(&mut o.doc, 0, 2);
        let x = num_of(c, "BeginX");
        set_num(c, "BeginX", x + cm(1.0));
        set_route_end(c, cm(3.0));
    }
    put("connector_rerouted.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // A path edited.
    let mut o = open(&path_drawing());
    let p = id_of(&o.doc, "p");
    with_a_fifth_point(&mut o.doc, p);
    put("path_edited.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // New shapes on the layers.
    {
        use crate::write_new::{connector, push_shape, rectangle};
        let o = open(&layered_drawing());
        let mut doc = o.doc.clone();
        for (k, member) in ["0", "1"].into_iter().enumerate() {
            let mut shape = rectangle(o.index.next_id(0).unwrap() + k as u32, &format!("R{k}"), (2.0 + 5.0 * k as f64, 8.0, 4.0, 2.0));
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(member)));
            push_shape(&mut doc, shape);
        }
        let mut c = connector(o.index.next_id(0).unwrap() + 2, (2.0, 14.0, 8.0, 14.0));
        Arc::make_mut(&mut c.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text("1")));
        c.name = Some(Arc::from("K"));
        c.name_u = Some(Arc::from("K"));
        push_shape(&mut doc, c);
        put("layers.odg", &real(&doc, &o.bytes, &o.index).unwrap().bytes);
    }
    // A bitmap fill dropped.
    let mut o = open(&bitmap_drawing());
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "FillPattern", "1");
    put("fill_dropped.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // A constant pin place with the pin compensating.
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let number = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap();
        let (loc, pin) = (number("LocPinX"), number("PinX"));
        for (name, v) in [("LocPinX", loc - 0.25), ("PinX", pin - 0.25)] {
            sheet.cells.insert(CellName::new(name), Cell::read(None, Some(&v.to_string()), Some("IN"), None, &mut 1_000));
        }
    }
    put("pin_place.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    println!("wrote 9 files in {}", dir.display());
}

#[test]
#[ignore = "reads the files LibreOffice converted, run by hand with ODG_R8FIX_DIR set"]
fn lo_kept_dropped_read() {
    let out = lo_folder().join("out");
    let read_one = |name: &str| {
        let bytes = std::fs::read(out.join(name)).unwrap_or_else(|e| panic!("out/{name}: run LibreOffice first ({e})"));
        diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("out/{name} did not read: {e}")).0
    };
    let cell = |doc: &Document, section: &str, ix: u32, name: &str| -> Option<f64> {
        let shape = doc.pages.get(0)?.shapes.iter().find(|s| s.sheet.sections.iter().any(|x| x.name.as_str() == section))?;
        let sec = shape.sheet.sections.iter().find(|x| x.name.as_str() == section)?;
        sec.rows.iter().find(|r| r.ix == Some(ix))?.cells.get(name)?.value.as_ref()?.number()
    };
    for name in ["run_size.odg", "run_bold.odg"] {
        let doc = read_one(name);
        // The run is the second row the reader makes (row 0 holds what the first characters' paragraph says).
        let rows: Vec<String> = (0..4)
            .map(|ix| {
                format!(
                    "{ix}: size {:?} style {:?} case {:?} pos {:?}",
                    cell(&doc, "Character", ix, "Size"),
                    cell(&doc, "Character", ix, "Style"),
                    cell(&doc, "Character", ix, "Case"),
                    cell(&doc, "Character", ix, "Pos")
                )
            })
            .collect();
        println!("{name}: {rows:?}");
    }
    for name in ["paragraph_indent.odg", "paragraph_mode.odg"] {
        let doc = read_one(name);
        println!(
            "{name}: align {:?} indent {:?} flags {:?} line {:?}",
            cell(&doc, "Paragraph", 0, "HorzAlign"),
            cell(&doc, "Paragraph", 0, "IndLeft"),
            cell(&doc, "Paragraph", 0, "Flags"),
            cell(&doc, "Paragraph", 0, "SpLine")
        );
    }
    let doc = read_one("layers.odg");
    let layer_of = |name: &str| {
        doc.pages
            .get(0)
            .unwrap()
            .shapes
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
            .map(|s| s.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
    };
    println!("layers: R0 {:?} R1 {:?} K {:?}", layer_of("R0"), layer_of("R1"), layer_of("K"));
    assert_eq!(
        layer_of("R0"),
        Some(Some("0".to_owned())),
        "a shape written on the first of the page's layers is on it when LibreOffice has saved it"
    );
    assert_eq!(layer_of("R1"), Some(Some("1".to_owned())));
    for name in ["connector_rerouted.odg", "path_edited.odg", "fill_dropped.odg", "pin_place.odg"] {
        let doc = read_one(name);
        println!("{name}: {} shapes", doc.pages.get(0).unwrap().shapes.len());
    }
}
