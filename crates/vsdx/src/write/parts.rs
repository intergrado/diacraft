//! The package-level trees a save edits: a part's tree with what surrounds its root, relationships and content types.
//!
//! A relationships part is edited as a tree: a row this crate does not touch stays as it is, with the attributes and the
//! order it had, an `Id` is never renumbered, and a row for no part is kept. Only the rows an edit adds or takes out change. The content types stream
//! is edited at its rows too, and keeps the bytes of the rows it does not touch (`types.rs`).

use std::collections::HashSet;

use diacraft_opc::name::rels_part_name;
use diacraft_opc::xml::{self, El, Node, XmlLimits};

use super::WriteError;
use super::build::element;
use crate::VsdxError;

pub(super) const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines
pub(super) const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
pub(super) const PK: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/";
pub(super) const NS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(super) const NS_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
pub(super) const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
pub(super) const CT_XML: &str = "application/xml";

pub(super) fn rel_type(base: &str, word: &str) -> String {
    format!("{base}{word}")
}

/// A part's tree, and the bytes before and after its root (a byte order mark, the XML declaration, comments), which the tree does not hold.
pub(super) struct Tree {
    pub root: El,
    pub prolog: Vec<u8>,
    pub epilog: Vec<u8>,
}

impl Tree {
    /// Parses the part `name`, within the default XML limits.
    pub(super) fn parse(name: &str, bytes: &[u8]) -> Result<Tree, WriteError> {
        let root =
            xml::parse_with(bytes, &XmlLimits::default()).map_err(|error| WriteError::Original(VsdxError::Xml { part: name.to_owned(), error }))?;
        let (prolog, epilog) = match xml::split_document(bytes) {
            Some((start, end)) => (bytes.get(..start).unwrap_or_default().to_vec(), bytes.get(end..).unwrap_or_default().to_vec()),
            None => (Vec::new(), Vec::new()),
        };
        Ok(Tree { root, prolog, epilog })
    }

    /// A tree for a part that is new: a standard declaration and a root.
    pub(super) fn new(root: El) -> Tree {
        Tree { root, prolog: Vec::new(), epilog: Vec::new() }
    }

    /// The part as bytes.
    pub(super) fn bytes(&self, name: &str) -> Result<Vec<u8>, WriteError> {
        xml::to_bytes(&self.root, &self.prolog, &self.epilog, true).map_err(|error| WriteError::Xml { part: name.to_owned(), error })
    }
}

/// The folder of a part name, with its trailing slash: `/visio/pages/pages.xml` is `/visio/pages/`.
pub(super) fn folder_of(part: &str) -> &str {
    part.rfind('/').map_or("/", |i| part.get(..=i).unwrap_or("/"))
}

/// The `Target` that reaches `target` from the part `source`, relative to `source`'s folder: `../masters/master1.xml`.
pub(crate) fn relative_target(source: &str, target: &str) -> String {
    let from: Vec<&str> = folder_of(source).split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count().min(to.len().saturating_sub(1));
    let mut out = String::new();
    for _ in common..from.len() {
        out.push_str("../");
    }
    out.push_str(&to.get(common..).unwrap_or_default().join("/"));
    out
}

// -------------------------------------------------------------------------------------------------------------- relationships

/// The rows of a relationships part as a tree.
pub(super) struct RelsTree {
    /// The part's name.
    pub name: String,
    pub tree: Tree,
    pub changed: bool,
}

/// One relationship row as it is written.
#[derive(Clone, Debug)]
pub(super) struct RelRow {
    pub id: String,
    pub ty: String,
    pub target: String,
    pub external: bool,
}

/// The `rId` numbers of a relationships part, walked up once however many ids are taken (see [`RelsTree::id_walk`]).
pub(super) struct IdWalk {
    used: HashSet<String>,
    n: u32,
}

impl IdWalk {
    /// The least `rId` not in use, which is then in use. `None` only if the numbers run out.
    pub(super) fn next(&mut self) -> Option<String> {
        loop {
            self.n = self.n.checked_add(1)?;
            let id = format!("rId{}", self.n);
            if self.used.insert(id.clone()) {
                return Some(id);
            }
        }
    }
}

impl RelsTree {
    /// A relationships part with no rows.
    pub(super) fn empty(name: &str) -> RelsTree {
        let root = element("Relationships", vec![("xmlns".to_owned(), NS_RELS.to_owned())], Vec::new());
        RelsTree { name: name.to_owned(), tree: Tree::new(root), changed: false }
    }

    pub(super) fn parse(name: &str, bytes: &[u8]) -> Result<RelsTree, WriteError> {
        let tree = Tree::parse(name, bytes)?;
        if !tree.root.is("Relationships") {
            return Err(WriteError::Mismatch("a relationships part has another root"));
        }
        Ok(RelsTree { name: name.to_owned(), tree, changed: false })
    }

