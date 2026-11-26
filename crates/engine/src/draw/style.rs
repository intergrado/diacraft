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
//!
//! **The routing styles** (ours, which extends): the codes of a page's `RouteStyle` and a
//! connector's `ShapeRouteStyle` are [`RouteStyle`], a style with the direction it flows (organization chart 3, 4, 10 and 11, flowchart 5, 6, 12 and 13, tree 7, 8, 14 and 15,
//! simple 17 to 20, and 21 and 22 for the simple style's two ways with none), read by [`route_style_of`] (a 0, a number between two codes and one outside 0 to 22 read as right
//! angle) and turned back to a code and to words by [`RouteStyle::code`], [`RouteStyle::family`] and [`RouteStyle::direction`]. What a connector follows is its own code or,
//! where that is 0, the page's ([`connector_style`]). A directional style **draws as right angle** and **leans** the way it says ([`lean_of`],
//! "a router should prefer the direction and may leave it", which the router takes up later). The Connectors menu's three choices stay the three of [`StyleChoice`];
//! `connector.style` takes the other words too, writing the style's code and straight legs (`ConLineRouteExt` 1).

use diacraft_doc::{CellEdit, Document, SheetRef, Value};

use super::read::Reader;
use super::route::Dir;
use super::scan;
use super::settle::settle_forced;
use super::{Problem, write_cells};
use crate::error::shown;
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
    match route_style_of(style) {
        RouteStyle::CentreToCentre => PathStyle::CentreToCentre,
        RouteStyle::Straight => PathStyle::Straight,
        _ if is(appearance, 2.0) => PathStyle::Curved,
        _ => PathStyle::RightAngle,
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
    set_connector_cells(doc, page, ids, choice.cells())
}

/// Gives connectors `ids` the routing style `style` (organization chart, flowchart, tree, network, centre to centre, simple, or one of the three of the menu) with straight
/// legs, and routes them again as [`set_connector_style`] does: `ShapeRouteStyle` is its code and `ConLineRouteExt` 1. → how many connectors changed.
pub fn set_connector_route_style(doc: &mut Document, page: u32, ids: &[u32], style: RouteStyle) -> Result<usize, Problem> {
    set_connector_cells(doc, page, ids, (f64::from(style.code()), 1.0))
}

/// The cells `ShapeRouteStyle` and `ConLineRouteExt` of connectors `ids` made `(style, appearance)`, a hand edit undone, and the connectors routed again.
fn set_connector_cells(doc: &mut Document, page: u32, ids: &[u32], (style, appearance): (f64, f64)) -> Result<usize, Problem> {
    let before = doc.clone();
    let (_, found) = scan::named(doc, page, ids)?;
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
    set_page_cells(doc, page, choice.cells())
}

/// [`set_page_style`] for a routing style with its direction: `RouteStyle` is its code and `LineRouteExt` 1. Nothing is routed again.
pub fn set_page_route_style(doc: &mut Document, page: u32, style: RouteStyle) -> Result<bool, Problem> {
    set_page_cells(doc, page, (f64::from(style.code()), 1.0))
}

/// The page's `RouteStyle` and `LineRouteExt` made `(style, appearance)`. → whether a cell changed.
fn set_page_cells(doc: &mut Document, page: u32, (style, appearance): (f64, f64)) -> Result<bool, Problem> {
    super::require_page(doc, page)?;
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

// ---- the routing styles of ------------------------------------------------------------------------------------------

/// The way a flow runs in a routing style that has one: the four columns of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    TopToBottom,
    LeftToRight,
    BottomToTop,
    RightToLeft,
}

