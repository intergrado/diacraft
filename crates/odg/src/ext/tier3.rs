//! Tier 3: which of a shape's tier-2 items LibreOffice's edits have made stale, and their removal (the design's "Tier 3").
//!
//! The reader decodes the extension a shape's style carries ([`super::payload::decode`]), works out the fingerprints of the outputs the file's native markup has
//! now ([`Natives`]), and [`judge`] compares each item's fingerprint (`diacraft:fp-<item>`, [`super::fingerprint`]) with the native one: **an item whose outputs
//! changed is stale and is dropped, so that what the markup says stands (native outputs win)**; an item whose outputs are as they were stays, whatever its inputs
//! did (a move, a resize). The items without a fingerprint, and those of a key this build does not judge, stay as they always did.
//!
//! **A position is the markup's, and what the cells say of it is judged by computing it again**: the cells that carry a frame (the pin's place in the shape,
//! the size, the ends of a line) are tier 2 when the shape's pin is not its centre or a formula sets one of them, and they compute a position that the markup says too
//! (`svg:x`, `svg:y`, `svg:width`, `svg:height`). **The edge of an unturned shape is the markup's** (`PinX - LocPinX` is the left edge, the pin's place `LocPinX` being the
//! one carried): the reader gives the pin the markup's edge plus the carried place, so that a move in LibreOffice is kept and the pin stays where it was in the shape, and
//! the pin's position, which the markup cannot say beside the frame, is never carried by a new shape. A carried size, or end of a line, that is not the markup's is stale
//! (the shape was resized): it is taken out, so that the native frame stands. This needs no fingerprint (the frame cells compare with the native frame directly, within
//! LibreOffice's grid of 0.01 mm, 0.0008 inch), so it works for a file DiaCraft wrote before tier 3 as well, and a save of the shape by DiaCraft, which writes the cells
//! and the frame together, keeps them as they were.
//!
//! **The work.** Each fingerprint is one look, and the native fingerprints are made once for a shape and only for the keys the shape's fingerprints name
//! ([`needs`]); the items of every stale key are taken out in one pass over each list, never one pass for each key (a shape may hold thousands of sections).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Cell, Cells, Section, SectionKey, Sheet, Value};

use super::fingerprint::{
    Fp, Key, OutlineFp, OutlineKind, cell_fp, is_custom_end, is_fingerprinted_cell, join_outline, name_fp, picture_fp, plain_of, text_fp, vsdx_fp,
};
use super::names::Gone;
use super::payload::Tier2;

/// The `Geometry` sections are the shape's outline; every other section has no native output.
const OUTLINE_SECTION: &str = "Geometry";

/// The sections that hold the rows a text's markers name.
const TEXT_ROWS: [&str; 3] = Tier2::TEXT_ROWS;

/// What the fingerprints of a tier 2 will be compared with: which natives to make.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    pub outline: bool,
    pub text: bool,
    /// The fingerprint of the text's formatting (`fp-format`).
    pub format: bool,
    /// The fingerprint of the picture the foreign data sits beside (`fp-foreign`).
    pub foreign: bool,
    pub name: bool,
    /// The cells whose native form is wanted.
    pub cells: Vec<String>,
}

impl Natives {
    /// Whether these are no more than the fingerprint of `draw:name`.
    pub(crate) fn is_name_only(&self) -> bool {
        self.outline.is_none() && self.text.is_none() && self.format.is_none() && self.foreign.is_none() && self.cells.is_empty()
    }
}

impl Needs {
    /// How many natives are asked for: the work of making them.
    pub(crate) fn count(&self) -> usize {
        usize::from(self.outline)
            + usize::from(self.text)
            + usize::from(self.format)
            + usize::from(self.foreign)
            + usize::from(self.name)
            + self.cells.len()
    }
}

/// The fingerprints of the outputs the file's markup has now, made once for a shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Natives {
    /// The outline: `None` when the shape's element has none (a rectangle, a line, a connector): the outline an item rests on is then gone.
    pub outline: Option<OutlineFp>,
    pub text: Option<Fp>,
    /// The formatting of the text as the markup says it (`fingerprint::format_fp`).
    pub format: Option<Fp>,
    /// The picture the markup shows (`fingerprint::picture_fp`): `Some` whenever it was asked for.
    pub foreign: Option<Fp>,
    pub name: Option<Fp>,
    pub cells: HashMap<String, Fp>,
}

