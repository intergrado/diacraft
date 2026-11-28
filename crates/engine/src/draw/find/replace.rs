//! Replace and Replace All.
//!
//! **Two steps, so that nothing is half done.** The first reads the drawing and plans every replacement: which matches of which places, what each place's new text is,
//! and which matches are left (and why: [`Why`]). It charges its work and its matches, and writes nothing. The second writes the plan, all of it, on the copy of the
//! document the command edits (`Session::edit_if`), so a failure puts back what was there, and the whole is one undo step.
//!
//! **How a place is written.**
//!
//! * *Text*: one pass over the shape's parts ([`rewrite_parts`]). The characters outside the matches stay in their runs; the text that replaces a match joins the run of
//!   the match's first character, as `shape.set_text`'s range does; a match across runs takes the characters of the later runs out of them; the
//!   markers and the fields stay where they are. A match that touches what a field shows is left.
//! * *Name*: the shape's `Name` (and its `NameU` when it was the same). The new name must be one a name may be ([`diacraft_doc::check_text`]).
//! * *Shape data and user-defined cells*: the cell's `Value`. A quoted-string formula stays a quoted-string formula with the new text, a constant stays a constant (and
//!   must still read as text, not as a number), and a cell a formula works out is left. A cell with `GUARD` on it is left.
//!
//! After the writes, what reads a changed text or cell is recalculated once for the page ([`settle_cells_on_pages`]).
//!
//! **A shape on a locked layer is passed over**: a replace across a page, the drawing or a selection that holds a group names no shape it
//! edits, so the registry's rule does not see the shapes it reaches. Each shape is judged by its own `LayerMember`, a member of a group by its own; every match
//! of one on a locked layer is left with [`Why::Locked`] and counted in [`Replaced::locked`], and Find, which only reads, still finds it.

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, Owner, ShapeRef, SheetRef, TextPart, Value, WriteOrigin, WriteOutcome};

use super::pattern::{Pattern, Refused, Work};
use super::source::{CellHow, How, Kind, Source, Why, Within, quoted, sources, stays_text};
use super::walk::{Flow, Scope, walk};
use super::{Cursor, MAX_FIND_STEPS, MAX_REPLACEMENTS, Spot, check_scope, too_big};
use crate::draw::settle::settle_cells_on_pages;
use crate::draw::{MAX_TEXT_CHARS, Problem, kept_text, shape_mut};

/// The most skipped matches a result lists: the count is exact.
pub const MAX_SKIPPED_LISTED: usize = 100;

/// **The most bytes of text one replacement may write, summed over the drawing**: 64 MiB, the size of the engine's other caps on what one command keeps (`snap::reads::MAX_KEPT_BYTES`).
/// Every text a replacement makes is held to a text's longest (`MAX_TEXT_CHARS`) on its own, which is not a cap on the drawing: a Replace All across 76,000 shapes by a long text is 76,000
/// times that. The process's memory budget (`memory.rs`) is judged by `Session::edit_if` **after** the edit has made its copy, so it cannot refuse what the plan has not counted: the plan charges
/// the bytes of every text, name and cell it will make **before it makes it**, and refuses in words past this. What is counted is an upper bound: the whole text the match is in, and the
/// replacement once for each match.
pub const MAX_REWRITE_BYTES: u64 = 64 * 1024 * 1024;

/// What a planned place and each of its matches cost in the plan itself, beside the text: charged with it.
const PLACE_BYTES: u64 = 192;
const MATCH_BYTES: u64 = 16;

/// Takes `bytes` off the budget of the text one replacement may write: `Err` in words when that passes [`MAX_REWRITE_BYTES`].
fn charge_bytes(spent: &mut u64, bytes: u64) -> Result<(), Problem> {
    *spent = spent.saturating_add(bytes);
    if *spent > MAX_REWRITE_BYTES {
        return Err(Problem::failed(format!(
            "this replacement would write more than {} MiB of text (the budget of one replacement): replace in the selection or on one page, or by shorter text",
            MAX_REWRITE_BYTES >> 20
        )));
    }
    Ok(())
}

