//! New shapes and connectors ("add a connector"):
//! written as **tier 1**, the shape the reader rebuilds with no data beside the file's own.
//!
//! * **A 2-D shape** is a `draw:custom-shape` with `draw:style-name`, `draw:text-style-name`, `draw:layer` (the layer its `LayerMember` names, `layout` for none: [`super::layers`]), `draw:name` when it has a
//!   `NameU`, and the frame as `svg:width`, `svg:height`, `svg:x` and `svg:y`; its text as `text:p` (an empty one when it has none); and a
//!   `draw:enhanced-geometry` of `svg:viewBox="0 0 21600 21600"`, `draw:type="non-primitive"` and the `draw:enhanced-path` of its outline
//!   ([`super::geometry::new_outline`]: sets, and arcs as `A` and `W`). It gets a new automatic graphic style (its style cells, and `draw:auto-grow-height="false"` when it holds text)
//!   and a new automatic paragraph style, named by [`super::styles::Names`].
//! * **A connector** is a `draw:connector` with its style, layer, glue (`draw:start-shape`, `draw:start-glue-point`, and the end's) and ends
//!   (`svg:x1` to `svg:y2`), its route as `svg:d` and `svg:viewBox` in LibreOffice's form, and its text as `text:p` (an empty one when it has none). A shape a connector names gets
//!   `xml:id` and `draw:id`, equal and `id` and the next number after the file's largest, in the order the connectors name them (probe R16), unless
//!   it has one, which is then used. The glue id of a row is the one R38 maps it to.
//! * **A picture** is a `draw:frame` holding a `draw:image`: in a package the image names a new entry under `Pictures/`, which the save adds with
//!   a manifest row ([`super::picture`]); in a flat drawing it holds the picture inline. What the frame's markup cannot say is tier 2, as a
//!   custom shape's is, and the prediction of the rebuild is a picture frame's (`native::Native::picture`).
//! * **What is refused, in words:** a new shape that is not tier 1 (a cell, a section, a connection point, a row of geometry, a layer or a style link
//!   the reader would not rebuild from what is written), turned or flipped, or in a group. A new shape is written in the model's order, in the same save as
//!   a reorder and with `draw:z-index` on a page whose shapes carry it ([`super::plan`]).

mod asread;
mod segment;

pub(crate) use asread::entry_name_ok;

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Connect, Sheet};

/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use asread::type_agrees;
pub(crate) use asread::{AsRead, held_graphic_names};

use super::WriteError;
use super::attrs::TagEdit;
use super::compare::{GlueEdit, GlueTo, NewShape, PageDiff};
use super::frame::Editor;
use super::geometry::new_outline;
use super::glue::ModelPoints;
use super::kept::{Need, need_of};
use super::lengths::length_text;
use super::own::{self, POSE_CELLS};
use super::picture::{WORDS_GLUE, WORDS_NOT_DRAWN, WORDS_PICTURE_FLIP, XLINK_URI, picture_type};
use super::route;
use super::scan::{Fam, ScanPage};
use super::splice::escape_value;
use super::stylemap::{CellVal, Op, StyleEdit, Val, ops_for, static_cell};
use super::styles::Written;
use crate::ext::{native, payload};
use crate::index::PageShapes;
use crate::read::Affine;
use crate::read::names::Ns;

fn refuse<T>(words: &'static str) -> Result<T, WriteError> {
    Err(WriteError::Unsupported(words))
}

/// The most markup of the file a new shape of the model path keeps and writes back, in bytes (a shape of a file holds a few hundred).
const MOST_KEPT_PER_SHAPE: usize = 1 << 20;

/// The words of the refusal of the markup of a file that a new shape keeps past [`MOST_KEPT_PER_SHAPE`].
const WORDS_KEPT_BIG: &str =
    "a new shape that keeps more than 1 MiB of the markup of the file it was read from (its attributes and the elements kept whole)";

/// What a new shape of the model path writes of the markup it kept of the file it was read from ([`Editor::kept_markup`]), made before its element and taken by the function that
/// writes it.
#[derive(Debug, Default)]
pub(crate) struct KeptMarkup {
    /// The attributes for its start tag, checked: the name as kept, its value, and the namespace its prefix stands for (`None` for a name with no prefix, which is in no namespace).
    attrs: Vec<(String, String, Option<String>)>,
    /// The declarations its start tag needs for them (prefix and URI): the place binds the prefix to another namespace than the one it was read in.
    declare: Vec<(String, String)>,
    /// The children kept whole, as text, each with the declarations it needs.
    body: String,
    /// The children the ODF 1.4 schema puts **before a shape's text** (found the glue points of 93 visits, and a title in 7, written after it, which the schema
    /// refuses): an accessible title and description (`svg:title`, `svg:desc`), the event listeners (`office:event-listeners`) and the glue points (`draw:glue-point`), in the order
    /// the file had them, which is the schema's in a file it takes. A frame's are in `after`.
    before: String,
    /// A picture frame's kept children that stood after the picture it shows in the file (`Placed::after` 1, which the reader gives a child after the shown one): another
    /// `draw:image` of the frame, written after the frame's own so that the pictures come in the order the file had them.
    behind: String,
    /// A frame's children that the standard puts after its content: its event listeners, its glue points, its title and description. A picture's own glue points go after
    /// its picture, and LibreOffice 26.8.2.2 drops one that comes before the picture (measured), gluing a connector that names it to the frame as a whole.
    after: String,
    /// A custom shape's own `draw:enhanced-geometry`, as text, while its stamp matches and its namespaces are known.
    geometry: Option<String>,
    /// The title and the description the shape has (its own, or its master's: `.odg` has no masters), already in `before` or `after` as `svg:title` and `svg:desc`; what the reader gives back as
    /// rows of the `User` section, for the prediction of what DiaCraft's data has to carry (`native::Native::alt`).
    alt: (String, String),
    /// A `draw:image` of the frame's kept children, written, is **the frame's own picture**: it names an entry of the model's that holds the picture's bytes (a carried object's replacement
    /// picture, which the model makes with the entry), so the frame needs no picture of its own ([`Editor::is_the_frames_picture`]). A kept image that is any other picture is another image of the frame.
    image: bool,
    /// Whether the rows of the shape are its own glue points, numbered from its first listed point (`diacraft:glue="own"`), for a shape that writes the geometry element its file held: `Some`
    /// exactly when that element is written ([`rows_are_own`]).
    glue_own: Option<bool>,
    /// The paragraphs a picture's `draw:image` held (the words LibreOffice draws on a picture, kept whole on its foreign data by the reader), as text, each with the declarations it
    /// needs: written back inside the `draw:image` the frame is written with.
    in_image: String,
    /// What it kept that is not written, for the note: attributes, and elements and other nodes.
    left_attrs: usize,
    left_elements: usize,
    /// What the shape held when it was read: the element it was read from, its style name, its units, its layer and its frame, which it is written back with
    /// while their stamps hold. `None` for a shape no `.odg` reader made.
    read: Option<AsRead>,
}

/// What a new custom shape a connector is glued to is, and what it keeps of its file's markup, found **before** the shapes are written: a connector names the glue point of
/// its target as the target's numbering says, and whether the target writes the geometry element its file held (which keeps the file's own glue ids) is known only once the markup is made.
/// The function that writes the shape takes this in place of making it again, so each is charged once.
struct Prepared {
    read: Option<AsRead>,
    refused: usize,
    kind: Kind,
    made: KeptMarkup,
}

/// The number of glue points a geometry element lists **that the reader makes rows of**: the pairs of numbers of its `draw:glue-points`, but none for a geometry the reader stops at before
/// it reads them (an extrusion or a fontwork, a view box with no width or height: [`crate::read::reads_no_glue_points`]). The shape then has the four standard rows and the points it lists
/// make no row, so the rows a file's `draw:glue-point` elements did not make are not "the listed points from row 0" (the rows were counted as if they were, the marker was
/// written and a connector's end went 4 on).
fn listed_glue_points(el: &diacraft_doc::RawElement) -> Listed {
    let value = |local: &str| el.attrs.iter().find(|(name, _)| local_of(name) == local).map(|(_, value)| value);
    if crate::read::reads_no_glue_points(value("extrusion"), value("text-path"), value("viewBox")) {
        return Listed { count: 0, shares: None };
    }
    let Some(points) = value("glue-points") else { return Listed { count: 0, shares: None } };
    let count = points.split_ascii_whitespace().count() / 2;
    // Where they are, as shares of the frame, when the element's view box and its numbers say so plainly (a point that is `$0` or an equation is no place here): what tells a shape whose
    // rows start with them from one that has the four standard rows first (`rows_are_own`).
    let shares = value("viewBox").and_then(|text| crate::read::geometry::view_box(text)?.ok()).and_then(|vb| {
        let numbers: Option<Vec<f64>> = points.split_ascii_whitespace().map(|t| t.parse::<f64>().ok().filter(|n| n.is_finite())).collect();
        let numbers = numbers?;
        (numbers.len() == 2 * count).then(|| numbers.as_chunks::<2>().0.iter().map(|[x, y]| vb.share(*x, *y)).collect())
    });
    Listed { count, shares }
}

/// The glue points a custom shape's geometry element lists that the reader makes rows of ([`listed_glue_points`]): how many, and where when the element says it plainly.
#[derive(Debug, Default, PartialEq)]
struct Listed {
    count: usize,
    shares: Option<Vec<(f64, f64)>>,
}

/// The geometry element `el` with each `draw:handle` given the attributes ODF 1.4 asks a handle for: **`draw:handle-position-x` and `-y`**, or, for a polar handle
/// (one that has `draw:handle-polar`), **`draw:handle-polar-pole-x` and `-y`** with `draw:handle-polar-radius` and `-angle`, from the two values of the deprecated `draw:handle-position`
/// and `draw:handle-polar` that ODF 1.2 and the programs that write it give (ODF 1.4 Part 3 §19.191 to §19.196). The package is written as ODF 1.4, and its schema
/// has a handle state those (the handles of 164 visits were refused); the deprecated attributes stay as they were, so a program that reads them reads the same handle.
/// `None` when no handle lacks them (the element is written as it is), and for a handle whose values are not two.
pub(crate) fn handles_of_1_4(el: &diacraft_doc::RawElement) -> Option<diacraft_doc::RawElement> {
    fn two(value: &str) -> Option<(&str, &str)> {
        let mut parts = value.split_ascii_whitespace();
        let (a, b) = (parts.next()?, parts.next()?);
        parts.next().is_none().then_some((a, b))
    }
    fn stated(handle: &diacraft_doc::RawElement) -> Option<diacraft_doc::RawAttrs> {
        let prefix = handle.name.split_once(':').map(|(p, _)| p)?;
        let get = |local: &str| handle.attrs.get(&format!("{prefix}:{local}"));
        let position = get("handle-position").and_then(two);
        let mut add: Vec<(String, String)> = Vec::new();
        match get("handle-polar").and_then(two) {
            Some((x, y)) if get("handle-polar-pole-x").is_none() && get("handle-polar-pole-y").is_none() => {
                add.push((format!("{prefix}:handle-polar-pole-x"), x.to_owned()));
                add.push((format!("{prefix}:handle-polar-pole-y"), y.to_owned()));
                if let Some((radius, angle)) = position {
                    if get("handle-polar-radius").is_none() {
                        add.push((format!("{prefix}:handle-polar-radius"), radius.to_owned()));
                    }
                    if get("handle-polar-angle").is_none() {
                        add.push((format!("{prefix}:handle-polar-angle"), angle.to_owned()));
                    }
                }
            }
            Some(_) => {}
            None => {
                let (x, y) = position?;
                if get("handle-position-x").is_some() || get("handle-position-y").is_some() {
                    return None;
                }
                add.push((format!("{prefix}:handle-position-x"), x.to_owned()));
                add.push((format!("{prefix}:handle-position-y"), y.to_owned()));
            }
        }
        if add.is_empty() {
            return None;
        }
        let mut attrs = handle.attrs.clone();
        for (k, v) in add {
            attrs.push(&k, &v);
        }
        Some(attrs)
    }
    let mut changed = false;
    let mut children = el.children.clone();
    for child in &mut children {
        let diacraft_doc::RawNode::Element(handle) = child else { continue };
        if local_of(&handle.name) != "handle" {
            continue;
        }
        if let Some(attrs) = stated(handle) {
            handle.attrs = attrs;
            changed = true;
        }
    }
    changed.then(|| diacraft_doc::RawElement { name: el.name.clone(), attrs: el.attrs.clone(), children })
}

/// Whether a shape's child of the namespace `uri` and the local name `local` is one the ODF 1.4 schema puts before the shape's text (`common-draw-shape-accessibility`, then
/// `office-event-listeners`, then `draw-glue-point`, in `draw-rect`, `draw-custom-shape`, `draw-line` and the others; after the content in `draw-frame`).
pub(crate) fn goes_before_text(uri: Option<&str>, local: &str) -> bool {
    matches!(
        (uri, local),
        (Some(super::scan::SVG_URI), "title" | "desc")
            | (Some(super::scan::OFFICE_URI), "event-listeners")
            | (Some(super::scan::DRAW_URI), "glue-point")
    )
}

/// The ODF namespace the writer's own prefix `prefix` stands for on a new element, by the prefixes it writes it under.
fn writer_uri(pfx: &super::emit::Pfx, prefix: &str) -> Option<&'static str> {
    [
        (&pfx.draw, super::scan::DRAW_URI),
        (&pfx.svg, super::scan::SVG_URI),
        (&pfx.fo, super::scan::FO_URI),
        (&pfx.style, super::scan::STYLE_URI),
        (&pfx.text, super::scan::TEXT_URI),
    ]
    .into_iter()
    .find(|(p, _)| p.as_str() == prefix)
    .map(|(_, uri)| uri)
}

