//! At the writer's level: a one-dimensional shape of a geometry of its own that the model adds to a page is written as the polyline it draws (a path when a row is a
//! curve), with its frame and its glue in DiaCraft's data, and the file reads back as the same segment; a polyline whose carried data is hostile is read as the polyline it is.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Connects, Document, Row, Section, Shape, Value};
use diacraft_odg::{Limits, read};

use crate::drawing::Drawing;
use crate::write_core::{Open, open_bytes, part, real};

/// A page with one rectangle, which the tests read.
fn opened() -> Open {
    let rect = r#"<draw:custom-shape draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    open_bytes(Drawing::a4().page(rect).bytes())
}

fn number(v: f64) -> Cell {
    Cell::constant(Value::from_number(v).unwrap()).with_unit("IN")
}

/// A segment from (`x1`, `y1`) to (`x2`, `y2`) inches whose geometry in its own frame is a vee, with a cubic curve for its second row when `curved`.
fn segment(id: u32, (x1, y1): (f64, f64), (x2, y2): (f64, f64), curved: bool) -> Shape {
    let length = (x2 - x1).hypot(y2 - y1);
    let mut shape = Shape::new(id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, v) in
        [("BeginX", x1), ("BeginY", y1), ("EndX", x2), ("EndY", y2), ("Width", length), ("Height", 0.0), ("LocPinX", length / 2.0), ("LocPinY", 0.0)]
    {
        sheet.cells.insert(CellName::new(name), number(v));
    }
    sheet.cells.insert(CellName::new("PinX"), number((x1 + x2) / 2.0));
    sheet.cells.insert(CellName::new("PinY"), number((y1 + y2) / 2.0));
    sheet.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_number((y2 - y1).atan2(x2 - x1)).unwrap()));
    let mut geometry = Section::new("Geometry", Some(0));
    geometry.cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_number(1.0).unwrap()));
    let rows = [("MoveTo", 0.0, 0.0), (if curved { "NURBSTo" } else { "LineTo" }, length / 2.0, 0.2), ("LineTo", length, 0.0)];
    for (i, (kind, x, y)) in rows.into_iter().enumerate() {
        let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(i as u32 + 1), ..Row::default() };
        row.cells.insert(CellName::new("X"), number(x));
        row.cells.insert(CellName::new("Y"), number(y));
        if curved && i == 1 {
            let text = format!("NURBS(1,3,1,1,{},0.2,0,1,{},0.2,0,1)", length / 3.0, length * 2.0 / 3.0);
            row.cells.insert(CellName::new("E"), Cell::constant(Value::from_text(&text)));
        }
        geometry.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(geometry));
    shape
}

/// The attribute of the first element that starts with `open` in `text`.
fn attr(text: &str, open: &str, name: &str) -> Option<String> {
    let tag = text.get(text.find(open)?..)?.split('>').next()?;
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag.get(at..)?.split('"').next()?.to_owned())
}

fn add(open: &mut Open, shape: Shape) {
    Arc::make_mut(open.doc.pages.get_mut(0).unwrap()).shapes.push(Arc::new(shape));
}

fn the_segment(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 7).expect("the segment")
}

/// A segment of lines is a `draw:polyline`, and one with a cubic row a `draw:path`; each reads back as the segment: its ends, its width, its geometry's rows of the model's types.
#[test]
fn a_segment_of_lines_is_a_polyline_and_one_with_a_curve_is_a_path_and_each_reads_back() {
    for curved in [false, true] {
        let mut open = opened();
        add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), curved));
        let saved = real(&open.doc, &open.bytes, &open.index).expect("the save");
        let content = part(&saved.bytes, "content.xml");
        let (element, data) = if curved { ("<draw:path ", "svg:d") } else { ("<draw:polyline ", "draw:points") };
        assert!(content.contains(element) && !content.contains("<draw:line "), "curved {curved}: {content}");
        let text = attr(&content, element, data).expect("the data");
        assert_eq!(text.contains(" C "), curved, "{text}");
        assert!(attr(&content, element, "svg:viewBox").is_some_and(|v| v.starts_with("0 0 ")), "a view box of the box");
        let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let segment = the_segment(&back);
        let at = |name: &str| segment.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
        assert!((at("BeginX") - 1.0).abs() < 1e-6 && (at("EndY") - 2.5).abs() < 1e-6, "its ends");
        assert!((at("Width") - 2.0f64.hypot(0.5)).abs() < 1e-6, "its width is the distance between its ends");
        let rows: Vec<String> = segment
            .sheet
            .sections
            .iter()
            .filter(|s| s.name.as_str() == "Geometry")
            .flat_map(|s| s.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()))
            .collect();
        assert_eq!(rows, ["MoveTo", if curved { "NURBSTo" } else { "LineTo" }, "LineTo"], "the model's rows, not the polyline's");
        // The same model saved through the index the save gave is no edit.
        let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index).expect("a second save");
        assert!(!again.edited, "curved {curved}: an unedited save is no edit");
        assert_eq!(again.bytes, saved.bytes);
    }
}

