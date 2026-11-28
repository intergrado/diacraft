//! Re-Layout Page, Auto Space, Spacing Options and page breaks: the commands behind the Design tab's Layout group, Home, Arrange, Position and View, Show, Page Breaks.
//! The rules are in `draw/relayout/` and `docs/editing-commands.md` ("Re-Layout Page, Auto Space and page breaks").
//!
//! Every length is in **paper inches**, as in the other editing commands; the page's cells hold drawing units and the command turns them at its edge. A page is named by `page`; left
//! out, it is the first page that holds every shape named, else the first page that is not a background. A command that names `ids` works on those shapes, and one that does not
//! works on every shape of the page.

use diacraft_doc::{CellAddr, CellEdit, Document, SheetRef, Value as CellValue};
use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::relayout::{self, Current, Request};
use crate::draw::{self, units_per_inch};
use crate::error::{Result, bad, failed};
use crate::stencil::number_text;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "page.relayout",
            "Re-Layout Page",
            ["Design", "Layout"],
            None,
            "style: text, optional, flowchart, hierarchy, compact_tree, radial or circular (default: the page's own, which is a flowchart where it holds none); \
             direction: text, optional, for a flowchart or a hierarchy top_to_bottom, bottom_to_top, left_to_right or right_to_left, for a compact_tree down_right, right_down, right_up, up_right, up_left, left_up, left_down or down_left \
             (the first word is the way the shapes of a parent run, the second the way each level is set off); alignment: text, optional, for a hierarchy start, center or end \
             (left, middle, right, top and bottom are taken for them): where a parent is over its children; \
             spacing: number, optional, paper inches, the space between shapes both ways (AvenueSizeX and AvenueSizeY of the page); spacing_x: number, optional, the space along x; spacing_y: number, optional, the space along y (these win over spacing); \
             block_x: number, optional, BlockSizeX in paper inches; block_y: number, optional, BlockSizeY; depth: whole number, optional, 0 to 3, PlaceDepth; flip: whole number, optional, 0 to 15, PlaceFlip (these are kept in the page for a file and the layout does not read them); \
             connector_style: text, optional, right_angle or straight, the page's RouteStyle; appearance: text, optional, straight or curved, the page's LineRouteExt; \
             apply_routing: true or false, optional (default false), the style and the appearance are written to every connector of the scope as well (a connector with no style given takes the page's); \
             enlarge_page: true or false, optional, the page's ResizePage: the page is enlarged to take the drawing in, by whole sheets of its paper; \
             ids: array of whole numbers, optional, the shapes to lay out (default: every shape of the page); \
             page: whole number, optional, the id of the page (default: the first page that holds the shapes named, else the first page that is not a background). \
             The shapes keep their sizes and none overlaps another, the corner of the diagram stays where it was, the connectors glued to the shapes are routed again, and the choice is written into the page's cells (PlaceStyle and the others). \
             A shape on a locked layer is left where it is and said in the answer's locked. At most 20000 shapes and 10000 connectors are laid out at once, and a page with more is refused in words",
            has_shapes,
            relayout_page
        ),
        cmd!(
            query "page.layout",
            "Layout Settings",
            ["Design", "Layout"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            layout_settings
        ),
        cmd!(
            "shape.autospace",
            "Auto Space",
            ["Home", "Arrange", "Position"],
            None,
            "ids: array of whole numbers, optional, the shapes to space (default: every shape of the page); \
             horizontal: number, optional, paper inches, the space between one column of shapes and the next (default: the page's AvenueSizeX); vertical: number, optional, the space between one row and the next (default: AvenueSizeY); \
             page: whole number, optional, the id of the page. \
             The leftmost column and the topmost row stay and the others move so that each gap is the space, the shapes of a column or a row keep their places in it, and with the page's AvoidPageBreaks on a shape is not left across a page break",
            has_shapes,
            autospace
        ),
        cmd!(
            "page.spacing",
            "Spacing Options",
            ["Home", "Arrange", "Position"],
            None,
            "horizontal: number, optional, more than 0, paper inches, the page's AvenueSizeX: the space Auto Space and Re-Layout Page leave between shapes along x; \
             vertical: number, optional, more than 0, AvenueSizeY, the space along y (give horizontal, vertical or both); \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            spacing
        ),
        cmd!(
            "page.avoid_page_breaks",
            "Avoid Page Breaks",
            ["Home", "Arrange", "Position"],
            None,
            "on: true or false, the page's AvoidPageBreaks: with it on, Auto Space does not leave a shape across a page break; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            avoid_page_breaks
        ),
        cmd!(
            "shape.move_off_page_breaks",
            "Move off Page Breaks",
            ["Home", "Arrange", "Position"],
            None,
            "ids: array of whole numbers, optional, the shapes to move (default: every shape of the page), a shape that a page break runs across goes along that axis to the side of the break it is nearer, until it is a twentieth of an inch from it, and a shape too large for one sheet is left and counted, \
             page: whole number, optional, the id of the page",
            has_shapes,
            move_off_page_breaks
        ),
        cmd!(
            query "page.breaks",
            "Page Breaks",
            ["View", "Show"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            page_breaks
        ),
        cmd!(
            query "page_breaks.settings",
            "Page Breaks Setting",
            ["View", "Show"],
            None,
            "none",
            has_document,
            page_breaks_settings
        ),
        cmd!(
            "page_breaks.set",
            "Page Breaks",
            ["View", "Show"],
            None,
            "on: true or false, the Page Breaks box of the drawing (View, Show, Page Breaks): on draws the lines where the page passes the printer paper",
            has_document,
            set_page_breaks
        ),
    ]
}