/// The local name of a name as written.
fn local_of(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// The words of the refusal of a connector glued to a shape whose id is empty (the review of the milestone-1 fixes): the reader reads `draw:end-shape=""`
/// as no shape, so the end would come back free.
const WORDS_ID_EMPTY: &str = "a connector glued to a shape whose id is empty (the file would read the glue as no shape's)";

/// The words of the refusal of a connector glued to a shape whose id an earlier shape of the page holds (the milestone-1 rerun's W2).
const WORDS_ID_HELD: &str =
    "a connector glued to a shape whose id an earlier shape of the page holds (the file would read it as glued to that shape)";

/// The number in an id of the form `id` and digits (`id12`): the ids the writer makes are of that form, and a new one is given the number above the largest the file holds.
/// An id with more than 18 digits, or with none, has no number (the part scan of a file has the same rule: [`super::scan`]).
pub(crate) fn id_number(id: &str) -> Option<u64> {
    id.strip_prefix("id")
        .filter(|digits| !digits.is_empty() && digits.len() <= 18 && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

/// The name the reader gives a shape of the file as the model writer writes it: its `xml:id`, else its `draw:id`, as it was read. A shape that holds neither has none.
///
/// **A `draw:id` that stands alone is written beside an `xml:id` of the same value** in a part of ODF 1.2 or later, which is every part the model writer makes: the schema of ODF 1.2 and later
/// takes `draw:id` only there (a file of ODF 1.0 or 1.1 has no `xml:id` to say it with, so a visit of one raised it to 1.4 with the `draw:id` alone). Both are an NCName in every version, and
/// **a value that is none is no id**: it is written in neither attribute, the shape has no name, and a connector glued to it is given one of the writer's own. A part that declares ODF 1.0 or
/// 1.1 (`legacy`) says `draw:id` as the shape held it.
fn held_name(shape: &diacraft_doc::Shape, legacy: bool) -> Option<&str> {
    let raw = &shape.ext.get()?.odf.kept.raw;
    match (raw.get("xml:id"), raw.get("draw:id")) {
        (Some(xml), _) => Some(xml),
        (None, Some(draw)) if legacy || super::kept::is_ncname(draw) => Some(draw),
        _ => None,
    }
}

/// A shape's attributes as pairs of name and value.
type Attrs<'a> = Vec<(&'a str, &'a str)>;

/// The attributes a shape kept, **with the `xml:id` a `draw:id` that stands alone needs beside it** (see [`held_name`]): put before the `draw:id`, equal to it, as the splice
/// writes the pair, in a part of ODF 1.2 or later (`legacy` is false) and for a shape that holds no `xml:id` of its own. A `draw:id` that is no NCName is left out, and counted (the second
/// of the pair that is given back): it is written in neither attribute. The list is made by `try_reserve`.
fn with_paired_ids(raw: &diacraft_doc::RawAttrs, legacy: bool) -> Result<(Attrs<'_>, usize), WriteError> {
    let pair = !legacy && raw.get("xml:id").is_none();
    let mut held: Vec<(&str, &str)> = Vec::new();
    held.try_reserve(raw.len().saturating_add(1))
        .map_err(|_| WriteError::Limit { what: "memory for the attributes of a new shape", max: raw.len() as u64 })?;
    let mut left = 0usize;
    for (name, value) in raw.iter() {
        if name == "draw:id" && pair {
            if !super::kept::is_ncname(value) {
                left = left.saturating_add(1);
                continue;
            }
            held.push(("xml:id", value));
        }
        held.push((name, value));
    }
    Ok((held, left))
}

/// The words of the refusal of a new shape whose pin is not where its frame puts it.
pub(crate) const WORDS_PIN_DISAGREES: &str = "a new shape whose pin is not where its size and the place of its pin put the frame (its cells disagree, as when `Width` was set and `LocPinX`, which is half of it, was not calculated again): the file would read it with the frame's centre as its pin";

/// Cells a new connector may hold besides its pose and style: the route's kind and its triggers, which the reader gives every connector.
const CONNECTOR_CELLS: [&str; 4] = ["ShapeRouteStyle", "ConLineRouteExt", "BegTrigger", "EndTrigger"];

/// What the save wrote of a new shape, for the check of the bytes made: the attributes on its start tag, the paragraphs with the text each reads as,
/// and the attributes on its `draw:enhanced-geometry`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewWrote {
    pub attrs: Vec<(Ns, &'static str)>,
    pub paras: Vec<(usize, String)>,
    pub kid: Vec<(Ns, &'static str)>,
    /// The outline written is a placeholder rectangle, because the shape's rows are not ones the writer writes as a path: the shape's `Geometry` is tier 2.
    pub outline_approximated: bool,
    /// What the shape carries as tier 2, when it carries any: the check of the bytes made reads the file back and compares what a shape carries with what was
    /// written (and nothing of what is written natively: the file's own markup is checked by the checks of its own).
    pub tier2: Option<std::sync::Arc<payload::Tier2>>,
}

/// A new shape's paragraphs: their text as the file holds it, whether the shape holds text, what each paragraph reads as, and the plain characters.
type Paragraphs = (String, bool, Vec<(usize, String)>, String, bool);

/// What the writer knows of the page's new shapes when it writes one: the shapes that are named by glue or by a formula (they keep their `ID`s), the
/// `ID`s of all the new shapes, and the index of the shapes of the file.
struct TierCtx<'a> {
    referenced: &'a HashSet<u32>,
    new_ids: &'a HashSet<u32>,
    held: &'a PageShapes,
}

/// The cells of a connector the reader rebuilds from the file's own glue: their formulas name the shapes by the `ID`s the reader gives.
const GLUE_CELLS: [&str; 6] = ["BeginX", "BeginY", "EndX", "EndY", "BegTrigger", "EndTrigger"];

/// The outline written for a shape whose rows are not ones the writer writes as a path: a rectangle on the frame, which the shape's `Geometry`, carried
/// as tier 2, replaces when the file is read by DiaCraft.
const PLACEHOLDER_PATH: &str = "M 0 0 L 21600 0 21600 21600 0 21600 Z N";

/// Whether the `Geometry` of a line is what the reader gives a `draw:line`: one section with a move to the origin and a line along the frame.
fn is_line_outline(sheet: &Sheet) -> bool {
    let sections: Vec<&std::sync::Arc<diacraft_doc::Section>> = sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").collect();
    let [section] = sections.as_slice() else { return false };
    if section.ix != Some(0) || !section.cells.is_empty() || section.rows.len() != 2 {
        return false;
    }
    let number = |row: &diacraft_doc::Row, cell: &str| {
        row.cells.get(cell).and_then(|c| match c.formula {
            diacraft_doc::FormulaSlot::Absent => c.value.as_ref()?.number(),
            _ => None,
        })
    };
    let (Some(a), Some(b)) = (section.rows.first(), section.rows.get(1)) else { return false };
    let formula = |row: &diacraft_doc::Row, cell: &str| row.cells.get(cell).and_then(|c| c.formula.text()).map(str::to_owned);
    a.row_type.as_deref() == Some("MoveTo")
        && b.row_type.as_deref() == Some("LineTo")
        && number(a, "X") == Some(0.0)
        && number(a, "Y") == Some(0.0)
        && formula(b, "X").as_deref() == Some("Width*1")
        && number(b, "Y") == Some(0.0)
        && a.cells.len() == 2
        && b.cells.len() == 2
}

/// The turn of a new 2-D shape (radians, counterclockwise, in (-pi, pi]) and its mirrors (`FlipX`, `FlipY`), from its pose. The linear part of a pose is the
/// turn after the flips (`own::local_to_parent`), so with the flips undone it is the turn alone; what is neither (a scale or a shear) is refused in words.
fn turn_and_flips(sheet: &Sheet, pose: &own::Pose) -> Result<(f64, (bool, bool)), WriteError> {
    let words = WriteError::Unsupported("a new shape whose frame is not numbers");
    let flips = own::flips_of(sheet).ok_or(words)?;
    let angle = own::turn_of(&own::unflipped(pose, flips)).ok_or(WriteError::Unsupported("a new shape that is scaled or sheared"))?;
    Ok((angle, (flips.0 == -1.0, flips.1 == -1.0)))
}

/// The text of a new element and what was written in it.
type Element = (String, NewWrote);

/// A shape a connector names: an element of the file, by its ordinal, or one this save makes, by its place among the new shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target {
    Ord(u32),
    New(usize),
}

/// The names the shapes connectors are glued to are named by on one page, made as the connectors ask for them (`Editor::name_target`): by shape, the name each is named by; the shapes of the
/// file that had none and are given one (their element's ordinal and the id); the first holder of each name among the file's shapes and among the new ones, indexed when the first
/// connector asks; and the number the last id given has.
#[derive(Default)]
struct Names<'n> {
    ids: HashMap<Target, String>,
    to_write: Vec<(u32, String)>,
    holders: Option<HashMap<String, u32>>,
    new_holders: Option<HashMap<&'n str, usize>>,
    next_id: u64,
}

/// The glue id ODF gives the row `row` of a shape: the four side middles map as DiaCraft's basic shapes order them, and beside exactly one flip of a shape
/// whose standard points LibreOffice numbers on the frame it draws (`flipped`: `read::connectors::glue_row_beside`), rows 1 and 3 are its points 1 and 3.
pub(crate) fn glue_id(row: u32, flipped: bool) -> u32 {
    crate::read::glue_row_beside(row, flipped)
}

/// What a new connector asks of the shapes it is glued to, by `ID`: the id it names each by, and the glue id of a row of each ([`glue_id`], or the row four on for a
/// shape whose rows are its own glue points).
type Targets<'a> = (&'a dyn Fn(u32) -> Option<String>, &'a dyn Fn(u32, u32) -> Option<u32>);

/// Whether a new 2-D shape is written with its connection points as its geometry's own glue points: a custom shape whose rows are not
/// the four side middles the reader gives every shape, and that [`super::geometry::glue_points_text`] can write.
fn writes_own_glue(sheet: &Sheet) -> bool {
    !standard_connection(sheet) && super::geometry::glue_points_text(sheet).is_some()
}

/// Whether the rows of a new custom shape are numbered from its first listed glue point, so that glue id *r* + 4 is row *r* and the shape's style says so (`diacraft:glue="own"`).
/// `held` is the number of glue points the geometry element the file held lists (`Some` when the shape writes that element back as it was, which keeps the file's glue
/// ids), and `None` when the geometry is made from the rows.
///
/// **A shape that writes its file's geometry has the numbering its file had**: a custom shape of a file that lists *k* glue points has either the four standard rows and
/// the listed ones from row 4 (the reader's numbering: glue id *g* is row *g*) or, when its style said `own`, the listed ones from row 0. The rows apart from those a `draw:glue-point`
/// element of the file made tell which: the first are 4 + *k* and the second are *k*. (The writer used to number every shape whose rows it could write as its own, and named the end 4 on a shape
/// whose rows were 4 + *k* by 8: every visit moved it 4 more, past the shape's last point.) A shape that makes its geometry from the rows numbers them as its own when it writes them so.
fn rows_are_own(sheet: &Sheet, held: Option<&Listed>) -> bool {
    match held {
        Some(listed) => listed.count > 0 && (rows_beside_held_glue(sheet) == listed.count || listing_leads_the_rows(sheet, listed)),
        None => writes_own_glue(sheet),
    }
}

/// Whether the rows of a shape that writes the geometry its file held **start with the points that geometry lists**: the first rows are those points, in place, and the
/// rows 4 on are not (a shape whose rows are the four standard ones and then the listed points has them from row 4). Which a count cannot tell once the session added rows: a shape
/// of *k* listed points that was doubled has more rows than the *k* the count asks for, and was taken for one with the four standard rows first, its marker dropped and its connectors' glue
/// numbered as the other kind's. A shape with a row a `draw:glue-point` of the file made is judged by the count alone, as before, and so is one whose geometry says its points otherwise
/// than as plain numbers.
fn listing_leads_the_rows(sheet: &Sheet, listed: &Listed) -> bool {
    let Some(shares) = listed.shares.as_ref().filter(|shares| shares.len() == listed.count) else { return false };
    let Some(points) = super::geometry::glue_points_of(sheet) else { return false };
    if rows_beside_held_glue(sheet) != points.len() {
        return false;
    }
    let near = |p: f64, q: f64| (p - q).abs() <= 1e-6;
    let at = |from: usize| {
        points
            .get(from..from.saturating_add(listed.count))
            .is_some_and(|rows| rows.iter().zip(shares).all(|(p, (x, y))| near(p.x, *x) && near(p.y, *y)))
    };
    at(0) && !at(4)
}

/// Whether the connection rows of a shape the `.odg` reader made are **its own glue points**, listed from row 0 by its geometry (`diacraft:glue="own"`): the shape keeps the
/// `draw:enhanced-geometry` its file had, which lists *k* glue points, and the rows no `draw:glue-point` element made are *k* in number. Such a shape has no standard rows 0 to 3, so
/// the four standard points LibreOffice puts a connector glued to the whole of the shape on are not its rows (the engine's takes the middles of the frame's sides). The
/// answer is the writer's own rule for the shape ([`rows_are_own`]): the count of the rows no element made, and, once a doubling has put the count out, the places the geometry lists ( a halving that leaves fewer rows than the geometry lists still puts it out until the file is read again).
pub fn rows_are_listed_glue(shape: &diacraft_doc::Shape) -> bool {
    let Some(el) = super::held::enhanced_geometry(shape, |e| local_of(&e.name) == "enhanced-geometry") else { return false };
    rows_are_own(&shape.sheet, Some(&listed_glue_points(el)))
}

/// How many rows of the `Connection` section a `draw:glue-point` element of the file did not make (the row has no held element, or the element's stamp is not the row's now).
fn rows_beside_held_glue(sheet: &Sheet) -> usize {
    let mut count = 0usize;
    for section in sheet.sections.iter().filter(|s| s.name.as_str() == "Connection") {
        for row in &section.rows {
            let made_by_element = row
                .ext
                .get()
                .and_then(|e| e.odf.held.as_ref())
                .is_some_and(|h| h.group == diacraft_doc::HeldGroup::Glue && h.stamp == Some(diacraft_doc::stamp_row(row)));
            if !made_by_element {
                count = count.saturating_add(1);
            }
        }
    }
    count
}

/// The scan's kinds of element whose four standard glue points LibreOffice numbers on the frame it draws (`read::shapes::glue_flipped`).
fn glue_on_frame(kind: super::scan::Kind) -> bool {
    use super::scan::Kind as K;
    matches!(kind, K::Custom | K::Rect | K::Ellipse | K::Circle | K::Frame)
}

/// Whether the sheet's `Connection` rows are DiaCraft's four side middles: then they come back as they are.
fn standard_connection(sheet: &Sheet) -> bool {
    sheet.sections.iter().find(|s| s.name.as_str() == "Connection").is_some_and(|section| standard_rows(&section.rows))
}

/// Whether the sheet's `Connection` rows come back as they are: DiaCraft's four side middles ([`standard_connection`]), or, **on the model path**, the four followed by rows that a
/// `draw:glue-point` of the file made and that are unchanged since (the element held beside each row has the row's stamp), which the model path writes back with the shape's kept markup
/// ([`Editor::kept_markup`]) and the reader reads again as the same rows with the same elements held (such a section was carried whole in tier 2, and the rows read back
/// from it had lost their elements, which the check after the save named as a connection that differs).
fn rebuilt_connection(sheet: &Sheet, model_path: bool) -> bool {
    if standard_connection(sheet) {
        return true;
    }
    let Some(section) = sheet.sections.iter().find(|s| s.name.as_str() == "Connection").filter(|_| model_path) else { return false };
    let (standard, held) = section.rows.split_at(section.rows.len().min(4));
    standard_rows(standard) && crate::ext::docfile::held_glue_rows(held)
}

/// Whether `rows` are DiaCraft's four side middles, in order.
fn standard_rows(rows: &[std::sync::Arc<diacraft_doc::Row>]) -> bool {
    let shares = [(0.5, 1.0), (0.0, 0.5), (0.5, 0.0), (1.0, 0.5)];
    if rows.len() != 4 {
        return false;
    }
    let share = |row: &diacraft_doc::Row, cell: &str, axis: &str| -> Option<f64> {
        let c = row.cells.get(cell)?;
        match c.formula.text() {
            None => c.value.as_ref()?.number().filter(|n| *n == 0.0).map(|_| 0.0),
            Some(f) if f == axis => Some(1.0),
            Some(f) => f.strip_prefix(axis)?.strip_prefix('*')?.parse::<f64>().ok(),
        }
    };
    // The reader's rows hold the directions a connector leaves by and `Type` 0 as well, and nothing else: rows that hold other
    // cells, or other directions, are not what the reader rebuilds. A row that says none of the three is the shares alone (the engine's new shapes, as before
    // this task): the reader gives it the standard ones, which is what the model's silence means.
    let directions = [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
    let constant = |row: &diacraft_doc::Row, cell: &str, want: f64| {
        row.cells.get(cell).is_some_and(|c| {
            matches!(c.formula, diacraft_doc::FormulaSlot::Absent)
                && c.extra.is_none()
                && c.value.as_ref().and_then(diacraft_doc::Value::number) == Some(want)
        })
    };
    rows.iter().zip(shares).zip(directions).all(|((row, (x, y)), (dx, dy))| {
        share(row, "X", "Width") == Some(x)
            && share(row, "Y", "Height") == Some(y)
            && (!row.cells.contains_key("DirX") || constant(row, "DirX", dx))
            && (!row.cells.contains_key("DirY") || constant(row, "DirY", dy))
            && (!row.cells.contains_key("Type") || constant(row, "Type", 0.0))
            && row.cells.keys().all(|name| matches!(name.as_str(), "X" | "Y" | "DirX" | "DirY" | "Type"))
    })
}

/// Sets the numbers a line end's attributes are worked out from (`own::decide_line_end`), **as the file will hold them**: the width of a marker is the mark's spread times a size
/// that is the class's base plus the line's weight, and the reader works the class back from the width and the weight *the file* gives the line. A line weight that is not written
/// natively (a formula, the cell of a style sheet, any value no attribute says) is tier 2, so the file's line has the weight its style gives and not the model's: the marker is made
/// for that weight, or the class read back is another (the review: a size of 4 beside a line weight that was a formula read back as 2).
fn line_end_numbers(edit: &mut StyleEdit, sheet: &Sheet) {
    own::decide_line_end(edit, sheet, |_| None);
    if !edit.changes.iter().any(|(c, _)| *c == "LineWeight") {
        edit.line.retain(|(name, _)| *name != "LineWeight");
    }
    // The same for the cap, which the form of a dash follows: a cap the file does not state is the round one for the dash's name, as the reader's is.
    if !edit.changes.iter().any(|(c, _)| *c == "LineCap") {
        edit.line.retain(|(name, _)| *name != "LineCap");
    }
}

/// Whether the style cell `name` is a line end's size that no end stands beside in `sheet`: the end's cell is none, or is the number 0 (no end). The file's markers cannot say
/// such a size, so it is not a cell written natively.
fn sizes_nothing(sheet: &Sheet, name: &str) -> bool {
    let end = match name {
        "BeginArrowSize" => "BeginArrow",
        "EndArrowSize" => "EndArrow",
        _ => return false,
    };
    sheet.cell(end).and_then(|c| c.value.as_ref()?.number()).is_none_or(|n| n == 0.0)
}

/// What the writer writes natively of a new shape's style cells: the edit, the attributes it comes to, and the names of the cells written. **A cell the
/// writer cannot say as a style attribute is left out and is tier 2** (a formula, a pattern of 2 or more, a colour in capitals, a cell no attribute stands
/// for), and so is every cell of the group it is written with (the four margins, the line ends) when one of them cannot be written.
#[cfg(test)]
fn native_style(sheet: &Sheet, allowed: &[&str]) -> (StyleEdit, Vec<Op>, Vec<&'static str>) {
    native_style_as(sheet, allowed, false, true)
}

/// [`native_style`], for a shape an `.odg` reader made when `read` (its colours are in capitals: [`native::style_reproduced_read`]). **With `fx`, a gradient or a hatch the cells say is
/// written as a display form of DiaCraft's own** ([`super::fx`]): its attributes come last, and the cells that stand for it are written or carried as it says.
pub(crate) fn native_style_as(sheet: &Sheet, allowed: &[&str], read: bool, fx: bool) -> (StyleEdit, Vec<Op>, Vec<&'static str>) {
    let mut edit = StyleEdit { fx: if fx { super::fx::fill_of(sheet) } else { None }, ..StyleEdit::default() };
    // The line ends that are the custom end (254): number 4's marker is written for each, and the cell is tier 2, so it is no cell written natively.
    let mut custom: Vec<&'static str> = Vec::new();
    for (name, cell) in &sheet.cells {
        let n = name.as_str();
        if POSE_CELLS.contains(&n) || allowed.contains(&n) {
            continue;
        }
        let Some(c) = static_cell(n) else { continue };
        if super::stylemap::is_custom_end(n, cell) {
            edit.changes.push((c, Some(CellVal::stand_in())));
            custom.push(c);
            continue;
        }
        let reproduced = if read { native::style_reproduced_read(n, cell) } else { native::style_reproduced(n, cell) };
        // A cell with a formula of its own is written with the value it was worked out to, and its formula rides beside it (`native::rebuilt` gives the reader's constant for it).
        let value = if reproduced { CellVal::of(cell).ok() } else { CellVal::cached(cell) };
        let Some(value) = value else { continue };
        edit.changes.push((c, Some(value)));
    }
    for (name, _) in super::stylemap::MARGINS {
        // Every margin cell the sheet has was taken as a change above, and the four are written together by the first of them.
        if let (Some(c), Some(cell)) = (static_cell(name), sheet.cells.get(name))
            && let Ok(value) = CellVal::of(cell)
        {
            edit.margins.push((c, Some(value)));
        }
    }
    // A size beside no end is no attribute: the markers say how big an end is, and a file with no end says nothing of its size, so the reader gives the cell back as none. It is
    // tier 2, as every cell the markup cannot say is (the review: it was counted as written and dropped without a word).
    edit.changes.retain(|(c, _)| !sizes_nothing(sheet, c));
    // The shadow is visible in the file when the cell that says so is written natively as 1: only then does the reader give `ShapeShdwType` 1 (`stylemap`).
    edit.shadow_visible = Some(edit.changes.iter().any(|(c, v)| *c == "ShdwPattern" && v.as_ref().and_then(|v| v.number) == Some(1.0)));
    // A new shape holds every cell it has (the engine's save gives it constants), so a line end is worked out from its own cells.
    line_end_numbers(&mut edit, sheet);
    let mut ops: Vec<Op>;
    loop {
        ops = Vec::new();
        let mut failed: Option<&'static str> = None;
        for (cell, value) in &edit.changes {
            match ops_for(cell, value.as_ref(), &edit) {
                Ok(more) => ops.extend(more),
                Err(_) => {
                    failed = Some(*cell);
                    break;
                }
            }
        }
        let Some(bad) = failed else { break };
        let group: &[&str] = if super::stylemap::MARGINS.iter().any(|(m, _)| *m == bad) {
            &["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"]
        } else if super::stylemap::ARROW_CELLS.contains(&bad) {
            &super::stylemap::ARROW_CELLS
        } else {
            &[]
        };
        edit.changes.retain(|(c, _)| *c != bad && !group.contains(c));
        if group.contains(&"TopMargin") {
            edit.margins.clear();
        }
        line_end_numbers(&mut edit, sheet);
    }
    // The display form of the fill, after the cells' own attributes: what it writes for an attribute takes the place of theirs.
    let said = edit.fx_ops();
    ops.retain(|o| !said.iter().any(|s| s.prop == o.prop && s.ns == o.ns && s.local == o.local));
    ops.extend(said);
    // The blur of a visible shadow, for the programs that draw it.
    if fx && let Some(blur) = super::fx::shadow_blur_op(sheet, &ops) {
        ops.push(blur);
    }
    let names = edit.changes.iter().map(|(c, _)| *c).filter(|c| !custom.contains(c)).collect();
    (edit, ops, names)
}

impl Editor<'_, '_> {
    /// The `style:name` of the style a new shape's automatic style names as its parent: the style sheet its three links name, by the name the
    /// file holds it under (`Editor::style_names`), and none for the root. A shape whose links name different style sheets is refused in words, as is one that names a style sheet
    /// the file cannot name (and a flat drawing has none to name: its styles are compared whole).
    fn parent_of(&self, sheet: &Sheet) -> Result<Option<String>, WriteError> {
        let [line, fill, text] = [sheet.links.line, sheet.links.fill, sheet.links.text].map(|l| l.unwrap_or(0));
        if (line, fill, text) == (0, 0, 0) {
            return Ok(None);
        }
        if fill != line || text != line {
            return refuse("a new shape whose links name different style sheets (the file says one parent)");
        }
        self.style_names.get(&line).cloned().map(Some).ok_or(WriteError::Unsupported("a new shape on a style sheet whose name the file cannot say"))
    }

    fn length(&self, inches: f64) -> Result<String, WriteError> {
        length_text(inches, self.unit).ok_or(WriteError::Unsupported("a length too large to write"))
    }

    /// The ids of the elements of the file that a connector names, read from the tag: `xml:id`, else `draw:id`, **by the name the reader gives them** (a literal tab or
    /// line break is a space, [`TagEdit::read_text`]), which is the name a reference must hold for the reader to find the shape.
    fn id_of(&mut self, scan: &ScanPage, ord: u32) -> Result<Option<String>, WriteError> {
        let shape = scan.shape(ord).ok_or(WriteError::Mismatch("a shape the connector names is not in the part"))?;
        let tag = TagEdit::new(self.bytes, shape.open, shape.scope, self.work)?;
        let at = tag.find(&mut self.scopes, Ns::Xml, "id", self.work).or_else(|| tag.find(&mut self.scopes, Ns::Draw, "id", self.work));
        Ok(at.map(|a| tag.read_text(a)))
    }

    /// The shapes a new connector is glued to at a point whose rows are their own glue points, so that row *r* is glue id *r* + 4 (`diacraft:glue="own"`): a new custom
    /// shape written so, and a shape of the file whose style says so (a drawing DiaCraft wrote); one look at the style of each shape of the file a new connector names. And the new custom
    /// shapes a connector is glued to, made ahead of their turn ([`Self::prepare_custom`]), by their place among the new shapes: whether one writes the geometry element its file
    /// held decides how its glue points are numbered, and the connector may stand before it in the page.
    fn glue_numbering(
        &mut self,
        scan: &ScanPage,
        news: &[NewShape],
        glued: &[(u32, u32)],
        target_of: &dyn Fn(u32) -> Result<Target, WriteError>,
    ) -> Result<(HashSet<u32>, HashMap<usize, Prepared>), WriteError> {
        let mut own_glue: HashSet<u32> = HashSet::new();
        let mut prepared: HashMap<usize, Prepared> = HashMap::new();
        let mut judged: HashSet<u32> = HashSet::new();
        // `glued` is each connection's shape and part (`ToPart`): a connection to a connection point (100 and the row) is numbered by the shape it goes to.
        for &(to_sheet, _) in glued.iter().filter(|(_, part)| *part >= 100) {
            if !judged.insert(to_sheet) {
                continue;
            }
            let own = match target_of(to_sheet)? {
                Target::New(i) => match news.get(i) {
                    Some(t) if t.shape.children.is_empty() && !is_new_picture(&t.shape) && t.shape.sheet.cell("BeginX").is_none() => {
                        let made = self.prepare_custom(t, scan)?;
                        // A shape written as the plain element it was read from has no glue points of its own to write.
                        let own = made.kind == Kind::Custom
                            && match (made.made.geometry.as_ref(), made.made.glue_own) {
                                (Some(_), Some(own)) => own,
                                _ => rows_are_own(&t.shape.sheet, None),
                            };
                        prepared.insert(i, made);
                        own
                    }
                    _ => false,
                },
                Target::Ord(o) => self.file_glue_own(scan, o)?,
            };
            if own {
                own_glue.insert(to_sheet);
            }
        }
        Ok((own_glue, prepared))
    }

    /// What a new custom shape held when an `.odg` reader made it and what it keeps of its file's markup, made ahead of its turn in [`Self::new_shapes`] for a shape a connector is glued
    /// to: the same two steps the loop takes for each shape ([`Self::as_read_kind`], [`Self::kept_markup`]), kept for the loop to take, so that each is charged once.
    fn prepare_custom(&mut self, n: &NewShape, scan: &ScanPage) -> Result<Prepared, WriteError> {
        let pfx = super::emit::Pfx::of(&mut self.scopes, scan.scope, self.work);
        let (read, refused, kind) = self.as_read_kind(n, Kind::Custom)?;
        let frame = read.as_ref().is_some_and(|r| r.element == "frame");
        let made = self.kept_markup(n, (kind, frame), scan, &pfx, read.as_ref())?;
        Ok(Prepared { read, refused, kind, made })
    }

    /// Whether the shape element `ord` of the file is a custom shape whose automatic style says its rows start at 0 with its first listed glue point (`diacraft:glue="own"`):
    /// a shape DiaCraft wrote so, read back. One look at its start tag and at its style's graphic properties.
    pub(super) fn file_glue_own(&mut self, scan: &ScanPage, ord: u32) -> Result<bool, WriteError> {
        let Some(shape) = scan.shape(ord) else { return Ok(false) };
        if shape.kind != super::scan::Kind::Custom {
            return Ok(false);
        }
        let tag = TagEdit::new(self.bytes, shape.open, shape.scope, self.work)?;
        let Some(at) = tag.find(&mut self.scopes, Ns::Draw, "style-name", self.work) else { return Ok(false) };
        let name = tag.text(at);
        let props = self.book.as_ref().and_then(|b| b.find_def(&name, Fam::Graphic).and_then(|i| b.defs.get(i))).and_then(|d| d.props[0]);
        let Some(props) = props else { return Ok(false) };
        let graphic = TagEdit::new(self.bytes, props.open, props.scope, self.work)?;
        Ok(graphic.find(&mut self.scopes, Ns::Diacraft, "glue", self.work).is_some_and(|a| graphic.text(a) == "own"))
    }

    /// The largest number among the ids of the form `id` and digits (`xml:id` and `draw:id`) that the shapes of the model hold, groups' members included: a walk of every page with a stack of its
    /// own, a step for each shape.
    fn largest_held_id(&mut self) -> Result<u64, WriteError> {
        let doc = self.doc;
        let mut best = 0u64;
        for page in doc.pages.iter() {
            let mut stack: Vec<&diacraft_doc::Shape> = Vec::new();
            stack
                .try_reserve(page.shapes.len())
                .map_err(|_| WriteError::Limit { what: "memory for the ids of a page's shapes", max: page.shapes.len() as u64 })?;
            stack.extend(page.shapes.iter().map(|s| &**s));
            while let Some(shape) = stack.pop() {
                self.work.charge_steps(1)?;
                if let Some(data) = shape.ext.get() {
                    for name in ["xml:id", "draw:id"] {
                        if let Some(number) = data.odf.kept.raw.get(name).and_then(id_number) {
                            best = best.max(number);
                        }
                    }
                }
                stack
                    .try_reserve(shape.children.len())
                    .map_err(|_| WriteError::Limit { what: "memory for the ids of a page's shapes", max: shape.children.len() as u64 })?;
                stack.extend(shape.children.iter().map(|c| &**c));
            }
        }
        Ok(best)
    }

    /// The name a connector gives the new shape `at` of `news`: **the id the shape holds** (the model path writes the ids a shape was read with), or a fresh `id<n>` above the
    /// largest (`next_id`, which only grows). A shape of the file keeps the id it was read with, and a connector names it by that. The reader resolves a name to the first shape of the page that
    /// holds it, and an empty name to none: a connector glued to a shape that is not the first holder of its id, or whose id is empty, would read back glued to another shape, or to none, and is
    /// refused in the words of the file's own shapes. `holders` is the first new shape that holds each name, made when the first connector is glued to a shape that holds one (a step for each
    /// shape).
    fn new_target_id<'n>(
        &mut self,
        news: &'n [NewShape],
        at: usize,
        holders: &mut Option<HashMap<&'n str, usize>>,
        next_id: &mut u64,
    ) -> Result<String, WriteError> {
        let legacy = self.legacy;
        let held = news.get(at).filter(|_| self.work.model_path).and_then(|t| held_name(&t.shape, legacy));
        let Some(name) = held else {
            *next_id = next_id.saturating_add(1);
            return Ok(format!("id{next_id}"));
        };
        if name.is_empty() {
            return refuse(WORDS_ID_EMPTY);
        }
        if holders.is_none() {
            self.work.charge_steps(news.len())?;
            let mut first: HashMap<&'n str, usize> = HashMap::new();
            first
                .try_reserve(news.len())
                .map_err(|_| WriteError::Limit { what: "memory for the ids of a page's new shapes", max: news.len() as u64 })?;
            for (place, t) in news.iter().enumerate() {
                if let Some(held) = held_name(&t.shape, legacy) {
                    first.entry(held).or_insert(place);
                }
            }
            *holders = Some(first);
        }
        if holders.as_ref().and_then(|first| first.get(name)).copied() != Some(at) {
            return refuse(WORDS_ID_HELD);
        }
        self.work.charge_copy(name.len(), self.limits)?;
        Ok(name.to_owned())
    }

    /// Names the shape `target`, once for the page: the name the reader finds it by. A shape of the file keeps its own (`xml:id`, else `draw:id`), and one that has none is given an id of the form
    /// `id` and a number above every one the part holds (`to_write` lists it, for its element is given the attributes). A shape that is not the first holder of its name on the page, or whose
    /// name is empty, is refused in words: the reader would read a connector glued to it as glued to the first holder, or to none. A new shape is named by [`Self::new_target_id`].
    fn name_target<'n>(&mut self, scan: &ScanPage, news: &'n [NewShape], target: Target, names: &mut Names<'n>) -> Result<(), WriteError> {
        if names.ids.contains_key(&target) {
            return Ok(());
        }
        let value = match target {
            Target::Ord(o) => match self.id_of(scan, o)? {
                // The reader resolves a name to the first shape of the page that holds it (`read::shapes::number`): a connector that names an id another
                // shape holds first would read back glued to that shape. No spec says what to write for such a shape, so the save is refused in words.
                // An id that is empty names no shape to the reader (`read::connectors::end_of`): refused in words.
                Some(existing) if existing.is_empty() => return refuse(WORDS_ID_EMPTY),
                Some(existing) if self.first_holder(scan, &existing, &mut names.holders)? != Some(o) => return refuse(WORDS_ID_HELD),
                Some(existing) => existing,
                None => {
                    names.next_id = names.next_id.saturating_add(1);
                    let made = format!("id{}", names.next_id);
                    names
                        .to_write
                        .try_reserve(1)
                        .map_err(|_| WriteError::Limit { what: "memory for the ids of a page's shapes", max: names.to_write.len() as u64 })?;
                    names.to_write.push((o, made.clone()));
                    made
                }
            },
            Target::New(i) => self.new_target_id(news, i, &mut names.new_holders, &mut names.next_id)?,
        };
        names.ids.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the names of a page's shapes", max: names.ids.len() as u64 })?;
        names.ids.insert(target, value);
        Ok(())
    }

    /// The ordinal of the first shape element of the page, in the reader's order, whose `xml:id` (else `draw:id`) is `id`: what the reader resolves a connector's
    /// reference to. The page's ids are read once, into `holders`, the first time one is asked (one look at each shape's start tag, counted as the others are).
    fn first_holder(&mut self, scan: &ScanPage, id: &str, holders: &mut Option<HashMap<String, u32>>) -> Result<Option<u32>, WriteError> {
        if holders.is_none() {
            let mut first: HashMap<String, u32> = HashMap::new();
            first
                .try_reserve(scan.shapes.len())
                .map_err(|_| WriteError::Limit { what: "memory for the ids of a page's shapes", max: scan.shapes.len() as u64 })?;
            for at in 0..scan.shapes.len() {
                let ord = u32::try_from(at + 1).unwrap_or(u32::MAX);
                if let Some(name) = self.id_of(scan, ord)? {
                    first.entry(name).or_insert(ord);
                }
            }
            *holders = Some(first);
        }
        Ok(holders.as_ref().and_then(|first| first.get(id).copied()))
    }

    /// Writes the new shapes of a page: their styles at the end of `office:automatic-styles` and the elements where `diff` puts them. Gives, for
    /// each new shape, the attributes written on its start tag, which the check of the bytes made reads back.
    ///
    /// **Nothing is put into the part here**: the text of each new element is given back with what was written in it, and the caller puts each where the
    /// page's order says (`plan::place_new`), in the order the model has them. `zs` is the `draw:z-index` each new shape is written with, when the page's
    /// shapes carry the attribute (ownership R8: written on a new top-level shape when the page's shapes carry it).
    pub(crate) fn new_shapes(
        &mut self,
        scan: &ScanPage,
        diff: &PageDiff,
        held: &PageShapes,
        zs: &[Option<i64>],
    ) -> Result<(Vec<String>, Vec<NewWrote>), WriteError> {
        let news = &diff.new;
        let new_at: HashMap<u32, usize> = news.iter().enumerate().map(|(i, n)| (n.shape.id, i)).collect();
        let mut by_connector: HashMap<u32, Vec<&Connect>> = HashMap::new();
        for c in &diff.new_connects {
            by_connector.entry(c.from_sheet).or_default().push(c);
        }
        // The segments of a geometry of their own that are written as the outline they draw, by the shape's place in the list: found before the elements are made. **Only a segment
        // that is glued at no end**: a polyline has no glue another program keeps, so a segment glued at an end, at one or at both, is a connector of the type `line`
        // ([`Editor::connector_text`]), whose glue is the element's own. A polyline has no glue, so the connections of the page are all the glue the elements are written with.
        let new_ids: HashSet<u32> = news.iter().map(|n| n.shape.id).collect();
        let polys = self.polys_of(news, diff, &by_connector)?;
        let target_of = |id: u32| -> Result<Target, WriteError> {
            match (new_at.get(&id), held.ordinal(id)) {
                (Some(i), _) => Ok(Target::New(*i)),
                (None, Some(o)) => Ok(Target::Ord(o)),
                _ => Err(WriteError::Unsupported("a connector glued to a shape the file does not hold")),
            }
        };
        // The glue of the connectors of the file that the model changed: the ends glued to a shape now, each as the connection the writer is to name.
        let glued_now: Vec<(GlueEdit, GlueTo)> = diff.glue.iter().filter_map(|e| e.to.map(|to| (*e, to))).collect();
        self.check_picture_glue(news, &new_at, &diff.new_connects, glued_now.iter().map(|(_, to)| *to))?;
        // The ids, in the order the connectors name their shapes: the begin of each, then its end.
        // **The model writer writes the ids a shape was read with** (`Editor::kept_markup` puts them on its tag as the file had them), so a new target is given an id above every
        // number the shapes of the model hold, found once for the save.
        if self.work.model_path && self.work.held_id_max.is_none() {
            let found = self.largest_held_id()?;
            self.work.held_id_max = Some(found);
            self.max_id = self.max_id.max(found);
        }
        let mut names = Names { next_id: self.max_id, ..Names::default() };
        for n in news.iter().filter(|n| n.shape.sheet.cell("BeginX").is_some()) {
            let mut glued: Vec<&Connect> = by_connector.get(&n.shape.id).cloned().unwrap_or_default();
            glued.sort_by_key(|c| c.from_cell.as_deref().is_some_and(|f| f.starts_with("End")));
            for c in glued {
                let target = target_of(c.to_sheet)?;
                self.name_target(scan, news, target, &mut names)?;
            }
        }
        // Then the shapes the connectors of the file are glued to now, in the order of the connectors and their ends.
        self.work.charge_steps(glued_now.len())?;
        for (_, to) in &glued_now {
            let target = target_of(to.target)?;
            self.name_target(scan, news, target, &mut names)?;
        }
        let Names { ids, to_write, .. } = names;
        for (ord, value) in to_write {
            let shape = scan.shape(ord).ok_or(WriteError::Mismatch("a shape the connector names is not in the part"))?;
            let mut tag = TagEdit::new(self.bytes, shape.open, shape.scope, self.work)?;
            self.work.charge_attribute(self.limits)?;
            // A part of ODF 1.0 or 1.1 names a shape by `draw:id` alone: `xml:id` is 1.2's.
            if !self.legacy {
                self.work.charge_attribute(self.limits)?;
                tag.add_xml_id(&mut self.splicer, &value)?;
            }
            tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "id", &value)?;
        }
        // The shapes glued to whose rows are their own glue points, so that row r is glue id r + 4 (`diacraft:glue="own"`), and the new custom shapes made ahead of
        // their turn for it (see [`Editor::glue_numbering`]; the loop below takes them).
        let glued_parts: Vec<(u32, u32)> = diff
            .new_connects
            .iter()
            .filter_map(|c| Some((c.to_sheet, c.to_part?)))
            .chain(glued_now.iter().map(|(_, to)| (to.target, to.part)))
            .collect();
        let (own_glue, mut prepared) = self.glue_numbering(scan, news, &glued_parts, &target_of)?;
        // The shapes another shape's formula or glue names keep their `ID`s in the file: the glue of the new connectors and the
        // formulas of the new shapes, but the cells of a glued end, which the reader rebuilds from the file's own glue whatever the model's `ID`s were.
        let mut referenced: HashSet<u32> = HashSet::new();
        // The new shapes the page's comments are on keep their `ID`s too: a comment names its shape by it.
        referenced.extend(diff.tied.iter().copied());
        for c in &diff.new_connects {
            // The shape a connector is glued to is named by it; the connector, which names, is not named.
            referenced.insert(c.to_sheet);
        }
        referenced.extend(glued_now.iter().map(|(_, to)| to.target));
        for n in news {
            self.work.charge_compared(self.limits)?;
            referenced.extend(native::named_sheets(&n.shape.sheet, &GLUE_CELLS));
        }
        // The members of each new group, by the group's place in the list, made once: a group finds its members in its own list and does not look at every new shape
        // (review: 40,000 one-member groups took 34 s, 60,000 over 75).
        let too_many = || WriteError::Limit { what: "memory for the new shapes of a page", max: news.len() as u64 };
        let mut members_of: Vec<Vec<usize>> = Vec::new();
        members_of.try_reserve_exact(news.len()).map_err(|_| too_many())?;
        members_of.resize_with(news.len(), Vec::new);
        for (m, n) in news.iter().enumerate() {
            if let Some(list) = n.parent.and_then(|p| members_of.get_mut(p)) {
                list.try_reserve(1).map_err(|_| too_many())?;
                list.push(m);
                self.work.group_members = self.work.group_members.saturating_add(1);
            }
        }
        // What the members of each new group take up in its frame, to the leaves: found once for the page (a pass over its new shapes), where a group's frame is judged.
        let inside = super::groups::spans_inside(news, &mut self.work.group_boxes)?;
        let tc = TierCtx { referenced: &referenced, new_ids: &new_ids, held };
        // A shape glued to whose standard points are numbered the other way round: one with exactly one flip, of a kind the reader numbers so (a new 2-D shape is written as a custom shape).
        let flipped = |c: u32| {
            diff.one_flip.contains(&c)
                && match target_of(c) {
                    Ok(Target::New(i)) => news
                        .get(i)
                        .is_some_and(|t| t.shape.children.is_empty() && !is_new_picture(&t.shape) && t.shape.sheet.cell("BeginX").is_none()),
                    Ok(Target::Ord(o)) => scan.shape(o).is_some_and(|t| glue_on_frame(t.kind)),
                    Err(_) => false,
                }
        };
        let names = |c: u32| target_of(c).ok().and_then(|t| ids.get(&t).cloned());
        let glue_of = |c: u32, row: u32| if own_glue.contains(&c) { row.checked_add(4) } else { Some(glue_id(row, flipped(c))) };
        // **The glue of the connectors of the file**: each end that is glued to a shape now names it, and the point of it, as a new connector's end does.
        for (edit, to) in &glued_now {
            let shape = scan.shape(edit.ord).ok_or(WriteError::Mismatch("a connector the model glued is not in the part"))?;
            let name = names(to.target).ok_or(WriteError::Unsupported("a connector glued to a shape the writer cannot name"))?;
            let point = match to.part {
                3 => None,
                part if part >= 100 => Some(
                    glue_of(to.target, part - 100)
                        .ok_or(WriteError::Unsupported("a connector glued to a connection point past the ones a shape can hold"))?,
                ),
                _ => return refuse("a connection of a kind the writer does not know"),
            };
            self.write_glue_end(shape, edit.start, &name, point)?;
        }
        let mut texts: Vec<String> = Vec::new();
        texts.try_reserve_exact(news.len()).map_err(|_| WriteError::Limit { what: "memory for the new shapes of a page", max: news.len() as u64 })?;
        let mut wrote_all = Vec::with_capacity(news.len());
        for (k, n) in news.iter().enumerate() {
            // The element is a child of the page, whatever it follows: its prefixes are those the page binds, and not those a shape it follows
            // declares on its own start tag.
            let pfx = super::emit::Pfx::of(&mut self.scopes, scan.scope, self.work);
            // A shape that holds the ids it was read with writes them as they were (`kept_markup`), and is not given one.
            let id = if self.work.model_path && held_name(&n.shape, self.legacy).is_some() { None } else { ids.get(&Target::New(k)).cloned() };
            let z = zs.get(k).copied().flatten();
            let sheet = &*n.shape.sheet;
            // A 1-D shape that has a route (a connector's route cells), or that is glued to a shape (`connector.add` makes one with neither route cell), is a
            // connector; one that is neither is a line, drawn with the line tool.
            // A new picture: a frame holding the picture, inline in a flat drawing and as a new entry of a package.
            let kind = if !n.shape.children.is_empty() {
                Kind::Group
            } else if is_new_picture(&n.shape) {
                Kind::Picture
            } else if sheet.cell("BeginX").is_none() {
                Kind::Custom
            } else if polys.get(k).is_some_and(Option::is_some) {
                Kind::Poly
            } else if sheet.cell("ShapeRouteStyle").is_none() && sheet.cell("ConLineRouteExt").is_none() && !by_connector.contains_key(&n.shape.id) {
                Kind::Line
            } else {
                Kind::Connector
            };
            // What the shape held when an `.odg` reader made it: a shape whose outline is its own element's, and as it was read, is written as that element. And the markup
            // of the file the shape keeps (the model path writes it back; see `kept_markup`). A shape a connector is glued to had both made already (see `prepared`).
            let (read, refused, kind, made) = match prepared.remove(&k) {
                Some(p) => (p.read, p.refused, p.kind, p.made),
                None => {
                    let (read, refused, kind) = self.as_read_kind(n, kind)?;
                    let frame = kind == Kind::Picture || read.as_ref().is_some_and(|r| r.element == "frame");
                    let made = self.kept_markup(n, (kind, frame), scan, &pfx, read.as_ref())?;
                    (read, refused, kind, made)
                }
            };
            self.kept = made;
            self.kept.read = read;
            self.kept.left_attrs = self.kept.left_attrs.saturating_add(refused);
            self.rich = self.rich_text_of(n, kind, scan, &pfx, diff.page_at)?;
            let (text, wrote) = match kind {
                Kind::Group => {
                    let own = members_of.get(k).map_or(&[][..], Vec::as_slice);
                    self.work.group_members = self.work.group_members.saturating_add(own.len() as u64);
                    // A group whose frame is not the box of its members is written as its members all the same, and says so.
                    let inside = inside.get(k).copied().unwrap_or(super::groups::Span::Empty);
                    if !super::groups::check_frame(n, inside)? {
                        let (glued, glued_total) = super::groups::glued_to(&diff.glued_to_groups, n.shape.id);
                        self.notes.push(crate::SaveNote::GroupFrameNotKept { page: diff.page_at, shape: n.shape.id, glued, glued_total });
                    }
                    self.group_open(n, &pfx, id.as_deref(), z)?
                }
                Kind::Line => self.line_text(n, diff, &pfx, id.as_deref(), z, &tc)?,
                Kind::Connector => self.connector_text(
                    n,
                    diff,
                    &pfx,
                    by_connector.get(&n.shape.id).map(Vec::as_slice).unwrap_or_default(),
                    (id.as_deref(), z),
                    &tc,
                    (&names, &glue_of),
                )?,
                Kind::Custom => self.custom_text(n, diff, &pfx, id.as_deref(), z, &tc)?,
                Kind::Poly => {
                    let placed =
                        polys.get(k).and_then(Option::as_ref).ok_or(WriteError::Mismatch("a segment written as its outline that was not placed"))?;
                    self.poly_text(n, diff, &pfx, id.as_deref(), z, &tc, placed)?
                }
                Kind::Picture => self.picture_text(n, diff, &pfx, id.as_deref(), z, &tc)?,
                Kind::Plain => {
                    let read = self.kept.read.take().ok_or(WriteError::Mismatch("a plain shape that holds nothing it was read with"))?;
                    self.plain_text(n, diff, &pfx, (id.as_deref(), z), &tc, &read)?
                }
            };
            if wrote.outline_approximated {
                self.notes.push(crate::SaveNote::OutlineApproximated { page: diff.page_at, shape: n.shape.id });
            }
            // What the shape held that no element of this writer's can hold is said, and not dropped without a word.
            let left = self.work.model_path.then_some((self.kept.left_attrs, self.kept.left_elements));
            self.notes.extend(unwritten(diff.page_at, &n.shape, kind, left));
            self.work.charge_copy(text.len(), self.limits)?;
            texts.push(text);
            wrote_all.push(wrote);
        }
        // A new group's element holds its members: the members come after it in the list, so from the last to the first each group takes the text of its own members
        // (which are made by then, a member that is a group having taken its own), and they have none of their own to put.
        for (k, n) in news.iter().enumerate().rev() {
            if n.shape.children.is_empty() {
                continue;
            }
            let mut inside = String::new();
            let own = members_of.get(k).map_or(&[][..], Vec::as_slice);
            self.work.group_members = self.work.group_members.saturating_add(own.len() as u64);
            for &m in own {
                inside.push_str(&std::mem::take(texts.get_mut(m).ok_or(WriteError::Mismatch("a new group has a member that is not made"))?));
            }
            let open = std::mem::take(texts.get_mut(k).ok_or(WriteError::Mismatch("a new group is not made"))?);
            let pfx = super::emit::Pfx::of(&mut self.scopes, scan.scope, self.work);
            // A group's title and description are its first children, before its members; its rows are carried by the document file's entry of it.
            let (alt, _) = self.alt_elements(n, &pfx)?;
            let whole = format!("{open}{alt}{inside}</{}:g>", pfx.draw);
            self.work.charge_copy(whole.len().saturating_sub(inside.len()), self.limits)?;
            if let Some(slot) = texts.get_mut(k) {
                *slot = whole;
            }
        }
        Ok((texts, wrote_all))
    }

    /// The new shapes written as the outline they draw ([`Self::poly_of`]), by the shape's place in the list: **none for a segment glued at an end** (a polyline
    /// has no glue another program keeps, so it is a connector, [`Self::connector_text`]).
    fn polys_of(
        &mut self,
        news: &[NewShape],
        diff: &PageDiff,
        by_connector: &HashMap<u32, Vec<&Connect>>,
    ) -> Result<Vec<Option<super::poly::Placed>>, WriteError> {
        let mut polys: Vec<Option<super::poly::Placed>> = Vec::new();
        polys.try_reserve_exact(news.len()).map_err(|_| WriteError::Limit { what: "memory for the new shapes of a page", max: news.len() as u64 })?;
        for n in news {
            let glued = by_connector.get(&n.shape.id).is_some_and(|own| !own.is_empty());
            polys.push(if glued { None } else { self.poly_of(n, diff)? });
        }
        Ok(polys)
    }

    /// The route of a new one-dimensional shape **of a geometry of its own**, on the page ([`super::poly`]); `None` for any other shape. **Only a segment that is not routed**: its route
    /// cells are none, or the fixed geometry's (`ShapeRouteStyle` 16, the style the master of such a figure holds, which a reroute leaves alone, with `ConLineRouteExt` beside it 1 when there is
    /// one: the appearance the engine gives it); that is no group and no picture; and whose geometry is more than the plain line between its ends and is lines and cubic curves of one section (the
    /// points [`super::poly::segs_of`] takes). It may be a member of a group (its points are put on the page by the groups around it). The rows looked at are charged.
    fn own_route(&mut self, n: &NewShape) -> Result<Option<Vec<super::route::Seg>>, WriteError> {
        let sheet = &*n.shape.sheet;
        let number = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
        // A route cell is no mark of a fixed figure unless it is the fixed geometry's own number (`ConLineRouteExt` beside it, when there is one, is the appearance the engine gives it).
        let fixed_or_unrouted = match (sheet.cell("ShapeRouteStyle"), sheet.cell("ConLineRouteExt")) {
            (None, None) => true,
            (Some(_), other) => number("ShapeRouteStyle") == Some(16.0) && other.is_none_or(|_| number("ConLineRouteExt") == Some(1.0)),
            (None, Some(_)) => false,
        };
        if !fixed_or_unrouted || sheet.cell("BeginX").is_none() || !n.shape.children.is_empty() || is_new_picture(&n.shape) || is_line_outline(sheet)
        {
            return Ok(None);
        }
        let mut steps: u64 = 0;
        let route = super::poly::segs_of(sheet, &n.outer, &mut steps);
        self.work.charge_steps(usize::try_from(steps).unwrap_or(usize::MAX))?;
        Ok(route)
    }

    /// Where a new one-dimensional shape of a geometry of its own is written as the outline it draws, and what that is ([`super::poly`]); `None` for any other shape, which is
    /// written as before. **A segment of [`Self::own_route`] that is on the page's top level** (a member of a group is measured from the group) **and is glued at no end** (the caller leaves a
    /// glued one out, a polyline has no glue another program keeps, so it is a connector).
    fn poly_of(&mut self, n: &NewShape, diff: &PageDiff) -> Result<Option<super::poly::Placed>, WriteError> {
        if n.parent.is_some() || n.outer != Affine::IDENTITY {
            return Ok(None);
        }
        Ok(self.own_route(n)?.and_then(|route| super::poly::placed(&route, diff.height)))
    }

    /// The text of a new shape from the model (the model path only: a save into a file writes a new shape's text as plain characters and carries the rest), for a shape that has text
    /// in its element: a 2-D shape, a line or a connector.
    fn rich_text_of(
        &mut self,
        n: &NewShape,
        kind: Kind,
        page: &ScanPage,
        pfx: &super::emit::Pfx,
        page_at: usize,
    ) -> Result<Option<super::text::model::Rich>, WriteError> {
        if self.work.model_path && matches!(kind, Kind::Custom | Kind::Line | Kind::Connector | Kind::Plain | Kind::Poly) {
            return self.rich_text(&n.shape, page, pfx, page_at);
        }
        Ok(None)
    }

    /// A frame has the four connection points ODF gives every shape, and no other this path writes from the rows: a connector glued to a later row of a new picture is
    /// refused in words, **but for a row a `draw:glue-point` of the file made and that is unchanged, on the model path**, which writes that element back with the picture's kept
    /// markup, so the file names the point and the reader makes the row of it again (an end glued to a frame's own glue point follows it).
    fn check_picture_glue(
        &mut self,
        news: &[NewShape],
        new_at: &HashMap<u32, usize>,
        connects: &[Connect],
        file_glue: impl Iterator<Item = GlueTo>,
    ) -> Result<(), WriteError> {
        let pictures: HashSet<u32> = news.iter().filter(|n| is_new_picture(&n.shape)).map(|n| n.shape.id).collect();
        let past: Vec<(u32, u32)> = connects
            .iter()
            .filter(|c| pictures.contains(&c.to_sheet))
            .filter_map(|c| Some((c.to_sheet, past_frame_point(c.to_part)?)))
            .chain(file_glue.filter(|to| pictures.contains(&to.target)).filter_map(|to| Some((to.target, past_frame_point(Some(to.part))?))))
            .collect();
        if past.is_empty() {
            return Ok(());
        }
        let kept = if self.work.model_path { self.held_glue_rows(news, new_at, &past)? } else { HashSet::new() };
        if past.iter().any(|at| !kept.contains(at)) {
            return refuse(WORDS_GLUE);
        }
        Ok(())
    }

    /// The rows among `wanted` (a new shape's `ID` and a row's `IX`) that a `draw:glue-point` of the file made and that are unchanged since (the element held beside the row has the
    /// row's stamp): the model path writes such an element back with the shape's kept markup ([`Self::kept_markup`]), so the file names the point. Each shape named is walked
    /// once, its rows charged as steps.
    fn held_glue_rows(&mut self, news: &[NewShape], new_at: &HashMap<u32, usize>, wanted: &[(u32, u32)]) -> Result<HashSet<(u32, u32)>, WriteError> {
        let shapes: HashSet<u32> = wanted.iter().map(|(id, _)| *id).collect();
        let mut out = HashSet::new();
        for id in shapes {
            let Some(n) = new_at.get(&id).and_then(|i| news.get(*i)) else { continue };
            for section in n.shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Connection") {
                self.work.charge_steps(section.rows.len())?;
                for row in &section.rows {
                    let held = row.ext.get().and_then(|e| e.odf.held.as_ref());
                    if let (Some(ix), Some(h)) = (row.ix, held)
                        && h.group == diacraft_doc::HeldGroup::Glue
                        && h.stamp == Some(diacraft_doc::stamp_row(row))
                    {
                        out.insert((id, ix));
                    }
                }
            }
        }
        Ok(out)
    }

    /// What a new shape of the **model path** writes of the markup of the file it was read from, which it keeps in its `odf` slot (the design's §3.4, "Elements kept
    /// whole"): the attributes the writer does not write itself, for the start tag ([`Editor::kept_tag`] puts them on it), the children kept whole, for the end, and a custom shape's own
    /// geometry element while its stamp matches. **A save into a file writes none of it** (it says what it left out: [`unwritten`]). What the writer writes in another form
    /// is left out (its own names, ids, `draw:z-index`, the declarations, and a custom shape's geometry element, by its namespace). What a shape keeps comes to at most
    /// [`MOST_KEPT_PER_SHAPE`] bytes of markup: one past it is refused in words.
    ///
    /// **What XML and its namespaces cannot hold is never written** (the security scan's findings): a name that is no qualified name, or a value with a character
    /// XML 1.0 does not allow, is refused in words. **A prefix is written only where it stands for the namespace the markup was read in** ([`super::kept`]): the one the element
    /// that holds the pages binds it to; a place that binds it otherwise gets a declaration of its own on the markup, and markup whose namespace the model does not know (a prefix the
    /// file bound on the shape's own element, or that only a sibling's kept element declares) is left out and counted for the note.
    fn kept_markup(
        &mut self,
        n: &NewShape,
        (kind, frame): (Kind, bool),
        page: &ScanPage,
        pfx: &super::emit::Pfx,
        read: Option<&AsRead>,
    ) -> Result<KeptMarkup, WriteError> {
        let mut out = self.kept_markup_of_file(n, (kind, frame), page, pfx, read)?;
        self.put_alt_text(&mut out, n, frame, pfx)?;
        Ok(out)
    }

    /// The new shape's alt text as the elements `svg:title` and `svg:desc`: **before the shape's text** and the other children the schema puts before it
    /// for a shape, after the frame's content and its event listeners and glue points for a frame. The texts are the shape's **effective** ones, its master's where it has none of its own
    /// (`.odg` has no masters), written as they are, escaped, and in any save: a new shape of the model path and one put into a file.
    fn put_alt_text(&mut self, out: &mut KeptMarkup, n: &NewShape, frame: bool, pfx: &super::emit::Pfx) -> Result<(), WriteError> {
        let (text, texts) = self.alt_elements(n, pfx)?;
        if frame {
            out.after.push_str(&text);
        } else {
            out.before.insert_str(0, &text);
        }
        out.alt = texts;
        Ok(())
    }

    /// The elements `svg:title` and `svg:desc` of a new shape's effective texts, and the texts: the markup and the title and the description it says (empty for none).
    fn alt_elements(&mut self, n: &NewShape, pfx: &super::emit::Pfx) -> Result<(String, (String, String)), WriteError> {
        let (title, description) = {
            let found = diacraft_doc::alt_text::read(&self.resolver, &diacraft_doc::SheetRef::Shape(n.at.clone()));
            (found.title.text.to_owned(), found.description.text.to_owned())
        };
        let mut text = String::new();
        if title.is_empty() && description.is_empty() {
            return Ok((text, (title, description)));
        }
        self.work.charge_steps(1 + (title.len() + description.len()) / 4096)?;
        for (local, value) in [("title", &title), ("desc", &description)] {
            if !value.is_empty() {
                text.push_str(&super::alt::element(&pfx.svg, local, value)?);
            }
        }
        Ok((text, (title, description)))
    }

    /// [`Editor::kept_markup`] without the alt text: what the shape keeps of the markup of the file it was read from.
    fn kept_markup_of_file(
        &mut self,
        n: &NewShape,
        (kind, frame): (Kind, bool),
        page: &ScanPage,
        pfx: &super::emit::Pfx,
        read: Option<&AsRead>,
    ) -> Result<KeptMarkup, WriteError> {
        let as_read = read.is_some();
        let mut out = KeptMarkup::default();
        if !self.work.model_path {
            return Ok(out);
        }
        // The paragraphs of a picture's `draw:image`, each written as a child kept whole is, with the declarations it needs; one whose namespaces are not known is left out and
        // counted. Bounded and counted as the shape's own children are.
        if kind == Kind::Picture
            && let Some(held) = n.shape.foreign.as_deref().and_then(|f| f.ext.get())
            && !held.odf.kept.is_empty()
        {
            let nodes: Vec<&diacraft_doc::RawNode> = held.odf.kept.iter().map(|placed| &placed.node).collect();
            self.work.charge_steps(nodes.len())?;
            if !super::model::bounded(nodes.iter().copied(), MOST_KEPT_PER_SHAPE) {
                return Err(WriteError::Unsupported(WORDS_KEPT_BIG));
            }
            for node in nodes {
                let diacraft_doc::RawNode::Element(el) = node else {
                    out.left_elements = out.left_elements.saturating_add(1);
                    continue;
                };
                match self.kept_element(el, page, pfx)? {
                    Some((text, _)) => out.in_image.push_str(&text),
                    None => out.left_elements = out.left_elements.saturating_add(1),
                }
            }
        }
        let Some(data) = n.shape.ext.get() else { return Ok(out) };
        let kept = &data.odf.kept;
        if kept.raw.is_empty() && kept.kept.is_empty() {
            return Ok(out);
        }
        // Counted before anything is made: each attribute and child is a step (and the `xml:id` that may go beside a `draw:id`).
        self.work.charge_steps(kept.raw.len().saturating_add(kept.kept.len()).saturating_add(1))?;
        let written = written_attributes(kind);
        let mut size = 0usize;
        // The declarations the reader kept beside the attributes whose prefix an element above the shape declared: each binds its prefix, for the attributes of this shape, to the
        // namespace the attribute was read in. A declaration XML Namespaces does not allow binds nothing.
        let locals: HashMap<&str, &str> = kept
            .raw
            .iter()
            .filter_map(|(name, value)| {
                super::kept::declared_by(name).filter(|prefix| super::kept::declaration_ok(prefix, value)).map(|prefix| (prefix, value))
            })
            .collect();
        // The attributes the shape kept, with the `xml:id` a `draw:id` that stands alone needs beside it (see [`with_paired_ids`]); a `draw:id` that is no name is said in the note.
        let (held, no_id) = with_paired_ids(&kept.raw, self.legacy)?;
        out.left_attrs = out.left_attrs.saturating_add(no_id);
        for (name, value) in held {
            // A shape read from a file keeps its own `draw:text-style-name`; a new shape names the paragraph style the writer makes it.
            let own_text_style = as_read && name == "draw:text-style-name";
            // The ids a shape of the file held are written as the file had them: the model path never copies a shape, and the id of a shape no connector names is no
            // less its own than one that is named.
            let own_id = matches!(name, "xml:id" | "draw:id");
            if (written.contains(&name) && !own_text_style && !own_id) || name == "draw:z-index" || name.starts_with("xmlns") {
                continue;
            }
            if !super::kept::is_qname(name) || super::splice::has_bad_char(value) {
                return refuse(super::splice::WORDS_XML_CHAR);
            }
            let escaped = usize::try_from(super::splice::escaped_len(value, b'"')).unwrap_or(usize::MAX);
            size = size.saturating_add(name.len()).saturating_add(escaped).saturating_add(4);
            if size > MOST_KEPT_PER_SHAPE {
                return refuse(WORDS_KEPT_BIG);
            }
            let uri = match name.split_once(':') {
                // A name with no prefix is in no namespace, wherever it is written.
                None => None,
                Some((prefix, _)) => match self.need(page, prefix, pfx, (true, locals.get(prefix).copied())) {
                    Need::Bound(uri) => Some(uri),
                    Need::Declare(uri) => {
                        if !out.declare.iter().any(|(p, _)| p == prefix) {
                            out.declare.push((prefix.to_owned(), uri.clone()));
                        }
                        Some(uri)
                    }
                    Need::Unknown => {
                        out.left_attrs = out.left_attrs.saturating_add(1);
                        continue;
                    }
                },
            };
            out.attrs.push((name.to_owned(), value.to_owned(), uri));
        }
        // A group's children are its members' and its own glue points (`draw:glue-point`, in ODF's drawing namespace where it is written), which go before the members by the
        // standard's content of `draw:g`: a connector glued to one names it, and the reader makes its row of it again. What else a group keeps
        // has no place in it here, and is said.
        if kind == Kind::Group {
            let mut points: Vec<&diacraft_doc::RawNode> = Vec::new();
            for placed in kept.kept.iter() {
                match &placed.node {
                    diacraft_doc::RawNode::Element(e)
                        if local_of(&e.name) == "glue-point" && self.name_uri(e, page, pfx).as_deref() == Some(super::scan::DRAW_URI) =>
                    {
                        points.push(&placed.node);
                    }
                    diacraft_doc::RawNode::Text(t) if t.trim().is_empty() => {}
                    _ => out.left_elements = out.left_elements.saturating_add(1),
                }
            }
            if !super::model::bounded(points.iter().copied(), MOST_KEPT_PER_SHAPE) {
                return Err(WriteError::Unsupported(WORDS_KEPT_BIG));
            }
            for node in points {
                let diacraft_doc::RawNode::Element(el) = node else { continue };
                match self.kept_element(el, page, pfx)? {
                    Some((text, _)) => out.body.push_str(&text),
                    None => out.left_elements = out.left_elements.saturating_add(1),
                }
            }
            return Ok(out);
        }
        // A custom shape's geometry element is the one in ODF's drawing namespace, by the namespace its prefix stands for where it is written.
        let is_geometry = |ed: &Self, e: &diacraft_doc::RawElement| {
            kind == Kind::Custom && local_of(&e.name) == "enhanced-geometry" && ed.name_uri(e, page, pfx).as_deref() == Some(super::scan::DRAW_URI)
        };
        let held_geometry = if kind == Kind::Custom { super::held::enhanced_geometry(&n.shape, |e| is_geometry(self, e)) } else { None };
        let mut held_first: Option<usize> = None;
        if let Some(el) = held_geometry
            && super::model::bounded_element(el, MOST_KEPT_PER_SHAPE)
        {
            held_first = self.held_geometry_markup(el, n, page, pfx, &mut out)?;
        }
        // The outline of a shape the file's element still says is made again from the rows when the element cannot be written (a prefix nothing binds, markup XML cannot hold, or more than
        // a shape may keep): the file's own markup of it is not written, and the save says so.
        if held_geometry.is_some() && out.geometry.is_none() {
            out.left_elements = out.left_elements.saturating_add(1);
        }
        // **A custom shape written from its rows that lists them as its own glue points keeps no `draw:glue-point` element of its file**: a rectangle or an ellipse that was flipped is
        // written as a custom shape, whose geometry lists every row; an element beside the list is a point more, with an id the list already holds.
        // Worked out when the first such element is met, and once: a shape with none pays nothing.
        let written_from_rows = kind == Kind::Custom && out.geometry.is_none();
        let mut lists_rows: Option<bool> = None;
        // The children: elements, but the geometry element (written in another form: the one held, or one made from the rows), each with the namespaces it needs.
        let mut nodes: Vec<(&diacraft_doc::RawNode, u32)> = Vec::new();
        for placed in kept.kept.iter() {
            match &placed.node {
                diacraft_doc::RawNode::Element(e) if is_geometry(self, e) => {}
                diacraft_doc::RawNode::Element(_) => nodes.push((&placed.node, placed.after)),
                diacraft_doc::RawNode::Text(t) if t.trim().is_empty() => {}
                // A text, a comment or an instruction a shape kept beside its elements has no place in the element written: said.
                _ => out.left_elements = out.left_elements.saturating_add(1),
            }
        }
        if !super::model::bounded(nodes.iter().map(|(node, _)| *node), MOST_KEPT_PER_SHAPE) {
            return Err(WriteError::Unsupported(WORDS_KEPT_BIG));
        }
        // **The glue points of the rows the session changed**: the elements the file had stand for rows that are as they were read, and the rows it added have none.
        let read_mirrors = held_geometry.filter(|el| !super::mirrors::is_object(el)).map_or((false, false), super::mirrors::mirrors_of);
        let points = self.model_points(n, kind, read, held_first, read_mirrors, pfx)?;
        for (node, stood_after) in nodes {
            let diacraft_doc::RawNode::Element(el) = node else { continue };
            let own = frame && self.is_the_frames_picture(n, el, page, pfx)?;
            let linked = self.link_out_of_the_package(el, page, pfx)?;
            let el = linked.as_ref().unwrap_or(el);
            match self.kept_element(el, page, pfx)? {
                Some((text, uri)) => {
                    if own && uri.as_deref() == Some(super::scan::DRAW_URI) {
                        out.image = true;
                    }
                    // An element of a row that was taken away or edited is taken out; the row is given its own, below.
                    let is_point = uri.as_deref() == Some(super::scan::DRAW_URI) && local_of(&el.name) == "glue-point";
                    let stale = is_point
                        && (*lists_rows.get_or_insert_with(|| written_from_rows && rows_are_own(&n.shape.sheet, None))
                            || points.as_ref().is_some_and(|p| !p.keeps(&el.attrs)));
                    if stale {
                        continue;
                    }
                    if goes_before_text(uri.as_deref(), local_of(&el.name)) {
                        if frame {
                            out.after.push_str(&text);
                        } else {
                            out.before.push_str(&text);
                        }
                        continue;
                    }
                    // A picture frame writes its own picture between the children that stood before the one it showed and those that stood after it.
                    if frame && kind == Kind::Picture && stood_after > 0 {
                        out.behind.push_str(&text);
                        continue;
                    }
                    out.body.push_str(&text);
                }
                None => out.left_elements = out.left_elements.saturating_add(1),
            }
        }
        if let Some(points) = &points {
            out.before.push_str(&points.made);
        }
        Ok(out)
    }

    /// A custom shape's own `draw:enhanced-geometry` `el`, written back while its namespaces are known (else the outline is made again from the rows, as for an edited one), into `out`; its
    /// handles say their place as ODF 1.4 asks, the version the part says. **A shape whose rows are its own glue points lists every row again** (as the splice
    /// writes it): the element it kept lists the points its file had, and the session may have added rows or moved one. Gives the row the glue point elements start at (past the four standard
    /// ones and the points the geometry lists), `None` for a shape whose rows are its own, which has none, and for a geometry that is not written.
    fn held_geometry_markup(
        &mut self,
        el: &diacraft_doc::RawElement,
        n: &NewShape,
        page: &ScanPage,
        pfx: &super::emit::Pfx,
        out: &mut KeptMarkup,
    ) -> Result<Option<usize>, WriteError> {
        let stated = handles_of_1_4(el);
        let el = stated.as_ref().unwrap_or(el);
        let listed = listed_glue_points(el);
        let own = rows_are_own(&n.shape.sheet, Some(&listed));
        self.work.charge_steps(listed.count)?;
        let relisted = if own { super::glue::relisted(el, &n.shape.sheet) } else { None };
        let el = relisted.as_ref().map_or(el, |(element, _)| element);
        // **The element says the mirrors and the text turn the shape has now**: its stamp covers the rows and the equations, not the flips the session may have given the shape
        // since the file was read, which the reader gives back from the mirrors the element says.
        let flipped = self.mirrors_made(el, &n.shape)?;
        let el = flipped.as_ref().unwrap_or(el);
        out.geometry = match self.kept_element(el, page, pfx) {
            Ok(found) => found.map(|(text, _)| text),
            Err(WriteError::Unsupported(_)) => None,
            Err(other) => return Err(other),
        };
        out.glue_own = Some(own);
        Ok((!own && out.geometry.is_some()).then_some(listed.count.saturating_add(4)))
    }

    /// The held geometry element `el` with the mirrors and the turn of its text that `shape` has now ([`super::mirrors::mirrored`]), `None` when it says them already. The
    /// copy is charged by the nodes it holds. A shape whose flips are not numbers is refused as every new shape of the model path is, and **so is what the file cannot give back of an object
    /// geometry the reader does not draw** (a 3-D extrusion, a fontwork): a flip, since it keeps no mirror for it, and a text, since it reads none from the shape.
    fn mirrors_made(&mut self, el: &diacraft_doc::RawElement, shape: &diacraft_doc::Shape) -> Result<Option<diacraft_doc::RawElement>, WriteError> {
        let sheet = &*shape.sheet;
        let flips = own::flips_of(sheet).ok_or(WriteError::Unsupported("a new shape whose frame is not numbers"))?;
        // A turn that is no number (or no finite one) cannot be said as an attribute.
        let txt = own::cell_number(sheet, "TxtAngle")
            .map(f64::to_degrees)
            .filter(|t| t.is_finite())
            .ok_or(WriteError::Unsupported("a new shape whose text turn is not a number"))?;
        let flip = (flips.0 == -1.0, flips.1 == -1.0);
        if super::mirrors::is_object(el) {
            if shape.text.as_ref().is_some_and(|t| !t.parts.is_empty()) {
                return Err(WriteError::Unsupported(super::mirrors::WORDS_OBJECT_TEXT));
            }
            return if flip == (false, false) { Ok(None) } else { Err(WriteError::Unsupported(super::mirrors::WORDS_OBJECT_FLIP)) };
        }
        if !super::mirrors::differs(el, flip, txt) {
            return Ok(None);
        }
        self.work.charge_steps(1usize.saturating_add(el.children.len()))?;
        super::mirrors::mirrored(el, flip, txt).map_err(WriteError::Unsupported)
    }

    /// The glue point elements the model writer writes of a shape the `.odg` reader made, for the rows the session changed ([`ModelPoints`]): of a custom shape that writes the
    /// geometry its file held, from the row the geometry's own points end at (`held_first`, `None` when the shape's rows are its own glue points, which the geometry lists), and of a rectangle,
    /// an ellipse or a circle, from row 4. `None` for any other shape, and when the rows are not a list the elements can say.
    fn model_points(
        &mut self,
        n: &NewShape,
        kind: Kind,
        read: Option<&AsRead>,
        held_first: Option<usize>,
        read_mirrors: (bool, bool),
        pfx: &super::emit::Pfx,
    ) -> Result<Option<ModelPoints>, WriteError> {
        use crate::read::{Kindof, glue_box};
        let Some(read) = read else { return Ok(None) };
        let (first, boxed) = match (kind, read.element.as_str()) {
            (Kind::Custom, _) => match held_first {
                Some(first) => (first, glue_box(Kindof::Custom, None)),
                None => return Ok(None),
            },
            (Kind::Plain, "rect") => (4, glue_box(Kindof::Rect, None)),
            (Kind::Plain, element @ ("ellipse" | "circle")) => {
                let draw_kind = read.outline.iter().find(|(name, _)| local_of(name) == "kind").map(|(_, value)| value.as_str());
                (4, glue_box(if element == "ellipse" { Kindof::Ellipse } else { Kindof::Circle }, draw_kind))
            }
            _ => return Ok(None),
        };
        let Some(frame) = super::geometry::GlueFrame::of(&n.shape.sheet, boxed) else { return Ok(None) };
        let rows: usize = n.shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Connection").map(|s| s.rows.len()).sum();
        self.work.charge_steps(rows)?;
        // A turn or a flip made since the file was read leaves the elements it held in the old page axes: they are written again.
        let (was_turn, was_flip) = asread::place_as_read(read, read_mirrors);
        let turned = frame.place.angle - was_turn;
        let moved = frame.place.flip != was_flip || turned.sin().abs() > 1e-9 || turned.cos() < 0.0;
        let Some(points) = ModelPoints::of(&n.shape.sheet, first, &frame, moved, pfx) else { return Ok(None) };
        self.work.charge_copy(points.made.len(), self.limits)?;
        Ok(Some(points))
    }

    /// The `xlink:href` of the kept element `el`, when it is a `draw:image` and the attribute is XLink's, by the prefixes where the shape is written on `page`.
    fn image_href<'e>(&self, el: &'e diacraft_doc::RawElement, page: &ScanPage, pfx: &super::emit::Pfx) -> Option<(&'e str, &'e str)> {
        if local_of(&el.name) != "image" {
            return None;
        }
        let (name, href) = el.attrs.iter().find(|(name, _)| name.contains(':') && local_of(name) == "href")?;
        let namespace_of = |prefix: &str| match super::kept::own_binding(&el.attrs, prefix) {
            Some(uri) => Some(uri.to_owned()),
            None => match self.need(page, prefix, pfx, (false, None)) {
                Need::Bound(uri) | Need::Declare(uri) => Some(uri),
                Need::Unknown => None,
            },
        };
        let element_prefix = el.name.split_once(':').map_or("", |(p, _)| p);
        let attribute_prefix = name.split_once(':').map_or("", |(p, _)| p);
        (namespace_of(element_prefix).as_deref() == Some(super::scan::DRAW_URI) && namespace_of(attribute_prefix).as_deref() == Some(XLINK_URI))
            .then_some((name, href))
    }

    /// Whether the kept `draw:image` `el` of the frame `n` is **the frame's own picture**: its link names an entry of the model's (`Document::parts`) that holds the same bytes as
    /// the picture the frame shows, as a carried object's replacement picture does (`write::model::objects`: the model makes the entry and the image that names it). Any other image the frame
    /// kept is a picture of its own the reader does not draw, an SVG or a PDF, and the frame still writes its picture beside it.
    fn is_the_frames_picture(
        &mut self,
        n: &NewShape,
        el: &diacraft_doc::RawElement,
        page: &ScanPage,
        pfx: &super::emit::Pfx,
    ) -> Result<bool, WriteError> {
        let Some(media) = n.shape.foreign.as_ref().and_then(|f| f.picture.as_ref()) else { return Ok(false) };
        let Some((_, href)) = self.image_href(el, page, pfx) else { return Ok(false) };
        if self.part_names.is_none() {
            let parts = &self.doc.parts;
            let cost = super::picture::index_cost(parts.iter().map(|p| &*p.name));
            self.work.charge_steps(parts.len())?;
            self.work.hold(cost, "memory for the names of the entries the model holds")?;
            let index = parts.iter().enumerate().map(|(at, p)| (p.name.to_string(), at)).collect();
            self.part_names = Some(index);
        }
        let Some(at) = self.part_names.as_ref().and_then(|index| crate::package::href_entry(href, index)) else { return Ok(false) };
        let Some(part) = self.doc.parts.get(at) else { return Ok(false) };
        if std::sync::Arc::ptr_eq(&part.bytes, &media.bytes) {
            return Ok(true);
        }
        if part.bytes.len() != media.bytes.len() {
            return Ok(false);
        }
        self.work.charge_scan(part.bytes.len(), self.limits).map_err(|_| WriteError::Limit {
            what: "bytes of pictures compared with the entry a kept image names",
            max: self.limits.max_write_bytes,
        })?;
        Ok(part.bytes[..] == media.bytes[..])
    }

    /// Whether the link `href` names the entry a picture of the model was read from (`Media::part`, the name it is written under): the names are collected at the first ask, by a walk of the
    /// model's shapes (pages and master pages, groups down to `MAX_SHAPE_DEPTH`), each shape a step, and held in the save's memory room (the names are the model's own `Arc`s).
    fn names_a_picture_of_the_model(&mut self, href: &str) -> Result<bool, WriteError> {
        if self.picture_names.is_none() {
            let doc = self.doc;
            let mut names: std::collections::HashSet<std::sync::Arc<str>> = std::collections::HashSet::new();
            let mut stack: Vec<(&diacraft_doc::ShapeList, usize)> =
                doc.pages.iter().map(|p| (&p.shapes, 0)).chain(doc.masters.iter().map(|m| (&m.shapes, 0))).collect();
            while let Some((list, depth)) = stack.pop() {
                for shape in list.iter() {
                    self.work.charge_steps(1)?;
                    if let Some(media) = shape.foreign.as_ref().and_then(|f| f.picture.as_ref()) {
                        self.work.hold(media.part.len() as u64 + 96, "memory for the names of the entries the model's pictures were read from")?;
                        names.insert(std::sync::Arc::clone(&media.part));
                    }
                    if depth < diacraft_doc::MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                        stack.push((&shape.children, depth + 1));
                    }
                }
            }
            self.picture_names = Some(names);
        }
        let decoded = crate::package::percent_decode(href);
        let normalised = decoded.strip_prefix("./").unwrap_or(&decoded);
        let as_written = href.strip_prefix("./").unwrap_or(href);
        Ok(self.picture_names.as_ref().is_some_and(|names| names.contains(normalised) || names.contains(as_written)))
    }

    /// The kept `draw:image` `el` with its link made a link **out of the package**, when it is one a package has no entry for: a flat drawing's frame of two images has the
    /// picture the reader does not draw (an SVG or a PDF) as a link to a file beside the document (`plan.svg`, `Pictures/p1.svg`), kept as the file spells it, and the same text in a package names
    /// an entry the package has not (ODF resolves an IRI with no `../` inside the package, and LibreOffice 26.8.2 then says a load error). The link stays a link: `../plan.svg`, as a program
    /// writes a link to a file beside a package. A link that names an entry of the package, one that is not a path in it (`http:`, `../x`, `#x`) and an image that holds its picture are
    /// written as they were; so is every link of a flat drawing (its pictures are inline, and a relative link is a file beside the document there too). `None` when `el` is none of that.
    fn link_out_of_the_package(
        &mut self,
        el: &diacraft_doc::RawElement,
        page: &ScanPage,
        pfx: &super::emit::Pfx,
    ) -> Result<Option<diacraft_doc::RawElement>, WriteError> {
        if self.flat {
            return Ok(None);
        }
        let Some(book) = self.pictures.as_deref() else { return Ok(None) };
        let Some((name, href)) = self.image_href(el, page, pfx) else { return Ok(None) };
        if book.holds_name(href) {
            return Ok(None);
        }
        let Some(out) = super::picture::out_of_the_package(href) else { return Ok(None) };
        // A link to the picture another frame of the model shows names an entry the save has yet to write (the picture is written under the name it was read from).
        if self.names_a_picture_of_the_model(href)? {
            return Ok(None);
        }
        // The copy made here is the element's own size again, charged as the text of a kept element is (it is bounded by the caller: [`MOST_KEPT_PER_SHAPE`] for the shape's children).
        self.work.charge_steps(el.attrs.len().saturating_add(1))?;
        self.work.charge_copy(super::model::element_total(el, MOST_KEPT_PER_SHAPE).unwrap_or(MOST_KEPT_PER_SHAPE), self.limits)?;
        let attrs =
            diacraft_doc::RawAttrs::from_pairs(el.attrs.iter().map(|(n, v)| (n.to_owned(), if n == name { out.clone() } else { v.to_owned() })));
        Ok(Some(diacraft_doc::RawElement { name: el.name.clone(), attrs, children: el.children.clone() }))
    }

    /// What `prefix` of the markup a new shape kept needs where the shape is written on `page` ([`super::kept::need_of`]): the namespace it was read in is the one the element that holds
    /// the pages binds it to, as far as the model knows it, and the place binds it as the page's own declarations do over those, or, for one of the writer's own prefixes, to the ODF
    /// namespace the writer writes under it (`on_tag`: the markup goes on the writer's own start tag, where a declaration would bind the writer's names too).
    ///
    /// `local` is the namespace the reader kept for the prefix beside the attribute that uses it, when an element above the shape declared the prefix on its own start tag: the
    /// markup was read in that namespace, whatever the element that holds the pages binds the prefix to.
    fn need(&self, page: &ScanPage, prefix: &str, pfx: &super::emit::Pfx, (on_tag, local): (bool, Option<&str>)) -> Need {
        if prefix == "xml" {
            return Need::Bound(super::kept::XML_URI.to_owned());
        }
        let Some(own) = page.ns.as_deref() else { return Need::Unknown };
        let held = self.scopes.holder_binding(prefix);
        let read = local.or(held);
        let writer = writer_uri(pfx, prefix);
        let here = match writer {
            Some(uri) => Some(uri),
            None => match own.iter().rev().find(|(p, _)| p == prefix) {
                Some((_, uri)) => Some(uri.as_str()).filter(|u| !u.is_empty() || prefix.is_empty()),
                None => held,
            },
        };
        need_of(read, here, writer.is_some() && on_tag)
    }

    /// The namespace the name of the kept element `el` is in where the shape is written on `page`: the one it declares itself, or the one [`Editor::need`] finds for its prefix.
    fn name_uri(&self, el: &diacraft_doc::RawElement, page: &ScanPage, pfx: &super::emit::Pfx) -> Option<String> {
        let prefix = el.name.split_once(':').map_or("", |(p, _)| p);
        if let Some(uri) = super::kept::own_binding(&el.attrs, prefix) {
            return Some(uri.to_owned());
        }
        match self.need(page, prefix, pfx, (false, None)) {
            Need::Bound(uri) | Need::Declare(uri) => Some(uri),
            Need::Unknown => None,
        }
    }

    /// The text of the kept element `el` where the shape is written on `page`, with the declarations it needs (each prefix it uses and does not declare itself that the place binds
    /// otherwise than it was read), and the namespace of its own name; `None` when the namespace of a prefix it uses is not known. Markup that holds a name or a declaration XML
    /// Namespaces does not allow, or what XML cannot hold, is refused in words. The walk and the copy are charged before the text is made.
    pub(crate) fn kept_element(
        &mut self,
        el: &diacraft_doc::RawElement,
        page: &ScanPage,
        pfx: &super::emit::Pfx,
    ) -> Result<Option<(String, Option<String>)>, WriteError> {
        let Some(free) = super::kept::free_prefixes(el) else { return refuse(super::splice::WORDS_XML_CHAR) };
        self.work.charge_steps(free.steps)?;
        let own = el.name.split_once(':').map_or("", |(p, _)| p);
        let mut declare: Vec<(String, String)> = Vec::new();
        let mut name_uri = super::kept::own_binding(&el.attrs, own).map(str::to_owned);
        for prefix in &free.prefixes {
            let uri = match self.need(page, prefix, pfx, (false, None)) {
                Need::Bound(uri) => uri,
                Need::Declare(uri) => {
                    declare.push((prefix.clone(), uri.clone()));
                    uri
                }
                Need::Unknown => return Ok(None),
            };
            if prefix == own {
                name_uri = Some(uri);
            }
        }
        if own == "xml" {
            name_uri = Some(super::kept::XML_URI.to_owned());
        }
        // What the text comes to, as the bound counts it (the caller bounded it), and the declarations put on it: charged before it is made.
        let estimate = super::model::element_total(el, MOST_KEPT_PER_SHAPE).unwrap_or(MOST_KEPT_PER_SHAPE);
        let declared = declare.iter().fold(0usize, |sum, (p, u)| sum.saturating_add(p.len()).saturating_add(u.len()).saturating_add(10));
        self.work.charge_copy(estimate.saturating_add(declared), self.limits)?;
        let text = super::kept::element_text(el, &declare).ok_or(WriteError::Unsupported(super::splice::WORDS_XML_CHAR))?;
        Ok(Some((text, name_uri)))
    }

    /// Puts the attributes the shape kept ([`KeptMarkup`]) on its start tag `tag`, which holds the writer's own: the declarations they need first, then each of them, **but one whose
    /// namespace and local name the tag holds already** (one the writer writes itself under another prefix, or a kept one twice: an element with two attributes of one namespace and
    /// local name is not namespace-well-formed, and the reader takes the first). What is left out so is counted for the note.
    fn kept_tag(&mut self, tag: &mut String, pfx: &super::emit::Pfx) -> Result<(), WriteError> {
        let attrs = std::mem::take(&mut self.kept.attrs);
        let declare = std::mem::take(&mut self.kept.declare);
        if attrs.is_empty() && declare.is_empty() {
            return Ok(());
        }
        // The writer's own names on the tag, by namespace and local name: the tag is read back as it stands.
        let probe = format!("{tag}>");
        self.work.charge_attribute_bytes(probe.len())?;
        let mut spans = Vec::new();
        diacraft_container::xml_scan::tag_attributes(probe.as_bytes(), 0, probe.len(), &mut spans)
            .ok_or(WriteError::Mismatch("a new shape's start tag that the writer cannot read back"))?;
        let mut names: HashSet<(String, String)> = HashSet::new();
        for span in &spans {
            let Some(name) = probe.get(span.name.0..span.name.1) else { continue };
            if super::kept::declared_by(name).is_some() {
                continue;
            }
            let key = match name.split_once(':') {
                Some(("xml", l)) => (super::kept::XML_URI.to_owned(), l.to_owned()),
                Some((p, l)) => (writer_uri(pfx, p).map_or_else(|| format!("?{p}"), str::to_owned), l.to_owned()),
                None => (String::new(), name.to_owned()),
            };
            names.insert(key);
        }
        let mut text = String::new();
        for (prefix, uri) in &declare {
            text.push_str(&format!(" xmlns:{prefix}=\"{}\"", escape_value(uri, b'"')));
        }
        for (name, value, uri) in attrs {
            if !names.insert((uri.unwrap_or_default(), local_of(&name).to_owned())) {
                self.kept.left_attrs = self.kept.left_attrs.saturating_add(1);
                continue;
            }
            text.push_str(&format!(" {name}=\"{}\"", escape_value(&value, b'"')));
        }
        tag.push_str(&text);
        Ok(())
    }

    /// The start tag of a new group's `draw:g`: its name when it has one (the reader gives it to `Name` and `NameU`), the `xml:id` and `draw:id` a connector that names
    /// it needs, and the `draw:z-index` of a shape of the top level on a page that carries it. The group has no style of its own, and no layer;
    /// its members follow, and its end tag is put after them.
    fn group_open(&mut self, n: &NewShape, pfx: &super::emit::Pfx, id: Option<&str>, z: Option<i64>) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        if n.shape.name_u.as_deref().is_some_and(super::splice::has_bad_char) || n.shape.name.as_deref().is_some_and(super::splice::has_bad_char) {
            return refuse(super::splice::WORDS_XML_CHAR);
        }
        // **A group the file read with an automatic style of its own** (`draw:style-name`) is written with it, as a shape of the file is: the style it named, its cells written
        // natively and what the file's style kept beside them, on the style sheet its links name. The cells it says are the markup's, and DiaCraft's document file carries no entry for them
        // (`groups::style_cells`). A group that names no style is a new group: it has none, and a link to a style sheet is refused.
        // The record of the style it named is made and checked as a shape's is (`AsRead::of_group`): a name XML cannot carry is refused in words, here, and not by the splice that would meet it later.
        let named = if self.work.model_path {
            match AsRead::of_group(&n.shape) {
                Ok(read) => read,
                Err(_) => return refuse(super::splice::WORDS_XML_CHAR),
            }
        } else {
            None
        };
        let graphic = if let Some(read) = named {
            let parent = self.parent_of(sheet)?;
            let (_, mut ops, mut said) = native_style_as(sheet, &[], true, true);
            super::held::graphic_ops_said(&n.shape, &mut ops, &mut said);
            let ops = self.in_cell_units(ops, sheet);
            self.graphic_style(Some(&read), ops, &[], parent.as_deref())?
        } else {
            if sheet.links.line.is_some_and(|l| l != 0) || sheet.links.fill.is_some_and(|l| l != 0) || sheet.links.text.is_some_and(|l| l != 0) {
                return refuse("a new group on a style sheet (its links are written with a later task)");
            }
            None
        };
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let mut tag = format!("<{0}:g{1}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), None, None), z, id, &mut wrote));
        self.work.charge_attribute(self.limits)?;
        // The attributes the file kept on the group, and its own glue points, the children that go before its members (the model path: `kept_markup`).
        self.kept_tag(&mut tag, pfx)?;
        let points = std::mem::take(&mut self.kept.body);
        Ok((format!("{tag}>{points}"), NewWrote { attrs: wrote, ..NewWrote::default() }))
    }

    /// A new automatic graphic style that sets `ops` (the style cells written natively) and carries the attributes of DiaCraft's extension `dia` (tier 2:
    /// none for a shape that has none), and a new paragraph style: their names. The shape's own style is made for each new shape, so a tier-2 shape has
    /// one that no other element names. The automatic graphic style names `parent` as its parent (`parent_of`): the style sheet a new shape's three links name.
    fn new_styles_on(
        &mut self,
        mut ops: Vec<Op>,
        holds_text: bool,
        dia: &[(String, String)],
        parent: Option<&str>,
    ) -> Result<(String, String), WriteError> {
        self.note_markers(&ops);
        if holds_text {
            ops.push(Op { prop: 0, ns: Ns::Draw, local: "auto-grow-height", val: Val::Text("false".to_owned()) });
        }
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
        let (graphic, paragraph) = (book.names.next(Fam::Graphic), book.names.next(Fam::Paragraph));
        let mut style_attrs = Vec::new();
        let text = self.new_style(&graphic, parent, &ops, dia, &mut style_attrs)?;
        self.put_style(text)?;
        let scope =
            self.book.as_ref().and_then(|b| b.auto.as_ref()).map(|a| a.scope).ok_or(WriteError::Unsupported("a part with no automatic styles"))?;
        let pfx = super::emit::Pfx::of(&mut self.scopes, scope, self.work);
        self.put_style(format!("<{0}:style {0}:name=\"{1}\" {0}:family=\"paragraph\"{2}/>", pfx.style, escape_value(&paragraph, b'"'), pfx.declare))?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
        book.written.push(Written { name: graphic.clone(), family: Fam::Graphic, attrs: style_attrs.clone(), prop: 0 });
        book.written.push(Written { name: paragraph.clone(), family: Fam::Paragraph, attrs: Vec::new(), prop: 0 });
        Ok((graphic, paragraph))
    }

    /// The fingerprint of the formatting of the text written for the new shape `n` (`fingerprint::format_fp`), for the tier-2 item that carries the text: **a text written as plain characters
    /// has the plain text's**, and **the model writer's text has what a read of the file a first pass made gives** ([`WriteWork::formats`], by the page's place in the model and the shape's `ID`), which
    /// the first pass has not (`None`: the item is judged by its characters alone, as it was). A text the file says whole carries no item.
    fn format_written(&self, diff: &PageDiff, n: &NewShape, plain: &str, text_native: bool) -> Option<crate::ext::fingerprint::Fp> {
        if text_native {
            return None;
        }
        if self.work.model_path {
            return self.work.formats.get(&(diff.page_at, n.shape.id)).copied();
        }
        Some(crate::ext::fingerprint::plain_format_fp(u64::try_from(plain.chars().count()).unwrap_or(u64::MAX)))
    }

    /// The text of the paragraphs of a new shape: on the **model path** the text the model writer made before the element (runs, paragraphs, lists, headings and fields:
    /// `write/text/model.rs`); in a save into a file its text as R24 writes it, or one empty paragraph, plain characters only (no run, no row: a text's
    /// formatting and its markers are tier 2, `ext::native`). Gives the text as the file holds it, whether the shape holds text, what each paragraph
    /// reads as, and the plain characters written (the shown text of a field among them).
    fn paragraphs_text(&mut self, n: &NewShape, pfx: &super::emit::Pfx) -> Result<Paragraphs, WriteError> {
        // The model writer's text, made before the element: runs, paragraphs, lists, headings and fields as the file has them, and a carriage return as a break.
        if let Some(rich) = self.rich.take() {
            return Ok((rich.xml, true, rich.read, rich.plain, rich.native));
        }
        let empty = || (format!("<{}:p/>", pfx.text), false, vec![(0, String::new())], String::new(), false);
        let Some(t) = &n.shape.text else { return Ok(empty()) };
        let piece = |p: &diacraft_doc::TextPart| -> usize {
            match p {
                diacraft_doc::TextPart::Chars(c) => c.len(),
                diacraft_doc::TextPart::Fld { text, .. } => text.len(),
                _ => 0,
            }
        };
        // The text is copied (as characters, then written), so a text of more than the retext's bound is not written.
        let bytes: usize = t.parts.iter().map(piece).fold(0, usize::saturating_add);
        if bytes > super::text::MOST_EDITED_TEXT {
            return refuse("a new shape with more than 16 MiB of text");
        }
        let mut whole = String::new();
        for part in &t.parts {
            let chars: &str = match part {
                diacraft_doc::TextPart::Chars(c) => c,
                diacraft_doc::TextPart::Fld { text, .. } => text,
                _ => continue,
            };
            if super::splice::has_bad_char(chars) {
                return refuse(super::splice::WORDS_XML_CHAR);
            }
            if chars.contains('\r') {
                return refuse(super::splice::WORDS_CARRIAGE_RETURN);
            }
            whole.push_str(chars);
        }
        if whole.is_empty() {
            return Ok(empty());
        }
        let mut out = String::new();
        let mut read: Vec<(usize, String)> = Vec::new();
        for (index, para) in whole.split('\n').enumerate() {
            let body = super::text::encode(para, &pfx.text, "");
            out.push_str(&if body.is_empty() { format!("<{}:p/>", pfx.text) } else { format!("<{0}:p>{body}</{0}:p>", pfx.text) });
            read.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a new shape", max: index as u64 })?;
            read.push((index, para.to_owned()));
        }
        Ok((out, true, read, whole, false))
    }

    /// Writes a 2-D shape's place on the tag of its element: its corner (`svg:x`, `svg:y`) when it is not turned, and a turned one as LibreOffice writes it, `rotate` then
    /// `translate` and no `svg:x` or `svg:y`. The corner after the turn is the centre less the turned half diagonal, as the reader's map takes (0, 0) of the frame.
    /// A custom shape and a picture frame are placed alike (the reader reads the one frame of both).
    fn place(
        &mut self,
        tag: &mut String,
        wrote: &mut Vec<(Ns, &'static str)>,
        pfx: &super::emit::Pfx,
        pose: &own::Pose,
        angle: f64,
        page_height: f64,
    ) -> Result<(), WriteError> {
        let (w, h) = pose.size;
        let centre = (pose.centre.0, page_height - pose.centre.1);
        if angle == 0.0 {
            for (local, v) in [("x", centre.0 - w / 2.0), ("y", centre.1 - h / 2.0)] {
                tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(v)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
        } else {
            let (s, c) = angle.sin_cos();
            let (hx, hy) = (w / 2.0 * c + h / 2.0 * s, -w / 2.0 * s + h / 2.0 * c);
            let text = super::transform::turned(angle, centre.0 - hx, centre.1 - hy, self.unit)
                .ok_or(WriteError::Unsupported("a length too large to write"))?;
            tag.push_str(&pfx.attr(Ns::Draw, "transform", &text));
            wrote.push((Ns::Draw, "transform"));
        }
        Ok(())
    }

    /// The tier 2 of a new shape, and the attributes of the extension that carry it (none when it has none): what of the shape the rebuild from what is
    /// written natively does not give (`ext::native`). A formula of it that names a shape of the file is refused: the shape of the file has no `ID` the
    /// save can promise to keep (the reader numbers shapes without a DiaCraft id by their place, which a new shape before it moves).
    fn extension_of(
        &mut self,
        n: &NewShape,
        native: &native::Native<'_>,
        tc: &TierCtx<'_>,
    ) -> Result<(payload::Tier2, Vec<(String, String)>), WriteError> {
        self.work.charge_compared(self.limits)?;
        // Sections are carried by their key, one attribute each: two of one key cannot be told apart in the file.
        let mut keys: HashSet<(&str, Option<u32>)> = HashSet::new();
        if !n.shape.sheet.sections.iter().all(|s| keys.insert((s.name.as_str(), s.ix))) {
            return refuse("a new shape with two sections of one name (the file carries a section by its name)");
        }
        let tier2 = native::tier2(&n.shape, native, tc.referenced.contains(&n.shape.id));
        // A text, or the rows of a text the file says in its characters, is stamped with the formatting its markup says: the model writer reads its first pass for it.
        self.work.carried_texts = self.work.carried_texts.saturating_add(u64::from(tier2.text.is_some() || tier2.carries_text_rows()));
        self.work.carried_shapes = self.work.carried_shapes.saturating_add(u64::from(tier2.vsdx.is_some() || tier2.foreign.is_some()));
        if native::carried_pin_the_reader_drops(native, &tier2) {
            return refuse(WORDS_PIN_DISAGREES);
        }
        let mut named: Vec<u32> = Vec::new();
        for cell in tier2.cells.values() {
            named.extend(native::formula_sheets(cell));
        }
        for section in &tier2.sections {
            for cell in section.cells.values().chain(section.rows.iter().flat_map(|r| r.cells.values())) {
                named.extend(native::formula_sheets(cell));
            }
        }
        if named.iter().any(|id| !tc.new_ids.contains(id) && tc.held.ordinal(*id).is_some()) {
            return refuse("a formula of a new shape that names a shape of the file (the file's shape keeps no ID the formula can rely on)");
        }
        // **The ends a line or a connector carries are written in the page's frame**: the file's markup is the page's, the reader judges a carried end against it and puts the
        // group's corner off every member's ends, and the model holds a member's in the group's frame (`native::with_ends_on_page`). The tier 2 the check of the file made compares is the model's.
        let on_page = if matches!(native.frame, native::Frame::OneD) {
            own::ends(&n.shape.sheet, &n.outer).and_then(|page| native::with_ends_on_page(&tier2, &page))
        } else {
            None
        };
        let attrs = match payload::encode(on_page.as_ref().unwrap_or(&tier2)) {
            Ok(Some(attrs)) => attrs,
            Ok(None) => return refuse(super::splice::WORDS_XML_CHAR),
            Err(words) => return refuse(words),
        };
        Ok((tier2, attrs))
    }

    fn custom_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &super::emit::Pfx,
        id: Option<&str>,
        z: Option<i64>,
        tc: &TierCtx<'_>,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        if n.shape.name_u.as_deref().is_some_and(super::splice::has_bad_char) || n.shape.name.as_deref().is_some_and(super::splice::has_bad_char) {
            return refuse(super::splice::WORDS_XML_CHAR);
        }
        let parent = self.parent_of(sheet)?;
        let pose = own::pose_2d(sheet, &n.outer).ok_or(WriteError::Unsupported("a new shape whose frame is not numbers"))?;
        let (angle, flip) = turn_and_flips(sheet, &pose)?;
        // The outline: the rows of tier 1 as a path; any other rows are tier 2, with a placeholder rectangle in the file for a viewer that does not read it.
        let (path, outline_native, subpaths, reader_form) = match new_outline(sheet) {
            Ok(outline) => {
                self.work.charge_steps(outline.rows)?;
                (outline.path, true, outline.subpaths, outline.reader_form)
            }
            Err(_) => (PLACEHOLDER_PATH.to_owned(), false, 1, false),
        };
        // **The geometry element the file held is written back while its stamp matches** (model path: the design's "written back while its stamp matches; otherwise made
        // again from the cells"): a shape of a file keeps its own `draw:enhanced-geometry`, which draws the shape in any program, where the rows alone give the placeholder above.
        // (Found by `kept_markup`, by its namespace, with the declarations it needs where it is written.)
        let held_geometry: Option<String> = self.kept.geometry.take();
        let held_own = held_geometry.as_ref().and_then(|_| self.kept.glue_own.take());
        let outline_native = outline_native || held_geometry.is_some();
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_before = std::mem::take(&mut self.kept.before);
        let alt = std::mem::take(&mut self.kept.alt);
        let asread = self.kept.read.take();
        let (paragraphs, holds_text, read, plain, text_native) = self.paragraphs_text(n, pfx)?;
        // A custom shape of a file that held no paragraph of its own is written with none: the empty paragraph a new shape is written with is the writer's.
        let (paragraphs, read) = if !holds_text && asread.as_ref().is_some_and(|r| r.element == "custom-shape" && !r.bare_text) {
            (String::new(), Vec::new())
        } else {
            (paragraphs, read)
        };
        let (_, mut ops, mut style_names) = native_style_as(sheet, &[], asread.is_some(), true);
        super::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = if asread.is_some() { self.in_cell_units(ops, sheet) } else { ops };
        let display = super::fx::display_of(sheet, &ops);
        let (w, h) = pose.size;
        // Where the shape is in its own parent's frame, which is where the reader rebuilds it (for a member of a group, measured from the group's box): the shape's
        // own cells, and not the page's.
        let here = own::pose_2d(sheet, &Affine::IDENTITY).ok_or(WriteError::Unsupported("a new shape whose frame is not numbers"))?;
        let layer = diff.layers.of_shape(sheet);
        let (layer_text, layer_row) = match &asread {
            Some(r) => (r.layer.clone(), r.layer.as_deref().and_then(|name| diff.layers.row_named(name))),
            None => (Some(layer.name.clone()), layer.row),
        };
        let native = native::Native {
            alt: Some((alt.0.as_str(), alt.1.as_str())),
            frame: native::Frame::TwoD { cx: here.centre.0, cy: here.centre.1, w, h, angle, flip },
            style: &style_names,
            display: Some(&display),
            // The rows the path is written from are the ones the reader gives back only in its own forms; otherwise they are carried beside the path. (The geometry element a
            // shape of a file kept is its own outline, and gives back its own rows.)
            geometry: held_geometry.is_some() || (outline_native && reader_form),
            outline_sections: subpaths,
            picture: false,
            connection: rebuilt_connection(sheet, self.work.model_path),
            connector: false,
            route: None,
            glued: [false, false],
            text: &plain,
            text_native,
            format: self.format_written(diff, n, &plain, text_native),
            picture_bytes: None,
            expected: diacraft_doc::ShapeType::Shape,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: if held_geometry.is_some() { None } else { Some(crate::ext::fingerprint::new_custom_outline_fp(&path)) },
        };
        let (tier2, mut dia) = self.extension_of(n, &native, tc)?;
        // Connection points that are not the four side middles are written as the geometry's own glue points, and the style says the rows start at 0 with the first of
        // them (`diacraft:glue="own"`): LibreOffice draws them with the geometry, mirrored and turned with it, where the session has them. (The geometry
        // element the file held has its own, and its numbering: a shape that writes it says its rows start at 0 only when they did, `rows_are_own`)
        let rows_own = held_own.unwrap_or_else(|| rows_are_own(sheet, None));
        let own_glue = if rows_own && held_geometry.is_none() { super::geometry::glue_points_text(sheet) } else { None };
        if rows_own {
            dia.push(("glue".to_owned(), "own".to_owned()));
        }
        let (graphic, paragraph) = match &asread {
            Some(_) => (self.graphic_style(asread.as_ref(), ops, &dia, parent.as_deref())?, None),
            None => {
                let (graphic, paragraph) = self.new_styles_on(ops, holds_text, &dia, parent.as_deref())?;
                (Some(graphic), Some(paragraph))
            }
        };
        let mut wrote = Vec::new();
        let mut tag = format!("<{0}:custom-shape{1}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), paragraph.as_deref(), layer_text.as_deref()), z, id, &mut wrote));
        // The turn the file holds beside one mirror is minus the model's (`read::turn_beside_mirrors`), so that LibreOffice draws the shape where the session has it.
        let turn = crate::read::turn_beside_mirrors(angle, flip.0, flip.1);
        if asread.is_some() {
            tag.push_str(&self.frame_attrs(asread.as_ref(), (sheet, &pose, turn, false), pfx, &mut wrote)?);
        } else {
            for (local, v) in [("width", w), ("height", h)] {
                tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(v)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
            self.place(&mut tag, &mut wrote, pfx, &pose, turn, diff.height)?;
        }
        let mut kid = vec![(Ns::Svg, "viewBox"), (Ns::Draw, "type"), (Ns::Draw, "enhanced-path")];
        let mut mirrors = String::new();
        for (on, local) in [(flip.0, "mirror-horizontal"), (flip.1, "mirror-vertical")] {
            if on {
                mirrors.push_str(&pfx.attr(Ns::Draw, local, "true"));
                kid.push((Ns::Draw, local));
            }
        }
        // LibreOffice turns the text of a shape with `FlipY` half a turn more than the frame it draws (`read::text_turn_beside_flips`), where the session's text reads as its
        // frame's turn: the text's own half turn written here undoes it, and the reader gives the shape no `TxtAngle` of it.
        if flip.1 {
            mirrors.push_str(&pfx.attr(Ns::Draw, "text-rotate-angle", "180"));
            kid.push((Ns::Draw, "text-rotate-angle"));
        }
        if let Some((points, ways)) = &own_glue {
            mirrors.push_str(&pfx.attr(Ns::Draw, "glue-points", points));
            kid.push((Ns::Draw, "glue-points"));
            if let Some(ways) = ways {
                mirrors.push_str(&pfx.attr(Ns::Draw, "glue-point-leaving-directions", ways));
                kid.push((Ns::Draw, "glue-point-leaving-directions"));
            }
        }
        let geometry = match held_geometry {
            Some(held) => {
                kid.clear();
                held
            }
            None => format!(
                "<{0}:enhanced-geometry{1}{2}{3}{4}/>",
                pfx.draw,
                pfx.attr(Ns::Svg, "viewBox", "0 0 21600 21600"),
                pfx.attr(Ns::Draw, "type", "non-primitive"),
                pfx.attr(Ns::Draw, "enhanced-path", &path),
                mirrors
            ),
        };
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        self.work.charge_attribute(self.limits)?;
        let tier2_written = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        Ok((
            format!("{tag}>{kept_before}{paragraphs}{geometry}{kept_body}</{}:custom-shape>", pfx.draw),
            NewWrote {
                attrs: wrote,
                paras: read,
                kid,
                // A guide is written with the placeholder outline on a frame of no size: it draws nothing in another program, so there is nothing to say of it.
                outline_approximated: !outline_native && n.shape.kind != diacraft_doc::ShapeType::Guide,
                tier2: tier2_written,
            },
        ))
    }

    /// A new picture: a `draw:frame` holding a `draw:image`, with its style, its layer, its name and its frame as a custom shape has, and no text (a text on a
    /// picture is refused in words). **In a package** the image names a new entry under `Pictures/` by its `xlink:href` and holds no data;
    /// the entry and its manifest row are the save's ([`super::picture`]). **In a flat drawing** it carries the picture inline: `draw:mime-type` says what the picture
    /// is, and `office:binary-data` holds it, base64 in lines of 72 characters separated by line feeds, with no `xlink:href`, since a flat document has no `Pictures/`
    /// to name.
    ///
    /// What the frame's native markup does not say is **tier 2** as a custom shape's is ([`native_style`], [`Self::extension_of`]): the cells no style attribute says,
    /// the `Img` cells, a layer, the links to a master, sections beside the four connection rows the reader gives every frame. The prediction of the rebuild is a
    /// picture frame's (`native::Native::picture`): the reader gives it no `Geometry` section whatever it was written with, so a model's `Geometry` is carried.
    fn picture_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &super::emit::Pfx,
        id: Option<&str>,
        z: Option<i64>,
        tc: &TierCtx<'_>,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        let Some(media) = n.shape.foreign.as_ref().and_then(|f| f.picture.as_ref()) else {
            return refuse("a new picture with no picture");
        };
        if n.shape.name_u.as_deref().is_some_and(super::splice::has_bad_char) || n.shape.name.as_deref().is_some_and(super::splice::has_bad_char) {
            return refuse(super::splice::WORDS_XML_CHAR);
        }
        if n.shape.text.as_ref().is_some_and(|t| !t.parts.is_empty()) {
            return refuse(super::own::WORDS_FRAME_TEXT);
        }
        // A picture on a style sheet names it as the parent of its own style, as a custom shape does (found 279 visits refused
        // for it); links that name different style sheets are refused in words (the file says one parent).
        let parent = self.parent_of(sheet)?;
        if !n.shape.children.is_empty() {
            return refuse("a new picture that is a group (a later version writes it)");
        }
        // The kind of picture by its own bytes, as the reader judges: the formats it draws.
        let Some(ty) = picture_type(&media.bytes) else { return refuse(WORDS_NOT_DRAWN) };
        // Where the picture is on the page, through the groups round it (a member of a new group has its cells in the group's frame: `n.outer` takes them to the page, as it
        // does for a custom shape), and where it is in its own parent's frame, which is where the reader rebuilds it.
        let pose = own::pose_2d(sheet, &n.outer).ok_or(WriteError::Unsupported("a new picture whose frame is not numbers"))?;
        let here = own::pose_2d(sheet, &Affine::IDENTITY).ok_or(WriteError::Unsupported("a new picture whose frame is not numbers"))?;
        // A turn is written as a custom shape's is (`draw:transform`), and the reader gives it back as `Angle`. A flip is not: a frame has no mirror in the rules of the
        // reader (the mirrors are on a custom shape's geometry), so a flipped picture would be drawn the right way up by every program but DiaCraft.
        let (angle, flip) = turn_and_flips(sheet, &pose)?;
        if flip != (false, false) {
            return refuse(WORDS_PICTURE_FLIP);
        }
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_after = std::mem::take(&mut self.kept.after);
        let alt = std::mem::take(&mut self.kept.alt);
        let kept_behind = std::mem::take(&mut self.kept.behind);
        // The paragraphs the picture's `draw:image` held, for the `draw:image` written.
        let in_image = std::mem::take(&mut self.kept.in_image);
        let asread = self.kept.read.take();
        let (_, mut ops, mut style_names) = native_style_as(sheet, &[], asread.is_some(), true);
        super::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = if asread.is_some() { self.in_cell_units(ops, sheet) } else { ops };
        let display = super::fx::display_of(sheet, &ops);
        let (w, h) = pose.size;
        let layer = diff.layers.of_shape(sheet);
        let (layer_text, layer_row) = match &asread {
            Some(r) => (r.layer.clone(), r.layer.as_deref().and_then(|name| diff.layers.row_named(name))),
            None => (Some(layer.name.clone()), layer.row),
        };
        let native = native::Native {
            alt: Some((alt.0.as_str(), alt.1.as_str())),
            frame: native::Frame::TwoD { cx: here.centre.0, cy: here.centre.1, w, h, angle, flip },
            style: &style_names,
            display: Some(&display),
            geometry: false,
            outline_sections: 1,
            picture: true,
            connection: rebuilt_connection(sheet, self.work.model_path),
            connector: false,
            route: None,
            glued: [false, false],
            text: "",
            text_native: false,
            format: None,
            picture_bytes: Some(&media.bytes[..]),
            expected: diacraft_doc::ShapeType::Foreign,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: None,
        };
        let (tier2, dia) = self.extension_of(n, &native, tc)?;
        // The entry of a package: the picture's name in it (the same bytes are one entry, and a name is the save's own). A flat drawing holds the picture itself.
        // **A frame that keeps a picture of its own** among the children the file had (a carried object's replacement: a `draw:image` naming an entry the model holds, with the bytes of the
        // frame's picture) is written with that one: the model path writes the kept children (`kept_markup`), and makes no entry of a picture it already has. **A kept image
        // that is another picture is no such thing**: the reader takes the picture it shows (the first it draws) out of the children it keeps, so an SVG or a PDF among them is
        // another image of the frame, written beside it; it was once taken for the frame's own as soon as the markup that binds its prefixes was written, and the picture was left out.
        let own_image = self.work.model_path && self.kept.image;
        let entry = match (self.pictures.as_deref_mut(), self.flat) {
            _ if own_image => None,
            (Some(book), _) => {
                // A picture of a file is written under the entry it was read from.
                let wanted = asread.is_some().then(|| media.part.as_ref()).filter(|name| asread::entry_name_ok(name));
                let added = match wanted {
                    Some(wanted) => book.add_as(&media.bytes, ty, Some(wanted), self.limits, &mut *self.work)?,
                    None => book.add(&media.bytes, ty, self.limits, &mut *self.work)?,
                };
                Some(added.to_owned())
            }
            (None, false) => return Err(WriteError::Mismatch("a new picture of a package without the book of the save's pictures")),
            (None, true) => None,
        };
        let (graphic, paragraph) = match &asread {
            Some(_) => (self.graphic_style(asread.as_ref(), ops, &dia, parent.as_deref())?, None),
            None => {
                let (graphic, paragraph) = self.new_styles_on(ops, false, &dia, parent.as_deref())?;
                (Some(graphic), Some(paragraph))
            }
        };
        let mut wrote = Vec::new();
        let mut tag = format!("<{0}:frame{1}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), paragraph.as_deref(), layer_text.as_deref()), z, id, &mut wrote));
        if asread.is_some() {
            tag.push_str(&self.frame_attrs(asread.as_ref(), (sheet, &pose, angle, false), pfx, &mut wrote)?);
        } else {
            for (local, v) in [("width", w), ("height", h)] {
                tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(v)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
            self.place(&mut tag, &mut wrote, pfx, &pose, angle, diff.height)?;
        }
        self.work.charge_attribute(self.limits)?;
        // The end of the `draw:image` written: the paragraphs it held, when it held any, and else none. A frame that writes no picture of its own (a carried object's replacement among
        // its kept children) has no place for them, and they are counted as left out.
        let close = if in_image.is_empty() { "/>".to_owned() } else { format!(">{in_image}</{}:image>", pfx.draw) };
        if own_image && !in_image.is_empty() {
            self.kept.left_elements = self.kept.left_elements.saturating_add(1);
        }
        let image = match entry {
            None if own_image => String::new(),
            // The `xlink` prefix is declared on the element: the part's own binding is not assumed, and the declaration is never wrong where it is.
            // A picture of a file has the attributes its element had, and the replacement of an object the form of its reference LibreOffice writes.
            Some(name) if asread.is_some() => {
                let href = if name.starts_with("ObjectReplacements/") { format!("./{name}") } else { name };
                let (attrs, left) = asread::image_attrs(&n.shape, pfx);
                self.kept.left_attrs = self.kept.left_attrs.saturating_add(left);
                format!("<{0}:image xmlns:xlink=\"{XLINK_URI}\" xlink:href=\"{1}\"{2}{close}", pfx.draw, escape_value(&href, b'"'), attrs)
            }
            Some(name) => format!(
                "<{0}:image xmlns:xlink=\"{XLINK_URI}\" xlink:href=\"{1}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"{close}",
                pfx.draw,
                escape_value(&name, b'"')
            ),
            None => {
                // The picture's characters are charged before they are made: four for three bytes, and a line feed for each 72.
                self.work.charge_copy(media.bytes.len().saturating_mul(3).div_ceil(2), self.limits)?;
                let data = super::convert::base64_lines(&media.bytes)?;
                // The office prefix the document's root children are written under, else a declaration of the namespace on the element.
                let (office, declare) = match &self.office {
                    Some(prefix) => (prefix.clone(), String::new()),
                    None => ("office".to_owned(), format!(" xmlns:office=\"{}\"", crate::ns::OFFICE)),
                };
                format!(
                    "<{0}:image{1}><{2}:binary-data{3}>{data}</{2}:binary-data>{in_image}</{0}:image>",
                    pfx.draw,
                    pfx.attr(Ns::Draw, "mime-type", ty.media),
                    office,
                    declare
                )
            }
        };
        let tier2_written = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        // The children the file kept (an object's element) come first: a consumer shows the first child it supports, and the replacement picture is the last resort. The
        // frame's own glue points come after its content (`KeptMarkup::after`).
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        Ok((
            format!("{tag}>{kept_body}{image}{kept_behind}{kept_after}</{}:frame>", pfx.draw),
            NewWrote { attrs: wrote, tier2: tier2_written, ..NewWrote::default() },
        ))
    }

    /// A new line drawn with the line tool: a `draw:line` holding its two ends, flipped by R2,
    /// with its layer, its name, an empty `text:p` and a new automatic graphic style and paragraph style. It has no geometry element, and no
    /// `draw:auto-grow-height` (R48 asks that of custom shapes). What else it holds (a text, a layer, a master, other sections) is tier 2.
    #[allow(clippy::too_many_arguments)]
    fn line_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &super::emit::Pfx,
        id: Option<&str>,
        z: Option<i64>,
        tc: &TierCtx<'_>,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        if n.shape.name_u.as_deref().is_some_and(super::splice::has_bad_char) || n.shape.name.as_deref().is_some_and(super::splice::has_bad_char) {
            return refuse(super::splice::WORDS_XML_CHAR);
        }
        let parent = self.parent_of(sheet)?;
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_before = std::mem::take(&mut self.kept.before);
        let alt = std::mem::take(&mut self.kept.alt);
        let asread = self.kept.read.take();
        let ends = own::ends(sheet, &n.outer).ok_or(WriteError::Unsupported("a new line whose ends are not numbers"))?;
        // Its text, as `text:p` (ODF lets a line hold some): plain characters, the rest of what a text holds being tier 2 (a Data Flow's label).
        let (paragraphs, holds_text, read, plain, text_native) = self.paragraphs_text(n, pfx)?;
        let (paragraphs, read) =
            if !holds_text && asread.as_ref().is_some_and(|r| !r.bare_text) { (String::new(), Vec::new()) } else { (paragraphs, read) };
        let (_, mut ops, mut style_names) = native_style_as(sheet, &[], asread.is_some(), true);
        super::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = if asread.is_some() { self.in_cell_units(ops, sheet) } else { ops };
        let display = super::fx::display_of(sheet, &ops);
        let layer = diff.layers.of_shape(sheet);
        let (layer_text, layer_row) = match &asread {
            Some(r) => (r.layer.clone(), r.layer.as_deref().and_then(|name| diff.layers.row_named(name))),
            None => (Some(layer.name.clone()), layer.row),
        };
        let native = native::Native {
            alt: Some((alt.0.as_str(), alt.1.as_str())),
            frame: native::Frame::OneD,
            style: &style_names,
            display: Some(&display),
            geometry: is_line_outline(sheet),
            outline_sections: 1,
            picture: false,
            // (A line, new or of the file, is read back with the four connection rows the reader gives every shape.)
            connection: !sheet.sections.iter().any(|s| s.name.as_str() == "Connection") || standard_connection(sheet),
            connector: false,
            route: None,
            glued: [false, false],
            text: &plain,
            text_native,
            format: self.format_written(diff, n, &plain, text_native),
            picture_bytes: None,
            expected: diacraft_doc::ShapeType::Shape,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: None,
        };
        let (tier2, dia) = self.extension_of(n, &native, tc)?;
        let (graphic, paragraph) = match &asread {
            Some(_) => (self.graphic_style(asread.as_ref(), ops, &dia, parent.as_deref())?, None),
            None => {
                let (graphic, paragraph) = self.new_styles_on(ops, false, &dia, parent.as_deref())?;
                (Some(graphic), Some(paragraph))
            }
        };
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let mut tag = format!("<{0}:line{1}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), paragraph.as_deref(), layer_text.as_deref()), z, id, &mut wrote));
        if asread.is_some() {
            tag.push_str(&self.ends_attrs(asread.as_ref(), sheet, ends, pfx, &mut wrote)?);
        } else {
            let values = [("x1", ends[0].0), ("y1", diff.height - ends[0].1), ("x2", ends[1].0), ("y2", diff.height - ends[1].1)];
            for (local, v) in values {
                tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(v)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
        }
        self.work.charge_attribute(self.limits)?;
        let tier2 = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        Ok((
            format!("{tag}>{kept_before}{paragraphs}{kept_body}</{}:line>", pfx.draw),
            NewWrote { attrs: wrote, paras: read, tier2, ..NewWrote::default() },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn connector_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &super::emit::Pfx,
        connects: &[&Connect],
        (id, z): (Option<&str>, Option<i64>),
        tc: &TierCtx<'_>,
        (id_of, glue_of): Targets<'_>,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        if n.shape.name_u.as_deref().is_some_and(super::splice::has_bad_char) || n.shape.name.as_deref().is_some_and(super::splice::has_bad_char) {
            return refuse(super::splice::WORDS_XML_CHAR);
        }
        let parent = self.parent_of(sheet)?;
        let kind = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
        // **A route kind the standard's three types cannot say** (the page's own route style a VSDX connector says as 0, a kind of the VSDX application's own, any pair of the two cells that is no
        // `standard`, `line` or `curve`) is written as the default type (it was refused, and `file.check_compatibility` gave the same refusal): the reader makes the
        // standard kind of that, and the cells that say the kind the connector has ride in DiaCraft's data ([`native::Native::route`]), so that it reads back with them.
        let draw_type: Option<String> = match (kind("ShapeRouteStyle"), kind("ConLineRouteExt")) {
            (Some(2.0), Some(1.0)) => Some("line".to_owned()),
            (Some(1.0), Some(2.0)) => Some("curve".to_owned()),
            _ => None,
        };
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_before = std::mem::take(&mut self.kept.before);
        let alt = std::mem::take(&mut self.kept.alt);
        let asread = self.kept.read.take();
        // The type the file wrote, as written, while it says what the cells say (an edit of the cells wins, and the type is written from them).
        let draw_type = match asread.as_ref().and_then(|r| r.conn_type.as_ref()) {
            Some(held) if asread::type_agrees(held.as_deref(), (kind("ShapeRouteStyle"), kind("ConLineRouteExt"))) => held.clone(),
            _ => draw_type,
        };
        // **A segment of a geometry of its own that is glued is a connector of the type `line`** (the Comm-link between two shapes): another program routes a connector by its
        // type and keeps no path of the file's (`svg:d`), so `line` is the one that draws what the figure runs along, the straight line between the glue points (`standard` and `lines`
        // would bend it, `curve` too), and it keeps the end glued when a shape moves. The points of the figure are the path all the same (the standard asks it of a producer), and its geometry,
        // the cells and the master ride in DiaCraft's data: a read of the file gives the zigzag back as it was, to the last digit, and not the path's, in hundredths of a millimetre.
        let own = self.own_route(n)?;
        let draw_type = if own.is_some() { Some("line".to_owned()) } else { draw_type };
        // The cells the reader makes of the type that is written, which the model's are carried against.
        let route = Some(asread::route_of_type(draw_type.as_deref()));
        let ends = own::ends(sheet, &n.outer).ok_or(WriteError::Unsupported("a new connector whose ends are not numbers"))?;
        // **A route with line jumps is written without them** (a new connector that crossed another, as connectors glued to a flipped shape often do,
        // was refused): the file's path has no place for a hop and LibreOffice draws none, so the path is the route's corners, and the rows the session has (the
        // hops) are tier 2, which DiaCraft reads back. A route of any other row (a rounded corner, a deleted row) is refused as before.
        // **A curved route** (the engine's: quadratic `NURBSTo` rows) is written as the cubics it is (the second refusal), and its rows are tier 2 as a hop's are.
        // **The route the file held is written as it was** while the rows are as they were read and the ends are where the file had them; a connector the file wrote
        // no path for, whose rows are still the straight line the reader makes of it, is written with none.
        let file_route: Option<Vec<(String, String)>> = asread.as_ref().filter(|r| self.ends_as_read(r, ends)).and_then(|r| r.route.clone());
        let no_path = asread.as_ref().is_some_and(|r| !r.route_held) && is_line_outline(sheet);
        let (segs, hops) = if file_route.is_some() || no_path {
            (Vec::new(), false)
        } else if let Some(figure) = own.clone() {
            (figure, false)
        } else {
            match route::of(sheet, &n.outer) {
                Some(segs) => (segs, false),
                None => match route::without_hops(sheet, &n.outer, &mut self.work.cells_compared) {
                    Some(segs) => (segs, true),
                    None => (
                        route::quadratic(sheet, &n.outer)
                            .ok_or(WriteError::Unsupported("a new connector whose route is not a path the writer writes"))?,
                        true,
                    ),
                },
            }
        };
        let (path, view_box) = if file_route.is_some() || no_path {
            (String::new(), String::new())
        } else {
            route::text(&segs, diff.height).ok_or(WriteError::Unsupported("a new connector whose route is not a path the writer writes"))?
        };
        let (paragraphs, holds_text, read, plain, text_native) = self.paragraphs_text(n, pfx)?;
        let (paragraphs, read) =
            if !holds_text && asread.as_ref().is_some_and(|r| !r.bare_text) { (String::new(), Vec::new()) } else { (paragraphs, read) };
        let (_, mut ops, mut style_names) = native_style_as(sheet, &CONNECTOR_CELLS, asread.is_some(), true);
        super::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = if asread.is_some() { self.in_cell_units(ops, sheet) } else { ops };
        let display = super::fx::display_of(sheet, &ops);
        let glued_end =
            |begin: bool| connects.iter().any(|c| c.from_cell.as_deref().is_some_and(|f| f.starts_with("Begin") == begin) && c.to_part != Some(3));
        let layer = diff.layers.of_shape(sheet);
        let (layer_text, layer_row) = match &asread {
            Some(r) => (r.layer.clone(), r.layer.as_deref().and_then(|name| diff.layers.row_named(name))),
            None => (Some(layer.name.clone()), layer.row),
        };
        let native = native::Native {
            alt: Some((alt.0.as_str(), alt.1.as_str())),
            frame: native::Frame::OneD,
            style: &style_names,
            display: Some(&display),
            // (The figure's own rows are carried: the reader makes rows of the path, which is in a coarser unit.)
            geometry: !hops && own.is_none(),
            outline_sections: 1,
            picture: false,
            connection: !sheet.sections.iter().any(|s| s.name.as_str() == "Connection") || standard_connection(sheet),
            connector: true,
            route,
            glued: [glued_end(true), glued_end(false)],
            text: &plain,
            text_native,
            format: self.format_written(diff, n, &plain, text_native),
            picture_bytes: None,
            expected: diacraft_doc::ShapeType::Shape,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: None,
        };
        let (tier2, dia) = self.extension_of(n, &native, tc)?;
        let (graphic, paragraph) = match &asread {
            Some(_) => (self.graphic_style(asread.as_ref(), ops, &dia, parent.as_deref())?, None),
            None => {
                let (graphic, paragraph) = self.new_styles_on(ops, false, &dia, parent.as_deref())?;
                (Some(graphic), Some(paragraph))
            }
        };
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let mut tag = format!("<{0}:connector{1}", pfx.draw, pfx.declare);
        // **A connector another connector is glued to is named**: the end that names it was written with the id given it, and a connector written without one would leave that end
        // naming nothing, so that the file read back has it free (its glue lost, its ends values) where the drawing had it glued.
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), paragraph.as_deref(), layer_text.as_deref()), z, id, &mut wrote));
        if let Some(t) = draw_type {
            tag.push_str(&pfx.attr(Ns::Draw, "type", &t));
        }
        for (start, label) in [(true, "start"), (false, "end")] {
            let Some(c) = connects.iter().find(|c| c.from_cell.as_deref().is_some_and(|f| f.starts_with("Begin") == start)) else { continue };
            let target = id_of(c.to_sheet).ok_or(WriteError::Unsupported("a connector glued to a shape the writer cannot name"))?;
            tag.push_str(&pfx.attr(Ns::Draw, &format!("{label}-shape"), &target));
            match c.to_part {
                Some(3) => {}
                Some(p) if p >= 100 => {
                    let attr = format!("{label}-glue-point");
                    let id = glue_of(c.to_sheet, p - 100)
                        .ok_or(WriteError::Unsupported("a connector glued to a connection point past the ones a shape can hold"))?;
                    tag.push_str(&pfx.attr(Ns::Draw, &attr, &id.to_string()));
                    wrote.push((Ns::Draw, if start { "start-glue-point" } else { "end-glue-point" }));
                }
                _ => return refuse("a connection of a kind the writer does not know"),
            }
        }
        if asread.is_some() {
            tag.push_str(&self.ends_attrs(asread.as_ref(), sheet, ends, pfx, &mut wrote)?);
        } else {
            let values = [("x1", ends[0].0), ("y1", diff.height - ends[0].1), ("x2", ends[1].0), ("y2", diff.height - ends[1].1)];
            for (local, v) in values {
                tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(v)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
        }
        // The route as a path is ODF 1.2's: a connector of a part of ODF 1.0 or 1.1 is its ends (the schemas of those versions have neither
        // `svg:d` nor `svg:viewBox` on a `draw:connector`).
        if let Some(held) = &file_route {
            tag.push_str(&self.held_route(pfx, held)?);
        } else if no_path {
        } else if !self.legacy {
            tag.push_str(&pfx.attr(Ns::Svg, "d", &path));
            tag.push_str(&pfx.attr(Ns::Svg, "viewBox", &view_box));
            wrote.push((Ns::Svg, "d"));
            wrote.push((Ns::Svg, "viewBox"));
        }
        self.work.charge_attribute(self.limits)?;
        let tier2 = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        Ok((
            format!("{tag}>{kept_before}{paragraphs}{kept_body}</{}:connector>", pfx.draw),
            NewWrote { attrs: wrote, paras: read, tier2, ..NewWrote::default() },
        ))
    }
}

