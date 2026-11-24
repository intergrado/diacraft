//! The writer beyond the core cases (and its vectors):
//! what a move, a resize, a turn and a delete do to each shape form, a connector, a group, a page that carries `draw:z-index`, the package's
//! own bookkeeping, a file that binds `svg` to the W3C namespace, and what the writer refuses.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connects, Value};
use diacraft_odg::{Limits, OriginalPackage, SaveNote, WriteError, WriteOptions, read, write_with};

use crate::build::{DRAWING, write};
use crate::drawing::{Drawing, NS, close, cm};
use crate::write_core::{
    PNG, compare, content_changes, custom, drawing_of, move_by, num_of, open, open_bytes, part, part_bytes, real, set_num, shape_mut,
};

/// A rectangle at the page's (`x`, `y`), `w` by `h` (centimetres), named `name`.
fn rect(name: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    format!("<draw:rect draw:name=\"{name}\" svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"{w}cm\" svg:height=\"{h}cm\"/>")
}

fn saved_content(o: &crate::write_core::Open) -> String {
    part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml")
}

#[test]
fn a_move_keeps_a_shear_and_a_turn_and_a_resize_drops_them_with_a_note() {
    // The shear and the turn stay as written; only the translation moves.
    let body = r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="skewX (0.2) rotate (0.3) translate (5cm 4cm)"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"draw:transform="skewX (0.2) rotate (0.3) translate (6cm 4cm)""#), "{after}");
    assert!(!after.contains("svg:x="), "no attribute was added: {after}");
    // A resize writes the transform whole (no shear), says so once, and the shape reads back with its new size.
    let mut o = open(&Drawing::a4().page(body));
    {
        let s = shape_mut(&mut o.doc, 0, 1);
        set_num(s, "Width", cm(6.0));
        set_num(s, "LocPinX", cm(3.0));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.notes, [SaveNote::ShearDropped { page: 0, shape: 1 }]);
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("skewX"), "{after}");
    let back = open_bytes(saved.bytes);
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "Width"), cm(6.0));
}

#[test]
fn a_turned_shape_moved_keeps_its_turn_and_moves_its_translation() {
    let body = r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (5cm 10cm)"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, -2.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"draw:transform="rotate (0.5235987755982988) translate (6cm 12cm)""#), "{after}");
    // A shape turned back to 0: the transform goes and its place is `svg:x` and `svg:y`, added.
    let mut o = open(&Drawing::a4().page(body));
    shape_mut(&mut o.doc, 0, 1).sheet = Arc::new({
        let mut sheet = (*shape_mut(&mut o.doc, 0, 1).sheet).clone();
        sheet.cells.remove("Angle");
        sheet
    });
    let after = saved_content(&o);
    assert!(!after.contains("draw:transform") && after.contains("svg:x=\"") && after.contains("svg:y=\""), "{after}");
    let back = open_bytes(real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let (s, was) = (back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), o.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
    close(num_of(s, "PinX"), num_of(was, "PinX"));
    close(num_of(s, "PinY"), num_of(was, "PinY"));
    assert!(s.sheet.cell("Angle").is_none());
}

#[test]
fn a_circle_given_by_its_centre_moves_by_its_centre() {
    // `svg:cx` becomes 6 cm after a move of 1 cm right, and `svg:x` is not written.
    let body = r#"<draw:circle draw:name="c" svg:cx="5cm" svg:cy="4cm" svg:r="1cm"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:cx="6cm" svg:cy="4cm" svg:r="1cm""#), "{after}");
    assert!(!after.contains("svg:x="));
    // Resized to a radius of 1.5 cm about its centre, it is written in the same form.
    let mut o = open(&Drawing::a4().page(body));
    {
        let s = shape_mut(&mut o.doc, 0, 1);
        for (cell, v) in [("Width", cm(3.0)), ("Height", cm(3.0)), ("LocPinX", cm(1.5)), ("LocPinY", cm(1.5))] {
            set_num(s, cell, v);
        }
    }
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:cx="5cm" svg:cy="4cm" svg:r="1.5cm""#), "{after}");
    // Turned, it has no such form: refused, with nothing written.
    let mut o = open(&Drawing::a4().page(body));
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, 1).sheet).cells.insert(CellName::new("Angle"), Cell::constant(Value::from_number(0.5).unwrap()));
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))));
}

#[test]
fn a_line_is_written_by_its_ends_and_loses_its_transform() {
    // A line `(0, 0)` to `(4 cm, 0)` under `translate (2cm 3cm)`, moved 1 cm right: `svg:x1` 3 cm, `svg:x2` 7 cm, both y 3 cm, no transform.
    let body = r#"<draw:line draw:name="l" svg:x1="0cm" svg:y1="0cm" svg:x2="4cm" svg:y2="0cm" draw:transform="translate (2cm 3cm)"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    {
        let s = shape_mut(&mut o.doc, 0, 1);
        for cell in ["BeginX", "EndX"] {
            let v = num_of(s, cell);
            set_num(s, cell, v + cm(1.0));
        }
    }
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:x1="3cm" svg:y1="3cm" svg:x2="7cm" svg:y2="3cm""#), "{after}");
    assert!(!after.contains("draw:transform"), "{after}");
    let back = open_bytes(real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let s = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    close(num_of(s, "BeginX"), cm(3.0));
    close(num_of(s, "EndX"), cm(7.0));
    close(num_of(s, "BeginY"), cm(29.7 - 3.0));
}

#[test]
fn a_group_moved_moves_its_members_and_a_member_moved_moves_itself() {
    let body = format!("<draw:g>{}{}</draw:g>", rect("r1", 2.0, 3.0, 4.0, 2.0), rect("r2", 7.0, 6.0, 3.0, 3.0));
    // The group has the box of its members: moved by 1 cm right and 2 cm up the page, each member goes with it.
    let mut o = open(&Drawing::a4().page(&body));
    move_by(&mut o.doc, 0, 1, 1.0, 2.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:x="3cm" svg:y="1cm""#) && after.contains(r#"svg:x="8cm" svg:y="4cm""#), "{after}");
    assert!(after.contains("<draw:g>") && !after.contains("<draw:g svg"), "the group's own element is as it was");
    let diff = compare(&o.bytes, &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (2, 0, 0), "{changed:?}");
    // One member moved inside its group: only it changes.
    let mut o = open(&Drawing::a4().page(&body));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let member = Arc::make_mut(group.children.get_mut(1).unwrap());
    let x = num_of(member, "PinX");
    set_num(member, "PinX", x + cm(1.0));
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:x="2cm" svg:y="3cm""#) && after.contains(r#"svg:x="8cm" svg:y="6cm""#), "{after}");
    // A member deleted from its group: its element goes and the group keeps the other.
    let mut o = open(&Drawing::a4().page(&body));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    group.children.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("draw:name=\"r1\"") && after.contains("draw:name=\"r2\""), "{after}");
    assert_eq!((saved.index.ordinal(0, 1), saved.index.ordinal(0, 2), saved.index.ordinal(0, 3)), (Some(1), None, Some(2)));
}

