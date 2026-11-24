//! Swimlanes: `swimlane.create`, `swimlane.insert`, `swimlane.insert_phase`, `swimlane.move`, `swimlane.resize`, `swimlane.orientation`, `swimlane.rotate_label` and the query
//! `swimlane.info`: the Cross-Functional Flowchart's lanes and phases. The rules are in `draw/container/swimlane.rs`, `draw/container/lists.rs` and `docs/editing-commands.md` ("Swimlanes
//! "). Lengths are paper inches, as every command's are.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids, units_per_inch};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::container::swimlane::{self, Create, DEFAULT_THICKNESS_INCHES, Inserted, MAX_NEW, Place, Sizes};
use crate::draw::container::{Dir, MIN_INCHES};
use crate::draw::{self, Problem};
use crate::error::{Result, bad, failed};
use crate::stencil::swimlane::{LANE_LABEL, PHASE_LABEL};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "swimlane.info",
            "Swimlane Properties",
            ["Cross-Functional Flowchart"],
            None,
            "ids: array of whole numbers, optional, the shapes to tell the part of (each that is the frame, a list, a lane or a phase of a structure is listed in `selected` with its role); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes, else the first page that is not a background)",
            has_document,
            info
        ),
        cmd!(
            "swimlane.create",
            "Add Swimlanes",
            ["Cross-Functional Flowchart"],
            None,
            "orientation: text, optional, horizontal (lanes one under the next, the default) or vertical (side by side); lanes: whole number, optional, 1 to 256 (default 3); \
             phases: whole number, optional, 0 to 256 (default 0, no list of phases); x: number, optional, the left edge of the first lane in inches from the page's left edge (default 0.5); \
             y: number, optional, the top edge of the first lane in inches from the page's bottom edge (default 0.5 inch below the page's top); length: number, optional, how long every lane is in inches \
             (default the page's width less 1 inch for horizontal lanes, its height less 1 for vertical ones); thickness: number, optional, how thick each lane is in inches (default 1.25); \
             label: text, optional, the lane label (default Function); phase_label: text, optional, the phase label (default Phase); page: whole number, optional, the id of the page \
             (default: the first page that is not a background). A frame that holds a list of lanes and, for phases, a list of phases is made at the back of the page, and it takes no shape that lies under it",
            has_document,
            create
        ),
        cmd!(
            "swimlane.insert",
            "Insert Swimlane",
            ["Cross-Functional Flowchart", "Insert", "Swimlane"],
            None,
            "lane: whole number, optional, a lane (the new lane goes before or after it) or another part of the structure (it goes at the end), and left out it is the page's first structure, at the end; \
             where: text, optional, before or after (default after); label: text, optional, the label (default Function); page: whole number, optional, the id of the page \
             (default: the page that has the shape named). The new lane is as thick as the lane next to it, and the lanes after it, with what they hold, make room",
            has_document,
            insert
        ),
        cmd!(
            "swimlane.insert_phase",
            "Insert Separator",
            ["Cross-Functional Flowchart", "Insert", "Separator"],
            None,
            "phase: whole number, optional, the phase to split (default the last of the structure that the shape named belongs to, or of the page's first); where: text, optional, before or after (default after): the new phase is the \
             first or the second part; at: number, optional, where to split, in inches from the page's left edge (horizontal lanes) or bottom edge (vertical lanes) (default the middle of the phase); \
             label: text, optional, the new phase's label (default Phase); page: whole number, optional, the id of the page. A structure with no phases is given a list of two. The length of the lanes is not changed",
            has_document,
            insert_phase
        ),
        cmd!(
            "swimlane.move",
            "Move Swimlane",
            ["Cross-Functional Flowchart", "Arrange"],
            None,
            "lane: whole number, a lane or a phase; to: whole number, its place in its list, from 0: it takes what it holds, and the others make room (dragging a lane by its header does the same); \
             page: whole number, optional, the id of the page (default: the page that has the shape)",
            has_document,
            move_lane
        ),
        cmd!(
            "swimlane.resize",
            "Resize Swimlane",
            ["Cross-Functional Flowchart", "Arrange"],
            None,
            "id: whole number, the shape: a lane, a phase, the list of lanes or the frame; size: number, optional, a lane's thickness or a phase's width in inches, at least 0.4 (a phase made wider takes the room \
             from the next one, and the length of the lanes does not change); length: number, optional, how long the lanes are in inches, all of them, and the last phase takes the difference; \
             page: whole number, optional, the id of the page (default: the page that has the shape)",
            has_document,
            resize
        ),
        cmd!(
            "swimlane.orientation",
            "Swimlane Orientation",
            ["Cross-Functional Flowchart", "Arrange"],
            None,
            "orientation: text, horizontal (lanes one under the next) or vertical (side by side); id: whole number, optional, any shape that is a part of the structure (default the page's first): it is turned over its \
             diagonal about its top left corner, the k-th lane staying the k-th, and what a lane holds staying in it; page: whole number, optional, the id of the page",
            has_document,
            orientation
        ),
        cmd!(
            "swimlane.rotate_label",
            "Rotate Lane Label",
            ["Cross-Functional Flowchart", "Design"],
            None,
            "ids: array of whole numbers, lanes (a frame or a list names all its lanes): the label's strip goes from the left of the lane to its top, or back, and the text turns with it and stays; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            rotate_label
        ),
    ]
}