    /// The rows, in order.
    pub(super) fn rows(&self) -> Vec<RelRow> {
        self.tree
            .root
            .children_named("Relationship")
            .filter_map(|r| {
                Some(RelRow {
                    id: r.attr("Id")?.to_owned(),
                    ty: r.attr("Type")?.to_owned(),
                    target: r.attr("Target")?.to_owned(),
                    external: r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")),
                })
            })
            .collect()
    }

    /// The first unused `rId` and a number: `rId1` if it is free.
    pub(super) fn next_id(&self) -> String {
        self.next_ids(1).into_iter().next().unwrap_or_default()
    }

    /// The `count` first unused `rId`s, in order, as `count` calls of [`RelsTree::next_id`] with each id added before the next would give them:
    /// the ids in use are collected **once**, and the numbers are walked once, so the work is the rows there and the ids made. (A call for each
    /// of many new rows collects the set again each time, which is the square of their number.) Fewer than `count` come back only if the
    /// numbers run out.
    pub(super) fn next_ids(&self, count: usize) -> Vec<String> {
        let mut walk = self.id_walk();
        let mut ids = Vec::new();
        while ids.len() < count {
            let Some(id) = walk.next() else { break };
            ids.push(id);
        }
        ids
    }

    /// The ids in use, collected once, for a caller that takes them one at a time between other work (the pages and masters an edit adds, each
    /// of which asks for its id as the index is patched): each [`IdWalk::next`] is the least id free now, and the walk goes up the numbers once
    /// however many it hands out. The walk knows the ids it handed out; a row added by anyone else after it was made is not known to it.
    pub(super) fn id_walk(&self) -> IdWalk {
        let used: HashSet<String> = self.tree.root.children_named("Relationship").filter_map(|r| r.attr("Id")).map(str::to_owned).collect();
        crate::work::id_scan(used.len());
        IdWalk { used, n: 0 }
    }

    /// Adds a row after the others.
    pub(super) fn add(&mut self, id: &str, ty: &str, target: &str) {
        self.add_row(&RelRow { id: id.to_owned(), ty: ty.to_owned(), target: target.to_owned(), external: false });
    }

    /// Adds a row after the others, with its `TargetMode` when it leads outside the package.
    pub(super) fn add_row(&mut self, row: &RelRow) {
        let mut attrs = vec![("Id".to_owned(), row.id.clone()), ("Type".to_owned(), row.ty.clone()), ("Target".to_owned(), row.target.clone())];
        if row.external {
            attrs.push(("TargetMode".to_owned(), "External".to_owned()));
        }
        let mut row = element("Relationship", attrs, Vec::new());
        super::build::with_prefix(&mut row, self.tree.root.prefix().to_owned().as_str());
        self.tree.root.children.push(Node::El(row));
        self.changed = true;
    }

    /// Takes out every row whose `Id` is in `ids`, and the white space before each, in **one pass** (a package may have tens of thousands of rows,
    /// and `remove` once for each would be quadratic). Returns how many rows went.
    pub(super) fn remove_ids(&mut self, ids: &HashSet<&str>) -> usize {
        let children = std::mem::take(&mut self.tree.root.children);
        crate::work::rels(children.len());
        let mut out: Vec<Node> = Vec::with_capacity(children.len());
        let mut removed = 0usize;
        for node in children {
            let goes = matches!(&node, Node::El(e) if e.is("Relationship") && e.attr("Id").is_some_and(|id| ids.contains(id)));
            if goes {
                removed += 1;
                if matches!(out.last(), Some(Node::Text(t)) if crate::passthrough::is_blank(t)) {
                    out.pop();
                }
            } else {
                out.push(node);
            }
        }
        self.tree.root.children = out;
        self.changed |= removed > 0;
        removed
    }

    /// Takes out the first row with this `Id`, and the white space before it.
    pub(super) fn remove(&mut self, id: &str) -> bool {
        let Some(at) = self.tree.root.children.iter().position(|n| matches!(n, Node::El(e) if e.is("Relationship") && e.attr("Id") == Some(id)))
        else {
            return false;
        };
        self.tree.root.children.remove(at);
        if at > 0 && matches!(self.tree.root.children.get(at - 1), Some(Node::Text(t)) if crate::passthrough::is_blank(t)) {
            self.tree.root.children.remove(at - 1);
        }
        self.changed = true;
        true
    }
}

/// The name of the relationships part of `source` (`/` for the package).
pub(super) fn rels_name(source: &str) -> String {
    rels_part_name(source)
}
