//! Pages, master pages and layers, and what `meta.xml` and `settings.xml`
//! give (the generator; the scale).
//!
//! * **A page** takes its size, margins and orientation from its master page's layout, `draw:name` as its names (`Page-<n>` for a page
//!   with none, `n` its ordinal from 1, our choice: the form a new page of DiaCraft's takes), and `ID`s 0, 1, 2, ... in document order.
//!   `PageScale` and `DrawingScale` are `ScaleNumerator` and `ScaleDenominator` in the unit of `fo:page-width`, 1 and 1 when absent.
//! * **A master page holding shapes** is a background page (`Background` 1) named after its display name, holding them, and every page on it
//!   has `BackPage` naming it. Background pages take the `ID`s after the drawing's pages, in master page order.
//! * **Layers**: the layer set of `office:master-styles`, less LibreOffice's five, is a `Layer` row on every page, in set order;
//!   a page with a layer set of its own has those rows instead.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{Cell, CellName, Document, OdfPage, Page, RawAttrs, RawElement, SharedVec, Sheet};

use super::Reader;
use super::alt;
use super::cells;
use super::flat::Taken;
use super::geometry::Budget;
use super::layers::{self, Layer};
use super::names::{Names, Ns};
use super::shapes::{self, Cx, PageCx};
use super::styles::{File, PageLayout, StyleSet};
use super::units::{self, LenUnit, Length};
use super::unread::{self, Link};
use crate::error::{LimitKind, Missing, OdgError};
use crate::flat::Child;
use crate::index::{PageShapes, PageSource, ShapeIndex};
use crate::report::shown;

/// `meta:generator` of `meta.xml`, cut short.
pub(crate) fn generator(r: &mut Reader, tree: &El) -> Result<Option<String>, OdgError> {
    let mut names = Names::default();
    let mark = names.enter(tree);
    if !names.is(tree, Ns::Office, "document-meta") {
        return Err(OdgError::UnexpectedRoot { part: "meta.xml" });
    }
    let found = generator_in(r, &mut names, tree);
    names.leave(mark);
    Ok(found)
}

/// The generator among the `office:meta` children of `holder`, whose bindings `names` holds: `meta.xml`'s root.
pub(crate) fn generator_in<'t>(r: &mut Reader, names: &mut Names<'t>, holder: &'t El) -> Option<String> {
    let mut found = None;
    for meta in holder.elements() {
        r.saw_element();
        let m = names.enter(meta);
        if found.is_none() && names.is(meta, Ns::Office, "meta") {
            found = generator_of(r, names, meta);
        }
        names.leave(m);
    }
    found
}

/// The first `meta:generator` among the children of `meta`, an `office:meta` whose own bindings `names` holds (also a flat drawing's first one).
pub(crate) fn generator_of<'t>(r: &mut Reader, names: &mut Names<'t>, meta: &'t El) -> Option<String> {
    let mut found = None;
    for item in meta.elements() {
        r.saw_element();
        let i = names.enter(item);
        if found.is_none() && names.is(item, Ns::Meta, "generator") {
            found = Some(shown(&item.text()));
        }
        names.leave(i);
    }
    found
}

/// `ScaleNumerator` and `ScaleDenominator` of `ooo:configuration-settings` in `settings.xml`, when both are there.
pub(crate) fn scale(r: &mut Reader, tree: &El) -> Result<Option<(f64, f64)>, OdgError> {
    let mut names = Names::default();
    let mark = names.enter(tree);
    if !names.is(tree, Ns::Office, "document-settings") {
        return Err(OdgError::UnexpectedRoot { part: "settings.xml" });
    }
    let found = scale_from(r, &names, children(tree, false), "settings.xml");
    names.leave(mark);
    Ok(found)
}

/// The elements under `el` as walk entries, the first last (the walk pops them), each marked `inside` or not.
fn children(el: &El, inside: bool) -> Vec<(&El, bool)> {
    let mut kids: Vec<(&El, bool)> = el.elements().map(|e| (e, inside)).collect();
    kids.reverse();
    kids
}

/// The drawing scale of a flat drawing's `office:settings` (it is `settings.xml`'s).
pub(crate) fn scale_in<'t>(r: &mut Reader, names: &Names<'t>, settings: &'t El) -> Option<(f64, f64)> {
    scale_from(r, names, vec![(settings, false)], "office:settings")
}

