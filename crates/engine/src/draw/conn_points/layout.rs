//! The arithmetic of a doubling and a halving: portions, places, generations, the rows after a halving, the formulas of a new point and the successor of a removed one
//! (design §3 to §6). Nothing here reads a document: the functions take
//! numbers and give numbers or text, so each can be checked on its own.
//!
//! * **Portions and places.** A doubling puts a new point in the middle, by arc length, of every portion of an edge ([`new_places`]): an open edge's N points have N + 1 portions
//!   (the stretches between points and from each end to the point nearest it), a closed edge's N points have N gaps round the curve (the last one past the start), and an empty
//!   closed edge gets two points, its start and halfway round. [`doubling_adds`] counts them and [`fits`] holds the sum to the cap.
//! * **Generations.** A doubling's points are one generation newer than the newest of the edge ([`next_generation`]); a halving takes the newest ([`halving_picks`]) and never an
//!   original, which is generation 0.
//! * **Rows.** A halving closes the gaps its removed rows leave ([`close_gaps`]).
//! * **Formulas.** A new point on a straight edge is a share of the box where both ends of its edge are shares, a plain length where both are plain lengths and a formula over the
//!   cells of the two ends otherwise ([`blend`]); a share is written as the masters write one ([`share_text`]).

use super::MAX_POINTS_PER_SHAPE;
use crate::stencil::number_text;

/// How close two distances may be to be as near: a removed point's successor then goes by the connector's other end, then by the lower row.
const TIE: f64 = 1.0e-9;

/// How many points a doubling of an edge with `points` of them makes (§3.2): the middle of every portion, so an open edge's N become 2N + 1 (it gains N + 1), a closed edge's N
/// become 2N (it gains N), and a closed edge with none gets two, its start and halfway round.
pub fn doubling_adds(closed: bool, points: u32) -> u32 {
    match (closed, points) {
        (false, n) => n.saturating_add(1),
        (true, 0) => 2,
        (true, n) => n,
    }
}

/// Whether a shape with `points` points can take `adds` more without passing [`MAX_POINTS_PER_SHAPE`].
pub fn fits(points: u32, adds: u32) -> bool {
    points.checked_add(adds).is_some_and(|total| total <= MAX_POINTS_PER_SHAPE)
}

/// The generation of the points a doubling makes on an edge whose newest is `newest` (0 when all its points are originals): one more, never 0, and `None` past 255.
pub fn next_generation(newest: u8) -> Option<u8> {
    newest.checked_add(1)
}

/// The newest generation among `gens` (0 is an original and is never newest): `None` when every point is an original.
pub fn newest_generation(gens: &[u8]) -> Option<u8> {
    gens.iter().copied().filter(|g| *g > 0).max()
}

/// For each of `gens`, whether a halving takes it: exactly the points of the newest generation, **never an original**, and none when every point is one.
pub fn halving_picks(gens: &[u8]) -> Vec<bool> {
    let newest = newest_generation(gens);
    gens.iter().map(|g| newest.is_some_and(|n| *g == n)).collect()
}

/// The middle of the portion from `a` to `b` along an edge.
pub fn portion_middle(a: f64, b: f64) -> f64 {
    a + (b - a) / 2.0
}

/// The middle of the gap round a closed edge of `length` from `a` forward to `b` (past the start when `b` is not after `a`; a gap from a place to itself is the whole curve), in
/// `[0, length)`.
pub fn closed_middle(a: f64, b: f64, length: f64) -> f64 {
    let gap = if b > a { b - a } else { b - a + length };
    let middle = a + gap / 2.0;
    if middle >= length { middle - length } else { middle }
}

/// How far along an edge of `length` each point a doubling makes goes, in the order of the portions: `alongs` are the places of the edge's points in their order along it. An
/// open edge gives the middle of each of the N + 1 portions, a closed edge with points the middle of each of its N gaps (the last past the start), and a closed edge with none
/// its start and halfway round.
pub fn new_places(length: f64, closed: bool, alongs: &[f64]) -> Vec<f64> {
    if closed {
        if alongs.is_empty() {
            return vec![0.0, length / 2.0];
        }
        return (0..alongs.len())
            .filter_map(|i| {
                let a = *alongs.get(i)?;
                let b = *alongs.get((i + 1) % alongs.len())?;
                Some(closed_middle(a, b, length))
            })
            .collect();
    }
    let mut bounds = Vec::with_capacity(alongs.len() + 2);
    bounds.push(0.0);
    bounds.extend_from_slice(alongs);
    bounds.push(length);
    bounds.windows(2).filter_map(|pair| if let [a, b] = pair { Some(portion_middle(*a, *b)) } else { None }).collect()
}

