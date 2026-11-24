//! The functions that walk a path and measure a shape's box: `PATHLENGTH`, `POINTALONGPATH`, `ANGLEALONGPATH`,
//! `NEARESTPOINTONPATH`, `DISTTOPATH`, `PATHSEGMENT`, `SEGMENTCOUNT`, `BOUNDINGBOXRECT` and `BOUNDINGBOXDIST`.
//!
//! **The path** is the rows of the `Geometry` section the first argument names (`Geometry1.Path` is the section with `IX` 0),
//! each cell read as the evaluator reads it, drawn one row at a time by `diacraft_geom::build_row_draws`, and walked by `pathwalk.rs`: its
//! lengths, `Rounding` as a chord across each corner of straight rows, and a share of the length along. The first argument is
//! read as a name and nothing else of it is evaluated; `IF` may choose it, and only the branch taken is looked at.
//!
//! **Frames.** Every number in, and every point out, is in the frame of the shape that holds the path: a turn or a flip of the
//! shape changes nothing here. A point comes out tagged with that shape, so that `PAR` and `LOC` carry it from there.
//!
//! **What reads what.** These functions read every cell of the section, `Width`, `Height` and `Rounding`, none of which the
//! formula names, so a formula that calls one is worked out after every edit (`facts.rs`, `DYNAMIC_FUNCTIONS`).
//!
//! **Cost.** A call reads the section's rows once (a step a cell), draws them (a step for each step drawn, twice, and for the splines the
//! multiplications they spend at 512 to a step) and walks them. A section of more than `diacraft_geom::MAX_ROWS` rows, or that draws more than
//! [`MAX_DRAWN_STEPS`] steps, or more than the budget has steps for, is `LOOP_ERROR`. The draws are memory held (a step and a row of steps, as
//! accounted) while the walk is built from them, and `Rounding` is read before they are made, so that a `Rounding` that calls a path function
//! of another shape does not find this section's draws held beside that shape's.
//!
//! **Project rules the spec leaves open**, each named where it is used: a travel a rounding error outside 0 to 1 is clamped ([`TRAVEL_SLACK`]),
//! one further out is `#VALUE!`; a first argument that is not a `Geometry` section is `#VALUE!`; a path with no length is `#REF!`
//! except for `PATHLENGTH`, which is 0.

use std::sync::Arc;

use diacraft_doc::{CellAddr, Member, RowKey, ShapeRef, SheetRef};
use diacraft_formula::Expr;
use diacraft_geom::kurbo::{Point, Rect, Size, Vec2};
use diacraft_geom::{Bound, DrawStep, GeomError, GeomRow, MAX_ROWS, Nurbs, Polyline, RowDraw, build_row_draws};
use diacraft_units::{Quantity, Unit};

use crate::cells::{CellEval, CellKey, THEMED_MARKER};
use crate::convert::signed_int;
use crate::eval::{Evaluator, Site};
use crate::fn_geom::point_value;
use crate::pathwalk::Walk;
use crate::registry::{FnDef, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR};
use crate::value::{CellError, ConstructorKind, Value};

/// How far outside 0 to 1 a `travel` may fall and still be read as the end it is next to: `NEARESTPOINTONPATH` feeding `POINTALONGPATH` can
/// come out a rounding error past 1. **Project rule** (gives `#VALUE!` out of range and no file holds one): 1E-9.
pub const TRAVEL_SLACK: f64 = 1e-9;

/// The multiplications of spline work that one step of the budget pays for: a step is about a microsecond and a multiplication a
/// nanosecond or two.
const MULTIPLICATIONS_PER_STEP: u64 = 512;

/// What a row held in memory costs, as accounted, while a section is read: the row and the cells it was made from.
const ROW_BYTES: u64 = 256;

/// What the draws of a section take once they are made: a step each and a row's `Vec` header and counts each, as the geometry crate lays them
/// out (a `DrawStep` is three points and a tag).
const DRAW_STEP_BYTES: u64 = std::mem::size_of::<DrawStep>() as u64;
const DRAW_ROW_BYTES: u64 = std::mem::size_of::<RowDraw>() as u64;

