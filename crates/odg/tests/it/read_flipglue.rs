//! What the reader makes of **mirrored and flipped shapes**, against what LibreOffice 26.8.2.2 draws of the same markup (a black box: hand-written flat
//! drawings, converted by LibreOffice with its own profile to SVG and PDF, each shape's outline, connector end and text run measured; LibreOffice was never given a file
//! of anyone else's). Each oracle below is written from that measurement, not from the reader:
//!
//! - **The picture** of a custom shape is its frame's whole `draw:transform` (whatever its determinant) and then each mirror across the page's vertical
//!   (`draw:mirror-horizontal`) or horizontal (`draw:mirror-vertical`) line through the frame's centre ([`picture`]).
//! - **The four standard glue points** are the middles of the sides of the frame as drawn, numbered 0 top, 1 right, 2 bottom, 3 left on the drawn frame **unmirrored**:
//!   the picture's images of the frame's side middles, with 1 and 3 exchanged when the picture flips (custom shapes, rectangles, ellipses, frames; [`standard_point`]).
//! - **A `draw:glue-point` given as lengths** is in the page's axes, from the box the shape takes up on the page (its turned frame's, or a whole ellipse's own),
//!   neither turned nor mirrored ([`element_point`]).
//! - **The text** turns with the drawn frame, half a turn more when the shape is mirrored vertically or its transform flips it vertically, plus
//!   `draw:text-rotate-angle` counterclockwise ([`TEXT_TURNS`] holds the numbers measured beside the transforms that flip).
//!
//! The model's side is worked out from its cells: a local point (y up) goes to the page by the flips about the local pin, the turn and the pin, and a block's
//! text turns by `Angle` + `TxtAngle` with no flip or two and `Angle` − `TxtAngle` with one.

use diacraft_doc::{Document, Shape};

use crate::drawing::{Drawing, page};

const PAGE_H: f64 = 29.7 / 2.54;
const W: f64 = 3.0 / 2.54;
const H: f64 = 1.5 / 2.54;
const MIRRORS: [&str; 4] = ["", "h", "v", "hv"];

/// A linear map of the page (y down): `x' = a x + c y`, `y' = b x + d y`.
type Lin = [f64; 4];

fn mul(m: Lin, n: Lin) -> Lin {
    [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3]]
}

fn apply(m: Lin, p: (f64, f64)) -> (f64, f64) {
    (m[0] * p.0 + m[2] * p.1, m[1] * p.0 + m[3] * p.1)
}

/// ODF `rotate (a)`: (u, v) to (u cos a + v sin a, -u sin a + v cos a).
fn rotate(a: f64) -> Lin {
    let (s, c) = a.sin_cos();
    [c, -s, s, c]
}

/// A frame of `W` by `H` whose transform is `prefix` (its text and its map), then `rotate (deg)`, then the translation that puts the frame's centre at `centre` (inches, y down).
struct Frame {
    lin: Lin,
    t: (f64, f64),
    centre: (f64, f64),
    text: String,
}

fn frame(prefix: (&str, Lin), deg: f64, centre: (f64, f64)) -> Frame {
    let a = deg.to_radians();
    let lin = mul(rotate(a), prefix.1);
    let q = apply(lin, (W / 2.0, H / 2.0));
    let t = (centre.0 - q.0, centre.1 - q.1);
    let text = format!("{}rotate ({a:?}) translate ({:.6}cm {:.6}cm)", prefix.0, t.0 * 2.54, t.1 * 2.54);
    Frame { lin, t, centre, text }
}

const NO_PREFIX: (&str, Lin) = ("", [1.0, 0.0, 0.0, 1.0]);
const SCALE_Y: (&str, Lin) = ("scale (1 -1) ", [1.0, 0.0, 0.0, -1.0]);
const SCALE_X: (&str, Lin) = ("scale (-1 1) ", [-1.0, 0.0, 0.0, 1.0]);

/// A reflection across a line at 30 degrees, as a `matrix`: a transform that flips the frame and turns it.
fn reflect60() -> (&'static str, Lin) {
    let (s, c) = 60f64.to_radians().sin_cos();
    (Box::leak(format!("matrix ({c:?} {s:?} {s:?} {:?} 0cm 0cm) ", -c).into_boxed_str()), [c, s, s, -c])
}

