//! The two ends of a straight edge as the cells of its rows say them (design §4.1).
//!
//! A new point on a straight edge is written as a share of the box where both ends of the edge are shares, as a plain length where both are plain lengths, and as a formula over
//! the cells of the two ends otherwise. What each end is comes from the `Geometry` row that drew the edge (its end) and from the row before that set the pen (its start), read
//! through the master where inherited: a `LineTo`, an `ArcTo` of bow 0 or a `RelLineTo` gives its own `X` and `Y`, and a piece of a `PolylineTo` gives a vertex of its constructor
//! (the row's own end for the last piece). Any other row gives no ends and the point is written as the plain length of the place it stands at, as is a point whose cells
//! disagree with the place of the edge as drawn (the caller compares them).

use std::sync::Arc;

use diacraft_doc::{SectionKey, ShapeRef, SheetRef};
use diacraft_geom::{GeomRow, Polyline};

use super::Skip;
use super::edges::{Edge, MAX_OUTLINE_ROWS, OutlineWork, cell_number, formula_text, row_number};
use super::layout::EndAxis;
use crate::draw::Reader;

/// What a `Geometry` row says of a cell: its formula text, if it has one, and its value.
#[derive(Clone, Debug)]
struct Cellish {
    formula: Option<String>,
    value: f64,
}

/// One row of a section.
#[derive(Clone, Debug)]
struct RowFacts {
    ix: u32,
    kind: Arc<str>,
    /// A relative row: its `X` and `Y` are shares of the box.
    relative: bool,
    x: Cellish,
    y: Cellish,
    polyline: Option<Polyline>,
}

/// The rows of one `Geometry` section, as the formulas of an edge's ends need them.
#[derive(Clone, Debug, Default)]
pub(super) struct Section {
    rows: Vec<RowFacts>,
}

/// A pair of ends: the `X` and then the `Y` of one place.
type Pair = [EndAxis; 2];

impl Section {
    /// Reads the effective rows of the section `ix` of the shape at `at`, charging them to the call's rows.
    pub(super) fn read(reader: &Reader<'_>, at: &ShapeRef, ix: Option<u32>, work: &mut OutlineWork) -> Result<Section, Skip> {
        let sheet = SheetRef::Shape(at.clone());
        let key = SectionKey::new("Geometry", ix);
        let res = reader.resolver();
        let held = res.rows_held(&sheet, &key).unwrap_or(0);
        if held > MAX_OUTLINE_ROWS {
            return Ok(Section::default());
        }
        work.charge_rows(held)?;
        let Some(Ok(rows)) = res.effective_rows_within(&sheet, &key, held) else { return Ok(Section::default()) };
        let mut out = Vec::with_capacity(rows.len());
        for (position, row) in rows.iter().enumerate() {
            let kind: Arc<str> = row.row_type.clone().unwrap_or_else(|| Arc::from(""));
            let cell = |name: &str| Cellish { formula: formula_text(row, name).map(str::to_owned), value: cell_number(row, name) };
            let polyline = if &*kind == "PolylineTo" {
                match diacraft_render::geom_rows(std::slice::from_ref(row)).0.into_iter().next() {
                    Some(GeomRow::PolylineTo { polyline, .. }) => polyline,
                    _ => None,
                }
            } else {
                None
            };
            out.push(RowFacts { ix: row_number(row, position), relative: kind.starts_with("Rel"), kind, x: cell("X"), y: cell("Y"), polyline });
        }
        Ok(Section { rows: out })
    }

    fn row(&self, ix: u32) -> Option<&RowFacts> {
        self.rows.iter().find(|r| r.ix == ix)
    }

    /// The ends of the cells of `row` as a place: `Geometry<section>.X<ix>` and `Y`.
    fn cells_of(row: &RowFacts, section: u32, (width, height): (f64, f64)) -> Pair {
        let named = |axis: &str| format!("Geometry{}.{axis}{}", u64::from(section) + 1, row.ix);
        [
            EndAxis::of_cell(row.x.formula.as_deref(), row.x.value, row.relative, "Width", width, named("X")),
            EndAxis::of_cell(row.y.formula.as_deref(), row.y.value, row.relative, "Height", height, named("Y")),
        ]
    }

    /// The place a row leaves the pen at, as ends: only a row that ends at its own `X` and `Y` does.
    fn pen_of(row: &RowFacts, section: u32, size: (f64, f64)) -> Option<Pair> {
        let sets_the_pen = matches!(
            &*row.kind,
            "MoveTo"
                | "RelMoveTo"
                | "LineTo"
                | "RelLineTo"
                | "ArcTo"
                | "EllipticalArcTo"
                | "RelEllipticalArcTo"
                | "PolylineTo"
                | "NURBSTo"
                | "RelCubBezTo"
                | "RelQuadBezTo"
        );
        sets_the_pen.then(|| Section::cells_of(row, section, size))
    }

    /// A vertex of a row's `POLYLINE` constructor as ends.
    fn vertex(poly: &Polyline, k: usize, (width, height): (f64, f64)) -> Option<Pair> {
        let (x, y) = *poly.points.get(k)?;
        Some([EndAxis::of_number(x, poly.x_is_share, width), EndAxis::of_number(y, poly.y_is_share, height)])
    }

    /// The start and the end of the straight edge `edge` of the section `section` (its `IX`) as ends of cells, for a shape of size `size`: `None` for an edge drawn by a row this
    /// does not read, which is then written as a plain length.
    pub(super) fn ends(&self, edge: &Edge, section: u32, size: (f64, f64)) -> Option<(Pair, Pair)> {
        let row = self.row(edge.rows.1)?;
        let k = usize::try_from(edge.step).ok()?;
        match &*row.kind {
            "LineTo" | "ArcTo" | "RelLineTo" => {
                let pen = self.row(edge.pen_row?).and_then(|r| Section::pen_of(r, section, size))?;
                Some((pen, Section::cells_of(row, section, size)))
            }
            "PolylineTo" => {
                let poly = row.polyline.as_ref()?;
                let start = match k.checked_sub(1) {
                    None => self.row(edge.pen_row?).and_then(|r| Section::pen_of(r, section, size))?,
                    Some(previous) => Section::vertex(poly, previous, size)?,
                };
                let end = if k < poly.points.len() { Section::vertex(poly, k, size)? } else { Section::cells_of(row, section, size) };
                Some((start, end))
            }
            _ => None,
        }
    }
}