/// The connection points ODF gives a frame: the four sides' middles.
const FRAME_POINTS: u32 = 4;

/// The row of a frame's own glue point a connection names by its part (`to_part`): the part less 100, from the row past the [`FRAME_POINTS`] ODF gives every frame; `None` for a
/// connection to no part, to one below 100, or to one of the four.
pub(crate) fn past_frame_point(to_part: Option<u32>) -> Option<u32> {
    to_part?.checked_sub(100).filter(|row| *row >= FRAME_POINTS)
}

/// Whether a new shape is a picture: a foreign shape that holds one (a package or a flat drawing writes it as a frame holding the picture).
pub(crate) fn is_new_picture(shape: &diacraft_doc::Shape) -> bool {
    shape.kind == diacraft_doc::ShapeType::Foreign && shape.foreign.as_ref().is_some_and(|f| f.picture.is_some())
}

/// Which element a new shape is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Custom,
    Line,
    Connector,
    /// A new group: a `draw:g` holding its members.
    Group,
    /// A `draw:frame` holding a picture inline: a new picture of a flat drawing.
    Picture,
    /// A shape read from a plain element of an `.odg` (`draw:rect`, `draw:ellipse`, `draw:polygon`, a text frame, ...), written as that element.
    Plain,
    /// A one-dimensional shape of a geometry of its own (a zigzag, a loop), written as the outline it draws: a `draw:polyline`, or a `draw:path` when a row is a curve, on the page
    /// ([`super::poly`]). Its frame, its ends and its geometry in their own axes are tier 2.
    Poly,
}

