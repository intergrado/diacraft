//! A custom shape that is **mirrored on one axis and turned**, in the file and in the model.
//!
//! `draw:mirror-horizontal` or `draw:mirror-vertical` on a `draw:enhanced-geometry`, with a `draw:transform` that turns the frame, say two things to a reader, and the
//! standard (ODF 1.3 Part 3 §19.192, §19.193, §19.228) fixes neither the order nor the direction: it says the geometry is mirrored and the shape is turned. LibreOffice 26.8.2.2,
//! a black box (own profile, a right triangle in a 3 cm by 1.5 cm frame, the five angles 0, 30, 90, 135 and 210 degrees, each with no mirror, a horizontal one, a vertical
//! one and both), draws `rotate (a)` after a single mirror as the shape turned by `a` **and then** mirrored across the vertical (or horizontal) line through the frame's
//! centre: the picture DiaCraft's model has for a turn of `-a` after the mirror in the frame. With both mirrors the order does not matter (a half turn commutes with every
//! turn), and with none there is no mirror. The shapes LibreOffice itself wrote for a user's mirror and turn (through its API: `MirroredX`, then a `RotateAngle` of `a` degrees
//! counterclockwise) hold `rotate (-a)` beside `draw:mirror-horizontal`, so the pair means what LibreOffice reads it as, in the files it writes as in the files it reads.
//!
//! So the model's `Angle` of a shape with exactly one mirror is **minus** the angle of the file's `rotate`: the reader negates it, and the writer writes the negation. The
//! tests keep the file's side to an oracle of its own, [`file_draws`], written from the measurement above and not from the reader or the writer.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Shape};

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, custom, num_of, open, part, real, set_num};
use crate::write_new::{push_shape, rectangle};

const PAGE_HEIGHT_IN: f64 = 29.7 / 2.54;

fn cell(formula: Option<&str>, value: f64, unit: Option<&str>) -> Cell {
    Cell::read(formula, Some(&value.to_string()), unit, None, &mut 1_000)
}

fn row(kind: &str, ix: u32, x: (Option<&str>, f64), y: (Option<&str>, f64)) -> Arc<Row> {
    let mut r = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
    r.cells.insert(CellName::new("X"), cell(x.0, x.1, Some("IN")));
    r.cells.insert(CellName::new("Y"), cell(y.0, y.1, Some("IN")));
    Arc::new(r)
}

/// A right triangle, its right angle at the lower left of its frame (`w` by `h` centimetres, the frame's top left at (`x`, `y`) from the page's top left): no shape that
/// is symmetric shows a mirror beside a turn.
fn triangle(id: u32, (x, y, w, h): (f64, f64, f64, f64)) -> Shape {
    let mut shape = rectangle(id, "Tri", (x, y, w, h));
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.sections.retain(|s| s.name.as_str() != "Geometry");
    let (w_in, h_in) = (cm(w), cm(h));
    let mut geometry = Section::new("Geometry", Some(0));
    geometry.rows.push(row("MoveTo", 1, (None, 0.0), (None, 0.0)));
    geometry.rows.push(row("LineTo", 2, (Some("Width"), w_in), (None, 0.0)));
    geometry.rows.push(row("LineTo", 3, (None, 0.0), (Some("Height"), h_in)));
    geometry.rows.push(row("LineTo", 4, (None, 0.0), (None, 0.0)));
    sheet.sections.push(Arc::new(geometry));
    shape
}

fn put_num(shape: &mut Shape, name: &str, value: f64, unit: Option<&str>) {
    let mut c = Cell::constant(diacraft_doc::Value::from_text(&value.to_string()));
    c.unit = unit.map(Arc::from);
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), c);
}

