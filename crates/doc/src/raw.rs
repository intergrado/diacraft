//! What the model has no typed place for, kept as the file had it: unknown attributes ([`RawAttrs`]) and unknown or unused
//! elements, comments and processing instructions ([`Passthrough`]).
//!
//! The rule is: an attribute or element the model does not understand survives a save,
//! at its place among its siblings and in its order. The model keeps three things apart:
//!
//! - what it has typed fields for (an `ID`, a `Master`, an `F`), which the reader reads into those fields;
//! - what MS-VSDX calls unused (`OriginalID`, `IsCustomName`, `Icon`, `ViewScale`): not typed, kept here, in file order;
//! - what no one knows (a foreign-namespace attribute, a `Tab` element under `Tabs`, `<?NewValue V?>`): kept here as well.
//!
//! An attribute whose typed field could not hold it (an `ID` of 4294967296) is kept here as the
//! text it was, and the typed field holds the nearest value its type allows: the entry here is the file's truth, and a writer
//! gives it back as it is.
//!
//! These are plain data. The reader of a file applies the caps of (element depth 200, and the size of
//! a part) before it builds them, so a [`RawElement`] is never deeper than a part's XML is allowed to be.

use std::sync::Arc;

/// A list of attribute `(name, value)` pairs in file order, names with their prefix as written. Cheap to clone and empty
/// (a null pointer) for the common case of none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawAttrs(Option<Arc<AttrList>>);

/// Attribute `(name, value)` pairs.
pub type AttrList = Vec<(Arc<str>, Arc<str>)>;

impl RawAttrs {
    /// No attributes.
    pub fn new() -> RawAttrs {
        RawAttrs(None)
    }

    /// The attributes, in this order.
    pub fn from_pairs<N: AsRef<str>, V: AsRef<str>>(pairs: impl IntoIterator<Item = (N, V)>) -> RawAttrs {
        let list: AttrList = pairs.into_iter().map(|(n, v)| (Arc::from(n.as_ref()), Arc::from(v.as_ref()))).collect();
        if list.is_empty() { RawAttrs(None) } else { RawAttrs(Some(Arc::new(list))) }
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|list| list.is_empty())
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |list| list.len())
    }

    /// The pairs, in file order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().flat_map(|list| list.iter()).map(|(n, v)| (&**n, &**v))
    }

    /// The first attribute called `name`, matched exactly (attribute names are case-sensitive in XML).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// The list behind the pointer it is shared by: a copy of these attributes holds the same one, and a count of the memory a model holds counts it once (`Arc::as_ptr`). `None` when
    /// there are none.
    pub fn shared_list(&self) -> Option<&Arc<AttrList>> {
        self.0.as_ref()
    }

    /// Adds an attribute after the others. Copies the list first if another copy of this one shares it.
    pub fn push(&mut self, name: &str, value: &str) {
        let list = self.0.get_or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(list).push((Arc::from(name), Arc::from(value)));
    }
}

/// An element kept whole: its name as written (prefix included), its attributes (the `xmlns` declarations among them) and
/// everything inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawElement {
    pub name: Arc<str>,
    pub attrs: RawAttrs,
    pub children: Vec<RawNode>,
}

/// A node kept as the file had it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawNode {
    Element(RawElement),
    /// Text, exactly: blanks, line breaks and all (`xml:space="preserve"`).
    Text(Arc<str>),
    Comment(Arc<str>),
    /// A processing instruction, such as `<?NewValue V?>`.
    Pi {
        target: Arc<str>,
        data: Arc<str>,
    },
}

/// A node with where it sat: `after` is how many of the typed children of its parent (cells, sections, rows, shapes: as the
/// parent counts them in file order) came before it. A writer that puts the typed children back in order puts this node
/// back after that many of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub after: u32,
    pub node: RawNode,
}

/// The unknown or unused child nodes of one element. Cheap to clone, and empty (a null pointer) for the common case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passthrough(Option<Arc<Vec<Placed>>>);

impl Passthrough {
    /// Nothing.
    pub fn new() -> Passthrough {
        Passthrough(None)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|list| list.is_empty())
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |list| list.len())
    }

    /// The nodes, in file order.
    pub fn iter(&self) -> impl Iterator<Item = &Placed> {
        self.0.iter().flat_map(|list| list.iter())
    }

    /// The list behind the pointer it is shared by (see [`RawAttrs::shared_list`]). `None` when there are none.
    pub fn shared_list(&self) -> Option<&Arc<Vec<Placed>>> {
        self.0.as_ref()
    }

    /// Adds a node after the others. Copies the list first if another copy of this one shares it.
    pub fn push(&mut self, after: u32, node: RawNode) {
        let list = self.0.get_or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(list).push(Placed { after, node });
    }

    /// The nodes to change in place: the list is copied first if another copy of this one shares it (the `DocumentSettings` element, whose
    /// words an edit changes).
    pub fn nodes_mut(&mut self) -> &mut Vec<Placed> {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())))
    }

    /// A passthrough of these nodes, in this order; none at all gives the empty one (a null pointer).
    pub fn from_nodes(nodes: Vec<Placed>) -> Passthrough {
        if nodes.is_empty() { Passthrough(None) } else { Passthrough(Some(Arc::new(nodes))) }
    }
}