/// The page the command works on: the one named, else the first page holding all `ids`, else the first page that is not a background.
fn page_of(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    match (named, ids.is_empty()) {
        (Some(page), _) => Ok(page),
        (None, false) => page_for(doc, None, ids, cmd),
        (None, true) => doc
            .pages
            .iter()
            .find(|pg| !pg.background)
            .or_else(|| doc.pages.iter().next())
            .map(|pg| pg.id)
            .ok_or_else(|| bad(cmd, "the document has no pages")),
    }
}

fn optional_id(p: &Value, key: &str, cmd: &str) -> Result<Option<u32>> {
    optional_whole(p, key, cmd)
}

fn place_of(p: &Value, cmd: &str) -> Result<Place> {
    match optional_text(p, "where", cmd)? {
        None | Some("after") => Ok(Place::After),
        Some("before") => Ok(Place::Before),
        Some(other) => Err(bad(cmd, format!("`where` is before or after, not `{}`", crate::error::shown(other)))),
    }
}

fn dir_of(word: &str, cmd: &str) -> Result<Dir> {
    match word {
        "horizontal" => Ok(Dir::Down),
        "vertical" => Ok(Dir::Across),
        other => Err(bad(cmd, format!("`orientation` is horizontal or vertical, not `{}`", crate::error::shown(other)))),
    }
}

fn dir_word(dir: Dir) -> &'static str {
    match dir {
        Dir::Down => "horizontal",
        Dir::Across => "vertical",
    }
}

/// A new part follows the page's theme and joins the active layers, and the page grows to take it in when its Auto Size is on, all in the command's own undo step.
fn settle_new(doc: &mut diacraft_doc::Document, page: u32, ids: &[u32]) -> std::result::Result<(), Problem> {
    for id in ids {
        crate::theme::follow_page_theme(doc, page, *id)?;
        draw::layers::join_active(doc, page, *id)?;
    }
    draw::fit_page(doc, page, ids).map(|_| ())
}

fn whole_in(p: &Value, key: &str, cmd: &str, default: usize, most: usize) -> Result<usize> {
    match optional_whole(p, key, cmd)? {
        None => Ok(default),
        Some(n) => {
            let n = usize::try_from(n).map_err(|_| bad(cmd, format!("`{key}` is too large")))?;
            if n > most {
                return Err(bad(cmd, format!("`{key}` must be at most {most}")));
            }
            Ok(n)
        }
    }
}

fn part_json(parts: &[swimlane::PartInfo], size_key: &str) -> Vec<Value> {
    parts.iter().map(|p| json!({ "id": p.id, size_key: p.size, "heading": p.heading.word(), "text": p.text, "rect": p.rect })).collect()
}