/// The scale the walk of `stack` finds: the item sets nest (a map of view entries); the walk keeps its own stack, in document order (children
/// pushed last first, so that the first item of a name is the one read), and reads only the set named for the configuration.
fn scale_from<'t>(r: &mut Reader, names: &Names<'t>, mut stack: Vec<(&'t El, bool)>, part: &str) -> Option<(f64, f64)> {
    let (mut numerator, mut denominator) = (None, None);
    while let Some((el, inside)) = stack.pop() {
        r.saw_element();
        let (ns, local) = names.element(el);
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let name = attrs.peek(Ns::Config, "name");
        if ns == Ns::Config && local == "config-item" && inside {
            let value = || units::number_any(el.text().trim());
            match name {
                Some("ScaleNumerator") if numerator.is_none() => numerator = value(),
                Some("ScaleDenominator") if denominator.is_none() => denominator = value(),
                _ => {}
            }
            continue;
        }
        let here = inside || (ns == Ns::Config && local == "config-item-set" && name == Some("ooo:configuration-settings"));
        stack.extend(children(el, here));
    }
    match (numerator, denominator) {
        (Some(n), Some(d)) if n > 0.0 && d > 0.0 => Some((n, d)),
        (None, None) => None,
        _ => {
            r.warn("scale_unreadable", || format!("the drawing scale in {part} cannot be read; 1 to 1 is used"));
            None
        }
    }
}

/// A master page of `styles.xml`.
pub(crate) struct Master<'t> {
    pub name: String,
    pub display: Option<String>,
    pub layout: Option<String>,
    /// What a page on this master page holds of it: its name, its layout's name and its own attributes, as written.
    odf: MasterOdf,
    el: &'t El,
    /// Its own layer set, if it has one.
    layers: Option<Vec<Layer>>,
    holds_shapes: bool,
}

/// The master pages of `styles.xml`, and the element that holds them, for the second pass that reads their shapes.
pub(crate) struct Masters<'t> {
    pub list: Vec<Master<'t>>,
    holder: Option<&'t El>,
}

/// A master page's own data that every page on it holds, shared by the pages (the strings and the list behind a pointer).
#[derive(Clone, Default)]
pub(crate) struct MasterOdf {
    name: Option<Arc<str>>,
    layout: Option<Arc<str>>,
    attrs: RawAttrs,
}

/// What a master page gives the pages on it: its layout, and its background page when it holds shapes.
pub(crate) struct MasterOut {
    pub name: String,
    pub layout: Option<String>,
    odf: MasterOdf,
    /// The background page made of it, with its index, when it holds shapes.
    pub background: Option<(Page, PageShapes)>,
}

/// Step 3 of the read: the font faces, `office:styles`, `office:automatic-styles` and `office:master-styles` of `styles.xml`.
///
/// **A flat drawing** (`flat` is `Some`) holds the part's children and `content.xml`'s in one tree: the first `office:font-face-decls`,
/// `office:styles`, `office:automatic-styles` and `office:master-styles` are read, the other children are not this part's, and the one container of automatic
/// styles and of faces is read as `content.xml`'s is (`File::Content`), which is what the pages' shapes and the master pages' shapes both look in first.
pub(crate) fn read_styles_part<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    tree: &'t El,
    set: &mut StyleSet,
    flat: Option<&Taken<'t>>,
) -> Result<Masters<'t>, OdgError> {
    let mut masters = Masters { list: Vec::new(), holder: None };
    let auto = if flat.is_some() { File::Content } else { File::Styles };
    // What no field reads is kept for the `.diacraft` copy of a packaged drawing; a flat drawing is never saved as one.
    let hold = flat.is_none();
    // How many children of the root the reader has read, for the place of one it keeps.
    let mut read_before = 0usize;
    for child in tree.elements() {
        r.saw_element();
        let mark = names.enter(child);
        if flat.is_some_and(|taken| taken.skips(names, child)) {
            names.leave(mark);
            continue;
        }
        let key = format!("styles.xml/{}", child.name);
        match names.element(child) {
            (Ns::Office, "font-face-decls") => {
                set.read_fonts(r, names, child, auto)?;
                hold_attrs(r, hold, &key, child)?;
                read_before += 1;
            }
            (Ns::Office, "styles") => {
                set.read_common(r, names, child)?;
                hold_attrs(r, hold, &key, child)?;
                read_before += 1;
            }
            (Ns::Office, "automatic-styles") => {
                set.read_automatic(r, names, child, auto)?;
                hold_attrs(r, hold, &key, child)?;
                read_before += 1;
            }
            (Ns::Office, "master-styles") => {
                masters.holder = Some(child);
                hold_attrs(r, hold, &key, child)?;
                let path = [Link::of(tree), Link::of(child)];
                let mut read_in = 0usize;
                for item in child.elements() {
                    r.saw_element();
                    let m = names.enter(item);
                    match names.element(item) {
                        (Ns::Draw, "layer-set") if set.layers.is_none() => {
                            set.layers = Some(layers::read_layer_set(r, names, item)?);
                            // The document's layer set, each layer whole.
                            set.doc.layer_set(r, names, item)?;
                            if hold {
                                hold_layer_set(r, names, &path, item, &format!("{key}/{}", item.name))?;
                            } else {
                                count_layer_set(r, names, item);
                            }
                            read_in += 1;
                        }
                        (Ns::Style, "master-page") => {
                            let master = master_page(r, names, item)?;
                            // A master page that holds no shape is kept whole, its attributes and children, whether or not a page is on it: a copy writes each, and one no page
                            // names was lost, as were the children of the others (a theme, notes). One that holds shapes is a background page, which keeps them as a page does.
                            if !master.holds_shapes {
                                set.doc.keep_style(r, names, super::odfdoc::Container::Master, item)?;
                            }
                            masters.list.push(master);
                            read_in += 1;
                        }
                        _ => {
                            r.keep("master_styles_child");
                            hold_child(r, hold, &path, read_in, item)?;
                        }
                    }
                    names.leave(m);
                }
                read_before += 1;
            }
            // A flat drawing's other children are counted by the flat reader, which tells them (`flat.rs`).
            _ => {
                r.keep("styles_part_child");
                if hold {
                    unread::child(r, &[Link::of(tree)], read_before, child)?;
                }
            }
        }
        names.leave(mark);
    }
    Ok(masters)
}

