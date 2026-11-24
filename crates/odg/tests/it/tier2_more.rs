//! What a LibreOffice copy does to tier 2, tier 2 kept through an edit of its shape, a model shaped like a VSDX drawing's saved and read back, and the
//! cost of tier 2 and of the document file (counted, never timed).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Shape, Value};
use diacraft_odg::{Limits, OriginalPackage, SaveNote, WriteOptions, WriteWork, read, write_counted};

use crate::docfile::rebuild;
use crate::drawing::Drawing;
use crate::tier2_write::same_cells;
use crate::write_core::{custom, move_by, open, open_bytes, part, real};
use crate::write_new::{push_shape, rectangle};

fn cell(formula: Option<&str>, value: &str) -> Cell {
    Cell::read(formula, Some(value), None, None, &mut 1_000)
}

/// A saved drawing whose new shape carries tier 2, and the shape's id.
fn saved_with_tier2() -> (Vec<u8>, diacraft_odg::ShapeIndex, u32) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("mine")));
    push_shape(&mut o.doc, shape);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    (saved.bytes, saved.index, id)
}

/// The new shape's element and its style, put again after themselves under another name and style, as LibreOffice's copy and paste does (a new `draw:name`, and the copy's style carries the foreign attribute of the original's).
fn with_a_copy(bytes: &[u8]) -> Vec<u8> {
    let content = part(bytes, "content.xml");
    let style_start = content.find("<style:style style:name=\"gr1\"").expect("the shape's style");
    let style_end = style_start + content[style_start..].find("</style:style>").unwrap() + "</style:style>".len();
    let style = &content[style_start..style_end];
    let copy_style = style.replace("style:name=\"gr1\"", "style:name=\"gr9\"");
    let shape_start = content.find("<draw:custom-shape draw:style-name=\"gr1\"").expect("the shape");
    let shape_end = shape_start + content[shape_start..].find("</draw:custom-shape>").unwrap() + "</draw:custom-shape>".len();
    let shape = &content[shape_start..shape_end];
    let copy_shape = shape.replace("draw:style-name=\"gr1\"", "draw:style-name=\"gr9\"").replace("draw:name=\"R\"", "draw:name=\"R_copy\"");
    let mut out = String::new();
    out.push_str(&content[..style_end]);
    out.push_str(&copy_style);
    out.push_str(&content[style_end..shape_end]);
    out.push_str(&copy_shape);
    out.push_str(&content[shape_end..]);
    rebuild(bytes, |name, b| Some(if name == "content.xml" { out.clone().into_bytes() } else { b.to_vec() }), |m| m.to_owned())
}