/// → `{page, structures: [{frame, lane_list, phase_list, orientation, length, rect, lanes: [{id, thickness, heading, text, rect}], phases: [{id, width, heading, text, rect}]}], selected: [{id, role, lane_list}]}`
/// (a query): the structures of the page, and for the shapes named what part each is (`frame`, `lanes`, `phases`, `lane` or `phase`) and which structure it is in.
fn info(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.info";
    let ids: Vec<u32> = match p.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        // A query: an empty list names no shape.
        Some(Value::Array(items)) if items.is_empty() => Vec::new(),
        Some(_) => required_ids(p, "ids", ID)?,
    };
    let page = page_of(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let found = swimlane::describe(doc, page).map_err(|e| e.into_engine(ID))?;
    let selected = swimlane::roles_of(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
    let structures: Vec<Value> = found
        .iter()
        .map(|st| {
            json!({
                "frame": st.frame,
                "lane_list": st.lane_list,
                "phase_list": st.phase_list,
                "orientation": dir_word(st.dir),
                "length": st.length,
                "rect": st.rect,
                "lanes": part_json(&st.lanes, "thickness"),
                "phases": part_json(&st.phases, "width"),
            })
        })
        .collect();
    let selected: Vec<Value> = selected.iter().map(|(id, role, list)| json!({ "id": id, "role": role, "lane_list": list })).collect();
    Ok(json!({ "page": page, "structures": structures, "selected": selected }))
}

/// Makes a structure → `{page, frame, lane_list, lanes, phase_list, phases}`.
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.create";
    let page = page_of(s, p, &[], ID)?;
    let dir = match optional_text(p, "orientation", ID)? {
        None => Dir::Down,
        Some(word) => dir_of(word, ID)?,
    };
    let lanes = whole_in(p, "lanes", ID, 3, MAX_NEW)?;
    let phases = whole_in(p, "phases", ID, 0, MAX_NEW)?;
    if lanes == 0 {
        return Err(bad(ID, format!("`lanes` must be from 1 to {MAX_NEW}")));
    }
    let (setup, k) = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        (draw::read_setup(doc, page).map_err(|e| e.into_engine(ID))?, units_per_inch(doc, page))
    };
    let x = optional_number(p, "x", ID)?.unwrap_or(0.5);
    let y = optional_number(p, "y", ID)?.unwrap_or(setup.height - 0.5);
    let room = match dir {
        Dir::Down => setup.width,
        Dir::Across => setup.height,
    };
    let length = optional_number(p, "length", ID)?.unwrap_or((room - 1.0).max(4.0));
    let thickness = optional_number(p, "thickness", ID)?.unwrap_or(DEFAULT_THICKNESS_INCHES);
    let lane_label = optional_text(p, "label", ID)?.unwrap_or(LANE_LABEL);
    let phase_label = optional_text(p, "phase_label", ID)?.unwrap_or(PHASE_LABEL);
    let above = s.id_floor(page);
    let made = s.edit("Add swimlanes", |doc| {
        let made = swimlane::create(
            doc,
            page,
            Create { dir, lanes, phases, left: x * k, top: y * k, length: length * k, thickness: thickness * k, lane_label, phase_label, above },
        )
        .map_err(|e| e.into_engine(ID))?;
        let mut all = vec![made.frame, made.lane_list];
        all.extend(&made.lanes);
        all.extend(made.phase_list);
        all.extend(&made.phases);
        settle_new(doc, page, &all).map_err(|e| e.into_engine(ID))?;
        Ok(made)
    })?;
    let highest = [made.frame, made.lane_list]
        .into_iter()
        .chain(made.lanes.iter().copied())
        .chain(made.phase_list)
        .chain(made.phases.iter().copied())
        .max()
        .unwrap_or(0);
    s.note_id(page, highest);
    Ok(json!({
        "page": page,
        "frame": made.frame,
        "lane_list": made.lane_list,
        "lanes": made.lanes,
        "phase_list": made.phase_list,
        "phases": made.phases,
        "orientation": dir_word(dir),
    }))
}

fn inserted_json(page: u32, done: &Inserted) -> Value {
    let mut out = json!({ "page": page, "id": done.id, "list": done.list, "index": done.index });
    if let (Some(map), Some(made)) = (out.as_object_mut(), done.made.as_ref()) {
        map.insert("phase_list".into(), json!(made.phase_list));
        map.insert("phases".into(), json!(made.phases));
    }
    out
}

/// Inserts a lane → `{page, id, list, index}`.
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.insert";
    let target = optional_id(p, "lane", ID)?;
    let ids: Vec<u32> = target.into_iter().collect();
    let page = page_of(s, p, &ids, ID)?;
    let place = place_of(p, ID)?;
    let label = optional_text(p, "label", ID)?.unwrap_or(LANE_LABEL);
    let above = s.id_floor(page);
    let done = s.edit("Insert swimlane", |doc| {
        let done = swimlane::insert_lane(doc, page, target, place, label, above).map_err(|e| e.into_engine(ID))?;
        settle_new(doc, page, &[done.id]).map_err(|e| e.into_engine(ID))?;
        Ok(done)
    })?;
    s.note_id(page, done.id);
    Ok(inserted_json(page, &done))
}

