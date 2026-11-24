//! What the model has no field for in an element the reader types, and holding it.
//!
//! The model holds an element's attributes, comments and unknown children where it has a place for them (`raw`, `Passthrough`, a typed field
//! that reproduces the text). An element the reader reads into a typed place with no field for the rest (`Colors`, `ColorEntry`, `Rel`,
//! `Shapes`, `Text`, `Connects`, a part's root, ...) used to drop a foreign attribute, a namespace declaration, a comment or a child, and to
//! count each under a class word. **Each is held** in the `vsdx` side of the extension slot of the object the element belongs to
//! (`Ext`, `ExtData::vsdx`), under the element's local name: a [`Kept`] of its foreign attributes (the declarations among them) and of its comments,
//! instructions, text and child elements, each after as many of the element's typed children as came before it (`Placed::after`, as everywhere else in
//! the model). A colour entry's key adds its `IX` (`ColorEntry/24`).
//!
//! **Which object owns which place.**
//!
//! | The element | Held on | Under the key |
//! |---|---|---|
//! | the document part's root, `Colors`, `ColorEntry`, `FaceNames`, `StyleSheets` | the document | `VisioDocument`, `Colors`, `ColorEntry/<IX>`, `FaceNames`, `StyleSheets` |
//! | the roots of the pages index and the masters index | the document | `Pages`, `Masters` |
//! | `FaceName` | the font's own slot | `FaceName` |
//! | the `Rel` of a page or master row | the page or master | `Rel` |
//! | the root of a page or master part, its `Shapes` and its `Connects` | the page or master | `PageContents` or `MasterContents`, `Shapes`, `Connects` |
//! | the `Shapes` of a group | the group's shape | `Shapes` |
//! | `Text` (its attributes: its parts are its comments and children) | the text | `Text` |
//! | the `Rel` of a `ForeignData` | the `ForeignData` | `Rel` |
//! | `Connect` (its attributes are its `raw`) | the connect | `Connect` |
//!
//! **What the file was spelt as, where the model holds a field and not the spelling** (lossless means the same content): a row of the pages index or the masters index that had no
//! `PageSheet` holds an entry `NoPageSheet` (empty: its presence says it), a document part that had no `FaceNames` holds `NoFaceNames` on the document, and a row of
//! the pages index that had no `Name` holds `NoName` (the model's name is then the `NameU`, and a writer writes no `Name` while it still is); a `ForeignData` whose text came
//! before its `Rel` holds `TextFirst`; a shape or a style whose own kept nodes and whose
//! sheet's kept nodes interleave holds `KeptOrder` (`order`: one letter for each, `S` for the element's and `H` for the sheet's, in the order of the file); a part in the namespace of 2011 holds its
//! `xmlns` in the entry of its root (the standard declaration is that of 2012). The property parts hold, in `VsdxHeld::props_rest`, an entry `coreProperties/{field}` or `Properties/{field}` for each
//! field element that has attributes or nodes beside its text (the entry of `created` and `modified` is held when empty too: a time with no `xsi:type`), `coreProperties/-title` for a core part with
//! no title, `{root}/-order` for fields that are not in the order a writer makes them in, and `-absent/{type}` (`core-properties`, `extended-properties`,
//! `custom-properties`) for a property part the package's relationships name and the package does not hold.
//!
//! A key can come up twice on one owner only for a colour entry whose `IX` is used twice: the entries are held in the order the file had them.
//!
//! **What is not held.** The standard declarations of a part's root (the default namespace of the vocabulary and `r` for the relationships namespace) are
//! not: every writer regenerates them. Blank text between elements is layout, as everywhere (`passthrough.rs`).
//!
//! **Charged before it is kept.** Every attribute, node and slot is taken from the read's pool before it is made, like everything the reader keeps
//! (`cost.rs`); a file past the pool is refused in words.

use std::sync::Arc;

use diacraft_doc::{Ext, Kept};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::price;

use crate::cost;
use crate::error::VsdxError;
use crate::ns::{NS_REL, Scope, ScopeExt};
use crate::reader::Reader;

