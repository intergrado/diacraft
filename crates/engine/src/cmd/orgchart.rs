//! The Organization Chart: the commands behind the Org Chart tab and the template's behaviour. The rules are named in
//! `draw/relayout/org.rs` and `draw/relayout/org_picture.rs`, and in `docs/editing-commands.md` ("The Organization Chart").
//!
//! A *position* is a shape dropped from the Organization Chart Shapes (`stencil.list`, `orgchart`); the hierarchy is the glue between positions (a connector from a manager to a report).
//! Every length is in **paper inches**. A page is named by `page`; left out, it is the first page that holds the shape named, else the first page that holds a position.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole};
use super::{CommandSpec, has_document, required_text, required_u32};
use crate::Session;
use crate::draw::relayout::org::{self, ARRANGEMENTS, ChartLayout, ChartRequest, LAYOUTS, Laid};
use crate::draw::relayout::org_picture;
use crate::draw::{self, pictures, units_per_inch};
use crate::error::{EngineError, Result, bad, failed};

/// The most shapes the window's question about the drawing reads before it says it did not look at them all.
const SUMMARY_SHAPES: usize = 500_000;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "orgchart.info",
            "Organization Chart",
            ["Org Chart"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that holds a position); ids: array of whole numbers, optional, shapes to say more of (the selection); quick: true or false, optional, stop looking for positions at the first page that holds one (the answer's pages and document_positions are then that page's, and complete is false). \
             The answer is {page, positions, links, in_loops, roots, layout, spacing, pages, document_positions, complete, selected}: how many positions the page has, how many connectors link two of them, how many \
             positions are in a reporting loop, the first positions nothing reports to, the page's layout (one of the six of orgchart.layout, or null), the spaces in paper inches, how many positions each page holds, \
             how many the drawing holds (and whether every shape was looked at), and for each shape named whether it is a position, its kind, its manager, how many reports it has, how its reports are arranged and what its picture is",
            has_document,
            info
        ),
        cmd!(
            "orgchart.layout",
            "Org Chart Layout",
            ["Org Chart", "Layout"],
            None,
            "layout: text, one of top_to_bottom, bottom_to_top, left_to_right, right_to_left (each manager's reports side by side under it) or hanging_right, hanging_left (each manager's reports in a column to one side): \
             the layout of the whole chart, which also clears each manager's own arrangement; spacing: number, optional, more than 0, paper inches, the space between positions both ways (default: the page's); \
             page: whole number, optional, the id of the page. The positions move (their sizes are kept), the connectors are glued at the points that face each other and routed again, and the choice is written into the \
             page's PlaceStyle, in one undo step. A chart of more than 20000 positions is refused in words",
            has_org_chart,
            layout
        ),
        cmd!(
            "orgchart.relayout",
            "Re-Layout Org Chart",
            ["Org Chart", "Layout"],
            None,
            "spacing: number, optional, more than 0, paper inches (default: the page's); page: whole number, optional, the id of the page. Lays the chart out again keeping the layout options already applied: the page's \
             layout and each manager's own arrangement",
            has_org_chart,
            relayout
        ),
        cmd!(
            "orgchart.arrange",
            "Arrange Subordinates",
            ["Org Chart", "Layout"],
            None,
            "id: whole number, the shape that is the manager (a position); arrangement: text, one of horizontal (its reports side by side under it), vertical_right, vertical_left (its reports in a column to the right or the left of its middle line, or for a chart that flows across the page below or above it) or inherit (the page's layout again); page: whole number, optional, the id of the page. The choice is written into the position's ShapePlaceStyle \
             and the chart is laid out again, in one undo step",
            has_document,
            arrange
        ),
        cmd!(
            "orgchart.link",
            "Link Position to Manager",
            ["Org Chart", "Layout"],
            None,
            "id: whole number, the shape that is to report (a position); manager: whole number, the position it reports to; page: whole number, optional, the id of the page. A connector is made from the manager to the position, the \
             connector that linked it to another manager is taken away, and the chart is laid out again, in one undo step (shape.drop of a position onto another does this). A manager that reports to the position, directly or \
             through others, is refused in words: it would close a loop",
            has_document,
            link
        ),
        cmd!(
            "orgchart.picture_set",
            "Insert Picture on Position",
            ["Org Chart", "Picture"],
            None,
            "id: whole number, the shape (a position); path: text, the picture file (PNG, JPEG, BMP, EMF or WMF is drawn, GIF and TIFF are kept and drawn as a placeholder, and the budgets are those of insert.picture); page: whole number, optional, \
             the id of the page. The picture goes in a square at the left of the position, its proportions kept, and the text moves to the right of it. A picture the position has is replaced (Change Picture) and shown",
            has_document,
            picture_set
        ),
        cmd!(
            "orgchart.picture_delete",
            "Delete Picture from Position",
            ["Org Chart", "Picture"],
            None,
            "id: whole number, the shape (a position); page: whole number, optional, the id of the page. Takes the picture away and gives the text the whole position again",
            has_document,
            picture_delete
        ),
        cmd!(
            "orgchart.picture_hide",
            "Hide Picture on Position",
            ["Org Chart", "Picture"],
            None,
            "id: whole number, the shape (a position); hidden: true or false, optional (default true), hides the picture and keeps it, or shows it again; page: whole number, optional, the id of the page",
            has_document,
            picture_hide
        ),
    ]
}

