//! Line ends (add a connector):
//! the `draw:marker` elements a save adds to `office:styles` of `styles.xml`, and the attributes of a style that name one.
//!
//! * **Names**: DiaCraft's own marker for a number is `DiaCraft_20_end_20_<n>`, `n` from 1 to 45 with no sign and no leading zero; a mark
//!   drawn with a band (a hollow or open part, the last column) is `DiaCraft_20_end_20_<n>_20_t<k>`, `k` the band class, 1 to 99. The display name has spaces
//!   (`DiaCraft end 16 t23`) and is always written, since LibreOffice rebuilds a marker's name from it. The reader knows a marker by its name alone, so one the
//!   file holds under the name is **used as it is, whatever it draws**: a program that saves our file rewrites the path and rounds the curves, and the name is
//!   still ours. Only the markers the file lacks are written, and the check of the bytes made looks at the form of those. A marker of DiaCraft's name that the
//!   save left no style naming is taken out, and never another producer's.
//! * **Width**: `draw:marker-start-width` and `-end-width` are the mark's spread (the for the numbers it does not measure) times
//!   the `S` for the size class at the line's weight, `S` being the class's base plus the weight, in points; class 6 is 18 pt. The reader finds the
//!   class again from the width within 0.1 pt.
//! * **The band**: a hollow or open part is a band of β = w / `S`, w being the line's weight (0.25 pt for a weight of 0), as a band class k = round(100 β)
//!   kept between 1 and 99; so the marker depends on the number and the band class, and a drawing holds one marker for each pair it uses.
//! * **Centre**: `draw:marker-start-center` and `-end-center` are `true` for the marks centres on the vertex (9, 10, 11, 20, 21 and 42), and
//!   written by no other.
//! * **The art is ours**: [`super::marker_art`] draws it, one path of the union of the pieces, the same at every size class that gives one band.
//!   **A custom end** (254) is a master the file has no place for: number 4's marker is written for it and the cell is carried by DiaCraft's extension (   [`drawn_as`]);
//!   a number past 45 and a size class outside 0 to 6 (read back as 2) have no art, and their cells are carried with no marker. A line weight past
//!   [`MOST_WEIGHT_PT`] is a width no file can hold: a line end on it is refused in words. The 1,024-byte bound on a path is on the markers drawn here
//!   ([`marker_art::MOST_PATH_BYTES`]); a held marker is used whatever its path.

use std::collections::{BTreeSet, HashMap};

use super::emit::Pfx;
use super::frame::Editor;
use super::marker_art;
use super::scan::{CommonStyles, MarkerDef, PartKind, scan_part};
use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::props::{BAND_SUFFIX, MARKER_PREFIX, SIZE_BASES, parse_marker, spread};

/// The prefix of DiaCraft's own marker names.
pub(crate) const PREFIX: &str = MARKER_PREFIX;

/// A marker of DiaCraft's own: a number from 1 to 45 and its band class, 0 for a mark drawn with no band.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Key {
    pub number: u32,
    pub band: u32,
}

impl Key {
    /// The marker's name: the prefix and the number, then the `_20_t` and the band class when there is one.
    pub(crate) fn name(self) -> String {
        if self.band == 0 { format!("{PREFIX}{}", self.number) } else { format!("{PREFIX}{}{BAND_SUFFIX}{}", self.number, self.band) }
    }

    /// The marker's display name: spaces for the underscores, which LibreOffice builds the name from again.
    pub(crate) fn display_name(self) -> String {
        if self.band == 0 { format!("DiaCraft end {}", self.number) } else { format!("DiaCraft end {} t{}", self.number, self.band) }
    }
}

/// The marker of DiaCraft's own that a name is, with the number and the band class it spells (0 for no suffix): the reader's rule.
pub(crate) fn key_of(name: &str) -> Option<Key> {
    parse_marker(name).map(|m| Key { number: m.number, band: m.band.unwrap_or(0) })
}

/// **A definition of `office:styles` of DiaCraft's own that a style names**: a line end's marker, or a gradient or a hatch ([`super::fx`]). Each is made from its name, so that a save
/// that names one adds it when the file lacks it, and takes it out when no style names it after the save, by the same rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Def {
    Marker(Key),
    Fx(super::fx::Fx),
}

impl Def {
    /// The definition's name (`draw:name`).
    pub(crate) fn name(&self) -> String {
        match self {
            Def::Marker(key) => key.name(),
            Def::Fx(fx) => fx.name(),
        }
    }