/// A segment added to a flat drawing (`.fodg`) is a polyline in the flat file, which stays flat, and reads back as the segment.
#[test]
fn a_segment_added_to_a_flat_drawing_is_a_polyline_in_the_flat_file() {
    let rect = r#"<draw:custom-shape draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let mut open = open_bytes(crate::flat::Flat::of(&Drawing::a4().page(rect)).bytes());
    add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), false));
    let saved = real(&open.doc, &open.bytes, &open.index).expect("the save");
    let text = String::from_utf8(saved.bytes.clone()).expect("a flat file is text");
    assert!(text.contains("<office:document ") && text.contains("<draw:polyline ") && !text.contains("<draw:line "), "{text}");
    let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(the_segment(&back).sheet.cell("BeginX").is_some(), "the segment's frame came back");
    let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index).expect("a second save");
    assert!(!again.edited && again.bytes == saved.bytes, "an unedited save is no edit");
}

/// The glue of a begin or an end of a segment to the first row of a shape.
fn glue(from: u32, to: u32, begin: bool) -> Connect {
    Connect {
        from_sheet: from,
        from_cell: Some(Arc::from(if begin { "BeginX" } else { "EndX" })),
        from_part: Some(if begin { 9 } else { 12 }),
        to_sheet: to,
        to_cell: Some(Arc::from("Connections.X1")),
        to_part: Some(100),
        ..Connect::default()
    }
}

