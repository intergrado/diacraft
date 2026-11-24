//! The review: a formula set on a pose cell whose value stays the same is an edit.
//!
//! The frame writes the pose cells' values (a move, a resize, a turn). `shape.pin` sets `LocPinX` to a formula whose value leaves the shape where it was: the frame
//! has nothing to write, and the save was no edit at all (`edited` false, no note), and the formula was gone when the file was read again. What the markup cannot say
//! is the extension's: the formula is carried as tier 2, on the shape's own style, and reads back. The ends of a connector are the glue's (the connections write
//! them): a formula set on one with the glue as it was is refused in words, and one that goes with the glue is the connection's.

use std::sync::Arc;

use diacraft_doc::{Cell, Shape};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{custom, open, open_bytes, part, real, shape_mut};

/// The cell with a formula of its own and the value and the unit `old` has.
fn with_formula(old: &Cell, formula: &str) -> Cell {
    let value = old.value.as_ref().map(|v| v.text().to_owned());
    Cell::read(Some(formula), value.as_deref(), old.unit.as_deref(), None, &mut 1_000)
}

fn formula_of(shape: &Shape, cell: &str) -> Option<String> {
    shape.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned))
}

/// A plain custom shape of a file: a formula set on each of its pose cells, the value the same. The save is an edit, the file's geometry is as it was (the frame has
/// nothing to say), the formula reads back with its value, and a second save is no edit.
#[test]
fn a_formula_set_on_a_pose_cell_with_the_value_the_same_is_carried_and_reads_back() {
    for cell in ["LocPinX", "LocPinY", "PinX", "PinY", "Width", "Height"] {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let old = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell(cell).cloned().unwrap_or_else(|| panic!("no {cell}"));
        let formula = format!("{}+0", old.value.as_ref().map(|v| v.text()).unwrap_or("0"));
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet).cells.insert(diacraft_doc::CellName::new(cell), with_formula(&old, &formula));
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{cell}: not saved: {e}"));
        assert!(saved.edited, "{cell}: a formula set on a pose cell is an edit, and the save said none");
        let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
        let geometry = |c: &str| {
            c.split("<draw:custom-shape")
                .nth(1)
                .and_then(|r| r.split('>').next())
                .map(|t| t.split(" draw:style-name=\"").next().unwrap_or(t).to_owned())
        };
        assert!(after.contains("diacraft:cells="), "{cell}: the formula is an attribute of the extension on the shape's style: {after}");
        let back = open_bytes(saved.bytes.clone());
        let shape = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
        assert_eq!(formula_of(shape, cell).as_deref(), Some(formula.as_str()), "{cell}: the formula reads back");
        let was = old.value.as_ref().and_then(|v| v.number()).unwrap_or(0.0);
        let now = shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
        assert!((was - now).abs() < 1e-9, "{cell}: the value is as it was: {was} {now}");
        assert_eq!(
            geometry(&before).map(|g| g.replace('\n', " ")),
            geometry(&after).map(|g| g.replace('\n', " ")),
            "{cell}: the frame is as the file had it"
        );
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{cell}: a second save is no edit");
    }
}

/// A line is no connector: the formula of its end is carried too.
#[test]
fn a_formula_set_on_the_end_of_a_line_is_carried() {
    let body = r#"<draw:line draw:name="l" svg:x1="1cm" svg:y1="1cm" svg:x2="4cm" svg:y2="2cm"><text:p/></draw:line>"#;
    let mut o = open(&Drawing::a4().page(body));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let old = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("EndX").cloned().expect("a line has an EndX");
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet).cells.insert(diacraft_doc::CellName::new("EndX"), with_formula(&old, "BeginX+3 cm"));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes);
    assert_eq!(formula_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "EndX").as_deref(), Some("BeginX+3 cm"));
}

