//! The document's own ODF data, held as the reader goes: the named definitions of `office:styles`, the layer set, the style elements no style sheet is
//! made of (kept whole, by family and name) and the units (the native-format design's §3.4). It ends in `Document.ext.odf`.
//!
//! # What is held where
//!
//! * **Definitions** (`OdfDoc.definitions`): each `draw:marker`, `draw:gradient`, `draw:opacity`, `draw:hatch`, `draw:stroke-dash` and `draw:fill-image` of `office:styles`
//!   that has a `draw:name`, as the element was written, in file order. At most [`MAX_DEFINITIONS`] of each family: **a file with more is refused** (`TooComplex`), since a
//!   model holds no more and a cut would lose what the `.diacraft` copy must keep. A fill image's bytes are the picture its `xlink:href` names (a part of the package) or
//!   its inline `office:binary-data`, read once for every definition that names the same part, **charged to the media budget before they are copied**, and `None` when the
//!   picture is not in the package or does not fit (a warning).
//! * **The layer set** (`OdfDoc.layers`): each `draw:layer` of the document's `draw:layer-set` (`office:master-styles`), whole and in order, the producers' five included.
//! * **The other style families** (`OdfDoc.styles`): every style element that is no style sheet, whole: the common styles of the text family and every default style, the list
//!   styles and data styles, the page layouts, and the automatic styles of the drawing-page and other families (a page's, a presentation's). **An automatic style of the graphic,
//!   paragraph and text families** (a shape's style, a paragraph's, a run's) **is kept as what no cell reads** ([`super::styles`], `remainder_style`): its own attributes, and for each of
//!   its properties elements the attributes and children no cell reads, as an element of the same name; the attributes a cell reads are in the cells, and a value the style gives a
//!   shape, a paragraph or a run is held with it, so a style that holds only what cells read is **kept as nothing** (a style of its own for each shape would be charged twice, by the
//!   cells and by the element). The common graphic and paragraph styles are style sheets of the model: what they hold that no cell reads is in their own slot (`Style.ext.odf`), and
//!   they are not kept here.
//!   **The family text** of a [`KeptStyle`] is `<container>:<kind>`: the container is `common` (`office:styles`), `master` (a master page of `office:master-styles` that holds no
//!   shape, kept whole, `pages.rs`) or `auto` (an `office:automatic-styles` of either part: the same word for `styles.xml`'s and `content.xml`'s, so that a flat
//!   drawing, which has one container of them, reads as the package that holds the same elements; `styles.xml`'s are held first), and the kind is the
//!   `style:family` of a `style:style` or
//!   `style:default-style` (`graphic`, `paragraph`, `text`, `drawing-page`, `presentation`, ...) or the local name of any other element (`list-style`, `page-layout`,
//!   `number-style`, ...). A default style has the name `""` (the element's own name, `style:default-style`, says what it is). Two styles of one container, family and name are
//!   both held, in file order (LibreOffice gives the automatic styles of `styles.xml` names of their own, `M...`, so a package that holds one name in both parts is rare).
//!
//! # Units
//!
//! The cell's own unit is used where the cell takes the value (every length a cell takes holds the unit it was written in, `cm`, `mm`, `in`, `pt` and `pc` as `CM`, `MM`,
//! `IN`, `PT` and `P`), and **`units` holds the unit of a length that no cell spells: a `px` length** (a cell has no unit for it) **and a length written in another
//! unit than the cell that takes its value** (a shape's `svg:y` takes `PinY`, whose unit is `svg:x`'s). Its keys are the attributes' names as written, its values the
//! unit as written (`px`, `cm`, ...). A length that is only held as text (a held value's attribute, a kept attribute) keeps its unit in the text.
//!
//! # Cost
//!
//! Each element is charged before it is copied (`shapes::element_cost`, which counts its items and its bytes), and a definition or a kept style is one item more for its
//! own record. Nothing is walked for each shape.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{DefFamily, Definition, KeptStyle, MAX_DEFINITIONS, RawElement};

use super::Reader;
use super::names::{Names, Ns};
use super::shapes::{element_cost, raw_element};
use super::styles::File;
use crate::error::OdgError;
use crate::package::Package;

/// The items a definition's or a kept style's own record costs: the record (a family, two names and a pointer, and its place in the list) and the allocation of the element
/// it points at.
const RECORD_ITEMS: usize = 2;

/// Which container of styles an element is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Container {
    /// `office:styles`.
    Common,
    /// `office:automatic-styles` of either part.
    Auto,
    /// `office:master-styles`: a master page that holds no shape, kept whole.
    Master,
}

impl Container {
    /// The container an element read from `_file` is in: a common style (`office:styles`) or an automatic one.
    pub(crate) fn of(_file: File, common: bool) -> Container {
        if common { Container::Common } else { Container::Auto }
    }

