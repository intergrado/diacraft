//! Stencils opened from files: what a session keeps of one, and what it answers about it.
//!
//! An opened stencil is the document its file reads as (`diacraft_vsdx::read`), kept behind an `Arc` and never changed, and a list of its
//! masters worked out once, when it is opened: each master's name, its size, whether it is one-dimensional, how many places a connector
//! can join, and its icon (looked at, not decoded: `icon.rs`). Nothing is drawn from it and nothing in it runs: a stencil that allows
//! macros is read like the rest, and its project is not a part the reader takes.
//!
//! **Costs.** The work of opening is the reader's, which is bounded by its limits, and then one pass over the masters, each with a handful
//! of cell reads and a pass over its own icon text if that is short enough to be an icon. A listing holds at most [`MAX_LISTED_MASTERS`]
//! masters of a stencil; the masters past it are still there to drop, by their `ID` or their name.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, Master, Owner, Resolver, ShapeRef, SheetRef};
use serde_json::{Value, json};

use super::bring::{Identity, guid_key};
use super::icon::{self, IconInfo, IconProblem};
use crate::memory::Hold;
use crate::text::shown;

/// The most stencils open at once: each holds its file's whole model.
pub const MAX_OPEN_STENCILS: usize = 16;

/// What the opened stencils together may hold of what the reader limits for one file (`diacraft_vsdx::Limits`): its 512 MiB of XML parsed, 12
/// million items kept and 512 MiB of pictures (20 million and 1 GiB before; 12 million items are some 2.4 GB of model and trees at their dearest). **A file's size says nothing of its model**: a file of a few hundred kilobytes may inflate to its
/// 512 MiB of XML, so sixteen of them would hold sixteen times the largest model `file.open` takes. Each stencil is charged what the reader
/// reports it took (`Stats::xml_bytes`, `items`, `media_bytes`), and the sum of all of them stays within one file's limits.
pub const MAX_OPEN_XML_BYTES: u64 = diacraft_vsdx::MAX_XML_BYTES;
/// See [`MAX_OPEN_XML_BYTES`]: the items the reader keeps.
pub const MAX_OPEN_ITEMS: u64 = diacraft_vsdx::MAX_ITEMS as u64;
/// See [`MAX_OPEN_XML_BYTES`]: the bytes of pictures the reader holds.
pub const MAX_OPEN_MEDIA_BYTES: u64 = diacraft_vsdx::MAX_MEDIA_BYTES;

/// What a stencil's model took of the reader's limits: the figures of its report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelCost {
    pub xml_bytes: u64,
    pub items: u64,
    pub media_bytes: u64,
    /// The bytes of memory the model took, as the reader charged them (`Stats::memory_bytes`): what a read of another file leaves room for while the stencil is open. **The
    /// sums above are caps of what the stencils may hold of one file's limits; this is what they hold of the memory** (the items were summed, and a model is
    /// not its items).
    pub memory_bytes: u64,
}

impl ModelCost {
    /// The cost the reader reports for a file it read.
    pub fn of(stats: &diacraft_vsdx::Stats) -> ModelCost {
        ModelCost {
            xml_bytes: stats.xml_bytes,
            items: u64::try_from(stats.items).unwrap_or(u64::MAX),
            media_bytes: stats.media_bytes,
            memory_bytes: stats.memory_bytes,
        }
    }
}

/// The most masters of one stencil a listing holds. The answer of a stencil of 65,536 masters would be several megabytes of text that no
/// window shows; `masters_total` says how many there are.
pub const MAX_LISTED_MASTERS: usize = 2_048;

/// The shapes of one master whose size a listing reads, and the most shapes it reads over a whole stencil.
const SIZED_PER_MASTER: usize = 8;
const MAX_SIZED_SHAPES: usize = 10_000;

/// The most characters of a master's or a stencil's name an answer holds.
const NAME_CHARS: usize = 128;

