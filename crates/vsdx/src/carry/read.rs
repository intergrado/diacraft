//! Reading the items of the carrier, and judging them (the design's 5.4, tier 3). See the module documentation of `carry`.
//!
//! **Order.** The reader calls this when the document, its pages, masters and styles are read: an item is judged against the objects the file now holds. The parts of the package
//! that DiaCraft's solution part names are DiaCraft's and are never held as parts of the package (`read.rs` marks them rebuilt); a part is **copied into the model only when the item that
//! names it is good**, and its bytes are charged to the pool first.
//!
//! **What each judgement costs.** One look at each item. A page, master or style is found by a table made once for the document; the shapes of a page that has items are walked twice (the
//! first finds each shape that has an item and its place, and a key two shapes share; the second puts the data where it belongs); the stamp of an object is made once and of a shared
//! sheet once. A fragment is decoded within the room the pool has left, and the room is charged what the decode spent: **a fragment the pool cannot pay for ends the reading of items,
//! and says so**; reading never fails the drawing for what the carrier holds.
//!
//! **What is left out, and how it is said.** An item whose object is not in the file, or whose key two objects have, is dropped without a word (a count in the report). An item
//! whose stamp no longer matches is dropped (a count). An item that is damaged, a part the package does not have, two records that name one part, a record whose bytes are not the ones
//! it describes, an entry name no package could hold and a part the model cannot hold any more of are dropped with a warning that names none of them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::stamp::Stamp;
use diacraft_doc::{Document, Format, MAX_PARTS, Shape};
use diacraft_opc::Rels;
use diacraft_opc::xml::El;
use diacraft_sheetxml::model::{
    BlobId, BlobSource, Budget, CodecError, DecodeNote, decode, decode_master, decode_page, decode_shape, decode_style, part_lists, stamp_of_bytes,
};
use diacraft_sheetxml::price::arc_str;

use super::stamp::{self, SheetStamps};
use super::{Kind, get_text, shell};
use crate::reader::Reader;

/// The bytes the copy of one node of a sheet (a cell with its name in a map, a pointer to a section or a row) takes, at the dearest: a cell is 80 bytes in a node of the map, its name is held
/// beside it, and the node has room for others.
const COPY_BYTES: u64 = 128;

/// The bytes of a text's characters that make one step of the judge's work: a copy of characters is a copy of bytes, which costs less than a node.
const TEXT_BYTES_PER_STEP: u64 = 64;

/// What an item's put copies, which it is charged for first.
#[derive(Default)]
struct Copies<'a> {
    /// The sheet the item puts data on, when it does.
    sheet: Option<&'a Arc<diacraft_doc::Sheet>>,
    /// The connects of the page or master, when the item puts data on one.
    connects: usize,
    /// The text of the shape, when the item puts data on it, and whether it puts parts among its parts and data in its slot (a put that does neither copies nothing of the text).
    text: Option<(&'a Arc<diacraft_doc::Text>, bool, bool)>,
    /// What the slots the item puts data in, and the shape or page, master or style itself, cost to copy (`size::slot`): each slot is sized by the one function, and charged here.
    slots: u64,
}

/// What an item's put costs in copies: **the steps taken from the judge's budget, and the bytes taken from the pool**. The sheet (`SheetStamps::copy_steps`), the connects, the text (its parts
/// and the nodes of the markup they keep, the lists of its slot, and the bytes of its characters at a step for each [`TEXT_BYTES_PER_STEP`]) and the slots, each sized by `size::slot`.
fn copy_cost(stamps: &mut SheetStamps, copies: &Copies<'_>) -> (u64, u64) {
    let (text_nodes, text_bytes) = copies.text.map_or((0, 0), |(text, parts, ext)| {
        let cost = stamps.text_cost(text);
        (if parts { cost.parts } else { 0 }.saturating_add(if ext { cost.ext } else { 0 }), if parts { cost.bytes } else { 0 })
    });
    let steps = copies
        .sheet
        .map_or(0, |s| stamps.copy_steps(s))
        .saturating_add(copies.connects as u64)
        .saturating_add(text_nodes)
        .saturating_add(text_bytes / TEXT_BYTES_PER_STEP)
        .saturating_add(copies.slots);
    let bytes = steps.saturating_sub(text_bytes / TEXT_BYTES_PER_STEP).saturating_mul(COPY_BYTES).saturating_add(text_bytes);
    (steps, bytes)
}

/// What putting `shell` on the shape `target` copies: its sheet when the shell has places in it, its text (the parts when the shell puts parts among them, the slot when it puts data in it),
/// the slot of the shape when the shell has data for it, the slot of its foreign data when the shell has some, and the shape itself. **The one place that says it**, so that a slot a put
/// copies is charged by the size function of its type (`size::slot`) and no other count.
fn shape_copies<'a>(target: &'a Shape, shell: &Shape) -> Copies<'a> {
    let sheet = shell::touches(&shell.sheet).then_some(&target.sheet);
    let text = target.text.as_ref().and_then(|t| shell.text.as_ref().map(|put| (t, !put.parts.is_empty(), shell::rides(&put.ext))));
    let own = if shell::rides(&shell.ext) { super::size::slot(&target.ext) } else { 0 };
    let foreign = match (&shell.foreign, &target.foreign) {
        (Some(put), Some(f)) if shell::rides(&put.ext) => 1 + super::size::slot(&f.ext),
        _ => 0,
    };
    Copies { sheet, connects: 0, text, slots: 1 + own + foreign }
}

/// The bytes a decode of a fragment may spend on blobs: the pool is charged for them where they are copied, so the codec's own budget is not the limit of anything.
const BLOBS_UNLIMITED: u64 = u64::MAX / 2;

/// An item: its kind, its wrapper and the element of the codec's vocabulary it holds.
#[derive(Clone, Copy)]
pub(crate) struct Item<'e> {
    pub kind: Kind,
    pub wrapper: &'e El,
    pub fragment: &'e El,
}

/// The items of the data that this build reads, by their place among the children of the `Data` element. An element of the carrier that is none (a kind this build does not know, a
/// wrapper with no fragment) is not here, and is kept with the settings.
pub(crate) fn known_items<'e>(data: &'e El, scope: &crate::ns::Scope) -> Vec<(usize, Item<'e>)> {
    let mut out = Vec::new();
    for (at, node) in data.children.iter().enumerate() {
        let diacraft_opc::xml::Node::El(child) = node else { continue };
        if !child.is(super::ITEM) || !scope.enter(child).uri_of_element(child).as_deref().is_some_and(diacraft_doc::is_diacraft_urn) {
            continue;
        }
        let Some(kind) = child.attr("kind").and_then(Kind::of) else { continue };
        let Some(fragment) = child.elements().find(|e| e.is(kind.fragment())) else { continue };
        out.push((at, Item { kind, wrapper: child, fragment }));
    }
    out
}

