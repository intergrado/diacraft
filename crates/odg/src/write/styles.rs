//! The automatic styles of a save: a
//! restyle writes into the shape's own automatic style, **cloning it first when other elements use it**, makes a new one when the shape names a
//! common style or none, takes out the styles no element names after the save, and renames nothing.
//!
//! * **What names a style** is found by one pass of the part ([`count_refs`]): every attribute whose value is the name of an automatic style, but
//!   the `style:name` of its own definition. That is every attribute of every vocabulary that could name a style (`draw:style-name`,
//!   `draw:text-style-name`, `text:style-name`, `style:parent-style-name`, `presentation:style-name`, ...), and a coincidence of values counts too:
//!   a style that a stray value keeps is kept, and a style that a stray value makes look shared is cloned, which are the safe errors.
//! * **A shared style is cloned**: the shape's attribute names a copy under the next free name ([`Names`]), the copy is put after the
//!   original, and the edit is made in the copy; the others keep the original, whose references fall by one. A style the shape alone names is
//!   edited where it is.
//! * **A shape on a common style, or on none** gets a new automatic style whose parent is that common style (none for none), which sets
//!   only the edited properties, put at the end of `office:automatic-styles`.
//! * **New names**: the prefix most of the file's automatic styles of the family use, and one more than the largest number any of
//!   them has with it in either part (`content.xml` and `styles.xml`). Nothing is renumbered.
//! * **Orphans** (delete): a style that something named before the save and nothing names after it is taken out, whether a delete or a
//!   restyle cut its last reference. A style nothing named in the file as it was is the file's own and is not touched.
//!
//! **Cost**: the references are found in one pass and kept sorted, a delete takes the references inside the unit it removes by two searches, a
//! clone costs the style's own bytes, and the orphans are found in one walk of the styles: linear in the part, counted ([`WriteWork`]).

use std::collections::{HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::xml_scan::{AttrSpan, TagKind, scan_tags, tag_attributes};

use super::attrs::TagEdit;
use super::emit::Pfx;
use super::frame::Editor;
use super::lengths::{length_text, same};
use super::scan::{AutoStyles, Fam, ScanShape, Scopes, StyleDef};
use super::splice::Splicer;
use super::stylemap::{GRAPHIC, Op, StyleEdit, Val, ops_for};
use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::names::Ns;
use crate::read::units::{LenUnit, length};

/// The most references to automatic styles a pass keeps.
const MOST_REFERENCES: usize = 8_000_000;

/// A new style the save made, for the check of the bytes made: its name, its family and the attributes written on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Written {
    pub name: String,
    pub family: Fam,
    pub attrs: Vec<(Ns, &'static str)>,
    /// The properties element the attributes are on: 0 graphic, 1 paragraph, 2 text.
    pub prop: usize,
}

/// The prefix and numbers new names of one family are made from.
#[derive(Clone, Debug)]
struct Series {
    prefix: String,
    max: u64,
}

/// The names of the automatic styles of both parts, and what a new name is made from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    taken: HashSet<String>,
    series: HashMap<u8, Series>,
}

fn family_key(f: Fam) -> u8 {
    match f {
        Fam::Graphic => 0,
        Fam::Paragraph => 1,
        Fam::Text => 2,
        Fam::DrawingPage => 3,
        Fam::Other => 4,
        Fam::Layout => 5,
        Fam::List => 6,
    }
}

/// The prefix LibreOffice gives a family's automatic styles, and `T` for the text styles it was not seen to make.
fn default_prefix(f: Fam) -> &'static str {
    match f {
        Fam::Graphic => "gr",
        Fam::Paragraph => "P",
        Fam::Text => "T",
        Fam::DrawingPage => "dp",
        Fam::Other => "S",
        Fam::Layout => "PM",
        Fam::List => "L",
    }
}

/// A name split into what comes before its trailing digits and the number they make, when it ends in digits that fit.
fn split_number(name: &str) -> Option<(&str, u64)> {
    let digits = name.bytes().rev().take_while(u8::is_ascii_digit).count();
    let at = name.len().checked_sub(digits).filter(|_| digits > 0 && digits <= 18)?;
    Some((name.get(..at)?, name.get(at..)?.parse().ok()?))
}

impl Names {
    /// The names of `defs`, of the automatic styles of one part: kept for the numbering of new ones.
    pub(crate) fn add_all<'n>(&mut self, names: impl Iterator<Item = (Fam, &'n str)>) {
        let mut counts: HashMap<(u8, String), u64> = HashMap::new();
        let mut maxes: HashMap<(u8, String), u64> = HashMap::new();
        for (family, name) in names {
            self.taken.insert(name.to_owned());
            if let Some((prefix, n)) = split_number(name) {
                let key = (family_key(family), prefix.to_owned());
                *counts.entry(key.clone()).or_insert(0) += 1;
                let m = maxes.entry(key).or_insert(0);
                *m = (*m).max(n);
            }
        }
        // The prefix most used per family (the least, by text, among equals, so that the same file gives the same name), and the largest
        // number of that prefix.
        for key in 0..=6u8 {
            let best = counts.iter().filter(|((k, _), _)| *k == key).max_by(|(a, x), (b, y)| x.cmp(y).then_with(|| b.1.cmp(&a.1)));
            if let Some(((_, prefix), _)) = best {
                let max = maxes.get(&(key, prefix.clone())).copied().unwrap_or(0);
                let slot = self.series.entry(key).or_insert_with(|| Series { prefix: prefix.clone(), max });
                if slot.prefix == *prefix {
                    slot.max = slot.max.max(max);
                }
            }
        }
    }

    /// Takes `names` out of what [`Names::next`] gives, and out of every series (the names of the common styles of the file).
    pub(crate) fn avoid(&mut self, names: &[String]) {
        self.taken.extend(names.iter().cloned());
    }

    /// The prefixes new names are made with: those of the series the file has, and LibreOffice's.
    pub(crate) fn prefixes(&self) -> HashSet<String> {
        let mut out: HashSet<String> = self.series.values().map(|s| s.prefix.clone()).collect();
        out.extend(
            [Fam::Graphic, Fam::Paragraph, Fam::Text, Fam::DrawingPage, Fam::Other, Fam::Layout, Fam::List].map(|f| default_prefix(f).to_owned()),
        );
        out
    }

    /// A new name for a style of `family`: its prefix and one more than the largest number in use, and one no style of either part has.
    pub(crate) fn next(&mut self, family: Fam) -> String {
        let key = family_key(family);
        let series = self.series.entry(key).or_insert_with(|| Series { prefix: default_prefix(family).to_owned(), max: 0 });
        loop {
            series.max = series.max.saturating_add(1);
            let name = format!("{}{}", series.prefix, series.max);
            if self.taken.insert(name.clone()) {
                return name;
            }
        }
    }
}

/// The references of a part to its automatic styles: how many name each, and where, by place.
type Refs = (Vec<u32>, Vec<(usize, u32)>, HashSet<String>);

