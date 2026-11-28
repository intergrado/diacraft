//! Built-in group masters: a master whose top-level shape is a `Group` with no geometry of its own, and whose members are shapes 6 and up.
//!
//! **Why a group.** A shape has one fill, one line pattern and one text, and every section shares them. A figure that needs two looks or two texts (a class's
//! three compartments, a lifeline's solid head over its dashed stem, a dark marker inside a light body) is members, each with its own. The group holds the connection points, the Shape Data, the
//! locks and the group properties, and its text is its members'.
//!
//! **Members follow the group by formulas**. A member's frame cells are formulas of the group (`Sheet.5!Width`) and of the members above it (`Sheet.6!Height`), each wrapped
//! in `GUARD`, so that a click or a drag moves only the group: a guarded write is refused in words. On an instance a formula that names a master shape reads the instance
//! (`Resolver::rebind`), and one that names a member reads the instance's copy of it, so two instances on a page never read each other.
//!
//! **A length a member's formulas hold as a constant follows the page's drop scale** (owner 2026-11-27): a member's frame, its outline and its connection points write each fixed
//! length times the group's factor, `<length>*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)` ([`drop_factor`] with [`Factor::Group`]), since a drop beyond the range of eight of the
//! page's drawing scale writes the factor on the group alone and no member holds one. A table writes it with `scaled!` (a macro of the stencil's root, so that a table of
//! `&'static str` can hold the text); the outline and the points get it from `Co`. A figure dropped beyond the range is then the same on paper as at 1:1, and a drawing whose root style holds no
//! factor reads 1 and keeps the lengths it had.
//!
//! **Compartments** ([`MemberFrame::Compartment`], `kit::compartments`). A compartment is as wide as the group, as tall as its text needs (`MAX(<least>*<factor>,TEXTHEIGHT(TheText,Width))`, where the
//! factor is the group's `DropOnPageScale` or 1 ([`drop_factor`]: the least height follows the page's drop scale, as a drop beyond the range of eight of it writes the factor)) and stands under
//! the compartments of lower rank: **its rank says where it stands (0 is the top) and the order of the members does not**, so that the member last in the table, the one typing goes to
//! Can be the name at the top. The group is as tall as its compartments together (its `Height` is `GUARD` of their sum), holds `LockHeight` 1 (a resize changes its width only
//! and typing a line makes it taller) and `IsTextEditTarget` 0 (typing goes to the members). **It grows and shrinks about its middle**: its pin stays at the middle of its box, as every group's must
//! for an `.odg` save to keep its frame (`odg` `groups.rs` `frame_is_box`: a `draw:g` has no frame of its own, and a pin set away from the middle is a note the per-stencil check refuses). A table's
//! dress says otherwise where it needs to (the dress is applied after these). A compartment's text is plain: a text that holds fields is not measured (`calc` `fn_measure.rs`), so the height would
//! stand where it was. **A compartment holds a `Rounding` of 0 of its own**: the compartments stand against one another, and a theme with rounded corners
//! would round each by itself (a notch in every join), where a cell of the shape outranks the `Theme` style's.
//!
//! **The build.** [`group_cells`] writes the group's own cells before its dress, and [`build_members`] makes the member shapes, each from the same parts a master's top-level shape is made of (an
//! outline, connection points, a dress). A master holds at most [`MAX_MEMBERS`] members (design §4.9) and the build never goes past it. Formulas have no cached value in the table: the master is
//! worked out alone when it is built (`settle_values`), so a cached value is what its formula gives.

#![allow(dead_code, reason = "the vocabulary of the group masters' build: a family uses a variant when it lands")]

use std::sync::Arc;

use diacraft_doc::{Cell, Links, Shape, ShapeType, Sheet};

use super::dress::{self, Dress};
use super::{
    MASTER_SHAPE_ID, MAX_ROWS, MAX_SECTIONS, MasterSpec, Outline, Point, TextBox, connection_section, length, length_formula, number_text,
    outline_sections, plain, put,
};