/// The parts the item list may name: the relationships of the solution part that lead to a part of the package, by `Id`.
struct Parts<'r> {
    by_id: HashMap<&'r str, &'r diacraft_opc::Rel>,
    /// The bytes of each part copied into the model so far, by the `Id` that names it: a part two items name is copied once.
    copies: HashMap<String, Option<Arc<[u8]>>>,
}

/// The source of the bytes a decode asks for, by the number of the blob in the item that is decoded.
struct Blobs(HashMap<u32, Arc<[u8]>>);

impl BlobSource for Blobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.0.get(&id.0).cloned()
    }
}

/// What a decode gave.
enum Decoded<T> {
    /// The fragment as a shell.
    Made(T, Vec<DecodeNote>),
    /// The fragment is damaged, or past a cap of the model: left out.
    Damaged,
    /// The pool cannot pay for it: the reading of items ends.
    Unaffordable,
}

/// The state of one judge: the caches that keep its cost linear and what it counted.
struct State {
    stamps: SheetStamps,
    /// The pool or the judge's own budget refused: nothing more is read.
    stopped: bool,
    /// What is left of the steps the judge may take ([`crate::limits::MAX_CARRY_STEPS`]).
    steps_left: u64,
    /// The steps ran out (and not the pool).
    out_of_steps: bool,
    /// The type of the file allows a macro project. **A macro project of the other format is read only into one that does.**
    macros: bool,
}

impl<'p> Reader<'p> {
    /// Judges the items and puts the good ones in the model. `rels` are the relationships of the solution part (empty when it has none); `part` is its name.
    pub(crate) fn carry_items(&mut self, doc: &mut Document, items: &[Item<'_>], rels: &Rels) {
        if items.is_empty() {
            return;
        }
        // The caches of the pages and masters the reader parsed hold the lists of their shapes: they are not needed again, and a list another holder shares is copied when an item
        // puts data on a shape of it.
        for parsed in self.parsed.values_mut().flatten().chain(self.masters_parsed.iter_mut().flatten()) {
            parsed.shapes = diacraft_doc::ShapeList::new();
            parsed.connects = diacraft_doc::Connects::new();
        }
        let macros = self.notes.report.variant.is_some_and(|v| v.macro_enabled);
        let mut state =
            State { stamps: SheetStamps::default(), stopped: false, steps_left: self.limits.max_carry_steps, out_of_steps: false, macros };
        let mut parts = Parts {
            by_id: rels
                .items
                .iter()
                .filter(|r| super::is_blob_rel_type(&r.ty) && r.target.as_deref().is_some_and(super::is_blob_target))
                .map(|r| (r.id.as_str(), r))
                .collect(),
            copies: HashMap::new(),
        };
        self.notes.report.carry.items = self.notes.report.carry.items.saturating_add(items.len() as u64);
        // Each kind that has one place and no key: more than one item of it names nothing.
        for kind in [Kind::Document, Kind::Parts] {
            let of: Vec<&Item<'_>> = items.iter().filter(|i| i.kind == kind).collect();
            match of.as_slice() {
                [] => {}
                [one] => self.judge_document(doc, one, &mut parts, &mut state),
                many => {
                    self.notes.report.carry.left_out = self.notes.report.carry.left_out.saturating_add(many.len() as u64);
                    self.notes
                        .warn("diacraft_item_ambiguous", || "DiaCraft's part has more than one item for the document, and none is read".to_owned());
                }
            }
        }
        self.judge_comments(doc, items, &mut state);
        self.judge_masters(doc, items, &mut state);
        self.judge_styles(doc, items, &mut state);
        self.judge_pages(doc, items, &mut state);
        self.judge_shapes(doc, items, &mut state);
        if state.out_of_steps {
            self.notes.warn("diacraft_items_not_read", || {
                "DiaCraft's part names more than the work a read may spend on items: the items that were not judged yet are left out".to_owned()
            });
        } else if state.stopped {
            self.notes.warn("diacraft_items_not_read", || {
                "the memory a read may hold is spent: the items of DiaCraft's part that were not read yet are left out".to_owned()
            });
        }
    }

    // ------------------------------------------------------------------------------------------------------------------- the budget

    /// Takes `steps` from the judge's budget; a judge that passes it stops, and says so at the end.
    fn spend(&mut self, state: &mut State, steps: u64) {
        match state.steps_left.checked_sub(steps) {
            Some(left) => state.steps_left = left,
            None => {
                state.steps_left = 0;
                state.stopped = true;
                state.out_of_steps = true;
            }
        }
    }

    /// Makes a stamp, and charges the judge's budget what it cost: **twice the steps it took**, for the stamp and for the copy of the sheet an item that puts data on it makes (the
    /// sheet of an object that is stamped is held by the table of stamps, so it is copied and not changed in place).
    fn stamped<T>(&mut self, state: &mut State, make: impl FnOnce(&mut SheetStamps) -> T) -> T {
        let before = stamp::steps();
        let out = make(&mut state.stamps);
        let spent = stamp::steps().saturating_sub(before);
        self.spend(state, spent.saturating_mul(2).max(1));
        out
    }

    /// Charges what an item's put costs in copies, before it is made: **what an item puts data on is copied where others share it** (the table of stamps holds each sheet and text it has
    /// looked at, so none is changed in place): the sheet (its nodes, `SheetStamps::copy_steps`), the list of connects, **the text** (its parts, the nodes of the markup they keep, and the
    /// bytes of its characters, which a part that rides is put among), and the slots of the shape, which are copied with it. The steps are taken from the judge's budget and, at
    /// [`COPY_BYTES`] a node and a byte for each byte, from the pool. → whether the put may be made: a judge that cannot pay stops and says so.
    fn copy_charge(&mut self, state: &mut State, copies: Copies<'_>) -> bool {
        let (steps, bytes) = copy_cost(&mut state.stamps, &copies);
        crate::work::carry_copied(usize::try_from(steps).unwrap_or(usize::MAX));
        self.spend(state, steps);
        if state.stopped {
            return false;
        }
        if self.take_bytes(bytes).is_err() {
            state.stopped = true;
            return false;
        }
        true
    }

    /// The items whose key is theirs alone: **two items for one object, or for none that can be told, name nothing**, and are all left out with a word (the document's two kinds are
    /// judged so too). A key that cannot be read is a damaged item. Each is found once, before any is judged: an object is never stamped for each of many items.
    fn unique<'a, 'e>(&mut self, list: Vec<&'a Item<'e>>, what: &'static str, key: impl Fn(&Item<'e>) -> Option<String>) -> Vec<&'a Item<'e>> {
        let keys: Vec<Option<String>> = list.iter().map(|i| key(i)).collect();
        let mut count: HashMap<&str, u32> = HashMap::with_capacity(keys.len());
        for k in keys.iter().flatten() {
            *count.entry(k.as_str()).or_insert(0) += 1;
        }
        let (mut damaged, mut twice) = (0u64, 0u64);
        let mut out = Vec::with_capacity(list.len());
        for (item, k) in list.iter().zip(&keys) {
            match k.as_deref().map(|k| count.get(k).copied().unwrap_or(0)) {
                None => damaged += 1,
                Some(1) => out.push(*item),
                Some(_) => twice += 1,
            }
        }
        let report = &mut self.notes.report.carry;
        report.left_out = report.left_out.saturating_add(damaged).saturating_add(twice);
        if damaged > 0 {
            self.notes.warn("diacraft_item_damaged", || format!("{damaged} items of DiaCraft's part for {what} have no key, and are left out"));
        }
        if twice > 0 {
            self.notes.warn("diacraft_item_ambiguous", || {
                format!("{twice} items of DiaCraft's part for {what} name an object that another item names, and are left out")
            });
        }
        out
    }