/// The attributes of the file's own, by the name the reader keeps them under, that a new element of this kind has no place for **and does not
/// miss**: the ones this writer writes itself (so the shape has them, as its own) and the ids it never copies (an id names one element; a copy has
/// the id of the shape it was copied from, and the writer gives a new shape one only when a connector names it).
fn written_attributes(kind: Kind) -> &'static [&'static str] {
    const CUSTOM: &[&str] =
        &["draw:style-name", "draw:text-style-name", "draw:layer", "draw:name", "xml:id", "draw:id", "svg:width", "svg:height", "svg:x", "svg:y"];
    const LINE: &[&str] =
        &["draw:style-name", "draw:text-style-name", "draw:layer", "draw:name", "xml:id", "draw:id", "svg:x1", "svg:y1", "svg:x2", "svg:y2"];
    const CONNECTOR: &[&str] = &[
        "draw:style-name",
        "draw:text-style-name",
        "draw:layer",
        "draw:name",
        "xml:id",
        "draw:id",
        "draw:type",
        "draw:start-shape",
        "draw:start-glue-point",
        "draw:end-shape",
        "draw:end-glue-point",
        "svg:x1",
        "svg:y1",
        "svg:x2",
        "svg:y2",
        "svg:d",
        "svg:viewBox",
    ];
    const GROUP: &[&str] = &["draw:name", "xml:id", "draw:id", "draw:z-index"];
    const POLY: &[&str] = &[
        "draw:style-name",
        "draw:text-style-name",
        "draw:layer",
        "draw:name",
        "xml:id",
        "draw:id",
        "svg:x",
        "svg:y",
        "svg:width",
        "svg:height",
        "svg:viewBox",
        "draw:points",
        "svg:d",
    ];
    match kind {
        Kind::Poly => POLY,
        Kind::Custom | Kind::Picture | Kind::Plain => CUSTOM,
        Kind::Line => LINE,
        Kind::Connector => CONNECTOR,
        Kind::Group => GROUP,
    }
}