/// The most steps (lines and curves) one section may draw, and the most multiplications its splines may spend. **Project rules:** the most rows
/// in files is 31,273 and the most points in a spline 773, so these are far past a real path. They bound what a hostile section holds
/// while one function works: about 250 bytes a step, the draws, the stroke and the fill beside them in `diacraft_geom` and the pieces of the
/// walk, so 125 MB at most. Only the draws are accounted (`with_draws`, 56 bytes a step, 28 MB at most); the rest is transient inside the
/// geometry crate or the walk, and nothing is read while it lives.
const MAX_DRAWN_STEPS: usize = 500_000;
const MAX_SPLINE_WORK: u64 = 50_000_000;

/// How many `IF` choices a first argument may nest.
const MAX_CHOICES: usize = 32;

/// The deepest group a box looks into, and the most shapes one box visits: a hostile file holds a chain or a tree of any size.
const MAX_BOX_DEPTH: usize = 32;

/// How many levels of `MAX_NATIVE_DEPTH` a level of group in a box counts for: a box of a group calls the box of each member from inside its own
/// (a frame that holds the list of members and the walks of the sections), and reads cells from each, which count `CELL_HOP_DEPTH` apiece. At 4,
/// the 32 levels a box looks into are 128 of the 160, and the cell reads of the deepest add their own.
const BOX_LEVEL_DEPTH: usize = 4;
const MAX_BOX_SHAPES: usize = 20_000;

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

fn missing() -> Value {
    Value::Error(CellError::Ref)
}

/// The number a cell's value holds, an error or an unsupported value being handed back (a cell that fails reads its last good value in
/// `view`, so this is an error only where there is none).
fn number_of(v: &Value) -> Result<f64, Value> {
    match v {
        Value::Number(q) => Ok(q.stored),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Error(_) | Value::Unsupported(_) => Err(v.clone()),
        _ => Ok(0.0),
    }
}

/// The numbers of the `POLYLINE` or `NURBS` constructor a cell holds, borrowed from the value (nothing is copied but into the row that is made).
fn constructor_numbers(v: &Value, kind: ConstructorKind) -> Option<&[f64]> {
    match v {
        Value::Constructor(c) if c.kind == kind => Some(&c.numbers),
        _ => None,
    }
}

/// The constructor a cell holds, as the geometry crate reads it: its evaluated call, or the text a file saved.
fn polyline_of(v: &Value) -> Option<Polyline> {
    match v {
        Value::Text(t) => Polyline::from_text(t).ok(),
        other => constructor_numbers(other, ConstructorKind::Polyline).and_then(|n| Polyline::from_args(n).ok()),
    }
}

fn nurbs_of(v: &Value) -> Option<Nurbs> {
    match v {
        Value::Text(t) => Nurbs::from_text(t).ok(),
        other => constructor_numbers(other, ConstructorKind::Nurbs).and_then(|n| Nurbs::from_args(n).ok()),
    }
}

/// The bytes a constructor cell's value holds: its numbers, or its text.
fn constructor_bytes(v: &Value) -> usize {
    match v {
        Value::Constructor(c) => c.numbers.len().saturating_mul(8),
        Value::Text(t) => t.len(),
        _ => 0,
    }
}

impl Evaluator<'_, '_> {
    /// The section a first argument names: the sheet that holds it and the section's `IX` (`Geometry1.Path` is `IX` 0). `IF` may choose
    /// between two references and only the branch taken is looked at.
    fn section_arg(&mut self, site: &Site<'_>, i: usize) -> Result<(SheetRef, u32), Value> {
        let Some(mut expr) = site.arg(i) else { return Err(bad()) };
        for _ in 0..MAX_CHOICES {
            match expr {
                Expr::Ref(r) => {
                    let (sheet, addr) = self.locate(r, site.env)?;
                    return match addr.section {
                        Some(s) if s.name.as_str() == "Geometry" => s.ix.map(|ix| (sheet, ix)).ok_or_else(bad),
                        _ => Err(bad()),
                    };
                }
                Expr::Call(c) if c.qualifiers.is_empty() && c.name.eq_ignore_ascii_case("IF") && (2..=3).contains(&c.args.len()) => {
                    let Some(Some(condition)) = c.args.first() else { return Err(bad()) };
                    let v = self.eval(condition, site.env);
                    let taken = self.read_bool(&v, site.env)?;
                    expr = match c.args.get(if taken { 1 } else { 2 }) {
                        Some(Some(branch)) => branch,
                        _ => return Err(bad()),
                    };
                }
                other => {
                    let v = self.eval(other, site.env);
                    return Err(if v.is_failure() { v } else { bad() });
                }
            }
        }
        Err(Value::Error(LOOP_ERROR))
    }

