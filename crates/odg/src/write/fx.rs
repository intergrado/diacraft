//! Shadows, gradients and hatches made in DiaCraft, written natively.
//!
//! **What is written.** A style says what it can for a program that draws it, and DiaCraft's data carries the exact cells, as it does for every cell the markup cannot say whole:
//!
//! * **A shadow**: `draw:shadow`, `-color`, `-opacity`, `-offset-x` and `-offset-y` are the cells' own (`stylemap`), which the reader gives back; the type 1 comes with a visible shadow
//!   ([`display_of`] says so to the rebuild). The blur and the size are carried.
//! * **A hatch**: a pattern of 2 to 7, 13 to 16 and 19 to 23 is a `draw:hatch` of DiaCraft's own in `office:styles` ([`HatchKey`]: its name says the number and the colour of the
//!   marks), named by `draw:fill-hatch-name`, over the ground of `draw:fill-color` (`draw:fill-hatch-solid`). The pattern, the marks and the ground are carried; the reader gives back the solid
//!   stand-in, which [`Display`] tells the rebuild. A stipple (8 to 12, 17, 18 and 24) is drawn as one flat mixture of its two colours (`render`'s `pattern.rs`), and ODF has no such
//!   fill, so it stays as it was: carried, with the marks' colour as its fill.
//! * **A gradient**: a `draw:gradient` of DiaCraft's own in `office:styles` ([`GradientKey`]: its name says its layout, its angle and its stops, so that the element is made
//!   from the name, as a line end's marker is made from its number), with the stops beyond two (or not at 0 and 1) as `loext:gradient-stop` children, the form LibreOffice reads (measured:
//!   LibreOffice 26.8.2.2 reads no `svg:linearGradient` fill). The stops' colours and places are what the name holds; their transparency, the angle's exact value and everything else are carried.
//!
//! **The measures** (LibreOffice 26.8.2.2 as a black box, in the task's report): `draw:angle` turns the gradient **counterclockwise** as seen on the page from a start at the top (0 is top to
//! bottom, 90 is left to right), and a number with no unit is degrees, so DiaCraft's angle *a* (clockwise from the right) is `draw:angle` = 90 − *a* degrees, written with `deg`. In a
//! radial or rectangular gradient the **start** colour is at the outer edge and the end colour at the centre, so the first stop of the rows (position 0, at the origin) is the end colour. A hatch's
//! `draw:rotation` is degrees counterclockwise from the right and a number with no unit is tenths of a degree, so `deg` is always written; `draw:distance` is measured across the lines.
//!
//! Every definition is **made from its name** ([`Def::of_name`]), so a save that names one adds it when the file lacks it and takes it out when no style names it after the save, by the
//! machinery that line ends use (`markers.rs`).

use diacraft_doc::{Cell, SectionKey, Sheet, Value};

use super::stylemap::{Op, Val};
use crate::ext::native::Display;
use crate::read::names::Ns;

/// The most stops a gradient has.
pub(crate) const MOST_STOPS: usize = 10;

/// The prefix of DiaCraft's own gradient names.
pub(crate) const GRADIENT_PREFIX: &str = "DiaCraft_20_gradient_20_";
/// The prefix of DiaCraft's own hatch names.
pub(crate) const HATCH_PREFIX: &str = "DiaCraft_20_hatch_20_";

/// The namespace of LibreOffice's extension, in which its gradient stops and its shadow's blur are written.
pub(crate) const LOEXT_URI: &str = crate::read::names::LOEXT_URI;

/// The separator a name uses: a space, as ODF writes it.
const SEP: &str = "_20_";

/// One stop of a gradient: its colour (`0xRRGGBB`) and its place in thousandths of the way along (0 to 1000).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Stop {
    pub rgb: u32,
    pub pos: u16,
}

/// A gradient of DiaCraft's own: the layout as `FillGradientDir` says it (0 linear, 1 to 7 radial, 8 to 12 rectangular), the angle as `draw:angle` says it (in tenths of a degree, 0 to
/// 3599: counterclockwise from a start at the top, and 0 for a layout that has none), and its stops, from the one at position 0 on, in position order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GradientKey {
    pub dir: u8,
    pub angle: u16,
    pub n: u8,
    pub stops: [Stop; MOST_STOPS],
}

/// A hatch of DiaCraft's own: the fill pattern (2 to 7, 13 to 16, 19 to 23) and the colour of its marks (`0xRRGGBB`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct HatchKey {
    pub pattern: u8,
    pub rgb: u32,
}

/// What a name in `office:styles` of DiaCraft's own makes: a gradient or a hatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Fx {
    Gradient(GradientKey),
    Hatch(HatchKey),
}

fn hex6(rgb: u32) -> String {
    format!("{:06x}", rgb & 0x00ff_ffff)
}