/// The farthest a size an answer gives may be from 0, in inches: the editing commands' own bound (`draw::MAX_COORD`).
const MAX_SIZE: f64 = 1.0e6;

/// What a master's icon is, as far as a listing can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconState {
    /// The master has none.
    None,
    /// It has one, and it is within the caps.
    Listed(IconInfo),
    /// It has text that is not an icon we list.
    Refused(IconProblem),
}

/// One master of an opened stencil, as a listing has it.
#[derive(Clone, Debug)]
pub struct MasterEntry {
    /// Where the master is in the stencil's model (`Document::masters`).
    pub position: usize,
    /// The master's `ID`. The first master with an `ID` is the one that has an entry; a hostile file's second one cannot be told from it.
    pub id: u32,
    pub name: String,
    /// The shape's size in inches: for a master of one shape, its `Width` and `Height`; of several, the box that holds them.
    pub width: f64,
    pub height: f64,
    pub connection_points: usize,
    pub one_d: bool,
    pub hidden: bool,
    /// How many shapes the master holds at its top level (a drop takes one).
    pub top_shapes: usize,
    pub icon: IconState,
    /// Whether the master's shape is a part of a structure: typed `Container` or `List` by its `User.msvStructureType`. The AutoConnect strip does
    /// not offer it, and AutoConnect does not join it.
    pub part: bool,
}

impl MasterEntry {
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "id": self.id.to_string(),
            "name": self.name,
            "width": self.width,
            "height": self.height,
            "connection_points": self.connection_points,
            "one_d": self.one_d,
            "hidden": self.hidden,
            "icon": match self.icon {
                IconState::Listed(info) => json!({ "format": info.format.word(), "bytes": info.bytes }),
                IconState::None | IconState::Refused(_) => Value::Null,
            },
        });
        if let (IconState::Refused(problem), Some(map)) = (self.icon, v.as_object_mut()) {
            map.insert("icon_refused".to_owned(), json!(problem.words()));
        }
        if let (true, Some(map)) = (self.part, v.as_object_mut()) {
            map.insert("part".to_owned(), json!(true));
            if !self.one_d {
                map.insert("autoconnect".to_owned(), json!(false));
            }
        }
        v
    }
}

/// A stencil file that is open in a session.
#[derive(Debug)]
pub struct OpenedStencil {
    id: String,
    name: String,
    /// Where the file is, with links resolved when it was opened: opening it again is answered with this stencil.
    canonical: Option<PathBuf>,
    /// The size of the file it was read from.
    file_bytes: u64,
    /// What its model took of the reader's limits, charged against [`MAX_OPEN_XML_BYTES`] and the others.
    cost: ModelCost,
    /// The memory its model took (`cost.memory_bytes`), held in the process's ledger for as long as the stencil is open in any session: a read of another file, in whatever session,
    /// leaves room for it.
    _hold: Hold,
    doc: Arc<Document>,
    entries: Vec<MasterEntry>,
    by_id: HashMap<u32, usize>,
    /// Masters whose `ID` an earlier master has: not addressable, and not listed.
    ignored: usize,
    /// How many masters of the model have each `UniqueID`, and each `BaseID` (as [`guid_key`] makes them), for the master matching.
    unique_counts: HashMap<String, u32>,
    base_counts: HashMap<String, u32>,
}