/// The rows of the first `Geometry` section of a shape, as the numbers of their `X` and `Y` cells.
fn rows_of(shape: &Shape) -> Vec<(f64, f64)> {
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry").expect("a geometry");
    let at = |row: &Row, name: &str| row.cells.get(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    section.rows.iter().map(|r| (at(r, "X"), at(r, "Y"))).collect()
}

/// A segment of a geometry of its own that is **glued** (to a shape that is new in the save or to one the file holds, at both ends or at one) is a `draw:connector` glued to its shapes by
/// `draw:start-shape` and `draw:end-shape` (a polyline has no glue another program keeps, so a Comm-link glued to two rectangles stayed where it was when one of them was
/// moved in LibreOffice). Its type is `line`: LibreOffice routes a connector by its type and keeps no path of the file's (`svg:d`), so it draws the straight line between the glue points, the line
/// the zigzag runs along. DiaCraft's data carries the zigzag, so a read gives the model's own rows back, exactly; and the polyline's glue attribute is not written.
#[test]
fn a_glued_segment_is_a_connector_of_the_type_line_glued_to_its_shapes_and_reads_back_with_its_own_rows() {
    // (the ends glued, the shapes glued to are new in the save, the route cells are the fixed geometry's: a Comm-link's)
    for (ends, new_shapes, fixed) in
        [([true, true], true, true), ([true, true], false, false), ([true, false], true, true), ([false, true], false, false)]
    {
        let what = format!(
            "ends {ends:?}, glued to {} shapes, route cells {}",
            if new_shapes { "new" } else { "the file's" },
            if fixed { "16 and 1" } else { "none" }
        );
        let mut open = opened();
        let file_id = open.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let (first, second) = if new_shapes {
            let first = Arc::clone(open.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
            for id in [5, 6] {
                let mut copy = (*first).clone();
                copy.id = id;
                add(&mut open, copy);
            }
            (5, 6)
        } else {
            (file_id, file_id)
        };
        let mut figure = segment(7, (1.0, 2.0), (3.0, 2.5), false);
        if fixed {
            let sheet = Arc::make_mut(&mut figure.sheet);
            sheet.cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(Value::from_number(16.0).unwrap()));
            sheet.cells.insert(CellName::new("ConLineRouteExt"), Cell::constant(Value::from_number(1.0).unwrap()));
        }
        add(&mut open, figure);
        let rows = vec![(ends[0], glue(7, first, true)), (ends[1], glue(7, second, false))];
        let page = Arc::make_mut(open.doc.pages.get_mut(0).unwrap());
        page.connects = Connects::from_vec(rows.into_iter().filter(|(on, _)| *on).map(|(_, c)| c).collect());
        let saved = real(&open.doc, &open.bytes, &open.index).unwrap_or_else(|e| panic!("{what}: the save: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(
            content.contains("<draw:connector ") && !content.contains("<draw:polyline ") && !content.contains("<draw:line "),
            "{what}: {content}"
        );
        assert!(!content.contains("diacraft:connects="), "{what}: the glue is the element's, not DiaCraft's data: {content}");
        assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{what}");
        assert_eq!(attr(&content, "<draw:connector ", "draw:start-shape").is_some(), ends[0], "{what}: the begin's shape");
        assert_eq!(attr(&content, "<draw:connector ", "draw:end-shape").is_some(), ends[1], "{what}: the end's shape");
        assert_eq!(attr(&content, "<draw:connector ", "draw:start-glue-point").is_some(), ends[0], "{what}: the begin's glue point");
        assert_eq!(attr(&content, "<draw:connector ", "draw:end-glue-point").is_some(), ends[1], "{what}: the end's glue point");
        // The path of the file's own form (an absolute move and relative steps): the vee's three points, the middle one 0.2 inch (508 hundredths of a millimetre) to the side.
        let d = attr(&content, "<draw:connector ", "svg:d").expect("the path");
        assert!(d.starts_with("M ") && d.matches('l').count() == 2, "{what}: the vee's two legs: {d}");
        let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
        assert!(report.warnings.is_empty(), "{what}: {:?}", report.warnings);
        let connects: Vec<(u32, u32, bool)> =
            back.pages.get(0).unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet, c.from_cell.as_deref() == Some("BeginX"))).collect();
        let wanted: Vec<(u32, u32, bool)> =
            [(ends[0], first, true), (ends[1], second, false)].into_iter().filter(|(on, _, _)| *on).map(|(_, to, begin)| (7, to, begin)).collect();
        assert_eq!(connects, wanted, "{what}: the connections of the page");
        let (made, read_back) = (the_segment(&open.doc), the_segment(&back));
        assert_eq!(rows_of(made), rows_of(read_back), "{what}: the zigzag's own rows, exactly, not the rounded path's");
        let at = |s: &Shape, name: &str| s.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
        if fixed {
            // The type `line` makes the reader's route cells 2 and 1: the figure's own (16 and 1, which a reroute leaves alone) are carried over them.
            assert_eq!((at(read_back, "ShapeRouteStyle"), at(read_back, "ConLineRouteExt")), (16.0, 1.0), "{what}: the route cells are the model's");
        }
        // The same model saved through the index the save gave is no edit.
        let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index)
            .unwrap_or_else(|e| panic!("{what}: a second save: {e}"));
        assert!(!again.edited, "{what}: an unedited save is no edit");
        assert_eq!(again.bytes, saved.bytes, "{what}");
    }
}

/// A segment of a geometry of its own that is glued at no end stays the polyline it draws, with no glue of any kind.
#[test]
fn a_segment_glued_at_no_end_stays_a_polyline() {
    let mut open = opened();
    add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), false));
    let saved = real(&open.doc, &open.bytes, &open.index).expect("the save");
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("<draw:polyline ") && !content.contains("<draw:connector "), "{content}");
    assert!(!content.contains("diacraft:connects="), "{content}");
}

