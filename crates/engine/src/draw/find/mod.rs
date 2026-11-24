//! Find and Replace (and "Replace text").
//!
//! * [`find`] lists the matches of a pattern in a scope (the shapes selected, one page, or every page) in document order, the first after a place if one is given, and
//!   goes round to the start when it reaches the end ([`Outcome::wrapped`]): Find Next is `find` with the place of the match it is at and a limit of one.
//! * [`replace`] replaces every match, or the one match it is given, in the same places, as one edit of the document.
//!
//! **Where it looks** ([`Within`]): shape text (what a text block shows, a field's result included), shape names, the values of shape data rows and the values of
//! user-defined cells ([`source`]). **What matches** is [`pattern`]'s: whole words, match case and the four special characters. The specification also lists Match
//! character width and Match Kana type, which are for Japanese text and appear only when the Regional option for Kana is on; this build has neither (the window
//! has no such option), and says so in its documentation.
//!
//! **Hostile drawings.** Every walk is charged to one [`Work`] of [`MAX_FIND_STEPS`] steps before it is done (8 steps for each shape, one for each character and 400 for each row of shape data read) and refused
//! in words past it; a listing is at most [`MAX_LISTED`] matches; a replacement is at most [`MAX_REPLACEMENTS`] matches; and a replacement in a text is made in one
//! pass over the text, so a Replace All of a hundred thousand matches in one text is not a hundred thousand edits. The cost is counted in `tests/it/find_cost.rs`.

mod pattern;
mod replace;
mod source;
mod walk;

pub use pattern::{MAX_PATTERN_CHARS, Pattern, Refused, Work};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use replace::touches_field;
pub use replace::{Replaced, Skipped, replace};
pub use source::{CellHow, Kind, MAX_ROWS_PER_SHAPE, ROW_STEPS, Why, Within};
pub use walk::Scope;

use diacraft_doc::Document;

use super::Problem;
use source::{Source, sources};
use walk::{Flow, Visit, walk};

/// The most steps one search or replacement may take.
pub const MAX_FIND_STEPS: u64 = 1 << 29;

/// The most matches one listing returns.
pub const MAX_LISTED: usize = 10_000;

/// The most matches one Replace All replaces: a hostile drawing could hold millions.
pub const MAX_REPLACEMENTS: usize = 1_000_000;

/// How many characters of text are kept either side of a match for [`Found::context`].
const CONTEXT_CHARS: usize = 20;

/// A place of a shape that holds text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    /// The `ID` of the page.
    pub page: u32,
    /// The `ID` of the shape: the first with it on the page.
    pub id: u32,
    pub kind: Kind,
    /// The row's name, for shape data and user-defined cells.
    pub row: Option<String>,
}

/// A match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub spot: Spot,
    /// The shape's name, when it has one.
    pub shape_name: Option<String>,
    /// The first character of the match, counted in characters of the text the place shows, from 0.
    pub start: usize,
    /// One past the last.
    pub end: usize,
    /// The text around the match, made safe to show.
    pub context: String,
}

/// Where a search goes on from: matches that begin at `offset` or after it in `spot`, and everything after `spot`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub spot: Spot,
    pub offset: usize,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The matches, in document order from the cursor, and then those before it if the search went round.
    pub found: Vec<Found>,
    /// There are more matches than were listed.
    pub truncated: bool,
    /// The list goes on, after the end of the scope, from its start.
    pub wrapped: bool,
    /// How many shapes were looked at.
    pub shapes: u64,
    /// The steps the search took.
    pub steps: u64,
    /// How many sections of shape data or user-defined cells were not read because a shape had more than [`MAX_ROWS_PER_SHAPE`] rows in one.
    pub unread: u64,
}

/// The words for a search that passed its budget.
pub(crate) fn too_big() -> Problem {
    Problem::failed(format!(
        "the search is too large to finish ({MAX_FIND_STEPS} steps): search the selection or one page, or look for something longer"
    ))
}

/// Refuses a scope that names a page that is not there.
pub(crate) fn check_scope(doc: &Document, scope: &Scope) -> Result<(), Problem> {
    match scope {
        Scope::All => Ok(()),
        Scope::Page(id) | Scope::Selection { page: id, .. } => {
            doc.page(*id).map(|_| ()).ok_or_else(|| Problem::bad(format!("there is no page {id}")))
        }
    }
}

