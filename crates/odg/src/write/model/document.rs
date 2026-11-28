//! The document's own data in the two parts the package is made with (the native-format design's §3.4 rows for the named definitions, the layer set, the other
//! style families, the page layouts and master pages): what the model holds of them is written into `content.xml` and `styles.xml` of the package the splice is given, so that the
//! splice sees a file that has them, as a file a drawing was opened from has.
//!
//! * **The named definitions** (markers, gradients, opacity gradients, hatches, dashes, fill images; `office:styles`) are written as they were read, in their order, after the common styles.
//!   **A fill image's picture is the entry its `xlink:href` names** (found the pictures of 89 of 93 visits lost with no word, every one a bitmap fill): the
//!   definition's bytes go in the package under the name the reader found them by, with a row of the manifest, so that a shape or a style sheet that names the fill shows it in any program.
//!   A name the package cannot give the picture (a name that is no entry name, one another part or another picture of other bytes takes, or no name at all, the inline data of a flat file) is a
//!   name of the writer's own, `Pictures/fill` and a number, and the definition's href says it; two definitions of the same bytes share one entry. A definition whose bytes the model does not
//!   hold (the reader held none: an address outside the package, a picture past the media budget) is written as it was read.
//! * **The layer set** (`draw:layer-set` of the master styles) is the model's list of layers, each with its attributes as read, when the model holds one.
//! * **The other style families** the reader kept whole (`OdfDoc::styles`, by family and name: the common styles of `office:styles`, the automatic styles of the page layouts, the drawing pages, the
//!   list styles and the data styles) are put in the container their family says. **The automatic styles of the graphic, paragraph and text families are the shapes' own** (what no cell
//!   reads of the style a shape names): they are not written here, **but for the ones nothing names** (the families `unused` and `unused-styles`: a style with no name, one no
//!   shape, paragraph or run found, and the second of a name a container holds twice), which are kept whole and written in the automatic styles of the part they came from. **A style written whole is
//!   written in the form ODF 1.4 takes** ([`super::forms`]): `text:enable-numbering` on its paragraph properties moves into an attribute of DiaCraft's, and a weight, a slant or a
//!   variant 1.4 lists no value for is written as the value it lists nearest, with the value it had carried.
//! * **The pages the `.odg` reader made** (a page that holds its master page's name) are written into `content.xml` as the elements they were, `draw:name`, the attributes the reader
//!   kept and the children it kept whole, and each master page they name is written into `styles.xml` with the attributes it had. **The master page and the page layout the package was
//!   made with are taken out** when such a page is there: the model's own are put in their place, and the page layout of a master page that the model holds no layout for is made from the
//!   page's cells. A page that holds no such data (one made in DiaCraft) is written by the splice as a new page, as it always was.
//!
//! Everything is bounded by the caps of a kept part ([`super::containers`]): **each piece is taken from what is left of the cap, by its size, and its copy charged by that size, before
//! it is made** (a page's kept children were each checked against the cap, which never went down, and charged a byte each, so that a page of many children each under
//! the cap was copied whole whatever their sum). Each element is a step charged before it is made.
//!
//! **Namespaces**. A piece is written as the model holds it, its prefixes as the file wrote them, in the container the file had it in, under the root's and the
//! containers' declarations the part is made with (the file's own, which the model holds): there each prefix stands for what it stood for where it was read. **A piece that uses a prefix
//! nothing binds where it goes** (one its file bound on an element the model does not keep the declarations of) is left out and counted for the note, never written unbound; a name that
//! is no qualified name, or a declaration XML Namespaces does not allow, is refused in words ([`crate::write::kept`]).

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{DefFamily, Definition, Document, Kept, OdfDoc, Page, RawElement, RawNode};
use diacraft_sheetxml::passthrough::{node_to_tree, to_tree};

use super::{WORDS_KEPT, WORDS_NAMES, element_total, head_total, node_total};
use crate::limits::Limits;
use crate::write::kept::{Bindings, binds, declaration_ok, declared_by, free_of, free_prefixes, in_force, is_qname};
use crate::write::{WriteError, WriteWork};

/// The most the model's definitions, styles and pages come to in one part, in bytes of markup (a part the reader reads is no larger).
const MOST: usize = 16 * 1024 * 1024;

/// The family of a master page the reader kept whole (`read/pages.rs`).
const MASTER_FAMILY: &str = "master:master-page";

/// What this step left out of what the model holds, by the kind of the note's count: a piece whose prefix nothing binds where it would go, and, among the containers'
/// pieces, one that the part made has no place for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Left {
    pub definitions: usize,
    pub layers: usize,
    pub styles: usize,
    /// Pieces of the containers' markup: a page's or a master page's attributes and children, a font face, and what the part made has no place for.
    pub containers: usize,
}

