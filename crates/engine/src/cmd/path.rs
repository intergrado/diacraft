//! The path of a connector: its style (`connector.style`), a leg moved by hand (`connector.edit_path`), Reset Connector (`connector.reset`),
//! the line jumps (`connector.line_jumps`) and, when it may take a new path (`connector.reroute`); the routing styles with a direction
//! (`connector.style`) and what the route is (`connector.path`, a query). The rules are `draw/style.rs`, `draw/hand.rs`,
//! `draw/reroute.rs` and `draw/jumps.rs`.
//!
//! Lengths are paper inches, as for `shape.*`.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids, required_number, units_per_inch};
use super::snap_params::{self, Ask, Words};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::{self, JumpChange, PathStyle, Reroute, RouteStyle, StyleChoice};
use crate::error::{Result, bad, failed, shown};
use crate::snap::{Subject, Use};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "connector.style",
            "Connector Style",
            ["Design", "Layout", "Connectors"],
            None,
            "style: text, right_angle, straight, curved, org_chart, flowchart, tree, network, centre_to_centre or simple (the last six write the style's code with straight legs); \
             direction: text, optional, for an org_chart, a flowchart, a tree or a simple style top_to_bottom (the default), left_to_right, bottom_to_top or right_to_left, \
             and for a simple style level_first or upright_first too (the other styles take none and refuse it); \
             ids: array of whole numbers, optional, the connectors to give the style and route again (a path edited \
             by hand is undone), and when it is left out the style is the page's default for new connectors and nothing is routed again; \
             page: whole number, optional, the id of the page (default: the first page holding all the connectors)",
            has_document,
            style
        ),
        cmd!(
            query "connector.path",
            "Connector Path",
            [],
            None,
            "id: whole number, the connector; page: whole number, optional, the id of the page (default: the first page with the connector)",
            has_document,
            path
        ),
        cmd!(
            "connector.edit_path",
            "Edit Connector Path",
            [],
            None,
            "id: whole number, the connector; leg: whole number, the leg to move, counted from 0 at the begin (see connector_path); \
             to: number, the line the leg goes to, in inches on the page: the height of a level leg, the place along x of an upright one; \
             page: whole number, optional, the id of the page (default: the first page with the connector); \
             snap: true or {zoom, view}, optional, snap the leg's line by the document's Snap & Glue settings, as the middle of the leg moving along the one axis it can move on \
             (the grid, the guides and the other shapes pull it), and the answer then has `to`, the line it went to, and `snap`",
            has_document,
            edit_path
        ),
        cmd!(
            "connector.reroute",
            "Reroute Connector",
            [],
            None,
            "ids: array of whole numbers, the connectors; \
             mode: text, freely (0: whenever the application sees a need), as_needed (1), never (2) or on_crossover (3: only when a leg would pass through a shape it is glued to, \
             and what a path edited by hand gives); page: whole number, optional, the id of the page (default: the first page holding all the connectors)",
            has_document,
            reroute
        ),
        cmd!(
            "connector.reset",
            "Reset Connector",
            ["Design", "Layout", "Connectors"],
            None,
            "ids: array of whole numbers, the connectors to take back to the route their style gives; \
             page: whole number, optional, the id of the page (default: the first page holding all the connectors)",
            has_document,
            reset
        ),
        cmd!(
            "connector.line_jumps",
            "Line Jumps",
            ["Design", "Layout", "Connectors"],
            None,
            "show: true or false, optional, the Show Line Jumps toggle (page only, on makes the page's code 1 when it is 0 and off makes it 0); \
             code: whole number 0 to 5, optional, which of two crossing connectors jumps (0 none, 1 horizontal lines, 2 vertical lines, 3 last routed, \
             4 top of the drawing order, 5 bottom); \
             style: whole number 0 to 9, optional, 0 and 1 arc, 2 gap, 3 square, 4 to 9 two to seven sides; \
             dir_x: whole number 0 to 2, optional, the side of a jump on a level leg (0 and 1 up, 2 down); \
             dir_y: whole number 0 to 2, optional, the side of a jump on an upright leg (0 and 1 left, 2 right); \
             factor_x: number 0 to 10, optional, the size of a jump on a level leg as a multiple of the page's line to line spacing (page only); \
             factor_y: number 0 to 10, optional, the same on an upright leg (page only); \
             ids: array of whole numbers, optional, the connectors whose own code, style and sides are set (they override the page's and 0 gives the \
             page's back), and when it is left out the page's cells are set; \
             page: whole number, optional, the id of the page (default: the first page holding all the connectors)",
            has_document,
            line_jumps
        ),
    ]
}

/// The ids a command names, if it names some: `ids` may be left out.
fn optional_ids(p: &Value, cmd: &str) -> Result<Option<Vec<u32>>> {
    if p.get("ids").is_none_or(Value::is_null) { Ok(None) } else { required_ids(p, "ids", cmd).map(Some) }
}

fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

