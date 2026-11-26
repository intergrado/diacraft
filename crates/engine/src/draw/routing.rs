//! The page's routing settings: what the Layout and Routing tab of Page Setup reads and writes.
//!
//! ours: the tab's mapping of controls to cells is "our reading from the cells' definitions":
//!
//! | control | cell | words |
//! |---|---|---|
//! | Style and Direction | `RouteStyle` | the codes of ([`RouteStyle`]) |
//! | Separate | `LineAdjustFrom` | 0 `unrelated`, 1 `all`, 2 `none`, 3 `by_style` |
//! | Overlap | `LineAdjustTo` | 0 `by_style`, 1 `all`, 2 `none`, 3 `related` |
//! | Appearance | `LineRouteExt` | 1 `straight`, 2 `curved` (0 reads as straight) |
//! | Move other shapes away on drop | `PlowCode` | 0 or 1 |
//! | Enable connector splitting | `PageShapeSplit` | 0 or 1 |
//! | Space between shapes | `AvenueSizeX`, `AvenueSizeY` | |
//! | Average shape size | `BlockSizeX`, `BlockSizeY` | |
//! | Connector to connector | `LineToLineX`, `LineToLineY` | |
//! | Connector to shape | `LineToNodeX`, `LineToNodeY` | |
//!
//! * **What a page reads** ([`page_routing`]): each cell through inheritance (the root style holds them in every file); a number outside a cell's list reads as the root's 0
//!   A spacing cell that is no number, below 0 or found nowhere is the root's US value (1/8 inch for the two connector gaps, 3/8 for the space between shapes and 1/4 for
//!   the shape size), in the page's drawing units as the jump size takes its own (`jumps.rs`); a connector gap of 0 is 0, and a space between shapes or a shape size must be more than
//!   0, as the layout reads them. Each setting says whether **the page holds it** (a cell of the page's own sheet) or reads it from the root or the default.
//! * **What it writes** ([`set_page_routing`]): the cells the call names, **each once and in one write**, and only those that read differently now; a call that writes nothing
//!   changes nothing. The lengths are **drawing units** (the cell holds a length of the drawing, which is paper's divided by the page's scale), which the command makes from
//!   its paper inches.
//! * (ours, a design call: the project rule follows the reading that a change of the page's routing policy asks for a reroute, and
//!   says the tab changes only dynamic connectors). A call that gives the page **another routing style** (not the same style again, and not a 0 made a 1)
//!   asks a new route of every connector of the page that **follows the page** (its own `ShapeRouteStyle` is 0), **is set to reroute freely** (`ConFixedCode` reads as 0) and is not on a
//!   locked layer, as a command about the connector does (`settle_forced`). A connector with a style of its own, one set to 1, 2 or 3 (3 is also the mark of a hand edit) and one on a
//!   locked layer are left as they are. **Nothing else routes anything:** not the appearance (it "concerns new connectors", and a connector takes it at its next
//!   route), not the spacing cells ("the next route keeps the new gap, and paths already saved stay as they are"), not Separate or Overlap, and not the Connectors menu
//!   with nothing selected (`connector.style` without `ids`: it "sets the default for new connectors").

use std::collections::HashSet;

use diacraft_doc::{CellAddr, CellEdit, Document, SheetRef, Source, Value};

use super::jump_settings::{LineJumpSettings, line_jump_settings};
use super::layers::Locks;
use super::read::Reader;
use super::reroute::Reroute;
use super::scan;
use super::settle::settle_forced;
use super::style::{RouteStyle, route_style_of};
use super::{MAX_COORD, Problem, require_page, write_cells};
use crate::stencil::number_text;

/// The root's US values of the spacing cells, in drawing units, for a page that reads none (1/8, 1/4 and 3/8 inch).
const DEFAULT_LINE_TO_LINE: f64 = 0.125;
const DEFAULT_LINE_TO_NODE: f64 = 0.125;
const DEFAULT_BETWEEN_SHAPES: f64 = 0.375;
const DEFAULT_SHAPE_SIZE: f64 = 0.25;

