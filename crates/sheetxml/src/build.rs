//! The model as a tree: the inverse of what the reader does, for elements that are new.
//!
//! **The attribute lists are the inverse of the reader's.** [`section_attrs`], [`row_attrs`] and the rest give, for an element of the model,
//! the attributes the reader would have read it from: the typed fields as text, and the attributes kept in `raw`, **where `raw` wins** (an
//! `ID` of `007` is kept in `raw` as it was, and the typed field holds the number; `passthrough.rs`). The patcher uses the same lists to see
//! which attribute of an element the model changed, so a reader and a writer cannot disagree about what an attribute is.
//!
//! **Order.** New elements are written in the order of R18: `Cell`, then `Section`; cells in the model are kept by name, so they come out in
//! the order of their names. Unknown nodes (the model's `Passthrough`) are put back after as many of the typed children as they followed
//! (`Placed::after`). Attributes come typed first, in the order of the schema, then the rest of `raw` in the order it was read.
//!
//! **Cells are written as the strings held**: `V` is the text of the value, `F` the text of the formula or its marker, `U` and `E`
//! verbatim. Nothing is parsed and printed again.
//!
//! **Names carry no prefix.** An element is made as `Cell`, `Row`, `Section`; [`with_prefix`] gives the elements of a made tree the prefix of
//! the part they are put in.

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Cell, Cells, Passthrough, Placed, RawAttrs, Row, Section, Sheet};

use crate::passthrough::node_to_tree;

/// Attributes as `(name, value)`, in order.
pub type Attrs = Vec<(String, String)>;

/// Builds an attribute list that the reader would read back as the same fields.
pub struct AttrBuilder<'a> {
    out: Attrs,
    raws: &'a [&'a RawAttrs],
}

impl<'a> AttrBuilder<'a> {
    /// A builder for an element whose unknown attributes are in `raws`, in the order they were read; the first that holds a name wins.
    #[must_use]
    pub fn new(raws: &'a [&'a RawAttrs]) -> Self {
        AttrBuilder { out: Vec::new(), raws }
    }

    /// The attribute `name`: the text kept in `raw` if there is one (it wins), else `value` if there is one.
    pub fn typed(&mut self, name: &str, value: Option<String>) {
        if let Some(raw) = self.raws.iter().find_map(|r| r.get(name)) {
            self.out.push((name.to_owned(), raw.to_owned()));
        } else if let Some(value) = value {
            self.out.push((name.to_owned(), value));
        }
    }

    /// The links to styles (`LineStyle`, `FillStyle`, `TextStyle`) of a sheet.
    pub fn links(&mut self, sheet: &Sheet) {
        self.typed("LineStyle", sheet.links.line.map(|n| n.to_string()));
        self.typed("FillStyle", sheet.links.fill.map(|n| n.to_string()));
        self.typed("TextStyle", sheet.links.text.map(|n| n.to_string()));
    }

    /// Every attribute of `raw` the typed ones did not name, in the order they were read.
    #[must_use]
    pub fn finish(mut self) -> Attrs {
        for raw in self.raws {
            for (k, v) in raw.iter() {
                if !self.out.iter().any(|(name, _)| name == k) {
                    self.out.push((k.to_owned(), v.to_owned()));
                }
            }
        }
        self.out
    }
}

/// A flag as its attribute: `1` when on, no attribute when off (what the reader's `read_flag` gives back).
#[must_use]
pub fn flag(on: bool) -> Option<String> {
    on.then(|| "1".to_owned())
}

/// A held text as an attribute value.
#[must_use]
pub fn text(value: &Option<std::sync::Arc<str>>) -> Option<String> {
    value.as_deref().map(str::to_owned)
}

/// The attributes of an element that is only a sheet (`PageSheet`, `DocumentSheet`): its links and the rest.
#[must_use]
pub fn sheet_only_attrs(sheet: &Sheet) -> Attrs {
    let raws = [&sheet.raw];
    let mut b = AttrBuilder::new(&raws);
    b.links(sheet);
    b.finish()
}

