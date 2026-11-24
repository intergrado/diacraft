//! Shape operations: `shape.union`, `shape.combine`, `shape.fragment`, `shape.intersect` and `shape.subtract`, through `Session::execute` as the control
//! channel calls them. Each test is named for the vector or the rule of it holds to.
//! Numbers are inches on a page of the default scale, where they are the cells' own.
//!
//! **How a result is read.** Not through the engine's own reader of rows: the test builds each shape's paths as the renderer does (`diacraft_render::geom_rows`,
//! `diacraft_geom::build_paths`), carries them through the shape's pose, and asks the path library for the area and the geometry crate whether a point is filled.
//! So what is checked is what is drawn, which is what the vectors give.

use std::sync::Arc;

use craft_pathops::FillRule;
use diacraft_doc::{
    Cell, CellAddr, CellName, Document, Links, Owner, Resolver, Row, Section, SectionKey, Shape, ShapeRef, ShapeType, Sheet, SheetRef, Value,
};
use diacraft_engine::{EngineError, Session};
use diacraft_geom::{SectionFlags, build_paths, combined_fill, even_odd_contains};
use kurbo::{Affine, BezPath, PathEl, Point, Size, Vec2};
use serde_json::{Value as Json, json};

use crate::{Snapshot, blank_session, run, snapshot};

pub(crate) type Pt = (f64, f64);

// ---- building shapes -----------------------------------------------------------------------------------------------------------

fn constant(n: f64) -> Cell {
    Cell::constant(Value::from_text(&format!("{n}")))
}

fn length(n: f64) -> Cell {
    constant(n).with_unit("IN")
}

/// A shape made by hand: a frame, and `Geometry` sections whose rows are constants in the shape's own frame, so that what a test expects is the numbers it wrote.
pub(crate) struct Build {
    shape: Shape,
    sections: u32,
}

impl Build {
    /// A shape `width` by `height` with its pin and its local pin in the middle, unturned.
    pub(crate) fn new(id: u32, width: f64, height: f64) -> Build {
        let mut shape = Shape::new(id);
        shape.kind = ShapeType::Shape;
        let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
        for (name, cell) in [
            ("PinX", length(0.0)),
            ("PinY", length(0.0)),
            ("Width", length(width)),
            ("Height", length(height)),
            ("LocPinX", length(width / 2.0)),
            ("LocPinY", length(height / 2.0)),
            ("Angle", constant(0.0)),
            ("FlipX", constant(0.0)),
            ("FlipY", constant(0.0)),
        ] {
            sheet.cells.insert(CellName::new(name), cell);
        }
        shape.sheet = Arc::new(sheet);
        Build { shape, sections: 0 }
    }

    fn sheet(&mut self) -> &mut Sheet {
        Arc::make_mut(&mut self.shape.sheet)
    }

    pub(crate) fn cell(mut self, name: &str, cell: Cell) -> Build {
        self.sheet().cells.insert(CellName::new(name), cell);
        self
    }

    pub(crate) fn pin(self, x: f64, y: f64) -> Build {
        self.cell("PinX", length(x)).cell("PinY", length(y))
    }

    pub(crate) fn angle_degrees(self, degrees: f64) -> Build {
        self.cell("Angle", constant(degrees.to_radians()))
    }

    pub(crate) fn flip(self, x: bool, y: bool) -> Build {
        self.cell("FlipX", constant(f64::from(u8::from(x)))).cell("FlipY", constant(f64::from(u8::from(y))))
    }