    // --------------------------------------------------------------------------------------------------------------------- the pool

    /// Decodes a fragment within the room the pool has left, and charges the pool what the decode spent. `None` is a refusal of the pool.
    fn decode_in<T>(&mut self, run: impl FnOnce(&mut Budget, &mut Vec<DecodeNote>) -> Result<T, CodecError>) -> Decoded<T> {
        let items = u64::try_from(self.sheets.items_left()).unwrap_or(u64::MAX);
        let mut budget = Budget::new(items, self.sheets.bytes_left(), BLOBS_UNLIMITED);
        let before = budget.clone();
        let mut notes: Vec<DecodeNote> = Vec::new();
        let out = run(&mut budget, &mut notes);
        // What the decode did is the file's cost, whatever it ended as.
        let spent_items = usize::try_from(before.items.saturating_sub(budget.items)).unwrap_or(usize::MAX);
        let spent_bytes = before.bytes.saturating_sub(budget.bytes);
        let paid = self.take_items(spent_items).is_ok() && self.take_exact(spent_bytes).is_ok();
        match out {
            Ok(value) if paid => Decoded::Made(value, notes),
            Ok(_) => Decoded::Unaffordable,
            Err(CodecError::Limit { what: "items in the drawing" | "bytes of the model in memory", .. }) => Decoded::Unaffordable,
            Err(_) => Decoded::Damaged,
        }
    }

    fn left_out(&mut self, state: &mut State, how: Decoded<()>, what: &'static str) {
        match how {
            Decoded::Unaffordable => state.stopped = true,
            _ => {
                self.notes.report.carry.left_out = self.notes.report.carry.left_out.saturating_add(1);
                self.notes.warn("diacraft_item_damaged", || format!("an item of DiaCraft's part for {what} is damaged, and is left out"));
            }
        }
    }

    fn drop_item(&mut self, why: Drop) {
        let report = &mut self.notes.report.carry;
        match why {
            Drop::Stale => report.stale = report.stale.saturating_add(1),
            Drop::Gone => report.gone = report.gone.saturating_add(1),
        }
    }

