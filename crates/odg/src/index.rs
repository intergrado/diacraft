//! Which element of the file each shape came from (the plan's shared interfaces): a save maps
//! shapes back to the elements of their page through it, and splices an edit into the element and nothing else.
//!
//! **Numbering.** The reader numbers each page's drawing shapes 1, 2, 3, ... in document order, a group before its members and members in
//! order (pre-order), and a shape takes that ordinal as its `ID`, unless its own automatic style carries a DiaCraft id (`diacraft:id`) that no
//! earlier shape on the page carries, in which case it takes that id and the numbering skips it. Every drawing
//! shape element is numbered, those this reader keeps without modelling them (a hyperlink wrapper and what it holds) included, so the numbers
//! of the others do not move when a later reader models more. A new shape takes an `ID` above the
//! page's high-water mark ([`ShapeIndex::next_id`]), which counts every number and every DiaCraft id the page holds.
//!
//! **What the index holds**: for each page of the document, in the order of `Document::pages`, where it came from ([`PageSource`]), the
//! ordinal of each modelled shape's element by `ID`, the ordinals of the elements kept unmodelled, and the high-water mark. An element kept
//! unmodelled has no shape in the model, so a writer keeps its bytes; it is not a deleted shape.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{Document, Page};

/// The element a page of the document came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSource {
    /// The `draw:page` with this ordinal among the drawing's pages, from 0.
    Page(usize),
    /// The `style:master-page` with this ordinal among the master pages, from 0: a master page holding shapes, read as a background page.
    MasterPage(usize),
}

/// The `Master` and `MasterShape` of a shape's link to a master.
pub(crate) type MasterLink = (Option<u32>, Option<u32>);

/// The shapes of one page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageShapes {
    /// Where the page came from.
    pub source: PageSource,
    /// The ordinal of each modelled shape's element, by the shape's `ID`.
    ordinals: HashMap<u32, u32>,
    /// The ordinals of the elements kept unmodelled, in document order.
    kept: Vec<u32>,
    /// Drawing shape elements numbered on the page.
    elements: u32,
    /// The largest `ID` the page holds or reserved.
    high_water: u32,
    /// The `ID`s of the shapes the reader found to be copies: their own style carries a DiaCraft id an earlier shape of the page carries too, so they
    /// are read from their native markup alone, in document order. A save says so in a note.
    copies: Vec<u32>,
    /// The `ID`s of the shapes that carry tier 2: their own style holds attributes of DiaCraft's extension that the reader put over the markup. A save
    /// looks in the style of a shape it edits only for these, so that the edit of a shape with none costs what it did.
    tier2: std::collections::HashSet<u32>,
    /// The keys of the tier-2 items of each shape that tier 3 found stale (the native outputs they rested on changed) and the reader dropped, by the shape's `ID`
    /// A save that rewrites such a shape's extension takes these items out of what the style carries, so that an edit of the shape cannot bring a stale
    /// item back as a current one.
    stale: std::collections::HashMap<u32, Vec<crate::ext::fingerprint::Key>>,
    /// The links to a master that tier 3 found stale and the reader dropped, by the shape's `ID`: the `Master` and `MasterShape` they held.
    stale_links: Vec<(u32, MasterLink)>,
    /// The native outputs of the markup that the tier-2 items of a shape rest on, by the shape's `ID`, as the file had them when it was read: what a save that
    /// writes the shape's extension stamps the fingerprints it lacks from.
    natives: std::collections::HashMap<u32, crate::ext::tier3::Natives>,
    /// The fingerprint of `draw:name` of the shapes whose natives are no more than that (every other shape): a map of its own, so that a drawing of many plain shapes
    /// holds eight bytes for each and not a whole set of natives.
    names: std::collections::HashMap<u32, crate::ext::fingerprint::Fp>,
    /// The page as the model had it when a save made or changed it: what the next save compares the model with, in place of what the file
    /// reads as. A save writes less of a new shape than a read gives it back (a read adds sections, formulas and attributes of its own), so the file
    /// read again is not the model saved; the model saved is.
    saved: Option<SavedPage>,
    /// The route the file holds of each connector whose route a save **kept** (a hop in the model, which the `.odg` has no place for; the route rows),
    /// by the connector's `ID`: its corners on the page, in inches, y up, hops and points on a leg taken out, as bits (`write::route::corners`). The model as saved
    /// (`saved`) holds the route the model has; the file holds this one. The next save compares the model's route with this one, and not with the baseline's, so a route that
    /// lost its hop is written when it is not the file's (the milestone-1 rerun's). A route a save writes has no entry: the file holds the model's.
    kept_routes: Arc<HashMap<u32, Vec<(u64, u64)>>>,
    /// The `ID` the model's page had when this index was made: a save finds the entry of a page of the model by it, since the pages of a model are
    /// added, taken away and put in another order by their `ID`s. `None` for an entry made without one, which is no page's by `ID` (every `u32` is an `ID` a page may
    /// have: a file's document file can give a page `4294967295`, so no value of the number stands for "none").
    page_id: Option<u32>,
    /// The page has a layer set of its own, whose rows it reads in place of the document's: a layer a save declares in the document's set is
    /// no layer of this page's markup. Said by the reader; an entry a save makes says false.
    own_layers: bool,
    /// The page's annotation children (`office:annotation`, or LibreOffice's `officeooo:annotation`) that the reader took as the document's comments, each by its place among
    /// the page's annotation children, ascending: a save that rewrites the page's comments takes these elements out and leaves the others, which the model does not hold.
    comment_places: Vec<u32>,
    /// How many annotation children the page has: the places above are among them.
    comment_total: u32,
}

