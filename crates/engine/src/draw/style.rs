//! A connector's path style: right-angle, straight or curved.
//!
//! Our rule where no specification settles it. What a connector draws is read from two pairs of cells, the
//! connector's own and the page's:
//!
//! | cell on the connector | page | 0 means | values |
//! |---|---|---|---|
//! | `ShapeRouteStyle` | `RouteStyle` | the page's | 1 right angle, 2 straight, 16 centre to centre, the rest (flowchart, tree, network, simple) as right angle |
//! | `ConLineRouteExt` | `LineRouteExt` | the page's | 1 straight legs, 2 curved |
//!
//! * **Straight** (`ShapeRouteStyle` 2): the route is the two ends joined, one leg that is level, upright or slanted.
//! * **Curved** (`ConLineRouteExt` 2, and not straight): the route is a smooth curve, saved as `NURBSTo` rows.
//! * **Right angle**, anything else: level and upright legs.
//! * **Centre to centre** (16) is left as it is for a connector of a file (it aims along the line between the shapes' centres, which the
//!   glue's places do not give); a command about the connector, and a connector DiaCraft made, route it straight.
//!
//! **What the Connectors menu writes** (our reading): Right-Angle writes `ShapeRouteStyle` 1 and `ConLineRouteExt` 1, Straight Lines writes 2
//! and 1, Curved Lines writes 1 and 2, on each connector selected, and routes it again, a hand edit (`ConFixedCode` 3) being undone; with no connector
//! named it writes the page's cells the same way (`RouteStyle`, `LineRouteExt`), which new connectors take and existing ones take at their next route
//! (the project rule), and reroutes nothing (asks for a reroute; the window can ask for it by naming the connectors).
//!
//! Our rule (`rows.rs`): a curved connector is the quadratic B-spline whose control polygon is the right-angle route, clamped so that it
//! starts on the begin and ends on the end, tangent to the first and the last leg. Two ends in line are a straight line.

use diacraft_doc::{CellEdit, Document, SheetRef, Value};

use super::read::Reader;
use super::scan;
use super::settle::settle_forced;
use super::{Problem, write_cells};
use crate::stencil::number_text;

/// What a connector draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStyle {
    RightAngle,
    Straight,
    Curved,
    /// `ShapeRouteStyle` 16: a file's connector of this style keeps its route when its shapes move.
    CentreToCentre,
}

/// A cell of the connector, or the page's own cell where the connector's is 0.
fn effective(reader: &Reader<'_>, page: u32, sheet: &SheetRef, own: &str, page_cell: &str) -> f64 {
    let held = reader.number(sheet, own);
    if held != 0.0 { held } else { reader.number(&SheetRef::Page(page), page_cell) }
}

fn is(value: f64, number: f64) -> bool {
    (value - number).abs() < 0.5
}

/// What the connector `sheet` draws.
pub(crate) fn path_style(reader: &Reader<'_>, page: u32, sheet: &SheetRef) -> PathStyle {
    let style = effective(reader, page, sheet, "ShapeRouteStyle", "RouteStyle");
    let appearance = effective(reader, page, sheet, "ConLineRouteExt", "LineRouteExt");
    style_of(style, appearance)
}

/// What a connector draws whose route style is `style` and whose appearance is `appearance`, each the connector's own or, where that is 0, the page's.
pub(crate) fn style_of(style: f64, appearance: f64) -> PathStyle {
    if is(style, 16.0) {
        PathStyle::CentreToCentre
    } else if is(style, 2.0) {
        PathStyle::Straight
    } else if is(appearance, 2.0) {
        PathStyle::Curved
    } else {
        PathStyle::RightAngle
    }
}

/// The choices of the Connectors menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleChoice {
    RightAngle,
    Straight,
    Curved,
}

impl StyleChoice {
    /// The route style and the appearance the choice writes.
    fn cells(self) -> (f64, f64) {
        match self {
            StyleChoice::RightAngle => (1.0, 1.0),
            StyleChoice::Straight => (2.0, 1.0),
            StyleChoice::Curved => (1.0, 2.0),
        }
    }
}

/// The value cell edit for a whole number.
fn number(n: f64) -> CellEdit {
    CellEdit::Value(Value::from_text(&number_text(n)))
}

/// Gives connectors `ids` the style `choice` and routes them again; a hand edit (`ConFixedCode` 3) is undone. → how many connectors changed. A shape
/// that is not a connector is an error and nothing changes. A connector that has the style already, and no hand edit, is left alone.
pub fn set_connector_style(doc: &mut Document, page: u32, ids: &[u32], choice: StyleChoice) -> Result<usize, Problem> {
    let before = doc.clone();
    let (_, found) = scan::named(doc, page, ids)?;
    let (style, appearance) = choice.cells();
    let mut writes = Vec::new();
    let mut changed: Vec<u32> = Vec::new();
    {
        let reader = Reader::new(doc);
        for (id, at) in &found {
            let sheet = SheetRef::Shape(at.clone());
            if !reader.is_one_d(&sheet) {
                return Err(Problem::bad(format!("shape {id} is not a connector")));
            }
            let held = |name: &str| reader.number(&sheet, name);
            let mut differs = false;
            for (name, wanted) in [("ShapeRouteStyle", style), ("ConLineRouteExt", appearance)] {
                if !is(held(name), wanted) || held(name) == 0.0 {
                    writes.push((sheet.clone(), diacraft_doc::CellAddr::direct(name), number(wanted)));
                    differs = true;
                }
            }
            if is(held("ConFixedCode"), 3.0) {
                writes.push((sheet.clone(), diacraft_doc::CellAddr::direct("ConFixedCode"), number(0.0)));
                differs = true;
            }
            if differs {
                changed.push(*id);
            }
        }
    }
    if changed.is_empty() {
        return Ok(0);
    }
    write_cells(doc, writes)?;
    settle_forced(doc, &before, page, &[], &changed, &changed)?;
    Ok(changed.len())
}

/// Gives the page the default style `choice`: new connectors take it, and so do existing ones at their next route. Nothing is
/// routed again. → whether a cell changed.
pub fn set_page_style(doc: &mut Document, page: u32, choice: StyleChoice) -> Result<bool, Problem> {
    super::require_page(doc, page)?;
    let (style, appearance) = choice.cells();
    let sheet = SheetRef::Page(page);
    let mut writes = Vec::new();
    {
        let reader = Reader::new(doc);
        for (name, wanted) in [("RouteStyle", style), ("LineRouteExt", appearance)] {
            let held = reader.number(&sheet, name);
            if !is(held, wanted) || held == 0.0 {
                writes.push((sheet.clone(), diacraft_doc::CellAddr::direct(name), number(wanted)));
            }
        }
    }
    if writes.is_empty() {
        return Ok(false);
    }
    write_cells(doc, writes)?;
    Ok(true)
}

/// What a page's default style is, as the Connectors menu names it: the page's `RouteStyle` and `LineRouteExt`, 0 being a right angle.
pub fn page_style(doc: &Document, page: u32) -> StyleChoice {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let (style, appearance) = (reader.number(&sheet, "RouteStyle"), reader.number(&sheet, "LineRouteExt"));
    if is(style, 2.0) {
        StyleChoice::Straight
    } else if is(appearance, 2.0) {
        StyleChoice::Curved
    } else {
        StyleChoice::RightAngle
    }
}
