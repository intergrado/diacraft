//! Page setup: the commands behind the Design tab's Page Setup group (Orientation, Size, Auto Size) and the Page Setup dialog's page tabs, and the
//! page-list commands that were not there (delete a page, reorder pages). The rules are in `draw/page_setup.rs`, `draw/page_fit.rs` and
//! `draw/pages.rs` and in `docs/editing-commands.md`, "Page setup".
//!
//! Every length is in **paper inches**, as in the other editing commands. A page is named by `page`; left out, it is the first page that is
//! not a background (a file lists its backgrounds first).

use diacraft_units::Unit;
use serde_json::{Value, json};

use super::params::{optional_number, optional_whole};
use super::{CommandSpec, always, has_document, optional_text, required_text, required_u32};
use crate::Session;
use crate::draw::paper::{self, Orientation};
use crate::draw::{self, PageSetup, PaperChoice, ScaleChoice, SizeChoice};
use crate::error::{Result, bad, failed, shown};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "page.presets",
            "Page Presets",
            ["Design", "Page Setup"],
            None,
            "none",
            always,
            presets
        ),
        cmd!(
            query "page.setup",
            "Page Setup",
            ["Design", "Page Setup"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            setup
        ),
        cmd!(
            "page.size",
            "Page Size",
            ["Design", "Page Setup"],
            None,
            "preset: text, optional, the id of a size in page.presets (letter, a4, ...): the page takes that size in the orientation it has, and when the size is a paper \
             the printer paper follows; \
             width: number, optional, with height a custom size in inches on paper (give a preset, or both width and height); \
             height: number, optional, see width; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            size
        ),
        cmd!(
            "page.orientation",
            "Page Orientation",
            ["Design", "Page Setup"],
            None,
            "orientation: text, portrait or landscape: the page and the printer paper are turned to it; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            orientation
        ),
        cmd!(
            "page.scale",
            "Drawing Scale",
            ["Design", "Page Setup"],
            None,
            "preset: text, optional, the id of a scale in page.presets (none, arch_1_4, metric_1_100, ...); \
             paper: number, optional, with drawing a custom scale: a length on paper (give a preset, or paper and drawing); \
             paper_unit: text, optional, the unit of paper: in, ft, yd, mi, mm, cm, m or km (default in); \
             drawing: number, optional, the length in the drawing that paper stands for; \
             drawing_unit: text, optional, the unit of drawing, as paper_unit (default in); \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            scale
        ),
        cmd!(
            "page.paper",
            "Printer Paper",
            ["Design", "Page Setup"],
            None,
            "preset: text, optional, the id of a size in page.presets that is a paper; \
             kind: whole number, optional, a paper kind from page.presets (give a preset or a kind, or neither); \
             orientation: text, optional, portrait or landscape, the way the paper is held; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            paper_command
        ),
        cmd!(
            "page.auto_size",
            "Auto Size",
            ["Design", "Page Setup"],
            None,
            "on: true or false, whether the page grows by whole sheets of its printer paper when a shape is put or moved past its edge; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            auto_size
        ),
        cmd!(
            "page.type",
            "Page Type",
            ["Design", "Page Setup"],
            None,
            "type: text, foreground or background: the page is made a background page (put with the background pages, and the pages that are not backgrounds can name it) or a page that is not one \
             (the pages that named it name none, the answer says how many, and it goes after the other pages), and a drawing keeps one page that is not a background; \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            page_type
        ),
        cmd!(
            "page.background",
            "Page Background",
            ["Design", "Page Setup"],
            None,
            "background: whole number, the id of a background page the page is shown over, or null or the text none for no background (a page names at most one, never itself and never a page that names it back, and a background page may have one of its own); \
             page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            background
        ),
        cmd!(
            "page.delete",
            "Delete Page",
            ["Page"],
            None,
            "id: whole number, the id of the page to delete, with its shapes (a drawing keeps one page that is not a background, and the pages that named a background page deleted name none)",
            has_two_pages,
            delete
        ),
        cmd!(
            "page.move",
            "Reorder Pages",
            ["Page"],
            None,
            "id: whole number, the id of the page to move (not a background page); \
             to: whole number, the place it takes among the pages that are not backgrounds, counted from 0; \
             renumber: true or false, optional, name the pages with a default name (Page-1, Page-2, ...) for the places they now have, refused when a name it would give is another page's (default false)",
            has_two_pages,
            move_page
        ),
    ]
}

/// A drawing with at least two pages: one page cannot be deleted (a drawing keeps one) or put in another place.
fn has_two_pages(s: &Session) -> std::result::Result<(), String> {
    has_document(s)?;
    if s.document().is_some_and(|doc| doc.pages.len() >= 2) { Ok(()) } else { Err("the drawing has one page".to_owned()) }
}