/// Whether a node of a shape's kept children is something this writer writes in another form: the geometry element of a custom shape, which it
/// writes again from the shape's `Geometry` section.
fn written_element(kind: Kind, node: &diacraft_doc::RawNode) -> bool {
    matches!((kind, node), (Kind::Custom, diacraft_doc::RawNode::Element(e)) if &*e.name == "draw:enhanced-geometry")
}

/// What a new shape holds that this save does not write, as a note, or `None` when it holds nothing of the kind.
/// `model_left` is, on the model path, what [`Editor::kept_markup`] left out of the markup the shape kept of its file (attributes, and elements and other nodes), which is all of it that
/// is not written there.
fn unwritten(page: usize, shape: &diacraft_doc::Shape, kind: Kind, model_left: Option<(usize, usize)>) -> Option<crate::SaveNote> {
    let written = written_attributes(kind);
    // (A declaration the shape kept beside an attribute is no attribute of its own.)
    let missing =
        |raw: &diacraft_doc::RawAttrs| raw.iter().filter(|(name, _)| !written.contains(name) && super::kept::declared_by(name).is_none()).count();
    let nodes = |kept: &diacraft_doc::Passthrough| {
        kept.iter()
            .filter(|p| match &p.node {
                diacraft_doc::RawNode::Text(t) => !t.trim().is_empty(),
                node => !written_element(kind, node),
            })
            .count()
    };
    // What the shape keeps of an `.odg` it was copied from (its `odf` slot) is as unwritten as what a VSDX shape keeps in `raw` and `passthrough`.
    // (The model path writes back what the shape keeps of the file it was read from, and counts what it left out: `Editor::kept_markup`.)
    let (odf_raw, odf_kept) = match model_left {
        Some(left) => left,
        None => shape.ext.get().map_or((0, 0), |d| (missing(&d.odf.kept.raw), nodes(&d.odf.kept.kept))),
    };
    // What the VSDX file held of the shape that no ODF markup says (its attributes and elements, `Data1` to `Data3`, its foreign data) rides in the shape's own style, and is no
    // part of what the file leaves out: only what the shape keeps of an `.odg` it was copied from is.
    let (attributes, elements, data, foreign) = (odf_raw, odf_kept, false, false);
    (attributes > 0 || elements > 0).then_some(crate::SaveNote::NewShapeUnwritten { page, shape: shape.id, attributes, elements, data, foreign })
}