/// A `Section`'s attributes.
#[must_use]
pub fn section_attrs(s: &Section) -> Attrs {
    let raws = [&s.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("N", Some(s.name.as_str().to_owned()));
    b.typed("IX", s.ix.map(|n| n.to_string()).filter(|_| s.name == "Geometry"));
    b.typed("Del", flag(s.del));
    b.finish()
}

/// A `Row`'s attributes.
#[must_use]
pub fn row_attrs(r: &Row) -> Attrs {
    let raws = [&r.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("N", text(&r.name));
    b.typed("LocalName", text(&r.local_name));
    b.typed("T", text(&r.row_type));
    b.typed("IX", r.ix.map(|n| n.to_string()));
    b.typed("Del", flag(r.del));
    b.finish()
}

/// A `Cell`'s attributes but its name: `V`, `U`, `E`, `F`, then the rest.
#[must_use]
pub fn cell_attrs(c: &Cell) -> Attrs {
    let raws = [&c.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("V", c.value.as_ref().map(|v| v.text().to_owned()));
    b.typed("U", text(&c.unit));
    b.typed("E", text(&c.error));
    b.typed("F", c.formula.text().map(str::to_owned));
    b.finish()
}

// ------------------------------------------------------------------------------------------------------------------ elements

/// An element with `name` (as it is to be written, prefix and all), `attrs` and `children`.
#[must_use]
pub fn element(name: &str, attrs: Attrs, children: Vec<Node>) -> El {
    El { name: name.to_owned(), attrs, children }
}

/// `typed` children with the unknown nodes of `passthrough` put back after as many of them as they followed. A node that followed more
/// than there are goes last.
#[must_use]
pub fn weave(typed: Vec<Node>, passthrough: &[&Passthrough]) -> Vec<Node> {
    let mut extra: Vec<(usize, Node)> = Vec::new();
    for list in passthrough {
        for Placed { after, node } in list.iter() {
            extra.push((usize::try_from(*after).unwrap_or(usize::MAX), node_to_tree(node)));
        }
    }
    place(typed, extra)
}

/// [`weave`] of the two lists of one element (its own and its sheet's) **in the order the nodes of each came**: `order` is one letter for each node in the order of the file, `S` for the next of
/// `own` and `H` for the next of `sheet`. Two nodes that followed the same number of typed children are put in that order, and what `order` does not say (a list that grew since) comes after, `own`
/// first. The same as [`weave`] when the two interleave in no way.
#[must_use]
pub fn weave_ordered(typed: Vec<Node>, own: &Passthrough, sheet: &Passthrough, order: &str) -> Vec<Node> {
    let tree = |placed: &Placed| (usize::try_from(placed.after).unwrap_or(usize::MAX), node_to_tree(&placed.node));
    let (mut own, mut sheet) = (own.iter(), sheet.iter());
    let mut extra: Vec<(usize, Node)> = Vec::new();
    for letter in order.chars() {
        let next = if letter == 'H' { sheet.next() } else { own.next() };
        if let Some(placed) = next {
            extra.push(tree(placed));
        }
    }
    extra.extend(own.map(tree));
    extra.extend(sheet.map(tree));
    place(typed, extra)
}

/// The typed children with the `extra` nodes put after as many of them as each followed (a stable sort: nodes of one place keep their order). A node that followed more than there are goes last.
fn place(typed: Vec<Node>, mut extra: Vec<(usize, Node)>) -> Vec<Node> {
    if extra.is_empty() {
        return typed;
    }
    extra.sort_by_key(|(after, _)| *after);
    let mut out = Vec::with_capacity(typed.len() + extra.len());
    let mut extra = extra.into_iter().peekable();
    for (at, node) in typed.into_iter().enumerate() {
        while let Some((_, n)) = extra.next_if(|(after, _)| *after <= at) {
            out.push(n);
        }
        out.push(node);
    }
    out.extend(extra.map(|(_, n)| n));
    out
}

/// A cell as an element: named `Cell`, with **no prefix** (the same of [`row_el`], [`section_el`] and [`sheet_only_el`]'s children; [`with_prefix`]
/// gives a made tree one).
#[must_use]
pub fn cell_el(name: &str, c: &Cell) -> El {
    let mut attrs = vec![("N".to_owned(), name.to_owned())];
    attrs.extend(cell_attrs(c));
    element("Cell", attrs, cell_children(c))
}

/// What is inside a cell: its text, and the elements, comments and instructions it holds.
#[must_use]
pub fn cell_children(c: &Cell) -> Vec<Node> {
    let Some(extra) = &c.extra else { return Vec::new() };
    let mut children = Vec::new();
    if let Some(t) = &extra.text {
        children.push(Node::Text(t.to_string()));
    }
    children.extend(extra.passthrough.iter().map(|p| node_to_tree(&p.node)));
    children
}

fn cell_nodes(cells: &Cells) -> Vec<Node> {
    cells.iter().map(|(name, cell)| Node::El(cell_el(name.as_str(), cell))).collect()
}

/// A row as an element.
#[must_use]
pub fn row_el(r: &Row) -> El {
    element("Row", row_attrs(r), weave(cell_nodes(&r.cells), &[&r.passthrough]))
}

/// A section as an element.
#[must_use]
pub fn section_el(s: &Section) -> El {
    let mut typed = cell_nodes(&s.cells);
    typed.extend(s.rows.iter().map(|r| Node::El(row_el(r))));
    element("Section", section_attrs(s), weave(typed, &[&s.passthrough]))
}

/// The cells and then the sections of a sheet, as nodes.
#[must_use]
pub fn sheet_nodes(sheet: &Sheet) -> Vec<Node> {
    let mut nodes = cell_nodes(&sheet.cells);
    nodes.extend(sheet.sections.iter().map(|s| Node::El(section_el(s))));
    nodes
}

/// An element that is only a sheet (`PageSheet`, `DocumentSheet`).
#[must_use]
pub fn sheet_only_el(name: &str, sheet: &Sheet) -> El {
    element(name, sheet_only_attrs(sheet), weave(sheet_nodes(sheet), &[&sheet.passthrough]))
}

/// Whether a sheet holds nothing at all: no cell, no section, no link, no attribute, no unknown node.
#[must_use]
pub fn sheet_is_empty(sheet: &Sheet) -> bool {
    sheet.cells.is_empty()
        && sheet.sections.is_empty()
        && sheet.links.line.is_none()
        && sheet.links.fill.is_none()
        && sheet.links.text.is_none()
        && sheet.raw.is_empty()
        && sheet.passthrough.is_empty()
}

/// `el` and everything in it with `prefix:` put before each unprefixed element name: for a new element placed in a part whose elements
/// carry a prefix. An empty prefix changes nothing. The walk keeps its own stack and does not recurse.
///
/// **It prefixes every element of the tree**, whatever the element is: for a tree whose elements the caller made all of (a shape, a page, a master,
/// which hold elements this crate knows nothing of), and whose unknown nodes, if any, are meant to be prefixed too. A tree that holds a kept unknown
/// node of a sheet (a `Passthrough` put back among the cells and sections) wants [`with_prefix_sheet`], which leaves it as the file had it.
pub fn with_prefix(el: &mut El, prefix: &str) {
    if prefix.is_empty() {
        return;
    }
    let mut stack: Vec<&mut El> = vec![el];
    while let Some(e) = stack.pop() {
        if !e.name.contains(':') {
            e.name = format!("{prefix}:{}", e.name);
        }
        for node in &mut e.children {
            if let Node::El(child) = node {
                stack.push(child);
            }
        }
    }
}

/// `el`, an element only a sheet that [`sheet_only_el`] made, with `prefix:` put before its own name and the names of the elements the codec made
/// in it, and before no other.
///
/// **What it prefixes, and what it leaves alone.** It walks the codec's own structure: below `el` the `Cell` and `Section` elements that have an
/// `N`, below a section its `Cell`s (with an `N`) and its `Row`s (with an `N` or an `IX`), below a row its `Cell`s. **Every other node is a kept
/// unknown one** (the model's `Passthrough` puts it back among the codec's elements, and the content of a cell is its own) and is left as it is,
/// whatever it is called and whatever it holds: an unknown element inside the sheet stays in no namespace, as the file had it, and so does
/// everything inside it, so that it is the element that was read. An empty prefix changes nothing. The walk keeps its own stack and does not
/// recurse.
pub fn with_prefix_sheet(el: &mut El, prefix: &str) {
    prefix_at(el, prefix, Place::Sheet);
}

/// A cell element made by [`cell_el`], prefixed: only its own name (what is inside a cell is the cell's own).
pub fn prefix_cell(el: &mut El, prefix: &str) {
    prefix_at(el, prefix, Place::Cell);
}

/// A row element made by [`row_el`], prefixed, with its cells.
pub fn prefix_row(el: &mut El, prefix: &str) {
    prefix_at(el, prefix, Place::Row);
}

/// A section element made by [`section_el`], prefixed, with its cells and rows.
pub fn prefix_section(el: &mut El, prefix: &str) {
    prefix_at(el, prefix, Place::Section);
}

/// Where in the codec's structure an element stands: the elements below it that the codec made are those of that place.
#[derive(Clone, Copy)]
enum Place {
    Sheet,
    Section,
    Row,
    /// A cell: what is inside it is not the codec's.
    Cell,
}

fn is_named(e: &El, local: &str) -> bool {
    !e.name.contains(':') && e.name == local && e.attr("N").is_some_and(|n| !n.is_empty())
}

fn is_row(e: &El) -> bool {
    !e.name.contains(':') && e.name == "Row" && (e.attr("N").is_some_and(|n| !n.is_empty()) || e.attr("IX").is_some())
}

fn prefix_at(el: &mut El, prefix: &str, place: Place) {
    if prefix.is_empty() {
        return;
    }
    if !el.name.contains(':') {
        el.name = format!("{prefix}:{}", el.name);
    }
    let mut stack: Vec<(&mut El, Place)> = vec![(el, place)];
    while let Some((e, place)) = stack.pop() {
        for node in &mut e.children {
            let Node::El(child) = node else { continue };
            let next = match place {
                Place::Sheet if is_named(child, "Cell") => Place::Cell,
                Place::Sheet if is_named(child, "Section") => Place::Section,
                Place::Section if is_named(child, "Cell") => Place::Cell,
                Place::Section if is_row(child) => Place::Row,
                Place::Row if is_named(child, "Cell") => Place::Cell,
                _ => continue,
            };
            if !child.name.contains(':') {
                child.name = format!("{prefix}:{}", child.name);
            }
            if !matches!(next, Place::Cell) {
                stack.push((child, next));
            }
        }
    }
}