/// What the writer does not say as a polyline is written as it was: a section that fills, one that is not shown, two sections, an arc row, and every point on a level are a line.
#[test]
fn a_geometry_the_writer_cannot_say_as_a_polyline_is_a_line_as_it_was() {
    type Change = Box<dyn Fn(&mut Shape)>;
    let cases: Vec<(&str, Change)> = vec![
        ("a section that fills", Box::new(|s| set_flag(s, "NoFill", 0.0))),
        ("a section that is not shown", Box::new(|s| set_flag(s, "NoShow", 1.0))),
        ("an arc row", Box::new(|s| set_row_type(s, 1, "ArcTo"))),
        (
            "two sections",
            Box::new(|s| {
                let again = Section::new("Geometry", Some(1));
                Arc::make_mut(&mut s.sheet).sections.push(Arc::new(again));
            }),
        ),
        ("every point on a level", Box::new(flatten)),
    ];
    for (what, change) in cases {
        let mut open = opened();
        let mut shape = segment(7, (1.0, 2.0), (3.0, 2.0), false);
        change(&mut shape);
        add(&mut open, shape);
        let saved = real(&open.doc, &open.bytes, &open.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(content.contains("<draw:line ") && !content.contains("<draw:polyline "), "{what}: {content}");
    }
}

fn geometry_mut(shape: &mut Shape) -> &mut Section {
    let sheet = Arc::make_mut(&mut shape.sheet);
    let at = sheet.sections.iter().position(|s| s.name.as_str() == "Geometry").expect("a geometry");
    Arc::make_mut(&mut sheet.sections[at])
}

fn set_flag(shape: &mut Shape, name: &str, v: f64) {
    geometry_mut(shape).cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap()));
}

fn set_row_type(shape: &mut Shape, row: usize, kind: &str) {
    let section = geometry_mut(shape);
    let mut changed = (*section.rows[row]).clone();
    changed.row_type = Some(Arc::from(kind));
    section.rows[row] = Arc::new(changed);
}

/// The vee made level: each row's y is 0.
fn flatten(shape: &mut Shape) {
    let section = geometry_mut(shape);
    for row in section.rows.iter_mut() {
        let mut changed = (**row).clone();
        changed.cells.insert(CellName::new("Y"), number(0.0));
        *row = Arc::new(changed);
    }
}

/// A polyline whose DiaCraft data carries glue is read as what it can be, with no panic and a note: glue that names a shape the page lacks is left out, a token of glue that is none is left
/// out, more than two are not read, and a frame no box comes of makes the polyline the markup says. **The writer no longer writes this glue** (a glued segment is a connector),
/// but a file an earlier build wrote holds it, so the reader still takes it, and the test puts it into the style of an unglued polyline by hand.
#[test]
fn a_polyline_whose_carried_glue_and_frame_are_hostile_is_read_as_what_it_can_be() {
    let mut open = opened();
    let first = Arc::clone(open.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
    let mut copy = (*first).clone();
    copy.id = 5;
    add(&mut open, copy);
    add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), false));
    let saved = real(&open.doc, &open.bytes, &open.index).expect("the save");
    let content = part(&saved.bytes, "content.xml");
    assert!(!content.contains("diacraft:connects="), "an unglued polyline carries no glue: {content}");
    // The glue an earlier build wrote, put on the segment's own style.
    let own = "b5:100:9:Connections_2EX1";
    let style = attr(&content, "<draw:polyline ", "draw:style-name").expect("the polyline's style");
    let from = content.find(&format!("<style:style style:name=\"{style}\"")).expect("the style");
    let props = from + content.get(from..).and_then(|rest| rest.find("<style:graphic-properties")).expect("its graphic properties");
    let at = props + "<style:graphic-properties".len();
    let content = format!("{} {GLUE}=\"{own}\"{}", &content[..at], &content[at..]);
    let read_with = |glue: &str| {
        let hostile = content.replace(&format!("{GLUE}=\"{own}\""), &format!("{GLUE}=\"{glue}\""));
        let package = crate::build::package(crate::build::DRAWING, rebuilt_files(&saved.bytes, &hostile));
        read(&package, &Limits::default()).expect("the file reads")
    };
    // The glue as it was written is read: the segment is glued to the copy at its begin.
    let (back, _report) = read_with(own);
    let connects: Vec<(u32, u32)> = back.pages.get(0).unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect();
    assert_eq!(connects, [(7, 5)], "the glue an earlier build wrote is still read");
    // A shape the page lacks, a token that is none, a second token for an end, a crowd of tokens: no connection of the page dangles, and no panic.
    for glue in [
        "b999:100:9:Connections_2EX1",
        "zz",
        "b5:1:1",
        &"b5:100:9:Connections_2EX1 ".repeat(5_000),
        "b5:100:9:Connections_2EX1 b6:1:1:A e5:1:1:B e5:1:1:C",
    ] {
        let (back, _report) = read_with(glue);
        let page = back.pages.get(0).unwrap();
        for connect in page.connects.iter() {
            assert!(page.shapes.iter().any(|s| s.id == connect.to_sheet), "{glue:.40}: a connection that dangles: {connect:?}");
        }
        assert!(page.connects.len() <= 2, "{glue:.40}: at most one for each end");
    }
    let (back, report) = read_with("b999:100:9:Connections_2EX1");
    assert!(back.pages.get(0).unwrap().connects.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("glue")), "the glue that names no shape is said: {:?}", report.warnings);
}