impl Left {
    /// The two counts together.
    pub(crate) fn plus(self, other: Left) -> Left {
        Left {
            definitions: self.definitions.saturating_add(other.definitions),
            layers: self.layers.saturating_add(other.layers),
            styles: self.styles.saturating_add(other.styles),
            containers: self.containers.saturating_add(other.containers),
        }
    }
}

/// A picture of a fill image definition, which the package made holds as an entry of its own: the entry's name, its bytes (the model's, shared) and the media type of its
/// manifest row.
#[derive(Clone, Debug)]
pub(crate) struct FillPicture {
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub media: String,
}

/// The XLink namespace, of a fill image's `xlink:href`.
const XLINK_URI: &str = "http://www.w3.org/1999/xlink";

/// The office namespace, of an inline picture's `office:binary-data`.
const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

/// The names the pictures of the fill images take, and the pictures already given one: the model's own entries and the entries the writer makes are no picture's, and a picture of the same
/// bytes as an earlier one is that one's entry.
pub(crate) struct FillNames {
    /// Lower-cased, as the package's entries are told apart.
    taken: HashSet<String>,
    pub(crate) made: Vec<FillPicture>,
    cursor: u64,
}

impl FillNames {
    pub(crate) fn of(doc: &Document) -> FillNames {
        let mut taken: HashSet<String> = doc.parts.iter().map(|p| p.name.to_ascii_lowercase()).collect();
        // The thumbnail the splice writes, whatever the model holds.
        taken.insert("thumbnails/thumbnail.png".to_owned());
        FillNames { taken, made: Vec::new(), cursor: 1 }
    }

    /// The entry the picture `bytes` is written as: an earlier picture's of the same bytes, else `wanted` (the name its definition gave it) when the package can give it that name, else
    /// `Pictures/fill` and the least number no entry takes. Each name tried, and each earlier picture the bytes are compared with, is a step; the comparison of bytes is charged.
    fn name_for(
        &mut self,
        bytes: &Arc<[u8]>,
        wanted: Option<&str>,
        media: String,
        limits: &Limits,
        work: &mut WriteWork,
    ) -> Result<String, WriteError> {
        for picture in &self.made {
            work.charge_steps(1)?;
            if Arc::ptr_eq(&picture.bytes, bytes) {
                return Ok(picture.name.clone());
            }
            if picture.bytes.len() == bytes.len() {
                work.charge_copy(bytes.len(), limits)?;
                if picture.bytes == *bytes {
                    return Ok(picture.name.clone());
                }
            }
        }
        let fits = |name: &str| crate::write::shapes::entry_name_ok(name) && !super::reserved(name);
        let name = match wanted.filter(|w| fits(w)) {
            Some(w) if self.taken.insert(w.to_ascii_lowercase()) => w.to_owned(),
            _ => {
                let ext = crate::picturetype::picture_kind(bytes).map_or("bin", |k| k.ext);
                loop {
                    work.charge_steps(1)?;
                    let name = format!("Pictures/fill{}.{ext}", self.cursor);
                    self.cursor = self.cursor.saturating_add(1);
                    if self.taken.insert(name.to_ascii_lowercase()) {
                        break name;
                    }
                }
            }
        };
        self.made.push(FillPicture { name: name.clone(), bytes: Arc::clone(bytes), media });
        Ok(name)
    }
}

/// The URI `prefix` is bound to on `el` (its own declarations first) or where `bindings` are in force.
fn uri_on<'a>(el: &'a El, bindings: &'a Bindings, prefix: &str) -> Option<&'a str> {
    let own = el.attrs.iter().rev().find(|(k, _)| k.strip_prefix("xmlns:") == Some(prefix)).map(|(_, v)| v.as_str());
    own.or_else(|| bindings.get(prefix).map(String::as_str))
}

/// Whether `name`, on `el` where `bindings` are in force, is `local` of the namespace `uri`.
fn named(el: &El, bindings: &Bindings, name: &str, uri: &str, local_name: &str) -> bool {
    name.split_once(':').is_some_and(|(p, l)| l == local_name && uri_on(el, bindings, p) == Some(uri))
}

/// The media type of the manifest's row for the fill picture read from `name`: the row the model holds for it, else the type the picture's bytes say, else the one its name says.
fn fill_media(doc: &Document, name: &str, bytes: &[u8]) -> String {
    let held = doc.ext.get().and_then(|d| d.odf.manifest_rows.iter().find(|(n, _)| &**n == name).map(|(_, a)| a));
    if let Some(media) = held.and_then(|attrs| attrs.iter().find(|(k, _)| local(k) == "media-type").map(|(_, v)| v)).filter(|m| !m.is_empty()) {
        return media.to_owned();
    }
    match crate::picturetype::picture_kind(bytes) {
        Some(kind) => kind.media.to_owned(),
        None => crate::write::media_for(name).to_owned(),
    }
}

