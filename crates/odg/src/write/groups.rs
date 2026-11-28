//! New groups: a group of new shapes is written as a `draw:g` holding its members, each placed on the
//! page by the map of the groups around it (a group has no frame of its own in ODF, so the members' attributes are the page's).
//!
//! **What the reader gives back**: a group whose frame is the smallest upright box that holds its members, no turn and no flip, its pin at the
//! box's middle and `LocPinX`, `LocPinY` half its size by the formulas `Width*0.5` and `Height*0.5`; each member's cells then measured from the box's lower-left corner.
//! A `draw:g` has no frame, no angle and no pin of its own: ODF 1.3 Part 3 §10.3.15 (the schema's `draw-g` and `draw-g-attlist`, read 2026-11-20) gives it an id, a name, a
//! style, a z-index, the anchor and caption attributes every shape has, and **`svg:y` alone**, with no `svg:x`, `svg:width`, `svg:height` or `draw:transform`, and its content
//! is its members; one coordinate is no frame, and R43 gives the reading. So **a group the writer writes is one the reader rebuilds from its members, and it is the box of
//! them.**
//!
//! **A group whose frame is that box is written as it is, whatever the turn of its members**: the group's frame is compared to the box at [`FRAME_TOLERANCE`],
//! which is the precision of the frame cells a drawing handed to the writer holds (every one at nine places, which carries a turned member's corner to a few parts in a
//! hundred million), and not at the writer's own 1E-12, which no turned member's box ever met. **A group whose frame is not that box** (a size it was held to by
//! `LockCalcWH` after a member moved out, a margin, a pin set away from the middle) **is written as its members all the same, and the save says so in a note**
//! ([`crate::SaveNote::GroupFrameNotKept`]): the file has no place for the frame, and the reopened group is the box of its members. Its members are placed by its
//! map, so they are where the session has them on the page. A group with a cell of its own beyond the frame, or a section, has data, which DiaCraft's document file
//! carries (`ext::docfile`, `group` entries: written for the model's groups by the file's own plan). **Refused in words:** a group's text, a group turned or flipped (its
//! angle is kept in the document file with the members written turned, which is not written yet), and a frame that is not numbers.

use diacraft_doc::Shape;

use super::WriteError;
use super::compare::NewShape;
use super::own;
use crate::read::Affine;

/// How far a new group's frame may be from the box of its members and still be that box, inches: a millionth of an inch (25 nanometres, a four-hundredth of the
/// hundredth of a millimetre a path is written in). **The frame cells of a drawing handed to the writer are at nine places** (`flatten.rs` of the engine writes each
/// number so, and so does `shape.group`), so the group's size and pin are off the exact box by half a billionth each, and a member's box worked from its pin, its size
/// and its angle each so rounded is off by that and by a turn of half a billionth of a radian carried to its corners: a few parts in a hundred million at the size of a
/// page, a hundred times less than this. A frame within it reads back as the session's (the reader makes the box from what is written, at 1E-12).
pub(crate) const FRAME_TOLERANCE: f64 = 1e-6;

/// The part of the size of a group that its frame may be off its members' box by, beside [`FRAME_TOLERANCE`]: a billionth of the largest length the frame has (a member
/// turned by a rounded angle is off by that angle's half billionth of a radian times its distance from the turn, which grows with the size of the group, and a drawing
/// nested deeply carries the rounding of each level), so that a group a thousand inches wide is held to a millionth of that and not to a millionth of an inch.
pub(crate) const FRAME_RELATIVE: f64 = 1e-9;

/// How far a group's frame may be from the box of its members in a drawing whose group has the largest length `scale` (inches): [`FRAME_TOLERANCE`] or
/// [`FRAME_RELATIVE`] of the length, whichever is more. (A length that is not a number leaves [`FRAME_TOLERANCE`].)
pub(crate) fn tolerance(scale: f64) -> f64 {
    FRAME_TOLERANCE.max(FRAME_RELATIVE * scale)
}

/// A box as `(x0, y0, x1, y1)`, inches, y up.
type Box4 = (f64, f64, f64, f64);

/// What the members of a group take up, in the frame their cells are in (the group's own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Span {
    /// No member yet.
    Empty,
    /// The smallest upright box that holds every member.
    Box(Box4),
    /// A member whose frame is not numbers.
    Unreadable,
}

