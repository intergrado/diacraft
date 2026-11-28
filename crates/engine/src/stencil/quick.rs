//! Quick Shapes: the masters of each stencil that sit above its divider, and the shapes the AutoConnect flyout offers from them.
//!
//! What the specification gives. Quick Shapes are the masters above the thin divider in a stencil, "typically two to six", that a person adds by dragging
//! across the divider or with Add to Quick Shapes and takes away the same way; a stencil row called Quick Shapes gathers those of every open stencil, each stencil's group
//! under its name; the AutoConnect strip shows **at most four** of them, the first four of the stencil that is selected, changed by selecting another stencil or a shape of
//! the group; Reset Stencil gives the stencil its own back.
//!
//! the decisions the specification leaves, ours; each has a row in the check list.
//! - **A stencil with nothing set has the first four masters that are not connectors and not hidden** ([`DEFAULT_QUICK`]): the first four of the stencil are what the strip offers by
//!   default, and AutoConnect's quick shapes offered the same. Where a person has set the group, it is as they set it, **empty if they took every master away**
//!   (then the strip offers the first four of the stencil as it did, so that a stencil is never one the arrows cannot use).
//! - **They are the drawing's** (like the AutoConnect box): the choice is kept in DiaCraft's own extension data of the drawing (a child called `quick_shapes`), so every format carries
//!   it (`.diacraft`, VSDX and `.odg`) and no stencil file is written to. It is **no undo step** and makes the drawing unsaved (a setting, "Unsaved, not undo"), as the View
//!   boxes are, and undo and redo carry it across.
//! - **A stencil is named by its id when it is built in and by the file's name when it is a file** (a `file-1` is numbered again by each session), a master by its id, so a
//!   drawing reopened with the stencil file opened again has the group it had. An entry for a stencil that is not open stays in the data, and one for a master that is gone is
//!   passed over when the group is read. **A file's name that the data cannot hold as it is** (a control character, or past [`MAX_KEY_BYTES`]: 64 characters of three bytes are)
//!   **is held in a form it can** (`file_key`): each control character as U+FFFD, cut at a character boundary at the limit, so names that differ only past the cut
//!   share a group, as two files of one name do.
//! - **Order**: the group is in the order the masters were added, and a master added again moves to the place asked (`at`, from 0). The masters below the divider keep the
//!   stencil's own order. Reordering the stencils themselves is not built, so the groups gather in the order the Shapes window lists the stencils.
//!
//! **Costs.** The data is at most [`MAX_DATA_BYTES`] of text, read once as JSON with at most [`MAX_QUICK_STENCILS`] stencils of [`MAX_QUICK_PER_STENCIL`] masters of
//! [`MAX_KEY_BYTES`] bytes each: data past them is unreadable (left as it is and never rewritten), and an add past them is refused in words. A group is read with one lookup by id for
//! each master it names, and a stencil with nothing set is read by one pass over its masters that stops at the fourth.
//!
//! **What is written reads back the same**. A change refuses in words a stencil's or a master's key the reader would refuse ([`Problem::BadKey`]), and
//! [`Held::write`] reads its own text back with the reader before it touches the drawing: a text that would not read back as what is held (past [`MAX_DATA_BYTES`], say) is not
//! written ([`Problem::Unwritable`]).

use std::borrow::Cow;

use diacraft_doc::Document;
use serde_json::{Value, json};

use super::opened::{MasterEntry, OpenedStencil, OpenedStencils};
use super::{MasterSpec, StencilSpec, builtin_stencil, find_master_in, master_json};

/// The child of DiaCraft's extension data that holds the choice, and its attribute.
pub const CHILD: &str = "quick_shapes";
const ATTR: &str = "v";