/// What a tier 2 holds, indexed once for a pass over its fingerprints: a shape may hold thousands of sections, each with a fingerprint, and a search of the sections
/// for each fingerprint would be a pass over them for each (the review found that pattern three times). The sections are by their keys (the first of a key
/// is the one a search finds), the removals of sections and cells are sets, and the flags say what else is held.
struct Held<'a> {
    t: &'a Tier2,
    text_removed: bool,
    name_removed: bool,
    name_u_removed: bool,
    sections: HashMap<(&'a str, Option<u32>), &'a Arc<Section>>,
    sections_removed: HashSet<(&'a str, Option<u32>)>,
    cells: &'a Cells,
    cells_removed: HashSet<&'a str>,
}

impl<'a> Held<'a> {
    fn of(t: &'a Tier2) -> Held<'a> {
        let mut h = Held {
            t,
            text_removed: false,
            name_removed: false,
            name_u_removed: false,
            sections: HashMap::with_capacity(t.sections.len()),
            sections_removed: HashSet::new(),
            cells: &t.cells,
            cells_removed: HashSet::new(),
        };
        for s in &t.sections {
            h.sections.entry((s.name.as_str(), s.ix)).or_insert(s);
        }
        for g in &t.gone {
            match g {
                Gone::Text => h.text_removed = true,
                Gone::Name => h.name_removed = true,
                Gone::NameU => h.name_u_removed = true,
                Gone::Section(k) => {
                    h.sections_removed.insert((k.name.as_str(), k.ix));
                }
                Gone::Cell(n) => {
                    h.cells_removed.insert(n.as_str());
                }
            }
        }
        h
    }

    /// Whether the tier 2 holds the item `key` stands for: the item itself, or the removal of what the markup would give. **The formatting of the text is held by the text, and by the text's rows that
    /// ride without it**: a text the file says in its characters has its `Character`, `Paragraph` and `Field` rows carried when the markup cannot say them whole, and they
    /// were put over the markup's with no judge, so a bold turned on or off in LibreOffice was not seen (12 of 18 files of the VSDX direction).
    fn holds(&self, key: &Key) -> bool {
        match key {
            Key::Text => self.t.text.is_some() || self.text_removed,
            Key::Format => self.t.text.is_some() || self.text_removed || self.t.carries_text_rows(),
            Key::Name => self.t.name.is_some() || self.t.name_u.is_some() || self.name_removed || self.name_u_removed,
            Key::Master => self.t.master.is_some() || self.t.master_shape.is_some(),
            Key::Vsdx => self.t.vsdx.is_some(),
            Key::Foreign => self.t.foreign.is_some(),
            Key::Section(k) => self.sections.contains_key(&(k.name.as_str(), k.ix)) || self.sections_removed.contains(&(k.name.as_str(), k.ix)),
            Key::Cell(name) => self.cells.contains_key(name.as_str()) || self.cells_removed.contains(name.as_str()),
        }
    }

    /// The keys of the items it holds that rest on native outputs, each once: the text, the names, the link to a master, each `Geometry` section (carried or
    /// removed), and each cell that is fingerprinted.
    fn keys(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        for key in [Key::Text, Key::Format, Key::Name, Key::Master, Key::Vsdx, Key::Foreign] {
            if self.holds(&key) {
                keys.push(key);
            }
        }
        let mut seen: HashSet<Option<u32>> = HashSet::new();
        let geometry = self.sections.keys().chain(self.sections_removed.iter()).filter(|(name, _)| *name == OUTLINE_SECTION);
        for (_, ix) in geometry {
            if seen.insert(*ix) {
                keys.push(Key::Section(SectionKey::new(OUTLINE_SECTION, *ix)));
            }
        }
        let mut cells: HashSet<&str> = HashSet::new();
        for name in self.cells.keys().map(diacraft_doc::CellName::as_str).chain(self.cells_removed.iter().copied()) {
            if is_fingerprinted_cell(name) && cells.insert(name) {
                keys.push(Key::Cell(name.to_owned()));
            }
        }
        keys
    }

    /// The text's rows it carries and the ones it takes out, in one order: what the formatting of a text that rides as its rows is made of.
    fn text_rows(&self) -> Vec<(&'a str, Option<u32>, Option<&'a Section>)> {
        let mut rows: Vec<(&str, Option<u32>, Option<&Section>)> = self
            .sections
            .iter()
            .filter(|((name, _), _)| TEXT_ROWS.contains(name))
            .map(|((name, ix), section)| (*name, *ix, Some(Arc::as_ref(*section))))
            .chain(self.sections_removed.iter().filter(|(name, _)| TEXT_ROWS.contains(name)).map(|(name, ix)| (*name, *ix, None)))
            .collect();
        rows.sort_by(|a, b| (a.0, a.1, a.2.is_some()).cmp(&(b.0, b.1, b.2.is_some())));
        rows
    }

    /// Whether the item `key` stands for is not what `was` held.
    fn changed_from(&self, was: &Held<'_>, key: &Key) -> bool {
        let (t, w) = (self.t, was.t);
        match key {
            // The parts only: the `.odg` data in a text's slot is no part of what tier 2 carries.
            Key::Text => t.text.as_ref().map(|x| &x.parts) != w.text.as_ref().map(|x| &x.parts) || self.text_removed != was.text_removed,
            // The formatting is the text's and its rows': either changed is a change of it.
            Key::Format => {
                t.text.as_ref().map(|x| &x.parts) != w.text.as_ref().map(|x| &x.parts)
                    || self.text_removed != was.text_removed
                    || self.text_rows() != was.text_rows()
            }
            Key::Name => {
                t.name != w.name || t.name_u != w.name_u || self.name_removed != was.name_removed || self.name_u_removed != was.name_u_removed
            }
            Key::Master => t.master != w.master || t.master_shape != w.master_shape,
            Key::Vsdx => t.vsdx != w.vsdx,
            Key::Foreign => t.foreign != w.foreign,
            Key::Section(k) => {
                let at = (k.name.as_str(), k.ix);
                self.sections.get(&at).copied() != was.sections.get(&at).copied()
                    || self.sections_removed.contains(&at) != was.sections_removed.contains(&at)
            }
            Key::Cell(name) => {
                let n = name.as_str();
                self.cells.get(n) != was.cells.get(n) || self.cells_removed.contains(n) != was.cells_removed.contains(n)
            }
        }
    }
}

/// The kind of judgement a key is given.
enum Native {
    /// The fingerprint of the outputs now.
    Now(Fp),
    /// The outputs are gone (the element has no outline any more).
    Gone,
    /// This build does not judge the key.
    NotJudged,
}

fn native_of(key: &Key, natives: &Natives) -> Native {
    match key {
        Key::Text => natives.text.map_or(Native::NotJudged, Native::Now),
        Key::Format => natives.format.map_or(Native::NotJudged, Native::Now),
        Key::Vsdx => natives.name.map_or(Native::NotJudged, |name| Native::Now(vsdx_fp(name, natives.outline.map(|o| o.whole)))),
        Key::Foreign => natives.foreign.map_or(Native::NotJudged, Native::Now),
        Key::Name => natives.name.map_or(Native::NotJudged, Native::Now),
        Key::Master => outline_native(natives),
        Key::Section(k) if k.name.as_str() == OUTLINE_SECTION => outline_native(natives),
        Key::Section(_) => Native::NotJudged,
        Key::Cell(name) if is_fingerprinted_cell(name) => natives.cells.get(name).copied().map_or(Native::NotJudged, Native::Now),
        Key::Cell(_) => Native::NotJudged,
    }
}

/// The outline now: its fingerprint, or gone when the element has none (`natives` is made for the keys [`needs`] names, which asks for the outline when a key rests
/// on it).
fn outline_native(natives: &Natives) -> Native {
    match natives.outline {
        Some(parts) => Native::Now(parts.whole),
        None => Native::Gone,
    }
}

/// The natives a tier 2 needs: those of every item it holds that rests on a native output, whether its fingerprint is there or not (the fingerprints present are judged
/// by them, and the natives themselves are kept for the writer, which stamps the fingerprints that are missing from them: `restamp`). A key this build does not judge
/// asks for nothing.
pub(crate) fn needs(t: &Tier2) -> Needs {
    let mut n = Needs::default();
    for key in Held::of(t).keys() {
        match &key {
            Key::Text => n.text = true,
            Key::Format => n.format = true,
            Key::Vsdx => {
                n.name = true;
                n.outline = true;
            }
            Key::Foreign => n.foreign = true,
            Key::Name => n.name = true,
            Key::Master => n.outline = true,
            Key::Section(k) if k.name.as_str() == OUTLINE_SECTION => n.outline = true,
            Key::Section(_) => {}
            Key::Cell(name) if is_fingerprinted_cell(name) => n.cells.push(name.clone()),
            Key::Cell(_) => {}
        }
    }
    n
}

/// How far a cell's value may be from the markup's frame before the frame is taken to have changed: LibreOffice keeps lengths on a grid of 0.01 mm, which is
/// 0.0004 inch, and two roundings may add.
const FRAME_TOLERANCE: f64 = 0.0008;

/// The pin and its place in the frame, by axis, and the size of the frame along it: the cells that put the left edge (or the bottom edge) of an unturned shape at
/// `PinX - LocPinX` (`PinY - LocPinY`).
const PIN_CELLS: [(&str, &str, &str); 2] = [("PinX", "LocPinX", "Width"), ("PinY", "LocPinY", "Height")];

/// The size of a two-dimensional frame: each cell is compared with the markup's own by itself.
const SIZE_CELLS: [&str; 2] = ["Width", "Height"];

/// The ends of a line or a connector, which are its frame (`svg:x1` to `svg:y2`): each is compared with the markup's by itself. A line's `Width` is the distance of its
/// ends, which the markup says by the ends and no other attribute, so it is not compared (a model whose `Width` is not that distance is a model that disagrees with
/// itself: the markup has the ends).
const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

fn number(cell: Option<&Cell>) -> Option<f64> {
    cell.and_then(|c| c.value.as_ref()).and_then(Value::number).filter(|n| n.is_finite())
}

/// How far a carried turn may be from the markup's before it is taken to say another turn: LibreOffice writes an angle on a grid of 0.01 degree (0.5 radian came back
/// as 0.50004), which is 1.745e-4 radian.
pub(crate) const TURN_TOLERANCE: f64 = 1.75e-4;

/// Whether a carried turn (`Angle`, `FlipX` or `FlipY`: its value) is the markup's within [`TURN_TOLERANCE`]. A value that is no number is not.
pub(crate) fn turn_close(carried: Option<f64>, markup: f64) -> bool {
    carried.is_some_and(|v| (v - markup).abs() <= TURN_TOLERANCE)
}

/// Whether the markup turns or flips the shape: its pin is then derived by the map of the turn ([`derive_turned`]), not by the edge alone.
fn markup_turned(native: &Sheet) -> bool {
    ["Angle", "FlipX", "FlipY"].iter().any(|n| native.cells.get(*n).is_some_and(|c| number(Some(c)).is_none_or(|v| v != 0.0)))
}

/// Whether the style carries a turn or a flip that says another than the markup's. The pin is not worked out then (the carried cell is a turn of its own, and nothing
/// says which of the two is the shape's); a carried turn that is the markup's (the cell holds a formula, say `30 deg`, that the markup's number comes to) is no reason.
fn carried_turn_disagrees(t: &Tier2, native: &Sheet) -> bool {
    ["Angle", "FlipX", "FlipY"].iter().any(|n| {
        let Some(carried) = t.cells.get(*n) else { return false };
        !turn_close(number(Some(carried)), number(native.cells.get(*n)).unwrap_or(0.0))
    })
}

/// The map gives a vector of a shape's own frame as the page sees it: the flips first (across the shape's own axes), then the turn counterclockwise.
pub(crate) fn map_of(angle: f64, flip: (bool, bool)) -> impl Fn(f64, f64) -> (f64, f64) {
    let (sin, cos) = angle.sin_cos();
    move |u: f64, v: f64| {
        let (u, v) = (if flip.0 { -u } else { u }, if flip.1 { -v } else { v });
        (cos * u - sin * v, sin * u + cos * v)
    }
}

/// The carried frame cells that the markup's frame says are stale: a size (or, for a line, an end) that is not the markup's. `native` is the sheet the markup gave,
/// with the frame cells as the reader rebuilds them. A line's frame is its ends and its `Width` is not compared (see [`END_CELLS`]).
pub(crate) fn stale_pose(t: &Tier2, native: &Sheet) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let one_d = native.cells.contains_key("BeginX");
    for name in if one_d { END_CELLS.as_slice() } else { SIZE_CELLS.as_slice() } {
        let name = *name;
        if let (Some(carried), Some(now)) = (number(t.cells.get(name)), number(native.cells.get(name)))
            && (carried - now).abs() > FRAME_TOLERANCE
        {
            out.push(name);
        }
    }
    out
}

/// What a pin's place is worth in the markup's frame: the formula `Width*k` (or `Height*k`, or the plain size) worked out with the markup's size, since the value the
/// style cached was worked out with the size the shape had when it was written; any other formula, or a constant, is the value it holds. The writer predicts the reader
/// with this very function.
pub(crate) fn place_value(cell: &Cell, size: Option<f64>, size_name: &str) -> Option<f64> {
    if let (Some(text), Some(size)) = (cell.formula.text(), size) {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if text == size_name {
            return Some(size);
        }
        if let Some(k) = text.strip_prefix(size_name).and_then(|r| r.strip_prefix('*')).and_then(|k| k.parse::<f64>().ok()).filter(|k| k.is_finite())
        {
            return Some(k * size);
        }
    }
    number(Some(cell))
}

/// The pin's position of an unturned, unflipped two-dimensional shape that carries its pin's place: **the markup's edge plus the place the shape carries**. The markup
/// says the edge (`PinX - LocPinX` as the reader rebuilt them) and the cells say where the pin is in the shape: a move or a resize in LibreOffice changes the edge and
/// leaves the place, and the pin follows. A pin the style carries that does not put the edge where the markup has it (by the values it was written with) is stale and is
/// taken out first, then the pin is derived. A turned or flipped shape is derived by [`derive_turned`]; a line, a cell the model lacks, and a shape that carries neither
/// the pin nor its place are not touched. The (at most two) cells it inserts are covered by the charge of `judge_tier2` for the cells a style carries.
pub(crate) fn derive_pins(t: &mut Tier2, native: &Sheet) {
    if native.cells.contains_key("BeginX") {
        return;
    }
    if carried_turn_disagrees(t, native) {
        return;
    }
    if markup_turned(native) {
        derive_turned(t, native);
        return;
    }
    for (pin, place, size_name) in PIN_CELLS {
        if !t.cells.contains_key(pin) && !t.cells.contains_key(place) {
            continue;
        }
        // A cell the model lacks (said gone) is no number to put the edge by: the axis is not touched.
        if t.gone.iter().any(|g| matches!(g, Gone::Cell(name) if name == pin || name == place)) {
            continue;
        }
        let (Some(np), Some(nl)) = (number(native.cells.get(pin)), number(native.cells.get(place))) else { continue };
        let edge = np - nl;
        let size = number(native.cells.get(size_name));
        if let Some(carried_pin) = number(t.cells.get(pin)) {
            // The pin was written with the place it had then; the place is worth what the markup's size makes of it now (a width widened in another program, a place
            // `Width*0.25`: the cached value is of the old width).
            let place_now = t.cells.get(place).and_then(|c| place_value(c, size, size_name)).unwrap_or(nl);
            if (carried_pin - place_now - edge).abs() > FRAME_TOLERANCE {
                t.cells.remove(pin);
            }
        }
        if t.cells.contains_key(pin) {
            continue;
        }
        let derived = t.cells.get(place).and_then(|carried| place_value(carried, size, size_name)).map(|l| edge + l);
        if let Some(value) = derived.filter(|d| d.is_finite()).and_then(Value::from_number) {
            let mut cell = Cell::constant(value);
            cell.unit = native.cells.get(pin).and_then(|c| c.unit.clone());
            t.cells.insert(diacraft_doc::CellName::new(pin), cell);
        }
    }
}

/// [`derive_pins`] for a shape the markup turns or flips: the pin is the frame's centre less the map of the pin's place (the writer's: subtract the pin's place
/// in the frame, flip, turn counterclockwise, add the pin), so the pin the carried place gives is `centre - M (w/2 - LocPinX, h/2 - LocPinY)`, where `M` is the turn and the
/// flips the markup has. Both axes move together under a turn, so a carried place of either makes both pins derived. A turn or a flip the style carries that says another
/// than the markup's is not worked out ([`carried_turn_disagrees`]: the caller), and a markup without the whole frame is not touched. The cells it inserts are at most
/// two, which the charge of `judge_tier2` for the frame cells covers.
fn derive_turned(t: &mut Tier2, native: &Sheet) {
    let carried = |name: &str| t.cells.contains_key(name);
    if !(carried("PinX") || carried("PinY") || carried("LocPinX") || carried("LocPinY")) {
        return;
    }
    if t.gone.iter().any(|g| matches!(g, Gone::Cell(name) if ["PinX", "PinY", "LocPinX", "LocPinY"].contains(&name.as_str()))) {
        return;
    }
    let n = |name: &str| number(native.cells.get(name));
    let (Some(npx), Some(npy), Some(nlx), Some(nly), Some(w), Some(h)) = (n("PinX"), n("PinY"), n("LocPinX"), n("LocPinY"), n("Width"), n("Height"))
    else {
        return;
    };
    let apply = map_of(n("Angle").unwrap_or(0.0), (n("FlipX") == Some(1.0), n("FlipY") == Some(1.0)));
    let place = |name: &str, native_value: f64, size_name: &str, size: f64| {
        t.cells.get(name).and_then(|c| place_value(c, Some(size), size_name)).unwrap_or(native_value)
    };
    let (lx, ly) = (place("LocPinX", nlx, "Width", w), place("LocPinY", nly, "Height", h));
    let (cx, cy) = (npx + apply(w / 2.0 - nlx, h / 2.0 - nly).0, npy + apply(w / 2.0 - nlx, h / 2.0 - nly).1);
    let (ox, oy) = apply(w / 2.0 - lx, h / 2.0 - ly);
    let derived = [("PinX", cx - ox), ("PinY", cy - oy)];
    let places = carried("LocPinX") || carried("LocPinY");
    for (name, value) in derived {
        if let Some(held) = number(t.cells.get(name))
            && (held - value).abs() > FRAME_TOLERANCE
        {
            t.cells.remove(name);
        }
        if !places || t.cells.contains_key(name) {
            continue;
        }
        if let Some(value) = Some(value).filter(|v| v.is_finite()).and_then(Value::from_number) {
            let mut cell = Cell::constant(value);
            cell.unit = native.cells.get(name).and_then(|c| c.unit.clone());
            t.cells.insert(diacraft_doc::CellName::new(name), cell);
        }
    }
}

/// What a judgement found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// The keys whose items were stale and are taken out.
    pub stale: Vec<Key>,
    /// The fingerprints that were not 16 lowercase hexadecimal digits: read as none.
    pub unreadable: usize,
    /// The link to a master that went with the stale `Master` item: the `Master` and `MasterShape` it held.
    pub master_link: Option<(Option<u32>, Option<u32>)>,
}

