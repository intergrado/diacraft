//! New style sheets: a style sheet the model has that the file lacks (the `Theme` style a theme
//! command makes) is written as **an empty common `graphic` style at the end of `office:styles`** of `styles.xml`, and **the whole of its sheet goes in DiaCraft's
//! document file** (a `style` child keyed by the style's name: `ext::docfile`), since what such a sheet holds (`THEMEVAL` formulas, number cells, rows of
//! gradient stops) has no property of ODF. LibreOffice sees an empty style, which shapes may name as their parent; it drops the document file, and a reader that finds
//! none gives the style no cells.
//!
//! * **Which styles.** Only styles **appended** to the file's: the first ones are the baseline's, unchanged (a change of a style sheet of the file is not written, but
//!   for the one case below), and the IDs of the file's are 0, 1, 2, ... in order, since the reader numbers the common styles so and a style added last is read with
//!   the next number. Each new one has the ID its place gives it, three links that name one style (the root or an earlier one: the form of the parent), a name, a
//!   `NameU` and nothing else of its own (no attribute and no node kept).
//! * **The name**: the `NameU` with each space written `_20_` and any other character an `NCName` does not allow at its place written `_`, its code point in
//!   upper-case hexadecimal and `_`; a name another style of the file has is refused (no `_1` is appended: the document file's entry is keyed by the name, and the
//!   shapes' parents name it). When the result is not the `NameU` the document file carries the `NameU`. `style:display-name` is the sheet's `Name`, written when it
//!   differs from the name.
//! * **The names the file holds**: a style sheet of the file is named by **the name its own markup gives it** (`style:name`, kept by
//!   the read as `NativeStyle::name_u`), and not by the `NameU` the model has, which the document file may have restored to another (a style `My Theme` the file holds as
//!   `My_20_Theme`): a parent written by the model's name named no style, and a second style of that name was not seen as a clash.
//! * **The parent**: `style:parent-style-name` names the style the three links name, and is left out for the root.
//! * **A style the file holds whose markup says nothing** (an empty common style, such as the `Theme` style after a LibreOffice save, which keeps the empty style and drops
//!   the document file) may have its cells and sections changed: the document file carries them as an entry for that style, as for a new one. Any other change of a
//!   style sheet the file holds is refused in words. **A style the model no longer has**, the last of the file's and empty in its markup (an undo of a style sheet a save
//!   added), is taken out of `office:styles`, and the document file loses its entry; any other is refused in words that say to redo.
//!
//! **The model writer** (a drawing no file of this format held, `WriteWork::model_path`) gives the same rules a wider form, since every style sheet it is given is new to the
//! package it makes, **the root's too**: the package holds the root the reader makes of the file's defaults, which is no model's (a VSDX file's root holds its own cells), so **the root's
//! sheet is carried like any other** (`root="1"` in the document file). A new style whose three links name different style sheets, one whose name is empty, too long or another style's,
//! is written as an empty common style whose parent is the style its first link names (the root where that is none) and a name of its own, **and the document file carries the truth**: the links
//! (`line-style`, `fill-style`, `text-style`), the `NameU` and the cells.
//!
//! **The numbering**. An OpenDocument file numbers its common styles by their places (the reader gives the style at place *p* the `ID` *p*), and a VSDX file's style
//! sheets are numbered by `ID`s with gaps (0, 1, 2, 6, 7, 30). The style sheets are written **in the model's order**, so that each is at a place of its own, and **the document file carries the
//! `ID` of each whose `ID` is not its place** (the `id` of its `style` entry: [`crate::ext::docfile`]); the reader gives the `ID`s back, and puts every link it made from the markup (a shape's parent,
//! a style's parent) through the same map. **Every link of the model is an `ID`**, and the places are found by a map of the `ID`s made once ([`places_of`]), so that an `ID` of 4,000,000 is a number
//! and never a place to count to; two style sheets of one `ID` are refused in words ([`WORDS_DUP`]), since a link would name neither. A style sheet with no `NameU` is refused in words
//! ([`WORDS_FORM`]): the file says a style by its name, and the document file keys the entry by it. **The splice writer** (a file the drawing was opened from) keeps its rule for a file whose
//! own numbering is its places: a new style sheet's `ID` is its place. A file whose numbering has gaps (one this crate wrote from a VSDX drawing) has no such rule to keep, and a new style
//! sheet takes any `ID` no other has, which the document file carries as it does a gap.
//!
//! Everything else is refused in words, and nothing is written. **What is written is read back** ([`check`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::emit::Pfx;
use super::frame::Editor;
use super::scan::{CommonStyleDef, CommonStyles};
use super::{WriteError, WriteWork};
use crate::ShapeIndex;
use crate::ext::docfile::NativeStyle;
use diacraft_doc::{Document, Links, Shape, Style};

/// The most characters of a style's name a new style is written with.
const MOST_NAME_BYTES: usize = 1024;

/// The refusal of a change of the document's style sheets that is not an appended style.
pub(crate) const WORDS_CHANGED: &str = "a change of a style sheet the file holds, or a style sheet taken from it";
/// The refusal of a style sheet taken away that cannot be: it is the root, or its markup holds a property, so that the undo that took it back is the change to take back in
/// turn (a redo gives the drawing the file has: the engine says so).
pub(crate) const WORDS_TAKEN: &str =
    "a style sheet the file holds taken back by an undo, which this save cannot take out of the file (only an empty common style added last can be)";
/// The refusal of two style sheets of one `ID`: a link names a style sheet by its `ID`, and would name neither.
pub(crate) const WORDS_DUP: &str = "two style sheets with the same ID (a style sheet is named by its ID, and a link to it would name neither)";
/// The refusal of more style sheets than a drawing holds, which the reader would refuse in the file.
pub(crate) const WORDS_MANY: &str = "more style sheets than a drawing holds (65,536, the most the reader reads)";
/// The refusal of a new style sheet that does not have the form a save writes.
pub(crate) const WORDS_FORM: &str = "a new style sheet that is not an empty common style the file's own markup can say (its ID must be the next one, its three links one style the file holds, and it needs a name)";

/// What a save does about the document's style sheets: the ones it adds, the ones it takes out, the ones of the file whose cells it carries, and the names the file holds
/// every style by (for the parents of shapes).
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub new: Vec<NewStyle>,
    /// The `style:name` of each style sheet of the file that the model no longer has: taken out of `office:styles`.
    pub gone: Vec<String>,
    /// The places of the style sheets of the file whose cells and sections changed, and which the document file carries.
    pub changed: Vec<u32>,
    /// The `style:name` of each style sheet of the model by its ID, made only when a shape's parent changes (it is the names a parent is written with).
    pub names: HashMap<u32, String>,
}

impl Plan {
    /// Whether the save does nothing about style sheets.
    pub(crate) fn is_empty(&self) -> bool {
        self.new.is_empty() && self.gone.is_empty() && self.changed.is_empty()
    }
}

/// A style sheet a save adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewStyle {
    /// Its `ID`: the model's, which the reader gives it back (the document file carries it when it is not the place).
    pub id: u32,
    /// Its place among the style sheets of the model, which is the number the file gives it.
    pub place: u32,
    /// `style:name`.
    pub name: String,
    /// `style:display-name`, when the sheet's `Name` is not the name.
    pub display: Option<String>,
    /// `style:parent-style-name`, for a sheet whose links name a style but the root.
    pub parent: Option<String>,
}