/// Numbers closer than this are the same.
const SAME: f64 = 1.0e-9;

/// The whole number a cell says, from 0 to 255: `None` for one that is no number, is between two or is outside.
fn whole(code: f64) -> Option<u8> {
    let nearest = code.round();
    (code.is_finite() && (code - nearest).abs() < 0.5 && (0.0..=255.0).contains(&nearest)).then_some(nearest as u8)
}

/// Which connectors running over each other are pushed apart: `LineAdjustFrom`, the tab's Separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separate {
    /// 0: those with no shape in common.
    Unrelated,
    /// 1: all.
    All,
    /// 2: none.
    NoLines,
    /// 3: what the routing style says.
    ByStyle,
}

impl Separate {
    pub const fn code(self) -> u8 {
        match self {
            Separate::Unrelated => 0,
            Separate::All => 1,
            Separate::NoLines => 2,
            Separate::ByStyle => 3,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Separate::Unrelated => "unrelated",
            Separate::All => "all",
            Separate::NoLines => "none",
            Separate::ByStyle => "by_style",
        }
    }

    /// What a cell says: a number outside 0 to 3 reads as the root's 0.
    pub fn of(code: f64) -> Separate {
        match whole(code) {
            Some(1) => Separate::All,
            Some(2) => Separate::NoLines,
            Some(3) => Separate::ByStyle,
            _ => Separate::Unrelated,
        }
    }
}

/// Which nearby connectors are laid over each other: `LineAdjustTo`, the tab's Overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    /// 0: what the routing style says.
    ByStyle,
    /// 1: any that run close.
    All,
    /// 2: none.
    NoLines,
    /// 3: those with a shape in common.
    Related,
}

impl Overlap {
    pub const fn code(self) -> u8 {
        match self {
            Overlap::ByStyle => 0,
            Overlap::All => 1,
            Overlap::NoLines => 2,
            Overlap::Related => 3,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Overlap::ByStyle => "by_style",
            Overlap::All => "all",
            Overlap::NoLines => "none",
            Overlap::Related => "related",
        }
    }

    /// What a cell says: a number outside 0 to 3 reads as the root's 0.
    pub fn of(code: f64) -> Overlap {
        match whole(code) {
            Some(1) => Overlap::All,
            Some(2) => Overlap::NoLines,
            Some(3) => Overlap::Related,
            _ => Overlap::ByStyle,
        }
    }
}

/// Whether new connectors are straight-legged or curved: `LineRouteExt`, the tab's Appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    /// 1, and 0 as the default.
    Straight,
    /// 2.
    Curved,
}

impl Appearance {
    /// The code a write gives it (a 0 that is read as straight is written as 1, as the Connectors menu writes it).
    pub const fn code(self) -> u8 {
        match self {
            Appearance::Straight => 1,
            Appearance::Curved => 2,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Appearance::Straight => "straight",
            Appearance::Curved => "curved",
        }
    }

    pub fn of(code: f64) -> Appearance {
        if whole(code) == Some(2) { Appearance::Curved } else { Appearance::Straight }
    }
}

/// Which of a page's settings the page holds itself (a cell of its own sheet) and which it reads from the root or finds nowhere (the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub style: bool,
    pub separate: bool,
    pub overlap: bool,
    pub appearance: bool,
    pub line_jumps: bool,
    pub plow: bool,
    pub split: bool,
    pub between_shapes: bool,
    pub shape_size: bool,
    pub connector_to_connector: bool,
    pub connector_to_shape: bool,
}