/// The bytes a place's replacement may write: its text, the replacement once for each match, and the plan's own.
fn bound_of(src: &Source<'_>, matches: usize, with: &str) -> u64 {
    let bytes = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    bytes(src.text.len())
        .saturating_add(bytes(matches).saturating_mul(bytes(with.len())))
        .saturating_add(PLACE_BYTES)
        .saturating_add(bytes(matches).saturating_mul(MATCH_BYTES))
}

/// A match that was left, with why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub spot: Spot,
    pub start: usize,
    pub end: usize,
    pub why: Why,
}

/// What a replacement did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    /// How many matches were replaced.
    pub replaced: usize,
    /// In how many shapes.
    pub shapes: usize,
    /// The `ID`s of the pages that changed, in order.
    pub pages: Vec<u32>,
    /// The matches that were left, the first [`MAX_SKIPPED_LISTED`] of them.
    pub skipped: Vec<Skipped>,
    /// How many matches were left in all.
    pub skipped_total: usize,
    /// How many of them were left because their shape is on a locked layer ([`Why::Locked`]): counted in `skipped_total` too.
    pub locked: usize,
    /// For one match replaced: where to go on from, after the new text.
    pub next: Option<Cursor>,
    /// The steps the plan took.
    pub steps: u64,
    /// How many sections of shape data or user-defined cells were not read because a shape had too many rows in one (as `Outcome::unread`).
    pub unread: u64,
}

/// What is to be written to one place.
struct PlacePlan {
    kind: Kind,
    row: Option<String>,
    how: How,
    /// The matches to replace, in order.
    apply: Vec<(usize, usize)>,
    /// The place's new text, for a name or a cell; a text is rebuilt from its parts.
    new_text: Option<String>,
}

/// What is to be written to one shape.
struct ShapePlan {
    page: u32,
    path: Vec<u32>,
    id: u32,
    places: Vec<PlacePlan>,
}

/// The characters of `old` with `spans` (sorted, not overlapping) replaced by `with`, in one pass.
fn rebuilt(old: &str, spans: &[(usize, usize)], with: &str) -> String {
    let mut out = String::with_capacity(old.len());
    let mut at = Walker::new(old);
    let mut done = 0_usize;
    for &(start, end) in spans {
        out.push_str(at.slice(done, start));
        out.push_str(with);
        done = end;
    }
    out.push_str(at.rest(done));
    out
}

/// A walk of a string that goes forward and turns characters into bytes: each call starts where the one before ended.
struct Walker<'a> {
    text: &'a str,
    chars: usize,
    byte: usize,
}

impl<'a> Walker<'a> {
    fn new(text: &'a str) -> Walker<'a> {
        Walker { text, chars: 0, byte: 0 }
    }

    /// The byte of the character `target`, which is not before the last one asked for.
    fn to(&mut self, target: usize) -> usize {
        if target > self.chars {
            let rest = self.text.get(self.byte..).unwrap_or("");
            let skip = target - self.chars;
            let (bytes, chars) = match rest.char_indices().nth(skip) {
                Some((b, _)) => (b, skip),
                None => (rest.len(), rest.chars().count()),
            };
            self.byte = self.byte.saturating_add(bytes);
            self.chars = self.chars.saturating_add(chars);
        }
        self.byte
    }

    /// The characters from `from` to `to`.
    fn slice(&mut self, from: usize, to: usize) -> &'a str {
        let a = self.to(from);
        let b = self.to(to).max(a);
        self.text.get(a..b).unwrap_or("")
    }

    /// The characters from `from` to the end.
    fn rest(&mut self, from: usize) -> &'a str {
        let a = self.to(from);
        self.text.get(a..).unwrap_or("")
    }
}