/// The masters a stencil has above its divider when nothing was set.
pub const DEFAULT_QUICK: usize = 4;
/// The masters the AutoConnect strip shows.
pub const OFFERED: usize = 4;
/// The most Quick Shapes one stencil holds.
pub const MAX_QUICK_PER_STENCIL: usize = 24;
/// The most stencils the data holds a group for.
pub const MAX_QUICK_STENCILS: usize = 32;
/// The longest name of a stencil or a master the data holds, in bytes.
pub const MAX_KEY_BYTES: usize = 128;
/// The most bytes of data that are read.
pub const MAX_DATA_BYTES: usize = 64 * 1024;

/// What the drawing's data says of a stencil's group: the stencil's key and its masters' keys, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// Whether the stencil is a built-in one (its key is its id) or a file (its key is the file's name).
    builtin: bool,
    stencil: String,
    masters: Vec<String>,
}

/// The groups a drawing holds, for the stencils that were set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    entries: Vec<Entry>,
}

/// Why the data cannot be read or changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The drawing's data says something this build cannot read: it is left as it is.
    Unreadable,
    /// A group for one more stencil than the data holds.
    TooManyStencils,
    /// More Quick Shapes in one stencil than it holds.
    TooMany,
    /// The drawing's extension data is of a version this build does not change.
    NotKept,
    /// A stencil's or a master's key the data cannot hold (empty, past [`MAX_KEY_BYTES`], or with a control character).
    BadKey,
    /// What would be written would not read back the same (past [`MAX_DATA_BYTES`], say).
    Unwritable,
}

impl Problem {
    /// The refusal, in words.
    pub fn words(self) -> String {
        match self {
            Problem::Unreadable => "this drawing holds Quick Shapes data that this build cannot read (it is kept as it is), so its Quick Shapes cannot be changed".to_owned(),
            Problem::TooManyStencils => format!("the drawing holds the Quick Shapes of {MAX_QUICK_STENCILS} stencils, which is the most: reset one with stencil.reset first"),
            Problem::TooMany => format!("a stencil holds at most {MAX_QUICK_PER_STENCIL} Quick Shapes: remove one with stencil.remove_quick first"),
            Problem::NotKept => "this drawing holds extension data of a version this build does not change (it is kept as it is), so its Quick Shapes cannot be kept in it".to_owned(),
            Problem::BadKey => format!(
                "the drawing's Quick Shapes data names a stencil and a master by a key of 1 to {MAX_KEY_BYTES} bytes with no control character, and this one has none such"
            ),
            Problem::Unwritable => {
                format!("the Quick Shapes would take more than the {} KiB of data a drawing holds, or would not read back as they are, so they are not changed", MAX_DATA_BYTES / 1024)
            }
        }
    }
}

/// Whether the data's reader takes `key` as a stencil's or a master's key.
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_KEY_BYTES && !key.chars().any(char::is_control)
}

/// What the data names a stencil file by: its name where the reader takes it as it is, else the name with each control character as U+FFFD, cut at the last
/// character boundary at or before [`MAX_KEY_BYTES`], so that what is written always reads back. A name of 64 characters of three or four bytes each is past the limit. Names that
/// differ only past the cut, or only in which control characters they hold, share a group, as two files of one name do. An empty name stays empty, which [`valid_key`] refuses.
/// The walk stops when the key is full: a name of any length costs at most [`MAX_KEY_BYTES`] characters.
fn file_key(name: &str) -> Cow<'_, str> {
    if name.len() <= MAX_KEY_BYTES && !name.chars().any(char::is_control) {
        return Cow::Borrowed(name);
    }
    let mut key = String::with_capacity(MAX_KEY_BYTES);
    for ch in name.chars() {
        let ch = if ch.is_control() { char::REPLACEMENT_CHARACTER } else { ch };
        if key.len().saturating_add(ch.len_utf8()) > MAX_KEY_BYTES {
            break;
        }
        key.push(ch);
    }
    Cow::Owned(key)
}