/// What a page says of its routing: the Layout and Routing tab's values. Lengths are the page's own units (**drawing units**), a pair being along x and along y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRouting {
    pub style: RouteStyle,
    pub separate: Separate,
    pub overlap: Overlap,
    pub appearance: Appearance,
    pub line_jumps: LineJumpSettings,
    /// `PlowCode`: shapes move aside for one that lands or moves close by (not acted on yet).
    pub plow: bool,
    /// `PageShapeSplit`: a connector may be split by a shape dropped on it (not acted on yet).
    pub split: bool,
    /// `AvenueSizeX` and `AvenueSizeY`: the tab's Space between shapes.
    pub between_shapes: (f64, f64),
    /// `BlockSizeX` and `BlockSizeY`: Average shape size.
    pub shape_size: (f64, f64),
    /// `LineToLineX` and `LineToLineY`: Connector to connector.
    pub connector_to_connector: (f64, f64),
    /// `LineToNodeX` and `LineToNodeY`: Connector to shape.
    pub connector_to_shape: (f64, f64),
    pub held: Held,
}

/// Whether the page's own sheet holds the cell (and not the root style or the default).
fn holds(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> bool {
    reader
        .resolver()
        .resolve(sheet, &CellAddr::direct(name))
        .is_some_and(|found| matches!(found.formula.source, Source::Local) || matches!(found.value.source, Source::Local))
}

/// What page `page` says of its routing. A page that is not there says the defaults.
pub fn page_routing(doc: &Document, page: u32) -> PageRouting {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let value = |name: &str| reader.number(&sheet, name);
    let any = |names: &[&str]| names.iter().any(|name| holds(&reader, &sheet, name));
    // A length is the cell's where there is one: a number above 0 (or 0 where `zero` allows it), else the root's.
    let length = |name: &str, default: f64, zero: bool| {
        let v = value(name);
        let usable = v.is_finite() && (v > 0.0 || (zero && v == 0.0 && reader.is_set(&sheet, &CellAddr::direct(name))));
        if usable { v } else { default }
    };
    let pair = |(x, y): (&str, &str), default: f64, zero: bool| (length(x, default, zero), length(y, default, zero));
    PageRouting {
        style: route_style_of(value("RouteStyle")),
        separate: Separate::of(value("LineAdjustFrom")),
        overlap: Overlap::of(value("LineAdjustTo")),
        appearance: Appearance::of(value("LineRouteExt")),
        line_jumps: line_jump_settings(doc, page),
        plow: value("PlowCode") != 0.0,
        split: value("PageShapeSplit") != 0.0,
        between_shapes: pair(("AvenueSizeX", "AvenueSizeY"), DEFAULT_BETWEEN_SHAPES, false),
        shape_size: pair(("BlockSizeX", "BlockSizeY"), DEFAULT_SHAPE_SIZE, false),
        connector_to_connector: pair(("LineToLineX", "LineToLineY"), DEFAULT_LINE_TO_LINE, true),
        connector_to_shape: pair(("LineToNodeX", "LineToNodeY"), DEFAULT_LINE_TO_NODE, true),
        held: Held {
            style: any(&["RouteStyle"]),
            separate: any(&["LineAdjustFrom"]),
            overlap: any(&["LineAdjustTo"]),
            appearance: any(&["LineRouteExt"]),
            line_jumps: any(&["LineJumpCode", "LineJumpStyle", "LineJumpFactorX", "LineJumpFactorY", "PageLineJumpDirX", "PageLineJumpDirY"]),
            plow: any(&["PlowCode"]),
            split: any(&["PageShapeSplit"]),
            between_shapes: any(&["AvenueSizeX", "AvenueSizeY"]),
            shape_size: any(&["BlockSizeX", "BlockSizeY"]),
            connector_to_connector: any(&["LineToLineX", "LineToLineY"]),
            connector_to_shape: any(&["LineToNodeX", "LineToNodeY"]),
        },
    }
}

/// A pair of lengths along x and along y, each given or left alone, in drawing units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pair {
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// What a call changes of a page's routing: each setting that is set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoutingChange {
    pub style: Option<RouteStyle>,
    pub separate: Option<Separate>,
    pub overlap: Option<Overlap>,
    pub appearance: Option<Appearance>,
    pub plow: Option<bool>,
    pub split: Option<bool>,
    pub between_shapes: Pair,
    pub shape_size: Pair,
    pub connector_to_connector: Pair,
    pub connector_to_shape: Pair,
}

