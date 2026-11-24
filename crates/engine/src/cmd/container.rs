//! Containers: `container.add`, `container.style`, `container.heading`, `container.margin`, `container.fit`, `container.auto_resize`, `container.lock`,
//! `container.disband`, `container.select_contents`, `container.add_member`, `container.attach_edge` and the queries `container.styles` and `container.info`. The rules are in
//! `draw/container` and `docs/editing-commands.md` ("Containers").
//! Lengths are paper inches, as every command's are.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids, required_number, units_per_inch};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::container::{self, Add, GALLERY, Heading, MARGIN_PRESETS, Resize};
use crate::draw::{self, Problem};
use crate::error::{Result, bad, failed};

/// Puts in the answer `out` the notes of the containers on a locked layer that the command passed over, after the notes it has: only when
/// there are some.
pub(super) fn note_locked(doc: &diacraft_doc::Document, page: u32, locked: &[u32], out: &mut Value) -> std::result::Result<(), Problem> {
    let notes = container::locked_notes(doc, page, locked)?;
    if notes.is_empty() {
        return Ok(());
    }
    if let Some(map) = out.as_object_mut() {
        let entry = map.entry("notes").or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(list) = entry {
            list.extend(notes.into_iter().map(Value::String));
        }
    }
    Ok(())
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "container.styles",
            "Container Styles",
            ["Insert", "Diagram Parts", "Container"],
            None,
            "none",
            has_document,
            styles
        ),
        cmd!(
            query "container.info",
            "Container Properties",
            ["Container Format"],
            None,
            "ids: array of whole numbers, the shapes to describe (one that is no container is told in held_by when a container holds it); page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            info
        ),
        cmd!(
            "container.add",
            "Add Container",
            ["Insert", "Diagram Parts", "Container"],
            None,
            "style: text, optional, one of the ids in container.styles (default strip); heading: text, optional, the heading (default none); \
             ids: array of whole numbers, optional, the shapes the container goes round and holds: it is the box that holds them with a margin and the heading's strip, at the back of the page \
             (shapes of the page's own list, none in a locked container, a shape of another container leaves it). Left out, an empty container is dropped at x, y and holds nothing, even over shapes; \
             x: number, optional, the middle of an empty container in inches from the page's left edge (default the page's middle); y: number, optional, in inches from the page's bottom edge; \
             width: number, optional, an empty container's width in inches (default 3); height: number, optional, its height in inches (default 2); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes, else the first page that is not a background)",
            has_document,
            add
        ),
        cmd!(
            "container.style",
            "Container Style",
            ["Container Format", "Container Styles"],
            None,
            "ids: array of whole numbers, the containers; style: text, one of the ids in container.styles: its frame, tint, heading strip and the heading's alignment replace what the container had, \
             and its size, heading text, members and settings stay; page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            style
        ),
        cmd!(
            "container.heading",
            "Heading Style",
            ["Container Format", "Container Styles"],
            None,
            "ids: array of whole numbers, the containers (made from the gallery); side: text, optional, top, right, bottom or left, where the heading sits; \
             rotate: true, optional, instead of side, moves the heading one side on, clockwise (Rotate Heading); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            heading
        ),
        cmd!(
            "container.margin",
            "Margins",
            ["Container Format", "Size"],
            None,
            "ids: array of whole numbers, the containers; margin: a number (inches, 0 or more) or text, one of none, narrow, medium or wide (the room kept round the members, 0, 0.1, 0.25 and 0.5 inches); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            margin
        ),
        cmd!(
            "container.fit",
            "Fit to Contents",
            ["Container Format", "Size"],
            None,
            "ids: array of whole numbers, the containers: each is made the box that just holds its members with its margin and the heading's strip (a container with no member, a turned one and \
             one that holds itself are left alone, and the answer says the last two); page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            fit
        ),
        cmd!(
            "container.auto_resize",
            "Automatic Resize",
            ["Container Format", "Size"],
            None,
            "ids: array of whole numbers, the containers; mode: text, none (the size stays), expand (it grows to hold a member that needs room and never shrinks) or fit (it is always the size that just holds its members); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            auto_resize
        ),
        cmd!(
            "container.lock",
            "Lock Container",
            ["Container Format", "Membership"],
            None,
            "ids: array of whole numbers, the containers; locked: true or false, optional, left out it toggles from the first container's setting: a locked container takes and loses no member \
             (a member moves inside it, cannot be moved out of it or deleted, and a shape dropped over it does not join); page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            lock
        ),
        cmd!(
            "container.disband",
            "Disband Container",
            ["Container Format", "Membership"],
            None,
            "ids: array of whole numbers, the containers: each frame is deleted and the shapes it held stay (members of the container that held the frame, when one did), and a locked container is refused; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            disband
        ),
        cmd!(
            query "container.select_contents",
            "Select Contents",
            ["Container Format", "Membership"],
            None,
            "ids: array of whole numbers, the containers; page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            select_contents
        ),
        cmd!(
            "container.add_member",
            "Add to Underlying Container",
            ["Container", "Add to Underlying Container"],
            None,
            "ids: array of whole numbers, the shapes: each joins the container beneath it (the smallest one whose box holds the middle of the shape's, never a locked one, and a shape of another container leaves it); \
             container: whole number, optional, the container instead of the one beneath, wherever the shape is (a container with Automatic Resize then grows to hold it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            add_member
        ),
        cmd!(
            "container.attach_edge",
            "Attach to Container Edge",
            ["Container", "Attach to Container Edge"],
            None,
            "id: whole number, the shape; container: whole number, the container; edge: text, left, right, top or bottom: the shape becomes a member whose middle is on that edge, and stays there as the container is moved or resized; \
             page: whole number, optional, the id of the page (default: the first page holding both shapes)",
            has_document,
            attach_edge
        ),
    ]
}

