//! The shape the container gallery makes: a framed box with a heading, no master (`docs/editing-commands.md`, "Containers").
//!
//! (ours: the specifications say what a container does and that the gallery has frames with the heading in different places, and give no cell of its
//! look). A container is a **shape with no master** (like a text box, `text_box.rs`) that holds its own cells, so that a drawing needs no stencil to have one and a file's
//! reader that does not know the shape draws it from its rows:
//!
//! * **The frame** is `Geometry` rows that follow `Width` and `Height` (a rectangle, corner brackets, or only the heading's divider), with the line and fill the style gives.
//!   The look is ours, drawn in code: no picture of another application is copied. The 14 styles of [`STYLES`] are the combinations of four frames, a tint or none, a heading
//!   strip with a divider line or none, and the heading's alignment.
//! * **The heading** is the shape's text, in a text block that is a strip along one side (`Txt*` cells as formulas of `Width` and `Height`), turned a quarter turn on the
//!   left and right sides. The text is bold; its alignment is the `Paragraph` row's `HorzAlign`. "Typing with the container selected" is the text edit of the shape.
//! * **What makes it a container** is `User.msvStructureType` = `Container` and a `Relationships` cell of ties (`draw/container`).
//!
//! Every length is in the page's drawing units. The caller gives the strip's thickness (`band`), which is fixed in the drawing, not a share of the box: a taller box has
//! a taller body and the same heading.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Links, Section, Shape, ShapeType, Sheet, Text, TextPart, Value};

use super::{MASTERS, MasterSpec, More, Outline, flagged_section, length, length_formula, number_text, plain, put, row};
use crate::formatting::Rgb;

/// The side of the box the heading strip lies along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    /// The sides in the order Rotate Heading takes them: one step clockwise each time.
    pub const AROUND: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    /// The word programs use for the side.
    pub fn word(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Right => "right",
            Side::Bottom => "bottom",
            Side::Left => "left",
        }
    }

    /// The side a word names (`top`, `right`, `bottom`, `left`).
    pub fn from_word(word: &str) -> Option<Side> {
        Side::AROUND.into_iter().find(|s| s.word() == word)
    }

    /// The number the container's `User.DiaCraftContainerHeading` holds.
    pub fn number(self) -> u8 {
        match self {
            Side::Top => 0,
            Side::Right => 1,
            Side::Bottom => 2,
            Side::Left => 3,
        }
    }

    /// The side a number names; any other number is the top.
    pub fn from_number(n: f64) -> Side {
        Side::AROUND.into_iter().find(|s| (f64::from(s.number()) - n).abs() < 0.5).unwrap_or(Side::Top)
    }

    /// The side one step on, clockwise.
    pub fn next(self) -> Side {
        match self {
            Side::Top => Side::Right,
            Side::Right => Side::Bottom,
            Side::Bottom => Side::Left,
            Side::Left => Side::Top,
        }
    }

    /// Whether the heading runs down the box (its text is turned).
    pub fn is_vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// How the frame is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A thin solid outline.
    Plain,
    /// A heavy solid outline.
    Heavy,
    /// A dashed outline.
    Dashed,
    /// Only the four corners.
    Brackets,
    /// No outline at all: only the heading's divider line, if the style has a strip.
    Open,
    /// A dashed line along one edge only (a phase's separator): the left edge when the heading is at the top or the bottom, and the top edge when it is at a side.
    Edge,
}

/// One style of the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleSpec {
    /// A stable lowercase id programs use (`container.add {style}`).
    pub id: &'static str,
    /// The words the gallery shows.
    pub name: &'static str,
    pub frame: Frame,
    /// A light tint fills the box.
    pub tint: bool,
    /// A divider line closes the heading strip.
    pub strip: bool,
    /// The heading is centred along its strip (left otherwise).
    pub centred: bool,
}