#[test]
fn libreoffice_copy_with_duplicate_ids_keeps_first_only() {
    let (bytes, _, id) = saved_with_tier2();
    let copied = with_a_copy(&bytes);
    let o = open_bytes(copied.clone());
    let page = o.doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 3, "the shape, its copy and the file's own");
    let (first, second) = (page.shapes.get(1).unwrap(), page.shapes.get(2).unwrap());
    assert_eq!(first.id, id, "the first of the two keeps its DiaCraft id");
    assert!(first.sheet.cell("Comment").is_some(), "and its tier 2");
    assert_ne!(second.id, id, "the copy takes the next number nothing holds");
    assert!(second.sheet.cell("Comment").is_none(), "and is read from its native markup alone");
    assert_eq!(second.name.as_deref(), Some("R_copy"));
    // The index names the copy, and the high-water mark counts every id read.
    let index_page = o.index.page(0).unwrap();
    assert_eq!(index_page.copies(), [second.id]);
    assert!(index_page.high_water() >= id.max(second.id));
    // An unedited save is the file, and says which shape lost DiaCraft's data.
    let unedited = real(&o.doc, &copied, &o.index).unwrap();
    assert!(!unedited.edited && unedited.bytes == copied);
    let named = |notes: &[SaveNote]| -> Vec<(usize, Vec<u32>, usize)> {
        notes
            .iter()
            .filter_map(|n| if let SaveNote::DuplicateIds { page, shapes, count } = n { Some((*page, shapes.clone(), *count)) } else { None })
            .collect()
    };
    assert_eq!(named(&unedited.notes), [(0, vec![second.id], 1)]);
    assert!(unedited.notes.iter().any(|n| n.to_string().contains(&second.id.to_string())), "the note names the shape");
    // An edit of the first shape: it keeps its tier 2 where it is, the copy's style is left as the file has it, and the note is given again.
    let mut moved = open_bytes(copied.clone());
    move_by(&mut moved.doc, 0, id, 1.0, 0.0);
    let saved = real(&moved.doc, &copied, &moved.index).unwrap();
    assert!(saved.edited);
    assert_eq!(named(&saved.notes), [(0, vec![second.id], 1)]);
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches(&format!("diacraft:id=\"{id}\"")).count(), 2, "both styles keep their attributes, as the file had them: {content}");
    let back = open_bytes(saved.bytes);
    let shapes = &back.doc.pages.get(0).unwrap().shapes;
    assert!(shapes.get(1).unwrap().sheet.cell("Comment").is_some() && shapes.get(2).unwrap().sheet.cell("Comment").is_none());
}

#[test]
fn the_tier_2_of_a_shape_is_kept_when_the_shape_is_moved_restyled_or_deleted_around() {
    let (bytes, _, id) = saved_with_tier2();
    let mut o = open_bytes(bytes.clone());
    move_by(&mut o.doc, 0, id, 1.5, 0.5);
    let saved = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("a move of a shape that carries tier 2 was refused: {e}"));
    assert!(saved.edited);
    let (before, after) = (part(&bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    let attrs = |c: &str| c.split("diacraft:version=").nth(1).map(|s| s.split("/>").next().unwrap_or_default().to_owned());
    assert_eq!(attrs(&before), attrs(&after), "the attributes of the extension are the bytes they were");
    let back = open_bytes(saved.bytes.clone());
    let shape = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert!(shape.sheet.cell("Comment").is_some());
    let was = open_bytes(bytes).doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    let now = shape.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((now - was - 1.5 / 2.54).abs() < 1e-6, "{was} {now}");
    // A shape of the file deleted beside it: the shape keeps its data.
    let (bytes, _, _) = saved_with_tier2();
    let mut o = open_bytes(bytes.clone());
    let first = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes =
        diacraft_doc::ShapeList::from_vec(vec![Arc::clone(o.doc.pages.get(0).unwrap().shapes.get(1).unwrap())]);
    let _ = first;
    let saved = real(&o.doc, &bytes, &o.index).unwrap();
    let back = open_bytes(saved.bytes);
    assert!(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Comment").is_some());
}

/// A shape as a drawing of the other format holds one: a master's instance with its links, shape data and user cells, a formula that names another shape,
/// a non-standard outline with an arc row, a layer, text with runs and a field, and a picture's data.
fn vsdx_shaped(id: u32, other: u32) -> Shape {
    let mut s = rectangle(id, "Process", (2.0, 3.0, 4.0, 2.0));
    s.name = Some(Arc::from("Process"));
    s.name_u = Some(Arc::from("Process.12"));
    s.master = Some(4);
    s.master_shape = Some(9);
    s.text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![
            diacraft_doc::TextPart::Cp(0),
            diacraft_doc::TextPart::Chars(Arc::from("Order ")),
            diacraft_doc::TextPart::Cp(1),
            diacraft_doc::TextPart::Chars(Arc::from("received\n")),
            diacraft_doc::TextPart::Pp(1),
            diacraft_doc::TextPart::Fld { ix: 0, text: Arc::from("12") },
        ],
        ..Default::default()
    }));
    let sheet = Arc::make_mut(&mut s.sheet);
    sheet.cells.insert(CellName::new("LayerMember"), cell(None, "2"));
    sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("a note")));
    sheet.cells.insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text("#A1B2C3")));
    sheet.cells.insert(CellName::new("Rotation"), cell(Some(&format!("Sheet.{other}!Angle")), "0"));
    sheet.cells.insert(CellName::new("LocPinX"), cell(None, "0.5"));
    let mut prop = Section::new("Property", None);
    let mut row = Row { name: Some(Arc::from("Cost")), ..Row::default() };
    row.cells.insert(CellName::new("Label"), Cell::constant(Value::from_text("Cost")));
    row.cells.insert(CellName::new("Value"), cell(None, "12.5"));
    prop.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(prop));
    let mut user = Section::new("User", None);
    let mut row = Row { name: Some(Arc::from("visVersion")), ..Row::default() };
    row.cells.insert(CellName::new("Value"), cell(Some("Width*2"), "6"));
    user.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(user));
    let mut character = Section::new("Character", None);
    let mut row = Row { ix: Some(1), ..Row::default() };
    row.cells.insert(CellName::new("Style"), cell(None, "1"));
    character.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(character));
    // The outline has an arc row, which the reader would give as another row.
    let geometry = sheet.sections.iter_mut().find(|g| g.name.as_str() == "Geometry").unwrap();
    let mut arc = Row { row_type: Some(Arc::from("ArcTo")), ix: Some(9), ..Row::default() };
    arc.cells.insert(CellName::new("X"), cell(Some("Width"), "1"));
    arc.cells.insert(CellName::new("Y"), cell(Some("Height"), "1"));
    arc.cells.insert(CellName::new("A"), cell(None, "0.25"));
    Arc::make_mut(geometry).rows.push(Arc::new(arc));
    s
}