/// A page with one rectangle in a part that declares `version`, which the tests read.
fn opened_in(version: &str) -> Open {
    let rect = r#"<draw:custom-shape draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let mut drawing = Drawing::a4().page(rect);
    drawing.version = version.to_owned();
    open_bytes(drawing.bytes())
}

/// How often `needle` stands in `text`.
fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

/// How many `svg:d` attributes the `draw:connector` start tags of `content` hold (the class F1 of the fuzz target: the schemas of ODF 1.0 and 1.1 have none on that element, while a
/// `draw:path`'s is ODF 1.0's own markup).
fn connector_routes(content: &str) -> usize {
    content.split("<draw:connector ").skip(1).map(|rest| count(rest.split('>').next().unwrap_or_default(), " svg:d=\"")).sum()
}

/// **A curved new segment is a `draw:path` with its `svg:d` in every version of ODF**: the schemas of 1.0 and 1.1 give `draw:path` the one attribute `svg:d` (required) and give
/// `draw:connector` none (class F1 is about connectors), so a part of 1.0, 1.1 or no version gets the path as one of 1.2 or 1.3 does, with no `xml:id` (which those versions do
/// not have) and no `svg:d` on any connector; it reads back with its own rows, the cubic among them, and a second save is no edit and the same bytes. With the fixed route cells too (the
/// writer's choice of the path over a connector does not depend on the version).
#[test]
fn a_curved_segment_added_to_a_part_of_any_version_is_a_path_with_one_svg_d_and_reads_back_curved() {
    for version in ["1.0", "1.1", "", "1.2", "1.3"] {
        let legacy = matches!(version, "1.0" | "1.1" | "");
        for fixed in [false, true] {
            let what = format!("version {version:?}, route cells {}", if fixed { "16 and 1" } else { "none" });
            let mut open = opened_in(version);
            let mut figure = segment(7, (1.0, 2.0), (3.0, 2.5), true);
            if fixed {
                let sheet = Arc::make_mut(&mut figure.sheet);
                sheet.cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(Value::from_number(16.0).unwrap()));
                sheet.cells.insert(CellName::new("ConLineRouteExt"), Cell::constant(Value::from_number(1.0).unwrap()));
            }
            add(&mut open, figure);
            let saved = real(&open.doc, &open.bytes, &open.index).unwrap_or_else(|e| panic!("{what}: the save: {e}"));
            let content = part(&saved.bytes, "content.xml");
            assert!(
                content.contains("<draw:path ")
                    && !content.contains("<draw:polyline ")
                    && !content.contains("<draw:line ")
                    && !content.contains("<draw:connector "),
                "{what}: {content}"
            );
            assert_eq!(count(&content, " svg:d=\""), 1, "{what}: the path's one svg:d: {content}");
            assert_eq!(connector_routes(&content), 0, "{what}: no svg:d on a connector: {content}");
            if legacy {
                assert_eq!(count(&content, " xml:id=\""), 0, "{what}: no xml:id in a part of ODF 1.0 or 1.1: {content}");
            }
            let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
            assert!(report.warnings.is_empty(), "{what}: {:?}", report.warnings);
            assert_eq!(report.office_version.as_deref().unwrap_or(""), version, "{what}: the version is the file's");
            let kinds: Vec<String> = the_segment(&back)
                .sheet
                .sections
                .iter()
                .filter(|s| s.name.as_str() == "Geometry")
                .flat_map(|s| s.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()))
                .collect();
            assert_eq!(kinds, ["MoveTo", "NURBSTo", "LineTo"], "{what}: the model's rows, the curve and all");
            assert_eq!(rows_of(the_segment(&open.doc)), rows_of(the_segment(&back)), "{what}: its own rows, exactly");
            let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index).expect("a second save");
            assert!(!again.edited && again.bytes == saved.bytes, "{what}: an unedited save is no edit, and the same bytes");
        }
    }
    // A segment of lines is a polyline in every version, with no `svg:d`.
    for version in ["1.0", "1.1", "1.2", "1.3"] {
        let mut open = opened_in(version);
        add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), false));
        let content = part(&real(&open.doc, &open.bytes, &open.index).expect("the save").bytes, "content.xml");
        assert!(content.contains("<draw:polyline ") && count(&content, " svg:d=\"") == 0, "{version}: {content}");
    }
}

