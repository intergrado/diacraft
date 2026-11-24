//! The ends and the length of a line or a connector, typed: what the Size & Position window's Begin X, Begin Y, End X, End Y and Length
//! fields send. The rules are named here and in `docs/editing-commands.md`:
//!
//! * Our rule. [`set_end`]
//!   puts one end of a one-dimensional shape at a place on the page, either coordinate or both, the other keeping its place. It is the work of
//!   `connector.reconnect` to a point ([`reconnect_connector`]) and no other path: the two cells of the end become plain places, `Width` and `Angle`
//!   follow by their formulas, a route DiaCraft wrote is made again and so is the label's place, a hand edit of the path is
//!   undone. The place is a point of the page, carried down through the groups the shape is in to the frame its cells are in
//!   **An end that is glued is refused, in words**: a plain write replaces the end's glue formula and so breaks the
//!   glue, and a person who types into a field of a glued end has not asked for that; `connector.reconnect` to a point is the way to take the glue off,
//!   and the window offers it as its own act. A place the end is at already is no change.
//! * Our rule (the window shows Length for a line, and "a line takes the angle from its begin point"). [`set_length`]
//!   sets the distance between the ends **keeping the direction**: the end that is not glued moves along the line, away from or towards the other,
//!   and the other stays. With no end glued it is the end that moves, and the begin stays; with the end glued and the begin free, the begin moves and
//!   the glued end stays (as ROTATE-1D-1 turns the free end about the glued one); with both glued the shapes they join set the length, and the
//!   command is refused. The direction is that of the ends in the shape's parent's frame, and for a line whose ends are one place, the cell `Angle`
//!   (a line of no length has no direction of its own). A length of 0 or less is refused: the angle of a line of no length is not defined by its
//!   ends, and a person who wants an end elsewhere types its place. The move is `connector.reconnect`'s too ([`reconnect_connector`]).
//!
//! Both are for one shape, as `connector.reconnect` is, and Auto Size is the command's: the end moves, so a page that grows takes it in
//! (`draw::fit_page`, in the same undo step).

use diacraft_doc::{Document, SheetRef};

use super::arrange::{SAME_PLACE, glued_ends};
use super::connectors::{End, Which, reconnect_connector};
use super::read::Reader;
use super::route::P;
use super::{MAX_COORD, Problem, checked, require_shape};

/// The words for an end that is glued.
fn glued_words(id: u32, which: Which) -> String {
    let end = if which == Which::Begin { "begin" } else { "end" };
    format!(
        "the {end} of shape {id} is glued to a shape and follows it: move the shape, or take the glue off with connector.reconnect to a place first"
    )
}

/// The ends of shape `id` in its parent's frame (where the cells hold them) and its parent's frame to the page, for a shape that is a line.
fn line_of(doc: &Document, page: u32, id: u32) -> Result<(SheetRef, diacraft_doc::ShapeRef, P, P), Problem> {
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let reader = Reader::new(doc);
    if !reader.is_one_d(&sheet) {
        return Err(Problem::bad(format!("shape {id} is not a line or a connector")));
    }
    let end = |x: &str, y: &str| (reader.number(&sheet, x), reader.number(&sheet, y));
    let ends = (end("BeginX", "BeginY"), end("EndX", "EndY"));
    Ok((sheet, at, ends.0, ends.1))
}

/// Puts the end `which` of line `id` at (`x`, `y`) on the page, in the page's drawing units; a coordinate left out keeps its place.
/// → whether anything changed: an end that is at the place already is left alone. An end that is glued is an error.
pub fn set_end(doc: &mut Document, page: u32, id: u32, which: Which, x: Option<f64>, y: Option<f64>) -> Result<bool, Problem> {
    if x.is_none() && y.is_none() {
        return Err(Problem::bad("give `x`, `y` or both"));
    }
    let wanted = (x.map(|v| checked("x", v)).transpose()?, y.map(|v| checked("y", v)).transpose()?);
    let (_, at, begin, finish) = line_of(doc, page, id)?;
    let (begin_glued, end_glued) = glued_ends(doc, page, id);
    if if which == Which::Begin { begin_glued } else { end_glued } {
        return Err(Problem::failed(glued_words(id, which)));
    }
    let (cells, to) = {
        let reader = Reader::new(doc);
        let cells = if which == Which::Begin { begin } else { finish };
        // The place on the page with the coordinates given, and then in the frame the cells are in.
        let now = reader.parent_to_page(&at, cells);
        let to = reader.point_from_page(&at, (wanted.0.unwrap_or(now.0), wanted.1.unwrap_or(now.1)));
        (cells, to)
    };
    if (to.0 - cells.0).abs() <= SAME_PLACE && (to.1 - cells.1).abs() <= SAME_PLACE {
        return Ok(false);
    }
    reconnect_connector(doc, page, id, which, &End::Point { x: to.0, y: to.1 })?;
    Ok(true)
}