    pub(crate) fn text(mut self, text: &str) -> Build {
        self.shape.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from(text))], ..Default::default() }));
        self
    }

    /// A section of rows: `MoveTo` the first point, `LineTo` each other, and for a closed one `LineTo` the first again. `no_fill` 1 marks one that only strokes.
    pub(crate) fn section(mut self, points: &[Pt], closed: bool, no_fill: bool) -> Build {
        let ix = self.sections;
        self.sections += 1;
        let mut section = Section::new("Geometry", Some(ix));
        for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
            section.cells.insert(CellName::new(flag), constant(if flag == "NoFill" && no_fill { 1.0 } else { 0.0 }));
        }
        let mut all: Vec<Pt> = points.to_vec();
        if closed && let Some(first) = points.first() {
            all.push(*first);
        }
        for (i, (x, y)) in all.iter().enumerate() {
            let mut row = Row {
                ix: Some(u32::try_from(i + 1).unwrap()),
                row_type: Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" })),
                ..Row::default()
            };
            row.cells.insert(CellName::new("X"), length(*x));
            row.cells.insert(CellName::new("Y"), length(*y));
            section.rows.push(Arc::new(row));
        }
        self.sheet().sections.push(Arc::new(section));
        self
    }

    /// Sets a flag cell (`NoShow`, `NoFill`, ...) of the section numbered `ix`.
    pub(crate) fn section_flag(mut self, ix: u32, name: &str, on: bool) -> Build {
        if let Some(found) = self.sheet().sections.iter_mut().find(|s| s.ix == Some(ix)) {
            Arc::make_mut(found).cells.insert(CellName::new(name), constant(f64::from(u8::from(on))));
        }
        self
    }

    pub(crate) fn build(self) -> Shape {
        self.shape
    }
}

/// An unturned rectangle with opposite corners (`x0`, `y0`) and (`x1`, `y1`) in the container's coordinates: one closed section of four `LineTo` rows.
pub(crate) fn rect(id: u32, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Build {
    let (w, h) = (x1 - x0, y1 - y0);
    Build::new(id, w, h).pin((x0 + x1) / 2.0, (y0 + y1) / 2.0).section(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], true, false)
}

/// A straight line from (`x0`, `y0`) to (`x1`, `y1`) as an open section with `NoFill` 1.
pub(crate) fn line(id: u32, (x0, y0): Pt, (x1, y1): Pt) -> Build {
    let (left, bottom) = (x0.min(x1), y0.min(y1));
    let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
    Build::new(id, w, h).pin(left + w / 2.0, bottom + h / 2.0).section(&[(x0 - left, y0 - bottom), (x1 - left, y1 - bottom)], false, true)
}

/// Puts shapes at the end of page 1's list, as one undo step of setup.
pub(crate) fn put(s: &mut Session, shapes: Vec<Shape>) {
    s.edit("set up shapes", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        for shape in shapes {
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
}

/// A session with these shapes on page 1.
pub(crate) fn session_with(shapes: Vec<Shape>) -> Session {
    let mut s = blank_session();
    put(&mut s, shapes);
    s
}

// ---- reading what is there ------------------------------------------------------------------------------------------------------

pub(crate) fn doc(s: &Session) -> &Document {
    s.document().unwrap()
}

pub(crate) fn at(d: &Document, id: u32) -> ShapeRef {
    d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))
}

/// The `ID`s of the page's top-level shapes, back to front.
pub(crate) fn top(d: &Document) -> Vec<u32> {
    d.page(1).unwrap().shapes.iter().map(|s| s.id).collect()
}

pub(crate) fn num(d: &Document, id: u32, name: &str) -> f64 {
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at(d, id)), &CellAddr::direct(name)).unwrap_or_else(|| panic!("shape {id} has no {name}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("shape {id}: {name} holds no number"))
}

/// A cell the shape itself holds directly, as (formula, value).
pub(crate) fn local(d: &Document, id: u32, name: &str) -> Option<(Option<String>, Option<String>)> {
    let shape = d.shape(&at(d, id)).unwrap();
    shape.sheet.cell(name).map(|c| (c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned())))
}

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

pub(crate) fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

/// The shape's own frame to its container's, as its cells say.
pub(crate) fn pose_map(d: &Document, id: u32) -> Affine {
    let (pin, loc) = (Vec2::new(num(d, id, "PinX"), num(d, id, "PinY")), Vec2::new(num(d, id, "LocPinX"), num(d, id, "LocPinY")));
    let flip = Affine::scale_non_uniform(if num(d, id, "FlipX") != 0.0 { -1.0 } else { 1.0 }, if num(d, id, "FlipY") != 0.0 { -1.0 } else { 1.0 });
    Affine::translate(pin) * Affine::rotate(num(d, id, "Angle")) * flip * Affine::translate(-loc)
}