/// The page the command works on: the one named, else the first page holding all `ids`.
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// → `{styles, margins, resize, headings, strip_inches}`: what the gallery and the Container Format tab offer.
fn styles(_: &mut Session, _: &Value) -> Result<Value> {
    let styles: Vec<Value> = GALLERY
        .iter()
        .map(|s| json!({ "id": s.id, "name": s.name, "frame": format!("{:?}", s.frame).to_ascii_lowercase(), "tint": s.tint, "strip": s.strip, "centred": s.centred }))
        .collect();
    let margins: Vec<Value> = MARGIN_PRESETS.iter().map(|(name, inches)| json!({ "name": name, "inches": inches })).collect();
    let headings: Vec<&str> = Heading::AROUND.iter().map(|h| h.word()).collect();
    Ok(json!({
        "styles": styles,
        "margins": margins,
        "resize": [Resize::None.word(), Resize::Expand.word(), Resize::Fit.word()],
        "headings": headings,
        "strip_inches": container::BAND_INCHES,
    }))
}

/// → `{page, containers: [{id, style, heading, resize, margin, locked, members, inside}], held_by: [{id, containers}]}` (a query): `held_by` is for the shapes among `ids` that are no
/// container and are in some.
fn info(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.info";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let (found, held_by) = container::describe(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
    let containers: Vec<Value> = found
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "style": c.style,
                "heading": c.heading.word(),
                "resize": c.resize.word(),
                "margin": c.margin,
                "locked": c.locked,
                "members": c.members,
                "inside": c.inside,
            })
        })
        .collect();
    let held_by: Vec<Value> = held_by.iter().map(|(id, containers)| json!({ "id": id, "containers": containers })).collect();
    Ok(json!({ "page": page, "containers": containers, "held_by": held_by }))
}