/// Whether some page of the drawing has a shape: what Re-Layout Page and Auto Space work on.
fn has_shapes(s: &Session) -> std::result::Result<(), String> {
    let doc = s.document().ok_or_else(|| "no document open".to_owned())?;
    if doc.pages.iter().any(|p| !p.shapes.is_empty()) { Ok(()) } else { Err("the drawing has no shapes to lay out".to_owned()) }
}

/// The page a call works on: the one named; else, for a call that names shapes, the first page that holds them all; else the first that is not a background.
fn target_page(s: &Session, p: &Value, ids: Option<&[u32]>, cmd: &str) -> Result<u32> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let named = optional_whole(p, "page", cmd)?;
    match (named, ids) {
        (Some(id), _) if doc.page(id).is_some() => Ok(id),
        (Some(id), _) => Err(bad(cmd, format!("there is no page {id}"))),
        (None, Some(ids)) if !ids.is_empty() => draw::page_holding(doc, None, ids).ok_or_else(|| bad(cmd, "the document has no pages")),
        (None, _) => doc.first_foreground_page().map(|page| page.id).ok_or_else(|| bad(cmd, "the document has no pages")),
    }
}

/// The shapes `p` names under `ids`, if it names any.
fn optional_ids(p: &Value, cmd: &str) -> Result<Option<Vec<u32>>> {
    match p.get("ids") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => required_ids(p, "ids", cmd).map(Some),
    }
}

