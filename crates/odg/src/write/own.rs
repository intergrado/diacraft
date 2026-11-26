//! The ownership table in code: which ODF attributes the writer writes, which cells feed each, and
//! what a change to one of those cells does; and the arithmetic that turns the cells of a shape into the attributes of its element and back.
//!
//! **The families.** What a save writes is a few families of attribute, each fed by named cells ([`TABLE`]); everything else of the element
//! stays as the file has it (the last column, "kept"). *Transform* is a shape's place, size and turn (`svg:x`, `svg:y`, `svg:width`,
//! `svg:height`, `draw:transform`; a line's `svg:x1` to `svg:y2`; a circle's `svg:cx`...); *Route* is a connector's path and its ends'
//! glue; *Order* is a shape's place among its sisters (the element's place, and `draw:z-index`). *Geometry* (`draw:enhanced-path` and the
//! other geometry attributes), *Text* and *Style* are the families the table also names and this task does not write: a change to a cell that
//! feeds one is found ([`shape_change`]) and refused as a change the writer does not yet write, never dropped, and writes them.
//! **The editor of attributes writes only attributes of this table** ([`owns`]): one that is not in it is refused, so the table says what the
//! writer writes.
//!
//! **The cells and the page.** A model's lengths are inches and its angles radians, with the page's origin at its lower left and y running
//! up; an element's are in its own units with y running down from the page's top. A shape's *pose* on the page is worked out through
//! every group around it: the frame's centre, the linear part of the turn and flip, and the size. The save compares the pose of
//! the model as it is with the pose of the model as it was read, and writes what differs: nothing, a **translation** (the frame moved and is
//! the same size and turn: the element's translation moves and a shear and every other term stay), or a **reshape** (written whole in the
//! canonical form of R7, dropping a shear with one note).

use diacraft_doc::{Passthrough, Shape, ShapeType, Sheet};

use crate::read::Affine;
use crate::write::lengths::same;

/// One family of what the writer writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    /// A shape's place, size and turn.
    Transform,
    /// A connector's path and glue.
    Route,
    /// A shape's place among its sisters.
    Order,
    /// A shape's outline (`draw:enhanced-path`, `draw:points`, `svg:d` of a path): not written by this task.
    Geometry,
    /// A shape's text: not written by this task.
    Text,
    /// A shape's styles and every other cell: not written by this task.
    Style,
    /// The attributes of a shape's automatic style that the cells of a restyle feed.
    Restyle,
    /// The attributes of a paragraph's or a run's automatic style that the cells of its row feed (R8, "Styles" and "Text"): written into the style the file's paragraph or
    /// span names, and into no shape's own style (a separate family from [`Family::Restyle`]).
    TextStyle,
    /// A page's size, margins and orientation, and the master page and layout that hold them.
    Page,
    /// The attribute that holds a shape's outline: `draw:points`, `draw:enhanced-path`, and a custom shape's `draw:modifiers`; and `svg:d` of a path.
    Outline,
    /// A shape's layer (`draw:layer`), written on a shape of the file only when the model puts it on a layer the save declares, and a layer's flags
    /// (`draw:display`, `draw:protected` of a `draw:layer` of the document's layer set).
    Layer,
}

/// What an edit of a shape's outline is: the model's sheet, which the outline is written from, whether its `Geometry` sections changed, and the
/// modifiers a handle changed (the place in `draw:modifiers` and the value).
#[derive(Clone, Debug)]
pub(crate) struct GeoEdit {
    pub sheet: std::sync::Arc<Sheet>,
    pub outline: bool,
    pub modifiers: Vec<(usize, f64)>,
}

/// The sections of a shape's sheet the writer sets apart when it looks for what only the extension can carry: the outline's, the handles', the modifiers' and
/// the two the text's formatting is read from. Each is the model's name of the section (a formula spells two of them another way).
pub(crate) const SPECIAL_SECTIONS: [&str; 5] = ["Geometry", crate::sections::CONTROL, crate::sections::USER, "Character", "Paragraph"];

/// The modifiers whose value differs between two sheets: the `User` rows `ODFMod<n>` by the number their `Value` holds (a handle's drag changes
/// the cached value of a formula and not the formula, which [`cell_same`] does not see).
fn modifier_changes(old: &Sheet, new: &Sheet, steps: &mut u64) -> Vec<(usize, f64)> {
    // The `User` section of each sheet is indexed once, by the number after `ODFMod` in a row's name: one pass of its rows, each a step, and one
    // look in the index for each modifier of the new sheet (a search of the rows for each modifier, by a name made for it, was quadratic in the rows).
    let index = |sheet: &Sheet, steps: &mut u64| -> std::collections::HashMap<usize, f64> {
        let mut out = std::collections::HashMap::new();
        for user in sheet.sections.iter().filter(|s| s.name.as_str() == "User") {
            for row in user.rows.iter() {
                *steps = steps.saturating_add(1);
                let number =
                    row.name.as_deref().and_then(|n| n.strip_prefix("ODFMod")).filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
                if let (Some(n), Some(value)) =
                    (number.and_then(|d| d.parse::<usize>().ok()), row.cells.get("Value").and_then(|c| c.value.as_ref()?.number()))
                {
                    out.entry(n).or_insert(value);
                }
            }
        }
        out
    };
    let was = index(old, steps);
    let mut changes: Vec<(usize, f64)> = Vec::new();
    for (n, b) in index(new, steps) {
        *steps = steps.saturating_add(1);
        if was.get(&n).is_some_and(|a| *a != b) {
            changes.push((n, b));
        }
    }
    changes.sort_unstable_by_key(|c| c.0);
    changes
}

/// What a change to the cells of a family does to its attributes (the column "On a cell edit").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Write the attribute again from the cells, in its place.
    Regenerate,
    /// Move the element, or write its `draw:z-index` where the page's shapes carry it.
    Place,
    /// A change is refused with the words given: a later task writes it.
    Refuse,
}

/// A row of the table: a family, the attributes it writes, the cells that feed them, and what an edit does.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub family: Family,
    /// The attributes (as `prefix:local` with the prefix of ODF's own conventions) the writer writes for it.
    pub attributes: &'static [&'static str],
    /// The cells that feed them: a change to one is a change to the family. The table's column of cells, kept beside the attributes so that the table reads as
    /// the specification's does; the save finds the cells of a family by its own comparisons.
    #[allow(dead_code)]
    pub cells: &'static [&'static str],
    pub action: Action,
    /// What a refusal says.
    pub words: &'static str,
}

/// The ownership table for this task's families.
pub(crate) const TABLE: &[Row] = &[
    Row {
        family: Family::Transform,
        attributes: &["svg:x", "svg:y", "svg:width", "svg:height", "draw:transform", "svg:cx", "svg:cy", "svg:r", "svg:rx", "svg:ry"],
        cells: &["PinX", "PinY", "LocPinX", "LocPinY", "Width", "Height", "Angle", "FlipX", "FlipY"],
        action: Action::Regenerate,
        words: "a change of a shape's place, size or turn",
    },
    Row {
        family: Family::Transform,
        attributes: &["svg:x1", "svg:y1", "svg:x2", "svg:y2", "draw:transform"],
        cells: &["BeginX", "BeginY", "EndX", "EndY"],
        action: Action::Regenerate,
        words: "a change of the ends of a line",
    },
    Row {
        family: Family::Route,
        attributes: &[
            "svg:d",
            "svg:viewBox",
            "draw:id",
            "draw:start-shape",
            "draw:start-glue-point",
            "draw:end-shape",
            "draw:end-glue-point",
            // Kept until DiaCraft routes the connector anew, and taken out with the route (Connectors).
            "draw:line-skew",
        ],
        cells: &["Geometry", "Connect"],
        action: Action::Regenerate,
        words: "a change of a connector's glue",
    },
    Row {
        family: Family::Layer,
        attributes: &["draw:layer", "draw:display", "draw:protected"],
        cells: &["LayerMember", "Layer"],
        action: Action::Regenerate,
        words: "a change of a shape's layer or of a layer's flags",
    },
    Row {
        family: Family::Order,
        attributes: &["draw:z-index"],
        cells: &[],
        action: Action::Place,
        words: "a change of a shape's place among the others",
    },
    Row {
        family: Family::Geometry,
        attributes: &["draw:enhanced-path", "draw:points", "svg:d", "draw:corner-radius", "draw:mirror-horizontal", "draw:mirror-vertical"],
        cells: &["Geometry", "Rounding", "FlipX", "FlipY"],
        action: Action::Refuse,
        words: "a change of a shape's geometry",
    },
    Row {
        family: Family::Restyle,
        attributes: &[
            "draw:style-name",
            "draw:fill",
            "draw:fill-color",
            "draw:fill-gradient-name",
            "draw:fill-hatch-name",
            "draw:fill-hatch-solid",
            "draw:fill-image-name",
            // The rest of a fill picture's attributes: taken out with the picture when the new kind of fill no longer uses it (`draw:fill`).
            "draw:fill-image-width",
            "draw:fill-image-height",
            "draw:fill-image-ref-point",
            "draw:fill-image-ref-point-x",
            "draw:fill-image-ref-point-y",
            "style:repeat",
            "draw:opacity",
            "draw:opacity-name",
            "draw:stroke",
            "svg:stroke-width",
            "svg:stroke-color",
            "svg:stroke-opacity",
            "svg:stroke-linecap",
            "draw:stroke-linejoin",
            "draw:marker-start",
            "draw:marker-end",
            "draw:marker-start-width",
            "draw:marker-end-width",
            "draw:marker-start-center",
            "draw:marker-end-center",
            // The shadow.
            "draw:shadow",
            "draw:shadow-color",
            "draw:shadow-opacity",
            "draw:shadow-offset-x",
            "draw:shadow-offset-y",
            "draw:textarea-vertical-align",
            "fo:padding",
            "fo:padding-top",
            "fo:padding-bottom",
            "fo:padding-left",
            "fo:padding-right",
            // Written by a retext of a custom shape whose style has none (the ownership row for it feeds from no cell).
            "draw:auto-grow-height",
            // A paragraph's or a run's style: written on a paragraph's tag and a span's, which no shape's tag is.
            "text:style-name",
            // The parent of a shape's automatic style: its three links.
            "style:parent-style-name",
        ],
        cells: &[
            "FillPattern",
            "FillForegnd",
            "FillForegndTrans",
            "LinePattern",
            "LineWeight",
            "LineColor",
            "LineColorTrans",
            "LineCap",
            "BeginArrow",
            "BeginArrowSize",
            "EndArrow",
            "EndArrowSize",
            "ShdwPattern",
            "ShapeShdwType",
            "ShdwForegnd",
            "ShdwForegndTrans",
            "ShapeShdwOffsetX",
            "ShapeShdwOffsetY",
            "VerticalAlign",
            "TopMargin",
            "BottomMargin",
            "LeftMargin",
            "RightMargin",
        ],
        action: Action::Regenerate,
        words: "a restyle of a shape",
    },
    Row {
        family: Family::Page,
        attributes: &[
            "fo:page-width",
            "fo:page-height",
            "fo:margin-left",
            "fo:margin-right",
            "fo:margin-top",
            "fo:margin-bottom",
            "style:print-orientation",
            "style:name",
            "style:display-name",
            "style:page-layout-name",
            "draw:master-page-name",
            // A page's own name (`draw:name`, shape and page): written for a page renamed, and for a page that is given the name it had.
            "draw:name",
        ],
        cells: &["PageWidth", "PageHeight", "PageLeftMargin", "PageRightMargin", "PageTopMargin", "PageBottomMargin", "PrintPageOrientation"],
        action: Action::Regenerate,
        words: "a change of a page's size, margins or orientation",
    },
    // The attributes of the style of a paragraph and of a run: written into the style the file's paragraph or span names (where it stands, or a copy), for the
    // cells of the row that changed (`runs::owned`), the other attributes staying as the file has them.
    Row {
        family: Family::TextStyle,
        attributes: &[
            "fo:font-size",
            "fo:font-weight",
            "fo:font-style",
            "fo:font-variant",
            "style:font-weight-complex",
            "style:font-style-complex",
            "style:text-underline-style",
            "style:text-underline-type",
            "style:text-line-through-style",
            "style:text-line-through-type",
            "style:text-overline-style",
            "style:text-underline-color",
            "style:text-underline-width",
            "style:text-line-through-color",
            "style:text-line-through-width",
            "style:text-overline-color",
            "style:text-overline-width",
            "fo:text-transform",
            "style:text-position",
            "fo:color",
            "fo:letter-spacing",
            "style:text-scale",
            "style:font-name",
            "fo:font-family",
            "style:font-name-asian",
            "style:font-family-asian",
            "style:font-name-complex",
            "style:font-family-complex",
            "fo:text-align",
            "fo:text-align-last",
            "style:writing-mode",
            "fo:margin-left",
            "fo:margin-right",
            "fo:text-indent",
            "fo:margin-top",
            "fo:margin-bottom",
            "fo:line-height",
            "style:line-spacing",
            "style:line-height-at-least",
        ],
        cells: &[
            "Size",
            "Style",
            "Case",
            "Pos",
            "Color",
            "Letterspace",
            "FontScale",
            "Font",
            "AsianFont",
            "ComplexScriptFont",
            "HorzAlign",
            "Flags",
            "IndLeft",
            "IndRight",
            "IndFirst",
            "SpBefore",
            "SpAfter",
            "SpLine",
        ],
        action: Action::Regenerate,
        words: "a restyle of a paragraph or a run",
    },
    Row {
        family: Family::Outline,
        attributes: &["draw:points", "draw:enhanced-path", "draw:modifiers"],
        cells: &["Geometry"],
        action: Action::Regenerate,
        words: "a change of a shape's outline",
    },
    Row { family: Family::Text, attributes: &[], cells: &["Text"], action: Action::Refuse, words: "a change of a shape's text" },
    Row {
        family: Family::Style,
        attributes: &[],
        cells: &["Style"],
        action: Action::Refuse,
        words: "a change of a shape's style or of another cell",
    },
];

/// The family an attribute the writer writes belongs to, or `None` for one the table does not own: the editor refuses to write it.
pub(crate) fn owns(attribute: &str) -> Option<Family> {
    TABLE.iter().find(|row| row.action != Action::Refuse && row.attributes.contains(&attribute)).map(|row| row.family)
}

/// The local name of an attribute the writer writes, as the table spells it (the name itself, and no copy): `None` for one it does not.
pub(crate) fn static_local(local: &str) -> Option<&'static str> {
    TABLE.iter().filter(|row| row.action != Action::Refuse).flat_map(|row| row.attributes.iter()).find_map(|a| {
        let (_, name) = a.split_once(':')?;
        (name == local).then_some(name)
    })
}

/// What a refusal of a family says.
pub(crate) fn refusal(family: Family) -> &'static str {
    TABLE.iter().find(|row| row.family == family && row.action == Action::Refuse).map_or("a change the writer does not write", |row| row.words)
}