/// The model's page as a save saw it: held by the pointer it was given, so that two are the same when they are the same page.
#[derive(Clone, Debug)]
pub(crate) struct SavedPage(pub(crate) Arc<Page>);

impl PartialEq for SavedPage {
    fn eq(&self, other: &SavedPage) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SavedPage {}

impl PageShapes {
    pub(crate) fn new(source: PageSource) -> PageShapes {
        PageShapes {
            source,
            ordinals: HashMap::new(),
            kept: Vec::new(),
            elements: 0,
            high_water: 0,
            copies: Vec::new(),
            tier2: Default::default(),
            stale: Default::default(),
            stale_links: Vec::new(),
            natives: Default::default(),
            names: Default::default(),
            saved: None,
            kept_routes: Arc::default(),
            page_id: None,
            own_layers: false,
            comment_places: Vec::new(),
            comment_total: 0,
        }
    }

    /// The places, among the page's annotation children, of those the reader took as comments, and how many annotation children the page has.
    pub fn comments(&self) -> (&[u32], u32) {
        (&self.comment_places, self.comment_total)
    }

    /// Says which of the page's `total` annotation children are the model's comments.
    pub(crate) fn set_comments(&mut self, places: Vec<u32>, total: u32) {
        self.comment_places = places;
        self.comment_total = total;
    }

    /// Whether the page reads the rows of a layer set of its own, and not the document's.
    pub(crate) fn own_layers(&self) -> bool {
        self.own_layers
    }

    /// Says whether the page reads the rows of a layer set of its own.
    pub(crate) fn set_own_layers(&mut self, own: bool) {
        self.own_layers = own;
    }

    /// The `ID` the model's page had when the index was made, `None` for an entry made without one.
    pub fn page_id(&self) -> Option<u32> {
        self.page_id
    }

    /// This entry, for the page of the model whose `ID` is `id`.
    pub(crate) fn with_page_id(mut self, id: u32) -> PageShapes {
        self.page_id = Some(id);
        self
    }

    /// The page as the model had it when a save made or changed the page, if one did: the baseline of the next save.
    pub(crate) fn saved(&self) -> Option<&Arc<Page>> {
        self.saved.as_ref().map(|s| &s.0)
    }

    /// These shapes, with `page` as the model's baseline for them.
    pub(crate) fn with_saved(mut self, page: Arc<Page>) -> PageShapes {
        self.saved = Some(SavedPage(page));
        self
    }

    /// The corners of the route the file holds of the connector `id`, where a save kept it (the field's note): `None` when no save did.
    pub(crate) fn kept_route(&self, id: u32) -> Option<Vec<(f64, f64)>> {
        self.kept_routes.get(&id).map(|corners| corners.iter().map(|(x, y)| (f64::from_bits(*x), f64::from_bits(*y))).collect())
    }

    /// The `ID`s of the connectors whose route a save kept, in no order.
    pub(crate) fn kept_route_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.kept_routes.keys().copied()
    }