/// Whether a root's attribute is one of the standard declarations every writer regenerates: the default namespace of the vocabulary **of 2012** (the one a writer makes), and `r` for the
/// relationships namespace. **A part in the namespace of 2011 holds its declaration** (`xmlns` in the `raw` of its `vsdx` entry), so that a writer gives the part back in the namespace it was in.
pub(crate) fn is_standard_declaration(name: &str, value: &str) -> bool {
    if name == "xmlns" {
        return matches!(crate::ns::ns_of_uri(value), crate::ns::Ns::Main2012);
    }
    name.strip_prefix("xmlns:") == Some("r") && value == NS_REL
}

/// The order in which the kept nodes of a shape or a style came: those of the element itself (`S`) and those of its sheet (`H`), in the order of the file. The two lists are placed by the
/// number of typed children each node followed, and a node of each that followed the same number was before or after the other: this says which.
#[derive(Default)]
pub(crate) struct KeptOrder {
    order: String,
    seen: (usize, usize),
}

impl KeptOrder {
    /// Notes what the two lists hold now: what grew since the last call came, in this order.
    pub(crate) fn note(&mut self, element: usize, sheet: usize) {
        for _ in self.seen.0..element {
            self.order.push('S');
        }
        for _ in self.seen.1..sheet {
            self.order.push('H');
        }
        self.seen = (element, sheet);
    }

    /// The order, when both lists hold nodes and so can interleave.
    fn interleaved(self) -> Option<String> {
        (self.seen.0 > 0 && self.seen.1 > 0).then_some(self.order)
    }
}

impl Reader<'_> {
    /// Holds the attributes of `el` that `held` does not name, the declarations among them, in `kept.raw`. **The items of the attributes are the caller's**
    /// (it took them with the element's own); `items` says to take them here, for an element whose items nothing took (a part's root).
    pub(crate) fn hold_attrs(&mut self, el: &El, kept: &mut Kept, held: impl Fn(&str) -> bool, items: bool) -> Result<(), VsdxError> {
        for (name, value) in &el.attrs {
            if held(name) {
                continue;
            }
            if items {
                self.take_items(1)?;
            }
            self.take_bytes(price::raw_attr_bytes(kept.raw.len(), name.len(), value.len()))?;
            kept.raw.push(name, value);
        }
        Ok(())
    }

    /// Holds what the root of a part has that no field of the model has: its attributes but the standard declarations. Its children are the caller's, which
    /// knows which it models and where each of the others goes.
    pub(crate) fn hold_root(&mut self, root: &El) -> Result<Kept, VsdxError> {
        let mut kept = Kept::default();
        for (name, value) in &root.attrs {
            if is_standard_declaration(name, value) {
                continue;
            }
            self.take_items(1)?;
            self.take_bytes(price::raw_attr_bytes(kept.raw.len(), name.len(), value.len()))?;
            kept.raw.push(name, value);
        }
        Ok(kept)
    }

    /// Holds every child of `el` that is not a blank run of text, after no typed child: for an element whose children the model does not read at all.
    pub(crate) fn hold_children(&mut self, el: &El, kept: &mut Kept) -> Result<(), VsdxError> {
        for node in &el.children {
            match node {
                Node::El(child) => self.pass_element(&mut kept.kept, 0, child)?,
                other => self.pass_other(&mut kept.kept, 0, other)?,
            }
        }
        Ok(())
    }

    /// The `Rel` of a page, a master or a `ForeignData`: its relationship id is held (in the field that says it); its other attributes, its declarations and
    /// its children are held here.
    pub(crate) fn hold_rel(&mut self, rel: &El, sc: &Scope) -> Result<Kept, VsdxError> {
        let mut kept = Kept::default();
        self.hold_attrs(rel, &mut kept, |name| sc.is_rel_id_attr(name), false)?;
        self.hold_children(rel, &mut kept)?;
        Ok(kept)
    }

    /// Puts `kept` in the `vsdx` side of `slot` under `key` **whether or not it holds anything**: an entry that says a thing was so (a `PageSheet` that was not there, a text that came before a `Rel`).
    pub(crate) fn put_mark<T: Default + Clone + PartialEq>(&mut self, slot: &mut Ext<T>, key: &str, kept: Kept) -> Result<(), VsdxError> {
        let have = slot.get().map_or(0, |data| data.vsdx.0.len());
        let record = if slot.is_empty() { cost::ext_data_bytes::<T>() } else { 0 };
        self.take_bytes(cost::rest_entry_bytes(have, key.len()).saturating_add(record))?;
        slot.make_mut().vsdx.0.push((Arc::from(key), kept));
        Ok(())
    }

    /// Puts the order the kept nodes of an element and of its sheet came in (`KeptOrder`) in `slot`, when they interleave.
    pub(crate) fn put_order<T: Default + Clone + PartialEq>(&mut self, slot: &mut Ext<T>, order: KeptOrder) -> Result<(), VsdxError> {
        let Some(order) = order.interleaved() else { return Ok(()) };
        let mut kept = Kept::default();
        self.take_items(1)?;
        self.take_bytes(price::raw_attr_bytes(0, "order".len(), order.len()))?;
        kept.raw.push("order", &order);
        self.put_mark(slot, "KeptOrder", kept)
    }

    /// Puts `kept` in the `vsdx` side of `slot` under `key`, if it holds anything. The record of the slot is made at the first entry, and both are
    /// charged before they are made.
    pub(crate) fn put_rest<T: Default + Clone + PartialEq>(&mut self, slot: &mut Ext<T>, key: &str, kept: Kept) -> Result<(), VsdxError> {
        if kept.raw.is_empty() && kept.kept.is_empty() {
            return Ok(());
        }
        let have = slot.get().map_or(0, |data| data.vsdx.0.len());
        let record = if slot.is_empty() { cost::ext_data_bytes::<T>() } else { 0 };
        self.take_bytes(cost::rest_entry_bytes(have, key.len()).saturating_add(record))?;
        slot.make_mut().vsdx.0.push((Arc::from(key), kept));
        Ok(())
    }

    /// As [`Reader::put_rest`], into a list that is not in a slot yet (a part's, shared by every row that names the part).
    pub(crate) fn push_rest(&mut self, list: &mut Vec<(Arc<str>, Kept)>, key: &str, kept: Kept) -> Result<(), VsdxError> {
        if kept.raw.is_empty() && kept.kept.is_empty() {
            return Ok(());
        }
        self.take_bytes(cost::rest_entry_bytes(list.len(), key.len()))?;
        list.push((Arc::from(key), kept));
        Ok(())
    }

    /// Puts a part's list (`Parsed::ext`) into the slot of a page or master that names the part, charging the slot's record and the list, which is a copy of its
    /// own (the entries are shared).
    pub(crate) fn attach_rest<T: Default + Clone + PartialEq>(&mut self, slot: &mut Ext<T>, list: &[(Arc<str>, Kept)]) -> Result<(), VsdxError> {
        if list.is_empty() {
            return Ok(());
        }
        let keys = list.iter().fold(0u64, |sum, (key, _)| sum.saturating_add(price::arc_str(key.len())));
        let entries = price::heap(list.len().saturating_mul(std::mem::size_of::<(Arc<str>, Kept)>()));
        self.take_bytes(cost::ext_data_bytes::<T>().saturating_add(entries).saturating_add(keys))?;
        slot.make_mut().vsdx.0.extend(list.iter().cloned());
        Ok(())
    }
}

