//! ODF formatting properties as cells: what each property gives, given what the chain of styles it is read
//! through resolves.
//!
//! A conversion has two inputs: a [`View`], the chains of styles in the order R3 walks them (a shape's paragraph style, then its graphic
//! style, then the default styles), and the set of properties that are the converted sheet's *own* (`own`): a style sheet's are those
//! its style sets itself; a shape's are those of its automatic styles, and those of the graphic chain its text link cannot reach.
//! A cell is written when one of the properties that feed it is own; its value is computed from what the view resolves, so a property that
//! needs another to mean anything (a dash needs the stroke width and the cap, a bit of `Style` needs the other bits) reads the others along
//! the same chain. A property the chain says nothing of takes the root's value, the consumer's choice R3 ends at.
//!
//! Each property's value is read strictly (`units.rs`); a value the standard does not define is ignored and the attribute kept.

use std::rc::Rc;

use diacraft_doc::Cell;
use diacraft_fonts::script::FontSlot;

use super::Reader;
use super::cells;
use super::styles::{Dash, File, Found, Kind, P, Resolved, Run, StyleSet, Tab};
use super::texts::StyleTexts;
use super::typed::{LineHeight, Marker, Name, Prop, Size, Spacing, Typed};
use super::units::{self, Length};
use crate::error::OdgError;

/// Which kinds of property a layer of a view serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Serves {
    All,
    /// A default graphic style: graphic properties, and text properties after the default paragraph style (the root holds both).
    GraphicDefault,
    /// A default paragraph style: paragraph and text properties.
    ParagraphDefault,
}

/// One answer of a view: where the value was found.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub found: Found,
    /// Which layer of the view, from 0.
    pub layer: usize,
}

impl Hit {
    /// How far the answer is: a nearer one wins a tie between two properties that feed one cell.
    fn rank(&self) -> (usize, usize) {
        (self.layer, self.found.depth)
    }
}

/// The chains a conversion reads through, nearest first.
pub(crate) struct View<'a> {
    pub set: &'a StyleSet,
    pub layers: Vec<(Rc<Resolved>, Serves)>,
}

impl<'a> View<'a> {
    /// The nearest answer for `p` on `kind`.
    pub(crate) fn hit(&self, r: &mut Reader, kind: Kind, p: P) -> Option<Hit> {
        for (layer, (resolved, serves)) in self.layers.iter().enumerate() {
            let applies = match serves {
                Serves::All => true,
                Serves::GraphicDefault => true,
                Serves::ParagraphDefault => kind != Kind::Graphic,
            };
            r.style_step();
            if applies && let Some(found) = resolved.get(kind, p) {
                return Some(Hit { found, layer });
            }
        }
        None
    }

    /// The value of `p` on `kind`, and where it was found.
    pub(crate) fn get(&self, r: &mut Reader, kind: Kind, p: P) -> Option<(&'a str, Hit)> {
        let hit = self.hit(r, kind, p)?;
        let value = self.set.get(hit.found.style)?.own(kind, p)?;
        Some((value, hit))
    }

    /// The value of `p` on `kind`.
    pub(crate) fn value(&self, r: &mut Reader, kind: Kind, p: P) -> Option<&'a str> {
        self.get(r, kind, p).map(|(v, _)| v)
    }

    /// What the nearest answer for `p` on `kind` reads as, and where it was found: read once, when its style was read (`typed.rs`).
    pub(crate) fn typed(&self, r: &mut Reader, kind: Kind, p: P) -> Option<(&'a Typed, Hit)> {
        self.prop(r, kind, p).map(|(prop, hit)| (&prop.typed, hit))
    }

    /// The nearest answer for `p` on `kind` as the style that gives it holds it: its text and what it reads as.
    pub(crate) fn prop(&self, r: &mut Reader, kind: Kind, p: P) -> Option<(&'a Prop, Hit)> {
        let hit = self.hit(r, kind, p)?;
        let prop = self.set.get(hit.found.style)?.own_prop(kind, p)?;
        Some((prop, hit))
    }

    /// `p` on `kind` as a length, when it is one.
    pub(crate) fn length(&self, r: &mut Reader, kind: Kind, p: P) -> Option<Length> {
        match self.typed(r, kind, p)?.0 {
            Typed::Length(l) => *l,
            _ => None,
        }
    }

    /// `p` on `kind` as a percentage, when it is one.
    pub(crate) fn percent(&self, r: &mut Reader, kind: Kind, p: P) -> Option<f64> {
        match self.typed(r, kind, p)?.0 {
            Typed::Percent(n) => *n,
            _ => None,
        }
    }

    /// `p` on `kind` as a colour, in the form a cell holds one (`#RRGGBB`), when it is one.
    pub(crate) fn colour(&self, r: &mut Reader, kind: Kind, p: P) -> Option<String> {
        match self.typed(r, kind, p)?.0 {
            Typed::Colour(c) => c.map(units::hex),
            _ => None,
        }
    }

    /// The nearest tab stops.
    pub(crate) fn tabs(&self) -> Option<&'a Rc<[Tab]>> {
        for (resolved, serves) in &self.layers {
            if *serves == Serves::GraphicDefault {
                continue;
            }
            if let Some(found) = resolved.tabs() {
                return self.set.get(found.style).and_then(|s| s.tabs.as_ref());
            }
        }
        None
    }
}

/// The root's values a property the chain says nothing of takes: the cells DiaCraft's new drawing's
/// root holds, as numbers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Base {
    /// `Size` of the root's `Character` row 0, inches.
    pub size: f64,
    /// `LineWeight`, inches.
    pub weight: f64,
    /// `LineCap`.
    pub cap: u8,
    /// `Style` of the root's `Character` row 0.
    pub style_bits: u32,
}