const fn style(id: &'static str, name: &'static str, frame: Frame, tint: bool, strip: bool, centred: bool) -> StyleSpec {
    StyleSpec { id, name, frame, tint, strip, centred }
}

/// The gallery, in the order it shows (the specification's gallery has 14 tiles; the look of each is ours).
pub const STYLES: [StyleSpec; 14] = [
    style("frame", "Frame", Frame::Plain, false, false, false),
    style("frame-centre", "Frame, centred heading", Frame::Plain, false, false, true),
    style("strip", "Strip", Frame::Plain, false, true, false),
    style("strip-centre", "Strip, centred heading", Frame::Plain, false, true, true),
    style("tint", "Tint", Frame::Plain, true, false, false),
    style("tint-strip", "Tint and strip", Frame::Plain, true, true, false),
    style("tint-strip-centre", "Tint and strip, centred heading", Frame::Plain, true, true, true),
    style("dashed", "Dashed", Frame::Dashed, false, false, false),
    style("dashed-strip", "Dashed and strip", Frame::Dashed, false, true, false),
    style("brackets", "Corner brackets", Frame::Brackets, false, false, false),
    style("brackets-centre", "Corner brackets, centred heading", Frame::Brackets, false, false, true),
    style("heavy", "Heavy", Frame::Heavy, false, true, false),
    style("heavy-tint", "Heavy and tint", Frame::Heavy, true, true, true),
    style("underline", "Underline", Frame::Open, false, true, false),
];

/// The style a new container has when none is named.
pub const DEFAULT_STYLE: &str = "strip";

/// The styles that are not in the gallery: the look of a phase of a cross-functional flowchart, a dashed separator and a divider under its label. A container that has
/// one reads as one made from the gallery, so that its outline is made again for its size.
pub const PHASE_STYLE: StyleSpec = style("phase", "Phase", Frame::Edge, false, true, true);

/// The style with this id: the gallery's, and the phase's.
pub fn find_style(id: &str) -> Option<&'static StyleSpec> {
    static PHASE: StyleSpec = PHASE_STYLE;
    STYLES.iter().find(|s| s.id == id).or_else(|| (id == PHASE.id).then_some(&PHASE))
}

/// The ink of the frame and the heading's divider, and the tint of a tinted box.
const INK: Rgb = Rgb(89, 89, 89);
const TINT: Rgb = Rgb(232, 238, 247);

/// A line weight in inches on paper: 1 point is 1/72 inch.
const THIN: f64 = 1.0 / 72.0;
const THICK: f64 = 2.25 / 72.0;