/// Gives line `id` the length `length`, in the page's drawing units, keeping its direction and the end that stays. → whether
/// anything changed: a line of that length already is left alone. A length of 0 or less, and a line with both ends glued, are errors.
pub fn set_length(doc: &mut Document, page: u32, id: u32, length: f64) -> Result<bool, Problem> {
    if !length.is_finite() || length <= 0.0 || length > MAX_COORD {
        return Err(Problem::bad(format!("`length` must be more than 0 and at most {MAX_COORD}")));
    }
    let (sheet, _, begin, finish) = line_of(doc, page, id)?;
    let (begin_glued, end_glued) = glued_ends(doc, page, id);
    if begin_glued && end_glued {
        return Err(Problem::failed(format!("both ends of shape {id} are glued: the shapes it joins set its length")));
    }
    let (dx, dy) = (finish.0 - begin.0, finish.1 - begin.1);
    let now = dx.hypot(dy);
    if (now - length).abs() <= SAME_PLACE {
        return Ok(false);
    }
    // The direction from the begin to the end; a line of no length has none by its ends and keeps its `Angle`.
    let direction = if now > SAME_PLACE {
        (dx / now, dy / now)
    } else {
        let angle = Reader::new(doc).number(&sheet, "Angle");
        (angle.cos(), angle.sin())
    };
    // The end that is not glued moves; with the end glued it is the begin, else the end.
    let (which, to) = if end_glued {
        (Which::Begin, (finish.0 - direction.0 * length, finish.1 - direction.1 * length))
    } else {
        (Which::End, (begin.0 + direction.0 * length, begin.1 + direction.1 * length))
    };
    let to = (checked("length", to.0)?, checked("length", to.1)?);
    reconnect_connector(doc, page, id, which, &End::Point { x: to.0, y: to.1 })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::draw::add_connector;

    fn doc_with_a_connector(from: (f64, f64), to: (f64, f64)) -> (Document, u32) {
        let mut doc = Document::new_blank();
        let info = add_connector(&mut doc, 1, &End::Point { x: from.0, y: from.1 }, &End::Point { x: to.0, y: to.1 }).unwrap();
        (doc, info.id)
    }

    fn ends(doc: &Document, id: u32) -> (P, P) {
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
        let reader = Reader::new(doc);
        let sheet = SheetRef::Shape(at);
        ((reader.number(&sheet, "BeginX"), reader.number(&sheet, "BeginY")), (reader.number(&sheet, "EndX"), reader.number(&sheet, "EndY")))
    }

    #[test]
    fn a_coordinate_left_out_keeps_its_place_and_an_end_already_there_is_no_change() {
        let (mut doc, id) = doc_with_a_connector((2.0, 3.0), (6.0, 3.0));
        assert!(set_end(&mut doc, 1, id, Which::End, Some(7.0), None).unwrap());
        assert_eq!(ends(&doc, id), ((2.0, 3.0), (7.0, 3.0)));
        assert!(set_end(&mut doc, 1, id, Which::Begin, None, Some(1.5)).unwrap());
        assert_eq!(ends(&doc, id), ((2.0, 1.5), (7.0, 3.0)));
        let was = doc.clone();
        assert!(!set_end(&mut doc, 1, id, Which::End, Some(7.0), Some(3.0)).unwrap(), "the place it has");
        assert_eq!(doc, was);
        assert!(set_end(&mut doc, 1, id, Which::End, None, None).unwrap_err().bad_params, "neither coordinate");
    }

    #[test]
    fn the_length_is_set_along_the_direction_the_line_has_and_the_begin_stays() {
        let (mut doc, id) = doc_with_a_connector((1.0, 1.0), (4.0, 5.0));
        assert!(set_length(&mut doc, 1, id, 10.0).unwrap());
        let (begin, finish) = ends(&doc, id);
        assert_eq!(begin, (1.0, 1.0));
        // 3-4-5 scaled to 10: the end is at (1 + 6, 1 + 8).
        assert!((finish.0 - 7.0).abs() < 1e-8 && (finish.1 - 9.0).abs() < 1e-8, "{finish:?}");
        let was = doc.clone();
        assert!(!set_length(&mut doc, 1, id, 10.0).unwrap());
        assert_eq!(doc, was);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_COORD * 2.0] {
            assert!(set_length(&mut doc, 1, id, bad).unwrap_err().bad_params, "{bad}");
        }
    }
}
