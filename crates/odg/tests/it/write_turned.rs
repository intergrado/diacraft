//! Item 3: a new shape that is turned or flipped is written, where it was
//! refused ("a new shape that is turned or flipped (written with a later task)").
//!
//! A turned shape is `svg:width`, `svg:height` and `draw:transform="rotate (<radians>) translate (<x> <y>)"` with no `svg:x` or `svg:y`, as LibreOffice writes it; a
//! flipped one has `draw:mirror-horizontal` or `draw:mirror-vertical` on its `draw:enhanced-geometry`. The reader gives both back as `Angle` and the flips, so a shape
//! that is turned and flipped about its centre needs no tier 2.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Value};
use diacraft_odg::WriteError;

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, custom, open, part, real};
use crate::write_new::{push_shape, rectangle};

fn page() -> Open {
    open(&Drawing::a4().page(&custom("s1", "1cm")))
}

fn put(shape: &mut Shape, name: &str, value: f64, unit: Option<&str>) {
    let mut c = Cell::constant(Value::from_text(&value.to_string()));
    c.unit = unit.map(Arc::from);
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), c);
}

fn number(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// Where the shape's frame is on the page: the image of its centre (inches, y up) and the linear part of its map, from its cells, as `own::local_to_parent`
/// reads them (a flipped axis is a factor of -1, then the turn, then the pin less the pin's place in the frame).
fn box_of(shape: &Shape) -> ((f64, f64), [f64; 4]) {
    let n = |name: &str| number(shape, name);
    let (s, c) = n("Angle").sin_cos();
    let flip = |name: &str| if n(name) == 1.0 { -1.0 } else { 1.0 };
    let (fx, fy) = (flip("FlipX"), flip("FlipY"));
    let (a, b, cc, d) = (c * fx, s * fx, -s * fy, c * fy);
    let (cx, cy) = (n("Width") / 2.0 - n("LocPinX"), n("Height") / 2.0 - n("LocPinY"));
    ((n("PinX") + a * cx + cc * cy, n("PinY") + b * cx + d * cy), [a, b, cc, d])
}

fn assert_same_box(model: &Shape, read: &Shape, what: &str) {
    let ((mx, my), ml) = box_of(model);
    let ((rx, ry), rl) = box_of(read);
    assert!(
        (mx - rx).abs() < 1e-9 && (my - ry).abs() < 1e-9,
        "{what}: the centre of the frame is ({mx}, {my}) in the model and ({rx}, {ry}) in the file"
    );
    // The linear part is the turn after the flips: two maps that are the same up to a whole turn of the angle are the same map.
    for (i, (a, b)) in ml.iter().zip(rl.iter()).enumerate() {
        assert!((a - b).abs() < 1e-9, "{what}: element {i} of the linear part is {a} in the model and {b} in the file");
    }
    assert!(
        (number(model, "Width") - number(read, "Width")).abs() < 1e-9 && (number(model, "Height") - number(read, "Height")).abs() < 1e-9,
        "{what}: size"
    );
}

/// The new shape's element in `content.xml`.
fn element(content: &str) -> String {
    let start = content.rfind("<draw:custom-shape ").expect("a custom shape");
    let end = content[start..].find("</draw:custom-shape>").unwrap();
    content[start..start + end].to_owned()
}

/// The shape a case writes, saved, and what the file holds of it.
struct Case {
    saved: diacraft_odg::Written,
    open: Open,
    shape: Shape,
}

fn save(mut shape: Shape, set: impl Fn(&mut Shape)) -> Case {
    let mut o = page();
    set(&mut shape);
    push_shape(&mut o.doc, shape.clone());
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a turned or flipped new shape is saved: {e}"));
    Case { saved, open: o, shape }
}

fn read_shape(case: &Case) -> (Shape, diacraft_odg::ReadReport) {
    let (doc, report) = diacraft_odg::read(&case.saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    (doc.pages.get(0).unwrap().shapes.get(1).expect("the new shape reads back").as_ref().clone(), report)
}

fn rectangle_4_by_2() -> Shape {
    rectangle(2, "Turned", (2.0, 3.0, 4.0, 2.0))
}

/// A shape 4 cm by 2 cm turned 30 degrees whose pin (the frame's centre) is at (7.232050807568877 cm, 19.833974596215562 cm from the bottom, so 9.866 cm from the
/// top) is `svg:width="4cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (5cm 10cm)"` and no `svg:x` or `svg:y`: the numbers of the vector, in
/// the form the spec gives. Read back it has the same angle and pin and no tier 2, and a second save is no edit.
#[test]
fn a_new_shape_turned_30_degrees_is_written_as_rotate_then_translate_and_reads_back_with_no_tier_2() {
    let case = save(rectangle_4_by_2(), |s| {
        put(s, "PinX", cm(7.232050807568877), Some("IN"));
        put(s, "PinY", cm(19.833974596215562), Some("IN"));
        put(s, "Angle", 0.5235987755982988, Some("RAD"));
    });
    let content = part(&case.saved.bytes, "content.xml");
    let shape = element(&content);
    assert!(shape.contains(r#"svg:width="4cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (5cm 10cm)""#), "{shape}");
    assert!(!shape.contains("svg:x=") && !shape.contains("svg:y="), "a turned shape has no svg:x or svg:y: {shape}");
    assert!(!content.contains(" diacraft:"), "no tier 2: the turn is what the reader gives back: {content}");
    let (read, report) = read_shape(&case);
    assert_eq!(report.warnings, Vec::<String>::new());
    assert!((number(&read, "Angle") - 0.5235987755982988).abs() < 1e-9, "Angle {}", number(&read, "Angle"));
    assert!((number(&read, "PinX") - cm(7.232050807568877)).abs() < 1e-9 && (number(&read, "PinY") - cm(19.833974596215562)).abs() < 1e-9);
    assert_same_box(&case.shape, &read, "V5");
    let again = real(&case.open.doc, &case.saved.bytes, &case.saved.index).unwrap();
    assert!(!again.edited && again.bytes == case.saved.bytes, "a second save is no edit");
}

/// Each angle round the circle, and a pin that is not the frame's centre, read back with the frame where the model has it: the shape draws where it did. An angle
/// past a half turn is written as the turn it is the other way round and its cell is carried (tier 2), so the model reads back as it was.
#[test]
fn a_turned_shape_is_where_the_model_has_it_for_any_angle_and_any_pin() {
    let pi = std::f64::consts::PI;
    for angle in [0.0, 0.1, pi / 4.0, pi / 2.0, 3.0 * pi / 4.0, pi - 1e-3, -pi / 3.0, -pi / 2.0, 3.0 * pi / 2.0, 2.5 * pi, -2.0 * pi + 0.2] {
        for pin_at_centre in [true, false] {
            let case = save(rectangle_4_by_2(), |s| {
                put(s, "Angle", angle, Some("RAD"));
                if !pin_at_centre {
                    // The pin is a corner of the frame, as a shape given a pin of its own has it.
                    let sheet = Arc::make_mut(&mut s.sheet);
                    sheet.cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("0")).with_unit("IN"));
                    sheet.cells.insert(CellName::new("LocPinY"), Cell::constant(Value::from_text("0")).with_unit("IN"));
                }
            });
            let (read, report) = read_shape(&case);
            assert_eq!(report.warnings, Vec::<String>::new(), "angle {angle} pin at centre {pin_at_centre}");
            assert_same_box(&case.shape, &read, &format!("angle {angle} pin at centre {pin_at_centre}"));
            let again = real(&case.open.doc, &case.saved.bytes, &case.saved.index).unwrap();
            assert!(!again.edited, "angle {angle} pin at centre {pin_at_centre}: a second save is no edit");
        }
    }
}

/// A mirror is `draw:mirror-horizontal` for `FlipX` and `draw:mirror-vertical` for `FlipY` on the shape's `draw:enhanced-geometry`, and the reader gives the flips
/// back with no tier 2; turned and flipped together, the file says both and the shape is where the model has it. The frame is where the flip about the pin puts it.
#[test]
fn a_flipped_new_shape_has_the_mirrors_of_the_geometry_and_reads_back_with_no_tier_2() {
    let pi = std::f64::consts::PI;
    for (flip_x, flip_y, angle) in
        [(1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 1.0, 0.0), (1.0, 0.0, pi / 6.0), (0.0, 1.0, pi / 2.0), (1.0, 1.0, -pi / 4.0)]
    {
        let case = save(rectangle_4_by_2(), |s| {
            put(s, "FlipX", flip_x, None);
            put(s, "FlipY", flip_y, None);
            put(s, "Angle", angle, Some("RAD"));
        });
        let content = part(&case.saved.bytes, "content.xml");
        let shape = element(&content);
        let what = format!("flips {flip_x} {flip_y} angle {angle}");
        assert_eq!(shape.contains(r#"draw:mirror-horizontal="true""#), flip_x == 1.0, "{what}: {shape}");
        assert_eq!(shape.contains(r#"draw:mirror-vertical="true""#), flip_y == 1.0, "{what}: {shape}");
        assert_eq!(shape.contains("draw:transform="), angle != 0.0, "{what}: {shape}");
        assert_eq!(shape.contains("svg:x="), angle == 0.0, "{what}: {shape}");
        assert!(!content.contains(" diacraft:"), "{what}: no tier 2: {content}");
        let (read, report) = read_shape(&case);
        assert_eq!(report.warnings, Vec::<String>::new(), "{what}");
        assert_eq!((number(&read, "FlipX"), number(&read, "FlipY")), (flip_x, flip_y), "{what}");
        assert_same_box(&case.shape, &read, &what);
        assert!(!real(&case.open.doc, &case.saved.bytes, &case.saved.index).unwrap().edited, "{what}: a second save is no edit");
    }
}

/// A flipped shape whose pin is not its frame's centre: the file has the shape's box where the flip about the pin puts it, with the mirror, and the pin, the pin's place
/// in the frame and the flips are what the reader gives back or are carried, so the model reads back as it was.
#[test]
fn a_flipped_and_turned_shape_with_a_pin_of_its_own_draws_where_the_model_has_it() {
    let case = save(rectangle_4_by_2(), |s| {
        put(s, "FlipX", 1.0, None);
        put(s, "Angle", 0.7, Some("RAD"));
        let sheet = Arc::make_mut(&mut s.sheet);
        sheet.cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("0.5")).with_unit("IN"));
        sheet.cells.insert(CellName::new("LocPinY"), Cell::constant(Value::from_text("1.25")).with_unit("IN"));
    });
    let (read, _) = read_shape(&case);
    assert_same_box(&case.shape, &read, "a flipped, turned shape with a pin of its own");
    // The pin's place in the frame is the shape's own and is carried: the file reads it back as the model has it.
    assert_eq!((number(&read, "LocPinX"), number(&read, "LocPinY")), (0.5, 1.25));
    assert!(!real(&case.open.doc, &case.saved.bytes, &case.saved.index).unwrap().edited);
}

/// A shape that holds a turn and a flip is no refusal of any kind now, and a frame that is not numbers still is, in words.
#[test]
fn a_frame_that_is_not_numbers_is_still_refused_in_words() {
    let mut o = page();
    let mut shape = rectangle_4_by_2();
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), Cell::constant(Value::from_text("not a number")));
    push_shape(&mut o.doc, shape);
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("frame is not numbers"), "{words}"),
        other => panic!("{other:?}"),
    }
}