    /// These shapes, with `routes` as the corners of the routes the file holds of the connectors a save kept.
    pub(crate) fn with_kept_routes(mut self, routes: HashMap<u32, Vec<(f64, f64)>>) -> PageShapes {
        let bits = |corners: Vec<(f64, f64)>| corners.into_iter().map(|(x, y)| (x.to_bits(), y.to_bits())).collect::<Vec<_>>();
        self.kept_routes = Arc::new(routes.into_iter().map(|(id, corners)| (id, bits(corners))).collect());
        self
    }

    /// A page's shapes from the parts the writer works out after a save: where each modelled shape's element now is, the kept ones, how many
    /// shape elements the page holds, and the high-water mark (which a deleted shape's `ID` stays in, so that it is not given out again).
    pub(crate) fn from_parts(source: PageSource, ordinals: HashMap<u32, u32>, kept: Vec<u32>, elements: u32, high_water: u32) -> PageShapes {
        PageShapes {
            source,
            ordinals,
            kept,
            elements,
            high_water,
            copies: Vec::new(),
            tier2: Default::default(),
            stale: Default::default(),
            stale_links: Vec::new(),
            natives: Default::default(),
            names: Default::default(),
            saved: None,
            kept_routes: Arc::default(),
            page_id: None,
            own_layers: false,
            comment_places: Vec::new(),
            comment_total: 0,
        }
    }

    /// Notes that the shape `id` is a copy: read without the DiaCraft data its style carries, because an earlier shape holds the same id.
    pub(crate) fn note_copy(&mut self, id: u32) {
        self.copies.push(id);
    }

    /// Notes that the shape `id` carries tier 2.
    pub(crate) fn note_tier2(&mut self, id: u32) {
        self.tier2.insert(id);
    }

    /// Notes that tier 3 found items of the shape `id` stale and the reader dropped them (`keys`), and the link to a master among them (`link`).
    pub(crate) fn note_stale(&mut self, id: u32, keys: Vec<crate::ext::fingerprint::Key>, link: Option<MasterLink>) {
        if let Some(link) = link {
            self.stale_links.push((id, link));
        }
        self.stale.entry(id).or_insert(keys);
    }

    /// Notes the native outputs the tier-2 items of the shape `id` rest on, as the file has them.
    pub(crate) fn note_natives(&mut self, id: u32, natives: crate::ext::tier3::Natives) {
        // The first note of an `ID` stands (a copy holds the `ID` of the shape it is a copy of), whichever map it went to.
        if self.natives.contains_key(&id) || self.names.contains_key(&id) {
            return;
        }
        match natives.name {
            Some(name) if natives.is_name_only() => {
                self.names.entry(id).or_insert(name);
            }
            _ => {
                self.natives.entry(id).or_insert(natives);
            }
        }
    }

    /// The native outputs of the shape `id` as the file had them when it was read: those its tier-2 items rest on, and its `draw:name`'s.
    pub(crate) fn natives(&self, id: u32) -> Option<crate::ext::tier3::Natives> {
        match self.natives.get(&id) {
            Some(held) => Some(held.clone()),
            None => self.names.get(&id).map(|name| crate::ext::tier3::Natives { name: Some(*name), ..Default::default() }),
        }
    }

    /// The fingerprint of the formatting of the text of each shape of the page whose natives hold one, by the shape's `ID`: the shapes whose carried text a save stamps with it.
    pub(crate) fn formats(&self) -> Vec<(u32, crate::ext::fingerprint::Fp)> {
        self.natives.iter().filter_map(|(id, n)| n.format.map(|f| (*id, f))).collect()
    }

    /// The keys of the items of the shape `id` that tier 3 found stale when the file was read.
    pub(crate) fn stale(&self, id: u32) -> &[crate::ext::fingerprint::Key] {
        self.stale.get(&id).map_or(&[], Vec::as_slice)
    }

    /// How many shapes of the page had items that tier 3 found stale.
    pub fn stale_shapes(&self) -> usize {
        self.stale.len()
    }

    /// The links to a master that tier 3 found stale, by the shape's `ID`, in document order.
    pub(crate) fn stale_links(&self) -> &[(u32, MasterLink)] {
        &self.stale_links
    }

    /// Whether the shape `id` carried tier 2 when the file was read.
    pub fn has_tier2(&self, id: u32) -> bool {
        self.tier2.contains(&id)
    }