/// The colour of a `#rrggbb` text, in either case; `None` for any other text.
pub(crate) fn rgb_of(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// [`GradientKey::lo_scale`] of the layout `dir` (`FillGradientDir`): 2 from a corner, the root of 5/4 over the root of 1/2 from the middle of an edge, and 1 from the centre and for the layouts
/// with no centre. The reader divides a file's places by the same number (`read/gradient.rs`), so that a gradient it reads is the one this module would write.
pub(crate) fn lo_scale_of(dir: u8) -> f64 {
    match dir {
        1 | 2 | 6 | 7 | 8 | 9 | 11 | 12 => 2.0,
        4 | 5 => 1.581_138_83,
        _ => 1.0,
    }
}

/// A number of thousandths as the shortest decimal (`250` is `0.25`, `1000` is `1`).
fn thousandths_text(n: u32) -> String {
    let (int, frac) = (n / 1000, n % 1000);
    if frac == 0 {
        return int.to_string();
    }
    let mut text = format!("{int}.{frac:03}");
    while text.ends_with('0') {
        text.pop();
    }
    text
}

/// A number of tenths as the shortest decimal (`905` is `90.5`, `900` is `90`).
fn tenths_text(n: u32) -> String {
    let (int, frac) = (n / 10, n % 10);
    if frac == 0 { int.to_string() } else { format!("{int}.{frac}") }
}

impl GradientKey {
    /// The layout's name in `draw:style`, and where its centre is (`draw:cx`, `draw:cy`, percentages, from the top left): the origins. `None` for a layout ODF has no form for
    /// (13, along the shape's path).
    pub(crate) fn layout(self) -> Option<(&'static str, Option<(u8, u8)>)> {
        Some(match self.dir {
            0 => ("linear", None),
            1 => ("radial", Some((100, 100))),
            2 => ("radial", Some((0, 100))),
            3 => ("radial", Some((50, 50))),
            4 => ("radial", Some((50, 100))),
            5 => ("radial", Some((50, 0))),
            6 => ("radial", Some((100, 0))),
            7 => ("radial", Some((0, 0))),
            8 => ("rectangular", Some((100, 100))),
            9 => ("rectangular", Some((0, 100))),
            // 10 and 13: the path gradient follows the shape's outline inward, which ODF has no form for; the rectangular one from the centre is the own fallback.
            10 | 13 => ("rectangular", Some((50, 50))),
            11 => ("rectangular", Some((100, 0))),
            12 => ("rectangular", Some((0, 0))),
            _ => return None,
        })
    }

    /// How much sooner than DiaCraft's own the file's gradient reaches its last colour, in a layout that has a centre (measured: LibreOffice 26.8.2.2 ends a radial or a rectangular gradient
    /// at **half** the distance from a corner to the far corner, where DiaCraft's reaches it at the far corner). On a square, from a corner it is 2, from the middle of an edge
    /// the root of 5/4 over the root of 1/2, and from the centre 1. The stops are put at that many times their places, so that the part of the gradient the file draws is DiaCraft's.
    pub(crate) fn lo_scale(self) -> f64 {
        lo_scale_of(self.dir)
    }

    /// The stops in use, in position order.
    fn used(&self) -> &[Stop] {
        self.stops.get(..usize::from(self.n)).unwrap_or(&[])
    }

    /// The name of the definition (`draw:name`): the prefix, the layout, the angle, and each stop as its colour, `p` and its place.
    pub(crate) fn name(&self) -> String {
        let mut name = format!("{GRADIENT_PREFIX}{}{SEP}{}", self.dir, self.angle);
        for s in self.used() {
            name.push_str(&format!("{SEP}{}p{}", hex6(s.rgb), s.pos));
        }
        name
    }

    /// The display name: spaces for the separators, which LibreOffice builds the name from again.
    pub(crate) fn display_name(&self) -> String {
        self.name().replace(SEP, " ")
    }

    /// The gradient a name spells: the canonical form only (the name made again from the key is the name given), within [`MOST_STOPS`], two stops at least, rising places.
    pub(crate) fn of_name(name: &str) -> Option<GradientKey> {
        let rest = name.strip_prefix(GRADIENT_PREFIX)?;
        let mut parts = rest.split(SEP);
        let (dir, angle) = (parts.next()?, parts.next()?);
        (dir.len() <= 2 && angle.len() <= 4).then_some(())?;
        let (dir, angle): (u8, u16) = (dir.parse().ok()?, angle.parse().ok()?);
        let mut stops = [Stop::default(); MOST_STOPS];
        let mut n = 0usize;
        for part in parts {
            let (colour, pos) = part.split_once('p')?;
            (pos.len() <= 4).then_some(())?;
            let slot = stops.get_mut(n)?;
            // The length first: a name of a file may be as long as the file, and a hex number of that many digits is read to its end.
            (colour.len() == 6).then_some(())?;
            *slot = Stop { rgb: u32::from_str_radix(colour, 16).ok()?, pos: pos.parse().ok()? };
            n += 1;
        }
        let key = GradientKey { dir, angle, n: u8::try_from(n).ok()?, stops };
        (n >= 2 && key.valid() && key.name() == name).then_some(key)
    }

    /// Whether the key is a gradient this module writes: a layout ODF has a form for, an angle in range (and none for a layout that has no angle), places that rise from 0 to 1000.
    fn valid(&self) -> bool {
        let n = usize::from(self.n);
        self.layout().is_some()
            && (2..=MOST_STOPS).contains(&n)
            && self.angle < 3600
            && (self.dir == 0 || self.angle == 0)
            && self.dir <= 13
            && self.used().iter().all(|s| s.pos <= 1000 && s.rgb <= 0x00ff_ffff)
            && self.used().windows(2).all(|w| w[0].pos <= w[1].pos)
    }

    /// The colour DiaCraft's ramp has at the place `at` (thousandths of the way along): the stops' colours blended straight, the first before the first stop and the last after
    /// the last.
    fn colour_at(&self, at: f64) -> u32 {
        let used = self.used();
        let (Some(first), Some(last)) = (used.first(), used.last()) else { return 0 };
        if at <= f64::from(first.pos) {
            return first.rgb;
        }
        for pair in used.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if at <= f64::from(b.pos) {
                let span = f64::from(b.pos) - f64::from(a.pos);
                let share = if span > 0.0 { (at - f64::from(a.pos)) / span } else { 1.0 };
                let channel = |shift: u32| {
                    let (x, y) = (f64::from((a.rgb >> shift) & 255), f64::from((b.rgb >> shift) & 255));
                    ((x + (y - x) * share).round().clamp(0.0, 255.0)) as u32
                };
                return (channel(16) << 16) | (channel(8) << 8) | channel(0);
            }
        }
        last.rgb
    }

    /// The stops as the file's gradient has them, by the place along the file's own ramp (thousandths, rising): for a layout with no centre the stops themselves; for one with a centre
    /// each stop at [`GradientKey::lo_scale`] times its place, those past the end left out and the colour DiaCraft has where the file's ramp ends put there.
    fn ramp(&self) -> Vec<(u32, u32)> {
        let used = self.used();
        let k = self.layout().and_then(|(_, centre)| centre).map_or(1.0, |_| self.lo_scale());
        if k == 1.0 {
            return used.iter().map(|s| (u32::from(s.pos), s.rgb)).collect();
        }
        let mut ramp: Vec<(u32, u32)> = used
            .iter()
            .filter(|s| f64::from(s.pos) * k <= 1000.0)
            .map(|s| ((f64::from(s.pos) * k).round().clamp(0.0, 1000.0) as u32, s.rgb))
            .collect();
        if ramp.last().is_none_or(|(at, _)| *at < 1000) {
            ramp.push((1000, self.colour_at(1000.0 / k)));
        }
        ramp
    }

    /// The element as it is written in `office:styles`, under the prefixes `draw` and `svg`: the two colours and the layout for a program that knows no more, and the stops where there are
    /// more than two or they are not at 0 and 1 (`loext:gradient-stop`, the form LibreOffice reads: offsets run from the start, and in a layout that has a centre from the outer edge in).
    pub(crate) fn element(&self, draw: &str, svg: &str, declare: &str) -> String {
        let Some((style, centre)) = self.layout() else { return String::new() };
        let ramp = self.ramp();
        let (Some(first), Some(last)) = (ramp.first().copied(), ramp.last().copied()) else { return String::new() };
        // A linear gradient runs from the first stop to the last; one with a centre has its start at the outer edge, which is the last stop, and its end at the centre.
        let (start, end) = if centre.is_none() { (first.1, last.1) } else { (last.1, first.1) };
        let mut open = format!(
            "<{draw}:gradient{declare} {draw}:name=\"{name}\" {draw}:display-name=\"{display}\" {draw}:style=\"{style}\"",
            name = self.name(),
            display = self.display_name(),
        );
        if let Some((cx, cy)) = centre {
            open.push_str(&format!(" {draw}:cx=\"{cx}%\" {draw}:cy=\"{cy}%\""));
        }
        open.push_str(&format!(
            " {draw}:start-color=\"#{}\" {draw}:end-color=\"#{}\" {draw}:start-intensity=\"100%\" {draw}:end-intensity=\"100%\" {draw}:angle=\"{}deg\" {draw}:border=\"0%\"",
            hex6(start),
            hex6(end),
            tenths_text(u32::from(self.angle)),
        ));
        if ramp.len() == 2 && first.0 == 0 && last.0 == 1000 {
            open.push_str("/>");
            return open;
        }
        // The stops in the order of their offsets: from the start, which is the last of the ramp for a layout with a centre.
        let mut offsets: Vec<(u32, u32)> = ramp.iter().map(|(at, rgb)| (if centre.is_none() { *at } else { 1000 - *at }, *rgb)).collect();
        offsets.sort_by_key(|(offset, _)| *offset);
        // The declaration of the extension goes on the element, whatever the root binds.
        let mut text = open.replacen(&format!("<{draw}:gradient"), &format!("<{draw}:gradient xmlns:loext=\"{LOEXT_URI}\""), 1);
        text.push('>');
        for (offset, rgb) in offsets {
            text.push_str(&format!(
                "<loext:gradient-stop {svg}:offset=\"{}\" loext:color-type=\"rgb\" loext:color-value=\"#{}\"/>",
                thousandths_text(offset),
                hex6(rgb)
            ));
        }
        text.push_str(&format!("</{draw}:gradient>"));
        text
    }
}