    fn kept_item(&mut self, notes: Vec<DecodeNote>) {
        let report = &mut self.notes.report.carry;
        report.kept = report.kept.saturating_add(1);
        if notes.iter().any(|n| matches!(n, DecodeNote::PictureLost { .. })) {
            self.notes.warn("diacraft_picture_lost", || "a picture that DiaCraft's part carries has no bytes, and is not shown".to_owned());
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- the document

    /// The document's item and the item of its parts.
    fn judge_document(&mut self, doc: &mut Document, item: &Item<'_>, parts: &mut Parts<'_>, state: &mut State) {
        crate::work::carry_judged(1);
        if state.stopped {
            return;
        }
        // The parts carry no stamp: they sit beside nothing the file says, and a part is judged by what it is (its size and the stamp of its bytes).
        if item.kind == Kind::Document {
            let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
                return self.left_out(state, Decoded::Damaged, "the document");
            };
            if stamp::document_stamp(doc) != fp {
                return self.drop_item(Drop::Stale);
            }
        }
        let blobs = match self.load_blobs(item.fragment, parts, state.macros) {
            Some(blobs) => blobs,
            None => return self.left_out(state, Decoded::Damaged, "the document's parts"),
        };
        let fragment = item.fragment;
        let decoded = self.decode_in(|budget, notes| decode(fragment, &[], &[], &blobs, budget, notes));
        let (shell, notes) = match decoded {
            Decoded::Made(shell, notes) => (shell, notes),
            Decoded::Damaged => return self.left_out(state, Decoded::Damaged, "the document"),
            Decoded::Unaffordable => return self.left_out(state, Decoded::Unaffordable, "the document"),
        };
        let lost = shell::document_put(doc, &shell);
        if lost > 0 {
            self.notes.warn("diacraft_item_damaged", || {
                "an item of DiaCraft's part for the document names faces the file does not have, which are left out".to_owned()
            });
        }
        self.put_parts(doc, &shell, state.macros);
        // Custom properties of the other format, after the file's own.
        let custom: Vec<_> = shell.props.custom.iter().filter(|c| c.format != Format::Vsdx).cloned().collect();
        if !custom.is_empty() {
            Arc::make_mut(&mut doc.props).custom.extend(custom);
        }
        self.kept_item(notes);
    }

    /// The parts of the other format that a decoded item holds go into the model: each under a name an `.odg` package could hold (`odf_entry_name_ok`) that no earlier part of the
    /// model has, even by case, and as many as the model may hold.
    ///
    /// **A part is what its name says it is**: the role of a part of an `.odg` is made again from its name (`odf_role_of`, the rule of the `.odg` reader), whatever the item says, and **a
    /// macro project (by the role or by the name) is read only when the type of the file allows macros**: a file of a type that promises none gets none, whatever its items claim.
    fn put_parts(&mut self, doc: &mut Document, shell: &Document, macros: bool) {
        let mut seen: HashSet<String> = doc.parts.iter().filter(|p| p.format != Format::Vsdx).map(|p| p.name.to_ascii_lowercase()).collect();
        let (mut refused, mut twice, mut over, mut macro_refused, mut roles) = (0u64, 0u64, 0u64, 0u64, 0u64);
        for part in shell.parts.iter().filter(|p| p.format != Format::Vsdx) {
            let derived = if part.format == Format::Odf { super::odf_role_of(&part.name) } else { part.role };
            let judged = diacraft_doc::Part { role: derived, ..(**part).clone() };
            if super::is_macro_part(&judged) && !macros {
                macro_refused += 1;
                continue;
            }
            let part = if derived == part.role {
                part
            } else {
                roles += 1;
                &Arc::new(judged)
            };
            let ok = match part.format {
                Format::Odf => super::odf_entry_name_ok(&part.name),
                _ => super::native_entry_name_ok(&part.name),
            };
            if !ok {
                refused += 1;
                continue;
            }
            if !seen.insert(part.name.to_ascii_lowercase()) {
                twice += 1;
                continue;
            }
            if doc.parts.len() >= MAX_PARTS {
                over += 1;
                continue;
            }
            self.notes.report.carry.parts_kept = self.notes.report.carry.parts_kept.saturating_add(1);
            doc.parts.push(Arc::clone(part));
        }
        if macro_refused > 0 {
            self.notes.warn("diacraft_macro_refused", || {
                format!("{macro_refused} parts of a macro project that DiaCraft's part carries are left out: the type of the file allows no macros")
            });
        }
        if roles > 0 {
            self.notes.warn("diacraft_part_role", || {
                format!("{roles} parts that DiaCraft's part carries state a role that their names do not say: the role of the name is taken")
            });
        }
        if refused > 0 {
            self.notes.warn("diacraft_part_name_refused", || {
                format!("{refused} parts that DiaCraft's part carries have a name that no package could hold, and are left out")
            });
        }
        if twice > 0 {
            self.notes.warn("diacraft_part_name_twice", || {
                format!("{twice} parts that DiaCraft's part carries have a name that an earlier part has, at most in case, and are left out")
            });
        }
        if over > 0 {
            self.notes.warn("diacraft_part_over_the_cap", || {
                format!("{over} parts that DiaCraft's part carries are left out: the drawing holds no more parts")
            });
        }
    }

    /// The bytes of the blobs an item's list names, copied into the model: by the relationship each record names, once for each part however many items name it. A record that names
    /// no relationship the solution part has, or a part the package lacks, or a part two records name, is left without bytes (and the codec drops what names it): each is a warning.
    /// A part is charged to the pool before it is copied. → `None` when the list is damaged.
    ///
    /// **The bytes of a macro project are not copied into a file that allows none** (`macros` false): the blobs that only macro parts name (by their role or their name) are left out before they
    /// are charged or read, and a warning says so.
    fn load_blobs(&mut self, fragment: &El, parts: &mut Parts<'_>, macros: bool) -> Option<Blobs> {
        let lists = part_lists(fragment).ok()?;
        let mut skip: HashSet<u32> = HashSet::new();
        if !macros {
            let (mut of_macros, mut of_others): (HashSet<u32>, HashSet<u32>) = (HashSet::new(), HashSet::new());
            for part in fragment.children_named("part") {
                let Some(blob) = part.attr("blob").and_then(|v| v.parse::<u32>().ok()) else { continue };
                let name = get_text(part, "name").unwrap_or_default();
                let by_role = matches!(part.attr("role"), Some("macro-project" | "macro-signature" | "macro-data"));
                let macro_part = part.attr("format") != Some("vsdx") && (by_role || super::odf_macro_name(&name));
                if macro_part {
                    of_macros.insert(blob)
                } else {
                    of_others.insert(blob)
                };
            }
            skip = of_macros.difference(&of_others).copied().collect();
        }
        let mut named: HashMap<&str, u32> = HashMap::new();
        for record in &lists.blobs {
            *named.entry(record.path.as_str()).or_insert(0) += 1;
        }
        let mut out: HashMap<u32, Arc<[u8]>> = HashMap::new();
        let (mut missing, mut twice, mut unaffordable, mut macro_skipped) = (0u64, 0u64, 0u64, 0u64);
        for record in &lists.blobs {
            crate::work::carry_part_read(1);
            self.notes.report.carry.parts_seen = self.notes.report.carry.parts_seen.saturating_add(1);
            if skip.contains(&record.id.0) {
                macro_skipped += 1;
                continue;
            }
            if named.get(record.path.as_str()).copied().unwrap_or(0) > 1 {
                twice += 1;
                continue;
            }
            let Some(row) = parts.by_id.get(record.path.as_str()) else {
                missing += 1;
                continue;
            };
            let Some(bytes) = row.target.as_deref().and_then(|target| self.bytes_of(target)) else {
                missing += 1;
                continue;
            };
            // What the record says it is: the size first, which costs nothing; the stamp of the bytes is the codec's to check as it takes them.
            if u64::try_from(bytes.len()).ok() != Some(record.size) {
                missing += 1;
                continue;
            }
            let copy = match parts.copies.get(&record.path) {
                Some(copy) => copy.clone(),
                None => {
                    // Charged first, as every byte the model keeps is.
                    let made = if self.take_exact(arc_str(bytes.len())).is_ok() {
                        crate::work::carry_blob_bytes(bytes.len());
                        Some(Arc::<[u8]>::from(bytes))
                    } else {
                        unaffordable += 1;
                        None
                    };
                    parts.copies.insert(record.path.clone(), made.clone());
                    made
                }
            };
            if let Some(copy) = copy
                && stamp_of_bytes(&copy) == record.stamp
            {
                out.insert(record.id.0, copy);
            } else {
                missing += 1;
            }
        }
        if macro_skipped > 0 {
            self.notes.warn("diacraft_macro_refused", || {
                format!("{macro_skipped} parts of a macro project that DiaCraft's part carries are left out: the type of the file allows no macros")
            });
        }
        if missing > 0 {
            self.notes.warn("diacraft_part_missing", || {
                format!("{missing} parts that DiaCraft's part names are not in the file, or are not what the item says, and are left out")
            });
        }
        if twice > 0 {
            self.notes.warn("diacraft_part_twice", || {
                format!("{twice} parts that DiaCraft's part carries are named by more than one record of one item, and are left out")
            });
        }
        if unaffordable > 0 {
            self.notes.warn("diacraft_items_not_read", || {
                format!("{unaffordable} parts that DiaCraft's part carries are left out: the memory a read may hold is spent")
            });
        }
        Some(Blobs(out))
    }

    // ------------------------------------------------------------------------------------------------------------ pages, masters, styles

    /// The threads of the comments: the item's links, put on the comments the comments part gave, when the stamp says the part is as it was when the item was written.
    /// More than one item names nothing; a link that names a comment the file lacks, or two that cannot be a thread, is left out (`Comments::apply_threads`); the work is one step for
    /// each link, taken from the judge's budget before the links are read.
    fn judge_comments(&mut self, doc: &mut Document, items: &[Item<'_>], state: &mut State) {
        let list: Vec<&Item<'_>> = items.iter().filter(|i| i.kind == Kind::Comments).collect();
        match list.as_slice() {
            [] => return,
            [_] => {}
            many => {
                self.notes.report.carry.left_out = self.notes.report.carry.left_out.saturating_add(many.len() as u64);
                self.notes.warn("diacraft_item_ambiguous", || "DiaCraft's part has more than one item for the comments, and none is read".to_owned());
                return;
            }
        }
        let Some(item) = list.first() else { return };
        crate::work::carry_judged(1);
        if state.stopped {
            return;
        }
        let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
            return self.left_out(state, Decoded::Damaged, "the comments");
        };
        let before = stamp::steps();
        let current = stamp::comments_stamp(&doc.comments);
        let spent = stamp::steps().saturating_sub(before);
        self.spend(state, spent.max(1));
        if state.stopped {
            return;
        }
        if current != fp {
            return self.drop_item(Drop::Stale);
        }
        // One step for each link, before any is read, and no more links than there are comments (a thread has a reply for each link).
        let links: Vec<&El> = item.fragment.children_named("link").collect();
        self.spend(state, links.len() as u64);
        if state.stopped {
            return;
        }
        if links.len() > doc.comments.len() {
            return self.left_out(state, Decoded::Damaged, "the comments");
        }
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        if pairs.try_reserve_exact(links.len()).is_err() {
            return self.left_out(state, Decoded::Unaffordable, "the comments");
        }
        let number = |text: Option<&str>| text.filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())).and_then(|t| t.parse::<u32>().ok());
        for link in links {
            let (Some(id), Some(parent)) = (number(link.attr("id")), number(link.attr("parent"))) else {
                return self.left_out(state, Decoded::Damaged, "the comments");
            };
            pairs.push((id, parent));
        }
        let made = doc.comments.apply_threads(&pairs);
        if made < pairs.len() {
            let left = (pairs.len() - made) as u64;
            self.notes.report.carry.left_out = self.notes.report.carry.left_out.saturating_add(left);
        }
        self.kept_item(Vec::new());
    }

    fn judge_pages(&mut self, doc: &mut Document, items: &[Item<'_>], state: &mut State) {
        let list: Vec<&Item<'_>> = items.iter().filter(|i| i.kind == Kind::Page).collect();
        let wanted = self.unique(list, "a page", |i| i.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok()).map(|n| n.to_string()));
        if wanted.is_empty() {
            return;
        }
        let mut table: HashMap<u32, (u32, usize)> = HashMap::with_capacity(doc.pages.len());
        for (at, page) in doc.pages.iter().enumerate() {
            table.entry(page.id).and_modify(|e| e.0 += 1).or_insert((1, at));
        }
        for item in wanted {
            crate::work::carry_judged(1);
            if state.stopped {
                return;
            }
            let Some(id) = item.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok()) else { continue };
            let Some(&(uses, at)) = table.get(&id) else {
                self.drop_item(Drop::Gone);
                continue;
            };
            if uses > 1 {
                self.drop_item(Drop::Gone);
                continue;
            }
            let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
                self.left_out(state, Decoded::Damaged, "a page");
                continue;
            };
            let current = doc.pages.get(at).map(|p| self.stamped(state, |stamps| stamp::page_stamp(p, stamps)));
            if current != Some(fp) {
                self.drop_item(Drop::Stale);
                continue;
            }
            let fragment = item.fragment;
            match self.decode_in(|budget, notes| decode_page(fragment, &NoBlobs, budget, notes)) {
                Decoded::Made(shell, notes) => {
                    let paid = {
                        let page = doc.pages.get(at);
                        let sheet = page.filter(|_| shell::touches(&shell.sheet)).map(|p| &p.sheet);
                        let connects = page.filter(|_| !shell.connects.is_empty()).map_or(0, |p| p.connects.len());
                        let slots = page.filter(|_| shell::rides(&shell.ext)).map_or(0, |p| 1 + super::size::slot(&p.ext));
                        self.copy_charge(state, Copies { sheet, connects, slots, ..Copies::default() })
                    };
                    if !paid {
                        return;
                    }
                    if let Some(page) = doc.pages.get_mut(at) {
                        self.after_put(shell::page_put(Arc::make_mut(page), &shell));
                    }
                    self.kept_item(notes);
                }
                Decoded::Damaged => self.left_out(state, Decoded::Damaged, "a page"),
                Decoded::Unaffordable => self.left_out(state, Decoded::Unaffordable, "a page"),
            }
        }
    }

    fn judge_masters(&mut self, doc: &mut Document, items: &[Item<'_>], state: &mut State) {
        let list: Vec<&Item<'_>> = items.iter().filter(|i| i.kind == Kind::Master).collect();
        let wanted = self.unique(list, "a master", |i| i.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok()).map(|n| n.to_string()));
        if wanted.is_empty() {
            return;
        }
        let mut table: HashMap<u32, (u32, usize)> = HashMap::with_capacity(doc.masters.len());
        for (at, master) in doc.masters.iter().enumerate() {
            table.entry(master.id).and_modify(|e| e.0 += 1).or_insert((1, at));
        }
        for item in wanted {
            crate::work::carry_judged(1);
            if state.stopped {
                return;
            }
            let Some(id) = item.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok()) else { continue };
            let Some(&(uses, at)) = table.get(&id) else {
                self.drop_item(Drop::Gone);
                continue;
            };
            if uses > 1 {
                self.drop_item(Drop::Gone);
                continue;
            }
            let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
                self.left_out(state, Decoded::Damaged, "a master");
                continue;
            };
            let current = doc.masters.get(at).map(|m| self.stamped(state, |stamps| stamp::master_stamp(m, stamps)));
            if current != Some(fp) {
                self.drop_item(Drop::Stale);
                continue;
            }
            let fragment = item.fragment;
            match self.decode_in(|budget, notes| decode_master(fragment, &NoBlobs, budget, notes)) {
                Decoded::Made(shell, notes) => {
                    let paid = {
                        let master = doc.masters.get(at);
                        let sheet = master.filter(|_| shell::touches(&shell.sheet)).map(|m| &m.sheet);
                        let connects = master.filter(|_| !shell.connects.is_empty()).map_or(0, |m| m.connects.len());
                        let slots = master.filter(|_| shell::rides(&shell.ext)).map_or(0, |m| 1 + super::size::slot(&m.ext));
                        self.copy_charge(state, Copies { sheet, connects, slots, ..Copies::default() })
                    };
                    if !paid {
                        return;
                    }
                    if let Some(master) = doc.masters.get_mut(at) {
                        self.after_put(shell::master_put(Arc::make_mut(master), &shell));
                    }
                    self.kept_item(notes);
                }
                Decoded::Damaged => self.left_out(state, Decoded::Damaged, "a master"),
                Decoded::Unaffordable => self.left_out(state, Decoded::Unaffordable, "a master"),
            }
        }
    }

    fn judge_styles(&mut self, doc: &mut Document, items: &[Item<'_>], state: &mut State) {
        // A style is keyed by its name, or by its `ID` (the writer's rule: when it has no name or another style has it). **An `ID` finds the style of that `ID` whatever its
        // name**, and a name the style of that name; either finds nothing when two styles have it.
        let item_key = |i: &Item<'_>| match (get_text(i.wrapper, "name"), i.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok())) {
            (Some(name), _) => Some(format!("n:{name}")),
            (None, Some(id)) => Some(format!("i:{id}")),
            (None, None) => None,
        };
        let list: Vec<&Item<'_>> = items.iter().filter(|i| i.kind == Kind::Style).collect();
        let wanted = self.unique(list, "a style", item_key);
        if wanted.is_empty() {
            return;
        }
        let mut table: HashMap<String, (u32, usize)> = HashMap::with_capacity(doc.styles.len().saturating_mul(2));
        for (at, style) in doc.styles.iter().enumerate() {
            table.entry(format!("i:{}", style.id)).and_modify(|e| e.0 += 1).or_insert((1, at));
            if let Some(name) = style.name.as_deref() {
                table.entry(format!("n:{name}")).and_modify(|e| e.0 += 1).or_insert((1, at));
            }
        }
        // Each item's style, found once: **two items that reach one style** (one by its name and one by its `ID`) name nothing, and are both left out with a word, as two items of one key are.
        let found: Vec<Option<(u32, usize)>> = wanted.iter().map(|i| item_key(i).and_then(|k| table.get(&k).copied())).collect();
        let mut reached: HashMap<usize, u32> = HashMap::with_capacity(found.len());
        for (_, at) in found.iter().flatten().filter(|(uses, _)| *uses == 1) {
            *reached.entry(*at).or_insert(0) += 1;
        }
        let twice = found.iter().flatten().filter(|(uses, at)| *uses == 1 && reached.get(at).copied().unwrap_or(0) > 1).count() as u64;
        if twice > 0 {
            self.notes.report.carry.left_out = self.notes.report.carry.left_out.saturating_add(twice);
            self.notes.warn("diacraft_item_ambiguous", || {
                format!("{twice} items of DiaCraft's part for a style name a style that another item names by its other key, and are left out")
            });
        }
        for (item, found) in wanted.into_iter().zip(found) {
            crate::work::carry_judged(1);
            if state.stopped {
                return;
            }
            let Some((uses, at)) = found else {
                self.drop_item(Drop::Gone);
                continue;
            };
            if uses > 1 {
                self.drop_item(Drop::Gone);
                continue;
            }
            if reached.get(&at).copied().unwrap_or(0) > 1 {
                continue;
            }
            let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
                self.left_out(state, Decoded::Damaged, "a style");
                continue;
            };
            let current = doc.styles.get(at).map(|s| self.stamped(state, |stamps| stamp::style_stamp(s, stamps)));
            if current != Some(fp) {
                self.drop_item(Drop::Stale);
                continue;
            }
            let fragment = item.fragment;
            match self.decode_in(|budget, notes| decode_style(fragment, &NoBlobs, budget, notes)) {
                Decoded::Made(shell, notes) => {
                    let paid = {
                        let style = doc.styles.get(at);
                        let sheet = style.filter(|_| shell::touches(&shell.sheet)).map(|s| &s.sheet);
                        let slots = style.filter(|_| shell::rides(&shell.ext)).map_or(0, |s| 1 + super::size::slot(&s.ext));
                        self.copy_charge(state, Copies { sheet, slots, ..Copies::default() })
                    };
                    if !paid {
                        return;
                    }
                    if let Some(style) = doc.styles.get_mut(at) {
                        self.after_put(shell::style_put(Arc::make_mut(style), &shell));
                    }
                    self.kept_item(notes);
                }
                Decoded::Damaged => self.left_out(state, Decoded::Damaged, "a style"),
                Decoded::Unaffordable => self.left_out(state, Decoded::Unaffordable, "a style"),
            }
        }
    }

    /// What follows a put: a place the shell names that the file does not have is a word.
    fn after_put(&mut self, lost: usize) {
        if lost > 0 {
            self.notes.warn("diacraft_item_damaged", || {
                format!("an item of DiaCraft's part names {lost} places that the file does not have, which are left out")
            });
        }
    }

    // ----------------------------------------------------------------------------------------------------------------------- shapes

    fn judge_shapes(&mut self, doc: &mut Document, items: &[Item<'_>], state: &mut State) {
        // The items of each page and master, by the `ID` of the page or master and the `ID` of the shape: **each shape has one item or none**, and the shapes that more than one item names
        // are left out with a word before any is judged.
        let list: Vec<&Item<'_>> = items.iter().filter(|i| matches!(i.kind, Kind::Shape | Kind::MasterShape)).collect();
        let list = self.unique(list, "a shape", |i| {
            let of = i.wrapper.attr("of").and_then(|v| v.parse::<u32>().ok())?;
            let id = i.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok())?;
            Some(format!("{}:{of}:{id}", i.kind.word()))
        });
        let mut by_container: HashMap<(bool, u32), Vec<&Item<'_>>> = HashMap::new();
        for item in list {
            let Some(of) = item.wrapper.attr("of").and_then(|v| v.parse::<u32>().ok()) else { continue };
            by_container.entry((item.kind == Kind::MasterShape, of)).or_default().push(item);
        }
        if by_container.is_empty() {
            return;
        }
        let mut pages: HashMap<u32, (u32, usize)> = HashMap::new();
        for (at, page) in doc.pages.iter().enumerate() {
            pages.entry(page.id).and_modify(|e| e.0 += 1).or_insert((1, at));
        }
        let mut masters: HashMap<u32, (u32, usize)> = HashMap::new();
        for (at, master) in doc.masters.iter().enumerate() {
            masters.entry(master.id).and_modify(|e| e.0 += 1).or_insert((1, at));
        }
        // In a fixed order, so that two reads of one file say the same.
        let mut containers: Vec<((bool, u32), Vec<&Item<'_>>)> = by_container.into_iter().collect();
        containers.sort_by_key(|(key, _)| *key);
        for ((in_master, of), list) in containers {
            let table = if in_master { &masters } else { &pages };
            let at = match table.get(&of) {
                Some(&(1, at)) => at,
                _ => {
                    for _ in &list {
                        crate::work::carry_judged(1);
                        self.drop_item(Drop::Gone);
                    }
                    continue;
                }
            };
            if state.stopped {
                return;
            }
            self.judge_shapes_of(doc, in_master, at, &list, state);
        }
    }

    /// The shapes of one page or master that have items: found (and a key two shapes share told) in one walk, and given their data in the next.
    fn judge_shapes_of(&mut self, doc: &mut Document, in_master: bool, at: usize, list: &[&Item<'_>], state: &mut State) {
        let wanted: HashSet<u32> = list.iter().filter_map(|i| i.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok())).collect();
        // The first walk: the place of each shape whose `ID` has an item, as the indexes down through the groups, and how many shapes have the `ID`.
        let mut places: HashMap<u32, (u32, Vec<u32>)> = HashMap::with_capacity(wanted.len());
        let mut visited: u64;
        {
            let shapes: &diacraft_doc::ShapeList = if in_master {
                match doc.masters.get(at) {
                    Some(m) => &m.shapes,
                    None => return,
                }
            } else {
                match doc.pages.get(at) {
                    Some(p) => &p.shapes,
                    None => return,
                }
            };
            let mut stack: Vec<(&diacraft_doc::ShapeList, usize)> = vec![(shapes, 0)];
            let mut path: Vec<u32> = Vec::new();
            visited = 0;
            while let Some((top, next)) = stack.last_mut() {
                let Some(shape) = top.get(*next) else {
                    stack.pop();
                    path.pop();
                    continue;
                };
                let here = *next;
                *next += 1;
                visited += 1;
                crate::work::carry_walked(1);
                if wanted.contains(&shape.id) {
                    match places.get_mut(&shape.id) {
                        Some(found) => found.0 += 1,
                        None => {
                            // The place is charged before it is kept.
                            let held = 24u64.saturating_add(4 * (path.len() as u64 + 1));
                            if self.take_bytes(held.saturating_add(48)).is_err() {
                                state.stopped = true;
                                return;
                            }
                            let mut place = path.clone();
                            place.push(u32::try_from(here).unwrap_or(u32::MAX));
                            places.insert(shape.id, (1, place));
                        }
                    }
                }
                if !shape.children.is_empty() && stack.len() < diacraft_doc::MAX_SHAPE_DEPTH {
                    path.push(u32::try_from(here).unwrap_or(u32::MAX));
                    stack.push((&shape.children, 0));
                }
            }
        }
        // The walk is charged to the judge's budget: a page that many containers share, or many pages that share one part, are walked for each of them.
        self.spend(state, visited);
        for item in list {
            crate::work::carry_judged(1);
            if state.stopped {
                return;
            }
            let Some(id) = item.wrapper.attr("id").and_then(|v| v.parse::<u32>().ok()) else { continue };
            let Some((1, place)) = places.get(&id).map(|(n, p)| (*n, p.clone())) else {
                self.drop_item(Drop::Gone);
                continue;
            };
            let Some(fp) = item.wrapper.attr("fp").and_then(Stamp::parse) else {
                self.left_out(state, Decoded::Damaged, "a shape");
                continue;
            };
            self.spend(state, place.len() as u64);
            let current = {
                let list = if in_master { doc.masters.get(at).map(|m| &m.shapes) } else { doc.pages.get(at).map(|p| &p.shapes) };
                list.and_then(|l| shape_at(l, &place)).map(|s| self.stamped(state, |stamps| stamp::shape_stamp(s, stamps)))
            };
            if current != Some(fp) {
                self.drop_item(Drop::Stale);
                continue;
            }
            let at_places: Vec<(u32, u32)> = item
                .wrapper
                .children_named("at")
                .map(|e| (e.attr("n").and_then(|v| v.parse().ok()).unwrap_or(u32::MAX), e.attr("k").and_then(|v| v.parse().ok()).unwrap_or(u32::MAX)))
                .collect();
            let fragment = item.fragment;
            match self.decode_in(|budget, notes| decode_shape(fragment, &NoBlobs, budget, notes)) {
                Decoded::Made(shell, notes) => {
                    let paid = {
                        let list = if in_master { doc.masters.get(at).map(|m| &m.shapes) } else { doc.pages.get(at).map(|p| &p.shapes) };
                        let target = list.and_then(|l| shape_at(l, &place));
                        match target {
                            Some(target) => self.copy_charge(state, shape_copies(target, &shell)),
                            None => true,
                        }
                    };
                    if !paid {
                        return;
                    }
                    let lost = {
                        let root = if in_master {
                            doc.masters.get_mut(at).map(|m| &mut Arc::make_mut(m).shapes)
                        } else {
                            doc.pages.get_mut(at).map(|p| &mut Arc::make_mut(p).shapes)
                        };
                        root.and_then(|l| shape_at_mut(l, &place)).map(|s| shell::shape_put(s, &shell, &at_places))
                    };
                    self.after_put(lost.unwrap_or(0));
                    self.kept_item(notes);
                }
                Decoded::Damaged => self.left_out(state, Decoded::Damaged, "a shape"),
                Decoded::Unaffordable => self.left_out(state, Decoded::Unaffordable, "a shape"),
            }
        }
    }
}