/// The cells a conversion gives, by where they go.
#[derive(Default, Debug)]
pub(crate) struct Out {
    /// Cells directly under the sheet.
    pub direct: Vec<(&'static str, Cell)>,
    /// Cells of a `Paragraph` row.
    pub paragraph: Vec<(&'static str, Cell)>,
    /// Cells of a `Character` row.
    pub character: Vec<(&'static str, Cell)>,
    /// The rows of `FillGradient`: colour and position of each stop.
    pub stops: Option<Vec<(Cell, Cell)>>,
    /// How many rows of `FillGradient` the sheet holds in all when it holds `stops`: the rows past the stops up to this many are deleted (`Del`), so that a style under the sheet whose gradient
    /// has more rows lends it none. The most rows any gradient of the drawing stands in with (`StyleSet::most_stop_rows`).
    pub stops_through: usize,
    /// The stops of a `Tabs` row: shared with the style that gives them, so that a conversion that shows a style's stops copies none.
    pub tabs: Option<Rc<[Tab]>>,
    /// The arrow cells (`BeginArrow`, `EndArrow`) whose marker is not one of DiaCraft's own, so that the cell holds the stand-in 4: tier 3 tells such a marker from
    /// DiaCraft's own number 4, which the cell holds too.
    pub other_ends: Vec<&'static str>,
}

impl Out {
    /// How many cells it gave.
    pub(crate) fn cells(&self) -> usize {
        self.direct.len()
            + self.paragraph.len()
            + self.character.len()
            + self.stops.as_ref().map_or(0, |s| s.len() * 2)
            + self.tabs.as_ref().map_or(0, |t| t.len() * 2)
    }
}

/// Whether a conversion takes a property as its own.
pub(crate) type Own<'o> = &'o dyn Fn(Kind, P) -> bool;

/// The text-block cells of graphic properties: these follow a shape's text link in the model, not its line and fill links.
pub(crate) fn is_text_block(p: P) -> bool {
    matches!(p, P::TextareaVerticalAlign | P::Padding | P::PaddingTop | P::PaddingBottom | P::PaddingLeft | P::PaddingRight | P::WritingMode)
}

/// The fill, line and shadow cells.
pub(crate) fn line_fill(r: &mut Reader, view: &View<'_>, own: Own<'_>, base: &Base, out: &mut Out) {
    let g = Kind::Graphic;
    let o = |p: P| own(g, p);
    // Fill.
    if o(P::Fill) || o(P::FillGradientName) || o(P::FillHatchSolid) {
        fill_pattern(r, view, out);
    }
    if o(P::FillColor)
        && let Some(c) = view.colour(r, g, P::FillColor)
    {
        out.direct.push(("FillForegnd", cells::text(&c)));
    }
    if o(P::Opacity)
        && let Some(p) = view.percent(r, g, P::Opacity)
    {
        // To nine places, as a share is written: 1 - 0.55 is 0.44999999999999996, which is no share a person set.
        let trans = cells::round9((1.0 - p / 100.0).clamp(0.0, 1.0));
        out.direct.push(("FillForegndTrans", cells::plain(trans)));
        out.direct.push(("FillBkgndTrans", cells::plain(trans)));
    }
    // Line.
    if o(P::Stroke) || o(P::StrokeDash) {
        match view.value(r, g, P::Stroke) {
            Some("none") => out.direct.push(("LinePattern", cells::plain(0.0))),
            Some("solid") => out.direct.push(("LinePattern", cells::plain(1.0))),
            Some("dash") => {
                let pattern = dash_pattern(r, view, base);
                out.direct.push(("LinePattern", cells::plain(f64::from(pattern))));
            }
            Some(_) => r.keep("property_value"),
            None => {}
        }
    }
    if o(P::StrokeWidth)
        && let Some(w) = view.length(r, g, P::StrokeWidth)
    {
        r.saw_px(w);
        out.direct.push(("LineWeight", cells::length(w.inches.max(0.0), w.unit.word())));
    }
    if o(P::StrokeColor)
        && let Some(c) = view.colour(r, g, P::StrokeColor)
    {
        out.direct.push(("LineColor", cells::text(&c)));
    }
    if o(P::StrokeOpacity)
        && let Some((typed, _)) = view.typed(r, g, P::StrokeOpacity)
    {
        let opacity = if let Typed::Share(share) = typed { *share } else { None };
        match opacity {
            Some(op) => {
                let trans = ((1.0 - op).clamp(0.0, 1.0) / 0.005).round() * 0.005;
                out.direct.push(("LineColorTrans", cells::plain(cells::round9(trans))));
            }
            None => r.keep("property_value"),
        }
    }
    if o(P::StrokeLinecap)
        && let Some(text) = view.value(r, g, P::StrokeLinecap)
    {
        match cap_of(text) {
            Some(cap) => out.direct.push(("LineCap", cells::plain(f64::from(cap)))),
            None => r.keep("property_value"),
        }
    }
    for (end, width, arrow, size) in
        [(P::MarkerStart, P::MarkerStartWidth, "BeginArrow", "BeginArrowSize"), (P::MarkerEnd, P::MarkerEndWidth, "EndArrow", "EndArrowSize")]
    {
        if o(end) || o(width) {
            line_end(r, view, base, (end, width), (arrow, size), out);
        }
    }
    // Shadow.
    if o(P::Shadow) {
        match view.value(r, g, P::Shadow) {
            Some("visible") => {
                out.direct.push(("ShdwPattern", cells::plain(1.0)));
                out.direct.push(("ShapeShdwType", cells::plain(1.0)));
            }
            Some("hidden") => out.direct.push(("ShdwPattern", cells::plain(0.0))),
            Some(_) => r.keep("property_value"),
            None => {}
        }
    }
    if o(P::ShadowColor)
        && let Some(c) = view.colour(r, g, P::ShadowColor)
    {
        out.direct.push(("ShdwForegnd", cells::text(&c)));
    }
    if o(P::ShadowOpacity)
        && let Some(p) = view.percent(r, g, P::ShadowOpacity)
    {
        out.direct.push(("ShdwForegndTrans", cells::plain(cells::round9((1.0 - p / 100.0).clamp(0.0, 1.0)))));
    }
    if o(P::ShadowOffsetX)
        && let Some(l) = view.length(r, g, P::ShadowOffsetX)
    {
        out.direct.push(("ShapeShdwOffsetX", cells::length(l.inches, l.unit.word())));
    }
    if o(P::ShadowOffsetY)
        && let Some(l) = view.length(r, g, P::ShadowOffsetY)
    {
        // ODF's y runs down the page and the cell's up.
        out.direct.push(("ShapeShdwOffsetY", cells::length(-l.inches, l.unit.word())));
    }
}

/// `svg:stroke-linecap` as `LineCap`: `round` 0, `butt` 1, `square` 2.
fn cap_of(text: &str) -> Option<u8> {
    match text {
        "round" => Some(0),
        "butt" => Some(1),
        "square" => Some(2),
        _ => None,
    }
}

/// `FillPattern` and the gradient cells from what the chain resolves `draw:fill` to.
fn fill_pattern(r: &mut Reader, view: &View<'_>, out: &mut Out) {
    let g = Kind::Graphic;
    match view.value(r, g, P::Fill) {
        Some("none") => out.direct.push(("FillPattern", cells::plain(0.0))),
        Some("solid") => {
            out.direct.push(("FillPattern", cells::plain(1.0)));
            out.direct.push(("FillGradientEnabled", cells::plain(0.0)));
        }
        Some("gradient") => {
            r.keep("gradient");
            let gradient = match view.typed(r, g, P::FillGradientName) {
                Some((Typed::Gradient(Name::Found(index)), _)) => view.set.gradient(*index),
                _ => None,
            };
            match gradient {
                Some(gradient) => {
                    // The stand-in, made when the definition was read (`gradient.rs`): the layout, the angle of a linear or axial one, and the stops.
                    out.direct.push(("FillPattern", cells::plain(1.0)));
                    out.direct.push(("FillGradientEnabled", cells::plain(1.0)));
                    out.direct.push(("FillGradientDir", cells::plain(f64::from(gradient.dir))));
                    if let Some(angle) = gradient.angle {
                        out.direct.push(("FillGradientAngle", cells::angle(angle)));
                    }
                    out.stops = Some(gradient.stops.iter().map(|(c, at)| (cells::text(&units::hex(*c)), cells::plain(*at))).collect());
                    out.stops_through = view.set.most_stop_rows();
                }
                None => {
                    r.warn("gradient_not_found", || "a fill names a gradient the drawing does not have; it fills solid".to_owned());
                    out.direct.push(("FillPattern", cells::plain(1.0)));
                    out.direct.push(("FillGradientEnabled", cells::plain(0.0)));
                }
            }
        }
        Some("hatch") => {
            r.keep("hatch");
            let solid = view.value(r, g, P::FillHatchSolid).and_then(units::boolean).unwrap_or(false);
            out.direct.push(("FillPattern", cells::plain(if solid { 1.0 } else { 0.0 })));
        }
        Some("bitmap") => {
            r.keep("bitmap_fill");
            out.direct.push(("FillPattern", cells::plain(0.0)));
        }
        Some(_) => r.keep("property_value"),
        None => {}
    }
}

/// The marks and gaps of each `LinePattern` from 2 to 23, in weights, for a round or projecting cap.
pub(crate) const PATTERNS: [(u8, &[f64]); 22] = [
    (2, &[7.0, 5.0]),
    (3, &[0.0, 5.0]),
    (4, &[7.0, 5.0, 0.0, 5.0]),
    (5, &[7.0, 5.0, 0.0, 5.0, 0.0, 5.0]),
    (6, &[7.0, 5.0, 7.0, 5.0, 0.0, 5.0]),
    (7, &[19.0, 5.0, 7.0, 5.0]),
    (8, &[19.0, 5.0, 7.0, 5.0, 7.0, 5.0]),
    (9, &[3.0, 3.0]),
    (10, &[0.0, 3.0]),
    (11, &[3.0, 3.0, 0.0, 3.0]),
    (12, &[3.0, 3.0, 0.0, 3.0, 0.0, 3.0]),
    (13, &[3.0, 3.0, 3.0, 3.0, 0.0, 3.0]),
    (14, &[9.0, 3.0, 3.0, 3.0]),
    (15, &[9.0, 3.0, 3.0, 3.0, 3.0, 3.0]),
    (16, &[15.0, 9.0]),
    (17, &[0.0, 9.0]),
    (18, &[15.0, 9.0, 0.0, 9.0]),
    (19, &[15.0, 9.0, 0.0, 9.0, 0.0, 9.0]),
    (20, &[15.0, 9.0, 15.0, 9.0, 0.0, 9.0]),
    (21, &[39.0, 9.0, 15.0, 9.0]),
    (22, &[39.0, 9.0, 15.0, 9.0, 15.0, 9.0]),
    (23, &[1.0, 2.0]),
];

/// The most marks a pattern of the table has.
const MOST_MARKS: u32 = 3;

/// The `LinePattern` of a `draw:stroke="dash"`: the dash's runs, in weights, matched to the table within 0.01 weight, after the flat
/// cap's form is taken back to the table's (with `LineCap` 1, each mark one weight longer and each gap one shorter). No match, a
/// dash that is not there or a run that cannot be read gives the stand-in 2, and the dash is kept.
fn dash_pattern(r: &mut Reader, view: &View<'_>, base: &Base) -> u8 {
    let g = Kind::Graphic;
    let dash = match view.typed(r, g, P::StrokeDash) {
        Some((Typed::Dash(Name::Found(index)), _)) => view.set.dash(*index),
        _ => None,
    };
    let weight = view.length(r, g, P::StrokeWidth).map_or(base.weight, |l| l.inches);
    let cap = view.value(r, g, P::StrokeLinecap).and_then(cap_of).unwrap_or(base.cap);
    let runs = dash.and_then(|d| runs(d, weight));
    // The runs made are walked once each, and counted: a dash of more marks than any pattern holds makes none.
    r.path_units(runs.as_ref().map_or(0, Vec::len));
    // By DiaCraft's name first: a dash named for pattern n whose runs are the table's for n, in the form its name
    // says (flat or not), is pattern n, whatever cap the style that shows it holds.
    let by_name = dash.and_then(|d| d.own).zip(runs.as_ref()).and_then(|((n, flat), runs)| matches_pattern(n, runs, flat).then_some(n));
    // Otherwise by the runs, in the form the style's cap gives.
    let found = by_name.or_else(|| {
        let runs = runs?;
        PATTERNS.iter().find(|(n, _)| matches_pattern(*n, &runs, cap == 1)).map(|(n, _)| *n)
    });
    match found {
        Some(n) => n,
        None => {
            r.keep("dash");
            2
        }
    }
}

/// Whether `runs` (marks then gaps, in weights) are pattern `n`'s, within 0.01 weight, after a flat cap's form is taken back to the table's
/// when `flat` (with `LineCap` 1, each mark one weight longer and each gap one shorter).
fn matches_pattern(n: u8, runs: &[f64], flat: bool) -> bool {
    let Some((_, table)) = PATTERNS.iter().find(|(p, _)| *p == n) else { return false };
    let back = |i: usize, run: f64| {
        if !flat {
            run
        } else if i.is_multiple_of(2) {
            run - 1.0
        } else {
            run + 1.0
        }
    };
    table.len() == runs.len() && table.iter().zip(runs).enumerate().all(|(i, (t, v))| (t - back(i, *v)).abs() <= 0.01)
}

/// The prefix of DiaCraft's own dash names: the number of the pattern follows it.
pub(crate) const DASH_PREFIX: &str = "DiaCraft_20_dash_20_";

/// What follows the number in the name of a dash drawn for a flat cap.
pub(crate) const FLAT_SUFFIX: &str = "_20_flat";

/// The pattern and cap form of DiaCraft's own dash of this name: `DiaCraft_20_dash_20_<n>` for 2 to 23, and `_20_flat` after it for the
/// form of a flat cap, the number written with no sign and no leading zero. `None` for any other name.
pub(crate) fn own_dash(name: &str) -> Option<(u8, bool)> {
    let rest = name.strip_prefix(DASH_PREFIX)?;
    let (digits, flat) = match rest.strip_suffix(FLAT_SUFFIX) {
        Some(digits) => (digits, true),
        None => (rest, false),
    };
    let n: u8 = digits.parse().ok()?;
    ((2..=23).contains(&n) && n.to_string() == digits).then_some((n, flat))
}

/// A dash's runs, mark then gap, in weights, or `None` when any count, length or the gap cannot be read or the weight is 0.
fn runs(dash: &Dash, weight: f64) -> Option<Vec<f64>> {
    let in_weights = |run: Run| match run {
        Run::Percent(p) => Some(p / 100.0),
        Run::Inches(i) => (weight > 0.0).then(|| i / weight),
    };
    let gap = in_weights(dash.distance?)?;
    let dots1 = dash.dots1.unwrap_or(0);
    let dots2 = dash.dots2.unwrap_or(0);
    if dots1.saturating_add(dots2) > MOST_MARKS || dots1.saturating_add(dots2) == 0 {
        return None;
    }
    let mut out = Vec::new();
    if dots1 > 0 {
        let mark = in_weights(dash.dots1_length?)?;
        for _ in 0..dots1 {
            out.extend([mark, gap]);
        }
    }
    if dots2 > 0 {
        let mark = in_weights(dash.dots2_length?)?;
        for _ in 0..dots2 {
            out.extend([mark, gap]);
        }
    }
    Some(out)
}

/// The spread of each line end across the line, in `S`: the where the table measures it, and the (DiaCraft's own rule) where it
/// does not: 8 and 19 take 4's box, 18 takes 6's, 31 to 37 are a disc and bars 1.5 across, 34 and 38 carry a long diamond 2.24 across, and 40, 44 and 45 are
/// heads and triangles 2 across. `None` for a number that is not 1 to 45.
pub(crate) fn spread(n: u32) -> Option<f64> {
    match n {
        1..=4 | 8 | 9 | 12..=17 | 19 | 23 | 39 | 40 | 43..=45 => Some(2.0),
        5 => Some(1.99),
        6 | 18 => Some(2.02),
        7 => Some(1.92),
        10 | 11 | 20 | 21 | 24..=33 | 35..=37 | 41 | 42 => Some(1.5),
        22 | 34 | 38 => Some(2.24),
        _ => None,
    }
}

/// The base of each size class, in points: the 0 to 5, and class 6 at the 18 points (DiaCraft's own rule).
pub(crate) const SIZE_BASES: [f64; 7] = [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0];

/// The number of DiaCraft's filled diamond, the custom end of UML composition: its marker is `DiaCraft_20_end_20_254`, the one marker whose number is not a line end of (the
/// cell it stands for is 254, the master).
pub(crate) const DIAMOND: u32 = 254;

/// The width of the filled diamond's marker at a size class of `base` points and a line of `weight_pt` points: ten times the height of the alignment box, which is the
/// weight times the unit at the class over the unit at Medium, both at that weight. 0 for a weight of 0: a box of no height draws nothing.
pub(crate) fn diamond_width(base: f64, weight_pt: f64) -> f64 {
    10.0 * weight_pt * (base + weight_pt) / (SIZE_BASES[2] + weight_pt)
}

/// How far a marker's width may be from a size class's, in points, for the class to be the marker's: over twice LibreOffice's largest rounding error
/// (0.046 pt) and under half the smallest gap between two classes of one number (0.54 pt).
pub(crate) const WIDTH_TOLERANCE: f64 = 0.1;

/// The size class whose width for number `n` at line weight `weight_pt` is nearest `width_pt`, when it is within [`WIDTH_TOLERANCE`] of it; `None` when
/// no class is, or the number has no spread.
pub(crate) fn size_class(n: u32, weight_pt: f64, width_pt: f64) -> Option<usize> {
    // The filled diamond's width follows the weight in a way of its own; at a weight of 0 every class is as wide as another (none), so no class is read.
    let diamond = n == DIAMOND;
    if diamond && !(weight_pt.is_finite() && weight_pt > 0.0) {
        return None;
    }
    let spread = if diamond { 0.0 } else { spread(n)? };
    let (class, off) = SIZE_BASES
        .iter()
        .enumerate()
        .map(|(class, base)| {
            let width = if diamond { diamond_width(*base, weight_pt) } else { spread * (base + weight_pt) };
            (class, (width - width_pt).abs())
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (off <= WIDTH_TOLERANCE).then_some(class)
}

/// A name of DiaCraft's own marker: the number and the band class its suffix says (`None` for a name with no suffix).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnMarker {
    pub number: u32,
    pub band: Option<u32>,
}

/// The prefix of DiaCraft's own marker names.
pub(crate) const MARKER_PREFIX: &str = "DiaCraft_20_end_20_";

/// The suffix that names a band class: `_20_t` and the class.
pub(crate) const BAND_SUFFIX: &str = "_20_t";

/// A whole number written with no sign and no leading zero, from `low` to `high`.
fn plain_number(digits: &str, low: u32, high: u32) -> Option<u32> {
    // Digits only and no leading zero; a number past 32 bits is refused by the parse, and past `high` by the range.
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (low..=high).contains(&n).then_some(n)
}

/// DiaCraft's own marker a name is: `DiaCraft_20_end_20_` and a number from 1 to 45 with no sign and no leading zero, then, optionally, `_20_t`
/// and a band class from 1 to 99 written the same way; or [`DIAMOND`] alone, with no band (the filled diamond). Any other spelling is no marker of DiaCraft's.
pub(crate) fn parse_marker(name: &str) -> Option<OwnMarker> {
    let rest = name.strip_prefix(MARKER_PREFIX)?;
    let at = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, tail) = rest.split_at(at);
    if digits == "254" {
        return tail.is_empty().then_some(OwnMarker { number: DIAMOND, band: None });
    }
    let number = plain_number(digits, 1, 45)?;
    if tail.is_empty() {
        return Some(OwnMarker { number, band: None });
    }
    let band = plain_number(tail.strip_prefix(BAND_SUFFIX)?, 1, 99)?;
    Some(OwnMarker { number, band: Some(band) })
}

/// The number DiaCraft's own marker of this name draws: `DiaCraft_20_end_20_<n>` for 1 to 45, with the band suffix or none.
pub(crate) fn own_marker(name: &str) -> Option<u32> {
    parse_marker(name).map(|m| m.number)
}

/// A line end's two cells.
fn line_end(r: &mut Reader, view: &View<'_>, base: &Base, props: (P, P), cells_named: (&'static str, &'static str), out: &mut Out) {
    let g = Kind::Graphic;
    let (end, width) = props;
    let (arrow, size) = cells_named;
    let Some((Typed::Marker(marker), _)) = view.typed(r, g, end) else { return };
    let n = match marker {
        Marker::Empty => {
            out.direct.push((arrow, cells::plain(0.0)));
            return;
        }
        Marker::Missing | Marker::Unresolved => {
            r.warn("marker_not_found", || "a line names a marker the drawing does not have; it draws no line end".to_owned());
            out.direct.push((arrow, cells::plain(0.0)));
            return;
        }
        Marker::Other => {
            // Any other marker: kept, with a solid triangle of size 2 standing in.
            r.keep("marker");
            out.other_ends.push(arrow);
            out.direct.push((arrow, cells::plain(4.0)));
            out.direct.push((size, cells::plain(2.0)));
            return;
        }
        Marker::Own(n) => *n,
    };
    // The filled diamond is the marker of a custom end (254), which the drawing's own data says: the markup says number 4 (the stand-in of every custom end) and the class its width
    // gives, so that a file read without DiaCraft's data draws what a plain reader draws for a custom end, and a file read with it takes the cell back from tier 2.
    out.direct.push((arrow, cells::plain(f64::from(if n == DIAMOND { 4 } else { n }))));
    let weight_pt = view.length(r, g, P::StrokeWidth).map_or(base.weight, |l| l.inches) * 72.0;
    let class = view.length(r, g, width).and_then(|w| size_class(n, weight_pt, w.inches * 72.0));
    match class {
        Some(class) => out.direct.push((size, cells::plain(class as f64))),
        None => {
            r.keep("marker_width");
            out.direct.push((size, cells::plain(2.0)));
        }
    }
}

/// The text-block cells of graphic properties.
pub(crate) fn text_block(r: &mut Reader, view: &View<'_>, own: Own<'_>, out: &mut Out) {
    let g = Kind::Graphic;
    if own(g, P::TextareaVerticalAlign) {
        match view.value(r, g, P::TextareaVerticalAlign) {
            Some("top") => out.direct.push(("VerticalAlign", cells::plain(0.0))),
            Some("middle") => out.direct.push(("VerticalAlign", cells::plain(1.0))),
            Some("bottom") => out.direct.push(("VerticalAlign", cells::plain(2.0))),
            Some("justify") => {
                r.keep("property_value");
                out.direct.push(("VerticalAlign", cells::plain(1.0)));
            }
            Some(_) => r.keep("property_value"),
            None => {}
        }
    }
    for (side, cell) in
        [(P::PaddingTop, "TopMargin"), (P::PaddingBottom, "BottomMargin"), (P::PaddingLeft, "LeftMargin"), (P::PaddingRight, "RightMargin")]
    {
        if !(own(g, side) || own(g, P::Padding)) {
            continue;
        }
        // The nearer of the side's own attribute and `fo:padding`; on one style the side's own wins.
        let specific = view.typed(r, g, side);
        let all = view.typed(r, g, P::Padding);
        let value = match (specific, all) {
            (Some((s, hs)), Some((a, ha))) => {
                if hs.rank() <= ha.rank() {
                    s
                } else {
                    a
                }
            }
            (Some((s, _)), None) => s,
            (None, Some((a, _))) => a,
            (None, None) => continue,
        };
        if let Typed::Length(Some(l)) = value {
            out.direct.push((cell, cells::length(l.inches, l.unit.word())));
        }
    }
    if own(g, P::WritingMode)
        && let Some(mode) = view.value(r, g, P::WritingMode)
    {
        let vertical = matches!(mode, "tb-rl" | "tb" | "tb-lr" | "sideways-rl" | "sideways-lr");
        out.direct.push(("TextDirection", cells::plain(if vertical { 1.0 } else { 0.0 })));
    }
}

/// The paragraph cells, with the tab stops and `DefaultTabStop`.
pub(crate) fn paragraph(r: &mut Reader, view: &View<'_>, own: Own<'_>, out: &mut Out) {
    let k = Kind::Paragraph;
    let rtl = |mode: Option<&str>| matches!(mode, Some("rl-tb" | "rl"));
    if own(k, P::TextAlign) || own(k, P::TextAlignLast) || own(k, P::WritingMode) {
        let mode = view.value(r, k, P::WritingMode);
        // `start` and `end` follow the paragraph's writing direction, so they are the line's first and last side whatever it is: 0 and 2. `left`
        // and `right` are the page's sides, which a right-to-left paragraph reads against: `left` is the line's end there, and `right` its start.
        let align = match view.value(r, k, P::TextAlign) {
            Some("left") => Some(if rtl(mode) { 2.0 } else { 0.0 }),
            Some("start") => Some(0.0),
            Some("center") => Some(1.0),
            Some("right") => Some(if rtl(mode) { 0.0 } else { 2.0 }),
            Some("end") => Some(2.0),
            Some("justify") => Some(if view.value(r, k, P::TextAlignLast) == Some("justify") { 4.0 } else { 3.0 }),
            Some(_) => {
                r.keep("property_value");
                None
            }
            None => None,
        };
        if let Some(align) = align {
            out.paragraph.push(("HorzAlign", cells::plain(align)));
        }
    }
    if own(k, P::WritingMode) {
        let mode = view.value(r, k, P::WritingMode);
        out.paragraph.push(("Flags", cells::plain(if rtl(mode) { 1.0 } else { 0.0 })));
    }
    for (p, cell) in [
        (P::MarginLeft, "IndLeft"),
        (P::MarginRight, "IndRight"),
        (P::TextIndent, "IndFirst"),
        (P::MarginTop, "SpBefore"),
        (P::MarginBottom, "SpAfter"),
    ] {
        if own(k, p)
            && let Some((typed, _)) = view.typed(r, k, p)
        {
            match typed {
                Typed::Length(Some(l)) => out.paragraph.push((cell, cells::length(l.inches, l.unit.word()))),
                _ => r.keep("property_value"),
            }
        }
    }
    if own(k, P::LineHeight)
        && let Some((typed, _)) = view.typed(r, k, P::LineHeight)
    {
        match typed {
            Typed::LineHeight(LineHeight::Percent(p)) => out.paragraph.push(("SpLine", cells::plain(-p / 100.0))),
            Typed::LineHeight(LineHeight::Length(l)) => out.paragraph.push(("SpLine", cells::length(l.inches, l.unit.word()))),
            Typed::LineHeight(LineHeight::Normal) => {}
            _ => r.keep("property_value"),
        }
    }
    if own(k, P::TabStopDistance)
        && let Some(l) = view.length(r, k, P::TabStopDistance)
    {
        out.direct.push(("DefaultTabStop", cells::length(l.inches, l.unit.word())));
    }
}

/// The script type of the letters a text reads through, for the one language of a run: the type most of the **letters** belong to, a letter being
/// a character of a script of its own (digits, punctuation and spaces are of none), and **a tie goes to the Latin type, then the Asian one** (the Latin triplet is the one a run
/// with no letters reads, so it wins where the letters do not say). Every byte is a unit of the work, charged before the walk.
pub(crate) fn script_type_of<'t>(r: &mut Reader, text: impl Iterator<Item = &'t str>) -> FontSlot {
    let mut letters = crate::lang::Letters::default();
    for piece in text {
        r.text_units(piece.len());
        letters.add(piece);
    }
    letters.slot()
}

/// The language the triplet of the script type `slot` says, as the tag a `LangID` holds, read through the chain the view is: the type's `style:rfc-language-tag`
/// when it is there and as near as the language (it says the same and more), else its language with its script and its country, each from the nearest style that states it. A triplet whose
/// language is `zxx` or `none` is **no language** (`x-none`), and a country or a script that is `none` (as LibreOffice writes beside `zxx`) says nothing. `None` when the triplet says no
/// language, or says one that is no well-formed tag (RFC 5646): the run then keeps what it inherits.
pub(crate) fn language_of(r: &mut Reader, view: &View<'_>, slot: FontSlot) -> Option<String> {
    use diacraft_doc::lang;
    let [language, script, country, rfc] = P::triplet(slot);
    let k = Kind::Text;
    let language = view.get(r, k, language);
    // A full tag that a style states farther off than the language is overruled by the language.
    let full = view.get(r, k, rfc).filter(|(_, hit)| language.is_none_or(|(_, near)| hit.rank() <= near.rank())).map(|(text, _)| text);
    if let Some(text) = full
        && let Ok(tag) = lang::parse(text)
    {
        return Some(if lang::is_no_language(tag.as_str()) { "x-none".to_owned() } else { tag.as_str().to_owned() });
    }
    let (language, _) = language?;
    if language.eq_ignore_ascii_case("zxx") || language.eq_ignore_ascii_case("none") {
        return Some("x-none".to_owned());
    }
    let parts: Vec<&str> = [script, country]
        .into_iter()
        .filter_map(|part| view.value(r, k, part))
        .filter(|text| !text.is_empty() && !text.eq_ignore_ascii_case("none"))
        .collect();
    // A tag longer than a tag may be is no tag: it is not joined (a style may state a megabyte for each of the three).
    let length = parts.iter().fold(language.len(), |sum, part| sum.saturating_add(part.len()).saturating_add(1));
    if length > diacraft_doc::lang::MAX_TAG_BYTES {
        return None;
    }
    let mut joined = String::with_capacity(length);
    joined.push_str(language);
    for part in parts {
        joined.push('-');
        joined.push_str(part);
    }
    let tag = lang::parse(&joined).ok()?;
    Some(if lang::is_no_language(tag.as_str()) { "x-none".to_owned() } else { tag.as_str().to_owned() })
}

/// The character cells. `slot` is the script type of the run's letters, whose triplet its `LangID` is read from ([`script_type_of`]): the Latin one for a style sheet, which has no
/// letters.
#[allow(clippy::too_many_arguments)]
pub(crate) fn character<'a>(
    r: &mut Reader,
    texts: &mut StyleTexts<'a>,
    view: &View<'a>,
    own: Own<'_>,
    size: Option<f64>,
    base: &Base,
    slot: FontSlot,
    out: &mut Out,
) -> Result<(), OdgError> {
    let k = Kind::Text;
    if own(k, P::FontSize)
        && let Some(size) = size
    {
        let unit = match view.typed(r, k, P::FontSize) {
            Some((Typed::Size(Size::Length(l)), _)) => l.unit.word(),
            _ => Some("PT"),
        };
        out.character.push(("Size", cells::length(size, unit)));
    }
    let bits = [P::FontWeight, P::FontStyle, P::UnderlineStyle, P::UnderlineType, P::FontVariant, P::FontWeightComplex, P::FontStyleComplex];
    if bits.iter().any(|p| own(k, *p)) {
        let mut style = base.style_bits;
        let mut set = |bit: u32, on: Option<bool>| match on {
            Some(true) => style |= bit,
            Some(false) => style &= !bit,
            None => {}
        };
        set(1, view.value(r, k, P::FontWeight).and_then(bold));
        set(2, view.value(r, k, P::FontStyle).map(|s| matches!(s, "italic" | "oblique")));
        let underline = view.value(r, k, P::UnderlineStyle).map(|s| s != "none");
        let double = view.value(r, k, P::UnderlineType) == Some("double");
        set(4, underline.map(|u| u && !double));
        set(8, view.value(r, k, P::FontVariant).map(|v| v == "small-caps"));
        set(16, view.value(r, k, P::FontWeightComplex).and_then(bold));
        set(32, view.value(r, k, P::FontStyleComplex).map(|s| matches!(s, "italic" | "oblique")));
        out.character.push(("Style", cells::plain(f64::from(style))));
    }
    if own(k, P::UnderlineStyle) || own(k, P::UnderlineType) {
        let on = view.value(r, k, P::UnderlineStyle).is_some_and(|s| s != "none");
        let double = view.value(r, k, P::UnderlineType) == Some("double");
        out.character.push(("DblUnderline", cells::plain(if on && double { 1.0 } else { 0.0 })));
    }
    if own(k, P::LineThroughStyle) || own(k, P::LineThroughType) {
        let on = view.value(r, k, P::LineThroughStyle).is_some_and(|s| s != "none");
        let double = view.value(r, k, P::LineThroughType) == Some("double");
        out.character.push(("Strikethru", cells::plain(if on && !double { 1.0 } else { 0.0 })));
        out.character.push(("DoubleStrikethrough", cells::plain(if on && double { 1.0 } else { 0.0 })));
    }
    if own(k, P::OverlineStyle) {
        let on = view.value(r, k, P::OverlineStyle).is_some_and(|s| s != "none");
        out.character.push(("Overline", cells::plain(if on { 1.0 } else { 0.0 })));
    }
    if own(k, P::TextTransform) {
        match view.value(r, k, P::TextTransform) {
            Some("uppercase") => out.character.push(("Case", cells::plain(1.0))),
            Some("capitalize") => out.character.push(("Case", cells::plain(2.0))),
            Some("none") => out.character.push(("Case", cells::plain(0.0))),
            Some(_) => r.keep("property_value"),
            None => {}
        }
    }
    if own(k, P::TextPosition)
        && let Some((Typed::Position { pos, percent_sign }, _)) = view.typed(r, k, P::TextPosition)
    {
        if *percent_sign {
            r.keep("text_position");
        }
        match pos {
            Some(pos) => out.character.push(("Pos", cells::plain(f64::from(*pos)))),
            None => r.keep("property_value"),
        }
    }
    if own(k, P::Color)
        && let Some(c) = view.colour(r, k, P::Color)
    {
        out.character.push(("Color", cells::text(&c)));
    }
    if own(k, P::LetterSpacing)
        && let Some((typed, _)) = view.typed(r, k, P::LetterSpacing)
    {
        match typed {
            Typed::Spacing(Spacing::Normal) => out.character.push(("Letterspace", cells::plain(0.0))),
            Typed::Spacing(Spacing::Length(l)) => out.character.push(("Letterspace", cells::length(l.inches, l.unit.word()))),
            _ => {}
        }
    }
    if own(k, P::TextScale)
        && let Some(p) = view.percent(r, k, P::TextScale)
    {
        out.character.push(("FontScale", cells::plain(p / 100.0)));
    }
    // The language: a cell when any of the twelve attributes is the conversion's own, whichever triplet it is in, since the type the run's letters belong to picks the triplet.
    if [FontSlot::Latin, FontSlot::Asian, FontSlot::Complex].into_iter().any(|s| P::triplet(s).into_iter().any(|p| own(k, p)))
        && let Some(tag) = language_of(r, view, slot)
    {
        // A tag of a cell's own short text is compared as it is; a longer one (a tag may be 255 bytes) is one shared value for each distinct tag, as a family is.
        let cell = if tag.len() <= cells::SHORT_TEXT { cells::text(&tag) } else { Cell::constant(r.shared_value(&tag)?) };
        out.character.push(("LangID", cell));
    }
    for (name, family, cell) in [
        (P::FontName, P::FontFamily, "Font"),
        (P::FontNameAsian, P::FontFamilyAsian, "AsianFont"),
        (P::FontNameComplex, P::FontFamilyComplex, "ComplexScriptFont"),
    ] {
        if own(k, name) || own(k, family) {
            let declared = match view.get(r, k, name) {
                Some((n, hit)) => {
                    let file = view.set.get(hit.found.style).map_or(File::Content, |s| s.file);
                    texts.face(r, n, file)?
                }
                None => None,
            };
            let face = declared.or_else(|| match view.prop(r, k, family) {
                // Cut of its white space and quotes when its style was read.
                Some((prop, _)) => match prop.typed {
                    Typed::Family { from, to } => prop.text.get(from..to),
                    _ => None,
                },
                None => None,
            });
            // The family is the file's text: one shared value however many cells show it.
            if let Some(face) = face {
                let value = texts.value(r, face)?;
                out.character.push((cell, Cell::constant(value)));
            }
        }
    }
    Ok(())
}

/// `fo:font-weight` as bold or not: `bold` or 600 to 900 bold, `normal` or 100 to 500 not.
pub(super) fn bold(text: &str) -> Option<bool> {
    match text {
        "bold" | "600" | "700" | "800" | "900" => Some(true),
        "normal" | "100" | "200" | "300" | "400" | "500" => Some(false),
        _ => None,
    }
}

/// Whether two cells of a conversion hold the same value and the same formula text, in work that a style's strings do not lengthen.
///
/// **A long text is told by where it is held.** A text longer than [`cells::SHORT_TEXT`] in a cell of a conversion is a style's text (a
/// font's family, a bullet's characters), and every such text is one shared value for each distinct text (`StyleTexts::value`, made by
/// `Reader::shared_value`): two cells that show equal long texts show one value, at one address, and two cells that show different long
/// texts show different values. So two long texts are the same when they are the same bytes in memory, and not the same otherwise, and no
/// byte is read, whatever their length: a million-character family shown by thousands of shape-style pairs is compared by an address each
/// time (which walked it). A short text is compared as it is, at most [`cells::SHORT_TEXT`] bytes, and counted
/// ([`crate::ReadWork::compared_bytes`]); texts of different lengths are different without a byte read.
///
/// **A formula text is told the same way,** and none of a conversion's cells holds one: the cells it makes come from `cells::plain`,
/// `cells::length`, `cells::text` and a shared value, which write a value and no formula, and a style gives values and no formula (a test
/// reads a style that states every property and finds none). The formulas the reader writes are its own short texts, on shapes' geometry.
/// Were one ever to be added, it would be told by the address of its parse (`Forms` shares one parse for each text it keeps).
///
/// **A text not told by this is not wrongly told the same.** A long text that is not a shared value and is a copy of another's bytes would
/// be called different; the answer is then "write the cell", which keeps a cell equal to what its link gives, and never drops one that
/// differs. No cell of a conversion has such a text.
pub(crate) fn same_cell(r: &mut Reader, a: &Cell, b: &Cell) -> bool {
    same_text(r, a.value.as_ref().map(diacraft_doc::Value::text), b.value.as_ref().map(diacraft_doc::Value::text))
        && same_text(r, a.formula.text(), b.formula.text())
}

/// Whether two optional texts are the same: both missing, or both there and the same by the rule of [`same_cell`].
fn same_text(r: &mut Reader, a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => {
            if std::ptr::eq(x, y) {
                return true;
            }
            if x.len() != y.len() || x.len() > cells::SHORT_TEXT {
                return false;
            }
            r.compared_bytes(x.len());
            x == y
        }
        (None, None) => true,
        _ => false,
    }
}

