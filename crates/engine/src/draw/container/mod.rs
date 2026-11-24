//! Containers: a frame that holds shapes, with a heading. The rules are in `docs/editing-commands.md` ("Containers").
//!
//! **The model is the ShapeSheet, and nothing else.** A container is a shape typed `User.msvStructureType` = `Container`; who it holds is its
//! `Relationships` cell, `DEPENDSON(1, Sheet.<id>!SheetRef(), ...)`, and each member names it back under 4 ([`ties`] keeps both sides true). Its settings are
//! `User.DiaCraftContainer*` rows ([`settings`]). Nothing here keeps a second list: `CONTAINERMEMBERCOUNT`, `CONTAINERCOUNT` and `CONTAINERSHEETREF` of `calc` read the same cells.
//!
//! * [`ties`] (`draw::ties`, **shared with the callouts**: one reader and one writer of the `Relationships` cell): the cell as a list of calls; a formula that is not one is refused in words and never rewritten.
//! * [`page`]: one read of a page, with the work charged: **one budget for a command** ([`MAX_WORK`], the edit's).
//! * [`membership`]: joining and leaving, the container a drop or a move puts a shape in.
//! * [`fit`]: the box a container needs, Fit to Contents and the automatic resizing, the shapes on its edges; at most [`MAX_FITTED`] containers a command.
//! * [`settings`]: Margins, Automatic Resize, Lock and the heading's side.
//! * [`cycles`]: **membership is a tree**: no command makes a tie that closes a loop (`membership`'s `Graph::join` asks first), and a loop a file
//!   holds is broken when the file is read, in words, with nothing else changed.
//!
//! **What every move, copy and delete does.** Moving a container moves what it holds ([`followers`], used by `draw::move_each`), copying it copies what it holds
//! ([`with_contents`]) and gives the copies ties among themselves ([`repair_copies`]), deleting it deletes what it holds, and a shape that is deleted leaves every tie it was named
//! in ([`release`]), so that a new shape that is given its `ID` after a save and a reopening is not taken for a member. A shape that is dropped, moved or pasted inside a container
//! joins it ([`arrive`]); a shape of a locked container cannot be moved out of it or deleted.
//!
//! A container on a locked layer is left as it is by the automatic steps, which pass over it and note it ([`locked_notes`]): it takes no
//! member, loses none and is not fitted. A command that would take a member out of it (Add Container round the member, Add to Underlying Container, Attach to Container Edge) is
//! refused in words; a disband of a container inside it does not give it the members. A container that holds a shape on a locked layer is not disbanded.
//!
//! Only the shapes of a page's own list are members: a member of a group is not, and a group is one shape. A turned or flipped container is not fitted.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, MAX_SHAPES_PER_PAGE, ShapeRef, SheetRef};

pub(crate) mod cycles;
mod fit;
mod lists;
mod membership;
mod page;
mod settings;
pub mod swimlane;

#[cfg(test)]
mod swimlane_tests;
#[cfg(test)]
mod tests;

pub use fit::{Fitted, MAX_FITTED};
pub use lists::{Dir, MAX_LIST_MEMBERS, MIN_INCHES, PHASE_BAND_INCHES};
pub(crate) use page::EditWork;
pub use page::MAX_WORK;
/// A box on the page (and, for the Kani harnesses of the crate, `crate::kani_proofs`).
pub(crate) use page::Rect;
pub use settings::{BAND_INCHES, DEFAULT_MARGIN_INCHES, MARGIN_PRESETS, Resize};

use self::fit::refit;
use self::membership::{Graph, arrive as arrive_in, underlying};
use self::page::{Page, Unique, Work};
use self::settings::{Settings, length_cell, number_cell, put as put_row, text_cell};
use super::shapes::{delete_shapes, page_stats};
use super::ties;
use super::{Problem, checked, page_mut, require_page, settle, shape_mut};
use crate::stencil::container::{
    DEFAULT_STYLE, HEADING_ROW, LOCK_ROW, MARGIN_ROW, Made, RESIZE_ROW, STYLE_ROW, STYLES, Side, container_shape, find_style, look,
};

pub use crate::stencil::container::{Frame, STYLES as GALLERY, Side as Heading, StyleSpec};

/// A container to add.
#[derive(Clone, Copy, Debug)]
pub struct Add<'a> {
    /// A style of the gallery (`DEFAULT_STYLE` when it is empty).
    pub style: &'a str,
    /// The heading text (empty for none).
    pub heading: &'a str,
    /// The shapes it goes round, which it then holds; none for an empty one dropped at `centre`.
    pub around: &'a [u32],
    /// The middle of an empty container, and its size, in drawing units.
    pub centre: (f64, f64),
    pub size: (f64, f64),
    /// The `ID` the session's high-water mark keeps new shapes above.
    pub above: u32,
}

/// A container that was added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: u32,
    /// The shapes it holds.
    pub members: Vec<u32>,
    pub style: &'static str,
    /// The container that took it, when it was made inside another.
    pub inside: Option<u32>,
    /// The containers that were fitted because of it.
    pub fitted: Vec<u32>,
    /// The containers on a locked layer it passed over: one it is inside does not take it, and is not fitted.
    pub locked: Vec<u32>,
}