/// Whether `ch` may start an `NCName` (XML 1.0, fifth edition, `NameStartChar` without the colon).
fn name_start(ch: char) -> bool {
    matches!(ch,
        'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

/// Whether `ch` may stand in an `NCName` after its first character (`NameChar` without the colon).
fn name_char(ch: char) -> bool {
    name_start(ch) || matches!(ch, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// `NameU` as a `style:name`: each space is `_20_`, a character an `NCName` does not allow at its place is `_`, its code point in upper-case hexadecimal and
/// `_`, and every other character stays. **The characters are the ones XML 1.0 gives a name** (a leading digit, `-` or `.` is escaped; letters of any script stay, and
/// so do the digits of a name and the characters past U+FFFF that XML 1.0 allows; `ª`, `µ`, `²` and `½`, which a program's `is_alphanumeric` calls letters and digits and an
/// `NCName` does not allow, and the noncharacters U+FFFE and U+FFFF, are escaped).
pub(crate) fn style_name(name_u: &str) -> String {
    let mut out = String::with_capacity(name_u.len());
    for (at, ch) in name_u.chars().enumerate() {
        if ch == ' ' {
            out.push_str("_20_");
        } else if if at == 0 { name_start(ch) } else { name_char(ch) } {
            out.push(ch);
        } else {
            out.push_str(&format!("_{:X}_", u32::from(ch)));
        }
    }
    out
}

/// The `style:name` the file holds a style sheet by: for a sheet of the baseline **the name the file's own markup gave it** (`file`, the read's native styles: the model's
/// `NameU` is another when the document file restored one), the model's when the read kept none (a model that was never read), and for a new one the spec's.
fn odf_name(style: &Style, base: usize, at: usize, file: &[NativeStyle]) -> Option<String> {
    if at >= base {
        return style.name_u.as_deref().map(style_name);
    }
    let own = file.get(at).and_then(|n| n.name_u.as_deref());
    own.or(style.name_u.as_deref()).map(str::to_owned)
}

/// Whether the file's markup says nothing of the style sheet `at` but its links: no cell and no section. Its sheet is DiaCraft's, and the document file carries it.
fn markup_is_empty(file: &[NativeStyle], at: usize) -> bool {
    file.get(at).is_some_and(|n| n.sheet.cells.is_empty() && n.sheet.sections.is_empty())
}

/// Whether two style sheets are one to the writer: every field the same, but for the `.odg` data in the slot (the names, the unmapped properties and the units that the reader
/// holds): a held value is no edit (as for a shape, `own::shape_kept_same`), and a style the model made has none.
pub(crate) fn style_same(old: &Style, now: &Style) -> bool {
    old.id == now.id
        && old.name == now.name
        && old.name_u == now.name_u
        && (Arc::ptr_eq(&old.sheet, &now.sheet) || old.sheet == now.sheet)
        && old.raw == now.raw
        && old.passthrough == now.passthrough
        && match (old.ext.get(), now.ext.get()) {
            (None, None) => true,
            (Some(a), Some(b)) => a.vsdx == b.vsdx && a.native == b.native,
            (Some(only), None) | (None, Some(only)) => only.vsdx.0.is_empty() && only.native == diacraft_doc::Kept::default(),
        }
}

/// Whether the style sheets of two documents are the same ones, as [`style_same`] says, in the same order.
pub(crate) fn styles_same(a: &Document, b: &Document) -> bool {
    a.styles.len() == b.styles.len() && a.styles.iter().zip(b.styles.iter()).all(|(x, y)| Arc::ptr_eq(x, y) || style_same(x, y))
}

/// Whether the model's `now` is the baseline's `old` but for the cells and sections of its sheet: the names, the links and what the model keeps of the element are as
/// they were.
fn differs_in_cells_alone(old: &Style, now: &Style) -> bool {
    old.id == now.id
        && old.name == now.name
        && old.name_u == now.name_u
        && old.raw == now.raw
        && old.passthrough == now.passthrough
        && old.sheet.links == now.sheet.links
        && old.sheet.raw == now.sheet.raw
        && old.sheet.passthrough == now.sheet.passthrough
}

/// The `style:name` each style sheet of `doc` from place `from` on is written under, **the model writer's form** (see the module documentation): the name of the `NameU`, and where that is
/// empty, longer than a name may be or another style's (the names in `taken` and each earlier one), `Style_<place>` and, for another style's, the name with `_<n>` after it (the first free).
/// A style with no `NameU` has no name to write (the caller refuses it). Pure; the plan and `Native::after` (the markup as it will be) both use it.
///
/// **The probes are linear.** A name that is taken is tried again with the next number, and **the next number to try for each base name is kept** (`next`), so the k-th style that wants a name
/// does not walk the k names before it: every probe either finds a free name (one for each style) or steps over a name that was in `taken` or was made here, and none is stepped over twice.
/// The probes are at most the styles plus the names in `taken`, and are counted in `probes` (the plan charges them to the save's steps).
pub(crate) fn model_names(doc: &Document, from: usize, taken: &mut HashSet<String>, probes: &mut u64) -> Vec<String> {
    let mut next: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (at, style) in doc.styles.iter().enumerate().skip(from) {
        let wanted =
            style.name_u.as_deref().map(style_name).filter(|n| !n.is_empty() && n.len() <= MOST_NAME_BYTES).unwrap_or_else(|| format!("Style_{at}"));
        *probes = probes.saturating_add(1);
        let name = if taken.insert(wanted.clone()) {
            wanted
        } else {
            let again = next.entry(wanted.clone()).or_insert(2);
            loop {
                let candidate = format!("{wanted}_{again}");
                *again = again.saturating_add(1);
                *probes = probes.saturating_add(1);
                if taken.insert(candidate.clone()) {
                    break candidate;
                }
            }
        };
        out.push(name);
    }
    out
}

/// The place of each style sheet of `doc` by its `ID` (the first of an `ID`), made once: a link names a style sheet by its `ID`, and the file numbers it by its place. `None` when the map
/// cannot be allocated. The map has one entry for each style sheet, so it is [`WORDS_DUP`]'s test too: it has fewer entries than there are style sheets when an `ID` is held twice.
pub(crate) fn places_of(doc: &Document) -> Option<HashMap<u32, usize>> {
    let mut places: HashMap<u32, usize> = HashMap::new();
    places.try_reserve(doc.styles.len()).ok()?;
    for (at, style) in doc.styles.iter().enumerate() {
        places.entry(style.id).or_insert(at);
    }
    Some(places)
}

/// The links the file's markup gives the new style sheet at place `at` of `doc`: its three links are **the `ID` of the style the markup names as its parent**, which is the style the first link
/// of the model names when that style sheet is before it (`places`: [`places_of`]), and the root otherwise (the model writer's form: the true links ride in the document file).
pub(crate) fn markup_links(doc: &Document, at: usize, places: &HashMap<u32, usize>) -> Links {
    let links = doc.styles.get(at).map(|s| s.sheet.links).unwrap_or_default();
    let named = links.line.or(links.fill).or(links.text).unwrap_or(0);
    let parent = if places.get(&named).is_some_and(|p| *p < at) { named } else { 0 };
    Links { line: Some(parent), fill: Some(parent), text: Some(parent) }
}

/// [`differs_in_cells_alone`], the `NameU` let differ too: the document file gives a style its `NameU` back when the file holds it by another.
fn differs_in_cells_or_name(old: &Style, now: &Style) -> bool {
    old.id == now.id
        && old.name == now.name
        && old.raw == now.raw
        && old.passthrough == now.passthrough
        && old.sheet.links == now.sheet.links
        && old.sheet.raw == now.sheet.raw
        && old.sheet.passthrough == now.sheet.passthrough
}

/// The style sheets `doc` has that the baseline `base` lacks, those it lacks that the baseline has, and those of the baseline whose cells it carries, in the form a save
/// writes them, or the refusal in words. **A package only**: a flat drawing has no document file for what the sheets hold, and compares its styles whole. `file` is
/// what the file's own markup gives each style of the baseline (empty for a model no read made). `steps` is the save's own count, charged for each style sheet looked at.
/// `model_path` says the model writer is writing (see the module documentation): the root's sheet is carried, a new style may have links, a name and a form the markup cannot say, and
/// **a style sheet of the baseline is the model's own**, written as the common style it was (`write/model/sheets.rs`), so one that differs in its cells or its `NameU` alone is carried.
///
/// **The numbering**: the places are the model's order and the `ID`s are the model's own, found by one map ([`places_of`]). The baseline's `ID`s are what the reader gave them
/// (the places, or the `ID`s a document file restored: a file this crate wrote from a drawing with gaps), a new style sheet's are any that no other has, and **only the splice writer, onto a
/// file whose own numbering is its places, holds a new style sheet to its place** (the file's numbering is then its own).
pub(crate) fn plan(doc: &Document, base: &Document, file: &[NativeStyle], steps: &mut u64, model_path: bool) -> Result<Plan, &'static str> {
    let n = base.styles.len();
    *steps = steps.saturating_add(doc.styles.len() as u64).saturating_add(1);
    // A document with no style sheet at all (the reader always makes the root, so a drawing read from a file is never one) has none to add them after.
    if n == 0 {
        return if doc.styles.is_empty() { Ok(Plan::default()) } else { Err(WORDS_CHANGED) };
    }
    // More style sheets than the reader reads are no file: said before anything is made of them.
    if doc.styles.len() > diacraft_doc::MAX_STYLES {
        return Err(WORDS_MANY);
    }
    let mut plan = Plan::default();
    // The file's own, in order, with the IDs the reader gave them: unchanged, or (the root excepted) changed in the cells and sections of a sheet the file's markup
    // says nothing of.
    for (at, (old, now)) in base.styles.iter().zip(doc.styles.iter()).enumerate() {
        if Arc::ptr_eq(old, now) || style_same(old, now) {
            continue;
        }
        // The model writer carries the root's sheet and names: the package it made holds the root the reader makes of the file's defaults, which is nobody's.
        if model_path && at == 0 && now.id == 0 {
            plan.changed.push(0);
            *steps = steps.saturating_add(1);
            continue;
        }
        // On the model path the baseline's other style sheets are the model's own, written as the common styles they were (`write/model/sheets.rs`): a difference in
        // their cells, or in the `NameU` the file holds them by, is what that markup could not say, and the document file carries it whatever the markup says.
        let carried = if model_path { differs_in_cells_or_name(old, now) } else { markup_is_empty(file, at) && differs_in_cells_alone(old, now) };
        if at == 0 || !carried {
            return Err(WORDS_CHANGED);
        }
        plan.changed.push(u32::try_from(at).unwrap_or(u32::MAX));
        *steps = steps.saturating_add(1);
    }
    // The last of the file's that the model no longer has: an undo of a style sheet a save added. The root is never taken.
    let keep = doc.styles.len().min(n);
    for at in keep..n {
        let Some(name) = file.get(at).and_then(|f| f.name_u.as_deref()).filter(|_| at != 0 && markup_is_empty(file, at)) else {
            return Err(WORDS_TAKEN);
        };
        plan.gone.push(name.to_owned());
        *steps = steps.saturating_add(1);
    }
    let mut names: HashSet<String> = HashSet::new();
    names.try_reserve(doc.styles.len()).map_err(|_| WORDS_FORM)?;
    for (at, style) in doc.styles.iter().enumerate().take(keep) {
        if let Some(name) = odf_name(style, n, at, file) {
            names.insert(name);
        }
    }
    // The model writer's names, made once (the first free, by place).
    let model_named = if model_path {
        let mut probes = 0u64;
        let list = model_names(doc, n, &mut names, &mut probes);
        // Each probe is a step of the save, charged where it was made: the names are made once for the plan.
        *steps = steps.saturating_add(probes);
        Some(list)
    } else {
        None
    };
    // The places of the `ID`s, made once and charged by the style sheets, for the style sheets to add (a save that adds none needs none); two style sheets of one `ID` are no numbering. The splice
    // writer holds a new style sheet to its place when the file's own numbering is its places: whether it is is a look at the baseline's `ID`s, made once.
    let mut places: HashMap<u32, usize> = HashMap::new();
    let mut own_numbering = false;
    if doc.styles.len() > n {
        places = places_of(doc).ok_or(WORDS_FORM)?;
        if places.len() != doc.styles.len() {
            return Err(WORDS_DUP);
        }
        if !model_path {
            *steps = steps.saturating_add(n as u64);
            own_numbering = base.styles.iter().enumerate().all(|(at, s)| u32::try_from(at).ok() == Some(s.id));
        }
    }
    for (at, style) in doc.styles.iter().enumerate().skip(n) {
        let place = u32::try_from(at).map_err(|_| WORDS_FORM)?;
        // The splice writer's own rule: the file's numbering is its places, and a new style sheet takes the next one.
        if !model_path && own_numbering && style.id != place {
            return Err(WORDS_FORM);
        }
        let (Some(name_u), Some(shown)) = (style.name_u.as_deref(), style.name.as_deref()) else { return Err(WORDS_FORM) };
        if shown.len() > MOST_NAME_BYTES {
            return Err(WORDS_FORM);
        }
        let name = match &model_named {
            Some(list) => list.get(at - n).cloned().ok_or(WORDS_FORM)?,
            None => {
                let links = style.sheet.links;
                let link = links.line.unwrap_or(0);
                let one = links.fill.unwrap_or(0) == link && links.text.unwrap_or(0) == link;
                // What the style's element held that no ODF markup says (`raw`, `passthrough`, its slot) rides in DiaCraft's document file. The style the links name is
                // one the file holds before this one (the root, or the style sheet at an earlier place).
                if !one || (link != 0 && !places.get(&link).is_some_and(|p| *p < at)) {
                    return Err(WORDS_FORM);
                }
                let name = style_name(name_u);
                if name_u.is_empty() || name.len() > MOST_NAME_BYTES || !names.insert(name.clone()) {
                    return Err(WORDS_FORM);
                }
                name
            }
        };
        if super::splice::has_bad_char(shown) || super::splice::has_bad_char(name_u) {
            return Err(super::splice::WORDS_XML_CHAR);
        }
        let link = if model_path { markup_links(doc, at, &places).line.unwrap_or(0) } else { style.sheet.links.line.unwrap_or(0) };
        let parent = match link {
            0 => None,
            link => {
                let at_parent = places.get(&link).copied().ok_or(WORDS_FORM)?;
                Some(match &model_named {
                    Some(list) if at_parent >= n => list.get(at_parent - n).cloned().ok_or(WORDS_FORM)?,
                    _ => doc.styles.get(at_parent).and_then(|p| odf_name(p, n, at_parent, file)).ok_or(WORDS_FORM)?,
                })
            }
        };
        plan.new.push(NewStyle { id: style.id, place, name: name.clone(), display: (shown != name).then(|| shown.to_owned()), parent });
        *steps = steps.saturating_add(1);
    }
    Ok(plan)
}

/// The `style:name` of every style sheet of `doc` by its ID: what an edit of a shape's parent names. The file's own are named by the name the file holds them under, the new ones by the
/// name the save writes them under (`new`, the plan's: the name, and for the model writer the one it made free). A style whose name the file cannot say is left out.
pub(crate) fn names_by_id(doc: &Document, base: &Document, file: &[NativeStyle], new: &[NewStyle]) -> HashMap<u32, String> {
    let n = base.styles.len();
    let mut names: HashMap<u32, String> =
        doc.styles.iter().enumerate().take(n).filter_map(|(at, s)| odf_name(s, n, at, file).map(|name| (s.id, name))).collect();
    names.extend(new.iter().map(|s| (s.id, s.name.clone())));
    names
}

impl Editor<'_, '_> {
    /// Puts the elements of the new style sheets at the end of `office:styles` (`common`): an empty `style:style` of the `graphic` family each, with its display name
    /// and its parent. Each attribute is charged as the splice charges one. A part with no `office:styles` is refused in words.
    pub(crate) fn put_common_styles(&mut self, common: Option<&CommonStyles>, new: &[NewStyle]) -> Result<(), WriteError> {
        if new.is_empty() {
            return Ok(());
        }
        let Some(common) = common else {
            return Err(WriteError::Unsupported("a styles part with no office:styles to put a new style sheet in"));
        };
        let pfx = Pfx::of(&mut self.scopes, common.scope, self.work);
        let escape = |t: &str| super::splice::escape_value(t, b'"');
        let mut text = String::new();
        for style in new {
            self.work.charge_attribute(self.limits)?;
            self.work.charge_attribute(self.limits)?;
            text.push_str(&format!("<{0}:style{1} {0}:name=\"{2}\" {0}:family=\"graphic\"", pfx.style, pfx.declare, escape(&style.name)));
            if let Some(display) = &style.display {
                self.work.charge_attribute(self.limits)?;
                text.push_str(&format!(" {}:display-name=\"{}\"", pfx.style, escape(display)));
            }
            if let Some(parent) = &style.parent {
                self.work.charge_attribute(self.limits)?;
                text.push_str(&format!(" {}:parent-style-name=\"{}\"", pfx.style, escape(parent)));
            }
            text.push_str("/>");
        }
        // Put in by `flush_common_styles`, after the markers the save adds.
        self.common_text.push_str(&text);
        Ok(())
    }

    /// Takes out of `office:styles` the common styles called `gone` (the file's last style sheets, which the model no longer has): the first element of that name, with the
    /// blank run before it. One that the part does not hold is refused in words (the baseline numbered it from another part).
    pub(crate) fn remove_common_styles(&mut self, defs: &[CommonStyleDef], gone: &[String]) -> Result<(), WriteError> {
        for name in gone {
            self.work.order_steps = self.work.order_steps.saturating_add(defs.len() as u64);
            let found = defs.iter().find(|d| d.name == *name && matches!(d.family.as_str(), "graphic" | "paragraph"));
            let Some(found) = found else {
                return Err(WriteError::Unsupported("a style sheet taken out that styles.xml does not hold as a common style"));
            };
            self.splicer.remove(found.lead, found.end);
        }
        Ok(())
    }

    /// Puts what the save added to `office:styles` (`common_text`) after the last element it holds, **in one edit**: at the end of it, or, for an empty element
    /// (`<office:styles/>`, whose last two bytes are `/>`), by writing the element whole with the text in it. Two edits of the same bytes would be refused.
    pub(crate) fn flush_common_styles(&mut self, common: Option<&CommonStyles>) {
        let text = std::mem::take(&mut self.common_text);
        let Some(common) = common.filter(|_| !text.is_empty()) else { return };
        if common.empty {
            self.splicer.replace(common.open.1.saturating_sub(2), common.open.1, format!(">{text}</{}>", common.qname));
        } else {
            self.splicer.insert(common.close_start, text);
        }
    }
}

/// What the file made must read back as: the style sheets the save added, took out or carried the cells of, and the links of the shapes whose parent it wrote.
#[derive(Debug, Default)]
pub(crate) struct Expect<'a> {
    pub plan: Option<&'a Plan>,
    /// The shapes whose three links name a style sheet the save wrote as their parent: the page's place in the document, the shape's `ID` and the links.
    pub links: Vec<(usize, u32, Links)>,
}

impl Expect<'_> {
    fn is_empty(&self) -> bool {
        self.plan.is_none_or(Plan::is_empty) && self.links.is_empty()
    }
}

/// Reads the bytes made back: each style sheet the save added or carried the cells of is there as the model has it (its `ID`, names, links and the whole of its sheet),
/// found by the number the reader gives it, the file reads with as many style sheets as the model has, **and each shape whose parent the save wrote reads with the links
/// the model has** (a parent that named no style was read back as the root, and the save said nothing). A difference is a refused save and never
/// a lost sheet. **A package of more than [`super::MOST_READ_BACK`] bytes is not read again** (the read is counted: [`WriteWork::read_back_bytes`]).
pub(crate) fn check(
    bytes: &[u8],
    doc: &Document,
    expect: &Expect<'_>,
    index: &ShapeIndex,
    limits: &crate::Limits,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    // A package past the most the read back reads is left unchecked, as the checks of the paragraphs and of tier 2 leave it (the writer builds no third model
    // beside the caller's for a drawing near the reader's caps). Most saves of a themed drawing write a parent (every shape put on it), so this read is no rare one.
    if expect.is_empty() || bytes.len() > super::MOST_READ_BACK {
        return Ok(());
    }
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    work.charge_scan(bytes.len(), limits)?;
    work.read_back_bytes = work.read_back_bytes.saturating_add(bytes.len() as u64);
    let (read, report) = crate::read::read(bytes, limits).map_err(|e| failed(&format!("the file made does not read again: {e}")))?;
    if read.styles.len() != doc.styles.len() {
        return Err(failed("the file made holds another number of style sheets than the model"));
    }
    if let Some(plan) = expect.plan {
        let places = plan.new.iter().map(|s| s.place).chain(plan.changed.iter().copied());
        for place in places {
            let at = place as usize;
            let (Some(wanted), Some(back)) = (doc.styles.get(at), read.styles.get(at)) else {
                return Err(failed("a new style sheet is not read again"));
            };
            work.cells_compared = work.cells_compared.saturating_add(1);
            if back.id != wanted.id || back.name != wanted.name || back.name_u != wanted.name_u || back.sheet.links != wanted.sheet.links {
                return Err(failed("a new style sheet is not read back with the names and links the model has"));
            }
            if back.sheet != wanted.sheet {
                return Err(failed("a new style sheet is not read back with the cells the model has"));
            }
        }
    }
    check_links(&read, &report.index, expect, index, work)
}

/// The links of each shape of a page read again, by the ordinal of the element it was read from.
type ReadLinks = (HashMap<u32, u32>, HashMap<u32, Links>);

/// Each shape whose parent the save wrote is read back with the links the model has: its element is the one the index the save made gives it, and the shape read
/// from that element has them.
fn check_links(read: &Document, read_index: &ShapeIndex, expect: &Expect<'_>, index: &ShapeIndex, work: &mut WriteWork) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut pages: HashMap<usize, ReadLinks> = HashMap::new();
    for (page_at, id, links) in &expect.links {
        work.order_steps = work.order_steps.saturating_add(1);
        if !pages.contains_key(page_at) {
            let shapes = read_index.page(*page_at).ok_or_else(|| failed("a page whose shape has a parent is not read again"))?;
            let page = read.pages.get(*page_at).ok_or_else(|| failed("a page whose shape has a parent is not read again"))?;
            let held = shapes.ordinal_map().len();
            work.order_steps = work.order_steps.saturating_add(held as u64);
            work.charge_steps(held.saturating_mul(2))?;
            let ids: HashMap<u32, u32> = shapes.ordinal_map().iter().map(|(id, o)| (*o, *id)).collect();
            let mut by_id: HashMap<u32, Links> = HashMap::new();
            let mut stack: Vec<&Arc<Shape>> = page.shapes.iter().collect();
            while let Some(shape) = stack.pop() {
                by_id.entry(shape.id).or_insert(shape.sheet.links);
                stack.extend(shape.children.iter());
            }
            pages.insert(*page_at, (ids, by_id));
        }
        let (ids, by_id) = pages.get(page_at).ok_or_else(|| failed("a page whose shape has a parent is not read again"))?;
        let ordinal = index.ordinal(*page_at, *id).ok_or_else(|| failed("a shape whose parent was written has no element in the file made"))?;
        let read_links =
            ids.get(&ordinal).and_then(|read_id| by_id.get(read_id)).ok_or_else(|| failed("a shape whose parent was written is not read again"))?;
        work.cells_compared = work.cells_compared.saturating_add(1);
        if read_links != links {
            return Err(failed("a shape is not read back with the links the model has (the style its parent names is not a style the file holds)"));
        }
    }
    Ok(())
}

/// The shapes of the save whose three links name a style sheet that the save wrote as their parent: a shape of the file whose links changed, and a new shape on a
/// style sheet. Each is read back by [`check`].
pub(crate) fn links_written(diff: &super::compare::Diff) -> Vec<(usize, u32, Links)> {
    let mut out: Vec<(usize, u32, Links)> = Vec::new();
    for page in &diff.pages {
        for change in &page.shapes {
            if let Some(parent) = change.style.as_ref().and_then(|s| s.parent) {
                out.push((page.page_at, change.id, Links { line: Some(parent), fill: Some(parent), text: Some(parent) }));
            }
        }
        for new in &page.new {
            let l = new.shape.sheet.links;
            if [l.line, l.fill, l.text].iter().any(|k| k.is_some_and(|id| id != 0)) {
                out.push((page.page_at, new.shape.id, l));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The style sheets a model adds, as [`plan`] says them for a model no read made (the file's own markup is not known).
    fn planned(doc: &Document, base: &Document, steps: &mut u64) -> Result<Vec<NewStyle>, &'static str> {
        plan(doc, base, &[], steps, false).map(|p| p.new)
    }

    /// A space is `_20_`, a character an `NCName` does not allow at its place is `_`, its code point in upper-case hexadecimal and `_`; letters, digits, `_`, `-` and
    /// `.` stay (the last two not first, and digits not first).
    #[test]
    fn a_name_is_written_as_says() {
        for (name_u, written) in [
            ("Theme", "Theme"),
            ("My Style", "My_20_Style"),
            ("1st", "_31_st"),
            ("-x", "_2D_x"),
            (".x", "_2E_x"),
            ("a-b.c_d9", "a-b.c_d9"),
            ("a/b", "a_2F_b"),
            ("a:b", "a_3A_b"),
            ("a<b>&\"'", "a_3C_b_3E__26__22__27_"),
            ("\u{e9}t\u{e9}", "\u{e9}t\u{e9}"),
            ("a\u{0}b", "a_0_b"),
            ("x\u{1F600}", "x\u{1F600}"),
            ("x\u{FFFE}", "x_FFFE_"),
            ("x\u{F0}\u{2E80}", "x\u{F0}\u{2E80}"),
            ("\u{AA}", "_AA_"),
            ("a\u{B5}", "a_B5_"),
            ("a\u{B2}\u{BD}", "a_B2__BD_"),
            ("a\u{B7}", "a\u{B7}"),
            ("\u{B7}a", "_B7_a"),
            ("a\u{300}", "a\u{300}"),
            ("\u{300}a", "_300_a"),
            ("\u{3b1}\u{3b2}9", "\u{3b1}\u{3b2}9"),
            ("", ""),
        ] {
            assert_eq!(style_name(name_u), written, "{name_u:?}");
        }
    }

    fn style(id: u32, name: &str, link: u32) -> std::sync::Arc<Style> {
        let sheet = diacraft_doc::Sheet {
            links: diacraft_doc::Links { line: Some(link), fill: Some(link), text: Some(link) },
            ..diacraft_doc::Sheet::default()
        };
        std::sync::Arc::new(Style {
            id,
            name: Some(std::sync::Arc::from(name)),
            name_u: Some(std::sync::Arc::from(name)),
            sheet: std::sync::Arc::new(sheet),
            ..Style::default()
        })
    }

    fn document(styles: Vec<std::sync::Arc<Style>>) -> Document {
        let mut doc = Document::empty();
        doc.styles = diacraft_doc::SharedVec::from_vec(styles);
        doc
    }

    /// **The names of the model writer are made in a linear number of probes** (the security scan's finding: the k-th of N styles that want one name tried the k names before it, which is N squared
    /// probes, uncharged). 50,000 style sheets that all want `Box`, with `Box`, `Box_2` to `Box_50000` already taken in the file: each style is one probe past the first, which steps over the
    /// 50,000 taken names once, and the probes come to at most the styles plus the names taken (the old loop made about 1,250 million). A yardstick of 50,000 distinct names costs one probe each.
    #[test]
    fn fifty_thousand_styles_that_want_one_name_are_named_in_a_linear_number_of_probes() {
        const N: usize = 50_000;
        let mut doc = document(vec![style(0, "No Style", 0)]);
        let more: Vec<std::sync::Arc<Style>> = (1..=N).map(|id| style(id as u32, "Box", 0)).collect();
        for s in more {
            doc.styles.push(s);
        }
        let mut taken: HashSet<String> = HashSet::from(["Box".to_owned()]);
        taken.extend((2..=N).map(|k| format!("Box_{k}")));
        let before = taken.clone();
        let held = taken.len();
        let mut probes = 0u64;
        let names = model_names(&doc, 1, &mut taken, &mut probes);
        assert_eq!(names.len(), N);
        let distinct: HashSet<&String> = names.iter().collect();
        assert_eq!(distinct.len(), N, "every style has a name of its own");
        assert!(names.iter().all(|n| !before.contains(n)), "none is a name that was taken");
        assert!(probes <= (2 * (N + held) + 16) as u64, "{probes} probes for {N} styles beside {held} taken names");
        // The yardstick: as many styles with names of their own, one probe each.
        let mut plain = document(vec![style(0, "No Style", 0)]);
        for id in 1..=N {
            plain.styles.push(style(id as u32, &format!("S{id}"), 0));
        }
        let mut yard = 0u64;
        model_names(&plain, 1, &mut HashSet::new(), &mut yard);
        assert_eq!(yard, N as u64);
        assert!(probes <= 4 * yard + (2 * held) as u64 + 16, "within a small constant of the yardstick: {probes} against {yard}");
    }

    fn base() -> Document {
        document(vec![style(0, "No Style", 0), style(1, "Standard", 0), style(2, "Heading", 1)])
    }

    /// Only styles added at the end, in the form a save writes, are planned; each other change of the styles is refused in words, and a style the file holds is
    /// compared by value.
    #[test]
    fn only_a_style_added_last_in_the_form_the_file_can_say_is_planned() {
        let base = base();
        let mut steps = 0;
        assert_eq!(planned(&base, &base, &mut steps), Ok(Vec::new()), "nothing new");
        assert_eq!(steps, 3 + 1, "a step for each style and one more");
        let with = |extra: Vec<std::sync::Arc<Style>>| {
            let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
            all.extend(extra);
            document(all)
        };
        // One new style on the root, one on an earlier style, one on a new one: parents by the names the file holds them by.
        let made = planned(&with(vec![style(3, "Theme", 0), style(4, "My Theme", 3), style(5, "Third", 2)]), &base, &mut 0).unwrap();
        assert_eq!(
            made,
            [
                NewStyle { id: 3, place: 3, name: "Theme".to_owned(), display: None, parent: None },
                NewStyle { id: 4, place: 4, name: "My_20_Theme".to_owned(), display: Some("My Theme".to_owned()), parent: Some("Theme".to_owned()) },
                NewStyle { id: 5, place: 5, name: "Third".to_owned(), display: None, parent: Some("Heading".to_owned()) },
            ]
        );
        // Changes of the file's own styles, a style taken away and a root that changed are the old refusal.
        let mut changed: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
        changed[1] = style(1, "Standard", 2);
        assert_eq!(planned(&document(changed), &base, &mut 0), Err(WORDS_CHANGED));
        assert_eq!(
            planned(&document(base.styles.iter().take(2).cloned().collect()), &base, &mut 0),
            Err(WORDS_TAKEN),
            "a style taken away that the file's markup is not known to leave empty"
        );
        assert_eq!(planned(&document(Vec::new()), &document(Vec::new()), &mut 0), Ok(Vec::new()), "no style sheet on either side");
        assert_eq!(
            planned(&with(vec![style(0, "No Style", 0)]), &document(Vec::new()), &mut 0),
            Err(WORDS_CHANGED),
            "a baseline with no root has none to add after"
        );
        // A baseline whose IDs are not its places (a file whose document file restored them) is the file's own, and is no change when the model holds it as it is.
        let mut gap: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
        gap[2] = style(5, "Heading", 1);
        assert_eq!(planned(&document(gap.clone()), &document(gap.clone()), &mut 0), Ok(Vec::new()), "the file's IDs are its own");
        // A style sheet of the file whose ID changed is a change of it.
        let mut moved = gap.clone();
        moved[2] = style(6, "Heading", 1);
        assert_eq!(planned(&document(moved), &document(gap), &mut 0), Err(WORDS_CHANGED));
        // A new style whose form is not the one the file can say.
        for bad in [
            style(9, "Theme", 0),
            style(3, "Standard", 0),
            style(3, "Theme", 3),
            style(3, "Theme", 7),
            {
                let mut s = (*style(3, "Theme", 0)).clone();
                s.name = None;
                std::sync::Arc::new(s)
            },
            {
                let mut s = (*style(3, "Theme", 0)).clone();
                s.name_u = None;
                std::sync::Arc::new(s)
            },
            {
                let mut s = (*style(3, "Theme", 0)).clone();
                let sheet = std::sync::Arc::make_mut(&mut s.sheet);
                sheet.links.fill = Some(1);
                std::sync::Arc::new(s)
            },
            {
                let mut s = (*style(3, "Theme", 0)).clone();
                let sheet = std::sync::Arc::make_mut(&mut s.sheet);
                sheet.links.text = Some(2);
                std::sync::Arc::new(s)
            },
            style(3, &"x".repeat(MOST_NAME_BYTES + 1), 0),
            style(3, "", 0),
        ] {
            assert_eq!(planned(&with(vec![bad]), &base, &mut 0), Err(WORDS_FORM));
        }
        // What the style's element kept (attributes, elements) rides in DiaCraft's document file: it is no reason to refuse the style.
        let kept = {
            let mut s = (*style(3, "Theme", 0)).clone();
            s.passthrough.push(0, diacraft_doc::RawNode::Comment(std::sync::Arc::from("kept")));
            s.raw = diacraft_doc::RawAttrs::from_pairs([("IsCustomName", "1")]);
            std::sync::Arc::new(s)
        };
        assert!(planned(&with(vec![kept]), &base, &mut 0).is_ok());
        // Two new styles that would be one name.
        assert_eq!(planned(&with(vec![style(3, "A B", 0), style(4, "A_20_B", 0)]), &base, &mut 0), Err(WORDS_FORM));
        // A character XML cannot hold in the display name.
        let mut shown = (*style(3, "Theme", 0)).clone();
        shown.name = Some(std::sync::Arc::from("a\u{1}b"));
        assert_eq!(planned(&with(vec![std::sync::Arc::new(shown)]), &base, &mut 0), Err(super::super::splice::WORDS_XML_CHAR));
    }

    /// **The numbering**: the places of the `ID`s are found by one map (an `ID` of 4,000,000 is a key, never a place), the links of a new style are `ID`s, the markup's parent is the
    /// `ID` of the style the first link names when that is a style before it, and two style sheets of one `ID` are refused in words.
    #[test]
    fn the_places_of_the_ids_are_one_map_and_the_markups_parent_is_an_id() {
        let doc = document(vec![
            style(0, "No Style", 0),
            style(1, "Standard", 0),
            style(4_000_000, "Far", 1),
            style(9, "Later", 4_000_000),
            style(30, "Last", 9),
        ]);
        let places = places_of(&doc).unwrap();
        assert_eq!(places.len(), 5);
        assert_eq!((places[&0], places[&1], places[&4_000_000], places[&9], places[&30]), (0, 1, 2, 3, 4));
        let one = |l: u32| Links { line: Some(l), fill: Some(l), text: Some(l) };
        // The parent of each: the ID of the style its first link names (an earlier place), else the root.
        assert_eq!(markup_links(&doc, 2, &places), one(1));
        assert_eq!(markup_links(&doc, 3, &places), one(4_000_000));
        assert_eq!(markup_links(&doc, 4, &places), one(9));
        // A link to a style that is later, to itself, to none, and no link at all.
        let mut odd = document(vec![style(0, "No Style", 0), style(5, "A", 6), style(6, "B", 6), style(7, "C", 99)]);
        std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(odd.styles.get_mut(2).unwrap()).sheet).links =
            Links { line: None, fill: Some(5), text: None };
        let places = places_of(&odd).unwrap();
        assert_eq!(markup_links(&odd, 1, &places), one(0), "a link to a later style names the root");
        assert_eq!(markup_links(&odd, 2, &places), one(5), "the first link that names one");
        assert_eq!(markup_links(&odd, 3, &places), one(0), "a link to no style");
        // An ID held twice: the map holds the first, and has fewer entries than there are style sheets.
        let twice = document(vec![style(0, "No Style", 0), style(6, "A", 0), style(6, "B", 0)]);
        let places = places_of(&twice).unwrap();
        assert_eq!((places.len(), places[&6]), (2, 1));
        assert_eq!(planned(&twice, &document(vec![style(0, "No Style", 0)]), &mut 0), Err(WORDS_DUP));
        assert_eq!(plan(&twice, &document(vec![style(0, "No Style", 0)]), &[], &mut 0, true).err(), Some(WORDS_DUP));
    }

    /// The model writer takes any `ID`s, in the model's order, with the place each is written at; the splice writer takes them only onto a file whose own numbering is not its places.
    #[test]
    fn new_style_sheets_take_any_unused_id_on_the_model_path_and_on_a_file_with_gaps() {
        let base = base();
        let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
        all.extend([style(6, "Box", 2), style(7, "Accent", 6), style(30, "Last", 7)]);
        let doc = document(all);
        // The splice writer, onto a file numbered by its places: the old refusal.
        assert_eq!(planned(&doc, &base, &mut 0), Err(WORDS_FORM));
        // The model writer: each is at its place, with its ID, and its parent is the style its first link names, by the name it is written under.
        let made = plan(&doc, &base, &[], &mut 0, true).unwrap().new;
        assert_eq!(made.iter().map(|s| (s.id, s.place)).collect::<Vec<_>>(), [(6, 3), (7, 4), (30, 5)]);
        assert_eq!(made.iter().map(|s| s.parent.as_deref()).collect::<Vec<_>>(), [Some("Heading"), Some("Box"), Some("Accent")]);
        // The splice writer onto a file whose own numbering has gaps: any unused ID, and the parent is found by its ID.
        let gapped = {
            let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
            all[2] = style(6, "Heading", 1);
            document(all)
        };
        let mut grown: Vec<std::sync::Arc<Style>> = gapped.styles.iter().cloned().collect();
        grown.push(style(31, "Added", 6));
        let made = planned(&document(grown), &gapped, &mut 0).unwrap();
        assert_eq!(made, [NewStyle { id: 31, place: 3, name: "Added".to_owned(), display: None, parent: Some("Heading".to_owned()) }]);
        // An ID the file holds is no ID for a new one, and a link must name a style sheet by its ID, as it is.
        let mut clash: Vec<std::sync::Arc<Style>> = gapped.styles.iter().cloned().collect();
        clash.push(style(6, "Added", 0));
        assert_eq!(planned(&document(clash), &gapped, &mut 0), Err(WORDS_DUP));
        let mut by_place: Vec<std::sync::Arc<Style>> = gapped.styles.iter().cloned().collect();
        by_place.push(style(31, "Added", 2));
        assert_eq!(
            planned(&document(by_place), &gapped, &mut 0),
            Err(WORDS_FORM),
            "2 is a place of the file and no ID of it: the link names no style sheet"
        );
    }

    /// More style sheets than the reader reads are refused in words before anything is made of them, whatever their numbering; the most is not.
    #[test]
    fn more_style_sheets_than_a_drawing_holds_are_refused_before_they_are_looked_at() {
        let base = base();
        let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
        all.extend((3..diacraft_doc::MAX_STYLES as u32).map(|id| style(id * 3, &format!("S{id}"), 0)));
        assert_eq!(all.len(), diacraft_doc::MAX_STYLES);
        assert!(plan(&document(all.clone()), &base, &[], &mut 0, true).is_ok(), "the most a drawing holds");
        all.push(style(1_000_000, "One more", 0));
        assert_eq!(plan(&document(all), &base, &[], &mut 0, true).err(), Some(WORDS_MANY));
    }

    /// The most a name may be, to the byte, for the name R11 writes and for the display name each alone: a `NameU` of spaces is written four times as long, and the sheet's
    /// `Name` is a text of its own.
    #[test]
    fn a_name_is_held_to_the_most_bytes_the_name_written_and_the_display_name_each_alone() {
        let base = base();
        let with = |extra: std::sync::Arc<Style>| {
            let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
            all.push(extra);
            document(all)
        };
        let named = |name_u: &str, shown: &str| {
            let mut s = (*style(3, name_u, 0)).clone();
            s.name = Some(std::sync::Arc::from(shown));
            std::sync::Arc::new(s)
        };
        let most = MOST_NAME_BYTES;
        // Exactly at the most: written (the name and the display name are the same here).
        assert!(planned(&with(named(&"x".repeat(most), &"x".repeat(most))), &base, &mut 0).is_ok(), "at the most");
        assert_eq!(planned(&with(named(&"x".repeat(most + 1), &"x".repeat(most + 1))), &base, &mut 0), Err(WORDS_FORM), "one past");
        // The name written alone: 257 spaces are 1,028 bytes written, with a display name that is short.
        assert_eq!(
            planned(&with(named(&" ".repeat(most / 4 + 1), "short")), &base, &mut 0),
            Err(WORDS_FORM),
            "the name past the most, the display name short"
        );
        assert!(planned(&with(named(&" ".repeat(most / 4), "short")), &base, &mut 0).is_ok(), "the name at the most exactly");
        // The display name alone: a short name with a display name one past the most, and one at it.
        assert_eq!(planned(&with(named("short", &"y".repeat(most + 1))), &base, &mut 0), Err(WORDS_FORM), "the display name past the most");
        assert!(planned(&with(named("short", &"y".repeat(most))), &base, &mut 0).is_ok(), "the display name at the most");
    }

    /// A package of one empty page whose `office:styles` holds `common`: what a save made, with the parts the reader needs and no other.
    fn package(common: &str) -> Vec<u8> {
        package_with(common, "", "")
    }

    /// [`package`] with `auto` in the automatic styles of `content.xml` and `shapes` on the page.
    fn package_with(common: &str, auto: &str, shapes: &str) -> Vec<u8> {
        use diacraft_container::zip::{ArchiveWriter, Method};
        const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
        const NS: &str = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
            r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
        );
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>{auto}</office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{shapes}</draw:page></office:drawing></office:body></office:document-content>"
        );
        let styles = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
        );
        let manifest = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MEDIA}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
        );
        let mut w = ArchiveWriter::new();
        w.add("mimetype", MEDIA.as_bytes(), Method::Stored).unwrap();
        w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
        w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
        w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
        w.finish().unwrap()
    }

    /// The read back of the bytes a save made (the self-check of the style sheets it added): the model's new styles are found as the model has them, by the number the
    /// reader gives each, and **each part of the comparison alone** fails it (the ID, the name, the `NameU`, the links, the sheet) and so does a count of styles the
    /// file does not have; it is made only when a style was added, and it counts the bytes it reads again.
    #[test]
    fn the_read_back_of_a_style_sheet_added_last_fails_for_each_difference_alone_and_is_made_only_for_a_new_style() {
        let limits = crate::Limits::default();
        let bytes = package(r#"<style:style style:name="Theme" style:family="graphic"/>"#);
        let (read, _) = crate::read::read(&bytes, &limits).unwrap();
        let at = read.styles.len() - 1;
        let new = vec![NewStyle {
            id: u32::try_from(at).unwrap(),
            place: u32::try_from(at).unwrap(),
            name: "Theme".to_owned(),
            display: None,
            parent: None,
        }];
        let check = |doc: &Document, new: &[NewStyle]| {
            let mut work = WriteWork::default();
            let plan = Plan { new: new.to_vec(), ..Plan::default() };
            let done = check(&bytes, doc, &Expect { plan: Some(&plan), links: Vec::new() }, &ShapeIndex::default(), &limits, &mut work);
            (done, work)
        };
        // As the file is read: it passes, and the bytes read again are counted (charged to the scan, too).
        let (done, work) = check(&read, &new);
        assert!(done.is_ok(), "{done:?}");
        assert_eq!(work.read_back_bytes, bytes.len() as u64);
        assert!(work.cells_compared >= 1, "the style is compared");
        // No new style: nothing is read again, whatever the model is.
        let mut other = read.clone();
        let mut model_style = (**other.styles.get(at).unwrap()).clone();
        model_style.name_u = Some(std::sync::Arc::from("Other"));
        other.styles = diacraft_doc::SharedVec::from_vec(
            other.styles.iter().take(at).cloned().chain(std::iter::once(std::sync::Arc::new(model_style))).collect(),
        );
        let (done, work) = check(&other, &[]);
        assert!(done.is_ok() && work.read_back_bytes == 0, "no style added, no read back");
        // Each difference alone.
        let differ = |change: &dyn Fn(&mut Style)| {
            let mut model = read.clone();
            let mut style = (**model.styles.get(at).unwrap()).clone();
            change(&mut style);
            model.styles =
                diacraft_doc::SharedVec::from_vec(model.styles.iter().take(at).cloned().chain(std::iter::once(std::sync::Arc::new(style))).collect());
            check(&model, &new).0
        };
        let words = |done: Result<(), WriteError>| match done {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("not a refused save: {other:?}"),
        };
        assert!(words(differ(&|s| s.id += 1)).contains("names and links"), "the ID");
        assert!(words(differ(&|s| s.name = Some(std::sync::Arc::from("Shown")))).contains("names and links"), "the name");
        assert!(words(differ(&|s| s.name_u = Some(std::sync::Arc::from("Other")))).contains("names and links"), "the NameU");
        assert!(
            words(differ(&|s| std::sync::Arc::make_mut(&mut s.sheet).links = diacraft_doc::Links { line: Some(1), fill: Some(1), text: Some(1) }))
                .contains("names and links"),
            "the links"
        );
        assert!(
            words(differ(&|s| {
                std::sync::Arc::make_mut(&mut s.sheet)
                    .cells
                    .insert(diacraft_doc::CellName::new("Tag"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("t")));
            }))
            .contains("the cells the model has"),
            "the sheet"
        );
        // A model with another number of style sheets than the file.
        let mut more = read.clone();
        more.styles.push(std::sync::Arc::new(Style { id: u32::try_from(more.styles.len()).unwrap(), ..Style::default() }));
        assert!(words(check(&more, &new).0).contains("another number of style sheets"));
        // A style the model has that the file does not (the ID past the end).
        let past = vec![NewStyle {
            id: u32::try_from(at + 5).unwrap(),
            place: u32::try_from(at + 5).unwrap(),
            name: "X".to_owned(),
            display: None,
            parent: None,
        }];
        assert!(words(check(&read, &past).0).contains("is not read again"));
    }

    /// The links of the shapes whose parent a save wrote are read back from the file made, by the element the index the save made gives each shape: a shape that reads back
    /// with the links the model has passes, and each way it does not fails alone (other links, a shape with no element in the index, a page the file does not read).
    #[test]
    fn the_links_of_a_shape_whose_parent_was_written_are_read_back_and_each_difference_fails_alone() {
        let limits = crate::Limits::default();
        let bytes = package_with(
            r#"<style:style style:name="Theme" style:family="graphic"/>"#,
            r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Theme"/>"#,
            r#"<draw:custom-shape draw:style-name="gr1" draw:name="s" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
        );
        let (read, report) = crate::read::read(&bytes, &limits).unwrap();
        let id = read.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).unwrap();
        let theme = u32::try_from(read.styles.len() - 1).unwrap();
        let on = |page: usize, id: u32, link: u32| Expect {
            plan: None,
            links: vec![(page, id, Links { line: Some(link), fill: Some(link), text: Some(link) })],
        };
        let run = |expect: &Expect<'_>| {
            let mut work = WriteWork::default();
            let done = check(&bytes, &read, expect, &report.index, &limits, &mut work);
            (done, work)
        };
        let (done, work) = run(&on(0, id, theme));
        assert!(done.is_ok(), "the links the file gives are the model's: {done:?}");
        assert_eq!(work.read_back_bytes, bytes.len() as u64);
        let words = |expect: &Expect<'_>| match run(expect).0 {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("not a refused save: {other:?}"),
        };
        assert!(words(&on(0, id, 0)).contains("links the model has"), "other links than the file's");
        assert!(words(&on(0, id + 40, theme)).contains("has no element"), "a shape the index does not hold");
        assert!(words(&on(7, id, theme)).contains("is not read again"), "a page the file does not read");
    }

    /// The read back of the style sheets and of the shapes' parents reads the file made again when it is of the most the read back reads, and leaves one byte more
    /// unchecked (the review's concern: a themed drawing writes a parent for every shape put on it, so this read is made on most saves of one).
    #[test]
    fn a_package_of_more_than_the_most_the_read_back_reads_is_not_read_again_for_its_style_sheets_or_parents_and_one_of_that_many_is() {
        let limits = crate::Limits::default();
        let links = vec![(0usize, 1u32, Links { line: Some(1), fill: Some(1), text: Some(1) })];
        let expect = Expect { plan: None, links };
        let doc = Document::empty();
        let mut work = WriteWork::default();
        assert!(check(&vec![0u8; super::super::MOST_READ_BACK + 1], &doc, &expect, &ShapeIndex::default(), &limits, &mut work).is_ok());
        assert_eq!(work.read_back_bytes, 0, "one byte more is not read");
        let mut work = WriteWork::default();
        let refused = check(&vec![0u8; super::super::MOST_READ_BACK], &doc, &expect, &ShapeIndex::default(), &limits, &mut work);
        assert!(matches!(&refused, Err(WriteError::SelfCheck(words)) if words.contains("does not read again")), "{refused:?}");
        assert_eq!(work.read_back_bytes, super::super::MOST_READ_BACK as u64, "the most is read, and charged");
        // Nothing to check, nothing read, whatever the size.
        let mut work = WriteWork::default();
        assert!(check(&[0u8; 8], &doc, &Expect::default(), &ShapeIndex::default(), &limits, &mut work).is_ok());
        assert_eq!(work.read_back_bytes, 0);
    }

    /// The names the shapes' parents are written with: the file's by the name it has them under, a new one by the spec's rule.
    #[test]
    fn the_names_of_the_styles_by_their_ids_are_the_files_and() {
        let base = base();
        let mut all: Vec<std::sync::Arc<Style>> = base.styles.iter().cloned().collect();
        all.push(style(3, "My Theme", 0));
        let names =
            names_by_id(&document(all), &base, &[], &[NewStyle { id: 3, place: 3, name: "My_20_Theme".to_owned(), display: None, parent: None }]);
        let mut listed: Vec<(u32, String)> = names.into_iter().collect();
        listed.sort();
        assert_eq!(listed, [(0, "No Style".to_owned()), (1, "Standard".to_owned()), (2, "Heading".to_owned()), (3, "My_20_Theme".to_owned())]);
    }
}