    /// The definition of DiaCraft's own that a name is: a marker by its number and band, a gradient or a hatch by what its name spells.
    pub(crate) fn of_name(name: &str) -> Option<Def> {
        key_of(name).map(Def::Marker).or_else(|| super::fx::Fx::of_name(name).map(Def::Fx))
    }
}

/// One line end's art: the marker's `svg:viewBox` and `svg:d`, and whether centres it on the vertex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Art {
    pub view_box: String,
    pub d: String,
    pub centred: bool,
}

/// The words of a refusal of a marker the art could not be made for (every number and band class is built by a test, so this is not met).
const WORDS_ART: &str = "a line end whose marker could not be drawn";

/// The words of a refusal of a custom line end where no tier 2 can carry its cell: it is a master drawn at the end, which one marker cannot
/// be, and number 4's marker written in its place would lose the custom end when the file is opened again.
pub(crate) const WORDS_CUSTOM: &str = "a custom line end (number 254, a master drawn at the end: a marker is one path in the line's colour, and saving it as an ordinary triangle would lose the custom end when the file is opened again)";

/// The words of a refusal of a number that is no line end.
const WORDS_NUMBER: &str = "a line end number that is not 0 to 45";

/// The words of a refusal of a size class the specification gives no base for.
pub(crate) const WORDS_SIZE: &str = "a line end size that is not 0 to 6 (the file would read it back as size 2)";

/// The art of marker `key`, or the words of why there is none. `steps` counts the work.
pub(crate) fn art(key: Key, steps: &mut u64) -> Result<Art, &'static str> {
    let built = marker_art::build(key.number, key.band, steps).ok_or(WORDS_ART)?;
    Ok(Art { view_box: built.view_box, d: built.d, centred: marker_art::is_centred(key.number) })
}

/// The size class a cell holds, when it is a whole number from 0 to 100 (`plan` refuses the ones the specification gives no base for).
pub(crate) fn class_of(value: f64) -> Option<u32> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= 100.0).then(|| u32::from(value as u8))
}

/// The size class of an end whose size cell holds `size`: 2 where there is none (the root style's), else the cell's class, or the words of a refusal when the cell
/// holds a number no class is (a class the writer cannot name is never taken for 2).
pub(crate) fn class_of_cell(size: Option<f64>) -> Result<u32, &'static str> {
    match size {
        None => Ok(2),
        Some(value) => class_of(value).ok_or(WORDS_SIZE),
    }
}

/// The most a line's weight may be, in points, for the end on it to be written: the marker's width follows the weight, and a
/// weight of 3e307 inches gave `infpt`, one of 1e300 a width of three hundred digits, and a band class that was no number. A million points is 13,888 inches, far
/// past any line a drawing holds, and the widest marker at it is under 2.3 million points, so every width written is a number of at most seven whole digits.
pub(crate) const MOST_WEIGHT_PT: f64 = 1_000_000.0;

/// The words of a refusal of a line end on a line heavier than [`MOST_WEIGHT_PT`].
pub(crate) const WORDS_WEIGHT: &str = "a line end on a line whose weight is more than a million points (the width of the marker follows the weight, and would be no length a file can hold)";

/// Whether `weight_pt` is a weight an end is written for: a number from 0 to [`MOST_WEIGHT_PT`], both included (a NaN or an infinity is none).
fn weight_in_range(weight_pt: f64) -> bool {
    (0.0..=MOST_WEIGHT_PT).contains(&weight_pt)
}

/// The line's weight in points: the one the cell holds (in inches) when it is a length, else 0.75 pt (the root style's); the words of a refusal for a
/// weight past [`MOST_WEIGHT_PT`], which is a length and no weight an end is written for.
pub(crate) fn weight_pt(weight: Option<f64>) -> Result<f64, &'static str> {
    let Some(inches) = weight.filter(|w| w.is_finite() && *w >= 0.0) else { return Ok(0.75) };
    // A product that overflows is an infinity, which is out of range.
    let pt = inches * 72.0;
    if weight_in_range(pt) { Ok(pt) } else { Err(WORDS_WEIGHT) }
}

/// The band class of a mark at size class base `base` and line weight `weight_pt`: β = w / `S`, w the weight or 0.25 pt for a weight of 0, `S` the base
/// plus the weight (the base alone for a weight of 0), as round(100 β) kept between 1 and 99.
fn band_class(base: f64, weight_pt: f64) -> u32 {
    let w = if weight_pt > 0.0 { weight_pt } else { 0.25 };
    let beta = w / (base + weight_pt);
    // A share, and finite: the weight is in range (`plan` asks) and the base above 1.
    (100.0 * beta).round().clamp(1.0, 99.0) as u32
}