    /// Argument `i` as a number, or `None` for an empty slot or one past the end.
    fn number_arg(&mut self, site: &Site<'_>, i: usize) -> Result<Option<f64>, Value> {
        let Some(e) = site.arg(i) else { return Ok(None) };
        let v = self.eval(e, site.env);
        if v.is_failure() {
            return Err(v);
        }
        let q = self.read_double(&v, site.env)?;
        if q.stored.is_finite() { Ok(Some(q.stored)) } else { Err(bad()) }
    }

    fn required_number(&mut self, site: &Site<'_>, i: usize) -> Result<f64, Value> {
        self.number_arg(site, i)?.ok_or_else(bad)
    }

    /// `travel`: a share of the length from 0 to 1 (out of range is `#VALUE!`, with [`TRAVEL_SLACK`] for rounding).
    fn travel_arg(&mut self, site: &Site<'_>, i: usize) -> Result<f64, Value> {
        let t = self.required_number(site, i)?;
        if !(-TRAVEL_SLACK..=1.0 + TRAVEL_SLACK).contains(&t) {
            return Err(bad());
        }
        Ok(t.clamp(0.0, 1.0))
    }

    /// `segment`: counted from 1, cut toward zero; below 1 reads as no segment (one stock formula passes one half and no error comes
    /// back).
    fn segment_arg(&mut self, site: &Site<'_>, i: usize) -> Result<Option<u32>, Value> {
        let Some(s) = self.number_arg(site, i)? else { return Ok(None) };
        let s = s.trunc();
        if s < 1.0 { Ok(None) } else { Ok(Some(if s >= f64::from(u32::MAX) { u32::MAX } else { s as u32 })) }
    }