impl Span {
    /// The span of both.
    pub(crate) fn join(self, other: Span) -> Span {
        match (self, other) {
            (Span::Unreadable, _) | (_, Span::Unreadable) => Span::Unreadable,
            (Span::Empty, s) | (s, Span::Empty) => s,
            (Span::Box((a, b, c, d)), Span::Box((e, f, g, h))) => Span::Box((a.min(e), b.min(f), c.max(g), d.max(h))),
        }
    }

    /// The span in the parent's frame, by the map `map` of the group's own frame to it: the box of the corners of the box carried over (the group is upright in the
    /// writer's cases, and a turned one is refused by its own check). `Unreadable` when the map is not numbers.
    pub(crate) fn carried(self, map: Option<Affine>) -> Span {
        match (self, map) {
            (Span::Empty, _) => Span::Empty,
            (Span::Unreadable, _) | (Span::Box(_), None) => Span::Unreadable,
            (Span::Box((x0, y0, x1, y1)), Some(m)) => [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .map(|&(x, y)| {
                    let p = m.apply(x, y);
                    Span::Box((p.0, p.1, p.0, p.1))
                })
                .fold(Span::Empty, Span::join),
        }
    }
}

/// The box a shape takes up in its parent's frame: its frame turned, or for a 1-D shape its two ends. `None` for a cell that is not a number.
pub(crate) fn extent(shape: &Shape) -> Option<Box4> {
    if shape.sheet.cell("BeginX").is_some() {
        let [a, b] = own::ends(&shape.sheet, &Affine::IDENTITY)?;
        return Some((a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)));
    }
    let pose = own::pose_2d(&shape.sheet, &Affine::IDENTITY)?;
    let [a, b, c, d] = pose.lin;
    let (w, h) = pose.size;
    let (ex, ey) = (((a.abs() * w) + (c.abs() * h)) / 2.0, ((b.abs() * w) + (d.abs() * h)) / 2.0);
    Some((pose.centre.0 - ex, pose.centre.1 - ey, pose.centre.0 + ex, pose.centre.1 + ey))
}

/// What the members of each new group take up in the group's own frame, for the list of new shapes `news` (a group is followed by its members, so a member's place is
/// after its group's): **the shapes that the reader would find inside the group, to the leaves.** A member that is a group counts by the box of its own members (carried
/// by its map), and not by its own frame, which the file does not keep: the reader makes each group's frame from its members, so the frame of a group round it is the box of
/// theirs. One pass from the last shape to the first, each shape once, so the cost is the number of shapes and no more. Gives one entry for each new shape (the span of a
/// shape that holds no member is [`Span::Empty`]), or none at all when the list holds no member of a group.
pub(crate) fn spans_inside(news: &[NewShape], steps: &mut u64) -> Result<Vec<Span>, WriteError> {
    if news.iter().all(|n| n.parent.is_none()) {
        return Ok(Vec::new());
    }
    let mut inside: Vec<Span> = Vec::new();
    inside.try_reserve_exact(news.len()).map_err(|_| WriteError::Limit { what: "memory for the new shapes of a page", max: news.len() as u64 })?;
    inside.resize(news.len(), Span::Empty);
    for (k, n) in news.iter().enumerate().rev() {
        *steps = steps.saturating_add(1);
        let Some(parent) = n.parent else { continue };
        let span = if n.shape.children.is_empty() {
            extent(&n.shape).map_or(Span::Unreadable, Span::Box)
        } else {
            inside.get(k).copied().unwrap_or(Span::Empty).carried(own::local_to_parent(&n.shape.sheet))
        };
        if let Some(slot) = inside.get_mut(parent) {
            *slot = slot.join(span);
        }
    }
    Ok(inside)
}

/// Whether a group of the model was read with an automatic graphic style of its own (`draw:style-name`, held by the reader): the model writer writes that style back with the group
/// As it writes a shape's, and the cells the style gives the group are the markup's.
///
/// **The one answer is [`super::shapes::AsRead::of_group`]'s**: the group names a style when its record is not `Ok(None)`, so a group whose name the writer refuses (a character XML
/// 1.0 does not allow) is a styled group, as the markup sees it (it refuses the save), and the document file and the markup never tell two groups apart.
pub(crate) fn names_a_style(shape: &Shape) -> bool {
    !matches!(super::shapes::AsRead::of_group(shape), Ok(None))
}