/// Adds a container → `{page, id, members, style, inside, fitted}`.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.add";
    let style = optional_text(p, "style", ID)?.unwrap_or("");
    let heading = optional_text(p, "heading", ID)?.unwrap_or("");
    let around: Vec<u32> = match p.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => required_ids(p, "ids", ID)?,
    };
    let page = {
        let named = optional_whole(p, "page", ID)?;
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        match (named, around.is_empty()) {
            (Some(page), _) => page,
            (None, false) => page_for(doc, None, &around, ID)?,
            (None, true) => doc
                .pages
                .iter()
                .find(|pg| !pg.background)
                .or_else(|| doc.pages.iter().next())
                .map(|pg| pg.id)
                .ok_or_else(|| bad(ID, "the document has no pages"))?,
        }
    };
    let (setup, k) = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        (draw::read_setup(doc, page).map_err(|e| e.into_engine(ID))?, units_per_inch(doc, page))
    };
    let x = optional_number(p, "x", ID)?.unwrap_or(setup.width / 2.0);
    let y = optional_number(p, "y", ID)?.unwrap_or(setup.height / 2.0);
    let (width, height) = (optional_number(p, "width", ID)?.unwrap_or(3.0), optional_number(p, "height", ID)?.unwrap_or(2.0));
    let above = s.id_floor(page);
    let added = s.edit("Add container", |doc| {
        let added = container::add(doc, page, Add { style, heading, around: &around, centre: (x * k, y * k), size: (width * k, height * k), above })
            .map_err(|e| e.into_engine(ID))?;
        let tidy = |doc: &mut diacraft_doc::Document| -> std::result::Result<(), Problem> {
            // A page that has a theme gives it to the new shape, and the active layers take it, in the same undo step; a page with Auto Size on grows to take it in.
            crate::theme::follow_page_theme(doc, page, added.id)?;
            draw::layers::join_active(doc, page, added.id)?;
            draw::fit_page(doc, page, &[added.id]).map(|_| ())
        };
        tidy(doc).map_err(|e| e.into_engine(ID))?;
        let mut out =
            json!({ "page": page, "id": added.id, "members": added.members, "style": added.style, "inside": added.inside, "fitted": added.fitted });
        note_locked(doc, page, &added.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok((added.id, out))
    })?;
    s.note_id(page, added.0);
    Ok(added.1)
}

/// Changes the look of containers → `{page, styled}`.
fn style(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.style";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let style = optional_text(p, "style", ID)?.ok_or_else(|| bad(ID, "missing `style`"))?;
    s.edit_if("Container style", |doc| {
        let styled = container::set_style(doc, page, &ids, style).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "styled": styled }), styled > 0))
    })
}