/// What each `Geometry` section of the shape draws, as the renderer builds it: (flags, stroke path, fill ring), in the shape's own frame.
pub(crate) fn built(d: &Document, id: u32) -> Vec<(SectionFlags, BezPath, BezPath)> {
    let sheet = SheetRef::Shape(at(d, id));
    let res = Resolver::new(d);
    let size = Size::new(num(d, id, "Width"), num(d, id, "Height"));
    let flag = |ix: Option<u32>, name: &str| {
        res.resolve(&sheet, &CellAddr::in_section("Geometry", ix, name))
            .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
            .unwrap_or(0.0)
            != 0.0
    };
    let mut out = Vec::new();
    for key in res.effective_sections(&sheet, "Geometry").unwrap_or_default() {
        let rows = res.effective_rows(&sheet, &key).unwrap_or_default();
        let (geom, _) = diacraft_render::geom_rows(&rows);
        let paths = build_paths(&geom, size).unwrap();
        let flags = SectionFlags { no_fill: flag(key.ix, "NoFill"), no_line: flag(key.ix, "NoLine"), no_show: flag(key.ix, "NoShow") };
        out.push((flags, paths.stroke, paths.fill));
    }
    out
}

/// The ring the shape fills, in its container's coordinates: its sections' rings that take the fill, together.
pub(crate) fn fill_ring(d: &Document, id: u32) -> BezPath {
    let sections = built(d, id);
    let mut ring = combined_fill(sections.iter().map(|(flags, _, fill)| (fill, *flags)));
    ring.apply_affine(pose_map(d, id));
    ring
}

/// The area the shape fills (an odd number of crossings).
pub(crate) fn area(d: &Document, id: u32) -> f64 {
    craft_pathops::try_area(&fill_ring(d, id), FillRule::EvenOdd).unwrap()
}

pub(crate) fn filled(d: &Document, id: u32, (x, y): Pt) -> bool {
    even_odd_contains(&fill_ring(d, id), Point::new(x, y))
}

/// The vertices of each shown section's path, in the shape's own frame: the points the rows pass through, a closed path's start once.
pub(crate) fn local_points(d: &Document, id: u32) -> Vec<Vec<Pt>> {
    built(d, id)
        .into_iter()
        .map(|(_, stroke, _)| {
            let mut points: Vec<Pt> = Vec::new();
            for el in stroke.elements() {
                match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => points.push((p.x, p.y)),
                    PathEl::ClosePath => {}
                }
            }
            if points.len() > 1 && points.first().zip(points.last()).is_some_and(|(a, b)| near(a.0, b.0) && near(a.1, b.1)) {
                points.pop();
            }
            points
        })
        .collect()
}

pub(crate) fn same_points(got: &[Pt], want: &[Pt]) -> bool {
    got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| near(g.0, w.0) && near(g.1, w.1)))
}

pub(crate) fn assert_points(what: &str, got: &[Pt], want: &[Pt]) {
    assert!(same_points(got, want), "{what}: got {got:?}, want {want:?}");
}

/// The row types and flags of a shape's section `ix`.
pub(crate) fn row_types(d: &Document, id: u32, ix: u32) -> Vec<String> {
    let shape = d.shape(&at(d, id)).unwrap();
    let section = shape.sheet.section(&SectionKey::new("Geometry", Some(ix))).unwrap();
    section.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()).collect()
}

/// The `IX` of each row of a shape's section `ix`.
pub(crate) fn row_ixs(d: &Document, id: u32, ix: u32) -> Vec<u32> {
    let shape = d.shape(&at(d, id)).unwrap();
    let section = shape.sheet.section(&SectionKey::new("Geometry", Some(ix))).unwrap();
    section.rows.iter().filter_map(|r| r.ix).collect()
}

pub(crate) fn flag_of(d: &Document, id: u32, ix: u32, name: &str) -> f64 {
    let shape = d.shape(&at(d, id)).unwrap();
    let section = shape.sheet.section(&SectionKey::new("Geometry", Some(ix))).unwrap();
    section.cells.get(name).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

pub(crate) fn section_count(d: &Document, id: u32) -> usize {
    d.shape(&at(d, id)).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry" && !s.del).count()
}

pub(crate) fn error_of(s: &mut Session, command: &str, params: &Json) -> EngineError {
    s.execute(command, params).expect_err("the command is refused")
}

pub(crate) fn assert_unchanged(s: &Session, before: &Snapshot, what: &str) {
    let now = snapshot(s);
    assert_eq!(now.document, before.document, "{what}: the document");
    assert_eq!((now.undo_steps, now.redo_steps, now.revision), (before.undo_steps, before.redo_steps, before.revision), "{what}: no undo step");
}

/// The `ID`s a result lists under `key`.
pub(crate) fn ids(result: &Json, key: &str) -> Vec<u32> {
    result[key].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect()
}

/// The only shape a result made.
pub(crate) fn made(result: &Json) -> u32 {
    let produced = ids(result, "ids");
    assert_eq!(produced.len(), 1, "one shape is made: {result}");
    produced[0]
}

fn two_squares() -> Session {
    session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).text("A").build(), rect(2, (2.0, 2.0, 4.0, 4.0)).text("B").build()])
}