/// What a line end is written as: its marker and the width of it in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plan {
    pub key: Key,
    pub width: f64,
}

/// The number whose marker is drawn for the end `number`: the custom end (254) is drawn as number 4, the cell itself being tier 2 (the reader rebuilds 4
/// from the marker, so a save that wrote no cell would lose the custom end); every other number is its own.
pub(crate) fn drawn_as(number: u32) -> u32 {
    if number == 254 { 4 } else { number }
}

/// The marker and the width of the end of `number` at size class `class` and line weight `weight_pt`: the spread times `S`, in points. The
/// words of a refusal for a number that is no line end (254, the custom end, in words of its own), for a class the specification gives no base for, and for a
/// weight that is no number from 0 to [`MOST_WEIGHT_PT`] (so the width and the band class are always finite and the width at most seven whole digits).
pub(crate) fn plan(number: u32, class: u32, weight_pt: f64) -> Result<Plan, &'static str> {
    if number == 254 {
        return Err(WORDS_CUSTOM);
    }
    let spread = spread(number).ok_or(WORDS_NUMBER)?;
    let base = usize::try_from(class).ok().and_then(|c| SIZE_BASES.get(c)).copied().ok_or(WORDS_SIZE)?;
    if !weight_in_range(weight_pt) {
        return Err(WORDS_WEIGHT);
    }
    let band = if marker_art::has_band(number) { band_class(base, weight_pt) } else { 0 };
    Ok(Plan { key: Key { number, band }, width: spread * (base + weight_pt) })
}

/// A width in points as an attribute writes it: at most four places, no trailing zero, in `pt` (the unit the specification gives it in, which the reader tells a class
/// within 0.1 of).
pub(crate) fn points_text(pt: f64) -> String {
    let rounded = (pt * 10_000.0).round() / 10_000.0;
    format!("{rounded}pt")
}

/// The element of a marker as it is written in `office:styles`, under the prefixes `draw` and `svg` (the display name has spaces).
pub(crate) fn element(key: Key, art: &Art, draw: &str, svg: &str, declare: &str) -> String {
    format!(
        "<{draw}:marker{declare} {draw}:name=\"{name}\" {draw}:display-name=\"{display}\" {svg}:viewBox=\"{view}\" {svg}:d=\"{d}\"/>",
        name = key.name(),
        display = key.display_name(),
        view = art.view_box,
        d = art.d
    )
}

impl Editor<'_, '_> {
    /// Puts the markers `needs` into `office:styles`, after the last element it holds, and gives the ones it wrote: a marker the file holds under the name is
    /// DiaCraft's by its name alone and is used as it is, whatever it draws (a save of another program rewrites the path and rounds the curves,
    /// and the reader takes the name for DiaCraft's marker all the same). The text of each element is charged as the splice charges it, and each marker is four
    /// attributes; the work of drawing each is counted ([`WriteWork::marker_art_steps`]). The markers of DiaCraft's name in `gone` that the file holds are taken
    /// out, and the ones it took out are given too.
    pub(crate) fn put_markers(
        &mut self,
        common: Option<&CommonStyles>,
        held: &[MarkerDef],
        needs: &BTreeSet<Def>,
        gone: &BTreeSet<Def>,
    ) -> Result<(BTreeSet<Def>, BTreeSet<Def>), WriteError> {
        let (mut wrote, mut removed): (BTreeSet<Def>, BTreeSet<Def>) = (BTreeSet::new(), BTreeSet::new());
        if needs.is_empty() && gone.is_empty() {
            return Ok((wrote, removed));
        }
        let Some(common) = common else {
            if needs.is_empty() {
                return Ok((wrote, removed));
            }
            return Err(WriteError::Unsupported("a styles part with no office:styles to put a line end's marker in"));
        };
        // The markers the file holds, by name, once: a look for each of at most 45 times 99.
        self.work.order_steps = self.work.order_steps.saturating_add(held.len() as u64);
        let mut by_name: HashMap<&str, Vec<&MarkerDef>> = HashMap::new();
        by_name.try_reserve(held.len()).map_err(|_| WriteError::Limit { what: "memory for the markers of a part", max: held.len() as u64 })?;
        for m in held {
            by_name.entry(m.name.as_str()).or_default().push(m);
        }
        for &key in gone.iter().filter(|k| !needs.contains(k)) {
            for m in by_name.get(key.name().as_str()).map_or(&[][..], Vec::as_slice) {
                self.splicer.remove(m.lead, m.end);
                removed.insert(key);
            }
        }
        let pfx = Pfx::of(&mut self.scopes, common.scope, self.work);
        let mut text = String::new();
        let mut steps = 0u64;
        for &def in needs {
            if by_name.contains_key(def.name().as_str()) {
                continue;
            }
            match def {
                Def::Marker(key) => {
                    let made = art(key, &mut steps).map_err(WriteError::Unsupported)?;
                    for _ in 0..4 {
                        self.work.charge_attribute(self.limits)?;
                    }
                    text.push_str(&element(key, &made, &pfx.draw, &pfx.svg, &pfx.declare));
                }
                Def::Fx(fx) => {
                    // Up to ten stops and a dozen attributes: charged before the element is made.
                    let attributes = match fx {
                        super::fx::Fx::Gradient(g) => 12usize.saturating_add(usize::from(g.n).saturating_mul(3)),
                        super::fx::Fx::Hatch(_) => 6,
                    };
                    for _ in 0..attributes {
                        self.work.charge_attribute(self.limits)?;
                    }
                    let made = fx.element(&pfx.draw, &pfx.svg, &pfx.declare);
                    if made.is_empty() {
                        return Err(WriteError::Unsupported("a gradient or a hatch whose definition could not be made"));
                    }
                    text.push_str(&made);
                }
            }
            wrote.insert(def);
        }
        self.work.marker_art_steps = self.work.marker_art_steps.saturating_add(steps);
        if text.is_empty() {
            return Ok((wrote, removed));
        }
        // Put in by `flush_common_styles`, with whatever else the save adds to `office:styles`.
        self.common_text.push_str(&text);
        Ok((wrote, removed))
    }
}