/// The cells that make a shape's place on the page, which the save compares by value and writes into the element: the nine of a frame and the
/// four ends of a segment. A change to one is a change of the family *Transform*, and the rest of the shape is compared without them.
pub(crate) const POSE_CELLS: [&str; 13] =
    ["PinX", "PinY", "LocPinX", "LocPinY", "Width", "Height", "Angle", "FlipX", "FlipY", "BeginX", "BeginY", "EndX", "EndY"];

/// The number a cell holds, or 0 when the sheet has no such cell (a frame's `Angle` and flips are only there when they are not 0); `None` for a
/// cell whose value is not a number.
pub(crate) fn cell_number(sheet: &Sheet, name: &str) -> Option<f64> {
    match sheet.cell(name) {
        None => Some(0.0),
        Some(cell) => cell.value.as_ref().and_then(diacraft_doc::Value::number),
    }
}

/// The map that takes a point of the shape's own frame to its parent's, inches, y up: subtract the pin's place in the frame, flip,
/// turn counterclockwise, add the pin. `None` for a cell that is not a number.
pub(crate) fn local_to_parent(sheet: &Sheet) -> Option<Affine> {
    let n = |name: &str| cell_number(sheet, name);
    let (pin_x, pin_y, loc_x, loc_y, angle) = (n("PinX")?, n("PinY")?, n("LocPinX")?, n("LocPinY")?, n("Angle")?);
    let flip = |name: &str| n(name).map(|v| if v == 1.0 { -1.0 } else { 1.0 });
    let (fx, fy) = (flip("FlipX")?, flip("FlipY")?);
    let (s, c) = angle.sin_cos();
    // x' = c fx u - s fy v, y' = s fx u + c fy v, then the pin; with the pin's place in the frame taken off first.
    let (a, b, cc, d) = (c * fx, s * fx, -s * fy, c * fy);
    Some(Affine { a, b, c: cc, d, e: pin_x - (a * loc_x + cc * loc_y), f: pin_y - (b * loc_x + d * loc_y) })
}

/// `outer` after `inner`: the map that does `inner` first and then `outer`.
pub(crate) fn compose(outer: &Affine, inner: &Affine) -> Affine {
    inner.then(outer)
}

/// A frame's place on the page: its centre (inches, y up), the linear part of its map (a, b, c, d), and its size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pose {
    pub centre: (f64, f64),
    pub lin: [f64; 4],
    pub size: (f64, f64),
}

/// The pose of the 2-D shape whose own sheet is `sheet`, in a parent whose map to the page is `parent`. `None` for a cell that is not a number.
pub(crate) fn pose_2d(sheet: &Sheet, parent: &Affine) -> Option<Pose> {
    let map = compose(parent, &local_to_parent(sheet)?);
    let (w, h) = (cell_number(sheet, "Width")?, cell_number(sheet, "Height")?);
    Some(Pose { centre: map.apply(w / 2.0, h / 2.0), lin: [map.a, map.b, map.c, map.d], size: (w, h) })
}

/// The two ends of the 1-D shape whose own sheet is `sheet`, in a parent whose map to the page is `parent`: inches, y up.
pub(crate) fn ends(sheet: &Sheet, parent: &Affine) -> Option<[(f64, f64); 2]> {
    let n = |name: &str| cell_number(sheet, name);
    Some([parent.apply(n("BeginX")?, n("BeginY")?), parent.apply(n("EndX")?, n("EndY")?)])
}

fn same_point(a: (f64, f64), b: (f64, f64)) -> bool {
    same(a.0, b.0) && same(a.1, b.1)
}

/// What a change of a 2-D shape's pose is.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Frame {
    /// Nothing changed.
    Same,
    /// The frame moved by this much (inches, y up) and is the same size and turn.
    Translate { dx: f64, dy: f64 },
    /// Its size or turn changed: the frame as it is now.
    Reshape(Pose),
}

/// The change from the pose a shape was read with to the pose it has: the family *Transform* of a 2-D shape.
pub(crate) fn frame(old: &Pose, new: &Pose) -> Frame {
    let same_size = same(old.size.0, new.size.0) && same(old.size.1, new.size.1);
    let same_turn = old.lin.iter().zip(&new.lin).all(|(a, b)| (a - b).abs() <= 1e-9);
    match (same_size && same_turn, same_point(old.centre, new.centre)) {
        (true, true) => Frame::Same,
        (true, false) => Frame::Translate { dx: new.centre.0 - old.centre.0, dy: new.centre.1 - old.centre.1 },
        (false, _) => Frame::Reshape(*new),
    }
}

/// The turn a pose's linear part is, counterclockwise in radians, when it is a turn and no more: not flipped, not scaled or sheared.
pub(crate) fn turn_of(pose: &Pose) -> Option<f64> {
    let [a, b, c, d] = pose.lin;
    let rotation = (a - d).abs() <= 1e-9 && (b + c).abs() <= 1e-9 && ((a * a + b * b) - 1.0).abs() <= 1e-9;
    rotation.then(|| if b == 0.0 && a > 0.0 { 0.0 } else { b.atan2(a) })
}

/// The flips a sheet holds, as the factors they put on the frame's axes: `-1` for a flipped axis (`FlipX` or `FlipY` 1), else `1`. `None` for a cell that is
/// not a number.
pub(crate) fn flips_of(sheet: &Sheet) -> Option<(f64, f64)> {
    let flip = |name: &str| cell_number(sheet, name).map(|v| if v == 1.0 { -1.0 } else { 1.0 });
    Some((flip("FlipX")?, flip("FlipY")?))
}

/// `pose` with the flips `flips` undone: the linear part of a shape is its turn after its flips (`local_to_parent`), so the first column times the flip of x
/// and the second times the flip of y leave the turn.
pub(crate) fn unflipped(pose: &Pose, flips: (f64, f64)) -> Pose {
    let [a, b, c, d] = pose.lin;
    // The factors are exactly 1 or -1 ([`flips_of`]): a flipped axis turns the column over.
    let by = |v: f64, f: f64| if f == -1.0 { -v } else { v };
    Pose { lin: [by(a, flips.0), by(b, flips.0), by(c, flips.1), by(d, flips.1)], ..*pose }
}

/// Whether the two sets of ends are the same segment.
pub(crate) fn same_ends(old: &[(f64, f64); 2], new: &[(f64, f64); 2]) -> bool {
    same_point(old[0], new[0]) && same_point(old[1], new[1])
}

/// Whether two cells say the same thing to a save: the same formula, and where there is none the same value; what a calculation made of a
/// formula (its cached value, its error, and the unit the value shows in, which ODF has no place for) is not an edit.
fn cell_same(a: &diacraft_doc::Cell, b: &diacraft_doc::Cell) -> bool {
    use diacraft_doc::FormulaSlot;
    if a.formula != b.formula || a.raw != b.raw || a.extra != b.extra {
        return false;
    }
    match a.formula {
        FormulaSlot::Local(_) | FormulaSlot::Inherit => true,
        FormulaSlot::Absent | FormulaSlot::NoFormula => match (&a.value, &b.value) {
            (Some(x), Some(y)) => x == y || x.number().zip(y.number()).is_some_and(|(p, q)| p == q),
            (None, None) => true,
            _ => false,
        },
    }
}

/// Whether two maps of cells hold the same cells but for those named in `skip`.
fn cells_same(a: &diacraft_doc::Cells, b: &diacraft_doc::Cells, skip: &[&str], steps: &mut u64) -> bool {
    let kept = |cells: &'_ diacraft_doc::Cells| cells.iter().filter(|(name, _)| !skip.contains(&name.as_str())).count();
    if kept(a) != kept(b) {
        return false;
    }
    a.iter().filter(|(name, _)| !skip.contains(&name.as_str())).all(|(name, cell)| {
        *steps = steps.saturating_add(1);
        b.get(name).is_some_and(|other| cell_same(cell, other))
    })
}

/// Whether two sections are the same to a save: their names, places, flags and rows, row by row.
fn section_same(a: &diacraft_doc::Section, b: &diacraft_doc::Section, steps: &mut u64) -> bool {
    a.name == b.name
        && a.ix == b.ix
        && a.del == b.del
        && a.raw == b.raw
        && a.passthrough == b.passthrough
        && a.rows.len() == b.rows.len()
        && cells_same(&a.cells, &b.cells, &[], steps)
        && a.rows.iter().zip(b.rows.iter()).all(|(x, y)| {
            *steps = steps.saturating_add(1);
            x.name == y.name
                && x.ix == y.ix
                && x.local_name == y.local_name
                && x.row_type == y.row_type
                && x.del == y.del
                && x.raw == y.raw
                && x.passthrough == y.passthrough
                && cells_same(&x.cells, &y.cells, &[], steps)
        })
}

/// Which `Geometry` section of a connector a save reads as its route: section 0.
const ROUTE_SECTION: u32 = 0;

/// The cells of a 1-D shape's two ends: a connector's are rebuilt from its glue (a formula that names the shape it is glued to), and the route is written from
/// them.
pub(crate) const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// Whether the formula, the error, the attributes and the content of the pose cells of two sheets are the same, for the cells `of` names: the frame writes the
/// pose cells' **values** (a move, a resize, a turn), and what the frame cannot say, a formula set on a cell whose value stays the same (`shape.pin` sets `LocPinX`
/// to a formula), is carried by DiaCraft's extension. A cell one sheet has and the other lacks is the frame's.
pub(crate) fn pose_extras_same<'a>(a: &Sheet, b: &Sheet, of: impl IntoIterator<Item = &'a &'static str>) -> bool {
    of.into_iter().all(|name| match (a.cells.get(*name), b.cells.get(*name)) {
        (Some(x), Some(y)) => x.formula.text() == y.formula.text() && x.error == y.error && x.raw == y.raw && x.extra == y.extra,
        _ => true,
    })
}

/// The cells that say where the pin stands in the frame: the markup's frame says where the shape is, and the reader puts the pin at its centre (`Width*0.5`), so a place
/// that is another constant is the extension's.
const PIN_PLACES: [&str; 2] = ["LocPinX", "LocPinY"];

/// Whether the place of the pin in the frame was edited as a constant: a cell of [`PIN_PLACES`] that has no formula in either sheet and holds another number. The frame
/// has nothing to say of it when the shape stands where it stood (the pin compensating), and the value of a pose cell is the frame's to write: it was no edit at all
/// A place with a formula that changes its value is a calculation the frame follows (a resize), and one whose formula changed is carried
/// by [`pose_extras_same`].
fn pin_place_edited(a: &Sheet, b: &Sheet, name: &str) -> bool {
    let number =
        |s: &Sheet| s.cells.get(name).filter(|c| matches!(c.formula, diacraft_doc::FormulaSlot::Absent)).and_then(|c| c.value.as_ref()?.number());
    matches!((number(a), number(b)), (Some(x), Some(y)) if !same(x, y))
}

/// Whether two sheets are the same to a save but for the values of the pose cells and, when `route` says so, a connector's route (`Geometry` section 0):
/// the links, the cells but [`POSE_CELLS`], the formula and the rest of each pose cell but its value (a connector's end cells are the glue's: the save knows them from
/// the connections), and every section, compared in order. `steps` counts what is compared.
pub(crate) fn sheets_same(a: &Sheet, b: &Sheet, route: bool, steps: &mut u64) -> bool {
    if *a == *b {
        return true;
    }
    let in_route = |s: &diacraft_doc::Section| route && s.name.as_str() == "Geometry" && s.ix == Some(ROUTE_SECTION);
    let (ka, kb): (Vec<_>, Vec<_>) = (a.sections.iter().filter(|s| !in_route(s)).collect(), b.sections.iter().filter(|s| !in_route(s)).collect());
    a.links == b.links
        && a.raw == b.raw
        && a.passthrough == b.passthrough
        && ka.len() == kb.len()
        && cells_same(&a.cells, &b.cells, &POSE_CELLS, steps)
        && pose_extras_same(a, b, POSE_CELLS.iter().filter(|name| !(route && END_CELLS.contains(name))))
        && !PIN_PLACES.iter().any(|name| pin_place_edited(a, b, name))
        && ka.iter().zip(&kb).all(|(x, y)| section_same(x, y, steps))
}

/// Whether two sheets are the same to a save, every cell compared as [`cell_same`] compares a shape's: the formula, and where there is none the
/// value, so that what a recalculation made of a formula (its cached value) is no edit (the page's and the document's sheets
/// were compared with a strict `==` that a recalculation alone made unequal, and every save was then refused). `steps` counts what is compared. A save of a
/// package compares what the document file carries by [`sheet_delta`](diacraft_sheetxml::sheet_delta); a flat drawing has no such file, and its save
/// compares the document's and the pages' sheets by this.
pub(crate) fn sheet_same(a: &Sheet, b: &Sheet, steps: &mut u64) -> bool {
    if *a == *b {
        return true;
    }
    a.links == b.links
        && a.raw == b.raw
        && a.passthrough == b.passthrough
        && a.sections.len() == b.sections.len()
        && cells_same(&a.cells, &b.cells, &[], steps)
        && a.sections.iter().zip(&b.sections).all(|(x, y)| section_same(x, y, steps))
}

/// [`sheet_same`], but for the cells named in `skip`, which are compared by the caller.
pub(crate) fn sheet_same_but(a: &Sheet, b: &Sheet, skip: &[&str], steps: &mut u64) -> bool {
    a.links == b.links
        && a.raw == b.raw
        && a.passthrough == b.passthrough
        && a.sections.len() == b.sections.len()
        && cells_same(&a.cells, &b.cells, skip, steps)
        && a.sections.iter().zip(&b.sections).all(|(x, y)| section_same(x, y, steps))
}

/// Whether one cell is the same in two sheets to a save: absent from both, or present in both and [`cell_same`].
pub(crate) fn one_cell_same(a: &Sheet, b: &Sheet, name: &str) -> bool {
    match (a.cells.get(name), b.cells.get(name)) {
        (None, None) => true,
        (Some(x), Some(y)) => cell_same(x, y),
        _ => false,
    }
}

/// Whether the route sections (`Geometry` section 0) of two sheets are the same to a save.
pub(crate) fn route_same(a: &Sheet, b: &Sheet, steps: &mut u64) -> bool {
    let find = |s: &Sheet| s.sections.iter().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(ROUTE_SECTION)).cloned();
    match (find(a), find(b)) {
        (Some(x), Some(y)) => section_same(&x, &y, steps),
        (None, None) => true,
        _ => false,
    }
}

/// Whether two shapes' foreign data are the same: the same attributes and text, and the same picture **by what it is** (its bytes), not by the name of the
/// place it was read from nor by the media type it was declared under there (a package's manifest says one, a flat drawing's inline picture is told by its
/// bytes). The same picture read from a package part (`Pictures/a.png`) and from a flat drawing's `office:binary-data` is one picture:
/// a drawing saved in the other form is not changed by it.
fn foreign_same(a: Option<&diacraft_doc::ForeignData>, b: Option<&diacraft_doc::ForeignData>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.attrs == b.attrs
                && a.rel_id == b.rel_id
                && a.text == b.text
                && a.passthrough == b.passthrough
                && match (&a.picture, &b.picture) {
                    (None, None) => true,
                    (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y) || x.bytes == y.bytes,
                    _ => false,
                }
        }
        _ => false,
    }
}