/// The cells of a group that its own style says (the model path: [`names_a_style`]): **the cells the reader reads back from the style the writer writes**, which are no data of DiaCraft's for
/// the group, and so no entry of the document file is made for them (`ext::docfile`). The same cells [`super::shapes`] writes natively for a shape of the file, but those a file's colour
/// or number spells back as they are (a cell with a formula, or a colour the file does not spell as the cell does, is carried beside the style, as an entry says).
pub(crate) fn style_cells(shape: &Shape) -> Vec<&'static str> {
    if !names_a_style(shape) {
        return Vec::new();
    }
    let (_, mut ops, mut names) = super::shapes::native_style_as(&shape.sheet, &[], true, true);
    super::held::graphic_ops_said(shape, &mut ops, &mut names);
    names.retain(|name| shape.sheet.cells.get(*name).is_some_and(|cell| crate::ext::native::style_reproduced_read(name, cell)));
    names
}

/// The words of the refusal of a text on a group (a new one, or one of the file): writes it as one more member of the group, a custom shape on the
/// group's frame whose own style carries a marker, which the reader folds back into the group's own sheet; that member is not written yet, and the text is not dropped.
pub(crate) const WORDS_GROUP_TEXT: &str = "a new group with text of its own (a group's text is written as one more member of the group, which is not written yet: take the text off the group, or put it on a shape beside it)";

/// The words of the refusal of a group that is turned or flipped, new or of the file (review): write its members turned or
/// flipped on the page and keep the group's own angle and flips in DiaCraft's document file, from which the reader puts the members back in the turned frame; that is
/// not written yet, and a group saved without it reads back with an angle of 0 and each member carrying the turn.
pub(crate) const WORDS_GROUP_TURN: &str = "a group that is turned or flipped (a group's own angle and flips are kept in DiaCraft's document file, with its members written turned, which is not written yet: turn or flip the members one by one, or take the turn off the group)";

/// The words of the refusal of a new picture that holds members (a picture is written as a frame holding the picture, and a group as a `draw:g` holding its members: the
/// shape cannot be both).
pub(crate) const WORDS_PICTURE_GROUP: &str =
    "a new picture that is a group (a picture is written as a frame and a group as its members: take the members out of the picture)";

/// The words of the refusal of a new line or connector that holds members (a one-dimensional shape is written as a line or a connector, and a group as its members: the
/// shape cannot be both, and the ends of a `draw:g` are nobody's).
pub(crate) const WORDS_LINE_GROUP: &str = "a new line or connector that is a group (a one-dimensional shape is written as a line or a connector and a group as its members: take the members out of it)";

/// The angle and the two flips a group's sheet holds, as numbers (a cell that is not there, or not a number, is 0).
pub(crate) fn turn_of_sheet(sheet: &diacraft_doc::Sheet) -> [f64; 3] {
    ["Angle", "FlipX", "FlipY"].map(|name| sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0))
}

/// Whether `cell` is one the model gained at 0 for a turn cell (`Angle`, `FlipX`, `FlipY`) that the file's group (`base`) lacks: a constant 0 and nothing else on it. The
/// reader makes these cells only when they are not 0, so a group that holds one at 0 turns no more than one that holds none ([`turn_of_sheet`] reads both as 0), and the
/// cell is no change of the group's frame.
fn is_turn_gained_at_zero(base: &diacraft_doc::Sheet, name: &str, cell: &diacraft_doc::Cell) -> bool {
    ["Angle", "FlipX", "FlipY"].contains(&name)
        && base.cell(name).is_none()
        && matches!(cell.formula, diacraft_doc::FormulaSlot::Absent)
        && cell.error.is_none()
        && cell.raw.is_empty()
        && cell.extra.is_none()
        && cell.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0)
}

/// Whether a group's change (`delta`, from the sheet `base` the file's group has) holds an edit of a frame cell that the file cannot say: a cell of the frame given a formula
/// or other content, one set as a number to put the pin elsewhere, or one taken out. The frame is the box of the members and no entry of the document file carries it, so
/// such an edit would be written nowhere. **A turn cell the model gained at 0 is not one** ([`is_turn_gained_at_zero`]): it is the turn the file's group has, and the
/// change that holds it beside another cell is saved.
pub(crate) fn frame_cell_edited(base: &diacraft_doc::Sheet, delta: &own::Tier2Delta) -> bool {
    delta.cells.iter().any(|(name, cell)| own::POSE_CELLS.contains(&name.as_str()) && !is_turn_gained_at_zero(base, name.as_str(), cell))
        || delta.removed_cells.iter().any(|name| own::POSE_CELLS.contains(&name.as_str()))
}