#[test]
fn a_page_that_carries_z_index_is_reordered_by_the_attribute_and_no_element_moves() {
    // `a`, `b`, `c` in this document order with `draw:z-index` 2, 0, 1 draw as `b`, `c`, `a`. `b` brought to the front gets the largest value.
    let z = |name: &str, z: i32, x: f64| {
        format!("<draw:rect draw:name=\"{name}\" draw:z-index=\"{z}\" svg:x=\"{x}cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>")
    };
    let body = format!("{}{}{}", z("a", 2, 1.0), z("b", 0, 3.0), z("c", 1, 5.0));
    let mut o = open(&Drawing::a4().page(&body));
    let names = |o: &crate::write_core::Open| -> Vec<String> {
        o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap().to_owned()).collect()
    };
    assert_eq!(names(&o), ["b", "c", "a"]);
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let b = page.shapes.remove(0).unwrap();
    page.shapes.push(b);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    // Each shape has the value of the place it has now: c 0, a 1, b 2. The elements stay where they were.
    assert!(
        after.contains(r#"draw:name="a" draw:z-index="1""#)
            && after.contains(r#"draw:name="b" draw:z-index="2""#)
            && after.contains(r#"draw:name="c" draw:z-index="0""#),
        "{after}"
    );
    assert!(after.find("draw:name=\"a\"") < after.find("draw:name=\"b\"") && after.find("draw:name=\"b\"") < after.find("draw:name=\"c\""));
    let back = open_bytes(saved.bytes);
    assert_eq!(names(&back), ["c", "a", "b"]);
    // A delete on such a page keeps the order of the others as the reader will read it.
    let mut o = open(&Drawing::a4().page(&body));
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let back = open_bytes(real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    assert_eq!(names(&back), ["c", "a"]);
}

#[test]
fn a_page_with_shapes_that_lack_z_index_keeps_its_order_after_a_delete_and_a_reorder() {
    // `a` and `c` carry z-index, `b` has none and takes its place from where it stands (index 1): after `a` is deleted it stands at 0.
    let body = concat!(
        r#"<draw:rect draw:name="a" draw:z-index="0" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:rect draw:name="b" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:rect draw:name="c" draw:z-index="5" svg:width="1cm" svg:height="1cm"/>"#
    );
    let o = open(&Drawing::a4().page(body));
    let names = |d: &diacraft_doc::Document| -> Vec<String> {
        d.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap().to_owned()).collect()
    };
    assert_eq!(names(&o.doc), ["a", "b", "c"]);
    let mut doc = o.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    assert_eq!(names(&open_bytes(saved.bytes).doc), ["b", "c"]);
}

/// A shape and a connector glued to its right side middle, with the connector's path.
pub const CONNECTOR_BODY: &str = concat!(
    r#"<draw:custom-shape xml:id="a" draw:name="A" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
    r#"<draw:connector draw:name="c" draw:start-shape="a" draw:start-glue-point="1" svg:x1="6cm" svg:y1="4cm" svg:x2="10cm" svg:y2="4cm" svg:d="M 6000 4000 h 4000" svg:viewBox="0 0 4001 1"/>"#
);

pub fn connector_drawing() -> Drawing {
    Drawing::a4().page(CONNECTOR_BODY)
}

#[test]
fn a_connector_keeps_its_path_in_a_save_that_does_not_reroute_it() {
    // Every connector read is marked as a route DiaCraft may route again, and an edit of another shape leaves its `svg:d` as the file has it.
    let d = Drawing::a4().page(&format!("{CONNECTOR_BODY}{}", rect("other", 12.0, 12.0, 1.0, 1.0)));
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 3, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:d="M 6000 4000 h 4000" svg:viewBox="0 0 4001 1""#), "{after}");
    assert!(after.contains(r#"svg:x="13cm""#));
    let diff = compare(&o.bytes, &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let (changed, _, _) = content_changes(&diff);
    assert_eq!(changed.len(), 1, "only the shape moved: {changed:?}");
}

/// Edits the `X` of the `LineTo` row of a connector's route.
pub fn set_route_end(connector: &mut diacraft_doc::Shape, x: f64) {
    let sheet = Arc::make_mut(&mut connector.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).unwrap();
    let section = Arc::make_mut(section);
    let row = section.rows.iter_mut().find(|r| r.row_type.as_deref() == Some("LineTo")).unwrap();
    Arc::make_mut(row).cells.get_mut("X").unwrap().value = Value::from_number(x);
}

#[test]
fn a_glued_connector_follows_a_shape_that_moved_and_was_rerouted() {
    // The shape moves 1 cm right, the engine reroutes the connector (its begin follows, its route is shorter), and the connector's `svg:x1`,
    // `svg:d` and `svg:viewBox` are written from the model; its glue attributes stay.
    let mut o = open(&connector_drawing());
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    {
        let c = shape_mut(&mut o.doc, 0, 2);
        let x = num_of(c, "BeginX");
        set_num(c, "BeginX", x + cm(1.0));
        set_route_end(c, cm(3.0));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"draw:start-shape="a" draw:start-glue-point="1" svg:x1="7cm" svg:y1="4cm" svg:x2="10cm" svg:y2="4cm" svg:d="M 7000 4000 h 3000" svg:viewBox="0 0 3001 1""#), "{after}");
    assert!(after.contains(r#"draw:name="A" svg:x="3cm""#));
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    let back = open_bytes(saved.bytes);
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "BeginX"), cm(7.0));
}

#[test]
fn a_connector_whose_shape_was_deleted_loses_that_end_s_glue() {
    // R9 delete: the shape's element goes; the connector's `draw:start-shape` and `draw:start-glue-point` go; its ends stay as they were.
    let mut o = open(&connector_drawing());
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        page.shapes.remove(0);
        page.connects = Connects::from_vec(Vec::new());
        // The engine leaves the end where it was, with no formula.
        let c = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut c.sheet);
        for name in ["BeginX", "BeginY"] {
            let cell = sheet.cells.get_mut(name).unwrap();
            cell.formula = diacraft_doc::FormulaSlot::Absent;
        }
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("draw:start-shape") && !after.contains("draw:start-glue-point") && !after.contains("draw:name=\"A\""), "{after}");
    assert!(
        after.contains(r#"<draw:connector draw:name="c" svg:x1="6cm" svg:y1="4cm" svg:x2="10cm" svg:y2="4cm" svg:d="M 6000 4000 h 4000""#),
        "{after}"
    );
    let back = open_bytes(saved.bytes);
    assert!(back.doc.pages.get(0).unwrap().connects.is_empty());
    // A glue made anew is not written by this task.
    let mut o = open(&connector_drawing());
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    page.connects.push(diacraft_doc::Connect {
        from_sheet: 2,
        from_cell: Some(Arc::from("EndX")),
        from_part: Some(12),
        to_sheet: 1,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(3),
        ..Default::default()
    });
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))));
}

/// A connector the file holds, glued to a shape the file does not hold (a new shape, or a shape an earlier save deleted and an undo put back): the writer writes the
/// glue of the connectors it makes and not a glue made anew on one of the file's, and it passed this one by, so a save was made and the file lacked a glue the model
/// has (the review). It is refused in words, whichever end is glued and whether the glue is made alone or beside a new shape's own.
#[test]
fn a_connector_of_the_file_glued_to_a_new_shape_is_refused_in_words_and_a_new_connector_to_a_file_shape_is_not() {
    for begin in [true, false] {
        let mut o = open(&connector_drawing());
        let id = o.index.next_id(0).unwrap();
        crate::write_new::push_shape(&mut o.doc, crate::write_new::rectangle(id, "N", (12.0, 3.0, 4.0, 2.0)));
        // The connector (shape 2) is glued to A at its begin by the file; its end is glued anew to the new shape, or its begin is.
        crate::write_new::glue(&mut o.doc, 2, begin, id, 2);
        let said = match real(&o.doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(words)) => words,
            other => panic!("begin={begin}: a connector of the file glued to a new shape was not refused: {:?}", other.map(|w| w.edited)),
        };
        assert!(said.contains("a connector of the file glued to a new shape"), "{said}");
    }
    // The new shape alone, with nothing of the file's glued to it, is written; and a new connector between the new shape and a shape of the file is too.
    let mut o = open(&connector_drawing());
    let id = o.index.next_id(0).unwrap();
    crate::write_new::push_shape(&mut o.doc, crate::write_new::rectangle(id, "N", (12.0, 3.0, 4.0, 2.0)));
    assert!(real(&o.doc, &o.bytes, &o.index).is_ok(), "a new shape with nothing glued to it");
    crate::write_new::push_shape(&mut o.doc, crate::write_new::connector(id + 1, (6.0, 4.0, 12.0, 4.0)));
    crate::write_new::glue(&mut o.doc, id + 1, true, 1, 0);
    crate::write_new::glue(&mut o.doc, id + 1, false, id, 1);
    let saved =
        real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a new connector between a new shape and one of the file's was refused: {e}"));
    assert!(part(&saved.bytes, "content.xml").contains("draw:end-shape="));
}

#[test]
fn a_signed_drawing_keeps_its_signature_and_says_it_no_longer_matches() {
    // An unedited save keeps the file and says nothing; a save after a move keeps the signature file's bytes and gives one note.
    let mut d = drawing_of(2);
    d.files.push(("META-INF/documentsignatures.xml".to_owned(), b"<sig/>".to_vec()));
    let mut o = open(&d);
    let unedited = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(unedited.notes.is_empty() && !unedited.edited);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.notes, [SaveNote::SignatureStale]);
    assert_eq!(part(&saved.bytes, "META-INF/documentsignatures.xml"), "<sig/>");
}

#[test]
fn the_thumbnail_is_written_on_an_edited_save_and_kept_otherwise() {
    // A package with a thumbnail keeps its bytes on an unedited save and has the picture passed after a move; one with none gains it with a row.
    let mut d = drawing_of(2);
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), vec![0x89, b'P', b'N', b'G', 9, 9]));
    let mut o = open(&d);
    assert_eq!(part_bytes(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "Thumbnails/thumbnail.png"), [0x89, b'P', b'N', b'G', 9, 9]);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(diacraft_odg::package::Package::read(&saved.bytes, &Limits::default()).unwrap().part("Thumbnails/thumbnail.png"), Some(&PNG[..]));
    assert_eq!(compare(&o.bytes, &saved.bytes).bytes_only, ["Thumbnails/thumbnail.png"]);
    // Passed none, an edited save keeps the old one.
    let kept = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default()).unwrap();
    assert_eq!(part_bytes(&kept.bytes, "Thumbnails/thumbnail.png"), [0x89, b'P', b'N', b'G', 9, 9]);
    // Not a PNG, or too large: refused, nothing written.
    let refuse = |thumbnail: Vec<u8>, limits: Limits| {
        write_with(
            &o.doc,
            Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &WriteOptions { thumbnail: Some(thumbnail), limits, ..WriteOptions::default() },
        )
    };
    assert!(matches!(refuse(b"GIF89a".to_vec(), Limits::default()), Err(WriteError::Thumbnail(_))));
    assert!(matches!(refuse(PNG.to_vec(), Limits { max_thumbnail_bytes: 10, ..Limits::default() }), Err(WriteError::Limit { .. })));
    // A picture of exactly the limit is written; one byte more is not. A signature alone is a thumbnail: the check reads no pixels.
    assert!(refuse(PNG.to_vec(), Limits { max_thumbnail_bytes: PNG.len(), ..Limits::default() }).is_ok());
    assert!(matches!(refuse(PNG.to_vec(), Limits { max_thumbnail_bytes: PNG.len() - 1, ..Limits::default() }), Err(WriteError::Limit { .. })));
    // And the numbers the writer's caps have by default.
    let l = Limits::default();
    assert_eq!((l.max_write_bytes, l.max_write_edits, l.max_write_attributes, l.max_write_steps), (1 << 30, 4_000_000, 4_000_000, 400_000_000));
    assert_eq!(l.max_thumbnail_bytes, 4 * 1024 * 1024);
}

