//! What the reader makes of the glue points of polygons, polylines, paths and frames, of the ways out of glue points on turned shapes, and of a
//! `draw:transform` whose translation has no unit, against what LibreOffice 26.8.2.2 draws of the same markup (a black box: hand-written flat drawings converted to SVG
//! with its own profile, each connector's end and last leg measured; LibreOffice was never given a file of anyone else's). Each oracle below is written from that
//! measurement, not from the reader:
//!
//! - **A polygon's, a polyline's and a path's glue points** are on the box of the outline it draws, on the page: the four standard points at the middles of its sides,
//!   numbered on the page (0 top, 1 right, 2 bottom, 3 left, whatever the turn and the flips); a `draw:glue-point` in the page's axes from that box, as on a custom
//!   shape. A curve's box is its own extent, not its control points' ([`outline_box`]). Every end of 196 within 0.001 inch.
//! - **A frame has no glue point of its own** in LibreOffice: a connector that names a `draw:glue-point` of a text frame or a picture frame is glued to the frame as a whole
//!   (its saved copy has no `draw:glue-point` and the connector no `draw:end-glue-point`). Found that this holds only for a point that comes **before** the
//!   frame's text box or picture, as this file's drawings had it: one after them (where the standard puts it) LibreOffice places as a custom shape's. By (an end
//!   glued to a glue point follows it, whatever kind of point it is) the element is a row in either order, placed so.
//! - **A way out** (`draw:escape-direction` `left`, `right`, `up`, `down`) is the page's own direction, on custom shapes, rectangles and polygons turned 0, 30, 90 and 135
//!   degrees, mirrored, and flipped by their transform; a polygon's standard points leave its box outwards on the page.
//! - **A translation with no unit** (`matrix (a b c d e f)`'s `e` and `f`, `translate`'s) is hundredths of a millimetre.
//!
//! The model's side is worked out from its cells: a local point (y up) goes to the page by the flips about the local pin, the turn and the pin.

use diacraft_doc::{Document, Shape};

use crate::drawing::{Drawing, cm, page};

const PAGE_H: f64 = 29.7 / 2.54;
const W: f64 = 3.0 / 2.54;
const H: f64 = 1.5 / 2.54;

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

/// A frame of `W` by `H` whose transform is `prefix` (its text and its map), then `rotate (deg)`, then the translation that puts the frame's centre at `centre` (inches,
/// y down).
struct Frame {
    lin: Lin,
    t: (f64, f64),
    text: String,
}

fn frame(prefix: (&str, Lin), deg: f64, centre: (f64, f64)) -> Frame {
    let a = deg.to_radians();
    let lin = mul(rotate(a), prefix.1);
    let q = apply(lin, (W / 2.0, H / 2.0));
    let t = (centre.0 - q.0, centre.1 - q.1);
    let text = format!("{}rotate ({a:?}) translate ({:.6}cm {:.6}cm)", prefix.0, t.0 * 2.54, t.1 * 2.54);
    Frame { lin, t, text }
}

impl Frame {
    /// The page point (inches, y down) of the frame point (`u`, `v`) (inches from the frame's top left, y down).
    fn at(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let p = apply(self.lin, (u, v));
        (p.0 + self.t.0, p.1 + self.t.1)
    }
}

const NO_PREFIX: (&str, Lin) = ("", [1.0, 0.0, 0.0, 1.0]);
const SCALE_Y: (&str, Lin) = ("scale (1 -1) ", [1.0, 0.0, 0.0, -1.0]);
const SCALE_X: (&str, Lin) = ("scale (-1 1) ", [-1.0, 0.0, 0.0, 1.0]);

/// The places measured: (prefix, turn in degrees).
fn places() -> [((&'static str, Lin), f64); 7] {
    [(NO_PREFIX, 0.0), (NO_PREFIX, 30.0), (NO_PREFIX, 135.0), (NO_PREFIX, 90.0), (SCALE_Y, 0.0), (SCALE_Y, 30.0), (SCALE_X, 30.0)]
}

fn shape_named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    page(doc, 0).shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("a shape named {name}"))
}