/// A fill image definition as the package made holds it, bounded and charged as [`tree`] makes a kept element (`None`: a prefix it uses is bound nowhere where it goes, left out and counted by
/// the caller), and whether a picture's entry goes with it: **its `xlink:href` names the entry the bytes are written as** (the href as read when it names that entry, set to it when it does not,
/// and put in with the attributes the schema asks for when the definition held its picture inline), and inline data is taken out, since the entry holds the bytes.
fn fill_image(
    definition: &Definition,
    doc: &Document,
    bindings: &Bindings,
    names: &mut FillNames,
    work: &mut WriteWork,
    limits: &Limits,
    left: &mut usize,
) -> Result<Option<El>, WriteError> {
    let Some(mut made) = tree(&definition.element, bindings, work, limits, left)? else { return Ok(None) };
    let Some(bytes) = definition.bytes.as_ref() else { return Ok(Some(made)) };
    let href_at = made.attrs.iter().position(|(k, _)| named(&made, bindings, k, XLINK_URI, "href"));
    let wanted = href_at.and_then(|at| made.attrs.get(at)).map(|(_, href)| {
        let decoded = crate::package::percent_decode(href);
        decoded.strip_prefix("./").unwrap_or(&decoded).to_owned()
    });
    let media = fill_media(doc, wanted.as_deref().unwrap_or_default(), bytes);
    let name = names.name_for(bytes, wanted.as_deref(), media, limits, work)?;
    // The inline data of a flat file: in a package the entry holds the bytes (ODF 1.4 Part 3 §16.42.6).
    let outer = made.attrs.clone();
    let holder = El { name: String::new(), attrs: outer, children: Vec::new() };
    made.children.retain(|n| !matches!(n, Node::El(e) if named(&holder, bindings, &e.name, OFFICE_URI, "binary-data")));
    match href_at {
        Some(_) if wanted.as_deref() == Some(name.as_str()) => {}
        Some(at) => {
            if let Some(slot) = made.attrs.get_mut(at) {
                slot.1.clone_from(&name);
            }
        }
        None => {
            let bound = bindings.iter().find(|(_, uri)| uri.as_str() == XLINK_URI).map(|(p, _)| p.clone());
            let prefix = match bound {
                Some(p) => p,
                None => {
                    made.attrs.push(("xmlns:xlink".to_owned(), XLINK_URI.to_owned()));
                    "xlink".to_owned()
                }
            };
            for (attr, value) in [("type", "simple"), ("href", name.as_str()), ("show", "embed"), ("actuate", "onLoad")] {
                let key = format!("{prefix}:{attr}");
                if !made.attrs.iter().any(|(k, _)| *k == key) {
                    made.attrs.push((key, value.to_owned()));
                }
            }
        }
    }
    work.charge_steps(made.attrs.len())?;
    Ok(Some(made))
}

/// The local name of an element or attribute name as written.
fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, l)| l)
}

/// The pages the `.odg` reader made, in the model's order: a page that is not a background and names the master page it was on.
pub(crate) fn slot_pages(doc: &Document) -> Vec<&Page> {
    doc.pages.iter().filter(|p| !p.background && p.ext.get().is_some_and(|d| d.odf.master_page.is_some())).map(|p| &**p).collect()
}

/// The font faces the `.odg` reader made: what each declaration held, its attributes and its children, as the file had them (borrowed: a face is copied only once it is bounded).
fn faces(doc: &Document) -> impl Iterator<Item = &Kept> {
    doc.fonts.iter().filter_map(|face| face.ext.get().map(|d| &d.odf)).filter(|kept| !(kept.raw.is_empty() && kept.kept.is_empty()))
}

/// Whether the model holds anything for `part` that this step writes.
pub(super) fn wants(part: &str, doc: &Document) -> bool {
    let Some(held) = doc.ext.get().map(|d| &d.odf) else { return false };
    let layers = !held.layers.is_empty();
    let styles = held.styles.iter().any(|s| container_of(part, &s.family).is_some() || (part == "styles.xml" && &*s.family == MASTER_FAMILY));
    let definitions = part == "styles.xml" && !held.definitions.is_empty();
    let sheets = part == "styles.xml" && super::sheets::count(doc) > 0;
    styles || definitions || sheets || (part == "styles.xml" && layers) || !slot_pages(doc).is_empty() || faces(doc).next().is_some()
}