    /// A cell read for what it shows. A cell saved as `Themed` has no value to read, and as an operand it is
    /// `Unsupported`; a function that works on the cell's number asks the cell's own formula (`THEMEVAL()`) what the theme gives, as the renderer
    /// does, and says of `Rounding`: our reasoning reads the marker through the theme's entry. A theme that cannot answer leaves the
    /// marker, and the function is `Unsupported`.
    ///
    /// The cell is in progress while its own formula is worked out, as any cell is, so a formula that reads the cell it is for (a `Rounding` that
    /// walks its own section) meets a cycle at the second read and not another level of the same walk, to the depth limit.
    pub(crate) fn read_themed(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Arc<CellEval> {
        let read = self.read_cell(sheet, addr);
        if matches!(&read.view, Value::Unsupported(name) if &**name == THEMED_MARKER) {
            let key: CellKey = (sheet.clone(), addr.clone());
            if self.ctx.in_progress.contains(&key) {
                self.ctx.stats.cycles += 1;
                return Arc::new(CellEval::looped(LOOP_ERROR));
            }
            self.ctx.in_progress.insert(key.clone());
            let own = self.compute_cell(sheet, addr, true);
            self.ctx.in_progress.remove(&key);
            if !own.raw.is_failure() {
                return Arc::new(own);
            }
        }
        read
    }

    /// A cell of the sheet as a number.
    fn plain_number(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Result<f64, Value> {
        let read = self.read_themed(sheet, addr);
        number_of(&read.view)
    }

    fn row_cell(&mut self, sheet: &SheetRef, ix: u32, key: &RowKey, cell: &str) -> Arc<CellEval> {
        self.read_cell(sheet, &CellAddr::in_row("Geometry", Some(ix), key.clone(), cell))
    }

    fn row_number(&mut self, sheet: &SheetRef, ix: u32, key: &RowKey, cell: &str) -> Result<f64, Value> {
        number_of(&self.row_cell(sheet, ix, key, cell).view)
    }

    /// What the section `ix` of `sheet` draws, row by row (`#REF!` for a section that is not there).
    fn geometry_draws(&mut self, sheet: &SheetRef, ix: u32) -> Result<Vec<RowDraw>, Value> {
        let Some(keys) = self.res.effective_sections(sheet, "Geometry") else { return Err(missing()) };
        let Some(key) = keys.into_iter().find(|k| k.ix == Some(ix)) else { return Err(missing()) };
        let Some(rows) = self.res.effective_rows(sheet, &key) else { return Err(missing()) };
        if rows.len() > MAX_ROWS || !self.steps(rows.len().saturating_add(1)) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let held = (rows.len() as u64).saturating_mul(ROW_BYTES);
        if !self.ctx.hold_bytes(held) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let mut constructors = 0u64;
        let out = self.draw_rows(sheet, ix, &rows, &mut constructors);
        self.ctx.release_bytes(held.saturating_add(constructors));
        out
    }

    /// Charges the constructor a row's cell holds before it is read into the row (a step for each 64 bytes: copying and parsing it costs its
    /// length, and a section of rows that each read one big constructor is that many times its size), and holds what the row keeps of it, as many
    /// bytes again as it has, until the section is drawn. `held` adds up what was held, for the caller to let go.
    fn take_constructor(&mut self, v: &Value, held: &mut u64) -> Result<(), Value> {
        let bytes = constructor_bytes(v);
        if bytes == 0 {
            return Ok(());
        }
        let keep = (bytes as u64).saturating_mul(2);
        if !self.work(bytes) || !self.ctx.hold_bytes(keep) {
            return Err(Value::Error(LOOP_ERROR));
        }
        *held = held.saturating_add(keep);
        Ok(())
    }

    fn draw_rows(
        &mut self,
        sheet: &SheetRef,
        ix: u32,
        rows: &[diacraft_doc::EffectiveRow<'_>],
        constructors: &mut u64,
    ) -> Result<Vec<RowDraw>, Value> {
        let size = Size::new(self.plain_number(sheet, &CellAddr::direct("Width"))?, self.plain_number(sheet, &CellAddr::direct("Height"))?);
        let mut geom: Vec<GeomRow> = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(kind) = row.row_type.as_deref() else { continue };
            let key = &row.key;
            let (x, y) = (self.row_number(sheet, ix, key, "X")?, self.row_number(sheet, ix, key, "Y")?);
            let abcd = |ev: &mut Self, names: &[&str]| -> Result<[f64; 4], Value> {
                let mut out = [0.0; 4];
                for (slot, name) in out.iter_mut().zip(names) {
                    *slot = ev.row_number(sheet, ix, key, name)?;
                }
                Ok(out)
            };
            let g = match kind {
                "MoveTo" => GeomRow::MoveTo { x, y },
                "RelMoveTo" => GeomRow::RelMoveTo { x, y },
                "LineTo" => GeomRow::LineTo { x, y },
                "RelLineTo" => GeomRow::RelLineTo { x, y },
                "ArcTo" => GeomRow::ArcTo { x, y, bow: abcd(self, &["A"])?[0] },
                "EllipticalArcTo" | "RelEllipticalArcTo" => {
                    let [a, b, c, d] = abcd(self, &["A", "B", "C", "D"])?;
                    if kind == "EllipticalArcTo" {
                        GeomRow::EllipticalArcTo { x, y, mid_x: a, mid_y: b, angle: c, ratio: d }
                    } else {
                        GeomRow::RelEllipticalArcTo { x, y, mid_x: a, mid_y: b, angle: c, ratio: d }
                    }
                }
                "PolylineTo" => {
                    let a = self.row_cell(sheet, ix, key, "A");
                    self.take_constructor(&a.view, constructors)?;
                    GeomRow::PolylineTo { x, y, polyline: polyline_of(&a.view) }
                }
                "NURBSTo" => {
                    let [a, b, c, d] = abcd(self, &["A", "B", "C", "D"])?;
                    let e = self.row_cell(sheet, ix, key, "E");
                    self.take_constructor(&e.view, constructors)?;
                    GeomRow::NurbsTo { x, y, second_last_knot: a, last_weight: b, first_knot: c, first_weight: d, nurbs: nurbs_of(&e.view) }
                }
                "SplineStart" => {
                    let [a, b, c, d] = abcd(self, &["A", "B", "C", "D"])?;
                    GeomRow::SplineStart { x, y, second_knot: a, first_knot: b, last_knot: c, degree: d }
                }
                "SplineKnot" => GeomRow::SplineKnot { x, y, knot: abcd(self, &["A"])?[0] },
                "RelCubBezTo" => {
                    let [a, b, c, d] = abcd(self, &["A", "B", "C", "D"])?;
                    GeomRow::RelCubBezTo { x, y, c1_x: a, c1_y: b, c2_x: c, c2_y: d }
                }
                "RelQuadBezTo" => {
                    let [a, b, ..] = abcd(self, &["A", "B"])?;
                    GeomRow::RelQuadBezTo { x, y, c_x: a, c_y: b }
                }
                "Ellipse" => {
                    let [a, b, c, d] = abcd(self, &["A", "B", "C", "D"])?;
                    GeomRow::Ellipse { x, y, first_x: a, first_y: b, second_x: c, second_y: d }
                }
                "InfiniteLine" => {
                    let [a, b, ..] = abcd(self, &["A", "B"])?;
                    GeomRow::InfiniteLine { x, y, other_x: a, other_y: b }
                }
                // A row of a type no spec lists draws nothing, as the renderer treats it.
                _ => continue,
            };
            geom.push(g);
        }
        // The splines may spend what the budget has left, at so many multiplications a step, and no more than a section may; and the rows may
        // make as many steps as the budget has left, and no more than a section may.
        let left = self.ctx.steps_left();
        let work = left.saturating_mul(MULTIPLICATIONS_PER_STEP).min(MAX_SPLINE_WORK);
        let most = usize::try_from(left).unwrap_or(usize::MAX).min(MAX_DRAWN_STEPS);
        // The same `Bound` a snap's outlines are built under (`build_paths_bounded`), held to the renderer's by the same clamp.
        let drawn = match build_row_draws(&geom, size, Bound { curve_work: work, path_elements: most }) {
            Ok(d) => d,
            Err(GeomError::BadSize) => return Err(bad()),
            Err(_) => return Err(Value::Error(LOOP_ERROR)),
        };
        // The pieces of a path cost a step each, and so does a hundred and some multiplications of a spline; and they are held while the
        // function works, as the evaluation accounts memory.
        let steps: usize = drawn.rows.iter().map(|r| r.steps.len()).sum();
        let spent = usize::try_from(drawn.curve_work / MULTIPLICATIONS_PER_STEP).unwrap_or(usize::MAX);
        if !self.steps(steps.saturating_add(spent)) {
            return Err(Value::Error(LOOP_ERROR));
        }
        Ok(drawn.rows)
    }

    /// Draws the section `ix` of `sheet` and hands the draws to `work`, holding what they take (a row of steps and a step, as accounted) until it
    /// is done: they are the largest thing a call keeps, and `work` must not read a cell, which could run another call that holds its own.
    fn with_draws<T>(&mut self, sheet: &SheetRef, ix: u32, work: impl FnOnce(&mut Self, &[RowDraw]) -> Result<T, Value>) -> Result<T, Value> {
        let draws = self.geometry_draws(sheet, ix)?;
        let held = draws.iter().fold(0u64, |sum, d| {
            let steps = (d.steps.capacity() as u64).saturating_mul(DRAW_STEP_BYTES);
            sum.saturating_add(DRAW_ROW_BYTES).saturating_add(steps)
        });
        if !self.ctx.hold_bytes(held) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let out = work(self, &draws);
        self.ctx.release_bytes(held);
        out
    }

    /// The walk of the section `ix` of `sheet`, with `Rounding` when `rounded`. `Rounding` is read before the section is drawn: it
    /// may be a formula that calls a path function of another shape, which holds that shape's draws while it works, and the section's draws are
    /// then not yet there to be held beside them. A section that is not there is `#REF!` whatever `Rounding` is.
    fn section_walk(&mut self, sheet: &SheetRef, ix: u32, rounded: bool) -> Result<Walk, Value> {
        if !self.has_section(sheet, ix) {
            return Err(missing());
        }
        let rounding = if rounded { self.plain_number(sheet, &CellAddr::direct("Rounding"))? } else { 0.0 };
        self.with_draws(sheet, ix, |ev, draws| match Walk::build(draws, rounding, |n| ev.steps(n.saturating_mul(2))) {
            Ok(Some(w)) => Ok(w),
            Ok(None) => Err(Value::Error(LOOP_ERROR)),
            Err(_) => Err(bad()),
        })
    }

    /// Whether the sheet has a `Geometry` section `ix`, here or inherited.
    fn has_section(&self, sheet: &SheetRef, ix: u32) -> bool {
        self.res.effective_sections(sheet, "Geometry").is_some_and(|keys| keys.iter().any(|k| k.ix == Some(ix)))
    }
}

/// Where a travel is: the length before the span, its length, and the segment it is limited to.
fn span_of(walk: &Walk, segment: Option<u32>) -> Result<(f64, f64, Option<u32>), Value> {
    match segment {
        None => Ok((0.0, walk.total, None)),
        Some(s) if s > walk.segments => Err(missing()),
        Some(s) => walk.segment_range(s).map(|(start, len)| (start, len, Some(s))).ok_or_else(missing),
    }
}

/// The place a travel along a span reaches (a span with no length is `#REF!`).
fn place_of(walk: &Walk, travel: f64, segment: Option<u32>) -> Result<crate::pathwalk::Place, Value> {
    let (start, len, row) = span_of(walk, segment)?;
    if len <= 0.0 || !len.is_finite() {
        return Err(missing());
    }
    walk.place_at(start + travel * len, row).ok_or_else(missing)
}

fn length(x: f64) -> Value {
    Value::Number(Quantity::new(x, Unit::DrawingDefault))
}

/// `PATHLENGTH(section[, segment])`.
fn pathlength(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let (sheet, ix) = ev.section_arg(site, 0)?;
        let segment = ev.segment_arg(site, 1)?;
        let walk = ev.section_walk(&sheet, ix, true)?;
        // A path with no length is 0, with no error.
        let (_, len, _) = span_of(&walk, segment)?;
        Ok::<Value, Value>(length(len))
    })();
    result.unwrap_or_else(|e| e)
}