/// Which places of a shape a search covers.
#[derive(Clone, Debug)]
enum Mode {
    /// All of them, from the first character.
    All,
    /// The shape is the cursor's: the places before the cursor's are left, and the cursor's is searched from its offset.
    After(Cursor),
    /// The shape is the cursor's, on the way round: the places before the cursor's, and the matches of the cursor's that end at its offset or before.
    Before(Cursor),
}

/// The place of `spot` among `sources`: where it is, or where the first place of a later kind is, and whether it is exactly it.
fn place_of(sources: &[Source<'_>], spot: &Spot) -> (usize, bool) {
    match sources.iter().position(|s| s.kind == spot.kind && s.row == spot.row) {
        Some(at) => (at, true),
        None => (sources.iter().position(|s| s.kind > spot.kind).unwrap_or(sources.len()), false),
    }
}

/// The excerpts of `hay` around `spans` (sorted, not overlapping), `CONTEXT_CHARS` either side, made safe to show. One pass over the text, two cursors that only go
/// forward: a long text with many matches is not walked once for each.
fn contexts(hay: &str, spans: &[(usize, usize)]) -> Vec<String> {
    struct Cur {
        chars: usize,
        byte: usize,
    }
    fn to(hay: &str, cur: &mut Cur, target: usize) -> usize {
        if target > cur.chars {
            let rest = hay.get(cur.byte..).unwrap_or("");
            let skip = target - cur.chars;
            let (bytes, chars) = match rest.char_indices().nth(skip) {
                Some((b, _)) => (b, skip),
                None => (rest.len(), rest.chars().count()),
            };
            cur.byte = cur.byte.saturating_add(bytes);
            cur.chars = cur.chars.saturating_add(chars);
        }
        cur.byte
    }
    let (mut lo, mut hi) = (Cur { chars: 0, byte: 0 }, Cur { chars: 0, byte: 0 });
    spans
        .iter()
        .map(|&(start, end)| {
            let from = to(hay, &mut lo, start.saturating_sub(CONTEXT_CHARS));
            let upto = to(hay, &mut hi, end.saturating_add(CONTEXT_CHARS));
            crate::text::shown(hay.get(from..upto.max(from)).unwrap_or(""), 2 * CONTEXT_CHARS + 80)
        })
        .collect()
}

/// What a search of one shape is told: the pattern, where to look, and how many matches it may still take.
struct Ask<'p> {
    pattern: &'p Pattern,
    within: Within,
    /// The most matches the whole search may collect (one more than is listed).
    room: usize,
}