/// A document's layer set (the first of `office:master-styles`): its attributes, and its children that are no `draw:layer` (each layer is held whole as a layer, `OdfDoc::layers`).
/// `path` leads to the container of the layer set; `key` is the layer set's key among the roots.
fn hold_layer_set<'t>(r: &mut Reader, names: &mut Names<'t>, path: &[Link<'_>], set: &'t El, key: &str) -> Result<(), OdgError> {
    unread::attrs(r, key, set)?;
    let mut inside: Vec<Link<'_>> = path.to_vec();
    inside.push(Link::of(set));
    let mut layers = 0usize;
    for child in set.elements() {
        let mark = names.enter(child);
        let is_layer = names.is(child, Ns::Draw, "layer");
        names.leave(mark);
        if is_layer {
            layers += 1;
        } else {
            unread::child(r, &inside, layers, child)?;
        }
    }
    Ok(())
}

/// Keeps the attributes of the container `el` under `key` in the roots (`unread::attrs`), or, for a flat drawing, which keeps none of them, counts them for the open's note.
fn hold_attrs(r: &mut Reader, hold: bool, key: &str, el: &El) -> Result<(), OdgError> {
    if hold {
        unread::attrs(r, key, el)
    } else {
        if !el.attrs.is_empty() {
            r.not_kept("container_markup", 1);
        }
        Ok(())
    }
}

/// Keeps `el`, a child no field reads, in the document's data (`unread::child`), or, for a flat drawing, which keeps none of it, counts it for the open's note.
fn hold_child(r: &mut Reader, hold: bool, path: &[Link<'_>], place: usize, el: &El) -> Result<(), OdgError> {
    if hold {
        unread::child(r, path, place, el)
    } else {
        r.not_kept("container_markup", 1);
        Ok(())
    }
}

/// What a flat drawing's layer set holds that no field reads, which [`hold_layer_set`] keeps for a package: its attributes and its children that are no `draw:layer`, counted for the
/// open's note.
fn count_layer_set<'t>(r: &mut Reader, names: &mut Names<'t>, set: &'t El) {
    if !set.attrs.is_empty() {
        r.not_kept("container_markup", 1);
    }
    for child in set.elements() {
        let mark = names.enter(child);
        let is_layer = names.is(child, Ns::Draw, "layer");
        names.leave(mark);
        if !is_layer {
            r.not_kept("container_markup", 1);
        }
    }
}