impl Flow {
    /// The word a command takes for it.
    pub const fn word(self) -> &'static str {
        match self {
            Flow::TopToBottom => "top_to_bottom",
            Flow::LeftToRight => "left_to_right",
            Flow::BottomToTop => "bottom_to_top",
            Flow::RightToLeft => "right_to_left",
        }
    }

    /// The way it runs on the page, whose y axis points up: what a directional style leans toward.
    pub const fn way(self) -> Dir {
        match self {
            Flow::TopToBottom => Dir::Down,
            Flow::LeftToRight => Dir::Right,
            Flow::BottomToTop => Dir::Up,
            Flow::RightToLeft => Dir::Left,
        }
    }

    /// The code in one row of the table, given as its four columns in the order top to bottom, left to right, bottom to top, right to left.
    const fn code_in(self, row: (u8, u8, u8, u8)) -> u8 {
        match self {
            Flow::TopToBottom => row.0,
            Flow::LeftToRight => row.1,
            Flow::BottomToTop => row.2,
            Flow::RightToLeft => row.3,
        }
    }
}

/// A routing style and, for the styles that have one, the direction it flows: what a page's `RouteStyle` and a connector's `ShapeRouteStyle` say (the table of 22
/// codes and a 0 that is the default, right angle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStyle {
    /// 1, and 0 as the default.
    RightAngle,
    /// 2.
    Straight,
    /// 3, 4, 10 and 11.
    OrgChart(Flow),
    /// 5, 6, 12 and 13.
    Flowchart(Flow),
    /// 7, 8, 14 and 15.
    Tree(Flow),
    /// 9.
    Network,
    /// 16.
    CentreToCentre,
    /// 17 to 20.
    Simple(Flow),
    /// 21: level, then upright.
    SimpleLevelFirst,
    /// 22: upright, then level.
    SimpleUprightFirst,
}

impl RouteStyle {
    /// The code that says it: right angle is 1, whatever default the cell read as.
    pub const fn code(self) -> u8 {
        match self {
            RouteStyle::RightAngle => 1,
            RouteStyle::Straight => 2,
            RouteStyle::OrgChart(flow) => flow.code_in((3, 4, 10, 11)),
            RouteStyle::Flowchart(flow) => flow.code_in((5, 6, 12, 13)),
            RouteStyle::Tree(flow) => flow.code_in((7, 8, 14, 15)),
            RouteStyle::Network => 9,
            RouteStyle::CentreToCentre => 16,
            RouteStyle::Simple(flow) => flow.code_in((17, 18, 19, 20)),
            RouteStyle::SimpleLevelFirst => 21,
            RouteStyle::SimpleUprightFirst => 22,
        }
    }

    /// The word of the style apart from its direction: what `page.set_routing` and `connector.style` take as `style`.
    pub const fn family(self) -> &'static str {
        Family::of(self).word()
    }

    /// The word of its direction, for a style that has one.
    pub const fn direction(self) -> Option<&'static str> {
        match self {
            RouteStyle::OrgChart(flow) | RouteStyle::Flowchart(flow) | RouteStyle::Tree(flow) | RouteStyle::Simple(flow) => Some(flow.word()),
            RouteStyle::SimpleLevelFirst => Some("level_first"),
            RouteStyle::SimpleUprightFirst => Some("upright_first"),
            RouteStyle::RightAngle | RouteStyle::Straight | RouteStyle::Network | RouteStyle::CentreToCentre => None,
        }
    }

    /// The style the words say, as `now` is turned by them: `style` names the style (left out, it is `now`'s), and `direction` its direction (left out, a style that takes one
    /// flows from the top to the bottom). The words are read in any case, with `-` or a space for `_`. Words that are no style or no direction, a direction the style does not
    /// take, and one of the two ways of the simple style given to another style are refused in words.
    pub fn choose(style: Option<&str>, direction: Option<&str>, now: RouteStyle) -> Result<RouteStyle, String> {
        let family = match style {
            Some(text) => Family::of_word(&normal(text)).ok_or_else(|| format!("`style` must be {}, not `{}`", Family::LIST, shown(text)))?,
            None => Family::of(now),
        };
        let way = match direction {
            Some(text) => Some(Way::of_word(&normal(text)).ok_or_else(|| {
                format!(
                    "`direction` must be top_to_bottom, left_to_right, bottom_to_top or right_to_left (and level_first or upright_first for the simple style), not `{}`",
                    shown(text)
                )
            })?),
            None => None,
        };
        family.with(way, style.is_some())
    }
}