fn n(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

fn flip(shape: &Shape, name: &str) -> f64 {
    if n(shape, name) == 1.0 { -1.0 } else { 1.0 }
}

/// The page point (inches, y down) of the local point (`x`, `y`) of a shape (y up), by its cells.
fn model_at(shape: &Shape, (x, y): (f64, f64)) -> (f64, f64) {
    let (s, c) = n(shape, "Angle").sin_cos();
    let (dx, dy) = ((x - n(shape, "LocPinX")) * flip(shape, "FlipX"), (y - n(shape, "LocPinY")) * flip(shape, "FlipY"));
    (n(shape, "PinX") + c * dx - s * dy, PAGE_H - (n(shape, "PinY") + s * dx + c * dy))
}

/// The page direction (y up) the local direction (`x`, `y`) of a shape has, by its cells.
fn model_direction(shape: &Shape, (x, y): (f64, f64)) -> (f64, f64) {
    let (s, c) = n(shape, "Angle").sin_cos();
    let (dx, dy) = (x * flip(shape, "FlipX"), y * flip(shape, "FlipY"));
    (c * dx - s * dy, s * dx + c * dy)
}

/// A cell of `Connection` row `ix` of a shape.
fn row_number(shape: &Shape, ix: u32, cell: &str) -> f64 {
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Connection").expect("a Connection section");
    let row = section.rows.iter().find(|r| r.ix == Some(ix)).unwrap_or_else(|| panic!("row {ix}"));
    row.cells.get(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// The `Connect` row of the end of the connector `name`.
fn end_connect<'a>(doc: &'a Document, name: &str) -> &'a diacraft_doc::Connect {
    let c = shape_named(doc, name);
    page(doc, 0)
        .connects
        .iter()
        .find(|k| k.from_sheet == c.id && k.from_cell.as_deref() == Some("EndX"))
        .unwrap_or_else(|| panic!("{name} is glued at its end"))
}

/// The row the end of the connector `name` is glued to, and the shape.
fn glued_row<'a>(doc: &'a Document, name: &str) -> (&'a Shape, u32) {
    let connect = end_connect(doc, name);
    let row = connect.to_part.expect("a part") - 100;
    (page(doc, 0).shapes.iter().find(|s| s.id == connect.to_sheet).expect("the shape glued to"), row)
}

/// Where the model has the end of the connector `name`: the row its end is glued to, on the shape, on the page.
fn glued_end(doc: &Document, name: &str) -> (f64, f64) {
    let (target, row) = glued_row(doc, name);
    model_at(target, (row_number(target, row, "X"), row_number(target, row, "Y")))
}

#[track_caller]
fn near(got: (f64, f64), want: (f64, f64), within: f64, what: &str) {
    let d = (got.0 - want.0).hypot(got.1 - want.1);
    assert!(d < within, "{what}: the model has it at {got:?}, LibreOffice at {want:?} ({d:.4} inch off)");
}

/// An element made of its attributes, what it holds and a mirror attribute for its geometry.
type Element = fn(&str, &str, &str) -> String;

/// A straight connector from a free point to glue point `id` of `target`, whose own end is written wrong on purpose (LibreOffice ends it at the point).
fn connector(name: &str, target: &str, id: u32, from: (f64, f64), kind: &str) -> String {
    format!(
        r#"<draw:connector draw:name="{name}" draw:type="{kind}" svg:x1="{0:.4}cm" svg:y1="{1:.4}cm" svg:x2="{0:.4}cm" svg:y2="{1:.4}cm" draw:end-shape="{target}" draw:end-glue-point="{id}"><text:p/></draw:connector>"#,
        from.0 * 2.54,
        from.1 * 2.54
    )
}

/// Three `draw:glue-point` elements: 5 in LibreOffice's own form (shares of the box from its centre, -30% and 20%), 6 and 7 from the box's top left and bottom right.
const POINTS: &str = r#"<draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/><draw:glue-point draw:id="6" svg:x="0.5cm" svg:y="0.3cm" draw:align="top-left" draw:escape-direction="auto"/><draw:glue-point draw:id="7" svg:x="-0.4cm" svg:y="-0.2cm" draw:align="bottom-right" draw:escape-direction="auto"/>"#;

/// An outline drawn in the view box `0 0 3000 1500` (the frame's 3 cm by 1.5 cm): its element, and its points in the frame (inches from the top left, y down), a curve
/// sampled finely.
struct Outline {
    element: fn(&str, &str) -> String,
    points: fn() -> Vec<(f64, f64)>,
    within: f64,
}