/// The references to automatic styles in a part: how many names each of `defs`, and where, sorted by place (the offset of the value).
pub(crate) fn count_refs(
    bytes: &[u8],
    defs: &[StyleDef],
    prefixes: &HashSet<String>,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Refs, WriteError> {
    work.charge_scan(bytes.len(), limits)?;
    let mut by_name: HashMap<&str, Vec<u32>> = HashMap::new();
    by_name.try_reserve(defs.len()).map_err(|_| starved("the automatic styles of a part", defs.len()))?;
    for (i, d) in defs.iter().enumerate() {
        by_name.entry(d.name.as_str()).or_default().push(u32::try_from(i).unwrap_or(u32::MAX));
    }
    let longest = defs.iter().map(|d| d.name.len()).max().unwrap_or(0);
    // Names of the form a new style's name takes (a prefix of a series and a number) that some attribute holds, whatever style it names or none: a
    // new name is never one of them, or the elements that name it, a style the file lacks, would come to name the new style (the fuzz target's second
    // finding: two shapes on a style `gr4` the file did not define, and a new style given that name).
    let mut dangling: HashSet<String> = HashSet::new();
    let mut total = vec![0u32; defs.len()];
    let mut refs: Vec<(usize, u32)> = Vec::new();
    let mut spans: Vec<AttrSpan> = Vec::new();
    let mut container = Work::default();
    let result = scan_tags(bytes, &limits.container.xml, &mut container, |tag| -> Result<(), WriteError> {
        if tag.kind == TagKind::Close {
            return Ok(());
        }
        spans.clear();
        work.charge_attribute_bytes(tag.end - tag.start)?;
        if tag_attributes(bytes, tag.start, tag.end, &mut spans).is_none() {
            return Err(WriteError::Mismatch("a start tag the writer cannot read the attributes of"));
        }
        for a in &spans {
            let Some(raw) = bytes.get(a.value.0..a.value.1) else { continue };
            if bytes.get(a.name.0..a.name.1).is_some_and(|n| n == b"xmlns" || n.starts_with(b"xmlns:")) {
                continue;
            }
            // One rule for every value, whatever its length: a value is looked up as it reads (references resolved), and one longer than any
            // style's name, as written and with no reference in it, cannot be a name and costs nothing but its look. A name over any cap is
            // counted like any other (a cap here counted a long name's references as none, and the style was edited in place while shared or
            // taken out while named).
            let has_reference = raw.contains(&b'&');
            if !has_reference
                && raw.len() <= 64
                && raw.last().is_some_and(u8::is_ascii_digit)
                && let Some((prefix, _)) = std::str::from_utf8(raw).ok().and_then(split_number)
                && prefixes.contains(prefix)
            {
                dangling.try_reserve(1).map_err(|_| starved("the names the part holds", dangling.len()))?;
                dangling.insert(String::from_utf8_lossy(raw).into_owned());
            }
            if !has_reference && raw.len() > longest {
                continue;
            }
            let text = if has_reference { crate::package::unescape(raw) } else { String::from_utf8_lossy(raw).into_owned() };
            let Some(hits) = by_name.get(text.as_str()) else { continue };
            for &i in hits {
                // The `style:name` of the definition is no reference to it.
                if defs.get(i as usize).is_some_and(|d| d.name_value == a.value) {
                    continue;
                }
                if refs.len() >= MOST_REFERENCES {
                    return Err(WriteError::Limit { what: "references to automatic styles in a part", max: MOST_REFERENCES as u64 });
                }
                refs.try_reserve(1).map_err(|_| starved("the references to the automatic styles of a part", refs.len()))?;
                refs.push((a.value.0, i));
                if let Some(slot) = total.get_mut(i as usize) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
        Ok(())
    });
    work.package.scan_events = work.package.scan_events.saturating_add(container.xml_events);
    match result {
        Ok(()) => {}
        Err(diacraft_container::xml_scan::ScanError::Visit(e)) => return Err(e),
        Err(diacraft_container::xml_scan::ScanError::Xml(e)) => {
            return Err(WriteError::Original(crate::OdgError::Xml { part: "content.xml".to_owned(), error: e }));
        }
    }
    refs.sort_unstable();
    Ok((total, refs, dangling))
}

fn starved(what: &'static str, held: usize) -> WriteError {
    WriteError::Limit { what, max: held as u64 }
}

/// The attributes of the graphic properties of a style that no cell reads (namespace, local name, value), and whether that is all the model's kept style of the name holds.
#[derive(Debug, Default)]
pub(crate) struct KeptAttrs {
    pub attrs: Vec<(Ns, String, String)>,
    pub complete: bool,
}

/// What an automatic graphic style written under a held name was made from: the parent, the attributes the cells write and the attributes of DiaCraft's extension.
pub(crate) type StyleInputs = (Option<String>, Vec<Op>, Vec<(String, String)>);

/// The automatic styles of the part being edited, who names each, and what the save has done to them.
pub(crate) struct StyleBook {
    pub defs: Vec<StyleDef>,
    pub auto: Option<AutoStyles>,
    /// A style was put in `office:automatic-styles` that the part did not have: its start tag is written, and its end tag is written by `Editor::finish_styles`.
    opened: bool,
    by_name: HashMap<String, Vec<usize>>,
    /// What named each style in the part as it was, and what names it now (a delete takes references with it, a restyle moves one).
    total: Vec<u32>,
    live: Vec<i64>,
    /// The references by place: where a delete finds those inside the unit it removes.
    refs: Vec<(usize, u32)>,
    removed: Vec<bool>,
    pub names: Names,
    /// The styles the save made.
    pub written: Vec<Written>,
    /// The automatic graphic styles the model writer wrote under the names the file's shapes held: each name with the text of the style written under it, so that
    /// a second shape that names it and has the same style names the one style, and one whose style differs takes a name of its own.
    pub held_styles: HashMap<String, String>,
    /// The names the file's shapes gave the paragraph style of their text (`draw:text-style-name`): the style of each is written once, empty, at the end of the part,
    /// unless the text writer wrote one of that name (`WriteWork::text_names_written`), which says more.
    pub held_paragraphs: std::collections::BTreeSet<String>,
    /// The kept styles of the model's automatic graphic family by name (their place in `OdfDoc::styles`), made when a shape of the file first asks for what its style kept: the attributes of the
    /// graphic properties no cell reads.
    pub remainders: Option<HashMap<String, usize>>,
    /// What each such style kept, made once for a name and shared by every shape that names it.
    pub remainder_cache: HashMap<String, std::sync::Arc<KeptAttrs>>,
    /// The kept styles of the model's automatic paragraph and text families by family (`true` for text) and name (their place in `OdfDoc::styles`), and the namespaces the root of
    /// `content.xml` bound as read: made when the first such style written under its own name asks for what it kept that no cell reads, charged once.
    pub text_remainders: Option<TextRemainders>,
    /// The inputs the automatic graphic style written under each name the file's shapes held was made from (its parent, the attributes its cells write and DiaCraft's own): a shape that names
    /// it with the same inputs names the style made, and no byte is copied for it.
    pub held_inputs: HashMap<String, StyleInputs>,
    /// What the extension attributes of an automatic style carry, decoded when a shape that names it was first edited, by the style's index: the next shape that
    /// names it takes a copy and does not read the file's attributes and decode them again.
    tier2: HashMap<usize, std::sync::Arc<HeldDia>>,
}

/// The kept text styles by family (`true` for text) and name, and the namespaces the root of `content.xml` bound as read ([`StyleBook::text_remainders`]).
pub(crate) type TextRemainders = (HashMap<(bool, String), usize>, super::kept::Bindings);

/// What an edit gives a shape's style to carry: the attributes of the extension, and what they say (which the check of the bytes made reads the file back against).
type MergedDia = (Vec<(String, String)>, crate::ext::payload::Tier2);

/// What the extension attributes of an automatic style hold: the pairs as the file has them, what they say, and the items the decode of them took.
pub(crate) struct HeldDia {
    pub pairs: Vec<(String, String)>,
    pub tier2: crate::ext::payload::Tier2,
    pub items: u64,
}

impl StyleBook {
    /// The book of a part: its styles, the references found in `bytes`, and the names of the automatic styles of the other part.
    pub(crate) fn build(
        bytes: &[u8],
        defs: Vec<StyleDef>,
        auto: Option<AutoStyles>,
        (others, common): (&[(Fam, String)], &[String]),
        limits: &Limits,
        work: &mut WriteWork,
    ) -> Result<StyleBook, WriteError> {
        let mut names = Names::default();
        names.add_all(defs.iter().map(|d| (d.family, d.name.as_str())).chain(others.iter().map(|(f, n)| (*f, n.as_str()))));
        // The names of the common styles are taken, and count for no series: a new automatic style never has the name of a common one.
        names.avoid(common);
        let (total, refs, dangling) = count_refs(bytes, &defs, &names.prefixes(), limits, work)?;
        names.taken.extend(dangling);
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        by_name.try_reserve(defs.len()).map_err(|_| starved("the automatic styles of a part", defs.len()))?;
        for (i, d) in defs.iter().enumerate() {
            by_name.entry(d.name.clone()).or_default().push(i);
        }
        let live = total.iter().map(|n| i64::from(*n)).collect();
        let removed = vec![false; defs.len()];
        Ok(StyleBook {
            defs,
            auto,
            opened: false,
            by_name,
            total,
            live,
            refs,
            removed,
            names,
            written: Vec::new(),
            held_styles: HashMap::new(),
            held_paragraphs: std::collections::BTreeSet::new(),
            remainders: None,
            remainder_cache: HashMap::new(),
            text_remainders: None,
            held_inputs: HashMap::new(),
            tier2: HashMap::new(),
        })
    }

    /// The automatic graphic style called `name`, when there is exactly one style of that name (a name two families share is no style to edit).
    fn graphic(&self, name: &str) -> Option<usize> {
        match self.by_name.get(name).map(Vec::as_slice) {
            Some([only]) if self.defs.get(*only).is_some_and(|d| d.family == Fam::Graphic) => Some(*only),
            _ => None,
        }
    }

    /// The automatic style of `family` called `name`, by its place among the definitions.
    pub(crate) fn find_def(&self, name: &str, family: Fam) -> Option<usize> {
        self.by_name.get(name)?.iter().copied().find(|i| self.defs.get(*i).is_some_and(|d| d.family == family))
    }

    /// How many things name the style at `i` now: those of the file as it was, less what an edit took out, plus what it put in.
    pub(crate) fn live_refs(&self, i: usize) -> i64 {
        self.live.get(i).copied().unwrap_or(0)
    }

    /// Something names the style at `i` that did not: a style the save found for a paragraph or a run is named once more.
    pub(crate) fn add_reference(&mut self, i: usize) {
        if let Some(slot) = self.live.get_mut(i) {
            *slot += 1;
        }
    }

    /// Something that named the style at `i` does not now.
    pub(crate) fn drop_reference(&mut self, i: usize) {
        if let Some(slot) = self.live.get_mut(i) {
            *slot -= 1;
        }
    }

    /// Whether `name` is the name of some automatic style, of any family.
    fn is_automatic(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// A unit from `from` to `to` is taken out of the part: the references inside it go, and a style nothing then names is an orphan.
    pub(crate) fn unit_removed(&mut self, from: usize, to: usize, work: &mut WriteWork) {
        let _ = self.unit_removed_orphans(from, to, work);
    }

    /// [`StyleBook::unit_removed`], giving the styles that the references inside the unit named and that nothing names now, which something named
    /// in the file as it was and which are not taken out yet: what the unit was the last to name.
    pub(crate) fn unit_removed_orphans(&mut self, from: usize, to: usize, work: &mut WriteWork) -> Result<Vec<usize>, WriteError> {
        let lo = self.refs.partition_point(|(at, _)| *at < from);
        let hi = self.refs.partition_point(|(at, _)| *at < to);
        work.order_steps = work.order_steps.saturating_add(2);
        let mut last: Vec<usize> = Vec::new();
        for &(_, def) in self.refs.get(lo..hi).unwrap_or_default() {
            work.order_steps = work.order_steps.saturating_add(1);
            let i = def as usize;
            let Some(slot) = self.live.get_mut(i) else { continue };
            *slot -= 1;
            if *slot <= 0 && self.total.get(i).is_some_and(|t| *t > 0) && !self.removed.get(i).copied().unwrap_or(true) {
                last.try_reserve(1).map_err(|_| starved("the styles a unit taken out named", last.len()))?;
                last.push(i);
            }
        }
        Ok(last)
    }

    /// The styles to take out: each that something named in the file as it was and nothing names now, not yet removed.
    pub(crate) fn orphans(&self) -> Vec<usize> {
        (0..self.defs.len())
            .filter(|i| {
                self.total.get(*i).is_some_and(|t| *t > 0)
                    && self.live.get(*i).is_some_and(|l| *l <= 0)
                    && !self.removed.get(*i).copied().unwrap_or(true)
            })
            .collect()
    }

    pub(crate) fn mark_removed(&mut self, i: usize) {
        if let Some(slot) = self.removed.get_mut(i) {
            *slot = true;
        }
    }
}

/// The declaration of LibreOffice's extension on the element that holds an attribute of it (`Ns::Other` in `ops`: `fx::shadow_blur_op`), or none.
fn loext_declaration(ops: &[Op]) -> String {
    if ops.iter().any(|o| o.ns == Ns::Other && o.val != Val::Remove) { format!(" xmlns:loext=\"{}\"", super::fx::LOEXT_URI) } else { String::new() }
}

/// The attributes of a restyle as `Op`s, by the properties element they go on: the edit's cells in the table's order.
fn ops_of(edit: &StyleEdit) -> Result<Vec<Op>, WriteError> {
    let mut ops = Vec::new();
    for (cell, value) in &edit.changes {
        ops.extend(ops_for(cell, value.as_ref(), edit).map_err(WriteError::Unsupported)?);
    }
    // The display form of the fill, after the cells' own attributes: what it writes for an attribute takes the place of theirs.
    let said = edit.fx_ops();
    ops.retain(|o| !said.iter().any(|s| s.prop == o.prop && s.ns == o.ns && s.local == o.local));
    ops.extend(said);
    Ok(ops)
}

/// What writing the attributes of a properties element needs of the editor, borrowed field by field so that the part's own splicer can be the
/// one written to.
pub(crate) struct Ctx<'x> {
    pub(crate) scopes: &'x mut Scopes,
    pub(crate) work: &'x mut WriteWork,
    pub(crate) limits: &'x Limits,
    pub(crate) unit: LenUnit,
}

impl Ctx<'_> {
    fn charge_one(&mut self) -> Result<(), WriteError> {
        self.work.charge_attribute(self.limits)
    }

    /// Writes `ops` on the tag of a properties element: each attribute in its place and its unit when it is there, added when it is not, taken
    /// out when the op says so and it is there. What it writes is noted in `wrote`.
    pub(crate) fn write_ops(
        &mut self,
        tag: &mut TagEdit<'_>,
        splicer: &mut Splicer<'_>,
        ops: &[Op],
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<(), WriteError> {
        for o in ops {
            let at = tag.find(self.scopes, o.ns, o.local, self.work);
            match (&o.val, at) {
                (Val::Remove, Some(at)) => {
                    self.charge_one()?;
                    tag.remove(splicer, at)?;
                }
                (Val::Remove, None) => {}
                (Val::Text(text), Some(at)) => {
                    if tag.text(at) != *text {
                        self.charge_one()?;
                        tag.set(splicer, at, text)?;
                    }
                    wrote.push((o.ns, o.local));
                }
                (Val::Text(text), None) => {
                    self.charge_one()?;
                    tag.add(splicer, self.scopes, self.work, o.ns, o.local, text)?;
                    wrote.push((o.ns, o.local));
                }
                (Val::Length(inches), held) => {
                    let existing = held.and_then(|at| length(&tag.text(at)).map(|l| (at, l)));
                    let words = WriteError::Unsupported("a length too large to write");
                    match (held, existing) {
                        (Some(_), Some((_, l))) if same(l.inches, *inches) => {}
                        (Some(_), Some((at, l))) => {
                            self.charge_one()?;
                            tag.set(splicer, at, &length_text(*inches, l.unit).ok_or(words)?)?;
                        }
                        // There, and no length: replaced in its place, in the page's unit, and never added twice.
                        (Some(at), None) => {
                            self.charge_one()?;
                            tag.set(splicer, at, &length_text(*inches, self.unit).ok_or(words)?)?;
                        }
                        (None, _) => {
                            self.charge_one()?;
                            let text = length_text(*inches, self.unit).ok_or(words)?;
                            tag.add(splicer, self.scopes, self.work, o.ns, o.local, &text)?;
                        }
                    }
                    wrote.push((o.ns, o.local));
                }
            }
        }
        Ok(())
    }

    /// The attributes `ops` write as text under the prefixes `pfx` gives, the removals left out: ` prefix:local="value"` each.
    pub(crate) fn attributes(&mut self, pfx: &Pfx, ops: &[Op], wrote: &mut Vec<(Ns, &'static str)>) -> Result<String, WriteError> {
        let mut text = String::new();
        for o in ops {
            let value = match &o.val {
                Val::Remove => continue,
                Val::Text(t) => t.clone(),
                Val::Length(inches) => length_text(*inches, self.unit).ok_or(WriteError::Unsupported("a length too large to write"))?,
            };
            self.charge_one()?;
            text.push_str(&pfx.attr(o.ns, o.local, &value));
            wrote.push((o.ns, o.local));
        }
        Ok(text)
    }

    /// The text of a new `style:graphic-properties` element holding what `ops` write and the attributes of the extension in `ext` (its declaration, if the place
    /// binds no prefix to it, and the attributes), put where `scope`'s namespaces are in force.
    fn props_element(&mut self, scope: u32, ops: &[Op], ext: &(String, String), wrote: &mut Vec<(Ns, &'static str)>) -> Result<String, WriteError> {
        let pfx = Pfx::of(self.scopes, scope, self.work);
        let attrs = self.attributes(&pfx, ops, wrote)?;
        let loext = loext_declaration(ops);
        Ok(format!("<{}:graphic-properties{}{loext}{}{attrs}{}/>", pfx.style, pfx.declare, ext.0, ext.1))
    }

    /// Makes the attributes of DiaCraft's extension that the writer owns on a properties tag exactly `attrs`, as local names and values: those the tag has that
    /// `attrs` lacks are taken out, those that differ are set, those it lacks are added, and `version` is `1` while any attribute of the extension is on the
    /// tag, and goes with the last. The names the writer does not own (`links`, `filled`, and a newer version's) are left as they are.
    pub(crate) fn write_dia(&mut self, tag: &mut TagEdit<'_>, splicer: &mut Splicer<'_>, attrs: &[(String, String)]) -> Result<(), WriteError> {
        use std::collections::{HashMap, HashSet};
        let held = tag.in_namespace(self.scopes, Ns::Diacraft, self.work);
        // The wanted attributes by name and the names the tag holds, each made once (the first of a name is the one a search finds), so that neither list is
        // searched for each of the other's: the work is the two lists, charged before it is done (the security review).
        self.work.charge_steps(held.len().saturating_add(attrs.len()))?;
        let mut wanted: HashMap<&str, &String> = HashMap::new();
        for (l, v) in attrs {
            wanted.entry(l.as_str()).or_insert(v);
        }
        let present: HashSet<&str> = held.iter().map(|(_, l)| l.as_str()).collect();
        let mut foreign = false;
        let mut version_at: Option<usize> = None;
        for (at, local) in &held {
            if local == crate::ext::names::VERSION_ATTR {
                version_at = Some(*at);
            } else if !crate::ext::names::owned(local) {
                foreign = true;
            } else {
                match wanted.get(local.as_str()) {
                    Some(value) => {
                        if tag.text(*at) != **value {
                            self.charge_one()?;
                            tag.set(splicer, *at, value)?;
                        }
                    }
                    None => {
                        self.charge_one()?;
                        tag.remove(splicer, *at)?;
                    }
                }
            }
        }
        let anything = foreign || !attrs.is_empty();
        match (anything, version_at) {
            (true, None) => {
                self.charge_one()?;
                tag.add(splicer, self.scopes, self.work, Ns::Diacraft, crate::ext::names::VERSION_ATTR, crate::ext::names::VERSION)?;
            }
            (false, Some(at)) => {
                self.charge_one()?;
                tag.remove(splicer, at)?;
            }
            _ => {}
        }
        for (local, value) in attrs {
            if !present.contains(local.as_str()) {
                self.charge_one()?;
                tag.add(splicer, self.scopes, self.work, Ns::Diacraft, local, value)?;
            }
        }
        Ok(())
    }
}

impl Editor<'_, '_> {
    pub(crate) fn ctx(&mut self) -> Ctx<'_> {
        Ctx { scopes: &mut self.scopes, work: &mut *self.work, limits: self.limits, unit: self.unit }
    }

    fn book_mut(&mut self) -> Result<&mut StyleBook, WriteError> {
        self.book.as_mut().ok_or(WriteError::Mismatch("a restyle without the styles of the part"))
    }

    fn def_of(&self, i: usize) -> Result<StyleDef, WriteError> {
        self.book.as_ref().and_then(|b| b.defs.get(i)).cloned().ok_or(WriteError::Mismatch("a style that is not in the part"))
    }

    /// Restyles the shape whose element is `shape` (see the module documentation).
    ///
    /// `grow`: the shape is a custom shape that holds text after a retext, and its style gets `draw:auto-grow-height="false"` when it has none, so
    /// that LibreOffice keeps the frame the model holds and does not fit it to the text (R48 of the shapes document, and the retext row of R9 in the
    /// ownership one). It goes into the style by the same rules as any other attribute: where the shape alone names the style, in the style; in a
    /// copy where others name it too; in a new style on the shape's common style or on none.
    pub(crate) fn restyle(
        &mut self,
        shape: &ScanShape,
        edit: &StyleEdit,
        grow: bool,
        tier2: Option<&super::own::Tier2Edit>,
    ) -> Result<Option<std::sync::Arc<crate::ext::payload::Tier2>>, WriteError> {
        let mut ops = ops_of(edit)?;
        // The parent the shape's automatic style names after the edit, when its links changed: the name of the style sheet the links name, or none for the root.
        let parent: Option<Option<String>> = match edit.parent {
            None => None,
            Some(0) => Some(None),
            Some(id) => Some(Some(
                self.style_names
                    .get(&id)
                    .cloned()
                    .ok_or(WriteError::Unsupported("a shape's links naming a style sheet whose name the file cannot say"))?,
            )),
        };
        let mut tag = TagEdit::new(self.bytes, shape.open, shape.scope, self.work)?;
        let at = tag.find(&mut self.scopes, Ns::Draw, "style-name", self.work);
        let current = at.map(|a| tag.text(a));
        let (held, live) = {
            let book = self.book.as_ref().ok_or(WriteError::Mismatch("a restyle without the styles of the part"))?;
            let held = current.as_deref().and_then(|n| book.graphic(n));
            if let Some(name) = current.as_deref()
                && held.is_none()
                && book.is_automatic(name)
            {
                return Err(WriteError::Unsupported("a shape whose style names an automatic style that is not one of the graphic family"));
            }
            (held, held.and_then(|i| book.live.get(i).copied()).unwrap_or(0))
        };
        // What DiaCraft's extension carries of the shape after this edit, when it is not what the style carries now.
        let merged = match tier2 {
            Some(t) => self.merged_dia(held, t)?,
            None => None,
        };
        let (dia, carried) = match merged {
            Some((attrs, carried)) => (Some(attrs), Some(std::sync::Arc::new(carried))),
            None => (None, None),
        };
        // A change of the line's weight changes the width of a marker of DiaCraft's own that the style names (the row: the width is the spread times `S`, which has the
        // weight in it); a marker of another make, or one the style does not name itself, keeps the width the file gives it.
        if let Some(i) = held {
            let widths = self.marker_widths_after_weight(i, edit)?;
            ops.extend(widths);
        }
        // The markers the style names after the edit (those of the cells that changed, and those the weight wrote again) are the save's to put in `office:styles`.
        self.note_markers(&ops);
        // The style says already whether the frame grows with its text, or it does not: only an attribute of its own counts ("has none").
        let has_grow = match held {
            Some(i) => {
                let def = self.def_of(i)?;
                match def.props.first().copied().flatten() {
                    Some(props) => {
                        let style_tag = TagEdit::untracked(self.bytes, props.open, props.scope, self.work)?;
                        style_tag.find(&mut self.scopes, Ns::Draw, "auto-grow-height", self.work).is_some()
                    }
                    None => false,
                }
            }
            None => false,
        };
        if grow && !has_grow {
            ops.push(Op { prop: GRAPHIC, ns: Ns::Draw, local: "auto-grow-height", val: Val::Text("false".to_owned()) });
        } else if edit.is_empty() && dia.is_none() {
            return Ok(None);
        }
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        match held {
            // The shape alone names the style: it is edited where it stands.
            Some(i) if live <= 1 => {
                let def = self.def_of(i)?;
                // What was decoded of the style stays in the book: a decode reads the attributes **the file has** (`owned_dia` reads the part as it was, and the splice
                // holds every edit apart from it), so the style as it will be is never what a second decode would see, and an entry for it is what a fresh decode
                // gives (S1: the statement that took the entry out here changed nothing and no test could tell it, so it is gone).
                self.edit_in_place(&def, &ops, dia.as_deref())?;
                self.set_parent(&def, parent.as_ref())?;
            }
            // Others name it too: the shape gets a copy, put after the original.
            Some(i) => {
                let def = self.def_of(i)?;
                // The copy holds the whole style: charged against what the reader reads of one part before it is made.
                let items = self.style_items(&def)?.saturating_add(ops.len() as u64);
                self.work.charge_tree_items(items, self.limits)?;
                let book = self.book_mut()?;
                let name = book.names.next(Fam::Graphic);
                if let Some(slot) = book.live.get_mut(i) {
                    *slot -= 1;
                }
                let copy = self.clone_style(&def, &name, &ops, dia.as_deref(), parent.as_ref(), &mut wrote)?;
                self.splicer.insert(def.end, copy);
                self.name_style(&mut tag, at, &name)?;
                self.noted(&name, wrote)?;
            }
            // A common style, or none: a new automatic style that names it as its parent, put at the end of `office:automatic-styles`.
            None => {
                let name = self.book_mut()?.names.next(Fam::Graphic);
                // The parent is the common style the shape names, or the one its links name now.
                let parent_name = match &parent {
                    Some(now) => now.as_deref(),
                    None => current.as_deref(),
                };
                let text = self.new_style(&name, parent_name, &ops, dia.as_deref().unwrap_or(&[]), &mut wrote)?;
                self.put_style(text)?;
                self.name_style(&mut tag, at, &name)?;
                self.noted(&name, wrote)?;
            }
        }
        Ok(carried)
    }

    /// The attributes of DiaCraft's extension the writer owns on the properties of the automatic style at `held`, as local names and values.
    fn owned_dia(&mut self, held: Option<usize>) -> Result<Vec<(String, String)>, WriteError> {
        let Some(i) = held else { return Ok(Vec::new()) };
        let def = self.def_of(i)?;
        let Some(props) = def.props.first().copied().flatten() else { return Ok(Vec::new()) };
        let tag = TagEdit::untracked(self.bytes, props.open, props.scope, self.work)?;
        let held = tag.in_namespace(&mut self.scopes, Ns::Diacraft, self.work);
        Ok(held
            .into_iter()
            .filter(|(_, local)| crate::ext::names::owned(local) && local != crate::ext::names::VERSION_ATTR)
            .map(|(at, local)| (local, tag.text(at)))
            .collect())
    }

    /// What the extension attributes of the style at `held` carry, decoded once for the whole save: a style that many shapes name is read from the file and decoded
    /// the first time, and each use of it after that takes a copy (the security review). Every use, the first included, is charged what the decode took,
    /// against the one budget of items the save's decodes share ([`WriteWork::charge_tier2`]), and the decode is given what is left of that budget.
    fn held_dia(&mut self, held: Option<usize>) -> Result<std::sync::Arc<HeldDia>, WriteError> {
        if let Some(i) = held
            && let Some(hit) = self.book.as_ref().and_then(|b| b.tier2.get(&i)).map(std::sync::Arc::clone)
        {
            self.work.charge_tier2(hit.items, self.limits)?;
            return Ok(hit);
        }
        let pairs = self.owned_dia(held)?;
        let (tier2, items) = if pairs.is_empty() {
            (crate::ext::payload::Tier2::default(), 0)
        } else {
            self.work.tier2_decodes = self.work.tier2_decodes.saturating_add(1);
            let room = self.work.tier2_room(self.limits);
            crate::ext::payload::decode_free(self.limits, room, &pairs).map_err(WriteError::Original)?
        };
        self.work.charge_tier2(items, self.limits)?;
        let made = std::sync::Arc::new(HeldDia { pairs, tier2, items });
        if let Some(i) = held
            && let Some(book) = self.book.as_mut()
        {
            book.tier2.insert(i, std::sync::Arc::clone(&made));
        }
        Ok(made)
    }

    /// What DiaCraft's extension carries of a shape after an edit, when it is not what the shape's style carries now: the tier 2 the style holds, refreshed from
    /// the model (a carried pose, text, name or link follows the model: a move of a shape whose pin is not its centre, or a retext of a carried text, would
    /// otherwise be read back as it was), with the cells, sections and removals the edit adds. `None` when nothing is to be written.
    ///
    /// The work is linear in what the style, the model and the edit hold: the sections of each are indexed by their keys once and looked up in the index, the
    /// removals are taken out in one pass over a set, and every loop is charged to the save's steps before it runs (the security review: a `find` or an `any` in a
    /// loop was a pass for each turn of it, and was not counted).
    fn merged_dia(&mut self, held: Option<usize>, t: &super::own::Tier2Edit) -> Result<Option<MergedDia>, WriteError> {
        // A copy holds none of what its style carries: that is the data of the shape it is a copy of, and its `ID`. What it is given is its own: the edit's
        // additions and its own `ID`, in a style of its own (or none, when it adds nothing, so that it no longer repeats the other shape's `ID`).
        let was = if t.copy {
            std::sync::Arc::new(HeldDia { pairs: self.owned_dia(held)?, tier2: crate::ext::payload::Tier2::default(), items: 0 })
        } else {
            self.held_dia(held)?
        };
        if was.pairs.is_empty() && t.delta.is_empty() {
            return Ok(None);
        }
        let mut t1 = was.tier2.clone();
        // What tier 3 found stale when the file was read is left out of what the style will carry: the model holds the markup's own reading of it.
        crate::ext::tier3::drop_stale(&mut t1, &t.stale);
        let (m, d) = (&*t.model, &t.delta);
        // A link to a master that went stale is not written, and the shape's own copies of what it inherited are: they are in no markup.
        if t.stale.contains(&crate::ext::fingerprint::Key::Master) {
            self.work.charge_steps(m.sheet.sections.len())?;
            crate::ext::tier3::keep_inherited(&mut t1, m);
        }
        // What the style carries follows the model.
        self.work.charge_steps(t1.cells.len().saturating_add(t1.sections.len()).saturating_add(m.sheet.sections.len()))?;
        follow_model(&mut t1, m);
        // What the edit adds.
        let (put, taken) = (d.cells.len().saturating_add(d.removed_cells.len()), d.sections.len().saturating_add(d.removed_sections.len()));
        self.work.charge_steps(put.saturating_add(taken).saturating_add(t1.sections.len()).saturating_add(t1.gone.len().saturating_mul(2)))?;
        add_edit(&mut t1, d);
        follow_fields(&mut t1, m, d.fields);
        self.work.charge_steps(t1.gone.len())?;
        t1.gone.sort_by_cached_key(crate::ext::names::Gone::token);
        t1.gone.dedup();
        give_id(&mut t1, t.id);
        // The fingerprints of tier 3 follow the items and the outputs: the file's own where the save leaves an output as it is (a missing one is stamped), made again
        // where the save writes the output from the model, taken out where what it writes is not known.
        let outline = self.outline_now(m, t)?;
        crate::ext::tier3::restamp(&mut t1, &was.tier2, t.natives.as_ref(), outline);
        let mut attrs = match crate::ext::payload::encode(&t1) {
            Ok(Some(attrs)) => attrs,
            Ok(None) => return Err(WriteError::Unsupported(super::splice::WORDS_XML_CHAR)),
            Err(words) => return Err(WriteError::Unsupported(words)),
        };
        self.work.charge_steps(was.pairs.len().saturating_add(attrs.len()))?;
        let mut before = was.pairs.clone();
        before.sort();
        attrs.sort();
        // (Sorted to be compared; written in the order the extension makes them.)
        if before == attrs {
            return Ok(None);
        }
        Ok(crate::ext::payload::encode(&t1).ok().flatten().map(|written| (written, t1)))
    }

    /// What the save does to the native outline the fingerprints of the outline stand for: nothing, the path of a custom shape (the text the writer makes from the
    /// model's sheet, whose fingerprint is made here), or something whose output is not known here.
    fn outline_now(&mut self, model: &diacraft_doc::Shape, t: &super::own::Tier2Edit) -> Result<crate::ext::tier3::OutlineNow, WriteError> {
        use crate::ext::tier3::OutlineNow;
        match t.outline {
            super::own::OutlineWrite::No => Ok(OutlineNow::File),
            super::own::OutlineWrite::Other => Ok(OutlineNow::Unknown),
            super::own::OutlineWrite::Path => {
                // Only a custom shape's path is made from the sheet by this function; any other kind of element is written in another way.
                if !t.natives.as_ref().and_then(|n| n.outline).is_some_and(|o| o.kind == crate::ext::fingerprint::OutlineKind::Custom) {
                    return Ok(OutlineNow::Unknown);
                }
                self.work.charge_steps(model.sheet.sections.len())?;
                match super::geometry::enhanced_path(&model.sheet) {
                    Ok(text) => {
                        self.work.charge_copy(text.len(), self.limits)?;
                        Ok(OutlineNow::Path(crate::ext::fingerprint::path_part(&text)))
                    }
                    Err(_) => Ok(OutlineNow::Unknown),
                }
            }
        }
    }

    /// The names and widths of the markers of DiaCraft's own that the automatic style at `i` names (`draw:marker-start`, `-end`), written again for the line weight
    /// `edit` sets, for each end whose own cells the edit does not change (those are written by their own ops): the band class, and so the marker, follows the weight.
    fn marker_widths_after_weight(&mut self, i: usize, edit: &StyleEdit) -> Result<Vec<Op>, WriteError> {
        let mut out: Vec<Op> = Vec::new();
        if !edit.changes.iter().any(|(c, _)| *c == "LineWeight") {
            return Ok(out);
        }
        let def = self.def_of(i)?;
        let Some(props) = def.props.first().copied().flatten() else { return Ok(out) };
        let tag = TagEdit::untracked(self.bytes, props.open, props.scope, self.work)?;
        let line = |name: &str| edit.line.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        for (arrow, size, marker, width) in
            [("BeginArrow", "BeginArrowSize", "marker-start", "marker-start-width"), ("EndArrow", "EndArrowSize", "marker-end", "marker-end-width")]
        {
            if edit.changes.iter().any(|(c, _)| *c == arrow || *c == size) {
                continue;
            }
            let Some(at) = tag.find(&mut self.scopes, Ns::Draw, marker, self.work) else { continue };
            let Some(own) = super::markers::key_of(&tag.text(at)) else { continue };
            // The weight and the size class are asked for only here, where a marker of DiaCraft's own is named: a weight no end can be written on is no fault of a
            // line that has none. A refusal is returned: a marker left with the name and the width of the old weight would read back as another band.
            let weight_pt = super::markers::weight_pt(line("LineWeight")).map_err(WriteError::Unsupported)?;
            let class = super::markers::class_of_cell(line(size)).map_err(WriteError::Unsupported)?;
            // The band class changes with the weight, so the marker is named again with its width (the marker is the number and the band class).
            let plan = super::markers::plan(own.number, class, weight_pt).map_err(WriteError::Unsupported)?;
            out.try_reserve(2).map_err(|_| starved("the markers of a style", out.len()))?;
            out.push(Op { prop: GRAPHIC, ns: Ns::Draw, local: marker, val: Val::Text(plan.key.name()) });
            out.push(Op { prop: GRAPHIC, ns: Ns::Draw, local: width, val: Val::Text(super::markers::points_text(plan.width)) });
        }
        Ok(out)
    }

    /// Notes the line ends the `ops` name a marker of DiaCraft's own for: their elements are added to `office:styles` (`markers.rs`).
    pub(crate) fn note_markers(&mut self, ops: &[Op]) {
        for o in ops {
            // The line ends, and the gradient and the hatch a fill names: each of DiaCraft's own name is added to `office:styles` when the file lacks it.
            if let (Ns::Draw, "marker-start" | "marker-end" | "fill-gradient-name" | "fill-hatch-name", Val::Text(t)) = (o.ns, o.local, &o.val)
                && let Some(def) = super::markers::Def::of_name(t)
            {
                self.markers.insert(def);
            }
        }
    }

    /// Records a style the save made: what the check of the bytes made looks for.
    fn noted(&mut self, name: &str, attrs: Vec<(Ns, &'static str)>) -> Result<(), WriteError> {
        self.book_mut()?.written.push(Written { name: name.to_owned(), family: Fam::Graphic, attrs, prop: 0 });
        Ok(())
    }

    /// Points the shape at the style `name`: `draw:style-name` in its place, or added.
    fn name_style(&mut self, tag: &mut TagEdit<'_>, at: Option<usize>, name: &str) -> Result<(), WriteError> {
        self.work.charge_attribute(self.limits)?;
        match at {
            Some(at) => tag.set(&mut self.splicer, at, name),
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "style-name", name),
        }
    }

    /// Makes the parent of the style `def` the one `parent` says (`Some(None)` takes it out): `style:parent-style-name` set, added or removed on its start tag.
    fn set_parent(&mut self, def: &StyleDef, parent: Option<&Option<String>>) -> Result<(), WriteError> {
        let Some(parent) = parent else { return Ok(()) };
        let source = self.bytes;
        let mut tag = TagEdit::untracked(source, def.open, def.scope, self.work)?;
        let mut splicer = std::mem::replace(&mut self.splicer, Splicer::new(&[]));
        let done = self.write_parent(&mut tag, &mut splicer, parent.as_deref());
        self.splicer = splicer;
        done
    }

    /// Writes `style:parent-style-name` on the start tag `tag` as `parent` says: its value set (or left, when it is the same), added, or removed for none.
    fn write_parent(&mut self, tag: &mut TagEdit<'_>, splicer: &mut Splicer<'_>, parent: Option<&str>) -> Result<(), WriteError> {
        let at = tag.find(&mut self.scopes, Ns::Style, "parent-style-name", self.work);
        match (at, parent) {
            (Some(at), Some(name)) => {
                if tag.text(at) != name {
                    self.work.charge_attribute(self.limits)?;
                    tag.set(splicer, at, name)?;
                }
            }
            (Some(at), None) => {
                self.work.charge_attribute(self.limits)?;
                tag.remove(splicer, at)?;
            }
            (None, Some(name)) => {
                self.work.charge_attribute(self.limits)?;
                tag.add(splicer, &mut self.scopes, self.work, Ns::Style, "parent-style-name", name)?;
            }
            (None, None) => {}
        }
        Ok(())
    }

    /// Edits the style `def` where it stands: its graphic properties, or a new element for them when it has none.
    fn edit_in_place(&mut self, def: &StyleDef, ops: &[Op], dia: Option<&[(String, String)]>) -> Result<(), WriteError> {
        let mut wrote = Vec::new();
        let source = self.bytes;
        match def.props.first().copied().flatten() {
            Some(props) => {
                let mut tag = TagEdit::untracked(source, props.open, props.scope, self.work)?;
                let mut ctx = Ctx { scopes: &mut self.scopes, work: &mut *self.work, limits: self.limits, unit: self.unit };
                ctx.write_ops(&mut tag, &mut self.splicer, ops, &mut wrote)?;
                if let Some(attrs) = dia {
                    ctx.write_dia(&mut tag, &mut self.splicer, attrs)?;
                }
            }
            None => {
                let ext = self.extension_attributes(def.scope, dia.unwrap_or(&[]))?;
                let element = self.ctx().props_element(def.scope, ops, &ext, &mut wrote)?;
                if def.empty {
                    // `<style:style .../>`: its last two bytes are `/>`.
                    self.splicer.replace(def.open.1.saturating_sub(2), def.open.1, format!(">{element}</{}>", def.qname));
                } else {
                    self.splicer.insert(def.close_start, element);
                }
            }
        }
        Ok(())
    }

    /// The text of a copy of the style `def` under the name `name`, with `ops` made in its graphic properties.
    fn clone_style(
        &mut self,
        def: &StyleDef,
        name: &str,
        ops: &[Op],
        dia: Option<&[(String, String)]>,
        parent: Option<&Option<String>>,
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<String, WriteError> {
        let source = self.bytes.get(def.lead..def.end).ok_or(WriteError::Mismatch("a style that is not in the part"))?;
        let shift = |p: (usize, usize)| (p.0.saturating_sub(def.lead), p.1.saturating_sub(def.lead));
        let mut sub = Splicer::new(source);
        let name_at = shift(def.name_value);
        sub.replace(name_at.0, name_at.1, super::splice::escape_value(name, b'"'));
        if let Some(parent) = parent {
            let mut tag = TagEdit::untracked(source, shift(def.open), def.scope, self.work)?;
            self.write_parent(&mut tag, &mut sub, parent.as_deref())?;
        }
        match def.props.first().copied().flatten() {
            Some(props) => {
                let mut tag = TagEdit::untracked(source, shift(props.open), props.scope, self.work)?;
                self.ctx().write_ops(&mut tag, &mut sub, ops, wrote)?;
                if let Some(attrs) = dia {
                    self.ctx().write_dia(&mut tag, &mut sub, attrs)?;
                }
            }
            None => {
                let ext = self.extension_attributes(def.scope, dia.unwrap_or(&[]))?;
                let element = self.ctx().props_element(def.scope, ops, &ext, wrote)?;
                if def.empty {
                    let open = shift(def.open);
                    sub.replace(open.1.saturating_sub(2), open.1, format!(">{element}</{}>", def.qname));
                } else {
                    sub.insert(def.close_start.saturating_sub(def.lead), element);
                }
            }
        }
        let made = sub.apply(self.work, self.limits)?;
        String::from_utf8(made).map_err(|_| WriteError::Mismatch("a style that is not text"))
    }

    /// Makes `ops` in the properties element of kind `prop` (1 a paragraph's, 2 a text's) of the automatic text or paragraph style `def` **where it stands** (a
    /// style is cloned only when another paragraph or run names it, and no style is renamed), the attributes it writes noted in `wrote`. A style with no such element
    /// is given one holding what `ops` write. Gives whether any edit was made: `ops` that change nothing of the style are none, and no edit is made for them.
    pub(crate) fn edit_text_style_in_place(
        &mut self,
        def: &StyleDef,
        prop: usize,
        ops: &[Op],
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<bool, WriteError> {
        let before = self.splicer.edits_len();
        let source = self.bytes;
        match def.props.get(prop).copied().flatten() {
            Some(props) => {
                let mut tag = TagEdit::untracked(source, props.open, props.scope, self.work)?;
                let mut ctx = Ctx { scopes: &mut self.scopes, work: &mut *self.work, limits: self.limits, unit: self.unit };
                ctx.write_ops(&mut tag, &mut self.splicer, ops, wrote)?;
            }
            None => {
                let element = if prop == 1 { "paragraph-properties" } else { "text-properties" };
                let pfx = Pfx::of(&mut self.scopes, def.scope, self.work);
                let attrs = self.ctx().attributes(&pfx, ops, wrote)?;
                if !attrs.is_empty() {
                    let element = format!("<{}:{element}{}{attrs}/>", pfx.style, pfx.declare);
                    self.work.charge_copy(element.len(), self.limits)?;
                    if def.empty {
                        self.splicer.replace(def.open.1.saturating_sub(2), def.open.1, format!(">{element}</{}>", def.qname));
                    } else {
                        self.splicer.insert(def.close_start, element);
                    }
                }
            }
        }
        Ok(self.splicer.edits_len() != before)
    }

    /// The items a copy of the style `def` holds: the events of its element (an element, a text, a comment each) and its attributes, counted by one scan of its bytes,
    /// **made once for a style** however many copies are made of it. An upper bound of what the reader counts for it (elements, attributes, texts and the rest, each one).
    pub(crate) fn style_items(&mut self, def: &StyleDef) -> Result<u64, WriteError> {
        if let Some(items) = self.texts.items.get(&def.open.0) {
            return Ok(*items);
        }
        let bytes = self.bytes.get(def.open.0..def.end).ok_or(WriteError::Mismatch("a style that is not in the part"))?;
        self.work.charge_scan(bytes.len(), self.limits)?;
        let (mut container, mut attributes) = (Work::default(), 0u64);
        let mut spans: Vec<AttrSpan> = Vec::new();
        let scanned = scan_tags(bytes, &self.limits.container.xml, &mut container, |tag| -> Result<(), WriteError> {
            if tag.kind == TagKind::Close {
                return Ok(());
            }
            spans.clear();
            tag_attributes(bytes, tag.start, tag.end, &mut spans).ok_or(WriteError::Mismatch("a style tag the writer cannot read"))?;
            attributes = attributes.saturating_add(spans.len() as u64);
            Ok(())
        });
        match scanned {
            Ok(()) => {}
            Err(diacraft_container::xml_scan::ScanError::Visit(e)) => return Err(e),
            Err(diacraft_container::xml_scan::ScanError::Xml(_)) => return Err(WriteError::Mismatch("a style the writer cannot scan")),
        }
        self.work.package.xml_events = self.work.package.xml_events.saturating_add(container.xml_events);
        let items = container.xml_events.saturating_add(attributes);
        self.texts.items.try_reserve(1).map_err(|_| starved("the styles of a part", self.texts.items.len()))?;
        self.texts.items.insert(def.open.0, items);
        Ok(items)
    }

    /// The text of a copy of the automatic text or paragraph style `def` with `ops` made in its properties element of kind `prop` (1 a paragraph's, 2 a text's) and every
    /// other byte of the style as the file has it: the attributes no cell maps, the elements it holds (a paragraph style's `style:tab-stops`, with their leaders), its
    /// parent and its other attributes (a value no cell maps is kept, and each is kept while its own cells are unchanged; the same rows apply to the styles
    /// of a paragraph and of a run). A style with no such element gets one, holding what `ops` write. The copy still has the name of `def` ([`rename_copy`] gives it
    /// another): `None` when `ops` change nothing, so that no style is made (and no name used) for an edit the style already says.
    pub(crate) fn clone_text_style(
        &mut self,
        def: &StyleDef,
        prop: usize,
        ops: &[Op],
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<Option<String>, WriteError> {
        let source = self.bytes.get(def.lead..def.end).ok_or(WriteError::Mismatch("a style that is not in the part"))?;
        let shift = |p: (usize, usize)| (p.0.saturating_sub(def.lead), p.1.saturating_sub(def.lead));
        let mut sub = Splicer::new(source);
        match def.props.get(prop).copied().flatten() {
            Some(props) => {
                let mut tag = TagEdit::untracked(source, shift(props.open), props.scope, self.work)?;
                self.ctx().write_ops(&mut tag, &mut sub, ops, wrote)?;
            }
            None => {
                let element = if prop == 1 { "paragraph-properties" } else { "text-properties" };
                let pfx = Pfx::of(&mut self.scopes, def.scope, self.work);
                let attrs = self.ctx().attributes(&pfx, ops, wrote)?;
                // Only what is taken out and is not there writes nothing: no empty element is made for it.
                if !attrs.is_empty() {
                    let element = format!("<{}:{element}{}{attrs}/>", pfx.style, pfx.declare);
                    if def.empty {
                        let open = shift(def.open);
                        sub.replace(open.1.saturating_sub(2), open.1, format!(">{element}</{}>", def.qname));
                    } else {
                        sub.insert(def.close_start.saturating_sub(def.lead), element);
                    }
                }
            }
        }
        let made = sub.apply(self.work, self.limits)?;
        if made == source {
            return Ok(None);
        }
        String::from_utf8(made).map(Some).map_err(|_| WriteError::Mismatch("a style that is not text"))
    }

    /// The text of a new automatic graphic style `name` whose parent is `parent`, setting what `ops` write.
    pub(crate) fn new_style(
        &mut self,
        name: &str,
        parent: Option<&str>,
        ops: &[Op],
        dia: &[(String, String)],
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<String, WriteError> {
        self.new_style_with(name, parent, ops, &[], dia, wrote)
    }

    /// [`Editor::new_style`] with the attributes of the style's graphic properties that no cell reads (`extra`: namespace, local name and value, put after what `ops` write and before DiaCraft's
    /// own), which the model writer takes from what the file's style kept of them.
    pub(crate) fn new_style_with(
        &mut self,
        name: &str,
        parent: Option<&str>,
        ops: &[Op],
        extra: &[(Ns, &str, &str)],
        dia: &[(String, String)],
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<String, WriteError> {
        let scope = self
            .book
            .as_ref()
            .and_then(|b| b.auto.as_ref())
            .map(|a| a.scope)
            .ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let escape = |t: &str| super::splice::escape_value(t, b'"');
        let mut text = format!("<{0}:style {0}:name=\"{1}\" {0}:family=\"graphic\"", pfx.style, escape(name));
        if let Some(parent) = parent {
            text.push_str(&format!(" {}:parent-style-name=\"{}\"", pfx.style, escape(parent)));
        }
        text.push_str(&pfx.declare);
        text.push('>');
        let inner = pfx.without_declarations();
        let mut attrs = self.ctx().attributes(&inner, ops, wrote)?;
        for (ns, local, value) in extra {
            self.ctx().charge_one()?;
            // The bytes of the value are charged before they are escaped and copied.
            self.work.charge_copy(value.len(), self.limits)?;
            attrs.push_str(&inner.attr(*ns, local, value));
        }
        let (declare, extension) = self.extension_attributes(scope, dia)?;
        let declare = format!("{declare}{}", loext_declaration(ops));
        // On the model path a style that says nothing has no properties element: an empty one is an element the file the shape was read from did not hold.
        if self.work.model_path && attrs.is_empty() && declare.is_empty() && extension.is_empty() {
            text.pop();
            text.push_str("/>");
            return Ok(text);
        }
        text.push_str(&format!("<{}:graphic-properties{declare}{attrs}{extension}/></{}:style>", inner.style, inner.style));
        Ok(text)
    }

    /// The attributes of DiaCraft's extension that `dia` (local names and values, the owned ones) makes on a new graphic properties element put where
    /// `scope`'s namespaces are in force: `diacraft:version="1"` and each of them under the prefix the place binds to the extension, or one declared
    /// on the element for it. Nothing for no attributes (a style with no tier 2 holds none of the extension's, not even the version).
    pub(crate) fn extension_attributes(&mut self, scope: u32, dia: &[(String, String)]) -> Result<(String, String), WriteError> {
        if dia.is_empty() {
            return Ok((String::new(), String::new()));
        }
        let (prefix, declare) = match self.scopes.prefix_for(scope, crate::write::scan::DIACRAFT_URI, self.work) {
            Some(p) => (p, String::new()),
            None => {
                let fresh = self.scopes.fresh_prefix(crate::ext::names::PREFIX, self.work);
                let declare = format!(" xmlns:{fresh}=\"{}\"", crate::write::scan::DIACRAFT_URI);
                (fresh, declare)
            }
        };
        let mut text = String::new();
        let mut one = |local: &str, value: &str, ctx: &mut Ctx<'_>| -> Result<(), WriteError> {
            ctx.charge_one()?;
            text.push_str(&format!(" {prefix}:{local}=\"{}\"", super::splice::escape_value(value, b'"')));
            Ok(())
        };
        let mut ctx = self.ctx();
        one("version", crate::ext::names::VERSION, &mut ctx)?;
        for (local, value) in dia {
            one(local, value, &mut ctx)?;
        }
        Ok((declare, text))
    }

    /// Puts the text of a new style at the end of `office:automatic-styles`.
    pub(crate) fn put_style(&mut self, text: String) -> Result<(), WriteError> {
        let auto = self
            .book
            .as_ref()
            .and_then(|b| b.auto.clone())
            .ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        if auto.made {
            // The part has no `office:automatic-styles`: the element is made before `office:body` with the styles put in it in the order they are made
            // (edits at one place keep the order they were made in), and closed by `finish_styles` after the last.
            let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new style without the styles of the part"))?;
            if !std::mem::replace(&mut book.opened, true) {
                self.splicer.insert(auto.close_start, format!("<{}>", auto.qname));
            }
            self.splicer.insert(auto.close_start, text);
        } else if auto.empty {
            // The element is written empty (`<office:automatic-styles/>`): its `/>` becomes `>` once, however many styles are put in, and they follow it, at the end
            // of the tag, in the order they were made (a replacement of the two bytes for each style was one edit of the same bytes for each).
            // `finish_styles` closes it after the last.
            let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new style without the styles of the part"))?;
            let end = auto.open.1;
            if !std::mem::replace(&mut book.opened, true) {
                // (The `/` and what stands between it and the `>`: `<q />` and `<q  />` are the same element, and the replaced bytes are those of the tag's last
                // slash alone, from the character before the `>`.)
                self.splicer.replace(end.saturating_sub(2), end, ">".to_owned());
            }
            self.splicer.insert(end, text);
        } else {
            self.splicer.insert(auto.close_start, text);
        }
        Ok(())
    }

    /// Closes the `office:automatic-styles` element a save made for a part that had none (or opened from an empty one), after the last style put in it.
    pub(crate) fn finish_styles(&mut self) {
        let Some(auto) = self.book.as_ref().filter(|b| b.opened).and_then(|b| b.auto.clone()) else { return };
        // (An element written empty has no closing tag to stand before: its `close_start` is the end of its opening tag, where the styles end: `scan.rs`.)
        self.splicer.insert(auto.close_start, format!("</{}>", auto.qname));
    }

    /// Takes out the styles that something named before the save and nothing names now, and then those that only they named: a style
    /// named by nothing but the parent attribute of one that goes (a parent of a style taken out, and its parent in turn) is an orphan of an orphan
    /// and goes too. The styles go by a queue, each once, and each reference inside a style taken out is looked at once and
    /// charged, so the work is the styles and the references and never a walk of all the styles for each one taken out.
    pub(crate) fn remove_orphans(&mut self) -> Result<(), WriteError> {
        let Some(book) = self.book.as_mut() else { return Ok(()) };
        let mut queue = book.orphans();
        let mut units: Vec<(usize, usize)> = Vec::new();
        while let Some(i) = queue.pop() {
            if book.removed.get(i).copied().unwrap_or(true) {
                continue;
            }
            book.mark_removed(i);
            let Some(def) = book.defs.get(i) else { continue };
            let (lead, end) = (def.lead, def.end);
            units.try_reserve(1).map_err(|_| starved("the styles taken out", units.len()))?;
            units.push((lead, end));
            let next = book.unit_removed_orphans(lead, end, self.work)?;
            queue.try_reserve(next.len()).map_err(|_| starved("the styles taken out", queue.len()))?;
            queue.extend(next);
        }
        for (lead, end) in units {
            self.splicer.remove(lead, end);
        }
        Ok(())
    }
}

/// The copy of the style `def` that [`Editor::clone_text_style`] made, under the name `name`: the value of `style:name` is replaced. The edits of the copy are all
/// after the start tag, in the properties, so the place of the name in the copy is its place in the style.
pub(crate) fn rename_copy(copy: &str, source: &[u8], def: &StyleDef, name: &str) -> Result<String, WriteError> {
    let (from, to) = (def.name_value.0.saturating_sub(def.lead), def.name_value.1.saturating_sub(def.lead));
    let same_start = |t: usize| copy.as_bytes().get(..t) == source.get(..t);
    match (copy.get(..from), copy.get(to..)) {
        (Some(head), Some(tail)) if same_start(to) => Ok(format!("{head}{}{tail}", super::splice::escape_value(name, b'"'))),
        _ => Err(WriteError::Mismatch("a style copy that is not the style up to its name")),
    }
}

/// Makes what a style carries follow the model: a cell, a section or a text the model holds is the model's, and one it lacks is said to be gone. The sections of
/// the model are indexed by their keys once (the first of a key winning, as a search for it would), so that none is searched for among the others.
fn follow_model(t1: &mut crate::ext::payload::Tier2, m: &diacraft_doc::Shape) {
    use crate::ext::names::Gone;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    let names: Vec<diacraft_doc::CellName> = t1.cells.keys().cloned().collect();
    for name in names {
        match m.sheet.cells.get(&name) {
            Some(c) => {
                t1.cells.insert(name, c.clone());
            }
            None => {
                t1.cells.remove(&name);
                t1.gone.push(Gone::Cell(name.as_str().to_owned()));
            }
        }
    }
    let mut in_model: BTreeMap<(&str, Option<u32>), &Arc<diacraft_doc::Section>> = BTreeMap::new();
    for s in &m.sheet.sections {
        in_model.entry((s.name.as_str(), s.ix)).or_insert(s);
    }
    let mut sections: Vec<Arc<diacraft_doc::Section>> = Vec::new();
    for held in std::mem::take(&mut t1.sections) {
        match in_model.get(&(held.name.as_str(), held.ix)) {
            Some(now) => sections.push(Arc::clone(now)),
            None => t1.gone.push(Gone::Section(diacraft_doc::SectionKey { name: held.name.clone(), ix: held.ix })),
        }
    }
    t1.sections = sections;
    if t1.text.is_some() {
        match &m.text {
            Some(text) => t1.text = Some((**text).clone()),
            None => {
                t1.text = None;
                t1.gone.push(Gone::Text);
            }
        }
    }
}

/// Adds what an edit carries to what a style carries: the cells put and the removals that name them taken out in one pass, then the cells it removes; the
/// sections it puts, each over the one of its key or last (by an index of the keys that follows the list as it grows), and those it removes.
fn add_edit(t1: &mut crate::ext::payload::Tier2, d: &super::own::Tier2Delta) {
    use crate::ext::names::Gone;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    let added: BTreeSet<&str> = d.cells.keys().map(diacraft_doc::CellName::as_str).collect();
    for (name, cell) in &d.cells {
        t1.cells.insert(name.clone(), cell.clone());
    }
    t1.gone.retain(|g| !matches!(g, Gone::Cell(n) if added.contains(n.as_str())));
    for name in &d.removed_cells {
        t1.cells.remove(name.as_str());
        t1.gone.push(Gone::Cell(name.clone()));
    }
    let mut index: BTreeMap<(String, Option<u32>), usize> = BTreeMap::new();
    for (at, s) in t1.sections.iter().enumerate() {
        index.entry((s.name.as_str().to_owned(), s.ix)).or_insert(at);
    }
    for section in &d.sections {
        let key = (section.name.as_str().to_owned(), section.ix);
        match index.get(&key).copied() {
            Some(at) => {
                if let Some(slot) = t1.sections.get_mut(at) {
                    *slot = Arc::clone(section);
                }
            }
            None => {
                index.insert(key, t1.sections.len());
                t1.sections.push(Arc::clone(section));
            }
        }
    }
    let put: BTreeSet<(&str, Option<u32>)> = d.sections.iter().map(|s| (s.name.as_str(), s.ix)).collect();
    t1.gone.retain(|g| !matches!(g, Gone::Section(k) if put.contains(&(k.name.as_str(), k.ix))));
    let taken: BTreeSet<(&str, Option<u32>)> = d.removed_sections.iter().map(|k| (k.name.as_str(), k.ix)).collect();
    t1.sections.retain(|s| !taken.contains(&(s.name.as_str(), s.ix)));
    for key in &d.removed_sections {
        t1.gone.push(Gone::Section(key.clone()));
    }
}

/// The shape's `ID` goes with its tier 2 as soon as that holds anything beside it; a style that has an `ID` keeps it, and a tier 2 that holds nothing has none.
fn give_id(t1: &mut crate::ext::payload::Tier2, id: u32) {
    if t1.id.is_none() && !t1.holds_nothing_but_id() {
        t1.id = Some(id);
    }
}

/// The names and links to a master a style carries follow the model when the edit changed them or the style carries them.
fn follow_fields(t1: &mut crate::ext::payload::Tier2, m: &diacraft_doc::Shape, fields: bool) {
    use crate::ext::names::Gone;
    let text = |v: &Option<std::sync::Arc<str>>| v.as_ref().map(|v| v.to_string());
    if fields || t1.name.is_some() {
        (t1.name, t1.gone) = names_of(text(&m.name), std::mem::take(&mut t1.gone), Gone::Name);
    }
    if fields || t1.name_u.is_some() {
        (t1.name_u, t1.gone) = names_of(text(&m.name_u), std::mem::take(&mut t1.gone), Gone::NameU);
    }
    if fields || t1.master.is_some() {
        t1.master = m.master;
    }
    if fields || t1.master_shape.is_some() {
        t1.master_shape = m.master_shape;
    }
}

/// A name the model holds, or the token that says it is gone, put into the list of removals the model has (a name that is there is no longer gone).
fn names_of(
    now: Option<String>,
    mut gone: Vec<crate::ext::names::Gone>,
    token: crate::ext::names::Gone,
) -> (Option<String>, Vec<crate::ext::names::Gone>) {
    gone.retain(|g| *g != token);
    if now.is_none() {
        gone.push(token);
    }
    (now, gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition of a style `source` that is the whole of the part, its name between the quotes at `name_value`.
    fn definition(source: &str) -> StyleDef {
        let at = source.find("T1").unwrap_or(0);
        StyleDef {
            family: Fam::Text,
            qname: "style:style".to_owned(),
            name: "T1".to_owned(),
            name_value: (at, at + 2),
            lead: 0,
            open: (0, source.find('>').map_or(0, |e| e + 1)),
            empty: false,
            close_start: source.len(),
            end: source.len(),
            scope: 0,
            props: [None, None, None],
        }
    }

    /// A copy of a style under another name is the style up to its name and the copy's own edits after it: the name is put in the place the style has it, escaped, and a
    /// copy that is not the style up to there is a fault and not a name written into the wrong bytes.
    #[test]
    fn a_copy_is_given_its_name_in_the_place_the_style_has_it_and_a_copy_that_differs_before_it_is_refused() {
        let source = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="12pt"/></style:style>"#;
        let def = definition(source);
        let copy = source.replace("12pt", "14pt");
        assert_eq!(
            rename_copy(&copy, source.as_bytes(), &def, "T2").ok().as_deref(),
            Some(r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-size="14pt"/></style:style>"#)
        );
        let quoted = rename_copy(&copy, source.as_bytes(), &def, "a\"b").unwrap_or_default();
        assert!(quoted.starts_with(r#"<style:style style:name="a&quot;b" style:family="text">"#), "{quoted}");
        // The edits are after the name, and a copy that changed a byte before the end of the name is not the style.
        let before = copy.replacen("<style:style", "<style:Style", 1);
        assert!(matches!(rename_copy(&before, source.as_bytes(), &def, "T2"), Err(WriteError::Mismatch(_))));
        let inside = copy.replacen("T1", "T9", 1);
        assert!(matches!(rename_copy(&inside, source.as_bytes(), &def, "T2"), Err(WriteError::Mismatch(_))));
        // A copy shorter than the name's place has no name to replace.
        assert!(matches!(rename_copy("<style:style", source.as_bytes(), &def, "T2"), Err(WriteError::Mismatch(_))));
    }

    /// `write_dia` on the start tag `tag` (which declares nothing: the prefix `diacraft` is bound in the scope of the test) to make the attributes of the extension
    /// that the writer owns exactly `attrs`: the tag as it is written.
    fn written_dia(tag: &str, attrs: &[(&str, &str)]) -> String {
        let (mut scopes, scope) = Scopes::with_scope(&[("diacraft", Ns::Diacraft)], None, None);
        let (mut work, limits) = (WriteWork::default(), Limits::default());
        let bytes = tag.as_bytes();
        let mut edit = TagEdit::new(bytes, (0, bytes.len()), scope, &mut work).unwrap_or_else(|e| panic!("the tag did not read: {e}"));
        let mut splicer = Splicer::new(bytes);
        let attrs: Vec<(String, String)> = attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        let mut ctx = Ctx { scopes: &mut scopes, work: &mut work, limits: &limits, unit: LenUnit::Cm };
        if let Err(e) = ctx.write_dia(&mut edit, &mut splicer, &attrs) {
            panic!("write_dia: {e}");
        }
        match splicer.apply(&mut work, &limits) {
            Ok(out) => String::from_utf8(out).unwrap_or_default(),
            Err(e) => panic!("the edits did not apply: {e}"),
        }
    }

    /// The version of the extension is `1` while any attribute of it is on the tag, and goes with the last: the version alone and the names the writer owns are
    /// taken out when nothing is wanted; a name the writer does not own keeps the version where it is; a tag with attributes and no version gets one.
    #[test]
    fn the_version_goes_with_the_last_attribute_of_the_extension_and_stays_with_one_the_writer_does_not_own() {
        // Nothing wanted: the names the writer owns and the version go.
        let out = written_dia(r#"<p diacraft:version="1" diacraft:name="x"/>"#, &[]);
        assert!(!out.contains("diacraft:"), "{out}");
        // Nothing wanted, but a name the writer does not own is there: it is left, and so is the version.
        let out = written_dia(r#"<p diacraft:version="1" diacraft:name="x" diacraft:filled="1"/>"#, &[]);
        assert!(out.contains(r#"diacraft:version="1""#) && out.contains(r#"diacraft:filled="1""#) && !out.contains("diacraft:name"), "{out}");
        // Something wanted and no version: the version is added, beside the attribute that is written.
        let out = written_dia(r#"<p diacraft:name="x"/>"#, &[("name", "y")]);
        assert!(out.contains(r#"diacraft:version="1""#) && out.contains(r#"diacraft:name="y""#), "{out}");
        // Something wanted and the version there: it is kept, and an attribute that is wanted and not there is added.
        let out = written_dia(r#"<p diacraft:version="1"/>"#, &[("name", "z")]);
        assert!(out.matches("diacraft:version").count() == 1 && out.contains(r#"diacraft:name="z""#), "{out}");
    }

    fn named(name: &str) -> crate::ext::payload::Tier2 {
        crate::ext::payload::Tier2 { name: Some(name.to_owned()), ..crate::ext::payload::Tier2::default() }
    }

    /// The shape's `ID` is given to tier 2 that holds something, kept where there is one, and not given to tier 2 that holds nothing.
    #[test]
    fn the_id_goes_with_tier_2_that_holds_something_and_is_never_replaced() {
        let mut t = named("n");
        give_id(&mut t, 9);
        assert_eq!(t.id, Some(9), "none, and something held");
        let mut t = crate::ext::payload::Tier2::default();
        give_id(&mut t, 9);
        assert_eq!(t.id, None, "none, and nothing held");
        let mut t = crate::ext::payload::Tier2 { id: Some(7), ..named("n") };
        give_id(&mut t, 9);
        assert_eq!(t.id, Some(7), "one is kept");
        let mut t = crate::ext::payload::Tier2 { id: Some(7), ..crate::ext::payload::Tier2::default() };
        give_id(&mut t, 9);
        assert_eq!(t.id, Some(7), "one is kept when nothing else is held");
    }

    /// What an edit puts takes the removals that name it out, for cells and for sections, and the removals of the others stay.
    #[test]
    fn what_an_edit_puts_is_no_longer_said_to_be_gone_and_what_it_does_not_put_still_is() {
        use crate::ext::names::Gone;
        use diacraft_doc::{Cell, CellName, Section, SectionKey, Value};
        let mut t1 = crate::ext::payload::Tier2 {
            gone: vec![
                Gone::Cell("A".to_owned()),
                Gone::Cell("B".to_owned()),
                Gone::Section(SectionKey::new("User", None)),
                Gone::Section(SectionKey::new("Property", None)),
                Gone::Name,
            ],
            ..crate::ext::payload::Tier2::default()
        };
        let mut d = super::super::own::Tier2Delta::default();
        d.cells.insert(CellName::new("A"), Cell::constant(Value::from_text("1")));
        d.sections.push(std::sync::Arc::new(Section::new("User", None)));
        add_edit(&mut t1, &d);
        assert_eq!(t1.gone, [Gone::Cell("B".to_owned()), Gone::Section(SectionKey::new("Property", None)), Gone::Name]);
        assert!(t1.cells.contains_key("A") && t1.sections.iter().any(|s| s.name.as_str() == "User"));
        // What it takes out is carried as gone, and is no longer carried.
        let mut d = super::super::own::Tier2Delta::default();
        d.removed_cells.push("A".to_owned());
        d.removed_sections.push(SectionKey::new("User", None));
        add_edit(&mut t1, &d);
        assert!(!t1.cells.contains_key("A") && t1.sections.is_empty());
        assert!(t1.gone.contains(&Gone::Cell("A".to_owned())) && t1.gone.contains(&Gone::Section(SectionKey::new("User", None))));
    }

    /// A name the model holds is no longer said to be gone, one it lacks is; and the other removals are left as they are.
    #[test]
    fn a_name_the_model_holds_is_taken_out_of_the_removals_and_one_it_lacks_is_put_in() {
        use crate::ext::names::Gone;
        let gone = || vec![Gone::Name, Gone::NameU, Gone::Text];
        let (name, left) = names_of(Some("x".to_owned()), gone(), Gone::Name);
        assert_eq!((name.as_deref(), left), (Some("x"), vec![Gone::NameU, Gone::Text]));
        let (name, left) = names_of(None, gone(), Gone::NameU);
        assert_eq!(name, None);
        assert!(left.len() == 3 && left.contains(&Gone::Name) && left.contains(&Gone::NameU) && left.contains(&Gone::Text), "{left:?}");
        let (_, left) = names_of(None, vec![Gone::Text], Gone::Name);
        assert_eq!(left, vec![Gone::Text, Gone::Name]);
    }

    /// A name or a link to a master that a style carries follows the model, and one it does not carry is taken from the model only when the edit changed the fields.
    #[test]
    fn the_names_and_links_a_style_carries_follow_the_model_and_the_others_only_when_the_fields_changed() {
        let model = diacraft_doc::Shape {
            name: Some(std::sync::Arc::from("N")),
            name_u: Some(std::sync::Arc::from("U")),
            master: Some(3),
            master_shape: Some(4),
            ..diacraft_doc::Shape::new(1)
        };
        // The fields changed: all four are the model's, carried or not.
        let mut t1 = crate::ext::payload::Tier2::default();
        follow_fields(&mut t1, &model, true);
        assert_eq!((t1.name.as_deref(), t1.name_u.as_deref(), t1.master, t1.master_shape), (Some("N"), Some("U"), Some(3), Some(4)));
        // Nothing changed and nothing is carried: nothing is.
        let mut t1 = crate::ext::payload::Tier2::default();
        follow_fields(&mut t1, &model, false);
        assert_eq!((t1.name.as_deref(), t1.name_u.as_deref(), t1.master, t1.master_shape), (None, None, None, None));
        // Nothing changed, and each one that is carried follows the model by itself.
        let carried = |t: crate::ext::payload::Tier2| {
            let mut t1 = t;
            follow_fields(&mut t1, &model, false);
            (t1.name, t1.name_u, t1.master, t1.master_shape)
        };
        let blank = crate::ext::payload::Tier2::default;
        assert_eq!(carried(crate::ext::payload::Tier2 { name: Some("old".to_owned()), ..blank() }), (Some("N".to_owned()), None, None, None));
        assert_eq!(carried(crate::ext::payload::Tier2 { name_u: Some("old".to_owned()), ..blank() }), (None, Some("U".to_owned()), None, None));
        assert_eq!(carried(crate::ext::payload::Tier2 { master: Some(9), ..blank() }), (None, None, Some(3), None));
        assert_eq!(carried(crate::ext::payload::Tier2 { master_shape: Some(9), ..blank() }), (None, None, None, Some(4)));
    }

    #[test]
    fn a_style_is_an_orphan_once_when_something_named_it_and_nothing_does() {
        let def = |name: &str| StyleDef {
            family: Fam::Graphic,
            qname: "style:style".into(),
            name: name.into(),
            name_value: (0, 0),
            lead: 0,
            open: (0, 0),
            empty: true,
            close_start: 0,
            end: 0,
            scope: 0,
            props: [None, None, None],
        };
        // gr1: named and no longer; gr2: still named; gr3: never named.
        let mut book = StyleBook {
            defs: vec![def("gr1"), def("gr2"), def("gr3")],
            auto: None,
            opened: false,
            by_name: HashMap::new(),
            total: vec![1, 2, 0],
            live: vec![0, 1, 0],
            refs: Vec::new(),
            removed: vec![false; 3],
            names: Names::default(),
            written: Vec::new(),
            held_styles: HashMap::new(),
            held_paragraphs: std::collections::BTreeSet::new(),
            remainders: None,
            remainder_cache: HashMap::new(),
            text_remainders: None,
            held_inputs: HashMap::new(),
            tier2: HashMap::new(),
        };
        assert_eq!(book.orphans(), [0]);
        book.mark_removed(0);
        assert!(book.orphans().is_empty(), "a style taken out is not taken out again");
    }

    #[test]
    fn the_references_scan_charges_the_bytes_of_each_start_tag_it_reads_and_none_for_an_end_tag() {
        // `<r>` is 3 bytes, `<a x="1"/>` 10, `<b y="gr1">` 11; `</b>` and `</r>` are not charged.
        let mut work = WriteWork::default();
        let (refs, _, _) = count_refs(br#"<r><a x="1"/><b y="gr1"></b></r>"#, &[], &HashSet::new(), &Limits::default(), &mut work).unwrap();
        assert!(refs.is_empty());
        assert_eq!(work.attribute_bytes, 3 + 10 + 11);
    }

    #[test]
    fn a_new_name_is_the_prefix_most_used_and_one_more_than_the_largest_number_of_either_part() {
        // `gr1` to `gr4` and `gr9` in one part, `gr12` in the other: `gr13`, and not the first free number. Names of another prefix and of
        // another family do not count for it.
        let mut names = Names::default();
        let content = ["gr1", "gr2", "gr3", "gr4", "gr9", "P1", "P2", "x7"];
        names.add_all(
            content
                .iter()
                .map(|n| (if n.starts_with("gr") { Fam::Graphic } else { Fam::Paragraph }, *n))
                .chain(std::iter::once((Fam::Graphic, "gr12"))),
        );
        assert_eq!(names.next(Fam::Graphic), "gr13");
        assert_eq!(names.next(Fam::Graphic), "gr14", "nothing is renumbered, and a name is given once");
        assert_eq!(names.next(Fam::Paragraph), "P3");
        // A family with no style gets LibreOffice's prefix, and a text style `T`.
        assert_eq!(names.next(Fam::Text), "T1");
        assert_eq!(names.next(Fam::DrawingPage), "dp1");
        // `a1`, `a2` and `x7` of one family: the prefix most of them use, `a`, gives `a3`.
        let mut other = Names::default();
        other.add_all(["a1", "a2", "x7"].into_iter().map(|n| (Fam::Graphic, n)));
        assert_eq!(other.next(Fam::Graphic), "a3");
        // The prefix that most use wins over one that is least by text: `z1`, `z2` and `b9` give `z3`.
        let mut most = Names::default();
        most.add_all(["z1", "z2", "b9"].into_iter().map(|n| (Fam::Graphic, n)));
        assert_eq!(most.next(Fam::Graphic), "z3");
        // The names of two parts are added by two calls: the largest number of the prefix that was chosen counts, and a prefix of the second part that
        // is not the one chosen does not.
        let mut two = Names::default();
        two.add_all(["gr1", "gr3"].into_iter().map(|n| (Fam::Graphic, n)));
        two.add_all(["gr9", "gr2"].into_iter().map(|n| (Fam::Graphic, n)));
        assert_eq!(two.next(Fam::Graphic), "gr10");
        let mut apart = Names::default();
        apart.add_all(["gr1", "gr3"].into_iter().map(|n| (Fam::Graphic, n)));
        apart.add_all(["q40"].into_iter().map(|n| (Fam::Graphic, n)));
        assert_eq!(apart.next(Fam::Graphic), "gr4", "the series of the first part stays");
        // Equal counts: the least prefix by text, so that one file always gives one name.
        let mut tied = Names::default();
        tied.add_all([(Fam::Graphic, "b5"), (Fam::Graphic, "a2")].into_iter());
        assert_eq!(tied.next(Fam::Graphic), "a3");
        // A name already taken by a style with no number is not given again.
        let mut taken = Names::default();
        taken.add_all([(Fam::Graphic, "gr1"), (Fam::Graphic, "gr3")].into_iter());
        assert_eq!(taken.next(Fam::Graphic), "gr4");
    }

    #[test]
    fn a_name_is_split_at_its_trailing_digits() {
        assert_eq!(split_number("gr13"), Some(("gr", 13)));
        assert_eq!(split_number("Default"), None);
        assert_eq!(split_number("7"), Some(("", 7)));
        assert_eq!(split_number("a_5_20"), Some(("a_5_", 20)));
        assert_eq!(split_number(&format!("p{}", "9".repeat(19))), None, "a number that does not fit is no number");
    }
}