fn optional_bool(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// A length in paper inches, more than 0 and within what the page may hold: in drawing units.
fn length(p: &Value, key: &str, cmd: &str, k: f64) -> Result<Option<f64>> {
    match optional_number(p, key, cmd)? {
        None => Ok(None),
        Some(v) if v > 0.0 && v * k <= draw::MAX_COORD => Ok(Some(v * k)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a number of inches more than 0 and within {} drawing units", draw::MAX_COORD))),
    }
}

/// A whole number of `key` from 0 to `most`.
fn small(p: &Value, key: &str, cmd: &str, most: u32) -> Result<Option<f64>> {
    match optional_whole(p, key, cmd)? {
        None => Ok(None),
        Some(v) if v <= most => Ok(Some(f64::from(v))),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a whole number from 0 to {most}"))),
    }
}

/// The page's connector style and appearance as the cells hold them: `RouteStyle` (1 right angle, 2 straight) and `LineRouteExt` (1 straight, 2 curved).
fn route_cells(p: &Value, cmd: &str, now: &Current, apply: bool) -> Result<Option<(f64, f64)>> {
    let (style, appearance) = (optional_text(p, "connector_style", cmd)?, optional_text(p, "appearance", cmd)?);
    if style.is_none() && appearance.is_none() && !apply {
        return Ok(None);
    }
    let style = match style {
        Some("right_angle") => 1.0,
        Some("straight") => 2.0,
        Some(_) => return Err(bad(cmd, "`connector_style` must be right_angle or straight")),
        None => f64::from(now.route.0.max(1)),
    };
    let appearance = match appearance {
        Some("straight") => 1.0,
        Some("curved") => 2.0,
        Some(_) => return Err(bad(cmd, "`appearance` must be straight or curved")),
        None => f64::from(now.route.1.max(1)),
    };
    Ok(Some((style, appearance)))
}

/// The request of a call (`page.relayout`).
fn request(p: &Value, cmd: &str, now: &Current, k: f64) -> Result<Request> {
    let style =
        relayout::choose(optional_text(p, "style", cmd)?, optional_text(p, "direction", cmd)?, optional_text(p, "alignment", cmd)?, now.style)
            .map_err(|words| bad(cmd, words))?;
    let both = length(p, "spacing", cmd, k)?;
    let (x, y) = (length(p, "spacing_x", cmd, k)?.or(both), length(p, "spacing_y", cmd, k)?.or(both));
    let mut cells: Vec<(&'static str, f64)> = Vec::new();
    for (name, v) in [("BlockSizeX", length(p, "block_x", cmd, k)?), ("BlockSizeY", length(p, "block_y", cmd, k)?)] {
        cells.extend(v.map(|v| (name, v)));
    }
    cells.extend(small(p, "depth", cmd, 3)?.map(|v| ("PlaceDepth", v)));
    cells.extend(small(p, "flip", cmd, 15)?.map(|v| ("PlaceFlip", v)));
    let apply = optional_bool(p, "apply_routing", cmd)?.unwrap_or(false);
    Ok(Request {
        style,
        avenue: (x.unwrap_or(now.avenue.0), y.unwrap_or(now.avenue.1)),
        cells,
        route: route_cells(p, cmd, now, apply)?,
        apply_routing: apply,
        resize_page: optional_bool(p, "enlarge_page", cmd)?,
        org: false,
    })
}

/// The layout settings as JSON: what the Configure Layout dialog shows. Lengths are paper inches.
fn settings_json(doc: &Document, page: u32, now: &Current) -> Value {
    let k = units_per_inch(doc, page);
    json!({
        "page": page,
        "style": relayout::kind_name(now.style),
        "direction": relayout::direction_name(now.style),
        "alignment": relayout::alignment_name(now.style),
        "place_style": now.place_style,
        "spacing": { "x": now.avenue.0 / k, "y": now.avenue.1 / k },
        "block": { "x": now.block.0 / k, "y": now.block.1 / k },
        "depth": now.depth,
        "flip": now.flip,
        "enlarge_page": now.resize_page,
        "avoid_page_breaks": now.avoid_page_breaks,
        "connector_style": if now.route.0 == 2 { "straight" } else { "right_angle" },
        "appearance": if now.route.1 == 2 { "curved" } else { "straight" },
    })
}

/// → `{page, style, direction, alignment, place_style, spacing, block, depth, flip, enlarge_page, avoid_page_breaks, connector_style, appearance}`: what the page's layout cells say.
fn layout_settings(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.layout";
    let page = target_page(s, p, None, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let now = relayout::current(doc, page).map_err(|e| e.into_engine(ID))?;
    Ok(settings_json(doc, page, &now))
}

/// Lays out the shapes → `{page, style, direction, alignment, place_style, nodes, joins, moved, routed, not_reglued, locked, inside_groups, followers, cells, enlarged, spacing}`. A layout that
/// changes nothing is no undo step.
fn relayout_page(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.relayout";
    let ids = optional_ids(p, ID)?;
    let page = target_page(s, p, ids.as_deref(), ID)?;
    s.edit_if("Re-Layout Page", |doc| {
        let k = units_per_inch(doc, page);
        let now = relayout::current(doc, page).map_err(|e| e.into_engine(ID))?;
        let req = request(p, ID, &now, k)?;
        let done = relayout::relayout(doc, page, ids.as_deref(), &req).map_err(|e| e.into_engine(ID))?;
        let after = relayout::current(doc, page).map_err(|e| e.into_engine(ID))?;
        let mut out = settings_json(doc, page, &after);
        if let Some(map) = out.as_object_mut() {
            map.insert("nodes".into(), json!(done.nodes));
            map.insert("joins".into(), json!(done.joins));
            map.insert("moved".into(), json!(done.moved));
            map.insert("routed".into(), json!(done.routed));
            map.insert("not_reglued".into(), json!(done.not_reglued));
            map.insert("locked".into(), json!(done.locked));
            map.insert("inside_groups".into(), json!(done.inside_groups));
            map.insert("followers".into(), json!(done.followers));
            map.insert("cells".into(), json!(done.cells));
            map.insert("enlarged".into(), json!(done.enlarged));
        }
        Ok((out, done.changed))
    })
}

/// Spaces the shapes → `{page, shapes, moved, columns, rows, spacing: {x, y}, locked, inside_groups}`.
fn autospace(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.autospace";
    let ids = optional_ids(p, ID)?;
    let page = target_page(s, p, ids.as_deref(), ID)?;
    s.edit_if("Auto Space", |doc| {
        let k = units_per_inch(doc, page);
        let gap = (length(p, "horizontal", ID, k)?, length(p, "vertical", ID, k)?);
        let done = relayout::autospace(doc, page, ids.as_deref(), gap).map_err(|e| e.into_engine(ID))?;
        let out = json!({
            "page": page,
            "shapes": done.shapes,
            "moved": done.moved,
            "columns": done.columns,
            "rows": done.rows,
            "spacing": { "x": done.gap.0 / k, "y": done.gap.1 / k },
            "locked": done.locked,
            "inside_groups": done.inside_groups,
        });
        Ok((out, done.changed))
    })
}

/// Sets the spaces → `{page, horizontal, vertical}` (paper inches, as the page holds them now).
fn spacing(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.spacing";
    let page = target_page(s, p, None, ID)?;
    s.edit_if("Spacing Options", |doc| {
        let k = units_per_inch(doc, page);
        let (horizontal, vertical) = (length(p, "horizontal", ID, k)?, length(p, "vertical", ID, k)?);
        if horizontal.is_none() && vertical.is_none() {
            return Err(bad(ID, "give `horizontal`, `vertical` or both"));
        }
        let changed = relayout::set_spacing(doc, page, horizontal, vertical).map_err(|e| e.into_engine(ID))?;
        let now = relayout::spacing_of(doc, page).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "horizontal": now.0 / k, "vertical": now.1 / k }), changed))
    })
}