impl Held {
    /// What `doc` holds (nothing for no drawing). `Err` for data that is there and unreadable.
    pub fn of(doc: Option<&Document>) -> Result<Held, Problem> {
        let Some(child) = doc.and_then(|d| d.diacraft_child(CHILD)) else { return Ok(Held::default()) };
        let Some(text) = child.attrs.get(ATTR) else { return Err(Problem::Unreadable) };
        Held::read(text)
    }

    /// What the data's text says.
    fn read(text: &str) -> Result<Held, Problem> {
        if text.len() > MAX_DATA_BYTES {
            return Err(Problem::Unreadable);
        }
        let value: Value = serde_json::from_str(text).map_err(|_| Problem::Unreadable)?;
        let list = value.as_array().ok_or(Problem::Unreadable)?;
        if list.len() > MAX_QUICK_STENCILS {
            return Err(Problem::Unreadable);
        }
        let mut entries: Vec<Entry> = Vec::with_capacity(list.len());
        for item in list {
            let [kind, stencil, masters] = item.as_array().map(Vec::as_slice).ok_or(Problem::Unreadable)? else {
                return Err(Problem::Unreadable);
            };
            let builtin = match kind.as_str() {
                Some("b") => true,
                Some("f") => false,
                _ => return Err(Problem::Unreadable),
            };
            let stencil = stencil.as_str().filter(|k| valid_key(k)).ok_or(Problem::Unreadable)?;
            let masters = masters.as_array().filter(|m| m.len() <= MAX_QUICK_PER_STENCIL).ok_or(Problem::Unreadable)?;
            let mut keys: Vec<String> = Vec::with_capacity(masters.len());
            for m in masters {
                let key = m.as_str().filter(|k| valid_key(k)).ok_or(Problem::Unreadable)?;
                if !keys.iter().any(|k| k == key) {
                    keys.push(key.to_owned());
                }
            }
            if entries.iter().any(|e| e.builtin == builtin && e.stencil == stencil) {
                return Err(Problem::Unreadable);
            }
            entries.push(Entry { builtin, stencil: stencil.to_owned(), masters: keys });
        }
        Ok(Held { entries })
    }

    /// The data's text: nothing for no entry.
    fn text(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let list: Vec<Value> = self.entries.iter().map(|e| json!([if e.builtin { "b" } else { "f" }, e.stencil, e.masters])).collect();
        Some(Value::Array(list).to_string())
    }

    /// Writes what is held into `doc`: the child is taken out when nothing is held. → whether `doc` changed; `Err` for extension data this build does not change, and for a text
    /// that would not read back as what is held (past [`MAX_DATA_BYTES`], say), which is not written.
    pub fn write(&self, doc: &mut Document) -> Result<bool, Problem> {
        let have = doc.diacraft_child(CHILD).and_then(|c| c.attrs.get(ATTR)).map(str::to_owned);
        let want = self.text();
        if have == want {
            return Ok(false);
        }
        // The text is read back by the reader itself, so every rule it has is met by what is written (one parse of at most `MAX_DATA_BYTES`: a longer text is refused unread).
        if let Some(text) = &want
            && !matches!(Held::read(text), Ok(back) if back == *self)
        {
            return Err(Problem::Unwritable);
        }
        let kept = match &want {
            Some(text) => doc.set_diacraft_child(CHILD, Some(&[(ATTR, text.as_str())])),
            None => doc.set_diacraft_child(CHILD, None),
        };
        if kept { Ok(true) } else { Err(Problem::NotKept) }
    }