fn mirror_attrs(m: &str) -> String {
    let mut out = String::new();
    if m.contains('h') {
        out.push_str(r#" draw:mirror-horizontal="true""#);
    }
    if m.contains('v') {
        out.push_str(r#" draw:mirror-vertical="true""#);
    }
    out
}

/// A right triangle (its right angle at the lower left of the frame), with `inner` inside the shape and `extra` on its geometry.
fn triangle(name: &str, f: &Frame, m: &str, inner: &str, extra: &str) -> String {
    format!(
        r#"<draw:custom-shape draw:id="{name}" xml:id="{name}" draw:name="{name}" svg:width="3cm" svg:height="1.5cm" draw:transform="{}">{inner}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 0 21600 L 21600 21600 Z N"{}{extra}/></draw:custom-shape>"#,
        f.text,
        mirror_attrs(m)
    )
}

/// A straight connector from a free point to glue point `id` of `target`, whose own end is written wrong on purpose (LibreOffice ends it at the point).
fn connector(name: &str, target: &str, id: u32, from: (f64, f64)) -> String {
    format!(
        r#"<draw:connector draw:name="{name}" draw:type="line" svg:x1="{0:.4}cm" svg:y1="{1:.4}cm" svg:x2="{0:.4}cm" svg:y2="{1:.4}cm" draw:end-shape="{target}" draw:end-glue-point="{id}"><text:p/></draw:connector>"#,
        from.0 * 2.54,
        from.1 * 2.54
    )
}

/// The picture LibreOffice draws of the frame point (`u`, `v`) (inches from the frame's top left, y down) of a shape mirrored `m`: the transform, then the mirrors across the
/// page's lines through the frame's centre. Inches, y down.
fn picture(f: &Frame, m: &str, (u, v): (f64, f64)) -> (f64, f64) {
    let p = apply(f.lin, (u, v));
    let (x, y) = (p.0 + f.t.0, p.1 + f.t.1);
    (if m.contains('h') { 2.0 * f.centre.0 - x } else { x }, if m.contains('v') { 2.0 * f.centre.1 - y } else { y })
}

/// Where LibreOffice ends a connector glued to standard point `id` (0 top, 1 right, 2 bottom, 3 left): the picture of the frame's side middle, the two sides exchanged
/// when the picture flips. Inches, y down.
fn standard_point(f: &Frame, m: &str, id: u32) -> (f64, f64) {
    let det = f.lin[0] * f.lin[3] - f.lin[1] * f.lin[2];
    let mirrors = usize::from(m.contains('h')) + usize::from(m.contains('v'));
    let flips = (det < 0.0) != (mirrors == 1);
    let id = match id {
        1 | 3 if flips => 4 - id,
        n => n,
    };
    let side = [(W / 2.0, 0.0), (W, H / 2.0), (W / 2.0, H), (0.0, H / 2.0)][id as usize];
    picture(f, m, side)
}

/// The box a shape takes up on the page: its turned frame's, or the whole ellipse's own.
fn box_of(f: &Frame, ellipse: bool) -> (f64, f64) {
    let (a, b, c, d) = (f.lin[0], f.lin[1], f.lin[2], f.lin[3]);
    if ellipse { ((W * a).hypot(H * c), (W * b).hypot(H * d)) } else { (W * a.abs() + H * c.abs(), W * b.abs() + H * d.abs()) }
}

/// Where LibreOffice ends a connector glued to a `draw:glue-point`: `share` is LibreOffice's own form (lengths with no `draw:align`, hundredths of a millimetre standing
/// for hundredths of a percent of the box from its centre), `top_left` a length from the box's top left corner (`draw:align="top-left"`). Inches, y down.
fn element_point(f: &Frame, ellipse: bool, share: Option<(f64, f64)>, top_left: Option<(f64, f64)>) -> (f64, f64) {
    let (bw, bh) = box_of(f, ellipse);
    match (share, top_left) {
        (Some((sx, sy)), _) => (f.centre.0 + sx * bw, f.centre.1 + sy * bh),
        (None, Some((dx, dy))) => (f.centre.0 - bw / 2.0 + dx, f.centre.1 - bh / 2.0 + dy),
        _ => f.centre,
    }
}

fn shape_named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    page(doc, 0).shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("a shape named {name}"))
}