/// The vector of the fuzz target's release-gate crash (`3a 3d 33 00 8f ff`, b8bc0aa0): a part of ODF 1.1 with the segment of op 51 (which 0, amount 143: five points, the second a cubic row,
/// glued at no end) on its first page. The target's check counted the path's `svg:d` as a fault of the writer, where ODF 1.0 has it on `draw:path`; the check now counts it on connectors alone.
/// The model is built as the target builds it (`segment_new`) on a page of one rectangle: the save is the path, with one `svg:d`, none on a connector and no `xml:id`, and reads back as the model.
#[test]
fn the_fuzz_targets_crash_vector_is_a_path_in_a_part_of_odf_1_1_with_no_svg_d_on_a_connector() {
    let mut open = opened_in("1.1");
    segment_new(&mut open.doc, (51, 0, 0x8f), 2);
    let saved = real(&open.doc, &open.bytes, &open.index).expect("the save");
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("<draw:path ") && !content.contains("<draw:connector "), "{content}");
    assert_eq!((count(&content, " svg:d=\""), connector_routes(&content), count(&content, " xml:id=\"")), (1, 0, 0), "{content}");
    let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
    assert_eq!(report.office_version.as_deref(), Some("1.1"), "the version is the file's");
    let made = open.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 2).expect("the segment of the vector");
    let read_back = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 2).expect("the segment read back");
    assert_eq!(rows_of(made), rows_of(read_back), "its own rows, the curve among them");
    let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index).expect("a second save");
    assert!(!again.edited && again.bytes == saved.bytes, "an unedited save is no edit, and the same bytes");
}

/// A glued segment in a part of ODF 1.1 is still a `draw:connector` of the type `line` glued to its shapes, and has neither `svg:d` nor `svg:viewBox` there (the
/// route is its ends, and the schemas of ODF 1.0 and 1.1 have neither on that element); the zigzag, and the curve, ride in DiaCraft's data, so a read gives the model's rows back exactly.
#[test]
fn a_glued_segment_in_a_part_of_odf_1_1_is_a_connector_with_no_route_and_reads_back_with_its_own_rows() {
    for curved in [false, true] {
        let mut open = opened_in("1.1");
        let file_id = open.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        add(&mut open, segment(7, (1.0, 2.0), (3.0, 2.5), curved));
        let page = Arc::make_mut(open.doc.pages.get_mut(0).unwrap());
        page.connects = Connects::from_vec(vec![glue(7, file_id, true)]);
        let saved = real(&open.doc, &open.bytes, &open.index).unwrap_or_else(|e| panic!("curved {curved}: the save: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(
            content.contains("<draw:connector ") && !content.contains("<draw:polyline ") && !content.contains("<draw:path "),
            "curved {curved}: {content}"
        );
        assert_eq!(count(&content, " svg:d=\""), 0, "curved {curved}: no route as a path: {content}");
        assert_eq!(connector_routes(&content), 0, "curved {curved}: and none on the connector: {content}");
        assert_eq!(count(&content, " xml:id=\""), 0, "curved {curved}: and no xml:id: {content}");
        assert!(attr(&content, "<draw:connector ", "svg:viewBox").is_none(), "curved {curved}: nor a view box on the connector: {content}");
        assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "curved {curved}");
        assert!(attr(&content, "<draw:connector ", "draw:start-shape").is_some(), "curved {curved}: glued to its shape");
        let (back, report) = read(&saved.bytes, &Limits::default()).expect("the file reads");
        assert!(report.warnings.is_empty(), "curved {curved}: {:?}", report.warnings);
        assert_eq!(rows_of(the_segment(&open.doc)), rows_of(the_segment(&back)), "curved {curved}: its own rows, exactly");
        let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index).expect("a second save");
        assert!(!again.edited && again.bytes == saved.bytes, "curved {curved}: an unedited save is no edit");
    }
}