/// What the model holds that this step does not write: the kept styles of the shapes' own families that no shape wrote (a shape's automatic style is written with the shape:
/// **a paragraph or run style is, under the name the file gave it, while the row it rests on is as it was**, and `written` names those). The fill images are written
/// And one that is not (a prefix nothing binds) is counted where it is left out: the first count is 0, kept for the note's form.
pub(super) fn unwritten(
    doc: &Document,
    written: &std::collections::HashSet<(bool, String)>,
    graphic: &std::collections::HashSet<String>,
) -> (usize, usize) {
    let Some(held) = doc.ext.get().map(|d| &d.odf) else { return (0, 0) };
    let images = 0;
    let styles = held
        .styles
        .iter()
        .filter(|s| {
            container_of("content.xml", &s.family).is_none() && container_of("styles.xml", &s.family).is_none() && &*s.family != MASTER_FAMILY
        })
        .filter(|s| {
            let text = match s.family.as_ref() {
                "auto:paragraph" => Some(false),
                "auto:text" => Some(true),
                _ => None,
            };
            let name: &str = &s.name;
            // A graphic style's kept attributes written with the style are no loss.
            if s.family.as_ref() == "auto:graphic" && graphic.contains(name) {
                return false;
            }
            !text.is_some_and(|is_text| written.contains(&(is_text, name.to_owned())))
        })
        .count();
    (images, styles)
}

/// Where a kept style stands: the container of `part` its family names, or `None` when it stands in the other part or is a shape's own.
fn container_of(part: &str, family: &str) -> Option<&'static str> {
    let (container, kind) = family.split_once(':')?;
    match (part, container, kind) {
        // A master page kept whole is written with the master pages, and not among the styles.
        ("styles.xml", "master", _) => None,
        // A shape's own style (graphic, paragraph, text): written with the shape.
        (_, "auto", "graphic" | "paragraph" | "text") => None,
        // An automatic style of those families that nothing names is kept whole, in the part it came from.
        ("content.xml", "unused", "graphic" | "paragraph" | "text") | ("styles.xml", "unused-styles", "graphic" | "paragraph" | "text") => {
            Some("automatic-styles")
        }
        ("styles.xml", "common", _) => Some("styles"),
        // A page layout is the styles part's own.
        ("styles.xml", "auto", "page-layout") => Some("automatic-styles"),
        ("content.xml", "auto", "page-layout") => None,
        ("content.xml", "auto", _) => Some("automatic-styles"),
        _ => None,
    }
}

/// The container of the part `part` a kept style of `family` is written in, by [`container_of`], but for an automatic drawing-page style, which goes in the part of what
/// names it, `named` (whether a master page names it, whether a page does): a master page's in `styles.xml`, and a page's, or one nothing names, in `content.xml`; one both name goes in both.
pub(crate) fn kept_container(part: &str, family: &str, (by_master, by_page): (bool, bool)) -> Option<&'static str> {
    if family != "auto:drawing-page" {
        return container_of(part, family);
    }
    match part {
        "styles.xml" if by_master => Some("automatic-styles"),
        "content.xml" if by_page || !by_master => Some("automatic-styles"),
        _ => None,
    }
}

/// The child of `root` whose local name is `name`.
fn child_mut<'a>(root: &'a mut El, name: &str) -> Option<&'a mut El> {
    root.children.iter_mut().find_map(|n| match n {
        Node::El(el) if local(&el.name) == name => Some(el),
        _ => None,
    })
}

/// Whether `prefix` is bound where an element's own declarations `own` are in force over `outer`: as [`binds`] says of the two together.
fn binds_over(own: &Bindings, outer: &Bindings, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "xml" {
        return true;
    }
    match own.get(prefix) {
        Some(uri) => !uri.is_empty(),
        None => binds(outer, prefix),
    }
}