/// A `style:master-page`: its names, its layout, its own layer set, and whether it holds drawing shapes.
fn master_page<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<Master<'t>, OdgError> {
    let attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let mut layers = None;
    let mut holds_shapes = false;
    for child in el.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "layer-set") {
            layers = Some(layers::read_layer_set(r, names, child)?);
        } else if is_shape(names, child) {
            holds_shapes = true;
        }
        names.leave(mark);
    }
    // Its names are held twice (here and in what it gives the pages), and it is one item more; its own data is held once more for the pages on it (the names, and
    // every attribute: two items each and the bytes of its name and value).
    let held = attrs.peek(Ns::Style, "name").map_or(0, str::len).saturating_mul(3)
        + attrs.peek(Ns::Style, "display-name").map_or(0, str::len)
        + attrs.peek(Ns::Style, "page-layout-name").map_or(0, str::len).saturating_mul(3)
        + attrs.iter().map(|a| a.name.len() + a.value.len()).fold(0usize, usize::saturating_add);
    r.take_items(1usize.saturating_add(attrs.len().saturating_mul(2)))?;
    r.take_bytes(held)?;
    let mut own = RawAttrs::new();
    for a in attrs.iter() {
        own.push(a.name, a.value);
    }
    let odf = MasterOdf {
        name: attrs.peek(Ns::Style, "name").map(Arc::from),
        layout: attrs.peek(Ns::Style, "page-layout-name").map(Arc::from),
        attrs: own,
    };
    Ok(Master {
        name: attrs.peek(Ns::Style, "name").unwrap_or_default().to_owned(),
        display: attrs.peek(Ns::Style, "display-name").map(str::to_owned),
        layout: attrs.peek(Ns::Style, "page-layout-name").map(str::to_owned),
        odf,
        el,
        layers,
        holds_shapes,
    })
}

/// Whether `el` is a drawing shape element.
fn is_shape<'t>(names: &Names<'t>, el: &'t El) -> bool {
    matches!(
        names.element(el),
        (
            Ns::Draw,
            "rect"
                | "line"
                | "polyline"
                | "polygon"
                | "regular-polygon"
                | "path"
                | "circle"
                | "ellipse"
                | "g"
                | "frame"
                | "custom-shape"
                | "connector"
                | "measure"
                | "caption"
                | "control"
                | "a"
        ) | (Ns::Dr3d, "scene")
    )
}

/// The page sheet a layout gives, and the page's height in inches. A page with no layout, or
/// one that gives no size, takes DiaCraft's new page's size, with a warning.
fn page_sheet(r: &mut Reader, layout: Option<&PageLayout>, scale: Option<&(f64, f64)>) -> Result<(Sheet, f64), OdgError> {
    let mut sheet = Sheet::default();
    let put = |r: &mut Reader, sheet: &mut Sheet, name: &str, cell: Cell| -> Result<(), OdgError> {
        r.take_cells(1)?;
        sheet.cells.insert(CellName::new(name), cell);
        Ok(())
    };
    let (width, height) = match layout.and_then(|l| l.width.zip(l.height)) {
        Some(size) => size,
        None => {
            r.warn("page_size_missing", || "a page's master page names no page layout with a size; DiaCraft's new page's size is used".to_owned());
            let blank = Document::new_blank();
            let number = |name: &str| blank.pages.get(0).and_then(|p| p.sheet.cell(name)).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
            (Length { inches: number("PageWidth"), unit: LenUnit::In }, Length { inches: number("PageHeight"), unit: LenUnit::In })
        }
    };
    put(r, &mut sheet, "PageWidth", cells::length(width.inches, width.unit.word()))?;
    put(r, &mut sheet, "PageHeight", cells::length(height.inches, height.unit.word()))?;
    if let Some(layout) = layout {
        for (margin, name) in layout.margins.iter().zip(["PageLeftMargin", "PageRightMargin", "PageTopMargin", "PageBottomMargin"]) {
            if let Some(m) = margin {
                put(r, &mut sheet, name, cells::length(m.inches, m.unit.word()))?;
            }
        }
        match layout.orientation.as_deref() {
            Some("portrait") => put(r, &mut sheet, "PrintPageOrientation", cells::plain(1.0))?,
            Some("landscape") => put(r, &mut sheet, "PrintPageOrientation", cells::plain(2.0))?,
            _ => {}
        }
    }
    let (numerator, denominator) = scale.copied().unwrap_or((1.0, 1.0));
    let unit = width.unit;
    put(r, &mut sheet, "PageScale", cells::length(numerator * unit.inches(), unit.word()))?;
    put(r, &mut sheet, "DrawingScale", cells::length(denominator * unit.inches(), unit.word()))?;
    Ok((sheet, height.inches))
}

