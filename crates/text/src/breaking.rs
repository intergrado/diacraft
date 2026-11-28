//! Cutting a paragraph into lines.
//!
//! Greedy, a line at a time, in logical order: a line takes clusters until the next would run past the measure, then breaks at the
//! last place the Unicode line breaking rules (UAX #14) allow, and inside the word if there is none. Spaces at the end of a line stay
//! on it and don't count. A forced break (U+2028) ends a line and starts the next with no first-line indent. The
//! measure may be zero or negative: each line then takes one cluster and the loop still ends.
//!
//! Every cluster is looked at once, or twice where a line is cut short of where the scan reached, so the work is linear in the
//! paragraph. Tab widths depend on where the pen is, so they are worked out here and kept for placing.

use crate::formats::TabAlign;
use crate::prepare::Kind;
use crate::shape::Shaped;

/// Rounding slack when comparing a width with the measure, in inches.
pub(crate) const EPS: f64 = 1e-9;

/// One line of a paragraph, as cluster indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineSpan {
    pub start: usize,
    /// The end of what is drawn: the clusters `start..end`, with the trailing spaces taken off.
    pub end: usize,
    /// The end including the trailing spaces, before the break that ended the line.
    pub full_end: usize,
    /// Where the next line starts.
    pub next: usize,
    /// The line was ended by a forced break, not by running out of room or text.
    pub forced: bool,
    /// The line was cut at a soft hyphen, which draws its hyphen.
    pub hyphen: bool,
}

/// Where lines start and how long they may be. `x` is where the line's text starts, measured from the text area's near edge (its left edge, or its right
/// edge in a right-to-left paragraph, which is laid out as the mirror image: `place.rs`), which is where tab stops are measured from; `avail` is how much
/// room it has.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Measure {
    pub first_x: f64,
    pub first_avail: f64,
    pub rest_x: f64,
    pub rest_avail: f64,
}

/// The tab stops of a paragraph.
#[derive(Clone, Copy)]
pub(crate) struct Tabs<'a> {
    pub stops: &'a [(f64, TabAlign)],
    pub default: f64,
}

/// Where lines may break: `allowed[i]` means a line may start at cluster `i`. Unicode line breaking, except that nothing may break
/// straight after a tab (break only at spaces), and a no-break space, a soft hyphen and a zero-width space take their
/// Unicode behaviour.
pub(crate) fn break_opportunities(shaped: &Shaped) -> Vec<bool> {
    let n = shaped.clusters.len();
    let mut allowed = vec![false; n + 1];
    for (byte, _) in unicode_linebreak::linebreaks(&shaped.text) {
        let i = shaped.clusters.partition_point(|c| c.byte < byte);
        if shaped.clusters.get(i).is_some_and(|c| c.byte == byte)
            && let Some(a) = allowed.get_mut(i)
        {
            *a = true;
        }
    }
    for i in 1..n {
        if shaped.clusters.get(i - 1).is_some_and(|c| c.kind == Kind::Tab)
            && let Some(a) = allowed.get_mut(i)
        {
            *a = false;
        }
    }
    allowed
}

/// The stop a tab at pen `pen` goes to: the first custom stop past it, else the next default step, and how text sits on it.
/// `None` when there is no stop at all, which is a default step of 0 and no custom stop.
fn next_stop(pen: f64, tabs: &Tabs<'_>) -> Option<(f64, TabAlign)> {
    let custom = tabs.stops.iter().find(|s| s.0 > pen + EPS).copied();
    let default = (tabs.default.is_finite() && tabs.default > EPS).then(|| (((pen + EPS) / tabs.default).floor() + 1.0) * tabs.default);
    // A custom stop ahead of the pen is the stop, however near a default step is; the steps apply past the last custom stop.
    custom.or(default.map(|d| (d, TabAlign::Left)))
}

/// The width of the text after the tab at `i`, up to the next tab or forced break, and its width before its first full stop.
fn segment_after(shaped: &Shaped, i: usize) -> (f64, f64) {
    let mut width = 0.0;
    let mut dot = None;
    for c in shaped.clusters.iter().skip(i + 1) {
        crate::work::cluster_step();
        if matches!(c.kind, Kind::Tab | Kind::SoftBreak) {
            break;
        }
        if dot.is_none() && shaped.text.get(c.byte..).is_some_and(|t| t.starts_with('.')) {
            dot = Some(width);
        }
        width += c.adv;
    }
    (width, dot.unwrap_or(width))
}