/// The page the command works on (see the module documentation).
pub(super) fn target_page(s: &Session, p: &Value, cmd: &str) -> Result<u32> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    match optional_whole(p, "page", cmd)? {
        Some(id) if doc.page(id).is_some() => Ok(id),
        Some(id) => Err(bad(cmd, format!("there is no page {id}"))),
        None => doc.first_foreground_page().map(|page| page.id).ok_or_else(|| bad(cmd, "the document has no pages")),
    }
}

fn optional_bool(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn round(v: f64) -> f64 {
    let rounded = (v * 1.0e9).round() / 1.0e9;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// A length cell's number as the unit it shows in has it: `V` is in inches. A unit that is empty or not one is the inch.
fn shown_in(inches: f64, unit: &str) -> (f64, String) {
    let unit = diacraft_units::parse_unit(unit).filter(|u| u.measure() == diacraft_units::Measure::Length).unwrap_or(Unit::Inch);
    (round(unit.from_internal(inches)), unit.code().to_owned())
}

fn scale_json(preset: &paper::ScalePreset) -> Value {
    json!({
        "id": preset.id,
        "name": preset.name,
        "kind": preset.kind,
        "family": paper::scale_family(preset.kind),
        "paper": preset.paper.0,
        "paper_unit": preset.paper.1.code(),
        "drawing": preset.drawing.0,
        "drawing_unit": preset.drawing.1.code(),
        "units_per_inch": round(preset.drawing.1.to_internal(preset.drawing.0) / preset.paper.1.to_internal(preset.paper.0)),
    })
}

/// The answer of every page setup command: what the page says now, `page.setup`'s.
fn setup_json(s: &PageSetup) -> Value {
    let (paper_value, paper_unit) = shown_in(s.scale.paper, &s.scale.paper_unit);
    let (drawing_value, drawing_unit) = shown_in(s.scale.drawing, &s.scale.drawing_unit);
    json!({
        "page": s.page,
        "name": s.name,
        "background": s.background,
        "back_page": s.back_page,
        "width": round(s.width),
        "height": round(s.height),
        "orientation": s.orientation.name(),
        "size": s.size.map(|p| json!({ "id": p.id, "name": p.name })),
        "drawing": { "width": round(s.drawing_width), "height": round(s.drawing_height), "units_per_inch": round(s.units_per_inch) },
        "paper": {
            "kind": s.paper.kind,
            "name": s.paper.known.map(|p| p.name),
            "assumed": s.paper.assumed,
            "width": round(s.paper.width),
            "height": round(s.paper.height),
            "orientation": s.paper.orientation.name(),
            "orientation_set": s.paper.orientation_set,
        },
        "scale": {
            "preset": s.scale.preset.map(|p| p.id),
            "kind": s.scale.kind,
            "family": paper::scale_family(s.scale.kind),
            "paper": paper_value,
            "paper_unit": paper_unit,
            "drawing": drawing_value,
            "drawing_unit": drawing_unit,
        },
        "auto_size": s.auto_size,
        "sheets": { "across": s.sheets.0, "down": s.sheets.1 },
    })
}

/// What `page` says now, as the answer of a command.
fn answer(doc: &diacraft_doc::Document, page: u32, cmd: &str) -> Result<Value> {
    draw::read_setup(doc, page).map(|s| setup_json(&s)).map_err(|p| p.into_engine(cmd))
}

/// → `{sizes, papers, scales}`: the named page sizes (`id`, `name`, `group`, `width` and `height` on paper in inches held portrait, `paper_kind` the
/// paper that size is, or null), the papers whose size is known (`kind`, `name`, `width`, `height`), and the drawing scales (`id`, `name`, `kind`,
/// `family`, `paper` and `drawing` in the units `paper_unit` and `drawing_unit` name, `units_per_inch`). The lists do not depend on the document.
fn presets(_: &mut Session, _: &Value) -> Result<Value> {
    let sizes: Vec<Value> = paper::SIZES
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "group": s.group,
                "width": round(s.width),
                "height": round(s.height),
                "paper_kind": s.paper_kind,
            })
        })
        .collect();
    let papers: Vec<Value> =
        paper::PAPERS.iter().map(|p| json!({ "kind": p.kind, "name": p.name, "width": round(p.width), "height": round(p.height) })).collect();
    let scales: Vec<Value> = paper::SCALES.iter().map(scale_json).collect();
    Ok(json!({ "sizes": sizes, "papers": papers, "scales": scales }))
}