/// The words of a command, in one case with `_` between: `Left-To-Right` and `left to right` are `left_to_right`.
fn normal(text: &str) -> String {
    text.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

/// A style apart from its direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    RightAngle,
    Straight,
    OrgChart,
    Flowchart,
    Tree,
    Network,
    CentreToCentre,
    Simple,
}

/// A direction as a command says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    Flow(Flow),
    LevelFirst,
    UprightFirst,
}

impl Way {
    fn of_word(word: &str) -> Option<Way> {
        Some(match word {
            "top_to_bottom" => Way::Flow(Flow::TopToBottom),
            "left_to_right" => Way::Flow(Flow::LeftToRight),
            "bottom_to_top" => Way::Flow(Flow::BottomToTop),
            "right_to_left" => Way::Flow(Flow::RightToLeft),
            "level_first" => Way::LevelFirst,
            "upright_first" => Way::UprightFirst,
            _ => return None,
        })
    }

    const fn word(self) -> &'static str {
        match self {
            Way::Flow(flow) => flow.word(),
            Way::LevelFirst => "level_first",
            Way::UprightFirst => "upright_first",
        }
    }
}

impl Family {
    /// The words of the styles, as an error lists them.
    const LIST: &'static str = "right_angle, straight, org_chart, flowchart, tree, network, centre_to_centre or simple";

    const fn word(self) -> &'static str {
        match self {
            Family::RightAngle => "right_angle",
            Family::Straight => "straight",
            Family::OrgChart => "org_chart",
            Family::Flowchart => "flowchart",
            Family::Tree => "tree",
            Family::Network => "network",
            Family::CentreToCentre => "centre_to_centre",
            Family::Simple => "simple",
        }
    }

    /// The style a word names: the spellings of the dialog (`Organization Chart`, `Center to Center`) too.
    fn of_word(word: &str) -> Option<Family> {
        Some(match word {
            "right_angle" => Family::RightAngle,
            "straight" => Family::Straight,
            "org_chart" | "organization_chart" | "organisation_chart" => Family::OrgChart,
            "flowchart" => Family::Flowchart,
            "tree" => Family::Tree,
            "network" => Family::Network,
            "centre_to_centre" | "center_to_center" => Family::CentreToCentre,
            "simple" => Family::Simple,
            _ => return None,
        })
    }

    const fn of(style: RouteStyle) -> Family {
        match style {
            RouteStyle::RightAngle => Family::RightAngle,
            RouteStyle::Straight => Family::Straight,
            RouteStyle::OrgChart(_) => Family::OrgChart,
            RouteStyle::Flowchart(_) => Family::Flowchart,
            RouteStyle::Tree(_) => Family::Tree,
            RouteStyle::Network => Family::Network,
            RouteStyle::CentreToCentre => Family::CentreToCentre,
            RouteStyle::Simple(_) | RouteStyle::SimpleLevelFirst | RouteStyle::SimpleUprightFirst => Family::Simple,
        }
    }

    /// The style of this family flowing as `way` says. `named` is whether the call named the style (and not only its direction), which words the refusal of a direction the style
    /// does not take.
    fn with(self, way: Option<Way>, named: bool) -> Result<RouteStyle, String> {
        let fixed = |style: RouteStyle| match way {
            None => Ok(style),
            Some(_) if named => Err(format!("the style `{}` takes no direction: leave `direction` out", self.word())),
            Some(_) => Err(format!("the style it has now, `{}`, takes no direction: give a `style` that does", self.word())),
        };
        let flowing = |make: fn(Flow) -> RouteStyle| match way {
            None => Ok(make(Flow::TopToBottom)),
            Some(Way::Flow(flow)) => Ok(make(flow)),
            Some(other) => Err(format!("`{}` is a direction of the simple style only", other.word())),
        };
        match self {
            Family::RightAngle => fixed(RouteStyle::RightAngle),
            Family::Straight => fixed(RouteStyle::Straight),
            Family::Network => fixed(RouteStyle::Network),
            Family::CentreToCentre => fixed(RouteStyle::CentreToCentre),
            Family::OrgChart => flowing(RouteStyle::OrgChart),
            Family::Flowchart => flowing(RouteStyle::Flowchart),
            Family::Tree => flowing(RouteStyle::Tree),
            Family::Simple => match way {
                Some(Way::LevelFirst) => Ok(RouteStyle::SimpleLevelFirst),
                Some(Way::UprightFirst) => Ok(RouteStyle::SimpleUprightFirst),
                _ => flowing(RouteStyle::Simple),
            },
        }
    }
}