/// Inserts a separator → `{page, id, list, index}` (and `phase_list`, `phases` when the structure had no phases).
fn insert_phase(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.insert_phase";
    let target = optional_id(p, "phase", ID)?;
    let ids: Vec<u32> = target.into_iter().collect();
    let page = page_of(s, p, &ids, ID)?;
    let place = place_of(p, ID)?;
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    let at = optional_number(p, "at", ID)?.map(|a| a * k);
    let label = optional_text(p, "label", ID)?.unwrap_or(PHASE_LABEL);
    let above = s.id_floor(page);
    let done = s.edit("Insert separator", |doc| {
        let done = swimlane::insert_phase(doc, page, target, place, at, label, above).map_err(|e| e.into_engine(ID))?;
        let mut all = vec![done.id];
        if let Some(made) = &done.made {
            all.extend(made.phase_list);
            all.extend(&made.phases);
        }
        settle_new(doc, page, &all).map_err(|e| e.into_engine(ID))?;
        Ok((done, all))
    })?;
    s.note_id(page, done.1.iter().copied().max().unwrap_or(0));
    Ok(inserted_json(page, &done.0))
}

/// Moves a lane or a phase → `{page, id, index}`.
fn move_lane(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.move";
    let lane = required_u32(p, "lane", ID)?;
    let to = optional_whole(p, "to", ID)?.ok_or_else(|| bad(ID, "missing `to`"))?;
    let to = usize::try_from(to).map_err(|_| bad(ID, "`to` is too large"))?;
    let page = page_of(s, p, &[lane], ID)?;
    s.edit_if("Move swimlane", |doc| {
        let before = doc.clone();
        let index = swimlane::move_to(doc, page, lane, to).map_err(|e| e.into_engine(ID))?;
        let changed = *doc != before;
        Ok((json!({ "page": page, "id": lane, "index": index }), changed))
    })
}

/// Resizes → `{page, id, changed}`.
fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.resize";
    let id = required_u32(p, "id", ID)?;
    let page = page_of(s, p, &[id], ID)?;
    let (size, length) = (optional_number(p, "size", ID)?, optional_number(p, "length", ID)?);
    if size.is_none() && length.is_none() {
        return Err(bad(ID, "give `size` (a lane's thickness or a phase's width) or `length` (the lanes' length), in inches"));
    }
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    s.edit_if("Resize swimlane", |doc| {
        let changed =
            swimlane::resize(doc, page, id, Sizes { size: size.map(|v| v * k), length: length.map(|v| v * k) }).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "changed": changed, "least": MIN_INCHES }), changed))
    })
}

/// Turns the structure → `{page, orientation, changed}`; one that is the way asked for changes nothing and is no undo step.
fn orientation(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.orientation";
    let word = optional_text(p, "orientation", ID)?.ok_or_else(|| bad(ID, "missing `orientation`: horizontal or vertical"))?;
    let want = dir_of(word, ID)?;
    let named = optional_id(p, "id", ID)?;
    let ids: Vec<u32> = named.into_iter().collect();
    let page = page_of(s, p, &ids, ID)?;
    let id = match named {
        Some(id) => id,
        None => {
            let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
            let found = swimlane::describe(doc, page).map_err(|e| e.into_engine(ID))?;
            found.first().map(|st| st.lane_list).ok_or_else(|| bad(ID, "the page has no swimlanes: `swimlane.create` makes a structure of lanes"))?
        }
    };
    s.edit_if("Swimlane orientation", |doc| {
        let changed = swimlane::set_orientation(doc, page, id, want).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "orientation": word, "changed": changed }), changed))
    })
}