    fn word(self) -> &'static str {
        match self {
            Container::Common => "common",
            Container::Auto => "auto",
            Container::Master => "master",
        }
    }
}

/// The families of definitions, in the order of their counters.
const FAMILIES: [(&str, DefFamily, &str); 6] = [
    ("marker", DefFamily::Marker, "marker definitions"),
    ("gradient", DefFamily::Gradient, "gradient definitions"),
    ("opacity", DefFamily::Opacity, "opacity definitions"),
    ("hatch", DefFamily::Hatch, "hatch definitions"),
    ("stroke-dash", DefFamily::StrokeDash, "stroke dash definitions"),
    ("fill-image", DefFamily::FillImage, "fill image definitions"),
];

/// The document's ODF data as the reader gathers it. See the module documentation.
#[derive(Default)]
pub(crate) struct DocData {
    /// The named definitions, in file order.
    pub definitions: Vec<Definition>,
    /// How many of each family: the counters the cap is checked against, before a definition is made.
    counts: [usize; 6],
    /// The style elements kept whole.
    pub styles: Vec<KeptStyle>,
    /// The document's layer set: each `draw:layer`, whole.
    pub layers: Vec<Arc<RawElement>>,
    /// The automatic styles of `content.xml` that name a drawing page's style, by name: shared with [`DocData::styles`], so that a page holds the same element it names.
    drawing_pages: HashMap<String, Arc<RawElement>>,
}

impl DocData {
    /// Holds `el` whole as a style element of `container`, if it is not one the caller keeps otherwise. Charged before it is copied.
    pub(crate) fn keep_style<'t>(&mut self, r: &mut Reader, names: &Names<'t>, container: Container, el: &'t El) -> Result<(), OdgError> {
        let (ns, local) = names.element(el);
        let attrs = names.attrs(el);
        let kind = match (ns, attrs.peek(Ns::Style, "family")) {
            (Ns::Style, Some(family)) if matches!(local, "style" | "default-style") => family,
            _ => local,
        };
        let name = attrs.peek(Ns::Style, "name").or_else(|| attrs.peek(Ns::Draw, "name")).unwrap_or_default();
        let family = format!("{}:{kind}", container.word());
        r.take_items(RECORD_ITEMS.saturating_add(element_cost(el)))?;
        r.take_bytes(family.len().saturating_add(name.len()))?;
        let element = Arc::new(raw_element(el));
        if container == Container::Auto && kind == "drawing-page" && !name.is_empty() {
            self.drawing_pages.entry(name.to_owned()).or_insert_with(|| Arc::clone(&element));
        }
        self.styles.push(KeptStyle { family: Arc::from(family), name: Arc::from(name), element });
        Ok(())
    }

    /// Holds `element`, what an automatic style of the graphic, paragraph or text family holds that no cell reads, as a style of `container` and `kind` (already charged).
    pub(crate) fn push_style(&mut self, container: Container, kind: &str, name: &str, element: RawElement) {
        let family = format!("{}:{kind}", container.word());
        self.styles.push(KeptStyle { family: Arc::from(family), name: Arc::from(name), element: Arc::new(element) });
    }

    /// The automatic drawing-page style called `name` in `content.xml`, as held.
    pub(crate) fn drawing_page(&self, name: &str) -> Option<&Arc<RawElement>> {
        self.drawing_pages.get(name)
    }

    /// Holds `el` as a definition, when it is one: a `draw:marker`, `draw:gradient`, `draw:opacity`, `draw:hatch`, `draw:stroke-dash` or `draw:fill-image` with a
    /// `draw:name`. Gives whether it was held. Refused past [`MAX_DEFINITIONS`] of its family, before it is made.
    pub(crate) fn definition<'t>(&mut self, r: &mut Reader, names: &Names<'t>, el: &'t El) -> Result<bool, OdgError> {
        let (ns, local) = names.element(el);
        if ns != Ns::Draw {
            return Ok(false);
        }
        let Some((at, (_, family, what))) = FAMILIES.iter().enumerate().find(|(_, (word, _, _))| *word == local) else { return Ok(false) };
        let attrs = names.attrs(el);
        let Some(name) = attrs.peek(Ns::Draw, "name") else { return Ok(false) };
        let count = self.counts.get_mut(at).ok_or(OdgError::TooComplex { what, max: MAX_DEFINITIONS as u64 })?;
        if *count >= MAX_DEFINITIONS {
            return Err(OdgError::TooComplex { what, max: MAX_DEFINITIONS as u64 });
        }
        *count += 1;
        r.take_items(RECORD_ITEMS.saturating_add(element_cost(el)))?;
        r.take_bytes(name.len())?;
        self.definitions.push(Definition { family: *family, name: Arc::from(name), element: Arc::new(raw_element(el)), bytes: None });
        Ok(true)
    }

    /// Holds each `draw:layer` of `set`, the document's layer set, whole and in order (the first set of the document only). Charged before it is copied.
    pub(crate) fn layer_set<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, set: &'t El) -> Result<(), OdgError> {
        for child in set.elements() {
            let mark = names.enter(child);
            if names.is(child, Ns::Draw, "layer") {
                r.take_items(RECORD_ITEMS.saturating_add(element_cost(child)))?;
                self.layers.push(Arc::new(raw_element(child)));
            }
            names.leave(mark);
        }
        Ok(())
    }

    /// Reads the bytes of each fill image's picture (see the module documentation). `package` is the package the drawing was read from, `None` for a flat drawing, whose
    /// pictures are inline.
    pub(crate) fn load_fill_images(&mut self, r: &mut Reader, package: Option<&Package>) -> Result<HashSet<String>, OdgError> {
        // Each part's bytes once: two definitions that name one picture share one copy, charged once.
        let mut loaded: HashMap<String, Option<Arc<[u8]>>> = HashMap::new();
        for definition in &mut self.definitions {
            if definition.family != DefFamily::FillImage {
                continue;
            }
            let bytes = fill_image_bytes(r, package, &definition.element, &mut loaded)?;
            definition.bytes = bytes;
        }
        // The package entries whose bytes a definition holds: they are pictures of the model, and no part of `Document.parts` beside (the `keep::apply` leaves out
        // the entries it is given).
        Ok(loaded.into_iter().filter_map(|(name, bytes)| bytes.map(|_| name)).collect())
    }
}

