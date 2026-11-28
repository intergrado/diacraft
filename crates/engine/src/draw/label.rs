//! The label of a connector.
//!
//! Our rule, a project rule; give the cells it writes. A connector's label
//!
//! * stays **upright**: `TxtAngle` turns the text block against the connector's `Angle` so that they net to nothing (the sum, or the
//!   difference under one flip, of the two);
//! * is **centred on the route's middle leg**: of an odd number of legs the one in the middle, of an even number the longer of the two in the middle
//!   (the first if they are as long), and the text block's centre is the middle of that leg (`TxtPinX`, `TxtPinY` are that point in the
//!   connector's own frame, and `TxtLocPinX`, `TxtLocPinY` the middle of the block, the defaults);
//! * has a **text background that hides the line under it**: `TextBkgnd` white (`#FFFFFF`) where the connector has none, painted as a rectangle
//!   hugging the text. White is the page colour of a new drawing; a background of another colour that the file chose is left.
//!
//! It is written for a connector DiaCraft made, whenever it is routed, and for a connector of a file only when it is routed again and has text
//! The text cells of a connector of a file that is not routed are not touched. A cell that already holds the value is not
//! written, and a cell a `GUARD` protects is left as it is.

use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, ShapeRef, SheetRef, Value, WriteOrigin};

use super::Problem;
use super::connectors::Routed;
use super::read::Reader;
use super::route::P;
use crate::stencil::number_text;

/// The colour of a label's background.
const LABEL_BACKGROUND: &str = "#FFFFFF";

/// How far a cell may be from the value wanted and be left as it is.
const SAME: f64 = 1.0e-9;

/// The middle leg of a route and the middle of it, or the point itself for a route of no length.
pub(crate) fn middle_of(points: &[P]) -> Option<P> {
    let legs: Vec<(P, P)> = points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect();
    let length = |leg: &(P, P)| (leg.1.0 - leg.0.0).hypot(leg.1.1 - leg.0.1);
    let at = match legs.len() {
        0 => return points.first().copied(),
        n if n % 2 == 1 => n / 2,
        n => {
            let (a, b) = (n / 2 - 1, n / 2);
            if legs.get(b).map(length) > legs.get(a).map(length) { b } else { a }
        }
    };
    let (from, to) = *legs.get(at)?;
    Some(((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0))
}

/// An angle brought to the range from just over -pi to pi.
fn wrapped(a: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    let r = a.rem_euclid(turn);
    if r > std::f64::consts::PI { r - turn } else { r }
}

/// The `TxtAngle` that nets the text upright with the connector's pose.
fn upright(angle: f64, flipped_once: bool) -> f64 {
    wrapped(if flipped_once { angle } else { -angle })
}

/// Whether a `TextBkgnd` value paints nothing: absent, 0 or 255. A `#RRGGBB` and any other number is a colour.
fn paints_nothing(text: Option<&str>) -> bool {
    match text.map(str::trim) {
        None | Some("") => true,
        Some(t) if t.starts_with('#') => false,
        Some(t) => t.parse::<f64>().is_ok_and(|n| n == 0.0 || n == 255.0),
    }
}

/// Places the label of connector `id`, whose route has just been written. Nothing happens for a connector of a file without text. → the cells it wrote, for the caller to work out what
/// reads them.
pub(crate) fn place(doc: &mut Document, page: u32, id: u32, routed: &Routed, ours: bool) -> Result<Vec<(SheetRef, CellAddr)>, Problem> {
    let writes = {
        let reader = Reader::new(doc);
        let Some(at) = reader.find(page, id) else { return Ok(Vec::new()) };
        writes_for(&reader, &at, routed, ours)
    };
    let cells = writes.iter().map(|(sheet, write)| (sheet.clone(), write.addr.clone())).collect();
    write(doc, writes)?;
    Ok(cells)
}

/// The cells the label of the connector at `at` is to be given for its route `routed`: none for a connector of a file without text, and none a cell holds
/// already. Read before anything is written, so that the labels of every connector an edit routed are worked out on one reading of the page (a label
/// reads its connector's own cells, which no other connector's route or label writes).
pub(crate) fn writes_for(reader: &Reader<'_>, at: &ShapeRef, routed: &Routed, ours: bool) -> Vec<(SheetRef, CellWrite)> {
    let has_text = reader.doc().shape(at).is_some_and(|s| s.text.is_some());
    if !ours && !has_text {
        return Vec::new();
    }
    let Some(centre) = middle_of(&routed.points) else { return Vec::new() };
    let sheet = SheetRef::Shape(at.clone());
    let local = routed.pose.down(centre);
    let mut wanted: Vec<(&str, f64)> = vec![("TxtPinX", local.0), ("TxtPinY", local.1)];
    wanted.push(("TxtAngle", upright(routed.pose.angle, routed.pose.flip_x != routed.pose.flip_y)));
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    let mut put = |name: &str, edit: CellEdit| {
        writes.push((sheet.clone(), CellWrite { addr: CellAddr::direct(name), edit, origin: WriteOrigin::Drawing }));
    };
    for (name, value) in wanted {
        let held = reader.number(&sheet, name);
        let differs = if name == "TxtAngle" { wrapped(held - value).abs() > SAME } else { (held - value).abs() > SAME };
        if value.is_finite() && differs {
            put(name, CellEdit::Value(Value::from_text(&number_text(value))));
        }
    }
    // The block's turning centre is its middle, so that the pin above is where the text is centred.
    for (name, size) in [("TxtLocPinX", "TxtWidth"), ("TxtLocPinY", "TxtHeight")] {
        let half = reader.number(&sheet, size) / 2.0;
        if half.is_finite() && (reader.number(&sheet, name) - half).abs() > SAME {
            put(name, CellEdit::Value(Value::from_text(&number_text(half))));
        }
    }
    let background = reader
        .resolver()
        .resolve(&sheet, &CellAddr::direct("TextBkgnd"))
        .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref().map(|v| v.text().to_owned())));
    if paints_nothing(background.as_deref()) {
        put("TextBkgnd", CellEdit::Value(Value::from_text(LABEL_BACKGROUND)));
    }
    writes
}