/// The row a row takes once the rows of `removed` (sorted, each once) are taken away and the rows after them move up: `ix` less the removed rows before it, and `None` for a removed
/// row. A section numbered from 0 with no gap stays so.
pub fn close_gaps(removed: &[u32], ix: u32) -> Option<u32> {
    let before = removed.partition_point(|r| *r < ix);
    if removed.get(before) == Some(&ix) {
        return None;
    }
    ix.checked_sub(u32::try_from(before).ok()?)
}

/// `k` rounded to the nine places a written share has.
pub fn rounded(k: f64) -> f64 {
    let r = (k * 1.0e9).round() / 1.0e9;
    if r.is_finite() { r } else { 0.0 }
}

/// A share `k` of the box (`of` is `Width` or `Height`) as a cell's formula, written as the masters write one: `None` for 0 (the cell is the constant 0), `of` for 1, else
/// `of*<k>` with nine places.
pub fn share_text(of: &str, k: f64) -> Option<String> {
    let k = rounded(k);
    if k == 0.0 {
        None
    } else if k == 1.0 {
        Some(of.to_owned())
    } else {
        Some(format!("{of}*{}", number_text(k)))
    }
}

/// One end of a straight edge on one axis, as the formula of its cell says.
#[derive(Clone, Debug, PartialEq)]
pub struct EndAxis {
    /// The end as a share of the box, where it is one: the formula `Width` or `Width*k` (`Height` on the other axis), the constant 0, a constant share of a relative row.
    pub share: Option<f64>,
    /// The end as a plain length, where its cell holds no formula.
    pub plain: Option<f64>,
    /// How a formula names the cell the end is read from (`Geometry1.X2`), where it is a cell of a row, and whether that row is relative (its cells are shares of the box).
    pub cell: Option<(String, bool)>,
    /// What the end is now, in the shape's own units.
    pub value: f64,
}

impl EndAxis {
    /// An end read from the cell `name` of a row: its formula text (none for a constant), its value, whether the row is relative (its cell is a share of the box), `of` the
    /// box's size name on this axis and `size` that size.
    pub fn of_cell(formula: Option<&str>, value: f64, relative: bool, of: &str, size: f64, name: String) -> EndAxis {
        let cell = Some((name, relative));
        match (formula.map(str::trim), relative) {
            // A relative row's cell is a share of the box; a constant is the share.
            (None, true) => EndAxis { share: Some(value), plain: None, cell, value: value * size },
            (Some(text), true) => EndAxis { share: text.parse::<f64>().ok().filter(|k| k.is_finite()), plain: None, cell, value: value * size },
            (None, false) => EndAxis { share: (value == 0.0).then_some(0.0), plain: Some(value), cell, value },
            (Some(text), false) => EndAxis { share: share_of(text, of), plain: None, cell, value },
        }
    }

    /// An end that is a number the row's constructor holds: a share of the box when `is_share`, else a plain length.
    pub fn of_number(number: f64, is_share: bool, size: f64) -> EndAxis {
        if is_share {
            EndAxis { share: Some(number), plain: None, cell: None, value: number * size }
        } else {
            EndAxis { share: (number == 0.0).then_some(0.0), plain: Some(number), cell: None, value: number }
        }
    }
}

/// `text` as a share of `of`: `Width` is 1 and `Width*<number>` that number (a plain decimal, no other form).
fn share_of(text: &str, of: &str) -> Option<f64> {
    if text == of {
        return Some(1.0);
    }
    let number = text.strip_prefix(of)?.strip_prefix('*')?;
    let plain = number.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-');
    plain.then(|| number.parse::<f64>().ok()).flatten().filter(|k| k.is_finite())
}

/// What a new point's cell holds: the formula (none for a constant) and its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Blended {
    pub formula: Option<String>,
    pub value: f64,
}