// ---- Union ---------------------------------------------------------------------------------------------------------

#[test]
fn union_of_two_overlapping_squares_is_one_shape_with_the_outline_of_both() {
    let mut s = two_squares();
    let result = run(&mut s, "shape.union", json!({"ids": [1, 2]}));
    let id = made(&result);
    assert_eq!(ids(&result, "deleted"), [1, 2]);
    let d = doc(&s);
    assert_eq!(top(d), [id], "the originals are gone");
    for (cell, want) in [
        ("Angle", 0.0),
        ("Width", 3.0),
        ("Height", 3.0),
        ("PinX", 2.5),
        ("PinY", 2.5),
        ("LocPinX", 1.5),
        ("LocPinY", 1.5),
        ("FlipX", 0.0),
        ("FlipY", 0.0),
    ] {
        assert_near(cell, num(d, id, cell), want);
    }
    assert_eq!(section_count(d, id), 1);
    assert_eq!(flag_of(d, id, 0, "NoFill"), 0.0);
    let points = local_points(d, id);
    assert_points("The eight corners", &points[0], &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0), (1.0, 2.0), (0.0, 2.0)]);
    assert_near("filled area", area(d, id), 7.0);
    assert!(filled(d, id, (2.5, 2.5)) && !filled(d, id, (3.5, 1.5)));
    // The point (2, 1) is written as shares of the box, as the specification says: Width*0.666666666667 and Height*0.333333333333.
    let shape = d.shape(&at(d, id)).unwrap();
    let section = shape.sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap();
    let corner = section.rows.iter().find(|r| near(num_of(r, "X"), 2.0) && near(num_of(r, "Y"), 1.0)).expect("the row at (2, 1)");
    assert_eq!(corner.cells.get("X").unwrap().formula.text(), Some("Width*0.666666666667"));
    assert_eq!(corner.cells.get("Y").unwrap().formula.text(), Some("Height*0.333333333333"));
}

/// How wide the box of some points is.
fn width_of(points: &[Pt]) -> f64 {
    let xs = points.iter().map(|p| p.0);
    xs.clone().fold(f64::NEG_INFINITY, f64::max) - xs.fold(f64::INFINITY, f64::min)
}

fn num_of(row: &Row, name: &str) -> f64 {
    row.cells.get(name).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

#[test]
fn disjoint_squares_unite_to_one_shape_of_two_sections() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 1.0, 1.0)).build(), rect(2, (2.0, 0.0, 3.0, 1.0)).build()]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2);
    assert_near("area", area(d, id), 2.0);
    assert_near("Width", num(d, id, "Width"), 3.0);
    assert_near("Height", num(d, id, "Height"), 1.0);
}

#[test]
fn four_bars_in_a_ring_unite_to_a_square_and_the_hole_it_encloses() {
    let bars = [(0.0, 0.0, 3.0, 1.0), (0.0, 2.0, 3.0, 3.0), (0.0, 0.0, 1.0, 3.0), (2.0, 0.0, 3.0, 3.0)];
    let mut s = session_with(bars.iter().enumerate().map(|(i, b)| rect(u32::try_from(i + 1).unwrap(), *b).build()).collect());
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2, 3, 4]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2, "the square and the hole");
    let mut points = local_points(d, id);
    points.sort_by(|a, b| width_of(a).total_cmp(&width_of(b)));
    assert_points("the hole", &points[0], &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]);
    assert_points("the square, without the vertices a straight side passes through", &points[1], &[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)]);
    assert_near("filled area", area(d, id), 8.0);
    assert!(!filled(d, id, (1.5, 1.5)) && filled(d, id, (0.5, 0.5)));
}