    fn entry(&self, builtin: bool, stencil: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.builtin == builtin && e.stencil == stencil)
    }

    /// The group for the stencil `key`, made if there is none and with the masters `current` (its default) in it, each once. A key the data cannot hold, the stencil's or one of
    /// `current`, is refused.
    fn entry_mut(&mut self, builtin: bool, key: &str, current: Vec<String>) -> Result<&mut Entry, Problem> {
        if !valid_key(key) {
            return Err(Problem::BadKey);
        }
        if let Some(at) = self.entries.iter().position(|e| e.builtin == builtin && e.stencil == key) {
            return self.entries.get_mut(at).ok_or(Problem::Unreadable);
        }
        if self.entries.len() >= MAX_QUICK_STENCILS {
            return Err(Problem::TooManyStencils);
        }
        if current.len() > MAX_QUICK_PER_STENCIL {
            return Err(Problem::TooMany);
        }
        if !current.iter().all(|k| valid_key(k)) {
            return Err(Problem::BadKey);
        }
        let mut masters: Vec<String> = Vec::with_capacity(current.len());
        for k in current {
            if !masters.contains(&k) {
                masters.push(k);
            }
        }
        self.entries.push(Entry { builtin, stencil: key.to_owned(), masters });
        self.entries.last_mut().ok_or(Problem::Unreadable)
    }
}

/// One master of a group, as the answers have it.
#[derive(Clone, Debug)]
pub struct Item {
    /// The master's key in its stencil: its id for a built-in one, its `ID` as text for a file's.
    pub key: String,
    pub one_d: bool,
    /// Whether the AutoConnect strip may offer it: no connector, no part of a structure, no master that no connector joins, and of a stencil that feeds the strip.
    pub feeds: bool,
    pub json: Value,
}