/// Rotates the labels → `{page, rotated}`.
fn rotate_label(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swimlane.rotate_label";
    let ids = required_ids(p, "ids", ID)?;
    let page = page_of(s, p, &ids, ID)?;
    s.edit("Rotate lane label", |doc| {
        let rotated = swimlane::rotate_label(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
        Ok(json!({ "page": page, "rotated": rotated }))
    })
}

/// Whether the paper-inch point (`x`, `y`) is in the box `r` (left, bottom, right, top).
fn within(r: &Option<[f64; 4]>, x: f64, y: f64) -> bool {
    r.is_some_and(|[x0, y0, x1, y1]| x >= x0 && x <= x1 && y >= y0 && y <= y1)
}

/// A drop of the Swimlane or the Separator of the Cross-Functional Flowchart stencil at the paper-inch point (`x`, `y`) of page `page` (`shape.drop`): on a page that has a
/// structure of lanes a Swimlane is **inserted** in the list of lanes under the point, before the lane it is dropped on when the point is in its first half and after it when it is in its second
/// (at the end when no lane is under it), and a Separator splits the phase it is dropped across, at the point; **on a page with no lanes** either is dropped as the shape it is drawn as
/// (`None`: the caller drops the master). → `{id, master: null, name, page, width, height}` as a drop answers.
pub(super) fn drop_part(s: &mut Session, master: &str, page: u32, x: f64, y: f64) -> Result<Option<Value>> {
    const ID: &str = "shape.drop";
    let found = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        swimlane::describe(doc, page).map_err(|e| e.into_engine(ID))?
    };
    // The structure the point is over, else the page's first.
    let at = found.iter().position(|st| within(&st.rect, x, y)).unwrap_or(0);
    let Some(here) = found.get(at) else { return Ok(None) };
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    let above = s.id_floor(page);
    if master == crate::stencil::xfunc::SWIMLANE {
        let under = here.lanes.iter().find(|l| within(&l.rect, x, y));
        let (target, place) = match under {
            Some(lane) => {
                let [x0, y0, x1, y1] = lane.rect.unwrap_or([0.0; 4]);
                let second = match here.dir {
                    Dir::Down => y < (y0 + y1) / 2.0,
                    Dir::Across => x > (x0 + x1) / 2.0,
                };
                (Some(lane.id), if second { Place::After } else { Place::Before })
            }
            None => (Some(here.lane_list), Place::After),
        };
        let done = s.edit("Drop swimlane", |doc| {
            let done = swimlane::insert_lane(doc, page, target, place, LANE_LABEL, above).map_err(|e| e.into_engine(ID))?;
            settle_new(doc, page, &[done.id]).map_err(|e| e.into_engine(ID))?;
            Ok(done.id)
        })?;
        s.note_id(page, done);
        return Ok(Some(dropped_json(s, page, done, "Swimlane")));
    }
    // The Separator: across the phase it is dropped on, at the point; with the phases out of reach, the last.
    let phase = here.phases.iter().find(|p| within(&p.rect, x, y)).map(|p| p.id);
    let along = match here.dir {
        Dir::Down => x,
        Dir::Across => y,
    };
    let (target, split) = match phase {
        Some(id) => (Some(id), Some(along * k)),
        None => (here.phases.last().map(|p| p.id).or(Some(here.lane_list)), None),
    };
    let done = s.edit("Drop separator", |doc| {
        let done = swimlane::insert_phase(doc, page, target, Place::After, split, PHASE_LABEL, above).map_err(|e| e.into_engine(ID))?;
        let mut all = vec![done.id];
        if let Some(made) = &done.made {
            all.extend(made.phase_list);
            all.extend(&made.phases);
        }
        settle_new(doc, page, &all).map_err(|e| e.into_engine(ID))?;
        Ok((done.id, all))
    })?;
    s.note_id(page, done.1.iter().copied().max().unwrap_or(0));
    Ok(Some(dropped_json(s, page, done.0, "Separator")))
}

/// The answer of a drop that inserted the part `id`: its box's size as the drop of a master tells it.
fn dropped_json(s: &Session, page: u32, id: u32, name: &str) -> Value {
    let size = s
        .document()
        .and_then(|doc| swimlane::describe(doc, page).ok())
        .and_then(|found| found.iter().flat_map(|st| st.lanes.iter().chain(st.phases.iter())).find(|p| p.id == id).and_then(|p| p.rect))
        .map_or((0.0, 0.0), |[x0, y0, x1, y1]| (x1 - x0, y1 - y0));
    json!({ "id": id, "master": Value::Null, "name": name, "page": page, "width": size.0, "height": size.1 })
}