/// Whether some page of the drawing has a position: what the commands of the tab work on.
fn has_org_chart(s: &Session) -> std::result::Result<(), String> {
    let doc = s.document().ok_or_else(|| "no document open".to_owned())?;
    let (_, total, _) = org::summary(doc, SUMMARY_SHAPES, true);
    if total > 0 { Ok(()) } else { Err("the drawing has no org chart: drop a position from the Organization Chart Shapes".to_owned()) }
}

/// The page a call works on: the one named; else, for a call that names a shape, the page that holds it; else the first page with a position.
fn target_page(s: &Session, p: &Value, shape: Option<u32>, cmd: &str) -> Result<u32> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    if let Some(id) = optional_whole(p, "page", cmd)? {
        return if doc.page(id).is_some() { Ok(id) } else { Err(bad(cmd, format!("there is no page {id}"))) };
    }
    if let Some(id) = shape {
        return draw::page_holding(doc, None, &[id]).ok_or_else(|| bad(cmd, format!("no page holds shape {id}")));
    }
    let (pages, _, _) = org::summary(doc, SUMMARY_SHAPES, true);
    pages
        .first()
        .map(|(id, _)| *id)
        .or_else(|| doc.pages.iter().find(|p| !p.background).or_else(|| doc.pages.iter().next()).map(|p| p.id))
        .ok_or_else(|| bad(cmd, "the document has no pages"))
}

/// A space in paper inches, more than 0 and within what the page may hold: in drawing units.
fn spacing_units(p: &Value, cmd: &str, k: f64) -> Result<Option<(f64, f64)>> {
    match optional_number(p, "spacing", cmd)? {
        None => Ok(None),
        Some(v) if v > 0.0 && v * k <= draw::MAX_COORD => Ok(Some((v * k, v * k))),
        Some(_) => Err(bad(cmd, format!("`spacing` must be a number of inches more than 0 and within {} drawing units", draw::MAX_COORD))),
    }
}

fn into(cmd: &str) -> impl Fn(draw::Problem) -> EngineError + '_ {
    move |p| p.into_engine(cmd)
}

/// What laying out said, as JSON (the numbers of the layout's own account).
fn laid_json(doc: &diacraft_doc::Document, page: u32, laid: &Laid) -> Value {
    let k = units_per_inch(doc, page);
    let now = draw::relayout::current(doc, page).ok();
    json!({
        "page": page,
        "positions": laid.positions,
        "layout": now.and_then(|n| ChartLayout::of_style(n.style)).map(ChartLayout::name),
        "spacing": now.map(|n| json!({ "x": n.avenue.0 / k, "y": n.avenue.1 / k })),
        "moved": laid.done.moved,
        "joins": laid.done.joins,
        "routed": laid.done.routed,
        "not_reglued": laid.done.not_reglued,
        "locked": laid.done.locked,
        "followers": laid.done.followers,
        "enlarged": laid.done.enlarged,
    })
}