/// How far a tab at cluster `i` moves the pen from `pen`: to its stop, with the text after it aligned as the stop says, and never
/// backwards. With no stop to go to it is one space.
fn tab_advance(shaped: &Shaped, i: usize, pen: f64, tabs: &Tabs<'_>) -> f64 {
    let space = shaped.clusters.get(i).map_or(0.0, |c| c.adv);
    let Some((pos, align)) = next_stop(pen, tabs) else { return space };
    let (width, dot) = segment_after(shaped, i);
    let start = match align {
        TabAlign::Left => pos,
        TabAlign::Centre => pos - width / 2.0,
        TabAlign::Right => pos - width,
        TabAlign::Decimal => pos - dot,
    };
    (start - pen).max(0.0)
}

/// Fit one line starting at cluster `start`.
fn fit_line(shaped: &Shaped, allowed: &[bool], start: usize, x0: f64, avail: f64, tabs: &Tabs<'_>, tab_adv: &mut [f64]) -> LineSpan {
    let n = shaped.clusters.len();
    let mut x = 0.0;
    let mut last_break: Option<usize> = None;
    let mut i = start;
    let (full_end, next, forced, overflowed) = loop {
        let Some(c) = shaped.clusters.get(i) else { break (n, n, false, false) };
        crate::work::cluster_step();
        if c.kind == Kind::SoftBreak {
            break (i, i + 1, true, false);
        }
        if i > start && allowed.get(i).copied().unwrap_or(false) {
            // A break after a soft hyphen draws a hyphen, which has to fit as well.
            let hyphen = shaped.clusters.get(i - 1).filter(|p| p.kind == Kind::SoftHyphen).map_or(0.0, |p| p.hyphen);
            if x + hyphen <= avail + EPS {
                last_break = Some(i);
            }
        }
        let w = if c.kind == Kind::Tab {
            let a = tab_advance(shaped, i, x0 + x, tabs);
            if let Some(slot) = tab_adv.get_mut(i) {
                *slot = a;
            }
            a
        } else {
            c.adv
        };
        if !c.space && i > start && x + w > avail + EPS {
            let at = last_break.filter(|b| *b > start).unwrap_or(i);
            break (at, at, false, true);
        }
        x += w;
        i += 1;
    };
    let mut end = full_end;
    while end > start && shaped.clusters.get(end - 1).is_some_and(|c| c.space) {
        end -= 1;
    }
    let hyphen = overflowed && end > start && shaped.clusters.get(end - 1).is_some_and(|c| c.kind == Kind::SoftHyphen);
    LineSpan { start, end, full_end, next: next.max(start + 1).min(n.max(start + 1)), forced, hyphen }
}

/// Cut a paragraph into lines, at most `max_lines` of them, filling `tab_adv` (one entry for each cluster) with the width of
/// every tab.
pub(crate) fn break_lines(
    shaped: &Shaped,
    allowed: &[bool],
    measure: &Measure,
    tabs: &Tabs<'_>,
    tab_adv: &mut [f64],
    max_lines: usize,
) -> Vec<LineSpan> {
    let n = shaped.clusters.len();
    if n == 0 {
        return vec![LineSpan { start: 0, end: 0, full_end: 0, next: 0, forced: false, hyphen: false }];
    }
    let mut spans = Vec::new();
    let mut start = 0;
    while start < n && spans.len() < max_lines {
        let (x0, avail) = if start == 0 { (measure.first_x, measure.first_avail) } else { (measure.rest_x, measure.rest_avail) };
        let span = fit_line(shaped, allowed, start, x0, avail, tabs, tab_adv);
        start = span.next;
        spans.push(span);
        // A forced break at the very end opens an empty last line: a break moves the text to a new line.
        if span.forced && span.next >= n && spans.len() < max_lines {
            spans.push(LineSpan { start: n, end: n, full_end: n, next: n, forced: false, hyphen: false });
        }
    }
    spans
}
