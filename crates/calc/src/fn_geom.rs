//! Points, frames and a few measures: `PNT`, `PNTX`, `PNTY`, `PAR`, `LOC`, `LOCTOLOC`, `LOCTOPAR`, `ANGLETOLOC`,
//! `ANGLETOPAR`, `GRAVITY`, `INTERSECTX`, `INTERSECTY`, `RECTSECT`, `NURBS` and `POLYLINE`.
//!
//! The nine functions that walk a path or measure a shape's box (`POINTALONGPATH`, `ANGLEALONGPATH`, `NEARESTPOINTONPATH`, `DISTTOPATH`,
//! `PATHLENGTH`, `PATHSEGMENT`, `SEGMENTCOUNT`, `BOUNDINGBOXRECT`, `BOUNDINGBOXDIST`) are in `fn_path.rs`.
//!
//! **Frames.** A shape's own frame goes to its parent's by one step: subtract the local pin, negate x under `FlipX`
//! and y under `FlipY`, turn counterclockwise by `Angle`, add the pin. A carry goes up from the source frame through every group and down
//! to the target frame; the groups the two chains share are left out, so no rounding comes in from them. Every length is in internal
//! inches and every angle in radians, whatever unit shows.

use std::f64::consts::PI;

use diacraft_doc::{CellAddr, Owner, ShapeRef, SheetRef};
use diacraft_formula::Expr;
use diacraft_units::{Quantity, Unit};

use crate::convert::{signed_int, unsigned_int};
use crate::eval::{Env, Evaluator, Site};
use crate::fn_math::wrap_turn;
use crate::registry::{FnDef, MANY, eager, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, GRAVITY_UPPER_WINDOW, MAX_FRAME_STEPS, PARALLEL_CROSS, RECTSECT_TIE};
use crate::value::{CellError, Constructor, ConstructorKind, Point, Value};

type Slots<'a> = &'a [Option<Value>];

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

/// A frame: the part that holds it, and the shapes from the outermost group to the shape itself. A page or master sheet has none.
#[derive(Clone, Debug)]
struct Frame {
    owner: Owner,
    shapes: Vec<ShapeRef>,
}

impl Frame {
    /// The frame of the parent: the group, or the page for a top-level shape. A page is its own parent.
    fn parent(&self) -> Frame {
        let mut shapes = self.shapes.clone();
        shapes.pop();
        Frame { owner: self.owner, shapes }
    }

    fn sheet(&self) -> SheetRef {
        match self.shapes.last() {
            Some(s) => SheetRef::Shape(s.clone()),
            None => match self.owner {
                Owner::Page(id) => SheetRef::Page(id),
                Owner::Master(id) => SheetRef::Master(id),
            },
        }
    }
}

/// One step's cells.
pub(crate) struct Step {
    pin: (f64, f64),
    loc: (f64, f64),
    angle: f64,
    flip_x: bool,
    flip_y: bool,
}

impl Step {
    fn flipped(&self, v: (f64, f64)) -> (f64, f64) {
        (if self.flip_x { -v.0 } else { v.0 }, if self.flip_y { -v.1 } else { v.1 })
    }

    fn turn(angle: f64, v: (f64, f64)) -> (f64, f64) {
        let (s, c) = angle.sin_cos();
        (v.0 * c - v.1 * s, v.0 * s + v.1 * c)
    }

    /// From the shape's frame to its parent's.
    pub(crate) fn up(&self, p: (f64, f64)) -> (f64, f64) {
        let v = self.flipped((p.0 - self.loc.0, p.1 - self.loc.1));
        let t = Step::turn(self.angle, v);
        (t.0 + self.pin.0, t.1 + self.pin.1)
    }

    /// From the parent's frame to the shape's.
    fn down(&self, p: (f64, f64)) -> (f64, f64) {
        let t = Step::turn(-self.angle, (p.0 - self.pin.0, p.1 - self.pin.1));
        let v = self.flipped(t);
        (v.0 + self.loc.0, v.1 + self.loc.1)
    }

    /// A direction up one step: the flips and the turn, with no pin.
    fn up_direction(&self, d: (f64, f64)) -> (f64, f64) {
        Step::turn(self.angle, self.flipped(d))
    }

    fn down_direction(&self, d: (f64, f64)) -> (f64, f64) {
        self.flipped(Step::turn(-self.angle, d))
    }
}