/// Whether two lists of kept children hold the same nodes in the same order, whatever the number of modelled siblings each is placed after.
///
/// A kept node is placed `after` the shapes before it in its parent, and a reorder or a delete of those shapes (the writer's own earlier save, or
/// the caller's edit) changes that number in the file the next read gives while the node stays where it is in the file. The number says where to
/// put the node back when a writer writes the typed children and the kept ones together; this writer never does, and a kept node's bytes stay in
/// the file's own place, so a different number is no change the save has to make. The nodes themselves are compared whole.
pub(crate) fn passthrough_same(a: &Passthrough, b: &Passthrough) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.node == y.node)
}

/// Whether two kept sets (`Kept`: attributes and children no field reads) are the same to a save: the attributes whole, the children by [`passthrough_same`].
fn kept_same(a: &diacraft_doc::Kept, b: &diacraft_doc::Kept) -> bool {
    a.raw == b.raw && passthrough_same(&a.kept, &b.kept)
}

/// Whether two shapes keep the same `.odg` markup: the attributes and the children no field reads, which the reader put in the `odf` slot of the shape and the writer leaves as the
/// file has them. **Only that part of the slot is compared**: the held values, the style names and the units beside it say nothing the save writes (a model writer reads them), so a
/// slot made or changed for them is no edit. A shape with no slot keeps nothing.
pub(crate) fn shape_kept_same(a: &Shape, b: &Shape) -> bool {
    match (a.ext.get(), b.ext.get()) {
        (None, None) => true,
        (Some(x), Some(y)) => kept_same(&x.odf.kept, &y.odf.kept),
        (Some(only), None) | (None, Some(only)) => only.odf.kept.raw.is_empty() && only.odf.kept.kept.is_empty(),
    }
}

/// [`shape_kept_same`] for pages.
pub(crate) fn page_kept_same(a: &diacraft_doc::Page, b: &diacraft_doc::Page) -> bool {
    match (a.ext.get(), b.ext.get()) {
        (None, None) => true,
        (Some(x), Some(y)) => kept_same(&x.odf.kept, &y.odf.kept),
        (Some(only), None) | (None, Some(only)) => only.odf.kept.raw.is_empty() && only.odf.kept.kept.is_empty(),
    }
}

/// What the cells of a shape's sheet say besides its pose and, for a connector, its route.
#[derive(Debug)]
// A value made for one shape at a time and given up the same turn, never kept in a collection: a box would be an allocation for each change.
#[allow(clippy::large_enum_variant)]
pub(crate) enum SheetChange {
    /// Nothing the save writes.
    Same,
    /// A restyle, an edit of the outline, or both, and what is carried by DiaCraft's extension (tier 2).
    Edit { style: Option<super::stylemap::StyleEdit>, geo: Option<GeoEdit>, delta: Option<Tier2Delta> },
    /// A change the writer does not write, in words.
    Refuse(&'static str),
}

/// What a shape's sheet changed that the file's own markup cannot say, so that DiaCraft's extension carries it: cells that are new or differ, and the
/// names of those the model lost; sections the same way; and whether the shape's names or its links to a master changed.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tier2Delta {
    pub cells: diacraft_doc::Cells,
    pub removed_cells: Vec<String>,
    pub sections: Vec<std::sync::Arc<diacraft_doc::Section>>,
    pub removed_sections: Vec<diacraft_doc::SectionKey>,
    /// `Name`, `NameU`, `Master` or `MasterShape` differ from the baseline's.
    pub fields: bool,
}

impl Tier2Delta {
    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.removed_cells.is_empty() && self.sections.is_empty() && self.removed_sections.is_empty() && !self.fields
    }
}

/// What a save makes of a shape's tier 2: the shape as the model has it, the `ID` it has, and what changed that only the extension can say. The shape's
/// own style may carry tier 2 already, and a pose, a text or a name that changed is written into the attributes that hold it too.
#[derive(Clone, Debug)]
pub(crate) struct Tier2Edit {
    pub id: u32,
    /// The shape was read as a copy of another (its style carries an `ID` an earlier shape carries): its style's attributes are that shape's, the copy holds
    /// none of DiaCraft's data, and an edit of it gives it a tier 2 of its own with its own `ID`.
    pub copy: bool,
    pub model: std::sync::Arc<Shape>,
    pub delta: Tier2Delta,
    /// The tier-2 items of the shape that tier 3 found stale when the file was read (their native outputs changed in another program): the reader left them out, and
    /// an edit that writes the extension again must not write them as current (`PageShapes::stale`).
    pub stale: Vec<crate::ext::fingerprint::Key>,
    /// What the save writes of the shape's outline (a path, its points or its modifiers): the native outputs the fingerprints of the outline stand for change with it.
    pub outline: OutlineWrite,
    /// The native outputs of the file's markup that the shape's tier-2 items rest on, as the file had them when it was read (`PageShapes::natives`): what a missing
    /// fingerprint is stamped from.
    pub natives: Option<crate::ext::tier3::Natives>,
}

/// What a save writes of a shape's outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutlineWrite {
    /// Nothing of it.
    No,
    /// The path of a custom shape (`draw:enhanced-path`) and no modifier: the output is the path text the writer makes from the model's sheet.
    Path,
    /// Anything else of it: a polygon's points, a path's `svg:d`, the modifiers.
    Other,
}

impl OutlineWrite {
    /// What the outline edit `geo` writes.
    pub(crate) fn of(geo: Option<&GeoEdit>) -> OutlineWrite {
        match geo {
            None => OutlineWrite::No,
            Some(g) if g.outline && g.modifiers.is_empty() => OutlineWrite::Path,
            Some(_) => OutlineWrite::Other,
        }
    }
}

/// Whether two `User` sections are the same but for the `Value` of the rows named `ODFMod<n>`, whose formula (when they have one) is the same.
fn user_same_but_values(a: &diacraft_doc::Section, b: &diacraft_doc::Section, steps: &mut u64) -> bool {
    a.name == b.name
        && a.ix == b.ix
        && a.del == b.del
        && a.raw == b.raw
        && a.passthrough == b.passthrough
        && cells_same(&a.cells, &b.cells, &[], steps)
        && a.rows.len() == b.rows.len()
        && a.rows.iter().zip(b.rows.iter()).all(|(x, y)| {
            *steps = steps.saturating_add(1);
            let modifier = x.name.as_deref().is_some_and(|n| n.starts_with("ODFMod"));
            x.name == y.name
                && x.row_type == y.row_type
                && x.del == y.del
                && if modifier {
                    cells_same(&x.cells, &y.cells, &["Value"], steps)
                        && x.cells.get("Value").map(|c| &c.formula) == y.cells.get("Value").map(|c| &c.formula)
                } else {
                    cells_same(&x.cells, &y.cells, &[], steps)
                }
        })
}

/// What an edit decides about the display forms before the cells are written: whether the shadow is visible in the file, and the gradient the edit makes, changes or takes off.
fn fx_before(old: &Sheet, new: &Sheet, edit: &mut super::stylemap::StyleEdit, delta: &mut Tier2Delta) {
    // The shadow is visible in the file when this edit writes the cell that says so as 1 (`stylemap`: the reader gives `ShapeShdwType` 1 only with a visible shadow).
    edit.shadow_visible = Some(edit.changes.iter().any(|(c, v)| *c == "ShdwPattern" && v.as_ref().and_then(|v| v.number) == Some(1.0)));
    // **A gradient of DiaCraft's own the edit makes or changes** is written as a display form: its attributes name a definition made from its stops and its layout, and
    // `FillGradientEnabled` is the attribute's own. The layout, the angle and the stops ride in DiaCraft's data, whole, whether or not they changed in this edit (the reader makes a stand-in
    // of the gradient, which the carried cells take the place of). A hatch of an existing shape is not written as a display form: its marks' colour is the attribute the ground
    // would take, which the file's own fingerprint of the cell would call stale.
    if let Some(fill @ super::fx::Fill::Gradient(_)) = super::fx::fill_of(new)
        && super::fx::fill_of(old) != Some(fill)
    {
        edit.fx = Some(fill);
    } else if matches!(super::fx::fill_of(old), Some(super::fx::Fill::Gradient(_)))
        && !matches!(super::fx::fill_of(new), Some(super::fx::Fill::Gradient(_)))
    {
        // The shape had a gradient written as a display form and has none now: the style says the plain fill the model has (a pattern of 0 is none, any other solid, which is the
        // colour of the marks for a pattern the file cannot say).
        edit.fx_off = Some(if constant_zero(new, "FillPattern") { "none" } else { "solid" });
        // The gradient switch the display form said (and the reader gave back as 1) is the model's alone now, if the model still has a gradient that the markup cannot say.
        if let Some(switch) = new.cells.get("FillGradientEnabled")
            && !constant_zero(new, "FillGradientEnabled")
        {
            carry(delta, &diacraft_doc::CellName::new("FillGradientEnabled"), Some(switch));
        }
    }
}

/// What follows from the cells the edit wrote, where the markup says another thing than the model's unchanged cell: the ground's transparency, which one attribute says
/// with the marks', and the type of a shadow, which follows its visibility.
fn fx_follow(new: &Sheet, edit: &super::stylemap::StyleEdit, delta: &mut Tier2Delta) {
    // **The ground's transparency follows the marks'** (one `draw:opacity` says both, and the reader gives both the share): an edit that writes the marks' share says it for the ground too, so a
    // ground whose share is another is carried, and one the model has none of is taken out in DiaCraft's data. (The random looks of `fx_native.rs` found a ground left as the file said it.)
    if let Some((_, written)) = edit.changes.iter().find(|(c, _)| *c == "FillForegndTrans") {
        let share = |c: &diacraft_doc::Cell| c.value.as_ref().and_then(diacraft_doc::Value::number);
        let marks = written.as_ref().and_then(|v| v.number);
        match new.cells.get("FillBkgndTrans") {
            Some(ground) if marks.is_none() || share(ground).zip(marks).is_none_or(|(g, m)| (g - m).abs() > 1e-9) => {
                carry(delta, &diacraft_doc::CellName::new("FillBkgndTrans"), Some(ground));
            }
            None if marks.is_some() => carry(delta, &diacraft_doc::CellName::new("FillBkgndTrans"), None),
            _ => {}
        }
    }
    // **The type of a shadow follows its visibility**: the reader gives `ShapeShdwType` 1 beside a visible shadow and none beside a hidden one, so an edit that changes the cell that
    // says the shadow is visible changes which of the two the markup says. Where the type is no longer what the markup gives it is carried, and where the markup now gives one the model lacks
    // the type is taken out in DiaCraft's data.
    if edit.changes.iter().any(|(c, _)| *c == "ShdwPattern") {
        let visible = edit.shadow_visible == Some(true);
        match (visible, new.cells.get("ShapeShdwType")) {
            (true, None) => carry(delta, &diacraft_doc::CellName::new("ShapeShdwType"), None),
            (false, Some(cell)) => carry(delta, &diacraft_doc::CellName::new("ShapeShdwType"), Some(cell)),
            _ => {}
        }
    }
}

/// What the stand-in the reader makes of a gradient written as a display form (`fill`) is put over, in DiaCraft's data: the cells of the layout and the angle, taken out when the model has none,
/// and the stops. (The stand-in makes the layout, and the angle of a linear gradient: those are taken out when the model has none; `RotateGradientWithShape` is carried when the
/// model has it and is no removal otherwise.)
fn fx_over_stand_in(new: &Sheet, fill: super::fx::Fill, delta: &mut Tier2Delta) {
    let linear = matches!(fill, super::fx::Fill::Gradient(key) if key.dir == 0);
    for name in ["FillGradientDir", "FillGradientAngle", "RotateGradientWithShape"] {
        if name == "FillGradientDir" || (name == "FillGradientAngle" && linear) || new.cells.contains_key(name) {
            carry(delta, &diacraft_doc::CellName::new(name), new.cells.get(name));
        }
    }
    if let Some(stops) = new.sections.iter().find(|s| s.name.as_str() == "FillGradient" && s.ix.is_none())
        && !delta.sections.iter().any(|s| s.name.as_str() == "FillGradient" && s.ix.is_none())
    {
        delta.sections.push(std::sync::Arc::clone(stops));
    }
}