/// Writes the cells of labels ([`writes_for`]), of one connector or of many, in one batch. A guarded cell is kept as it is (`WriteOutcome::KeptGuarded`): a label
/// that a file pinned stays where it is, and the edit goes on.
pub(crate) fn write(doc: &mut Document, writes: Vec<(SheetRef, CellWrite)>) -> Result<(), Problem> {
    if writes.is_empty() {
        return Ok(());
    }
    doc.write_batch(writes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn near(a: Option<P>, b: P) -> bool {
        a.is_some_and(|a| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9)
    }

    #[test]
    fn the_middle_leg_of_an_odd_number_of_legs_is_the_one_in_the_middle() {
        // A Z: out 1, across 4, in 1.
        assert!(near(middle_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 4.0), (2.0, 4.0)]), (1.0, 2.0)));
        // One straight leg: its middle.
        assert!(near(middle_of(&[(0.0, 0.0), (4.0, 0.0)]), (2.0, 0.0)));
    }

    #[test]
    fn of_an_even_number_the_longer_of_the_two_in_the_middle_and_the_first_if_they_are_as_long() {
        // An L, 5 then 3: the first. 3 then 5: the second.
        assert!(near(middle_of(&[(0.0, 0.0), (5.0, 0.0), (5.0, 3.0)]), (2.5, 0.0)));
        assert!(near(middle_of(&[(0.0, 0.0), (3.0, 0.0), (3.0, 5.0)]), (3.0, 2.5)));
        assert!(near(middle_of(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)]), (2.0, 0.0)));
        // Four legs: the second and third are the middle two.
        assert!(near(middle_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (6.0, 2.0), (6.0, 3.0)]), (3.5, 2.0)));
    }

    #[test]
    fn a_route_of_no_length_or_no_points_has_its_point_or_none() {
        assert!(near(middle_of(&[(1.0, 1.0), (1.0, 1.0)]), (1.0, 1.0)));
        assert!(near(middle_of(&[(2.0, 3.0)]), (2.0, 3.0)));
        assert_eq!(middle_of(&[]), None);
    }

    #[test]
    fn the_turn_that_makes_the_text_upright_is_the_negative_of_the_angle_and_its_positive_under_one_flip() {
        assert!((upright(0.5, false) + 0.5).abs() < 1e-12);
        assert!((upright(0.5, true) - 0.5).abs() < 1e-12);
        // Brought into range: minus three quarters of a turn is a quarter turn the other way.
        assert!((upright(-3.0 * std::f64::consts::FRAC_PI_2, false) + std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn a_background_of_0_255_or_nothing_paints_nothing_and_any_colour_is_kept() {
        for none in [None, Some(""), Some("0"), Some("255"), Some(" 0 ")] {
            assert!(paints_nothing(none), "{none:?}");
        }
        for colour in [Some("#FFFFFF"), Some("#000000"), Some("2"), Some("1")] {
            assert!(!paints_nothing(colour), "{colour:?}");
        }
    }
}
