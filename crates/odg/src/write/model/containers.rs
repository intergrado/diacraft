//! The roots of `content.xml` and `styles.xml`, and what the document's containers hold that no field reads (hold it in the model; the design's §3.4 rows for the
//! roots and for the elements kept whole).
//!
//! **The roots.** The attributes the model holds for the root of a part (`OdfDoc::roots`, the namespace declarations among them) are written on the root of the part made, in their order, with the
//! declarations the part's own names need (`office`, `style`, `text`, `draw`, `fo`, `svg`) put in front when the root has none; a root that binds one of them to another namespace is refused in
//! words. A model that holds no root leaves the root as [`crate::base`] makes it (`office:version="1.4"`).
//!
//! **The containers.** A child of the document's containers that no field reads (`office:scripts`, an unknown child of `office:body`, `office:drawing` or `office:master-styles`, or of the root of a
//! part) is kept whole in `OdfDoc::kept`, inside one element for each container it stands in, nested from the root of its part, after the number of children the reader read before it
//! (`read/unread.rs`). **It is put back in the container of the part made, at that place** (among the children the part made holds, the ones kept before it counted: so what stood together
//! stays together, in the order it had). The attributes of a container are in the roots under `<part>/<element>/<element>` and are put on the element they name.
//!
//! **What is not placed**, and counted for the note ([`crate::SaveNote::DocumentDataUnwritten`]): an entry that stands in a container the part made does not have (a master page other than the
//! one the package is made with, a layer set), an entry whose elements are not one chain, and a container's attributes whose element the part made does not have. **Nor is a piece whose prefix
//! nothing binds where it would go**: an attribute of the root or of a container that neither the element nor one above it declares the prefix of, and an entry that uses a
//! prefix the declarations in force in its container do not bind (its file bound it on an element the model does not keep the declarations of). Such a piece would make the part one that a
//! namespace-aware reader refuses; a name that is no qualified name, or a declaration XML Namespaces does not allow, is refused in words ([`crate::write::kept`]).

use std::collections::HashMap;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, Placed, RawElement, RawNode};

use super::document::Left;
use super::{WORDS_KEPT, WORDS_NAMES, bounded, ensure_declarations, set_version};
use crate::base::VERSION;
use crate::limits::Limits;
use crate::write::kept::{Bindings, binds, declaration_ok, declare_into, declared_by, in_force, is_qname, placeable};
use crate::write::{WriteError, WriteWork};

const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const STYLE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const TEXT_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const DRAW_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const FO_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const SVG_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";

/// What the document's containers hold that a part can be given back, at most, in bytes of markup: the reader reads no more of a part (and a `.diacraft` file holds no more).
const MOST_KEPT: usize = 16 * 1024 * 1024;

/// The local name of an element or attribute name as written.
fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, l)| l)
}

/// What an entry of `OdfDoc::kept` is: the containers it stands in, outermost first (the root of the part excluded), and what is kept there.
struct Chain<'a> {
    /// The local names of the containers.
    containers: Vec<&'a str>,
    node: &'a RawNode,
}

/// Whether an element is a container the writer can find in the part it makes, for a chain through it: the body and the drawing, and the master styles.
fn is_container(parent: &str, child: &str) -> bool {
    matches!((parent, child), ("document-content", "body") | ("body", "drawing") | ("document-styles", "master-styles"))
}

/// A child of the master styles that is a container the reader reads into fields (a master page, a layer set): a chain through one is not placed.
fn is_unplaceable(parent: &str, child: &str) -> bool {
    parent == "master-styles" && matches!(child, "master-page" | "layer-set")
}

/// Splits an entry into the chain of containers and the node kept: `None` when the entry is not one chain (a wrapper with several children), or goes through a container that is not placed.
fn chain_of(entry: &RawNode) -> Option<(&str, Chain<'_>)> {
    let RawNode::Element(root) = entry else { return None };
    let root_name = local(&root.name);
    let mut containers = Vec::new();
    let mut here: &RawElement = root;
    loop {
        let [only] = here.children.as_slice() else { return None };
        match only {
            RawNode::Element(child) if is_container(local(&here.name), local(&child.name)) => {
                containers.push(local(&child.name));
                here = child;
            }
            RawNode::Element(child) if is_unplaceable(local(&here.name), local(&child.name)) => return None,
            leaf => return Some((root_name, Chain { containers, node: leaf })),
        }
    }
}