/// The sweep of the fuzz target's segments again, in a part of ODF 1.0 and in one of 1.1: no save gains an `xml:id` or an `svg:d` on a connector (the target's class F1: the schemas of those
/// versions have no `svg:d` on `draw:connector`, and a `draw:path` has one), reads back, and saved again through the index of the save is no edit.
#[test]
fn a_sweep_of_the_fuzz_targets_segments_in_a_part_of_odf_1_0_or_1_1_gains_no_xml_id_and_no_svg_d_on_a_connector() {
    let (mut written, mut paths, mut connectors) = (0usize, 0usize, 0usize);
    for version in ["1.0", "1.1"] {
        for which in (0..=255u8).step_by(9) {
            for amount in (0..=255u8).step_by(4) {
                let mut open = opened_in(version);
                let was = part(&open.bytes, "content.xml");
                segment_new(&mut open.doc, (51, which, amount), 2);
                match real(&open.doc, &open.bytes, &open.index) {
                    Ok(saved) => {
                        written += 1;
                        let content = part(&saved.bytes, "content.xml");
                        paths += usize::from(content.contains("<draw:path "));
                        connectors += usize::from(content.contains("<draw:connector "));
                        assert!(
                            count(&content, " xml:id=\"") <= count(&was, " xml:id=\""),
                            "{version} {which} {amount}: a part of ODF 1.0 or 1.1 gained xml:id: {content}"
                        );
                        assert!(
                            connector_routes(&content) <= connector_routes(&was),
                            "{version} {which} {amount}: a part of ODF 1.0 or 1.1 gained an svg:d on a connector: {content}"
                        );
                        let (back, _report) = read(&saved.bytes, &Limits::default())
                            .unwrap_or_else(|e| panic!("{version} {which} {amount}: the file did not read: {e}"));
                        let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index)
                            .unwrap_or_else(|e| panic!("{version} {which} {amount}: a second save: {e}"));
                        assert!(!again.edited, "{version} {which} {amount}: a second save of the model read back is no edit");
                    }
                    Err(e) => assert!(matches!(e.class(), "write_unsupported" | "write_limit"), "{version} {which} {amount}: {e} ({})", e.class()),
                }
            }
        }
    }
    assert!(written > 1_200, "{written} saves were made");
    assert!(paths > 200 && connectors > 200, "the sweep reaches paths ({paths}) and connectors ({connectors})");
}

/// The attribute DiaCraft's data carries the glue of a polyline's ends in (an earlier build wrote it; the reader still takes it).
const GLUE: &str = "diacraft:connects";

/// The files of `bytes` with `content.xml` replaced by `content`: what a package of the same parts with another body is made of.
fn rebuilt_files(bytes: &[u8], content: &str) -> Vec<(String, Vec<u8>)> {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in package.parts().filter(|n| *n != "mimetype" && *n != "META-INF/manifest.xml") {
        let data = if name == "content.xml" { content.as_bytes().to_vec() } else { package.part(name).unwrap().to_vec() };
        files.push((name.to_owned(), data));
    }
    files
}

