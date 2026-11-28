//! Lists: shapes that hold their members **in a sequence**, and the layout that keeps the sequence: the lanes of a cross-functional flowchart and its phases.
//!
//! **What a list is.** A shape typed `List` (`User.msvStructureType`) holds its members under code 2 of its `Relationships` cell, **in the order the
//! formula names them** (a writer must not sort it; reads the place in that order), and each member names the list under code 5 (both sides). The lists
//! DiaCraft makes say so with a row of ours, `User.DiaCraftListDirection` (0: the members lie one under the next, 1: side by side). **A list of lanes that another program wrote is laid out
//! too** ([`FileParts`]): what each of its shapes is, and which way it runs, are read from the file's own cells and boxes, and no row of DiaCraft's is
//! written into them. A list of anything else that a file holds (a legend's, say) is left as it is: its members move with it, and nothing lays it out.
//!
//! (ours: the specifications say a list arranges its members in sequence, and that a lane's width changes and the lanes after it
//! make room; they give no rule of the arrangement). A list of lanes is laid out in **one pass, in memory**: the members lie one
//! after the other in their order, the first at the list's top left corner, each as thick as it is (never less than [`MIN_INCHES`]), **all as long as the longest** (or as long as a command
//! says); a lane that moves to a new place takes what it holds with it, by the same walk a move of a container does. The list is the box of its members, and the frame that holds the lists
//! is the box of them. The phases lie across the lanes: a band [`PHASE_BAND_INCHES`] thick along the lanes' first side, each phase as wide as it is, **the last taking what the lanes'
//! length leaves** (or the phase after the one that was resized: a phase made wider takes the room from the next one, and the length of the lanes is not changed by it), and as high as the
//! band and the lanes together. A list is read from the cells it has: nothing keeps a second copy of the layout.
//!
//! **What runs it.** The commands of `swimlane.*`, and the same steps that keep containers in order: a lane moved is put in its place by where its middle lies (a drag of its header reorders it at
//! the place it is let go), a lane resized, a lane deleted, a lane fitted to its contents ([`after_move`], [`after_resize`], [`after_delete`]). Each is charged to the edit's one budget
//! (`page::Work`), and a list of more than [`MAX_LIST_MEMBERS`] members is refused in words and not laid out.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{CellAddr, Document, RowKey};

use super::page::{Page, Rect, Unique, Work};
use super::settings::Settings;
use crate::draw::shapes::{move_each, resize_cells};
use crate::draw::{Problem, settle};
use crate::stencil::container::{Side, StyleSpec, find_style};
use crate::stencil::swimlane::{DIRECTION_ROW, FRAME, LANE, LANES, PHASE, PHASES, POOL, ROLE_ROW};

/// The most members of a list that are laid out. A list of more is refused in words (a file may hold a list of any length, and it is moved with its members as any other).
pub const MAX_LIST_MEMBERS: usize = 512;

/// The least a lane is thick and a phase is wide, in paper inches.
pub const MIN_INCHES: f64 = 0.4;

/// The thickness of the band a phase list lies in, along the first side of the lanes, in paper inches.
pub const PHASE_BAND_INCHES: f64 = 0.35;

/// Boxes that differ by less than this (in drawing units) are the same.
const EPS: f64 = 1.0e-9;

/// What a shape is in a cross-functional flowchart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Frame,
    Lanes,
    Phases,
    Lane,
    Phase,
    Other,
}

impl Role {
    fn from_word(word: &str) -> Role {
        match word {
            FRAME | POOL => Role::Frame,
            LANES => Role::Lanes,
            PHASES => Role::Phases,
            LANE => Role::Lane,
            PHASE => Role::Phase,
            _ => Role::Other,
        }
    }
}

/// Which way the members of a list lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// One under the next (horizontal lanes).
    Down,
    /// Side by side (vertical lanes).
    Across,
}

impl Dir {
    pub fn number(self) -> f64 {
        match self {
            Dir::Down => 0.0,
            Dir::Across => 1.0,
        }
    }

    pub fn from_number(n: f64) -> Dir {
        if (n - 1.0).abs() < 0.5 { Dir::Across } else { Dir::Down }
    }

    /// The other way.
    pub fn turned(self) -> Dir {
        match self {
            Dir::Down => Dir::Across,
            Dir::Across => Dir::Down,
        }
    }
}