/// The element of `root` at the path of local names `containers`: each is looked for among the children of the one before it.
fn container_mut<'a>(root: &'a mut El, containers: &[&str]) -> Option<&'a mut El> {
    let mut here = root;
    for name in containers {
        here = here.children.iter_mut().find_map(|n| match n {
            Node::El(el) if local(&el.name) == *name => Some(el),
            _ => None,
        })?;
    }
    Some(here)
}

/// The entries that stand in one container: the containers it is reached through, and each entry with the number of children the reader read before it.
type Group<'a> = (Vec<&'a str>, Vec<(usize, Node)>);

/// What `dress` did.
pub(super) struct Dressed {
    pub bytes: Vec<u8>,
    /// What the model holds for the part that it was not given, by kind: pieces of the document's containers (an entry of the kept data, or the attributes of a container) the part made has
    /// no place for, and pieces whose prefixes nothing binds where they would go.
    pub left: Left,
}

/// Takes out of `attrs` (an element's) each attribute whose prefix neither the element's own declarations nor `outer` (what is in force on its parent) bind, and gives how many: such an
/// attribute would make the part one no namespace-aware reader reads. A name that is no qualified name, or a declaration XML Namespaces does not allow, is refused in words.
fn drop_unbound(attrs: &mut Vec<(String, String)>, outer: &Bindings, work: &mut WriteWork) -> Result<usize, WriteError> {
    work.charge_steps(attrs.len())?;
    for (k, v) in attrs.iter() {
        if !is_qname(k) || declared_by(k).is_some_and(|p| !declaration_ok(p, v)) {
            return Err(WriteError::Unsupported(WORDS_NAMES));
        }
    }
    let mut here = outer.clone();
    declare_into(&mut here, attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let before = attrs.len();
    attrs.retain(|(k, _)| declared_by(k).is_some() || k.split_once(':').is_none_or(|(p, _)| binds(&here, p)));
    Ok(before.saturating_sub(attrs.len()))
}

/// The part `part` (`content.xml` or `styles.xml`) of the package made from `text`, with the model's root and containers put in. A model that holds none of it gives `text` as it is.
pub(super) fn dress(
    part: &str,
    text: &str,
    doc: &Document,
    limits: &Limits,
    work: &mut WriteWork,
    pictures: &mut super::document::FillNames,
) -> Result<Dressed, WriteError> {
    let Some(held) = doc.ext.get().map(|d| &d.odf) else { return Ok(Dressed { bytes: text.as_bytes().to_vec(), left: Left::default() }) };
    let own_root = held.roots.iter().find(|(name, _)| &**name == part);
    let prefix = format!("{part}/");
    let attrs_of_containers: Vec<_> = held.roots.iter().filter(|(name, _)| name.starts_with(&prefix)).collect();
    let root_local = if part == "content.xml" { "document-content" } else { "document-styles" };
    let entries: Vec<&Placed> =
        held.kept.kept.iter().filter(|p| matches!(&p.node, RawNode::Element(root) if local(&root.name) == root_local)).collect();
    if own_root.is_none() && attrs_of_containers.is_empty() && entries.is_empty() && !super::document::wants(part, doc) {
        return Ok(Dressed { bytes: text.as_bytes().to_vec(), left: Left::default() });
    }
    // Each entry and each container with attributes is a step of the save, charged before anything is made of it.
    work.charge_steps(entries.len().saturating_add(attrs_of_containers.len()))?;
    // Bounded before a tree is made of it: the entries of this part, and the attributes.
    if !bounded(entries.iter().map(|p| &p.node), MOST_KEPT) {
        return Err(WriteError::Unsupported(WORDS_KEPT));
    }
    let mut root = diacraft_container::xml::parse(text.as_bytes()).map_err(|e| WriteError::SelfCheck(format!("the package made is not XML: {e}")))?;
    let mut unplaced = 0usize;
    if let Some((_, attrs)) = own_root {
        // The package is written as an ODF 1.4 one (every part says so), whatever the file the model was read from was: the two prefixes of the pre-standard W3C namespaces (`fo` and
        // `svg`, which a file of ODF 1.0 binds so, and which the reader takes as the standard's) are bound to the standard's, which the names this writer writes are in.
        let standard = |name: &str, value: &str| match (name, value) {
            ("xmlns:fo", "http://www.w3.org/1999/XSL/Format") => FO_URI.to_owned(),
            ("xmlns:svg", "http://www.w3.org/2000/svg") => SVG_URI.to_owned(),
            _ => value.to_owned(),
        };
        let mut pairs: Vec<(String, String)> = attrs.iter().map(|(k, v)| (k.to_owned(), standard(k, v))).collect();
        let declared = ensure_declarations(
            &mut pairs,
            &[("office", OFFICE_URI), ("style", STYLE_URI), ("text", TEXT_URI), ("draw", DRAW_URI), ("fo", FO_URI), ("svg", SVG_URI)],
        )?;
        set_version(&mut pairs, &declared, OFFICE_URI, VERSION);
        // An attribute of the root whose prefix the root does not declare is not written.
        unplaced = unplaced.saturating_add(drop_unbound(&mut pairs, &Bindings::new(), work)?);
        root.attrs = pairs;
    }
    for (key, attrs) in attrs_of_containers {
        let names: Vec<&str> = key.split('/').skip(1).map(local).collect();
        // What is in force on the container's parent, for the prefixes of its attributes.
        work.charge_steps(root.attrs.len())?;
        let outer = in_force(&root, names.split_last().map_or(&[][..], |(_, parent)| parent));
        let Some(el) = container_mut(&mut root, &names) else {
            unplaced += 1;
            continue;
        };
        // Where each attribute stands, found once: an element of a million attributes is no quadratic walk.
        let mut at: HashMap<String, usize> = el.attrs.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect();
        for (k, v) in attrs.iter() {
            match at.get(k) {
                Some(&i) => {
                    if let Some(slot) = el.attrs.get_mut(i) {
                        slot.1 = v.to_owned();
                    }
                }
                None => {
                    at.insert(k.to_owned(), el.attrs.len());
                    el.attrs.push((k.to_owned(), v.to_owned()));
                }
            }
        }
        unplaced = unplaced.saturating_add(drop_unbound(&mut el.attrs, &outer, work)?);
    }
    // The entries, grouped by the container they stand in, so that each container is found once and each is filled in one pass: the places are counted among the children the reader
    // read, and the container is searched for among the elements the part made (the deepest containers first, so that the root's own children are not the ones a search walks).
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    // What is in force in each container an entry goes in, found once for each container.
    let mut in_container: HashMap<String, Bindings> = HashMap::new();
    for entry in entries {
        let Some((_, chain)) = chain_of(&entry.node) else {
            unplaced += 1;
            continue;
        };
        let key = chain.containers.join("/");
        if !in_container.contains_key(&key) {
            work.charge_steps(root.attrs.len())?;
            in_container.insert(key.clone(), in_force(&root, &chain.containers));
        }
        let Some((fits, steps)) = in_container.get(&key).and_then(|bindings| placeable(chain.node, bindings)) else {
            return Err(WriteError::Unsupported(WORDS_NAMES));
        };
        work.charge_steps(steps)?;
        // An entry whose prefix nothing binds in its container is not put in (it would be unbound there).
        if !fits {
            unplaced += 1;
            continue;
        }
        let place = usize::try_from(entry.after).unwrap_or(usize::MAX);
        let node = diacraft_sheetxml::passthrough::node_to_tree(chain.node);
        match by_path.get(&key) {
            Some(&at) => {
                if let Some((_, list)) = groups.get_mut(at) {
                    list.push((place, node));
                }
            }
            None => {
                by_path.insert(key, groups.len());
                groups.push((chain.containers, vec![(place, node)]));
            }
        }
    }
    groups.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    for (path, list) in groups {
        let Some(container) = container_mut(&mut root, &path) else {
            unplaced = unplaced.saturating_add(list.len());
            continue;
        };
        let original = std::mem::take(&mut container.children);
        let mut merged: Vec<Node> = Vec::with_capacity(original.len().saturating_add(list.len()));
        let mut rest = original.into_iter();
        let mut taken = 0usize;
        for (place, node) in list {
            while taken < place {
                match rest.next() {
                    Some(child) => merged.push(child),
                    None => break,
                }
                taken += 1;
            }
            merged.push(node);
        }
        merged.extend(rest);
        container.children = merged;
    }
    let left = super::document::put(part, &mut root, doc, limits, work, pictures)?.plus(Left { containers: unplaced, ..Left::default() });
    let bytes = diacraft_container::xml::to_bytes(&root, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", b"\n", true)
        .map_err(|_| WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_copy(bytes.len(), limits)?;
    Ok(Dressed { bytes, left })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use super::*;

    fn el(name: &str, children: Vec<RawNode>) -> RawNode {
        RawNode::Element(RawElement { name: Arc::from(name), attrs: diacraft_doc::RawAttrs::new(), children })
    }

    fn chain(node: &RawNode) -> Option<(Vec<&str>, String)> {
        chain_of(node).map(|(_, c)| (c.containers, format!("{:?}", c.node)))
    }

    /// The containers of an entry are the ones the writer finds in a part it makes (the body and the drawing, the master styles), outermost first, whatever prefix they are written
    /// with; what stands in the first one that is not one of them is what is kept.
    #[test]
    fn an_entry_is_a_chain_of_containers_and_what_is_kept_in_the_last() {
        let note = el("x:note", vec![]);
        let through = el("o:document-content", vec![el("o:body", vec![el("o:drawing", vec![note.clone()])])]);
        let (containers, leaf) = chain(&through).unwrap();
        assert_eq!(containers, ["body", "drawing"]);
        assert!(leaf.contains("x:note"), "{leaf}");
        // A child of the root itself: no container.
        let scripts = el("office:document-content", vec![note.clone()]);
        assert_eq!(chain(&scripts).unwrap().0, Vec::<&str>::new());
        // The styles part.
        let styles = el("office:document-styles", vec![el("office:master-styles", vec![note.clone()])]);
        assert_eq!(chain(&styles).unwrap().0, ["master-styles"]);
        // What the reader reads into fields is no container to put a thing in: a master page and a layer set are not placed.
        for name in ["style:master-page", "draw:layer-set"] {
            let into_fields = el("office:document-styles", vec![el("office:master-styles", vec![el(name, vec![note.clone()])])]);
            assert!(chain_of(&into_fields).is_none(), "{name}");
        }
        // Not one chain: a wrapper with two children, or none, and a node that is no element.
        assert!(chain_of(&el("office:document-content", vec![note.clone(), note.clone()])).is_none());
        assert!(chain_of(&el("office:document-content", vec![])).is_none());
        assert!(chain_of(&RawNode::Text(Arc::from("t"))).is_none());
    }

    /// A container is found by the local names of the path, each among the children of the one before it; a path that leaves the tree finds nothing.
    #[test]
    fn a_container_is_found_by_its_local_names() {
        let drawing = El { name: "o:drawing".to_owned(), ..El::default() };
        let body = El { name: "o:body".to_owned(), attrs: Vec::new(), children: vec![Node::El(drawing)] };
        let mut root = El { name: "o:root".to_owned(), attrs: Vec::new(), children: vec![Node::Text("x".to_owned()), Node::El(body)] };
        assert_eq!(container_mut(&mut root, &[]).unwrap().name, "o:root");
        assert_eq!(container_mut(&mut root, &["body", "drawing"]).unwrap().name, "o:drawing");
        assert!(container_mut(&mut root, &["drawing"]).is_none());
        assert!(container_mut(&mut root, &["body", "nothing"]).is_none());
    }
}