fn vb(x: f64, y: f64) -> (f64, f64) {
    (x / 3000.0 * W, y / 1500.0 * H)
}

/// The kinds measured: a triangle filling the frame, a triangle that does not reach its far sides (a polygon and a polyline), and a path whose cubic stays above its
/// control points (the curve's box is 1125 of the 1500 high; its control points' the whole).
fn outlines() -> [(&'static str, Outline); 4] {
    let triangle = || [(0.0, 0.0), (0.0, 1500.0), (3000.0, 1500.0)].map(|(x, y)| vb(x, y)).to_vec();
    let small = || [(0.0, 0.0), (0.0, 1000.0), (2000.0, 1000.0)].map(|(x, y)| vb(x, y)).to_vec();
    let curve = || {
        let pts = [(0.0, 0.0), (0.0, 1500.0), (3000.0, 1500.0), (3000.0, 0.0)];
        (0..=20_000)
            .map(|k| {
                let t = f64::from(k) / 20_000.0;
                let u = 1.0 - t;
                let b = [u * u * u, 3.0 * t * u * u, 3.0 * t * t * u, t * t * t];
                let x: f64 = pts.iter().zip(b).map(|(p, b)| p.0 * b).sum();
                let y: f64 = pts.iter().zip(b).map(|(p, b)| p.1 * b).sum();
                vb(x, y)
            })
            .collect()
    };
    [
        (
            "polygon",
            Outline {
                element: |attrs, inner| {
                    format!(r#"<draw:polygon {attrs} svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1500 3000,1500">{inner}<text:p/></draw:polygon>"#)
                },
                points: triangle,
                within: 1e-6,
            },
        ),
        (
            "small polygon",
            Outline {
                element: |attrs, inner| {
                    format!(r#"<draw:polygon {attrs} svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1000 2000,1000">{inner}<text:p/></draw:polygon>"#)
                },
                points: small,
                within: 1e-6,
            },
        ),
        (
            "small polyline",
            Outline {
                element: |attrs, inner| {
                    format!(
                        r#"<draw:polyline {attrs} svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1000 2000,1000">{inner}<text:p/></draw:polyline>"#
                    )
                },
                points: small,
                within: 1e-6,
            },
        ),
        (
            "path",
            Outline {
                element: |attrs, inner| {
                    format!(
                        r#"<draw:path {attrs} svg:viewBox="0 0 3000 1500" svg:d="M 0 0 C 0 1500 3000 1500 3000 0 Z">{inner}<text:p/></draw:path>"#
                    )
                },
                points: curve,
                within: 2e-5,
            },
        ),
    ]
}

/// The box (x0, y0, x1, y1; inches, y down) of an outline's points on the page.
fn outline_box(f: &Frame, points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let page: Vec<(f64, f64)> = points.iter().map(|p| f.at(*p)).collect();
    let (xs, ys) = (page.iter().map(|p| p.0), page.iter().map(|p| p.1));
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

/// Where LibreOffice ends a connector glued to point `id` of a shape whose box on the page is `b` (inches, y down).
fn on_box(b: (f64, f64, f64, f64), id: u32) -> (f64, f64) {
    let (x0, y0, x1, y1) = b;
    let (cx, cy, bw, bh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0);
    match id {
        0 => (cx, y0),
        1 => (x1, cy),
        2 => (cx, y1),
        3 => (x0, cy),
        5 => (cx - 0.3 * bw, cy + 0.2 * bh),
        6 => (x0 + cm(0.5), y0 + cm(0.3)),
        _ => (x1 - cm(0.4), y1 - cm(0.2)),
    }
}

const IDS: [u32; 7] = [0, 1, 2, 3, 5, 6, 7];

/// Bug 1: the four standard glue points and three `draw:glue-point` elements of polygons, polylines and a path, turned and flipped by their transform, are where
/// LibreOffice ends its connectors: on the box of the outline on the page. The reader had them on the frame, turning with it (0.15 to 1.21 inch off when turned, and
/// off on an outline that does not fill its frame even unturned).
#[test]
fn gluef2_the_glue_points_of_a_polygon_a_polyline_and_a_path_are_on_the_box_of_its_outline_on_the_page() {
    let mut checked = 0;
    for (kind, outline) in outlines() {
        for (prefix, deg) in places() {
            let f = frame(prefix, deg, (4.0, 5.0));
            let attrs = format!(r#"draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}""#, f.text);
            let mut shapes = (outline.element)(&attrs, POINTS);
            for id in IDS {
                shapes.push_str(&connector(&format!("c{id}"), "s", id, (1.0, 1.0), "line"));
            }
            let (doc, _) = Drawing::a4().page(&shapes).read();
            let b = outline_box(&f, &(outline.points)());
            for id in IDS {
                near(
                    glued_end(&doc, &format!("c{id}")),
                    on_box(b, id),
                    outline.within,
                    &format!("a {kind} beside {}at {deg} degrees, point {id}", prefix.0),
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 4 * 7 * 7);
}

/// A polygon square on the page that fills its frame keeps the rows, as before.
#[test]
fn gluef2_a_polygon_square_on_the_page_that_fills_its_frame_keeps_the_frames_rows() {
    let shapes = r#"<draw:polygon draw:id="s" xml:id="s" draw:name="s" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="1.5cm" svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1500 3000,1500"><draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/><text:p/></draw:polygon>"#;
    let (doc, _) = Drawing::a4().page(shapes).read();
    let s = shape_named(&doc, "s");
    let section = s.sheet.sections.iter().find(|x| x.name.as_str() == "Connection").expect("rows");
    let formula =
        |ix: u32, cell: &str| section.rows.iter().find(|r| r.ix == Some(ix)).and_then(|r| r.cells.get(cell)?.formula.text().map(str::to_owned));
    assert_eq!((formula(0, "X").as_deref(), formula(0, "Y").as_deref()), (Some("Width*0.5"), Some("Height")), "The top middle");
    assert!(formula(5, "X").is_some_and(|f| f.starts_with("Width*(0.5+")), "The form: {:?}", formula(5, "X"));
}

/// A polygon's rows are sums of its size: a resize keeps each at its place on the box of the outline, as LibreOffice's do.
#[test]
fn gluef2_a_turned_polygons_rows_are_formulas_of_its_size() {
    let f = frame(NO_PREFIX, 30.0, (4.0, 5.0));
    let attrs = format!(r#"draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}""#, f.text);
    let (doc, _) = Drawing::a4().page(&(outlines()[0].1.element)(&attrs, POINTS)).read();
    let s = shape_named(&doc, "s");
    let section = s.sheet.sections.iter().find(|x| x.name.as_str() == "Connection").expect("rows");
    for row in &section.rows {
        for cell in ["X", "Y"] {
            let text = row.cells.get(cell).and_then(|c| c.formula.text()).unwrap_or_default();
            assert!(text.contains("Width") || text.contains("Height"), "row {:?} {cell}: {text:?}", row.ix);
        }
    }
}

/// The ways out of glue points (`draw:escape-direction`) on turned, mirrored and flipped shapes are the page's own directions, and a polygon's standard points leave the
/// box of its outline outwards on the page. The reader kept them in the shape's frame, so they turned with it.
#[test]
fn gluef2_a_glue_points_way_out_is_the_pages_own_direction_on_a_turned_or_flipped_shape() {
    let ways = [(5, "left", (-1.0, 0.0)), (6, "right", (1.0, 0.0)), (7, "up", (0.0, 1.0)), (8, "down", (0.0, -1.0))];
    let points: String = ways
        .iter()
        .map(|(id, word, _)| {
            format!(r#"<draw:glue-point draw:id="{id}" svg:x="{:.1}cm" svg:y="0cm" draw:escape-direction="{word}"/>"#, (f64::from(*id) - 6.5) * 0.4)
        })
        .collect();
    let kinds: [(&str, Element); 3] = [
        ("custom", |attrs, inner, mirror| {
            format!(
                r#"<draw:custom-shape {attrs}>{inner}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"{mirror}/></draw:custom-shape>"#
            )
        }),
        ("rect", |attrs, inner, _| format!(r#"<draw:rect {attrs}>{inner}<text:p/></draw:rect>"#)),
        ("polygon", |attrs, inner, _| {
            format!(r#"<draw:polygon {attrs} svg:viewBox="0 0 3000 1500" draw:points="0,0 3000,0 3000,1500 0,1500">{inner}<text:p/></draw:polygon>"#)
        }),
    ];
    let mut checked = 0;
    for (kind, element) in kinds {
        for (prefix, deg, mirror) in [
            (NO_PREFIX, 0.0, false),
            (NO_PREFIX, 30.0, false),
            (NO_PREFIX, 90.0, false),
            (NO_PREFIX, 135.0, false),
            (NO_PREFIX, 0.0, true),
            (NO_PREFIX, 30.0, true),
            (SCALE_Y, 0.0, false),
            (SCALE_Y, 30.0, false),
        ] {
            if mirror && kind != "custom" {
                continue;
            }
            let f = frame(prefix, deg, (4.0, 5.0));
            let attrs = format!(r#"draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}""#, f.text);
            let (doc, _) = Drawing::a4().page(&element(&attrs, &points, if mirror { r#" draw:mirror-horizontal="true""# } else { "" })).read();
            let s = shape_named(&doc, "s");
            let what = format!("a {kind} beside {}at {deg} degrees{}", prefix.0, if mirror { ", mirrored" } else { "" });
            for (id, word, want) in ways {
                let got = model_direction(s, (row_number(s, id, "DirX"), row_number(s, id, "DirY")));
                near(got, want, 1e-9, &format!("{what}, point {id} ({word})"));
                checked += 1;
            }
            if kind == "polygon" {
                // The standard rows (top, left, bottom, right: ODF 0, 3, 2, 1) leave the box outwards on the page: up, left, down, right.
                for (row, want) in [(0, (0.0, 1.0)), (1, (-1.0, 0.0)), (2, (0.0, -1.0)), (3, (1.0, 0.0))] {
                    let got = model_direction(s, (row_number(s, row, "DirX"), row_number(s, row, "DirY")));
                    near(got, want, 1e-9, &format!("{what}, standard row {row}"));
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 8 * 4 + 6 * 4 + 6 * 8);
}

/// Bug 2: glued a connector that names a frame's `draw:glue-point` to the frame as a whole, which nothing then moved
/// with the frame. A connector end glued to a glue point of a shape follows that point whatever kind of point it is, so a frame's `draw:glue-point` elements are rows,
/// placed where LibreOffice 26.8.2.2 places a frame's point that follows its content (in the page's axes from the box the frame takes up, as a custom shape's: measured by
/// Turned 0 and 30 degrees), and an end that names one is glued to its row. The elements stay held beside their rows, and nothing is said. The points come
/// before the frame's content here, as in the drawings (LibreOffice drops such a point; the reader reads it all the same).
#[test]
fn gluef2_a_frames_glue_point_elements_are_rows_in_its_own_frame_and_a_connector_to_one_is_glued_to_it() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGNgAAAAAgABSK+kcQAAAABJRU5ErkJggg==";
    let inners = [
        ("text frame", "<draw:text-box><text:p>x</text:p></draw:text-box>".to_owned()),
        ("picture frame", format!("<draw:image><office:binary-data>{png}</office:binary-data></draw:image>")),
    ];
    for (what, inner) in inners {
        for deg in [0.0, 30.0] {
            let f = frame(NO_PREFIX, deg, (4.0, 5.0));
            let points = r#"<draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/><draw:glue-point draw:id="6" svg:x="0.5cm" svg:y="0.3cm" draw:align="top-left" draw:escape-direction="auto"/>"#;
            let shapes = [
                format!(r#"<draw:frame draw:id="s" xml:id="s" draw:name="s" svg:width="3cm" svg:height="1.5cm" draw:transform="{}">{points}{inner}</draw:frame>"#, f.text),
                connector("c5", "s", 5, (1.0, 1.0), "line"),
                connector("c6", "s", 6, (1.0, 1.0), "line"),
                connector("c1", "s", 1, (1.0, 1.0), "line"),
            ]
            .concat();
            let (doc, report) = Drawing::a4().page(&shapes).read();
            let what = format!("a {what} at {deg} degrees");
            assert_eq!(end_connect(&doc, "c5").to_part, Some(105), "{what}: c5 is glued to point 5");
            assert_eq!(end_connect(&doc, "c5").to_cell.as_deref(), Some("Connections.X6"), "{what}: c5");
            assert_eq!(end_connect(&doc, "c6").to_part, Some(106), "{what}: c6 is glued to point 6");
            assert_eq!(end_connect(&doc, "c1").to_part, Some(103), "{what}: a standard point is a row as before");
            let s = shape_named(&doc, "s");
            let rows: Vec<Option<u32>> =
                s.sheet.sections.iter().filter(|x| x.name.as_str() == "Connection").flat_map(|x| x.rows.iter().map(|r| r.ix)).collect();
            assert_eq!(rows, [Some(0), Some(1), Some(2), Some(3), Some(5), Some(6)], "{what}: the four standard rows and the two elements'");
            assert_eq!(report.warned("glue_point_frame"), 0, "{what}: nothing is said");
            assert_eq!(report.warned("glue_point_id"), 0, "{what}: nothing is said");
            // In the page's axes from the box the turned frame takes up (y down): 5 at -30 % and +20 % of the box from its centre, 6 at 0.5 cm and 0.3 cm from its top left
            // (within what the six places of the transform's translation keep).
            let (s30, c30) = deg.to_radians().sin_cos();
            let (bw, bh) = (W * c30.abs() + H * s30.abs(), W * s30.abs() + H * c30.abs());
            let centre = f.at((W / 2.0, H / 2.0));
            near(glued_end(&doc, "c5"), (centre.0 - 0.3 * bw, centre.1 + 0.2 * bh), 1e-6, &format!("{what}, point 5"));
            near(glued_end(&doc, "c6"), (centre.0 - bw / 2.0 + cm(0.5), centre.1 - bh / 2.0 + cm(0.3)), 1e-6, &format!("{what}, point 6"));
            // The elements stay in the frame's kept markup, each held beside its row.
            let kept = format!("{:?}", crate::drawing::odf(s));
            assert!(kept.matches("glue-point").count() >= 2, "{what}: {kept}");
        }
    }
}

/// Bug 3: a `draw:transform` whose translation is a plain number (`matrix (a b c d 0 0)`, `matrix (1 0 0 1 2000 3000)`, `translate (2000)`) is read as LibreOffice reads it,
/// hundredths of a millimetre, and not dropped whole as unreadable (the reader read the shape as if it had no transform, square at the page's corner).
#[test]
fn gluef2_a_translation_with_no_unit_is_hundredths_of_a_millimetre() {
    let (s30, c30) = 30f64.to_radians().sin_cos();
    let cases = [
        (format!("matrix ({c30:?} {:?} {s30:?} {c30:?} 0 0)", -s30), 30.0, (1.0, 0.5)),
        ("matrix (1 0 0 1 2000 3000)".to_owned(), 0.0, (3.0, 3.5)),
        ("matrix (1 0 0 1 2cm 3000)".to_owned(), 0.0, (3.0, 3.5)),
        ("translate (2000 3000)".to_owned(), 0.0, (3.0, 3.5)),
        ("translate (2000)".to_owned(), 0.0, (3.0, 0.5)),
        ("matrix (1 0 0 1 2 3)".to_owned(), 0.0, (1.002, 0.503)),
    ];
    for (transform, deg, centre_cm) in cases {
        let shapes = format!(r#"<draw:rect draw:name="r" svg:width="2cm" svg:height="1cm" draw:transform="{transform}"><text:p/></draw:rect>"#);
        let (doc, report) = Drawing::a4().page(&shapes).read();
        assert_eq!(report.warned("transform_unreadable"), 0, "{transform}");
        let r = shape_named(&doc, "r");
        assert!((n(r, "Angle").to_degrees() - deg).abs() < 1e-9, "{transform}: Angle {}", n(r, "Angle").to_degrees());
        if deg == 0.0 {
            let (px, py) = (n(r, "PinX"), PAGE_H - n(r, "PinY"));
            assert!(
                (px - cm(centre_cm.0)).abs() < 1e-9 && (py - cm(centre_cm.1)).abs() < 1e-9,
                "{transform}: the pin at ({px}, {py}) inches, y down"
            );
        } else {
            // The frame turned 30 degrees about the page's corner: its centre is the image of (1 cm, 0.5 cm).
            let (u, v) = (cm(1.0), cm(0.5));
            let want = (u * c30 + v * s30, -u * s30 + v * c30);
            let (px, py) = (n(r, "PinX"), PAGE_H - n(r, "PinY"));
            assert!((px - want.0).abs() < 1e-9 && (py - want.1).abs() < 1e-9, "{transform}: the pin at ({px}, {py}), want {want:?}");
        }
    }
}