impl OpenedStencil {
    /// The stencil the document `doc` is. The entries of its masters are worked out here, once.
    pub fn new(id: String, name: String, canonical: Option<PathBuf>, file_bytes: u64, cost: ModelCost, doc: Document) -> OpenedStencil {
        let (entries, by_id, ignored) = {
            let res = Resolver::new(&doc);
            let mut entries = Vec::with_capacity(doc.masters.len().min(4_096));
            let mut by_id: HashMap<u32, usize> = HashMap::new();
            let mut ignored = 0_usize;
            // What is left of the icon text a listing reads (`icon::MAX_ICON_SCAN`).
            let mut icon_text_left = icon::MAX_ICON_SCAN;
            let mut sized_left = MAX_SIZED_SHAPES;
            for (position, master) in doc.masters.iter().enumerate() {
                if by_id.contains_key(&master.id) {
                    ignored += 1;
                    continue;
                }
                by_id.insert(master.id, entries.len());
                entries.push(entry_of(&res, position, master, &mut icon_text_left, &mut sized_left));
            }
            (entries, by_id, ignored)
        };
        let (mut unique_counts, mut base_counts): (HashMap<String, u32>, HashMap<String, u32>) = (HashMap::new(), HashMap::new());
        for master in &doc.masters {
            if let Some(u) = master.unique_id.as_deref() {
                *unique_counts.entry(guid_key(u)).or_insert(0) += 1;
            }
            if let Some(b) = master.raw.get("BaseID") {
                *base_counts.entry(guid_key(b)).or_insert(0) += 1;
            }
        }
        OpenedStencil {
            id,
            name,
            canonical,
            file_bytes,
            cost,
            _hold: Hold::new(cost.memory_bytes),
            doc: Arc::new(doc),
            entries,
            by_id,
            ignored,
            unique_counts,
            base_counts,
        }
    }

    /// `file-1`: what `stencil.close` and `shape.drop` name it by.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The file's name without its type.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn file_bytes(&self) -> u64 {
        self.file_bytes
    }

    /// The model the file reads as.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The masters that can be dropped, in the file's order.
    pub fn entries(&self) -> &[MasterEntry] {
        &self.entries
    }

    /// How many masters of the file are addressable.
    pub fn master_count(&self) -> usize {
        self.entries.len()
    }

    /// How many masters were left out because an earlier one has their `ID`.
    pub fn ignored(&self) -> usize {
        self.ignored
    }

    /// The master a person or a program means by `key`: its `ID` (a whole number, as text) or its name, in any case, with spaces and
    /// underscores alike; an `ID` wins over a name. The first of several masters with one name is the one.
    pub fn find(&self, key: &str) -> Option<&MasterEntry> {
        if key.len() > 256 {
            return None;
        }
        let key = key.trim();
        if let Ok(id) = key.parse::<u32>()
            && let Some(at) = self.by_id.get(&id)
        {
            return self.entries.get(*at);
        }
        let simple = |text: &str| text.trim().to_lowercase().replace(['_', '-'], " ");
        let wanted = simple(key);
        // A name longer than a key can be is not looked at: a stencil of thousands of masters with names of megabytes costs a search nothing.
        let named = |m: &MasterEntry| {
            let master = self.doc.masters.get(m.position);
            [Some(m.name.as_str()), master.and_then(|x| x.name_u.as_deref()), master.and_then(|x| x.name.as_deref())]
                .into_iter()
                .flatten()
                .any(|n| n.len() <= 1_024 && simple(n) == wanted)
        };
        self.entries.iter().find(|m| named(m))
    }

    /// The master with this `ID`, by one lookup (no name is looked at).
    pub fn entry_by_id(&self, id: u32) -> Option<&MasterEntry> {
        self.by_id.get(&id).and_then(|at| self.entries.get(*at))
    }

    /// What decides whether a master of this stencil may be matched by its `UniqueID` or its `BaseID` (`bring.rs`).
    pub fn identity(&self, entry: &MasterEntry) -> Identity {
        let master = self.doc.masters.get(entry.position);
        let unique = master.and_then(|m| m.unique_id.as_deref()).map(guid_key);
        let base = master.and_then(|m| m.raw.get("BaseID")).map(guid_key);
        let one = |counts: &HashMap<String, u32>, key: Option<String>| key.is_some_and(|k| counts.get(&k).copied() == Some(1));
        Identity { unique_is_unique: one(&self.unique_counts, unique), base_is_unique: one(&self.base_counts, base) }
    }

    /// The master's icon, decoded.
    pub fn icon_bytes(&self, entry: &MasterEntry) -> Result<(icon::IconFormat, Vec<u8>), IconError> {
        let text = self.doc.masters.get(entry.position).and_then(|m| m.icon.as_deref()).ok_or(IconError::None)?;
        icon::decode(text).map_err(IconError::Problem)
    }

    /// The master's icon text without its white space, for an answer.
    pub fn icon_text(&self, entry: &MasterEntry) -> Option<String> {
        let text = self.doc.masters.get(entry.position).and_then(|m| m.icon.as_deref())?;
        // Only text that `inspect` took is passed on, so this is a pass over at most `MAX_ICON_TEXT` characters.
        icon::inspect(text).ok().map(|_| icon::compact(text))
    }

    /// The stencil as `stencil.list` and `stencil.open` answer it.
    pub fn to_json(&self) -> Value {
        let masters: Vec<Value> = self.entries.iter().take(MAX_LISTED_MASTERS).map(MasterEntry::to_json).collect();
        let mut v = json!({
            "id": self.id,
            "name": self.name,
            "builtin": false,
            "masters": masters,
            "masters_total": self.entries.len(),
        });
        if let (true, Some(map)) = (self.ignored > 0, v.as_object_mut()) {
            map.insert("masters_ignored".to_owned(), json!(self.ignored));
        }
        v
    }

    /// Whether the file this stencil came from is `path` (both with links resolved).
    fn is_file(&self, path: &std::path::Path) -> bool {
        self.canonical.as_deref() == Some(path)
    }
}