/// The bytes of one fill image: its inline `office:binary-data`, or the package part its `xlink:href` names (the package's own bytes, shared and not copied).
fn fill_image_bytes(
    r: &mut Reader,
    package: Option<&Package>,
    element: &RawElement,
    loaded: &mut HashMap<String, Option<Arc<[u8]>>>,
) -> Result<Option<Arc<[u8]>>, OdgError> {
    // Inline data wins over an href beside it, as for a picture.
    for child in &element.children {
        if let diacraft_doc::RawNode::Element(e) = child
            && e.name.rsplit(':').next() == Some("binary-data")
        {
            let pieces: Vec<&str> = e
                .children
                .iter()
                .filter_map(|n| match n {
                    diacraft_doc::RawNode::Text(t) => Some(&**t),
                    _ => None,
                })
                .collect();
            let bytes = super::shapes::inline_bytes(r, &pieces);
            if bytes.is_none() {
                r.warn("fill_image_unreadable", || {
                    "a fill image's inline data is not base64 or does not fit the picture budget; its bytes are not held".to_owned()
                });
            }
            return Ok(bytes);
        }
    }
    let Some(href) = element.attrs.get("xlink:href") else { return Ok(None) };
    let Some(package) = package else { return Ok(None) };
    let Some(part) = super::shapes::resolve_href(package, href) else {
        r.warn("fill_image_unreadable", || "a fill image names a picture the package does not hold; its bytes are not held".to_owned());
        return Ok(None);
    };
    if let Some(done) = loaded.get(&part) {
        return Ok(done.clone());
    }
    let made = package.part_shared(&part).filter(|bytes| take_media(r, bytes.len()));
    loaded.insert(part, made.clone());
    Ok(made)
}

/// Takes `n` bytes of the media budget for a copy of a picture, before it is made; says so, once, when the picture does not fit.
fn take_media(r: &mut Reader, n: usize) -> bool {
    let n = n as u64;
    match r.media_left.checked_sub(n) {
        Some(left) => {
            r.media_left = left;
            r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(n);
            true
        }
        None => {
            r.warn_once("fill_image_too_large", || "a fill image does not fit the picture budget; its bytes are not held".to_owned());
            false
        }
    }
}

/// Ends the document's ODF data: the fill images' bytes are read, and what was gathered goes into the document's slot (`Document.ext.odf`), charged before the slot is made.
/// A document that holds none has no slot. Gives the names of the package entries a fill image's bytes are, which are not also parts of the model.
pub(crate) fn finish(
    r: &mut Reader,
    doc: &mut diacraft_doc::Document,
    mut data: DocData,
    package: Option<&Package>,
) -> Result<HashSet<String>, OdgError> {
    let entries = data.load_fill_images(r, package)?;
    if data.definitions.is_empty() && data.styles.is_empty() && data.layers.is_empty() {
        return Ok(entries);
    }
    r.take_items(super::cost::EXT_DOC)?;
    let slot = &mut doc.ext.make_mut().odf;
    slot.definitions = data.definitions;
    slot.styles = data.styles;
    slot.layers = data.layers;
    Ok(entries)
}