/// The largest figures of the XML trees the reader parsed, for the report.
#[derive(Default, Clone, Copy)]
pub(crate) struct XmlMetrics {
    pub items: usize,
    pub depth: usize,
    pub attr_bytes: usize,
    /// The most bytes the tree of one part held (`tree_bytes`).
    pub tree_bytes: u64,
}

/// The bytes of memory a tree holds: each element's node and the allocations behind it (its name, its list of attributes and each name and value in it, its list
/// of children, and each text, comment and instruction among them), at the capacity each has and as the allocator hands them out. **The root is counted too.**
/// The container's parser makes a list of attributes with room for four at its first, so an element of one attribute holds the room of four, and a list of children
/// has room for what doubling left it: this is what the tree holds and not what an average item costs. No recursion.
pub(crate) fn tree_bytes(root: &El) -> u64 {
    use diacraft_sheetxml::model::cost::chunk;
    // An allocation of a string or a list: nothing when it has no room at all.
    fn heap(capacity: usize, size: usize) -> u64 {
        if capacity == 0 { 0 } else { chunk(capacity.saturating_mul(size)) as u64 }
    }
    let mut total = std::mem::size_of::<El>() as u64;
    let mut stack: Vec<&El> = vec![root];
    while let Some(el) = stack.pop() {
        total = total.saturating_add(heap(el.name.capacity(), 1)).saturating_add(heap(el.attrs.capacity(), std::mem::size_of::<(String, String)>()));
        for (k, v) in &el.attrs {
            total = total.saturating_add(heap(k.capacity(), 1)).saturating_add(heap(v.capacity(), 1));
        }
        total = total.saturating_add(heap(el.children.capacity(), std::mem::size_of::<Node>()));
        for node in &el.children {
            let more = match node {
                Node::El(child) => {
                    stack.push(child);
                    0
                }
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => heap(t.capacity(), 1),
                Node::Pi { target, data } => heap(target.capacity(), 1).saturating_add(heap(data.capacity(), 1)),
            };
            total = total.saturating_add(more);
        }
    }
    total
}