/// Why an item was dropped without a word.
enum Drop {
    Stale,
    Gone,
}

/// A source with no bytes: the items of a page, master, style or shape hold none.
struct NoBlobs;

impl BlobSource for NoBlobs {
    fn get(&self, _: BlobId) -> Option<Arc<[u8]>> {
        None
    }
}

/// The shape at a place: the index in the list, then in the shape's children, and so on down.
fn shape_at<'a>(list: &'a diacraft_doc::ShapeList, place: &[u32]) -> Option<&'a Shape> {
    let (first, rest) = place.split_first()?;
    let mut shape: &Arc<Shape> = list.get(usize::try_from(*first).ok()?)?;
    for index in rest {
        shape = shape.children.get(usize::try_from(*index).ok()?)?;
    }
    Some(shape)
}

/// The shape at a place, to change: each shape down the path is made the model's own where another holder shares it.
fn shape_at_mut<'a>(list: &'a mut diacraft_doc::ShapeList, place: &[u32]) -> Option<&'a mut Shape> {
    let (first, rest) = place.split_first()?;
    let mut shape: &mut Arc<Shape> = list.get_mut(usize::try_from(*first).ok()?)?;
    for index in rest {
        shape = Arc::make_mut(shape).children.get_mut(usize::try_from(*index).ok()?)?;
    }
    Some(Arc::make_mut(shape))
}