/// What differs between two sheets of one shape, but for the pose cells and, when `route` says so, a connector's route: the style cells that
/// changed ([`super::stylemap`]), or why the change is not one the writer writes. Sections are compared whole, row by row, and any that
/// differs is refused here; cells directly under the sheet are told apart one by one.
pub(crate) fn sheet_change(old: &Sheet, new_sheet: &std::sync::Arc<Sheet>, route: bool, steps: &mut u64) -> SheetChange {
    use super::stylemap::{CellVal, MARGINS, StyleEdit, ops_for, static_cell};
    let new: &Sheet = new_sheet;
    // A handle's drag changes the cached value of a modifier's formula and not its formula: found by the number, before the sheets are compared.
    let modifiers = modifier_changes(old, new, steps);
    if sheets_same(old, new, route, steps) && modifiers.is_empty() {
        return SheetChange::Same;
    }
    let in_route = |s: &diacraft_doc::Section| route && s.name.as_str() == "Geometry" && s.ix == Some(ROUTE_SECTION);
    let (ka, kb): (Vec<_>, Vec<_>) = (old.sections.iter().filter(|s| !in_route(s)).collect(), new.sections.iter().filter(|s| !in_route(s)).collect());
    // The three links name one style sheet or they do not: a shape whose links were one style and are another is written as another parent of its automatic style
    // Any other change of the links is refused.
    let mut parent: Option<u32> = None;
    if old.links != new.links {
        let one = |l: &diacraft_doc::Links| {
            let (line, fill, text) = (l.line.unwrap_or(0), l.fill.unwrap_or(0), l.text.unwrap_or(0));
            (line == fill && fill == text).then_some(line)
        };
        match (one(&old.links), one(&new.links)) {
            (Some(was), Some(now)) => parent = (was != now).then_some(now),
            _ => return SheetChange::Refuse(WORDS_LINKS),
        }
    }
    if old.raw != new.raw || old.passthrough != new.passthrough {
        return SheetChange::Refuse("a change of an attribute or an element the model keeps without a cell");
    }
    // The sections: the outline's (`Geometry`) is told apart, a handle's (`Control`) follows the modifiers, the modifiers' (`User`) are compared but
    // for their values, and every other section is the same or the change is refused.
    type Sections = Vec<std::sync::Arc<diacraft_doc::Section>>;
    let of = |v: &[&std::sync::Arc<diacraft_doc::Section>], names: &[&str], keep: bool| -> Sections {
        v.iter().copied().filter(|s| names.contains(&s.name.as_str()) == keep).cloned().collect()
    };
    let same_all = |a: &Sections, b: &Sections, steps: &mut u64| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| section_same(x, y, steps));
    let special = SPECIAL_SECTIONS;
    // What the file's markup does not say is carried by the extension: a changed section of another name is carried whole, and so is a `User` section that
    // differs in more than a modifier's value, and a `Control` section that no modifier follows.
    let mut delta = Tier2Delta::default();
    let (oa, ob) = (of(&ka, &special, false), of(&kb, &special, false));
    if !same_all(&oa, &ob, steps) {
        section_delta_into(&oa, &ob, &mut delta, steps);
    }
    let (ua, ub) = (of(&ka, &["User"], true), of(&kb, &["User"], true));
    if !(ua.len() == ub.len() && ua.iter().zip(&ub).all(|(x, y)| user_same_but_values(x, y, steps))) {
        section_delta_into(&ua, &ub, &mut delta, steps);
    }
    let (ca, cb) = (of(&ka, &[crate::sections::CONTROL], true), of(&kb, &[crate::sections::CONTROL], true));
    if !same_all(&ca, &cb, steps) && modifiers.is_empty() {
        section_delta_into(&ca, &cb, &mut delta, steps);
    }
    let (ga, gb) = (of(&ka, &["Geometry"], true), of(&kb, &["Geometry"], true));
    let outline = !same_all(&ga, &gb, steps);
    let mut edit = StyleEdit::default();
    let names: std::collections::BTreeSet<&diacraft_doc::CellName> = old.cells.keys().chain(new.cells.keys()).collect();
    for name in names {
        if POSE_CELLS.contains(&name.as_str()) {
            // A change of a pose cell's value is the frame's, written natively; a change of its formula, error, attributes or content is carried.
            match (old.cells.get(name), new.cells.get(name)) {
                (Some(a), Some(b)) if a.formula.text() != b.formula.text() || a.error != b.error || a.raw != b.raw || a.extra != b.extra => {
                    delta.cells.insert(name.clone(), b.clone());
                }
                // The place of the pin edited as a constant: the markup cannot say it, and it is carried (a move or a resize that leaves the place is the frame's).
                (Some(_), Some(b)) if PIN_PLACES.contains(&name.as_str()) && pin_place_edited(old, new, name.as_str()) => {
                    delta.cells.insert(name.clone(), b.clone());
                }
                (None, Some(b)) => {
                    delta.cells.insert(name.clone(), b.clone());
                }
                (Some(_), None) => delta.removed_cells.push(name.as_str().to_owned()),
                _ => {}
            }
            continue;
        }
        let (was, now) = (old.cells.get(name), new.cells.get(name));
        if let (Some(a), Some(b)) = (was, now)
            && cell_same(a, b)
        {
            continue;
        }
        // A gradient that is not enabled writes no attribute, and a cell that is 0 is the same as one that is not there (a fill turned to none reads
        // back with no such cell, where the model made of a solid fill had it at 0).
        let off = |c: Option<&diacraft_doc::Cell>| c.is_none_or(|c| c.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0));
        if name.as_str() == "FillGradientEnabled" && off(was) && off(now) {
            continue;
        }
        *steps = steps.saturating_add(1);
        let Some(cell) = static_cell(name.as_str()) else {
            // A cell no attribute of the file stands for is carried by the extension.
            carry(&mut delta, name, now);
            continue;
        };
        // The custom end (254) is drawn as number 4's marker and its cell is carried: the reader rebuilds 4 from the marker.
        if let Some(c) = now.filter(|c| super::stylemap::is_custom_end(cell, c)) {
            carry(&mut delta, name, Some(c));
            edit.changes.push((cell, Some(CellVal::stand_in())));
            continue;
        }
        match now.map(CellVal::of).transpose() {
            Ok(value) => edit.changes.push((cell, value)),
            // A style cell that holds a formula is carried; the attribute the file has is its last value written.
            Err(_) => carry(&mut delta, name, now),
        }
    }
    if edit.changes.iter().any(|(c, _)| MARGINS.iter().any(|(m, _)| m == c)) {
        for (cell, _) in MARGINS {
            let Some(c) = static_cell(cell) else { continue };
            // A margin that holds a formula is not written with the other three: it is carried.
            match new.cells.get(cell).map(CellVal::of).transpose() {
                Ok(v) => edit.margins.push((c, v)),
                Err(_) => carry(&mut delta, &diacraft_doc::CellName::new(cell), new.cells.get(cell)),
            }
        }
    }
    fx_before(old, new, &mut edit, &mut delta);
    // A change the attributes cannot say is carried instead, and the style cells that go with it are not written either, as a group.
    loop {
        let failed = edit.changes.iter().find(|(cell, value)| ops_for(cell, value.as_ref(), &edit).is_err()).map(|(cell, _)| *cell);
        let Some(bad) = failed else { break };
        let group: &[&str] = if MARGINS.iter().any(|(m, _)| *m == bad) {
            &["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"]
        } else if super::stylemap::ARROW_CELLS.contains(&bad) {
            &super::stylemap::ARROW_CELLS
        } else {
            &[]
        };
        let gone: Vec<&'static str> = edit.changes.iter().map(|(c, _)| *c).filter(|c| *c == bad || group.contains(c)).collect();
        edit.changes.retain(|(c, _)| !gone.contains(c));
        if group.contains(&"TopMargin") {
            edit.margins.clear();
        }
        for name in gone {
            carry(&mut delta, &diacraft_doc::CellName::new(name), new.cells.get(name));
        }
    }
    fx_follow(new, &edit, &mut delta);
    if let Some(fill) = edit.fx {
        fx_over_stand_in(new, fill, &mut delta);
    }
    let geo = (outline || !modifiers.is_empty()).then(|| GeoEdit { sheet: std::sync::Arc::clone(new_sheet), outline, modifiers });
    if !delta.sections.is_empty() {
        // Two sections of one name cannot be told apart in the file.
        let mut keys = std::collections::HashSet::new();
        if !new.sections.iter().all(|s| keys.insert((s.name.as_str(), s.ix))) {
            return SheetChange::Refuse("a sheet that holds two sections of one name (the file carries a section by its name)");
        }
    }
    edit.parent = parent;
    if edit.is_empty() && geo.is_none() && delta.is_empty() {
        SheetChange::Same
    } else {
        SheetChange::Edit { style: (!edit.is_empty()).then_some(edit), geo, delta: (!delta.is_empty()).then_some(delta) }
    }
}

/// Whether the cell `name` of the sheet holds the constant 0.
fn constant_zero(sheet: &Sheet, name: &str) -> bool {
    sheet.cell(name).is_some_and(|c| {
        matches!(c.formula, diacraft_doc::FormulaSlot::Absent) && c.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0)
    })
}

/// A cell that only the extension carries: its value in the model, or its name as gone.
fn carry(delta: &mut Tier2Delta, name: &diacraft_doc::CellName, now: Option<&diacraft_doc::Cell>) {
    match now {
        Some(cell) => {
            delta.cells.insert(name.clone(), cell.clone());
        }
        None => delta.removed_cells.push(name.as_str().to_owned()),
    }
}

/// The sections of `a` (the baseline's) and `b` (the model's) that differ, by their keys: those of `b` that are new or differ are carried, those of `a` that `b`
/// lacks are gone.
///
/// The work is linear in the sections of the two (the security review): those of `a` are indexed by their keys once, those of `b` are looked up in the
/// index, and the keys of `b` are kept in a set that the removals are looked up in, so that no section is searched for among the others; and every turn of
/// every loop is charged to `steps`, the save's own count.
fn section_delta_into(
    a: &[std::sync::Arc<diacraft_doc::Section>],
    b: &[std::sync::Arc<diacraft_doc::Section>],
    delta: &mut Tier2Delta,
    steps: &mut u64,
) {
    use std::collections::{BTreeMap, BTreeSet};
    type Key<'a> = (&'a str, Option<u32>);
    // The first of a key is the one a search for the key finds.
    let mut held: BTreeMap<Key<'_>, &std::sync::Arc<diacraft_doc::Section>> = BTreeMap::new();
    for s in a {
        *steps = steps.saturating_add(1);
        held.entry((s.name.as_str(), s.ix)).or_insert(s);
    }
    let mut kept: BTreeSet<Key<'_>> = BTreeSet::new();
    for s in b {
        *steps = steps.saturating_add(1);
        kept.insert((s.name.as_str(), s.ix));
        match held.get(&(s.name.as_str(), s.ix)) {
            Some(o) if std::sync::Arc::ptr_eq(o, s) || section_same(o, s, steps) => {}
            _ => delta.sections.push(std::sync::Arc::clone(s)),
        }
    }
    for s in a {
        *steps = steps.saturating_add(1);
        if !kept.contains(&(s.name.as_str(), s.ix)) {
            delta.removed_sections.push(diacraft_doc::SectionKey { name: s.name.clone(), ix: s.ix });
        }
    }
}

/// The words of the refusal of a change of a shape's style links that is not from one style sheet to another (the form of a parent).
pub(crate) const WORDS_LINKS: &str =
    "a change of a shape's style links that leaves them naming different style sheets (the file says a parent, which is one style sheet)";

/// The words of a refusal of a text set on a picture or an object frame.
pub(crate) const WORDS_FRAME_TEXT: &str = "a text on a picture or an object frame (the file's place for the text of a graphic frame is not one this save writes, and the reader reads none from it)";

/// Sets [`StyleEdit::fill_kind`] for an edit that sets a fill colour or turns a gradient off and says nothing of `FillPattern`: `solid` when the model's fill
/// pattern is 1, so that a style that states no fill mode, or a gradient, a hatch or a bitmap, is drawn with the colour set. `new` is the shape's sheet and
/// `resolved` is asked for the value of a fill cell (`FillPattern`, `FillGradientEnabled`) that the sheet holds none of, and gives what the shape's styles give
/// it, so a pattern and a gradient the shape inherits are found as its own are. Nothing is stated when the model still holds a gradient (the colour is then not
/// the fill), and for any other pattern: 0 is a fill the file already states (`none`, and a bitmap or a hatch that is not solid, which the reader reads as 0), and
/// it is the file's to keep as it is, since the edit does not change the pattern.
pub(crate) fn decide_fill(edit: &mut super::stylemap::StyleEdit, new: &Sheet, mut resolved: impl FnMut(&str) -> Option<f64>) {
    let set = |name: &str| edit.changes.iter().any(|(c, v)| *c == name && v.is_some());
    let gradient_off = edit.changes.iter().any(|(c, v)| *c == "FillGradientEnabled" && v.as_ref().is_some_and(|v| v.number == Some(0.0)));
    if edit.changes.iter().any(|(c, _)| *c == "FillPattern") || !(set("FillForegnd") || gradient_off) {
        return;
    }
    let mut held = |name: &str| new.cell(name).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number).or_else(|| resolved(name));
    if held("FillGradientEnabled").is_some_and(|n| n != 0.0) {
        return;
    }
    edit.fill_kind = (held("FillPattern") == Some(1.0)).then_some("solid");
}

/// Sets [`StyleEdit::line`] for an edit that changes a line-end cell: the numbers of the five cells a line end's attributes are worked out from, each the new
/// sheet's own or, where it holds none, what `resolved` says its line style gives it (`None`: nothing does, and the number is left out).
pub(crate) fn decide_line_end(edit: &mut super::stylemap::StyleEdit, new: &Sheet, mut resolved: impl FnMut(&str) -> Option<f64>) {
    use super::stylemap::{ARROW_CELLS, LINE_END_CELLS};
    // A line end's cells, or the line's weight, which the width of a marker of DiaCraft's own is worked out from (the row).
    if !edit.changes.iter().any(|(c, _)| ARROW_CELLS.contains(c) || *c == "LineWeight") {
        return;
    }
    edit.line.clear();
    edit.diamonds.clear();
    for name in LINE_END_CELLS {
        let own = new.cell(name).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number);
        if let Some(n) = own.or_else(|| resolved(name)) {
            edit.line.push((name, n));
        }
        // DiaCraft's filled diamond is a custom end (254) that names the master of that name: it has a marker of its own.
        if new.cell(name).is_some_and(|c| crate::ext::fingerprint::is_filled_diamond(name, c)) {
            edit.diamonds.push(name);
        }
    }
}

/// Whether the `Character` and `Paragraph` sections of two sheets are the same to a save: the same sections, row by row (the retext compares them by what the
/// text uses of them).
fn text_rows_same(a: &Sheet, b: &Sheet, steps: &mut u64) -> bool {
    let pick = |s: &Sheet| -> Vec<std::sync::Arc<diacraft_doc::Section>> {
        s.sections.iter().filter(|x| matches!(x.name.as_str(), "Character" | "Paragraph")).cloned().collect()
    };
    let (x, y) = (pick(a), pick(b));
    x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| std::sync::Arc::ptr_eq(p, q) || section_same(p, q, steps))
}

/// What a shape's change is, besides its pose and, for a connector, its route.
#[derive(Debug)]
// As `SheetChange`: made for one shape at a time and given up the same turn.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ShapeChange {
    Same,
    /// A restyle, a retext, or both.
    Edit {
        style: Option<super::stylemap::StyleEdit>,
        text: Option<super::text::TextEdit>,
        geo: Option<GeoEdit>,
        delta: Option<Tier2Delta>,
    },
    /// A change the writer does not write, in words.
    Refuse(&'static str),
    /// A text whose paragraphs, in the lists a retext makes of them, would pass the memory a save may hold.
    Memory,
}

/// What differs between two shapes besides their pose and route (see [`sheet_change`]): a restyle, a retext, nothing, or a refusal in words.
pub(crate) fn shape_change(
    old: &Shape,
    new: &Shape,
    route: bool,
    palette: &dyn Fn(i64) -> Option<String>,
    steps: &mut u64,
    room: &mut u64,
) -> ShapeChange {
    *steps = steps.saturating_add(1);
    // A text is compared when its characters or markers differ, or a row of the runs or paragraphs does (the rows are compared by their cells there).
    let rows_differ = !text_rows_same(&old.sheet, &new.sheet, steps);
    let text = if crate::ext::text_same(old.text.as_ref(), new.text.as_ref()) && !rows_differ {
        None
    } else if old.kind == ShapeType::Foreign || new.kind == ShapeType::Foreign {
        // A picture or an object frame: the reader reads no text from it (its picture is the frame's), and no spec says where in the file the text of a
        // graphic frame goes, so a text set on one is refused, in words, and not written where it would read back as none (class F3).
        return ShapeChange::Refuse(WORDS_FRAME_TEXT);
    } else {
        match super::text::text_edit(old, new, palette, steps, room) {
            Ok(edit) => edit,
            Err(words) if words == super::text::MEMORY_WORDS => return ShapeChange::Memory,
            Err(words) => return ShapeChange::Refuse(words),
        }
    };
    // The names and the links to a master are carried by the extension; everything else the shape holds is the writer's to write or to refuse.
    let fields = old.name != new.name || old.name_u != new.name_u || old.master != new.master || old.master_shape != new.master_shape;
    let plain = old.kind == new.kind
        && old.del == new.del
        && old.data1 == new.data1
        && old.data2 == new.data2
        && old.data3 == new.data3
        && foreign_same(old.foreign.as_deref(), new.foreign.as_deref())
        && old.raw == new.raw
        && passthrough_same(&old.passthrough, &new.passthrough)
        && shape_kept_same(old, new);
    if !plain {
        return ShapeChange::Refuse(refusal(Family::Style));
    }
    let (style, geo, mut delta) = match sheet_change(&old.sheet, &new.sheet, route, steps) {
        SheetChange::Same => (None, None, None),
        SheetChange::Edit { style, geo, delta } => (style, geo, delta),
        SheetChange::Refuse(words) => return ShapeChange::Refuse(words),
    };
    if fields {
        delta.get_or_insert_with(Tier2Delta::default).fields = true;
    }
    if style.is_none() && text.is_none() && geo.is_none() && delta.is_none() {
        ShapeChange::Same
    } else {
        ShapeChange::Edit { style, text, geo, delta }
    }
}