/// The style a `RouteStyle` or `ShapeRouteStyle` cell says: the code nearest the number, and **right angle for a 0** (the default), **for a number outside 0 to 22 or
/// between two codes, and for one that is no number** (such a value is kept as the cell holds it and read by the cell's range).
pub fn route_style_of(code: f64) -> RouteStyle {
    let nearest = code.round();
    if !(code.is_finite() && (code - nearest).abs() < 0.5 && (0.0..=22.0).contains(&nearest)) {
        return RouteStyle::RightAngle;
    }
    // (`nearest` is a whole number from 0 to 22 here: the cast loses nothing.)
    match nearest as u8 {
        2 => RouteStyle::Straight,
        3 => RouteStyle::OrgChart(Flow::TopToBottom),
        4 => RouteStyle::OrgChart(Flow::LeftToRight),
        5 => RouteStyle::Flowchart(Flow::TopToBottom),
        6 => RouteStyle::Flowchart(Flow::LeftToRight),
        7 => RouteStyle::Tree(Flow::TopToBottom),
        8 => RouteStyle::Tree(Flow::LeftToRight),
        9 => RouteStyle::Network,
        10 => RouteStyle::OrgChart(Flow::BottomToTop),
        11 => RouteStyle::OrgChart(Flow::RightToLeft),
        12 => RouteStyle::Flowchart(Flow::BottomToTop),
        13 => RouteStyle::Flowchart(Flow::RightToLeft),
        14 => RouteStyle::Tree(Flow::BottomToTop),
        15 => RouteStyle::Tree(Flow::RightToLeft),
        16 => RouteStyle::CentreToCentre,
        17 => RouteStyle::Simple(Flow::TopToBottom),
        18 => RouteStyle::Simple(Flow::LeftToRight),
        19 => RouteStyle::Simple(Flow::BottomToTop),
        20 => RouteStyle::Simple(Flow::RightToLeft),
        21 => RouteStyle::SimpleLevelFirst,
        22 => RouteStyle::SimpleUprightFirst,
        _ => RouteStyle::RightAngle,
    }
}

/// The way a routing style leans ("direction is a lean, not a rule"): the flow of an organization chart, a flowchart, a tree and a simple style that has one (the codes
/// 3 to 8, 10 to 15 and 17 to 20), and none for the others. A router prefers it and may leave it.
pub fn lean_of(style: RouteStyle) -> Option<Dir> {
    match style {
        RouteStyle::OrgChart(flow) | RouteStyle::Flowchart(flow) | RouteStyle::Tree(flow) | RouteStyle::Simple(flow) => Some(flow.way()),
        _ => None,
    }
}

/// The routing style a connector follows: its own `ShapeRouteStyle`, or the page's `RouteStyle` where its own is 0.
pub(crate) fn route_style(reader: &Reader<'_>, page: u32, sheet: &SheetRef) -> RouteStyle {
    route_style_of(effective(reader, page, sheet, "ShapeRouteStyle", "RouteStyle"))
}

