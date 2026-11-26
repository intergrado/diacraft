//! The page's routing settings: `page.routing` and `page.set_routing`, the commands behind the Page Setup dialog's Layout and Routing tab and its Spacing dialog
//! The rules are in `draw/routing.rs` and `docs/editing-commands.md`, "The page's routing settings".
//!
//! Every length is in **paper inches**, as in the other editing commands; the page's cells hold drawing units and the command turns them at its edge. A page is named by
//! `page`; left out, it is the first page that is not a background.

use serde_json::{Map, Value, json};

use super::page_setup::target_page;
use super::params::units_per_inch;
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::routing::{Appearance, Overlap, Pair, RoutingChange, Separate, page_routing, set_page_routing_routed};
use crate::draw::{MAX_COORD, RouteStyle};
use crate::error::{Result, bad, failed, shown};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "page.routing",
            "Layout and Routing",
            ["Design", "Layout"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            routing
        ),
        cmd!(
            "page.set_routing",
            "Page Routing",
            ["Design", "Layout"],
            None,
            "style: text, optional, the page's routing style: right_angle, straight, org_chart, flowchart, tree, network, centre_to_centre or simple (RouteStyle: a new style routes again the connectors that follow the page and are set to reroute freely); \
             direction: text, optional, for an org_chart, a flowchart, a tree or a simple style top_to_bottom (the default), left_to_right, bottom_to_top or right_to_left, and for a simple style level_first or upright_first too (with no style it turns the page's own style, and a style that takes none refuses it); \
             separate: text, optional, which connectors running over each other are pushed apart: unrelated, all, none or by_style (LineAdjustFrom); \
             overlap: text, optional, which nearby connectors may be laid over each other: by_style, all, none or related (LineAdjustTo); \
             appearance: text, optional, straight or curved, the look of new connectors (LineRouteExt: connectors already there take it at their next route); \
             plow: true or false, optional, move other shapes away on drop (PlowCode: written and kept, not acted on yet); \
             split: true or false, optional, enable connector splitting (PageShapeSplit: written and kept, not acted on yet); \
             between_shapes: object, optional, the space between shapes as {x, y} in paper inches, each more than 0 and either alone (AvenueSizeX and AvenueSizeY); \
             shape_size: object, optional, the average shape size as {x, y} (BlockSizeX and BlockSizeY); \
             connector_to_connector: object, optional, the least space between connectors as {x, y} (LineToLineX and LineToLineY); \
             connector_to_shape: object, optional, the least space between connectors and shapes as {x, y} (LineToNodeX and LineToNodeY); \
             page: whole number, optional, the id of the page (default: the first page that is not a background). \
             The cells the call names are written in one undo step, in the page's drawing units, and nothing else is: routes already made stay as they are, and give the lengths of a drawing scale in paper inches",
            has_document,
            set_routing
        ),
    ]
}