#[test]
fn a_vsdx_shaped_model_saved_as_odg_reads_back_the_same_cells_for_everything_tier_2_carries() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let (a, b) = (o.index.next_id(0).unwrap(), o.index.next_id(0).unwrap() + 1);
    // `a` names `b` in a formula; both are new, so both keep their ids.
    push_shape(&mut o.doc, vsdx_shaped(a, b));
    push_shape(&mut o.doc, rectangle(b, "Target", (10.0, 3.0, 4.0, 2.0)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.notes.iter().any(|n| n.class() == "outline_approximated"), "the arc row is not a path of tier 1: {:?}", saved.notes);
    let back = open_bytes(saved.bytes.clone());
    let shapes = &back.doc.pages.get(0).unwrap().shapes;
    let (ra, rb) = (shapes.get(1).unwrap(), shapes.get(2).unwrap());
    let (ma, mb) = (o.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), o.doc.pages.get(0).unwrap().shapes.get(2).unwrap());
    // The ids that formulas name are kept, whatever the shapes' places in the file.
    assert_eq!((ra.id, rb.id), (a, b));
    assert_eq!(ra.sheet.cell("Rotation").and_then(|c| c.formula.text()), Some(format!("Sheet.{b}!Angle").as_str()));
    same_cells(&ma.sheet, &ra.sheet).expect("the shape that carries tier 2 reads back with its cells");
    assert_eq!((ra.master, ra.master_shape, ra.name_u.as_deref()), (Some(4), Some(9), Some("Process.12")));
    // The parts: the `.odg` data in the text's slot is the reader's, and the model's text has none.
    assert_eq!(ra.text.as_ref().map(|t| &t.parts), ma.text.as_ref().map(|t| &t.parts));
    same_cells(&mb.sheet, &rb.sheet).expect("and the plain one");
    // The target carries an id only because it is named; it carries nothing else.
    let content = part(&saved.bytes, "content.xml");
    let b_style = content.split(&format!("diacraft:id=\"{b}\"")).nth(1).unwrap().split("/>").next().unwrap();
    assert!(!b_style.contains("diacraft:cells") && !b_style.contains("diacraft:s-"), "{b_style}");
    // The file read again and saved with what it was read as is no edit.
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    let _ = read as fn(&[u8], &Limits) -> _;
}