    /// The `ID`s of the shapes read as copies, in document order.
    pub fn copies(&self) -> &[u32] {
        &self.copies
    }

    /// Each modelled shape's element ordinal, by `ID`.
    pub(crate) fn ordinal_map(&self) -> &HashMap<u32, u32> {
        &self.ordinals
    }

    /// Notes an element numbered `ordinal` whose shape has `id`: modelled or kept.
    pub(crate) fn note(&mut self, ordinal: u32, id: u32, modelled: bool) {
        self.elements = self.elements.max(ordinal);
        self.high_water = self.high_water.max(id);
        if modelled {
            self.ordinals.entry(id).or_insert(ordinal);
        } else {
            self.kept.push(ordinal);
        }
    }

    /// Whether the ordinals the page holds, modelled and kept together, are the numbers 1 to the number of elements, each once: every element
    /// the reader numbers is a shape of the model or one kept unmodelled.
    pub(crate) fn ordinals_fit(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        let within = |o: &u32| (1..=self.elements).contains(o);
        self.ordinals.values().chain(self.kept.iter()).all(|o| within(o) && seen.insert(*o)) && seen.len() == self.elements as usize
    }

    /// The ordinal of the element of the shape `id`, from 1 in document order.
    pub fn ordinal(&self, id: u32) -> Option<u32> {
        self.ordinals.get(&id).copied()
    }

    /// How many shapes are modelled.
    pub fn modelled(&self) -> usize {
        self.ordinals.len()
    }

    /// The ordinals of the elements kept without a shape in the model, in document order.
    pub fn kept(&self) -> &[u32] {
        &self.kept
    }

    /// How many drawing shape elements the page holds.
    pub fn elements(&self) -> u32 {
        self.elements
    }

    /// The largest `ID` the page holds or reserved.
    pub fn high_water(&self) -> u32 {
        self.high_water
    }
}

/// Bytes of memory the model costs for each item the reader counts: **the reader's own figure** (`read::ITEM_BYTES`, the bytes an item stands for).
/// It is the engine's hold of an open drawing in the process's ledger ([`ReadCost::model_bytes`]) and, twice over, the save's memory gate ([`ReadCost::save_bytes`]).
/// It was 54, the most a kind was measured at by the growth of the process's resident size (40 to 54), which misses a vector's room that nothing has touched.
/// Counted (`tests/it/memory_counted.rs`, what the model and its index keep, byte by byte and allocation by allocation), by the law the
/// memory tests hold a model to (its bytes, an eighth of them and 24 for each allocation), a connector needed 64.1 bytes an item and a common style
/// 63.9, and the kinds the reader charged too few items for more (a user-defined field 105, a package entry 95, an empty page 88, a paragraph with a span 79, a
/// kept hyperlink's wrapper 70) until the reader charged them what they keep (`read::cost::priced`): at 64 every kind measured is covered.
pub(crate) const ITEM_BYTES: u64 = 64;

/// Bytes of memory a parsed tree costs for each item (element, attribute or text) in it: measured at 210 (`limits.rs`).
pub(crate) const TREE_NODE_BYTES: u64 = 210;

/// What the read that made a model cost, in the units a save's memory estimate is made of: a save holds the
/// caller's model while it reads the file again to compare it with, so it must know what the model weighs without reading the file to find
/// out. The reader sets it on the index it gives, and a save carries it to the index it gives. It is an estimate, in bytes of the memory a
/// read holds ([`ReadCost::model_bytes`] kept, [`ReadCost::transient_bytes`] while it reads), and no more than that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadCost {
    /// Items the model holds ([`crate::ReadWork::items`]).
    pub items: u64,
    /// Bytes of pictures the model holds.
    pub media_bytes: u64,
    /// Items of the largest tree the read parsed, as far as the read's counts say: elements, attributes and events, at most the reader's cap.
    pub tree_nodes: u64,
    /// Bytes of the package the read inflated.
    pub inflated_bytes: u64,
    /// What the model the read made held when it was made, counted from the model itself ([`crate::figures`]): a save counts the model it is
    /// given the same way, and what it has grown by since is charged before the file is read again.
    pub figures: crate::figures::Figures,
}

impl ReadCost {
    /// What the model keeps: its items and its pictures.
    pub fn model_bytes(&self) -> u64 {
        self.items.saturating_mul(ITEM_BYTES).saturating_add(self.media_bytes)
    }