/// What `connector.style` was asked for: one of the Connectors menu's three, or a routing style with its direction.
#[derive(Clone, Copy)]
enum Asked {
    Menu(StyleChoice, &'static str),
    Route(RouteStyle),
}

/// The style and direction `p` names. A direction goes with the styles that take one: for the three of the menu and the others that take none it is refused in words.
fn asked(p: &Value, cmd: &str) -> Result<Asked> {
    let text = optional_text(p, "style", cmd)?.ok_or_else(|| bad(cmd, "missing `style`"))?;
    let direction = optional_text(p, "direction", cmd)?;
    let menu = match text.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "right_angle" => Some((StyleChoice::RightAngle, "right_angle")),
        "straight" => Some((StyleChoice::Straight, "straight")),
        "curved" => Some((StyleChoice::Curved, "curved")),
        _ => None,
    };
    if let Some((choice, name)) = menu {
        if direction.is_some() {
            return Err(bad(cmd, format!("the style `{name}` takes no direction: leave `direction` out")));
        }
        return Ok(Asked::Menu(choice, name));
    }
    // A word that is no style is refused with the words of all the styles there are, the menu's three among them.
    RouteStyle::choose(Some(text), None, RouteStyle::RightAngle).map_err(|_| {
        bad(
            cmd,
            format!(
                "`style` must be right_angle, straight, curved, org_chart, flowchart, tree, network, centre_to_centre or simple, not `{}`",
                shown(text)
            ),
        )
    })?;
    RouteStyle::choose(Some(text), direction, RouteStyle::RightAngle).map(Asked::Route).map_err(|words| bad(cmd, words))
}

/// Sets the path style → `{page, style, styled, default_changed}`, and `direction` for a style that has one: how many connectors changed, and whether the page's default did.
fn style(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.style";
    let asked = asked(p, ID)?;
    let ids = optional_ids(p, ID)?;
    let page = target_page(s, p, ids.as_deref().unwrap_or(&[]), ID)?;
    s.edit_if("Connector style", |doc| {
        let (styled, default_changed) = match (&ids, asked) {
            (Some(ids), Asked::Menu(choice, _)) => (draw::set_connector_style(doc, page, ids, choice).map_err(|e| e.into_engine(ID))?, false),
            (Some(ids), Asked::Route(style)) => (draw::set_connector_route_style(doc, page, ids, style).map_err(|e| e.into_engine(ID))?, false),
            (None, Asked::Menu(choice, _)) => (0, draw::set_page_style(doc, page, choice).map_err(|e| e.into_engine(ID))?),
            (None, Asked::Route(style)) => (0, draw::set_page_route_style(doc, page, style).map_err(|e| e.into_engine(ID))?),
        };
        let mut said = json!({ "page": page, "styled": styled, "default_changed": default_changed });
        if let Some(map) = said.as_object_mut() {
            match asked {
                Asked::Menu(_, name) => map.insert("style".to_owned(), json!(name)),
                Asked::Route(style) => {
                    if let Some(direction) = style.direction() {
                        map.insert("direction".to_owned(), json!(direction));
                    }
                    map.insert("style".to_owned(), json!(style.family()))
                }
            };
        }
        Ok((said, styled > 0 || default_changed))
    })
    // (A curved style on a drawing saved as an `.odg` was told so here, while the save refused a curved connector: the writer writes it as cubics,
    // and the page's own style has been saved.)
}

/// The word of what a connector draws.
fn draws_word(draws: PathStyle) -> &'static str {
    match draws {
        PathStyle::RightAngle => "right_angle",
        PathStyle::Straight => "straight",
        PathStyle::Curved => "curved",
        PathStyle::CentreToCentre => "centre_to_centre",
    }
}