/// Why an icon is not given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconError {
    /// The master has none.
    None,
    Problem(IconProblem),
}

/// A master's entry: read from the master shape's cells by inheritance (a size may come from a style).
fn entry_of(res: &Resolver<'_>, position: usize, master: &Master, icon_text_left: &mut usize, sized_left: &mut usize) -> MasterEntry {
    // The shapes a master holds at its top level that are not deleted (`Del`): the count the resolver gives a master, and what an instance
    // of it is made of. Positions of the first few are kept, for the size.
    let top_shapes = master.shapes.iter().filter(|s| !s.del).count();
    let live: Vec<usize> = master.shapes.iter().enumerate().filter(|(_, s)| !s.del).map(|(at, _)| at).take(SIZED_PER_MASTER).collect();
    // Sizing a master is a few cell reads for each shape it looks at, and the whole stencil's share is bounded: a stencil of two million shapes
    // in masters of sixty-four would otherwise cost twelve million reads on opening. A master past the share is listed with no size.
    let sized = live.len() <= *sized_left;
    if sized {
        *sized_left -= live.len();
    }
    let live = if sized { live } else { Vec::new() };
    let number = |sheet: &SheetRef, name: &str| -> Option<f64> {
        res.resolve(sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number()).filter(|n| n.is_finite())
    };
    let is_set = |sheet: &SheetRef, name: &str| {
        res.resolve(sheet, &CellAddr::direct(name)).is_some_and(|f| f.formula.cell.is_some() || f.value.cell.is_some())
    };
    let at = |k: usize| SheetRef::Shape(ShapeRef::top(Owner::Master(master.id), u32::try_from(k).unwrap_or(u32::MAX)));
    let (mut width, mut height, mut one_d, mut part) = (0.0, 0.0, false, false);
    if top_shapes == 1 && !live.is_empty() {
        let first = live.first().copied().unwrap_or(0);
        let sheet = at(first);
        width = number(&sheet, "Width").unwrap_or(0.0);
        height = number(&sheet, "Height").unwrap_or(0.0);
        one_d = ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| is_set(&sheet, name));
        let typed = CellAddr::in_row("User", None, diacraft_doc::RowKey::name("msvStructureType"), "Value");
        part = res
            .resolve(&sheet, &typed)
            .and_then(|f| f.value.cell?.value.as_ref().map(|v| matches!(v.text().trim(), "Container" | "List")))
            .unwrap_or(false);
    } else if top_shapes > 1 && !live.is_empty() {
        // The box that holds the shapes, from their pins and sizes (turns are not followed: this is for a tile's proportions). Only the first
        // few are looked at, so a master of thousands costs what they do.
        let (mut left, mut bottom, mut right, mut top) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for k in live.iter().copied() {
            let sheet = at(k);
            let (w, h) = (number(&sheet, "Width").unwrap_or(0.0), number(&sheet, "Height").unwrap_or(0.0));
            let (px, py) = (number(&sheet, "PinX").unwrap_or(0.0), number(&sheet, "PinY").unwrap_or(0.0));
            let (lx, ly) = (number(&sheet, "LocPinX").unwrap_or(w / 2.0), number(&sheet, "LocPinY").unwrap_or(h / 2.0));
            left = left.min(px - lx);
            bottom = bottom.min(py - ly);
            right = right.max(px - lx + w);
            top = top.max(py - ly + h);
        }
        width = (right - left).max(0.0);
        height = (top - bottom).max(0.0);
    }
    // The places a connector can join: the rows of the `Connection` section of each shape the master holds at its top level (of a master of several,
    // which drops as a group, those of all of them: a connector glues to a member). Counted whether or not the master was sized.
    let connection_points: usize = master
        .shapes
        .iter()
        .filter(|s| !s.del)
        .map(|s| {
            s.sheet
                .sections
                .iter()
                .find(|sec| sec.name.as_str() == "Connection" && !sec.del)
                .map_or(0, |sec| sec.rows.iter().filter(|r| !r.del).count())
        })
        .fold(0_usize, usize::saturating_add);
    let size = |n: f64| if n.is_finite() && n.abs() <= MAX_SIZE { n } else { 0.0 };
    let name = [master.name.as_deref(), master.name_u.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|n| !n.is_empty())
        .map_or_else(|| format!("Master {}", master.id), |n| shown(n, NAME_CHARS));
    let icon = match master.icon.as_deref() {
        None => IconState::None,
        // A text past the cap is refused without a pass over it; one within it is charged for the pass.
        Some(text) if text.len() <= icon::MAX_ICON_TEXT && text.len() > *icon_text_left => IconState::Refused(IconProblem::Skipped),
        Some(text) => {
            if text.len() <= icon::MAX_ICON_TEXT {
                *icon_text_left -= text.len();
            }
            match icon::inspect(text) {
                Ok(info) => IconState::Listed(info),
                Err(problem) => IconState::Refused(problem),
            }
        }
    };
    let hidden = matches!(master.raw.get("Hidden"), Some("1" | "true"));
    MasterEntry { position, id: master.id, name, width: size(width), height: size(height), connection_points, one_d, hidden, top_shapes, icon, part }
}

