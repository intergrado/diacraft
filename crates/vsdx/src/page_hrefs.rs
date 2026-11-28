//! The links of an `.odg` to a page by its name, when the writer renames the page (`page_names.rs`, `page_follow.rs`).
//!
//! **An `.odg` page link is markup the model keeps**, and a VSDX file carries it in DiaCraft's own part: a `text:a` round some text (a run of the text's ODF data) and a `draw:a` round a shape (a node of
//! the page's kept markup), each with an `xlink:href` of the form `#<page>` ("links to a page by its name"). A page the writer renames is named by DiaCraft and not by the person, so the
//! link is written to name the new name, and the drawing read from the file (and an `.odg` made from it again) has a link that still finds its page. Only an `xlink:href` whose whole text is `#` and the name
//! of a page that was renamed is changed (a link to a page that has no such name, to the web, to a file, and the text of any other attribute are left as they are), wherever the model keeps the markup of a page
//! or of a shape: the page's and the shape's kept nodes, the kept nodes of a text, the elements of its runs and paragraphs, and the markers in its parts.
//!
//! The page a link names is the first page of the list that has the name, as the links of a drawing are followed. The work is one walk of the pages and their shapes (a shape that holds none is
//! shared, not copied), no deeper than the model allows and no deeper into a kept element than [`MOST_DEPTH`].

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{
    Document, Kept, MAX_SHAPE_DEPTH, OdfParagraph, OdfRun, Passthrough, Placed, RawAttrs, RawElement, RawNode, Shape, ShapeList, Text, TextPart,
};

/// The attribute that holds a link's target.
const HREF: &str = "xlink:href";
/// How deep into a kept element the walk goes: the markup of a drawing is shallower, and a hostile file is not followed past it.
const MOST_DEPTH: usize = 256;

/// The `#<old name>` of each page that was renamed, and the `#<new name>` it has now.
type Names = HashMap<String, String>;

/// The links to rename: the first page of each name that the writer gave another name.
fn renamed_names(now: &Document, was: &Document) -> Names {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut map = Names::new();
    for (before, after) in was.pages.iter().zip(now.pages.iter()) {
        // (The first page of a name is the one a link finds.)
        if seen.insert(&before.name) && before.name != after.name {
            map.insert(format!("#{}", before.name), format!("#{}", after.name));
        }
    }
    map
}

/// The attributes with each `xlink:href` that names a renamed page rewritten; `None` when none does.
fn attrs(attrs: &RawAttrs, names: &Names, count: &mut usize) -> Option<RawAttrs> {
    let wanted = |name: &str, value: &str| name == HREF && names.contains_key(value);
    if !attrs.iter().any(|(name, value)| wanted(name, value)) {
        return None;
    }
    let pairs: Vec<(String, String)> = attrs
        .iter()
        .map(|(name, value)| match names.get(value).filter(|_| name == HREF) {
            Some(new) => {
                *count = count.saturating_add(1);
                (name.to_owned(), new.clone())
            }
            None => (name.to_owned(), value.to_owned()),
        })
        .collect();
    Some(RawAttrs::from_pairs(pairs))
}

/// An element with its own attributes and those of everything in it rewritten; `None` when nothing is.
fn element(el: &RawElement, names: &Names, count: &mut usize, depth: usize) -> Option<RawElement> {
    let own = attrs(&el.attrs, names, count);
    let mut children: Option<Vec<RawNode>> = None;
    if depth < MOST_DEPTH {
        for (at, child) in el.children.iter().enumerate() {
            if let Some(new) = node(child, names, count, depth + 1)
                && let Some(slot) = children.get_or_insert_with(|| el.children.clone()).get_mut(at)
            {
                *slot = new;
            }
        }
    }
    if own.is_none() && children.is_none() {
        return None;
    }
    Some(RawElement {
        name: Arc::clone(&el.name),
        attrs: own.unwrap_or_else(|| el.attrs.clone()),
        children: children.unwrap_or_else(|| el.children.clone()),
    })
}

fn node(n: &RawNode, names: &Names, count: &mut usize, depth: usize) -> Option<RawNode> {
    match n {
        RawNode::Element(el) => element(el, names, count, depth).map(RawNode::Element),
        _ => None,
    }
}

/// An `Arc`ed element rewritten, or `None`.
fn shared(el: &Arc<RawElement>, names: &Names, count: &mut usize) -> Option<Arc<RawElement>> {
    element(el, names, count, 0).map(Arc::new)
}

/// Kept nodes with their links rewritten; `None` when none is.
fn passthrough(p: &Passthrough, names: &Names, count: &mut usize) -> Option<Passthrough> {
    let mut changed: Option<Vec<Placed>> = None;
    for (at, placed) in p.iter().enumerate() {
        if let Some(new) = node(&placed.node, names, count, 0)
            && let Some(slot) = changed.get_or_insert_with(|| p.iter().cloned().collect()).get_mut(at)
        {
            *slot = Placed { after: placed.after, node: new };
        }
    }
    changed.map(Passthrough::from_nodes)
}

fn kept(k: &Kept, names: &Names, count: &mut usize) -> Option<Kept> {
    let nodes = passthrough(&k.kept, names, count);
    let own = attrs(&k.raw, names, count);
    if nodes.is_none() && own.is_none() {
        return None;
    }
    Some(Kept { raw: own.unwrap_or_else(|| k.raw.clone()), kept: nodes.unwrap_or_else(|| k.kept.clone()) })
}