fn n(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// The page point (inches, y down) of the local point (`x`, `y`) of a shape (y up), by its cells.
fn model_at(shape: &Shape, (x, y): (f64, f64)) -> (f64, f64) {
    let (s, c) = n(shape, "Angle").sin_cos();
    let flip = |name: &str| if n(shape, name) == 1.0 { -1.0 } else { 1.0 };
    let (dx, dy) = ((x - n(shape, "LocPinX")) * flip("FlipX"), (y - n(shape, "LocPinY")) * flip("FlipY"));
    (n(shape, "PinX") + c * dx - s * dy, PAGE_H - (n(shape, "PinY") + s * dx + c * dy))
}

/// The local point of `Connection` row `ix` of a shape.
fn row_at(shape: &Shape, ix: u32) -> (f64, f64) {
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Connection").expect("a Connection section");
    let row = section.rows.iter().find(|r| r.ix == Some(ix)).unwrap_or_else(|| panic!("row {ix}"));
    let v = |name: &str| row.cells.get(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    (v("X"), v("Y"))
}

/// Where the model has the end of the connector `name`: the row its end is glued to, on the shape, on the page.
fn glued_end(doc: &Document, name: &str) -> (f64, f64) {
    let c = shape_named(doc, name);
    let connect = page(doc, 0)
        .connects
        .iter()
        .find(|k| k.from_sheet == c.id && k.from_cell.as_deref() == Some("EndX"))
        .unwrap_or_else(|| panic!("{name} is glued at its end"));
    let row = connect.to_part.expect("a part") - 100;
    let target = page(doc, 0).shapes.iter().find(|s| s.id == connect.to_sheet).expect("the shape glued to");
    model_at(target, row_at(target, row))
}

#[track_caller]
fn near(got: (f64, f64), want: (f64, f64), what: &str) {
    let d = (got.0 - want.0).hypot(got.1 - want.1);
    assert!(d < 1e-6, "{what}: the model has it at {got:?}, LibreOffice at {want:?} ({d:.3} inch off)");
}

/// Bug 1: a mirror beside a `draw:transform` that flips the frame itself (`scale (1 -1)`, `scale (-1 1)`, a reflecting `matrix`). LibreOffice draws the mirror after the whole
/// transform, so the turn beside one mirror is the other way round there too. The reader left it as read (0.15 to 0.21 of overlap with LibreOffice's picture, 0.56 inch off).
#[test]
fn flipglue_a_mirror_beside_a_transform_that_flips_the_frame_reads_as_libreoffice_draws_it() {
    let mut cases = 0;
    for prefix in [SCALE_Y, SCALE_X, reflect60(), NO_PREFIX] {
        for m in MIRRORS {
            for deg in [0.0, 30.0, 135.0] {
                let f = frame(prefix, deg, (4.0, 5.0));
                let (doc, _) = Drawing::a4().page(&triangle("t", &f, m, "", "")).read();
                let t = shape_named(&doc, "t");
                let what = format!("{}{m:?} at {deg} degrees", prefix.0);
                // The triangle's corners: the frame's top left, bottom left and bottom right; the model's local (0, H), (0, 0), (W, 0).
                for ((u, v), local) in [((0.0, 0.0), (0.0, H)), ((0.0, H), (0.0, 0.0)), ((W, H), (W, 0.0))] {
                    near(model_at(t, local), picture(&f, m, (u, v)), &what);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 48);
}

/// Bug 2: a connector glued to standard point 1 or 3 of a shape with one flip ends on the other side of it in LibreOffice (about the shape's width off: 1.18 inch for these).
/// Custom shapes in each mirror state, turned and not, and shapes whose transform flips them (custom shapes, rectangles, ellipses, a text frame).
#[test]
fn flipglue_the_standard_glue_points_of_a_flipped_shape_are_where_libreoffice_ends_its_connectors() {
    let from = (1.0, 1.0);
    let mut checked = 0;
    for prefix in [NO_PREFIX, SCALE_Y, SCALE_X] {
        for m in MIRRORS {
            for deg in [0.0, 30.0, 135.0] {
                let f = frame(prefix, deg, (4.0, 5.0));
                let mut shapes = triangle("t", &f, m, "", "");
                for id in 0..4 {
                    shapes.push_str(&connector(&format!("c{id}"), "t", id, from));
                }
                let (doc, _) = Drawing::a4().page(&shapes).read();
                for id in 0..4 {
                    near(glued_end(&doc, &format!("c{id}")), standard_point(&f, m, id), &format!("{}{m:?} at {deg} degrees, point {id}", prefix.0));
                    checked += 1;
                }
            }
        }
    }
    // Shapes of other kinds whose transform flips them: no mirror, the same rule.
    for (element, inner) in [("rect", "<text:p/>"), ("ellipse", "<text:p/>"), ("frame", "<draw:text-box><text:p>x</text:p></draw:text-box>")] {
        for prefix in [SCALE_Y, SCALE_X] {
            for deg in [0.0, 30.0] {
                let f = frame(prefix, deg, (4.0, 5.0));
                let mut shapes = format!(
                    r#"<draw:{element} draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}">{inner}</draw:{element}>"#,
                    f.text
                );
                for id in 0..4 {
                    shapes.push_str(&connector(&format!("c{id}"), "s", id, from));
                }
                let (doc, _) = Drawing::a4().page(&shapes).read();
                for id in 0..4 {
                    near(
                        glued_end(&doc, &format!("c{id}")),
                        standard_point(&f, "", id),
                        &format!("a {element} beside {}at {deg} degrees, point {id}", prefix.0),
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 144 + 48);
}

/// A polygon keeps the fixed map: LibreOffice puts its standard points on the box of its own points, which no flip of the model reaches (unchanged).
#[test]
fn flipglue_a_polygon_beside_a_flipping_transform_keeps_the_fixed_map() {
    let f = frame(SCALE_Y, 0.0, (4.0, 5.0));
    let mut shapes = format!(
        r#"<draw:polygon draw:id="p" xml:id="p" draw:name="p" svg:width="3cm" svg:height="1.5cm" svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1500 3000,1500" draw:transform="{}"><text:p/></draw:polygon>"#,
        f.text
    );
    shapes.push_str(&connector("c1", "p", 1, (1.0, 1.0)));
    let (doc, _) = Drawing::a4().page(&shapes).read();
    let c = shape_named(&doc, "c1");
    let connect = page(&doc, 0).connects.iter().find(|k| k.from_sheet == c.id).expect("glued");
    assert_eq!(connect.to_part, Some(103), "point 1 is row 3 on a polygon, flipped or not");
}

/// Bug 3: a `draw:glue-point` in LibreOffice's own form and one with `draw:align` are where LibreOffice puts them, in the page's axes from the shape's box, on custom shapes
/// in each mirror state turned or not and beside transforms that flip them, and on turned rectangles and ellipses. The reader had them in the shape's frame, mirrored and
/// turned with it: 0.16 to 1.08 inch off.
#[test]
fn flipglue_a_glue_point_element_is_where_libreoffice_puts_it_on_a_turned_or_flipped_shape() {
    let points = r#"<draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/><draw:glue-point draw:id="6" svg:x="0.5cm" svg:y="0.3cm" draw:align="top-left" draw:escape-direction="auto"/>"#;
    let from = (1.0, 1.0);
    let mut checked = 0;
    let mut check = |doc: &Document, f: &Frame, ellipse: bool, what: &str| {
        near(glued_end(doc, "c5"), element_point(f, ellipse, Some((-0.3, 0.2)), None), &format!("{what}, point 5"));
        near(glued_end(doc, "c6"), element_point(f, ellipse, None, Some((0.5 / 2.54, 0.3 / 2.54))), &format!("{what}, point 6"));
        checked += 2;
    };
    for prefix in [NO_PREFIX, SCALE_Y, SCALE_X] {
        for m in MIRRORS {
            for deg in [0.0, 30.0, 135.0] {
                let f = frame(prefix, deg, (4.0, 5.0));
                let shapes = [triangle("t", &f, m, points, ""), connector("c5", "t", 5, from), connector("c6", "t", 6, from)].concat();
                let (doc, _) = Drawing::a4().page(&shapes).read();
                check(&doc, &f, false, &format!("a triangle {}{m:?} at {deg} degrees", prefix.0));
            }
        }
    }
    for (element, ellipse) in [("rect", false), ("ellipse", true)] {
        for prefix in [NO_PREFIX, SCALE_Y] {
            for deg in [0.0, 30.0, 135.0] {
                let f = frame(prefix, deg, (4.0, 5.0));
                let shapes = [
                    format!(
                        r#"<draw:{element} draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}">{points}<text:p/></draw:{element}>"#,
                        f.text
                    ),
                    connector("c5", "s", 5, from),
                    connector("c6", "s", 6, from),
                ]
                .concat();
                let (doc, _) = Drawing::a4().page(&shapes).read();
                check(&doc, &f, ellipse, &format!("a {element} {}at {deg} degrees", prefix.0));
            }
        }
    }
    assert_eq!(checked, 72 + 24);
}

/// The rows of such points stay shares of the shape: a formula of `Width` and `Height`, so a resize keeps each at its share of the box, as LibreOffice's do; and a shape that
/// is square on the page and unflipped reads them as before.
#[test]
fn flipglue_a_glue_point_elements_row_is_a_formula_of_the_shapes_size() {
    let points = r#"<draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/>"#;
    let row_formulas = |doc: &Document| {
        let t = shape_named(doc, "t");
        let section = t.sheet.sections.iter().find(|s| s.name.as_str() == "Connection").expect("rows");
        let row = section.rows.iter().find(|r| r.ix == Some(5)).expect("row 5");
        (row.cells.get("X").and_then(|c| c.formula.text().map(str::to_owned)), row.cells.get("Y").and_then(|c| c.formula.text().map(str::to_owned)))
    };
    let (doc, _) = Drawing::a4().page(&triangle("t", &frame(NO_PREFIX, 0.0, (4.0, 5.0)), "", points, "")).read();
    let (x, y) = row_formulas(&doc);
    assert!(x.as_deref().is_some_and(|f| f.starts_with("Width*(0.5+-2999.99") && f.ends_with("/10000)")), "The form: {x:?}");
    assert!(y.as_deref().is_some_and(|f| f.starts_with("Height*(0.5-1999.99") && f.ends_with("/10000)")), "The form: {y:?}");
    let (doc, _) = Drawing::a4().page(&triangle("t", &frame(NO_PREFIX, 0.0, (4.0, 5.0)), "h", points, "")).read();
    assert_eq!(row_formulas(&doc), (Some("Width*0.8".to_owned()), Some("Height*0.3".to_owned())), "mirrored: the share the other side of the centre");
    let (doc, _) = Drawing::a4().page(&triangle("t", &frame(NO_PREFIX, 30.0, (4.0, 5.0)), "h", points, "")).read();
    let (x, y) = row_formulas(&doc);
    assert!(x.as_deref().is_some_and(|f| f.contains("Width") && f.contains("Height")), "{x:?}");
    assert!(y.as_deref().is_some_and(|f| f.contains("Width") && f.contains("Height")), "{y:?}");
}

/// The text turns LibreOffice 26.8.2.2 drew (counterclockwise, degrees) beside transforms that flip: (element, prefix, angle, turn). The triangles' have no mirror.
const TEXT_TURNS: [(&str, &str, f64, f64); 12] = [
    ("custom-shape", "y", 0.0, 180.0),
    ("custom-shape", "y", 30.0, 210.0),
    ("custom-shape", "x", 0.0, 0.0),
    ("custom-shape", "x", 30.0, 30.0),
    ("rect", "y", 0.0, 180.0),
    ("rect", "y", 30.0, 210.0),
    ("rect", "x", 0.0, 0.0),
    ("rect", "x", 30.0, 30.0),
    ("ellipse", "y", 0.0, 180.0),
    ("ellipse", "y", 30.0, 210.0),
    ("ellipse", "x", 0.0, 0.0),
    ("ellipse", "x", 30.0, 30.0),
];

/// The turn the model gives a shape's text, counterclockwise as seen, degrees in [0, 360).
fn model_text_turn(shape: &Shape) -> f64 {
    let flips = usize::from(n(shape, "FlipX") == 1.0) + usize::from(n(shape, "FlipY") == 1.0);
    let (angle, text) = (n(shape, "Angle").to_degrees(), n(shape, "TxtAngle").to_degrees());
    (if flips == 1 { angle - text } else { angle + text }).rem_euclid(360.0)
}

#[track_caller]
fn same_turn(got: f64, want: f64, what: &str) {
    let d = (got - want).rem_euclid(360.0);
    assert!(d.min(360.0 - d) < 1e-6, "{what}: the model turns the text {got} degrees, LibreOffice {want}");
}

/// Bug 4: the text of a vertically mirrored custom shape is half a turn more than its frame's in LibreOffice (and of one whose transform flips it vertically), and the
/// text's own turn (`draw:text-rotate-angle`) is counterclockwise whatever the mirrors. The reader gave it the frame's turn: upside down.
#[test]
fn flipglue_the_text_of_a_mirrored_shape_turns_as_libreoffice_turns_it() {
    let text = r#"<text:p>Ab</text:p>"#;
    for m in MIRRORS {
        for deg in [0.0, 30.0, 135.0] {
            for own in [None, Some(90.0), Some(180.0)] {
                let f = frame(NO_PREFIX, deg, (4.0, 5.0));
                let extra = own.map(|t| format!(r#" draw:text-rotate-angle="{t}""#)).unwrap_or_default();
                let (doc, _) = Drawing::a4().page(&triangle("t", &f, m, text, &extra)).read();
                let drawn = if m.len() == 1 { -deg } else { deg };
                let want = drawn + if m.contains('v') { 180.0 } else { 0.0 } + own.unwrap_or(0.0);
                same_turn(model_text_turn(shape_named(&doc, "t")), want, &format!("{m:?} at {deg} degrees, the text's own turn {own:?}"));
            }
        }
    }
    for (element, prefix, deg, want) in TEXT_TURNS {
        let f = frame(if prefix == "y" { SCALE_Y } else { SCALE_X }, deg, (4.0, 5.0));
        let shape = if element == "custom-shape" {
            triangle("t", &f, "", text, "")
        } else {
            format!(r#"<draw:{element} draw:name="t" svg:width="3cm" svg:height="1.5cm" draw:transform="{}">{text}</draw:{element}>"#, f.text)
        };
        let (doc, _) = Drawing::a4().page(&shape).read();
        same_turn(
            model_text_turn(shape_named(&doc, "t")),
            want,
            &format!("a {element} beside {}at {deg} degrees", f.text.split("rotate").next().unwrap_or("")),
        );
    }
}

/// A shape with no flip keeps its text's turn as the file says it, whatever the number (no turn added and none brought into a range).
#[test]
fn flipglue_the_texts_own_turn_of_an_unflipped_shape_is_the_files_number() {
    let f = frame(NO_PREFIX, 0.0, (4.0, 5.0));
    let (doc, _) = Drawing::a4().page(&triangle("t", &f, "", "<text:p>Ab</text:p>", r#" draw:text-rotate-angle="270""#)).read();
    let t = shape_named(&doc, "t");
    assert!((n(t, "TxtAngle") - 270f64.to_radians()).abs() < 1e-12);
    let (doc, _) = Drawing::a4().page(&triangle("t", &f, "", "<text:p>Ab</text:p>", "")).read();
    assert!(shape_named(&doc, "t").sheet.cell("TxtAngle").is_none(), "no turn, no cell");
    // The half turn of a vertical mirror and the text's own half turn are none.
    let (doc, _) = Drawing::a4().page(&triangle("t", &f, "v", "<text:p>Ab</text:p>", r#" draw:text-rotate-angle="180""#)).read();
    assert!(shape_named(&doc, "t").sheet.cell("TxtAngle").is_none(), "a half turn and a half turn are no cell");
}

/// Beside one mirror the turn read is minus the file's, in the cell's own unit (degrees), at a half turn and nearly a whole one as at 30.
#[test]
fn flipglue_the_turn_negated_beside_one_mirror_keeps_its_unit_at_a_half_turn_and_nearly_a_whole_one() {
    for deg in [30.0, 180.0, 359.9] {
        for m in ["h", "v"] {
            let f = frame(NO_PREFIX, deg, (4.0, 5.0));
            let (doc, _) = Drawing::a4().page(&triangle("t", &f, m, "", "")).read();
            let t = shape_named(&doc, "t");
            let angle = t.sheet.cell("Angle").expect("a turn");
            assert_eq!(angle.unit.as_deref(), Some("DEG"), "{m} at {deg}: the unit");
            // Minus the file's turn, a whole turn apart at the most (the frame's turn is read in (-180, 180] degrees).
            let got = n(t, "Angle");
            let d = (got + deg.to_radians()).rem_euclid(std::f64::consts::TAU);
            assert!(d.min(std::f64::consts::TAU - d) < 1e-9, "{m} at {deg}: {got} is minus the file's turn");
            // And the picture is LibreOffice's.
            near(model_at(t, (0.0, H)), picture(&f, m, (0.0, 0.0)), &format!("{m} at {deg}"));
        }
    }
}