    /// What a read holds beside the model while it runs: the tree it parses and the parts it inflated.
    pub fn transient_bytes(&self) -> u64 {
        self.tree_nodes.saturating_mul(TREE_NODE_BYTES).saturating_add(self.inflated_bytes)
    }

    /// The peak of a save: the caller's model, the model of the file read again, and what that read holds beside it.
    pub fn save_bytes(&self) -> u64 {
        self.model_bytes().saturating_mul(2).saturating_add(self.transient_bytes())
    }
}

/// What a save knew of the parts it made, for a later save to find out that the index it is given is the one that goes with the bytes: the checksum
/// and the size each of `content.xml` and `styles.xml` has in the package's directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub content: (u32, u64),
    pub styles: (u32, u64),
}

/// Where each page's shapes came from. See the [module documentation](self).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeIndex {
    pages: Vec<PageShapes>,
    cost: ReadCost,
    /// The parts of the package a save made this index for; none for an index a read made.
    fingerprint: Option<Fingerprint>,
}

impl ShapeIndex {
    pub(crate) fn push(&mut self, page: PageShapes) {
        self.pages.push(page);
    }

    /// An index of these pages, in the order of the document's, with the cost of the read it stands for.
    pub(crate) fn from_pages(pages: Vec<PageShapes>, cost: ReadCost) -> ShapeIndex {
        ShapeIndex { pages, cost, fingerprint: None }
    }

    /// The parts of the package the save that made this index wrote, when one did.
    pub(crate) fn fingerprint(&self) -> Option<&Fingerprint> {
        self.fingerprint.as_ref()
    }

    /// Notes the parts of the package this index goes with.
    pub(crate) fn with_fingerprint(mut self, fingerprint: Fingerprint) -> ShapeIndex {
        self.fingerprint = Some(fingerprint);
        self
    }

    /// What the read that made the model cost: see [`ReadCost`]. All zeros for an index no read made.
    pub fn read_cost(&self) -> &ReadCost {
        &self.cost
    }

    /// Sets what the read cost.
    pub(crate) fn set_read_cost(&mut self, cost: ReadCost) {
        self.cost = cost;
    }

    /// Records the figures of the model the read made.
    pub(crate) fn set_figures(&mut self, figures: crate::figures::Figures) {
        self.cost.figures = figures;
    }

    /// The shapes of the page at `page` in `Document::pages`.
    pub fn page(&self, page: usize) -> Option<&PageShapes> {
        self.pages.get(page)
    }

    /// The index with its entries in the order of the pages of a model whose `ID`s are `ids`: the entry of each page, found by the `ID` the index recorded for it, and an
    /// entry that holds no shape for a page the index lacks (a page the model added, whose every shape is new). A caller that goes through the index by the place of the
    /// model's pages (the engine's flattening of the shapes a save writes) reads the entry of the page it is looking at after the pages were added, taken away or put
    /// in another order. `None` when the entries already stand as the `ID`s say, and when the index does not say which page each entry is (an entry made without an
    /// `ID`): the index is then read by place, as it always was.
    pub fn aligned_to(&self, ids: &[u32]) -> Option<ShapeIndex> {
        let same = ids.len() == self.pages.len() && self.pages.iter().zip(ids).all(|(entry, id)| entry.page_id() == Some(*id));
        if same || self.pages.iter().any(|entry| entry.page_id().is_none()) {
            return None;
        }
        let mut places: HashMap<u32, usize> = HashMap::with_capacity(self.pages.len());
        for (at, entry) in self.pages.iter().enumerate() {
            if let Some(id) = entry.page_id() {
                places.entry(id).or_insert(at);
            }
        }
        let pages = ids
            .iter()
            .map(|id| match places.get(id).and_then(|at| self.pages.get(*at)) {
                Some(entry) => entry.clone(),
                None => PageShapes::new(PageSource::Page(usize::MAX)).with_page_id(*id),
            })
            .collect();
        Some(ShapeIndex { pages, cost: self.cost, fingerprint: None })
    }

    /// Gives the entry at `at` the `ID` of the page of the model it stands for.
    pub(crate) fn set_page_id(&mut self, at: usize, id: u32) {
        if let Some(page) = self.pages.get_mut(at) {
            page.page_id = Some(id);
        }
    }