/// The cell of a new point at the share `t` of a straight edge from the end `a` to the end `b` on one axis: both ends shares of the box `of` (of size `size`)
/// give a share, both plain lengths a constant, and anything else a formula over the cells of the two ends (`Geometry1.X2+(Geometry1.X3-Geometry1.X2)*0.25`, inside
/// `Width*(...)` where both ends are cells of relative rows).
pub fn blend(a: &EndAxis, b: &EndAxis, t: f64, of: &str, size: f64) -> Blended {
    let mix = |x: f64, y: f64| (1.0 - t) * x + t * y;
    if let (Some(ka), Some(kb)) = (a.share, b.share) {
        let k = rounded(mix(ka, kb));
        return Blended { formula: share_text(of, k), value: k * size };
    }
    if let (Some(pa), Some(pb)) = (a.plain, b.plain) {
        return Blended { formula: None, value: mix(pa, pb) };
    }
    let value = mix(a.value, b.value);
    // One end that is not a cell of a row (a vertex of a constructor) is written as its number.
    let term = |end: &EndAxis, absolute: bool| match &end.cell {
        Some((name, relative)) if *relative && absolute => format!("({of}*{name})"),
        Some((name, _)) => name.clone(),
        None => number_text(end.value),
    };
    let both_relative = [a, b].iter().all(|e| e.cell.as_ref().is_some_and(|(_, relative)| *relative));
    let t_text = number_text(t);
    if both_relative {
        let (ta, tb) = (term(a, false), term(b, false));
        return Blended { formula: Some(format!("{of}*({ta}+({tb}-{ta})*{t_text})")), value };
    }
    let (ta, tb) = (term(a, true), term(b, true));
    Blended { formula: Some(format!("{ta}+({tb}-{ta})*{t_text}")), value }
}

/// The way out and the type of the points a doubling makes: the way out the edge's points all share (an edge with none, or whose ways differ, has none to take),
/// and the type they share, else 0.
pub fn common_way_out(ways: &[(f64, f64)]) -> Option<(f64, f64)> {
    let first = *ways.first()?;
    ways.iter().all(|w| (w.0 - first.0).abs() <= TIE && (w.1 - first.1).abs() <= TIE).then_some(first)
}

/// The type the points of an edge share, else 0.
pub fn common_kind(kinds: &[f64]) -> f64 {
    match kinds.first() {
        Some(first) if kinds.iter().all(|k| (k - first).abs() <= TIE) => *first,
        _ => 0.0,
    }
}

/// The unit normal that points away from what a run encloses, at a place where the run goes `tangent` (a unit vector): to the right of the way it goes when the run turns
/// counterclockwise (`area` above 0), to the left when it turns clockwise.
pub fn outward(tangent: (f64, f64), area: f64) -> (f64, f64) {
    if area >= 0.0 { (tangent.1, -tangent.0) } else { (-tangent.1, tangent.0) }
}

/// A point that may take the connectors of a removed point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    /// The row, by its number before the halving.
    pub ix: u32,
    /// Where it is in the shape's own frame.
    pub place: (f64, f64),
    /// Where it is on the page.
    pub page: (f64, f64),
}

/// The successor of a removed point at `removed` (in the shape's frame) among `candidates`: the nearest; of two as near, the one nearer the connector's other end `toward` on
/// the page (when there is one to weigh); then the lower row.
pub fn successor(removed: (f64, f64), candidates: &[Candidate], toward: Option<(f64, f64)>) -> Option<Candidate> {
    let far = |c: &Candidate| (c.place.0 - removed.0).hypot(c.place.1 - removed.1);
    let best = candidates.iter().map(far).fold(f64::INFINITY, f64::min);
    if !best.is_finite() {
        return None;
    }
    let near: Vec<&Candidate> = candidates.iter().filter(|c| far(c) <= best + TIE).collect();
    let away = |c: &Candidate| toward.map_or(0.0, |w| (c.page.0 - w.0).hypot(c.page.1 - w.1));
    let closest = near.iter().map(|c| away(c)).fold(f64::INFINITY, f64::min);
    near.into_iter().filter(|c| away(c) <= closest + TIE).min_by_key(|c| c.ix).copied()
}