/// The most members a group master holds (design §4.9).
pub(super) const MAX_MEMBERS: usize = 32;

/// The frame of one member: where it stands and how big it is, as cells of the group.
#[derive(Clone, Copy, Debug)]
pub(super) enum MemberFrame {
    /// Four formula texts written in the table: of the group (`Sheet.5!Width*0.5`) and of the members above it (`Sheet.6!Height`). Each is `GUARD`ed when built, so that only the group moves.
    Formulas { pin_x: &'static str, pin_y: &'static str, width: &'static str, height: &'static str },
    /// A compartment: as wide as the group, stacked from its top under the compartments of lower `rank` (0 is the top; equal ranks stand in the order of the members), and as tall as its text needs,
    /// and at least `min_in` inches on the master's paper (times the group's `DropOnPageScale`, 1 where it holds none: [`drop_factor`]).
    Compartment { rank: u8, min_in: f64 },
}

/// One member of a group master.
pub(super) struct MemberSpec {
    /// The member's name in the master (`Name`, `Attributes`, `Stem`).
    pub(super) name: &'static str,
    pub(super) frame: MemberFrame,
    /// A polygon, an ellipse, paths or code: a member is neither a group nor a connector (one that is draws nothing).
    pub(super) outline: Outline,
    pub(super) points: &'static [Point],
    pub(super) dress: Option<&'static Dress>,
}

/// The `ID` of the member at `place` in the master: 6 and up (`MASTER_SHAPE_ID` is the group's).
fn member_id(place: usize) -> u32 {
    MASTER_SHAPE_ID.saturating_add(1).saturating_add(u32::try_from(place).unwrap_or(u32::MAX))
}

/// The members a build takes: the first [`MAX_MEMBERS`].
fn taken(members: &[MemberSpec]) -> &[MemberSpec] {
    members.get(..MAX_MEMBERS).unwrap_or(members)
}

/// The places of the compartments among `members`, from the top: by rank, and by the order of the members among equal ranks.
fn stack(members: &[MemberSpec]) -> Vec<usize> {
    let mut found: Vec<(u8, usize)> = taken(members)
        .iter()
        .enumerate()
        .filter_map(|(place, m)| match m.frame {
            MemberFrame::Compartment { rank, .. } => Some((rank, place)),
            MemberFrame::Formulas { .. } => None,
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, place)| place).collect()
}

/// Whether the master is made of compartments: its height is theirs.
pub(super) fn has_compartments(members: &[MemberSpec]) -> bool {
    taken(members).iter().any(|m| matches!(m.frame, MemberFrame::Compartment { .. }))
}

/// A formula text as a guarded one: `GUARD(<text>)`, and not twice.
fn guarded(text: &str) -> String {
    if text.trim_start().starts_with("GUARD(") { text.to_owned() } else { format!("GUARD({text})") }
}

/// A length formula with no cached value: the master's build works it out.
fn unvalued(text: &str) -> Cell {
    Cell::with_formula(text, None).with_unit("IN")
}

/// The least height of a compartment, in inches: a number within 0 and the largest a master may be.
fn least(min_in: f64) -> f64 {
    if min_in.is_finite() { min_in.clamp(0.0, 10.0) } else { 0.0 }
}

/// Whose `DropOnPageScale` a formula of a master's shape reads as its factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Factor {
    /// The shape's own: a shape of one piece (and the top-level shape of a group) holds the factor a drop beyond the range of eight writes on the instance's top-level shape, which is the shape
    /// itself.
    Shape,
    /// The group's: a member of a group master holds no factor of its own, so it reads the group's, which is the master's top-level shape ([`MASTER_SHAPE_ID`]) and
    /// which an instance's formula that names it reads on the instance (`Sheet.5!`).
    Group,
}

/// The factor as the text of a formula: `IF(DropOnPageScale>0,DropOnPageScale,1)`, or `IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)` for a member.
const SHAPE_FACTOR: &str = "IF(DropOnPageScale>0,DropOnPageScale,1)";
const GROUP_FACTOR: &str = "IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)";