/// The lines of a hatch: `draw:style`, the direction of its lines in degrees counterclockwise from the right (`draw:rotation`), and the distance between lines in inches
/// (`render`'s `pattern.rs` draws the same number's lines this far apart, across the lines).
pub(crate) fn hatch_form(pattern: u8) -> Option<(&'static str, u16, &'static str)> {
    Some(match pattern {
        2 => ("single", 45, "0.1in"),
        3 => ("double", 0, "0.1in"),
        4 => ("double", 45, "0.1in"),
        5 => ("single", 135, "0.1in"),
        6 => ("single", 0, "0.1in"),
        7 => ("single", 90, "0.1in"),
        13 => ("single", 0, "0.06in"),
        14 => ("single", 90, "0.06in"),
        15 => ("single", 135, "0.06in"),
        16 => ("single", 45, "0.06in"),
        19 => ("single", 0, "0.04in"),
        20 => ("single", 90, "0.04in"),
        21 => ("single", 135, "0.04in"),
        22 => ("single", 45, "0.04in"),
        23 => ("double", 0, "0.04in"),
        _ => return None,
    })
}

impl HatchKey {
    /// The name of the definition: the prefix, the pattern number and the colour of the marks.
    pub(crate) fn name(self) -> String {
        format!("{HATCH_PREFIX}{}{SEP}{}", self.pattern, hex6(self.rgb))
    }