/// A length in inches as the millimetres a refusal tells: whole when it is, else one place.
pub fn millimetres(inches: f64) -> String {
    let mm = inches * 25.4;
    if (mm - mm.round()).abs() < 0.05 { format!("{}", mm.round()) } else { format!("{mm:.1}") }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_doubling_makes_the_middle_of_every_portion() {
        // Open: N points make 2N + 1, so N + 1 are added: 0 -> 1 -> 3 -> 7.
        assert_eq!([0, 1, 3, 7].map(|n| doubling_adds(false, n)), [1, 2, 4, 8]);
        // Closed: N points have N gaps, so N are added; an empty closed edge gets two.
        assert_eq!([0, 1, 2, 4].map(|n| doubling_adds(true, n)), [2, 1, 2, 4]);
        assert_eq!(doubling_adds(false, u32::MAX), u32::MAX, "no wrap");
    }

    #[test]
    fn the_cap_is_256_points_and_nothing_wraps() {
        assert!(fits(255, 1) && fits(0, 256) && fits(127, 129));
        assert!(!fits(128, 129) && !fits(256, 1) && !fits(u32::MAX, 1) && !fits(1, u32::MAX));
    }

    #[test]
    fn a_generation_is_one_more_than_the_newest_and_never_nought_or_past_255() {
        assert_eq!(next_generation(0), Some(1));
        assert_eq!(next_generation(7), Some(8));
        assert_eq!(next_generation(254), Some(255));
        assert_eq!(next_generation(255), None);
        assert_eq!((1..=255u8).map(|g| next_generation(g - 1).unwrap()).collect::<Vec<_>>(), (1..=255u8).collect::<Vec<_>>());
    }

    #[test]
    fn a_halving_picks_the_newest_generation_and_never_an_original() {
        assert_eq!(newest_generation(&[]), None);
        assert_eq!(newest_generation(&[0, 0, 0]), None, "originals are never newest");
        assert_eq!(newest_generation(&[0, 1, 2, 1]), Some(2));
        assert_eq!(halving_picks(&[0, 1, 2, 1, 2, 0]), [false, false, true, false, true, false]);
        assert_eq!(halving_picks(&[0, 0]), [false, false], "nothing when every point is an original");
        assert_eq!(halving_picks(&[]), Vec::<bool>::new());
        for gens in [[0u8, 0, 3, 3], [255, 0, 1, 255], [1, 1, 1, 1]] {
            let picks = halving_picks(&gens);
            assert!(gens.iter().zip(&picks).all(|(g, p)| !*p || *g > 0), "{gens:?}: an original was picked");
        }
    }

    #[test]
    fn the_middles_of_portions_are_between_their_ends_and_a_closed_gap_may_wrap() {
        assert_eq!(portion_middle(0.0, 2.0), 1.0);
        assert_eq!(portion_middle(3.0, 3.0), 3.0);
        assert_eq!(closed_middle(2.0, 6.0, 10.0), 4.0);
        assert_eq!(closed_middle(8.0, 2.0, 10.0), 0.0, "past the start: 8 to 12 is 10 and back to 0");
        assert_eq!(closed_middle(6.0, 4.0, 10.0), 0.0, "6 to 14 has its middle at 10, which is the start");
        assert_eq!(closed_middle(3.0, 3.0, 10.0), 8.0, "one point's gap is the whole curve");
        for (a, b) in [(0.0, 9.9), (9.9, 0.0), (5.0, 5.0), (0.1, 0.2), (9.0, 1.0)] {
            let m = closed_middle(a, b, 10.0);
            assert!((0.0..10.0).contains(&m), "({a}, {b}) -> {m}");
        }
    }

    #[test]
    fn the_new_places_of_each_kind_of_edge() {
        // Open, no point: its middle. One point in the middle of 2: the middles of both halves. Uneven points 0.8 and 1.2 on a 4 inch edge: 0.4, 1.0 and 2.6.
        assert_eq!(new_places(2.0, false, &[]), [1.0]);
        assert_eq!(new_places(2.0, false, &[1.0]), [0.5, 1.5]);
        let uneven = new_places(4.0, false, &[0.8, 1.2]);
        assert!(uneven.iter().zip([0.4, 1.0, 2.6]).all(|(a, b)| (a - b).abs() < 1.0e-12) && uneven.len() == 3, "{uneven:?}");
        // Closed: none gives its start and halfway round; N give N gaps, the last past the start.
        assert_eq!(new_places(8.0, true, &[]), [0.0, 4.0]);
        assert_eq!(new_places(8.0, true, &[0.0, 4.0]), [2.0, 6.0]);
        assert_eq!(new_places(8.0, true, &[2.0]), [6.0], "a point's gap is the whole curve");
        assert_eq!(new_places(8.0, true, &[0.0, 2.0, 4.0, 6.0]), [1.0, 3.0, 5.0, 7.0]);
        assert_eq!(new_places(8.0, true, &[6.0, 7.0]), [6.5, 2.5], "the last gap runs from 7 past the start to 6: its middle is 2.5 round");
        // The count is what `doubling_adds` says.
        for (closed, alongs) in [(false, vec![]), (false, vec![1.0, 2.0, 3.0]), (true, vec![]), (true, vec![1.0, 5.0, 6.0])] {
            assert_eq!(new_places(10.0, closed, &alongs).len(), doubling_adds(closed, u32::try_from(alongs.len()).unwrap()) as usize);
        }
    }

    #[test]
    fn closing_the_gaps_renumbers_what_stays_and_gives_none_for_what_goes() {
        let removed = [10, 11, 12, 13];
        assert_eq!(close_gaps(&removed, 9), Some(9));
        assert_eq!(close_gaps(&removed, 10), None);
        assert_eq!(close_gaps(&removed, 13), None);
        assert_eq!(close_gaps(&removed, 14), Some(10));
        assert_eq!(close_gaps(&removed, 15), Some(11));
        assert_eq!(close_gaps(&[], 5), Some(5));
        // Over 0 to 15 with those removed, the rows that stay take every number from 0 to 11 once, in order.
        let after: Vec<u32> = (0..16).filter_map(|ix| close_gaps(&removed, ix)).collect();
        assert_eq!(after, (0..12).collect::<Vec<_>>());
        assert_eq!(close_gaps(&[0], 0), None);
        assert_eq!(close_gaps(&[u32::MAX], u32::MAX), None);
    }

    #[test]
    fn a_share_is_written_as_the_masters_write_one() {
        assert_eq!(share_text("Width", 0.0), None);
        assert_eq!(share_text("Width", 1.0).as_deref(), Some("Width"));
        assert_eq!(share_text("Height", 0.75).as_deref(), Some("Height*0.75"));
        assert_eq!(share_text("Width", 1.0 / 3.0).as_deref(), Some("Width*0.333333333"));
        assert_eq!(share_text("Width", 1.0e-12), None, "rounded to nothing is the constant 0");
        for k in [0.1, 0.25, 0.375, 2.0 / 3.0, 0.999_999_999_9] {
            let text = share_text("Width", k).unwrap();
            let back: f64 = text.strip_prefix("Width*").map_or(1.0, |n| n.parse().unwrap());
            assert!((back - k).abs() < 5.0e-10, "{k} -> {text}");
        }
    }

    fn end(formula: Option<&str>, value: f64) -> EndAxis {
        EndAxis::of_cell(formula, value, false, "Width", 2.0, "Geometry1.X2".to_owned())
    }

    #[test]
    fn two_ends_that_are_shares_give_a_share_two_plain_lengths_a_constant_and_a_control_row_form() {
        // The Rectangle's top: from `Width` to the constant 0, a quarter of the way: 0.75.
        let top = blend(&end(Some("Width"), 2.0), &end(None, 0.0), 0.25, "Width", 2.0);
        assert_eq!((top.formula.as_deref(), top.value), (Some("Width*0.75"), 1.5));
        // Both ends 0 or 1 of the box: a zero share is the constant, a whole one the size's name.
        assert_eq!(blend(&end(Some("Width*0"), 0.0), &end(Some("Width*0"), 0.0), 0.5, "Width", 2.0).formula, None);
        assert_eq!(blend(&end(Some("Width*1"), 2.0), &end(Some("Width"), 2.0), 0.5, "Width", 2.0).formula.as_deref(), Some("Width"));
        // Plain lengths: the length.
        let plain = blend(&end(None, 0.5), &end(None, 1.5), 0.5, "Width", 2.0);
        assert_eq!((plain.formula, plain.value), (None, 1.0));
        // A control at one end: row form, over the cells of the two ends.
        let a = EndAxis::of_cell(None, 0.0, false, "Width", 2.0, "Geometry1.X2".to_owned());
        let b = EndAxis::of_cell(Some("Controls.X1"), 2.0, false, "Width", 2.0, "Geometry1.X3".to_owned());
        let row = blend(&a, &b, 0.5, "Width", 2.0);
        assert_eq!((row.formula.as_deref(), row.value), (Some("Geometry1.X2+(Geometry1.X3-Geometry1.X2)*0.5"), 1.0));
        // Both ends cells of relative rows: inside `Width*(...)`.
        let ra = EndAxis::of_cell(Some("Controls.X1"), 0.0, true, "Width", 2.0, "Geometry1.X2".to_owned());
        let rb = EndAxis::of_cell(Some("Controls.X2"), 0.0, true, "Width", 2.0, "Geometry1.X3".to_owned());
        assert_eq!(blend(&ra, &rb, 0.25, "Width", 2.0).formula.as_deref(), Some("Width*(Geometry1.X2+(Geometry1.X3-Geometry1.X2)*0.25)"));
    }

    #[test]
    fn a_constant_share_of_a_relative_row_is_a_share_and_a_formula_in_one_is_not() {
        let a = EndAxis::of_cell(None, 0.5, true, "Width", 4.0, "Geometry1.X2".to_owned());
        let b = EndAxis::of_cell(None, 1.0, true, "Width", 4.0, "Geometry1.X3".to_owned());
        assert_eq!(blend(&a, &b, 0.5, "Width", 4.0).formula.as_deref(), Some("Width*0.75"));
        let c = EndAxis::of_cell(Some("Geometry1.X1*2"), 1.0, true, "Width", 4.0, "Geometry1.X3".to_owned());
        assert_eq!(c.share, None);
    }

    #[test]
    fn a_polyline_vertex_is_a_share_or_a_plain_length_by_its_constructors_type() {
        let share = EndAxis::of_number(0.25, true, 4.0);
        let plain = EndAxis::of_number(1.0, false, 4.0);
        assert_eq!((share.share, share.value), (Some(0.25), 1.0));
        assert_eq!((plain.share, plain.plain, plain.value), (None, Some(1.0), 1.0));
        assert_eq!(EndAxis::of_number(0.0, false, 4.0).share, Some(0.0), "a plain nought is also the share nought");
    }

    #[test]
    fn the_ways_out_the_types_and_the_outward_normal() {
        assert_eq!(common_way_out(&[]), None);
        assert_eq!(common_way_out(&[(0.0, 1.0), (0.0, 1.0)]), Some((0.0, 1.0)));
        assert_eq!(common_way_out(&[(0.0, 1.0), (1.0, 0.0)]), None);
        assert_eq!(common_way_out(&[(0.0, 0.0), (0.0, 0.0)]), Some((0.0, 0.0)), "all nothing gives nothing");
        assert_eq!(common_kind(&[]), 0.0);
        assert_eq!(common_kind(&[2.0, 2.0]), 2.0);
        assert_eq!(common_kind(&[0.0, 2.0]), 0.0);
        // Counterclockwise: the normal is to the right of the way it goes; clockwise, the left.
        assert_eq!(outward((-1.0, 0.0), 2.0), (0.0, 1.0));
        assert_eq!(outward((-1.0, 0.0), -2.0), (-0.0, -1.0));
    }

    #[test]
    fn a_removed_points_successor_is_the_nearest_then_by_the_other_end_then_the_lower_row() {
        let at = |ix: u32, x: f64| Candidate { ix, place: (x, 1.0), page: (x, 1.0) };
        let cands = [at(0, 1.0), at(4, 1.5), at(5, 0.5)];
        // The point at 1.25 is as near rows 0 and 4: with the other end to the right it goes to row 4, to the left to row 0, with none to weigh the lower row.
        assert_eq!(successor((1.25, 1.0), &cands, Some((5.0, 1.5))).map(|c| c.ix), Some(4));
        assert_eq!(successor((1.25, 1.0), &cands, Some((-3.0, 1.5))).map(|c| c.ix), Some(0));
        assert_eq!(successor((1.25, 1.0), &cands, None).map(|c| c.ix), Some(0));
        // Plainly nearest.
        assert_eq!(successor((1.75, 1.0), &cands, None).map(|c| c.ix), Some(4));
        assert_eq!(successor((0.25, 1.0), &cands, None).map(|c| c.ix), Some(5));
        assert_eq!(successor((1.0, 1.0), &[], None), None);
    }

    #[test]
    fn a_length_is_told_in_millimetres_whole_or_to_a_tenth() {
        assert_eq!(millimetres(3.0 / 25.4), "3");
        assert_eq!(millimetres(0.0625), "1.6");
        assert_eq!(millimetres(0.03125), "0.8");
        assert_eq!(millimetres(0.125), "3.2");
    }
}