/// Step 3's second pass: the master pages' shapes, read as background pages. Gives what every master page gives the pages on it.
pub(crate) fn read_master_pages<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    masters: &Masters<'t>,
    cx: &mut Cx<'_>,
    scale: Option<&(f64, f64)>,
) -> Result<Vec<MasterOut>, OdgError> {
    let mut out = Vec::with_capacity(masters.list.len());
    let holder_mark = masters.holder.map(|h| names.enter(h));
    // The root of the part and the element that holds the master pages are the declarations the model holds: a master page's own, and its shapes', are local.
    let held = names.hold();
    for (ordinal, master) in masters.list.iter().enumerate() {
        let background = if master.holds_shapes {
            let layout = master.layout.as_deref().and_then(|l| cx.set.page_layouts.get(l));
            let (mut sheet, height) = page_sheet(r, layout, scale)?;
            let own_layers = master.layers.as_deref().or(cx.set.layers.as_deref());
            let layers = layers::layer_rows(r, &mut sheet, own_layers)?;
            let mut pcx = PageCx {
                height,
                layers,
                budget: Budget::new(r.limits.max_path_commands_per_page),
                groups: crate::ext::docfile::PageGroups::new(),
                ambiguous: std::collections::HashSet::new(),
                annotations: super::comments::PageAnnotations::default(),
            };
            let mark = names.enter(master.el);
            let mut read = shapes::read_page(r, names, cx, master.el, &mut pcx, PageSource::MasterPage(ordinal))?;
            read.index.set_own_layers(master.layers.is_some());
            names.leave(mark);
            let name = master.display.clone().unwrap_or_else(|| master.name.clone());
            // The page itself, its sheet, its names and its places, before it is made.
            r.take_items(super::cost::PAGE)?;
            let mut page = Page::new(0, &name);
            page.background = true;
            page.sheet = Arc::new(sheet);
            page.shapes = SharedVec::from_vec(read.shapes);
            let odf = PageOdf { master: Some(&master.odf), layout, drawing_page_style: None, written_master: None };
            keep_page_markup(r, &mut page, read.passthrough, diacraft_doc::RawAttrs::new(), odf)?;
            page.connects = diacraft_doc::Connects::from_vec(read.connects);
            Some((page, read.index))
        } else {
            None
        };
        out.push(MasterOut { name: master.name.clone(), layout: master.layout.clone(), odf: master.odf.clone(), background });
    }
    if held {
        names.release();
    }
    if let Some(mark) = holder_mark {
        names.leave(mark);
    }
    Ok(out)
}

/// Step 4's first half: the font faces and automatic styles of `content.xml`, before its pages are read.
pub(crate) fn read_content_styles<'t>(r: &mut Reader, names: &mut Names<'t>, tree: &'t El, set: &mut StyleSet) -> Result<(), OdgError> {
    for child in tree.elements() {
        let mark = names.enter(child);
        match names.element(child) {
            (Ns::Office, "font-face-decls") => {
                r.saw_element();
                set.read_fonts(r, names, child, File::Content)?;
                unread::attrs(r, &format!("content.xml/{}", child.name), child)?;
            }
            (Ns::Office, "automatic-styles") => {
                r.saw_element();
                set.read_automatic(r, names, child, File::Content)?;
                unread::attrs(r, &format!("content.xml/{}", child.name), child)?;
            }
            _ => {}
        }
        names.leave(mark);
    }
    Ok(())
}

/// The master pages, and each one's place by name.
struct MasterIndex<'m> {
    list: &'m [MasterOut],
    by_name: HashMap<&'m str, usize>,
    /// Each master page's page layout, looked up by its name once, here, and not for each page on the master page.
    layouts: Vec<Option<&'m PageLayout>>,
}

/// A page read from `content.xml`, before the document gives it its `ID`.
pub(crate) struct PageOut {
    pub page: Page,
    pub master: Option<String>,
    pub index: PageShapes,
    /// The annotations of the page the model took as comments, each with its place among the page's annotation children.
    pub annotations: Vec<super::comments::Annotation>,
}