/// Whether the shape's own text holds anything: characters, or a field or a mark.
pub(crate) fn holds_text(shape: &Shape) -> bool {
    shape.text.as_ref().is_some_and(|t| t.parts.iter().any(|p| !matches!(p, diacraft_doc::TextPart::Chars(c) if c.is_empty())))
}

/// The connectors glued to `group` among `list` (`(group, connector)`, sorted: [`super::compare::PageDiff::glued_to_groups`]): the first [`super::MOST_GLUED_NAMED`] of
/// them, and how many there are.
pub(crate) fn glued_to(list: &[(u32, u32)], group: u32) -> (Vec<u32>, usize) {
    let (from, to) = (list.partition_point(|(g, _)| *g < group), list.partition_point(|(g, _)| *g <= group));
    let found = list.get(from..to).unwrap_or_default();
    (found.iter().take(super::MOST_GLUED_NAMED).map(|(_, connector)| *connector).collect(), found.len())
}

/// Whether the frame cells of a group whose sheet is `sheet` are what the reader rebuilds from the box `(x0, y0, x1, y1)` its members take up in its own frame, whose
/// origin is the box's lower-left corner: the box of the members runs from the origin to the group's size `(w, h)`, and the pin is at its middle, **at [`tolerance`]**.
/// A group that is not the box is one whose frame a `draw:g` cannot keep.
pub(crate) fn frame_is_box(sheet: &diacraft_doc::Sheet, (w, h): (f64, f64), (x0, y0, x1, y1): Box4) -> bool {
    let tol = tolerance(w.abs().max(h.abs()).max((x1 - x0).abs()).max((y1 - y0).abs()));
    let near = |a: f64, b: f64| (a - b).abs() <= tol;
    let is_the_box = near(x0, 0.0) && near(y0, 0.0) && near(x1, w) && near(y1, h);
    // The pin at the box's middle, which the pin cells say: the local pin is half the size, and the frame's cells are the box's.
    let local = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
    let pin_at_the_middle = local("LocPinX").is_some_and(|v| near(v, w / 2.0)) && local("LocPinY").is_some_and(|v| near(v, h / 2.0));
    is_the_box && pin_at_the_middle
}

