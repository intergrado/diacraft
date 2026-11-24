//! What the document's containers hold that no field of the model reads (the native-format design's §3.4, "Elements kept whole"): `office:scripts`, a child of
//! `office:body`, `office:drawing`, `office:master-styles` or a layer set that is not one this reader knows, and the attributes of those containers. A master page that holds no shape is
//! no longer among them: it is kept whole, as a style of the family `master:master-page` (`pages.rs`), so that a copy writes it back where it stood. A `.diacraft` file keeps
//! no original, so **whatever the model does not hold is lost the day a drawing opened from an `.odg` is saved as one**.
//!
//! # Where it goes
//!
//! * **A container's attributes** (the namespace declarations among them) are in `OdfDoc.roots`, as the roots' are, under the key `<part>/<element>/<element>`: the part's name, then the
//!   names of the elements down to the container, as written (`content.xml/office:body/office:drawing`). The roots' own keys are the part's name alone.
//! * **A child** that no field reads is in `OdfDoc.kept`, whole, inside **one element for each container it stands in, nested from the root of its part**, the element of the root first
//!   (`office:document-content`, `office:document-styles`, the manifest's keys of `roots` say which part). The element a container is made as holds no attribute, but for a master page,
//!   which holds its `style:name` (the one attribute that says which master page it is); its only child is the next container, or the child that is kept. The entry's place is the number
//!   of children of the innermost container that the reader read and that came before the one kept, so that a writer puts it back among them where it was.
//!
//! A flat drawing holds none of it: it has no package to be saved as a `.diacraft` file from, and its root holds both parts' children.
//!
//! # Cost
//!
//! Each entry is charged before it is made, by the items and bytes of the element kept ([`super::shapes::element_cost`]) and of the elements round it.

use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{Passthrough, RawAttrs, RawElement, RawNode};

use super::Reader;
use super::shapes::{element_cost, raw_element};
use crate::error::OdgError;

/// A container on the way to what is kept: its name as written and, for a master page, the attribute that says which one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Link<'a> {
    pub name: &'a str,
    pub id: Option<(&'a str, &'a str)>,
}

impl<'a> Link<'a> {
    /// A container known by its name alone.
    pub(crate) fn of(el: &'a El) -> Link<'a> {
        Link { name: el.name.as_str(), id: None }
    }
}

/// Items an entry costs beside the element it holds: its place in the list and the allocation of its node.
const ENTRY: usize = 2;

/// Items an element round a kept child costs: the element, its name and its place in the one that holds it.
const WRAPPER: usize = 3;

/// What the read found that no field reads, to be put in the model when it is made.
#[derive(Default)]
pub(crate) struct Unread {
    nodes: Passthrough,
    roots: Vec<(Arc<str>, RawAttrs)>,
}

/// Keeps `el`, a child of the container the `path` of links reaches (the first link is the root of its part), after `place` children the reader read. Charged first.
pub(crate) fn child(r: &mut Reader<'_>, path: &[Link<'_>], place: usize, el: &El) -> Result<(), OdgError> {
    let bytes =
        path.iter().fold(0usize, |sum, l| sum.saturating_add(l.name.len()).saturating_add(l.id.map_or(0, |(k, v)| k.len().saturating_add(v.len()))));
    let items = element_cost(el)
        .saturating_add(ENTRY)
        .saturating_add(path.len().saturating_mul(WRAPPER))
        .saturating_add(super::cost::text(bytes).div_ceil(super::ITEM_BYTES));
    r.take_items(items)?;
    let mut node = RawNode::Element(raw_element(el));
    for link in path.iter().rev() {
        let attrs = link.id.map_or_else(RawAttrs::new, |(k, v)| RawAttrs::from_pairs([(k, v)]));
        node = RawNode::Element(RawElement { name: Arc::from(link.name), attrs, children: vec![node] });
    }
    r.unread.nodes.push(u32::try_from(place).unwrap_or(u32::MAX), node);
    Ok(())
}

/// Keeps the attributes of `el` (a container), under `key` in the roots, when it has any. Charged first.
pub(crate) fn attrs(r: &mut Reader<'_>, key: &str, el: &El) -> Result<(), OdgError> {
    if el.attrs.is_empty() {
        return Ok(());
    }
    let bytes = el.attrs.iter().fold(key.len(), |sum, (k, v)| sum.saturating_add(k.len()).saturating_add(v.len()));
    r.take_items(ENTRY.saturating_add(el.attrs.len().saturating_mul(2)).saturating_add(super::cost::text(bytes).div_ceil(super::ITEM_BYTES)))?;
    r.unread.roots.push((Arc::from(key), RawAttrs::from_pairs(el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())))));
    Ok(())
}

impl Unread {
    /// Puts what was found in the document's ODF data (already charged).
    pub(crate) fn put(self, slot: &mut diacraft_doc::OdfDoc) {
        if !self.nodes.is_empty() {
            slot.kept.kept = self.nodes;
        }
        // The parts' own roots are put first (`keep::apply`), and these come after them.
        slot.roots.extend(self.roots);
    }
}