/// The factor by which a length the master holds as a constant follows the page's drop scale (owner 2026-11-26, owner 2026-11-27):
/// the `DropOnPageScale` that a drop beyond the range of eight writes on the instance's top-level shape, and 1 where the shape reads none. A cell found nowhere reads 0
/// As it does in a drawing whose root style has no such cell, and a factor of 0 would take the length away, so the formula falls back to 1 there and such a drawing
/// keeps the lengths it had. **A length written in a formula is written `<length>*<factor>`**: `0.4*IF(DropOnPageScale>0,DropOnPageScale,1)`. A shape of one piece reads its own cell
/// ([`Factor::Shape`]), and a member of a group the group's ([`Factor::Group`], `Sheet.5!`: no member holds a factor of its own). Only a length is scaled: a share of the
/// width or the height already follows the size, and a ratio, an angle or a count is no length.
pub(super) fn drop_factor(of: Factor) -> &'static str {
    match of {
        Factor::Shape => SHAPE_FACTOR,
        Factor::Group => GROUP_FACTOR,
    }
}

/// The four frame formulas of a member (not yet guarded), in the order `(PinX, PinY, Width, Height)`, given the places of the compartments above it.
fn frame_texts(frame: &MemberFrame, above: &[usize]) -> [String; 4] {
    let group = format!("Sheet.{MASTER_SHAPE_ID}");
    match frame {
        MemberFrame::Formulas { pin_x, pin_y, width, height } => {
            [(*pin_x).to_owned(), (*pin_y).to_owned(), (*width).to_owned(), (*height).to_owned()]
        }
        MemberFrame::Compartment { min_in, .. } => {
            let mut pin_y = format!("{group}!Height");
            for place in above {
                pin_y.push_str(&format!("-Sheet.{}!Height", member_id(*place)));
            }
            pin_y.push_str("-Height*0.5");
            let height = format!("MAX({}*{},TEXTHEIGHT(TheText,Width))", number_text(least(*min_in)), drop_factor(Factor::Group));
            [format!("{group}!Width*0.5"), pin_y, format!("{group}!Width"), height]
        }
    }
}

/// What a master of this outline adds to its top-level shape's own cells, **before its dress** (so that a dress says otherwise): for a group of compartments, a height that is the sum of theirs,
/// `LockHeight` and `IsTextEditTarget` 0. Its pin stays at the middle of its box.
pub(super) fn group_cells(members: &[MemberSpec], sheet: &mut Sheet) {
    let places = stack(members);
    if places.is_empty() {
        return;
    }
    let sum: Vec<String> = places.iter().map(|place| format!("Sheet.{}!Height", member_id(*place))).collect();
    let cells = &mut sheet.cells;
    put(cells, "Height", unvalued(&guarded(&sum.join("+"))));
    put(cells, "LockHeight", plain(1.0));
    put(cells, "IsTextEditTarget", plain(0.0));
}

/// The member shapes of a group master `spec`, in order (6 and up), within the master's caps.
pub(super) fn build_members(spec: &MasterSpec, members: &[MemberSpec]) -> Vec<Arc<Shape>> {
    let (mut rows_left, mut sections_left) = (MAX_ROWS, MAX_SECTIONS);
    let places = stack(members);
    let mut out = Vec::with_capacity(members.len().min(MAX_MEMBERS));
    for (place, member) in taken(members).iter().enumerate() {
        // The compartments above this one: those before it in the stack.
        let above: Vec<usize> = places.iter().copied().take_while(|other| *other != place).collect();
        out.push(Arc::new(member_shape(spec, place, member, &above, (&mut rows_left, &mut sections_left))));
    }
    out
}

