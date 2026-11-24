//! A shape's `Geometry` sections, read from its resolved sheet and built into paths by `diacraft_geom`.
//!
//! The sections and rows are the shape's effective ones (the master's rows and the shape's own, merged cell by cell),
//! each cell read as saved. `geom` takes numbers, so this module is the one place a row's cells become a [`GeomRow`].

use diacraft_doc::{CellAddr, CellName, EffectiveRow, Resolved, Resolver, SheetRef};
use diacraft_geom::kurbo::{BezPath, Size};
use diacraft_geom::{BuiltPaths, GeomRow, Nurbs, Polyline, SectionFlags, build_paths};

use crate::limits::MAX_GEOMETRY_SECTIONS;
use crate::read::Cx;
use crate::skips::SkipKind;

/// The paths of one `Geometry` section, in the shape's own frame (drawing units, x right, y up).
pub(crate) struct SectionPaths {
    /// The section's `IX`.
    pub ix: Option<u32>,
    pub flags: SectionFlags,
    pub stroke: BezPath,
    pub fill: BezPath,
}

/// The paths of a shape.
#[derive(Default)]
pub(crate) struct ShapeGeometry {
    pub sections: Vec<SectionPaths>,
    /// Rows built, for the frame's budget.
    pub rows: usize,
    /// Whether the shape has a `Geometry` section at all (a section of no rows draws nothing but still counts as a path).
    pub has_geometry: bool,
}

/// The cells of one row, by name.
struct RowCells<'r, 'a> {
    cells: &'r [(CellName, Resolved<'a>)],
}

impl<'a> RowCells<'_, 'a> {
    fn found(&self, name: &str) -> Option<&Resolved<'a>> {
        self.cells.iter().find(|(n, _)| n.as_str() == name).map(|(_, r)| r)
    }

    /// The number a cell holds as saved, 0 for a cell the row lacks. A value that is not a number reads 0, as a missing one does.
    fn num(&self, name: &str) -> f64 {
        self.found(name).and_then(|r| r.value.cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(0.0)
    }

    /// The text of a cell, for the constructor cells.
    fn text(&self, name: &str) -> Option<&str> {
        self.found(name).and_then(|r| r.value.cell).and_then(|c| c.value.as_ref()).map(|v| v.text())
    }
}

/// One row as `geom` takes it, or `None` for a row type no spec lists.
fn geom_row(kind: &str, c: &RowCells<'_, '_>) -> Option<GeomRow> {
    let (x, y) = (c.num("X"), c.num("Y"));
    let (a, b, cc, d) = (c.num("A"), c.num("B"), c.num("C"), c.num("D"));
    Some(match kind {
        "MoveTo" => GeomRow::MoveTo { x, y },
        "RelMoveTo" => GeomRow::RelMoveTo { x, y },
        "LineTo" => GeomRow::LineTo { x, y },
        "RelLineTo" => GeomRow::RelLineTo { x, y },
        "ArcTo" => GeomRow::ArcTo { x, y, bow: a },
        "EllipticalArcTo" => GeomRow::EllipticalArcTo { x, y, mid_x: a, mid_y: b, angle: cc, ratio: d },
        "RelEllipticalArcTo" => GeomRow::RelEllipticalArcTo { x, y, mid_x: a, mid_y: b, angle: cc, ratio: d },
        "PolylineTo" => GeomRow::PolylineTo { x, y, polyline: c.text("A").and_then(|t| Polyline::from_text(t).ok()) },
        "NURBSTo" => GeomRow::NurbsTo {
            x,
            y,
            second_last_knot: a,
            last_weight: b,
            first_knot: cc,
            first_weight: d,
            nurbs: c.text("E").and_then(|t| Nurbs::from_text(t).ok()),
        },
        "SplineStart" => GeomRow::SplineStart { x, y, second_knot: a, first_knot: b, last_knot: cc, degree: d },
        "SplineKnot" => GeomRow::SplineKnot { x, y, knot: a },
        "RelCubBezTo" => GeomRow::RelCubBezTo { x, y, c1_x: a, c1_y: b, c2_x: cc, c2_y: d },
        "RelQuadBezTo" => GeomRow::RelQuadBezTo { x, y, c_x: a, c_y: b },
        "Ellipse" => GeomRow::Ellipse { x, y, first_x: a, first_y: b, second_x: cc, second_y: d },
        "InfiniteLine" => GeomRow::InfiniteLine { x, y, other_x: a, other_y: b },
        _ => return None,
    })
}