#[test]
fn outer_boundaries_run_anticlockwise_and_holes_clockwise() {
    let bars = [(0.0, 0.0, 3.0, 1.0), (0.0, 2.0, 3.0, 3.0), (0.0, 0.0, 1.0, 3.0), (2.0, 0.0, 3.0, 3.0)];
    let mut s = session_with(bars.iter().enumerate().map(|(i, b)| rect(u32::try_from(i + 1).unwrap(), *b).build()).collect());
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2, 3, 4]})));
    let d = doc(&s);
    let mut signed: Vec<f64> = built(d, id).iter().map(|(_, stroke, _)| kurbo::Shape::area(stroke)).collect();
    signed.sort_by(f64::total_cmp);
    assert!(signed[0] < 0.0 && signed[1] > 0.0, "a hole runs clockwise (negative) and the outer boundary anticlockwise: {signed:?}");
    assert_near("the outer boundary", signed[1], 9.0);
    assert_near("the hole", signed[0], -1.0);
}

#[test]
fn what_counts_as_area_is_what_a_nofill_zero_section_would_fill_even_with_no_fill_pattern() {
    // A has FillPattern 0 and an extra section with NoFill 1, the line from (1, 2) to (3, 2).
    let a = rect(1, (1.0, 1.0, 3.0, 3.0)).cell("FillPattern", constant(0.0)).section(&[(0.0, 1.0), (2.0, 1.0)], false, true).build();
    let mut s = session_with(vec![a, rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_near("The outline, so A's unfilled square counted", area(d, id), 7.0);
    assert_eq!(local(d, id, "FillPattern").and_then(|c| c.1).as_deref(), Some("0"), "A's formatting, so the produced shape is unfilled");
    assert_eq!(section_count(d, id), 1, "the NoFill 1 line is not carried");
}

#[test]
fn stacked_identical_shapes_unite_and_intersect_to_the_shape_itself() {
    for command in ["shape.union", "shape.intersect"] {
        let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (0.0, 0.0, 2.0, 2.0)).build()]);
        let id = made(&run(&mut s, command, json!({"ids": [1, 2]})));
        let d = doc(&s);
        assert_near(command, area(d, id), 4.0);
        assert_eq!(section_count(d, id), 1, "{command}: neither nothing nor twice");
    }
}

// ---- Combine -------------------------------------------------------------------------------------------------------