/// The static name of an attribute the writer writes, for the check of the bytes made.
fn static_name(local: &str) -> &'static str {
    match local {
        "x" => "x",
        "y" => "y",
        "width" => "width",
        "height" => "height",
        "x1" => "x1",
        "y1" => "y1",
        "x2" => "x2",
        "y2" => "y2",
        "z-index" => "z-index",
        _ => "d",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of an id: `id` and one to eighteen ASCII digits, and nothing else has one; the number is the digits' own, whatever zeros lead them.
    #[test]
    fn an_id_has_a_number_when_it_is_id_and_one_to_eighteen_digits() {
        for (id, number) in [("id0", 0), ("id1", 1), ("id007", 7), ("id12", 12), ("id999999999999999999", 999_999_999_999_999_999)] {
            assert_eq!(id_number(id), Some(number), "{id}");
        }
        for id in [
            "",
            "id",
            "i",
            "d1",
            "ID1",
            "id-1",
            "id+1",
            "id1a",
            "id 1",
            "id\u{0661}",
            "xid1",
            "id1000000000000000000",
            "id00000000000000000001",
            "1",
            "other",
        ] {
            assert_eq!(id_number(id), None, "{id:?}");
        }
    }

    /// A row of the cells `cells`, each a constant, or with the formula it is given, and the type `kind`.
    fn row(kind: &str, cells: &[(&str, Option<&str>, &str)]) -> diacraft_doc::Row {
        let mut r = diacraft_doc::Row { row_type: Some(std::sync::Arc::from(kind)), ..diacraft_doc::Row::default() };
        for (name, formula, value) in cells {
            r.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::read(*formula, Some(value), None, None, &mut 1_000));
        }
        r
    }

    /// A sheet that holds the geometry sections `sections`, each of the rows it is given.
    fn sheet_of(sections: Vec<(Option<u32>, Vec<diacraft_doc::Row>)>) -> Sheet {
        let mut sheet = Sheet::default();
        for (ix, rows) in sections {
            let mut section = diacraft_doc::Section::new("Geometry", ix);
            for r in rows {
                section.rows.push(std::sync::Arc::new(r));
            }
            sheet.sections.push(std::sync::Arc::new(section));
        }
        sheet
    }

    /// A new shape's style cell that the attributes cannot say is left out (it is tier 2), and the cells it is written with go with it only when they are a group:
    /// a dashed line leaves the colour and the margins written; a margin that is no length takes the four margins; a line end that is no number takes its size.
    #[test]
    fn a_style_cell_a_new_shape_cannot_write_takes_with_it_only_the_cells_of_its_group() {
        let sheet = |cells: &[(&str, &str, Option<&str>)]| {
            let mut s = Sheet::default();
            for (name, value, unit) in cells {
                s.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::read(None, Some(value), *unit, None, &mut 1_000));
            }
            s
        };
        let names = |cells: &[(&str, &str, Option<&str>)]| {
            let (_, _, mut names) = native_style(&sheet(cells), &[]);
            names.sort_unstable();
            names
        };
        let margins = [
            ("TopMargin", "0.1", Some("IN")),
            ("BottomMargin", "0.1", Some("IN")),
            ("LeftMargin", "0.1", Some("IN")),
            ("RightMargin", "0.1", Some("IN")),
        ];
        // A custom pattern (254, which the table has no row for) is left out, and the colour and the margins beside it are written.
        let mut with_custom = vec![("LinePattern", "254", None), ("LineColor", "#aabbcc", None)];
        with_custom.extend(margins);
        assert_eq!(names(&with_custom), ["BottomMargin", "LeftMargin", "LineColor", "RightMargin", "TopMargin"]);
        let (edit, ..) = native_style(&sheet(&with_custom), &[]);
        assert_eq!(edit.margins.len(), 4, "the margins are written whole");
        // A pattern of the table is a dash of DiaCraft's own, written with them.
        let mut with_dash = vec![("LinePattern", "2", None), ("LineColor", "#aabbcc", None)];
        with_dash.extend(margins);
        assert_eq!(names(&with_dash), ["BottomMargin", "LeftMargin", "LineColor", "LinePattern", "RightMargin", "TopMargin"]);
        // A margin that is no length takes the other three with it, and the colour stays.
        assert_eq!(names(&[("TopMargin", "abc", None), ("LeftMargin", "0.1", Some("IN")), ("LineColor", "#aabbcc", None)]), ["LineColor"]);
        // A line end that is no number takes its size with it, and the colour stays.
        assert_eq!(names(&[("BeginArrow", "2000", None), ("BeginArrowSize", "2", None), ("LineColor", "#aabbcc", None)]), ["LineColor"]);
    }

    /// The four connection rows the reader gives a rectangle are the side middles, with the directions and the type 0 where they are said, and a row that says
    /// anything else is not those: each part of a direction's cell by itself.
    #[test]
    fn the_four_side_middles_are_the_rows_with_their_shares_and_directions_and_nothing_else() {
        let cell = |formula: Option<&str>, value: &str| diacraft_doc::Cell::read(formula, Some(value), None, None, &mut 1_000);
        type Share<'a> = (Option<&'a str>, &'a str);
        let shares: [(Share<'_>, Share<'_>); 4] = [
            ((Some("Width*0.5"), "1"), (Some("Height"), "2")),
            ((None, "0"), (Some("Height*0.5"), "1")),
            ((Some("Width*0.5"), "1"), (None, "0")),
            ((Some("Width"), "2"), (Some("Height*0.5"), "1")),
        ];
        let directions = [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
        // The sheet of four rows, each given by `edit` (the row's number and the row to change).
        let sheet = |edit: &dyn Fn(usize, &mut diacraft_doc::Row)| {
            let mut section = diacraft_doc::Section::new("Connection", None);
            for (at, ((x, y), (dx, dy))) in shares.iter().zip(directions).enumerate() {
                let mut row = diacraft_doc::Row::default();
                row.cells.insert(diacraft_doc::CellName::new("X"), cell(x.0, x.1));
                row.cells.insert(diacraft_doc::CellName::new("Y"), cell(y.0, y.1));
                row.cells.insert(diacraft_doc::CellName::new("DirX"), cell(None, &dx.to_string()));
                row.cells.insert(diacraft_doc::CellName::new("DirY"), cell(None, &dy.to_string()));
                row.cells.insert(diacraft_doc::CellName::new("Type"), cell(None, "0"));
                edit(at, &mut row);
                section.rows.push(std::sync::Arc::new(row));
            }
            let mut sheet = Sheet::default();
            sheet.sections.push(std::sync::Arc::new(section));
            sheet
        };
        let put = |row: &mut diacraft_doc::Row, name: &str, c: diacraft_doc::Cell| drop(row.cells.insert(diacraft_doc::CellName::new(name), c));
        assert!(standard_connection(&sheet(&|_, _| {})), "with the directions and the type");
        assert!(
            standard_connection(&sheet(&|_, r| {
                for name in ["DirX", "DirY", "Type"] {
                    r.cells.remove(name);
                }
            })),
            "the shares alone"
        );
        let mut extra = cell(None, "0");
        extra.extra =
            Some(std::sync::Arc::new(diacraft_doc::CellExtra { text: Some(std::sync::Arc::from("t")), ..diacraft_doc::CellExtra::default() }));
        let broken: Vec<(&str, Sheet)> = vec![
            (
                "a direction with a formula",
                sheet(&|at, r| {
                    if at == 0 {
                        put(r, "DirY", cell(Some("1*1"), "1"))
                    }
                }),
            ),
            (
                "a direction with content",
                sheet(&|at, r| {
                    if at == 1 {
                        put(r, "DirY", {
                            let mut c = extra.clone();
                            c.value = Some(diacraft_doc::Value::from_text("0"));
                            c
                        })
                    }
                }),
            ),
            (
                "a direction of another value in x",
                sheet(&|at, r| {
                    if at == 3 {
                        put(r, "DirX", cell(None, "-1"))
                    }
                }),
            ),
            (
                "a direction of another value in y",
                sheet(&|at, r| {
                    if at == 0 {
                        put(r, "DirY", cell(None, "-1"))
                    }
                }),
            ),
            (
                "a type that is not 0",
                sheet(&|at, r| {
                    if at == 2 {
                        put(r, "Type", cell(None, "1"))
                    }
                }),
            ),
            (
                "a type with a formula",
                sheet(&|at, r| {
                    if at == 2 {
                        put(r, "Type", cell(Some("0*1"), "0"))
                    }
                }),
            ),
            (
                "a cell the reader does not give",
                sheet(&|at, r| {
                    if at == 1 {
                        put(r, "Foo", cell(None, "0"))
                    }
                }),
            ),
            (
                "a share that is another",
                sheet(&|at, r| {
                    if at == 3 {
                        put(r, "Y", cell(Some("Height*0.25"), "0.5"))
                    }
                }),
            ),
        ];
        for (what, sheet) in broken {
            assert!(!standard_connection(&sheet), "{what}");
        }
        assert!(!standard_connection(&Sheet::default()), "no connection section");
        let mut three = sheet(&|_, _| {});
        if let Some(section) = three.sections.first_mut() {
            std::sync::Arc::make_mut(section).rows.pop();
        }
        assert!(!standard_connection(&three), "three rows");
    }

    /// The outline the reader gives a line is one section numbered 0 of a move to the origin and a line along the frame, and each part of that by itself is what
    /// makes another outline not one.
    #[test]
    fn the_outline_of_a_line_is_one_section_of_a_move_to_the_origin_and_a_line_along_the_frame() {
        let start = || row("MoveTo", &[("X", None, "0"), ("Y", None, "0")]);
        let end = || row("LineTo", &[("X", Some("Width*1"), "1"), ("Y", None, "0")]);
        assert!(is_line_outline(&sheet_of(vec![(Some(0), vec![start(), end()])])));
        // A section of another name beside it is not a geometry section and does not count.
        let mut beside = sheet_of(vec![(Some(0), vec![start(), end()])]);
        beside.sections.push(std::sync::Arc::new(diacraft_doc::Section::new("User", None)));
        assert!(is_line_outline(&beside));
        let not_a_line: Vec<(&str, Sheet)> = vec![
            ("no section", sheet_of(vec![])),
            ("two sections", sheet_of(vec![(Some(0), vec![start(), end()]), (Some(1), vec![start(), end()])])),
            ("another index", sheet_of(vec![(Some(1), vec![start(), end()])])),
            ("no index", sheet_of(vec![(None, vec![start(), end()])])),
            ("one row", sheet_of(vec![(Some(0), vec![start()])])),
            ("three rows", sheet_of(vec![(Some(0), vec![start(), end(), end()])])),
            ("the first is no move", sheet_of(vec![(Some(0), vec![row("LineTo", &[("X", None, "0"), ("Y", None, "0")]), end()])])),
            ("the second is no line", sheet_of(vec![(Some(0), vec![start(), row("MoveTo", &[("X", Some("Width*1"), "1"), ("Y", None, "0")])])])),
            ("a start off the origin in x", sheet_of(vec![(Some(0), vec![row("MoveTo", &[("X", None, "1"), ("Y", None, "0")]), end()])])),
            ("a start off the origin in y", sheet_of(vec![(Some(0), vec![row("MoveTo", &[("X", None, "0"), ("Y", None, "1")]), end()])])),
            ("a start x that is a formula", sheet_of(vec![(Some(0), vec![row("MoveTo", &[("X", Some("0*1"), "0"), ("Y", None, "0")]), end()])])),
            (
                "an end that is not along the frame",
                sheet_of(vec![(Some(0), vec![start(), row("LineTo", &[("X", Some("Width*2"), "1"), ("Y", None, "0")])])]),
            ),
            ("an end off the frame in y", sheet_of(vec![(Some(0), vec![start(), row("LineTo", &[("X", Some("Width*1"), "1"), ("Y", None, "1")])])])),
            (
                "a cell more in the start",
                sheet_of(vec![(Some(0), vec![row("MoveTo", &[("X", None, "0"), ("Y", None, "0"), ("Z", None, "0")]), end()])]),
            ),
            (
                "a cell more in the end",
                sheet_of(vec![(Some(0), vec![start(), row("LineTo", &[("X", Some("Width*1"), "1"), ("Y", None, "0"), ("Z", None, "0")])])]),
            ),
        ];
        for (what, sheet) in not_a_line {
            assert!(!is_line_outline(&sheet), "{what}");
        }
        // A cell of the section's own is no line's.
        let mut flagged = sheet_of(vec![(Some(0), vec![start(), end()])]);
        if let Some(section) = flagged.sections.first_mut() {
            std::sync::Arc::make_mut(section)
                .cells
                .insert(diacraft_doc::CellName::new("NoFill"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        }
        assert!(!is_line_outline(&flagged), "a flag of the section");
    }

    /// A new picture is a foreign shape that holds a picture: a foreign shape with no picture (an object that shows none), a foreign shape with no foreign data and a
    /// shape of another kind that holds one are none.
    #[test]
    fn a_new_picture_is_a_foreign_shape_that_holds_a_picture_and_nothing_else_is() {
        let media = || {
            std::sync::Arc::new(diacraft_doc::Media {
                part: "p".into(),
                content_type: "image/png".into(),
                bytes: std::sync::Arc::from(&b"\x89PNG"[..]),
            })
        };
        let with = |kind: diacraft_doc::ShapeType, picture: Option<std::sync::Arc<diacraft_doc::Media>>| {
            let mut shape = diacraft_doc::Shape::new(1);
            shape.kind = kind;
            shape.foreign =
                picture.map(|p| std::sync::Arc::new(diacraft_doc::ForeignData { picture: Some(p), ..diacraft_doc::ForeignData::default() }));
            shape
        };
        assert!(is_new_picture(&with(diacraft_doc::ShapeType::Foreign, Some(media()))));
        assert!(!is_new_picture(&with(diacraft_doc::ShapeType::Shape, Some(media()))), "a shape of another kind that holds one");
        assert!(!is_new_picture(&with(diacraft_doc::ShapeType::Foreign, None)), "foreign, with no foreign data");
        let mut no_picture = with(diacraft_doc::ShapeType::Foreign, Some(media()));
        no_picture.foreign = Some(std::sync::Arc::new(diacraft_doc::ForeignData::default()));
        assert!(!is_new_picture(&no_picture), "foreign data that holds no picture");
    }

    #[test]
    fn the_name_the_check_reads_back_is_the_attribute_written() {
        for name in ["x", "y", "width", "height", "x1", "y1", "x2", "y2", "d", "z-index"] {
            assert_eq!(static_name(name), name);
        }
    }

    /// A sheet whose `Connection` section holds one row for each of `shares` (the `X` share of the width and the `Y` share of the height), numbered from 0.
    fn connection_sheet(shares: &[(f64, f64)]) -> Sheet {
        let cell = |formula: &str, value: &str| diacraft_doc::Cell::read(Some(formula), Some(value), None, None, &mut 1_000);
        let mut section = diacraft_doc::Section::new("Connection", None);
        for (at, (x, y)) in shares.iter().enumerate() {
            let mut row = diacraft_doc::Row { ix: u32::try_from(at).ok(), ..diacraft_doc::Row::default() };
            row.cells.insert(diacraft_doc::CellName::new("X"), cell(&format!("Width*{x}"), &x.to_string()));
            row.cells.insert(diacraft_doc::CellName::new("Y"), cell(&format!("Height*{y}"), &y.to_string()));
            section.rows.push(std::sync::Arc::new(row));
        }
        let mut sheet = Sheet::default();
        sheet.sections.push(std::sync::Arc::new(section));
        sheet
    }

    /// The side middles in the order the reader gives every shape (top, left, bottom, right) and four listed glue points, as the vector of lists them.
    const MIDDLES: [(f64, f64); 4] = [(0.5, 1.0), (0.0, 0.5), (0.5, 0.0), (1.0, 0.5)];

    /// A geometry that lists `n` points and says nowhere they are.
    fn count(n: usize) -> Listed {
        Listed { count: n, shares: None }
    }

    /// How many points a geometry lists ([`super::listed_glue_points`]'s count).
    fn listed_glue_points(el: &diacraft_doc::RawElement) -> usize {
        super::listed_glue_points(el).count
    }

    /// **A shape that writes the geometry its file held has the numbering its file had**: the four standard rows and the listed points from row 4 (the reader's numbering),
    /// or the listed points from row 0 when the style said so. A shape that makes its geometry from the rows numbers them as its own when it writes them so, as before.
    #[test]
    fn a_shape_that_writes_its_files_geometry_is_numbered_as_its_rows_say() {
        // The side middles, and four more points listed after them (the same places, which a shape may list as well as any).
        let beside_middles: Vec<(f64, f64)> = MIDDLES.iter().chain(MIDDLES.iter()).copied().collect();
        // The four standard rows and four listed ones: the reader's numbering when the geometry lists four.
        assert!(!rows_are_own(&connection_sheet(&beside_middles), Some(&count(4))), "4 + 4 rows, the file lists 4: glue id g is row g");
        // Four rows and four listed points: the style said `own`, whatever the rows are (here the side middles too).
        assert!(rows_are_own(&connection_sheet(&MIDDLES), Some(&count(4))), "4 rows, the file lists 4: glue id 4 + r is row r");
        assert!(rows_are_own(&connection_sheet(&[(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)]), Some(&count(3))));
        // A geometry that lists none has no rows of its own to number.
        assert!(!rows_are_own(&connection_sheet(&MIDDLES), Some(&count(0))));
        // The geometry made from the rows: own when the rows are not the four standard ones and can be written, as before.
        assert!(rows_are_own(&connection_sheet(&beside_middles), None), "4 + 4 rows are written as the geometry's own points");
        assert!(!rows_are_own(&connection_sheet(&MIDDLES), None), "the four standard rows are nothing to write");
    }

    /// **A shape of *k* listed points whose rows start with them is numbered as its own after the session added rows**: the count alone took a doubled shape (more rows than the *k*
    /// it lists) for one that has the four standard rows first, and dropped its marker and numbered its connectors' glue the other way. The geometry's own places say which: the first rows are
    /// them in place, and the rows 4 on are not.
    #[test]
    fn a_shape_whose_first_rows_are_the_listed_points_is_its_own_whatever_rows_the_session_added() {
        // The geometry lists the four side middles and two points on the top, in the view box 21600 square: the places as shares, y from the bottom.
        let geometry = diacraft_doc::RawElement {
            name: std::sync::Arc::from("draw:enhanced-geometry"),
            attrs: diacraft_doc::RawAttrs::from_pairs([
                ("svg:viewBox", "0 0 21600 21600"),
                ("draw:glue-points", "10800 0 0 10800 10800 21600 21600 10800 5400 0 16200 0"),
            ]),
            children: Vec::new(),
        };
        let listed = listed_glue_points_of(&geometry);
        assert_eq!((listed.count, listed.shares.as_ref().map(Vec::len)), (6, Some(6)));
        let six: Vec<(f64, f64)> = vec![(0.5, 1.0), (0.0, 0.5), (0.5, 0.0), (1.0, 0.5), (0.25, 1.0), (0.75, 1.0)];
        assert!(rows_are_own(&connection_sheet(&six), Some(&listed)), "the six rows are the six listed points");
        let mut more = six.clone();
        more.extend([(0.0, 0.25), (0.0, 0.75)]);
        assert!(rows_are_own(&connection_sheet(&more), Some(&listed)), "a doubling added two rows: the first six are still the listed points");
        // Four standard rows first, then the listed points: not its own, however many rows the session added after.
        let mut beside: Vec<(f64, f64)> = MIDDLES.to_vec();
        beside.extend(six.iter().copied());
        assert!(!rows_are_own(&connection_sheet(&beside), Some(&listed)), "the listed points are rows 4 on");
        beside.push((0.5, 0.25));
        assert!(!rows_are_own(&connection_sheet(&beside), Some(&listed)), "and a row more is still the reader's numbering");
        // A geometry that says its points as anything but plain numbers is judged by the count alone.
        let parametric = diacraft_doc::RawElement {
            attrs: diacraft_doc::RawAttrs::from_pairs([("svg:viewBox", "0 0 21600 21600"), ("draw:glue-points", "$0 0 0 10800 10800 21600")]),
            ..geometry.clone()
        };
        assert!(!rows_are_own(&connection_sheet(&more), Some(&listed_glue_points_of(&parametric))));
    }

    /// The same as [`listed_glue_points`], keeping where the points are.
    fn listed_glue_points_of(el: &diacraft_doc::RawElement) -> Listed {
        super::listed_glue_points(el)
    }

    #[test]
    fn the_glue_points_a_geometry_lists_are_its_pairs_of_numbers() {
        let geometry = |attrs: &[(&str, &str)]| diacraft_doc::RawElement {
            name: std::sync::Arc::from("draw:enhanced-geometry"),
            attrs: diacraft_doc::RawAttrs::from_pairs(attrs.iter().copied()),
            children: Vec::new(),
        };
        assert_eq!(listed_glue_points(&geometry(&[("draw:glue-points", "10800 0 0 10800 10800 21600 21600 10800")])), 4);
        assert_eq!(
            listed_glue_points(&geometry(&[("draw:glue-point-leaving-directions", "0 90"), ("x:glue-points", "1 2 3")])),
            1,
            "by the local name, and a pair is two numbers"
        );
        assert_eq!(listed_glue_points(&geometry(&[("draw:glue-point-leaving-directions", "0 90")])), 0);
        assert_eq!(listed_glue_points(&geometry(&[])), 0);
    }

    /// **The points a geometry lists that the reader makes rows of**: none for a geometry the reader stops at before it reads them, an extrusion, a fontwork or a view box with no
    /// width or height (the shape has the four standard rows, and the points it lists make none, so the rows are not "the listed points from row 0"); the pairs of numbers for any other.
    #[test]
    fn the_points_of_a_geometry_the_reader_stops_at_before_its_glue_points_are_none() {
        let geometry = |attrs: &[(&str, &str)]| diacraft_doc::RawElement {
            name: std::sync::Arc::from("draw:enhanced-geometry"),
            attrs: diacraft_doc::RawAttrs::from_pairs(attrs.iter().copied()),
            children: Vec::new(),
        };
        let points = ("draw:glue-points", "10800 0 0 10800 10800 21600 21600 10800");
        assert_eq!(listed_glue_points(&geometry(&[points, ("draw:extrusion", "true")])), 0, "an extrusion");
        assert_eq!(listed_glue_points(&geometry(&[points, ("draw:text-path", "true")])), 0, "a fontwork");
        assert_eq!(listed_glue_points(&geometry(&[points, ("x:extrusion", "true")])), 0, "by the local name, as the glue points are told");
        assert_eq!(listed_glue_points(&geometry(&[points, ("svg:viewBox", "0 0 0 21600")])), 0, "a view box with no width");
        assert_eq!(listed_glue_points(&geometry(&[points, ("svg:viewBox", "0 0 21600 -1")])), 0, "a view box with no height");
        assert_eq!(listed_glue_points(&geometry(&[points, ("draw:extrusion", "false")])), 4, "an extrusion switched off");
        assert_eq!(listed_glue_points(&geometry(&[points, ("draw:extrusion", "1")])), 4, "the reader reads `true` and `false` only");
        assert_eq!(listed_glue_points(&geometry(&[points, ("svg:viewBox", "0 0 21600 21600")])), 4, "a view box that maps onto the frame");
        assert_eq!(
            listed_glue_points(&geometry(&[points, ("svg:viewBox", "not a box")])),
            4,
            "an unreadable view box is the default box, and the points are read"
        );
    }

    /// A kept image's link is a link out of the package when it is a path in the package that names no entry, by the book of the save's names; any other link, and a name the
    /// book holds, are as they were.
    #[test]
    fn a_kept_images_link_is_made_a_link_out_of_the_package_only_when_the_package_has_no_entry_of_that_name() {
        use super::super::picture::{PictureBook, out_of_the_package};
        assert_eq!(out_of_the_package("plan.svg").as_deref(), Some("../plan.svg"));
        assert_eq!(out_of_the_package("Pictures/p1.svg").as_deref(), Some("../Pictures/p1.svg"));
        assert_eq!(out_of_the_package("./plan.svg").as_deref(), Some("../plan.svg"), "a leading ./ is the package's own folder");
        for outside in [
            "",
            "../plan.svg",
            "/tmp/plan.svg",
            "#Page 2",
            "http://example.org/plan.svg",
            "file:///tmp/plan.svg",
            "ObjectReplacements/Object 1",
            "./ObjectReplacements/Object 1",
        ] {
            assert_eq!(out_of_the_package(outside), None, "{outside:?} is no path in the package");
        }
        let book = PictureBook::default();
        assert!(
            book.holds_name("../plan.svg") && book.holds_name("http://example.org/a.png") && book.holds_name(""),
            "a link that is no path in the package is held"
        );
        assert!(!book.holds_name("plan.svg") && !book.holds_name("Pictures/p1.svg"), "an empty book holds no name");
    }
}