/// Step 4's second half: the drawing's pages, each with its shapes.
pub(crate) fn read_content_pages<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    tree: &'t El,
    cx: &mut Cx<'_>,
    masters: &[MasterOut],
    scale: Option<&(f64, f64)>,
    flat: Option<&Taken<'t>>,
) -> Result<Vec<PageOut>, OdgError> {
    let mut pages = Vec::new();
    let mut drawing = false;
    let most = r.limits.max_pages.min(diacraft_doc::MAX_PAGES);
    // Each master page by name, the first of a name where two share one: a page's master page is one lookup.
    let mut by_name: HashMap<&str, usize> = HashMap::with_capacity(masters.len());
    for (at, master) in masters.iter().enumerate() {
        by_name.entry(master.name.as_str()).or_insert(at);
    }
    let set = cx.set;
    let layouts = masters
        .iter()
        .map(|master| {
            master.layout.as_deref().and_then(|name| {
                r.name_bytes(name.len());
                set.page_layouts.get(name)
            })
        })
        .collect();
    let masters = MasterIndex { list: masters, by_name, layouts };
    // What no field reads is kept for the `.diacraft` copy of a packaged drawing; a flat drawing is never saved as one.
    let hold = flat.is_none();
    let mut read_before = 0usize;
    for child in tree.elements() {
        let mark = names.enter(child);
        // A flat drawing reads the first `office:body` only.
        if names.is(child, Ns::Office, "body") && flat.is_none_or(|taken| taken.is_first(Child::Body, child)) {
            r.saw_element();
            hold_attrs(r, hold, &format!("content.xml/{}", child.name), child)?;
            let path = [Link::of(tree), Link::of(child)];
            let mut read_in = 0usize;
            for body in child.elements() {
                r.saw_element();
                let b = names.enter(body);
                match names.element(body) {
                    (Ns::Office, "drawing") => {
                        drawing = true;
                        hold_attrs(r, hold, &format!("content.xml/{}/{}", child.name, body.name), body)?;
                        // The declarations down to this element are the model's own (the roots); a page's own, and its shapes', are local.
                        let held = names.hold();
                        let inside = [path[0], path[1], Link::of(body)];
                        let mut read_pages = 0usize;
                        for page in body.elements() {
                            r.saw_element();
                            let p = names.enter(page);
                            if names.is(page, Ns::Draw, "page") {
                                if pages.len() >= most {
                                    return Err(OdgError::Limit { kind: LimitKind::Pages, max: most as u64 });
                                }
                                let ordinal = pages.len();
                                pages.push(read_page(r, names, cx, page, &masters, scale, ordinal)?);
                                read_pages += 1;
                            } else {
                                r.keep("drawing_child");
                                hold_child(r, hold, &inside, read_pages, page)?;
                            }
                            names.leave(p);
                        }
                        if held {
                            names.release();
                        }
                        read_in += 1;
                    }
                    (Ns::Office, kind @ ("presentation" | "text" | "spreadsheet" | "chart" | "image")) => {
                        return Err(OdgError::NotADrawing { media_type: format!("office:{kind}") });
                    }
                    _ => {
                        r.keep("body_child");
                        hold_child(r, hold, &path, read_in, body)?;
                    }
                }
                names.leave(b);
            }
            read_before += 1;
        } else if names.is(child, Ns::Office, "font-face-decls")
            || names.is(child, Ns::Office, "automatic-styles")
            || names.is(child, Ns::Office, "body")
        {
            // Read apart (`read_content_styles`), or a second body of a flat drawing (`flat.rs` says so).
            read_before += 1;
        } else if hold {
            // `office:scripts`, or an element of the part this reader does not know. A flat drawing's other children are its other parts', or counted by the flat reader (`flat.rs`).
            unread::child(r, &[Link::of(tree)], read_before, child)?;
        }
        names.leave(mark);
    }
    if !drawing {
        return Err(OdgError::NoDrawingContent { missing: if flat.is_some() { Missing::FlatBody } else { Missing::DrawingBody } });
    }
    Ok(pages)
}