#[test]
fn combine_keeps_a_section_for_each_original_and_the_overlap_is_a_hole() {
    let mut s = two_squares();
    let result = run(&mut s, "shape.combine", json!({"ids": [1, 2]}));
    let id = made(&result);
    let d = doc(&s);
    assert_eq!(top(d), [id]);
    assert_eq!(section_count(d, id), 2);
    for (cell, want) in [("Width", 3.0), ("Height", 3.0), ("PinX", 2.5), ("PinY", 2.5)] {
        assert_near(cell, num(d, id, cell), want);
    }
    let points = local_points(d, id);
    assert_points("A's square in the new frame", &points[0], &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    assert_points("B's square in the new frame", &points[1], &[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)]);
    for ix in 0..2 {
        assert_eq!(row_types(d, id, ix), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
        assert_eq!(row_ixs(d, id, ix), [1, 2, 3, 4, 5], "the rows are numbered from 1, as a file's are");
        assert_eq!(flag_of(d, id, ix, "NoFill"), 0.0);
    }
    assert_near("filled area, with the overlap cut out", area(d, id), 6.0);
    assert!(!filled(d, id, (2.5, 2.5)) && filled(d, id, (1.5, 1.5)) && filled(d, id, (3.5, 3.5)));
    let text = d.shape(&at(d, id)).unwrap().text.clone().expect("the first shape's text");
    assert!(matches!(&text.parts[..], [diacraft_doc::TextPart::Chars(c)] if &**c == "A"));
}

#[test]
fn combine_keeps_open_paths_with_their_own_flags() {
    let open = line(1, (0.0, 0.0), (1.0, 1.0)).build();
    let mut s = session_with(vec![open, rect(2, (2.0, 0.0, 3.0, 1.0)).build()]);
    let id = made(&run(&mut s, "shape.combine", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2);
    assert_eq!(flag_of(d, id, 0, "NoFill"), 1.0, "the open path stays open and unfilled");
    assert_eq!(flag_of(d, id, 1, "NoFill"), 0.0);
    assert_eq!(row_types(d, id, 0), ["MoveTo", "LineTo"]);
}

// ---- Fragment --------------------------------------------------------------------------------------------------------

#[test]
fn fragment_of_two_overlapping_squares_makes_three_shapes_in_order() {
    let mut s = two_squares();
    let result = run(&mut s, "shape.fragment", json!({"ids": [1, 2]}));
    let made = ids(&result, "ids");
    assert_eq!(made.len(), 3);
    let d = doc(&s);
    assert_eq!(top(d), made, "the pieces are frontmost, the last listed in front");
    // B less A, A less B, the overlap: area and pin of each.
    for (id, area_want, pin) in [(made[0], 3.0, (3.0, 3.0)), (made[1], 3.0, (2.0, 2.0)), (made[2], 1.0, (2.5, 2.5))] {
        assert_near("area", area(d, id), area_want);
        assert_near("PinX", num(d, id, "PinX"), pin.0);
        assert_near("PinY", num(d, id, "PinY"), pin.1);
        assert!(d.shape(&at(d, id)).unwrap().text.is_none(), "no text");
    }
    assert_points("B less A", &page_points(d, made[0]), &[(3.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0), (2.0, 3.0), (3.0, 3.0)]);
    assert_points("A less B", &page_points(d, made[1]), &[(1.0, 1.0), (3.0, 1.0), (3.0, 2.0), (2.0, 2.0), (2.0, 3.0), (1.0, 3.0)]);
    assert_points("the overlap", &page_points(d, made[2]), &[(2.0, 2.0), (3.0, 2.0), (3.0, 3.0), (2.0, 3.0)]);
}

/// The vertices of the shape's first section on the page.
pub(crate) fn page_points(d: &Document, id: u32) -> Vec<Pt> {
    let map = pose_map(d, id);
    local_points(d, id)[0].iter().map(|p| map * Point::new(p.0, p.1)).map(|p| (p.x, p.y)).collect()
}

#[test]
fn fragment_with_a_cutting_line_cuts_the_region_and_leaves_nothing_of_the_line() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).build(), line(2, (2.0, -1.0), (2.0, 3.0)).build()]);
    let made = ids(&run(&mut s, "shape.fragment", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    assert_eq!(top(d), made, "nothing of the line remains");
    for id in &made {
        assert_near("area", area(d, *id), 4.0);
    }
    assert_near("the left half first", num(d, made[0], "PinX"), 1.0);
    assert_near("then the right", num(d, made[1], "PinX"), 3.0);
}

// ---- Intersect -------------------------------------------------------------------------------------------------------

#[test]
fn intersect_of_three_is_the_area_all_of_them_cover() {
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).text("A").build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
        rect(3, (2.5, 0.0, 5.0, 5.0)).build(),
    ]);
    let id = made(&run(&mut s, "shape.intersect", json!({"ids": [1, 2, 3]})));
    let d = doc(&s);
    for (cell, want) in [("Width", 0.5), ("Height", 1.0), ("PinX", 2.75), ("PinY", 2.5)] {
        assert_near(cell, num(d, id, cell), want);
    }
    assert_near("area", area(d, id), 0.5);
    assert!(d.shape(&at(d, id)).unwrap().text.is_some(), "A's text");
    assert_eq!(top(d), [id]);
}

#[test]
fn intersect_with_no_common_area_is_refused_and_changes_nothing() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 1.0, 1.0)).build(), rect(2, (2.0, 2.0, 3.0, 3.0)).build()]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.intersect", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("no area in common"), "{error}");
    assert_unchanged(&s, &before, "V11");
    // Shapes that only touch along an edge leave no area either.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 1.0, 1.0)).build(), rect(2, (1.0, 0.0, 2.0, 1.0)).build()]);
    let before = snapshot(&s);
    error_of(&mut s, "shape.intersect", &json!({"ids": [1, 2]}));
    assert_unchanged(&s, &before, "E13");
}

// ---- Subtract ------------------------------------------------------------------------------------------------------