/// What a formula's references say of the frame its point is in.
enum Tag {
    /// No reference: the sheet's own frame.
    None,
    One(SheetRef),
    /// References to more than one sheet.
    Many,
}

impl Evaluator<'_, '_> {
    fn frame_of(&mut self, sheet: &SheetRef) -> Result<Frame, Value> {
        match sheet {
            SheetRef::Page(id) => Ok(Frame { owner: Owner::Page(*id), shapes: Vec::new() }),
            SheetRef::Master(id) => Ok(Frame { owner: Owner::Master(*id), shapes: Vec::new() }),
            SheetRef::Shape(at) => {
                // A shape that is not there, or a member the instance deleted, reads as missing.
                if self.res.shape(at).is_none() {
                    return Err(Value::Error(CellError::Ref));
                }
                let mut chain = vec![at.clone()];
                let mut cur = at.clone();
                while let Some(parent) = cur.parent() {
                    if chain.len() >= MAX_FRAME_STEPS {
                        return Err(Value::Error(CellError::Value));
                    }
                    chain.push(parent.clone());
                    cur = parent;
                }
                chain.reverse();
                Ok(Frame { owner: at.owner, shapes: chain })
            }
            // A document or a style has no frame.
            SheetRef::Document | SheetRef::Style(_) => Err(Value::Error(CellError::Ref)),
        }
    }