    /// **Holds the pages of `doc` as the pages the save was given**, in place of the ones the entries were made with: the model writer splices a copy of its caller's
    /// drawing with what DiaCraft's document file carries taken out of its pages (a VSDX page's relationship, part name, attributes and elements), and its caller saves **its own** drawing next, which
    /// a read of the file gives back whole. Held as the copy, every page of a VSDX drawing saved as an `.odg` was "not that page" to the next save, which wrote it again as a new page, with no edit made.
    /// Each entry that holds a page takes the page of `doc` at its place when that page has the entry's `ID`; any other entry is left as it is. One look at each entry, and no copy.
    pub(crate) fn hold_pages_of(&mut self, doc: &Document) {
        for (entry, page) in self.pages.iter_mut().zip(doc.pages.iter()) {
            if entry.saved.is_some() && entry.page_id == Some(page.id) {
                entry.saved = Some(SavedPage(Arc::clone(page)));
            }
        }
    }

    /// How many pages the index holds: as many as the document.
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// The ordinal of the element of shape `id` on the page at `page`, from 1 in the page's document order.
    pub fn ordinal(&self, page: usize, id: u32) -> Option<u32> {
        self.page(page)?.ordinal(id)
    }

    /// The page's high-water mark: no shape on it has a larger `ID`.
    pub fn high_water(&self, page: usize) -> Option<u32> {
        self.page(page).map(PageShapes::high_water)
    }

    /// The `ID` a new shape on the page takes: one above the high-water mark, or `None` past `u32::MAX` or for a page the index does not
    /// hold.
    pub fn next_id(&self, page: usize) -> Option<u32> {
        self.high_water(page)?.checked_add(1)
    }
}