#[test]
fn subtract_changes_the_first_shape_in_place_and_deletes_the_others() {
    let mut s = session_with(vec![
        rect(1, (0.0, 0.0, 4.0, 4.0)).text("A").build(),
        rect(2, (1.0, 1.0, 2.0, 2.0)).build(),
        rect(3, (3.0, 3.0, 5.0, 5.0)).build(),
    ]);
    let result = run(&mut s, "shape.subtract", json!({"ids": [1, 2, 3]}));
    assert_eq!(ids(&result, "deleted"), [2, 3]);
    assert_eq!(result["kept"], json!(1));
    let d = doc(&s);
    assert_eq!(top(d), [1], "A keeps its id and its place; B and C are deleted");
    assert!(d.shape(&at(d, 1)).unwrap().text.is_some(), "A keeps its text");
    assert_eq!(section_count(d, 1), 2, "the outline and the hole");
    let mut points = local_points(d, 1);
    points.sort_by_key(|p| p.len());
    assert_points("the hole", &points[0], &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]);
    assert_points("the outline", &points[1], &[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (3.0, 3.0), (3.0, 4.0), (0.0, 4.0)]);
    assert_near("area", area(d, 1), 14.0);
    for (cell, want) in [("Width", 4.0), ("Height", 4.0), ("PinX", 2.0), ("PinY", 2.0)] {
        assert_near(cell, num(d, 1, cell), want);
    }
    assert!(!filled(d, 1, (1.5, 1.5)) && !filled(d, 1, (3.5, 3.5)) && filled(d, 1, (0.5, 0.5)));
}

#[test]
fn subtract_goes_by_the_first_shape_named_so_a_shape_wholly_inside_another_leaves_nothing() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 4.0, 4.0)).build(), rect(2, (1.0, 1.0, 2.0, 2.0)).build(), rect(3, (3.0, 3.0, 5.0, 5.0)).build()]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.subtract", &json!({"ids": [2, 1, 3]}));
    assert!(error.to_string().contains("nothing of shape 2 is left"), "{error}");
    assert_unchanged(&s, &before, "V13");
}

#[test]
fn subtract_refits_the_box_of_the_first_shape_unless_its_size_is_locked() {
    let make = |lock: bool| {
        let mut a = rect(1, (0.0, 0.0, 4.0, 2.0)).cell("LocPinX", Cell::with_formula("Width*0.5", Some(Value::from_text("2"))).with_unit("IN"));
        if lock {
            a = a.cell("LockCalcWH", constant(1.0));
        }
        session_with(vec![a.build(), rect(2, (2.0, -1.0, 5.0, 3.0)).build()])
    };
    let mut s = make(false);
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let d = doc(&s);
    for (cell, want) in [("Width", 2.0), ("Height", 2.0), ("PinX", 1.0), ("PinY", 1.0), ("LocPinX", 1.0)] {
        assert_near(cell, num(d, 1, cell), want);
    }
    assert_points("the geometry", &local_points(d, 1)[0], &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    assert_near("nothing moved on the page", area(d, 1), 4.0);
    assert!(filled(d, 1, (1.0, 1.0)) && !filled(d, 1, (3.0, 1.0)));
    // With LockCalcWH the frame stays and the geometry is written in it.
    let mut s = make(true);
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let d = doc(&s);
    for (cell, want) in [("Width", 4.0), ("Height", 2.0), ("PinX", 2.0), ("PinY", 1.0)] {
        assert_near(cell, num(d, 1, cell), want);
    }
    assert_points("the geometry in the old frame", &local_points(d, 1)[0], &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
}

#[test]
fn subtract_keeps_the_sections_that_do_not_take_the_fill_and_the_frame_they_are_written_in() {
    // A (0, 0)-(4, 2) has a line along its middle with NoFill 1, and a hidden outline (NoShow 1); B takes its right half away.
    let a = rect(1, (0.0, 0.0, 4.0, 2.0))
        .section(&[(0.0, 1.0), (4.0, 1.0)], false, true)
        .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0)], true, false)
        .build();
    let mut a = a;
    // The third section is the hidden one.
    let sheet = Arc::make_mut(&mut a.sheet);
    let hidden = Arc::make_mut(sheet.sections.get_mut(2).unwrap());
    hidden.cells.insert(CellName::new("NoShow"), constant(1.0));
    let mut s = session_with(vec![a, rect(2, (2.0, -1.0, 5.0, 3.0)).build()]);
    let result = run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    assert_eq!(result["kept"], json!(1));
    let d = doc(&s);
    // The line and the hidden outline are as they were, in the same frame, and the fill is the left half, in a new section.
    assert_eq!(flag_of(d, 1, 1, "NoFill"), 1.0);
    assert_eq!(row_types(d, 1, 1), ["MoveTo", "LineTo"]);
    assert_eq!(flag_of(d, 1, 2, "NoShow"), 1.0);
    assert_eq!(section_count(d, 1), 3, "the replaced section's place is taken by the new one");
    for (cell, want) in [("Width", 4.0), ("Height", 2.0), ("PinX", 2.0), ("PinY", 1.0)] {
        assert_near(cell, num(d, 1, cell), want);
    }
    assert_near("the fill", area(d, 1), 4.0);
    assert!(filled(d, 1, (1.0, 0.5)) && !filled(d, 1, (3.0, 1.5)));
    assert_points("the line is where it was", &local_points(d, 1).iter().find(|p| p.len() == 2).unwrap().clone(), &[(0.0, 1.0), (4.0, 1.0)]);
}