/// The matches in the places of the shape in `visit`, appended to `into` up to `ask.room` in all. → whether `into` is full.
fn search_shape<'a>(
    resolver: &diacraft_doc::Resolver<'a>,
    visit: &Visit<'a, '_>,
    ask: &Ask<'_>,
    mode: &Mode,
    into: &mut Vec<Found>,
    work: &mut Work,
) -> Result<bool, Refused> {
    let list = sources(resolver, visit, ask.within, work)?;
    if list.is_empty() {
        return Ok(false);
    }
    let (first, exact) = match mode {
        Mode::All => (0, false),
        Mode::After(cursor) | Mode::Before(cursor) => place_of(&list, &cursor.spot),
    };
    // The places covered: from the cursor's on, or (on the way round) up to and including the cursor's.
    let range = match mode {
        Mode::Before(_) => 0..first.saturating_add(usize::from(exact)).min(list.len()),
        _ => first..list.len(),
    };
    let shape_name = visit.shape.name.as_deref().or(visit.shape.name_u.as_deref()).map(str::to_owned);
    for at in range {
        let Some(src) = list.get(at) else { break };
        let (from, limit_end) = match mode {
            Mode::After(cursor) if exact && at == first => (cursor.offset, usize::MAX),
            Mode::Before(cursor) if exact && at == first => (0, cursor.offset),
            _ => (0, usize::MAX),
        };
        let left = ask.room.saturating_sub(into.len());
        let mut spans: Vec<(usize, usize)> = Vec::new();
        ask.pattern.find_in(&src.text, from, work, |start, end| {
            if end > limit_end {
                return false;
            }
            spans.push((start, end));
            spans.len() < left
        })?;
        if spans.is_empty() {
            continue;
        }
        let spot = Spot { page: visit.page, id: visit.shape.id, kind: src.kind, row: src.row.clone() };
        for (span, context) in spans.iter().zip(contexts(&src.text, &spans)) {
            into.push(Found { spot: spot.clone(), shape_name: shape_name.clone(), start: span.0, end: span.1, context });
        }
        if into.len() >= ask.room {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the shape in `visit` is the one a cursor is at.
fn is_cursor(visit: &Visit<'_, '_>, cursor: &Cursor) -> bool {
    visit.page == cursor.spot.page && visit.shape.id == cursor.spot.id
}

/// The matches of `pattern` in `scope`, at most `limit` (from 1 to [`MAX_LISTED`]).
///
/// With a `cursor` the list starts at the first match after it, and with `wrap` it goes on, at the end of the scope, from the start up to the cursor
/// ([`Outcome::wrapped`] says it did). A cursor whose shape is not in the scope (it was deleted, or is not selected) is no cursor: the list starts at the start.
pub fn find(
    doc: &Document,
    scope: &Scope,
    pattern: &Pattern,
    within: Within,
    cursor: Option<&Cursor>,
    limit: usize,
    wrap: bool,
) -> Result<Outcome, Problem> {
    check_scope(doc, scope)?;
    let limit = limit.clamp(1, MAX_LISTED);
    // One more than is listed tells whether there are more.
    let ask = Ask { pattern, within, room: limit.saturating_add(1) };
    let resolver = diacraft_doc::Resolver::new(doc);
    let mut work = Work::new(MAX_FIND_STEPS);
    let mut found: Vec<Found> = Vec::new();
    let (shapes, first_pass) = run(doc, scope, &ask, cursor, wrap, &resolver, &mut work, &mut found).map_err(|Refused| too_big())?;
    let truncated = found.len() > limit;
    found.truncate(limit);
    // The list went round when what it lists holds a match from the second pass, which is whatever comes after the first pass's.
    let wrapped = first_pass.is_some_and(|n| found.len() > n);
    Ok(Outcome { found, truncated, wrapped, shapes, steps: work.used(), unread: work.unread() })
}

/// The passes of [`find`]. → the shapes looked at, and how many matches the first pass found when there was a second one (`None`: there was not).
#[allow(clippy::too_many_arguments)]
fn run<'a>(
    doc: &'a Document,
    scope: &Scope,
    ask: &Ask<'_>,
    cursor: Option<&Cursor>,
    wrap: bool,
    resolver: &diacraft_doc::Resolver<'a>,
    work: &mut Work,
    found: &mut Vec<Found>,
) -> Result<(u64, Option<usize>), Refused> {
    let mut shapes = 0_u64;
    let all = |found: &mut Vec<Found>, work: &mut Work| -> Result<u64, Refused> {
        walk(doc, scope, work, &mut |visit, work| {
            let full = search_shape(resolver, visit, ask, &Mode::All, found, work)?;
            Ok(if full { Flow::Stop } else { Flow::Next })
        })
    };
    let Some(cursor) = cursor else {
        return Ok((all(found, work)?, None));
    };
    // The first pass: from the cursor to the end of the scope.
    let mut reached = false;
    shapes = shapes.saturating_add(walk(doc, scope, work, &mut |visit, work| {
        let mode = if reached {
            Mode::All
        } else if is_cursor(visit, cursor) {
            reached = true;
            Mode::After(cursor.clone())
        } else {
            return Ok(Flow::Next);
        };
        let full = search_shape(resolver, visit, ask, &mode, found, work)?;
        Ok(if full { Flow::Stop } else { Flow::Next })
    })?);
    if !reached {
        // The cursor's shape is not in the scope: this is a search from the start.
        return Ok((shapes.saturating_add(all(found, work)?), None));
    }
    if !wrap || found.len() >= ask.room {
        return Ok((shapes, None));
    }
    // The second pass, round to the start: the shapes before the cursor's, then the places of its own that come before the cursor.
    let before = found.len();
    let mut at_cursor = false;
    shapes = shapes.saturating_add(walk(doc, scope, work, &mut |visit, work| {
        let mode = if is_cursor(visit, cursor) {
            at_cursor = true;
            Mode::Before(cursor.clone())
        } else {
            Mode::All
        };
        let full = search_shape(resolver, visit, ask, &mode, found, work)?;
        Ok(if full || at_cursor { Flow::Stop } else { Flow::Next })
    })?);
    Ok((shapes, Some(before)))
}
