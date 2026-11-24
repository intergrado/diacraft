//! The state of one read: the package, the caps and what is left of each, the warnings, and the caches that keep a hostile file's cost
//! linear in its size.
//!
//! **What is cached, and why.** A part that two pages (or two masters) name is parsed once and shared (`Parsed`): a thousand pages that
//! all point at one 15 MB part would otherwise be a thousand parses of it, which is how a file of a few hundred kilobytes could cost
//! minutes. A picture is copied into the model once however many shapes show it. A relationships part is parsed once and held as a map,
//! since `Rels::get` is a scan and a masters index may hold 65,536 rows. Every one of these is bounded by the package's own size.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Connects, Media, Passthrough, ShapeList};
use diacraft_opc::xml::{El, Node, XmlLimits};
use diacraft_opc::{OpcError, Package, Rel, Rels, XmlError};
use diacraft_sheetxml::SheetReader;

use crate::cost::{GROWTH_BYTES, TREE_ITEM_BYTES, parse_fixed_bytes};
use crate::error::{LimitKind, VsdxError};
use crate::limits::{Caps, Limits};
use crate::report::Notes;

/// A relationships part as a map from `Id` to the row, the first row winning (as `Rels::get` does, and as the package's own warning says). The rows are kept
/// in the order of the file as well, whole, for the reader that holds the ones no field of the model says (`held.rs`).
pub(crate) struct RelMap {
    rows: Vec<Rel>,
    /// `Id` to the place of the first row that has it.
    by_id: HashMap<String, usize>,
}

impl RelMap {
    pub(crate) fn new(rels: Rels) -> RelMap {
        let mut by_id = HashMap::with_capacity(rels.items.len());
        for (at, rel) in rels.items.iter().enumerate() {
            by_id.entry(rel.id.clone()).or_insert(at);
        }
        RelMap { rows: rels.items, by_id }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Rel> {
        self.by_id.get(id).and_then(|&at| self.rows.get(at))
    }

    /// Every row, in the order of the file.
    pub(crate) fn rows(&self) -> &[Rel] {
        &self.rows
    }
}

/// What reading one page or master part gave: shared by every page or master that names it.
#[derive(Clone)]
pub(crate) struct Parsed {
    pub shapes: ShapeList,
    pub connects: Connects,
    /// What the part's root, its `Shapes` and its `Connects` hold that the model has no field for (`PageContents` or `MasterContents`, `Shapes`, `Connects`):
    /// the page or master that names the part holds a copy of the list, in its own slot.
    pub ext: Vec<(Arc<str>, diacraft_doc::Kept)>,
    /// Shapes in the part, nested ones counted.
    pub shape_count: usize,
    /// The items and the cells that reading the part took from the document's budgets, for charging them again when another row names it.
    pub items: usize,
    pub cells: usize,
}

/// The pictures copied into the model, by the part they came from.
#[derive(Default)]
pub(crate) struct MediaCache {
    /// A part as a picture, by folded name: loaded once however many shapes show it.
    images: HashMap<String, Option<Arc<Media>>>,
    /// The picture an unsupported target (an embedded object) falls back to, by the folded name of the target.
    fallbacks: HashMap<String, Option<Arc<Media>>>,
    /// Bytes of pictures copied so far.
    pub bytes: u64,
}

impl MediaCache {
    pub(crate) fn image(&self, folded: &str) -> Option<&Option<Arc<Media>>> {
        self.images.get(folded)
    }

    pub(crate) fn put_image(&mut self, folded: String, media: Option<Arc<Media>>) {
        self.images.insert(folded, media);
    }

    pub(crate) fn fallback(&self, folded: &str) -> Option<&Option<Arc<Media>>> {
        self.fallbacks.get(folded)
    }

    pub(crate) fn put_fallback(&mut self, folded: String, media: Option<Arc<Media>>) {
        self.fallbacks.insert(folded, media);
    }

    /// The folded names of the parts that were loaded as pictures.
    pub(crate) fn loaded(&self) -> impl Iterator<Item = &str> {
        self.images.iter().filter(|(_, media)| media.is_some()).map(|(name, _)| name.as_str())
    }