#[cfg(test)]
mod section_agrees;
#[cfg(test)]
mod stamp_agrees;

#[cfg(test)]
mod tests {
    use super::*;

    /// What an outline edit writes, for the fingerprints of the outline: nothing, a custom shape's path alone (the output is known), or anything else (a modifier
    /// with or without the path): only the first two can have their fingerprint made again.
    #[test]
    fn what_an_outline_edit_writes_is_told_from_a_path_alone() {
        let edit = |outline: bool, modifiers: Vec<(usize, f64)>| GeoEdit { sheet: std::sync::Arc::new(Sheet::default()), outline, modifiers };
        assert_eq!(OutlineWrite::of(None), OutlineWrite::No);
        assert_eq!(OutlineWrite::of(Some(&edit(true, Vec::new()))), OutlineWrite::Path);
        assert_eq!(OutlineWrite::of(Some(&edit(false, vec![(0, 1.0)]))), OutlineWrite::Other, "a modifier alone writes the modifiers, not a path");
        assert_eq!(OutlineWrite::of(Some(&edit(true, vec![(0, 1.0)]))), OutlineWrite::Other, "a path and a modifier");
        assert_eq!(OutlineWrite::of(Some(&edit(false, Vec::new()))), OutlineWrite::Other, "an edit that writes neither is none of the known ones");
    }