// ------------------------------------------------------------------------------------------------------------------ the cost

/// A drawing with `n` new shapes, each with a user cell and a user section, saved: the work it cost.
fn cost_of_shapes(n: usize) -> WriteWork {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    for k in 0..n {
        let mut shape = rectangle(first + k as u32, &format!("R{k}"), (2.0, 3.0, 4.0, 2.0));
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
        let mut user = Section::new("User", None);
        let mut row = Row { name: Some(Arc::from("k")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), cell(Some("Width*2"), "6"));
        user.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(user));
        push_shape(&mut o.doc, shape);
    }
    let mut work = WriteWork::default();
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{n} shapes: {e}"));
    work
}

#[test]
fn many_new_shapes_that_carry_tier_2_cost_work_in_proportion_to_their_number() {
    let (small, big) = (cost_of_shapes(50), cost_of_shapes(200));
    let fields: [(&str, u64, u64); 7] = [
        ("bytes_copied", small.bytes_copied, big.bytes_copied),
        ("bytes_written", small.bytes_written, big.bytes_written),
        ("attributes_written", small.attributes_written, big.attributes_written),
        ("cells_compared", small.cells_compared, big.cells_compared),
        ("scan_bytes", small.scan_bytes, big.scan_bytes),
        // The read back of the shapes that carry tier 2 finds each in an index of the page, made once (a search of the page for each was a pass over it for each).
        ("order_steps", small.order_steps, big.order_steps),
        ("steps", small.steps_used(), big.steps_used()),
    ];
    for (what, s, b) in fields {
        // Four times the shapes is at most about four times the work, with the constant a file with one shape and its package have.
        assert!(b <= s.saturating_mul(4).saturating_add(4_000), "{what}: {s} for 50 shapes and {b} for 200");
        assert!(s > 0 || what == "steps", "{what} counted nothing");
    }
    assert!(big.bytes_written > small.bytes_written * 2, "what is written grows with the shapes");
}

/// A model with `n` masters of `shapes` shapes each, saved and read back: (the file's size, the items its read took).
fn cost_of_masters(n: usize, shapes: usize) -> (usize, u64) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    for m in 0..n {
        let mut master = diacraft_doc::Master { id: m as u32, name: Some(Arc::from(format!("M{m}"))), ..diacraft_doc::Master::default() };
        let list: Vec<Arc<Shape>> = (0..shapes)
            .map(|k| {
                let mut s = Shape::new(k as u32 + 1);
                Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("Width"), cell(Some("Height*2"), "2"));
                Arc::new(s)
            })
            .collect();
        master.shapes = diacraft_doc::ShapeList::from_vec(list);
        o.doc.masters.push(Arc::new(master));
    }
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{n} masters: {e}"));
    let file = part(&saved.bytes, "DiaCraft/document.xml").len();
    let (doc, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(doc.masters.len(), n);
    (file, report.work.items)
}

#[test]
fn many_masters_and_a_large_document_file_cost_in_proportion_to_what_they_hold() {
    let (small_file, small_items) = cost_of_masters(40, 5);
    let (big_file, big_items) = cost_of_masters(160, 5);
    // Four times the masters: about four times the file and the items its read takes, the page and the package a constant.
    assert!(big_file <= small_file * 4 + 2_000 && big_file >= small_file * 3, "{small_file} {big_file}");
    assert!(big_items <= small_items * 4 + 2_000 && big_items >= small_items * 2, "{small_items} {big_items}");
    // A document with many shapes in each master, as large a file as the others.
    let (deep_file, deep_items) = cost_of_masters(8, 100);
    assert!(deep_file > 10_000 && deep_items > 800, "{deep_file} {deep_items}");
}