#[test]
fn an_edited_save_adds_the_rows_a_manifest_lacks_and_keeps_every_other() {
    // `Pictures/a.png` is not listed: a row for it follows the last, every other row is as it was, and a row for a file the package lacks stays.
    let mut d = drawing_of(2);
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let entries = {
        let mut files = d.files();
        let rows = [("/", DRAWING), ("content.xml", "text/xml"), ("styles.xml", "text/xml"), ("Pictures/b.png", "image/png")];
        let mut manifest = String::from(
            "<?xml version='1.0' encoding='UTF-8'?>\n<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'>\n",
        );
        for (path, media) in rows {
            manifest.push_str(&format!("  <manifest:file-entry manifest:full-path='{path}' manifest:media-type='{media}'/>\n"));
        }
        manifest.push_str("</manifest:manifest>\n");
        files.push(("META-INF/manifest.xml".to_owned(), manifest.into_bytes()));
        files
    };
    let mut o = open_bytes(write(Some(DRAWING), &entries));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "META-INF/manifest.xml"), part(&saved.bytes, "META-INF/manifest.xml"));
    let head = before.trim_end_matches("</manifest:manifest>\n");
    assert!(after.starts_with(head), "every row as it was, in its quotes and spacing: {after}");
    let added = &after[head.len()..];
    for name in ["Pictures/a.png", "meta.xml", "Thumbnails/thumbnail.png"] {
        assert!(
            added.contains(&format!("manifest:full-path=\"{name}\"")) || added.contains(&format!("manifest:full-path='{name}'")),
            "{name}: {added}"
        );
    }
    assert!(
        added.contains("\n  <manifest:file-entry manifest:full-path=\"Pictures/a.png\" manifest:media-type=\"image/png\"/>"),
        "the white space before the last row, then the row: {added}"
    );
    assert!(after.contains("Pictures/b.png"), "the row for a missing file stays");
    // Read back, with a warning for the one missing file only.
    let report = read(&saved.bytes, &Limits::default()).unwrap().1;
    assert_eq!(report.warning_counts.get("manifest_incomplete"), None);
    assert_eq!(report.warning_counts.get("manifest_lists_missing_file"), Some(&1));
}

#[test]
fn an_edited_save_writes_mimetype_as_the_standard_asks() {
    // `mimetype` second in the archive is read with a warning; an unedited save keeps it where it is, an edited one writes it first.
    let mut entries = vec![("content.xml".to_owned(), drawing_of(2).content_xml().into_bytes())];
    entries.push(("styles.xml".to_owned(), drawing_of(2).styles_xml().into_bytes()));
    let manifest = crate::build::manifest(DRAWING, &[("content.xml", "text/xml"), ("styles.xml", "text/xml")]);
    entries.push(("META-INF/manifest.xml".to_owned(), manifest.into_bytes()));
    let bytes = {
        let mut w = diacraft_container::zip::ArchiveWriter::new();
        w.add("content.xml", &entries[0].1, diacraft_container::zip::Method::Deflated).unwrap();
        w.add("mimetype", DRAWING.as_bytes(), diacraft_container::zip::Method::Stored).unwrap();
        w.add("styles.xml", &entries[1].1, diacraft_container::zip::Method::Deflated).unwrap();
        w.add("META-INF/manifest.xml", &entries[2].1, diacraft_container::zip::Method::Deflated).unwrap();
        w.finish().unwrap()
    };
    let mut o = open_bytes(bytes);
    let unedited = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(unedited.bytes, o.bytes);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(&saved.bytes[30..38], b"mimetype");
    assert_eq!(&saved.bytes[38..38 + DRAWING.len()], DRAWING.as_bytes());
    assert!(diacraft_odg::package::Package::read(&saved.bytes, &Limits::default()).unwrap().mimetype_regular());
    assert_eq!(diacraft_odg::sniff(&saved.bytes), Some(diacraft_odg::Variant::Drawing));
}

#[test]
fn what_the_writer_does_not_write_yet_is_refused_and_nothing_is_written() {
    let base = open(&drawing_of(2));
    let refused = |doc: &diacraft_doc::Document, words: &str| match real(doc, &base.bytes, &base.index) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
        other => panic!("expected a refusal of {words}: {other:?}"),
    };
    // A restyle, a retext, a geometry edit, a flip, a shape added, a page renamed, the title changed, a page deleted.
    let mut doc = base.doc.clone();
    Arc::make_mut(&mut shape_mut(&mut doc, 0, 1).sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("0.5")));
    // A cell no attribute says is carried by the extension, and the file reads back with it.
    let saved = real(&doc, &base.bytes, &base.index).unwrap_or_else(|e| panic!("a user cell was refused: {e}"));
    let back = open_bytes(saved.bytes);
    assert!(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Comment").is_some());
    let mut doc = base.doc.clone();
    // A text with a tab stop marker is not written (a paragraph added is, `write_runs.rs`).
    shape_mut(&mut doc, 0, 1).text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![diacraft_doc::TextPart::Tp(0), diacraft_doc::TextPart::Chars(Arc::from("new"))],
        ..Default::default()
    }));
    refused(&doc, "tab stop");
    let mut doc = base.doc.clone();
    Arc::make_mut(&mut shape_mut(&mut doc, 0, 1).sheet).cells.insert(CellName::new("Rounding"), Cell::constant(Value::from_text("0.2")));
    let saved = real(&doc, &base.bytes, &base.index).unwrap_or_else(|e| panic!("a rounding was refused: {e}"));
    assert!(open_bytes(saved.bytes).doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Rounding").is_some());
    let mut doc = base.doc.clone();
    {
        let s = shape_mut(&mut doc, 0, 1);
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("FlipY"), Cell::constant(Value::from_text("1")));
    }
    refused(&doc, "flip");
    let mut doc = base.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.push(Arc::new(diacraft_doc::Shape::new(99)));
    // A new shape with an empty sheet is written: what the reader would rebuild of it that the model lacks is said gone, and it reads back empty.
    let saved = real(&doc, &base.bytes, &base.index).unwrap_or_else(|e| panic!("a new shape with an empty sheet: {e}"));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes);
    let read = back.doc.pages.get(0).unwrap().shapes.get(2).unwrap().clone();
    assert!(read.sheet.cells.is_empty() && read.sheet.sections.is_empty(), "{:?}", read.sheet);
    // A page renamed is written.
    let mut doc = base.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name = Arc::from("Renamed");
    let saved = real(&doc, &base.bytes, &base.index).unwrap_or_else(|e| panic!("a page renamed was refused: {e}"));
    assert_eq!(open_bytes(saved.bytes).doc.pages.get(0).unwrap().name.as_ref(), "Renamed");
    let mut doc = base.doc.clone();
    doc.title = Arc::from("Another");
    refused(&doc, "document");
    // A drawing of no page is no drawing: the words say a drawing keeps one.
    let mut doc = base.doc.clone();
    doc.pages = diacraft_doc::SharedVec::new();
    refused(&doc, "no page");
}

#[test]
fn an_original_that_is_not_the_file_the_model_came_from_is_refused_or_written_as_the_model_and_never_a_panic() {
    // `a` holds one page and `b` six. A model's shapes may be renamed and moved, and its pages may be added and taken away, so no edit tells the files apart by
    // the pages: the model of one file against the bytes and the index of another is either refused, or a save that reads back as the model (the pages the model lacks
    // taken away, the pages the file lacks added), and never a panic.
    let a = open(&Drawing::a4().page(&(1..=3).map(|k| custom(&format!("a{k}"), &format!("{k}cm"))).collect::<String>()));
    let b = open(&drawing_of(5).page(&custom("only", "1cm")));
    // The model of one file, the bytes and the index of another.
    let against = |doc: &diacraft_doc::Document, o: &crate::write_core::Open| real(doc, &o.bytes, &o.index);
    for (doc, o) in [(&a.doc, &b), (&b.doc, &a)] {
        if let Ok(saved) = against(doc, o) {
            assert_eq!(open_bytes(saved.bytes).doc.pages.len(), doc.pages.len(), "a save is the model's pages");
        }
    }
    // The right file and the index of another.
    assert!(real(&a.doc, &a.bytes, &b.index).is_err());
    // No index for the pages at all.
    assert!(matches!(real(&a.doc, &a.bytes, &diacraft_odg::ShapeIndex::default()), Err(WriteError::Mismatch(_) | WriteError::Unsupported(_))));
    // Bytes that are no package; a package that is not a drawing.
    assert!(matches!(real(&a.doc, b"not a zip", &a.index), Err(WriteError::Original(_))));
    // A drawing that was never saved has no original, and is written from the model (`model_write.rs`).
}

#[test]
fn an_index_that_does_not_hold_the_files_pages_is_refused_whichever_of_the_three_differs() {
    let pages = |n: usize| {
        let mut d = Drawing::a4();
        for k in 0..n {
            d = d.page(&custom(&format!("p{k}"), "1cm"));
        }
        open(&d)
    };
    let (one, two, three) = (pages(1), pages(2), pages(3));
    // The index is of the file or it is nothing: one that holds other pages than the file has is refused, whatever the model holds, from the page counts and before the
    // file is read for more (what it would read is held to the index's cost, and an index that disagrees is not to be trusted for one).
    let mismatch = |saved: Result<diacraft_odg::Written, WriteError>, what: &str| match saved {
        Err(e @ WriteError::Mismatch(_)) => assert_eq!(e.class(), "write_mismatch", "{what}"),
        other => panic!("{what}: {other:?}"),
    };
    mismatch(real(&one.doc, &two.bytes, &one.index), "the model and the index hold one page, the file two");
    mismatch(real(&two.doc, &two.bytes, &one.index), "an index of fewer pages than the file");
    mismatch(real(&two.doc, &two.bytes, &three.index), "an index of more pages than the file");
    mismatch(real(&three.doc, &two.bytes, &three.index), "the model and the index hold three pages, the file two");
    // A model of other pages than the file's, with the file's own index, is pages added or taken away (`write_page_ops.rs`), which are written: the model of one page is the
    // file's two with the second taken away, the model of three the file's two with one added.
    assert!(real(&one.doc, &two.bytes, &two.index).is_ok_and(|w| w.index.pages() == 1), "a page taken away");
    let mut added = two.doc.clone();
    let mut page = diacraft_doc::Page::new(2, "Page-3");
    page.sheet = Arc::clone(&added.pages.get(0).unwrap().sheet);
    added.pages.push(Arc::new(page));
    assert!(real(&added, &two.bytes, &two.index).is_ok_and(|w| w.index.pages() == 3), "a page added");
    // And the three that agree save.
    assert!(real(&two.doc, &two.bytes, &two.index).is_ok());
}