/// The three corners of the triangle as the model has them on the page: inches, y down. A local point (`u`, `v`) of the frame, y up, goes to the page by the flips about the
/// pin, the turn counterclockwise, and the pin.
fn model_draws(shape: &Shape) -> [(f64, f64); 3] {
    let n = |name: &str| num_of(shape, name);
    let (s, c) = n("Angle").sin_cos();
    let flip = |name: &str| if n(name) == 1.0 { -1.0 } else { 1.0 };
    let (fx, fy) = (flip("FlipX"), flip("FlipY"));
    let at = |u: f64, v: f64| {
        let (du, dv) = ((u - n("LocPinX")) * fx, (v - n("LocPinY")) * fy);
        (n("PinX") + c * du - s * dv, PAGE_HEIGHT_IN - (n("PinY") + s * du + c * dv))
    };
    [at(0.0, 0.0), at(n("Width"), 0.0), at(0.0, n("Height"))]
}

fn attr(element: &str, name: &str) -> Option<String> {
    let at = element.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(element[at..at + element[at..].find('"')?].to_owned())
}

fn inches(text: &str) -> f64 {
    let (n, unit) = text.split_at(text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len()));
    let n: f64 = n.trim().parse().unwrap_or_else(|_| panic!("a length: {text}"));
    n / match unit {
        "cm" => 2.54,
        "mm" => 25.4,
        "in" => 1.0,
        other => panic!("a unit of the test's files: {other}"),
    }
}