/// Reads back the markers a save names: each of `needs` is in `office:styles` of the part made, and each of `wrote`, the ones this save put there, once and
/// drawing the form it wrote (one the file already held is used as it is: its name is what makes it DiaCraft's); and none of `removed`, the ones the save took
/// out, is there.
pub(crate) fn check(
    part: &[u8],
    needs: &BTreeSet<Def>,
    wrote: &BTreeSet<Def>,
    removed: &BTreeSet<Def>,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let scan = scan_part(part, PartKind::Styles, limits, work).map_err(|e| WriteError::SelfCheck(format!("the part made does not scan: {e}")))?;
    // The markers the part holds, by the key their name spells, in one pass (a pass for each of up to 45 times 100 keys is that many passes of a list a part may make
    // long).
    work.order_steps = work.order_steps.saturating_add(scan.markers.len() as u64);
    let mut names: HashMap<String, Def> = HashMap::new();
    for count in [needs.len(), removed.len()] {
        names.try_reserve(count).map_err(|_| failed("there is not the memory to read the markers back"))?;
    }
    for &key in needs.iter().chain(removed) {
        names.insert(key.name(), key);
    }
    let mut held: HashMap<Def, Vec<&MarkerDef>> = HashMap::new();
    held.try_reserve(names.len()).map_err(|_| failed("there is not the memory to read the markers back"))?;
    for m in &scan.markers {
        if let Some(key) = names.get(&m.name) {
            held.entry(*key).or_default().push(m);
        }
    }
    for key in removed.iter().filter(|k| !needs.contains(k)) {
        if held.contains_key(key) {
            return Err(failed("a marker of DiaCraft's name that no style names is still in office:styles"));
        }
    }
    let mut steps = 0u64;
    for &key in needs {
        let found: &[&MarkerDef] = held.get(&key).map_or(&[], Vec::as_slice);
        if found.is_empty() {
            return Err(failed("a marker a style names is not in office:styles"));
        }
        if !wrote.contains(&key) {
            continue;
        }
        // A gradient or a hatch is made from its name, which is what the check found by.
        let Def::Marker(key) = key else {
            if found.len() > 1 {
                return Err(failed("a definition of DiaCraft's name is in office:styles twice"));
            }
            continue;
        };
        let made = art(key, &mut steps).map_err(failed)?;
        match found {
            [one] if one.view_box.as_deref() == Some(made.view_box.as_str()) && one.d.as_deref() == Some(made.d.as_str()) => {}
            [_] => return Err(failed("a marker of DiaCraft's name does not draw the form that was written")),
            _ => return Err(failed("a marker of DiaCraft's name is in office:styles twice")),
        }
    }
    work.marker_art_steps = work.marker_art_steps.saturating_add(steps);
    Ok(())
}

#[cfg(test)]
mod tests;