/// One `draw:page`.
#[allow(clippy::too_many_arguments)]
fn read_page<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    el: &'t El,
    masters: &MasterIndex<'_>,
    scale: Option<&(f64, f64)>,
    ordinal: usize,
) -> Result<PageOut, OdgError> {
    let attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let master_name = attrs.peek(Ns::Draw, "master-page-name").map(str::to_owned);
    let master_at = master_name.as_deref().and_then(|m| {
        r.report.work.name_lookups = r.report.work.name_lookups.saturating_add(1);
        masters.by_name.get(m).copied()
    });
    let master = master_at.and_then(|at| masters.list.get(at));
    if master_name.is_some() && master.is_none() {
        r.warn("master_page_not_found", || "a page names a master page the drawing does not have".to_owned());
    }
    let layout = master_at.and_then(|at| masters.layouts.get(at).copied().flatten());
    let (mut sheet, height) = page_sheet(r, layout, scale)?;
    // A page's own layer set wins over the document's.
    let mut own_layers = None;
    for child in el.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "layer-set") && own_layers.is_none() {
            own_layers = Some(layers::read_layer_set(r, names, child)?);
        }
        names.leave(mark);
    }
    let layers = layers::layer_rows(r, &mut sheet, own_layers.as_deref().or(cx.set.layers.as_deref()))?;
    if own_layers.is_none() && r.document_layers.is_none() {
        // The rows of the document's layer set, as a page with none of its own has them: what a page the writer adds reads.
        r.document_layers = Some(sheet.section(&diacraft_doc::SectionKey::new("Layer", None)).cloned());
    }
    // The page's own `svg:title` and `svg:desc` are the rows `visAltText` and `visAltDescription` of its sheet. **Only read, never taken out of the markup**: the
    // page keeps its elements as it always did, and DiaCraft's own data says the rest (no `.odg` of the corpus has one on a page, so a page is written no element of its own).
    alt::find(names, el).put(r, &mut sheet)?;
    let name = attrs.peek(Ns::Draw, "name").map_or_else(|| format!("Page-{}", ordinal + 1), str::to_owned);
    // What DiaCraft's document file says of the groups of this page, found by the page's name and its number among the pages of that name.
    let groups = r.groups.take_page(&name);
    let mut pcx = PageCx {
        height,
        layers,
        budget: Budget::new(r.limits.max_path_commands_per_page),
        groups,
        ambiguous: std::collections::HashSet::new(),
        annotations: super::comments::PageAnnotations::default(),
    };
    let mut read = shapes::read_page(r, names, cx, el, &mut pcx, PageSource::Page(ordinal))?;
    read.index.set_own_layers(own_layers.is_some());
    // The entries no group of the page took (the group was renamed or taken away by another program): said once, below.
    let with_data = pcx.groups.values().filter(|e| !e.is_item_only()).count();
    r.groups.leave(with_data);
    // The items of groups that are gone are dropped silently, and counted.
    let gone = pcx.groups.values().filter(|e| e.item.is_some()).count();
    r.report.carry.gone = r.report.carry.gone.saturating_add(gone as u64);
    // The page itself, its sheet, its names and its places, before it is made.
    r.take_items(super::cost::PAGE)?;
    let mut page = Page::new(0, &name);
    page.sheet = Arc::new(sheet);
    page.shapes = SharedVec::from_vec(read.shapes);
    page.connects = diacraft_doc::Connects::from_vec(read.connects);
    // Charged before they are kept: a pair of strings for each attribute but the name, and every attribute's bytes (the name is the page's).
    let kept = attrs.iter().filter(|a| !(a.ns == Ns::Draw && a.local == "name")).count();
    r.take_items(kept.saturating_mul(2) + 1)?;
    r.take_bytes(attrs.iter().map(|a| a.name.len() + a.value.len()).sum())?;
    let mut raw = diacraft_doc::RawAttrs::new();
    for a in attrs.iter() {
        if !(a.ns == Ns::Draw && a.local == "name") {
            raw.push(a.name, a.value);
        }
    }
    let drawing_page_style = attrs.peek(Ns::Draw, "style-name").and_then(|name| cx.set.doc.drawing_page(name)).cloned();
    let odf = PageOdf { master: master.map(|m| &m.odf), layout, drawing_page_style, written_master: master_name.as_deref() };
    keep_page_markup(r, &mut page, read.passthrough, raw, odf)?;
    let annotations = read.annotations.taken.into_iter().map(|(_, a)| a).collect();
    Ok(PageOut { page, master: master_name, index: read.index, annotations })
}

/// What a page holds of its master page, its layout and its drawing-page style.
struct PageOdf<'a> {
    /// The master page the page is on, or the master page a background page is made of.
    master: Option<&'a MasterOdf>,
    /// The layout the master page names, if the drawing has it.
    layout: Option<&'a PageLayout>,
    /// The automatic drawing-page style the page names (`draw:style-name`), shared with the document's list of kept styles.
    drawing_page_style: Option<Arc<RawElement>>,
    /// The `draw:master-page-name` a page wrote, as written (a name the drawing may not have).
    written_master: Option<&'a str>,
}