/// What a connector's cells say of its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectorStyle {
    /// What it draws.
    pub draws: PathStyle,
    /// The routing style it follows, with its direction.
    pub route: RouteStyle,
    /// Whether the style is the connector's own (`ShapeRouteStyle` is not 0) and not the page's.
    pub own: bool,
}

/// The style of connector `id` on a page. A page or a shape that is not there and a shape that is not a connector are refused in words.
pub fn connector_style(doc: &Document, page: u32, id: u32) -> Result<ConnectorStyle, Problem> {
    super::require_page(doc, page)?;
    let reader = Reader::new(doc);
    let at = reader.find(page, id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
    let sheet = SheetRef::Shape(at);
    if !reader.is_one_d(&sheet) {
        return Err(Problem::bad(format!("shape {id} is not a connector")));
    }
    Ok(ConnectorStyle {
        draws: path_style(&reader, page, &sheet),
        route: route_style(&reader, page, &sheet),
        own: reader.number(&sheet, "ShapeRouteStyle") != 0.0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The table: the code, and the style and direction it says.
    const TABLE: [(u8, &str, Option<&str>); 22] = [
        (1, "right_angle", None),
        (2, "straight", None),
        (3, "org_chart", Some("top_to_bottom")),
        (4, "org_chart", Some("left_to_right")),
        (5, "flowchart", Some("top_to_bottom")),
        (6, "flowchart", Some("left_to_right")),
        (7, "tree", Some("top_to_bottom")),
        (8, "tree", Some("left_to_right")),
        (9, "network", None),
        (10, "org_chart", Some("bottom_to_top")),
        (11, "org_chart", Some("right_to_left")),
        (12, "flowchart", Some("bottom_to_top")),
        (13, "flowchart", Some("right_to_left")),
        (14, "tree", Some("bottom_to_top")),
        (15, "tree", Some("right_to_left")),
        (16, "centre_to_centre", None),
        (17, "simple", Some("top_to_bottom")),
        (18, "simple", Some("left_to_right")),
        (19, "simple", Some("bottom_to_top")),
        (20, "simple", Some("right_to_left")),
        (21, "simple", Some("level_first")),
        (22, "simple", Some("upright_first")),
    ];

    #[test]
    fn each_code_of_route_r1_is_its_style_and_direction_and_goes_back_to_its_code() {
        for (code, family, direction) in TABLE {
            let style = route_style_of(f64::from(code));
            assert_eq!((style.code(), style.family(), style.direction()), (code, family, direction), "{code}");
            // The words choose it again, from any style the call was turned from.
            for now in [RouteStyle::RightAngle, RouteStyle::Tree(Flow::RightToLeft), RouteStyle::SimpleLevelFirst] {
                assert_eq!(RouteStyle::choose(Some(family), direction, now), Ok(style), "{code} from {now:?}");
            }
        }
        assert_eq!(route_style_of(0.0), RouteStyle::RightAngle, "0 is the default, right angle");
        // A number that is no code, between two or outside 0 to 22, or no number, is right angle.
        for odd in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 22.5, 23.0, 255.0, 1.0e300, -1.0e300, 2.5, 20.5] {
            assert_eq!(route_style_of(odd), RouteStyle::RightAngle, "{odd}");
        }
        // The nearest code is the one a cell with a rounding in it says, as every cell of this kind is read.
        assert_eq!(route_style_of(5.4), RouteStyle::Flowchart(Flow::TopToBottom));
        assert_eq!(route_style_of(4.6), RouteStyle::Flowchart(Flow::TopToBottom));
        assert_eq!(route_style_of(-0.4), RouteStyle::RightAngle);
    }

    #[test]
    fn the_codes_ending_a_flow_lean_the_way_it_runs_and_no_other_leans() {
        let flows = [(Flow::TopToBottom, Dir::Down), (Flow::LeftToRight, Dir::Right), (Flow::BottomToTop, Dir::Up), (Flow::RightToLeft, Dir::Left)];
        for (flow, way) in flows {
            for style in [RouteStyle::OrgChart(flow), RouteStyle::Flowchart(flow), RouteStyle::Tree(flow), RouteStyle::Simple(flow)] {
                assert_eq!(lean_of(style), Some(way), "{style:?}");
            }
        }
        // The codes 1, 2, 9, 16, 21 and 22 lean no way, and no number does that is not a code: 3 to 8, 10 to 15 and 17 to 20 are the 16 that do.
        let leaning: Vec<u8> = (0..=30).filter(|n| lean_of(route_style_of(f64::from(*n))).is_some()).collect();
        assert_eq!(leaning, [3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 17, 18, 19, 20]);
    }

    #[test]
    fn the_style_a_connector_draws_is_read_as_it_was_before_the_table() {
        // `style_of` takes 16 and 2 from the table now: every number reads as it read when it compared them itself.
        let before = |style: f64, appearance: f64| {
            if is(style, 16.0) {
                PathStyle::CentreToCentre
            } else if is(style, 2.0) {
                PathStyle::Straight
            } else if is(appearance, 2.0) {
                PathStyle::Curved
            } else {
                PathStyle::RightAngle
            }
        };
        let numbers = [
            -1.0,
            -0.4,
            0.0,
            0.4,
            0.5,
            0.6,
            1.0,
            1.5,
            1.6,
            2.0,
            2.4,
            2.5,
            2.6,
            3.0,
            5.0,
            15.4,
            15.5,
            15.6,
            16.0,
            16.4,
            16.5,
            22.0,
            99.0,
            f64::NAN,
            f64::INFINITY,
        ];
        for style in numbers {
            for appearance in numbers {
                assert_eq!(style_of(style, appearance), before(style, appearance), "{style} {appearance}");
            }
        }
    }

    #[test]
    fn the_words_of_a_style_are_read_in_any_case_and_a_direction_goes_with_the_style_that_takes_it() {
        let now = RouteStyle::Flowchart(Flow::LeftToRight);
        // A direction alone turns the style it has.
        assert_eq!(RouteStyle::choose(None, Some("Bottom-To-Top"), now), Ok(RouteStyle::Flowchart(Flow::BottomToTop)));
        assert_eq!(RouteStyle::choose(None, Some("level_first"), RouteStyle::Simple(Flow::TopToBottom)), Ok(RouteStyle::SimpleLevelFirst));
        // A style alone flows from the top to the bottom, even where it is the style it has.
        assert_eq!(RouteStyle::choose(Some("flowchart"), None, now), Ok(RouteStyle::Flowchart(Flow::TopToBottom)));
        assert_eq!(RouteStyle::choose(Some(" Organization Chart "), None, now), Ok(RouteStyle::OrgChart(Flow::TopToBottom)));
        assert_eq!(RouteStyle::choose(Some("Center to Center"), None, now), Ok(RouteStyle::CentreToCentre));
        assert_eq!(RouteStyle::choose(Some("simple"), Some("upright_first"), now), Ok(RouteStyle::SimpleUprightFirst));
        // What the style does not take is refused in words, and the words name it.
        for (style, direction, now, words) in [
            (Some("right_angle"), Some("top_to_bottom"), now, "the style `right_angle` takes no direction"),
            (Some("network"), Some("left_to_right"), now, "the style `network` takes no direction"),
            (None, Some("left_to_right"), RouteStyle::Straight, "the style it has now, `straight`, takes no direction"),
            (Some("tree"), Some("level_first"), now, "`level_first` is a direction of the simple style only"),
            (
                Some("zigzag"),
                None,
                now,
                "`style` must be right_angle, straight, org_chart, flowchart, tree, network, centre_to_centre or simple, not `zigzag`",
            ),
            (Some("tree"), Some("sideways"), now, "not `sideways`"),
        ] {
            let said = RouteStyle::choose(style, direction, now).unwrap_err();
            assert!(said.contains(words), "{style:?} {direction:?}: {said}");
        }
    }
}