/// The parts of a text with `applied` (matches in the characters the text shows, sorted, not overlapping, none touching a field) replaced by `with`: one pass.
pub(crate) fn rewrite_parts(parts: &[TextPart], applied: &[(usize, usize)], with: &str) -> Vec<TextPart> {
    let mut out: Vec<TextPart> = Vec::with_capacity(parts.len());
    let mut pos = 0_usize;
    let mut next = 0_usize;
    for part in parts {
        match part {
            TextPart::Chars(s) => {
                let n = s.chars().count();
                let end_of_part = pos.saturating_add(n);
                // Matches this part holds some of: the first one still open starts before the part's end.
                if !applied.get(next).is_some_and(|&(start, _)| start < end_of_part) {
                    out.push(part.clone());
                    pos = end_of_part;
                    continue;
                }
                let mut text = String::with_capacity(s.len());
                let mut at = Walker::new(s);
                let mut copied = pos;
                while let Some(&(start, end)) = applied.get(next) {
                    if start >= end_of_part {
                        break;
                    }
                    text.push_str(at.slice(copied - pos, start.max(pos) - pos));
                    if start >= pos {
                        text.push_str(with);
                    }
                    copied = end.min(end_of_part);
                    if end <= end_of_part {
                        next += 1;
                    } else {
                        break;
                    }
                }
                text.push_str(at.rest(copied - pos));
                // A part with no characters left is taken out; the markers beside it stay.
                if !text.is_empty() {
                    out.push(TextPart::Chars(Arc::from(text)));
                }
                pos = end_of_part;
            }
            TextPart::Fld { text, .. } => {
                pos = pos.saturating_add(text.chars().count());
                out.push(part.clone());
            }
            other => out.push(other.clone()),
        }
    }
    out
}

/// Whether the match `(start, end)` touches a field's result, for matches and `fields` that are both sorted: `fi` is where the walk of the fields has got to.
pub(crate) fn touches_field(fields: &[(usize, usize)], fi: &mut usize, start: usize, end: usize) -> bool {
    while fields.get(*fi).is_some_and(|&(_, field_end)| field_end <= start) {
        *fi += 1;
    }
    fields.get(*fi).is_some_and(|&(field_start, _)| field_start < end)
}

/// The matches a place's plan leaves: `(start, end, why)`.
type Left = Vec<(usize, usize, Why)>;

/// What a place's plan is: the matches to replace, those left with why, and the new text of a name or a cell.
type Planned = (Vec<(usize, usize)>, Left, Option<String>);

/// A place's matches, split into those to replace and those left, with the bytes of what is to be written charged to `spent` before any of it is made.
fn plan_place(src: &Source<'_>, matches: Vec<(usize, usize)>, with: &str, spent: &mut u64) -> Result<Planned, Problem> {
    let with_chars = with.chars().count();
    let old = src.text.chars().count();
    let all_left = |why: Why, matches: Vec<(usize, usize)>| -> Result<Planned, Problem> {
        Ok((Vec::new(), matches.into_iter().map(|(s, e)| (s, e, why)).collect(), None))
    };
    // How long the text is once `matches` are replaced.
    let grown = |matches: &[(usize, usize)]| -> usize {
        let removed: usize = matches.iter().map(|&(s, e)| e.saturating_sub(s)).fold(0, usize::saturating_add);
        old.saturating_sub(removed).saturating_add(matches.len().saturating_mul(with_chars))
    };
    match &src.how {
        How::Text { fields } => {
            let mut fi = 0_usize;
            let mut apply: Vec<(usize, usize)> = Vec::with_capacity(matches.len());
            let mut left: Left = Vec::new();
            for (s, e) in matches {
                if touches_field(fields, &mut fi, s, e) {
                    left.push((s, e, Why::InField));
                } else {
                    apply.push((s, e));
                }
            }
            let new_len = grown(&apply);
            if !apply.is_empty() && new_len > MAX_TEXT_CHARS && new_len > old {
                left.extend(apply.into_iter().map(|(s, e)| (s, e, Why::TooLong)));
                return Ok((Vec::new(), left, None));
            }
            if !apply.is_empty() {
                charge_bytes(spent, bound_of(src, apply.len(), with))?;
            }
            Ok((apply, left, None))
        }
        How::Name => {
            if grown(&matches) > diacraft_doc::MAX_NAME_CHARS {
                return all_left(Why::NotAllowed, matches);
            }
            charge_bytes(spent, bound_of(src, matches.len(), with))?;
            let new = rebuilt(&src.text, &matches, with);
            if diacraft_doc::check_text(&new).is_err() {
                return all_left(Why::NotAllowed, matches);
            }
            Ok((matches, Vec::new(), Some(new)))
        }
        How::Cell { how: CellHow::Computed, .. } => all_left(Why::Computed, matches),
        How::Cell { how, .. } => {
            let new_len = grown(&matches);
            if new_len > MAX_TEXT_CHARS && new_len > old {
                return all_left(Why::TooLong, matches);
            }
            charge_bytes(spent, bound_of(src, matches.len(), with))?;
            let new = rebuilt(&src.text, &matches, with);
            let text_ok = crate::draw::text_edit::check_text(&new).is_ok();
            let reads_ok = *how == CellHow::Literal || stays_text(&new);
            if !text_ok || !reads_ok {
                return all_left(Why::NotAllowed, matches);
            }
            Ok((matches, Vec::new(), Some(new)))
        }
    }
}