#[test]
fn a_second_save_with_nothing_changed_is_the_first_save_byte_for_byte() {
    // The model holds the numbers a calculation made, the file the decimals written: nothing is an edit but an edit.
    let mut o = open(&drawing_of(4));
    move_by(&mut o.doc, 0, 2, 1.1, -0.3);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited, "{:?}", compare(&first.bytes, &again.bytes));
    assert_eq!(again.bytes, first.bytes);
    assert_eq!(again.index, first.index);
}

#[test]
fn an_edit_of_a_file_that_binds_svg_to_the_w3c_namespace_declares_odf_s_own_under_a_prefix_of_its_own() {
    // A shape placed by `draw:transform` alone, written with `svg:x` and `svg:y` once it is turned back to 0, gets them under ODF's
    // namespace declared on the element as `svg1`; the file's own `svg` (the W3C's) and its attributes are not touched.
    let mut d = Drawing::a4().page(r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#);
    d.ns = d.ns.replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "http://www.w3.org/2000/svg");
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 1, 0.0, 0.0);
    // Reshaped (wider), unturned: the transform goes, `svg:x` and `svg:y` come.
    {
        let s = shape_mut(&mut o.doc, 0, 1);
        set_num(s, "Width", cm(6.0));
        set_num(s, "LocPinX", cm(3.0));
    }
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:width="6cm""#), "the file's own attribute is kept under its own prefix: {after}");
    assert!(after.contains(r#"xmlns:svg1="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#), "{after}");
    assert!(after.contains("svg1:x=\"") && after.contains("svg1:y=\""), "{after}");
    assert!(after.contains(r#"xmlns:svg="http://www.w3.org/2000/svg""#), "the file's binding is as it was");
    assert!(!after.contains(" svg:x=") && !after.contains(" svg:y="), "nothing is written under the W3C binding: {after}");
    // Both readers are right: this reader reads the position, and the model reads back as it was edited.
    let back = open_bytes(real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let (s, was) = (back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), o.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
    close(num_of(s, "PinX"), num_of(was, "PinX"));
    close(num_of(s, "PinY"), num_of(was, "PinY"));
    close(num_of(s, "Width"), cm(6.0));
    // With ODF's own `svg`, the plain prefix is used and nothing is declared.
    let mut o = open(&Drawing::a4().page(r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#));
    set_num(shape_mut(&mut o.doc, 0, 1), "Width", cm(6.0));
    set_num(shape_mut(&mut o.doc, 0, 1), "LocPinX", cm(3.0));
    let after = saved_content(&o);
    assert!(after.contains(" svg:x=\"") && !after.contains("svg1"), "{after}");
}

#[test]
fn a_prefix_that_names_odf_s_namespace_is_found_whatever_it_is_called() {
    // The least prefix bound to ODF's `svg` is used for what is added: here `s`, with `svg` bound to something else, and `t` bound too.
    let mut d = Drawing::a4().page(r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#);
    d.ns = d.ns.replace("xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"", "xmlns:svg=\"http://example.com/svg\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:s=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"");
    d.pages = d.pages.replace("svg:width", "s:width").replace("svg:height", "s:height");
    let mut o = open(&d);
    set_num(shape_mut(&mut o.doc, 0, 1), "Width", cm(6.0));
    set_num(shape_mut(&mut o.doc, 0, 1), "LocPinX", cm(3.0));
    let after = saved_content(&o);
    assert!(after.contains(" s:x=\"") && after.contains(" s:y=\"") && !after.contains("svg1"), "{after}");
}

#[test]
fn a_part_written_in_utf_16_is_read_and_a_change_to_it_is_refused() {
    let d = drawing_of(2);
    let content: Vec<u8> = std::iter::once(0xFEFFu16)
        .chain(d.content_xml().replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"").encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut files = d.files();
    files[0].1 = content;
    let mut o = open_bytes(crate::build::package(DRAWING, files));
    assert!(!real(&o.doc, &o.bytes, &o.index).unwrap().edited);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))));
}

#[test]
fn a_cap_refuses_the_save_with_a_class_and_writes_nothing() {
    let mut o = open(&drawing_of(3));
    // Moved in both directions: two edits to the part.
    move_by(&mut o.doc, 0, 2, 1.0, 0.5);
    let with = |limits: Limits| {
        write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions { limits, ..WriteOptions::default() })
    };
    let class = |limits: Limits| with(limits).err().map(|e| e.class());
    assert_eq!(with(Limits::default()).map(|w| w.edited).ok(), Some(true));
    for (what, tight) in [
        ("edits", Limits { max_write_edits: 1, ..Limits::default() }),
        ("bytes", Limits { max_write_bytes: 100, ..Limits::default() }),
        ("attributes", Limits { max_write_attributes: 0, ..Limits::default() }),
        ("steps", Limits { max_write_steps: 1, ..Limits::default() }),
    ] {
        assert_eq!(class(tight), Some("write_limit"), "{what}");
    }
    // The reader's caps hold for the read of the original.
    assert_eq!(class(Limits { max_shapes: 1, ..Limits::default() }), Some("too_many_shapes"));
    let _ = (custom, NS);
}

/// A page body of three rectangles with a note of another vocabulary between the first and the second, and the same inside a group.
const WITH_NOTES: &str = concat!(
    r#"<draw:rect draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
    r#"<ex:note xmlns:ex="urn:example">kept</ex:note>"#,
    r#"<draw:rect draw:name="b" svg:x="3cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
    r#"<draw:rect draw:name="c" svg:x="5cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
    r#"<draw:g draw:name="g">"#,
    r#"<draw:rect draw:name="ga" svg:x="1cm" svg:y="5cm" svg:width="1cm" svg:height="1cm"/>"#,
    r#"<ex:note xmlns:ex="urn:example">kept in the group</ex:note>"#,
    r#"<draw:rect draw:name="gb" svg:x="3cm" svg:y="5cm" svg:width="1cm" svg:height="1cm"/>"#,
    r#"</draw:g>"#
);

#[test]
fn a_shape_whose_own_attributes_read_alike_is_saved_and_only_the_names_the_save_wrote_are_looked_for_twice() {
    // Found by the fuzz target (final run), the first seconds after the check of the bytes made was made to read back the
    // tags it edited: `qvg:x` and `ssvg:x` are prefixes bound to nothing, which the reader reads as no namespace, both alike; the first check
    // that looked for every name twice refused the save of a move of that shape, a fault of the check and not of the file or the save.
    let body = concat!(
        r#"<draw:rect draw:name="a" me="a" qvg:x="1cm" ssvg:x="1cm" svg:y="1cm" svtidgw:h="2cm" svg:height="1cm"/>"#,
        r#"<draw:ellipse draw:name="b" svg:x="4cm" svg:y="1cm" svg:wih="2cm" svg:height="1cm"/>"#
    );
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(saved.edited);
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains(r#"qvg:x="1cm" ssvg:x="1cm""#), "the file's own attributes are as they were: {content}");
    assert!(content.contains(r#"svg:x=""#), "and the one the move needs is written: {content}");
    // It reads again, and a second save of the model through the new index is no edit.
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.pages.get(0).map(|p| p.shapes.len()), Some(2));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited);
}

#[test]
fn a_second_save_after_a_reorder_or_a_delete_past_a_kept_node_is_no_edit() {
    // A kept node is placed `after` the modelled shapes before it; a reorder or a delete of those moves it in the file, and the model the caller
    // still holds carries the old number. That is no change of the kept node: the second save through the new index finds nothing to write.
    type Edit = (&'static str, fn(&mut diacraft_doc::Document));
    let edits: [Edit; 4] = [
        ("a page reorder past the note", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let a = page.shapes.remove(0).unwrap();
            page.shapes.insert(2, a).unwrap();
        }),
        ("a page delete before the note", |doc| {
            Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.remove(0);
        }),
        ("a group reorder past the note", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let group = Arc::make_mut(page.shapes.get_mut(3).unwrap());
            let first = group.children.remove(0).unwrap();
            group.children.push(first);
        }),
        ("a group delete before the note", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            Arc::make_mut(page.shapes.get_mut(3).unwrap()).children.remove(0);
        }),
    ];
    for (what, edit) in edits {
        let o = open(&Drawing::a4().page(WITH_NOTES));
        let mut doc = o.doc.clone();
        edit(&mut doc);
        let first = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: the first save: {e:?}"));
        assert!(first.edited, "{what}");
        let again = real(&doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("{what}: the second save: {e:?}"));
        assert!(!again.edited, "{what}: nothing changed since the first save");
        assert_eq!(again.bytes, first.bytes, "{what}");
        // Both notes are still in the file, as they were.
        let content = part(&again.bytes, "content.xml");
        assert!(content.contains(">kept</ex:note>") && content.contains(">kept in the group</ex:note>"), "{what}: {content}");
    }
}

#[test]
fn a_kept_node_that_the_model_lost_or_changed_is_still_refused() {
    // Only the number of shapes before a kept node is let go: the node itself is compared.
    let o = open(&Drawing::a4().page(WITH_NOTES));
    let mut doc = o.doc.clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).ext.make_mut().odf.kept.kept = diacraft_doc::Passthrough::new();
    assert!(matches!(real(&doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))), "the page's kept node is gone from the model");
    let mut doc = o.doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(page.shapes.get_mut(3).unwrap()).ext.make_mut().odf.kept.kept = diacraft_doc::Passthrough::new();
    assert!(matches!(real(&doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))), "the group's kept node is gone from the model");
}