fn row(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// The role a shape's own (or inherited) `User.DiaCraftSwimlane` row gives it: `None` for a shape with no such row, which may still be a part by the file's own cells ([`role`]). The read
/// is charged.
pub(super) fn own_role(page: &Page<'_>, id: u32) -> Result<Option<Role>, Problem> {
    page.work.charge(4)?;
    let Some(sheet) = page.sheet(id) else { return Ok(None) };
    let found = page.reader.resolver().resolve(&sheet, &row(ROLE_ROW));
    // A cell that holds a formula and no saved value is what the formula works out to, as the picture reads it.
    Ok(found.and_then(|f| page.reader.worked().text_of(&sheet, &row(ROLE_ROW), &f).map(|text| Role::from_word(text.trim()))))
}

/// The role of a shape: from its own `User.DiaCraftSwimlane`, and for a shape that has none, from the file's own cells ([`FileParts`]). The reads are charged.
pub(super) fn role(page: &Page<'_>, id: u32) -> Result<Role, Problem> {
    match own_role(page, id)? {
        Some(role) => Ok(role),
        None => file_role(page, id),
    }
}

/// Whether the shape is the frame of a pool: its own `User.DiaCraftSwimlane` row says `pool`. A pool is a frame like any other, with the pool's name in a strip on the side of its heading
/// (ours: a line separates a pool's label from what it holds, and the layout leaves room for it): the frame is the box of its lists and that strip. The read is charged.
pub(super) fn is_pool(page: &Page<'_>, id: u32) -> Result<bool, Problem> {
    page.work.charge(4)?;
    let Some(sheet) = page.sheet(id) else { return Ok(false) };
    let found = page.reader.resolver().resolve(&sheet, &row(ROLE_ROW));
    Ok(found.and_then(|f| page.reader.worked().text_of(&sheet, &row(ROLE_ROW), &f).map(|text| text.trim() == POOL)).unwrap_or(false))
}

/// The box of a pool's frame round the box of its lists, `to`: the strip for the pool's name, on the side of its heading. Any other frame is the box of its lists.
fn with_strip(page: &Page<'_>, frame: u32, to: Screen) -> Result<Screen, Problem> {
    if !is_pool(page, frame)? {
        return Ok(to);
    }
    let settings = Settings::read(page, frame)?;
    Ok(match settings.heading {
        Side::Left => Screen { l: to.l - settings.band, ..to },
        Side::Right => Screen { r: to.r + settings.band, ..to },
        Side::Top => Screen { t: to.t - settings.band, ..to },
        Side::Bottom => Screen { b: to.b + settings.band, ..to },
    })
}

/// How much of a pool frame's size across the lanes (`dir`) is its strip: the lanes are the rest. Nothing for a frame that is no pool, and for a strip that lies along the lanes.
fn strip_across(page: &Page<'_>, frame: u32, dir: Dir) -> Result<f64, Problem> {
    if !is_pool(page, frame)? {
        return Ok(0.0);
    }
    let settings = Settings::read(page, frame)?;
    let across = match dir {
        Dir::Down => matches!(settings.heading, Side::Left | Side::Right),
        Dir::Across => matches!(settings.heading, Side::Top | Side::Bottom),
    };
    Ok(if across { settings.band } else { 0.0 })
}

/// The direction of a list that DiaCraft lays out: its `User.DiaCraftListDirection`, and for a list of a file that has none, the way its lanes lie ([`file_direction`]); `None` for a shape
/// that is neither. The reads are charged.
pub(super) fn direction(page: &Page<'_>, id: u32) -> Result<Option<Dir>, Problem> {
    page.work.charge(4)?;
    let Some(sheet) = page.sheet(id) else { return Ok(None) };
    if page.reader.is_set(&sheet, &row(DIRECTION_ROW)) {
        return Ok(Some(Dir::from_number(page.reader.addr_number(&sheet, &row(DIRECTION_ROW)))));
    }
    if own_role(page, id)?.is_some() || page.typed(id) != Some("List") {
        return Ok(None);
    }
    file_direction(page, id)
}

// ---- the lists another program wrote ----------------------------------------------------------------------------------------------------------------

/// What a master's name says a shape is: the names C2 gives the masters of the cross-functional samples (`Swimlane List`, `Phase List`, `Swimlane (vertical)`,
/// `Separator (vertical)`) and of the stencil (`Swimlane`, `Separator`). The frame (`CFF Container`) needs no name: it is the container that holds the lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Named {
    Lanes,
    Phases,
    Lane,
    Phase,
}

const NAMES: [(&str, Named); 6] = [
    ("Swimlane List", Named::Lanes),
    ("Phase List", Named::Phases),
    ("Swimlane", Named::Lane),
    ("Swimlane (vertical)", Named::Lane),
    ("Separator", Named::Phase),
    ("Separator (vertical)", Named::Phase),
];

fn named_by(name: &str) -> Option<Named> {
    let name = name.trim();
    NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, named)| *named)
}

/// What a list of a file is by the names: a list of lanes, of phases, or one no name tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Lanes,
    Phases,
    Unknown,
}

/// What the file's own cells make of the swimlanes of a page, for the shapes that have no row of DiaCraft's (ours: the specifications give the cells of a list and of a
/// container, the structure, B1 and R5, and the masters' names, C2, and **no cell that says which way a list runs**):
///
/// * **A list of lanes or of phases** is a shape typed `List` by `User.msvStructureType` that holds at least one member under code 2, at most [`MAX_LIST_MEMBERS`], each typed `Container`
///   (B1: a lane is a container inside a list). A list of anything else, a legend's say, is no list of lanes.
/// * **Which it is**: the name of its master (`Swimlane List`, `Phase List`), else the names of its members' masters (`Swimlane`, `Separator`, and their `(vertical)` forms), else none.
/// * **The frame** is the first container typed `Container` the list names under code 4. Of the lists the frame holds, the one named for lanes is the lanes and the one named for phases the
///   phases; one that no name tells is what the frame still lacks. **A frame whose lists the names do not tell apart** (two lists that none names, two of one kind, or more than two) **is
///   left as it is**, and so are its lists: which is the lanes would be a guess. A list in no frame is a list of lanes unless its name says phases.
/// * **The direction** ([`file_direction`]) is read from the lanes' boxes.
///
/// Each is found once for a page read, when first asked, and every read is charged to the command's budget.
#[derive(Debug, Default)]
pub(super) struct FileParts {
    /// What the masters' names say, by the master's `ID`: built in one pass over the masters, on the first shape that has a master.
    names: Option<HashMap<u32, Named>>,
    lists: HashMap<u32, Option<Kind>>,
    frames: HashMap<u32, Option<(u32, Option<u32>)>>,
    roles: HashMap<u32, Role>,
    dirs: HashMap<u32, Option<Dir>>,
}