    pub(crate) fn display_name(self) -> String {
        format!("DiaCraft hatch {} {}", self.pattern, hex6(self.rgb))
    }

    /// The hatch a name spells: the canonical form only.
    pub(crate) fn of_name(name: &str) -> Option<HatchKey> {
        let rest = name.strip_prefix(HATCH_PREFIX)?;
        let (pattern, colour) = rest.split_once(SEP)?;
        (colour.len() == 6 && pattern.len() <= 3).then_some(())?;
        let key = HatchKey { pattern: pattern.parse().ok()?, rgb: u32::from_str_radix(colour, 16).ok()? };
        (hatch_form(key.pattern).is_some() && key.name() == name).then_some(key)
    }

    pub(crate) fn element(self, draw: &str, declare: &str) -> String {
        let Some((style, rotation, distance)) = hatch_form(self.pattern) else { return String::new() };
        format!(
            "<{draw}:hatch{declare} {draw}:name=\"{}\" {draw}:display-name=\"{}\" {draw}:style=\"{style}\" {draw}:color=\"#{}\" {draw}:distance=\"{distance}\" {draw}:rotation=\"{rotation}deg\"/>",
            self.name(),
            self.display_name(),
            hex6(self.rgb)
        )
    }
}

impl Fx {
    /// The definition a name in `office:styles` is, when it is one of DiaCraft's own.
    pub(crate) fn of_name(name: &str) -> Option<Fx> {
        GradientKey::of_name(name).map(Fx::Gradient).or_else(|| HatchKey::of_name(name).map(Fx::Hatch))
    }

    pub(crate) fn name(&self) -> String {
        match self {
            Fx::Gradient(g) => g.name(),
            Fx::Hatch(h) => h.name(),
        }
    }

    /// The element, as `office:styles` holds it.
    pub(crate) fn element(&self, draw: &str, svg: &str, declare: &str) -> String {
        match self {
            Fx::Gradient(g) => g.element(draw, svg, declare),
            Fx::Hatch(h) => h.element(draw, declare),
        }
    }
}

/// The share of the marks' colour in the flat mixture `render`'s `pattern.rs` paints for a stipple (8 to 12, 17, 18 and 24): "a speckle too fine to see as dots at any zoom the window shows is the
/// same colour from a distance". `None` for any other number.
pub(crate) fn stipple_share(pattern: u8) -> Option<f64> {
    Some(match pattern {
        8 => 0.85,
        9 | 17 | 24 => 0.5,
        10 => 0.35,
        11 => 0.2,
        12 => 0.1,
        18 => 0.6,
        _ => return None,
    })
}