/// A text with the links of its markers and of its ODF data rewritten; `None` when none is.
fn text(t: &Text, names: &Names, count: &mut usize) -> Option<Text> {
    let mut own: Option<Text> = None;
    for (at, part) in t.parts.iter().enumerate() {
        if let TextPart::OdfRaw(raw) = part
            && let Some(new) = node(raw, names, count, 0)
            && let Some(slot) = own.get_or_insert_with(|| t.clone()).parts.get_mut(at)
        {
            *slot = TextPart::OdfRaw(Box::new(new));
        }
    }
    if let Some(data) = t.ext.get() {
        let mut odf = None;
        if let Some(new) = kept(&data.odf.kept, names, count) {
            odf.get_or_insert_with(|| data.odf.clone()).kept = new;
        }
        for (at, paragraph) in data.odf.paragraphs.iter().enumerate() {
            if let Some(new) = paragraph_links(paragraph, names, count)
                && let Some(slot) = odf.get_or_insert_with(|| data.odf.clone()).paragraphs.get_mut(at)
            {
                *slot = new;
            }
        }
        if let Some(odf) = odf {
            own.get_or_insert_with(|| t.clone()).ext.make_mut().odf = odf;
        }
    }
    own
}

fn paragraph_links(p: &OdfParagraph, names: &Names, count: &mut usize) -> Option<OdfParagraph> {
    let mut own: Option<OdfParagraph> = None;
    if let Some(new) = p.element.as_ref().and_then(|el| shared(el, names, count)) {
        own.get_or_insert_with(|| p.clone()).element = Some(new);
    }
    for (at, run) in p.runs.iter().enumerate() {
        if let Some(new) = run.element.as_ref().and_then(|el| shared(el, names, count)) {
            let runs = &mut own.get_or_insert_with(|| p.clone()).runs;
            if let Some(slot) = runs.get_mut(at) {
                *slot = OdfRun { element: Some(new), ..run.clone() };
            }
        }
    }
    own
}

/// A shape with the links in its kept nodes, its text and its ODF data rewritten; `None` when none is.
fn shape(s: &Shape, names: &Names, count: &mut usize) -> Option<Shape> {
    let mut own: Option<Shape> = None;
    if let Some(new) = passthrough(&s.passthrough, names, count) {
        own.get_or_insert_with(|| s.clone()).passthrough = new;
    }
    if let Some(new) = s.ext.get().and_then(|data| kept(&data.odf.kept, names, count)) {
        own.get_or_insert_with(|| s.clone()).ext.make_mut().odf.kept = new;
    }
    if let Some(new) = s.text.as_ref().and_then(|t| text(t, names, count)) {
        own.get_or_insert_with(|| s.clone()).text = Some(Arc::new(new));
    }
    own
}

/// A list of shapes with each shape's links rewritten, members of groups included; `None` when none is. Shapes that hold none are shared with the list given.
fn list(shapes: &ShapeList, names: &Names, count: &mut usize, depth: usize) -> Option<ShapeList> {
    if depth >= MAX_SHAPE_DEPTH {
        return None;
    }
    let mut kept: Vec<Arc<Shape>> = Vec::new();
    let mut changed = false;
    for s in shapes.iter() {
        let mut copy = shape(s, names, count);
        let members = copy.as_ref().map_or(&s.children, |own| &own.children);
        if !members.is_empty()
            && let Some(inner) = list(members, names, count, depth + 1)
        {
            copy.get_or_insert_with(|| Shape::clone(s)).children = inner;
        }
        match copy {
            Some(own) => {
                changed = true;
                kept.push(Arc::new(own));
            }
            None => kept.push(Arc::clone(s)),
        }
    }
    changed.then(|| ShapeList::from_vec(kept))
}

/// Makes the links of `now` that named a page of `was` by its name (`#<name>`) name it by its new name, where `now` is `was` with the pages renamed (the lists are in the same order). → how many links.
pub(crate) fn follow_hrefs(now: &mut Document, was: &Document) -> usize {
    let names = renamed_names(now, was);
    if names.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for at in 0..now.pages.len() {
        let Some(page) = now.pages.get(at) else { continue };
        let mut own: Option<diacraft_doc::Page> = None;
        if let Some(new) = passthrough(&page.passthrough, &names, &mut count) {
            own.get_or_insert_with(|| diacraft_doc::Page::clone(page)).passthrough = new;
        }
        if let Some(new) = page.ext.get().and_then(|data| kept(&data.odf.kept, &names, &mut count)) {
            own.get_or_insert_with(|| diacraft_doc::Page::clone(page)).ext.make_mut().odf.kept = new;
        }
        if let Some(shapes) = list(&page.shapes, &names, &mut count, 0) {
            own.get_or_insert_with(|| diacraft_doc::Page::clone(page)).shapes = shapes;
        }
        if let Some(own) = own {
            // (`at` is a place in the list.)
            let _ = now.pages.set(at, Arc::new(own));
        }
    }
    for at in 0..now.masters.len() {
        let Some(master) = now.masters.get(at) else { continue };
        if let Some(shapes) = list(&master.shapes, &names, &mut count, 0) {
            let mut own = diacraft_doc::Master::clone(master);
            own.shapes = shapes;
            let _ = now.masters.set(at, Arc::new(own));
        }
    }
    count
}