/// What the name of the shape's master says it is.
fn named(page: &Page<'_>, id: u32) -> Result<Option<Named>, Problem> {
    page.work.charge(1)?;
    let Some(master) = page.shape_ref(id).and_then(|at| page.doc.shape(&at)).and_then(|s| s.master) else { return Ok(None) };
    let built = page.file.borrow().names.is_some();
    if !built {
        let mut names: HashMap<u32, Named> = HashMap::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for m in page.doc.masters.iter() {
            page.work.charge(1)?;
            // The first master of an `ID` is the one an instance names (`Document::master`).
            if !seen.insert(m.id) {
                continue;
            }
            if let Some(found) = [m.name_u.as_deref(), m.name.as_deref()].into_iter().flatten().find_map(named_by) {
                names.insert(m.id, found);
            }
        }
        page.file.borrow_mut().names = Some(names);
    }
    Ok(page.file.borrow().names.as_ref().and_then(|n| n.get(&master).copied()))
}

/// Whether the shape `list` is a list of lanes or phases, and which by the names: `None` for one that is not (see [`FileParts`]). A list of DiaCraft's is what its row says.
fn kind_of(page: &Page<'_>, list: u32) -> Result<Option<Kind>, Problem> {
    let known = page.file.borrow().lists.get(&list).copied();
    if let Some(known) = known {
        return Ok(known);
    }
    let found = find_kind(page, list)?;
    page.file.borrow_mut().lists.insert(list, found);
    Ok(found)
}

fn find_kind(page: &Page<'_>, list: u32) -> Result<Option<Kind>, Problem> {
    match own_role(page, list)? {
        Some(Role::Lanes) => return Ok(Some(Kind::Lanes)),
        Some(Role::Phases) => return Ok(Some(Kind::Phases)),
        Some(_) => return Ok(None),
        None => {}
    }
    if page.typed(list) != Some("List") {
        return Ok(None);
    }
    let members = page.list_members_of(list)?;
    if members.is_empty() || members.len() > MAX_LIST_MEMBERS {
        return Ok(None);
    }
    let (mut lanes, mut phases) = (0_usize, 0_usize);
    for m in &members {
        page.work.charge(1)?;
        if page.typed(*m) != Some("Container") {
            return Ok(None);
        }
        match named(page, *m)? {
            Some(Named::Lane) => lanes += 1,
            Some(Named::Phase) => phases += 1,
            _ => {}
        }
    }
    Ok(Some(match named(page, list)? {
        Some(Named::Lanes) => Kind::Lanes,
        Some(Named::Phases) => Kind::Phases,
        _ if lanes > 0 && phases == 0 => Kind::Lanes,
        _ if phases > 0 && lanes == 0 => Kind::Phases,
        _ => Kind::Unknown,
    }))
}