/// `POINTALONGPATH(section, travel[, offset][, segment])`: the place, and `offset` square to the travel, positive to
/// its left.
fn pointalongpath(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let (sheet, ix) = ev.section_arg(site, 0)?;
        let travel = ev.travel_arg(site, 1)?;
        let offset = ev.number_arg(site, 2)?.unwrap_or(0.0);
        let segment = ev.segment_arg(site, 3)?;
        let walk = ev.section_walk(&sheet, ix, true)?;
        let place = place_of(&walk, travel, segment)?;
        // The left of travel, with y up: the direction turned a quarter turn counterclockwise.
        let left = Vec2::new(-place.tangent.y, place.tangent.x);
        let at = place.point + left * offset;
        if !(at.x.is_finite() && at.y.is_finite()) {
            return Err(bad());
        }
        Ok::<Value, Value>(point_value(at.x, at.y, Some(sheet)))
    })();
    result.unwrap_or_else(|e| e)
}

/// `ANGLEALONGPATH(section, travel[, segment])`: the direction of travel, from minus pi to pi.
fn anglealongpath(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let (sheet, ix) = ev.section_arg(site, 0)?;
        let travel = ev.travel_arg(site, 1)?;
        let segment = ev.segment_arg(site, 2)?;
        let walk = ev.section_walk(&sheet, ix, true)?;
        let place = place_of(&walk, travel, segment)?;
        Ok::<Value, Value>(Value::Number(Quantity::new(place.tangent.y.atan2(place.tangent.x), Unit::AngleDefault)))
    })();
    result.unwrap_or_else(|e| e)
}