    pub(crate) fn cell_number(&mut self, sheet: &SheetRef, name: &str) -> f64 {
        match &self.read_cell(sheet, &CellAddr::direct(name)).view {
            Value::Number(q) => q.stored,
            Value::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }

    pub(crate) fn step_of(&mut self, s: &ShapeRef) -> Step {
        let sheet = SheetRef::Shape(s.clone());
        Step {
            pin: (self.cell_number(&sheet, "PinX"), self.cell_number(&sheet, "PinY")),
            loc: (self.cell_number(&sheet, "LocPinX"), self.cell_number(&sheet, "LocPinY")),
            angle: self.cell_number(&sheet, "Angle"),
            flip_x: self.cell_number(&sheet, "FlipX") != 0.0,
            flip_y: self.cell_number(&sheet, "FlipY") != 0.0,
        }
    }

    /// The steps a carry takes: up from `src` and down to `dst`, each in order; `Err` when the two are on different pages.
    fn route(&mut self, src: &Frame, dst: &Frame) -> Result<(Vec<Step>, Vec<Step>), Value> {
        if src.owner != dst.owner {
            return Err(Value::Error(CellError::Value));
        }
        let common = src.shapes.iter().zip(&dst.shapes).take_while(|(a, b)| a == b).count();
        if !self.steps(src.shapes.len() + dst.shapes.len() + 1) {
            return Err(Value::Error(crate::rules::LOOP_ERROR));
        }
        let up: Vec<Step> = src.shapes.iter().skip(common).rev().map(|s| self.step_of(s)).collect();
        let down: Vec<Step> = dst.shapes.iter().skip(common).map(|s| self.step_of(s)).collect();
        Ok((up, down))
    }

    fn carry(&mut self, src: &Frame, dst: &Frame, p: (f64, f64)) -> Result<(f64, f64), Value> {
        let (up, down) = self.route(src, dst)?;
        let p = up.iter().fold(p, |p, s| s.up(p));
        Ok(down.iter().fold(p, |p, s| s.down(p)))
    }

    fn carry_angle(&mut self, src: &Frame, dst: &Frame, angle: f64) -> Result<f64, Value> {
        let (up, down) = self.route(src, dst)?;
        let d = (angle.cos(), angle.sin());
        let d = up.iter().fold(d, |d, s| s.up_direction(d));
        let d = down.iter().fold(d, |d, s| s.down_direction(d));
        Ok(d.1.atan2(d.0))
    }

    /// The sheets the references in `exprs` name: one, none, or several.
    fn tag_of(&mut self, exprs: &[&Expr], env: &Env) -> Tag {
        let mut found: Option<SheetRef> = None;
        let mut stack: Vec<&Expr> = exprs.to_vec();
        while let Some(e) = stack.pop() {
            // A step a node of the arguments: a `PNT` of 60,000 references is 60,000 steps and not one.
            if !self.steps(1) {
                return Tag::Many;
            }
            match e {
                Expr::Ref(r) => {
                    if let Ok((sheet, _)) = self.locate(r, env) {
                        match &found {
                            None => found = Some(sheet),
                            Some(f) if *f == sheet => {}
                            Some(_) => return Tag::Many,
                        }
                    }
                }
                Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
                Expr::Binary(_, a, b) => {
                    stack.push(a);
                    stack.push(b);
                }
                Expr::Call(c) => stack.extend(c.args.iter().flatten()),
                _ => {}
            }
        }
        found.map_or(Tag::None, Tag::One)
    }

    /// An argument that names a sheet (`srcRef`, `dstRef`): a reference's sheet, or the error the expression gives.
    fn frame_arg(&mut self, site: &Site<'_>, i: usize) -> Result<Frame, Value> {
        let Some(e) = site.arg(i) else { return Err(bad()) };
        match e {
            Expr::Ref(r) => {
                let (sheet, _) = self.locate(r, site.env)?;
                self.frame_of(&sheet)
            }
            other => {
                let v = self.eval(other, site.env);
                Err(if v.is_failure() { v } else { bad() })
            }
        }
    }

    fn point_arg(&mut self, site: &Site<'_>, i: usize) -> Result<Point, Value> {
        let Some(e) = site.arg(i) else { return Err(bad()) };
        match self.eval(e, site.env) {
            Value::Point(p) => Ok(*p),
            v if v.is_failure() => Err(v),
            _ => Err(bad()),
        }
    }

    /// The frame a point is in: its own tag, else the sheet its references name, else the formula's sheet.
    fn point_frame(&mut self, p: &Point, site: &Site<'_>, arg: usize) -> Result<Frame, Value> {
        if p.mixed {
            return Err(Value::Error(CellError::Value));
        }
        if let Some(f) = &p.frame {
            return self.frame_of(f);
        }
        let tag = match site.arg(arg) {
            Some(e) => self.tag_of(&[e], site.env),
            None => Tag::None,
        };
        match tag {
            Tag::One(s) => self.frame_of(&s),
            Tag::None => self.frame_of(&site.target.sheet.clone()),
            Tag::Many => Err(Value::Error(CellError::Value)),
        }
    }
}

pub(crate) fn point_value(x: f64, y: f64, frame: Option<SheetRef>) -> Value {
    Value::Point(Box::new(Point { x: Quantity::new(x, Unit::DrawingDefault), y: Quantity::new(y, Unit::DrawingDefault), frame, mixed: false }))
}

fn pnt(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    // Any other number of arguments is `#VALUE!`.
    let (Some(xe), Some(ye)) = (site.arg(0), site.arg(1)) else { return bad() };
    if site.len() != 2 {
        return bad();
    }
    let xv = ev.eval(xe, site.env);
    if xv.is_failure() {
        return xv;
    }
    let yv = ev.eval(ye, site.env);
    if yv.is_failure() {
        return yv;
    }
    let x = match ev.read_double(&xv, site.env) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let y = match ev.read_double(&yv, site.env) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let (frame, mixed) = match ev.tag_of(&[xe, ye], site.env) {
        Tag::None => (None, false),
        Tag::One(s) => (Some(s), false),
        Tag::Many => (None, true),
    };
    Value::Point(Box::new(Point { x, y, frame, mixed }))
}

/// The coordinate of a point, or a number as it is: a plain coordinate reads as `DL`.
fn coordinate(v: &Value, pick_y: bool) -> Value {
    match v {
        Value::Point(p) => {
            let q = if pick_y { p.y } else { p.x };
            Value::Number(if q.unit == Unit::Unitless { Quantity::new(q.stored, Unit::DrawingDefault) } else { q })
        }
        Value::Number(_) => v.clone(),
        _ => bad(),
    }
}

fn pntx(_: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    match crate::convert::slot(slots, 0) {
        Ok(v) => coordinate(v, false),
        Err(e) => e,
    }
}

fn pnty(_: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    match crate::convert::slot(slots, 0) {
        Ok(v) => coordinate(v, true),
        Err(e) => e,
    }
}

/// `PAR` and `LOC`: the point carried from its frame to the sheet's parent, or to the sheet.
fn carrier(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, to_parent: bool) -> Value {
    if site.len() != 1 {
        return bad();
    }
    let p = match ev.point_arg(site, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let result = (|| {
        let from = ev.point_frame(&p, site, 0)?;
        let own = ev.frame_of(&site.target.sheet.clone())?;
        let to = if to_parent { own.parent() } else { own };
        let (x, y) = ev.carry(&from, &to, (p.x.stored, p.y.stored))?;
        Ok::<Value, Value>(point_value(x, y, Some(to.sheet())))
    })();
    result.unwrap_or_else(|e| e)
}

fn par(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    carrier(ev, site, true)
}

fn loc(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    carrier(ev, site, false)
}

/// `LOCTOLOC` and `LOCTOPAR`: the point's numbers read in `srcRef`'s frame and carried to `dstRef`'s frame or its parent's.
fn point_to(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, to_parent: bool) -> Value {
    if site.len() != 3 {
        return bad();
    }
    let p = match ev.point_arg(site, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let result = (|| {
        let from = ev.frame_arg(site, 1)?;
        let dst = ev.frame_arg(site, 2)?;
        let to = if to_parent { dst.parent() } else { dst };
        let (x, y) = ev.carry(&from, &to, (p.x.stored, p.y.stored))?;
        Ok::<Value, Value>(point_value(x, y, Some(to.sheet())))
    })();
    result.unwrap_or_else(|e| e)
}

fn loctoloc(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    point_to(ev, site, false)
}

fn loctopar(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    point_to(ev, site, true)
}

/// `ANGLETOLOC` and `ANGLETOPAR`: a direction carried between frames, folded to -pi to pi.
fn angle_to(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, to_parent: bool) -> Value {
    if site.len() != 3 {
        return bad();
    }
    let Some(ae) = site.arg(0) else { return bad() };
    let av = ev.eval(ae, site.env);
    if av.is_failure() {
        return av;
    }
    let angle = match ev.read_double(&av, site.env) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    let result = (|| {
        let from = ev.frame_arg(site, 1)?;
        let dst = ev.frame_arg(site, 2)?;
        let to = if to_parent { dst.parent() } else { dst };
        let a = ev.carry_angle(&from, &to, angle)?;
        Ok::<Value, Value>(Value::Number(Quantity::new(a, Unit::AngleDefault)))
    })();
    result.unwrap_or_else(|e| e)
}

fn angletoloc(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    angle_to(ev, site, false)
}

fn angletopar(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    angle_to(ev, site, true)
}

fn gravity(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let angle = match ev.arg_double(site, slots, 0) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    let limit = |ev: &mut Evaluator<'_, '_>, i: usize, default: f64| -> Result<f64, Value> {
        match slots.get(i).and_then(Option::as_ref) {
            None => Ok(default),
            Some(v) => ev.read_double(v, site.env).map(|q| q.stored),
        }
    };
    let l1 = match limit(ev, 1, PI / 2.0) {
        Ok(a) => wrap_turn(a),
        Err(e) => return e,
    };
    let l2 = match limit(ev, 2, 3.0 * PI / 2.0) {
        Ok(a) => wrap_turn(a),
        Err(e) => return e,
    };
    let (lo, hi) = if l1 <= l2 { (l1, l2) } else { (l2, l1) };
    let a = wrap_turn(angle);
    // An angle or a limit that is not finite is NaN here, and no angle is between limits then (every test below is false): 0.
    // Above the smaller limit and up to the larger, with a window past it.
    let between = a > lo + 1e-12 && a <= hi + GRAVITY_UPPER_WINDOW;
    Value::Number(Quantity::new(if between { PI } else { 0.0 }, Unit::AngleDefault))
}

/// The crossing of two lines, each a point and an angle from the positive x-axis: its x or y, in the unit of the first
/// coordinate that carries one.
fn intersect(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, want_y: bool) -> Value {
    let mut q = [Quantity::new(0.0, Unit::Unitless); 6];
    for (i, slot) in q.iter_mut().enumerate() {
        match ev.arg_double(site, slots, i) {
            Ok(v) => *slot = v,
            Err(e) => return e,
        }
    }
    let unit = [q[0], q[1], q[3], q[4]].iter().map(|v| v.unit).find(|u| *u != Unit::Unitless);
    let (x1, y1, a1, x2, y2, a2) = (q[0].stored, q[1].stored, q[2].stored, q[3].stored, q[4].stored, q[5].stored);
    let d1 = (a1.cos(), a1.sin());
    let d2 = (a2.cos(), a2.sin());
    let cross = d1.0 * d2.1 - d1.1 * d2.0;
    // Lines that do not cross, a half turn apart included, are `#DIV/0!`.
    if cross.abs() < PARALLEL_CROSS {
        return Value::Error(CellError::DivZero);
    }
    let t = ((x2 - x1) * d2.1 - (y2 - y1) * d2.0) / cross;
    let r = if want_y { y1 + t * d1.1 } else { x1 + t * d1.0 };
    if !r.is_finite() {
        return Value::Error(CellError::DivZero);
    }
    match unit {
        Some(u) => Value::Number(Quantity::new(r, u)),
        None => Value::num(r),
    }
}

fn intersectx(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    intersect(ev, site, slots, false)
}

fn intersecty(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    intersect(ev, site, slots, true)
}

/// `RECTSECT(width, height, x, y, option)`: which of the four sectors the diagonals cut a rectangle into holds the point, measured from the
/// centre: 0 centre, 1 right, 2 top, 3 left, 4 bottom.
fn rectsect(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let mut n = [0.0f64; 4];
    for (i, slot) in n.iter_mut().enumerate() {
        match ev.arg_double(site, slots, i) {
            Ok(q) => *slot = q.stored,
            Err(e) => return e,
        }
    }
    let option = match ev.arg_bool(site, slots, 4) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let [w, h, x, y] = n;
    if x == 0.0 && y == 0.0 {
        return Value::num(0.0);
    }
    let gx = x.abs() * h.abs();
    let gy = y.abs() * w.abs();
    let tie = (gx - gy).abs() <= RECTSECT_TIE * gx.max(gy).max(1.0);
    let horizontal = if tie { !option } else { gx > gy };
    let sector = match (horizontal, if horizontal { x } else { y } < 0.0) {
        (true, false) => 1.0,
        (true, true) => 3.0,
        (false, false) => 2.0,
        (false, true) => 4.0,
    };
    Value::num(sector)
}

/// `NURBS` and `POLYLINE` only gather their arguments, evaluated. `lead` is the arguments before the tuples and
/// `step` the width of a tuple; a count that does not fit is `#VALUE!`.
fn gather(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, kind: ConstructorKind, lead: usize, step: usize) -> Value {
    if slots.len() < lead || !(slots.len() - lead).is_multiple_of(step) {
        return Value::Error(CellError::Value);
    }
    let mut numbers = Vec::with_capacity(slots.len());
    for i in 0..slots.len() {
        match ev.arg_double(site, slots, i) {
            Ok(q) => numbers.push(q.stored),
            Err(e) => return e,
        }
    }
    Value::Constructor(Box::new(Constructor { kind, numbers }))
}

fn nurbs(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    // `degree` is a signed whole number and the two types unsigned ones: a value that is not one is `#VALUE!`.
    if slots.len() >= 4 {
        let degree = ev.arg_double(site, slots, 1).and_then(signed_int);
        let xt = ev.arg_double(site, slots, 2).and_then(unsigned_int);
        let yt = ev.arg_double(site, slots, 3).and_then(unsigned_int);
        for r in [degree.map(|_| ()), xt.map(|_| ()), yt.map(|_| ())] {
            if let Err(e) = r {
                return e;
            }
        }
    }
    gather(ev, site, slots, ConstructorKind::Nurbs, 4, 4)
}

fn polyline(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    gather(ev, site, slots, ConstructorKind::Polyline, 2, 2)
}

pub(crate) static DEFS: &[FnDef] = &[
    lazy("ANGLETOLOC", 3, 3, angletoloc),
    lazy("ANGLETOPAR", 3, 3, angletopar),
    eager("GRAVITY", 1, 3, gravity),
    eager("INTERSECTX", 6, 6, intersectx),
    eager("INTERSECTY", 6, 6, intersecty),
    lazy("LOC", 1, 1, loc),
    lazy("LOCTOLOC", 3, 3, loctoloc),
    lazy("LOCTOPAR", 3, 3, loctopar),
    eager("NURBS", 4, MANY, nurbs),
    lazy("PAR", 1, 1, par),
    lazy("PNT", 2, 2, pnt),
    eager("PNTX", 1, 1, pntx),
    eager("PNTY", 1, 1, pnty),
    eager("POLYLINE", 2, MANY, polyline),
    eager("RECTSECT", 5, 5, rectsect),
];