/// The figures of one XML tree that [`XmlMetrics`] keeps the largest of: a pure function of the tree, so that they can be taken on another thread than the read's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeFigures {
    pub items: usize,
    pub depth: usize,
    pub attr_bytes: usize,
    /// The bytes the tree holds ([`tree_bytes`]).
    pub bytes: u64,
}

impl TreeFigures {
    /// The figures of the tree `root`: its items as `opc` counts them, its depth and its longest attribute, and the bytes it holds. No recursion.
    pub(crate) fn of(root: &El) -> TreeFigures {
        let (mut items, mut deepest, mut longest) = (0usize, 0usize, 0usize);
        let mut stack: Vec<(&El, usize)> = vec![(root, 1)];
        while let Some((el, depth)) = stack.pop() {
            items = items.saturating_add(1);
            deepest = deepest.max(depth);
            for (k, v) in &el.attrs {
                items = items.saturating_add(1);
                longest = longest.max(k.len()).max(v.len());
            }
            for node in &el.children {
                match node {
                    Node::El(child) => stack.push((child, depth + 1)),
                    _ => items = items.saturating_add(1),
                }
            }
        }
        TreeFigures { items, depth: deepest, attr_bytes: longest, bytes: tree_bytes(root) }
    }
}

impl XmlMetrics {
    /// Folds in the tree `root`: its items as `opc` counts them, its depth and its longest attribute. No recursion. The bytes it holds are given back.
    pub(crate) fn note(&mut self, root: &El) -> u64 {
        self.merge(&TreeFigures::of(root))
    }

    /// Folds in the figures of a tree taken already ([`TreeFigures::of`]), and gives the bytes it holds.
    pub(crate) fn merge(&mut self, figures: &TreeFigures) -> u64 {
        self.items = self.items.max(figures.items);
        self.depth = self.depth.max(figures.depth);
        self.attr_bytes = self.attr_bytes.max(figures.attr_bytes);
        self.tree_bytes = self.tree_bytes.max(figures.bytes);
        figures.bytes
    }
}