/// → `{page, name, background, width, height, orientation, size, drawing, paper, scale, auto_size, sheets}`: a page's setup. `width` and `height`
/// are the page on paper in inches, `orientation` the page's (`portrait` or `landscape`), `size` the named size it is in either orientation or null;
/// `drawing` has `PageWidth` and `PageHeight` in drawing units and `units_per_inch`; `paper` the printer paper (`kind`, `name`, `assumed` when the
/// size is a stand-in, `width`, `height` and `orientation` of one sheet as held, `orientation_set`); `scale` the drawing scale (`preset` or null,
/// `kind`, `family`, `paper` and `drawing` with their units); `auto_size`; and `sheets` how many sheets of the paper the page covers across and down.
fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.setup";
    let page = target_page(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    answer(doc, page, ID)
}

/// The size a call asks for: a named one, or `width` and `height`.
fn size_choice(p: &Value, cmd: &str) -> Result<SizeChoice> {
    let preset = optional_text(p, "preset", cmd)?;
    let (width, height) = (optional_number(p, "width", cmd)?, optional_number(p, "height", cmd)?);
    match (preset, width, height) {
        (Some(id), None, None) => {
            let found = paper::size_preset(id).ok_or_else(|| bad(cmd, format!("no size `{}` (see page.presets)", shown(id))))?;
            Ok(SizeChoice::Preset(found))
        }
        (None, Some(width), Some(height)) => Ok(SizeChoice::Custom { width, height }),
        (None, Some(_), None) | (None, None, Some(_)) => Err(bad(cmd, "`width` and `height` go together: give both or neither")),
        (None, None, None) => Err(bad(cmd, "give `preset`, or `width` and `height`")),
        (Some(_), _, _) => Err(bad(cmd, "give `preset`, or `width` and `height`, not both")),
    }
}