impl RoutingChange {
    /// Whether it changes nothing: no setting is set.
    pub fn is_empty(&self) -> bool {
        *self == RoutingChange::default()
    }
}

/// What [`set_page_routing_routed`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutingDone {
    /// Whether a cell was written.
    pub changed: bool,
    /// How many connectors were asked to route again: they are routed within the edit's limit, and the answer of the command says the ones left.
    pub routed: usize,
}

/// A page cell's edit for a number.
fn number_edit(n: f64) -> CellEdit {
    CellEdit::Value(Value::from_text(&number_text(n)))
}

/// A length to write: a number above 0 and within the farthest a coordinate may be.
fn length_ok(name: &str, v: f64) -> Result<f64, Problem> {
    if v.is_finite() && v > 0.0 && v <= MAX_COORD {
        Ok(v)
    } else {
        Err(Problem::bad(format!("`{name}` must be a number more than 0 and within {MAX_COORD} drawing units")))
    }
}

/// [`set_page_routing_routed`], → whether a cell was written.
pub fn set_page_routing(doc: &mut Document, page: u32, change: &RoutingChange) -> Result<bool, Problem> {
    set_page_routing_routed(doc, page, change).map(|done| done.changed)
}

/// Writes the settings `change` sets as the page's own cells, **in one write**, and, when the page's routing style is another, asks the connectors that follow
/// the page to route again. A length that is not above 0 and within [`MAX_COORD`] is refused in words, and nothing is written.
pub fn set_page_routing_routed(doc: &mut Document, page: u32, change: &RoutingChange) -> Result<RoutingDone, Problem> {
    require_page(doc, page)?;
    let before = doc.clone();
    let sheet = SheetRef::Page(page);
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let restyled = {
        let reader = Reader::new(doc);
        let old = route_style_of(reader.number(&sheet, "RouteStyle"));
        let mut put = |name: &'static str, wanted: f64| {
            if (reader.number(&sheet, name) - wanted).abs() > SAME {
                writes.push((sheet.clone(), CellAddr::direct(name), number_edit(wanted)));
            }
        };
        if let Some(style) = change.style {
            put("RouteStyle", f64::from(style.code()));
        }
        if let Some(separate) = change.separate {
            put("LineAdjustFrom", f64::from(separate.code()));
        }
        if let Some(overlap) = change.overlap {
            put("LineAdjustTo", f64::from(overlap.code()));
        }
        if let Some(appearance) = change.appearance {
            put("LineRouteExt", f64::from(appearance.code()));
        }
        if let Some(on) = change.plow {
            put("PlowCode", f64::from(u8::from(on)));
        }
        if let Some(on) = change.split {
            put("PageShapeSplit", f64::from(u8::from(on)));
        }
        let pairs = [
            (("AvenueSizeX", "AvenueSizeY"), change.between_shapes),
            (("BlockSizeX", "BlockSizeY"), change.shape_size),
            (("LineToLineX", "LineToLineY"), change.connector_to_connector),
            (("LineToNodeX", "LineToNodeY"), change.connector_to_shape),
        ];
        for ((x, y), pair) in pairs {
            for (name, v) in [(x, pair.x), (y, pair.y)] {
                if let Some(v) = v {
                    put(name, length_ok(name, v)?);
                }
            }
        }
        change.style.is_some_and(|style| style != old)
    };
    if writes.is_empty() {
        return Ok(RoutingDone { changed: false, routed: 0 });
    }
    write_cells(doc, writes)?;
    let mut routed = 0;
    if restyled {
        let ids = followers(doc, page)?;
        if !ids.is_empty() {
            // The connectors are the command's subject, so they are routed whatever else holds (a hand edit and a fixed code are not among them: they were left out above).
            settle_forced(doc, &before, page, &[], &ids, &ids)?;
            routed = ids.len();
        }
    }
    Ok(RoutingDone { changed: true, routed })
}