/// A stencil as a group is made from it: a built-in one or an opened file.
#[derive(Clone, Copy)]
pub enum View<'a> {
    Builtin(&'static StencilSpec),
    File(&'a OpenedStencil),
}

impl<'a> View<'a> {
    /// The stencil's id, as `stencil.list` has it.
    pub fn id(&self) -> &str {
        match self {
            View::Builtin(s) => s.id,
            View::File(s) => s.id(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            View::Builtin(s) => s.name,
            View::File(s) => s.name(),
        }
    }

    pub fn is_builtin(&self) -> bool {
        matches!(self, View::Builtin(_))
    }

    /// What the data names the stencil by: its id for a built-in one, the file's name for a file, in a form the data's reader takes (`file_key`).
    pub fn key(&self) -> Cow<'_, str> {
        match self {
            View::Builtin(s) => Cow::Borrowed(s.id),
            View::File(s) => file_key(s.name()),
        }
    }

    /// The stencil `id` names among the built-in stencils and the open files, if there is one.
    pub fn find(opened: &'a OpenedStencils, id: &str) -> Option<View<'a>> {
        if let Some(file) = opened.get(id) {
            return Some(View::File(file));
        }
        builtin_stencil(id.trim().to_ascii_lowercase().as_str()).map(View::Builtin)
    }

    /// The stencils whose Quick Shapes the window gathers: the built-in ones that are open, then the files, in the order the window lists them.
    pub fn open(opened: &'a OpenedStencils) -> Vec<View<'a>> {
        let mut views: Vec<View<'a>> = opened.open_builtins().iter().filter_map(|id| builtin_stencil(id)).map(View::Builtin).collect();
        views.extend(opened.iter().map(|s| View::File(s)));
        views
    }

    fn builtin_item(m: &MasterSpec) -> Item {
        Item { key: m.id.to_owned(), one_d: m.is_connector(), feeds: m.on_strip(), json: with_stencil(master_json(m), None) }
    }

    fn file_item(&self, e: &MasterEntry) -> Item {
        Item { key: e.id.to_string(), one_d: e.one_d, feeds: !e.one_d && !e.part, json: with_stencil(e.to_json(), Some(self.id())) }
    }

    /// The master `key` names in this stencil (its id), if there is one that is shown.
    fn item(&self, key: &str) -> Option<Item> {
        match self {
            View::Builtin(s) => s.masters().iter().find(|m| m.id == key).map(View::builtin_item),
            View::File(s) => {
                let entry = s.entry_by_id(key.parse::<u32>().ok()?).filter(|e| !e.hidden)?;
                Some(self.file_item(entry))
            }
        }
    }

    /// The master a person or a program means by `text` (an id, or a name in any case): its key.
    pub fn resolve(&self, text: &str) -> Option<String> {
        match self {
            View::Builtin(s) => find_master_in(s, text).map(|m| m.id.to_owned()),
            View::File(s) => s.find(text).filter(|e| !e.hidden).map(|e| e.id.to_string()),
        }
    }

    /// The first [`DEFAULT_QUICK`] masters that are not connectors and not hidden: what a stencil has when nothing was set.
    fn default_group(&self) -> Vec<Item> {
        match self {
            View::Builtin(s) => s.masters().iter().filter(|m| !m.is_connector()).take(DEFAULT_QUICK).map(View::builtin_item).collect(),
            View::File(s) => s.entries().iter().filter(|e| !e.one_d && !e.hidden).take(DEFAULT_QUICK).map(|e| self.file_item(e)).collect(),
        }
    }

    /// The group of this stencil, and whether the drawing set it: the masters the data names that are there, in its order, or the default.
    pub fn group(&self, held: &Held) -> (Vec<Item>, bool) {
        match held.entry(self.is_builtin(), &self.key()) {
            Some(entry) => (entry.masters.iter().filter_map(|k| self.item(k)).collect(), true),
            None => (self.default_group(), false),
        }
    }

    /// The first [`OFFERED`] masters of the stencil's group that the strip may offer; **the first of the stencil that it may when the group has none**; none for a stencil that
    /// feeds the strip nothing, which [`offer_from`] passes over.
    pub fn offer(&self, held: &Held) -> Vec<Item> {
        let (group, _) = self.group(held);
        let offered: Vec<Item> = group.into_iter().filter(|i| i.feeds).take(OFFERED).collect();
        if offered.is_empty() { self.first_offered() } else { offered }
    }

    /// The first [`OFFERED`] masters of the stencil that the strip may offer and that are not hidden, in the stencil's own order: one pass that stops at the fourth.
    fn first_offered(&self) -> Vec<Item> {
        match self {
            View::Builtin(s) => s.masters().iter().filter(|m| m.on_strip()).take(OFFERED).map(View::builtin_item).collect(),
            View::File(s) => s.entries().iter().filter(|e| !e.one_d && !e.part && !e.hidden).take(OFFERED).map(|e| self.file_item(e)).collect(),
        }
    }

    /// The keys the group names now, for changing: the data's, or the default's when nothing was set.
    fn keys(&self, held: &Held) -> Vec<String> {
        self.group(held).0.into_iter().map(|i| i.key).collect()
    }
}

/// `json` with the stencil it is of (`stencil`: the id) so that a window can drop it: a built-in stencil's master drops by its id alone, and a file's by its id and the stencil.
fn with_stencil(mut json: Value, file: Option<&str>) -> Value {
    if let (Some(id), Some(map)) = (file, json.as_object_mut()) {
        map.insert("stencil".to_owned(), json!(id));
    }
    json
}

/// The first [`OFFERED`] masters that the strip may offer of the groups of all the open stencils, in the order the window lists them: the strip of a Quick Shapes stencil that is
/// selected. None when no group holds one: [`first_offer`] then gives the strip its masters.
pub fn offer_gathered(opened: &OpenedStencils, held: &Held) -> Vec<(String, Item)> {
    let mut out: Vec<(String, Item)> = Vec::with_capacity(OFFERED);
    for view in View::open(opened) {
        for item in view.group(held).0.into_iter().filter(|i| i.feeds) {
            if out.len() >= OFFERED {
                return out;
            }
            out.push((view.id().to_owned(), item));
        }
    }
    out
}

/// Our rule where the specification is silent. The strip shows the first four shapes of the stencil that is selected
/// and that "not all stencils feed it (the Arrow Shapes stencil does not)", and no page says what the strip shows while such a stencil is selected. So: **a master feeds the strip** when it
/// is no connector, **no part of a structure** (a Swimlane or a Separator, whose drop is put in a list of lanes or phases, and a stencil file's master whose shape is typed `Container` or
/// `List`: `MasterEntry::part`; and a built-in master of the container role, a package or a subject: `MasterSpec::is_container`), **no built-in master that no connector joins** (a title or a key, which have no connection point) and of a stencil that feeds it (every built-in one but Arrow
/// Shapes, and every stencil file). Such masters stay Quick Shapes where they are (the group is
/// the person's), and the strip passes over them. **A stencil that feeds the strip nothing** (Arrow Shapes, Cross-Functional Flowchart Shapes, a file of parts) gives it what the first open
/// stencil that feeds it offers, in the order the window lists them: the default R3 names, "the stencil that follows the Quick Shapes stencil". → the stencil the masters are of, and them;
/// `view`'s own id and none when no open stencil feeds the strip.
pub fn offer_from(opened: &OpenedStencils, view: &View<'_>, held: &Held) -> (String, Vec<Item>) {
    let own = view.offer(held);
    if !own.is_empty() {
        return (view.id().to_owned(), own);
    }
    first_offer(opened, held).unwrap_or_else(|| (view.id().to_owned(), Vec::new()))
}

/// What the first open stencil that feeds the strip offers: its id and the masters. `None` when no open stencil feeds it.
pub fn first_offer(opened: &OpenedStencils, held: &Held) -> Option<(String, Vec<Item>)> {
    View::open(opened).into_iter().map(|v| (v.id().to_owned(), v.offer(held))).find(|(_, offered)| !offered.is_empty())
}

/// Puts the master `key` into the group of `view` at `at` (from 0; the end when `None`), moving it when it is there already. A master key the data cannot hold is refused.
pub fn add(held: &mut Held, view: &View<'_>, key: &str, at: Option<usize>) -> Result<(), Problem> {
    if !valid_key(key) {
        return Err(Problem::BadKey);
    }
    let current = view.keys(held);
    let entry = held.entry_mut(view.is_builtin(), &view.key(), current)?;
    let mut keys: Vec<String> = entry.masters.iter().filter(|k| k.as_str() != key).cloned().collect();
    if keys.len() >= MAX_QUICK_PER_STENCIL {
        return Err(Problem::TooMany);
    }
    let place = at.unwrap_or(keys.len()).min(keys.len());
    keys.insert(place, key.to_owned());
    entry.masters = keys;
    Ok(())
}

/// Takes the master `key` out of the group of `view`: whether it was in it. A key the data cannot hold, the master's or the stencil's, is refused.
pub fn remove(held: &mut Held, view: &View<'_>, key: &str) -> Result<bool, Problem> {
    let stencil = view.key();
    if !valid_key(key) || !valid_key(&stencil) {
        return Err(Problem::BadKey);
    }
    let current = view.keys(held);
    if !current.iter().any(|k| k == key) {
        return Ok(false);
    }
    let entry = held.entry_mut(view.is_builtin(), &stencil, current)?;
    entry.masters.retain(|k| k != key);
    Ok(true)
}

/// Gives the stencil its own group back (Reset Stencil): whether the drawing had set it.
pub fn reset(held: &mut Held, view: &View<'_>) -> bool {
    let before = held.entries.len();
    let key = view.key();
    held.entries.retain(|e| !(e.builtin == view.is_builtin() && e.stencil == *key));
    held.entries.len() != before
}

/// The attributes of the child as they are held, for what undo and redo carry across: see `crate::view`.
pub fn held_attrs(doc: &Document) -> Option<diacraft_doc::RawAttrs> {
    doc.diacraft_child(CHILD).map(|c| c.attrs.clone())
}

#[cfg(test)]
#[path = "quick_tests.rs"]
mod tests;