/// The cells of a page that a layout gives (size, margins and orientation): a page's stamp covers them, as the reader set them.
const PAGE_CELLS: [&str; 7] =
    ["PageWidth", "PageHeight", "PageLeftMargin", "PageRightMargin", "PageTopMargin", "PageBottomMargin", "PrintPageOrientation"];

/// Puts the markup of a page's element that no field reads in the page's `odf` slot, with what the page holds of its master page and layout: the attributes of the `draw:page` and
/// the children kept whole, the master page's and the layout's names and their properties that no cell reads, the drawing-page style, the units of the layout's
/// lengths and the stamp of the page's cells. It is the `.odg`'s, so it is never put in the VSDX `raw` and `passthrough`. The slot is charged before it is made.
fn keep_page_markup(
    r: &mut Reader,
    page: &mut Page,
    passthrough: diacraft_doc::Passthrough,
    raw: diacraft_doc::RawAttrs,
    odf: PageOdf<'_>,
) -> Result<(), OdgError> {
    let master_page: Option<Arc<str>> = odf.written_master.map(Arc::from).or_else(|| odf.master.and_then(|m| m.name.clone()));
    let has_odf = odf.master.is_some() || odf.layout.is_some() || odf.drawing_page_style.is_some() || master_page.is_some();
    if raw.is_empty() && passthrough.is_empty() && !has_odf {
        return Ok(());
    }
    r.take_items(super::cost::EXT_PAGE)?;
    // The name the page wrote is a string of its own.
    r.take_bytes(odf.written_master.map_or(0, str::len))?;
    let stamp = has_odf.then(|| diacraft_doc::stamp_cells(&page.sheet, &PAGE_CELLS));
    let data = page.ext.make_mut();
    data.odf = OdfPage {
        kept: diacraft_doc::Kept { raw, kept: passthrough },
        master_page,
        layout: odf.master.and_then(|m| m.layout.clone()),
        master_page_attrs: odf.master.map(|m| m.attrs.clone()).unwrap_or_default(),
        layout_props: odf.layout.map(|l| l.unmapped.clone()).unwrap_or_default(),
        drawing_page_style: odf.drawing_page_style,
        units: odf.layout.map(|l| l.units.clone()).unwrap_or_default(),
        stamp,
    };
    Ok(())
}

/// Step 5: the pages in document order with `ID`s from 0, then the background pages, `BackPage` set, and the index in the same
/// order.
pub(crate) fn assemble(
    r: &mut Reader,
    doc: &mut Document,
    index: &mut ShapeIndex,
    pages: Vec<PageOut>,
    masters: Vec<MasterOut>,
) -> Result<(), OdgError> {
    let most = r.limits.max_pages.min(diacraft_doc::MAX_PAGES);
    let backgrounds = masters.iter().filter(|m| m.background.is_some()).count();
    if pages.len().saturating_add(backgrounds) > most {
        return Err(OdgError::Limit { kind: LimitKind::Pages, max: most as u64 });
    }
    // Each background page's `ID`, in the order of its master page; a page's back page is the first master page of the name it gives.
    let mut next = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    let mut ids: Vec<Option<u32>> = Vec::with_capacity(masters.len());
    for m in &masters {
        ids.push(m.background.is_some().then(|| {
            let id = next;
            next = next.saturating_add(1);
            id
        }));
    }
    let mut first_of: HashMap<&str, usize> = HashMap::with_capacity(masters.len());
    for (at, m) in masters.iter().enumerate() {
        first_of.entry(m.name.as_str()).or_insert(at);
    }
    let back_page_of: HashMap<String, u32> =
        first_of.iter().filter_map(|(name, &at)| ids.get(at).copied().flatten().map(|id| ((*name).to_owned(), id))).collect();
    for (i, out) in pages.into_iter().enumerate() {
        let mut page = out.page;
        page.id = u32::try_from(i).unwrap_or(u32::MAX);
        page.back_page = out.master.as_deref().and_then(|m| back_page_of.get(m)).copied();
        let id = page.id;
        if !out.annotations.is_empty() {
            r.gather.pages.push((i, out.annotations));
        }
        doc.pages.push(Arc::new(page));
        index.push(out.index.with_page_id(id));
    }
    for (m, id) in masters.into_iter().zip(ids) {
        if let (Some((mut page, shapes_index)), Some(id)) = (m.background, id) {
            page.id = id;
            doc.pages.push(Arc::new(page));
            index.push(shapes_index.with_page_id(id));
        }
    }
    r.report.stats.pages = doc.pages.len();
    r.report.stats.background_pages = backgrounds;
    Ok(())
}