/// The connectors of the page a new page style asks to route again, by `ID`: one a command could route (made by DiaCraft, or glued at an end), whose own
/// `ShapeRouteStyle` is 0, set to reroute freely and not on a locked layer. One walk of the page.
fn followers(doc: &Document, page: u32) -> Result<Vec<u32>, Problem> {
    let Some(p) = doc.page(page) else { return Ok(Vec::new()) };
    let glued: HashSet<u32> =
        p.connects.iter().filter(|c| matches!(c.from_cell.as_deref(), Some("BeginX" | "BeginY" | "EndX" | "EndY"))).map(|c| c.from_sheet).collect();
    let found = scan::scan_every(doc, page)?;
    let reader = Reader::new(doc);
    let mut locks = Locks::default();
    let mut ids: Vec<u32> = Vec::new();
    for (id, at) in &found {
        let Some(shape) = doc.shape(at) else { continue };
        // A connector is glued at an end or made by DiaCraft (a `User` row): the cheap test before any cell is looked up.
        if !glued.contains(id) && !shape.sheet.sections.iter().any(|s| s.name.as_str() == "User") {
            continue;
        }
        let sheet = SheetRef::Shape(at.clone());
        if !reader.is_one_d(&sheet) || !(glued.contains(id) || reader.is_routed(&sheet)) {
            continue;
        }
        if reader.number(&sheet, "ShapeRouteStyle") != 0.0 || Reroute::from_code(reader.number(&sheet, "ConFixedCode")) != Reroute::Freely {
            continue;
        }
        if locks.of(doc, page, shape)?.is_some() {
            continue;
        }
        ids.push(*id);
    }
    ids.sort_unstable();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn each_list_of_the_tab_goes_to_its_codes_and_words_and_back() {
        for code in 0..=3_u8 {
            let separate = Separate::of(f64::from(code));
            let overlap = Overlap::of(f64::from(code));
            assert_eq!((separate.code(), overlap.code()), (code, code));
        }
        let words: Vec<&str> = (0..=3).map(|c| Separate::of(f64::from(c)).word()).collect();
        assert_eq!(words, ["unrelated", "all", "none", "by_style"]);
        let words: Vec<&str> = (0..=3).map(|c| Overlap::of(f64::from(c)).word()).collect();
        assert_eq!(words, ["by_style", "all", "none", "related"]);
        // Outside 0 to 3 is the root's 0.
        for odd in [4.0, 7.0, 255.0, -1.0, 1.5, f64::NAN, f64::INFINITY, 1.0e300] {
            assert_eq!((Separate::of(odd), Overlap::of(odd)), (Separate::Unrelated, Overlap::ByStyle), "{odd}");
        }
        // 0 and 1 are straight, 2 is curved, and the tab writes 1 and 2.
        assert_eq!(
            [0.0, 1.0, 2.0, 3.0, 9.0].map(Appearance::of),
            [Appearance::Straight, Appearance::Straight, Appearance::Curved, Appearance::Straight, Appearance::Straight]
        );
        assert_eq!((Appearance::Straight.code(), Appearance::Curved.code()), (1, 2));
    }

    #[test]
    fn a_change_that_sets_nothing_is_empty() {
        assert!(RoutingChange::default().is_empty());
        for change in [
            RoutingChange { plow: Some(false), ..RoutingChange::default() },
            RoutingChange { connector_to_shape: Pair { x: None, y: Some(0.5) }, ..RoutingChange::default() },
            RoutingChange { style: Some(RouteStyle::Network), ..RoutingChange::default() },
        ] {
            assert!(!change.is_empty(), "{change:?}");
        }
    }
}