/// The frame of the list `list`: the first container typed `Container` that the list names as one it is in.
fn frame_of(page: &Page<'_>, list: u32) -> Result<Option<u32>, Problem> {
    for c in page.containers_of(list)? {
        page.work.charge(1)?;
        if page.typed(c) == Some("Container") {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// The list of lanes and the list of phases the container `frame` holds, as [`FileParts`] tells them apart: `None` when it holds no list of lanes, or lists the names do not tell apart.
fn frame_lists(page: &Page<'_>, frame: u32) -> Result<Option<(u32, Option<u32>)>, Problem> {
    let known = page.file.borrow().frames.get(&frame).copied();
    if let Some(known) = known {
        return Ok(known);
    }
    let found = find_frame_lists(page, frame)?;
    page.file.borrow_mut().frames.insert(frame, found);
    Ok(found)
}

fn find_frame_lists(page: &Page<'_>, frame: u32) -> Result<Option<(u32, Option<u32>)>, Problem> {
    let (mut lanes, mut phases, mut unknown) = (Vec::new(), Vec::new(), Vec::new());
    for m in page.members_of(frame)? {
        page.work.charge(1)?;
        if page.typed(m) != Some("List") {
            continue;
        }
        match kind_of(page, m)? {
            Some(Kind::Lanes) => lanes.push(m),
            Some(Kind::Phases) => phases.push(m),
            Some(Kind::Unknown) => unknown.push(m),
            None => continue,
        }
        // A structure has two lists at most: a third leaves the frame as it is, however many more it holds.
        if lanes.len() + phases.len() + unknown.len() > 2 {
            return Ok(None);
        }
    }
    Ok(match (lanes.as_slice(), phases.as_slice(), unknown.as_slice()) {
        ([l], [], []) => Some((*l, None)),
        ([l], [p], []) | ([l], [], [p]) | ([], [p], [l]) => Some((*l, Some(*p))),
        ([], [], [l]) => Some((*l, None)),
        _ => None,
    })
}

/// The role a list with no row of DiaCraft's has: `Lanes`, `Phases` or `Other`.
fn file_list_role(page: &Page<'_>, list: u32) -> Result<Role, Problem> {
    let known = page.file.borrow().roles.get(&list).copied();
    if let Some(known) = known {
        return Ok(known);
    }
    let found = match kind_of(page, list)? {
        None => Role::Other,
        Some(kind) => match frame_of(page, list)? {
            Some(frame) => match frame_lists(page, frame)? {
                Some((lanes, _)) if lanes == list => Role::Lanes,
                Some((_, Some(phases))) if phases == list => Role::Phases,
                _ => Role::Other,
            },
            None if kind == Kind::Phases => Role::Other,
            None => Role::Lanes,
        },
    };
    page.file.borrow_mut().roles.insert(list, found);
    Ok(found)
}

/// The role of a shape typed `List` that a container names as its list: its own row's, or the file's cells' ([`file_list_role`]). **It never asks the role of another container**, so
/// a chain of containers each naming the next as its list, or two that name each other, is one step and not a walk down the chain (the security scan's finding: the role of a
/// container followed the role of the shape it names, one call for each link, and a file of such a chain overflowed the stack).
fn list_role(page: &Page<'_>, list: u32) -> Result<Role, Problem> {
    if page.typed(list) != Some("List") {
        return Ok(Role::Other);
    }
    match own_role(page, list)? {
        Some(role) => Ok(role),
        None => file_list_role(page, list),
    }
}

/// The role a shape with no row of DiaCraft's has by the file's own cells. Bounded: it reads the shape's ties, the list it names ([`list_role`], which asks no container) and the lists it
/// holds ([`frame_lists`], which asks no role), and none of them reads another shape's role by this function.
fn file_role(page: &Page<'_>, id: u32) -> Result<Role, Problem> {
    match page.typed(id) {
        Some("List") => file_list_role(page, id),
        Some("Container") => {
            if let Some(list) = page.list_of(id)? {
                match list_role(page, list)? {
                    Role::Lanes => return Ok(Role::Lane),
                    Role::Phases => return Ok(Role::Phase),
                    _ => {}
                }
            }
            // A frame: the container its list of lanes names as the one it is in.
            if let Some((lanes, _)) = frame_lists(page, id)?
                && frame_of(page, lanes)? == Some(id)
            {
                return Ok(Role::Frame);
            }
            Ok(Role::Other)
        }
        _ => Ok(Role::Other),
    }
}

/// The direction of a list of a file that has no row of DiaCraft's: of a list of lanes, the way its lanes lie ([`lie_of`]); of a list of phases, across the lanes of its structure.
fn file_direction(page: &Page<'_>, list: u32) -> Result<Option<Dir>, Problem> {
    match file_list_role(page, list)? {
        Role::Lanes => {
            let known = page.file.borrow().dirs.get(&list).copied();
            if let Some(known) = known {
                return Ok(known);
            }
            let members = page.list_members_of(list)?;
            let mut boxes: Vec<Screen> = Vec::with_capacity(members.len().min(MAX_LIST_MEMBERS));
            for m in members.iter().take(MAX_LIST_MEMBERS) {
                page.work.charge(1)?;
                if let Some(b) = screen_of(page, *m)? {
                    boxes.push(b);
                }
            }
            let tolerance = if page.k.is_finite() && page.k > 0.0 { page.k * LIE_TOLERANCE_INCHES } else { LIE_TOLERANCE_INCHES };
            let found = (!boxes.is_empty()).then(|| lie_of(&boxes, tolerance));
            page.file.borrow_mut().dirs.insert(list, found);
            Ok(found)
        }
        Role::Phases => match of_list(page, list)? {
            Some(s) if s.lanes != list => Ok(direction(page, s.lanes)?.map(Dir::turned)),
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// How near two edges are to be one, in paper inches, when the lanes of a file are read for the way they lie.
const LIE_TOLERANCE_INCHES: f64 = 1.0e-3;

/// Our rule: no cell the specifications give says which way a list runs. The way the lanes of a list lie, from their boxes: one under the next when they share their left
/// and right edges and not their top and bottom ones, side by side for the other way round; otherwise (a lane dragged out of line, or a list of one) by what most of them are, wider than
/// tall (one under the next) or taller than wide (side by side); one under the next when that is even.
pub(super) fn lie_of(boxes: &[Screen], tolerance: f64) -> Dir {
    if let Some(first) = boxes.first().copied()
        && boxes.len() >= 2
    {
        let near = |a: f64, b: f64| (a - b).abs() <= tolerance;
        let sides = boxes.iter().all(|b| near(b.l, first.l) && near(b.r, first.r));
        let ends = boxes.iter().all(|b| near(b.t, first.t) && near(b.b, first.b));
        if sides && !ends {
            return Dir::Down;
        }
        if ends && !sides {
            return Dir::Across;
        }
    }
    let wide = boxes.iter().filter(|b| b.width() > b.height() + tolerance).count();
    let tall = boxes.iter().filter(|b| b.height() > b.width() + tolerance).count();
    if tall > wide { Dir::Across } else { Dir::Down }
}

/// A box with the y axis down and the origin at the top left, in which "first" is "up and to the left" whatever the page's own axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Screen {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl Screen {
    pub(super) fn of(r: Rect) -> Screen {
        Screen { l: r.x0, t: -r.y1, r: r.x1, b: -r.y0 }
    }

    pub(super) fn rect(self) -> Rect {
        Rect { x0: self.l, y0: -self.b, x1: self.r, y1: -self.t }
    }

    pub(super) fn width(self) -> f64 {
        self.r - self.l
    }

    pub(super) fn height(self) -> f64 {
        self.b - self.t
    }

    pub(super) fn union(self, o: Screen) -> Screen {
        Screen { l: self.l.min(o.l), t: self.t.min(o.t), r: self.r.max(o.r), b: self.b.max(o.b) }
    }

    /// How far the box goes along the way the members of a list lie.
    pub(super) fn main(self, dir: Dir) -> f64 {
        match dir {
            Dir::Down => self.height(),
            Dir::Across => self.width(),
        }
    }

    /// How far the box goes across it.
    pub(super) fn cross(self, dir: Dir) -> f64 {
        match dir {
            Dir::Down => self.width(),
            Dir::Across => self.height(),
        }
    }

    /// The middle, along the way the members lie.
    fn middle(self, dir: Dir) -> f64 {
        match dir {
            Dir::Down => self.t.midpoint(self.b),
            Dir::Across => self.l.midpoint(self.r),
        }
    }

    fn differs(self, o: Screen) -> bool {
        (self.l - o.l).abs() > EPS || (self.t - o.t).abs() > EPS || (self.r - o.r).abs() > EPS || (self.b - o.b).abs() > EPS
    }
}

/// The parts of a cross-functional flowchart that belong together: the frame, the list of lanes and the list of phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Structure {
    pub frame: Option<u32>,
    pub lanes: u32,
    pub phases: Option<u32>,
}

/// The first shape among `ids` that has the role `want`.
fn first_with(page: &Page<'_>, ids: &[u32], want: Role) -> Result<Option<u32>, Problem> {
    for id in ids {
        if role(page, *id)? == want {
            return Ok(Some(*id));
        }
    }
    Ok(None)
}

/// The structure that the list `list` is a part of (`list` is a list of lanes or of phases).
fn of_list(page: &Page<'_>, list: u32) -> Result<Option<Structure>, Problem> {
    let kind = role(page, list)?;
    if !matches!(kind, Role::Lanes | Role::Phases) {
        return Ok(None);
    }
    let frame = first_with(page, &page.containers_of(list)?, Role::Frame)?;
    let in_frame = match frame {
        Some(f) => page.members_of(f)?,
        None => Vec::new(),
    };
    Ok(match kind {
        Role::Lanes => Some(Structure { frame, lanes: list, phases: first_with(page, &in_frame, Role::Phases)? }),
        _ => first_with(page, &in_frame, Role::Lanes)?.map(|lanes| Structure { frame, lanes, phases: Some(list) }),
    })
}

/// The structure a shape is a part of: a lane, a phase, a list or the frame; `None` for any other shape, or a part DiaCraft did not make.
pub(super) fn structure_of(page: &Page<'_>, id: u32) -> Result<Option<Structure>, Problem> {
    if !page.has(id) {
        return Ok(None);
    }
    match role(page, id)? {
        Role::Lane | Role::Phase => match page.list_of(id)? {
            Some(list) => of_list(page, list),
            None => Ok(None),
        },
        Role::Lanes | Role::Phases => of_list(page, id),
        Role::Frame => {
            let held = page.members_of(id)?;
            match first_with(page, &held, Role::Lanes)? {
                Some(lanes) => Ok(Some(Structure { frame: Some(id), lanes, phases: first_with(page, &held, Role::Phases)? })),
                None => Ok(None),
            }
        }
        Role::Other => Ok(None),
    }
}

/// What a layout is asked for: what changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Trigger {
    /// The lanes (or their order, or their number) changed: every lane is as long as the longest, and the last phase takes what is left.
    Lanes,
    /// The lanes are this long, in drawing units.
    Length(f64),
    /// This phase was made wider or narrower: the phase after it, or before it for the last, takes the difference and the lanes keep their length.
    Phase(u32),
}

/// Where the layout begins: the list's own top left corner (a lane that was moved is put back), or the box of the lanes as they are now (a lane resized takes the room it needs, on the side
/// it was dragged to).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    List,
    Lanes,
}

/// What makes the outline of a shape of the gallery's looks again for a new size: where the shape is in the page's list, its style, the side of its heading and its strip's thickness.
pub(super) type Outline = (usize, &'static StyleSpec, Side, f64);

/// A box to write: the shape, its new box, and what makes its outline again for the new size (a shape of the gallery's looks).
struct Write {
    id: u32,
    to: Rect,
    outline: Option<Outline>,
}

/// What a layout does: shapes to move (with what they hold) and boxes to write.
pub(super) struct Plan {
    moves: Vec<(u32, (f64, f64))>,
    writes: Vec<Write>,
}

impl Plan {
    pub(super) fn is_empty(&self) -> bool {
        self.moves.is_empty() && self.writes.is_empty()
    }
}

fn too_many(list: u32, n: usize) -> Problem {
    Problem::failed(format!("the list {list} holds {n} members, more than the {MAX_LIST_MEMBERS} that DiaCraft lays out: take some away first"))
}

/// The outline a shape of the gallery has made again for a new size: where it is in the page's list, its style, the side of its heading and its strip.
pub(super) fn outline_of(page: &Page<'_>, id: u32) -> Result<Option<Outline>, Problem> {
    let settings = Settings::read(page, id)?;
    Ok(settings.style.as_deref().and_then(find_style).zip(page.at(id)).map(|(style, at)| (at, style, settings.heading, settings.band)))
}

/// A write of the box `to` for the shape `id`, when it is not the box it has (`now`).
fn write_for(page: &Page<'_>, id: u32, now: Screen, to: Screen) -> Result<Option<Write>, Problem> {
    if !to.differs(now) {
        return Ok(None);
    }
    Ok(Some(Write { id, to: to.rect(), outline: outline_of(page, id)? }))
}

/// A box a shape has, as one of the screen's.
fn screen_of(page: &Page<'_>, id: u32) -> Result<Option<Screen>, Problem> {
    Ok(page.bounds(id)?.map(Screen::of))
}

/// Whether the shape is turned or flipped (a lane that is cannot be laid out square to the page).
fn turned(page: &Page<'_>, id: u32) -> bool {
    page.sheet(id).is_some_and(|s| page.reader.pose(&s).is_turned_or_flipped())
}

/// The layout of the structure `s` for what `trigger` says changed: one pass over its lanes and phases, everything planned in memory. `None` when there is nothing to lay out (a list DiaCraft
/// did not make, no lane, a lane that has no box or is turned, a phase that has no box).
pub(super) fn plan(page: &Page<'_>, s: &Structure, trigger: Trigger, anchor: Anchor) -> Result<Option<Plan>, Problem> {
    let k = page.k;
    let Some(dir) = direction(page, s.lanes)? else { return Ok(None) };
    let ids = page.list_members_of(s.lanes)?;
    if ids.is_empty() {
        return Ok(None);
    }
    if ids.len() > MAX_LIST_MEMBERS {
        return Err(too_many(s.lanes, ids.len()));
    }
    let least = MIN_INCHES * k;
    let mut old: Vec<(u32, Screen)> = Vec::with_capacity(ids.len());
    for id in &ids {
        page.work.charge(2)?;
        let Some(b) = screen_of(page, *id)? else { return Ok(None) };
        if turned(page, *id) {
            return Ok(None);
        }
        old.push((*id, b));
    }
    let list_now = screen_of(page, s.lanes)?;
    let all = old.iter().skip(1).fold(old.first().map_or(Screen { l: 0.0, t: 0.0, r: 0.0, b: 0.0 }, |(_, b)| *b), |a, (_, b)| a.union(*b));
    let (l, t) = match (anchor, list_now) {
        (Anchor::List, Some(b)) => (b.l, b.t),
        (Anchor::Lanes, Some(b)) => (all.l.min(b.l), all.t.min(b.t)),
        (_, None) => (all.l, all.t),
    };
    // The phases, when there are some: their boxes and where the length takes up the difference.
    let phase_ids = match s.phases {
        Some(list) if direction(page, list)?.is_some() => page.list_members_of(list)?,
        _ => Vec::new(),
    };
    if phase_ids.len() > MAX_LIST_MEMBERS {
        return Err(too_many(s.phases.unwrap_or(0), phase_ids.len()));
    }
    let mut phases: Vec<(u32, Screen)> = Vec::with_capacity(phase_ids.len());
    for id in &phase_ids {
        page.work.charge(2)?;
        let Some(b) = screen_of(page, *id)? else { return Ok(None) };
        phases.push((*id, b));
    }
    // How long the lanes are.
    let longest = old.iter().map(|(_, b)| b.cross(dir)).fold(0.0_f64, f64::max).max(least);
    let mut len = match trigger {
        Trigger::Length(x) => x.max(least),
        _ => longest,
    };
    // A phase's width is along the lanes: the way the phase list runs, which is across the way the lanes lie.
    let mut widths: Vec<f64> = phases.iter().map(|(_, b)| b.main(dir.turned()).max(0.0)).collect();
    if !phases.is_empty() {
        let n = widths.len();
        let absorber = match trigger {
            Trigger::Phase(id) => phases.iter().position(|(p, _)| *p == id).and_then(|i| if i + 1 < n { Some(i + 1) } else { i.checked_sub(1) }),
            _ => None,
        }
        .unwrap_or(n - 1);
        let others: f64 = widths.iter().enumerate().filter(|(i, _)| *i != absorber).map(|(_, w)| *w).sum();
        let mut want = len - others;
        if want < least {
            if matches!(trigger, Trigger::Phase(_)) {
                return Err(Problem::bad(format!(
                    "the phase after the one that was resized would be thinner than the least, {MIN_INCHES} inch: a phase is at least {MIN_INCHES} inch wide; make the room first"
                )));
            }
            len = others + least;
            want = least;
        }
        if let Some(w) = widths.get_mut(absorber) {
            *w = want;
        }
    }
    // The lanes, one after the other.
    let mut writes: Vec<Write> = Vec::new();
    let mut moves: Vec<(u32, (f64, f64))> = Vec::new();
    let mut at = 0.0_f64;
    let mut lanes_box: Option<Screen> = None;
    for (id, o) in &old {
        let thick = o.main(dir).max(least);
        let to = match dir {
            Dir::Down => Screen { l, t: t + at, r: l + len, b: t + at + thick },
            Dir::Across => Screen { l: l + at, t, r: l + at + thick, b: t + len },
        };
        at += thick;
        lanes_box = Some(lanes_box.map_or(to, |b| b.union(to)));
        let (dx, dy) = (to.l - o.l, to.t - o.t);
        if dx.abs() > EPS || dy.abs() > EPS {
            moves.push((*id, (dx, -dy)));
        }
        let (still_w, still_h) = ((to.width() - o.width()).abs() <= EPS, (to.height() - o.height()).abs() <= EPS);
        if !(still_w && still_h) {
            writes.push(Write { id: *id, to: to.rect(), outline: outline_of(page, *id)? });
        }
    }
    let Some(lanes_box) = lanes_box else { return Ok(None) };
    let list_write = match list_now {
        Some(now) => write_for(page, s.lanes, now, lanes_box)?,
        None => Some(Write { id: s.lanes, to: lanes_box.rect(), outline: None }),
    };
    writes.extend(list_write);
    // The phases: a band along the first side of the lanes, each phase as high (or wide) as the band and the lanes together.
    let mut whole = lanes_box;
    let phase_list_now = match s.phases {
        Some(list) => screen_of(page, list)?,
        None => None,
    };
    if let Some(list) = s.phases.filter(|_| !phases.is_empty()) {
        let band = PHASE_BAND_INCHES * k;
        let mut at = 0.0_f64;
        let mut band_box: Option<Screen> = None;
        for ((id, o), w) in phases.iter().zip(&widths) {
            let mut to = match dir {
                Dir::Down => Screen { l: l + at, t: t - band, r: l + at + w, b: lanes_box.b },
                Dir::Across => Screen { l: l - band, t: t + at, r: lanes_box.r, b: t + at + w },
            };
            // A phase of a file keeps how far it stands out before the lanes and how far it reaches across them, as it did against the lanes' box.
            if let Some(now) = list_now
                && own_role(page, *id)?.is_none()
            {
                to = match dir {
                    Dir::Down => Screen { t: lanes_box.t - (now.t - o.t), b: lanes_box.b + (o.b - now.b), ..to },
                    Dir::Across => Screen { l: lanes_box.l - (now.l - o.l), r: lanes_box.r + (o.r - now.r), ..to },
                };
            }
            at += w;
            band_box = Some(band_box.map_or(to, |b| b.union(to)));
            if let Some(write) = write_for(page, *id, *o, to)? {
                writes.push(write);
            }
        }
        if let Some(band_box) = band_box {
            if let Some(write) = write_for_list(page, list, band_box)? {
                writes.push(write);
            }
            whole = whole.union(band_box);
        }
    }
    if let Some(frame) = s.frame
        && let Some(now) = screen_of(page, frame)?
    {
        // DiaCraft's frame is the box of its lists; a frame of a file keeps the room it had round them (a title, a margin), side by side.
        let of_file = own_role(page, frame)?.is_none();
        let held = list_now.map(|lanes| phase_list_now.map_or(lanes, |p| lanes.union(p))).filter(|_| of_file);
        let to = match held {
            Some(h) => Screen {
                l: whole.l - (h.l - now.l).max(0.0),
                t: whole.t - (h.t - now.t).max(0.0),
                r: whole.r + (now.r - h.r).max(0.0),
                b: whole.b + (now.b - h.b).max(0.0),
            },
            None => whole,
        };
        let to = with_strip(page, frame, to)?;
        if let Some(write) = write_for(page, frame, now, to)? {
            writes.push(write);
        }
    }
    Ok(Some(Plan { moves, writes }))
}

/// A write of the box `to` for the list `id` (a list that has no box is given one).
fn write_for_list(page: &Page<'_>, id: u32, to: Screen) -> Result<Option<Write>, Problem> {
    match screen_of(page, id)? {
        Some(now) => write_for(page, id, now, to),
        None => Ok(Some(Write { id, to: to.rect(), outline: None })),
    }
}

/// Writes a plan: what moves goes first (with what it holds), then each box is written, and the page is recalculated once. → whether anything was written.
pub(super) fn apply(doc: &mut Document, page: u32, plan: Plan) -> Result<bool, Problem> {
    if plan.is_empty() {
        return Ok(false);
    }
    let size = doc.page(page).map_or(0, |p| p.shapes.len());
    let work = Work::new();
    work.charge(size / 16 + 1)?;
    // A shape on a locked layer is not moved or resized by a layout either: the refusal names it and the layer, and nothing is written.
    let touched: Vec<u32> = plan.moves.iter().map(|(id, _)| *id).chain(plan.writes.iter().map(|w| w.id)).collect();
    if let Some((id, layer)) = crate::draw::layers::first_locked(doc, page, &touched)? {
        return Err(Problem::bad(crate::draw::layers::follower_words(id, &layer, "laid out")));
    }
    if !plan.moves.is_empty() {
        work.charge(16 + size / 64)?;
        move_each(doc, page, &plan.moves)?;
    }
    if !plan.writes.is_empty() {
        let before = doc.clone();
        let mut done: Vec<u32> = Vec::with_capacity(plan.writes.len());
        for w in &plan.writes {
            work.charge(16 + size / 64)?;
            resize_cells(doc, page, w.id, w.to.x0, w.to.y0, w.to.width(), w.to.height())?;
            if let Some((at, style, side, band)) = w.outline {
                super::regeometry(doc, page, at, style, side, (w.to.width(), w.to.height()), band)?;
            }
            done.push(w.id);
        }
        settle(doc, &before, page, &done, &[])?;
    }
    Ok(true)
}

/// Lays out the structure `s` (read, planned and written). → whether anything changed.
pub(super) fn relayout(doc: &mut Document, page: u32, s: &Structure, trigger: Trigger, anchor: Anchor) -> Result<bool, Problem> {
    let planned = {
        let p = Page::read(doc, page)?;
        plan(&p, s, trigger, anchor)?
    };
    match planned {
        Some(plan) => apply(doc, page, plan),
        None => Ok(false),
    }
}

/// The members of a list in the order they have and in the order their middles lie.
pub(super) struct Order {
    pub now: Vec<u32>,
    pub wanted: Vec<u32>,
}

/// The members of the list `list` in the order their middles lie along the way the list runs (the order they have now settles a tie): where a lane was let go decides its place. `None` when
/// the list is not one DiaCraft lays out, or a member has no box.
pub(super) fn order_by_place(page: &Page<'_>, list: u32) -> Result<Option<Order>, Problem> {
    let Some(dir) = direction(page, list)? else { return Ok(None) };
    let members = page.list_members_of(list)?;
    if members.len() > MAX_LIST_MEMBERS {
        return Err(too_many(list, members.len()));
    }
    let mut placed: Vec<(f64, usize, u32)> = Vec::with_capacity(members.len());
    for (n, id) in members.iter().enumerate() {
        page.work.charge(2)?;
        let Some(b) = screen_of(page, *id)? else { return Ok(None) };
        placed.push((b.middle(dir), n, *id));
    }
    placed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(Some(Order { now: members, wanted: placed.into_iter().map(|(_, _, id)| id).collect() }))
}

/// The lists that the shapes `ids` are in, each once, among the lists DiaCraft lays out.
fn lists_of(page: &Page<'_>, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    let mut out = Unique::new();
    for id in ids {
        if !page.has(*id) {
            continue;
        }
        if let Some(list) = page.list_of(*id)?
            && direction(page, list)?.is_some()
        {
            out.push(list);
        }
    }
    Ok(out.into_vec())
}

/// Whether the shape is a member of a list DiaCraft lays out: a membership step leaves it to the list (a lane is not made a member of the frame that holds its list).
pub(super) fn is_laid(page: &Page<'_>, id: u32) -> Result<bool, Problem> {
    match page.list_of(id)? {
        Some(list) => Ok(direction(page, list)?.is_some()),
        None => Ok(false),
    }
}

/// After shapes were moved: each list that holds one of them is put in the order of where its members lie, and laid out again. A page with no list costs nothing.
pub(super) fn after_move(doc: &mut Document, page: u32, ids: &[u32]) -> Result<(), Problem> {
    let found: Vec<(Structure, Vec<u32>, u32)> = {
        let p = Page::read(doc, page)?;
        let mut found = Vec::new();
        for list in lists_of(&p, ids)? {
            let Some(s) = of_list(&p, list)? else { continue };
            let Some(order) = order_by_place(&p, list)? else { continue };
            found.push((s, if order.now == order.wanted { Vec::new() } else { order.wanted }, list));
        }
        found
    };
    for (s, wanted, list) in found {
        if !wanted.is_empty() {
            super::swimlane::set_order(doc, page, list, &wanted)?;
        }
        relayout(doc, page, &s, Trigger::Lanes, Anchor::List)?;
    }
    Ok(())
}

/// After shapes were resized: a lane makes the lanes after it make room, a phase takes from the next one, the list of lanes or the frame sets the length of the lanes.
pub(super) fn after_resize(doc: &mut Document, page: u32, ids: &[u32]) -> Result<(), Problem> {
    let mut found: Vec<(Structure, Trigger)> = Vec::new();
    let mut at: HashMap<u32, usize> = HashMap::new();
    {
        let p = Page::read(doc, page)?;
        for id in ids {
            if !p.has(*id) {
                continue;
            }
            let kind = role(&p, *id)?;
            let trigger = match kind {
                Role::Lane => Trigger::Lanes,
                Role::Phase => Trigger::Phase(*id),
                // A frame of a file is a container that holds the lists with room round them: resized by hand it is fitted as one, and the lanes keep their length.
                Role::Frame if own_role(&p, *id)?.is_none() => continue,
                Role::Lanes | Role::Frame => match structure_of(&p, *id)? {
                    Some(s) => match (direction(&p, s.lanes)?, screen_of(&p, *id)?) {
                        // A pool's frame is its lanes and the strip of its name: the lanes are as long as the rest.
                        (Some(dir), Some(b)) => {
                            Trigger::Length(b.cross(dir) - if role(&p, *id)? == Role::Frame { strip_across(&p, *id, dir)? } else { 0.0 })
                        }
                        _ => continue,
                    },
                    None => continue,
                },
                Role::Phases | Role::Other => continue,
            };
            let Some(s) = structure_of(&p, *id)? else { continue };
            // A structure is laid out once, by the strongest thing that changed in it.
            let strength = |t: &Trigger| match t {
                Trigger::Phase(_) => 2,
                Trigger::Length(_) => 1,
                Trigger::Lanes => 0,
            };
            match at.get(&s.lanes).copied().and_then(|n| found.get_mut(n)) {
                Some(known) => {
                    if strength(&trigger) > strength(&known.1) {
                        known.1 = trigger;
                    }
                }
                None => {
                    at.insert(s.lanes, found.len());
                    found.push((s, trigger));
                }
            }
        }
    }
    for (s, trigger) in found {
        relayout(doc, page, &s, trigger, Anchor::Lanes)?;
    }
    Ok(())
}

/// After shapes were taken out of lists (a delete): each list among `lists` that DiaCraft lays out is laid out again, so that the lanes after the one that went close the gap.
pub(super) fn after_delete(doc: &mut Document, page: u32, lists: &[u32]) -> Result<(), Problem> {
    let found: Vec<Structure> = {
        let p = Page::read(doc, page)?;
        let mut found: Vec<Structure> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for list in lists {
            if !p.has(*list) || direction(&p, *list)?.is_none() {
                continue;
            }
            if let Some(s) = of_list(&p, *list)?
                && seen.insert(s.lanes)
            {
                found.push(s);
            }
        }
        found
    };
    for s in found {
        relayout(doc, page, &s, Trigger::Lanes, Anchor::List)?;
    }
    Ok(())
}

/// The lists that hold the shapes `ids` and that DiaCraft lays out: what a delete lays out again after it.
pub(super) fn holding_lists(page: &Page<'_>, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    lists_of(page, ids)
}