/// Gives a page a size → the page's setup. A size it has already is no undo step.
fn size(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.size";
    let page = target_page(s, p, ID)?;
    let choice = size_choice(p, ID)?;
    s.edit_if("Page size", |doc| {
        let changed = draw::set_size(doc, page, choice).map_err(|e| e.into_engine(ID))?;
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// Turns a page and its paper → the page's setup. A page that has the orientation, on paper that has it, is no undo step.
fn orientation(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.orientation";
    let page = target_page(s, p, ID)?;
    let word = required_text(p, "orientation", ID)?;
    let target = Orientation::parse(word).ok_or_else(|| bad(ID, "`orientation` must be `portrait` or `landscape`"))?;
    s.edit_if("Page orientation", |doc| {
        let changed = draw::set_orientation(doc, page, target).map_err(|e| e.into_engine(ID))?;
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// The length and the unit a custom scale gives for `key` and `unit_key`: the unit is inches when left out.
fn scale_length(p: &Value, key: &str, unit_key: &str, cmd: &str) -> Result<Option<(f64, Unit)>> {
    let Some(value) = optional_number(p, key, cmd)? else {
        if p.get(unit_key).is_some_and(|v| !v.is_null()) {
            return Err(bad(cmd, format!("`{unit_key}` goes with `{key}`")));
        }
        return Ok(None);
    };
    let unit = match optional_text(p, unit_key, cmd)? {
        None => Unit::Inch,
        Some(text) => {
            draw::length_unit(text).ok_or_else(|| bad(cmd, format!("`{unit_key}` must be in, ft, yd, mi, mm, cm, m or km, not `{}`", shown(text))))?
        }
    };
    Ok(Some((value, unit)))
}

/// The scale a call asks for: a listed one, or `paper` and `drawing`.
fn scale_choice(p: &Value, cmd: &str) -> Result<ScaleChoice> {
    let preset = optional_text(p, "preset", cmd)?;
    let (paper_length, drawing_length) = (scale_length(p, "paper", "paper_unit", cmd)?, scale_length(p, "drawing", "drawing_unit", cmd)?);
    match (preset, paper_length, drawing_length) {
        (Some(id), None, None) => {
            let found = paper::scale_preset(id).ok_or_else(|| bad(cmd, format!("no scale `{}` (see page.presets)", shown(id))))?;
            Ok(ScaleChoice::Preset(found))
        }
        (None, Some(paper), Some(drawing)) => Ok(ScaleChoice::Custom { paper, drawing }),
        (None, Some(_), None) | (None, None, Some(_)) => Err(bad(cmd, "`paper` and `drawing` go together: give both or neither")),
        (None, None, None) => Err(bad(cmd, "give `preset`, or `paper` and `drawing`")),
        (Some(_), _, _) => Err(bad(cmd, "give `preset`, or `paper` and `drawing`, not both")),
    }
}

/// Gives a page a drawing scale and keeps its size on paper → the page's setup.
fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.scale";
    let page = target_page(s, p, ID)?;
    let choice = scale_choice(p, ID)?;
    s.edit_if("Drawing scale", |doc| {
        let changed = draw::set_scale(doc, page, choice).map_err(|e| e.into_engine(ID))?;
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// The printer paper a call asks for.
fn paper_choice(p: &Value, cmd: &str) -> Result<PaperChoice> {
    let preset = optional_text(p, "preset", cmd)?;
    let kind = optional_whole(p, "kind", cmd)?;
    let way = optional_text(p, "orientation", cmd)?
        .map(|word| Orientation::parse(word).ok_or_else(|| bad(cmd, "`orientation` must be `portrait` or `landscape`")))
        .transpose()?;
    let kind = match (preset, kind) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `preset` or `kind`, not both")),
        (Some(id), None) => {
            let found = paper::size_preset(id).ok_or_else(|| bad(cmd, format!("no size `{}` (see page.presets)", shown(id))))?;
            Some(found.paper_kind.ok_or_else(|| bad(cmd, format!("the size `{}` is not a paper whose size is known", found.id)))?)
        }
        (None, kind) => kind,
    };
    if kind.is_none() && way.is_none() {
        return Err(bad(cmd, "give a paper (`preset` or `kind`), an `orientation`, or both"));
    }
    Ok(PaperChoice { kind, orientation: way })
}

/// Gives a page its printer paper and the way round it is held; the page's own size is not touched → the page's setup.
fn paper_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.paper";
    let page = target_page(s, p, ID)?;
    let choice = paper_choice(p, ID)?;
    s.edit_if("Printer paper", |doc| {
        let changed = draw::set_paper(doc, page, choice).map_err(|e| e.into_engine(ID))?;
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// Turns Auto Size on or off → the page's setup. A page that is in the state asked for is no undo step.
fn auto_size(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.auto_size";
    let page = target_page(s, p, ID)?;
    let on = optional_bool(p, "on", ID)?.ok_or_else(|| bad(ID, "missing `on`"))?;
    s.edit_if(if on { "Auto Size on" } else { "Auto Size off" }, |doc| {
        let changed = draw::set_auto_size(doc, page, on).map_err(|e| e.into_engine(ID))?;
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// Deletes a page → `{id, name, pages, unassigned}`: the page's name, how many pages the drawing has now, and how many pages named it as their background and name none now.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.delete";
    let id = required_u32(p, "id", ID)?;
    s.edit("Delete page", |doc| {
        let before = doc.clone();
        let deleted = draw::delete_page(doc, id).map_err(|e| e.into_engine(ID))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok(json!({ "id": id, "name": deleted.name, "pages": doc.pages.len(), "unassigned": deleted.unassigned }))
    })
}

/// Makes a page a background page or a page that is not one → the page's setup, and `unassigned`: how many pages named it as their background and name none now. A page that is
/// of the type already is no undo step.
fn page_type(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.type";
    let page = target_page(s, p, ID)?;
    let background = match required_text(p, "type", ID)? {
        "background" => true,
        "foreground" => false,
        other => return Err(bad(ID, format!("`type` must be foreground or background, not `{}`", shown(other)))),
    };
    s.edit_if(if background { "Page type: background" } else { "Page type: foreground" }, |doc| {
        let before = doc.clone();
        let done = draw::set_page_type(doc, page, background).map_err(|e| e.into_engine(ID))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        let mut said = answer(doc, page, ID)?;
        if let Value::Object(map) = &mut said {
            map.insert("unassigned".to_owned(), json!(done.unassigned));
        }
        Ok((said, done.changed))
    })
}

/// Gives a page a background page, or none → the page's setup. A page that has the background is no undo step.
fn background(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.background";
    let page = target_page(s, p, ID)?;
    let back = match p.get("background") {
        None => return Err(bad(ID, "missing `background`: the id of a background page, or null or none")),
        Some(Value::Null) => None,
        Some(Value::String(word)) if word.eq_ignore_ascii_case("none") => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| bad(ID, "`background` must be the id of a background page, or null or none"))?,
        ),
    };
    s.edit_if("Page background", |doc| {
        let before = doc.clone();
        let changed = draw::set_background(doc, page, back).map_err(|e| e.into_engine(ID))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok((answer(doc, page, ID)?, changed))
    })
}

/// Moves a page among the pages that are not backgrounds → `{id, from, to, renamed}`: the place it had and has, counted from 0, and how many pages
/// `renumber` renamed. A move to the place a page has, with nothing to rename, is no undo step.
fn move_page(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.move";
    let id = required_u32(p, "id", ID)?;
    let to = usize::try_from(required_u32(p, "to", ID)?).map_err(|_| bad(ID, "`to` is too large"))?;
    let renumber = optional_bool(p, "renumber", ID)?.unwrap_or(false);
    s.edit_if("Reorder pages", |doc| {
        let before = doc.clone();
        let moved = draw::move_page(doc, id, to, renumber).map_err(|e| e.into_engine(ID))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok((json!({ "id": id, "from": moved.from, "to": moved.to, "renamed": moved.renamed }), moved.from != moved.to || moved.renamed > 0))
    })
}