/// The colour of a stipple of `marks` over `ground` and `share` of marks: each channel blended straight and rounded, as `Rgba::lerp` does.
pub(crate) fn mixture(ground: u32, marks: u32, share: f64) -> u32 {
    let channel = |shift: u32| {
        let (a, b) = (f64::from((ground >> shift) & 255), f64::from((marks >> shift) & 255));
        ((a + (b - a) * share).round().clamp(0.0, 255.0)) as u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// What a fill is, written as a display form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fill {
    Gradient(GradientKey),
    /// A hatch, and the colour of its ground (`0xRRGGBB`).
    Hatch(HatchKey, u32),
    /// A stipple, as the one flat colour it is drawn in (`0xRRGGBB`).
    Stipple(u32),
}

/// The number a cell holds as a constant (no formula, no error, nothing else of its own), finite.
fn constant(sheet: &Sheet, name: &str) -> Option<f64> {
    plain(sheet.cell(name)?).and_then(Value::number).filter(|n| n.is_finite())
}

fn plain(cell: &Cell) -> Option<&Value> {
    (matches!(cell.formula, diacraft_doc::FormulaSlot::Absent) && cell.extra.is_none() && cell.error.is_none()).then_some(())?;
    cell.value.as_ref()
}

/// The colour a cell holds as `#rrggbb`: its constant, or **the value it was worked out to** when it has a formula of its own (`RGB(192,0,0)` is what `shape.gradient` writes; the formula rides
/// in DiaCraft's data beside the display form, and is never lost).
fn constant_colour(cell: Option<&Cell>) -> Option<u32> {
    let cell = cell?;
    let own = matches!(cell.formula, diacraft_doc::FormulaSlot::Absent | diacraft_doc::FormulaSlot::Local(_));
    (own && cell.extra.is_none() && cell.error.is_none()).then_some(())?;
    rgb_of(cell.value.as_ref()?.text())
}

/// The stops of the shape's own `FillGradient` rows: `GradientStopColor` and `GradientStopPosition` of each, constants, a colour of `#rrggbb` and a place from 0 to 1, in place order (a
/// stable sort). `None` when the shape has no such rows, more than [`MOST_STOPS`] or fewer than two, or a row with a cell that is not a constant of that form.
fn stops_of(sheet: &Sheet) -> Option<([Stop; MOST_STOPS], usize)> {
    let section = sheet.section(&SectionKey::new("FillGradient", None))?;
    if section.del {
        return None;
    }
    let mut list: Vec<Stop> = Vec::new();
    for row in section.rows.iter() {
        if row.del {
            continue;
        }
        if list.len() >= MOST_STOPS {
            return None;
        }
        let rgb = constant_colour(row.cells.get("GradientStopColor"))?;
        let at = plain(row.cells.get("GradientStopPosition")?)?.number().filter(|n| n.is_finite() && (0.0..=1.0).contains(n))?;
        list.push(Stop { rgb, pos: (at * 1000.0).round().clamp(0.0, 1000.0) as u16 });
    }
    if list.len() < 2 {
        return None;
    }
    list.sort_by_key(|s| s.pos);
    let mut stops = [Stop::default(); MOST_STOPS];
    for (slot, stop) in stops.iter_mut().zip(&list) {
        *slot = *stop;
    }
    Some((stops, list.len()))
}

/// The angle a `FillGradientAngle` of `radians` is as `draw:angle` says it, in tenths of a degree (0 to 3599): 90 degrees less the angle, since DiaCraft's turns clockwise from the right and
/// ODF's counterclockwise from a start at the top (measured, see the module's documentation).
pub(crate) fn odf_angle_tenths(radians: f64) -> u16 {
    let degrees = 90.0 - radians.to_degrees();
    let tenths = (degrees.rem_euclid(360.0) * 10.0).round();
    // A finite number from 0 to 3600; 3600 is 0.
    if tenths.is_finite() { (tenths as i64).rem_euclid(3600) as u16 } else { 0 }
}

/// The fill a shape's own cells say, as a display form: **a gradient** when `FillGradientEnabled` is a constant other than 0 beside a `FillPattern` that is a constant other than 0, with a
/// layout ODF has a form for and its own stops; **a hatch** when `FillPattern` is a constant of a hatch's and the marks and the ground are constant colours. `None` for any other fill: it is
/// written as it always was.
pub(crate) fn fill_of(sheet: &Sheet) -> Option<Fill> {
    // The pattern the shape has of its own; a shape that has none paints the one its style gives, which `shape.gradient` has made one that paints.
    let pattern = match sheet.cell("FillPattern") {
        None => None,
        Some(_) => Some(constant(sheet, "FillPattern")?),
    };
    if pattern.is_some_and(|p| p == 0.0 || p.fract() != 0.0 || p > 255.0) {
        return None;
    }
    if constant(sheet, "FillGradientEnabled").is_some_and(|e| e != 0.0) {
        let dir = constant(sheet, "FillGradientDir").unwrap_or(0.0);
        if !(0.0..=13.0).contains(&dir) || dir.fract() != 0.0 {
            return None;
        }
        let dir = dir as u8;
        let (stops, n) = stops_of(sheet)?;
        // A cell that is there and is not a constant is the model's own, which the markup cannot say.
        let angle = match sheet.cell("FillGradientAngle") {
            None => std::f64::consts::FRAC_PI_2,
            Some(_) => constant(sheet, "FillGradientAngle")?,
        };
        let key = GradientKey { dir, angle: if dir == 0 { odf_angle_tenths(angle) } else { 0 }, n: u8::try_from(n).ok()?, stops };
        return key.valid().then_some(Fill::Gradient(key));
    }
    let pattern = pattern? as u8;
    let marks = constant_colour(sheet.cell("FillForegnd"))?;
    let ground = constant_colour(sheet.cell("FillBkgnd"))?;
    if hatch_form(pattern).is_some() {
        return Some(Fill::Hatch(HatchKey { pattern, rgb: marks }, ground));
    }
    // A stipple is one flat colour, which has the opacity of its marks and ground only while both are opaque.
    let opaque = |name: &str| sheet.cell(name).is_none() || constant(sheet, name) == Some(0.0);
    (opaque("FillForegndTrans") && opaque("FillBkgndTrans")).then_some(())?;
    Some(Fill::Stipple(mixture(ground, marks, stipple_share(pattern)?)))
}

impl Fill {
    /// The attributes of the style that say the fill, after `draw:fill` and the names it takes out (`stylemap::fill_ops`).
    pub(crate) fn ops(&self) -> Vec<Op> {
        let op = |local: &'static str, val: Val| Op { prop: super::stylemap::GRAPHIC, ns: Ns::Draw, local, val };
        let mut ops = match self {
            Fill::Gradient(g) => {
                let mut ops = super::stylemap::fill_ops("gradient");
                ops.push(op("fill-gradient-name", Val::Text(g.name())));
                ops
            }
            Fill::Hatch(h, ground) => {
                let mut ops = super::stylemap::fill_ops("hatch");
                ops.push(op("fill-hatch-name", Val::Text(h.name())));
                ops.push(op("fill-hatch-solid", Val::Text("true".to_owned())));
                ops.push(op("fill-color", Val::Text(format!("#{}", hex6(*ground)))));
                ops
            }
            Fill::Stipple(mix) => {
                let mut ops = super::stylemap::fill_ops("solid");
                ops.push(op("fill-color", Val::Text(format!("#{}", hex6(*mix)))));
                ops
            }
        };
        // An attribute is taken out and set again in one edit: taking it out first would make two edits of the same bytes in a style that has it.
        let set: Vec<&'static str> = ops.iter().filter(|o| o.val != Val::Remove).map(|o| o.local).collect();
        ops.retain(|o| o.val != Val::Remove || !set.contains(&o.local));
        ops
    }

    /// Whether the fill is a gradient.
    pub(crate) fn is_gradient(&self) -> bool {
        matches!(self, Fill::Gradient(_))
    }
}

/// **The blur of a visible shadow, as the file says it**: `loext:shadow-blur`, the one attribute LibreOffice reads for it (measured: 26.8.2.2 reads no `draw:shadow-blur`), under the
/// extension's namespace, which the element that holds it declares. DiaCraft's blur is a Gaussian of a standard deviation one and a half times smaller than the cell, and a blur of
/// *b* in LibreOffice is one of about *b* over two (measured, between 0.45 and 0.58 for a blur of 0.05 to 1 cm), so the file says four thirds of the cell. `None` for a shadow that is not
/// written as visible, and for a blur that is no length from 0 to 1,000 points. **Only a new style says it**: the attribute is no name of the ODF namespaces, and an edit of a style that
/// holds a shadow leaves the blur the file has.
pub(crate) fn shadow_blur_op(sheet: &Sheet, ops: &[Op]) -> Option<Op> {
    let visible = ops.iter().rev().find(|o| o.ns == Ns::Draw && o.local == "shadow").is_some_and(|o| o.val == Val::Text("visible".to_owned()));
    let blur = constant(sheet, "ShapeShdwBlur").filter(|b| *b > 0.0 && *b <= 1000.0 / 72.0)?;
    visible.then(|| Op { prop: super::stylemap::GRAPHIC, ns: Ns::Other, local: "shadow-blur", val: Val::Length(blur * 4.0 / 3.0) })
}

/// What the display forms a style's attributes say do beside the cells written natively (see [`Display`]): read off the attributes written, so that a value a held attribute replaced is
/// not counted.
pub(crate) fn display_of(sheet: &Sheet, ops: &[Op]) -> Display {
    let mut display = Display::default();
    // The last value written to an attribute is the one the file has.
    let last = |local: &str| ops.iter().rev().find(|o| o.ns == Ns::Draw && o.local == local && o.prop == super::stylemap::GRAPHIC).map(|o| &o.val);
    let text = |local: &str| match last(local) {
        Some(Val::Text(t)) => Some(t.as_str()),
        _ => None,
    };
    // A visible shadow reads back with `ShapeShdwType` 1 whatever the model holds.
    if text("shadow") == Some("visible") {
        display.adds.push("ShapeShdwType");
    }
    match text("fill") {
        Some("gradient") if text("fill-gradient-name").is_some_and(|n| GradientKey::of_name(n).is_some()) => {
            // The reader makes a stand-in of the gradient: the layout, the stops, and the solid pattern, and the angle of a linear one.
            display.adds.push("FillGradientDir");
            if text("fill-gradient-name").and_then(GradientKey::of_name).is_some_and(|key| key.dir == 0) {
                display.adds.push("FillGradientAngle");
            }
            display.adds_sections.push("FillGradient");
            display.unjudged.push("FillPattern");
        }
        Some("hatch") if text("fill-hatch-name").is_some_and(|n| HatchKey::of_name(n).is_some()) => {
            // The reader makes the solid pattern, and the fill colour is the ground (`draw:fill-color`): the marks' colour is the hatch's.
            display.unjudged.push("FillPattern");
            display.native.push(("FillForegnd", text("fill-color").map(|c| Cell::constant(Value::from_text(c)))));
        }
        // A stipple is the solid fill of its flat colour: the reader gives the solid pattern and that colour as the fill's, which the model's own cells are carried against.
        Some("solid") => {
            if let (Some(Fill::Stipple(mix)), Some(shown)) = (fill_of(sheet), text("fill-color"))
                && shown == format!("#{}", hex6(mix))
            {
                display.unjudged.push("FillPattern");
                display.native.push(("FillForegnd", Some(Cell::constant(Value::from_text(shown)))));
            }
        }
        _ => {}
    }
    display
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(dir: u8, angle: u16, stops: &[(u32, u16)]) -> GradientKey {
        let mut list = [Stop::default(); MOST_STOPS];
        for (slot, (rgb, pos)) in list.iter_mut().zip(stops) {
            *slot = Stop { rgb: *rgb, pos: *pos };
        }
        GradientKey { dir, angle, n: stops.len() as u8, stops: list }
    }

    /// The names of the keys this module makes are read back as the keys, and every name that is not the canonical one of a key is no key.
    #[test]
    fn a_name_is_made_from_the_key_and_the_key_from_the_name() {
        let keys = [
            gradient(0, 0, &[(0xff0000, 0), (0x0000ff, 1000)]),
            gradient(0, 900, &[(0xff0000, 0), (0x00ff00, 250), (0x0000ff, 1000)]),
            gradient(3, 0, &[(0x102030, 0), (0xffffff, 1000)]),
            gradient(12, 0, &[(0x000000, 0), (0x000001, 333), (0x0a0b0c, 334), (0xffffff, 1000)]),
            gradient(0, 3599, &[(0xabcdef, 0); MOST_STOPS]),
        ];
        for key in keys {
            let name = key.name();
            assert_eq!(GradientKey::of_name(&name), Some(key), "{name}");
            assert_eq!(Fx::of_name(&name), Some(Fx::Gradient(key)));
        }
        assert_eq!(gradient(0, 900, &[(0xff0000, 0), (0x0000ff, 1000)]).name(), "DiaCraft_20_gradient_20_0_20_900_20_ff0000p0_20_0000ffp1000");
        for bad in [
            "DiaCraft_20_gradient_20_0_20_900_20_ff0000p0",
            "DiaCraft_20_gradient_20_00_20_900_20_ff0000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_0_20_900_20_FF0000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_0_20_900_20_ff0000p00_20_0000ffp1000",
            "DiaCraft_20_gradient_20_0_20_900_20_ff0000p1000_20_0000ffp0",
            "DiaCraft_20_gradient_20_14_20_0_20_ff0000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_3_20_900_20_ff0000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_0_20_3600_20_ff0000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_0_20_900_20_ff0000p0_20_0000ffp1001",
            "DiaCraft_20_gradient_20_0_20_900_20_ff000p0_20_0000ffp1000",
            "DiaCraft_20_gradient_20_",
            "gradient",
            "",
        ] {
            assert_eq!(GradientKey::of_name(bad), None, "{bad:?}");
        }
        // More stops than a gradient has.
        let long = format!("{GRADIENT_PREFIX}0{SEP}0{}", (0..11).map(|i| format!("{SEP}000000p{}", i * 10)).collect::<String>());
        assert_eq!(GradientKey::of_name(&long), None);
    }

    /// A name of a file may be as long as the file: a part that is no colour, no number or no place is refused by its length, before any digit of it is read.
    #[test]
    fn a_name_as_long_as_a_file_is_refused_and_not_read_to_its_end() {
        let long = "0".repeat(2_000_000);
        for name in [
            format!("{GRADIENT_PREFIX}0{SEP}0{SEP}{long}p0{SEP}00ff00p1000"),
            format!("{GRADIENT_PREFIX}0{SEP}0{SEP}ff0000p{long}{SEP}00ff00p1000"),
            format!("{GRADIENT_PREFIX}{long}{SEP}0{SEP}ff0000p0{SEP}00ff00p1000"),
            format!("{GRADIENT_PREFIX}0{SEP}{long}{SEP}ff0000p0{SEP}00ff00p1000"),
            format!("{GRADIENT_PREFIX}0{SEP}0{}", format!("{SEP}00ff00p1").repeat(200_000)),
            format!("{HATCH_PREFIX}4{SEP}{long}"),
            format!("{HATCH_PREFIX}{long}{SEP}202020"),
        ] {
            assert_eq!(Fx::of_name(&name), None, "{}", &name[..40]);
        }
    }

    #[test]
    fn a_hatch_name_says_the_number_and_the_colour_and_no_other_name_is_a_hatch() {
        let key = HatchKey { pattern: 4, rgb: 0x202020 };
        assert_eq!(key.name(), "DiaCraft_20_hatch_20_4_20_202020");
        assert_eq!(HatchKey::of_name(&key.name()), Some(key));
        for pattern in 0..=255u8 {
            let key = HatchKey { pattern, rgb: 0xabcdef };
            assert_eq!(HatchKey::of_name(&key.name()).is_some(), hatch_form(pattern).is_some(), "{pattern}");
        }
        for bad in [
            "DiaCraft_20_hatch_20_4_20_20202",
            "DiaCraft_20_hatch_20_04_20_202020",
            "DiaCraft_20_hatch_20_8_20_202020",
            "DiaCraft_20_hatch_20_4_20_ABCDEF",
            "DiaCraft_20_hatch_20_4",
        ] {
            assert_eq!(HatchKey::of_name(bad), None, "{bad}");
        }
    }

    /// The angle is 90 degrees less DiaCraft's (measured: LibreOffice turns it counterclockwise from a start at the top), in tenths of a degree, and never 3600 or more.
    #[test]
    fn the_angle_of_the_file_is_ninety_degrees_less_the_angle_of_the_cell() {
        use std::f64::consts::{FRAC_PI_2, PI};
        for (radians, tenths) in [
            (0.0, 900),
            (FRAC_PI_2, 0),
            (PI, 2700),
            (3.0 * FRAC_PI_2, 1800),
            (-FRAC_PI_2, 1800),
            (1.0_f64.to_radians(), 890),
            (91.0_f64.to_radians(), 3590),
            (2.0 * PI, 900),
        ] {
            assert_eq!(odf_angle_tenths(radians), tenths, "{radians}");
        }
        assert_eq!(odf_angle_tenths(f64::NAN), 0);
        assert_eq!(odf_angle_tenths(f64::INFINITY), 0);
        assert!(odf_angle_tenths(1e300) < 3600);
    }

    #[test]
    fn the_element_of_a_gradient_has_two_colours_and_the_stops_only_where_they_say_more() {
        let two = gradient(0, 0, &[(0xff0000, 0), (0x0000ff, 1000)]).element("draw", "svg", "");
        assert_eq!(
            two,
            "<draw:gradient draw:name=\"DiaCraft_20_gradient_20_0_20_0_20_ff0000p0_20_0000ffp1000\" draw:display-name=\"DiaCraft gradient 0 0 ff0000p0 0000ffp1000\" draw:style=\"linear\" draw:start-color=\"#ff0000\" draw:end-color=\"#0000ff\" draw:start-intensity=\"100%\" draw:end-intensity=\"100%\" draw:angle=\"0deg\" draw:border=\"0%\"/>"
        );
        let three = gradient(0, 905, &[(0xff0000, 0), (0x00ff00, 250), (0x0000ff, 1000)]).element("draw", "svg", "");
        assert!(three.contains("draw:angle=\"90.5deg\"") && three.contains("xmlns:loext=\""), "{three}");
        assert!(three.contains("<loext:gradient-stop svg:offset=\"0.25\" loext:color-type=\"rgb\" loext:color-value=\"#00ff00\"/>"), "{three}");
        // A radial gradient starts at the outer edge: the last stop is the start colour, and the offsets run from it.
        let radial = gradient(3, 0, &[(0xff0000, 0), (0x00ff00, 250), (0x0000ff, 1000)]).element("draw", "svg", "");
        assert!(
            radial.contains("draw:start-color=\"#0000ff\" draw:end-color=\"#ff0000\"") && radial.contains("draw:cx=\"50%\" draw:cy=\"50%\""),
            "{radial}"
        );
        let at = |s: &str| radial.find(s).unwrap_or(usize::MAX);
        assert!(at("svg:offset=\"0\"") < at("svg:offset=\"0.75\"") && at("svg:offset=\"0.75\"") < at("svg:offset=\"1\""), "{radial}");
        // Two stops that are not at 0 and 1 are stops too (the colour holds beyond them).
        assert!(gradient(0, 0, &[(0xff0000, 250), (0x0000ff, 750)]).element("draw", "svg", "").contains("loext:gradient-stop"));
        // A layout with no form writes nothing; the path gradient is written as the rectangular one from the centre.
        let none = GradientKey { dir: 14, ..gradient(0, 0, &[(0, 0), (1, 1000)]) };
        assert_eq!(none.element("draw", "svg", ""), "");
        assert!(
            gradient(13, 0, &[(0, 0), (1, 1000)]).element("draw", "svg", "").contains("draw:style=\"rectangular\" draw:cx=\"50%\" draw:cy=\"50%\"")
        );
    }

    /// A layout with a centre that LibreOffice ends sooner (measured) has its stops put at that many times their places, with DiaCraft's colour where the file's ramp ends.
    #[test]
    fn a_gradient_from_a_corner_has_its_stops_at_twice_their_places_and_the_colour_where_the_file_ends() {
        let key = gradient(1, 0, &[(0xff0000, 0), (0x0000ff, 1000)]);
        // The colour at half the way is the blend of the two.
        assert_eq!(key.colour_at(500.0), 0x800080);
        assert_eq!(key.colour_at(0.0), 0xff0000);
        assert_eq!(key.colour_at(2000.0), 0x0000ff);
        assert_eq!(key.ramp(), vec![(0, 0xff0000), (1000, 0x800080)]);
        let el = key.element("draw", "svg", "");
        assert!(el.contains("draw:start-color=\"#800080\" draw:end-color=\"#ff0000\"") && !el.contains("loext"), "{el}");
        // Stops past the middle are left out, and one before it is where twice its place says.
        let three = gradient(1, 0, &[(0xff0000, 0), (0x00ff00, 200), (0x0000ff, 1000)]);
        let ramp = three.ramp();
        assert_eq!(ramp.len(), 3, "{ramp:?}");
        assert_eq!((ramp[0], ramp[1].0), ((0, 0xff0000), 400));
        assert_eq!(ramp[2].0, 1000);
        // From the middle of an edge the file ends a little sooner; from the centre not at all.
        assert_eq!(gradient(3, 0, &[(0xff0000, 0), (0x0000ff, 1000)]).ramp(), vec![(0, 0xff0000), (1000, 0x0000ff)]);
        assert_eq!(gradient(5, 0, &[(0xff0000, 0), (0x0000ff, 1000)]).ramp().last().map(|r| r.0), Some(1000));
        assert!(gradient(5, 0, &[(0xff0000, 0), (0x0000ff, 1000)]).ramp().last().is_some_and(|r| r.1 != 0x0000ff));
        // A linear gradient is not remapped.
        assert_eq!(gradient(0, 0, &[(0xff0000, 0), (0x0000ff, 1000)]).ramp(), vec![(0, 0xff0000), (1000, 0x0000ff)]);
    }

    /// A stipple is drawn as one flat mixture (`render`'s `pattern.rs`), so the file says that colour: the marks' share of each channel, rounded as `Rgba::lerp` rounds.
    #[test]
    fn a_stipple_is_the_flat_mixture_of_its_two_colours() {
        assert_eq!(mixture(0xffffff, 0x000000, 0.5), 0x808080);
        assert_eq!(mixture(0xfff0c0, 0xa02010, 0.5), 0xd0 << 16 | 0x88 << 8 | 0x68);
        assert_eq!(mixture(0x102030, 0xa0b0c0, 0.0), 0x102030);
        assert_eq!(mixture(0x102030, 0xa0b0c0, 1.0), 0xa0b0c0);
        for (pattern, share) in [(8, 0.85), (9, 0.5), (10, 0.35), (11, 0.2), (12, 0.1), (17, 0.5), (18, 0.6), (24, 0.5)] {
            assert_eq!(stipple_share(pattern), Some(share), "{pattern}");
        }
        for pattern in (0..=255u8).filter(|p| ![8, 9, 10, 11, 12, 17, 18, 24].contains(p)) {
            assert_eq!(stipple_share(pattern), None, "{pattern}");
        }
        // The lines of a hatch and the stipples are the 23 patterns from 2 to 24 between them.
        for pattern in 2..=24u8 {
            assert!(hatch_form(pattern).is_some() != stipple_share(pattern).is_some(), "{pattern}");
        }
    }

    #[test]
    fn the_element_of_a_hatch_names_its_lines_and_its_colour() {
        let two = HatchKey { pattern: 4, rgb: 0x202020 }.element("draw", "");
        assert_eq!(
            two,
            "<draw:hatch draw:name=\"DiaCraft_20_hatch_20_4_20_202020\" draw:display-name=\"DiaCraft hatch 4 202020\" draw:style=\"double\" draw:color=\"#202020\" draw:distance=\"0.1in\" draw:rotation=\"45deg\"/>"
        );
        assert!(HatchKey { pattern: 15, rgb: 0 }.element("draw", "").contains("draw:rotation=\"135deg\""));
        assert_eq!(HatchKey { pattern: 9, rgb: 0 }.element("draw", ""), "");
    }
}