/// Turns Avoid Page Breaks on or off → `{page, on}`. A page that holds the value already is no undo step.
fn avoid_page_breaks(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.avoid_page_breaks";
    let on = optional_bool(p, "on", ID)?.ok_or_else(|| bad(ID, "missing `on`"))?;
    let page = target_page(s, p, None, ID)?;
    s.edit_if("Avoid Page Breaks", |doc| {
        let now = relayout::current(doc, page).map_err(|e| e.into_engine(ID))?.avoid_page_breaks;
        if now != on {
            let edit = CellEdit::Value(CellValue::from_text(&number_text(f64::from(u8::from(on)))));
            draw::write_cells(doc, vec![(SheetRef::Page(page), CellAddr::direct("AvoidPageBreaks"), edit)]).map_err(|e| e.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "on": on }), now != on))
    })
}

/// Moves the shapes off the page breaks → `{page, shapes, moved, too_large, locked, inside_groups}`.
fn move_off_page_breaks(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.move_off_page_breaks";
    let ids = optional_ids(p, ID)?;
    let page = target_page(s, p, ids.as_deref(), ID)?;
    s.edit_if("Move off Page Breaks", |doc| {
        let done = relayout::move_off_breaks(doc, page, ids.as_deref()).map_err(|e| e.into_engine(ID))?;
        let out = json!({
            "page": page,
            "shapes": done.shapes,
            "moved": done.moved,
            "too_large": done.too_large,
            "locked": done.locked,
            "inside_groups": done.inside_groups,
        });
        Ok((out, done.changed))
    })
}

/// → `{page, units_per_inch, x: [...], y: [...]}`: where the lines of the page breaks fall, in paper inches from the page's lower left corner (a page that is one sheet has none). `Err` in
/// words for a page that cannot be printed as it is.
fn page_breaks(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.breaks";
    let page = target_page(s, p, None, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let breaks = relayout::page_breaks(doc, page).map_err(|e| e.into_engine(ID))?;
    let k = units_per_inch(doc, page);
    Ok(json!({
        "page": page,
        "units_per_inch": k,
        "x": breaks.x.iter().map(|v| v / k).collect::<Vec<_>>(),
        "y": breaks.y.iter().map(|v| v / k).collect::<Vec<_>>(),
    }))
}

/// → `{on, held}`: whether the drawing shows its page breaks, and what its data holds (`null` for nothing).
fn page_breaks_settings(s: &mut Session, _: &Value) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed("page_breaks.settings", "no document open"))?;
    Ok(json!({ "on": crate::view::page_breaks_on(doc), "held": crate::view::page_breaks_held(doc) }))
}

/// Turns the Page Breaks box on or off → `{on, held}`. **No undo step** (the owner's ruling of 2026-11-22, "Unsaved, not undo", which extends to the settings of the
/// View tab's Visual Aids): the drawing is changed and unsaved, and Ctrl+Z leaves the box alone.
fn set_page_breaks(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page_breaks.set";
    let on = optional_bool(p, "on", ID)?.ok_or_else(|| bad(ID, "missing `on`"))?;
    s.edit_view(ID, |doc| {
        let changed = crate::view::set_page_breaks(doc, on).map_err(|words| failed(ID, words))?;
        Ok((json!({ "on": crate::view::page_breaks_on(doc), "held": crate::view::page_breaks_held(doc) }), changed))
    })
}