/// The ids of the built-in stencils a new session has open, in the order of the catalogue: the shipped ones that are open by default.
fn all_builtin_ids() -> Vec<&'static str> {
    super::default_open_ids()
}

/// The stencils open in a session: the built-in ones that are (a drawing made from a template opens the ones that kind needs) and the
/// stencil files, in the order they were opened. Cheap to clone: a command that fails puts the list back as it was.
#[derive(Clone, Debug)]
pub struct OpenedStencils {
    items: Vec<Arc<OpenedStencil>>,
    /// The number the next stencil's id has. Ids are not given again in a session.
    next: u32,
    /// The built-in stencils that are open, in the order the Shapes window lists them. A new session has all of them open, in the order of the
    /// catalogue; a template names the ones it opens (`template/`), and `stencil.open {id}` opens one more by hand (`stencil.close` refuses a built-in stencil).
    builtins: Vec<&'static str>,
}

impl Default for OpenedStencils {
    fn default() -> Self {
        OpenedStencils { items: Vec::new(), next: 0, builtins: all_builtin_ids() }
    }
}

/// Why a stencil could not be added to the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddError {
    TooMany,
    TooMuch,
}

impl OpenedStencils {
    pub fn new() -> OpenedStencils {
        OpenedStencils::default()
    }