/// A length on the page as paper inches, to nine places.
fn inches(length: f64, k: f64) -> f64 {
    let rounded = (length / k * 1.0e9).round() / 1.0e9;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// What a connector's route is → `{page, id, points, style, route_style, reroute, hand_edited, editable}`. `points` are the corners of the route on the page, the begin first, in
/// paper inches (`[x, y]` each), and `null` for a connector whose rows are no path of lines (a curve); `style` is what it draws (`right_angle`, `straight`, `curved` or
/// `centre_to_centre`); `route_style` is the routing style it follows, `{style, direction, code, own}`: its own `ShapeRouteStyle` or the page's `RouteStyle` where its own is 0
/// `direction` being `null` for a style with none and `own` whether it is the connector's own; `reroute` is its setting (`freely`, `as_needed`, `never` or
/// `on_crossover`); `hand_edited` whether its `ConFixedCode` is 3 and `editable` whether `connector.edit_path` can move its legs (what `draw::connector_path` gives a window).
fn path(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.path";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, &[id], ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let facts = draw::connector_style(doc, page, id).map_err(|e| e.into_engine(ID))?;
    let reroute = draw::connector_reroute(doc, page, id).unwrap_or(Reroute::Freely);
    let k = units_per_inch(doc, page);
    let found = draw::connector_path(doc, page, id);
    let points = found.as_ref().map(|f| f.points.iter().map(|(x, y)| json!([inches(*x, k), inches(*y, k)])).collect::<Vec<Value>>());
    Ok(json!({
        "page": page,
        "id": id,
        "points": points,
        "style": draws_word(facts.draws),
        "route_style": { "style": facts.route.family(), "direction": facts.route.direction(), "code": facts.route.code(), "own": facts.own },
        "reroute": reroute.word(),
        "hand_edited": found.as_ref().map_or(reroute == Reroute::OnCrossover, |f| f.hand_edited),
        "editable": found.as_ref().is_some_and(|f| f.editable),
    }))
}

/// Moves a leg of a connector by hand → `{page, id, changed}`; with `snap`, also `to` (the line the leg went to, inches) and `snap`.
fn edit_path(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.edit_path";
    let id = required_u32(p, "id", ID)?;
    let leg = usize::try_from(required_u32(p, "leg", ID)?).map_err(|_| bad(ID, "`leg` is too large"))?;
    let asked = required_number(p, "to", ID)?;
    let ask = snap_params::parse(p, ID, Words::PLACE)?;
    let page = target_page(s, p, &[id], ID)?;
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    // The middle of the leg, on the line asked for, is snapped along the one axis the leg moves on, as a box that keeps to an axis is; the line it comes
    // to is the one the leg goes to. A leg the path does not have, or a connector that has no legs to move, is left to the edit to refuse in words.
    let snapped = ask.and_then(|ask| snapped_leg(s, page, id, leg, (asked, k), ask));
    let to = snapped.as_ref().map_or(asked, |(to, _)| *to);
    let mut out = s.edit_if("Edit connector path", |doc| {
        let changed = draw::edit_leg(doc, page, id, leg, to * k).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })?;
    if let (Some(map), Some((to, told))) = (out.as_object_mut(), snapped) {
        map.insert("to".into(), json!((to * 1.0e6).round() / 1.0e6));
        map.insert("snap".into(), told);
    }
    Ok(out)
}

/// The line leg `leg` of connector `id` comes to when the snap has pulled it from `asked` (paper inches; `k` drawing units to an inch), and the snap told as the commands
/// tell it. `None` when the leg or the page is not there.
fn snapped_leg(s: &Session, page: u32, id: u32, leg: usize, (asked, k): (f64, f64), ask: Ask) -> Option<(f64, Value)> {
    let doc = s.document()?;
    let path = draw::connector_path(doc, page, id)?;
    let subject = draw::leg_subject(&path.points, k, leg, asked)?;
    let read = snap_params::read_for(s, page)?;
    let ask = Ask { lock: Some(subject.axis), ..ask };
    let snapped = snap_params::run_on(doc, &read, Subject::Point(subject.place), Use::Place, &[id], &[], ask);
    let at = snapped.apply(subject.place);
    let to = if subject.axis == crate::snap::Axis::Y { at.1 } else { at.0 };
    to.is_finite().then(|| (to, snap_params::snapped_json(&snapped)))
}

/// Sets when connectors may take a new path → `{page, mode, changed}`: how many connectors changed.
fn reroute(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.reroute";
    let ids = required_ids(p, "ids", ID)?;
    let word = optional_text(p, "mode", ID)?.ok_or_else(|| bad(ID, "missing `mode`"))?;
    let mode =
        Reroute::from_word(word).ok_or_else(|| bad(ID, format!("`mode` must be freely, as_needed, never or on_crossover, not `{}`", shown(word))))?;
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Reroute connector", |doc| {
        let changed = draw::set_reroute(doc, page, &ids, mode).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "mode": mode.word(), "changed": changed }), changed > 0))
    })
}

/// Reset Connector → `{page, reset}`: how many connectors changed.
fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.reset";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Reset connector", |doc| {
        let reset = draw::reset_connectors(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "reset": reset }), reset > 0))
    })
}

fn optional_flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// Sets the line jump cells of the page or of connectors, and rebuilds the jumps → `{page, changed}`.
fn line_jumps(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.line_jumps";
    let whole = |key: &str| optional_whole(p, key, ID);
    let change = JumpChange {
        show: optional_flag(p, "show", ID)?,
        code: whole("code")?,
        style: whole("style")?,
        dir_x: whole("dir_x")?,
        dir_y: whole("dir_y")?,
        factor_x: optional_number(p, "factor_x", ID)?,
        factor_y: optional_number(p, "factor_y", ID)?,
    };
    if change == JumpChange::default() {
        return Err(bad(ID, "give at least one of show, code, style, dir_x, dir_y, factor_x and factor_y"));
    }
    let ids = optional_ids(p, ID)?;
    let page = target_page(s, p, ids.as_deref().unwrap_or(&[]), ID)?;
    s.edit_if("Line jumps", |doc| {
        let changed = draw::set_line_jumps(doc, page, ids.as_deref(), &change).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "changed": changed }), changed))
    })
}