    /// How many entries the table of pictures has, for pricing a walk of it.
    pub(crate) fn entries(&self) -> usize {
        self.images.len()
    }
}

/// Everything one read keeps.
pub(crate) struct Reader<'p> {
    pub pkg: &'p Package,
    pub limits: &'p Limits,
    pub caps: Caps,
    pub notes: Notes,
    /// The sheet codec, with the budgets of the document's cells, items and formula nodes, and the tallies of its sheets (`diacraft-sheetxml`).
    pub sheets: SheetReader,
    pub xml_bytes_left: u64,
    pub shapes_left: usize,
    pub media: MediaCache,
    /// The `ID`s of the document's styles, once they are read (for the links that name none).
    pub style_ids: HashSet<u32>,
    /// A master's `ID` to its place in the document's list, the first of a repeated `ID` winning (as `Document::master` does).
    pub master_index: HashMap<u32, usize>,
    /// What each master's part gave, in the order of the document's list (`None` for a master with no readable part).
    pub masters_parsed: Vec<Option<Parsed>>,
    /// The `ID`s of all the shapes in a master's part, by the master's place in the list: built when a master is first needed.
    pub master_shape_ids: HashMap<usize, HashSet<u32>>,
    /// The parts parsed, by folded name.
    pub parsed: HashMap<String, Option<Parsed>>,
    pub max_shape_depth: usize,
    pub connects: usize,
    /// How many styles, masters and pages have been read so far (a refused file reports what it got to).
    pub styles_read: usize,
    pub masters_read: usize,
    pub pages_read: usize,
    /// The largest `.rels` part read (bytes, rows) and the XML tree maxima, for `Stats`.
    pub max_rels_bytes: u64,
    pub max_rels_rows: usize,
    pub xml: crate::unheld::XmlMetrics,
    /// What the read holds beside the model's own fields: the parts, the relationships, the rest of the properties (`held.rs`).
    pub held: crate::held::HeldState,
    /// The part being read, for a refusal to say where it was (the section of a sheet being read is the sheet codec's: `sheets`).
    pub current_part: Option<Arc<str>>,
    /// The parts of the pages and masters being parsed ahead on other threads, when the read has any (`ahead.rs`).
    pub look: Option<crate::ahead::Lookahead<'p>>,
}