#[cfg(test)]
mod tests {
    use diacraft_doc::{Ext, ForeignData, HeldStyleValue, Kept, OdfParagraph, OdfText, OdfTextStyle, Text, TextPart};

    use super::*;

    /// A shape whose text holds `paragraphs` paragraphs of the other format's data, two values each: a text a crafted model makes dear to copy.
    fn shape_with_a_text_of(paragraphs: usize) -> Shape {
        let mut text = Text::default();
        text.parts.push(TextPart::Chars(Arc::from("a")));
        let value = HeldStyleValue { group: diacraft_doc::TextGroup::Size, stamp: None, attrs: Default::default(), kept: Default::default() };
        let paragraph = OdfParagraph { style: OdfTextStyle { held: vec![value; 2], ..OdfTextStyle::default() }, ..OdfParagraph::default() };
        text.ext.make_mut().odf = OdfText { paragraphs: vec![paragraph; paragraphs], ..OdfText::default() };
        let mut shape = Shape::new(1);
        shape.text = Some(Arc::new(text));
        shape
    }

    /// The shell of an item that puts a part among the parts of the text and data in the slot of the text.
    fn shell_that_puts_on_the_text() -> Shape {
        let mut text = Text { parts: vec![TextPart::OdfRaw(Box::new(diacraft_doc::RawNode::Text(Arc::from("x"))))], ext: Ext::default() };
        text.ext.make_mut().odf.kept = Kept { raw: diacraft_doc::RawAttrs::from_pairs([("a", "1")]), ..Kept::default() };
        let mut shell = Shape::new(1);
        shell.text = Some(Arc::new(text));
        shell
    }