/// The names of a package's entries, in the order they lie in the file.
fn entry_names(bytes: &[u8]) -> Vec<String> {
    diacraft_container::zip::Archive::read(bytes, &diacraft_container::Limits::default()).unwrap().entries().iter().map(|e| e.name.clone()).collect()
}

#[test]
fn the_entries_an_edited_save_adds_go_before_a_manifest_that_lies_last_and_at_the_end_when_it_does_not() {
    const MANIFEST: &str = "META-INF/manifest.xml";
    const THUMBNAIL: &str = "Thumbnails/thumbnail.png";
    // The usual layout: the manifest is the last entry, and the new entries (the thumbnail, a meta part) lie before it.
    let mut o = open(&drawing_of(2));
    assert_eq!(entry_names(&o.bytes).last().map(String::as_str), Some(MANIFEST));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let names = entry_names(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let at = |name: &str| names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name} in {names:?}"));
    assert_eq!(at(MANIFEST), names.len() - 1, "{names:?}");
    assert!(at(THUMBNAIL) < at(MANIFEST) && at("meta.xml") < at(MANIFEST), "{names:?}");
    // Another producer's layout, with the manifest in the middle: the new entries follow the last entry.
    let d = drawing_of(2);
    let mut files = d.files();
    let rows = [("content.xml", "text/xml"), ("styles.xml", "text/xml")];
    files.insert(1, (MANIFEST.to_owned(), crate::build::manifest(DRAWING, &rows).into_bytes()));
    let mut o = open_bytes(write(Some(DRAWING), &files));
    let before = entry_names(&o.bytes);
    assert_ne!(before.last().map(String::as_str), Some(MANIFEST), "{before:?}");
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let names = entry_names(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let at = |name: &str| names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name} in {names:?}"));
    assert!(at(MANIFEST) < at(THUMBNAIL), "{names:?}");
    assert_eq!(&names[..before.len()], &before[..], "the file's own entries keep their places");
}