/// The writer's fuzz target puts a segment of a geometry of its own on the page (its op 51, `segment_new`, copied here by hand): a sweep of its bytes, each model saved, read back, and saved
/// again through the index of the save, which is no edit; a refusal is in words. A save that the writer's own check refused, or that did not read again, is a crash.
#[test]
fn a_sweep_of_the_fuzz_targets_segments_is_saved_read_back_and_saved_again_as_no_edit() {
    let mut written = 0usize;
    for which in (0..=255u8).step_by(9) {
        for amount in (0..=255u8).step_by(4) {
            let mut open = opened();
            segment_new(&mut open.doc, (51, which, amount), 2);
            match real(&open.doc, &open.bytes, &open.index) {
                Ok(saved) => {
                    written += 1;
                    let (back, _report) =
                        read(&saved.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{which} {amount}: the file did not read: {e}"));
                    let again = real(&back, &saved.bytes, &read(&saved.bytes, &Limits::default()).unwrap().1.index)
                        .unwrap_or_else(|e| panic!("{which} {amount}: a second save: {e}"));
                    assert!(!again.edited, "{which} {amount}: a second save of the model read back is no edit");
                }
                Err(e) => assert!(matches!(e.class(), "write_unsupported" | "write_limit"), "{which} {amount}: {e} ({})", e.class()),
            }
        }
    }
    assert!(written > 600, "{written} saves were made");
}

/// The fuzz target's op 51.
fn segment_new(doc: &mut Document, (_, which, amount): (u8, u8, u8), next_id: u32) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let next_id = next_id.max(page.shapes.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1));
    let number = |v: f64| Value::from_number(v).unwrap_or_else(|| Value::from_text("0"));
    let (x1, y1) = (1.0 + f64::from(which % 8) / 2.0, 1.0 + f64::from(which / 8 % 8) / 2.0);
    let (x2, y2) = (x1 + f64::from(amount % 16) / 4.0, y1 + f64::from(amount / 16 % 4) / 2.0 - 0.5);
    let length = (x2 - x1).hypot(y2 - y1);
    let mut shape = Shape::new(next_id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, v) in
        [("BeginX", x1), ("BeginY", y1), ("EndX", x2), ("EndY", y2), ("Width", length), ("Height", 0.0), ("LocPinX", length / 2.0), ("LocPinY", 0.0)]
    {
        sheet.cells.insert(CellName::new(name), Cell::constant(number(v)).with_unit("IN"));
    }
    sheet.cells.insert(CellName::new("PinX"), Cell::constant(number((x1 + x2) / 2.0)).with_unit("IN"));
    sheet.cells.insert(CellName::new("PinY"), Cell::constant(number((y1 + y2) / 2.0)).with_unit("IN"));
    sheet.cells.insert(CellName::new("Angle"), Cell::constant(number((y2 - y1).atan2(x2 - x1))));
    if which & 0x80 != 0 {
        sheet.cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(number(16.0)));
        sheet.cells.insert(CellName::new("ConLineRouteExt"), Cell::constant(number(1.0)));
    }
    let mut geometry = Section::new("Geometry", Some(0));
    geometry.cells.insert(CellName::new("NoFill"), Cell::constant(number(if which & 0x40 != 0 { 0.0 } else { 1.0 })));
    if amount & 0x40 != 0 {
        geometry.cells.insert(CellName::new("NoLine"), Cell::constant(number(f64::from(amount & 1))));
    }
    let points = 2 + usize::from(amount % 5);
    for k in 0..points {
        let x = if k + 1 == points { length } else { length * k as f64 / (points - 1) as f64 };
        let y = if k == 0 || k + 1 == points {
            0.0
        } else if k % 2 == 1 {
            0.1
        } else {
            -0.1
        };
        let kind = match (k, amount & 0x80 != 0) {
            (0, _) => "MoveTo",
            (1, true) => "NURBSTo",
            _ => "LineTo",
        };
        let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(k as u32 + 1), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::constant(number(x)).with_unit("IN"));
        row.cells.insert(CellName::new("Y"), Cell::constant(number(y)).with_unit("IN"));
        if kind == "NURBSTo" {
            let (a, b) = (length / 3.0, length * 2.0 / 3.0);
            let text = format!("NURBS(1,3,1,1,{a},{y},0,1,{b},{y},0,1)");
            row.cells.insert(CellName::new("E"), Cell::constant(Value::from_text(&text)));
        }
        geometry.rows.push(Arc::new(row));
    }
    sheet.sections.push(Arc::new(geometry));
    let target = page.shapes.get(0).map(|s| s.id).filter(|_| amount & 0x20 != 0);
    page.shapes.push(Arc::new(shape));
    if let Some(target) = target {
        let mut rows: Vec<Connect> = page.connects.iter().cloned().collect();
        rows.push(Connect {
            from_sheet: next_id,
            from_cell: Some(Arc::from("BeginX")),
            from_part: Some(9),
            to_sheet: target,
            to_cell: Some(Arc::from("Connections.X1")),
            to_part: Some(100),
            ..Connect::default()
        });
        page.connects = Connects::from_vec(rows);
    }
}