/// Replaces the matches of `pattern` in `scope` by `with`, or the one match `at` (`(spot, start, end)`).
///
/// With `at`, the match must still be there, and if it is one that is left (a field's, a computed cell's) that is the error. Without it every match is replaced, those
/// that cannot be being listed in [`Replaced::skipped`]. The document is as it was if this is an error. `with` is checked like any text of a shape
/// (no control characters but a tab and a line feed). An empty `with` takes the matches out.
pub fn replace(
    doc: &mut Document,
    scope: &Scope,
    pattern: &Pattern,
    with: &str,
    within: Within,
    at: Option<(&Spot, usize, usize)>,
) -> Result<Replaced, Problem> {
    check_scope(doc, scope)?;
    crate::draw::text_edit::check_text(with)?;
    let mut work = Work::new(MAX_FIND_STEPS);
    let mut plans: Vec<ShapePlan> = Vec::new();
    let mut skipped_all: Vec<Skipped> = Vec::new();
    let mut skipped_total = 0_usize;
    let mut planned = 0_usize;
    // The bytes of text the plan will make, charged before each is made ([`MAX_REWRITE_BYTES`]).
    let mut spent = 0_u64;
    let mut refusal: Option<Problem> = None;
    // The matches on locked layers are left, and counted: each shape by its own list, a member of a group by its own.
    let mut locks = crate::draw::layers::Locks::default();
    let mut locked = 0_usize;
    {
        let shared: &Document = doc;
        let resolver = diacraft_doc::Resolver::new(shared);
        walk(shared, scope, &mut work, &mut |visit, work| {
            if let Some((spot, _, _)) = at
                && (visit.page != spot.page || visit.shape.id != spot.id)
            {
                return Ok(Flow::Next);
            }
            let on_locked = match locks.of(shared, visit.page, visit.shape) {
                Ok(layer) => layer.is_some(),
                Err(problem) => {
                    refusal = Some(problem);
                    return Ok(Flow::Stop);
                }
            };
            let list = sources(&resolver, visit, within, work)?;
            let mut places: Vec<PlacePlan> = Vec::new();
            for src in list {
                if let Some((spot, _, _)) = at
                    && (src.kind != spot.kind || src.row != spot.row)
                {
                    continue;
                }
                let mut matches: Vec<(usize, usize)> = Vec::new();
                let mut too_many = false;
                pattern.find_in(&src.text, 0, work, |s, e| {
                    if planned.saturating_add(matches.len()) >= MAX_REPLACEMENTS {
                        too_many = true;
                        return false;
                    }
                    matches.push((s, e));
                    true
                })?;
                if too_many {
                    refusal = Some(Problem::failed(format!(
                        "more than {MAX_REPLACEMENTS} matches: replace in the selection or one page, or look for something longer"
                    )));
                    return Ok(Flow::Stop);
                }
                if let Some((_, start, end)) = at {
                    matches.retain(|&(s, e)| s == start && e == end);
                }
                if matches.is_empty() {
                    continue;
                }
                // A shape on a locked layer is not edited: every match of it is left, with nothing planned or charged for it.
                let planned_place = if on_locked {
                    Ok((Vec::new(), matches.into_iter().map(|(s, e)| (s, e, Why::Locked)).collect(), None))
                } else {
                    plan_place(&src, matches, with, &mut spent)
                };
                let (apply, left, new_text) = match planned_place {
                    Ok(planned_place) => planned_place,
                    Err(problem) => {
                        refusal = Some(problem);
                        return Ok(Flow::Stop);
                    }
                };
                if on_locked {
                    locked = locked.saturating_add(left.len());
                }
                // Every match counts towards the cap, those that are left as well as those that are replaced.
                planned = planned.saturating_add(apply.len()).saturating_add(left.len());
                let spot = Spot { page: visit.page, id: visit.shape.id, kind: src.kind, row: src.row.clone() };
                for (start, end, why) in left {
                    skipped_total = skipped_total.saturating_add(1);
                    if skipped_all.len() < MAX_SKIPPED_LISTED {
                        skipped_all.push(Skipped { spot: spot.clone(), start, end, why });
                    }
                }
                if !apply.is_empty() {
                    let how = match &src.how {
                        How::Text { .. } => How::Text { fields: Vec::new() },
                        other => other.clone(),
                    };
                    places.push(PlacePlan { kind: src.kind, row: src.row.clone(), how, apply, new_text });
                }
            }
            if !places.is_empty() {
                plans.push(ShapePlan { page: visit.page, path: visit.path.to_vec(), id: visit.shape.id, places });
            }
            Ok(if at.is_some() { Flow::Stop } else { Flow::Next })
        })
        .map_err(|Refused| too_big())?;
    }
    if let Some(problem) = refusal {
        return Err(problem);
    }
    if let Some((spot, start, end)) = at
        && plans.is_empty()
    {
        return Err(match skipped_all.first() {
            Some(left) => Problem::failed(format!("that match is not replaced: {}", left.why.words())),
            None => Problem::failed(format!(
                "the text at {start} to {end} is no longer a match: it changed, or the place ({}) is not there",
                spot.kind.word()
            )),
        });
    }
    let steps = work.used();
    let mut result = Replaced { skipped: skipped_all, skipped_total, locked, steps, unread: work.unread(), ..Replaced::default() };
    if plans.is_empty() {
        return Ok(result);
    }
    // The writes. Names and texts are written to the shapes; cells go through the document's batch of writes, which judges a guard.
    let was = doc.clone();
    let mut cell_writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    let mut touched: Vec<(SheetRef, CellAddr)> = Vec::new();
    let mut cell_places: Vec<(usize, usize)> = Vec::new();
    for (shape_ix, plan) in plans.iter().enumerate() {
        let sheet_ref = ShapeRef { owner: Owner::Page(plan.page), path: plan.path.clone() };
        for (place_ix, place) in plan.places.iter().enumerate() {
            match (&place.how, &place.new_text) {
                (How::Name, Some(new)) => {
                    let shape = shape_mut(doc, plan.page, &plan.path).ok_or_else(|| Problem::failed("a shape went away"))?;
                    let old = shape.name.as_deref().filter(|n| !n.is_empty()).map(str::to_owned);
                    let new_name: Arc<str> = Arc::from(new.as_str());
                    match old {
                        Some(old) => {
                            if shape.name_u.as_deref() == Some(old.as_str()) {
                                shape.name_u = Some(Arc::clone(&new_name));
                            }
                            shape.name = Some(new_name);
                        }
                        None => shape.name_u = Some(new_name),
                    }
                }
                (How::Text { .. }, _) => {
                    let Some(text) = doc.shape(&sheet_ref).and_then(|shape| shape.text.as_ref()) else { continue };
                    let parts = rewrite_parts(&text.parts, &place.apply, with);
                    // As `set_text_range`: the ODF data of the text goes with it, for the writer to match its paragraphs by content, and a text replaced to nothing stays an empty `Text`
                    // of its own over a master's text.
                    let ext = text.ext.clone();
                    let kept = kept_text(doc, &sheet_ref, parts, ext);
                    let shape = shape_mut(doc, plan.page, &plan.path).ok_or_else(|| Problem::failed("a shape went away"))?;
                    shape.text = kept;
                    touched.push((SheetRef::Shape(sheet_ref.clone()), CellAddr::direct("TheText")));
                }
                (How::Cell { key, how }, Some(new)) => {
                    let section = if place.kind == Kind::Data { "Property" } else { "User" };
                    let addr = CellAddr::in_row(section, None, key.clone(), "Value");
                    let edit = match how {
                        CellHow::Literal => {
                            CellEdit::Formula { formula: Arc::new(diacraft_formula::parse(&quoted(new))), value: Some(Value::from_text(new)) }
                        }
                        _ => CellEdit::Value(Value::from_text(new)),
                    };
                    cell_writes.push((SheetRef::Shape(sheet_ref.clone()), CellWrite { addr: addr.clone(), edit, origin: WriteOrigin::Drawing }));
                    touched.push((SheetRef::Shape(sheet_ref.clone()), addr));
                    cell_places.push((shape_ix, place_ix));
                }
                _ => {}
            }
        }
    }
    let outcomes = doc.write_batch(cell_writes).map_err(Problem::from)?;
    let mut guarded: Vec<(usize, usize)> = Vec::new();
    for (outcome, at_place) in outcomes.iter().zip(&cell_places) {
        if *outcome != WriteOutcome::Written {
            guarded.push(*at_place);
        }
    }
    // The tally. A cell that a guard kept is a match left.
    let mut pages: Vec<u32> = Vec::new();
    let mut shapes = 0_usize;
    for (shape_ix, plan) in plans.iter().enumerate() {
        let mut shape_changed = false;
        for (place_ix, place) in plan.places.iter().enumerate() {
            if guarded.contains(&(shape_ix, place_ix)) {
                for &(start, end) in &place.apply {
                    result.skipped_total = result.skipped_total.saturating_add(1);
                    if result.skipped.len() < MAX_SKIPPED_LISTED {
                        let spot = Spot { page: plan.page, id: plan.id, kind: place.kind, row: place.row.clone() };
                        result.skipped.push(Skipped { spot, start, end, why: Why::NotAllowed });
                    }
                }
                continue;
            }
            result.replaced = result.replaced.saturating_add(place.apply.len());
            shape_changed = true;
        }
        if shape_changed {
            shapes += 1;
            if !pages.contains(&plan.page) {
                pages.push(plan.page);
            }
        }
    }
    result.shapes = shapes;
    let by_page: Vec<(u32, Vec<(SheetRef, CellAddr)>)> = pages
        .iter()
        .map(|page| {
            let cells = touched.iter().filter(|(sheet, _)| matches!(sheet, SheetRef::Shape(at) if at.owner == Owner::Page(*page))).cloned().collect();
            (*page, cells)
        })
        .collect();
    // The connectors glued to the shapes the recalculation moved are routed again within one budget for all the pages.
    settle_cells_on_pages(doc, &was, by_page)?;
    result.pages = pages;
    if let (Some((spot, start, _)), Some(plan)) = (at, plans.first())
        && result.replaced > 0
    {
        result.next = Some(Cursor { spot: Spot { page: plan.page, ..spot.clone() }, offset: start.saturating_add(with.chars().count()) });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> TextPart {
        TextPart::Chars(Arc::from(s))
    }

    fn fld(s: &str) -> TextPart {
        TextPart::Fld { ix: 0, text: Arc::from(s) }
    }

    /// What a list of parts shows: the characters and the fields' results.
    fn shown(parts: &[TextPart]) -> String {
        parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(s) => s.to_string(),
                TextPart::Fld { text, .. } => text.to_string(),
                _ => String::new(),
            })
            .collect()
    }

    fn words(parts: &[TextPart]) -> Vec<String> {
        parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(s) => format!("{s:?}"),
                TextPart::Cp(i) => format!("cp{i}"),
                TextPart::Fld { text, .. } => format!("fld={text}"),
                _ => "other".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_string_is_rebuilt_in_one_pass_with_its_matches_replaced() {
        assert_eq!(rebuilt("hello world", &[(0, 5)], "bye"), "bye world");
        assert_eq!(rebuilt("hello world", &[(0, 5), (6, 11)], ""), " ");
        assert_eq!(rebuilt("héllo wörld", &[(1, 2), (7, 8)], "o"), "hollo world", "characters, not bytes");
        assert_eq!(rebuilt("abc", &[], "x"), "abc");
        assert_eq!(rebuilt("abc", &[(0, 3)], "xy"), "xy");
    }

    #[test]
    fn the_parts_of_a_text_are_rewritten_keeping_their_markers_and_fields() {
        let parts = vec![TextPart::Cp(0), chars("Hello world"), TextPart::Cp(1), chars(" Hello"), fld("42")];
        // Two matches, one in each run.
        let out = rewrite_parts(&parts, &[(0, 5), (12, 17)], "Bye");
        assert_eq!(words(&out), ["cp0", "\"Bye world\"", "cp1", "\" Bye\"", "fld=42"]);
        // A match across the runs: the new text joins the first, the second keeps its marker.
        let out = rewrite_parts(&parts, &[(10, 13)], "D-H");
        assert_eq!(words(&out), ["cp0", "\"Hello worlD-H\"", "cp1", "\"ello\"", "fld=42"]);
        // A match that takes a whole run out leaves its marker and no characters.
        let out = rewrite_parts(&parts, &[(11, 17)], "");
        assert_eq!(words(&out), ["cp0", "\"Hello world\"", "cp1", "fld=42"]);
        // A match across three runs.
        let three = vec![chars("ab"), TextPart::Cp(1), chars("cd"), TextPart::Cp(2), chars("ef")];
        let out = rewrite_parts(&three, &[(1, 5)], "X");
        assert_eq!(words(&out), ["\"aX\"", "cp1", "cp2", "\"f\""]);
    }

    /// The parts of a text and a list of matches, made from a number: the characters shown by the parts are what `rebuilt` gives for the same matches.
    #[test]
    fn rewriting_parts_agrees_with_rebuilding_the_shown_text_for_many_shapes_of_text() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _ in 0..2_000 {
            // Parts: runs of one to six characters from a small alphabet, markers between, and a field now and then.
            let mut parts: Vec<TextPart> = Vec::new();
            let mut fields: Vec<(usize, usize)> = Vec::new();
            let mut at = 0_usize;
            for _ in 0..(1 + next(6)) {
                let run: String = (0..(1 + next(6))).map(|_| ['a', 'b', 'é', ' ', '🙂'][next(5) as usize]).collect();
                at += run.chars().count();
                parts.push(chars(&run));
                if next(3) == 0 {
                    parts.push(TextPart::Cp(1));
                }
                if next(5) == 0 {
                    let f = "99";
                    fields.push((at, at + 2));
                    at += 2;
                    parts.push(fld(f));
                }
            }
            let text = shown(&parts);
            let total = text.chars().count();
            // Matches: non-overlapping spans that touch no field.
            let mut spans: Vec<(usize, usize)> = Vec::new();
            let mut pos = 0_usize;
            while pos < total {
                let start = pos + next(4) as usize;
                let end = start + 1 + next(4) as usize;
                if end > total {
                    break;
                }
                if !fields.iter().any(|&(fs, fe)| start < fe && end > fs) {
                    spans.push((start, end));
                    pos = end;
                } else {
                    pos = start + 1;
                }
            }
            let with = ["", "X", "XYZ", "é🙂"][next(4) as usize];
            let out = rewrite_parts(&parts, &spans, with);
            assert_eq!(shown(&out), rebuilt(&text, &spans, with), "parts {parts:?}, spans {spans:?}, with {with:?}");
            // Every marker and field is still there.
            let kept = |list: &[TextPart]| list.iter().filter(|p| !matches!(p, TextPart::Chars(_))).count();
            assert_eq!(kept(&out), kept(&parts));
        }
    }

    #[test]
    fn a_match_is_left_when_it_touches_a_field_and_the_walk_over_the_fields_is_in_order() {
        let fields = [(5, 7), (12, 13)];
        let mut fi = 0;
        let left: Vec<bool> =
            [(0, 4), (4, 6), (7, 12), (12, 14), (14, 20), (20, 22)].iter().map(|&(s, e)| touches_field(&fields, &mut fi, s, e)).collect();
        assert_eq!(left, [false, true, false, true, false, false]);
    }
}