/// The place nearest (`x`, `y`) of a call `NEARESTPOINTONPATH(section, x, y)` or `DISTTOPATH(section, x, y)`.
fn closest_to_arguments(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Result<(Walk, Point, crate::pathwalk::Closest), Value> {
    let (sheet, ix) = ev.section_arg(site, 0)?;
    let x = ev.required_number(site, 1)?;
    let y = ev.required_number(site, 2)?;
    let walk = ev.section_walk(&sheet, ix, true)?;
    if walk.total <= 0.0 {
        return Err(missing());
    }
    // Looking at each piece costs a few steps.
    if !ev.steps(walk.piece_count().saturating_mul(4)) {
        return Err(Value::Error(LOOP_ERROR));
    }
    let p = Point::new(x, y);
    let closest = walk.nearest(p).ok_or_else(missing)?;
    Ok((walk, p, closest))
}

/// `NEARESTPOINTONPATH(section, x, y)`: how far along the path, from 0 to 1, the nearest place lies.
fn nearestpointonpath(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    match closest_to_arguments(ev, site) {
        Ok((walk, _, closest)) => Value::num((closest.along / walk.total).clamp(0.0, 1.0)),
        Err(e) => e,
    }
}

/// `DISTTOPATH(section, x, y)`: the gap to the nearest place, positive to the left of travel there and negative to the
/// right.
fn disttopath(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    match closest_to_arguments(ev, site) {
        Ok((walk, p, closest)) => {
            let Some(place) = walk.place_at(closest.along, None) else { return missing() };
            let to = p - place.point;
            let side = place.tangent.cross(to);
            length(if side < 0.0 { -closest.distance } else { closest.distance })
        }
        Err(e) => e,
    }
}

/// `PATHSEGMENT(section, travel)`: the number of the segment that share of the length along, from 1.
fn pathsegment(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let (sheet, ix) = ev.section_arg(site, 0)?;
        let travel = ev.travel_arg(site, 1)?;
        let walk = ev.section_walk(&sheet, ix, true)?;
        if walk.total <= 0.0 {
            return Err(missing());
        }
        let row = walk.segment_at(travel * walk.total).ok_or_else(missing)?;
        Ok::<Value, Value>(Value::num(f64::from(row)))
    })();
    result.unwrap_or_else(|e| e)
}