/// A shape's account for `orgchart.info`.
fn shape_json(doc: &diacraft_doc::Document, chart: &org::Chart, id: u32) -> Value {
    let Some(position) = chart.position(id) else {
        return json!({ "id": id, "position": false });
    };
    let (manager, reports) = chart.manager_and_reports(id);
    let picture = org_picture::state(doc, &position.at);
    json!({
        "id": id,
        "position": true,
        "kind": position.kind,
        "manager": manager,
        "reports": reports,
        "arrangement": org::arrangement_of(doc, &position.at).name(),
        "picture": { "has": picture.has, "hidden": picture.hidden },
    })
}

/// → see the command's parameters. Reads only.
fn info(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.info";
    let ids: Vec<u32> = match p.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => list
            .iter()
            .take(1_024)
            .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(ID, "`ids` must hold whole numbers")))
            .collect::<Result<Vec<u32>>>()?,
        Some(_) => return Err(bad(ID, "`ids` must be an array of whole numbers")),
    };
    let page = target_page(s, p, ids.first().copied(), ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let quick = matches!(p.get("quick"), Some(Value::Bool(true)));
    let (pages, total, complete) = org::summary(doc, SUMMARY_SHAPES, quick);
    let chart = org::chart(doc, page).map_err(into(ID))?;
    let now = draw::relayout::current(doc, page).map_err(into(ID))?;
    let k = units_per_inch(doc, page);
    let roots = chart.roots(64);
    Ok(json!({
        "page": page,
        "positions": chart.positions.len(),
        "links": chart.links.len(),
        "in_loops": chart.in_loops(),
        "roots": roots,
        "layout": if now.place_style == 0 { None } else { ChartLayout::of_style(now.style).map(ChartLayout::name) },
        "layouts": LAYOUTS.iter().map(|l| l.name()).collect::<Vec<_>>(),
        "arrangements": ARRANGEMENTS.iter().map(|a| a.name()).collect::<Vec<_>>(),
        "spacing": { "x": now.avenue.0 / k, "y": now.avenue.1 / k },
        "pages": pages.iter().map(|(id, n)| json!({ "page": id, "positions": n })).collect::<Vec<_>>(),
        "document_positions": total,
        "complete": complete,
        "selected": ids.iter().map(|id| shape_json(doc, &chart, *id)).collect::<Vec<_>>(),
    }))
}

/// Runs a layout of the chart as one edit → the layout's account.
fn lay_out(
    s: &mut Session,
    label: &str,
    cmd: &str,
    page: u32,
    make: impl FnOnce(&mut diacraft_doc::Document, f64) -> Result<ChartRequest>,
) -> Result<Value> {
    s.edit_if(label, |doc| {
        let k = units_per_inch(doc, page);
        let req = make(doc, k)?;
        let laid = org::lay_out(doc, page, &req).map_err(into(cmd))?;
        Ok((laid_json(doc, page, &laid), laid.changed))
    })
}

fn layout(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.layout";
    let word = required_text(p, "layout", ID)?;
    let chosen = ChartLayout::parse(word)
        .ok_or_else(|| bad(ID, format!("`layout` must be one of {}", LAYOUTS.iter().map(|l| l.name()).collect::<Vec<_>>().join(", "))))?;
    let page = target_page(s, p, None, ID)?;
    lay_out(s, "Org Chart Layout", ID, page, |_, k| Ok(ChartRequest { layout: Some(chosen), spacing: spacing_units(p, ID, k)? }))
}

fn relayout(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.relayout";
    let page = target_page(s, p, None, ID)?;
    lay_out(s, "Re-Layout Org Chart", ID, page, |_, k| Ok(ChartRequest { layout: None, spacing: spacing_units(p, ID, k)? }))
}

fn arrange(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.arrange";
    let id = required_u32(p, "id", ID)?;
    let word = required_text(p, "arrangement", ID)?;
    let how = org::Arrangement::parse(word)
        .ok_or_else(|| bad(ID, format!("`arrangement` must be one of {}", ARRANGEMENTS.iter().map(|a| a.name()).collect::<Vec<_>>().join(", "))))?;
    let page = target_page(s, p, Some(id), ID)?;
    s.edit_if("Arrange Subordinates", |doc| {
        org::arrange(doc, page, id, how).map_err(into(ID))?;
        let laid = org::lay_out(doc, page, &ChartRequest::default()).map_err(into(ID))?;
        let mut out = laid_json(doc, page, &laid);
        if let Some(map) = out.as_object_mut() {
            map.insert("id".into(), json!(id));
            map.insert("arrangement".into(), json!(how.name()));
        }
        Ok((out, laid.changed))
    })
}