    #[test]
    fn every_item_that_puts_on_a_big_shared_text_is_charged_its_parts_and_the_paragraphs_of_its_slot() {
        let target = shape_with_a_text_of(1_000);
        let shell = shell_that_puts_on_the_text();
        let mut stamps = SheetStamps::default();
        // One item: the part, the 1,000 paragraphs of 1 + (1 + 2) each, the slot's other sides, and the shape.
        let copies = shape_copies(&target, &shell);
        let (steps, bytes) = copy_cost(&mut stamps, &copies);
        let paragraphs = 1_000 * 4 + 1 + 1 + 1;
        assert!(steps >= paragraphs && steps <= paragraphs + 8, "{steps}");
        assert!(bytes >= steps * COPY_BYTES, "each node is charged to the pool: {bytes}");
        // The text is walked once, and every item is charged its copy: fifty items are fifty times the steps.
        let before = stamp::steps();
        let total: u64 = (0..50).map(|_| copy_cost(&mut stamps, &shape_copies(&target, &shell)).0).sum();
        assert_eq!(total, 50 * steps);
        assert_eq!(stamp::steps(), before, "the cost of the text is known: it is not walked again for each item");
    }

    #[test]
    fn a_put_that_adds_nothing_to_the_text_or_the_foreign_data_is_charged_nothing_for_them() {
        let target = shape_with_a_text_of(1_000);
        // A shell with no text, no foreign data and no slot of the shape: the shape alone.
        let (steps, _) = copy_cost(&mut SheetStamps::default(), &shape_copies(&target, &Shape::new(1)));
        assert_eq!(steps, 1);
        // A shell with a text whose slot carries data and no part: the slot is copied, and the parts are not.
        let mut shell = Shape::new(1);
        let mut text = Text::default();
        text.ext.make_mut().odf.kept = Kept { raw: diacraft_doc::RawAttrs::from_pairs([("a", "1")]), ..Kept::default() };
        shell.text = Some(Arc::new(text));
        let (steps, _) = copy_cost(&mut SheetStamps::default(), &shape_copies(&target, &shell));
        assert!(steps > 4_000, "the slot's paragraphs: {steps}");
        // Foreign data: its slot is charged by the same function, when the shell has data for it.
        let mut foreign_target = Shape::new(1);
        let mut foreign = ForeignData::default();
        for i in 0..300 {
            foreign.ext.make_mut().vsdx.0.push((Arc::from(format!("k{i}")), Kept::default()));
        }
        foreign_target.foreign = Some(Arc::new(foreign));
        let mut put = ForeignData::default();
        put.ext.make_mut().odf.raw = diacraft_doc::RawAttrs::from_pairs([("a", "1")]);
        let mut shell = Shape::new(1);
        shell.foreign = Some(Arc::new(put));
        let (steps, _) = copy_cost(&mut SheetStamps::default(), &shape_copies(&foreign_target, &shell));
        assert!(steps >= 300, "the entries of the foreign data's slot: {steps}");
        assert_eq!(
            copy_cost(&mut SheetStamps::default(), &shape_copies(&foreign_target, &Shape::new(1))).0,
            1,
            "and nothing when the shell has none"
        );
    }
}