/// Judges the fingerprints of `t` against `natives`, and the frame cells it carries against `sheet`, the sheet the markup gave, and takes out the items of every stale
/// key (see the module documentation). A frame cell that is stale is reported as the key of the cell.
pub(crate) fn judge(t: &mut Tier2, natives: &Natives, sheet: &Sheet) -> Verdict {
    let mut verdict = Verdict::default();
    {
        let held = Held::of(t);
        for (local, raw) in &t.fps {
            let Some(key) = Key::parse(local) else { continue };
            let Some(written) = Fp::parse(raw) else {
                verdict.unreadable += 1;
                continue;
            };
            if !held.holds(&key) {
                continue;
            }
            let stale = match native_of(&key, natives) {
                Native::Now(now) => now != written,
                Native::Gone => true,
                Native::NotJudged => false,
            };
            if stale {
                verdict.stale.push(key);
            }
        }
    }
    // The characters and their formatting are one item: one stale key says so (the formatting's goes when the characters' does).
    if verdict.stale.contains(&Key::Text) {
        verdict.stale.retain(|k| *k != Key::Format);
    }
    verdict.stale.extend(stale_pose(t, sheet).into_iter().map(|name| Key::Cell(name.to_owned())));
    if verdict.stale.is_empty() {
        derive_pins(t, sheet);
        return verdict;
    }
    if verdict.stale.contains(&Key::Master) {
        verdict.master_link = Some((t.master, t.master_shape));
    }
    take_out(t, &verdict.stale);
    derive_pins(t, sheet);
    verdict
}

/// Takes out every item the keys name, with the removals that name them and the fingerprints themselves: one pass over each list.
fn take_out(t: &mut Tier2, keys: &[Key]) {
    let mut text = false;
    let mut name = false;
    let (mut vsdx, mut foreign) = (false, false);
    let mut master = false;
    let mut sections: HashSet<(&str, Option<u32>)> = HashSet::new();
    let mut cells: HashSet<&str> = HashSet::new();
    for key in keys {
        match key {
            Key::Text | Key::Format => text = true,
            Key::Name => name = true,
            Key::Master => master = true,
            Key::Vsdx => vsdx = true,
            Key::Foreign => foreign = true,
            Key::Section(k) => {
                sections.insert((k.name.as_str(), k.ix));
            }
            Key::Cell(n) => {
                cells.insert(n.as_str());
            }
        }
    }
    if text {
        t.text = None;
    }
    if name {
        t.name = None;
        t.name_u = None;
    }
    if master {
        t.master = None;
        t.master_shape = None;
    }
    if vsdx {
        t.vsdx = None;
    }
    if foreign {
        t.foreign = None;
    }
    // **A text that is stale takes its rows with it**: the rows a text's markers name (`Character`, `Paragraph`, `Field`) are the model's numbering of what the text said, and put over the
    // sections the markup gave they would name other formatting than the markup's own text does.
    let text_row = |name: &str| text && TEXT_ROWS.contains(&name);
    t.sections.retain(|s| !sections.contains(&(s.name.as_str(), s.ix)) && !text_row(s.name.as_str()));
    t.cells.retain(|n, _| !cells.contains(n.as_str()));
    t.gone.retain(|g| match g {
        Gone::Text => !text,
        Gone::Name | Gone::NameU => !name,
        Gone::Section(k) => !sections.contains(&(k.name.as_str(), k.ix)) && !text_row(k.name.as_str()),
        Gone::Cell(n) => !cells.contains(n.as_str()),
    });
    let mut attrs: HashSet<String> = keys.iter().map(Key::attr).collect();
    // The two fingerprints of the carried text go with it, whichever of them was stale.
    if text {
        attrs.insert(Key::Text.attr());
        attrs.insert(Key::Format.attr());
    }
    t.fps.retain(|local, _| !attrs.contains(local));
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// The cells a style writes natively, and the display forms written beside the cells: what the fingerprints of the carried cells are made from.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Written<'a> {
    /// The style cells written natively, each read back as the value written.
    pub native: &'a [&'a str],
    /// The fingerprinted cells whose native output a display form says another value for (`native::Display::native`).
    pub display: &'a [(&'static str, Option<Cell>)],
    /// The cells that get no fingerprint, and are never judged stale (`native::Display::unjudged`).
    pub unjudged: &'a [&'static str],
}

/// Puts into the tier 2 of a **new shape** the fingerprint of the native outputs each of its items rests on, as the writer wrote them: the plain characters of the
/// text, the `draw:name`, the outline (the fingerprint of the geometry element written, when the element has one), and each fingerprinted cell as the reader
/// will rebuild it from the markup, which is absent, or number 4 for the custom line end the writer draws as number 4's marker.
pub(crate) fn stamp_new(
    t: &mut Tier2,
    text: &str,
    format: Option<Fp>,
    name: Option<&str>,
    outline: Option<Fp>,
    picture: Option<&[u8]>,
    cells: Written<'_>,
) {
    let (written, display, unjudged) = (cells.native, cells.display, cells.unjudged);
    let keys = Held::of(t).keys();
    for key in keys {
        let fp = match &key {
            Key::Text => Some(text_fp(text)),
            Key::Format => format,
            Key::Vsdx => Some(vsdx_fp(name_fp(name), outline)),
            Key::Foreign => Some(picture_fp(picture)),
            Key::Name => Some(name_fp(name)),
            Key::Master | Key::Section(_) => outline,
            Key::Cell(cell) if unjudged.contains(&cell.as_str()) => None,
            Key::Cell(cell) => t.cells.get(cell.as_str()).map(|c| {
                // The custom end is drawn as number 4's marker, which the reader rebuilds as the cell 4; a cell with a formula whose value the style says (`written`) is rebuilt as
                // that value; any other carried cell is written as no attribute.
                // A display form written beside the cell (a hatch for `FillPattern`) is read back as the cell it says, which is the native output the fingerprint stands for.
                let native = if is_custom_end(cell, c) {
                    Value::from_number(4.0).map(Cell::constant)
                } else if let Some((_, shown)) = display.iter().find(|(name, _)| *name == cell.as_str()) {
                    shown.clone()
                } else if written.contains(&cell.as_str()) {
                    c.value.clone().map(Cell::constant)
                } else {
                    None
                };
                cell_fp(cell, native.as_ref())
            }),
        };
        if let Some(fp) = fp {
            t.fps.insert(key.attr(), fp.text());
        }
    }
}

/// The sections an instance inherits from its master and keeps its own copies of when the link goes stale: the shape data (`Property`, which a formula spells `Prop.`)
/// and the user rows. Each is the model's name of the section, as the VSDX reader names it (SECNAME: by the formula's word, `Prop`, no section was ever found and the
/// shape data was lost).
pub(crate) const INHERITED_SECTIONS: [&str; 2] = [crate::sections::PROPERTY, crate::sections::USER];

/// Puts the model's own copies of the inherited sections into the tier 2 a style will carry, whole (each over the section of its key): the link to the master was stale
/// and is not written, and **the copies the reader made of what the master gave the shape are in no markup**, so that without this the data would be gone after the
/// save that took the link out.
pub(crate) fn keep_inherited(t: &mut Tier2, model: &diacraft_doc::Shape) {
    for name in INHERITED_SECTIONS {
        let Some(own) = model.sheet.sections.iter().find(|s| s.name.as_str() == name) else { continue };
        match t.sections.iter().position(|s| s.name.as_str() == name && s.ix.is_none()) {
            Some(at) => t.sections[at] = Arc::clone(own),
            None => t.sections.push(Arc::clone(own)),
        }
        t.gone.retain(|g| !matches!(g, Gone::Section(k) if k.name.as_str() == name && k.ix.is_none()));
    }
}

/// Takes the items tier 3 found stale when the file was read out of what a style carries, with their fingerprints, so that an edit of the shape does not write
/// them again as current items (`PageShapes::stale`).
pub(crate) fn drop_stale(t: &mut Tier2, stale: &[Key]) {
    if !stale.is_empty() {
        take_out(t, stale);
    }
}

/// What a save does to the native output the fingerprints of the outline stand for (the path, the modifiers, the geometry element's other parts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutlineNow {
    /// The save does not write the outline: the file's own is the output.
    File,
    /// The save writes the path of a custom shape's `draw:enhanced-path` and nothing else of the outline: this is the fingerprint of the path part it wrote; the
    /// other parts are the file's.
    Path(Fp),
    /// The save writes the outline in a way whose output is not known here (the modifiers, a path, a polygon's points).
    Unknown,
}