fn link(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.link";
    let (id, manager) = (required_u32(p, "id", ID)?, required_u32(p, "manager", ID)?);
    let page = target_page(s, p, Some(id), ID)?;
    let above = s.id_floor(page);
    let out = s.edit_if("Link Position to Manager", |doc| {
        let linked = org::link(doc, page, id, manager, above).map_err(into(ID))?;
        let laid = org::lay_out(doc, page, &ChartRequest::default()).map_err(into(ID))?;
        let mut out = laid_json(doc, page, &laid);
        if let Some(map) = out.as_object_mut() {
            map.insert("id".into(), json!(id));
            map.insert("manager".into(), json!(manager));
            map.insert("connector".into(), json!(linked.connector));
            map.insert("replaced".into(), json!(linked.replaced));
        }
        Ok((out, linked.changed || laid.changed))
    })?;
    if let Some(connector) = out.get("connector").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
        s.note_id(page, connector);
    }
    Ok(out)
}

/// The refusal for a file that was not read.
fn file_problem(cmd: &str, error: crate::file::FileError) -> EngineError {
    match error {
        crate::file::FileError::NotFound | crate::file::FileError::NotAFile => bad(cmd, error.to_string()),
        other => failed(cmd, other.to_string()),
    }
}

fn picture_set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.picture_set";
    let id = required_u32(p, "id", ID)?;
    let path = required_text(p, "path", ID)?;
    if path.is_empty() {
        return Err(bad(ID, "`path` must name the picture file"));
    }
    let page = target_page(s, p, Some(id), ID)?;
    // The size is looked at before a byte is read: a file past the most a picture may be is refused by its length.
    let path = std::path::Path::new(path);
    let len = crate::file::file_len(path).map_err(|e| file_problem(ID, e))?;
    if len > pictures::MAX_PICTURE_BYTES {
        return Err(bad(
            ID,
            format!(
                "the picture's file is {} MiB, and a picture may be at most {} MiB",
                len.div_ceil(1024 * 1024),
                pictures::MAX_PICTURE_BYTES / (1024 * 1024)
            ),
        ));
    }
    let bytes = crate::file::read_file(path, pictures::MAX_PICTURE_BYTES).map_err(|e| file_problem(ID, e))?;
    let info = pictures::inspect(&bytes).map_err(into(ID))?;
    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(bytes);
    let size = shared.len();
    s.edit_if("Insert Picture on Position", |doc| {
        let done = org_picture::set_picture(doc, page, id, &info, shared).map_err(into(ID))?;
        let mut out = json!({
            "page": page,
            "id": id,
            "format": info.format.word(),
            "drawn": info.format.drawn(),
            "shared": done.shared,
            "replaced": done.replaced,
            "bytes": size,
        });
        if let (false, Some(map)) = (info.format.drawn(), out.as_object_mut()) {
            map.insert(
                "note".to_owned(),
                json!(format!(
                    "a {} is kept in the drawing as it is and shown as a placeholder: DiaCraft does not decode that format yet",
                    info.format.word().to_ascii_uppercase()
                )),
            );
        }
        Ok((out, true))
    })
}

fn picture_delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.picture_delete";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, Some(id), ID)?;
    s.edit_if("Delete Picture from Position", |doc| {
        let had = org_picture::delete_picture(doc, page, id).map_err(into(ID))?;
        Ok((json!({ "page": page, "id": id, "deleted": had }), had))
    })
}

fn picture_hide(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "orgchart.picture_hide";
    let id = required_u32(p, "id", ID)?;
    let hidden = match p.get("hidden") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(ID, "`hidden` must be true or false")),
    };
    let page = target_page(s, p, Some(id), ID)?;
    s.edit_if("Hide Picture on Position", |doc| {
        let changed = org_picture::hide_picture(doc, page, id, hidden).map_err(into(ID))?;
        Ok((json!({ "page": page, "id": id, "hidden": hidden }), changed))
    })
}