/// The inches a drawing length is on paper, to nine places.
fn paper(length: f64, k: f64) -> f64 {
    let inches = length / k;
    let rounded = (inches * 1.0e9).round() / 1.0e9;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// A pair of lengths, as the answers give it: paper inches.
fn pair_json(pair: (f64, f64), k: f64) -> Value {
    json!({ "x": paper(pair.0, k), "y": paper(pair.1, k) })
}

/// The answer of both commands: what the page says of its routing. `held` says for each setting whether the page holds it itself and not the root or the default.
fn routing_json(doc: &diacraft_doc::Document, page: u32) -> Value {
    let now = page_routing(doc, page);
    let k = units_per_inch(doc, page);
    let jumps = now.line_jumps;
    json!({
        "page": page,
        "style": now.style.family(),
        "direction": now.style.direction(),
        "style_code": now.style.code(),
        "separate": now.separate.word(),
        "overlap": now.overlap.word(),
        "appearance": now.appearance.word(),
        "line_jumps": {
            "code": jumps.code,
            "style": jumps.style,
            "factor_x": jumps.factor_x,
            "factor_y": jumps.factor_y,
            "dir_x": jumps.dir_x,
            "dir_y": jumps.dir_y,
        },
        "plow": now.plow,
        "split": now.split,
        "between_shapes": pair_json(now.between_shapes, k),
        "shape_size": pair_json(now.shape_size, k),
        "connector_to_connector": pair_json(now.connector_to_connector, k),
        "connector_to_shape": pair_json(now.connector_to_shape, k),
        "held": {
            "style": now.held.style,
            "separate": now.held.separate,
            "overlap": now.held.overlap,
            "appearance": now.held.appearance,
            "line_jumps": now.held.line_jumps,
            "plow": now.held.plow,
            "split": now.held.split,
            "between_shapes": now.held.between_shapes,
            "shape_size": now.held.shape_size,
            "connector_to_connector": now.held.connector_to_connector,
            "connector_to_shape": now.held.connector_to_shape,
        },
    })
}

/// → `{page, style, direction, style_code, separate, overlap, appearance, line_jumps, plow, split, between_shapes, shape_size, connector_to_connector, connector_to_shape, held}`: what
/// the page says of its routing. `style` and `direction` (`null` for a style that has none) are the words `page.set_routing` takes, and `style_code` is the code of `RouteStyle`
/// (right angle is 1 whether the cell reads 0 or 1); `line_jumps` is `{code, style, factor_x, factor_y, dir_x, dir_y}` as `connector.line_jumps` sets them; each of the four spacing
/// groups is `{x, y}` in paper inches; `held` has a flag for each of the others and says whether the page holds the setting itself, or the root or the default gives it.
fn routing(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.routing";
    let page = target_page(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    Ok(routing_json(doc, page))
}

fn optional_bool(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The word `p` gives for `key`, if it gives one, as one of `list`: read in any case, with `-` or a space for `_`.
fn choice<T: Copy>(p: &Value, key: &str, cmd: &str, list: &[(&str, T)]) -> Result<Option<T>> {
    let Some(text) = optional_text(p, key, cmd)? else { return Ok(None) };
    let word = text.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match list.iter().find(|(name, _)| *name == word) {
        Some((_, value)) => Ok(Some(*value)),
        None => {
            let names: Vec<&str> = list.iter().map(|(name, _)| *name).collect();
            let (last, first) = names.split_last().map_or(("", &[][..]), |(last, first)| (*last, first));
            Err(bad(cmd, format!("`{key}` must be {} or {last}, not `{}`", first.join(", "), shown(text))))
        }
    }
}

/// One length of a pair: a number more than 0 in paper inches, as drawing units.
fn length(map: &Map<String, Value>, axis: &str, key: &str, cmd: &str, k: f64) -> Result<Option<f64>> {
    match map.get(axis) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64().filter(|n| n.is_finite()) {
            Some(n) if n > 0.0 && n * k <= MAX_COORD => Ok(Some(n * k)),
            Some(_) => Err(bad(cmd, format!("`{key}.{axis}` must be a number of inches more than 0 and within {MAX_COORD} drawing units"))),
            None => Err(bad(cmd, format!("`{key}.{axis}` must be a number"))),
        },
    }
}

/// The pair `p` gives for `key`: `{x, y}`, either alone, in paper inches, as drawing units (`k` of them to the inch).
fn pair(p: &Value, key: &str, cmd: &str, k: f64) -> Result<Pair> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Pair::default()),
        Some(Value::Object(map)) => {
            if let Some(unknown) = map.keys().find(|name| !matches!(name.as_str(), "x" | "y")) {
                return Err(bad(cmd, format!("`{key}` has an unknown key `{}`: it takes x and y", shown(unknown))));
            }
            let found = Pair { x: length(map, "x", key, cmd, k)?, y: length(map, "y", key, cmd, k)? };
            if found == Pair::default() {
                return Err(bad(cmd, format!("`{key}` needs `x`, `y` or both")));
            }
            Ok(found)
        }
        Some(_) => Err(bad(cmd, format!("`{key}` must be an object with `x`, `y` or both"))),
    }
}

/// What a call changes. `now` is the page's routing style, which a direction alone turns.
fn change_of(p: &Value, cmd: &str, k: f64, now: RouteStyle) -> Result<RoutingChange> {
    let (style, direction) = (optional_text(p, "style", cmd)?, optional_text(p, "direction", cmd)?);
    let style =
        if style.is_none() && direction.is_none() { None } else { Some(RouteStyle::choose(style, direction, now).map_err(|words| bad(cmd, words))?) };
    Ok(RoutingChange {
        style,
        separate: choice(
            p,
            "separate",
            cmd,
            &[("unrelated", Separate::Unrelated), ("all", Separate::All), ("none", Separate::NoLines), ("by_style", Separate::ByStyle)],
        )?,
        overlap: choice(
            p,
            "overlap",
            cmd,
            &[("by_style", Overlap::ByStyle), ("all", Overlap::All), ("none", Overlap::NoLines), ("related", Overlap::Related)],
        )?,
        appearance: choice(p, "appearance", cmd, &[("straight", Appearance::Straight), ("curved", Appearance::Curved)])?,
        plow: optional_bool(p, "plow", cmd)?,
        split: optional_bool(p, "split", cmd)?,
        between_shapes: pair(p, "between_shapes", cmd, k)?,
        shape_size: pair(p, "shape_size", cmd, k)?,
        connector_to_connector: pair(p, "connector_to_connector", cmd, k)?,
        connector_to_shape: pair(p, "connector_to_shape", cmd, k)?,
    })
}

/// Sets the page's routing → what `page.routing` answers, and `routed`: how many connectors were asked to route again (those that follow the page and are set to
/// reroute freely, when the style is another). The cells the call names are written in one undo step; a call that finds them as they are is no undo step.
fn set_routing(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.set_routing";
    let page = target_page(s, p, ID)?;
    s.edit_if("Page routing", |doc| {
        let k = units_per_inch(doc, page);
        let change = change_of(p, ID, k, page_routing(doc, page).style)?;
        if change.is_empty() {
            return Err(bad(
                ID,
                "give at least one of style, direction, separate, overlap, appearance, plow, split, between_shapes, shape_size, connector_to_connector and connector_to_shape",
            ));
        }
        let done = set_page_routing_routed(doc, page, &change).map_err(|e| e.into_engine(ID))?;
        let mut out = routing_json(doc, page);
        if let Some(map) = out.as_object_mut() {
            map.insert("routed".to_owned(), json!(done.routed));
        }
        Ok((out, done.changed))
    })
}