/// The rows of a section, as `geom` rows, and how many were left out: a row of a type nobody lists. For a caller outside a frame (the engine's snapping
/// builds a shape's paths from the rows it resolved); [`build_paths`](diacraft_geom::build_paths) takes the rows.
pub fn geom_rows(rows: &[EffectiveRow<'_>]) -> (Vec<GeomRow>, usize) {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let cells = RowCells { cells: &row.cells };
        if let Some(g) = row.row_type.as_deref().and_then(|kind| geom_row(kind, &cells)) {
            out.push(g);
        }
    }
    let skipped = rows.len().saturating_sub(out.len());
    (out, skipped)
}

/// The rows of a section, as `geom` rows. A row of a type nobody lists is counted and left out.
fn section_rows(cx: &Cx<'_, '_>, rows: &[EffectiveRow<'_>], who: &dyn Fn() -> String) -> Vec<GeomRow> {
    let (out, skipped) = geom_rows(rows);
    if skipped > 0 {
        cx.note_n(SkipKind::GeometryRow, skipped, || format!("{}: a row with no known type", who()));
    }
    out
}

/// The geometry of the shape at `at`, for a frame that may build `rows_left` more rows. `size` is `Width` and `Height`, which the relative
/// rows need. Returns what was built, and `None` for the paths when the budget does not allow them.
pub(crate) fn build(cx: &Cx<'_, '_>, at: &SheetRef, size: Size, rows_left: usize, who: &dyn Fn() -> String) -> Option<ShapeGeometry> {
    let res: &Resolver<'_> = &cx.res;
    let keys = res.effective_sections(at, "Geometry")?;
    // Listing the sections of a sheet costs about 279 microseconds for 1,024 of them (the Task 4 review): each counts as a row.
    let mut out = ShapeGeometry { has_geometry: !keys.is_empty(), rows: keys.len().min(MAX_GEOMETRY_SECTIONS), ..ShapeGeometry::default() };
    if keys.len() > MAX_GEOMETRY_SECTIONS {
        cx.note_n(SkipKind::SectionsCut, keys.len() - MAX_GEOMETRY_SECTIONS, who);
    }
    for key in keys.iter().take(MAX_GEOMETRY_SECTIONS) {
        let Some(rows) = res.effective_rows(at, key) else {
            continue;
        };
        out.rows = out.rows.saturating_add(rows.len());
        if out.rows > rows_left {
            return None;
        }
        let geom_rows = section_rows(cx, &rows, who);
        let flags = SectionFlags {
            no_fill: flag(cx, at, key.ix, "NoFill"),
            no_line: flag(cx, at, key.ix, "NoLine"),
            no_show: flag(cx, at, key.ix, "NoShow"),
        };
        match build_paths(&geom_rows, size) {
            Ok(built) => {
                note_warnings(&built, cx, who);
                let BuiltPaths { stroke, fill, infinite_lines, .. } = built;
                if !infinite_lines.is_empty() {
                    cx.note_n(SkipKind::InfiniteLine, infinite_lines.len(), who);
                }
                out.sections.push(SectionPaths { ix: key.ix, flags, stroke, fill });
            }
            Err(error) => {
                cx.note(SkipKind::GeometryFailed, || format!("{}: {error}", who()));
            }
        }
    }
    Some(out)
}

fn flag(cx: &Cx<'_, '_>, at: &SheetRef, ix: Option<u32>, name: &str) -> bool {
    cx.flag_at(at, &CellAddr::in_section("Geometry", ix, name), false)
}

fn note_warnings(built: &BuiltPaths, cx: &Cx<'_, '_>, who: &dyn Fn() -> String) {
    if built.warnings.is_empty() {
        return;
    }
    cx.note_n(SkipKind::GeometryRow, built.warnings.len(), || {
        let first = built.warnings.first().map(|w| w.to_string()).unwrap_or_default();
        format!("{}: {first}", who())
    });
}