/// A part name for a message or a `detail`, when it is one the format or the package structure names (a part reached by a relationship from the
/// document, or by a fixed name), and its role when it might be anything a person typed: **a part name is package structure, not content, and
/// `detail` carries it only when it holds nothing but letters, digits and `/ . _ -` and is short**; any other name is called by its role.
pub fn part_label(role: &'static str, name: &str) -> Arc<str> {
    const MOST: usize = 80;
    let plain = !name.is_empty()
        && name.len() <= MOST
        && name.starts_with('/')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'));
    if plain { Arc::from(name) } else { Arc::from(role) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_sheetxml::model::cost::chunk;

    fn string(text: &str, capacity: usize) -> String {
        let mut s = String::with_capacity(capacity);
        s.push_str(text);
        s
    }

    #[test]
    fn the_bytes_of_a_tree_are_its_nodes_and_every_allocation_behind_them_at_the_capacity_each_has() {
        let c = |n: usize| chunk(n) as u64;
        // A bare element: the node, and the name's allocation.
        let bare = El { name: "a".into(), attrs: vec![], children: vec![] };
        assert_eq!(tree_bytes(&bare), std::mem::size_of::<El>() as u64 + c(1));
        // An element with no name has no allocation for it.
        let nameless = El { name: String::new(), attrs: vec![], children: vec![] };
        assert_eq!(tree_bytes(&nameless), std::mem::size_of::<El>() as u64);
        // One attribute in a list with room for four (as the parser makes it), a name of 40 bytes of capacity, and a value of 3.
        let mut attrs: Vec<(String, String)> = Vec::with_capacity(4);
        attrs.push((string("k", 40), string("vvv", 3)));
        let root = El { name: string("root", 4), attrs, children: Vec::with_capacity(0) };
        let expected = std::mem::size_of::<El>() as u64 + c(4) + c(4 * std::mem::size_of::<(String, String)>()) + c(40) + c(3);
        assert_eq!(tree_bytes(&root), expected);
        // Children: the list holds the room it has (here for eight), and a text, a comment, an instruction and an element in it are each allocations.
        let mut children: Vec<Node> = Vec::with_capacity(8);
        children.push(Node::Text(string("t", 1)));
        children.push(Node::CData(string("cd", 2)));
        children.push(Node::Comment(string("c", 30)));
        children.push(Node::Pi { target: string("x", 1), data: string("y", 1) });
        children.push(Node::El(El { name: "b".into(), attrs: vec![], children: vec![] }));
        let root = El { name: "r".into(), attrs: vec![], children };
        // The root's node and name, the list of eight nodes, the allocations of the text, the CDATA, the comment and the two strings of the instruction, and the name of
        // the element among them (its node is a slot of the list).
        let expected = std::mem::size_of::<El>() as u64 + c(1) + c(8 * std::mem::size_of::<Node>()) + c(1) + c(2) + c(30) + c(1) + c(1) + c(1);
        assert_eq!(tree_bytes(&root), expected);
    }

    #[test]
    fn the_largest_tree_is_noted_with_its_items_its_depth_and_its_longest_attribute_and_the_bytes_come_back() {
        let leaf = El { name: "b".into(), attrs: vec![("k".into(), "0123456789".into())], children: vec![] };
        let root = El { name: "a".into(), attrs: vec![("x".into(), "y".into())], children: vec![Node::El(leaf), Node::Text("t".into())] };
        let mut metrics = XmlMetrics::default();
        let bytes = metrics.note(&root);
        assert_eq!(bytes, tree_bytes(&root));
        // Elements `a` and `b`, their two attributes and the text: five items, two levels, a value of ten bytes.
        assert_eq!((metrics.items, metrics.depth, metrics.attr_bytes, metrics.tree_bytes), (5, 2, 10, bytes));
        // A smaller tree does not lower what the largest was.
        let small = El { name: "a".into(), attrs: vec![], children: vec![] };
        let smaller = metrics.note(&small);
        assert!(smaller < bytes);
        assert_eq!((metrics.items, metrics.depth, metrics.attr_bytes, metrics.tree_bytes), (5, 2, 10, bytes));
    }

    /// The structure of `doc`'s tree beyond the characters in it, for each item: what [`tree_bytes`] says less twice the text the parse reads (the strings may have grown by
    /// doubling), over the items the container counts. [`TREE_ITEM_BYTES`] is what no document makes more than.
    fn dearest_item(doc: &[u8]) -> (u64, u64, u64) {
        let root = diacraft_opc::xml::parse(doc).unwrap();
        let mut metrics = XmlMetrics::default();
        let bytes = metrics.note(&root);
        let text = diacraft_opc::xml::parse_size(doc).text_bytes;
        let structure = bytes.saturating_sub(2 * text);
        (structure, metrics.items as u64, structure.div_ceil(metrics.items as u64))
    }

    fn many(piece: &str, n: usize) -> String {
        piece.repeat(n)
    }

    #[test]
    fn no_tree_costs_more_for_an_item_than_the_price_a_parse_is_bounded_by() {
        let chain = |n: usize, open: &str, close: &str, bottom: &str| format!("{}{bottom}{}", open.repeat(n), close.repeat(n));
        let docs: Vec<(&str, String)> = vec![
            // An element that has one child holds the room of four: a chain of them, 190 deep, many times in a root (the dearest tree of elements).
            ("chains of 190 elements", format!("<r>{}</r>", many(&chain(190, "<a>", "</a>", ""), 300))),
            ("chains of 195 elements", format!("<r>{}</r>", many(&chain(195, "<a>", "</a>", ""), 300))),
            ("one chain of 195 elements", format!("<r>{}</r>", chain(195, "<a>", "</a>", ""))),
            ("chains that end in a text", format!("<r>{}</r>", many(&chain(190, "<a>", "</a>", "x"), 300))),
            ("chains that end in a comment", format!("<r>{}</r>", many(&chain(190, "<a>", "</a>", "<!--c-->"), 300))),
            ("chains that end in an instruction", format!("<r>{}</r>", many(&chain(190, "<a>", "</a>", "<?p d?>"), 300))),
            ("chains that end in a CDATA section", format!("<r>{}</r>", many(&chain(190, "<a>", "</a>", "<![CDATA[x]]>"), 300))),
            ("chains of elements of one attribute each", format!("<r>{}</r>", many(&chain(190, "<a k=\"v\">", "</a>", ""), 300))),
            ("chains of elements of two attributes each", format!("<r>{}</r>", many(&chain(190, "<a k=\"v\" l=\"w\">", "</a>", ""), 300))),
            ("elements that hold one instruction", format!("<r>{}</r>", many("<a><?p d?></a>", 20_000))),
            ("elements that hold one text", format!("<r>{}</r>", many("<a>x</a>", 20_000))),
            ("elements that hold one comment", format!("<r>{}</r>", many("<a><!--c--></a>", 20_000))),
            ("empty elements", format!("<r>{}</r>", many("<z/>", 50_000))),
            ("empty elements of one attribute", format!("<r>{}</r>", many("<z k=\"v\"/>", 50_000))),
            ("empty elements of five attributes", format!("<r>{}</r>", many("<z a=\"1\" b=\"2\" c=\"3\" d=\"4\" e=\"5\"/>", 20_000))),
            (
                "empty elements of nine attributes",
                format!("<r>{}</r>", many("<z a=\"1\" b=\"2\" c=\"3\" d=\"4\" e=\"5\" f=\"6\" g=\"7\" h=\"8\" i=\"9\"/>", 10_000)),
            ),
            ("one element of 20,000 attributes", format!("<r {}/>", (0..20_000).map(|i| format!("k{i}=\"v\"")).collect::<Vec<_>>().join(" "))),
            ("a text that is many pieces and references", format!("<r>{}</r>", many("ab&amp;", 50_000))),
            ("texts between elements", format!("<r>{}</r>", many("t<z/>", 30_000))),
            ("two elements, then three, then five", format!("<r>{}</r>", many("<a><b/><c/></a><d><e/><f/><g/></d>", 5_000))),
            ("a root of four children, each of two", format!("<r>{}</r>", many("<a><b/><b/></a>", 4))),
            ("a lone root", "<r/>".to_owned()),
        ];
        let mut dearest = 0;
        for (name, doc) in &docs {
            let (structure, items, per_item) = dearest_item(doc.as_bytes());
            eprintln!("[tree] {name}: {items} items, {structure} bytes beyond the text, {per_item} an item");
            assert!(per_item <= crate::cost::TREE_ITEM_BYTES, "{name}: {per_item} bytes an item, the bound is {}", crate::cost::TREE_ITEM_BYTES);
            dearest = dearest.max(per_item);
        }
        // The price is the dearest a tree can be: it was not far over the dearest here (322, the chains: 335 with the text counted once) when the container's lists grew by
        // doubling. They grow by an eighth now (`diacraft_container::xml`: an element's list of children is made tight), so a tree costs about a third of it
        // (110 an item at the dearest here) and the price is an upper bound with room: a bound that is over by that is retuned by the pool's own arithmetic, not by this test.
        assert!(
            dearest > 0 && dearest <= crate::cost::TREE_ITEM_BYTES,
            "the dearest tree here is {dearest} bytes an item, the bound is {}",
            crate::cost::TREE_ITEM_BYTES
        );
    }

    #[test]
    fn a_very_deep_tree_is_walked_without_recursion() {
        let mut node = El { name: "z".into(), attrs: vec![], children: vec![] };
        for _ in 0..100_000 {
            node = El { name: "z".into(), attrs: vec![], children: vec![Node::El(node)] };
        }
        assert!(tree_bytes(&node) > 100_000 * std::mem::size_of::<El>() as u64);
        let mut metrics = XmlMetrics::default();
        metrics.note(&node);
        assert_eq!((metrics.items, metrics.depth), (100_001, 100_001));
        // Dropping the tree recurses once for each level, as the container's own parse depth cap (512) keeps from ever being a problem: leave it be.
        std::mem::forget(node);
    }
}