/// A package as it was read, for a save that splices an edit into it (the plan's shared interfaces; the writer is).
#[derive(Clone, Copy, Debug)]
pub struct OriginalPackage<'a> {
    /// The file's bytes, as read.
    pub bytes: &'a [u8],
    /// The index the read gave, or the one the last save gave.
    pub index: &'a ShapeIndex,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The natives of a shape are kept in the map of their own, or, when they are no more than the fingerprint of `draw:name`, as that fingerprint alone (eight bytes for
    /// each shape of a drawing); either way the shape's natives are what was noted, the first of an `ID` wins, and a shape with none has none.
    #[test]
    fn natives_are_kept_whole_or_as_the_fingerprint_of_the_name_alone() {
        use crate::ext::fingerprint::{name_fp, text_fp};
        use crate::ext::tier3::Natives;
        let name_only = Natives { name: Some(name_fp(Some("a"))), ..Natives::default() };
        let whole = Natives { name: Some(name_fp(Some("b"))), text: Some(text_fp("t")), ..Natives::default() };
        let mut page = PageShapes::new(PageSource::Page(0));
        page.note_natives(1, name_only.clone());
        page.note_natives(2, whole.clone());
        assert_eq!((page.names.len(), page.natives.len()), (1, 1), "the name alone in its map, the rest in the other");
        assert!(page.names.contains_key(&1) && page.natives.contains_key(&2));
        assert_eq!((page.natives(1), page.natives(2), page.natives(3)), (Some(name_only.clone()), Some(whole.clone()), None));
        // The first note of an `ID` stands.
        page.note_natives(1, Natives { name: Some(name_fp(Some("other"))), ..Natives::default() });
        page.note_natives(2, name_only.clone());
        assert_eq!((page.natives(1), page.natives(2)), (Some(name_only), Some(whole)));
        // Natives with no name are not "the name alone".
        let nameless = Natives { text: Some(text_fp("t")), ..Natives::default() };
        page.note_natives(4, nameless.clone());
        assert_eq!((page.names.len(), page.natives(4)), (1, Some(nameless)));
    }

    /// The model's page a save holds is compared by the page it is: two saved pages are the same when they are the one page, and not when they only
    /// read alike (a comparison by value would be a walk of the page, and an index is compared in tests and in the writer's own checks).
    #[test]
    fn a_saved_page_is_the_same_page_only_when_it_is_the_one_the_save_was_given() {
        let page = Arc::new(Page::new(0, "p"));
        let same = PageShapes::new(PageSource::Page(0)).with_saved(Arc::clone(&page));
        assert_eq!(same, PageShapes::new(PageSource::Page(0)).with_saved(Arc::clone(&page)));
        assert_ne!(same, PageShapes::new(PageSource::Page(0)).with_saved(Arc::new(Page::new(0, "p"))), "another page that reads alike");
        assert_ne!(same, PageShapes::new(PageSource::Page(0)), "no saved page is not a saved page");
        assert!(PageShapes::new(PageSource::Page(0)).saved().is_none() && same.saved().is_some_and(|p| Arc::ptr_eq(p, &page)));
    }

    /// An index that does not say which page each entry is (an entry made without an `ID`) is read by place, and one that does is aligned to the pages' `ID`s: the entry of each,
    /// and an empty entry for a page it lacks.
    #[test]
    fn an_index_is_aligned_to_the_ids_of_the_pages_only_when_it_says_which_page_each_entry_is() {
        let entry = |id: Option<u32>| match id {
            Some(id) => PageShapes::new(PageSource::Page(0)).with_page_id(id),
            None => PageShapes::new(PageSource::Page(0)),
        };
        let mut without = ShapeIndex::default();
        without.push(entry(None));
        without.push(entry(Some(5)));
        assert!(without.aligned_to(&[5, 6]).is_none(), "one entry without an ID: by place");
        let mut with = ShapeIndex::default();
        with.push(entry(Some(5)));
        with.push(entry(Some(6)));
        assert!(with.aligned_to(&[5, 6]).is_none(), "already as the IDs say");
        let aligned = with.aligned_to(&[6]).expect("a page taken away");
        assert_eq!((aligned.pages(), aligned.page(0).and_then(PageShapes::page_id)), (1, Some(6)));
        let aligned = with.aligned_to(&[6, 9, 5]).expect("a page added and the order changed");
        let ids: Vec<Option<u32>> = (0..3).map(|k| aligned.page(k).and_then(PageShapes::page_id)).collect();
        assert_eq!(ids, [Some(6), Some(9), Some(5)]);
        // The largest number is an `ID` like any other (it stood for "no ID").
        let mut top = ShapeIndex::default();
        top.push(entry(Some(u32::MAX)));
        assert_eq!(top.page(0).and_then(PageShapes::page_id), Some(u32::MAX));
        assert!(top.aligned_to(&[u32::MAX]).is_none(), "already as the IDs say");
        let aligned = top.aligned_to(&[1, u32::MAX]).expect("a page added before it");
        assert_eq!((0..2).map(|k| aligned.page(k).and_then(PageShapes::page_id)).collect::<Vec<_>>(), [Some(1), Some(u32::MAX)]);
        // Two entries of one `ID`: the first is the page's.
        let mut twice = ShapeIndex::default();
        twice.push(PageShapes::new(PageSource::Page(3)).with_page_id(1));
        twice.push(PageShapes::new(PageSource::Page(4)).with_page_id(1));
        let aligned = twice.aligned_to(&[1, 2]).expect("another page");
        assert_eq!(aligned.page(0).map(|p| p.source), Some(PageSource::Page(3)));
    }

    /// The corners of the route the file holds of a connector a save kept, by the connector's `ID`: kept exactly (bits, so a value that is no
    /// round number comes back as it went in), none for a connector with none, replaced as a whole by another set, and carried by a clone of the page.
    #[test]
    fn the_routes_a_save_kept_are_held_by_connector_exactly_and_go_with_a_clone_of_the_page() {
        let odd = (0.1 + 0.2, std::f64::consts::PI);
        let page = PageShapes::new(PageSource::Page(0));
        assert_eq!((page.kept_route(1), page.kept_route_ids().count()), (None, 0));
        let page = page.with_kept_routes(HashMap::from([(1, vec![odd, (-0.0, 1.0e-300)]), (7, vec![(2.0, 3.0)])]));
        assert_eq!(page.kept_route(1), Some(vec![odd, (-0.0, 1.0e-300)]));
        assert_eq!(page.kept_route(7), Some(vec![(2.0, 3.0)]));
        assert_eq!(page.kept_route(2), None);
        let mut ids: Vec<u32> = page.kept_route_ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 7]);
        assert_eq!(page.clone().kept_route(7), Some(vec![(2.0, 3.0)]), "a clone holds them");
        assert_eq!(page, page.clone());
        let other = page.clone().with_kept_routes(HashMap::from([(7, vec![(2.0, 3.0)])]));
        assert_ne!(other, page, "a page that holds other routes is another page");
        assert_eq!(other.kept_route(1), None, "a set replaces the one before it");
    }
}