/// The look of a container: what a style, a side and a size decide, ready to be put on a shape (new or old).
pub(crate) struct Look {
    /// The cells to write, by name.
    pub cells: Vec<(&'static str, Cell)>,
    /// The `Geometry` sections, `IX` 0 up.
    pub geometry: Vec<Section>,
    /// The `Paragraph` and `Character` sections.
    pub text_sections: Vec<Section>,
}

fn colour(rgb: Rgb) -> Cell {
    Cell::with_formula(&rgb.formula(), Some(Value::from_text(&rgb.hex())))
}

/// A coordinate of a path: the formula (of `Width` and `Height`) and the value it has at the box.
type Co = (String, f64);

fn co(formula: &str, value: f64) -> Co {
    (formula.to_owned(), value)
}

/// A section of one path through `points`, a `MoveTo` to the first and a `LineTo` to each other, back to the first when `closed`. **Filled and stroked, no flag**: `NoFill`,
/// `NoLine` and `NoShow` are all 0 (see [`look`]).
fn section_of(ix: u32, closed: bool, points: &[(Co, Co)]) -> Section {
    let mut section = flagged_section(ix, true);
    let at = |c: &Co| if c.0 == "0" { length(0.0) } else { length_formula(&c.0, c.1) };
    let back = if closed { points.first() } else { None };
    for (i, (x, y)) in points.iter().chain(back).enumerate() {
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        section.rows.push(row(u32::try_from(i + 1).unwrap_or(u32::MAX), Some(kind), vec![("X", at(x)), ("Y", at(y))]));
    }
    section
}

/// The look a container of `style` has with its heading on `side`, for a box of `width` by `height` and a strip `band` thick.
///
/// **The outline is shares of the box, and no flag**. The rows are in the forms DiaCraft's basic shapes use (`0`, `Width`, `Width*k` and the same of
/// `Height`), because those are the forms a custom shape of an `.odg` saved in place can hold, and a section with a flag (`NoFill`, `NoLine`, `NoShow`) is one it cannot yet
/// (a later version writes its sets); a container made in a drawing read from an `.odg` has to save. So every path is a **filled, stroked section** whose area is nothing where
/// it is not the ring: the strip's divider is two points, a corner bracket goes out and back along its two arms. The divider's and the brackets' places are shares worked out
/// for this size; a command that changes the size makes the outline again (`regeometry`), and a drag of the box by hand leaves the divider where its share puts it.
pub(crate) fn look(style: &StyleSpec, side: Side, (width, height): (f64, f64), band: f64) -> Look {
    let (w, h, zero) = (co("Width", width), co("Height", height), co("0", 0.0));
    let share_w = |k: f64| co(&format!("Width*{}", number_text(k)), width * k);
    let share_h = |k: f64| co(&format!("Height*{}", number_text(k)), height * k);
    let part = |amount: f64, whole: f64| if whole > 0.0 { (amount / whole).clamp(0.0, 1.0) } else { 0.0 };
    let mut geometry: Vec<Section> = Vec::new();
    let next = |geometry: &Vec<Section>| u32::try_from(geometry.len()).unwrap_or(u32::MAX);
    // The box: a ring that takes the fill and the outline. A style with no outline has no tint either (the gallery has none).
    if matches!(style.frame, Frame::Plain | Frame::Heavy | Frame::Dashed) {
        geometry.push(section_of(
            next(&geometry),
            true,
            &[(zero.clone(), zero.clone()), (w.clone(), zero.clone()), (w.clone(), h.clone()), (zero.clone(), h.clone())],
        ));
    }
    if style.frame == Frame::Brackets {
        // Each corner: two arms of a fifth of the shorter side, drawn out and back so that they enclose nothing.
        let arm = 0.2 * width.min(height);
        let (ax, ay) = (part(arm, width), part(arm, height));
        let (near_x, far_x, near_y, far_y) = (share_w(ax), share_w(1.0 - ax), share_h(ay), share_h(1.0 - ay));
        let corners: [[(Co, Co); 3]; 4] = [
            [(near_x.clone(), zero.clone()), (zero.clone(), zero.clone()), (zero.clone(), near_y.clone())],
            [(far_x.clone(), zero.clone()), (w.clone(), zero.clone()), (w.clone(), near_y.clone())],
            [(w.clone(), far_y.clone()), (w.clone(), h.clone()), (far_x, h.clone())],
            [(zero.clone(), far_y), (zero.clone(), h.clone()), (near_x, h.clone())],
        ];
        for [a, corner, c] in corners {
            geometry.push(section_of(next(&geometry), true, &[a, corner.clone(), c, corner]));
        }
    }
    // The separator of a phase: one edge of the box, the line that marks where the phase begins.
    if style.frame == Frame::Edge {
        let ends: [(Co, Co); 2] = if side.is_vertical() {
            [(zero.clone(), h.clone()), (w.clone(), h.clone())]
        } else {
            [(zero.clone(), zero.clone()), (zero.clone(), h.clone())]
        };
        geometry.push(section_of(next(&geometry), false, &ends));
    }
    // The divider of the strip: along its inner edge, the whole length of the side.
    if style.strip {
        let ends: [(Co, Co); 2] = match side {
            Side::Top => {
                let y = share_h(1.0 - part(band, height));
                [(zero.clone(), y.clone()), (w.clone(), y)]
            }
            Side::Bottom => {
                let y = share_h(part(band, height));
                [(zero.clone(), y.clone()), (w.clone(), y)]
            }
            Side::Left => {
                let x = share_w(part(band, width));
                [(x.clone(), zero.clone()), (x, h.clone())]
            }
            Side::Right => {
                let x = share_w(1.0 - part(band, width));
                [(x.clone(), zero.clone()), (x, h.clone())]
            }
        };
        geometry.push(section_of(next(&geometry), false, &ends));
    }
    text_look(style, side, (width, height), band, geometry)
}

/// The cells and the text sections of a look, with the geometry already made.
fn text_look(style: &StyleSpec, side: Side, (width, height): (f64, f64), band: f64, geometry: Vec<Section>) -> Look {
    let mut cells: Vec<(&'static str, Cell)> = vec![
        ("FillPattern", plain(if style.tint { 1.0 } else { 0.0 })),
        ("FillForegnd", colour(TINT)),
        ("LinePattern", plain(if matches!(style.frame, Frame::Dashed | Frame::Edge) { 2.0 } else { 1.0 })),
        ("LineColor", colour(INK)),
        ("LineWeight", length(if style.frame == Frame::Heavy { THICK } else { THIN })),
        ("VerticalAlign", plain(1.0)),
    ];
    // The heading's text block: a strip along the side. The block's pin is its middle, and a quarter turn takes the block's width along the side.
    let half = band * 0.5;
    let (tx, ty, tw, angle): (Cell, Cell, Cell, f64) = match side {
        Side::Top => (
            length_formula("Width*0.5", width * 0.5),
            length_formula(&format!("Height-{}", number_text(half)), height - half),
            length_formula("Width*1", width),
            0.0,
        ),
        Side::Bottom => (length_formula("Width*0.5", width * 0.5), length(half), length_formula("Width*1", width), 0.0),
        Side::Left => (length(half), length_formula("Height*0.5", height * 0.5), length_formula("Height*1", height), 90.0),
        Side::Right => (
            length_formula(&format!("Width-{}", number_text(half)), width - half),
            length_formula("Height*0.5", height * 0.5),
            length_formula("Height*1", height),
            -90.0,
        ),
    };
    let along = if side.is_vertical() { height } else { width };
    cells.push(("TxtPinX", tx));
    cells.push(("TxtPinY", ty));
    cells.push(("TxtWidth", tw));
    cells.push(("TxtHeight", length(band)));
    cells.push(("TxtLocPinX", length_formula("TxtWidth*0.5", along * 0.5)));
    cells.push(("TxtLocPinY", length_formula("TxtHeight*0.5", half)));
    cells.push(("TxtAngle", plain(f64::to_radians(angle))));
    let mut paragraph = Section::new("Paragraph", None);
    paragraph.rows.push(row(0, None, vec![("HorzAlign", plain(if style.centred { 1.0 } else { 0.0 }))]));
    let mut character = Section::new("Character", None);
    // Bold: bit 1 of `Style`.
    character.rows.push(row(0, None, vec![("Style", plain(1.0))]));
    Look { cells, geometry, text_sections: vec![paragraph, character] }
}

/// The user cells of a container that make it one: the role, and the settings (`User.DiaCraftContainer*`, ours: the specifications name no cell for the margin, the
/// resizing, the lock or the heading). `margin` is in drawing units.
pub(crate) fn user_rows(style: &StyleSpec, side: Side, margin: f64, resize: u8, locked: bool) -> Section {
    let mut user = Section::new("User", None);
    // A text a cell holds says `STR`.
    let text = |t: &str| Cell::with_formula(&format!("\"{t}\""), Some(Value::from_text(t))).with_unit("STR");
    user.rows.push(user_row("msvStructureType", text("Container")));
    user.rows.push(user_row(STYLE_ROW, text(style.id)));
    user.rows.push(user_row(HEADING_ROW, plain(f64::from(side.number()))));
    user.rows.push(user_row(MARGIN_ROW, length(margin)));
    user.rows.push(user_row(RESIZE_ROW, plain(f64::from(resize))));
    user.rows.push(user_row(LOCK_ROW, plain(f64::from(u8::from(locked)))));
    user
}

/// The names of the `User` rows of a container's settings.
pub(crate) const STYLE_ROW: &str = "DiaCraftContainerStyle";
pub(crate) const HEADING_ROW: &str = "DiaCraftContainerHeading";
pub(crate) const MARGIN_ROW: &str = "DiaCraftContainerMargin";
pub(crate) const RESIZE_ROW: &str = "DiaCraftContainerResize";
pub(crate) const LOCK_ROW: &str = "DiaCraftContainerLock";

/// Adds rows to the `User` section of a shape (the section a container has), or makes the rows of the same name new (the swimlane roles).
pub(crate) fn put_user_rows(shape: &mut Shape, rows: Vec<(&str, Cell)>) {
    let mut counted = shape.counted_sheet();
    let place = match counted.sheet.sections.iter().position(|s| s.name.as_str() == "User") {
        Some(place) => place,
        None => counted.push_section(Arc::new(Section::new("User", None))),
    };
    let Some(slot) = counted.sheet.sections.get_mut(place) else { return };
    let section = Arc::make_mut(slot);
    for (name, value) in rows {
        section.retain_rows(|r| r.name.as_deref() != Some(name));
        section.append_row(user_row(name, value));
    }
}

/// Takes rows out of the `User` section of a shape.
pub(crate) fn drop_user_rows(shape: &mut Shape, names: &[&str]) {
    let counted = shape.counted_sheet();
    if let Some(slot) = counted.sheet.sections.iter_mut().find(|s| s.name.as_str() == "User") {
        Arc::make_mut(slot).retain_rows(|r| !r.name.as_deref().is_some_and(|n| names.contains(&n)));
    }
}

fn user_row(name: &str, value: Cell) -> Arc<diacraft_doc::Row> {
    let mut row = diacraft_doc::Row { name: Some(Arc::from(name)), ..diacraft_doc::Row::default() };
    row.cells.insert(CellName::new("Value"), value);
    Arc::new(row)
}

/// The four middles of the sides, as the Rectangle master has them, so that a connector can be glued to a container (a connector dragged to its edge).
fn sides(width: f64, height: f64) -> Section {
    let points = MASTERS.iter().find(|m| m.id == "rectangle").map_or(&[][..], |m| m.points);
    super::connections(&MasterSpec {
        identity: (0, 0),
        id: "container",
        name: "Container",
        width,
        height,
        outline: Outline::Polygon(&[]),
        points,
        more: &More::NONE,
    })
}

/// What a new container is made with.
pub(crate) struct Made<'a> {
    pub id: u32,
    pub centre: (f64, f64),
    pub size: (f64, f64),
    pub style: &'static StyleSpec,
    pub side: Side,
    /// The strip's thickness and the margin round the members, in drawing units.
    pub band: f64,
    pub margin: f64,
    pub resize: u8,
    pub heading: &'a str,
}