/// An open section with `NoFill` 0 has a fill ring (a line there and back) that covers no area, so it is a section "with no fill area" and is kept as R27 says of the open
/// ones, and a first shape that keeps a section keeps its frame: the box (0, 0)-(4, 4) with the line (0.5, 0.5)-(0.5, 3.5) in it, less a rectangle that takes the right
/// of it away, is the left half with the line still there, in the frame of 4 by 4.
#[test]
fn subtract_keeps_an_open_section_that_takes_the_fill_but_covers_no_area_and_so_keeps_the_frame() {
    let a = rect(1, (0.0, 0.0, 4.0, 4.0)).section(&[(0.5, 0.5), (0.5, 3.5)], false, false).build();
    let mut s = session_with(vec![a, rect(2, (2.0, -1.0, 5.0, 5.0)).build()]);
    let result = run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    assert_eq!(result["kept"], json!(1));
    let d = doc(&s);
    assert_eq!(section_count(d, 1), 2, "the line is kept and the box is replaced by the left half");
    let points = local_points(d, 1);
    assert_points("the line is where it was", points.iter().find(|p| p.len() == 2).expect("the line"), &[(0.5, 0.5), (0.5, 3.5)]);
    assert_points("the left half", points.iter().find(|p| p.len() == 4).expect("the half"), &[(0.0, 0.0), (2.0, 0.0), (2.0, 4.0), (0.0, 4.0)]);
    for (cell, want) in [("Width", 4.0), ("Height", 4.0), ("PinX", 2.0), ("PinY", 2.0)] {
        assert_near(cell, num(d, 1, cell), want);
    }
    assert_near("the fill", area(d, 1), 8.0);
    assert!(filled(d, 1, (1.0, 2.0)) && !filled(d, 1, (3.0, 2.0)));
}

#[test]
fn a_first_shape_in_front_of_the_others_keeps_its_place_and_its_id_when_they_go_from_behind_it() {
    let mut s =
        session_with(vec![rect(2, (1.0, 1.0, 2.0, 2.0)).build(), rect(3, (6.0, 6.0, 7.0, 7.0)).build(), rect(1, (0.0, 0.0, 4.0, 4.0)).build()]);
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let d = doc(&s);
    assert_eq!(top(d), [3, 1], "the first shape is the front one, and keeps its place there");
    assert_near("area", area(d, 1), 15.0);
}

#[test]
fn subtract_inside_a_group_changes_the_member_in_the_groups_frame() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 4.0, 4.0)).build(), rect(2, (2.0, -1.0, 5.0, 3.0)).build(), rect(3, (8.0, 8.0, 9.0, 9.0)).build()]);
    let result = run(&mut s, "shape.group", json!({"ids": [1, 2, 3]}));
    let group = u32::try_from(result["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let d = doc(&s);
    let members: Vec<u32> = d.shape(&at(d, group)).unwrap().children.iter().map(|m| m.id).collect();
    assert_eq!(members, [1, 3], "B is gone and A stays where it was in the group");
    // A is now the left half and the top, an L: its area is 4 * 4 less the 2 by 3 B covered inside it.
    assert_near("area", area(d, 1), 16.0 - 2.0 * 3.0);
    assert_eq!(d.shape(&at(d, group)).unwrap().kind, ShapeType::Group);
}