/// One member: its frame (guarded formulas), its pose, its text block, its outline, its connection points and its dress, linked to the root style like every master's shape.
fn member_shape(
    spec: &MasterSpec,
    place: usize,
    member: &MemberSpec,
    above: &[usize],
    (rows_left, sections_left): (&mut usize, &mut usize),
) -> Shape {
    let id = member_id(place);
    // The nominal size the cached values of the outline are first made at; the master's own recalculation gives them what the formulas give.
    let (w, h) = match member.frame {
        MemberFrame::Compartment { min_in, .. } => (spec.width, least(min_in)),
        MemberFrame::Formulas { .. } => (spec.width, spec.height),
    };
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    let [pin_x, pin_y, width, height] = frame_texts(&member.frame, above);
    for (name, text) in [("PinX", pin_x), ("PinY", pin_y), ("Width", width), ("Height", height)] {
        put(cells, name, unvalued(&guarded(&text)));
    }
    put(cells, "LocPinX", length_formula("Width*0.5", w * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", h * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    if matches!(member.frame, MemberFrame::Compartment { .. }) {
        // A compartment shares its edges with the ones above and below it, and a theme with rounded corners gives every shape that follows the `Theme` style a `Rounding` (`THEMEVAL`): each
        // compartment would round all four of its corners and leave a notch in every join. A cell of the shape outranks its style, so the compartment holds its own, 0, and the
        // stack reads as one box. (The model has no rounding of the outer corners alone, so the stack is square.)
        put(cells, "Rounding", length(0.0));
    }
    let text = member.outline.text_box();
    let (centre_x, centre_y) = (text.x + text.w / 2.0, text.y + text.h / 2.0);
    put(cells, "TxtPinX", length_formula(&format!("Width*{}", number_text(centre_x)), centre_x * w));
    put(cells, "TxtPinY", length_formula(&format!("Height*{}", number_text(centre_y)), centre_y * h));
    put(cells, "TxtLocPinX", length_formula("TxtWidth*0.5", text.w * w * 0.5));
    put(cells, "TxtLocPinY", length_formula("TxtHeight*0.5", text.h * h * 0.5));
    put(cells, "TxtWidth", length_formula(&format!("Width*{}", number_text(text.w)), text.w * w));
    put(cells, "TxtHeight", length_formula(&format!("Height*{}", number_text(text.h)), text.h * h));
    put(cells, "TxtAngle", plain(0.0));
    for mut section in outline_sections(&member.outline, (w, h), Factor::Group) {
        if *sections_left == 0 {
            break;
        }
        *sections_left -= 1;
        let keep = section.rows.len().min(*rows_left);
        section.rows.truncate(keep);
        *rows_left -= keep;
        sheet.sections.push(Arc::new(section));
    }
    if !member.points.is_empty() {
        sheet.sections.push(Arc::new(connection_section(member.points, (w, h), Factor::Group)));
    }
    let text = member.dress.and_then(|dress| dress::decorate(dress, &mut sheet, Factor::Group)).map(Arc::new);
    Shape {
        id,
        name: Some(Arc::from(member.name)),
        name_u: Some(Arc::from(member.name)),
        kind: ShapeType::Shape,
        sheet: Arc::new(sheet),
        text,
        ..Shape::new(id)
    }
}

impl Outline {
    /// Where the text goes.
    pub(super) fn text_box(&self) -> TextBox {
        match self {
            Outline::Paths { text, .. } | Outline::Code { text, .. } | Outline::Group { text, .. } => *text,
            Outline::Polygon(_) | Outline::Ellipse | Outline::Connector(_) | Outline::Fixed1D { .. } => TextBox::FULL,
        }
    }
}

/// How many `Geometry` sections and rows the members' outlines make, and how many connection points they have: what counts against the master's caps.
pub(super) fn size_of_members(members: &[MemberSpec], outline_size: impl Fn(&Outline) -> (usize, usize)) -> (usize, usize, usize) {
    let mut total = (0_usize, 0_usize, 0_usize);
    for member in members {
        let (sections, rows) = outline_size(&member.outline);
        total = (total.0.saturating_add(sections), total.1.saturating_add(rows), total.2.saturating_add(member.points.len()));
    }
    total
}