/// Whether a new group's frame is what the reader rebuilds from the markup the group is written as (see the module's words): `Ok(true)` when it is the box of its
/// members, with its pin at the box's middle, **at [`tolerance`]**; `Ok(false)` when it is not, which is written as its members all the same and said in a note
/// And the reason in words when the group cannot be written at all. `inside` is what the members take up in the group's frame ([`spans_inside`]).
pub(crate) fn check_frame(group: &NewShape, inside: Span) -> Result<bool, WriteError> {
    if holds_text(&group.shape) {
        return Err(WriteError::Unsupported(WORDS_GROUP_TEXT));
    }
    // A picture that holds members is no group: its picture would be left out of a `draw:g`. It was refused by its frame, which is nothing like its members' box (and was
    // saved with its picture told as left out when the frame happened to be); a frame that is not the box is no refusal now, so it is refused here.
    if super::shapes::is_new_picture(&group.shape) {
        return Err(WriteError::Unsupported(WORDS_PICTURE_GROUP));
    }
    // A one-dimensional shape that holds members is no group either: its ends are what a line or a connector is, and a `draw:g` has none; the reader gives back a group
    // with a frame, whose moves and ends the session's shape does not follow, and a second save of it is refused.
    if group.shape.sheet.cell("BeginX").is_some() {
        return Err(WriteError::Unsupported(WORDS_LINE_GROUP));
    }
    let sheet = &*group.shape.sheet;
    let pose = own::pose_2d(sheet, &group.outer).ok_or(WriteError::Unsupported("a new group whose frame is not numbers"))?;
    if own::turn_of(&pose) != Some(0.0) {
        return Err(WriteError::Unsupported(WORDS_GROUP_TURN));
    }
    match inside {
        Span::Box(b) => Ok(frame_is_box(sheet, pose.size, b)),
        Span::Unreadable => Err(WriteError::Unsupported("a new group with a member whose frame is not numbers")),
        Span::Empty => Err(WriteError::Unsupported("a new group with no members")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellExtra, CellName, RawAttrs, Sheet, Value};

    use super::*;

    fn number(v: f64) -> Cell {
        Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0")))
    }

    fn delta(cells: Vec<(&str, Cell)>, removed: &[&str]) -> own::Tier2Delta {
        let mut d = own::Tier2Delta::default();
        for (name, cell) in cells {
            d.cells.insert(CellName::new(name), cell);
        }
        d.removed_cells = removed.iter().map(|n| (*n).to_owned()).collect();
        d
    }

    fn frame() -> Sheet {
        let mut sheet = Sheet::default();
        for name in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY"] {
            sheet.cells.insert(CellName::new(name), number(1.0));
        }
        sheet
    }

    /// A turn cell the model gained at 0 is no edit of the group's frame, and every other change of a pose cell is.
    #[test]
    fn a_turn_cell_gained_at_zero_is_no_frame_edit_and_any_other_change_of_a_frame_cell_is() {
        let base = frame();
        // Gained at 0, a constant and nothing else: the turn the file's group has.
        for turn in ["Angle", "FlipX", "FlipY"] {
            assert!(!frame_cell_edited(&base, &delta(vec![(turn, number(0.0))], &[])), "{turn} at 0");
        }
        assert!(!frame_cell_edited(&base, &delta(vec![("Angle", number(0.0)), ("FlipX", number(0.0)), ("FlipY", number(0.0))], &[])));
        // A cell that is no pose cell, or no change at all.
        assert!(!frame_cell_edited(&base, &delta(vec![("Comment", Cell::constant(Value::from_text("hi")))], &[])));
        assert!(!frame_cell_edited(&base, &own::Tier2Delta::default()));
        // A turn that is not 0, a value that is no number, a cell with a formula, an error, an attribute or content of its own.
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", number(0.5))], &[])), "a turn");
        assert!(frame_cell_edited(&base, &delta(vec![("FlipX", number(1.0))], &[])), "a flip");
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", Cell::constant(Value::from_text("none")))], &[])), "no number");
        let mut with = number(0.0);
        with.formula = diacraft_doc::FormulaSlot::NoFormula;
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", with)], &[])), "a formula slot");
        let mut with = number(0.0);
        with.error = Some(Arc::from("#REF!"));
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", with)], &[])), "an error");
        let mut with = number(0.0);
        with.raw = RawAttrs::from_pairs([("X", "1")]);
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", with)], &[])), "an attribute");
        let mut with = number(0.0);
        with.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("t")), ..CellExtra::default() }));
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", with)], &[])), "content");
        // A cell the file's group has too is not gained: a change of it is an edit (a formula, or a pin set as a number).
        let mut had = frame();
        had.cells.insert(CellName::new("Angle"), number(0.0));
        assert!(frame_cell_edited(&had, &delta(vec![("Angle", number(0.0))], &[])), "present in both");
        // The other pose cells at 0 are not turn cells, and a cell taken out is an edit whatever it is.
        for name in ["PinX", "LocPinX", "Width", "BeginX", "BeginY", "EndX", "EndY"] {
            assert!(frame_cell_edited(&base, &delta(vec![(name, number(0.0))], &[])), "{name} at 0");
        }
        for name in ["LocPinX", "Angle", "FlipY"] {
            assert!(frame_cell_edited(&base, &delta(Vec::new(), &[name])), "{name} taken out");
        }
        assert!(!frame_cell_edited(&base, &delta(Vec::new(), &["Comment"])), "a cell of no frame taken out");
        // One cell that is an edit among the cells gained at 0 makes the change one.
        assert!(frame_cell_edited(&base, &delta(vec![("Angle", number(0.0)), ("PinX", number(0.0))], &[])));
    }

    /// The span of both: nothing is the other, a member that is not numbers makes the whole so, and two boxes are the box of both (each edge the nearer of the two).
    #[test]
    fn two_spans_join_into_the_box_of_both_and_a_member_that_is_not_numbers_spoils_the_whole() {
        let a = Span::Box((1.0, 2.0, 5.0, 6.0));
        let b = Span::Box((0.0, 3.0, 4.0, 9.0));
        assert_eq!(a.join(b), Span::Box((0.0, 2.0, 5.0, 9.0)));
        assert_eq!(b.join(a), Span::Box((0.0, 2.0, 5.0, 9.0)));
        assert_eq!(Span::Empty.join(a), a);
        assert_eq!(a.join(Span::Empty), a);
        assert_eq!(Span::Empty.join(Span::Empty), Span::Empty);
        assert_eq!(Span::Unreadable.join(a), Span::Unreadable);
        assert_eq!(a.join(Span::Unreadable), Span::Unreadable);
        assert_eq!(Span::Unreadable.join(Span::Empty), Span::Unreadable);
        assert_eq!(Span::Empty.join(Span::Unreadable), Span::Unreadable);
    }

    /// A span carried into a parent's frame: nothing stays nothing, a member that is not numbers stays so and so does a box with no map, and a box by a map is the box of
    /// its four corners (a shift moves it, a quarter turn about the origin swaps its sides).
    #[test]
    fn a_span_carried_into_the_parents_frame_is_the_box_of_its_corners_by_the_map() {
        let b = Span::Box((1.0, 2.0, 5.0, 6.0));
        let shift = Affine { e: 10.0, f: 20.0, ..Affine::IDENTITY };
        assert_eq!(Span::Empty.carried(Some(shift)), Span::Empty);
        assert_eq!(Span::Empty.carried(None), Span::Empty);
        assert_eq!(Span::Unreadable.carried(Some(shift)), Span::Unreadable);
        assert_eq!(Span::Unreadable.carried(None), Span::Unreadable);
        assert_eq!(b.carried(None), Span::Unreadable, "a map that is not numbers");
        assert_eq!(b.carried(Some(shift)), Span::Box((11.0, 22.0, 15.0, 26.0)));
        assert_eq!(b.carried(Some(Affine::IDENTITY)), b);
        let quarter = Affine { a: 0.0, b: 1.0, c: -1.0, d: 0.0, e: 0.0, f: 0.0 };
        assert_eq!(b.carried(Some(quarter)), Span::Box((-6.0, 1.0, -2.0, 5.0)), "x' = -y and y' = x");
    }

    fn a_group(local_pin: f64) -> NewShape {
        let mut shape = Shape::new(7);
        shape.kind = diacraft_doc::ShapeType::Group;
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, v) in [("PinX", 1.0), ("PinY", 1.0), ("Width", 2.0), ("Height", 2.0), ("LocPinX", local_pin), ("LocPinY", local_pin)] {
            sheet.cells.insert(CellName::new(name), number(v));
        }
        NewShape {
            shape: Arc::new(shape),
            after: crate::write::compare::Anchor::First,
            parent: None,
            outer: Affine::IDENTITY,
            at: diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(0), 0),
        }
    }

    /// What `check_frame` says of a group by what its members take up: the words when there is nothing, or a member is not numbers (the pass that finds the span never
    /// gives a group with members an empty one, so those are the words of a bug that is said and not a crash), `Ok(true)` for the box with the pin at its middle, and `Ok(false)` for
    /// a frame that is not the box and for a pin elsewhere.
    #[test]
    fn check_frame_says_words_for_no_span_and_a_member_that_is_not_numbers_and_tells_the_box_from_what_is_not() {
        let refused = |g: &NewShape, span: Span| match check_frame(g, span) {
            Err(WriteError::Unsupported(words)) => words,
            other => panic!("not refused: {other:?}"),
        };
        let g = a_group(1.0);
        assert_eq!(refused(&g, Span::Empty), "a new group with no members");
        assert_eq!(refused(&g, Span::Unreadable), "a new group with a member whose frame is not numbers");
        assert!(matches!(check_frame(&g, Span::Box((0.0, 0.0, 2.0, 2.0))), Ok(true)), "the box, the pin at the middle");
        assert!(matches!(check_frame(&g, Span::Box((0.0, 0.0, 2.0, 2.5))), Ok(false)), "a frame that is not the box");
        assert!(matches!(check_frame(&a_group(0.5), Span::Box((0.0, 0.0, 2.0, 2.0))), Ok(false)), "a pin that is not at the middle");
    }
}
