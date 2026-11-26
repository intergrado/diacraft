//! The hidden line-end masters (DD18).
//!
//! A line end that is a master of DiaCraft's own form (a filled diamond for UML composition) is a **hidden built-in master**: it is in no stencil, so no listing, window or search shows it
//! And a connector that uses it names it in `More::brings`, so that `ensure_master` copies it into the drawing first. They are series 22 in their GUIDs (`D1AC0016-...`, ids
//! `line-end.<name>`).
//!
//! **The master is written so that any program that follows the documented rule draws it**, because another program meets it in a saved file:
//!
//! * the page sheet's `PatternFlags` holds 2, a line end that turns with the line and is not scaled ([`pattern_flags`]); no other master of the catalogue holds a flag;
//! * it is drawn **pointing right**, with its **pin on its tip**: `LocPinX` is `Width` and `LocPinY` half the `Height` ([`pin_at_tip`]), the pin the line's end point is put on;
//! * its alignment box is square, `Width` equal to `Height` (called `h`), and an unscaled end is as tall as the line is heavy at Medium size. The drawing is not inside the box:
//!   the filled diamond's one closed section runs from the tip back `20 h` along the line, with its sides `10 h` back and `5 h` across it, so that it is twice as long as it is wide
//!   And at the default weight of 0.75 point it is 15 points long and 7.5 across;
//! * its section is filled and draws no outline (`NoLine`), and the shape's own fill is black, which is what a program draws it in that takes the master's own look. DiaCraft draws it in
//!   the line's colour, as it does every mark.

use super::{Dress, Flag, MasterSpec, More, Outline, PathSpec, Seg, TextBox, fh, fw, mix};

/// The prefix of the id of every hidden line end.
const ID_PREFIX: &str = "line-end.";

/// `PatternFlags` of a line end that turns with the line and is not scaled: bit 2.
const LINE_END: u32 = 2;

/// The side of the filled diamond's alignment box, in inches: its `Width` and its `Height`, `h`. Any size gives the same end, which is as tall as the weight; this one keeps the
/// numbers of the figure (a diamond 1 inch long and half an inch across) in a range a person reading the master finds ordinary.
const BOX: f64 = 0.05;

/// The filled diamond: the tip on the pin at the middle of the box's right edge, the sides `10 h` behind the tip and `5 h` above and below the line, the back `20 h` behind the tip,
/// back to the tip. A line of the rows of `Width` and `Height`, so that a resize of the master moves every vertex with it.
static FILLED_DIAMOND_PATH: [PathSpec; 1] = [PathSpec {
    fill: true,
    segs: &[
        Seg::Move(fw(1.0), fh(0.5)),
        Seg::Line(mix(1.0, -10.0), fh(5.5)),
        Seg::Line(mix(1.0, -20.0), fh(0.5)),
        Seg::Line(mix(1.0, -10.0), fh(-4.5)),
        Seg::Line(fw(1.0), fh(0.5)),
    ],
    no_show: Flag::Off,
    no_line: Flag::On,
}];

/// What the filled diamond's shape wears: a solid black fill and no outline, for a program that draws a master by its own look.
static FILLED_DIAMOND_DRESS: Dress = Dress { fill: Some("RGB(0,0,0)"), fill_pattern: Some(1), line_pattern: Some(Flag::Off), ..Dress::NONE };

static HIDDEN_FILLED_DIAMOND: More = More { dress: Some(&FILLED_DIAMOND_DRESS), hidden: true, brings: &[] };

/// The hidden line ends, in no stencil: [`super::find_any`] finds them for the connectors that bring them.
pub(super) static MASTERS: [MasterSpec; 1] = [MasterSpec {
    id: "line-end.filled-diamond",
    name: "Filled Diamond",
    width: BOX,
    height: BOX,
    identity: (22, 1),
    outline: Outline::Paths { paths: &FILLED_DIAMOND_PATH, text: TextBox::FULL },
    points: &[],
    more: &HIDDEN_FILLED_DIAMOND,
}];

/// The `PatternFlags` of the master `spec` is built with: 2 for a line end, 0 for every other master of the catalogue.
pub(super) fn pattern_flags(spec: &MasterSpec) -> u32 {
    if spec.id.starts_with(ID_PREFIX) { LINE_END } else { 0 }
}

/// Puts the pin of a line end on its tip, the middle of its right edge: `LocPinX` is `Width` where the generic master puts it at the middle of the box. Any other master is
/// left as it is built. `width` and `height` are the master's default size.
pub(super) fn pin_at_tip(
    spec: &MasterSpec,
    cells: &mut std::collections::BTreeMap<diacraft_doc::CellName, diacraft_doc::Cell>,
    width: f64,
    height: f64,
) {
    if pattern_flags(spec) == LINE_END {
        super::put(cells, "LocPinX", super::length_formula("Width", width));
        super::put(cells, "LocPinY", super::length_formula("Height*0.5", height * 0.5));
    }
}

#[cfg(test)]
mod tests;