/// The ends of a connector are the glue's: a formula set on one, with the glue as it was, is refused in words and never dropped. A formula on its other pose cells
/// is carried as any shape's is.
#[test]
fn a_formula_set_on_the_end_of_a_connector_with_its_glue_as_it_was_is_refused_in_words() {
    let body = r#"<draw:connector draw:name="c" svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="1cm" svg:d="M 0 0 L 5000 0" svg:viewBox="0 0 5000 1"><text:p/></draw:connector>"#;
    let mut o = open(&Drawing::a4().page(body));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    assert!(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("ShapeRouteStyle").is_some(), "the model reads a connector");
    let old = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("EndX").cloned().unwrap();
    let mut edited = o.doc.clone();
    Arc::make_mut(&mut shape_mut(&mut edited, 0, id).sheet).cells.insert(diacraft_doc::CellName::new("EndX"), with_formula(&old, "BeginX+5 cm"));
    match real(&edited, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("end of a connector whose glue is as it was"), "{words}"),
        other => panic!("a formula on the end of a connector: {:?}", other.map(|w| (w.edited, w.bytes.len()))),
    }
    // The pin's own cells carry theirs.
    let pin = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").cloned().unwrap();
    let formula = format!("{}+0", pin.value.as_ref().map(|v| v.text()).unwrap_or("0"));
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet).cells.insert(diacraft_doc::CellName::new("PinX"), with_formula(&pin, &formula));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
}

/// The ends of a connector are the glue's: when the glue of an end goes (the shape it was glued to is deleted, and the engine leaves the end where it was, with no
/// formula) the end's formulas change with it and the connection says so, so **nothing is carried for them** (the extension would repeat what the markup says), and the
/// save is not refused.
#[test]
fn the_end_formulas_that_go_with_a_glue_are_the_connections_and_carry_nothing() {
    let body = concat!(
        r#"<draw:custom-shape xml:id="a" draw:name="A" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
        r#"<draw:custom-shape xml:id="b" draw:name="B" svg:x="10cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
        r#"<draw:connector draw:name="c" draw:start-shape="a" draw:start-glue-point="1" draw:end-shape="b" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="10cm" svg:y2="4cm" svg:d="M 6000 4000 h 4000" svg:viewBox="0 0 4001 1"/>"#
    );
    let mut o = open(&Drawing::a4().page(body));
    assert!(
        o.doc.pages.get(0).unwrap().shapes.get(2).unwrap().sheet.cell("EndX").is_some_and(|c| c.formula.text().is_some()),
        "a glued end has a formula"
    );
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        page.shapes.remove(1);
        let kept: Vec<_> = page.connects.iter().filter(|c| c.to_sheet != 2).cloned().collect();
        page.connects = diacraft_doc::Connects::from_vec(kept);
        let sheet = Arc::make_mut(&mut Arc::make_mut(page.shapes.get_mut(1).unwrap()).sheet);
        for name in ["EndX", "EndY"] {
            sheet.cells.get_mut(name).unwrap().formula = diacraft_doc::FormulaSlot::Absent;
        }
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("an unglued end is saved: {e}"));
    assert!(saved.edited);
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("diacraft:version"), "the formulas that went with the glue are carried by no attribute of the extension: {after}");
    assert!(!after.contains("draw:end-shape"), "the end is unglued: {after}");
}