    fn sheet(cells: &[(&str, f64)]) -> Sheet {
        let mut s = Sheet::default();
        for (name, v) in cells {
            s.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(*v).unwrap()));
        }
        s
    }

    /// A sheet whose `User` section holds `n` modifier rows `ODFMod0` to `ODFMod{n-1}`, each valued its number, and the value of `changed` is 7.5.
    fn modifiers(n: usize, changed: Option<usize>) -> Sheet {
        let mut section = diacraft_doc::Section::new("User", None);
        for i in 0..n {
            let mut row = diacraft_doc::Row { name: Some(std::sync::Arc::from(format!("ODFMod{i}"))), ..diacraft_doc::Row::default() };
            let v = if changed == Some(i) { 7.5 } else { i as f64 };
            row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(v).unwrap()));
            section.rows.push(std::sync::Arc::new(row));
        }
        let mut s = Sheet::default();
        s.sections.push(std::sync::Arc::new(section));
        s
    }

    #[test]
    fn the_modifiers_of_a_sheet_are_indexed_once_and_each_is_one_look_and_a_step() {
        // Security finding (MEDIUM): a search of the `User` rows for each modifier by a name made for it was quadratic in the rows and charged
        // nothing. Chained sizes: the steps follow the rows (4 times for 4 times the rows), and 20,000 modifiers take well under a second where the
        // search took tens of seconds.
        let mut counts = Vec::new();
        for n in [1_000usize, 4_000, 16_000] {
            let (old, new) = (modifiers(n, None), modifiers(n, Some(n - 1)));
            let mut steps = 0u64;
            assert_eq!(modifier_changes(&old, &new, &mut steps), [(n - 1, 7.5)], "{n}");
            assert!(steps >= 2 * n as u64, "each row of each sheet is a step: {steps}");
            counts.push(steps);
        }
        assert!(counts[1] <= counts[0] * 4 + 8 && counts[2] <= counts[1] * 4 + 8, "{counts:?}");
        let (old, new) = (modifiers(20_000, None), std::sync::Arc::new(modifiers(20_000, Some(19_999))));
        let started = std::time::Instant::now();
        let change = sheet_change(&old, &new, false, &mut 0);
        assert!(matches!(change, SheetChange::Edit { geo: Some(_), .. }), "{change:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        // A modifier that is the same, one the other sheet lacks, and a row that is no modifier are no change.
        let (a, b) = (modifiers(3, None), modifiers(5, None));
        assert!(modifier_changes(&a, &b, &mut 0).is_empty() && modifier_changes(&b, &a, &mut 0).is_empty());
    }

    /// What a sheet keeps beside its cells, each by itself, is a change the writer does not write, and is refused in words and
    /// not left out of the file as the same (the links, the attributes and the elements the model keeps without a cell).
    #[test]
    fn a_change_of_what_a_sheet_keeps_beside_its_cells_is_refused_in_words() {
        let base = sheet(&[("A", 1.0)]);
        let with = |f: &dyn Fn(&mut Sheet)| {
            let mut s = base.clone();
            f(&mut s);
            std::sync::Arc::new(s)
        };
        let refused = |new: std::sync::Arc<Sheet>, words: &str| match sheet_change(&base, &new, false, &mut 0) {
            SheetChange::Refuse(w) => assert!(w.contains(words), "{w}"),
            other => panic!("expected a refusal of {words}: {other:?}"),
        };
        refused(with(&|s| s.links.line = Some(3)), "style links");
        refused(with(&|s| s.raw.push("k", "v")), "attribute or an element");
        refused(with(&|s| s.passthrough.push(0, diacraft_doc::RawNode::Text(std::sync::Arc::from("x")))), "attribute or an element");
        assert!(matches!(sheet_change(&base, &std::sync::Arc::new(base.clone()), false, &mut 0), SheetChange::Same));
    }

    /// `n` sections named `S0` to `S{n-1}`, each with a row of one cell valued `value`.
    fn sections_of(n: usize, value: f64) -> Vec<std::sync::Arc<diacraft_doc::Section>> {
        (0..n)
            .map(|k| {
                let mut s = diacraft_doc::Section::new(&format!("S{k}"), None);
                let mut row = diacraft_doc::Row { name: Some(std::sync::Arc::from("r")), ..diacraft_doc::Row::default() };
                row.cells
                    .insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(value).unwrap()));
                s.rows.push(std::sync::Arc::new(row));
                std::sync::Arc::new(s)
            })
            .collect()
    }

    /// The comparison of two lists of sections is told apart by their keys with an index, and every turn of every loop is charged (the security review: a search
    /// of the other list for each section was a pass over it, and the count held the turns of the outer loop alone). Sections that are the same list cost three
    /// turns each, one for each pass, whatever their number; four times the sections is four times the steps.
    #[test]
    fn the_sections_of_two_lists_are_told_apart_by_an_index_and_every_turn_is_charged() {
        for n in [100usize, 400] {
            let a = sections_of(n, 1.0);
            let same: Vec<_> = a.iter().map(|s| std::sync::Arc::new((**s).clone())).collect();
            let mut steps = 0u64;
            let mut delta = Tier2Delta::default();
            section_delta_into(&a, &same[..], &mut delta, &mut steps);
            assert!(delta.sections.is_empty() && delta.removed_sections.is_empty(), "{n}: equal lists differ");
            // Each section is indexed once, looked up once, and looked at once by the pass for what is gone; a section that is the same is compared whole at
            // a cost of its own (`section_same`), which is charged by that function.
            assert!(steps >= 3 * n as u64, "{n} sections charged {steps} steps");
            assert!(steps <= 3 * n as u64 + 40 * n as u64, "{n} sections charged {steps} steps: more than three passes and the comparisons");
        }
        // Lists that have none of the keys of each other compare no section whole: the three passes over them are the whole count, to the step.
        for n in [100usize, 400] {
            let a = sections_of(n, 1.0);
            let b: Vec<_> = sections_of(n, 1.0)
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    let mut s = (**s).clone();
                    s.name = diacraft_doc::SectionName::new(&format!("T{k}"));
                    std::sync::Arc::new(s)
                })
                .collect();
            let (mut steps, mut delta) = (0u64, Tier2Delta::default());
            section_delta_into(&a, &b, &mut delta, &mut steps);
            assert_eq!((delta.sections.len(), delta.removed_sections.len()), (n, n));
            assert_eq!(steps, 3 * n as u64, "{n} sections that share no key");
        }
        // A list that has none of the keys of the other: all of one carried, all of the other gone, in order.
        let (a, b) = (sections_of(3, 1.0), sections_of(5, 2.0));
        let b: Vec<_> = b
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let mut s = (**s).clone();
                s.name = diacraft_doc::SectionName::new(&format!("T{k}"));
                std::sync::Arc::new(s)
            })
            .collect();
        let mut delta = Tier2Delta::default();
        section_delta_into(&a, &b, &mut delta, &mut 0);
        let carried: Vec<&str> = delta.sections.iter().map(|s| s.name.as_str()).collect();
        let gone: Vec<&str> = delta.removed_sections.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(carried, ["T0", "T1", "T2", "T3", "T4"]);
        assert_eq!(gone, ["S0", "S1", "S2"]);
        // The same keys with other values: each carried, and none gone.
        let (a, b) = (sections_of(4, 1.0), sections_of(4, 9.0));
        let mut delta = Tier2Delta::default();
        section_delta_into(&a, &b, &mut delta, &mut 0);
        assert_eq!(delta.sections.len(), 4);
        assert!(delta.removed_sections.is_empty());
        // A key that is twice in the baseline is found at its first place, as a search for it finds it.
        let mut twice = sections_of(1, 1.0);
        twice.push(std::sync::Arc::new((*sections_of(1, 5.0)[0]).clone()));
        let one = sections_of(1, 5.0);
        let mut delta = Tier2Delta::default();
        section_delta_into(&twice, &one, &mut delta, &mut 0);
        assert_eq!(delta.sections.len(), 1, "the first of the key is the one compared, and it is not the same");
    }

    /// The `User` rows: only the value of a modifier (`ODFMod<n>`, a handle's drag) may change; any other difference, each by itself, is refused: a
    /// row's name or type, a row deleted, another cell of a modifier, the formula of its value, a cell of any other row, and what the section keeps.
    #[test]
    fn a_change_of_a_user_row_is_carried_whole_unless_it_is_the_value_of_a_modifier() {
        use diacraft_doc::{Cell, CellName, Row, Section};
        let row = |name: &str, cells: &[(&str, Option<&str>, f64)]| {
            let mut r = Row { name: Some(std::sync::Arc::from(name)), ..Row::default() };
            for (cell, formula, v) in cells {
                r.cells.insert(CellName::new(cell), Cell::read(*formula, Some(&v.to_string()), None, None, &mut 1_000));
            }
            r
        };
        let sheet_of = |rows: Vec<Row>, section: &dyn Fn(&mut Section)| {
            let mut sec = Section::new("User", None);
            for r in rows {
                sec.rows.push(std::sync::Arc::new(r));
            }
            section(&mut sec);
            let mut s = Sheet::default();
            s.sections.push(std::sync::Arc::new(sec));
            std::sync::Arc::new(s)
        };
        let formula = Some("Controls.Row_1.X/Width*21600");
        let rows = |f: &dyn Fn(&mut Vec<Row>)| {
            let mut rows = vec![row("ODFMod0", &[("Value", formula, 5400.0)]), row("Other", &[("Value", None, 2.0)])];
            f(&mut rows);
            rows
        };
        let none: &dyn Fn(&mut Section) = &|_| {};
        let base = sheet_of(rows(&|_| {}), none);
        let outcome = |new: std::sync::Arc<Sheet>| sheet_change(&base, &new, false, &mut 0);
        // Any difference but the value of a modifier is carried by DiaCraft's extension: the `User` section, whole.
        let refused = |new: std::sync::Arc<Sheet>, what: &str| match outcome(new) {
            SheetChange::Edit { delta: Some(d), .. } => {
                assert!(d.sections.iter().any(|s| s.name.as_str() == "User"), "{what}: {d:?}");
            }
            other => panic!("{what}: expected the section to be carried: {other:?}"),
        };
        refused(sheet_of(rows(&|r| r[0].name = Some(std::sync::Arc::from("ODFMod1"))), none), "a modifier's name");
        refused(sheet_of(rows(&|r| r[1].row_type = Some(std::sync::Arc::from("T"))), none), "a row's type");
        refused(sheet_of(rows(&|r| r[1].del = true), none), "a row deleted");
        refused(sheet_of(rows(&|r| r[0].del = true), none), "a modifier deleted");
        refused(sheet_of(rows(&|r| r[1] = row("Other", &[("Value", None, 3.0)])), none), "the value of a row that is no modifier");
        refused(sheet_of(rows(&|r| r[0] = row("ODFMod0", &[("Value", formula, 5400.0), ("Extra", None, 1.0)])), none), "another cell of a modifier");
        refused(
            sheet_of(rows(&|r| r[0] = row("ODFMod0", &[("Value", Some("Controls.Row_1.X/Width*100"), 5400.0)])), none),
            "the formula of a modifier's value",
        );
        refused(sheet_of(rows(&|r| r.push(row("More", &[]))), none), "a row added");
        refused(sheet_of(rows(&|_| {}), &|s| s.raw.push("k", "v")), "the section's attributes");
        refused(
            sheet_of(rows(&|_| {}), &|s| s.passthrough.push(0, diacraft_doc::RawNode::Text(std::sync::Arc::from("x")))),
            "the section's elements",
        );
        refused(
            sheet_of(rows(&|_| {}), &|s| drop(s.cells.insert(CellName::new("Flag"), Cell::constant(diacraft_doc::Value::from_text("1"))))),
            "the section's cells",
        );
        refused(sheet_of(rows(&|_| {}), &|s| s.del = true), "the section deleted");
        refused(sheet_of(rows(&|_| {}), &|s| s.ix = Some(1)), "the section's index");
        // The value of a modifier is the change a handle's drag makes, and it is written as that.
        match outcome(sheet_of(rows(&|r| r[0] = row("ODFMod0", &[("Value", formula, 2700.0)])), none)) {
            SheetChange::Edit { geo: Some(g), style: None, delta } => {
                assert_eq!(g.modifiers, [(0, 2700.0)]);
                assert!(delta.is_none(), "a modifier's value alone carries nothing: {delta:?}");
            }
            other => panic!("expected the edit of a modifier: {other:?}"),
        }
    }

    #[test]
    fn the_table_owns_what_the_writer_writes_and_refuses_the_rest() {
        assert_eq!(owns("svg:x"), Some(Family::Transform));
        assert_eq!(owns("draw:transform"), Some(Family::Transform));
        assert_eq!(owns("draw:z-index"), Some(Family::Order));
        assert_eq!(owns("svg:d"), Some(Family::Route));
        assert_eq!(owns("draw:enhanced-path"), Some(Family::Outline), "an outline is written");
        assert_eq!(owns("draw:glue-points"), None, "glue points are not");
        assert_eq!(owns("draw:fill"), Some(Family::Restyle), "a restyle is written");
        assert_eq!(owns("fo:padding-top"), Some(Family::Restyle));
        assert_eq!(owns("draw:engine"), None, "DiaCraft never writes it");
        // A paragraph's and a run's attributes are written into a copy of the style the file has, each by its cell; what no cell maps is not owned.
        for owned in [
            "fo:text-align",
            "fo:line-height",
            "style:line-spacing",
            "fo:font-size",
            "style:text-underline-style",
            "style:text-underline-color",
            "style:font-name",
        ] {
            assert_eq!(owns(owned), Some(Family::TextStyle), "{owned}");
        }
        for kept in ["fo:hyphenate", "fo:language", "style:leader-style", "style:tab-stop-distance", "fo:orphans"] {
            assert_eq!(owns(kept), None, "{kept} is kept as the file has it");
        }
        // A fill picture's attributes go with the picture, and a connector's skew goes with its route.
        assert_eq!(owns("draw:fill-image-ref-point-y"), Some(Family::Restyle));
        // The text attributes are no shape's restyle: a family of their own.
        assert_ne!(owns("fo:font-size"), Some(Family::Restyle));
        assert_ne!(owns("fo:text-align"), Some(Family::Restyle));
        assert_eq!(owns("style:repeat"), Some(Family::Restyle));
        assert_eq!(owns("draw:line-skew"), Some(Family::Route));
        assert!(refusal(Family::Text).contains("text"));
        assert!(refusal(Family::Geometry).contains("geometry"));
    }

    #[test]
    fn a_frames_map_follows_xform() {
        // A 4 by 2 frame pinned at its centre (2, 1) of the parent, turned a quarter turn: its centre stays, and its x axis runs up.
        let s = sheet(&[
            ("PinX", 2.0),
            ("PinY", 1.0),
            ("LocPinX", 2.0),
            ("LocPinY", 1.0),
            ("Width", 4.0),
            ("Height", 2.0),
            ("Angle", std::f64::consts::FRAC_PI_2),
        ]);
        let m = local_to_parent(&s).unwrap();
        let (x, y) = m.apply(4.0, 1.0);
        assert!((x - 2.0).abs() < 1e-12 && (y - 3.0).abs() < 1e-12, "the right-middle of the frame is above the pin: {x} {y}");
        let pose = pose_2d(&s, &Affine::IDENTITY).unwrap();
        assert!(same_point(pose.centre, (2.0, 1.0)));
        assert!((turn_of(&pose).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        // A flip is no turn.
        let flipped = sheet(&[("PinX", 0.0), ("PinY", 0.0), ("LocPinX", 0.0), ("LocPinY", 0.0), ("Width", 1.0), ("Height", 1.0), ("FlipY", 1.0)]);
        assert_eq!(turn_of(&pose_2d(&flipped, &Affine::IDENTITY).unwrap()), None);
        // A cell with no number is no pose.
        let mut bad = s.clone();
        bad.cells.insert(diacraft_doc::CellName::new("PinX"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("text")));
        assert!(local_to_parent(&bad).is_none());
    }

    /// Item 7: the place of the pin edited as a constant is an edit the frame cannot write, and a place with a formula, or another cell, is not.
    #[test]
    fn a_pin_place_that_is_a_constant_and_another_number_is_an_edit_and_is_carried() {
        use diacraft_doc::{Cell, CellName, Value};
        let base = sheet(&[("PinX", 2.0), ("PinY", 1.0), ("LocPinX", 1.0), ("LocPinY", 0.5), ("Width", 2.0), ("Height", 1.0)]);
        let with = |name: &str, v: f64| {
            let mut s = base.clone();
            s.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap()));
            s
        };
        let mut steps = 0u64;
        for (name, v) in [("LocPinX", 0.75), ("LocPinY", 0.25)] {
            let moved = with(name, v);
            assert!(pin_place_edited(&base, &moved, name), "{name}");
            assert!(!sheets_same(&base, &moved, false, &mut steps), "{name}: the place is no pose value the frame writes");
            // Carried, with the pin compensating or not.
            let delta = match sheet_change(&base, &std::sync::Arc::new(moved), false, &mut steps) {
                SheetChange::Edit { delta: Some(delta), .. } => delta,
                other => panic!("{name}: {other:?}"),
            };
            assert_eq!(delta.cells.keys().map(|n| n.as_str().to_owned()).collect::<Vec<_>>(), [name]);
            assert!(!pin_place_edited(&base, &base, name));
        }
        // The same place to the tolerance of a length is no edit, and the other pose cells' values are the frame's.
        assert!(!pin_place_edited(&base, &with("LocPinX", 1.0 + 1e-12), "LocPinX"));
        assert!(sheets_same(&base, &with("LocPinX", 1.0 + 1e-12), false, &mut steps));
        for name in ["PinX", "PinY", "Width", "Height"] {
            assert!(sheets_same(&base, &with(name, 9.0), false, &mut steps), "{name}");
        }
        // A place with a formula that has another value is a calculation (a resize), in either sheet; a place the other sheet lacks is not an edit of a value.
        let formula = |v: f64| {
            let mut s = base.clone();
            s.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.5", Value::from_number(v)));
            s
        };
        assert!(!pin_place_edited(&formula(1.0), &formula(2.0), "LocPinX"));
        assert!(!pin_place_edited(&formula(1.0), &with("LocPinX", 7.0), "LocPinX"));
        assert!(!pin_place_edited(&with("LocPinX", 7.0), &formula(1.0), "LocPinX"));
        let mut lacking = base.clone();
        lacking.cells.remove("LocPinX");
        assert!(!pin_place_edited(&base, &lacking, "LocPinX") && !pin_place_edited(&lacking, &base, "LocPinX"));
        // A cell that holds text holds no number to differ by.
        let mut text = base.clone();
        text.cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("left")));
        assert!(!pin_place_edited(&base, &text, "LocPinX"));
    }

    #[test]
    fn a_pose_change_is_nothing_a_translation_or_a_reshape() {
        let base = sheet(&[("PinX", 2.0), ("PinY", 1.0), ("LocPinX", 1.0), ("LocPinY", 0.5), ("Width", 2.0), ("Height", 1.0)]);
        let pose = |s: &Sheet| pose_2d(s, &Affine::IDENTITY).unwrap();
        assert!(matches!(frame(&pose(&base), &pose(&base)), Frame::Same));
        let moved = sheet(&[("PinX", 3.0), ("PinY", 0.5), ("LocPinX", 1.0), ("LocPinY", 0.5), ("Width", 2.0), ("Height", 1.0)]);
        match frame(&pose(&base), &pose(&moved)) {
            Frame::Translate { dx, dy } => assert!((dx - 1.0).abs() < 1e-12 && (dy + 0.5).abs() < 1e-12),
            other => panic!("{other:?}"),
        }
        let wider = sheet(&[("PinX", 2.0), ("PinY", 1.0), ("LocPinX", 1.5), ("LocPinY", 0.5), ("Width", 3.0), ("Height", 1.0)]);
        assert!(matches!(frame(&pose(&base), &pose(&wider)), Frame::Reshape(_)));
        // A group's own map is part of a member's pose: moving the group moves the member.
        let group = Affine { e: 5.0, f: -1.0, ..Affine::IDENTITY };
        assert!(
            matches!(frame(&pose_2d(&base, &Affine::IDENTITY).unwrap(), &pose_2d(&base, &group).unwrap()), Frame::Translate { dx, dy } if (dx - 5.0).abs() < 1e-12 && (dy + 1.0).abs() < 1e-12)
        );
    }

    #[test]
    fn a_cell_a_calculation_recomputed_is_not_an_edit_and_a_constant_changed_is() {
        use diacraft_doc::{Cell, Value};
        let with_formula = |v: f64| Cell::with_formula("Width*0.5", Value::from_number(v));
        assert!(cell_same(&with_formula(1.0), &with_formula(2.0)), "the cached value of a formula follows what it reads");
        assert!(!cell_same(&with_formula(1.0), &Cell::with_formula("Width*0.25", Value::from_number(1.0))), "a formula changed is an edit");
        assert!(cell_same(&Cell::constant(Value::from_text("1.50")), &Cell::constant(Value::from_text("1.5"))), "the same number");
        assert!(!cell_same(&Cell::constant(Value::from_text("1")), &Cell::constant(Value::from_text("2"))));
        assert!(cell_same(&Cell::constant(Value::from_text("#FF0000")), &Cell::constant(Value::from_text("#FF0000"))));
        assert!(!cell_same(&Cell::constant(Value::from_text("#FF0000")), &Cell::constant(Value::from_text("#00FF00"))));
    }

    #[test]
    fn a_cell_with_an_attribute_or_a_child_the_other_lacks_is_not_the_same_and_two_empty_cells_are() {
        use diacraft_doc::{Cell, CellExtra, Value};
        use std::sync::Arc;
        let plain = || Cell::constant(Value::from_text("1"));
        assert!(cell_same(&plain(), &plain()));
        let mut raw = plain();
        raw.raw.push("N", "v");
        assert!(!cell_same(&plain(), &raw), "an attribute the model keeps");
        assert!(!cell_same(&raw, &plain()));
        let mut extra = plain();
        extra.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("t")), ..CellExtra::default() }));
        assert!(!cell_same(&plain(), &extra), "text inside the cell");
        let mut formula = plain();
        formula.formula = Cell::with_formula("Width*0.5", Some(Value::from_text("1"))).formula;
        assert!(!cell_same(&plain(), &formula), "a formula");
        // Neither has a value, nor a formula: the same. One has a value: not.
        assert!(cell_same(&Cell::new(), &Cell::new()));
        assert!(!cell_same(&Cell::new(), &plain()));
        assert!(!cell_same(&plain(), &Cell::new()));
    }

    /// A sheet with these sections, each given as a name, an `IX` and the number its one cell `X` holds.
    fn with_sections(sections: &[(&str, Option<u32>, f64)]) -> Sheet {
        use diacraft_doc::{Cell, CellName, Section, Value};
        let mut s = Sheet::default();
        for (name, ix, x) in sections {
            let mut section = Section::new(name, *ix);
            section.cells.insert(CellName::new("X"), Cell::constant(Value::from_number(*x).unwrap()));
            s.sections.push(std::sync::Arc::new(section));
        }
        s
    }

    #[test]
    fn a_connectors_route_is_the_geometry_section_numbered_0_and_nothing_else_is_let_go() {
        let mut steps = 0;
        let was = with_sections(&[("Geometry", Some(0), 1.0), ("Geometry", Some(1), 5.0), ("Control", Some(0), 7.0)]);
        // The route changed: the same where it is the route's to change, not otherwise.
        let route_moved = with_sections(&[("Geometry", Some(0), 2.0), ("Geometry", Some(1), 5.0), ("Control", Some(0), 7.0)]);
        assert!(sheets_same(&was, &route_moved, true, &mut steps));
        assert!(!sheets_same(&was, &route_moved, false, &mut steps));
        assert!(!route_same(&was, &route_moved, &mut steps));
        assert!(route_same(&was, &was.clone(), &mut steps));
        // Another section changed: never the same, as a route or not.
        for other in [
            with_sections(&[("Geometry", Some(0), 1.0), ("Geometry", Some(1), 6.0), ("Control", Some(0), 7.0)]),
            with_sections(&[("Geometry", Some(0), 1.0), ("Geometry", Some(1), 5.0), ("Control", Some(0), 8.0)]),
        ] {
            assert!(!sheets_same(&was, &other, true, &mut steps));
            assert!(!sheets_same(&was, &other, false, &mut steps));
            assert!(route_same(&was, &other, &mut steps), "the route itself is the same");
        }
        // A route section the model has and the file lacks, or the file has and the model lacks.
        let none = with_sections(&[("Geometry", Some(1), 5.0), ("Control", Some(0), 7.0)]);
        assert!(sheets_same(&was, &none, true, &mut steps), "a route is the route's to add or take out");
        assert!(!sheets_same(&was, &none, false, &mut steps));
        assert!(!route_same(&was, &none, &mut steps) && !route_same(&none, &was, &mut steps));
        assert!(route_same(&none, &none.clone(), &mut steps), "neither has one");
        // The route is found by its name and its number together: a section numbered 0 of another name, or a `Geometry` of another number, that
        // comes first is not it.
        let decoys =
            |route: f64, controls: f64| with_sections(&[("Control", Some(0), controls), ("Geometry", Some(1), 5.0), ("Geometry", Some(0), route)]);
        assert!(!route_same(&decoys(1.0, 7.0), &decoys(2.0, 7.0), &mut steps), "the route differs");
        assert!(route_same(&decoys(1.0, 7.0), &decoys(1.0, 8.0), &mut steps), "the route is the same and a section that is not it differs");
        // A sheet's links, attributes and children kept in place are compared.
        let mut raw = was.clone();
        raw.raw.push("k", "v");
        assert!(!sheets_same(&was, &raw, true, &mut steps));
        let mut cell = was.clone();
        cell.cells.insert(diacraft_doc::CellName::new("Fill"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        assert!(!sheets_same(&was, &cell, true, &mut steps));
        let mut posed = was.clone();
        posed.cells.insert(diacraft_doc::CellName::new("PinX"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        assert!(sheets_same(&was, &posed, true, &mut steps), "the pose cells are compared apart");
    }

    #[test]
    fn two_segments_are_the_same_when_each_end_is() {
        let a = [(1.0, 2.0), (3.0, 4.0)];
        assert!(same_ends(&a, &a));
        assert!(!same_ends(&a, &[(1.0, 2.0), (3.0, 4.5)]), "the second end moved");
        assert!(!same_ends(&a, &[(1.5, 2.0), (3.0, 4.0)]), "the first end moved");
        assert!(same_ends(&a, &[(1.0 + 1e-14, 2.0), (3.0, 4.0 - 1e-14)]), "within the tolerance");
    }

    #[test]
    fn a_turn_is_read_off_the_linear_part_of_a_pose_and_what_is_more_than_a_turn_has_none() {
        let pose = |lin: [f64; 4]| Pose { centre: (0.0, 0.0), lin, size: (1.0, 1.0) };
        let angle = |lin: [f64; 4]| turn_of(&pose(lin));
        assert_eq!(angle([1.0, 0.0, 0.0, 1.0]), Some(0.0));
        // A turn of a half turn is pi and not 0, whatever the sign of the cosine is (the zero the sine rounds to is told from the turn).
        assert_eq!(angle([-1.0, 0.0, 0.0, -1.0]), Some(std::f64::consts::PI));
        assert_eq!(angle([0.0, 1.0, -1.0, 0.0]), Some(std::f64::consts::FRAC_PI_2));
        assert_eq!(angle([0.0, -1.0, 1.0, 0.0]), Some(-std::f64::consts::FRAC_PI_2));
        // No turn is no turn, with a positive zero, from a sine of either zero.
        let no_turn = angle([1.0, -0.0, 0.0, 1.0]);
        assert!(no_turn.is_some_and(|a| a == 0.0 && a.is_sign_positive()), "{no_turn:?}");
        // A flip, a scale and a shear are more than a turn.
        assert_eq!(angle([-1.0, 0.0, 0.0, 1.0]), None, "a flip");
        assert_eq!(angle([1.0, 0.0, 0.0, -1.0]), None, "a flip");
        assert_eq!(angle([2.0, 0.0, 0.0, 2.0]), None, "a scale");
        assert_eq!(angle([1.0, 0.0, 0.5, 1.0]), None, "a shear");
        assert_eq!(angle([1.0, 0.5, 0.0, 1.0]), None, "a shear the other way");
    }

    #[test]
    fn a_change_of_a_shape_is_told_by_what_changed_and_what_the_writer_does_not_say_is_carried() {
        let base = with_sections(&[("Geometry", Some(0), 1.0), ("User", None, 1.0)]);
        let change = |new: Sheet| sheet_change(&base, &std::sync::Arc::new(new), false, &mut 0);
        // What no attribute of the file says is carried by the extension: the names of the sections and cells it holds.
        let carried = |c: SheetChange| match c {
            SheetChange::Edit { delta: Some(d), .. } => {
                let mut names: Vec<String> = d.sections.iter().map(|s| s.name.as_str().to_owned()).collect();
                names.extend(d.cells.keys().map(|n| n.as_str().to_owned()));
                names
            }
            other => panic!("expected a change that is carried: {other:?}"),
        };
        assert!(matches!(change(base.clone()), SheetChange::Same));
        // An outline: a geometry section added, taken out, or a cell of one changed.
        let geometry = |c: SheetChange| matches!(c, SheetChange::Edit { geo: Some(g), style: None, .. } if g.outline);
        assert!(geometry(change(with_sections(&[("Geometry", Some(0), 1.0), ("Geometry", Some(1), 1.0), ("User", None, 1.0)]))));
        assert!(geometry(change(with_sections(&[("User", None, 1.0)]))));
        assert!(geometry(change(with_sections(&[("Geometry", Some(0), 2.0), ("User", None, 1.0)]))));
        // A user row that is no modifier, or a section the writer does not write.
        assert_eq!(carried(change(with_sections(&[("Geometry", Some(0), 1.0), ("User", None, 2.0)]))), ["User"]);
        assert_eq!(carried(change(with_sections(&[("Geometry", Some(0), 1.0), ("User", None, 1.0), ("Connection", None, 1.0)]))), ["Connection"]);
        // A handle's place that no modifier follows.
        assert_eq!(carried(change(with_sections(&[("Geometry", Some(0), 1.0), ("User", None, 1.0), ("Control", None, 1.0)]))), ["Control"]);
        // A cell: one the table says is geometry, one it does not know, and a style cell.
        let with = |name: &str, v: &str| {
            let mut s = base.clone();
            s.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(v)));
            s
        };
        assert_eq!(carried(change(with("Rounding", "0.2"))), ["Rounding"]);
        assert_eq!(carried(change(with("Comment", "x"))), ["Comment"]);
        assert!(matches!(change(with("FillForegnd", "#336699")), SheetChange::Edit { style: Some(_), geo: None, .. }));
    }

    /// A sheet of a custom shape as the reader and the VSDX reader build it: the handles' section (`Control`, which a formula spells `Controls.`; the name is a
    /// parameter so that the formula's word can be tried) of one row whose `X` holds `x`, and the `User` row `ODFMod0` that the handle drives, holding `modifier`. The
    /// formulas are the reader's own (the handle's `X` is a share of the frame); a drag changes the cached values and not them.
    fn handle_sheet(section: &str, x: f64, modifier: f64) -> Sheet {
        use diacraft_doc::{Cell, CellName, Row, Section};
        let cell = |formula: Option<&str>, v: f64| Cell::read(formula, Some(&v.to_string()), None, None, &mut 1_000);
        let mut handle = Row { name: Some(std::sync::Arc::from("Row_1")), ..Row::default() };
        handle.cells.insert(CellName::new("X"), cell(Some("Width*0.25"), x));
        handle.cells.insert(CellName::new("XDyn"), cell(Some("Controls.Row_1.X"), x));
        let mut control = Section::new(section, None);
        control.rows.push(std::sync::Arc::new(handle));
        let mut driven = Row { name: Some(std::sync::Arc::from("ODFMod0")), ..Row::default() };
        driven.cells.insert(CellName::new("Value"), cell(Some("Controls.Row_1.X/Width*21600"), modifier));
        let mut user = Section::new(crate::sections::USER, None);
        user.rows.push(std::sync::Arc::new(driven));
        let mut s = Sheet::default();
        s.sections.push(std::sync::Arc::new(control));
        s.sections.push(std::sync::Arc::new(user));
        s
    }

    /// The handle of `sheet` written as the constant `x`, as a write of a value to the cell leaves it (the formula is gone).
    fn constant_x(sheet: &mut Sheet, x: f64) {
        use diacraft_doc::{Cell, CellName, Value};
        let section = std::sync::Arc::make_mut(&mut sheet.sections[0]);
        for row in section.rows.iter_mut() {
            std::sync::Arc::make_mut(row).cells.insert(CellName::new("X"), Cell::constant(Value::from_number(x).unwrap()));
        }
    }

    /// The names the writer matches by are the model's (`Control`, which a formula writes `Controls`), so that a handle a drag moved is a modifier's change and the
    /// handles' section is not carried whole beside it (SECNAME). A section of the formula's spelling is not the handles': it is any other section, and is carried.
    #[test]
    fn the_handles_section_is_the_models_control_and_a_drag_of_a_handle_is_a_change_of_its_modifier() {
        assert!(SPECIAL_SECTIONS.iter().all(|name| diacraft_doc::SECTION_NAMES.contains(name)), "{SPECIAL_SECTIONS:?}");
        assert!(SPECIAL_SECTIONS.contains(&crate::sections::CONTROL) && SPECIAL_SECTIONS.contains(&crate::sections::USER));
        let control = crate::sections::CONTROL;
        let was = handle_sheet(control, 1.0, 5400.0);
        match sheet_change(&was, &std::sync::Arc::new(handle_sheet(control, 0.5, 2700.0)), false, &mut 0) {
            SheetChange::Edit { style: None, geo: Some(g), delta: None } => {
                assert_eq!(g.modifiers, [(0, 2700.0)], "the modifier the handle drives");
                assert!(!g.outline);
            }
            other => panic!("a handle dragged, with its modifier following: {other:?}"),
        }
        // A recalculation that moves the handle under its formula (the frame changed) and no modifier is no change of the section: nothing is carried.
        assert!(matches!(sheet_change(&was, &std::sync::Arc::new(handle_sheet(control, 0.5, 5400.0)), false, &mut 0), SheetChange::Same));
        // A handle written as a constant and no modifier following is the handles' section, carried whole.
        let mut alone = handle_sheet(control, 0.5, 5400.0);
        constant_x(&mut alone, 0.5);
        match sheet_change(&was, &std::sync::Arc::new(alone), false, &mut 0) {
            SheetChange::Edit { delta: Some(d), geo: None, .. } => {
                let carried: Vec<&str> = d.sections.iter().map(|s| s.name.as_str()).collect();
                assert_eq!(carried, [control], "{d:?}");
            }
            other => panic!("a handle moved, no modifier following: {other:?}"),
        }
        // Handles that did not change are not carried because something else did (the baseline's sections are found by the same name as the model's).
        let mut commented = handle_sheet(control, 1.0, 5400.0);
        commented.cells.insert(diacraft_doc::CellName::new("Comment"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("x")));
        match sheet_change(&was, &std::sync::Arc::new(commented), false, &mut 0) {
            SheetChange::Edit { delta: Some(d), .. } => assert!(d.sections.is_empty() && d.cells.contains_key("Comment"), "{d:?}"),
            other => panic!("a cell changed beside handles that did not: {other:?}"),
        }
        // The handles under the word a formula uses make another section: it is not the handles', and no modifier excuses its change.
        let (was, mut dragged) = (handle_sheet("Controls", 1.0, 5400.0), handle_sheet("Controls", 0.5, 2700.0));
        constant_x(&mut dragged, 0.5);
        match sheet_change(&was, &std::sync::Arc::new(dragged), false, &mut 0) {
            SheetChange::Edit { delta: Some(d), .. } => assert!(d.sections.iter().any(|s| s.name.as_str() == "Controls"), "{d:?}"),
            other => panic!("a section named by the formula's word is carried whole: {other:?}"),
        }
    }

    /// A section of the user cells that is added or taken out has no modifier to follow, and is carried.
    #[test]
    fn a_user_section_that_is_added_or_taken_out_is_carried() {
        let none = with_sections(&[("Geometry", Some(0), 1.0)]);
        let with_user = with_sections(&[("Geometry", Some(0), 1.0), ("User", None, 1.0)]);
        let change = |old: &Sheet, new: &Sheet| sheet_change(old, &std::sync::Arc::new(new.clone()), false, &mut 0);
        match change(&none, &with_user) {
            SheetChange::Edit { delta: Some(d), .. } => assert!(d.sections.iter().any(|s| s.name.as_str() == "User"), "{d:?}"),
            other => panic!("a user section added: {other:?}"),
        }
        match change(&with_user, &none) {
            SheetChange::Edit { delta: Some(d), .. } => assert_eq!(d.removed_sections, [diacraft_doc::SectionKey::new("User", None)], "{d:?}"),
            other => panic!("a user section taken out: {other:?}"),
        }
    }

    /// A pose cell's value is the frame's and is written natively; what the cell holds beside it (a formula, an error, attributes, content), a pose cell that is
    /// new and one that is gone are carried, each by itself. (A sheet that differs in the pose cells alone is no edit of the sheet to a save: the frame's change
    /// is told apart by the pose, so the sheets here differ in a user cell too.)
    #[test]
    fn a_pose_cell_is_carried_for_what_it_holds_beside_its_value_and_for_being_added_or_taken_out() {
        use diacraft_doc::{Cell, CellExtra, CellName};
        let plain = || Cell::read(None, Some("1"), None, None, &mut 1_000);
        let sheet_with = |cell: Option<Cell>, comment: bool| {
            let mut s = Sheet::default();
            if let Some(c) = cell {
                s.cells.insert(CellName::new("PinX"), c);
            }
            if comment {
                s.cells.insert(CellName::new("Comment"), Cell::read(None, Some("c"), None, None, &mut 1_000));
            }
            s
        };
        let change =
            |old: Option<Cell>, new: Option<Cell>| sheet_change(&sheet_with(old, false), &std::sync::Arc::new(sheet_with(new, true)), false, &mut 0);
        let delta_of = |what: &str, c: SheetChange| match c {
            SheetChange::Edit { delta: Some(d), .. } => d,
            other => panic!("{what}: expected a delta: {other:?}"),
        };
        let carried = |what: &str, c: SheetChange| {
            let d = delta_of(what, c);
            assert!(d.cells.contains_key("PinX") && d.cells.contains_key("Comment") && d.removed_cells.is_empty(), "{what}: {d:?}");
        };
        carried("a formula", change(Some(plain()), Some(Cell::read(Some("Width*2"), Some("1"), None, None, &mut 1_000))));
        carried("an error", change(Some(plain()), Some(Cell::read(None, Some("1"), None, Some("#REF!"), &mut 1_000))));
        let mut raw = plain();
        raw.raw.push("k", "v");
        carried("an attribute", change(Some(plain()), Some(raw)));
        let mut extra = plain();
        extra.extra = Some(std::sync::Arc::new(CellExtra { text: Some(std::sync::Arc::from("t")), ..CellExtra::default() }));
        carried("content", change(Some(plain()), Some(extra)));
        carried("a cell that is new", change(None, Some(plain())));
        let d = delta_of("a cell that is gone", change(Some(plain()), None));
        assert!(d.cells.contains_key("Comment") && !d.cells.contains_key("PinX") && d.removed_cells == ["PinX"], "{d:?}");
        // The value alone is the frame's: the cell is not carried, and the user cell beside it is.
        let d = delta_of("a value", change(Some(plain()), Some(Cell::read(None, Some("2"), None, None, &mut 1_000))));
        assert!(d.cells.contains_key("Comment") && !d.cells.contains_key("PinX") && d.removed_cells.is_empty(), "{d:?}");
    }

    /// A style cell the attributes cannot say is carried by the extension, and the cells that go with it are carried too only when they are a group (the four
    /// margins, the four of an arrow): a margin that changes beside a dashed line is written, and a line end that cannot be written takes its size with it.
    #[test]
    fn a_style_cell_that_cannot_be_written_takes_with_it_only_the_cells_of_its_group() {
        use diacraft_doc::{Cell, CellName};
        let inches = |v: &str| Cell::read(None, Some(v), Some("IN"), None, &mut 1_000);
        let plain = |v: &str| Cell::read(None, Some(v), None, None, &mut 1_000);
        let change = |cells: Vec<(&str, Cell)>| {
            let mut s = Sheet::default();
            for (n, c) in cells {
                s.cells.insert(CellName::new(n), c);
            }
            sheet_change(&Sheet::default(), &std::sync::Arc::new(s), false, &mut 0)
        };
        // A dashed line cannot be written as an attribute: it is carried, and the margin beside it is written, whole.
        match change(vec![("LinePattern", plain("2")), ("TopMargin", inches("0.1"))]) {
            SheetChange::Edit { style: Some(edit), delta: Some(d), .. } => {
                assert!(d.cells.contains_key("LinePattern") && !d.cells.contains_key("TopMargin"), "{d:?}");
                assert!(edit.changes.iter().any(|(c, _)| *c == "TopMargin") && !edit.margins.is_empty(), "{edit:?}");
            }
            other => panic!("a dash beside a margin: {other:?}"),
        }
    }

    /// Two sections of one name cannot be told apart in the file: a change that carries a section is refused when the model holds two of one key, and a model that
    /// holds two the file has already and carries none of them is not refused for it.
    #[test]
    fn a_carried_section_is_refused_when_the_model_holds_two_of_its_key_and_two_that_are_not_carried_are_not() {
        let old = with_sections(&[("Geometry", Some(0), 1.0), ("Property", None, 1.0)]);
        let change = |old: &Sheet, new: &Sheet| sheet_change(old, &std::sync::Arc::new(new.clone()), false, &mut 0);
        let twice = with_sections(&[("Geometry", Some(0), 1.0), ("Property", None, 2.0), ("Property", None, 3.0)]);
        match change(&old, &twice) {
            SheetChange::Refuse(words) => assert!(words.contains("two sections of one name"), "{words}"),
            other => panic!("two sections of one key, carried: {other:?}"),
        }
        // The same two on both sides and a user cell beside them: nothing is carried of the sections, and the change is not refused.
        let both = with_sections(&[("Geometry", Some(0), 1.0), ("Property", None, 2.0), ("Property", None, 3.0)]);
        let mut with_cell = both.clone();
        with_cell.cells.insert(diacraft_doc::CellName::new("Comment"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("x")));
        match change(&both, &with_cell) {
            SheetChange::Edit { delta: Some(d), .. } => assert!(d.sections.is_empty() && d.cells.contains_key("Comment"), "{d:?}"),
            other => panic!("two sections that are not carried: {other:?}"),
        }
    }

    /// The names of a shape and its links to a master are the extension's, and each alone makes the change one that has fields.
    #[test]
    fn each_of_the_names_and_links_of_a_shape_alone_is_a_change_of_its_fields() {
        let base = Shape::new(1);
        let change = |new: Shape| shape_change(&base, &new, false, &|_| None, &mut 0, &mut 1_000_000);
        assert!(matches!(change(base.clone()), ShapeChange::Same));
        let edits: Vec<(&str, Shape)> = vec![
            ("name", Shape { name: Some(std::sync::Arc::from("n")), ..base.clone() }),
            ("name_u", Shape { name_u: Some(std::sync::Arc::from("u")), ..base.clone() }),
            ("master", Shape { master: Some(4), ..base.clone() }),
            ("master_shape", Shape { master_shape: Some(5), ..base.clone() }),
        ];
        for (what, new) in edits {
            match change(new) {
                ShapeChange::Edit { delta: Some(d), style: None, text: None, geo: None } => assert!(d.fields, "{what}: {d:?}"),
                other => panic!("{what}: expected a change of the fields: {other:?}"),
            }
        }
    }

    /// A sheet of the cells `name` and its value, each a number or the text.
    fn cells(list: &[(&str, &str)]) -> Sheet {
        let mut s = Sheet::default();
        for (name, v) in list {
            s.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(v)));
        }
        s
    }

    fn colour_edit(names: &[(&'static str, &str)]) -> super::super::stylemap::StyleEdit {
        let value = |v: &str| super::super::stylemap::CellVal { text: v.to_owned(), number: v.parse().ok(), unit: None };
        super::super::stylemap::StyleEdit {
            changes: names.iter().map(|(n, v)| (*n, Some(value(v)))).collect(),
            margins: Vec::new(),
            fill_kind: None,
            shadow_visible: None,
            fx: None,
            fx_off: None,
            line: Vec::new(),
            diamonds: Vec::new(),
            parent: None,
        }
    }

    /// What `decide_fill` decides for the edit `edit` of a shape whose own sheet is `sheet` and whose styles give the cells `inherited`.
    fn decided(edit: &[(&'static str, &str)], sheet: Sheet, inherited: &[(&str, f64)]) -> Option<&'static str> {
        let mut e = colour_edit(edit);
        decide_fill(&mut e, &sheet, |name| inherited.iter().find(|(n, _)| *n == name).map(|(_, v)| *v));
        e.fill_kind
    }

    #[test]
    fn the_fill_a_colour_asks_for_is_solid_when_the_models_fill_pattern_is_1_and_nothing_else() {
        let colour = [("FillForegnd", "#123456")];
        let decide = decided;
        // The pattern of the shape's own sheet decides, and what its styles give is not asked for.
        assert_eq!(decide(&colour, cells(&[("FillPattern", "1")]), &[("FillPattern", 0.0)]), Some("solid"));
        let mut e = colour_edit(&colour);
        decide_fill(&mut e, &cells(&[("FillPattern", "1"), ("FillGradientEnabled", "0")]), |name| panic!("{name} is asked of the styles"));
        assert_eq!(e.fill_kind, Some("solid"));
        // A pattern of 0 is a fill the file already states (`none`; the reader reads a bitmap and a hatch that is not solid as 0 too): nothing is written, so that
        // the colour set on a bitmap does not take its image away.
        assert_eq!(decide(&colour, cells(&[("FillPattern", "0")]), &[("FillPattern", 1.0)]), None);
        assert_eq!(decide(&colour, cells(&[]), &[("FillPattern", 0.0)]), None);
        // With none of its own, the styles give it.
        assert_eq!(decide(&colour, cells(&[]), &[("FillPattern", 1.0)]), Some("solid"));
        assert_eq!(decide(&colour, cells(&[]), &[]), None, "a pattern no style gives is not guessed");
        assert_eq!(decide(&colour, cells(&[("FillPattern", "5")]), &[]), None, "a pattern that is neither 0 nor 1 is the pattern's to write");
        // A gradient turned off is a fill that is solid; a gradient still on is not a colour's fill.
        let off = [("FillGradientEnabled", "0")];
        assert_eq!(decide(&off, cells(&[("FillPattern", "1"), ("FillGradientEnabled", "0")]), &[]), Some("solid"));
        assert_eq!(decide(&colour, cells(&[("FillPattern", "1"), ("FillGradientEnabled", "1")]), &[]), None);
        // The gradient is looked for where the pattern is: in the styles when the shape's own sheet holds none, and the shape's own cell wins over the styles'.
        assert_eq!(decide(&colour, cells(&[("FillPattern", "1")]), &[("FillGradientEnabled", 1.0)]), None, "a gradient the styles give is still on");
        assert_eq!(decide(&colour, cells(&[]), &[("FillPattern", 1.0), ("FillGradientEnabled", 1.0)]), None);
        assert_eq!(decide(&colour, cells(&[("FillPattern", "1"), ("FillGradientEnabled", "0")]), &[("FillGradientEnabled", 1.0)]), Some("solid"));
        assert_eq!(decide(&colour, cells(&[("FillPattern", "1")]), &[("FillGradientEnabled", 0.0)]), Some("solid"));
        // An edit that states the pattern leaves it to the pattern's own cell, and an edit that is neither is no fill's.
        assert_eq!(decide(&[("FillPattern", "1"), ("FillForegnd", "#123456")], cells(&[("FillPattern", "1")]), &[]), None);
        assert_eq!(decide(&[("LineWeight", "0.1")], cells(&[("FillPattern", "1")]), &[]), None);
        // Only the gradient cell turned to 0 is a gradient turned off: another cell that is 0, or the gradient cell turned to something else, is not.
        assert_eq!(decide(&[("LineWeight", "0")], cells(&[("FillPattern", "1")]), &[]), None);
        assert_eq!(decide(&[("FillGradientEnabled", "1")], cells(&[("FillPattern", "1")]), &[]), None);
        assert_eq!(decide(&[("FillGradientEnabled", "0"), ("LineWeight", "0.1")], cells(&[("FillPattern", "1")]), &[]), Some("solid"));
        assert_eq!(decide(&[("FillGradientEnabled", "0"), ("LineWeight", "0.1")], cells(&[("FillPattern", "0")]), &[]), None);
        // A colour taken away (the cell gone) asks for nothing.
        let mut gone = colour_edit(&[]);
        gone.changes.push(("FillForegnd", None));
        decide_fill(&mut gone, &cells(&[("FillPattern", "1")]), |_| None);
        assert_eq!(gone.fill_kind, None);
    }

    #[test]
    fn the_flips_of_a_sheet_are_the_factors_on_its_axes_and_undone_they_leave_the_turn() {
        assert_eq!(flips_of(&cells(&[])), Some((1.0, 1.0)));
        assert_eq!(flips_of(&cells(&[("FlipX", "1")])), Some((-1.0, 1.0)));
        assert_eq!(flips_of(&cells(&[("FlipY", "1"), ("FlipX", "0")])), Some((1.0, -1.0)));
        assert_eq!(flips_of(&cells(&[("FlipX", "x")])), None, "a flip that is no number");
        // A shape turned 30 degrees and flipped any way has a linear part that is not 30 degrees as soon as it is flipped both ways (it is 210); with its
        // flips undone it is 30 in each of the four.
        let angle = 30f64.to_radians().to_string();
        for (fx, fy) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut sheet =
                cells(&[("PinX", "2"), ("PinY", "1"), ("LocPinX", "1"), ("LocPinY", "0.5"), ("Width", "2"), ("Height", "1"), ("Angle", &angle)]);
            for (name, on) in [("FlipX", fx), ("FlipY", fy)] {
                if on {
                    sheet.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
                }
            }
            let pose = pose_2d(&sheet, &Affine::IDENTITY).unwrap();
            let turn = turn_of(&unflipped(&pose, flips_of(&sheet).unwrap())).unwrap();
            assert!((turn.to_degrees() - 30.0).abs() < 1e-9, "{fx} {fy}: {}", turn.to_degrees());
            if fx && fy {
                let whole = turn_of(&pose).unwrap().to_degrees();
                assert!((whole - 30.0).abs() > 1.0, "flipped both ways the linear part is a turn of half a turn more: {whole}");
            }
        }
    }

    #[test]
    fn a_text_set_on_a_picture_or_an_object_frame_is_refused_in_words_and_on_a_text_frame_it_is_not() {
        use diacraft_doc::Text;
        use diacraft_doc::TextPart;
        let text = |s: &str| Some(std::sync::Arc::new(Text { parts: vec![TextPart::Chars(std::sync::Arc::from(s))], ..Default::default() }));
        let old = Shape { kind: ShapeType::Foreign, ..Shape::new(1) };
        let new = Shape { kind: ShapeType::Foreign, text: text("Ab"), ..Shape::new(1) };
        match shape_change(&old, &new, false, &crate::write::runs::no_palette, &mut 0, &mut crate::write::runs::all_the_room()) {
            ShapeChange::Refuse(words) => assert_eq!(words, WORDS_FRAME_TEXT),
            other => panic!("{other:?}"),
        }
        // The same on a shape that is not a picture is an edit of the text.
        let old = Shape::new(1);
        let new = Shape { text: text("Ab"), ..Shape::new(1) };
        match shape_change(&old, &new, false, &crate::write::runs::no_palette, &mut 0, &mut crate::write::runs::all_the_room()) {
            ShapeChange::Refuse(words) => assert_ne!(words, WORDS_FRAME_TEXT, "{words}"),
            ShapeChange::Edit { .. } => {}
            other => panic!("{other:?}"),
        }
        // The refusal is for a picture on either side of the change: a shape that was one and is not, and one that was not and is.
        for (was, now) in [(ShapeType::Foreign, ShapeType::Shape), (ShapeType::Shape, ShapeType::Foreign)] {
            let old = Shape { kind: was.clone(), ..Shape::new(1) };
            let new = Shape { kind: now.clone(), text: text("Ab"), ..Shape::new(1) };
            assert!(
                matches!(shape_change(&old, &new, false, &crate::write::runs::no_palette, &mut 0, &mut crate::write::runs::all_the_room()), ShapeChange::Refuse(words) if words == WORDS_FRAME_TEXT),
                "{was:?} to {now:?}"
            );
        }
        // A picture frame whose text the model has not changed is no change at all.
        let frame = Shape { kind: ShapeType::Foreign, text: text("Ab"), ..Shape::new(1) };
        assert!(matches!(
            shape_change(&frame, &frame.clone(), false, &crate::write::runs::no_palette, &mut 0, &mut crate::write::runs::all_the_room()),
            ShapeChange::Same
        ));
    }

    /// A sheet whose `Character` section holds a row of `Style` `style`, built anew for each call (so that no two share a section), and a `User` row of `user`.
    fn with_rows(style: Option<f64>, user: f64) -> Sheet {
        let mut s = Sheet::default();
        if let Some(style) = style {
            let mut section = diacraft_doc::Section::new("Character", None);
            let mut row = diacraft_doc::Row { ix: Some(0), ..diacraft_doc::Row::default() };
            row.cells.insert(diacraft_doc::CellName::new("Style"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(style).unwrap()));
            section.rows.push(std::sync::Arc::new(row));
            s.sections.push(std::sync::Arc::new(section));
        }
        let mut other = diacraft_doc::Section::new("User", None);
        let mut row = diacraft_doc::Row { name: Some(std::sync::Arc::from("Mine")), ..diacraft_doc::Row::default() };
        row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(user).unwrap()));
        other.rows.push(std::sync::Arc::new(row));
        s.sections.push(std::sync::Arc::new(other));
        s
    }

    #[test]
    fn the_rows_of_a_text_are_the_same_by_what_they_hold_and_not_by_the_section_that_holds_them() {
        let mut steps = 0u64;
        // Two sections built apart that hold the same row are the same: a model that was rebuilt is no edit.
        assert!(text_rows_same(&with_rows(Some(1.0), 0.0), &with_rows(Some(1.0), 0.0), &mut steps));
        assert!(steps > 0, "the rows were compared, and charged");
        // A row that holds another value, or a sheet with a section the other has none of, are not.
        assert!(!text_rows_same(&with_rows(Some(1.0), 0.0), &with_rows(Some(3.0), 0.0), &mut steps));
        assert!(!text_rows_same(&with_rows(Some(1.0), 0.0), &with_rows(None, 0.0), &mut steps));
        assert!(!text_rows_same(&with_rows(None, 0.0), &with_rows(Some(1.0), 0.0), &mut steps));
        // The rows of other sections are no part of what the text uses.
        assert!(text_rows_same(&with_rows(Some(1.0), 0.0), &with_rows(Some(1.0), 9.0), &mut steps));
        // The same sections (one pointer) are the same, and cost nothing to tell.
        let one = with_rows(Some(1.0), 0.0);
        let mut free = 0u64;
        assert!(text_rows_same(&one, &one.clone(), &mut free));
        assert_eq!(free, 0, "the shared section is not walked");
    }

    fn foreign() -> diacraft_doc::ForeignData {
        diacraft_doc::ForeignData {
            attrs: diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
            rel_id: Some(std::sync::Arc::from("rId1")),
            text: Some(std::sync::Arc::from("text")),
            picture: Some(std::sync::Arc::new(diacraft_doc::Media {
                part: std::sync::Arc::from("Pictures/a.png"),
                content_type: std::sync::Arc::from("image/png"),
                bytes: std::sync::Arc::from(&b"\x89PNG data"[..]),
            })),
            ..diacraft_doc::ForeignData::default()
        }
    }

    #[test]
    fn foreign_data_is_the_same_by_each_of_its_parts_and_a_picture_by_its_bytes() {
        let one = foreign();
        assert!(foreign_same(None, None) && foreign_same(Some(&one), Some(&one.clone())));
        assert!(!foreign_same(Some(&one), None) && !foreign_same(None, Some(&one)));
        // Each part alone makes the data another's.
        let mut other = one.clone();
        other.attrs = diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Object")]);
        assert!(!foreign_same(Some(&one), Some(&other)), "the attributes");
        let mut other = one.clone();
        other.rel_id = Some(std::sync::Arc::from("rId2"));
        assert!(!foreign_same(Some(&one), Some(&other)), "the relationship");
        let mut other = one.clone();
        other.text = None;
        assert!(!foreign_same(Some(&one), Some(&other)), "the text");
        let mut other = one.clone();
        other.picture = None;
        assert!(!foreign_same(Some(&one), Some(&other)) && !foreign_same(Some(&other), Some(&one)), "a picture and none");
        let mut other = one.clone();
        let media = other.picture.as_ref().map(|m| diacraft_doc::Media { bytes: std::sync::Arc::from(&b"\x89PNG other"[..]), ..(**m).clone() });
        other.picture = media.map(std::sync::Arc::new);
        assert!(!foreign_same(Some(&one), Some(&other)), "the bytes of the picture");
        // The place it was read from and the type it was declared under are not what the picture is.
        let mut alike = one.clone();
        let media = alike.picture.as_ref().map(|m| diacraft_doc::Media {
            part: std::sync::Arc::from("a picture"),
            content_type: std::sync::Arc::from("image/x-own"),
            ..(**m).clone()
        });
        alike.picture = media.map(std::sync::Arc::new);
        assert!(foreign_same(Some(&one), Some(&alike)), "the same bytes under another name and type");
    }
}
