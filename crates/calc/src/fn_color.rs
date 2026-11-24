//! Colour and theme: making a colour, taking one apart, moving it lighter or darker, and asking a theme for a value.
//!
//! **What is here.** The 28 functions of the batch. `THEMEVAL` answers where the sheet selects no theme,
//! and under an explicit theme from the theme part's lists (`theme_eval.rs`: the variants, the fill, line and effect lists, the font
//! list), as do `THEMECBV` and `THEMEPROP`. It is [`Value::Unsupported`], and the saved value stands, where the file has no theme part,
//! where the part's list or entry could not be read, for a custom colour scheme and for the properties not worked out.
//!
//! **Arithmetic.** `TINT`, `SHADE`, `TONE`, `HSL` and the parts work in whole numbers on the 0 to 240 scale (
//! rules found by fitting cached values, which each cite); `MSOTINT` and `MSOSHADE` work in single-precision real numbers and cut the
//! channel.

use std::sync::Arc;

use diacraft_doc::{CellAddr, FormulaSlot, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_formula::Expr;
use diacraft_units::{Quantity, Unit};

use crate::convert::{signed_int, unsigned_int};
use crate::eval::{Env, Evaluator, Site};
use crate::registry::{FnDef, eager, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, EMBELLISHMENT_WITH_NO_THEME, FIRST_THEME_PART, KEY_BYTES, LOOP_ERROR};
use crate::theme_names::{P, canonical, host_property, mapped_root_cell, other_host_cell, table_name};
use crate::theme_tables::{COLOUR_PROPERTIES, COLOUR_SCHEMES, EFFECT_PROPERTIES, EFFECT_SCHEMES};
use crate::value::{CellError, Rgb, Value};

type Slots<'a> = &'a [Option<Value>];

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

impl Evaluator<'_, '_> {
    /// `vColor`: a colour stays; a number is an index, cut to a whole number, naming the fixed palette (0 to 23) or a
    /// `ColorEntry` of the document, and an index in neither is colour 0; a string spelling a formula that gives a colour is that colour
    /// (project rule). `Ok(None)` for what is not a colour at all, which the part functions read as 0; an error or an unsupported
    /// function is `Err`.
    pub(crate) fn read_color(&mut self, v: &Value, env: &Env) -> Result<Option<Rgb>, Value> {
        match v {
            Value::Color(c) => Ok(Some(*c)),
            Value::Number(q) if q.unit == Unit::Unitless || q.dim() == 0 => Ok(Some(self.palette(q.stored))),
            Value::Bool(b) => Ok(Some(self.palette(if *b { 1.0 } else { 0.0 }))),
            Value::Text(s) => match self.eval_string_formula(s, env) {
                Some(Value::Color(c)) => Ok(Some(c)),
                Some(e @ Value::Error(_)) if e != Value::Error(CellError::Ref) => Err(e),
                // A function this evaluator does not have, read from a string, is unknown and not "not a colour".
                Some(u @ Value::Unsupported(_)) => Err(u),
                _ => Ok(None),
            },
            Value::Error(_) | Value::Unsupported(_) => Err(v.clone()),
            _ => Ok(None),
        }
    }

    fn palette(&self, index: f64) -> Rgb {
        let black = Rgb { r: 0, g: 0, b: 0 };
        if !index.is_finite() {
            return black;
        }
        let rgba = self.doc.color(index.trunc() as i64).or_else(|| self.doc.color(0));
        rgba.map_or(black, |c| Rgb { r: c.r, g: c.g, b: c.b })
    }

    /// A colour argument that is an error when it cannot be read: for the functions that take two colours or a colour and a number.
    fn color_arg(&mut self, site: &Site<'_>, slots: Slots<'_>, i: usize) -> Result<Rgb, Value> {
        let v = crate::convert::slot(slots, i)?;
        self.read_color(v, site.env)?.ok_or_else(bad)
    }
}

/// Hue, lightness and saturation on the 0 to 240 scale, in whole numbers.
pub(crate) fn to_hls(c: Rgb) -> (i64, i64, i64) {
    let (r, g, b) = (i64::from(c.r), i64::from(c.g), i64::from(c.b));
    let top = r.max(g).max(b);
    let low = r.min(g).min(b);
    let both = top + low;
    let span = top - low;
    let l = (both * 240 + 255) / 510;
    if span == 0 {
        // `unknown`: 160 for a grey, from the published sample.
        return (160, l, 0);
    }
    let s = if l <= 120 {
        (span * 240 + both / 2) / both
    } else {
        let room = 510 - both;
        (span * 240 + room / 2) / room
    };
    let d = |v: i64| ((top - v) * 40 + span / 2) / span;
    let (dr, dg, db) = (d(r), d(g), d(b));
    let mut h = if r == top {
        db - dg
    } else if g == top {
        80 + dr - db
    } else {
        160 + dg - dr
    };
    if h < 0 {
        h += 240;
    }
    if h > 240 {
        h -= 240;
    }
    (h, l, s)
}

/// A colour from hue, lightness and saturation on the 0 to 240 scale. Lightness and saturation are held at 240.
fn from_hls(h: i64, l: i64, s: i64) -> Rgb {
    let (l, s) = (l.clamp(0, 240), s.clamp(0, 240));
    if s == 0 {
        let v = ((l * 255 + 120) / 240).clamp(0, 255) as u8;
        return Rgb { r: v, g: v, b: v };
    }
    let hi = if l <= 120 { (l * (240 + s) + 120) / 240 } else { l + s - (l * s + 120) / 240 };
    let lo = 2 * l - hi;
    let part = |mut h: i64| {
        if h < 0 {
            h += 240;
        }
        if h > 240 {
            h -= 240;
        }
        if h < 40 {
            lo + ((hi - lo) * h + 20) / 40
        } else if h < 120 {
            hi
        } else if h < 160 {
            lo + ((hi - lo) * (160 - h) + 20) / 40
        } else {
            lo
        }
    };
    let byte = |v: i64| ((v * 255 + 120) / 240).clamp(0, 255) as u8;
    Rgb { r: byte(part(h + 80)), g: byte(part(h)), b: byte(part(h - 80)) }
}

/// A component `v` on 0 to 240 moved by a whole amount `d`, the smaller of the amount and a percent of the room left.
fn step(v: i64, d: i64) -> i64 {
    if d >= 0 { (v + d.min((240 - v) * d / 100)).min(240) } else { (v - d.abs().min(v * d.abs() / 100)).max(0) }
}

fn color_result(c: Rgb) -> Value {
    Value::Color(c)
}

// ---- making a colour ------------------------------------------------------------------------------------------------------

/// The low byte of a `vUnsignedInt` argument.
fn byte(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, i: usize) -> Result<i64, Value> {
    let q = ev.arg_double(site, slots, i)?;
    Ok(i64::from(unsigned_int(q)? & 0xFF))
}

fn rgb(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match (byte(ev, site, slots, 0), byte(ev, site, slots, 1), byte(ev, site, slots, 2)) {
        (Ok(r), Ok(g), Ok(b)) => color_result(Rgb { r: r as u8, g: g as u8, b: b as u8 }),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => e,
    }
}

/// The three numbers `HSL` reads: each its low byte, then a hue above 240 held at 240.
fn hsl_numbers(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Result<(i64, i64, i64), Value> {
    let h = byte(ev, site, slots, 0)?.min(240);
    let s = byte(ev, site, slots, 1)?;
    let l = byte(ev, site, slots, 2)?;
    Ok((h, s, l))
}

fn hsl(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match hsl_numbers(ev, site, slots) {
        Ok((h, s, l)) => color_result(from_hls(h, l, s)),
        Err(e) => e,
    }
}

// ---- the parts of a colour ------------------------------------------------------------------------------------------------

/// `RED`, `GREEN` and `BLUE`: input that cannot be read gives 0.
fn channel(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, f: fn(Rgb) -> u8) -> Value {
    let Ok(v) = crate::convert::slot(slots, 0) else { return bad() };
    match ev.read_color(v, site.env) {
        Ok(c) => Value::num(c.map_or(0.0, |c| f64::from(f(c)))),
        Err(e) => e,
    }
}

fn red(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    channel(ev, site, slots, |c| c.r)
}

fn green(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    channel(ev, site, slots, |c| c.g)
}

fn blue(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    channel(ev, site, slots, |c| c.b)
}

/// `HUE`, `SAT` and `LUM`: the part of the colour, except that a colour that comes straight out of an `HSL`
/// call keeps its three numbers (`which` picks hue 0, saturation 1, lightness 2); input that cannot be read gives 0.
fn hls_part(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, which: usize) -> Value {
    let Some(arg) = site.arg(0) else { return bad() };
    if let Expr::Call(c) = arg
        && c.is("HSL")
        && c.qualifiers.is_empty()
    {
        let mut slots: Vec<Option<Value>> = Vec::new();
        for a in &c.args {
            slots.push(a.as_ref().map(|e| ev.eval(e, site.env)));
        }
        if let Some(failure) = slots.iter().flatten().find(|v| v.is_failure()) {
            return failure.clone();
        }
        if slots.len() != 3 {
            return bad();
        }
        return match hsl_numbers(ev, site, &slots) {
            Ok((h, s, l)) => Value::num([h, s, l][which] as f64),
            Err(e) => e,
        };
    }
    let v = ev.eval(arg, site.env);
    match ev.read_color(&v, site.env) {
        Ok(Some(c)) => {
            let (h, l, s) = to_hls(c);
            Value::num([h, s, l][which] as f64)
        }
        Ok(None) => Value::num(0.0),
        Err(e) => e,
    }
}

fn hue(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    hls_part(ev, site, 0)
}

fn sat(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    hls_part(ev, site, 1)
}

fn lum(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    hls_part(ev, site, 2)
}

/// `HUEDIFF`, `SATDIFF` and `LUMDIFF`: the first colour's part less the second's.
fn part_diff(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, pick: fn((i64, i64, i64)) -> i64) -> Value {
    match (ev.color_arg(site, slots, 0), ev.color_arg(site, slots, 1)) {
        (Ok(a), Ok(b)) => Value::num((pick(to_hls(a)) - pick(to_hls(b))) as f64),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

fn huediff(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part_diff(ev, site, slots, |(h, _, _)| h)
}

fn lumdiff(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part_diff(ev, site, slots, |(_, l, _)| l)
}

fn satdiff(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part_diff(ev, site, slots, |(_, _, s)| s)
}

// ---- moving a colour ------------------------------------------------------------------------------------------------------

/// The colour and the amount of `TINT`, `SHADE` and `TONE`: `vColor` and `vSignedInt`.
fn color_and_amount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Result<(Rgb, i64), Value> {
    let c = ev.color_arg(site, slots, 0)?;
    let amount = signed_int(ev.arg_double(site, slots, 1)?)?;
    Ok((c, amount))
}

fn tint(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match color_and_amount(ev, site, slots) {
        Ok((c, amount)) => {
            let (h, l, s) = to_hls(c);
            color_result(from_hls(h, step(l, amount), s))
        }
        Err(e) => e,
    }
}

fn shade(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match color_and_amount(ev, site, slots) {
        Ok((c, amount)) => {
            let (h, l, s) = to_hls(c);
            color_result(from_hls(h, step(l, -amount), s))
        }
        Err(e) => e,
    }
}

fn tone(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match color_and_amount(ev, site, slots) {
        Ok((c, amount)) => {
            let (h, l, s) = to_hls(c);
            color_result(from_hls(h, l, step(s, -amount)))
        }
        Err(e) => e,
    }
}

/// `MSOTINT(color, delta)`: lightness up by `p` percent of the room to the top, or down by `|p|` percent of itself, in single-precision
/// real numbers, each channel cut to a whole number. `delta` is held to -100 to 100.
fn mso_tint(c: Rgb, delta: i64) -> Rgb {
    let p = delta.clamp(-100, 100) as f32;
    let (r, g, b) = (f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0);
    let top = r.max(g).max(b);
    let low = r.min(g).min(b);
    let l = (top + low) / 2.0;
    let span = top - low;
    let s = if span == 0.0 {
        0.0
    } else if l <= 0.5 {
        span / (top + low)
    } else {
        span / (2.0 - top - low)
    };
    let h = if span == 0.0 {
        0.0
    } else {
        let sector = if top == r {
            (g - b) / span
        } else if top == g {
            2.0 + (b - r) / span
        } else {
            4.0 + (r - g) / span
        };
        let h = sector / 6.0;
        if h < 0.0 { h + 1.0 } else { h }
    };
    let l2 = if p >= 0.0 { 1.0 - (1.0 - l) * (1.0 - p / 100.0) } else { l * (1.0 - p.abs() / 100.0) };
    let cut = |x: f32| (x * 255.0).trunc().clamp(0.0, 255.0) as u8;
    if s == 0.0 {
        let v = cut(l2);
        return Rgb { r: v, g: v, b: v };
    }
    let q = if l2 <= 0.5 { l2 * (1.0 + s) } else { l2 + s - l2 * s };
    let w = 2.0 * l2 - q;
    let channel = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let x = if t < 1.0 / 6.0 {
            w + (q - w) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            w + (q - w) * (2.0 / 3.0 - t) * 6.0
        } else {
            w
        };
        cut(x)
    };
    Rgb { r: channel(h + 1.0 / 3.0), g: channel(h), b: channel(h - 1.0 / 3.0) }
}

fn msotint(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match color_and_amount(ev, site, slots) {
        Ok((c, delta)) => color_result(mso_tint(c, delta)),
        Err(e) => e,
    }
}

fn msoshade(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match color_and_amount(ev, site, slots) {
        Ok((c, delta)) => color_result(mso_tint(c, -delta)),
        Err(e) => e,
    }
}

/// `BLEND(color1, color2, fraction)`: a mix channel by channel, `fraction` being the share of `color2`; each channel rounds to the nearest
/// whole number (project rule). A fraction outside 0 to 1 is `#VALUE!`.
fn blend(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let (a, b) = match (ev.color_arg(site, slots, 0), ev.color_arg(site, slots, 1)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let f = match ev.arg_double(site, slots, 2) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&f) {
        return Value::Error(CellError::Value);
    }
    let mix = |x: u8, y: u8| (f64::from(x) * (1.0 - f) + f64::from(y) * f).round().clamp(0.0, 255.0) as u8;
    color_result(Rgb { r: mix(a.r, b.r), g: mix(a.g, b.g), b: mix(a.b, b.b) })
}

// ---- the theme ------------------------------------------------------------------------------------------------------------

impl Evaluator<'_, '_> {
    /// One of the five scheme cells it: a number found along the sheet, and 65534 reads the enclosing group's cell where the
    /// shape is a member of a group that selects a theme (`scheme_group`), and else the page sheet's own cell, a page holding none being 0. The
    /// same of `VariationColorIndex`, `VariationStyleIndex` and `EmbellishmentIndex`.
    ///
    /// **The oracle's second reading** ([`Evaluator::under_first_theme_part`]): of the five scheme cells, one that the sheet and the page hold none of (the
    /// cell is absent, or 65534 over a page that holds none) reads [`FIRST_THEME_PART`] and not 0, so that `THEMEVAL` finds the file's first theme part
    /// (the narrowing, a project rule). A cell held as a number, 0 included, reads as it is held.
    ///
    /// **These readings are project rules, which no sample tests (B; check-list rows):** that an absent cell, or 65534 over a page that
    /// holds none, selects the first part; that an outright 0 is a sheet that says it reads no theme, and stays 0 in the second reading as in the first; and
    /// that a page that holds its own 65534 is not a page that holds none (the number is read as it is, and no part holds it).
    pub(crate) fn scheme_cell(&mut self, sheet: &SheetRef, name: &str) -> f64 {
        let own = self.read_cell(sheet, &CellAddr::direct(name));
        let n = own.view.quantity().map_or(0.0, |q| q.stored);
        // What the sheet holds none of: 0, and the first part when the second reading is on and the cell is one of the five.
        let none = if self.ctx.first_theme_part && SCHEME_CELLS.contains(&name) { FIRST_THEME_PART } else { 0.0 };
        if n != 65534.0 {
            return if n == 0.0 && !own.found { none } else { n };
        }
        if let Some(group) = self.scheme_group(sheet)
            && let Some(held) = self.group_cell(&group, name)
        {
            return held;
        }
        let page = match sheet {
            SheetRef::Shape(s) => match s.owner {
                Owner::Page(p) => SheetRef::Page(p),
                Owner::Master(m) => SheetRef::Master(m),
            },
            other => other.clone(),
        };
        if &page == sheet {
            return none;
        }
        let read = self.read_cell(&page, &CellAddr::direct(name));
        if !read.found {
            return none;
        }
        read.view.quantity().map_or(0.0, |q| q.stored)
    }

    /// The five scheme cells: `ThemeIndex`, `ColorSchemeIndex`, `EffectSchemeIndex`, `ConnectorSchemeIndex`, `FontSchemeIndex`.
    pub(crate) fn scheme_cells(&mut self, sheet: &SheetRef) -> [f64; 5] {
        SCHEME_CELLS.map(|n| self.scheme_cell(sheet, n))
    }

    /// The group whose scheme cells a shape reads where its own chain gives 65534, in place of the page's (group members;  `inferred-from-sample`, 124 files, 35 sources).
    /// A member of a group that **holds none of the five cells itself** reads the **nearest
    /// enclosing group whose own sheet holds all five as numbers other than 0 and 65534**, nearest first and before the page. A member that holds
    /// some or all of the five is left to its own cells and chain (the rule says that case is unknown). The member's chain giving 0 is not this
    /// step at all: [`Evaluator::scheme_cell`] asks only where the chain gives 65534. The answer is found once for a shape and kept.
    fn scheme_group(&mut self, sheet: &SheetRef) -> Option<SheetRef> {
        let SheetRef::Shape(at) = sheet else { return None };
        if at.path.len() < 2 {
            return None;
        }
        if let Some(known) = self.scheme_groups.get(sheet) {
            return known.clone();
        }
        let found = self.find_scheme_group(sheet, at);
        // The answer is held memory, accounted.
        if self.ctx.hold_bytes(KEY_BYTES) {
            self.scheme_groups.insert(sheet.clone(), found.clone());
        }
        found
    }

    fn find_scheme_group(&mut self, sheet: &SheetRef, at: &ShapeRef) -> Option<SheetRef> {
        if SCHEME_CELLS.iter().any(|n| self.local_cell(sheet, &CellAddr::direct(n)).is_some()) {
            return None;
        }
        // The groups above the shape, nearest first: a step each (a path is at most `MAX_SHAPE_DEPTH` long).
        for depth in (1..at.path.len()).rev() {
            if !self.steps(1) {
                return None;
            }
            let group = SheetRef::Shape(ShapeRef { owner: at.owner, path: at.path.get(..depth)?.to_vec() });
            if SCHEME_CELLS.iter().all(|n| self.group_number(&group, n).is_some_and(|x| x != 0.0 && x != 65534.0)) {
                return Some(group);
            }
        }
        None
    }

    /// The number the sheet of a group holds itself in a cell, and not one it inherits.
    fn group_number(&mut self, group: &SheetRef, name: &str) -> Option<f64> {
        let addr = CellAddr::direct(name);
        self.local_cell(group, &addr)?;
        self.read_cell(group, &addr).view.quantity().map(|q| q.stored)
    }

    /// What the group holds in one of the eight cells it hands a member, where it holds a number that is not 65534 (that reads the page).
    fn group_cell(&mut self, group: &SheetRef, name: &str) -> Option<f64> {
        if !SCHEME_CELLS.contains(&name) && !GROUP_EXTRA_CELLS.contains(&name) {
            return None;
        }
        self.group_number(group, name).filter(|x| *x != 65534.0)
    }
}

/// The five scheme cells.
const SCHEME_CELLS: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The three cells a group hands a member beside the five, where it holds them.
const GROUP_EXTRA_CELLS: [&str; 3] = ["VariationColorIndex", "VariationStyleIndex", "EmbellishmentIndex"];

fn isthemed(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    // The colour scheme cell by is above 0 (over MS-VSDX's four cells).
    Value::Bool(ev.scheme_cell(&site.target.sheet, "ColorSchemeIndex") > 0.0)
}

fn cellisthemed(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_bool(site, slots, 0) {
        Ok(b) => Value::Bool(b),
        Err(e) => e,
    }
}

/// `THEMEGUARD(value)`: its argument.
fn themeguard(_: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    crate::convert::slot(slots, 0).cloned().unwrap_or_else(|e| e)
}

/// `THEMERESTORE(value)`: 0, with its argument not worked out.
fn themerestore(_: &mut Evaluator<'_, '_>, _: &Site<'_>) -> Value {
    Value::num(0.0)
}

/// `USE(master)`: 254, always. The name only names a master.
fn use_call(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    Value::num(254.0)
}

/// An answer under the name of the function that was asked, so that a report counts the right one as unsupported.
fn named(v: Value, name: &str) -> Value {
    if matches!(v, Value::Unsupported(_)) { Value::unsupported(name) } else { v }
}

/// `THEMEPROP(name)`: any of the five scheme cells at 0 gives 2 for `Embellishment` and 0 for `Multiformat` (the first against
/// MS-VSDX's 0, by the samples); the rest reads the variants of the theme part. A name other than `Embellishment` and
/// `Multiformat` is `#VALUE!`.
fn themeprop(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let name = match ev.arg_text(slots, 0) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // Compared in place: a name of a megabyte is not copied to be lower-cased, and is not either word.
    let embellishment = name.eq_ignore_ascii_case("embellishment");
    if !embellishment && !name.eq_ignore_ascii_case("multiformat") {
        return Value::Error(CellError::Value);
    }
    let cells = ev.scheme_cells(&site.target.sheet);
    if cells.contains(&0.0) {
        return Value::num(if embellishment { EMBELLISHMENT_WITH_NO_THEME } else { 0.0 });
    }
    let answer = ev.select(&site.target.sheet, cells).and_then(|sel| if embellishment { ev.embellishment(&sel) } else { ev.multiformat(&sel) });
    named(answer.unwrap_or_else(|v| v), "THEMEPROP")
}

/// `THEMECBV(color, stop)`: the stop is checked (1 to 10); with no theme the colour comes back as it is;
/// otherwise the modifiers of that stop of the fill entry are applied to it.
fn themecbv(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let c = match ev.color_arg(site, slots, 0) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let stop = match ev.arg_double(site, slots, 1) {
        Ok(q) => q.stored.trunc(),
        Err(e) => return e,
    };
    if !(1.0..=10.0).contains(&stop) {
        return Value::Error(CellError::Value);
    }
    let cells = ev.scheme_cells(&site.target.sheet);
    if cells.contains(&0.0) {
        return color_result(c);
    }
    let answer = ev.select(&site.target.sheet, cells).and_then(|sel| ev.stop_modifiers(&sel, c, stop as usize)).map(color_result);
    named(answer.unwrap_or_else(|v| v), "THEMECBV")
}

/// The property names of and their no-theme colours: MS-VSDX's numbering 0 to 8 for the scheme colours.
pub(crate) const THEMEVAL_COLOURS: [(&str, u32); 15] = [
    ("Dark", 0x000000),
    ("Light", 0xFFFFFF),
    ("AccentColor", 0xC05046),
    ("AccentColor2", 0x9DBB61),
    ("AccentColor3", 0xAB9AC0),
    ("AccentColor4", 0x4BACC6),
    ("AccentColor5", 0xF59D56),
    ("AccentColor6", 0xFFC000),
    ("BackgroundColor", 0xFFFFFF),
    ("TextColor", 0x000000),
    ("FillColor", 0x96AFCF),
    ("FillColor2", 0xBFCEE1),
    ("LineColor", 0x1F477D),
    ("ConnectorColor", 0x1F477D),
    ("ShadowColor", 0x464646),
];

pub(crate) fn rgb_of(n: u32) -> Value {
    Value::Color(Rgb { r: (n >> 16) as u8, g: (n >> 8) as u8, b: n as u8 })
}

/// What a `THEMEVAL` call asks about.
enum Ask {
    /// A name, in any case.
    Name(String),
    /// A number: MS-VSDX's 0 to 8, or the value of a Quick Style colour cell, 100 to 106 and 200 to 206.
    Number(f64),
    /// No argument: the property of the cell the call sits in.
    Host(Option<CellAddr>),
}

/// The property a call names, worked out. An error or an unsupported function in the argument is the answer.
fn read_ask(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Result<Ask, Value> {
    let Some(e) = site.arg(0) else { return Ok(Ask::Host(site.env.cell.clone())) };
    match ev.eval(e, site.env) {
        Value::Text(t) => {
            // The property is copied: charged by its length.
            if !ev.work(t.len()) {
                return Err(Value::Error(LOOP_ERROR));
            }
            // The page prints a number as a quoted digit; MS-VSDX's numbering is the one followed, so `"5"` is 5 (ASSUMED: no spec says
            // whether a string of digits reads as a number).
            if (1..=4).contains(&t.len())
                && t.bytes().all(|b| b.is_ascii_digit())
                && let Ok(n) = t.parse::<u16>()
            {
                return Ok(Ask::Number(f64::from(n)));
            }
            Ok(Ask::Name(t.to_string()))
        }
        Value::Number(q) => Ok(Ask::Number(q.stored)),
        v @ (Value::Error(_) | Value::Unsupported(_)) => Err(v),
        _ => Err(Value::Error(CellError::Value)),
    }
}

/// `THEMEVAL([property][, default])`: with no theme selected `default`, or for a call naming a property the table's
/// colour and the root style's cell for any other property, or for a call naming none the root style's cell of the host's name, colours
/// included (R3 steps 2 and 3); under an explicit theme the answer is in the theme part (`theme_eval.rs`), and where the file
/// has no part, or the part's entry could not be read, it is unsupported and the saved value stands.
fn themeval(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let sheet = site.target.sheet.clone();
    let ask = match read_ask(ev, site) {
        Ok(a) => a,
        Err(v) => return v,
    };
    let cells = ev.scheme_cells(&sheet);
    let has_default = site.arg(1).is_some();
    let no_theme = if has_default { cells.contains(&0.0) } else { cells[0] == 0.0 || cells[1] == 0.0 || cells[4] == 0.0 };
    if no_theme {
        return themeval_without_theme(ev, site, ask);
    }
    // The property is known before the theme is: a name the table lacks is `#VALUE!` whatever the file holds.
    let property = match &ask {
        Ask::Name(n) => canonical(n),
        // A host in no table asks for a property the theme has no answer for: the root style's cell of the host's name, with a theme as without.
        Ask::Host(Some(addr)) => match host_property(addr) {
            Some(p) => Some(p),
            None => return themeval_uncovered_host(ev, addr),
        },
        Ask::Host(None) => None,
        Ask::Number(x) => number_property(*x),
    };
    let Some(property) = property else { return Value::Error(CellError::Value) };
    let answer = ev.select(&sheet, cells).and_then(|sel| match (&ask, property) {
        (Ask::Number(x), P::Other) => ev.quick_property(&sel, *x),
        // A scheme of 0 for the lists is no list: the fallback of for formats.
        (_, p) if needs_lists(p) && cells[if sel.connector { 3 } else { 2 }] == 0.0 => ev.fallback(p, false),
        (_, p) => ev.property(&sel, p),
    });
    answer.unwrap_or_else(|v| v)
}

/// A number as a property: 0 to 8 are the scheme colours, and a Quick Style colour value (`P::Other`, answered by its number) is a colour of
/// the variant. Any other number is not in the table.
fn number_property(x: f64) -> Option<P> {
    if !x.is_finite() {
        return None;
    }
    match x.trunc() as i64 {
        n @ 0..=8 => Some(P::Slot(n as u8)),
        100..=106 | 200..=206 => Some(P::Other),
        _ => None,
    }
}

/// Whether a property is read from the lists of the effect or connector scheme, and not from the colour or font scheme alone.
fn needs_lists(p: P) -> bool {
    !matches!(p, P::Slot(_) | P::VariantColour(_) | P::LatinFont | P::Other)
}

/// `THEMEVAL` where the sheet selects no theme (R3 steps 2 and 3): the name test first, since a `default` does not save an
/// unknown name (our rule); then `default`; then, for a call that names a property, the table's colour for a colour and the root style's
/// cell for any other property (the table), and for a call that names none the root style's cell of the host's name, colours included.
fn themeval_without_theme(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, ask: Ask) -> Value {
    let default = site.arg(1);
    match ask {
        Ask::Name(name) => {
            let Some(property) = canonical(&name) else { return Value::Error(CellError::Value) };
            if let Some(default) = default {
                return ev.eval(default, site.env);
            }
            named_without_theme(ev, &name, property)
        }
        Ask::Number(x) => {
            // MS-VSDX's numbering 0 to 8 names the scheme colours of the table; a Quick Style cell's value (100 and above) reads the theme, and
            // with no default and no theme there is nothing to read.
            let colour = THEMEVAL_COLOURS.get(x.trunc() as usize).filter(|_| x.is_finite() && (0.0..=8.0).contains(&x.trunc()));
            match (default, colour) {
                (Some(default), _) => ev.eval(default, site.env),
                (None, Some((_, rgb))) => rgb_of(*rgb),
                (None, None) => Value::unsupported("THEMEVAL"),
            }
        }
        Ask::Host(addr) => {
            if let Some(default) = default {
                return ev.eval(default, site.env);
            }
            let Some(addr) = addr else { return Value::Error(CellError::Value) };
            // A call naming no property, which is what a cell holding the marker `Themed` stands for, reads its own property, and with no
            // theme that is the root style's cell of the host's name, colours included. MS-VSDX's column of
            // colours is for a call that names one, which the arm above gives.
            match host_property(&addr) {
                // In the table, and not worked out from a theme here (the other fonts, `UseGroupGradient`): the root's cell of the same name.
                Some(P::Other) => ev.read_cell(&SheetRef::Style(0), &other_host_cell(&addr)).view.clone(),
                Some(p) => themeval_host_root(ev, site, p),
                None => themeval_uncovered_host(ev, &addr),
            }
        }
    }
}

/// A named property with no theme and no `default` (column): the table's colour for a colour, the accents for the
/// variant colours and black for the seventh, the number 4 for `ConnectorEnd2`, and the root style's cell the name maps to for every other name.
fn named_without_theme(ev: &mut Evaluator<'_, '_>, name: &str, property: P) -> Value {
    // R15 gives `ConnectorEnd2` the number 4 in every sample, with no theme, where the root's `EndArrow` holds another value. Under a theme the
    // name reads as `ConnectorEnd`.
    if name.eq_ignore_ascii_case("ConnectorEnd2") {
        return Value::num(4.0);
    }
    match property {
        P::Slot(k) => THEMEVAL_COLOURS.get(usize::from(k)).map_or_else(|| Value::unsupported("THEMEVAL"), |(_, rgb)| rgb_of(*rgb)),
        P::VariantColour(k) => variant_without_theme(k),
        p => match table_name(p) {
            Some(colour) => {
                THEMEVAL_COLOURS.iter().find(|(n, _)| *n == colour).map_or_else(|| Value::unsupported("THEMEVAL"), |(_, rgb)| rgb_of(*rgb))
            }
            None => themeval_root_cell(ev, name),
        },
    }
}

/// `VariantColor1` to `VariantColor7` with no theme and no `default`: the table's six accents in order, `AccentColor` to `AccentColor6`, and for
/// the seventh `#000000`, the table's `Dark` (737 cells in 33 files with no theme part, and the same in 274 cells of files with
/// one).
fn variant_without_theme(k: u8) -> Value {
    // Accent colours stand at 2 to 7 of the table: the variant colour `k`, counting from 0, is the table's entry `k + 2`; the
    // seventh is its entry 0, `Dark`.
    let entry = if k < 6 { usize::from(k) + 2 } else { 0 };
    THEMEVAL_COLOURS.get(entry).map_or_else(|| Value::unsupported("THEMEVAL"), |(_, rgb)| rgb_of(*rgb))
}

/// A call that names no property, in a cell the table does not cover (`TextBkgnd`, a `Character` row's `ColorTrans`, a stop row past the ten): the
/// root style's cell at the host's own address, and 0 where the root holds none, with a theme selected or none (6,990 cells with no
/// theme and 1,902 under one agree, and no call with no argument gives `#VALUE!`).
fn themeval_uncovered_host(ev: &mut Evaluator<'_, '_>, addr: &CellAddr) -> Value {
    let root = ev.read_cell(&SheetRef::Style(0), addr);
    if root.found { root.view.clone() } else { Value::num(0.0) }
}

/// What a call that names no property gives in a cell of the table, where the sheet selects no theme and the call has no `default`: the root
/// style's cell of the cell's own property. The root holds a colour cell's value as an index of the palette or as a colour,
/// and the call gives a colour, as `THEMEVAL` does in every other case; a root cell that is not a colour (a number of a pattern, a weight, a
/// flag) is given as it is.
fn themeval_host_root(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, p: P) -> Value {
    let root = ev.fallback(p, true).unwrap_or_else(|v| v);
    if table_name(p).is_none() {
        return root;
    }
    match ev.read_color(&root, site.env) {
        Ok(Some(c)) => Value::Color(c),
        Ok(None) => root,
        Err(v) => v,
    }
}

/// A property that is no colour of the table, on a sheet with no theme: the root style's cell that the name maps to (
/// `ConnectorWeight` is `LineWeight`, `ConnectorBeginSize` is `BeginArrowSize`, `ShadowXOffset` is `ShapeShdwOffsetX`, `LatinFont` is the `Font`
/// of the first `Character` row, `FillStop2Position` is the `GradientStopPosition` of the second `FillGradient` row). A name the table lacks is
/// `#VALUE!`, and the cell that is not in the root is the cell's own default.
fn themeval_root_cell(ev: &mut Evaluator<'_, '_>, name: &str) -> Value {
    let Some(addr) = mapped_root_cell(name) else { return Value::Error(CellError::Value) };
    ev.read_cell(&SheetRef::Style(0), &addr).view.clone()
}

/// `THEME(property)`: a colour of the table at the row `User.msvThemeColors` holds, or an effect of the at the row
/// `User.msvThemeEffects` holds; 254 reads a custom scheme from the master `USE` names; a row that is not there is scheme 0.
fn theme(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let property = match ev.arg_text(slots, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let colour = COLOUR_PROPERTIES.iter().position(|p| p.eq_ignore_ascii_case(&property));
    let effect = EFFECT_PROPERTIES.iter().position(|p| p.eq_ignore_ascii_case(&property));
    let latin = property.eq_ignore_ascii_case("LatinFont");
    if colour.is_none() && effect.is_none() && !latin {
        return Value::Error(CellError::Value);
    }
    let sheet = site.target.sheet.clone();
    let row = if colour.is_some() { "msvThemeColors" } else { "msvThemeEffects" };
    let addr = CellAddr::in_row("User", None, RowKey::name(row), "Value");
    let held = ev.read_cell(&sheet, &addr);
    let scheme = if held.found { held.view.quantity().map_or(0.0, |q| q.stored) } else { 0.0 };
    if scheme == 254.0 {
        return custom_theme(ev, &sheet, &addr, &property);
    }
    let scheme = if scheme.is_finite() && scheme >= 0.0 { scheme as usize } else { 0 };
    // A scheme number past the table reads scheme 0.
    if let Some(i) = colour {
        let table = COLOUR_SCHEMES.get(scheme).or_else(|| COLOUR_SCHEMES.first());
        return table.and_then(|row| row.get(i)).map_or_else(bad, |c| rgb_of(*c));
    }
    let table = EFFECT_SCHEMES.get(scheme).or_else(|| EFFECT_SCHEMES.first());
    let Some((font, numbers)) = table else { return bad() };
    if latin {
        return Value::text(font);
    }
    let Some(i) = effect else { return bad() };
    let x = numbers.get(i).copied().unwrap_or(0.0);
    // Lengths are in inches and angles in radians, as a cell stores them.
    let unit = if property.ends_with("Weight") || property.ends_with("Offset") {
        Unit::DrawingDefault
    } else if property.ends_with("Direction") {
        Unit::AngleDefault
    } else {
        Unit::Unitless
    };
    Value::Number(Quantity::new(x, unit))
}

/// A custom scheme: the master the `USE` in the row's cell names, its `User` row `msvTheme` plus the property. With no master
/// that matches, scheme 0 (project rule).
fn custom_theme(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, addr: &CellAddr, property: &str) -> Value {
    let name = ev.res.resolve(sheet, addr).and_then(|r| match r.formula.cell.map(|c| &c.formula) {
        Some(FormulaSlot::Local(f)) => match &f.ast {
            Ok(Expr::Call(c)) if c.is("USE") => match c.args.first() {
                Some(Some(Expr::Str(s))) | Some(Some(Expr::Guid(s))) => Some(Arc::clone(s)),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    });
    let master = name.and_then(|n| ev.master_named_or_unique(&n));
    if let Some(id) = master {
        let shape = SheetRef::Shape(ShapeRef::top(Owner::Master(id), 0));
        // The row's name is built from the property, charged by its length.
        if !ev.work(property.len()) {
            return Value::Error(LOOP_ERROR);
        }
        let cell = CellAddr::in_row("User", None, RowKey::name(&format!("msvTheme{property}")), "Value");
        let read = ev.read_cell(&shape, &cell);
        if read.found {
            return read.view.clone();
        }
    }
    // Scheme 0.
    let i = COLOUR_PROPERTIES.iter().position(|p| p.eq_ignore_ascii_case(property));
    match i {
        Some(i) => COLOUR_SCHEMES.first().and_then(|row| row.get(i)).map_or_else(bad, |c| rgb_of(*c)),
        None => bad(),
    }
}

/// `FONT(name)` and `FONTTOID(name)`: the font's identifier. The model's font table is the document's `FaceName` list, and the identifier
/// is the place a face has in it, which is the host's; a name it does not know is `#VALUE!`.
fn font(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    let name = match ev.arg_text(slots, 0) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match ev.font_place(&name) {
        Some(i) => Value::num(i as f64),
        None => Value::Error(CellError::Value),
    }
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("BLEND", 3, 3, blend),
    eager("BLUE", 1, 1, blue),
    eager("CELLISTHEMED", 1, 1, cellisthemed),
    eager("FONT", 1, 1, font),
    eager("FONTTOID", 1, 1, font),
    eager("GREEN", 1, 1, green),
    eager("HSL", 3, 3, hsl),
    lazy("HUE", 1, 1, hue),
    eager("HUEDIFF", 2, 2, huediff),
    eager("ISTHEMED", 0, 0, isthemed),
    lazy("LUM", 1, 1, lum),
    eager("LUMDIFF", 2, 2, lumdiff),
    eager("MSOSHADE", 2, 2, msoshade),
    eager("MSOTINT", 2, 2, msotint),
    eager("RED", 1, 1, red),
    eager("RGB", 3, 3, rgb),
    lazy("SAT", 1, 1, sat),
    eager("SATDIFF", 2, 2, satdiff),
    eager("SHADE", 2, 2, shade),
    eager("THEME", 1, 1, theme),
    eager("THEMECBV", 2, 2, themecbv),
    eager("THEMEGUARD", 1, 1, themeguard),
    eager("THEMEPROP", 1, 1, themeprop),
    lazy("THEMERESTORE", 1, 1, themerestore),
    lazy("THEMEVAL", 0, 2, themeval),
    eager("TINT", 2, 2, tint),
    eager("TONE", 2, 2, tone),
    eager("USE", 1, 1, use_call),
];