/// Item 7: the pin's place edited as a constant, with the pin compensating and the frame on the page as it was. The frame has nothing to say
/// (the shape stands where it stood), and `LocPinX` is a pose cell whose value the frame writes: the edit was no edit at all, and the place was the centre again
/// when the file was read. What the markup cannot say is the extension's: the place (no longer `Width*0.5`) is carried, the pin the markup's edge and it give is
/// the model's, and a second save is no edit. The first edit takes the place from a formula to a constant; the second edits the constant, which is the case the
/// frame's comparison passed by (the formula was the same: none).
#[test]
fn a_constant_pin_place_with_the_pin_compensating_and_the_frame_as_it_was_is_carried_and_reads_back() {
    let num = |s: &Shape, name: &str| s.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    let first = |doc: &diacraft_doc::Document| doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    // Sets the place and the pin to `place` and `pin` as constants, in inches.
    let set = |doc: &mut diacraft_doc::Document, id: u32, place: (&str, f64), pin: (&str, f64)| {
        let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
        for (name, v) in [place, pin] {
            sheet.cells.insert(diacraft_doc::CellName::new(name), Cell::read(None, Some(&v.to_string()), Some("IN"), None, &mut 1_000));
        }
    };
    let frame = |c: &str| {
        c.split("<draw:custom-shape")
            .nth(1)
            .and_then(|r| r.split('>').next())
            .map(|t| t.split(" draw:style-name=\"").next().unwrap_or(t).replace('\n', " "))
    };
    for (place, pin) in [("LocPinX", "PinX"), ("LocPinY", "PinY")] {
        let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = first(&o.doc).id;
        let (old_place, old_pin) = (num(&first(&o.doc), place), num(&first(&o.doc), pin));
        // The first edit: a place that is a constant, the pin compensating.
        let mut edited = o.doc.clone();
        set(&mut edited, id, (place, old_place - 0.25), (pin, old_pin - 0.25));
        let saved = real(&edited, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{place}: not saved: {e}"));
        assert!(saved.edited, "{place}: the place of the pin is an edit");
        let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
        assert!(after.contains("diacraft:cells="), "{place}: the place is an attribute of the extension on the shape's style: {after}");
        assert_eq!(frame(&before), frame(&after), "{place}: the frame is as the file had it");
        let back = open_bytes(saved.bytes.clone());
        let read = first(&back.doc);
        assert!((num(&read, place) - (old_place - 0.25)).abs() < 1e-9 && (num(&read, pin) - (old_pin - 0.25)).abs() < 1e-9, "{place}: {read:?}");
        assert_eq!(formula_of(&read, place), None, "{place}: a constant");
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{place}: a second save is no edit");
        let same = real(&edited, &saved.bytes, &saved.index).unwrap();
        assert!(!same.edited && same.bytes == saved.bytes, "{place}: the model saved again over what was written is no edit");
        // The second edit: the constant changes, the pin compensating, over the file the first one made.
        let mut again_edited = back.doc.clone();
        set(&mut again_edited, id, (place, old_place - 0.5), (pin, old_pin - 0.5));
        let second = real(&again_edited, &saved.bytes, &back.index).unwrap_or_else(|e| panic!("{place}: not saved: {e}"));
        assert!(second.edited, "{place}: the edit of a constant place with the pin compensating was no edit at all");
        assert_eq!(frame(&part(&second.bytes, "content.xml")), frame(&before), "{place}: the frame is as the file had it");
        let back = open_bytes(second.bytes.clone());
        let read = first(&back.doc);
        assert!((num(&read, place) - (old_place - 0.5)).abs() < 1e-9 && (num(&read, pin) - (old_pin - 0.5)).abs() < 1e-9, "{place}: {read:?}");
        let third = real(&back.doc, &second.bytes, &back.index).unwrap();
        assert!(!third.edited && third.bytes == second.bytes, "{place}: a second save is no edit");
    }
}

/// The place of the pin edited alone (the pin as it was): the frame moves, which the frame writes, and the place is carried: the pin the markup's edge and the place
/// give is the model's, whichever the constant is edited from, and a second save is no edit.
#[test]
fn a_constant_pin_place_edited_alone_moves_the_frame_and_is_carried() {
    let num = |s: &Shape, name: &str| s.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    let first = |doc: &diacraft_doc::Document| doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    let constant = |v: f64| Cell::read(None, Some(&v.to_string()), Some("IN"), None, &mut 1_000);
    for (place, pin) in [("LocPinX", "PinX"), ("LocPinY", "PinY")] {
        let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = first(&o.doc).id;
        let (old_place, old_pin) = (num(&first(&o.doc), place), num(&first(&o.doc), pin));
        let mut edited = o.doc.clone();
        Arc::make_mut(&mut shape_mut(&mut edited, 0, id).sheet).cells.insert(diacraft_doc::CellName::new(place), constant(old_place - 0.25));
        let saved = real(&edited, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{place}: not saved: {e}"));
        assert!(saved.edited, "{place}");
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains("diacraft:cells="), "{place}: the place is carried: {after}");
        let back = open_bytes(saved.bytes.clone());
        let read = first(&back.doc);
        assert!((num(&read, place) - (old_place - 0.25)).abs() < 1e-9, "{place}: {}", num(&read, place));
        assert!((num(&read, pin) - old_pin).abs() < 1e-9, "{place}: the pin is as the model had it: {} {old_pin}", num(&read, pin));
        // The edge moved with the place: the shape is a quarter of an inch from where it was along the axis.
        let was = first(&o.doc);
        assert_ne!(part(&o.bytes, "content.xml"), after);
        let moved = if place == "LocPinX" { num(&read, "PinX") - num(&read, "LocPinX") } else { num(&read, "PinY") - num(&read, "LocPinY") };
        let edge = if place == "LocPinX" { num(&was, "PinX") - num(&was, "LocPinX") } else { num(&was, "PinY") - num(&was, "LocPinY") };
        assert!((moved - edge - 0.25).abs() < 1e-9, "{place}: {moved} {edge}");
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{place}: a second save is no edit");
        let same = real(&edited, &saved.bytes, &saved.index).unwrap();
        assert!(!same.edited && same.bytes == saved.bytes, "{place}: the model saved again over what was written is no edit");
    }
}