/// What LibreOffice 26.8.2.2 draws of a custom shape's element (see the module's words), for the triangle written as the right triangle in the frame: the frame's corners
/// are taken from `svg:x`, `svg:y` or from `draw:transform="rotate (<a>) translate (<x> <y>)"` (the frame's point (`u`, `v`), y down, goes to (`u` cos a + `v` sin a, `v` cos a - `u` sin a)
/// beside the translation), and **a single mirror is applied to the picture of the turned frame**, across the vertical (`draw:mirror-horizontal`) or the horizontal
/// (`draw:mirror-vertical`) line through its centre. Inches, y down.
fn file_draws(element: &str) -> [(f64, f64); 3] {
    let (w, h) = (inches(&attr(element, "svg:width").expect("a width")), inches(&attr(element, "svg:height").expect("a height")));
    let (angle, (tx, ty)) = match attr(element, "draw:transform") {
        Some(t) => {
            let rotate: f64 =
                t.split("rotate (").nth(1).and_then(|r| r.split(')').next()).and_then(|a| a.trim().parse().ok()).expect("a rotate term");
            let translate = t.split("translate (").nth(1).and_then(|r| r.split(')').next()).expect("a translate term");
            let mut parts = translate.split_whitespace();
            (rotate, (inches(parts.next().expect("an x")), inches(parts.next().expect("a y"))))
        }
        None => (0.0, (inches(&attr(element, "svg:x").expect("an x")), inches(&attr(element, "svg:y").expect("a y")))),
    };
    let (s, c) = angle.sin_cos();
    let frame = |u: f64, v: f64| (tx + u * c + v * s, ty - u * s + v * c);
    let centre = frame(w / 2.0, h / 2.0);
    let mirror_x = element.contains(r#"draw:mirror-horizontal="true""#);
    let mirror_y = element.contains(r#"draw:mirror-vertical="true""#);
    // The triangle in the frame, y down: its right angle at the lower left.
    [(0.0, h), (w, h), (0.0, 0.0)].map(|(u, v)| {
        let (mut x, mut y) = frame(u, v);
        if mirror_x {
            x = 2.0 * centre.0 - x;
        }
        if mirror_y {
            y = 2.0 * centre.1 - y;
        }
        (x, y)
    })
}

#[track_caller]
fn assert_same_drawing(a: [(f64, f64); 3], b: [(f64, f64); 3], what: &str) {
    for (k, (p, q)) in a.iter().zip(&b).enumerate() {
        assert!((p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4, "{what}: corner {k} is {p:?} in one and {q:?} in the other");
    }
}

fn element(content: &str) -> String {
    let start = content.rfind("<draw:custom-shape ").expect("a custom shape");
    let end = content[start..].find("</draw:custom-shape>").expect("its end");
    content[start..start + end].to_owned()
}

fn page() -> Open {
    open(&Drawing::a4().page(&custom("s1", "1cm")))
}

/// The turns of the matrix, degrees: nothing, a third of a quarter, a quarter, past an eighth over a quarter, past a half, and two the other way round.
const ANGLES: [f64; 7] = [0.0, 30.0, 90.0, 135.0, 210.0, -30.0, -135.0];
/// The mirrors of the matrix: (`FlipX`, `FlipY`).
const FLIPS: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];

fn flipped_and_turned(flip: (f64, f64), degrees: f64) -> Shape {
    let mut shape = triangle(2, (3.0, 5.0, 4.0, 2.0));
    put_num(&mut shape, "FlipX", flip.0, None);
    put_num(&mut shape, "FlipY", flip.1, None);
    put_num(&mut shape, "Angle", degrees.to_radians(), Some("RAD"));
    shape
}

fn saved(shape: Shape) -> (Open, diacraft_odg::Written, Shape) {
    let mut o = page();
    push_shape(&mut o.doc, shape.clone());
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("saved: {e}"));
    (o, written, shape)
}

/// **The file draws what the session has.** A new shape with one mirror and a turn is written so that LibreOffice (the oracle of this file, [`file_draws`]) draws the corners
/// the session has: the turn the file holds is minus the model's, beside a single mirror. Every flip at every angle of the matrix, the pins at the frame's centre.
#[test]
fn a_new_shape_mirrored_on_one_axis_and_turned_is_written_so_that_libreoffice_draws_it_where_the_session_has_it() {
    for flip in FLIPS {
        for degrees in ANGLES {
            let what = format!("flips {flip:?} turned {degrees} degrees");
            let (_, written, shape) = saved(flipped_and_turned(flip, degrees));
            let content = part(&written.bytes, "content.xml");
            assert_same_drawing(model_draws(&shape), file_draws(&element(&content)), &what);
        }
    }
}

/// **DiaCraft reads its own file as the session has it**, each case of the matrix: the pin, the turn and the flips come back, or a pose that draws the same, with no tier 2 and
/// no warning, and a second save is no edit.
#[test]
fn a_new_shape_mirrored_and_turned_reads_back_as_the_session_has_it_and_saves_again_as_no_edit() {
    for flip in FLIPS {
        for degrees in ANGLES {
            let what = format!("flips {flip:?} turned {degrees} degrees");
            let (o, written, shape) = saved(flipped_and_turned(flip, degrees));
            let content = part(&written.bytes, "content.xml");
            // A turn past a half turn is written as the turn it is the other way round, and the cell that says it is carried (`write_turned.rs`): no tier 2 inside the half turns.
            assert_eq!(
                content.contains(" diacraft:"),
                degrees.abs() > 180.0,
                "{what}: tier 2 is carried for a turn past a half turn and for no other: {content}"
            );
            let (doc, report) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
            assert_eq!(report.warnings, Vec::<String>::new(), "{what}");
            let read = doc.pages.get(0).unwrap().shapes.get(1).expect("the new shape reads back").as_ref().clone();
            for name in ["PinX", "PinY", "Width", "Height"] {
                assert!((num_of(&shape, name) - num_of(&read, name)).abs() < 1e-9, "{what}: {name}");
            }
            assert_eq!((num_of(&read, "FlipX"), num_of(&read, "FlipY")), flip, "{what}: the flips");
            let turn = (num_of(&shape, "Angle") - num_of(&read, "Angle")).rem_euclid(std::f64::consts::TAU);
            assert!(
                turn < 1e-9 || std::f64::consts::TAU - turn < 1e-9,
                "{what}: the turn is {} in the model and {} in the file",
                num_of(&shape, "Angle"),
                num_of(&read, "Angle")
            );
            assert_same_drawing(model_draws(&shape), model_draws(&read), &what);
            let again = real(&o.doc, &written.bytes, &written.index).unwrap();
            assert!(!again.edited && again.bytes == written.bytes, "{what}: a second save is no edit");
        }
    }
}

/// An existing drawing of one right triangle of the page, as LibreOffice 26.8.2.2 wrote it for a user's mirror and turn (the pairs of its output for `MirroredX` then a
/// turn of 30 degrees counterclockwise, and for `MirroredY` then 30; the translations of its output; hand-written here, not copied from a file). A 3 cm by 1.5 cm frame
/// whose centre is at (5.9 cm, 6.3 cm) from the page's top left.
fn libreoffice_wrote(mirror: &str, rotate: &str, translate: &str) -> Open {
    open(&Drawing::a4().page(&format!(
        "<draw:custom-shape draw:name=\"t\" svg:width=\"3cm\" svg:height=\"1.5cm\" draw:transform=\"rotate ({rotate}) translate ({translate})\">\
         <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 21600 L 21600 21600 L 0 0 Z N\"{mirror}/>\
         </draw:custom-shape>"
    )))
}

const MIRROR_H: &str = r#" draw:mirror-horizontal="true""#;
const MIRROR_V: &str = r#" draw:mirror-vertical="true""#;

fn first(o: &Open) -> Shape {
    o.doc.pages.get(0).unwrap().shapes.get(0).expect("a shape").as_ref().clone()
}

/// **DiaCraft reads what LibreOffice wrote.** The shape a user mirrored horizontally and turned 30 degrees counterclockwise is `draw:mirror-horizontal` with `rotate (-0.523598775598299)`
/// in LibreOffice's file; the model has `FlipX` and an `Angle` of +30 degrees: the picture the user saw, which is the model's flip in the frame and then its turn.
#[test]
fn a_shape_libreoffice_wrote_with_one_mirror_and_a_turn_reads_as_the_turn_the_user_made() {
    let o = libreoffice_wrote(MIRROR_H, "-0.523598775598299", "4.976cm 4.9cm");
    let shape = first(&o);
    assert_eq!((num_of(&shape, "FlipX"), num_of(&shape, "FlipY")), (1.0, 0.0));
    assert!((num_of(&shape, "Angle") - 30.0_f64.to_radians()).abs() < 1e-9, "Angle {}", num_of(&shape, "Angle"));
    // The pin is the frame's centre, where the file puts it, and the corners are the file's.
    assert!((num_of(&shape, "PinX") - cm(5.9)).abs() < 1e-3 && (num_of(&shape, "PinY") - (PAGE_HEIGHT_IN - cm(6.3))).abs() < 1e-3);
    let element = "<draw:custom-shape svg:width=\"3cm\" svg:height=\"1.5cm\" draw:transform=\"rotate (-0.523598775598299) translate (4.976cm 4.9cm)\" draw:mirror-horizontal=\"true\"";
    assert_same_drawing(model_draws(&shape), file_draws(element), "mirror horizontal, file turn -30");
}

/// The same for the vertical mirror: the shape LibreOffice wrote for `MirroredY` and 30 degrees is `draw:mirror-vertical` with `rotate (2.61799387799149)` (a turn of 150 degrees), and the
/// picture is the one the horizontal mirror gives for 30 degrees (LibreOffice drew the two alike): `FlipY` 1 and an `Angle` of -150 degrees draw it.
#[test]
fn a_shape_libreoffice_wrote_with_a_vertical_mirror_and_a_turn_reads_as_the_picture_it_draws() {
    let v = libreoffice_wrote(MIRROR_V, "2.61799387799149", "6.824cm 11.998cm");
    let shape = first(&v);
    assert_eq!((num_of(&shape, "FlipX"), num_of(&shape, "FlipY")), (0.0, 1.0));
    assert!((num_of(&shape, "Angle") + 150.0_f64.to_radians()).abs() < 1e-9, "Angle {}", num_of(&shape, "Angle"));
    // The corners LibreOffice drew for it are those of the horizontal mirror at 30 degrees about the same centre (the two shapes of its file sit in different places: compare
    // them about their centres).
    let h = first(&libreoffice_wrote(MIRROR_H, "-0.523598775598299", "4.976cm 4.9cm"));
    let about = |s: &Shape| {
        let corners = model_draws(s);
        let c = (num_of(s, "PinX"), PAGE_HEIGHT_IN - num_of(s, "PinY"));
        corners.map(|p| (p.0 - c.0, p.1 - c.1))
    };
    assert_same_drawing(about(&shape), about(&h), "the vertical mirror at a file turn of 150 degrees is the horizontal mirror at -30");
}

/// Both mirrors, and none, are read as they were: the turn is the file's.
#[test]
fn a_shape_with_both_mirrors_or_none_keeps_the_turn_of_the_file() {
    for (mirror, flips) in [("", (0.0, 0.0)), ("both", (1.0, 1.0))] {
        let mirror = if mirror == "both" { format!("{MIRROR_H}{MIRROR_V}") } else { String::new() };
        let shape = first(&libreoffice_wrote(&mirror, "-0.523598775598299", "4.976cm 4.9cm"));
        assert_eq!((num_of(&shape, "FlipX"), num_of(&shape, "FlipY")), flips);
        assert!((num_of(&shape, "Angle") + 30.0_f64.to_radians()).abs() < 1e-9, "Angle {}", num_of(&shape, "Angle"));
    }
}

/// **A turn edited on a shape that is mirrored and turned** is written as the file's own convention: the shape the user turned from 30 to 60 degrees has `rotate (-1.0471975511965976)`
/// beside its mirror, and LibreOffice draws the corners the session has. The shape's frame is where it was.
#[test]
fn turning_a_shape_libreoffice_wrote_with_one_mirror_writes_the_turn_beside_the_mirror_as_libreoffice_reads_it() {
    for (mirror, rotate, translate) in [(MIRROR_H, "-0.523598775598299", "4.976cm 4.9cm"), (MIRROR_V, "2.61799387799149", "6.824cm 11.998cm")] {
        for degrees in [60.0_f64, 0.0, -45.0, 170.0] {
            let o = libreoffice_wrote(mirror, rotate, translate);
            let mut doc = o.doc.clone();
            let id = first(&o).id;
            set_num(crate::write_core::shape_mut(&mut doc, 0, id), "Angle", degrees.to_radians());
            let written = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{mirror} {degrees}: saved: {e}"));
            let content = part(&written.bytes, "content.xml");
            let edited = doc.pages.get(0).unwrap().shapes.get(0).unwrap().as_ref().clone();
            assert_same_drawing(model_draws(&edited), file_draws(&element(&content)), &format!("{mirror} turned to {degrees}"));
            // The shape still has its mirror, and reads back with the turn it was given.
            let (back, report) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
            assert_eq!(report.warnings, Vec::<String>::new());
            let back = back.pages.get(0).unwrap().shapes.get(0).unwrap().as_ref().clone();
            assert_eq!((num_of(&back, "FlipX"), num_of(&back, "FlipY")), (num_of(&edited, "FlipX"), num_of(&edited, "FlipY")));
            let turn = (num_of(&edited, "Angle") - num_of(&back, "Angle")).rem_euclid(std::f64::consts::TAU);
            assert!(turn < 1e-9 || std::f64::consts::TAU - turn < 1e-9, "{mirror} turned to {degrees}: read back as {}", num_of(&back, "Angle"));
        }
    }
}

/// **A shape no edit touches keeps its bytes.** The pair LibreOffice wrote is as it was after the page's other shape moved, and after the shape itself moved the rotate term is
/// kept as written: only the translation changes.
#[test]
fn a_shape_with_one_mirror_and_a_turn_keeps_its_bytes_when_it_is_not_turned() {
    let o = libreoffice_wrote(MIRROR_H, "-0.523598775598299", "4.976cm 4.9cm");
    // Untouched: a save with nothing changed is the file.
    let again = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!again.edited && again.bytes == o.bytes);
    // Moved 1 cm right and 0.5 cm up: the rotate term and the mirror are as they were, the translation moved with the shape.
    let mut doc = o.doc.clone();
    let id = first(&o).id;
    crate::write_core::move_by(&mut doc, 0, id, 1.0, 0.5);
    let written = real(&doc, &o.bytes, &o.index).unwrap();
    let element = element(&part(&written.bytes, "content.xml"));
    assert!(element.contains(r#"draw:transform="rotate (-0.523598775598299) translate (5.976cm 4.4cm)""#), "{element}");
    assert!(element.contains(MIRROR_H), "{element}");
}