    pub fn get(&self, id: &str) -> Option<&Arc<OpenedStencil>> {
        self.items.iter().find(|s| s.id == id)
    }

    /// The built-in stencils that are open, by id, in the order the Shapes window lists them.
    pub fn open_builtins(&self) -> &[&'static str] {
        &self.builtins
    }

    /// Opens the built-in stencil `id` (at the end of the list) → `Some(true)`; `Some(false)` if it is open already; `None` if no built-in stencil has
    /// that id (in any case).
    pub fn open_builtin(&mut self, id: &str) -> Option<bool> {
        let id = super::find_stencil(id)?.id;
        if self.builtins.contains(&id) {
            return Some(false);
        }
        self.builtins.push(id);
        Some(true)
    }

    /// The built-in stencils that are open become exactly `ids`, in that order: an id twice or that no built-in stencil has is left out.
    pub fn set_builtins(&mut self, ids: &[&str]) {
        let mut next: Vec<&'static str> = Vec::with_capacity(ids.len().min(super::stencils().len()));
        for id in ids {
            if let Some(found) = super::find_stencil(id)
                && !next.contains(&found.id)
            {
                next.push(found.id);
            }
        }
        self.builtins = next;
    }

    /// The built-in stencils that are open by default are open, in the order of the catalogue: what a new drawing, a drawing read from a file and a new session have.
    pub fn reset_builtins(&mut self) {
        self.builtins = all_builtin_ids();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<OpenedStencil>> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The stencil open from the file at `path` (links resolved), if there is one.
    pub fn of_file(&self, path: &std::path::Path) -> Option<&Arc<OpenedStencil>> {
        self.items.iter().find(|s| s.is_file(path))
    }

    /// Whether another stencil may be opened at all: fewer than [`MAX_OPEN_STENCILS`] are.
    pub fn room_for_one(&self) -> Result<(), AddError> {
        if self.items.len() >= MAX_OPEN_STENCILS { Err(AddError::TooMany) } else { Ok(()) }
    }

    /// Whether a stencil whose model took `cost` fits beside the open ones: the sums of all of them stay within what the reader limits for one file.
    pub fn fits(&self, cost: &ModelCost) -> Result<(), AddError> {
        let held = |f: fn(&ModelCost) -> u64| self.items.iter().map(|s| f(&s.cost)).fold(0_u64, u64::saturating_add);
        let within = held(|c| c.xml_bytes).saturating_add(cost.xml_bytes) <= MAX_OPEN_XML_BYTES
            && held(|c| c.items).saturating_add(cost.items) <= MAX_OPEN_ITEMS
            && held(|c| c.media_bytes).saturating_add(cost.media_bytes) <= MAX_OPEN_MEDIA_BYTES;
        if within { Ok(()) } else { Err(AddError::TooMuch) }
    }

    /// The memory the models of the open stencils took when they were read, summed ([`ModelCost::memory_bytes`]).
    pub fn memory_bytes(&self) -> u64 {
        self.items.iter().map(|s| s.cost.memory_bytes).fold(0_u64, u64::saturating_add)
    }

    /// The id the next stencil is given, and the list moves on: `file-1`, `file-2`, ...
    pub fn next_id(&mut self) -> String {
        self.next = self.next.saturating_add(1);
        format!("file-{}", self.next)
    }

    /// Adds a stencil (its id came from [`OpenedStencils::next_id`]).
    pub fn add(&mut self, stencil: OpenedStencil) -> Arc<OpenedStencil> {
        let stencil = Arc::new(stencil);
        self.items.push(Arc::clone(&stencil));
        stencil
    }

    /// Closes the stencil with this id; whether there was one.
    pub fn close(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|s| s.id != id);
        self.items.len() != before
    }
}