/// The tab stops as a `Tabs` row's cells: `Position<j>` and `Alignment<j>` for each stop `j` from 0.
pub(crate) fn tab_cells(tabs: &[Tab]) -> Vec<(String, Cell)> {
    let mut out = Vec::with_capacity(tabs.len() * 2);
    for (j, tab) in tabs.iter().enumerate() {
        out.push((format!("Position{j}"), cells::length(tab.position.inches, tab.position.unit.word())));
        out.push((format!("Alignment{j}"), cells::plain(f64::from(tab.alignment))));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, FormulaSlot, Value};

    use super::{
        BAND_SUFFIX, DIAMOND, MARKER_PREFIX, OwnMarker, Reader, SIZE_BASES, cells, diamond_width, own_marker, parse_marker, same_cell, size_class,
        spread,
    };
    use crate::limits::Limits;

    /// A cell holding `text` as a value of its own allocation (not shared with any other cell's).
    fn valued(text: &str) -> Cell {
        Cell::constant(Value::from_text(text))
    }

    /// A cell with the local formula `source` (its own parse).
    fn formula(source: &str) -> Cell {
        Cell { formula: FormulaSlot::Local(Arc::new(diacraft_formula::parse(source))), ..Cell::default() }
    }

    #[test]
    fn short_texts_are_compared_as_they_are_and_the_bytes_read_are_counted() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        assert!(same_cell(&mut r, &valued("0.25"), &valued("0.25")));
        assert_eq!(r.report.work.compared_bytes, 4, "equal texts of two places are read");
        assert!(!same_cell(&mut r, &valued("0.25"), &valued("0.26")));
        assert_eq!(r.report.work.compared_bytes, 8);
        // Different lengths are different without a byte read, and so is a missing value.
        assert!(!same_cell(&mut r, &valued("0.25"), &valued("0.2")));
        assert!(!same_cell(&mut r, &valued("0.25"), &Cell::default()));
        assert!(!same_cell(&mut r, &Cell::default(), &valued("0.25")));
        assert_eq!(r.report.work.compared_bytes, 8);
        // Two cells with no value and no formula are the same.
        assert!(same_cell(&mut r, &Cell::default(), &Cell::default()));
        assert_eq!(r.report.work.compared_bytes, 8);
        // One value in two cells is one text at one address: the same, and nothing is read.
        let shared = valued("0.25");
        assert!(same_cell(&mut r, &shared, &shared.clone()));
        assert_eq!(r.report.work.compared_bytes, 8);
    }

    #[test]
    fn a_text_of_the_short_limit_is_read_and_a_longer_one_is_told_by_its_address() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let of = |len: usize, last: char| format!("{}{last}", "a".repeat(len - 1));
        let short = cells::SHORT_TEXT;
        // At the limit, equal texts of two places are the same and different ones are not; both are read.
        assert!(same_cell(&mut r, &valued(&of(short, 'a')), &valued(&of(short, 'a'))));
        assert!(!same_cell(&mut r, &valued(&of(short, 'a')), &valued(&of(short, 'b'))));
        assert_eq!(r.report.work.compared_bytes, 2 * short as u64);
        // One byte past it, a text is the same as the text it is (one shared value, one address) and as no other: no byte is read, and a
        // copy of the same bytes that is not the shared value is not the same (a cell written that need not be, never one dropped).
        let long = valued(&of(short + 1, 'a'));
        assert!(same_cell(&mut r, &long, &long.clone()));
        assert!(!same_cell(&mut r, &long, &valued(&of(short + 1, 'a'))), "a copy is another value");
        assert!(!same_cell(&mut r, &long, &valued(&of(short + 1, 'b'))));
        assert!(!same_cell(&mut r, &long, &valued(&of(short + 2, 'a'))));
        assert_eq!(r.report.work.compared_bytes, 2 * short as u64);
    }

    #[test]
    fn a_formula_text_is_told_the_same_way_as_a_value() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let long = "Width*0.5+".repeat(cells::SHORT_TEXT) + "Height";
        let (a, b, c) = (formula(&long), formula(&long), formula(&long.replace("Height", "Heighu")));
        assert_eq!(long.len(), c.formula.text().map_or(0, str::len), "equal lengths");
        assert_ne!(Some(long.as_str()), c.formula.text());
        // One parse in two cells is one text: the same. Two parses of a long text, equal or not, are told by their address, and read nowhere.
        assert!(same_cell(&mut r, &a, &a.clone()));
        assert!(!same_cell(&mut r, &a, &b));
        assert!(!same_cell(&mut r, &a, &c));
        assert_eq!(r.report.work.compared_bytes, 0);
        // A short formula is read, and the markers are words.
        assert!(same_cell(&mut r, &formula("Width*0.5"), &formula("Width*0.5")));
        assert!(!same_cell(&mut r, &formula("Width*0.5"), &formula("Width*0.6")));
        assert_eq!(r.report.work.compared_bytes, 18);
        let marker = |slot: FormulaSlot| Cell { formula: slot, ..Cell::default() };
        assert!(same_cell(&mut r, &marker(FormulaSlot::Inherit), &marker(FormulaSlot::Inherit)));
        assert!(same_cell(&mut r, &marker(FormulaSlot::NoFormula), &marker(FormulaSlot::NoFormula)));
        assert!(!same_cell(&mut r, &marker(FormulaSlot::Inherit), &marker(FormulaSlot::NoFormula)));
        assert!(!same_cell(&mut r, &marker(FormulaSlot::Absent), &marker(FormulaSlot::Inherit)));
        assert!(!same_cell(&mut r, &marker(FormulaSlot::Absent), &formula("Width")));
        // A cell is the same only when its value and its formula both are.
        let both = |f: &str, v: &str| Cell { formula: formula(f).formula, value: Some(Value::from_text(v)), ..Cell::default() };
        assert!(same_cell(&mut r, &both("Width", "1"), &both("Width", "1")));
        assert!(!same_cell(&mut r, &both("Width", "1"), &both("Width", "2")));
        assert!(!same_cell(&mut r, &both("Width", "1"), &both("Height", "1")));
    }

    #[test]
    fn diacraft_s_markers_by_their_exact_names() {
        assert_eq!(own_marker("DiaCraft_20_end_20_13"), Some(13));
        assert_eq!(own_marker("DiaCraft_20_end_20_1"), Some(1));
        assert_eq!(own_marker("DiaCraft_20_end_20_45"), Some(45));
        for other in ["DiaCraft_20_end_20_013", "DiaCraft_20_end_20_+5", "DiaCraft_20_end_20_0", "DiaCraft_20_end_20_46", "DiaCraft_end_13", "Arrow"]
        {
            assert_eq!(own_marker(other), None, "{other}");
        }
    }

    #[test]
    fn a_band_suffix_is_read_as_the_class_and_any_other_spelling_is_no_marker_of_diacraft_s() {
        let own = |n: u32, band: Option<u32>| Some(OwnMarker { number: n, band });
        assert_eq!(parse_marker("DiaCraft_20_end_20_16_20_t23"), own(16, Some(23)));
        assert_eq!(parse_marker("DiaCraft_20_end_20_4_20_t1"), own(4, Some(1)));
        assert_eq!(parse_marker("DiaCraft_20_end_20_45_20_t99"), own(45, Some(99)));
        assert_eq!(parse_marker("DiaCraft_20_end_20_16"), own(16, None));
        assert_eq!(own_marker("DiaCraft_20_end_20_16_20_t23"), Some(16));
        for bad in [
            "DiaCraft_20_end_20_16_20_t0",
            "DiaCraft_20_end_20_16_20_t023",
            "DiaCraft_20_end_20_16_20_t100",
            "DiaCraft_20_end_20_16_20_T23",
            "DiaCraft_20_end_20_16_20_t",
            "DiaCraft_20_end_20_16_20_t+23",
            "DiaCraft_20_end_20_16_20_t-23",
            "DiaCraft_20_end_20_16_20_t2 3",
            "DiaCraft_20_end_20_16_20_t23_",
            "DiaCraft_20_end_20_16_t23",
            "DiaCraft_20_end_20_16 _20_t23",
            "DiaCraft_20_end_20__20_t23",
            "DiaCraft_20_end_20_0_20_t23",
            "DiaCraft_20_end_20_46_20_t23",
            "DiaCraft_20_end_20_1234_20_t23",
            "diacraft_20_end_20_16",
            "DiaCraft_20_end_20_100",
            "DiaCraft_20_end_20_999",
            "DiaCraft_20_end_20_16_20_t100",
            "DiaCraft_20_end_20_16_20_t999",
            "DiaCraft_20_end_20_4294967296",
        ] {
            assert_eq!(parse_marker(bad), None, "{bad}");
        }
        // A long run of digits is no number of ours, however many there are.
        let long = "9".repeat(1_000_000);
        assert_eq!(parse_marker(&format!("{MARKER_PREFIX}{long}")), None);
        assert_eq!(parse_marker(&format!("{MARKER_PREFIX}16{BAND_SUFFIX}{long}")), None);
        assert_eq!(parse_marker(&format!("{MARKER_PREFIX}{}", "1".repeat(1_000))), None);
    }

    #[test]
    fn every_number_has_a_spread_and_the_size_class_is_the_nearest_within_a_tenth_of_a_point() {
        for n in 1..=45 {
            assert!(spread(n).is_some(), "{n}");
        }
        assert_eq!((spread(0), spread(46), spread(254)), (None, None, None));
        // Number 13 (spread 2) at 0.75 pt: the classes are 2 x (base + 0.75).
        for (class, base) in SIZE_BASES.iter().enumerate() {
            let width = 2.0 * (base + 0.75);
            assert_eq!(size_class(13, 0.75, width), Some(class));
            assert_eq!(size_class(13, 0.75, width + 0.099), Some(class));
            assert_eq!(size_class(13, 0.75, width - 0.099), Some(class));
            assert_eq!(size_class(13, 0.75, width + 0.101), None);
            assert_eq!(size_class(13, 0.75, width - 0.101), None);
        }
        // No width matches two classes: the nearest is the one, and a width between two that is far from both is none.
        assert_eq!(size_class(13, 0.75, 2.0 * (2.52 + 0.75) + 0.54 / 2.0), None);
        assert_eq!(size_class(99, 0.75, 6.54), None, "a number with no spread has no class");
        assert_eq!(size_class(13, 0.75, f64::NAN), None);
    }

    /// The filled diamond is the marker `DiaCraft_20_end_20_254` with no band, wide as ten boxes are tall, and its class is read back from that width at the line's weight.
    #[test]
    fn the_filled_diamond_is_marker_254_with_no_band_and_its_class_is_read_from_a_width_that_follows_the_weight() {
        let own = |band: Option<u32>| Some(OwnMarker { number: DIAMOND, band });
        assert_eq!(parse_marker("DiaCraft_20_end_20_254"), own(None));
        assert_eq!(own_marker("DiaCraft_20_end_20_254"), Some(DIAMOND));
        for bad in [
            "DiaCraft_20_end_20_254_20_t23",
            "DiaCraft_20_end_20_0254",
            "DiaCraft_20_end_20_+254",
            "DiaCraft_20_end_20_2540",
            "DiaCraft_20_end_20_253",
            "DiaCraft_20_end_20_255",
            "DiaCraft_20_end_20_254_20_",
        ] {
            assert_eq!(parse_marker(bad), None, "{bad}");
        }
        // The width at Medium is ten weights (the box is as tall as the line is heavy), and at another class the weight times that class's unit over the Medium unit.
        assert!((diamond_width(2.52, 0.75) - 7.5).abs() < 1e-12 && (diamond_width(2.52, 3.0) - 30.0).abs() < 1e-12);
        assert!((diamond_width(3.96, 0.75) - 7.5 * (3.96 + 0.75) / (2.52 + 0.75)).abs() < 1e-12);
        assert_eq!(diamond_width(18.0, 0.0), 0.0, "a box with no height has no width");
        for weight in [0.25, 0.75, 1.5, 6.0, 40.0] {
            for (class, base) in SIZE_BASES.iter().enumerate() {
                let width = diamond_width(*base, weight);
                assert_eq!(size_class(DIAMOND, weight, width), Some(class), "class {class} at {weight} pt");
                assert_eq!(size_class(DIAMOND, weight, width + 0.05), Some(class));
            }
        }
        // A weight of 0 has no width to tell a class by, and a width far from every class is none.
        assert_eq!(size_class(DIAMOND, 0.0, 0.0), None);
        assert_eq!(size_class(DIAMOND, f64::NAN, 7.5), None);
        assert_eq!(size_class(DIAMOND, 0.75, 7.5 + 100.0), None);
        assert_eq!(size_class(DIAMOND, 0.75, f64::NAN), None);
    }
}