impl<'p> Reader<'p> {
    /// A reader of `pkg`, which was made from a file of `file_bytes`. **The pool the trees and the model are charged to is what is left of
    /// `Limits::max_memory` when the file, the package inflated and the room a growing list needs beside itself ([`GROWTH_BYTES`]) are counted** (`cost.rs`): a file
    /// and a package that are the whole of it leave a pool of nothing, and the first thing the read charges for is refused.
    pub(crate) fn new(pkg: &'p Package, limits: &'p Limits, file_bytes: usize) -> Reader<'p> {
        let caps = Caps::of(limits);
        let package = pkg.parts().filter_map(|p| pkg.part(p)).map(|b| b.len() as u64).fold(0u64, u64::saturating_add);
        let pool = limits.max_memory.saturating_sub(file_bytes as u64).saturating_sub(package).saturating_sub(GROWTH_BYTES);
        Reader {
            pkg,
            limits,
            caps,
            notes: Notes::new(limits.max_report_entries),
            sheets: SheetReader::new(Caps::sheets(limits)).with_bytes(pool).with_sharing(),
            xml_bytes_left: limits.max_xml_bytes,
            shapes_left: caps.shapes,
            media: MediaCache::default(),
            style_ids: HashSet::new(),
            master_index: HashMap::new(),
            masters_parsed: Vec::new(),
            master_shape_ids: HashMap::new(),
            parsed: HashMap::new(),
            max_shape_depth: 0,
            connects: 0,
            styles_read: 0,
            masters_read: 0,
            pages_read: 0,
            max_rels_bytes: 0,
            max_rels_rows: 0,
            xml: crate::unheld::XmlMetrics::default(),
            held: crate::held::HeldState::default(),
            current_part: None,
            look: None,
        }
    }

    /// Takes `n` items from the document's total: called before what they pay for is stored. A file past the total is refused whole.
    pub(crate) fn take_items(&mut self, n: usize) -> Result<(), VsdxError> {
        Ok(self.sheets.take_items(n)?)
    }

    /// Takes `n` bytes of memory from the pool: called before what they pay for is made. A read that would pass the pool is refused whole.
    pub(crate) fn take_bytes(&mut self, n: u64) -> Result<(), VsdxError> {
        Ok(self.sheets.take_bytes(n)?)
    }

    /// Takes `n` bytes of memory from the pool as they are, for what is counted from the thing itself (a tree, a picture) and needs no margin.
    pub(crate) fn take_exact(&mut self, n: u64) -> Result<(), VsdxError> {
        Ok(self.sheets.take_exact(n)?)
    }

    /// Gives back the bytes of a tree that is dropped.
    pub(crate) fn give_bytes(&mut self, n: u64) {
        self.sheets.give_bytes(n);
    }

    /// The limits of the tree of a part about to be parsed (`bytes`): the container's, with no more items than the pool can hold at [`TREE_ITEM_BYTES`] each after what
    /// the parse holds besides them is taken out (the room the strings of the tree can take, twice the text, and the copy of it that a part in UTF-16 is decoded into,
    /// [`parse_fixed_bytes`]), so that **a tree that would not fit is refused as it is built and never made** (the bound was 224 an item, the dearest
    /// tree 336, and none of the text was counted). A part whose text alone would not fit is refused before anything is made of it: `too_much_memory`.
    pub(crate) fn xml_limits(&mut self, bytes: &[u8]) -> Result<XmlLimits, VsdxError> {
        let limits = self.xml_limits_settled(bytes)?;
        self.settle_if_binding(bytes, &limits);
        Ok(limits)
    }

    /// The limits of the parse of `bytes` that **a read with no work ahead** has (the pool as it would be once what is set aside for the work ahead is back: `SheetReader::bytes_left_settled`): what the
    /// read is held to and refused by, and what a tree parsed ahead is taken by. It allocates nothing, and **no parse is made by it while the work ahead holds memory** ([`Reader::xml_limits`]).
    pub(crate) fn xml_limits_settled(&self, bytes: &[u8]) -> Result<XmlLimits, VsdxError> {
        self.xml_limits_at(bytes, self.sheets.bytes_left_settled())
    }

    fn xml_limits_at(&self, bytes: &[u8], left: u64) -> Result<XmlLimits, VsdxError> {
        let room = left
            .checked_sub(parse_fixed_bytes(diacraft_opc::xml::parse_size(bytes)))
            .ok_or(VsdxError::Limit { kind: LimitKind::Memory, max: self.limits.max_memory })?;
        let by_pool = usize::try_from(room / TREE_ITEM_BYTES).unwrap_or(usize::MAX);
        Ok(XmlLimits { max_nodes: self.limits.opc.xml.max_nodes.min(by_pool), ..self.limits.opc.xml.clone() })
    }

    /// Ends the work ahead of the read when a parse made within `limits` (what a read with none is held to) could allocate more than the budget can pay beside the memory the work holds: **a parse
    /// allocates by its limits, so it is made only where the memory it may use is free**. A tree has at most one item to two bytes of its part, so a parse within the least of the two limits
    /// cannot reach either: when the pool beside the work ahead has the items for that, nothing is ended and the parse is the same within either.
    pub(crate) fn settle_if_binding(&mut self, bytes: &[u8], limits: &XmlLimits) {
        if self.sheets.lookahead() == 0 {
            return;
        }
        let most_items = (bytes.len() / 2).saturating_add(1);
        let free = self.xml_limits_at(bytes, self.sheets.bytes_left());
        if !free.is_ok_and(|free| free.max_nodes >= limits.max_nodes.min(most_items)) {
            self.sheets.settle_lookahead();
        }
    }

    /// What a parse that failed says: a tree past the items the pool can hold is a read past the pool, not a part that is too complex.
    pub(crate) fn xml_failure(&self, part: &str, error: XmlError, used: &XmlLimits) -> VsdxError {
        if matches!(error, XmlError::TooManyNodes { .. }) && used.max_nodes < self.limits.opc.xml.max_nodes {
            return VsdxError::Limit { kind: LimitKind::Memory, max: self.limits.max_memory };
        }
        VsdxError::Xml { part: part.to_owned(), error }
    }

    /// Takes `kept` bytes of the pool for what a small part (a theme, a property part) keeps of itself, and gives the limits of its tree: **what the pool holds beside this
    /// charge**, as for every parse of this reader. `None` when either does not fit, and then nothing is charged: the part is not read. The tree is let go inside the read
    /// of the part, before anything else is made, so it is bounded and not held.
    pub(crate) fn begin_small_part(&mut self, bytes: &[u8], kept: u64) -> Option<XmlLimits> {
        self.take_bytes(kept).ok()?;
        match self.xml_limits(bytes) {
            Ok(limits) => Some(limits),
            Err(_) => {
                self.give_bytes(diacraft_sheetxml::price::with_margin(kept));
                None
            }
        }
    }

    /// What a failed parse of a relationships part says: as [`Reader::xml_failure`], for the error `opc` gives.
    pub(crate) fn rels_failure(&self, error: OpcError, used: &XmlLimits) -> VsdxError {
        let package_cap = self.limits.opc.xml.max_nodes.min(self.limits.opc.max_package_xml_nodes);
        if matches!(&error, OpcError::Xml { error: XmlError::TooManyNodes { .. }, .. }) && used.max_nodes < package_cap {
            return VsdxError::Limit { kind: LimitKind::Memory, max: self.limits.max_memory };
        }
        VsdxError::from(error)
    }

    /// Takes the bytes of an XML part about to be parsed from the document's total.
    pub(crate) fn take_xml_bytes(&mut self, len: usize) -> Result<(), VsdxError> {
        match self.xml_bytes_left.checked_sub(len as u64) {
            Some(left) => {
                self.xml_bytes_left = left;
                Ok(())
            }
            None => Err(VsdxError::Limit { kind: LimitKind::XmlBytes, max: self.limits.max_xml_bytes }),
        }
    }

    /// Keeps an element and everything in it, whole, as an unknown element of a passthrough, after `typed` typed children. Its items are
    /// taken from the total first.
    pub(crate) fn pass_element(&mut self, into: &mut Passthrough, typed: usize, el: &El) -> Result<(), VsdxError> {
        Ok(self.sheets.pass_element(into, typed, el)?)
    }

    /// Keeps a node that is not an element (text, a comment, an instruction) as an unknown node, unless it is blank text between elements,
    /// which is layout and is not kept.
    pub(crate) fn pass_other(&mut self, into: &mut Passthrough, typed: usize, node: &Node) -> Result<(), VsdxError> {
        Ok(self.sheets.pass_other(into, typed, node)?)
    }
}