/// Moves the heading → `{page, heading, resized}`.
fn heading(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.heading";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let side = optional_text(p, "side", ID)?;
    let rotate = flag(p, "rotate", ID)?.unwrap_or(false);
    let side = match (side, rotate) {
        (Some(_), true) => return Err(bad(ID, "give `side` or `rotate`, not both")),
        (Some(word), false) => Some(
            Heading::from_word(word).ok_or_else(|| bad(ID, format!("`side` is top, right, bottom or left, not `{}`", crate::error::shown(word))))?,
        ),
        (None, true) => None,
        (None, false) => return Err(bad(ID, "give `side` (top, right, bottom or left) or `rotate`: true")),
    };
    s.edit("Container heading", |doc| {
        let fitted = container::set_heading(doc, page, &ids, side).map_err(|e| e.into_engine(ID))?;
        let mut out = json!({ "page": page, "resized": fitted.resized, "skipped": fitted.skipped, "moved": fitted.moved });
        note_locked(doc, page, &fitted.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok(out)
    })
}

/// Sets the margin → `{page, resized}`.
fn margin(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.margin";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let inches = match p.get("margin") {
        Some(Value::String(word)) => MARGIN_PRESETS.iter().find(|(name, _)| name == word).map(|(_, inches)| *inches).ok_or_else(|| {
            bad(ID, format!("`margin` is a number of inches or one of none, narrow, medium, wide, not `{}`", crate::error::shown(word)))
        })?,
        Some(_) => required_number(p, "margin", ID)?,
        None => return Err(bad(ID, "missing `margin`")),
    };
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    s.edit("Container margins", |doc| {
        let fitted = container::set_margin(doc, page, &ids, inches * k).map_err(|e| e.into_engine(ID))?;
        let mut out = json!({ "page": page, "margin": inches, "resized": fitted.resized, "skipped": fitted.skipped, "moved": fitted.moved });
        note_locked(doc, page, &fitted.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok(out)
    })
}

/// Fit to Contents → `{page, resized, skipped, moved}`; a fit that changes nothing is no undo step.
fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.fit";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Fit container to contents", |doc| {
        let fitted = container::fit(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
        let changed = !fitted.resized.is_empty() || !fitted.moved.is_empty();
        let mut out = json!({ "page": page, "resized": fitted.resized, "skipped": fitted.skipped, "moved": fitted.moved });
        note_locked(doc, page, &fitted.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok((out, changed))
    })
}

/// Automatic Resize → `{page, mode, resized}`.
fn auto_resize(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.auto_resize";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let word = optional_text(p, "mode", ID)?.ok_or_else(|| bad(ID, "missing `mode`"))?;
    let mode = Resize::from_word(word).ok_or_else(|| bad(ID, format!("`mode` is none, expand or fit, not `{}`", crate::error::shown(word))))?;
    s.edit("Container automatic resize", |doc| {
        let fitted = container::set_resize(doc, page, &ids, mode).map_err(|e| e.into_engine(ID))?;
        let mut out = json!({ "page": page, "mode": mode.word(), "resized": fitted.resized, "skipped": fitted.skipped, "moved": fitted.moved });
        note_locked(doc, page, &fitted.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok(out)
    })
}

/// Lock Container → `{page, locked}`.
fn lock(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.lock";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let locked = flag(p, "locked", ID)?;
    s.edit("Lock container", |doc| {
        let now = container::lock(doc, page, &ids, locked).map_err(|e| e.into_engine(ID))?;
        Ok(json!({ "page": page, "locked": now }))
    })
}

/// Disband Container → `{page, disbanded}`, and `notes` when a container on a locked layer that held one was passed over (it is not given the members).
fn disband(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.disband";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    s.edit("Disband container", |doc| {
        let (gone, locked) = container::disband(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
        let mut out = json!({ "page": page, "disbanded": gone });
        note_locked(doc, page, &locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok(out)
    })
}

/// Select Contents → `{page, ids}`: the shapes to select (a query: selection is the window's).
fn select_contents(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.select_contents";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let members = container::select_contents(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
    Ok(json!({ "page": page, "ids": members }))
}

/// Add to Underlying Container → `{page, joined: [{shape, container}], resized}`.
fn add_member(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.add_member";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let named = optional_whole(p, "container", ID)?;
    s.edit_if("Add to container", |doc| {
        let (joined, fitted) = container::add_member(doc, page, &ids, named).map_err(|e| e.into_engine(ID))?;
        let changed = !joined.is_empty();
        let list: Vec<Value> = joined.iter().map(|j| json!({ "shape": j.shape, "container": j.container })).collect();
        let mut out = json!({ "page": page, "joined": list, "resized": fitted.resized });
        note_locked(doc, page, &fitted.locked, &mut out).map_err(|e| e.into_engine(ID))?;
        Ok((out, changed))
    })
}

/// Attach to Container Edge → `{page, moved}`.
fn attach_edge(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "container.attach_edge";
    let id = required_u32(p, "id", ID)?;
    let container_id = required_u32(p, "container", ID)?;
    let page = target_page(s, p, &[id, container_id], ID)?;
    let word = optional_text(p, "edge", ID)?.ok_or_else(|| bad(ID, "missing `edge`"))?;
    let edge = ["left", "right", "top", "bottom"]
        .iter()
        .position(|w| *w == word)
        .ok_or_else(|| bad(ID, format!("`edge` is left, right, top or bottom, not `{}`", crate::error::shown(word))))?;
    s.edit("Attach to container edge", |doc| {
        let fitted = container::attach_edge(doc, page, id, container_id, edge).map_err(|e| e.into_engine(ID))?;
        Ok(json!({ "page": page, "moved": fitted.moved }))
    })
}