/// Makes the fingerprints a tier 2 carries follow an edit of the shape, and **stamps the ones it lacks**: `t` is what the style will carry after the edit, `was` what
/// it carried (decoded from the file), `natives` the native outputs of the file's markup that its items rest on, as the file had them when it was read (`None` when the
/// read did not make them), and `outline` what the save does to the outline.
///
/// A fingerprint says what the native output was when the item was written, so that an edit of the output in another program is told. **A fingerprint is therefore
/// taken out only where the save rewrites its output**, and is the output's own wherever the save leaves it as the file has it, whether the item changed or not (a
/// name given in DiaCraft is carried, `draw:name` is not rewritten, so the fingerprint is the file's own `draw:name`'s: a rename in another program then differs). Per
/// item: the **text** is made again from the characters the save writes (the model's) where the item changed, and is the file's own where it did not; the **name** is
/// the file's own `draw:name` (no save of a shape of a file rewrites it); the **link to a master** and the **`Geometry` sections** are the file's outline where the save
/// does not write it, the outline made again from the path the save wrote and the file's other parts where it writes a custom shape's path, and are taken out where it
/// writes the outline in another way; a **cell** is the file's own where the item did not change, and is taken out where it did (the save may rewrite the attribute,
/// and what it writes is not known here). An item the file's natives do not say (no natives, or an output the element has not) keeps the fingerprint it had if it did not
/// change, and has none otherwise: it is then not judged, as before tier 3, and never taken for stale on a guess. A fingerprint whose item is gone goes with it.
pub(crate) fn restamp(t: &mut Tier2, was: &Tier2, natives: Option<&Natives>, outline: OutlineNow) {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    {
        let (now, before) = (Held::of(t), Held::of(was));
        // A fingerprint this build does not know is a newer build's: kept as it is.
        for (local, raw) in &t.fps {
            if Key::parse(local).is_none() {
                out.insert(local.clone(), raw.clone());
            }
        }
        for key in now.keys() {
            let unchanged = before.holds(&key) && !now.changed_from(&before, &key);
            // The fingerprint the tier 2 holds, when it is one.
            let kept = t.fps.get(&key.attr()).filter(|raw| Fp::parse(raw).is_some()).cloned();
            let fp: Option<String> = match &key {
                Key::Text if unchanged => natives.and_then(|n| n.text).map(Fp::text).or(kept),
                Key::Text => Some(text_fp(&t.text.as_ref().map(plain_of).unwrap_or_default()).text()),
                // The formatting of a text the save writes is not known here (the retext keeps the file's own styles): taken out, and the characters judge the item, as before the ruling.
                Key::Format if unchanged => natives.and_then(|n| n.format).map(Fp::text).or(kept),
                Key::Format => None,
                // The VSDX data sits beside the name and the outline: the file's own where the save leaves the outline as it is; where the save writes it, it is not known here.
                Key::Vsdx => match outline {
                    OutlineNow::File => natives
                        .and_then(|n| n.name.map(|name| vsdx_fp(name, n.outline.map(|o| o.whole))))
                        .map(Fp::text)
                        .or(if unchanged { kept } else { None }),
                    _ => None,
                },
                Key::Foreign => natives.and_then(|n| n.foreign).map(Fp::text).or(if unchanged { kept } else { None }),
                Key::Name => natives.and_then(|n| n.name).map(Fp::text).or(if unchanged { kept } else { None }),
                Key::Master | Key::Section(_) => {
                    let file = natives.and_then(|n| n.outline);
                    match outline {
                        OutlineNow::File => file.map(|o| o.whole.text()).or(if unchanged { kept } else { None }),
                        OutlineNow::Path(path) => file
                            .filter(|o| o.kind == OutlineKind::Custom)
                            .map(|o| join_outline(OutlineKind::Custom, path, o.modifiers, o.rest).text()),
                        OutlineNow::Unknown => None,
                    }
                }
                Key::Cell(name) if unchanged => natives.and_then(|n| n.cells.get(name)).map(|fp| fp.text()).or(kept),
                Key::Cell(_) => None,
            };
            if let Some(fp) = fp {
                out.insert(key.attr(), fp);
            }
        }
    }
    t.fps = out;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Section, Text, TextPart, Value};

    use super::super::fingerprint::{cell_fp, name_fp, text_fp};
    use super::*;

    /// The key of a `Geometry` section `ix`.
    fn outline_key(ix: Option<u32>) -> Key {
        Key::Section(SectionKey::new(OUTLINE_SECTION, ix))
    }

    fn fp(n: u64) -> Fp {
        Fp::parse(&format!("{n:016x}")).unwrap()
    }

    /// An outline whose whole is `fp(n)` (its parts are others, so that a test of the whole cannot take a part for it).
    fn whole(n: u64) -> OutlineFp {
        OutlineFp { kind: super::super::fingerprint::OutlineKind::Custom, whole: fp(n), path: fp(n + 100), modifiers: fp(n + 200), rest: fp(n + 300) }
    }

    fn with(t: &mut Tier2, key: &Key, fp: Fp) {
        t.fps.insert(key.attr(), fp.text());
    }

    fn text() -> Text {
        Text { parts: vec![TextPart::Chars("hi".into())], ..Default::default() }
    }

    fn cell(n: f64) -> Cell {
        Cell::constant(Value::from_number(n).unwrap())
    }

    /// A tier 2 that holds one of each judged item, each with its fingerprint.
    fn full() -> (Tier2, Natives) {
        let mut t = Tier2 { name: Some("N".into()), master: Some(3), master_shape: Some(4), text: Some(text()), ..Tier2::default() };
        t.cells.insert(CellName::new("EndArrow"), cell(254.0));
        t.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("c")));
        t.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        t.sections.push(Arc::new(Section::new("User", None)));
        let natives = Natives {
            outline: Some(whole(1)),
            text: Some(text_fp("hi")),
            format: None,
            foreign: None,
            name: Some(name_fp(Some("n"))),
            cells: HashMap::from([("EndArrow".to_owned(), cell_fp("EndArrow", Some(&cell(4.0))))]),
        };
        with(&mut t, &Key::Text, natives.text.unwrap());
        with(&mut t, &Key::Name, natives.name.unwrap());
        with(&mut t, &Key::Master, fp(1));
        with(&mut t, &outline_key(Some(0)), fp(1));
        with(&mut t, &Key::Cell("EndArrow".to_owned()), natives.cells["EndArrow"]);
        (t, natives)
    }

    /// Outputs as they were: nothing is stale and nothing is taken out.
    #[test]
    fn an_item_whose_outputs_are_as_they_were_stays() {
        let (mut t, natives) = full();
        let before = t.clone();
        let v = judge(&mut t, &natives, &Sheet::default());
        assert_eq!(v, Verdict::default());
        assert_eq!(t, before);
    }

    /// Each kind of output changed makes its own item stale, and only it: the text, the names, the outline (the master link and the sections), and a cell.
    #[test]
    fn an_item_whose_outputs_changed_is_stale_and_taken_out_and_no_other_is() {
        for (what, key) in [("text", Key::Text), ("name", Key::Name), ("outline", Key::Master), ("cell", Key::Cell("EndArrow".to_owned()))] {
            let (mut t, mut natives) = full();
            match what {
                "text" => natives.text = Some(text_fp("hello")),
                "name" => natives.name = Some(name_fp(Some("renamed"))),
                "outline" => natives.outline = Some(whole(2)),
                _ => {
                    natives.cells.insert("EndArrow".to_owned(), cell_fp("EndArrow", Some(&cell(5.0))));
                }
            }
            let v = judge(&mut t, &natives, &Sheet::default());
            let stale: HashSet<Key> = v.stale.iter().cloned().collect();
            let want: HashSet<Key> =
                if what == "outline" { HashSet::from([Key::Master, outline_key(Some(0))]) } else { HashSet::from([key.clone()]) };
            assert_eq!(stale, want, "{what}");
            // The stale items are gone with their fingerprints; the others, and the items that have no output, are as they were.
            assert_eq!(t.text.is_some(), what != "text", "{what}");
            assert_eq!((t.name.is_some()), what != "name", "{what}");
            assert_eq!((t.master.is_some(), t.master_shape.is_some()), (what != "outline", what != "outline"), "{what}");
            assert_eq!(t.cells.contains_key("EndArrow"), what != "cell", "{what}");
            assert!(t.cells.contains_key("Comment"), "{what}: a cell with no native output is never stale");
            assert_eq!(t.sections.iter().any(|s| s.name.as_str() == "Geometry"), what != "outline", "{what}");
            assert!(t.sections.iter().any(|s| s.name.as_str() == "User"), "{what}: a section with no outline is never stale");
            assert_eq!(t.fps.len(), 5 - v.stale.len(), "{what}: a stale item's fingerprint goes with it");
        }
    }

    /// An outline the element no longer has (a rectangle where a custom shape was) is a change of the outline.
    #[test]
    fn an_outline_that_is_gone_makes_what_rests_on_it_stale() {
        let (mut t, mut natives) = full();
        natives.outline = None;
        let v = judge(&mut t, &natives, &Sheet::default());
        assert!(v.stale.contains(&Key::Master) && v.stale.contains(&outline_key(Some(0))), "{v:?}");
    }

    /// A removal is judged by its key like the item it removes: a removed text is stale when the text's outputs changed.
    #[test]
    fn a_removal_is_stale_with_the_outputs_it_removed_and_goes_with_it() {
        let mut t = Tier2 {
            gone: vec![Gone::Text, Gone::Name, Gone::Cell("EndArrow".to_owned()), Gone::Section(SectionKey::new("Geometry", Some(1)))],
            ..Tier2::default()
        };
        let natives = Natives {
            outline: Some(whole(9)),
            text: Some(text_fp("x")),
            format: None,
            foreign: None,
            name: Some(name_fp(None)),
            cells: HashMap::from([("EndArrow".to_owned(), fp(3))]),
        };
        with(&mut t, &Key::Text, fp(7));
        with(&mut t, &Key::Name, name_fp(None));
        with(&mut t, &Key::Cell("EndArrow".to_owned()), fp(3));
        with(&mut t, &outline_key(Some(1)), fp(8));
        let v = judge(&mut t, &natives, &Sheet::default());
        let stale: HashSet<Key> = v.stale.into_iter().collect();
        assert_eq!(stale, HashSet::from([Key::Text, outline_key(Some(1))]));
        assert_eq!(t.gone, vec![Gone::Name, Gone::Cell("EndArrow".to_owned())]);
    }

    /// An item that has no fingerprint is not judged, a fingerprint that is not one is read as none (and counted), a fingerprint of an item the tier 2 does not
    /// hold judges nothing, and a key this build does not judge (a section that is no outline, a cell that is no style cell) asks for nothing.
    #[test]
    fn what_is_not_fingerprinted_is_not_judged() {
        let mut t = Tier2 { text: Some(text()), master: Some(1), ..Tier2::default() };
        t.cells.insert(CellName::new("Comment"), cell(1.0));
        t.fps.insert("fp-text".to_owned(), "123".to_owned());
        t.fps.insert("fp-name".to_owned(), fp(5).text());
        t.fps.insert("fp-c-Comment".to_owned(), fp(5).text());
        t.fps.insert("fp-s-User".to_owned(), fp(5).text());
        t.fps.insert("fp-future".to_owned(), "zzz".to_owned());
        let natives = Natives {
            text: Some(fp(6)),
            format: None,
            foreign: None,
            name: Some(fp(6)),
            outline: Some(whole(6)),
            cells: HashMap::from([("Comment".to_owned(), fp(6))]),
        };
        // The natives are made for every item that rests on an output (the writer stamps the missing fingerprints from them); the judge reads only the readable ones.
        assert_eq!(
            needs(&t),
            Needs { outline: true, text: true, format: true, foreign: false, name: false, cells: Vec::new() },
            "the text and the link rest on outputs, the name has no item, and the cell and the section rest on none"
        );
        let v = judge(&mut t, &natives, &Sheet::default());
        assert_eq!(v, Verdict { stale: Vec::new(), unreadable: 1, master_link: None });
        assert!(t.text.is_some() && t.master == Some(1) && t.cells.contains_key("Comment"));
    }

    /// Natives are "the name alone" when the fingerprint of `draw:name` is all they hold.
    #[test]
    fn natives_that_hold_no_more_than_the_name_are_the_name_alone() {
        let name = Some(fp(1));
        assert!(Natives { name, ..Natives::default() }.is_name_only());
        assert!(Natives::default().is_name_only(), "none at all is no more than the name");
        assert!(!Natives { name, outline: Some(whole(1)), ..Natives::default() }.is_name_only());
        assert!(!Natives { name, text: Some(fp(2)), ..Natives::default() }.is_name_only());
        assert!(!Natives { name, cells: HashMap::from([("EndArrow".to_owned(), fp(3))]), ..Natives::default() }.is_name_only());
    }

    /// The natives a tier 2 needs are those of the items it holds that rest on an output, each once, whether they carry a fingerprint or not.
    #[test]
    fn the_natives_wanted_are_those_of_the_items_that_rest_on_an_output() {
        let (t, _) = full();
        assert_eq!(needs(&t), Needs { outline: true, text: true, format: true, foreign: false, name: true, cells: vec!["EndArrow".to_owned()] });
        let mut bare = t.clone();
        bare.fps.clear();
        assert_eq!(needs(&bare), needs(&t), "with no fingerprint at all");
        assert_eq!(needs(&Tier2::default()), Needs::default());
        assert_eq!(needs(&bare).count(), 5, "the outline, the characters, their formatting, the name and a cell");
        // A user row and a cell outside the list rest on none.
        let mut other = Tier2::default();
        other.cells.insert(CellName::new("Comment"), cell(1.0));
        other.sections.push(Arc::new(Section::new("User", None)));
        assert_eq!(needs(&other), Needs::default());
    }

    /// The master link that went is given back for the shape's own copies of what it inherited (`stale_master_link_keeps_shape_data`).
    #[test]
    fn the_link_a_stale_master_item_held_is_given_back() {
        let (mut t, mut natives) = full();
        natives.outline = Some(whole(2));
        let v = judge(&mut t, &natives, &Sheet::default());
        assert_eq!(v.master_link, Some((Some(3), Some(4))));
        let (mut t, mut natives) = full();
        natives.text = Some(text_fp("other"));
        assert_eq!(judge(&mut t, &natives, &Sheet::default()).master_link, None);
    }

    /// The items of thousands of stale keys are taken out in one pass over each list: the sections, the removals and the cells of a shape that holds 8,192 of
    /// each are judged and taken out whole.
    #[test]
    fn thousands_of_stale_keys_are_taken_out_whole() {
        let mut t = Tier2::default();
        for k in 0..8_192u32 {
            t.sections.push(Arc::new(Section::new("Geometry", Some(k))));
            t.gone.push(Gone::Section(SectionKey::new("Geometry", Some(k + 10_000))));
            with(&mut t, &outline_key(Some(k)), fp(1));
            with(&mut t, &outline_key(Some(k + 10_000)), fp(1));
        }
        let natives = Natives { outline: Some(whole(2)), ..Natives::default() };
        let v = judge(&mut t, &natives, &Sheet::default());
        assert_eq!(v.stale.len(), 16_384);
        assert!(t.sections.is_empty() && t.gone.is_empty() && t.fps.is_empty());
    }

    /// The fingerprints a new shape's tier 2 is given: each item that rests on native outputs gets the fingerprint of the outputs as written, the text of the plain
    /// characters, the name of the `draw:name`, the master link and the geometry that of the outline, a cell that of the cell the reader will rebuild (none, or 4 for
    /// the custom end), and an item that rests on no output gets none.
    #[test]
    fn a_new_shapes_items_are_given_the_fingerprints_of_the_outputs_as_written() {
        let (mut t, natives) = full();
        t.fps.clear();
        t.cells.insert(CellName::new("EndArrow"), cell(254.0));
        t.cells.insert(CellName::new("LinePattern"), cell(7.0));
        t.gone.push(Gone::Section(SectionKey::new("Geometry", Some(3))));
        stamp_new(&mut t, "hi", Some(fp(9)), Some("n"), Some(fp(1)), None, Written::default());
        assert_eq!(t.fps.get("fp-text"), Some(&text_fp("hi").text()));
        assert_eq!(t.fps.get("fp-format"), Some(&fp(9).text()), "the formatting of the markup written");
        assert_eq!(t.fps.get("fp-name"), Some(&name_fp(Some("n")).text()));
        assert_eq!(t.fps.get("fp-master"), Some(&fp(1).text()));
        assert_eq!(t.fps.get("fp-s-Geometry-0"), Some(&fp(1).text()));
        assert_eq!(t.fps.get("fp-s-Geometry-3"), Some(&fp(1).text()), "the removal of a geometry section rests on the outline too");
        assert_eq!(t.fps.get("fp-c-EndArrow"), natives.cells.get("EndArrow").map(|f| f.text()).as_ref(), "the custom end is number 4's marker");
        assert_eq!(t.fps.get("fp-c-LinePattern"), Some(&cell_fp("LinePattern", None).text()), "a carried cell is no attribute written");
        assert!(!t.fps.contains_key("fp-c-Comment") && !t.fps.contains_key("fp-s-User"), "{:?}", t.fps);
        assert_eq!(t.fps.len(), 8);
        // No outline written (a line, a connector): the outline's items are not fingerprinted, the others are.
        let (mut t, _) = full();
        t.fps.clear();
        stamp_new(&mut t, "hi", None, None, None, None, Written::default());
        assert!(!t.fps.contains_key("fp-format"), "a formatting that is not known is not stamped");
        assert!(!t.fps.contains_key("fp-master") && !t.fps.contains_key("fp-s-Geometry-0"));
        assert_eq!(t.fps.get("fp-name"), Some(&name_fp(None).text()));
        assert!(t.fps.contains_key("fp-text") && t.fps.contains_key("fp-c-EndArrow"));
    }

    /// An item that tier 3 found stale is taken out of what a style will carry, with its fingerprint, and a list of none takes out nothing.
    #[test]
    fn what_was_found_stale_is_taken_out_of_what_a_style_will_carry() {
        let (mut t, _) = full();
        let before = t.clone();
        drop_stale(&mut t, &[]);
        assert_eq!(t, before);
        drop_stale(&mut t, &[Key::Text, outline_key(Some(0))]);
        assert!(t.text.is_none() && t.sections.iter().all(|s| s.name.as_str() != "Geometry"));
        assert!(!t.fps.contains_key("fp-text") && !t.fps.contains_key("fp-s-Geometry-0") && t.fps.contains_key("fp-master"));
        assert!(t.name.is_some() && t.master.is_some());
    }

    /// The fingerprints follow an edit, with no natives to say what the file's outputs are: one whose item is as it was stays (its text kept exactly), one whose text
    /// changed is made again from the characters the save wrote, and one whose item changed (an output that may be rewritten, or not known) is taken out; a save that
    /// writes the outline in a way not known takes out the outline's.
    #[test]
    fn the_fingerprints_follow_an_edit_of_what_they_stand_for() {
        let (was, _) = full();
        // As it was.
        let mut t = was.clone();
        restamp(&mut t, &was, None, OutlineNow::File);
        assert_eq!(t.fps, was.fps);
        // The text changed: the fingerprint is made again from the characters.
        let mut t = was.clone();
        t.text = Some(Text { parts: vec![TextPart::Cp(0), TextPart::Chars("bye".into())], ..Default::default() });
        restamp(&mut t, &was, None, OutlineNow::File);
        assert_eq!(t.fps.get("fp-text"), Some(&text_fp("bye").text()));
        assert_eq!(t.fps.get("fp-name"), was.fps.get("fp-name"));
        // The name, the link and a cell changed, and nothing says what the file's outputs are: the fingerprints are taken out and the others stay.
        let mut t = was.clone();
        t.name = Some("other".into());
        t.master = Some(9);
        t.cells.insert(CellName::new("EndArrow"), cell(5.0));
        restamp(&mut t, &was, None, OutlineNow::File);
        for gone in ["fp-name", "fp-master", "fp-c-EndArrow"] {
            assert!(!t.fps.contains_key(gone), "{gone}");
        }
        assert!(t.fps.contains_key("fp-text") && t.fps.contains_key("fp-s-Geometry-0"));
        // A geometry section changed, and a removal that appeared.
        let mut t = was.clone();
        let mut section = Section::new("Geometry", Some(0));
        section.cells.insert(CellName::new("NoFill"), cell(1.0));
        t.sections[0] = Arc::new(section);
        restamp(&mut t, &was, None, OutlineNow::File);
        assert!(!t.fps.contains_key("fp-s-Geometry-0") && t.fps.contains_key("fp-master"));
        let mut t = was.clone();
        t.gone.push(Gone::Text);
        restamp(&mut t, &was, None, OutlineNow::File);
        assert_eq!(t.fps.get("fp-text"), Some(&text_fp(&plain_of(was.text.as_ref().unwrap())).text()), "the removal of the text changed the item");
        // An item that went takes its fingerprint with it, and one that was not there has none to keep.
        let mut t = was.clone();
        t.text = None;
        t.cells.remove("EndArrow");
        restamp(&mut t, &was, None, OutlineNow::File);
        assert!(!t.fps.contains_key("fp-text") && !t.fps.contains_key("fp-c-EndArrow"));
        // The outline written in a way not known: the outline's fingerprints go, whatever the items did.
        let mut t = was.clone();
        restamp(&mut t, &was, None, OutlineNow::Unknown);
        assert!(!t.fps.contains_key("fp-master") && !t.fps.contains_key("fp-s-Geometry-0"));
        assert!(t.fps.contains_key("fp-text") && t.fps.contains_key("fp-name") && t.fps.contains_key("fp-c-EndArrow"));
        // The fingerprints of a key this build does not know are kept as they are, and a fingerprint of an item that is not held goes.
        let mut t = was.clone();
        t.fps.insert("fp-future".to_owned(), "zz".to_owned());
        t.fps.insert("fp-c-LinePattern".to_owned(), fp(3).text());
        restamp(&mut t, &was, None, OutlineNow::File);
        assert_eq!(t.fps.get("fp-future").map(String::as_str), Some("zz"));
        assert!(!t.fps.contains_key("fp-c-LinePattern"));
    }

    /// The file's natives: a fingerprint is the output's own wherever the save leaves the output as the file has it, and a missing one is stamped (the review's
    ///). A name given in DiaCraft is carried while `draw:name` is not rewritten, so the fingerprint is the file's own `draw:name`'s whether the name changed or
    /// not; the outline is the file's own unless the save writes it; a cell that did not change is the file's own and one that did is taken out.
    #[test]
    fn a_missing_fingerprint_is_stamped_from_the_files_own_outputs_and_an_unwritten_output_keeps_its_own() {
        let (with_fps, natives) = full();
        // Nothing carries a fingerprint (a file, or an item a DiaCraft edit added): every item that rests on an output is stamped from the file's own.
        let mut was = with_fps.clone();
        was.fps.clear();
        let mut t = was.clone();
        restamp(&mut t, &was, Some(&natives), OutlineNow::File);
        assert_eq!(t.fps, with_fps.fps, "stamped from the file's outputs, the item unchanged");
        // The name changed in the edit: the name output was not rewritten, so the fingerprint is the file's own `draw:name`'s (with or without one before).
        for before in [was.clone(), with_fps.clone()] {
            let mut t = before.clone();
            t.name = Some("renamed".into());
            restamp(&mut t, &before, Some(&natives), OutlineNow::File);
            assert_eq!(t.fps.get("fp-name"), Some(&natives.name.unwrap().text()));
        }
        // The text changed: made from the characters the save wrote, natives or none.
        let mut t = was.clone();
        t.text = Some(Text { parts: vec![TextPart::Chars("bye".into())], ..Default::default() });
        restamp(&mut t, &was, Some(&natives), OutlineNow::File);
        assert_eq!(t.fps.get("fp-text"), Some(&text_fp("bye").text()));
        // A cell that changed may be rewritten natively by the save: taken out, with natives or none. One that did not is the file's own.
        let mut t = was.clone();
        t.cells.insert(CellName::new("EndArrow"), cell(5.0));
        restamp(&mut t, &was, Some(&natives), OutlineNow::File);
        assert!(!t.fps.contains_key("fp-c-EndArrow"));
        // A fingerprint that is not 16 digits is none: the natives replace it, and without natives it goes.
        let mut t = with_fps.clone();
        t.fps.insert("fp-name".to_owned(), "short".to_owned());
        restamp(&mut t, &with_fps, Some(&natives), OutlineNow::File);
        assert_eq!(t.fps.get("fp-name"), Some(&natives.name.unwrap().text()));
        t.fps.insert("fp-name".to_owned(), "short".to_owned());
        restamp(&mut t, &with_fps, None, OutlineNow::File);
        assert!(!t.fps.contains_key("fp-name"), "an unreadable one with nothing to replace it is dropped");
        // An output the element has not (a rectangle has no outline): nothing is stamped for the items that rest on it.
        let mut t = was.clone();
        let rect = Natives { outline: None, ..natives.clone() };
        restamp(&mut t, &was, Some(&rect), OutlineNow::File);
        assert!(!t.fps.contains_key("fp-master") && !t.fps.contains_key("fp-s-Geometry-0"));
        assert!(t.fps.contains_key("fp-name"));
    }

    /// A save that writes a custom shape's path and nothing else of the outline makes the outline's fingerprint again from the new path and the file's other parts
    /// (the probe of the review: an outline edit in DiaCraft, then the path edited in LibreOffice); any other write of the outline, or another kind of element,
    /// takes it out.
    #[test]
    fn a_path_written_gives_the_outline_again_from_the_new_path_and_the_parts_left() {
        use super::super::fingerprint::{join_outline, path_part};
        let (was, natives) = full();
        let new_path = path_part("M 0 0 L 9 9 Z N");
        let parts = natives.outline.unwrap();
        let wanted = join_outline(OutlineKind::Custom, new_path, parts.modifiers, parts.rest).text();
        assert_ne!(wanted, parts.whole.text());
        let mut t = was.clone();
        restamp(&mut t, &was, Some(&natives), OutlineNow::Path(new_path));
        assert_eq!(t.fps.get("fp-master"), Some(&wanted));
        assert_eq!(t.fps.get("fp-s-Geometry-0"), Some(&wanted));
        // Not when the element is not a custom shape's, or when there are no natives.
        let other = Natives { outline: Some(OutlineFp { kind: OutlineKind::Path, ..parts }), ..natives.clone() };
        for (natives, what) in [(Some(&other), "a path element"), (None, "no natives")] {
            let mut t = was.clone();
            restamp(&mut t, &was, natives, OutlineNow::Path(new_path));
            assert!(!t.fps.contains_key("fp-master") && !t.fps.contains_key("fp-s-Geometry-0"), "{what}");
        }
        // The file's outline when the save does not write it, and none when it writes it in a way not known.
        let mut t = was.clone();
        restamp(&mut t, &was, Some(&natives), OutlineNow::File);
        assert_eq!(t.fps.get("fp-master"), Some(&parts.whole.text()));
        restamp(&mut t, &was, Some(&natives), OutlineNow::Unknown);
        assert!(!t.fps.contains_key("fp-master"));
    }

    /// The sections an instance inherits are matched by the model's names (`Property`, which a formula writes `Prop`), as the VSDX reader names them: a section matched by
    /// the formula's word is one no shape of a model has, and the shape data a stale link left the shape was never copied (SECNAME).
    #[test]
    fn the_inherited_sections_are_the_models_names_and_not_the_words_a_formula_uses() {
        assert_eq!(INHERITED_SECTIONS, [crate::sections::PROPERTY, crate::sections::USER]);
        assert!(INHERITED_SECTIONS.iter().all(|name| diacraft_doc::SECTION_NAMES.contains(name)), "{INHERITED_SECTIONS:?}");
        // A model as the VSDX reader builds it: the shape data of `Property` and a user row. Each is kept; a section of the formula's word is no shape data.
        let mut model = diacraft_doc::Shape::new(1);
        let sheet = Arc::make_mut(&mut model.sheet);
        for name in ["Property", "Prop", "User"] {
            sheet.sections.push(Arc::new(Section::new(name, None)));
        }
        let mut t = Tier2::default();
        keep_inherited(&mut t, &model);
        let names: Vec<&str> = t.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Property", "User"]);
    }

    /// A link that went stale leaves the shape its own copies of the sections it inherited, whole, over a section of the name the style carries and added otherwise.
    #[test]
    fn the_copies_a_stale_link_keeps_replace_the_sections_of_their_names() {
        let mut model = diacraft_doc::Shape::new(1);
        let sheet = Arc::make_mut(&mut model.sheet);
        for name in ["Property", "User", "Other"] {
            let mut section = Section::new(name, None);
            section.cells.insert(CellName::new("Mark"), cell(7.0));
            sheet.sections.push(Arc::new(section));
        }
        let mut t = Tier2::default();
        t.sections.push(Arc::new(Section::new("User", None)));
        t.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        t.gone.push(Gone::Section(SectionKey::new("Property", None)));
        t.gone.push(Gone::Section(SectionKey::new("Other", None)));
        keep_inherited(&mut t, &model);
        let names: Vec<&str> = t.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["User", "Geometry", "Property"], "User replaced where it stood, Property added, Other is not inherited");
        assert!(t.sections.iter().filter(|s| s.name.as_str() != "Geometry").all(|s| s.cells.contains_key("Mark")));
        assert_eq!(t.gone, vec![Gone::Section(SectionKey::new("Other", None))], "the removal of an inherited section no longer holds");
        // A model with neither adds nothing.
        let mut t = Tier2::default();
        keep_inherited(&mut t, &diacraft_doc::Shape::new(2));
        assert!(t.sections.is_empty());
    }

    /// The index of what a tier 2 holds tells the first section of a key, the removals and the cells, and each key once.
    #[test]
    fn what_a_tier_2_holds_is_indexed_once_and_each_key_is_listed_once() {
        let mut t = Tier2::default();
        t.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        t.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        t.gone.push(Gone::Section(SectionKey::new("Geometry", Some(0))));
        t.gone.push(Gone::Section(SectionKey::new("Geometry", Some(2))));
        t.gone.push(Gone::Cell("EndArrow".to_owned()));
        t.cells.insert(CellName::new("EndArrow"), cell(1.0));
        t.cells.insert(CellName::new("FillForegnd"), cell(1.0));
        t.cells.insert(CellName::new("Comment"), cell(1.0));
        let held = Held::of(&t);
        let mut keys = held.keys();
        keys.sort_by_key(Key::attr);
        assert_eq!(keys, vec![Key::Cell("EndArrow".to_owned()), Key::Cell("FillForegnd".to_owned()), outline_key(Some(0)), outline_key(Some(2))]);
        assert!(held.holds(&outline_key(Some(2))) && !held.holds(&outline_key(Some(1))) && !held.holds(&Key::Text) && !held.holds(&Key::Master));
        assert!(held.holds(&Key::Cell("Comment".to_owned())) && !held.holds(&Key::Cell("Nothing".to_owned())));
    }

    /// The names are held when any one of the four things that stand for them is: the name, the universal name, and the removal of either. Each alone is enough, and none
    /// is held when none is there (and the same for the link to a master, whose two numbers each stand for it).
    #[test]
    fn the_names_and_the_link_are_held_when_any_one_of_what_stands_for_them_is() {
        let held = |t: &Tier2, key: &Key| Held::of(t).holds(key);
        assert!(!held(&Tier2::default(), &Key::Name));
        assert!(held(&Tier2 { name: Some("a".into()), ..Tier2::default() }, &Key::Name));
        assert!(held(&Tier2 { name_u: Some("a".into()), ..Tier2::default() }, &Key::Name));
        assert!(held(&Tier2 { gone: vec![Gone::Name], ..Tier2::default() }, &Key::Name));
        assert!(held(&Tier2 { gone: vec![Gone::NameU], ..Tier2::default() }, &Key::Name));
        assert!(!held(&Tier2 { gone: vec![Gone::Text], ..Tier2::default() }, &Key::Name));
        assert!(!held(&Tier2::default(), &Key::Master));
        assert!(held(&Tier2 { master: Some(1), ..Tier2::default() }, &Key::Master));
        assert!(held(&Tier2 { master_shape: Some(1), ..Tier2::default() }, &Key::Master));
        assert!(held(&Tier2 { text: Some(text()), ..Tier2::default() }, &Key::Text));
        assert!(held(&Tier2 { gone: vec![Gone::Text], ..Tier2::default() }, &Key::Text));
        assert!(!held(&Tier2 { name: Some("a".into()), ..Tier2::default() }, &Key::Text));
    }

    fn sheet_of(cells: &[(&str, f64)]) -> Sheet {
        let mut sheet = Sheet::default();
        for (name, v) in cells {
            sheet.cells.insert(CellName::new(name), cell(*v));
        }
        sheet
    }

    fn carried(cells: &[(&str, f64)]) -> Tier2 {
        let mut t = Tier2::default();
        for (name, v) in cells {
            t.cells.insert(CellName::new(name), cell(*v));
        }
        t
    }

    /// The pin of a shape that carries its pin's place is the markup's edge plus the place (the cached value worked out again with the markup's size when the place is a
    /// formula of it), a carried pin that does not put the edge where the markup has it is taken out first, and a pin that does is kept.
    #[test]
    fn the_pin_of_a_shape_that_carries_its_place_is_the_markups_edge_plus_the_place() {
        // The markup: a shape 4 wide whose centre is at 9 and whose left edge is at 7, and 2 high whose centre is at 5 (bottom edge 4); the rebuilt pin is the centre.
        let native = sheet_of(&[("PinX", 9.0), ("LocPinX", 2.0), ("PinY", 5.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        let pinx = |t: &Tier2| number(t.cells.get("PinX"));
        let derive = |mut t: Tier2, native: &Sheet| {
            derive_pins(&mut t, native);
            t
        };
        // A place of 0 carried: the pin is the left edge. A formula of the width is worked out with the markup's width, whatever value it cached.
        assert_eq!(pinx(&derive(carried(&[("LocPinX", 0.0)]), &native)), Some(7.0));
        let quarter = || {
            let mut t = Tier2::default();
            t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.25", Value::from_number(9.0)));
            t
        };
        assert_eq!(pinx(&derive(quarter(), &native)), Some(8.0));
        let spaced = {
            let mut t = Tier2::default();
            t.cells.insert(CellName::new("LocPinX"), Cell::with_formula(" Width * 1 ", Value::from_number(9.0)));
            t
        };
        assert_eq!(pinx(&derive(spaced, &native)), Some(11.0));
        let plain = {
            let mut t = Tier2::default();
            t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width", Value::from_number(9.0)));
            t
        };
        assert_eq!(pinx(&derive(plain, &native)), Some(11.0));
        // Another formula, or one of another axis, is the value it holds.
        let other = {
            let mut t = Tier2::default();
            t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Height*0.5", Value::from_number(0.5)));
            t
        };
        assert_eq!(pinx(&derive(other, &native)), Some(7.5));
        // The pin's place is the markup's when it is not carried: nothing is derived from a pin alone.
        assert_eq!(pinx(&derive(Tier2::default(), &native)), None);
        // The y axis is its own: the bottom edge is 4, and a place of 0.5 puts the pin at 4.5.
        let y = derive(carried(&[("LocPinY", 0.5)]), &native);
        assert_eq!(number(y.cells.get("PinY")), Some(4.5));
        assert_eq!(pinx(&y), None, "the x axis is not touched by a carried y place");
        // The unit is the markup's pin's.
        let mut with_unit = native.clone();
        with_unit.cells.get_mut("PinX").unwrap().unit = Some("IN".into());
        assert_eq!(derive(carried(&[("LocPinX", 0.0)]), &with_unit).cells.get("PinX").and_then(|c| c.unit.as_deref()), Some("IN"));
        // A carried pin that puts the edge where the markup has it (7 less the place 0) is kept as it is, and one that does not is taken out and derived again: the shape
        // was moved 3 further, so the edge is at 10.
        assert_eq!(pinx(&derive(carried(&[("PinX", 7.0), ("LocPinX", 0.0)]), &native)), Some(7.0));
        let moved = sheet_of(&[("PinX", 12.0), ("LocPinX", 2.0), ("PinY", 5.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        assert_eq!(pinx(&derive(carried(&[("PinX", 7.0), ("LocPinX", 0.0)]), &moved)), Some(10.0));
        // Within LibreOffice's grid (0.0004 inch, two roundings) the carried pin is the markup's; past it, it is not.
        assert_eq!(pinx(&derive(carried(&[("PinX", 7.0007), ("LocPinX", 0.0)]), &native)), Some(7.0007));
        assert_eq!(pinx(&derive(carried(&[("PinX", 7.0009), ("LocPinX", 0.0)]), &native)), Some(7.0));
        assert_eq!(pinx(&derive(carried(&[("PinX", 6.9991), ("LocPinX", 0.0)]), &native)), Some(7.0));
        // A pin carried with no place is the pin the markup's centre gives: it stays when it is the markup's and goes (the markup's stands) when it is not.
        assert_eq!(pinx(&derive(carried(&[("PinX", 9.0)]), &native)), Some(9.0));
        assert_eq!(pinx(&derive(carried(&[("PinX", 7.0)]), &native)), None);
    }

    /// A turn the style carries, a line, a cell the model lacks, and a shape that carries neither the pin nor its place are not touched.
    #[test]
    fn a_turn_a_line_a_gone_cell_or_nothing_carried_leaves_the_pin_alone() {
        let native = sheet_of(&[("PinX", 9.0), ("LocPinX", 2.0), ("PinY", 5.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        let untouched = |t: Tier2, native: &Sheet| {
            let before = t.cells.clone();
            let mut t = t;
            derive_pins(&mut t, native);
            t.cells == before
        };
        assert!(untouched(Tier2::default(), &native));
        assert!(untouched(carried(&[("Comment", 1.0)]), &native));
        for turn in ["Angle", "FlipX", "FlipY"] {
            // A turn or a flip the style carries is not worked out; one the markup has is (the pin is derived by the map of the turn); a turn of 0 is none.
            let mut held = carried(&[("LocPinX", 0.0)]);
            held.cells.insert(CellName::new(turn), cell(1.0));
            assert!(untouched(held, &native), "{turn} carried");
            let mut turned = native.clone();
            turned.cells.insert(CellName::new(turn), cell(1.0));
            assert!(!untouched(carried(&[("LocPinX", 0.0)]), &turned), "{turn} in the markup is worked out");
            let mut zero = carried(&[("LocPinX", 0.0)]);
            zero.cells.insert(CellName::new(turn), cell(0.0));
            assert!(!untouched(zero, &native), "{turn} of 0 is no turn: the pin is derived as the unturned shape's is");
        }
        let line = sheet_of(&[("BeginX", 1.0), ("EndX", 3.0), ("PinX", 2.0), ("LocPinX", 1.0)]);
        assert!(untouched(carried(&[("LocPinX", 0.0)]), &line), "a line's frame is its ends");
        let mut gone = carried(&[("LocPinX", 0.0)]);
        gone.gone.push(Gone::Cell("PinX".to_owned()));
        assert!(untouched(gone, &native), "a pin the model lacks");
        let mut gone = carried(&[("LocPinX", 0.0)]);
        gone.gone.push(Gone::Cell("LocPinY".to_owned()));
        assert!(!untouched(gone, &native), "the other axis's cell is no reason");
        // A place that is no number and a markup with no pin are not worked out.
        let mut text = Tier2::default();
        text.cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("left")));
        assert!(untouched(text, &native));
        assert!(untouched(carried(&[("LocPinX", 0.0)]), &sheet_of(&[("Width", 4.0)])));
    }

    /// A carried size and the ends of a line are compared with the markup's by themselves; a line's `Width` is not compared.
    #[test]
    fn a_carried_size_and_ends_are_judged_by_themselves() {
        let native = sheet_of(&[("PinX", 9.0), ("LocPinX", 2.0), ("Width", 4.0), ("Height", 2.0)]);
        assert!(stale_pose(&carried(&[("Width", 4.0004), ("Height", 2.0)]), &native).is_empty());
        assert_eq!(stale_pose(&carried(&[("Width", 5.0), ("Height", 1.0)]), &native), ["Width", "Height"]);
        // The ends of a line are its frame (the markup says them by `svg:x1` to `svg:y2`), and its `Width` is their distance, which the markup has no attribute for: a
        // carried `Width` is not compared, an end is. A model whose `Width` is not the distance of its ends disagrees with itself (the fuzz of the writer found it).
        let line = sheet_of(&[("BeginX", 1.0), ("EndX", 3.0), ("Width", 2.0), ("PinX", 2.0), ("LocPinX", 1.0)]);
        assert!(stale_pose(&carried(&[("Width", 9.0), ("Height", 5.0), ("BeginX", 1.0), ("EndX", 3.0), ("PinX", 7.0)]), &line).is_empty());
        assert_eq!(stale_pose(&carried(&[("Width", 9.0), ("BeginX", 1.5), ("EndX", 3.0), ("EndY", 4.0)]), &line), ["BeginX"]);
        // A cell the markup does not have, or a carried one that is no number, is not compared; the pin is not a size.
        assert!(stale_pose(&carried(&[("BeginY", 9.0)]), &line).is_empty());
        assert!(stale_pose(&carried(&[("Depth", 9.0), ("PinX", 1.0), ("LocPinX", 1.0)]), &native).is_empty());
        let mut text = Tier2::default();
        text.cells.insert(CellName::new("Width"), Cell::constant(Value::from_text("wide")));
        assert!(stale_pose(&text, &native).is_empty());
    }

    /// The stale size is reported as the key of its cell and taken out like any cell, and the pin is derived from the place the style carries.
    #[test]
    fn a_stale_size_is_taken_out_by_the_judge_like_any_cell_and_the_pin_is_derived() {
        let mut t = carried(&[("LocPinX", 0.0), ("Width", 5.0), ("Comment", 1.0)]);
        let native = sheet_of(&[("PinX", 12.0), ("LocPinX", 2.0), ("Width", 4.0)]);
        let v = judge(&mut t, &Natives::default(), &native);
        assert_eq!(v.stale, vec![Key::Cell("Width".to_owned())]);
        let kept: Vec<&str> = t.cells.keys().map(CellName::as_str).collect();
        assert_eq!(kept, ["Comment", "LocPinX", "PinX"], "the pin is derived, the stale width is gone");
        assert_eq!(number(t.cells.get("PinX")), Some(10.0), "the edge 10 and the place 0");
        // Nothing stale: the pin is derived all the same.
        let mut t = carried(&[("LocPinX", 0.0)]);
        assert!(judge(&mut t, &Natives::default(), &native).stale.is_empty());
        assert_eq!(number(t.cells.get("PinX")), Some(10.0));
    }

    /// Whether an item is not what it was is told by each thing that stands for it: a text, the names (the name, the universal name, and the removal of either), the link
    /// (each of its two numbers), the section of a key (the first of it, and its removal) and a cell (and its removal).
    #[test]
    fn an_item_is_changed_when_any_one_of_what_stands_for_it_changed() {
        let changed = |now: &Tier2, was: &Tier2, key: &Key| Held::of(now).changed_from(&Held::of(was), key);
        let base = || Tier2 {
            name: Some("a".into()),
            name_u: Some("u".into()),
            master: Some(1),
            master_shape: Some(2),
            text: Some(text()),
            ..Tier2::default()
        };
        // Nothing changed: no key is.
        let geometry = Key::Section(SectionKey::new("Geometry", Some(0)));
        let cell_key = Key::Cell("EndArrow".to_owned());
        for key in [Key::Text, Key::Name, Key::Master, geometry.clone(), cell_key.clone()] {
            assert!(!changed(&base(), &base(), &key), "{key:?}");
        }
        // The text.
        let mut t = base();
        t.text = Some(Text { parts: vec![TextPart::Chars("other".into())], ..Default::default() });
        assert!(changed(&t, &base(), &Key::Text));
        let mut t = base();
        t.gone.push(Gone::Text);
        assert!(changed(&t, &base(), &Key::Text), "a removal of the text that appeared");
        assert!(changed(&base(), &t, &Key::Text), "and one that went");
        // Each of the four things that stand for the names, alone.
        let mut t = base();
        t.name = Some("b".into());
        assert!(changed(&t, &base(), &Key::Name) && !changed(&t, &base(), &Key::Master));
        let mut t = base();
        t.name_u = Some("v".into());
        assert!(changed(&t, &base(), &Key::Name));
        let mut t = base();
        t.gone.push(Gone::Name);
        assert!(changed(&t, &base(), &Key::Name));
        let mut t = base();
        t.gone.push(Gone::NameU);
        assert!(changed(&t, &base(), &Key::Name));
        // The two numbers of the link, alone.
        let mut t = base();
        t.master = Some(9);
        assert!(changed(&t, &base(), &Key::Master));
        let mut t = base();
        t.master_shape = Some(9);
        assert!(changed(&t, &base(), &Key::Master));
        // A section: replaced, put, taken out, and its removal appearing or going.
        let mut with = base();
        with.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        assert!(changed(&with, &base(), &geometry) && changed(&base(), &with, &geometry));
        let mut other = with.clone();
        let mut section = Section::new("Geometry", Some(0));
        section.cells.insert(CellName::new("NoFill"), cell(1.0));
        other.sections[0] = Arc::new(section);
        assert!(changed(&other, &with, &geometry));
        assert!(!changed(&with, &with.clone(), &geometry));
        let mut removed = base();
        removed.gone.push(Gone::Section(SectionKey::new("Geometry", Some(0))));
        assert!(changed(&removed, &base(), &geometry) && changed(&base(), &removed, &geometry));
        assert!(!changed(&removed, &removed.clone(), &geometry));
        // Another section's change is no change of this one.
        let mut elsewhere = base();
        elsewhere.sections.push(Arc::new(Section::new("Geometry", Some(1))));
        assert!(!changed(&elsewhere, &base(), &geometry));
        // A cell: put, changed, and its removal.
        let mut put = base();
        put.cells.insert(CellName::new("EndArrow"), cell(1.0));
        assert!(changed(&put, &base(), &cell_key) && changed(&base(), &put, &cell_key));
        let mut other = put.clone();
        other.cells.insert(CellName::new("EndArrow"), cell(2.0));
        assert!(changed(&other, &put, &cell_key));
        let mut removed = base();
        removed.gone.push(Gone::Cell("EndArrow".to_owned()));
        assert!(changed(&removed, &base(), &cell_key) && !changed(&removed, &removed.clone(), &cell_key));
        let mut elsewhere = base();
        elsewhere.cells.insert(CellName::new("Comment"), cell(1.0));
        assert!(!changed(&elsewhere, &base(), &cell_key));
    }

    /// The natives wanted by a fingerprint of a section are the outline's when the section is a `Geometry` one and none when it is another: a fingerprint of the user
    /// rows judges nothing and asks for nothing.
    #[test]
    fn a_fingerprint_of_a_section_that_is_no_outline_asks_for_no_native() {
        let mut t = Tier2::default();
        t.sections.push(Arc::new(Section::new("User", None)));
        t.fps.insert("fp-s-User".to_owned(), fp(1).text());
        assert_eq!(needs(&t), Needs::default());
        t.sections.push(Arc::new(Section::new("Geometry", Some(0))));
        t.fps.insert("fp-s-Geometry-0".to_owned(), fp(1).text());
        assert_eq!(needs(&t), Needs { outline: true, ..Needs::default() });
        // And the judge leaves the user rows alone whatever the outline did.
        let natives = Natives { outline: Some(whole(2)), ..Natives::default() };
        let v = judge(&mut t, &natives, &Sheet::default());
        assert_eq!(v.stale, vec![outline_key(Some(0))]);
        assert!(t.sections.iter().any(|s| s.name.as_str() == "User"));
    }

    /// A size is the markup's within the tolerance, and a difference of exactly the tolerance is within it; a pin whose edge is off by exactly the tolerance is the
    /// markup's too.
    #[test]
    fn the_tolerance_of_a_frame_is_inclusive() {
        let native = sheet_of(&[("Width", 0.0), ("Height", 1.0)]);
        assert!(stale_pose(&carried(&[("Width", FRAME_TOLERANCE)]), &native).is_empty(), "exactly the tolerance is the same");
        assert_eq!(stale_pose(&carried(&[("Width", FRAME_TOLERANCE * 1.01)]), &native), ["Width"]);
        let native = sheet_of(&[("PinX", 0.0), ("LocPinX", 0.0), ("Width", 2.0)]);
        let mut t = carried(&[("PinX", FRAME_TOLERANCE), ("LocPinX", 0.0)]);
        derive_pins(&mut t, &native);
        assert_eq!(number(t.cells.get("PinX")), Some(FRAME_TOLERANCE), "a pin off by exactly the tolerance is kept as it is");
        let mut t = carried(&[("PinX", FRAME_TOLERANCE * 1.01), ("LocPinX", 0.0)]);
        derive_pins(&mut t, &native);
        assert_eq!(number(t.cells.get("PinX")), Some(0.0), "and one past it is derived again");
        // The same for a shape the markup flips (a frame of no size, so that the sums are exact).
        let flipped = sheet_of(&[("PinX", 0.0), ("PinY", 0.0), ("LocPinX", 0.0), ("LocPinY", 0.0), ("Width", 0.0), ("Height", 0.0), ("FlipX", 1.0)]);
        let mut t = carried(&[("PinX", FRAME_TOLERANCE), ("LocPinX", 0.0)]);
        derive_pins(&mut t, &flipped);
        assert_eq!(number(t.cells.get("PinX")), Some(FRAME_TOLERANCE), "a flipped shape's pin off by exactly the tolerance is kept as it is");
        let mut t = carried(&[("PinX", FRAME_TOLERANCE * 1.01), ("LocPinX", 0.0)]);
        derive_pins(&mut t, &flipped);
        assert_eq!(number(t.cells.get("PinX")), Some(0.0), "and one past it is derived again");
    }

    /// The pin of a turned or flipped shape is the frame's centre less the turn and the flips of the pin's place (`centre - M (w/2 - LocPinX, h/2 - LocPinY)`), both
    /// axes together; a carried pin that is not that is taken out first.
    #[test]
    fn the_pin_of_a_turned_or_flipped_shape_is_the_centre_less_the_map_of_the_place() {
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        let frame = |cells: &[(&str, f64)]| {
            let mut v = vec![("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)];
            v.extend_from_slice(cells);
            sheet_of(&v)
        };
        let derive = |t: &Tier2, native: &Sheet| {
            let mut t = t.clone();
            derive_pins(&mut t, native);
            t
        };
        let quarter = std::f64::consts::FRAC_PI_2;
        // Turned a quarter turn counterclockwise, the pin at the left end of the middle line (w/2 = 2 along the turned axis, which points up the page): the pin is 2 below.
        let t = derive(&carried(&[("LocPinX", 0.0)]), &frame(&[("Angle", quarter)]));
        assert!(close(number(t.cells.get("PinX")), 9.0) && close(number(t.cells.get("PinY")), 3.0), "{:?}", t.cells);
        // The same with the place of the other axis (the pin at the bottom of the left edge: 2 along x and 1 along y).
        let t = derive(&carried(&[("LocPinY", 0.0)]), &frame(&[("Angle", quarter)]));
        assert!(close(number(t.cells.get("PinX")), 9.0 + 1.0) && close(number(t.cells.get("PinY")), 5.0), "{:?}", t.cells);
        // Flipped across x: the turned axis points the other way.
        let t = derive(&carried(&[("LocPinX", 0.0)]), &frame(&[("FlipX", 1.0)]));
        assert!(close(number(t.cells.get("PinX")), 11.0) && close(number(t.cells.get("PinY")), 5.0), "{:?}", t.cells);
        let t = derive(&carried(&[("LocPinY", 0.0)]), &frame(&[("FlipY", 1.0)]));
        assert!(close(number(t.cells.get("PinX")), 9.0) && close(number(t.cells.get("PinY")), 6.0), "{:?}", t.cells);
        // A markup whose own place is not the centre (the reader never gives one, but a sheet may): the centre is the pin plus the map of the vector from the place to
        // the centre. Pin (9, 5) at (1, 0.5) of a 4 by 2 frame turned a quarter turn: the centre is (9, 5) + (-0.5, 1) = (8.5, 6); the pin moved to the place 0 of the width
        // (and the height's 0.5 as it was) is (8.5, 6) - M(2, 0.5) = (8.5, 6) - (-0.5, 2) = (9, 4).
        let off_centre =
            sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 1.0), ("LocPinY", 0.5), ("Width", 4.0), ("Height", 2.0), ("Angle", quarter)]);
        let t = derive(&carried(&[("LocPinX", 0.0)]), &off_centre);
        assert!(close(number(t.cells.get("PinX")), 9.0) && close(number(t.cells.get("PinY")), 4.0), "{:?}", t.cells);
        // A carried pin that is what the carried place gives is kept; one that is not (the shape moved) is taken out and derived again.
        let held = carried(&[("PinX", 9.0), ("PinY", 3.0), ("LocPinX", 0.0)]);
        let t = derive(&held, &frame(&[("Angle", quarter)]));
        assert!(close(number(t.cells.get("PinX")), 9.0) && close(number(t.cells.get("PinY")), 3.0));
        let moved =
            sheet_of(&[("PinX", 14.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0), ("Angle", quarter)]);
        let t = derive(&held, &moved);
        assert!(close(number(t.cells.get("PinX")), 14.0) && close(number(t.cells.get("PinY")), 3.0), "{:?}", t.cells);
        // A pin with no place is the markup's: derived from the native place, it is the native pin, so a carried one that is not that is taken out.
        let t = derive(&carried(&[("PinX", 7.0)]), &frame(&[("Angle", quarter)]));
        assert!(!t.cells.contains_key("PinX"));
        let t = derive(&carried(&[("PinX", 9.0)]), &frame(&[("Angle", quarter)]));
        assert!(close(number(t.cells.get("PinX")), 9.0));
        // A turn the style carries is not worked out; nor a cell the model lacks; nor a markup without the whole frame; nor a style with none of the four.
        let before = |t: &Tier2, native: &Sheet| derive(t, native).cells == t.cells;
        let mut turned = carried(&[("LocPinX", 0.0), ("Angle", 1.0)]);
        assert!(before(&turned, &frame(&[("Angle", quarter)])));
        turned.cells.remove("Angle");
        turned.gone.push(Gone::Cell("PinY".to_owned()));
        assert!(before(&turned, &frame(&[("Angle", quarter)])));
        assert!(before(&carried(&[("LocPinX", 0.0)]), &sheet_of(&[("PinX", 9.0), ("LocPinX", 2.0), ("Angle", quarter)])));
        assert!(before(&carried(&[("Comment", 1.0)]), &frame(&[("Angle", quarter)])));
        // The unit is the markup's pin's.
        let mut with_unit = frame(&[("Angle", quarter)]);
        with_unit.cells.get_mut("PinY").unwrap().unit = Some("IN".into());
        assert_eq!(derive(&carried(&[("LocPinX", 0.0)]), &with_unit).cells.get("PinY").and_then(|c| c.unit.as_deref()), Some("IN"));
    }

    /// A place that works out to no finite number (a formula `Width*k` with a `k` of 1e308) gives no pin: a number that is not finite is no place.
    #[test]
    fn a_place_that_is_not_finite_gives_no_pin() {
        let native = sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        let mut turned = native.clone();
        turned.cells.insert(CellName::new("Angle"), cell(0.5));
        for native in [native, turned] {
            let mut t = Tier2::default();
            t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*1e308", Value::from_number(0.0)));
            derive_pins(&mut t, &native);
            assert!(!t.cells.contains_key("PinX"), "no pin of infinity: {:?}", t.cells.get("PinX"));
        }
    }

    /// **Of the review.** A turn the style carries that is the markup's (a formula `30 deg` whose value is the markup's angle, within LibreOffice's grid of 0.01
    /// degree: it wrote 0.5 as 0.50004) is no reason to leave the pin alone; one that says another turn is. The tolerance is inclusive.
    #[test]
    fn a_carried_turn_that_is_the_markups_does_not_stop_the_pin_being_derived() {
        let quarter = std::f64::consts::FRAC_PI_2;
        let native =
            sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0), ("Angle", quarter)]);
        let derived = |t: &mut Tier2, native: &Sheet| {
            derive_pins(t, native);
            t.cells.contains_key("PinX")
        };
        for (carried_angle, wanted) in
            [(quarter, true), (quarter + 1.7e-4, true), (quarter - 1.7e-4, true), (quarter + 1.9e-4, false), (quarter + 0.3, false)]
        {
            let mut t = carried(&[("LocPinX", 0.0), ("Angle", carried_angle)]);
            assert_eq!(derived(&mut t, &native), wanted, "a carried angle of {carried_angle}");
        }
        // The review's probe D as LibreOffice wrote it: a carried angle of 0.5 radian (a formula `28.6479 deg`) and the markup's 0.50004.
        let lo = sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0), ("Angle", 0.50004)]);
        let mut t = carried(&[("LocPinX", 0.0), ("Angle", 0.5)]);
        assert!(derived(&mut t, &lo), "0.5 against 0.50004: the pin is derived");
        // The markup's turn is the one used: the pin is the centre less the map of the place by the markup's angle, not the carried one's.
        let mut t = carried(&[("LocPinX", 0.0), ("Angle", quarter + 1.7e-4)]);
        derive_pins(&mut t, &native);
        assert!(
            (number(t.cells.get("PinY")).unwrap() - 3.0).abs() < 1e-9 && (number(t.cells.get("PinX")).unwrap() - 9.0).abs() < 1e-9,
            "{:?}",
            t.cells
        );
        // Exactly the tolerance is the markup's, past it is not (an unturned markup: the angle the markup has is 0).
        let flat = sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        let mut t = carried(&[("LocPinX", 0.0), ("Angle", TURN_TOLERANCE)]);
        assert!(derived(&mut t, &flat), "exactly the tolerance");
        let mut t = carried(&[("LocPinX", 0.0), ("Angle", TURN_TOLERANCE * 1.01)]);
        assert!(!derived(&mut t, &flat), "past it");
        // A flip is a turn of 0 or 1: the same carried as the markup's is no reason, another is; a carried turn that is no number is another.
        let flipped = sheet_of(&[("PinX", 9.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0), ("FlipX", 1.0)]);
        let mut t = carried(&[("LocPinX", 0.0), ("FlipX", 1.0)]);
        assert!(derived(&mut t, &flipped));
        let mut t = carried(&[("LocPinX", 0.0), ("FlipX", 1.0)]);
        assert!(!derived(&mut t, &flat), "a flip the markup does not have");
        let mut t = carried(&[("LocPinX", 0.0)]);
        t.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_text("left")));
        assert!(!derived(&mut t, &native), "an angle that is no number");
        // The carried turn stays carried (it is the model's formula): only the pin is derived.
        let mut t = carried(&[("LocPinX", 0.0), ("Angle", quarter)]);
        derive_pins(&mut t, &native);
        assert!(t.cells.contains_key("Angle"));
    }

    /// **Of the review.** A carried pin is compared with the edge by the place worked out with the markup's size (`Width*0.25` of the width now), and not by the
    /// place's cached value (of the width it was written at): a file of carries `PinX` with `LocPinX` `Width*0.25`, and LibreOffice widened the shape from 4 to 8 with
    /// the left edge kept. The pin is stale, and is the edge plus the place now.
    #[test]
    fn a_carried_pin_is_compared_by_the_place_worked_out_with_the_markups_size() {
        // The markup: 8 wide, the left edge at 2 (the rebuilt pin is the centre, 6, and its place 4). The style: the pin at 3, the place `Width*0.25` cached 1 (width 4).
        let native = sheet_of(&[("PinX", 6.0), ("PinY", 5.0), ("LocPinX", 4.0), ("LocPinY", 1.0), ("Width", 8.0), ("Height", 2.0)]);
        let mut t = carried(&[("PinX", 3.0)]);
        t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.25", Value::from_number(1.0)));
        derive_pins(&mut t, &native);
        assert_eq!(number(t.cells.get("PinX")), Some(4.0), "the left edge stays at 2: the pin is 2 + 0.25 * 8");
        // The same file with the width as it was: the pin stands.
        let same = sheet_of(&[("PinX", 4.0), ("PinY", 5.0), ("LocPinX", 2.0), ("LocPinY", 1.0), ("Width", 4.0), ("Height", 2.0)]);
        let mut t = carried(&[("PinX", 3.0)]);
        t.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.25", Value::from_number(1.0)));
        derive_pins(&mut t, &same);
        assert_eq!(number(t.cells.get("PinX")), Some(3.0));
        // A place that is no formula of the size is the value it holds.
        let mut t = carried(&[("PinX", 3.0), ("LocPinX", 1.0)]);
        derive_pins(&mut t, &native);
        assert_eq!(number(t.cells.get("PinX")), Some(3.0), "a constant place of 1 and the edge at 2");
    }
}