#[test]
fn an_ellipse_by_its_centre_moves_by_it_and_a_frame_with_a_stray_centre_attribute_moves_by_its_frame() {
    // V45, for an ellipse: `svg:cx` and `svg:cy` move, `svg:rx` and `svg:ry` stay, and no `svg:x` is written.
    let body = r#"<draw:ellipse draw:name="e" svg:cx="5cm" svg:cy="4cm" svg:rx="2cm" svg:ry="1cm"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:cx="6cm" svg:cy="4cm" svg:rx="2cm" svg:ry="1cm""#), "{after}");
    assert!(!after.contains("svg:x="), "{after}");
    // The reader reads the centre form only when it has the whole of it; an element with part of it is a frame, and the stray attribute is
    // not the writer's to move.
    for (kind, stray) in [
        ("circle", r#"svg:cx="9cm""#),
        ("circle", r#"svg:cy="9cm""#),
        ("circle", r#"svg:r="1cm""#),
        ("circle", r#"svg:cx="9cm" svg:cy="9cm""#),
        ("ellipse", r#"svg:cx="9cm""#),
        ("ellipse", r#"svg:cx="9cm" svg:cy="9cm""#),
        ("ellipse", r#"svg:cx="9cm" svg:cy="9cm" svg:rx="1cm""#),
        ("ellipse", r#"svg:rx="1cm" svg:ry="1cm""#),
        ("ellipse", r#"svg:ry="1cm""#),
        ("ellipse", r#"svg:cy="9cm" svg:rx="1cm" svg:ry="1cm""#),
    ] {
        let body = format!(r#"<draw:{kind} draw:name="s" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm" {stray}/>"#);
        let mut o = open(&Drawing::a4().page(&body));
        move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        let after = saved_content(&o);
        assert!(after.contains(r#"svg:x="2cm" svg:y="1cm""#) && after.contains(stray), "{kind} with {stray}: {after}");
    }
}

/// Two shapes and a connector glued to both, begin to the first and end to the second.
const TWO_GLUES: &str = concat!(
    r#"<draw:custom-shape xml:id="a" draw:name="A" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
    r#"<draw:custom-shape xml:id="b" draw:name="B" svg:x="10cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
    r#"<draw:connector draw:name="c" draw:start-shape="a" draw:start-glue-point="1" draw:end-shape="b" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="10cm" svg:y2="4cm" svg:d="M 6000 4000 h 4000" svg:viewBox="0 0 4001 1"/>"#
);

#[test]
fn a_connector_loses_the_glue_of_an_end_whose_shape_was_deleted_and_the_next_save_through_the_new_index_finds_nothing_to_do() {
    let mut o = open(&Drawing::a4().page(TWO_GLUES));
    let rows = o.doc.pages.get(0).unwrap().connects.len();
    assert_eq!(rows, 2, "begin and end are glued");
    {
        // Shape B (ID 2) is deleted, and the connection to it with it; the engine leaves the end where it was, with no formula.
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        page.shapes.remove(1);
        let kept: Vec<_> = page.connects.iter().filter(|c| c.to_sheet != 2).cloned().collect();
        assert_eq!(kept.len(), 1);
        page.connects = Connects::from_vec(kept);
        let c = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let sheet = Arc::make_mut(&mut c.sheet);
        for name in ["EndX", "EndY"] {
            sheet.cells.get_mut(name).unwrap().formula = diacraft_doc::FormulaSlot::Absent;
        }
    }
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&first.bytes, "content.xml");
    assert!(!after.contains("draw:end-shape") && !after.contains("draw:end-glue-point") && !after.contains(r#"draw:name="B""#), "{after}");
    assert!(after.contains(r#"draw:start-shape="a" draw:start-glue-point="1""#), "the begin keeps its glue: {after}");
    // The element numbers moved by the delete, and the connector's `ID` in the model is not the one the file now gives it: the connection row
    // that is left is the file's own, by the elements, and the second save through the new index has nothing to write.
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited, "{:?}", compare(&first.bytes, &again.bytes));
    assert_eq!(again.bytes, first.bytes);
    // The other way: shape A (ID 1), which the begin is glued to, is deleted, and shape B and the connector are renumbered by it. The connection that
    // is left is the end's, and the model names its shapes by the IDs it had, which are not the ones the file now gives them.
    let mut o = open(&Drawing::a4().page(TWO_GLUES));
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        page.shapes.remove(0);
        let kept: Vec<_> = page.connects.iter().filter(|c| c.to_sheet != 1).cloned().collect();
        assert_eq!(kept.len(), 1);
        page.connects = Connects::from_vec(kept);
        let c = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let sheet = Arc::make_mut(&mut c.sheet);
        for name in ["BeginX", "BeginY"] {
            sheet.cells.get_mut(name).unwrap().formula = diacraft_doc::FormulaSlot::Absent;
        }
    }
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&first.bytes, "content.xml");
    assert!(!after.contains("draw:start-shape") && after.contains(r#"draw:end-shape="b" draw:end-glue-point="3""#), "{after}");
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited, "{:?}", compare(&first.bytes, &again.bytes));
    // A connection the file does not have is refused, whichever way round its ends are.
    let mut o = open(&Drawing::a4().page(TWO_GLUES));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let mut row = page.connects.iter().next().cloned().unwrap();
    std::mem::swap(&mut row.from_sheet, &mut row.to_sheet);
    page.connects.push(row);
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))));
}

#[test]
fn an_ellipse_by_its_centre_resized_keeps_its_form_and_a_circle_cannot_be_given_two_radii() {
    // 4 cm by 2 cm about (5, 4) becomes 6 cm by 5 cm about the same centre: `svg:rx` is half the width and `svg:ry` half the height.
    let body = r#"<draw:ellipse draw:name="e" svg:cx="5cm" svg:cy="4cm" svg:rx="2cm" svg:ry="1cm"/>"#;
    let resize = |body: &str, w: f64, h: f64| {
        let mut o = open(&Drawing::a4().page(body));
        let s = shape_mut(&mut o.doc, 0, 1);
        for (cell, v) in [("Width", cm(w)), ("Height", cm(h)), ("LocPinX", cm(w / 2.0)), ("LocPinY", cm(h / 2.0))] {
            set_num(s, cell, v);
        }
        o
    };
    let o = resize(body, 6.0, 5.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"svg:cx="5cm" svg:cy="4cm" svg:rx="3cm" svg:ry="2.5cm""#), "{after}");
    let back = open_bytes(real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "Width"), cm(6.0));
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "Height"), cm(5.0));
    // A circle has one radius.
    let circle = r#"<draw:circle draw:name="c" svg:cx="5cm" svg:cy="4cm" svg:r="1cm"/>"#;
    let o = resize(circle, 4.0, 2.0);
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(_))));
    let o = resize(circle, 4.0, 4.0);
    assert!(saved_content(&o).contains(r#"svg:r="2cm""#));
}

#[test]
fn a_deleted_group_takes_its_members_and_every_group_in_them_and_the_next_save_finds_the_right_element() {
    let body = format!(
        "{}<draw:g draw:name=\"g\">{}<draw:g draw:name=\"g2\">{}</draw:g>{}</draw:g>{}",
        rect("a", 1.0, 1.0, 1.0, 1.0),
        rect("m1", 3.0, 1.0, 1.0, 1.0),
        rect("m2", 5.0, 1.0, 1.0, 1.0),
        rect("m3", 7.0, 1.0, 1.0, 1.0),
        rect("z", 9.0, 1.0, 1.0, 1.0)
    );
    let mut o = open(&Drawing::a4().page(&body));
    let names = |d: &diacraft_doc::Document| -> Vec<String> {
        d.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap_or("?").to_owned()).collect()
    };
    assert_eq!(names(&o.doc), ["a", "g", "z"]);
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(1);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&first.bytes, "content.xml");
    assert!(!after.contains("draw:g") && !after.contains("m1") && !after.contains("m2") && !after.contains("m3"), "{after}");
    // The index counts the two elements the file holds now, and the shape `z` is the second: moving it through that index moves it.
    assert_eq!(first.index.page(0).map(|p| p.elements()), Some(2));
    move_by(&mut o.doc, 0, 7, 1.0, 0.0);
    let second = real(&o.doc, &first.bytes, &first.index).unwrap();
    let after = part(&second.bytes, "content.xml");
    assert!(after.contains(r#"draw:name="z" svg:x="10cm""#) && after.contains(r#"draw:name="a" svg:x="1cm""#), "{after}");
}

#[test]
fn an_element_the_reader_keeps_unmodelled_is_renumbered_with_the_rest_and_goes_with_the_group_that_held_it() {
    let wrapped = format!("<draw:a xlink:href=\"http://example.org\">{}</draw:a>", rect("b", 3.0, 1.0, 1.0, 1.0));
    let id_of = |d: &diacraft_doc::Document, name: &str| -> u32 {
        d.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).map(|s| s.id).unwrap_or_else(|| panic!("no shape {name}"))
    };
    // Elements in document order: `a` 1, the wrapper 2 and the shape in it 3 (both kept, the reader models neither), `c` 4. Deleting `a` numbers
    // the wrapper 1, the shape in it 2 and `c` 3.
    let body = format!("{}{wrapped}{}", rect("a", 1.0, 1.0, 1.0, 1.0), rect("c", 5.0, 1.0, 1.0, 1.0));
    let mut o = open(&Drawing::a4().page(&body));
    assert_eq!(o.index.page(0).map(|p| (p.kept().to_vec(), p.elements())), Some((vec![2, 3], 4)));
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(first.index.page(0).map(|p| (p.kept().to_vec(), p.elements())), Some((vec![1, 2], 3)));
    let c = id_of(&o.doc, "c");
    move_by(&mut o.doc, 0, c, 1.0, 0.0);
    let second = real(&o.doc, &first.bytes, &first.index).unwrap();
    let after = part(&second.bytes, "content.xml");
    assert!(
        after.contains(r#"draw:name="c" svg:x="6cm""#) && after.contains(r#"draw:name="b" svg:x="3cm""#) && after.contains("<draw:a "),
        "{after}"
    );
    // A wrapper inside a group that is deleted goes with it, and is no longer kept.
    let body = format!("<draw:g draw:name=\"g\">{wrapped}</draw:g>{}", rect("c", 5.0, 1.0, 1.0, 1.0));
    let mut o = open(&Drawing::a4().page(&body));
    assert_eq!(o.index.page(0).map(|p| p.kept().to_vec()), Some(vec![2, 3]));
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(first.index.page(0).map(|p| (p.kept().to_vec(), p.elements())), Some((Vec::new(), 1)));
    assert!(!part(&first.bytes, "content.xml").contains("draw:a "));
}

#[test]
fn every_kind_of_shape_element_is_numbered_as_the_reader_numbers_it_so_an_edit_after_them_finds_its_element() {
    let kinds = [
        r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#,
        // A shape element inside a shape that is not a group: the reader does not number it, and neither may the scan.
        r#"<draw:rect svg:width="1cm" svg:height="1cm"><draw:ellipse svg:width="1cm" svg:height="1cm"/></draw:rect>"#,
        r#"<draw:ellipse svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:circle svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:line svg:x1="1cm" svg:y1="1cm" svg:x2="2cm" svg:y2="2cm"/>"#,
        r#"<draw:polyline svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 10 10" draw:points="0,0 10,0 10,10"/>"#,
        r#"<draw:polygon svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"/>"#,
        r#"<draw:path svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 10 10" svg:d="M0 0 L10 0 L10 10 Z"/>"#,
        r#"<draw:regular-polygon svg:width="3cm" svg:height="3cm" draw:corners="3" draw:concave="false"/>"#,
        r#"<draw:frame svg:width="3cm" svg:height="1cm"><draw:text-box><text:p>t</text:p></draw:text-box></draw:frame>"#,
        r#"<draw:custom-shape svg:width="1cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
        r#"<draw:frame svg:width="3cm" svg:height="1cm"><draw:text-box><draw:custom-shape svg:width="1cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape></draw:text-box></draw:frame>"#,
        r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="4cm" svg:y2="1cm" svg:d="M 1000 1000 h 3000" svg:viewBox="0 0 3001 1"/>"#,
        r#"<draw:g><draw:rect svg:width="1cm" svg:height="1cm"/></draw:g>"#,
        r#"<draw:measure svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="1cm"><text:p>4</text:p></draw:measure>"#,
        r#"<draw:caption svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:caption-point-x="1cm" draw:caption-point-y="2cm"/>"#,
        r#"<draw:control svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:control="c1"/>"#,
        r#"<dr3d:scene svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:a xlink:href="http://example.org"><draw:rect svg:width="1cm" svg:height="1cm"/></draw:a>"#,
    ];
    let body = format!("{}{}", kinds.concat(), r#"<draw:rect draw:name="last" svg:x="2cm" svg:y="2cm" svg:width="1cm" svg:height="1cm"/>"#);
    let mut o = open(&Drawing::a4().page(&body));
    let last = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("last")).map(|s| s.id).unwrap();
    move_by(&mut o.doc, 0, last, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"draw:name="last" svg:x="3cm""#), "{after}");
    // Each kind, alone before the one that moves.
    for kind in kinds {
        let body = format!("{kind}{}", r#"<draw:rect draw:name="last" svg:x="2cm" svg:y="2cm" svg:width="1cm" svg:height="1cm"/>"#);
        let mut o = open(&Drawing::a4().page(&body));
        let last = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("last")).map(|s| s.id).unwrap();
        move_by(&mut o.doc, 0, last, 1.0, 0.0);
        let saved = real(&o.doc, &o.bytes, &o.index);
        assert!(saved.is_ok(), "{kind}: {:?}", saved.err());
        assert!(part(&saved.unwrap().bytes, "content.xml").contains(r#"draw:name="last" svg:x="3cm""#), "{kind}");
    }
}

#[test]
fn a_default_namespace_names_elements_and_never_attributes() {
    const DRAWING_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    // Elements in the default namespace are the drawing's: a rectangle written without a prefix is found, numbered and moved.
    let body = format!(
        r#"<rect xmlns="{DRAWING_NS}" draw:name="plain" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/><draw:rect draw:name="last" svg:x="2cm" svg:y="2cm" svg:width="1cm" svg:height="1cm"/>"#
    );
    let mut o = open(&Drawing::a4().page(&body));
    let names: Vec<String> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap_or("?").to_owned()).collect();
    assert_eq!(names, ["plain", "last"]);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let after = saved_content(&o);
    assert!(after.contains(r#"draw:name="plain" svg:x="2cm""#) && after.contains(r#"draw:name="last" svg:x="3cm""#), "{after}");
    // An attribute without a prefix is in no namespace, whatever the default is: `href` under a default of XLink's namespace names nothing, so
    // the frame whose object this is does not keep the object alive, and deleting the real frame takes it.
    let d = Drawing::a4().page(concat!(
        r#"<draw:frame draw:name="ole" svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/></draw:frame>"#,
        r#"<draw:frame draw:name="decoy" svg:x="2cm" svg:y="8cm" svg:width="5cm" svg:height="3cm"><draw:image xmlns="http://www.w3.org/1999/xlink" href="./Object 1"/></draw:frame>"#,
    ));
    let mut d = d;
    d.files.push(("Object 1/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
    let mut o = open(&d);
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!crate::write_core::has_part(&saved.bytes, "Object 1/content.xml"), "the object went with its frame");
}

#[test]
fn a_fresh_prefix_is_one_no_element_of_the_part_declares() {
    // The W3C binding of `svg`: a shape placed by its transform alone, widened, needs ODF's namespace declared under a prefix of its own. `svg1`
    // and `svg2` are bound on the group before it, closed by then; the prefix is the first that is bound nowhere in the part, `svg3`, which is free
    // wherever it is written (it is looked for once for the part, and no lookup walks a chain of the part's declarations).
    let body = concat!(
        r#"<draw:g xmlns:svg1="urn:example:one" xmlns:svg2="urn:example:two"><draw:rect draw:name="in" svg:width="1cm" svg:height="1cm" draw:transform="translate (1cm 1cm)"/></draw:g>"#,
        r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#
    );
    let mut d = Drawing::a4().page(body);
    d.ns = d.ns.replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "http://www.w3.org/2000/svg");
    let mut o = open(&d);
    let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("s")).map(|s| s.id).unwrap();
    let s = shape_mut(&mut o.doc, 0, id);
    set_num(s, "Width", cm(6.0));
    set_num(s, "LocPinX", cm(3.0));
    let after = saved_content(&o);
    let tag = after.split("<draw:rect draw:name=\"s\"").nth(1).unwrap_or_default();
    assert!(tag.contains(r#"xmlns:svg3="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#), "{after}");
    assert!(tag.contains("svg3:x=\"") && !tag.contains("svg1:") && !tag.contains("svg2:"), "{after}");
}

#[test]
fn the_prefix_an_attribute_is_added_under_is_the_least_one_in_force_on_its_tag() {
    // `a`, bound to ODF's svg namespace on a shape that is closed, is not in force on the next; `b`, bound on the shape itself, is, and is less than
    // the `svg` the root binds, so it is the one; and the shape after that sees `svg` alone.
    const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
    let wide = |name: &str, extra: &str| {
        format!(r#"<draw:rect {extra} draw:name="{name}" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#)
    };
    let body = format!(
        r#"<draw:rect xmlns:a="{SVG}" draw:name="in" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>{}{}"#,
        wide("s", &format!(r#"xmlns:b="{SVG}""#)),
        wide("t", "")
    );
    let mut o = open(&Drawing::a4().page(&body));
    for name in ["s", "t"] {
        let id = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).map(|s| s.id).unwrap();
        let shape = shape_mut(&mut o.doc, 0, id);
        set_num(shape, "Width", cm(6.0));
        set_num(shape, "LocPinX", cm(3.0));
    }
    let after = saved_content(&o);
    let tag_of =
        |name: &str| after.split(&format!("draw:name=\"{name}\"")).nth(1).and_then(|rest| rest.split("/>").next()).unwrap_or_default().to_owned();
    let (s, t) = (tag_of("s"), tag_of("t"));
    assert!(s.contains("b:x=\"") && s.contains("b:y=\"") && !s.contains("a:x") && !s.contains(" svg:x="), "{after}");
    assert!(t.contains(" svg:x=\"") && !t.contains("b:x") && !t.contains("a:x"), "{after}");
}

#[test]
fn text_between_shapes_stays_when_a_shape_is_deleted_and_white_space_goes_with_it() {
    let body = format!("note {} more {}\n  {}", rect("a", 1.0, 1.0, 1.0, 1.0), rect("b", 3.0, 1.0, 1.0, 1.0), rect("c", 5.0, 1.0, 1.0, 1.0));
    let mut o = open(&Drawing::a4().page(&body));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    page.shapes.remove(2);
    page.shapes.remove(0);
    let after = saved_content(&o);
    // `a` had text before it, which stays; `c` had white space only, which goes with it.
    assert!(after.contains(&format!("note  more {}</draw:page>", rect("b", 3.0, 1.0, 1.0, 1.0))), "{after}");
}

#[test]
fn a_shape_of_a_master_page_is_edited_in_styles_xml_and_content_xml_is_left_as_it_is() {
    let mut d = Drawing::a4().page(&rect("p", 1.0, 1.0, 1.0, 1.0));
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect draw:name="bg" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/><draw:rect draw:name="bg2" svg:x="5cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></style:master-page>"#,
    );
    let mut o = open(&d);
    let (at, id) = o
        .doc
        .pages
        .iter()
        .enumerate()
        .find_map(|(at, p)| p.shapes.iter().find(|s| s.name.as_deref() == Some("bg2")).map(|s| (at, s.id)))
        .expect("the master page's shape is in the model");
    move_by(&mut o.doc, at, id, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml"), "the pages' part is the file's own");
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(r#"draw:name="bg2" svg:x="6cm""#) && styles.contains(r#"draw:name="bg" svg:x="1cm""#), "{styles}");
    // A second save through the new index finds nothing to do, and a delete of the first master shape renumbers the second.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited);
    Arc::make_mut(o.doc.pages.get_mut(at).unwrap()).shapes.remove(0);
    let deleted = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    let styles = part(&deleted.bytes, "styles.xml");
    assert!(!styles.contains(r#"draw:name="bg" "#) && styles.contains(r#"draw:name="bg2" svg:x="6cm""#), "{styles}");
}

#[test]
fn an_ole_object_element_is_an_object_and_an_attribute_that_is_not_a_reference_keeps_nothing_alive() {
    // `draw:object-ole` holds the object as `draw:object` does; and a shape whose `draw:name` is the object's folder name does not name the object.
    let mut d = Drawing::a4().page(concat!(
        r#"<draw:frame draw:name="ole" svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object-ole xlink:href="./Object 1"/></draw:frame>"#,
        r#"<draw:rect draw:name="Object 1" svg:x="2cm" svg:y="8cm" svg:width="5cm" svg:height="3cm"/>"#,
    ));
    d.files.push(("Object 1/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
    let mut o = open(&d);
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!crate::write_core::has_part(&saved.bytes, "Object 1/content.xml"), "the object went with its frame");
    assert!(part(&saved.bytes, "content.xml").contains(r#"draw:name="Object 1""#), "the other shape is still there");
}

#[test]
fn a_prefix_rebound_inside_an_element_is_the_drawings_again_after_it() {
    // `draw` is another namespace inside the wrapper, and the drawing's again after it: the shape after the group is found and moved.
    let body = concat!(
        r#"<draw:g draw:name="g"><x:wrap xmlns:x="urn:x" xmlns:draw="urn:other"><draw:rect draw:name="inner" svg:width="1cm" svg:height="1cm"/></x:wrap></draw:g>"#,
        r#"<draw:rect draw:name="last" svg:x="2cm" svg:y="2cm" svg:width="1cm" svg:height="1cm"/>"#
    );
    let mut o = open(&Drawing::a4().page(body));
    let last = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some("last")).map(|s| s.id).unwrap();
    move_by(&mut o.doc, 0, last, 1.0, 0.0);
    assert!(saved_content(&o).contains(r#"draw:name="last" svg:x="3cm""#));
}

#[test]
fn a_new_thumbnail_is_deflated_and_a_replaced_one_keeps_its_method() {
    // An entry a save adds is deflated, and one it changes keeps the method it had. The test's builder stores a `.png`, so a thumbnail the
    // package has is stored, and stays so when it is replaced.
    let method = |bytes: &[u8]| {
        let archive = diacraft_container::zip::Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
        archive.entries().iter().find(|e| e.name == "Thumbnails/thumbnail.png").and_then(|e| e.compression())
    };
    let mut o = open(&drawing_of(2));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let added = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(method(&added.bytes), Some(diacraft_container::zip::Method::Deflated), "a new thumbnail");
    assert_eq!(part_bytes(&added.bytes, "Thumbnails/thumbnail.png"), PNG.to_vec());
    let mut d = drawing_of(2);
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), vec![0x89, b'P', b'N', b'G', 9, 9]));
    let mut o = open(&d);
    assert_eq!(method(&o.bytes), Some(diacraft_container::zip::Method::Stored));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let replaced = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(method(&replaced.bytes), Some(diacraft_container::zip::Method::Stored), "a replaced thumbnail keeps its method");
    assert_eq!(part_bytes(&replaced.bytes, "Thumbnails/thumbnail.png"), PNG.to_vec());
}

#[test]
fn a_meta_part_past_the_readers_cap_gets_the_generator_too() {
    // Another producer's generator must not stay in a file DiaCraft saved, however large `meta.xml` is. The reader leaves a `meta.xml` over 4 MB
    // unread; the writer scans it (linear, and charged to the bytes a save may scan) and writes its own generator.
    let mut d = drawing_of(2);
    let padding = "<meta:keyword>k</meta:keyword>".repeat(150_000);
    d.meta = Some(crate::write_core::full_meta().replace("</office:meta>", &format!("{padding}</office:meta>")));
    assert!(d.meta.as_ref().is_some_and(|m| m.len() > 4 * 1024 * 1024));
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    let meta = part(&saved.bytes, "meta.xml");
    assert!(
        meta.contains(&format!("<meta:generator>DiaCraft/{}</meta:generator>", env!("CARGO_PKG_VERSION"))),
        "{}",
        &meta[meta.len().saturating_sub(300)..]
    );
    assert!(!meta.contains("TestWriter/9$Linux"), "the other producer's generator is gone");
    assert!(meta.contains("<dc:title>A title</dc:title>"), "and the rest of the part is as it was");
}

#[test]
fn an_object_in_a_hyperlink_wrapper_in_a_deleted_group_goes_with_the_group() {
    // The wrapper and the frame in it are elements the reader keeps without a shape of the model, so the delete of the group does not name them;
    // they go with it, and the object the frame shows goes with them, where a shape nothing deleted still shows its own.
    let body = concat!(
        r#"<draw:g draw:name="g"><draw:a xlink:href="http://example.org"><draw:frame svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/></draw:frame></draw:a></draw:g>"#,
        r#"<draw:frame draw:name="other" svg:x="2cm" svg:y="8cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 2"/></draw:frame>"#
    );
    let mut d = Drawing::a4().page(body);
    for name in ["Object 1/content.xml", "Object 2/content.xml"] {
        d.files.push((name.to_owned(), b"<office:document-content/>".to_vec()));
    }
    let mut o = open(&d);
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!crate::write_core::has_part(&saved.bytes, "Object 1/content.xml"), "the object of the frame in the wrapper went with the group");
    assert!(crate::write_core::has_part(&saved.bytes, "Object 2/content.xml"), "the object another frame shows stays");
    assert!(!part(&saved.bytes, "META-INF/manifest.xml").contains("Object 1"), "and so did its row");
}

#[test]
fn a_read_leaves_its_cost_on_the_index_and_a_save_carries_it_on() {
    // A save holds the caller's model while it reads the file again, so it needs what the model weighs without reading the file to find
    // out. The reader puts the cost on the index it gives; a save gives the index of the file it made the cost of the read it compared with.
    let o = open(&drawing_of(40));
    let (_, report) = read(&o.bytes, &Limits::default()).unwrap();
    let cost = *o.index.read_cost();
    assert_eq!((cost.items, cost.media_bytes, cost.inflated_bytes), (report.work.items, report.work.media_bytes, report.work.package.inflated));
    assert!(cost.items > 40 && cost.tree_nodes > 0 && cost.inflated_bytes > 0, "{cost:?}");
    // 64 bytes an item, the reader's own figure (54 before).
    assert_eq!(cost.model_bytes(), cost.items * 64 + cost.media_bytes);
    assert_eq!(cost.transient_bytes(), cost.tree_nodes * 210 + cost.inflated_bytes);
    assert_eq!(cost.save_bytes(), 2 * cost.model_bytes() + cost.transient_bytes(), "the model twice and what a read holds beside one");
    let mut o = o;
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    // The file read again costs what the file did; the figures are those of the model saved (a move changed the text of two numbers).
    let carried = *first.index.read_cost();
    assert_eq!(diacraft_odg::ReadCost { figures: cost.figures, ..carried }, cost);
    assert_eq!((carried.figures.shapes, carried.figures.cells), (cost.figures.shapes, cost.figures.cells));
    assert!(carried.figures.text_bytes.abs_diff(cost.figures.text_bytes) < 20, "{carried:?} against {cost:?}");
    // An index nothing read has no cost, and a save does not read the file to find one: it is held to the reader's own caps.
    assert_eq!(diacraft_odg::ShapeIndex::default().read_cost(), &diacraft_odg::ReadCost::default());
}

#[test]
fn a_save_that_would_hold_more_memory_than_it_may_is_refused_before_the_file_is_read() {
    let mut o = open(&drawing_of(40));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let need = o.index.read_cost().save_bytes();
    let save = |memory: u64| {
        let mut work = diacraft_odg::WriteWork::default();
        let options = WriteOptions { limits: Limits { max_write_memory: memory, ..Limits::default() }, ..WriteOptions::default() };
        let saved = diacraft_odg::write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
        (saved, work)
    };
    // The least cap at which the save is let be (found by halving: a refusal for the memory read nothing, and the estimate does not go down as
    // the cap goes up). One byte under it is a refusal that names the memory and the cap, with nothing read (the baseline's cost is zero).
    let before_read = |memory: u64| {
        let (saved, work) = save(memory);
        let refused = matches!(&saved, Err(WriteError::Limit { what, max }) if what.contains("memory") && *max == memory);
        (refused && (work.baseline_read.items, work.baseline_read.elements, work.scan_bytes) == (0, 0, 0), saved.is_ok())
    };
    let (mut under, mut over) = (0u64, 3_000_000_000u64);
    assert!(before_read(under).0 && before_read(over).1, "a cap of nothing refuses and one of 3 GB lets a save of forty shapes be");
    while over - under > 1 {
        let mid = under + (over - under) / 2;
        if before_read(mid).0 { under = mid } else { over = mid }
    }
    let least = over;
    assert!(before_read(least).1, "a save at the least cap is made");
    assert!(before_read(least - 1).0, "and one byte under it is refused before the file is read");
    // The estimate holds what the reader's figures say a save needs, and the file and its biggest part besides.
    let (file, content) = (o.bytes.len() as u64, part(&o.bytes, "content.xml").len() as u64);
    assert!(least >= need + file + content, "{least} against {need}, the file of {file} bytes and the part of {content}");
    let (refused, work) = save(least - 1);
    match refused {
        Err(WriteError::Limit { what, max }) => assert!(what.contains("memory") && max == least - 1, "{what} {max}"),
        other => panic!("{:?}", other.map(|w| w.bytes.len())),
    }
    assert_eq!((work.baseline_read.items, work.baseline_read.elements, work.scan_bytes), (0, 0, 0), "refused before the file was read");
    // The cap that is the default stops a file of the cost of the reader's own every-budget input: its estimate is 6.1 GB (`write_memory.rs`, `every_budget_refused`:
    // 970 MB of pictures all read; it was 3.8 GB while the file held 720 MB of parts nothing names, which a read does not inflate).
    assert!(Limits::default().max_write_memory < 3_000_000_000);
}

#[test]
fn a_file_that_costs_more_than_its_index_said_is_refused_as_it_is_read() {
    // The index of a small drawing with the file of a large one: a caller's mistake, and a file that a hostile package could not make (the index is
    // the reader's), but a save does not take the index's word for what it is about to read: the read is held to the room the index leaves.
    let small = open(&drawing_of(3));
    let big = open(&drawing_of(4_000));
    let (items_small, items_big) = (small.index.read_cost().items, big.index.read_cost().items);
    assert!(items_big > 10 * items_small);
    // A cap that holds the small drawing's save and leaves less room than the big file's items.
    let memory = small.index.read_cost().save_bytes() + 54 * (items_big / 2);
    let options = WriteOptions { limits: Limits { max_write_memory: memory, ..Limits::default() }, ..WriteOptions::default() };
    let saved = write_with(&small.doc, Some(&OriginalPackage { bytes: &big.bytes, index: &small.index }), &options);
    match saved {
        Err(WriteError::Limit { what, .. }) => assert!(what.contains("memory"), "{what}"),
        other => panic!("{:?}", other.map(|w| w.bytes.len())),
    }
    // With the room for it, the same pair is refused as what it is: not the file the model came from.
    let roomy = WriteOptions::default();
    assert!(matches!(
        write_with(&small.doc, Some(&OriginalPackage { bytes: &big.bytes, index: &small.index }), &roomy),
        Err(WriteError::Mismatch(_) | WriteError::Unsupported(_))
    ));
}

#[test]
fn the_prefix_for_an_added_drawing_attribute_follows_the_declarations_in_force_on_each_shape() {
    // Two shapes the scan sees alike but for the least prefix bound to ODF's drawing namespace in force on them: the second binds `a` to it. A shape
    // turned has a `draw:transform` added, under the least prefix in force on its own tag.
    const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    let body = format!(
        r#"<draw:rect draw:name="p" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/><draw:rect xmlns:a="{DRAW}" draw:name="q" svg:x="1cm" svg:y="4cm" svg:width="2cm" svg:height="1cm"/>"#
    );
    let mut o = open(&Drawing::a4().page(&body));
    for id in [1, 2] {
        let shape = shape_mut(&mut o.doc, 0, id);
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), Cell::constant(Value::from_number(0.5).unwrap()));
    }
    let after = saved_content(&o);
    let tag_of =
        |name: &str| after.split(&format!("draw:name=\"{name}\"")).nth(1).and_then(|rest| rest.split("/>").next()).unwrap_or_default().to_owned();
    let (p, q) = (tag_of("p"), tag_of("q"));
    assert!(p.contains(" draw:transform=\"") && !p.contains(" a:transform="), "{after}");
    assert!(q.contains(" a:transform=\"") && !q.contains(" draw:transform="), "{after}");
}

#[test]
fn a_part_that_declares_more_namespaces_than_a_part_holds_is_refused_and_one_that_declares_fewer_is_not() {
    // The scan keeps what is in force from the declarations it meets, and refuses a part past 100,000 of them with a class (a real part declares a
    // few dozen); each declaration is counted as it is met.
    let save = |declarations: usize| {
        let mut d = Drawing::a4().page(&custom("s1", "1cm"));
        for k in 0..declarations {
            d.ns.push_str(&format!(" xmlns:j{k}=\"urn:j:{k}\""));
        }
        let mut o = open(&d);
        move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        real(&o.doc, &o.bytes, &o.index)
    };
    assert!(save(99_000).is_ok());
    match save(100_001) {
        Err(WriteError::Limit { what, max }) => assert!(what.contains("namespace declarations") && max == 100_000, "{what} {max}"),
        other => panic!("{:?}", other.map(|w| w.bytes.len())),
    }
}

#[test]
fn an_attribute_whose_value_is_no_length_is_replaced_in_its_place_and_never_added_a_second_time() {
    // Found by the fuzz target: `svg:y="8(cmm"` is no length, so the reader reads the shape as if it had none; a move that wrote the
    // attribute added another beside it, and a tag that names an attribute twice is not XML.
    let body = r#"<draw:custom-shape draw:name="c" svg:x="1cm" svg:y="8(cmm"/><draw:rect draw:name="r" svg:x="1cm" svg:y="3cm" svg:width="wide" svg:height="1cm"/>"#;
    let mut o = open(&Drawing::a4().page(body));
    move_by(&mut o.doc, 0, 1, 1.0, 0.5);
    {
        let shape = shape_mut(&mut o.doc, 0, 2);
        set_num(shape, "Width", cm(3.0));
        set_num(shape, "LocPinX", cm(1.5));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    for (name, attribute) in [("c", "svg:y="), ("c", "svg:x="), ("r", "svg:width="), ("r", "svg:x=")] {
        let tag = after.split(&format!("draw:name=\"{name}\"")).nth(1).and_then(|rest| rest.split("/>").next()).unwrap_or_default();
        assert_eq!(tag.matches(attribute).count(), 1, "{name}: {attribute} in {after}");
    }
    // Each in its place: the shape that had no width is 3 cm wide about its pin, which is its left edge, 1.5 cm in; and the shape that had no `svg:y`
    // (the reader reads 0) is moved 0.5 cm up.
    assert!(after.contains(r#"draw:name="r" svg:x="-0.5cm" svg:y="3cm" svg:width="3cm""#), "{after}");
    assert!(after.contains(r#"draw:name="c" svg:x="2cm" svg:y="-0.5cm""#), "{after}");
    assert!(open_bytes(saved.bytes).doc.pages.get(0).is_some(), "and the file reads");
}