/// `SEGMENTCOUNT(pathRef)`: the drawn rows, every one counted.
fn segmentcount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let (sheet, ix) = ev.section_arg(site, 0)?;
        let count = ev.with_draws(&sheet, ix, |_, draws| {
            Ok(draws.iter().filter(|d| d.steps.iter().any(|s| !matches!(s, diacraft_geom::DrawStep::Jump(_)))).count())
        })?;
        Ok::<Value, Value>(Value::num(count as f64))
    })();
    result.unwrap_or_else(|e| e)
}

// ---- the box ------------------------------------------------------------------------------------------------------------------------

/// The four corners of a box, as pairs.
fn corners(r: Rect) -> [(f64, f64); 4] {
    [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)]
}

/// The upright box round some points.
fn round_points(points: impl IntoIterator<Item = (f64, f64)>) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for (x, y) in points {
        let p = Rect::new(x, y, x, y);
        out = Some(out.map_or(p, |r| r.union(p)));
    }
    out.filter(|r| r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite())
}

impl Evaluator<'_, '_> {
    /// The box of the shape `at` in its own frame (a project rule): the extent of its `Geometry` sections that are shown
    /// (`NoShow` 0), with the boxes of a group's members carried into its frame; and where it has no path at all, its upright width-height box.
    fn shape_box(&mut self, at: &ShapeRef, depth: usize, visited: &mut usize) -> Result<Rect, Value> {
        // A level of group is a native frame beside the cell reads and nodes the evaluation holds, so it counts in the one depth they share.
        if !self.enter_n(BOX_LEVEL_DEPTH) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let out = self.shape_box_level(at, depth, visited);
        self.leave_n(BOX_LEVEL_DEPTH);
        out
    }