/// The tree of a kept element, bounded and charged first, or `None` when a prefix it uses is bound nowhere where `bindings` are in force (it is left out, and counted by the caller): the
/// budget `left` goes down by what it comes to, and the copy is charged that, before it is made.
fn tree(el: &RawElement, bindings: &Bindings, work: &mut WriteWork, limits: &Limits, left: &mut usize) -> Result<Option<El>, WriteError> {
    work.charge_steps(1)?;
    let Some(bytes) = element_total(el, *left) else { return Err(WriteError::Unsupported(WORDS_KEPT)) };
    let free = free_prefixes(el).ok_or(WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_steps(free.steps)?;
    if !free.prefixes.iter().all(|p| binds(bindings, p)) {
        return Ok(None);
    }
    *left = left.saturating_sub(bytes);
    work.charge_copy(bytes, limits)?;
    Ok(Some(to_tree(el)))
}

/// A `style:font-face` made of what the reader kept of the declaration, as [`tree`] makes a kept element: bounded, checked and charged before it is copied.
fn face_tree(kept: &Kept, bindings: &Bindings, work: &mut WriteWork, limits: &Limits, left: &mut usize) -> Result<Option<El>, WriteError> {
    const NAME: &str = "style:font-face";
    work.charge_steps(1)?;
    let mut total = head_total(NAME, kept.raw.iter(), *left).ok_or(WriteError::Unsupported(WORDS_KEPT))?;
    for placed in kept.kept.iter() {
        let node = node_total(&placed.node, left.saturating_sub(total)).ok_or(WriteError::Unsupported(WORDS_KEPT))?;
        total = total.saturating_add(node);
    }
    let free = free_of(NAME, &kept.raw, kept.kept.iter().map(|p| &p.node)).ok_or(WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_steps(free.steps)?;
    if !free.prefixes.iter().all(|p| binds(bindings, p)) {
        return Ok(None);
    }
    *left = left.saturating_sub(total);
    work.charge_copy(total, limits)?;
    Ok(Some(El {
        name: NAME.to_owned(),
        attrs: kept.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        children: kept.kept.iter().map(|p| node_to_tree(&p.node)).collect(),
    }))
}

/// The attributes the model holds for an element this step makes (a page's, a master page's), checked, and the declarations among them: a name that is no qualified name, or a
/// declaration XML Namespaces does not allow, is refused in words; an attribute whose prefix neither the element's own declarations nor `outer` bind is left out and counted in `out`.
fn kept_attrs(attrs: &[(&str, &str)], outer: &Bindings, out: &mut usize) -> Result<(Vec<(String, String)>, Bindings), WriteError> {
    let mut own = Bindings::new();
    for (k, v) in attrs {
        if !is_qname(k) {
            return Err(WriteError::Unsupported(WORDS_NAMES));
        }
        if let Some(prefix) = declared_by(k) {
            if !declaration_ok(prefix, v) {
                return Err(WriteError::Unsupported(WORDS_NAMES));
            }
            own.insert(prefix.to_owned(), (*v).to_owned());
        }
    }
    let mut kept = Vec::with_capacity(attrs.len());
    for (k, v) in attrs {
        let bound = declared_by(k).is_some() || k.split_once(':').is_none_or(|(p, _)| binds_over(&own, outer, p));
        if bound {
            kept.push(((*k).to_owned(), (*v).to_owned()));
        } else {
            *out = out.saturating_add(1);
        }
    }
    Ok((kept, own))
}

/// A new `style:master-page` for the page `page` names: the attributes the master page had when it was read, or its name and its layout. The attributes are taken from what is left, and
/// charged, before they are copied; one whose prefix nothing binds there is left out ([`kept_attrs`]).
fn master_page(
    page: &Page,
    name: &str,
    outer: &Bindings,
    work: &mut WriteWork,
    limits: &Limits,
    left: &mut usize,
    out: &mut Left,
) -> Result<El, WriteError> {
    let odf = page.ext.get().map(|d| &d.odf);
    let held = odf.map(|o| &o.master_page_attrs);
    let head = head_total("style:master-page", held.into_iter().flat_map(|a| a.iter()), *left).ok_or(WriteError::Unsupported(WORDS_KEPT))?;
    *left = left.saturating_sub(head);
    work.charge_copy(head, limits)?;
    let pairs: Vec<(&str, &str)> = held.map(|a| a.iter().collect()).unwrap_or_default();
    let (mut attrs, _) = kept_attrs(&pairs, outer, &mut out.containers)?;
    // A master page the file wrote with no name is written with none, and the page that names it by `draw:master-page-name=""` still does: a name is made only for a page
    // that holds none of the master page's attributes and names a master page of a name.
    if !name.is_empty() && !attrs.iter().any(|(k, _)| local(k) == "name") {
        attrs.insert(0, ("style:name".to_owned(), name.to_owned()));
    }
    if !attrs.iter().any(|(k, _)| local(k) == "page-layout-name")
        && let Some(layout) = odf.and_then(|o| o.layout.as_ref())
    {
        attrs.push(("style:page-layout-name".to_owned(), layout.to_string()));
    }
    Ok(El { name: "style:master-page".to_owned(), attrs, children: Vec::new() })
}

/// The master pages of `styles.xml` (`masters`, the master styles of the part made) for a model whose `.odg` pages are `slots`: the one the package was made with goes when there are such
/// pages, and each master page they name comes, once, in the order the pages first name it, with the attributes the page holds of it; **the master pages the reader kept whole**
/// give the children of the first of each name, and the ones no page names follow, whole. A piece whose prefix nothing binds there is left out and counted in `out`.
#[allow(clippy::too_many_arguments)]
fn put_master_pages(
    masters: &mut El,
    held: &OdfDoc,
    slots: &[&Page],
    masters_at: &Bindings,
    work: &mut WriteWork,
    limits: &Limits,
    left: &mut usize,
    out: &mut Left,
) -> Result<(), WriteError> {
    // The master pages the reader kept whole: a page's master page takes its children from the first of its name, and the ones no page names follow, whole.
    // Each is found by its name in one lookup (a map made once, a step for each), the first of a name that no page took before.
    let kept_masters: Vec<&RawElement> = held.styles.iter().filter(|s| &*s.family == MASTER_FAMILY).map(|s| &*s.element).collect();
    work.charge_steps(kept_masters.len())?;
    let mut by_name: std::collections::HashMap<&str, std::collections::VecDeque<usize>> = std::collections::HashMap::new();
    for (at, el) in kept_masters.iter().enumerate() {
        by_name.entry(el.attrs.get("style:name").unwrap_or_default()).or_default().push_back(at);
    }
    let mut taken = vec![false; kept_masters.len()];
    if !slots.is_empty() {
        // The master page the package was made with goes, and each the pages name comes, once, in the order the pages first name it.
        masters.children.retain(|n| !matches!(n, Node::El(el) if local(&el.name) == "master-page"));
        let mut seen: HashSet<&str> = HashSet::new();
        for page in slots {
            let Some(name) = page.ext.get().and_then(|d| d.odf.master_page.as_deref()) else { continue };
            if !seen.insert(name) {
                continue;
            }
            work.charge_steps(1)?;
            let mut made = master_page(page, name, masters_at, work, limits, left, out)?;
            let found = by_name.get_mut(name).and_then(std::collections::VecDeque::pop_front);
            if let Some((el, taken)) = found.and_then(|at| kept_masters.get(at).zip(taken.get_mut(at))) {
                *taken = true;
                made.children = kept_children(el, &made, masters_at, work, limits, left, out)?;
            }
            masters.children.push(Node::El(made));
        }
    }
    for (el, taken) in kept_masters.iter().zip(&taken) {
        if *taken {
            continue;
        }
        match tree(el, masters_at, work, limits, left)? {
            Some(made) => masters.children.push(Node::El(made)),
            None => out.containers = out.containers.saturating_add(1),
        }
    }
    Ok(())
}

/// The children of a master page the reader kept whole (`kept`), for the master page `made` of its name that the model writes from a page's data: each taken from what is left of the budget by
/// its size, and its copy charged that size, before it is made; one whose prefix nothing binds there is left out and counted (as a page's kept children are, [`draw_page`]).
fn kept_children(
    kept: &RawElement,
    made: &El,
    outer: &Bindings,
    work: &mut WriteWork,
    limits: &Limits,
    left: &mut usize,
    out: &mut Left,
) -> Result<Vec<Node>, WriteError> {
    let mut own = Bindings::new();
    for (k, v) in &made.attrs {
        if let Some(prefix) = declared_by(k) {
            own.insert(prefix.to_owned(), v.clone());
        }
    }
    let mut children = Vec::with_capacity(kept.children.len());
    for node in &kept.children {
        work.charge_steps(1)?;
        let Some(bytes) = node_total(node, *left) else { return Err(WriteError::Unsupported(WORDS_KEPT)) };
        if let RawNode::Element(el) = node {
            let free = free_prefixes(el).ok_or(WriteError::Unsupported(WORDS_NAMES))?;
            work.charge_steps(free.steps)?;
            if !free.prefixes.iter().all(|p| binds_over(&own, outer, p)) {
                out.containers = out.containers.saturating_add(1);
                continue;
            }
        }
        *left = left.saturating_sub(bytes);
        work.charge_copy(bytes, limits)?;
        children.push(node_to_tree(node));
    }
    Ok(children)
}

/// A new `draw:page` for a page the `.odg` reader made: `draw:name`, the attributes it kept and the children it kept whole. **The element, then each child, is taken from what is left of
/// the budget by its size, and its copy charged that size, before it is made**; a child or an attribute whose prefix nothing binds where it goes is left out and
/// counted.
fn draw_page(page: &Page, outer: &Bindings, work: &mut WriteWork, limits: &Limits, left: &mut usize, out: &mut Left) -> Result<El, WriteError> {
    let odf = page.ext.get().map(|d| &d.odf);
    let raw = odf.map(|o| &o.kept.raw);
    let head = head_total("draw:page", std::iter::once(("draw:name", &*page.name)).chain(raw.into_iter().flat_map(|r| r.iter())), *left)
        .ok_or(WriteError::Unsupported(WORDS_KEPT))?;
    *left = left.saturating_sub(head);
    work.charge_copy(head, limits)?;
    let pairs: Vec<(&str, &str)> = raw.map(|r| r.iter().collect()).unwrap_or_default();
    let (kept, own) = kept_attrs(&pairs, outer, &mut out.containers)?;
    let mut attrs: Vec<(String, String)> = vec![("draw:name".to_owned(), page.name.to_string())];
    attrs.extend(kept);
    let mut children: Vec<Node> = Vec::new();
    for placed in odf.map(|o| o.kept.kept.iter()).into_iter().flatten() {
        work.charge_steps(1)?;
        let Some(bytes) = node_total(&placed.node, *left) else { return Err(WriteError::Unsupported(WORDS_KEPT)) };
        if let RawNode::Element(el) = &placed.node {
            let free = free_prefixes(el).ok_or(WriteError::Unsupported(WORDS_NAMES))?;
            work.charge_steps(free.steps)?;
            if !free.prefixes.iter().all(|p| binds_over(&own, outer, p)) {
                out.containers = out.containers.saturating_add(1);
                continue;
            }
        }
        *left = left.saturating_sub(bytes);
        work.charge_copy(bytes, limits)?;
        children.push(node_to_tree(&placed.node));
    }
    Ok(El { name: "draw:page".to_owned(), attrs, children })
}

/// Puts the model's document data into the part `part` (`content.xml` or `styles.xml`) whose tree is `root`. Gives what was left out, by kind: pieces with no place, and pieces whose
/// prefixes nothing binds where they would go.
pub(super) fn put(
    part: &str,
    root: &mut El,
    doc: &Document,
    limits: &Limits,
    work: &mut WriteWork,
    pictures: &mut FillNames,
) -> Result<Left, WriteError> {
    let Some(held): Option<&OdfDoc> = doc.ext.get().map(|d| &d.odf) else { return Ok(Left::default()) };
    let mut out = Left::default();
    let mut left = MOST;
    let slots = slot_pages(doc);
    // The bindings in force in each container a piece goes in, found once, before the tree is changed: the root's and the containers' declarations, which are the file's (each is a
    // walk of the declarations on the way, charged).
    work.charge_steps(root.attrs.len().saturating_mul(5))?;
    let faces_at = in_force(root, &["font-face-decls"]);
    let styles_at = in_force(root, &["styles"]);
    let auto_at = in_force(root, &["automatic-styles"]);
    let masters_at = in_force(root, &["master-styles"]);
    let drawing_at = in_force(root, &["body", "drawing"]);
    // The font faces: the same declarations in both parts, as the programs that write both write them.
    if faces(doc).next().is_some()
        && let Some(decls) = child_mut(root, "font-face-decls")
    {
        for face in faces(doc) {
            match face_tree(face, &faces_at, work, limits, &mut left)? {
                Some(made) => decls.children.push(Node::El(made)),
                None => out.containers = out.containers.saturating_add(1),
            }
        }
    }
    if part == "styles.xml"
        && let Some(styles) = child_mut(root, "styles")
    {
        for definition in held.definitions.iter() {
            if definition.family == DefFamily::FillImage {
                // With its picture's entry.
                match fill_image(definition, doc, &styles_at, pictures, work, limits, &mut left)? {
                    Some(made) => styles.children.push(Node::El(made)),
                    None => out.definitions = out.definitions.saturating_add(1),
                }
                continue;
            }
            match tree(&definition.element, &styles_at, work, limits, &mut left)? {
                Some(made) => styles.children.push(Node::El(made)),
                None => out.definitions = out.definitions.saturating_add(1),
            }
        }
    }
    // The style sheets the `.odg` reader made, as the common styles they were (`sheets.rs`): after the kept common styles, in the model's order, which is the order the
    // reader numbers them in.
    let sheets =
        if part == "styles.xml" { super::sheets::common_styles(doc, &styles_at, limits, work, &mut left)? } else { (Vec::new(), Default::default()) };
    // An automatic drawing-page style goes in the part of the element that names it: a master page's in `styles.xml`, where the master page finds it, and a page's, or one
    // nothing names, in `content.xml`. The reader holds both parts' automatic styles under one word, so the names tell the part.
    let named_by = |attrs: &diacraft_doc::RawAttrs| attrs.get("draw:style-name").map(str::to_owned);
    work.charge_steps(slots.len().saturating_add(held.styles.len()))?;
    let page_styles: HashSet<String> = slots.iter().filter_map(|p| p.ext.get().and_then(|d| named_by(&d.odf.kept.raw))).collect();
    let master_styles: HashSet<String> = slots
        .iter()
        .filter_map(|p| p.ext.get().and_then(|d| named_by(&d.odf.master_page_attrs)))
        .chain(held.styles.iter().filter(|s| &*s.family == MASTER_FAMILY).filter_map(|s| named_by(&s.element.attrs)))
        .collect();
    for kept in held.styles.iter() {
        let named = (master_styles.contains(&*kept.name), page_styles.contains(&*kept.name));
        let Some(container) = kept_container(part, &kept.family, named) else { continue };
        let bindings = if container == "styles" { &styles_at } else { &auto_at };
        let Some(target) = child_mut(root, container) else {
            out.containers = out.containers.saturating_add(1);
            continue;
        };
        match tree(&kept.element, bindings, work, limits, &mut left)? {
            Some(mut made) => {
                // A style written whole holds what a file of an earlier version takes and 1.4 does not: the form 1.4 takes, the file's carried ([`super::forms`]).
                out.styles = out.styles.saturating_add(super::forms::in_1_4_form(&mut made, bindings, work)?);
                target.children.push(Node::El(made));
            }
            None => out.styles = out.styles.saturating_add(1),
        }
    }
    let (sheet_styles, sheets_written) = sheets;
    if !sheet_styles.is_empty() {
        match child_mut(root, "styles") {
            Some(styles) => styles.children.extend(sheet_styles.into_iter().map(Node::El)),
            None => out.containers = out.containers.saturating_add(sheet_styles.len()),
        }
    }
    out.styles = out.styles.saturating_add(sheets_written.left);
    // The dashes of DiaCraft's own name that the style sheets name get their definitions, where the definitions the model holds, as they were read, have none of that name: a
    // style sheet whose line pattern was edited names a dash the file it was read from did not define, and another program draws what the definition says.
    if !sheets_written.dashes.is_empty()
        && let Some(styles) = child_mut(root, "styles")
    {
        let defined: HashSet<String> = styles
            .children
            .iter()
            .filter_map(|n| match n {
                Node::El(el) if local(&el.name) == "stroke-dash" => el.attrs.iter().find(|(k, _)| local(k) == "name").map(|(_, v)| v.clone()),
                _ => None,
            })
            .collect();
        work.charge_steps(styles.children.len())?;
        for key in &sheets_written.dashes {
            if defined.contains(&key.name()) {
                continue;
            }
            work.charge_steps(1)?;
            if let Some(attributes) = key.attributes() {
                let attrs = attributes.into_iter().map(|(name, value)| (format!("draw:{name}"), value)).collect();
                styles.children.push(Node::El(El { name: "draw:stroke-dash".to_owned(), attrs, children: Vec::new() }));
            }
        }
    }
    if part == "styles.xml" {
        if let Some(masters) = child_mut(root, "master-styles") {
            if !held.layers.is_empty() {
                masters.children.retain(|n| !matches!(n, Node::El(el) if local(&el.name) == "layer-set"));
                let mut set = El { name: "draw:layer-set".to_owned(), attrs: Vec::new(), children: Vec::new() };
                for layer in held.layers.iter() {
                    match tree(layer, &masters_at, work, limits, &mut left)? {
                        Some(made) => set.children.push(Node::El(made)),
                        None => out.layers = out.layers.saturating_add(1),
                    }
                }
                masters.children.insert(0, Node::El(set));
            }
            put_master_pages(masters, held, &slots, &masters_at, work, limits, &mut left, &mut out)?;
        }
        if !slots.is_empty()
            && let Some(auto) = child_mut(root, "automatic-styles")
        {
            // The layout the package was made with goes when the model holds layouts of its own (the kept styles above put them in).
            let own = held.styles.iter().any(|s| &*s.family == "auto:page-layout");
            if own {
                // The kept layouts were pushed last: the layouts the package made are the ones before them.
                let kept = held.styles.iter().filter(|s| &*s.family == "auto:page-layout").count();
                let total = auto.children.iter().filter(|n| matches!(n, Node::El(el) if local(&el.name) == "page-layout")).count();
                let mut drop = total.saturating_sub(kept);
                auto.children.retain(|n| {
                    if drop > 0 && matches!(n, Node::El(el) if local(&el.name) == "page-layout") {
                        drop -= 1;
                        return false;
                    }
                    true
                });
            }
        }
    }
    if part == "content.xml" && !slots.is_empty() {
        let drawing = child_mut(root, "body").and_then(|b| child_mut(b, "drawing"));
        match drawing {
            Some(drawing) => {
                drawing.children.retain(|n| !matches!(n, Node::El(el) if local(&el.name) == "page"));
                for page in &slots {
                    work.charge_steps(1)?;
                    drawing.children.push(Node::El(draw_page(page, &drawing_at, work, limits, &mut left, &mut out)?));
                }
            }
            None => out.containers = out.containers.saturating_add(slots.len()),
        }
    }
    Ok(out)
}