/// A refusal for a shape the page does not list at its top level.
fn not_here(id: u32) -> Problem {
    Problem::bad(format!("page has no shape {id} in its own list: a shape inside a group is not put in a container"))
}

fn not_a_container(id: u32) -> Problem {
    Problem::bad(format!("shape {id} is not a container (`User.msvStructureType` is not `Container` and it holds no members)"))
}

fn locked_words(member: u32, container: u32, what: &str) -> Problem {
    Problem::failed(format!("shape {member} is a member of the locked container {container}, so it cannot be {what}: unlock the container first"))
}

/// The refusal of a command that would take the member `member` out of the container `container`, which is on the locked layer `layer`, so that it is `what`.
fn frozen_words(member: u32, container: u32, layer: &super::layers::Layer, what: &str) -> Problem {
    Problem::bad(format!(
        "container {container} is on layer {} (\"{}\"), which is locked, and shape {member} is a member of it: a container on a locked layer takes and loses no member, so the \
         shape cannot be {what}; unlock the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    ))
}

/// Refuses a command that would take the shape `id` out of a container on a locked layer that it is in.
fn refuse_frozen_holder(page: &Page<'_>, id: u32, what: &str) -> Result<(), Problem> {
    for c in page.containers_of(id)? {
        if let Some(layer) = page.locked_layer(c)? {
            return Err(frozen_words(id, c, &layer, what));
        }
    }
    Ok(())
}

/// The words of a note for each container of `ids` (on page `page`) that a command passed over because it is on a locked layer: it was
/// left as it was. One walk of the page finds them.
pub fn locked_notes(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<String>, Problem> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let found = super::scan::scan(doc, page, &wanted)?.found;
    let mut locks = super::layers::Locks::default();
    let mut out = Vec::new();
    for id in ids {
        let Some(shape) = found.get(id).and_then(|at| doc.shape(at)) else { continue };
        if let Some(layer) = locks.of(doc, page, shape)? {
            out.push(format!(
                "container {id} is on layer {} (\"{}\"), which is locked, so it was left as it was: no shape joined or left it and it was not resized",
                layer.ix,
                crate::text::shown(&layer.name, 64)
            ));
        }
    }
    Ok(out)
}

/// The first locked container a shape is in that is not among `leaving`.
fn locked_holder(page: &Page<'_>, id: u32, leaving: &HashSet<u32>) -> Result<Option<u32>, Problem> {
    for c in page.containers_of(id)? {
        if !leaving.contains(&c) && page.candidate(c)?.locked {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// The box of the shapes `members` made into a container's: their box grown by `margin` and by `band` on the heading's side.
fn box_round(page: &Page<'_>, members: &[u32], margin: f64, band: f64, side: Side) -> Result<Option<Rect>, Problem> {
    let mut all: Option<Rect> = None;
    for m in members {
        if let Some(b) = page.bounds(*m)? {
            all = Some(all.map_or(b, |a| a.union(b)));
        }
    }
    let Some(r) = all else { return Ok(None) };
    let mut need = Rect { x0: r.x0 - margin, y0: r.y0 - margin, x1: r.x1 + margin, y1: r.y1 + margin };
    match side {
        Side::Top => need.y1 += band,
        Side::Bottom => need.y0 -= band,
        Side::Left => need.x0 -= band,
        Side::Right => need.x1 += band,
    }
    Ok(Some(need))
}

/// Adds a container (`container.add`): round the shapes `around`, or an empty one at `centre`. It is put at the back of the page, so that it draws behind what it holds.
pub fn add(doc: &mut Document, page: u32, req: Add<'_>) -> Result<Added, Problem> {
    let style = find_style(if req.style.is_empty() { DEFAULT_STYLE } else { req.style }).ok_or_else(|| {
        let known: Vec<&str> = STYLES.iter().map(|s| s.id).collect();
        Problem::bad(format!("no container style `{}`: the gallery has {}", crate::error::shown(req.style), known.join(", ")))
    })?;
    super::text_edit::check_text(req.heading)?;
    require_page(doc, page)?;
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(req.above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let mut members = Unique::new();
    for m in req.around {
        members.push(*m);
    }
    let members: Vec<u32> = members.into_vec();
    let (k, round) = {
        let p = Page::read(doc, page)?;
        for m in &members {
            if !p.has(*m) {
                return Err(not_here(*m));
            }
            if let Some(c) = locked_holder(&p, *m, &HashSet::new())? {
                return Err(locked_words(*m, c, "put in another container"));
            }
            refuse_frozen_holder(&p, *m, "put in another container")?;
        }
        let band = BAND_INCHES * p.k;
        let margin = DEFAULT_MARGIN_INCHES * p.k;
        let round = if members.is_empty() { None } else { box_round(&p, &members, margin, band, Side::Top)? };
        if !members.is_empty() && round.is_none() {
            return Err(Problem::failed("the shapes have no box to put a container round"));
        }
        (p.k, round)
    };
    let band = BAND_INCHES * k;
    let margin = DEFAULT_MARGIN_INCHES * k;
    let (centre, size) = match round {
        Some(r) => (r.centre(), (r.width().max(1.0e-3), r.height().max(1.0e-3))),
        None => {
            let (cx, cy) = (checked("x", req.centre.0)?, checked("y", req.centre.1)?);
            let (w, h) = (checked("width", req.size.0)?, checked("height", req.size.1)?);
            if w <= 0.0 || h <= 0.0 {
                return Err(Problem::bad("`width` and `height` must be more than 0"));
            }
            ((cx, cy), (w, h))
        }
    };
    let made = Made { id, centre, size, style, side: Side::Top, band, margin, resize: Resize::Expand.number(), heading: req.heading };
    let shape = container_shape(&made);
    // At the back, behind what it holds (and behind everything else: a container is a background frame).
    page_mut(doc, page)
        .ok_or_else(|| Problem::failed("the page went away"))?
        .shapes
        .insert(0, Arc::new(shape))
        .map_err(|_| Problem::failed("the page's list of shapes is too long"))?;
    let (inside, graph, passed) = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        // The shapes leave the containers they were in, a container's list once, and join the new one.
        let mut by_old: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut at: HashMap<u32, usize> = HashMap::new();
        for m in &members {
            for c in graph.containers_of(&p, *m)? {
                let place = *at.entry(c).or_insert_with(|| {
                    by_old.push((c, Vec::new()));
                    by_old.len() - 1
                });
                if let Some((_, list)) = by_old.get_mut(place) {
                    list.push(*m);
                }
            }
        }
        for (c, list) in &by_old {
            graph.leave_many(&p, *c, list)?;
        }
        for m in &members {
            graph.join(&p, id, *m, None)?;
        }
        // The container it goes in is looked for with the shapes it holds now counted: read from the page alone, it held nothing yet, and a container it was put
        // round, under its middle, took it, so that each held the other.
        let inside = underlying(&p, &mut graph, id, None)?;
        if let Some(outer) = inside {
            graph.join(&p, outer, id, None)?;
        }
        (inside, graph, p.passed_over())
    };
    graph.write(doc, page)?;
    let fitted = match inside {
        Some(outer) => refit(doc, page, &[outer], false)?,
        None => Fitted::default(),
    }
    .passed(passed);
    Ok(Added { id, members, style: style.id, inside, fitted: fitted.resized, locked: fitted.locked })
}

/// Checks that each of `ids` is a container of the page, and returns the page's read.
fn require_containers<'a>(doc: &'a Document, page: u32, ids: &[u32]) -> Result<Page<'a>, Problem> {
    let p = Page::read(doc, page)?;
    for id in ids {
        if !p.has(*id) {
            return Err(not_here(*id));
        }
        if !p.is_container(*id) {
            return Err(not_a_container(*id));
        }
    }
    Ok(p)
}

/// Fit to Contents (`container.fit`): each container is made the box that just holds its members.
pub fn fit(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Fitted, Problem> {
    require_containers(doc, page, ids)?;
    refit(doc, page, ids, true)
}

/// The places in the page's list of the containers `ids`.
fn places(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<usize>, Problem> {
    let p = require_containers(doc, page, ids)?;
    Ok(ids.iter().filter_map(|id| p.at(*id)).collect())
}

/// Automatic Resize (`container.auto_resize`): sets how the containers follow their members, and brings them to it now.
pub fn set_resize(doc: &mut Document, page: u32, ids: &[u32], mode: Resize) -> Result<Fitted, Problem> {
    let at = places(doc, page, ids)?;
    let before = doc.clone();
    for a in at {
        put_row(doc, page, a, RESIZE_ROW, number_cell(f64::from(mode.number())))?;
    }
    settle(doc, &before, page, ids, &[])?;
    refit(doc, page, ids, false)
}

/// Margins (`container.margin`): the room round the members, in drawing units. The containers are brought to it now by their setting.
pub fn set_margin(doc: &mut Document, page: u32, ids: &[u32], margin: f64) -> Result<Fitted, Problem> {
    let margin = checked("margin", margin)?;
    if margin < 0.0 {
        return Err(Problem::bad("`margin` must not be negative"));
    }
    let at = places(doc, page, ids)?;
    let before = doc.clone();
    for a in at {
        put_row(doc, page, a, MARGIN_ROW, length_cell(margin))?;
    }
    settle(doc, &before, page, ids, &[])?;
    refit(doc, page, ids, false)
}

/// Lock Container (`container.lock`): `locked` or, when `None`, the other way from the first container.
pub fn lock(doc: &mut Document, page: u32, ids: &[u32], locked: Option<bool>) -> Result<bool, Problem> {
    let (at, new) = {
        let p = require_containers(doc, page, ids)?;
        let first = match ids.first() {
            Some(id) => Settings::read(&p, *id)?.locked,
            None => false,
        };
        (ids.iter().filter_map(|id| p.at(*id)).collect::<Vec<usize>>(), locked.unwrap_or(!first))
    };
    for a in at {
        put_row(doc, page, a, LOCK_ROW, number_cell(f64::from(u8::from(new))))?;
    }
    Ok(new)
}

/// Puts the outline of `look` in place of the container's `Geometry` sections, where the first of them was.
fn put_geometry(sheet: &mut diacraft_doc::Sheet, geometry: Vec<diacraft_doc::Section>) {
    let place = sheet.sections.iter().position(|s| s.name.as_str() == "Geometry").unwrap_or(sheet.sections.len());
    sheet.sections.retain(|s| s.name.as_str() != "Geometry");
    let place = place.min(sheet.sections.len());
    for (n, section) in geometry.into_iter().enumerate() {
        sheet.sections.insert(place + n, Arc::new(section));
    }
}

/// Makes the outline of the container at `at` again for its size (the strip's divider and the brackets are shares of the box worked out for a size): its `Geometry` and
/// nothing else, so that the look and the settings the person gave stay (`look` says why the outline is shares).
pub(super) fn regeometry(
    doc: &mut Document,
    page: u32,
    at: usize,
    style: &StyleSpec,
    side: Side,
    size: (f64, f64),
    band: f64,
) -> Result<(), Problem> {
    let index = u32::try_from(at).map_err(|_| Problem::failed("a shape's place is out of range"))?;
    let new = look(style, side, size, band);
    let shape = shape_mut(doc, page, &[index]).ok_or_else(|| Problem::failed("a shape went away"))?;
    put_geometry(Arc::make_mut(&mut shape.sheet), new.geometry);
    Ok(())
}

/// Puts the look of `style` with the heading on `side` on the container at `at`, keeping its size, its text, its ties and its settings.
fn restyle(doc: &mut Document, page: u32, at: usize, style: &StyleSpec, side: Side, size: (f64, f64), band: f64) -> Result<(), Problem> {
    let index = u32::try_from(at).map_err(|_| Problem::failed("a shape's place is out of range"))?;
    let new = look(style, side, size, band);
    {
        let shape = shape_mut(doc, page, &[index]).ok_or_else(|| Problem::failed("a shape went away"))?;
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, cell) in new.cells {
            sheet.cells.insert(diacraft_doc::CellName::new(name), cell);
        }
        put_geometry(sheet, new.geometry);
        // The heading's alignment is the style's; the other cells of its paragraph and characters are the person's.
        for section in new.text_sections.into_iter().filter(|s| s.name.as_str() == "Paragraph") {
            if let Some(row) = section.rows.first() {
                for (name, cell) in row.cells.iter() {
                    let addr = diacraft_doc::CellAddr::in_row("Paragraph", None, diacraft_doc::RowKey::Ix(0), name.as_str());
                    sheet.set_cell(&addr, cell.clone()).map_err(|e| Problem::failed(e.to_string()))?;
                }
            }
        }
    }
    put_row(doc, page, at, STYLE_ROW, text_cell(style.id))?;
    put_row(doc, page, at, HEADING_ROW, number_cell(f64::from(side.number())))?;
    Ok(())
}

/// What a style or heading change needs of one container: where it is in the page's list, its size, the strip's thickness and its settings.
struct Restyle {
    at: usize,
    size: (f64, f64),
    band: f64,
    settings: Settings,
}

/// What a style or heading change needs of each container: where it is in the page's list, its size and the strip's thickness.
fn restyling(doc: &Document, page: u32, ids: &[u32], need_style: bool) -> Result<Vec<Restyle>, Problem> {
    let p = require_containers(doc, page, ids)?;
    let mut out = Vec::new();
    for id in ids {
        let settings = Settings::read(&p, *id)?;
        if need_style && settings.style.is_none() {
            return Err(Problem::failed(format!(
                "shape {id} was not made from the container gallery, so its heading is part of its own drawing: choose a container style for it first (`container.style`)"
            )));
        }
        let (Some(at), Some(b), Some(sheet)) = (p.at(*id), p.bounds(*id)?, p.sheet(*id)) else { continue };
        if p.reader.pose(&sheet).is_turned_or_flipped() {
            return Err(Problem::failed(format!(
                "shape {id} is turned or flipped: a container's style is changed only when it is square to the page"
            )));
        }
        out.push(Restyle { at, size: (b.width(), b.height()), band: BAND_INCHES * p.k, settings });
    }
    Ok(out)
}

/// A container style (`container.style`): the look of one of the gallery's styles, on the containers, keeping their size, heading text and members.
pub fn set_style(doc: &mut Document, page: u32, ids: &[u32], style_id: &str) -> Result<usize, Problem> {
    let style = find_style(style_id).ok_or_else(|| Problem::bad(format!("no container style `{}`", crate::error::shown(style_id))))?;
    let plan = restyling(doc, page, ids, false)?;
    let before = doc.clone();
    for r in &plan {
        restyle(doc, page, r.at, style, r.settings.heading, r.size, r.band)?;
    }
    settle(doc, &before, page, ids, &[])?;
    Ok(plan.len())
}

/// Heading Style (`container.heading`): the side the heading is on, or the next one round (`None`, Rotate Heading). The containers are brought to their setting.
pub fn set_heading(doc: &mut Document, page: u32, ids: &[u32], side: Option<Side>) -> Result<Fitted, Problem> {
    let plan = restyling(doc, page, ids, true)?;
    let before = doc.clone();
    for r in &plan {
        let style = r.settings.style.as_deref().and_then(find_style).ok_or_else(|| Problem::failed("a container's style went away"))?;
        let new = side.unwrap_or_else(|| r.settings.heading.next());
        restyle(doc, page, r.at, style, new, r.size, r.band)?;
    }
    settle(doc, &before, page, ids, &[])?;
    refit(doc, page, ids, false)
}

/// Disband Container (`container.disband`): the containers go and the shapes they held stay, as members of the container that held the container, if one did and it is not on a
/// locked layer (one that is is passed over, and noted). A locked container is not disbanded, nor one that holds a shape on a locked layer, whose ties would
/// be rewritten. → how many containers went, and the containers on a locked layer passed over.
pub fn disband(doc: &mut Document, page: u32, ids: &[u32]) -> Result<(usize, Vec<u32>), Problem> {
    let mut named = Unique::new();
    for id in ids {
        named.push(*id);
    }
    let gone: Vec<u32> = named.into_vec();
    let gone_set: HashSet<u32> = gone.iter().copied().collect();
    let (graph, passed) = {
        let p = require_containers(doc, page, &gone)?;
        let mut graph = Graph::new();
        for id in &gone {
            if Settings::read(&p, *id)?.locked {
                return Err(Problem::failed(format!("container {id} is locked, so it cannot be disbanded: unlock it first")));
            }
            // A member on a locked layer would have its ties to the container rewritten.
            for m in p.members_of(*id)? {
                if let Some(layer) = p.locked_layer(m)? {
                    return Err(Problem::bad(format!(
                        "shape {m} is on layer {} (\"{}\"), which is locked, and it is a member of container {id}: a disband would rewrite its ties to the container, and a \
                         shape on a locked layer is not edited; unlock the layer first",
                        layer.ix,
                        crate::text::shown(&layer.name, 64)
                    )));
                }
            }
        }
        for id in &gone {
            let above = graph.containers_of(&p, *id)?.into_iter().find(|c| !gone_set.contains(c));
            let members = p.members_of(*id)?;
            graph.leave_many(&p, *id, &members)?;
            if let Some(a) = above {
                // A container on a locked layer takes no member: the shapes are left in none.
                if p.on_locked_layer(a)? {
                    p.pass_over(a);
                } else {
                    for m in &members {
                        graph.join(&p, a, *m, None)?;
                    }
                }
            }
            // The frame that goes leaves the containers it was in, a container on a locked layer too: no tie names a shape that is gone.
            for c in graph.containers_of(&p, *id)? {
                graph.leave(&p, c, *id)?;
            }
        }
        (graph, p.passed_over())
    };
    graph.write(doc, page)?;
    // Only the frames are taken out: what they held stays, and a connector glued to a frame is left with that end free.
    Ok((delete_shapes(doc, page, &gone)?, passed))
}

/// Select Contents (`container.select_contents`): the shapes the containers hold, each once (a query).
pub fn select_contents(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    let p = require_containers(doc, page, ids)?;
    let mut out = Unique::new();
    for id in ids {
        for m in p.members_of(*id)? {
            out.push(m);
        }
    }
    Ok(out.into_vec())
}

/// How a shape was put in a container by [`add_member`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Joined {
    pub shape: u32,
    pub container: u32,
}

/// Add to Underlying Container (`container.add_member`): each shape joins the container beneath it, or `container` when one is named. A shape that is in another container
/// leaves it. → the shapes that joined (one already in the container is left out), and the containers to fit.
pub fn add_member(doc: &mut Document, page: u32, ids: &[u32], container: Option<u32>) -> Result<(Vec<Joined>, Fitted), Problem> {
    let mut joined: Vec<Joined> = Vec::new();
    let (graph, passed) = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        if let Some(c) = container {
            if !p.has(c) {
                return Err(not_here(c));
            }
            if !p.takes_members(c)? {
                return Err(not_a_container(c));
            }
        }
        for id in ids {
            if !p.has(*id) {
                return Err(not_here(*id));
            }
            let target = match container {
                Some(c) => c,
                None => match underlying(&p, &mut graph, *id, None)? {
                    Some(c) => c,
                    // The one under it may be on a locked layer, which takes no member: the refusal says so.
                    None => return Err(none_under(&p, *id)?),
                },
            };
            // What the shape holds as the command has the ties now, the shapes it took a moment ago included.
            if graph.would_loop(&p, target, *id)? {
                return Err(membership::holds_words(*id, target));
            }
            if p.candidate(target)?.locked {
                return Err(Problem::failed(format!("container {target} is locked, so it takes no new member: unlock it first")));
            }
            let now = graph.containers_of(&p, *id)?;
            if now == [target] {
                continue;
            }
            for c in &now {
                if p.candidate(*c)?.locked {
                    return Err(locked_words(*id, *c, "moved to another container"));
                }
                if let Some(layer) = p.locked_layer(*c)? {
                    return Err(frozen_words(*id, *c, &layer, "moved to another container"));
                }
                graph.leave(&p, *c, *id)?;
            }
            graph.join(&p, target, *id, None)?;
            joined.push(Joined { shape: *id, container: target });
        }
        // A container on a locked layer the shapes were over, passed over for the one under it, is noted.
        (graph, p.passed_over())
    };
    graph.write(doc, page)?;
    let mut to_fit = Unique::new();
    for j in &joined {
        to_fit.push(j.container);
    }
    let to_fit = to_fit.into_vec();
    let fitted = if to_fit.is_empty() { Fitted::default() } else { refit(doc, page, &to_fit, false)? };
    Ok((joined, fitted.passed(passed)))
}

/// The refusal of Add to Underlying Container for the shape `id`, under which no container lies that takes members: the words name the container on a locked layer that was passed
/// over, when one was.
fn none_under(page: &Page<'_>, id: u32) -> Result<Problem, Problem> {
    let frozen = page.passed_over().first().copied();
    let layer = match frozen {
        Some(c) => page.locked_layer(c)?.map(|l| (c, l)),
        None => None,
    };
    Ok(match layer {
        Some((c, layer)) => Problem::failed(format!(
            "no container that takes members lies under shape {id}: container {c} is on layer {} (\"{}\"), which is locked, and takes no member; unlock the layer first",
            layer.ix,
            crate::text::shown(&layer.name, 64)
        )),
        None => Problem::failed(format!("no container lies under shape {id}")),
    })
}

/// Attach a shape to a container's edge (`container.attach_edge`): `edge` is `left`, `right`, `top` or `bottom` (0 to 3). The shape becomes a member that sits on that edge,
/// its middle on the line, and stays there as the container is moved or resized (codes 7 to 10). → the shapes moved.
pub fn attach_edge(doc: &mut Document, page: u32, id: u32, container: u32, edge: usize) -> Result<Fitted, Problem> {
    if edge > 3 {
        return Err(Problem::bad("`edge` is left, right, top or bottom"));
    }
    let graph = {
        let p = Page::read(doc, page)?;
        if !p.has(id) {
            return Err(not_here(id));
        }
        if !p.has(container) || !p.takes_members(container)? {
            return Err(not_a_container(container));
        }
        let mut graph = Graph::new();
        if graph.would_loop(&p, container, id)? {
            return Err(Problem::bad(format!("shape {id} holds container {container}: a container is not attached to what it holds")));
        }
        if p.candidate(container)?.locked {
            return Err(Problem::failed(format!("container {container} is locked, so it takes no new member: unlock it first")));
        }
        for c in graph.containers_of(&p, id)?.into_iter().filter(|c| *c != container) {
            if p.candidate(c)?.locked {
                return Err(locked_words(id, c, "moved to another container"));
            }
            if let Some(layer) = p.locked_layer(c)? {
                return Err(frozen_words(id, c, &layer, "moved to another container"));
            }
            graph.leave(&p, c, id)?;
        }
        graph.join(&p, container, id, Some(edge))?;
        graph
    };
    graph.write(doc, page)?;
    // Put it on the edge now: the shapes on the container's edges go to the edges the container has.
    let moves = {
        let p = Page::read(doc, page)?;
        let to = p.bounds(container)?.ok_or_else(|| Problem::failed("the container has no box"))?;
        fit::edge_moves(&p, container, &to)?
    };
    let mut out = Fitted::default();
    if !moves.is_empty() {
        super::shapes::move_each(doc, page, &moves)?;
        out.moved = moves.iter().map(|(m, _)| *m).collect();
    }
    Ok(out)
}

/// What a container says of itself, for the Container Format tab and the `container.info` query.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub id: u32,
    pub style: Option<String>,
    pub heading: Side,
    pub resize: Resize,
    /// In paper inches.
    pub margin: f64,
    pub locked: bool,
    pub members: Vec<u32>,
    /// The containers that hold it.
    pub inside: Vec<u32>,
}

/// For each shape that is no container, the containers that hold it.
pub type HeldBy = Vec<(u32, Vec<u32>)>;

/// What each of `ids` is: a container's settings and members, and for a shape that is no container the containers that hold it (what the window lights when a member is
/// selected).
pub fn describe(doc: &Document, page: u32, ids: &[u32]) -> Result<(Vec<Info>, HeldBy), Problem> {
    let p = Page::read(doc, page)?;
    let mut out = Vec::new();
    let mut held_by: HeldBy = Vec::new();
    for id in ids {
        if p.has(*id) && !p.is_container(*id) {
            let holders = p.containers_of(*id)?;
            if !holders.is_empty() {
                held_by.push((*id, holders));
            }
            continue;
        }
        if !p.has(*id) {
            continue;
        }
        let s = Settings::read(&p, *id)?;
        out.push(Info {
            id: *id,
            style: s.style.clone(),
            heading: s.heading,
            resize: s.resize,
            margin: if p.k > 0.0 { s.margin / p.k } else { s.margin },
            locked: s.locked,
            members: p.members_of(*id)?,
            inside: p.containers_of(*id)?,
        });
    }
    Ok((out, held_by))
}

/// What a move, a drop or a paste of `ids` did to membership, and the fitting that followed. A page with no container costs one
/// read of its list. A shape of a locked container that was moved out of it is refused in words. → the containers fitted, and those on a locked layer passed over
/// Which the command notes.
pub(crate) fn arrive(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Fitted, Problem> {
    if ids.is_empty() {
        return Ok(Fitted::default());
    }
    let (graph, affected, passed) = {
        let p = Page::read(doc, page)?;
        if p.containers.is_empty() {
            return Ok(Fitted::default());
        }
        let mut graph = Graph::new();
        // A member of a locked container stays in it, and where it is moved to must still be in it.
        let moved: HashSet<u32> = ids.iter().copied().collect();
        for id in ids {
            if !p.has(*id) {
                continue;
            }
            for c in p.containers_of(*id)? {
                if moved.contains(&c) {
                    continue;
                }
                let cand = p.candidate(c)?;
                if !cand.locked {
                    continue;
                }
                let (Some(me), Some(b)) = (p.bounds(*id)?, cand.rect) else { continue };
                if !b.contains(me.centre()) {
                    return Err(locked_words(*id, c, "moved out of it"));
                }
            }
        }
        let affected = arrive_in(&p, &mut graph, ids)?;
        (graph, affected, p.passed_over())
    };
    graph.write(doc, page)?;
    let fitted = if affected.is_empty() { Fitted::default().passed(passed) } else { refit(doc, page, &affected, false)?.passed(passed) };
    // A lane or a phase that was moved is put in its place in its list by where it lies, and the list is laid out again.
    lists::after_move(doc, page, ids)?;
    Ok(fitted)
}

/// The containers `ids` are in, for a caller that fits them after a change that took shapes out (a delete): containers that are not among `ids` themselves.
pub(crate) fn holding(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    let p = Page::read(doc, page)?;
    if p.containers.is_empty() {
        return Ok(Vec::new());
    }
    let named: HashSet<u32> = ids.iter().copied().collect();
    let mut out = Unique::new();
    for id in ids {
        for c in p.containers_of(*id)? {
            if !named.contains(&c) {
                out.push(c);
            }
        }
    }
    // The lists that held a lane or a phase that goes are laid out again after it.
    for list in lists::holding_lists(&p, ids)? {
        if !named.contains(&list) {
            out.push(list);
        }
    }
    Ok(out.into_vec())
}

/// Fits containers after a change that took shapes out of them (a delete): each follows its own Automatic Resize setting. → what the fit did (a container on a locked layer is
/// passed over).
pub(crate) fn fit_after(doc: &mut Document, page: u32, containers: &[u32]) -> Result<Fitted, Problem> {
    if containers.is_empty() {
        return Ok(Fitted::default());
    }
    let fitted = refit(doc, page, containers, false)?;
    lists::after_delete(doc, page, containers)?;
    Ok(fitted)
}

/// A container's outline to make again for the size it was given.
struct Outline {
    at: usize,
    style: &'static StyleSpec,
    side: Side,
    size: (f64, f64),
    band: f64,
}

/// Fits the containers that hold shapes that `ids` changed in size (a resize): after a command that did not move them across a container's edge. The shapes on the edges of a
/// container that was resized by hand go to the edges. → what the fit did: the containers resized, and those on a locked layer passed over.
pub(crate) fn changed(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Fitted, Problem> {
    let (holders, own) = {
        let p = Page::read(doc, page)?;
        if p.containers.is_empty() {
            return Ok(Fitted::default());
        }
        let mut holders = Unique::new();
        let mut own: Vec<u32> = Vec::new();
        for id in ids {
            for c in p.containers_of(*id)? {
                holders.push(c);
            }
            if p.is_container(*id) {
                own.push(*id);
            }
        }
        (holders.into_vec(), own)
    };
    let mut fitted = Fitted::default();
    if !holders.is_empty() {
        fitted = refit(doc, page, &holders, false)?;
    }
    let mut moves: Vec<(u32, (f64, f64))> = Vec::new();
    let mut again: Vec<Outline> = Vec::new();
    {
        let p = Page::read(doc, page)?;
        for id in own {
            let Some(to) = p.bounds(id)? else { continue };
            moves.extend(fit::edge_moves(&p, id, &to)?);
            // The outline of a container of the gallery is made again for the size it was given.
            let settings = Settings::read(&p, id)?;
            if let (Some(style), Some(at)) = (settings.style.as_deref().and_then(find_style), p.at(id)) {
                again.push(Outline { at, style, side: settings.heading, size: (to.width(), to.height()), band: settings.band });
            }
        }
    }
    if !again.is_empty() {
        let before = doc.clone();
        for o in &again {
            regeometry(doc, page, o.at, o.style, o.side, o.size, o.band)?;
        }
        settle(doc, &before, page, ids, &[])?;
    }
    if !moves.is_empty() {
        super::shapes::move_each(doc, page, &moves)?;
    }
    // A lane resized makes the lanes after it make room, a phase takes from the next one, the length of the lanes follows the frame.
    lists::after_resize(doc, page, ids)?;
    Ok(fitted)
}

/// Whether the shape at `at` (`None` for one that is not on the page) holds members: its own or its master's `Relationships` has a call of code 1. A shape that is not at the
/// top of the page's list does not.
fn holds_members(reader: &super::Reader<'_>, at: Option<ShapeRef>) -> Result<bool, Problem> {
    let Some(at) = at else { return Ok(false) };
    if at.path.len() != 1 {
        return Ok(false);
    }
    let found = reader.resolver().resolve(&SheetRef::Shape(at), &diacraft_doc::CellAddr::direct("Relationships"));
    Ok(ties::Ties::of_cell(found.and_then(|f| f.formula.cell)).holds_parts())
}

/// `ids` and what the containers among them hold, nested ones too: what a copy or a delete of a container takes.
pub(crate) fn with_contents(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    require_page(doc, page)?;
    let work = Work::new();
    let mut any = false;
    {
        let reader = super::Reader::new(doc);
        // Where each shape named is, in one walk of the page, where a search for each walked the page from its top for each.
        let named = super::scan::Named::of(doc, page, ids.iter().copied())?;
        for id in ids {
            work.charge(4)?;
            if holds_members(&reader, named.get(*id).cloned())? {
                any = true;
                break;
            }
        }
    }
    if !any {
        return Ok(ids.to_vec());
    }
    let p = Page::read(doc, page)?;
    let mut out = Unique::new();
    for id in ids {
        out.push(*id);
    }
    let have: Vec<u32> = ids.iter().copied().filter(|id| p.has(*id)).collect();
    for m in p.contents(&have)? {
        out.push(m);
    }
    Ok(out.into_vec())
}

/// Refuses a delete of a member of a locked container (the container itself, and shapes deleted with it, are not refused).
pub(crate) fn check_delete(doc: &Document, page: u32, ids: &[u32]) -> Result<(), Problem> {
    let p = Page::read(doc, page)?;
    if p.containers.is_empty() {
        return Ok(());
    }
    let going: HashSet<u32> = ids.iter().copied().collect();
    for id in ids {
        if let Some(c) = locked_holder(&p, *id, &going)? {
            return Err(locked_words(*id, c, "deleted"));
        }
    }
    Ok(())
}

/// Rewrites the `Relationships` of the shapes at `hits` with the ties given.
fn write_back(doc: &mut Document, page: u32, hits: Vec<(usize, ties::Ties)>) -> Result<(), Problem> {
    for (at, mut ties) in hits {
        ties.drop_emptied(&[ties::MEMBERS]);
        let source = ties.print();
        let index = u32::try_from(at).map_err(|_| Problem::failed("a shape's place is out of range"))?;
        let shape = shape_mut(doc, page, &[index]).ok_or_else(|| Problem::failed("a shape went away"))?;
        let sheet = Arc::make_mut(&mut shape.sheet);
        if source == "SUM()" {
            sheet.cells.remove("Relationships");
        } else {
            let old = sheet.cell("Relationships").cloned();
            let mut cell = diacraft_doc::Cell::with_formula(&source, Some(diacraft_doc::Value::from_text("0")));
            cell.unit = old.and_then(|c| c.unit);
            sheet.cells.insert(diacraft_doc::CellName::new("Relationships"), cell);
        }
    }
    Ok(())
}

/// After shapes were taken out: every tie that names one of them goes, on the shapes that stay, so that a new shape never takes a deleted one's place in a container. One walk
/// of the page's list.
pub(crate) fn release(doc: &mut Document, page: u32, gone: &BTreeSet<u32>) -> Result<(), Problem> {
    if gone.is_empty() {
        return Ok(());
    }
    let gone_set: HashSet<u32> = gone.iter().copied().collect();
    let hits: Vec<(usize, ties::Ties)> = {
        let Some(p) = doc.page(page) else { return Ok(()) };
        let work = Work::new();
        let mut hits = Vec::new();
        for (at, shape) in p.shapes.iter().enumerate() {
            work.charge(1)?;
            let Some(cell) = shape.sheet.cell("Relationships") else { continue };
            let diacraft_doc::FormulaSlot::Local(formula) = &cell.formula else { continue };
            work.charge(formula.source.len() / 64)?;
            if !super::remap::sheet_refs(&formula.source).iter().any(|(_, _, id)| gone.contains(id)) {
                continue;
            }
            // A formula that is not a list of ties is left as it is.
            let mut ties = ties::Ties::of_cell(Some(cell));
            if !ties.is_rewritable() {
                continue;
            }
            if ties.forget_many(&gone_set) {
                hits.push((at, ties));
            }
        }
        hits
    };
    write_back(doc, page, hits)
}

/// After a paste: the ties of the copies name only copies. A copy's ties were renumbered where they named a shape that was copied too (`remap`); a tie to a shape that was not
/// copied is taken out. `copies` is the `ID`s of the copies. The copies then join the container they were put inside (`arrive`). **A loop among the copies is broken**
/// (the copies of a tree are a tree, and a loop a library caller wrote by hand is not copied as one), and the notes say so; the copies are looked at again only
/// when some of them tie others. → the notes.
pub(crate) fn repair_copies(doc: &mut Document, page: u32, copies: &HashSet<u32>) -> Result<Vec<String>, Problem> {
    let (hits, linked) = {
        let Some(p) = doc.page(page) else { return Ok(Vec::new()) };
        let work = Work::new();
        let mut hits = Vec::new();
        let mut linked = false;
        for (at, shape) in p.shapes.iter().enumerate() {
            work.charge(1)?;
            if !copies.contains(&shape.id) {
                continue;
            }
            let Some(cell) = shape.sheet.cell("Relationships") else { continue };
            work.charge(cell.formula.text().map_or(0, str::len) / 64)?;
            let mut ties = ties::Ties::of_cell(Some(cell));
            if !ties.is_rewritable() {
                continue;
            }
            let all = ties.all_ids();
            let outside: HashSet<u32> = all.iter().copied().filter(|id| !copies.contains(id)).collect();
            // A copy that names a copy.
            linked |= all.len() > outside.len();
            if !outside.is_empty() && ties.forget_many(&outside) {
                hits.push((at, ties));
            }
        }
        (hits, linked)
    };
    write_back(doc, page, hits)?;
    if !linked {
        return Ok(Vec::new());
    }
    let broken = cycles::untangle_page(doc, page, Some(copies))?;
    Ok(if broken.is_empty() { Vec::new() } else { vec![cycles::loop_words(page, &broken)] })
}