/// The shape `made` describes: a container with no members (its `Relationships` is added with the first).
pub(crate) fn container_shape(made: &Made<'_>) -> Shape {
    let (width, height) = made.size;
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    put(cells, "PinX", length(made.centre.0));
    put(cells, "PinY", length(made.centre.1));
    put(cells, "Width", length(width));
    put(cells, "Height", length(height));
    put(cells, "LocPinX", length_formula("Width*0.5", width * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", height * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    let look = look(made.style, made.side, made.size, made.band);
    for (name, cell) in look.cells {
        put(&mut sheet.cells, name, cell);
    }
    sheet.sections.extend(look.geometry.into_iter().map(Arc::new));
    sheet.sections.extend(look.text_sections.into_iter().map(Arc::new));
    sheet.sections.push(Arc::new(user_rows(made.style, made.side, made.margin, made.resize, false)));
    sheet.sections.push(Arc::new(sides(width, height)));
    let text = (!made.heading.is_empty()).then(|| Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(made.heading))], ..Default::default() }));
    Shape { kind: ShapeType::Shape, sheet: Arc::new(sheet), text, ..Shape::new(made.id) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_gallery_has_fourteen_styles_with_distinct_ids_and_names() {
        assert_eq!(STYLES.len(), 14);
        let mut ids: Vec<&str> = STYLES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 14, "ids differ");
        let mut names: Vec<&str> = STYLES.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 14, "names differ");
        assert!(find_style(DEFAULT_STYLE).is_some());
    }

    #[test]
    fn every_style_is_sections_with_rows_in_the_forms_of_basic_shapes_and_no_flag_for_every_side() {
        // A flag on a section (`NoFill`, `NoLine`) and a coordinate that is not `0`, `Width`, `Width*k` or the same of `Height` are what an `.odg` saved in place cannot hold yet.
        for style in &STYLES {
            for side in Side::AROUND {
                let look = look(style, side, (4.0, 3.0), 0.3);
                assert!(!look.geometry.is_empty(), "{} {side:?} draws something", style.id);
                for section in &look.geometry {
                    assert!(!section.rows.is_empty());
                    for flag in ["NoFill", "NoLine", "NoShow"] {
                        let value =
                            section.cells.iter().find(|(n, _)| n.as_str() == flag).and_then(|(_, c)| c.value.as_ref()).map(|v| v.text().to_owned());
                        assert_eq!(value.as_deref(), Some("0"), "{} {flag}", style.id);
                    }
                    for row in &section.rows {
                        for (name, cell) in row.cells.iter() {
                            let form = cell.formula.text().unwrap_or("0");
                            let ok = form == "0"
                                || form == "Width"
                                || form == "Height"
                                || form.strip_prefix("Width*").or_else(|| form.strip_prefix("Height*")).is_some_and(|k| k.parse::<f64>().is_ok());
                            assert!(ok, "{} {side:?} {name}: {form}", style.id);
                        }
                    }
                }
                assert!(
                    !style.tint || matches!(style.frame, Frame::Plain | Frame::Heavy | Frame::Dashed),
                    "a tint needs a ring to fill: {}",
                    style.id
                );
            }
        }
    }

    #[test]
    fn sides_go_round_clockwise_and_come_back() {
        let mut side = Side::Top;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(side.word());
            side = side.next();
        }
        assert_eq!(seen, ["top", "right", "bottom", "left"]);
        assert_eq!(side, Side::Top);
        for s in Side::AROUND {
            assert_eq!(Side::from_word(s.word()), Some(s));
            assert_eq!(Side::from_number(f64::from(s.number())), s);
        }
        assert_eq!(Side::from_word("middle"), None);
        assert_eq!(Side::from_number(f64::NAN), Side::Top);
    }

    #[test]
    fn a_container_has_its_role_its_settings_its_strip_and_four_connection_points() {
        let made = Made {
            id: 7,
            centre: (4.0, 3.0),
            size: (4.0, 2.0),
            style: find_style("strip").unwrap(),
            side: Side::Top,
            band: 0.3,
            margin: 0.25,
            resize: 1,
            heading: "Stage",
        };
        let shape = container_shape(&made);
        assert_eq!(shape.id, 7);
        let user = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").unwrap();
        let names: Vec<&str> = user.rows.iter().filter_map(|r| r.name.as_deref()).collect();
        assert_eq!(names, ["msvStructureType", STYLE_ROW, HEADING_ROW, MARGIN_ROW, RESIZE_ROW, LOCK_ROW]);
        let geometry = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count();
        assert_eq!(geometry, 2, "the box and the divider");
        let connection = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Connection").unwrap();
        assert_eq!(connection.rows.len(), 4);
    }
}