    fn shape_box_level(&mut self, at: &ShapeRef, depth: usize, visited: &mut usize) -> Result<Rect, Value> {
        *visited += 1;
        if depth > MAX_BOX_DEPTH || *visited > MAX_BOX_SHAPES || !self.steps(1) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let sheet = SheetRef::Shape(at.clone());
        let mut found: Option<Rect> = None;
        let keys = self.res.effective_sections(&sheet, "Geometry").unwrap_or_default();
        for key in keys {
            let Some(ix) = key.ix else { continue };
            if self.plain_number(&sheet, &CellAddr::in_section("Geometry", Some(ix), "NoShow"))? != 0.0 {
                continue;
            }
            let extent = self.with_draws(&sheet, ix, |ev, draws| match Walk::build(draws, 0.0, |n| ev.steps(n.saturating_mul(2))) {
                Ok(Some(w)) => Ok(w.extent()),
                Ok(None) => Err(Value::Error(LOOP_ERROR)),
                Err(_) => Err(bad()),
            })?;
            if let Some(r) = extent {
                found = Some(found.map_or(r, |f| f.union(r)));
            }
        }
        let members = self.res.effective_members(at).unwrap_or_default();
        for member in members {
            let Member::Local(child) = member else { continue };
            let inner = self.shape_box(&child, depth + 1, visited)?;
            let step = self.step_of(&child);
            if let Some(r) = round_points(corners(inner).map(|c| step.up(c))) {
                found = Some(found.map_or(r, |f| f.union(r)));
            }
        }
        match found {
            Some(r) => Ok(r),
            None => {
                let w = self.plain_number(&sheet, &CellAddr::direct("Width"))?;
                let h = self.plain_number(&sheet, &CellAddr::direct("Height"))?;
                Ok(Rect::new(0.0, 0.0, w, h).abs())
            }
        }
    }

    /// The box of the shape the call is read on, in its parent's frame: upright in that frame, so a turned or flipped shape's edges are of the
    /// box that holds it. The pin's place in that frame goes with it.
    fn parent_box(&mut self, site: &Site<'_>) -> Result<(Rect, Point), Value> {
        let SheetRef::Shape(at) = site.target.sheet.clone() else { return Err(missing()) };
        let mut visited = 0usize;
        let local = self.shape_box(&at, 0, &mut visited)?;
        let step = self.step_of(&at);
        let up = round_points(corners(local).map(|c| step.up(c))).ok_or_else(bad)?;
        let sheet = site.target.sheet.clone();
        let pin = Point::new(self.cell_number(&sheet, "PinX"), self.cell_number(&sheet, "PinY"));
        Ok((up, pin))
    }

    fn box_index(&mut self, site: &Site<'_>, most: i64) -> Result<i64, Value> {
        let q = {
            let Some(e) = site.arg(0) else { return Err(bad()) };
            let v = self.eval(e, site.env);
            self.read_double(&v, site.env)?
        };
        let n = signed_int(q)?;
        if (0..=most).contains(&n) { Ok(n) } else { Err(bad()) }
    }
}

/// `BOUNDINGBOXRECT(index)`: 0 the left edge, 1 the right, 2 the top, 3 the bottom.
fn boundingboxrect(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let index = ev.box_index(site, 3)?;
        let (b, _) = ev.parent_box(site)?;
        Ok::<Value, Value>(length(match index {
            0 => b.x0,
            1 => b.x1,
            2 => b.y1,
            _ => b.y0,
        }))
    })();
    result.unwrap_or_else(|e| e)
}

/// `BOUNDINGBOXDIST(index)`: width, height, the four gaps to the pin, and the pin against the box's middle.
fn boundingboxdist(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let result = (|| {
        let index = ev.box_index(site, 7)?;
        let (b, pin) = ev.parent_box(site)?;
        Ok::<Value, Value>(length(match index {
            0 => b.x1 - b.x0,
            1 => b.y1 - b.y0,
            2 => pin.x - b.x0,
            3 => b.x1 - pin.x,
            4 => b.y1 - pin.y,
            5 => pin.y - b.y0,
            6 => pin.x - (b.x0 + b.x1) / 2.0,
            _ => pin.y - (b.y0 + b.y1) / 2.0,
        }))
    })();
    result.unwrap_or_else(|e| e)
}

pub(crate) static DEFS: &[FnDef] = &[
    lazy("ANGLEALONGPATH", 2, 3, anglealongpath),
    lazy("BOUNDINGBOXDIST", 1, 1, boundingboxdist),
    lazy("BOUNDINGBOXRECT", 1, 1, boundingboxrect),
    lazy("DISTTOPATH", 3, 3, disttopath),
    lazy("NEARESTPOINTONPATH", 3, 3, nearestpointonpath),
    lazy("PATHLENGTH", 1, 2, pathlength),
    lazy("PATHSEGMENT", 2, 2, pathsegment),
    lazy("POINTALONGPATH", 2, 4, pointalongpath),
    lazy("SEGMENTCOUNT", 1, 1, segmentcount),
];
