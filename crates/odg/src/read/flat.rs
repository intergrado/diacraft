//! Reading a flat drawing, `.fodg`.
//!
//! A flat drawing is one XML document whose root is `office:document`. It is read **as the package that holds the same elements**:
//! `office:meta` is `meta.xml`'s, `office:settings` is `settings.xml`'s, `office:scripts` and `office:body` are `content.xml`'s, `office:styles` and
//! `office:master-styles` are `styles.xml`'s, and `office:font-face-decls` and `office:automatic-styles` are both parts' at once. So the one tree is
//! parsed once, within the reader's caps for a tree, and the reader's own steps run over its children:
//!
//! 1. the root: `office:document` by namespace, whatever its prefix, with its `office:mimetype` and `office:version`; another kind of document
//!    is `not_a_drawing`, a media type of no kind this crate knows (or none) is read as a drawing with the warning `mimetype_irregular`;
//! 2. the children, each by its name wherever it stands: **the first of each is used** and the rest are kept as bytes, with one warning
//!    `flat_order_irregular` for a child out of the order or repeated; the first child in DiaCraft's namespace is DiaCraft's extension (R35,
//!    [`Document::diacraft`]), a second one is one warning `extension_repeated`, and any other foreign child is kept and counted;
//! 3. the generator and the scale from `office:meta` and `office:settings`, the styles from the four styles children (**one container** of automatic
//!    styles, searched before `office:styles`, with `style_name_repeated` for a name the container holds twice), then the master pages' shapes and the
//!    pages, as for a package.
//!
//! **Pictures**: inline `office:binary-data` is the picture, and an `xlink:href` beside it is ignored; a `draw:image` with only an href is linked, never
//! opened, drawn as a placeholder and counted ([`crate::ReadReport::linked_pictures`], one warning `linked_picture_not_read` for the document). An inline
//! object (`office:document` or `math:math` inside `draw:object`) is part of its frame and is never read as part of the drawing.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::{self, El, Node};
use diacraft_container::{ContainerError, Work};
use diacraft_doc::Document;

use super::names::{Names, Ns};
use super::{Reader, pages, shapes, sheets, styles, texts};
use crate::error::{LimitKind, OdgError};
use crate::flat::{Child, Claim};
use crate::index::ShapeIndex;

/// The first of each of the eight children of `office:document`, and so which ones the reader uses.
pub(crate) struct Taken<'t> {
    first: [Option<&'t El>; 8],
}

impl<'t> Taken<'t> {
    /// Whether `el` is the first `child` of the root.
    pub(crate) fn is_first(&self, child: Child, el: &El) -> bool {
        self.first.get(child.rank()).copied().flatten().is_some_and(|first| std::ptr::eq(first, el))
    }

    /// Whether the styles part's reader passes over `el`: it is not one of the four styles children, or it is a second of its name.
    pub(crate) fn skips(&self, names: &Names<'t>, el: &El) -> bool {
        let (ns, local) = names.element(el);
        match if ns == Ns::Office { Child::of(local) } else { None } {
            Some(c @ (Child::FontFaceDecls | Child::Styles | Child::AutomaticStyles | Child::MasterStyles)) => !self.is_first(c, el),
            _ => true,
        }
    }
}

/// How many items and bytes an element kept whole costs the model: each element, attribute and text one item, and the bytes of every name and value.
fn raw_cost(root: &El) -> (usize, usize) {
    let (mut items, mut bytes) = (0usize, 0usize);
    let mut stack = vec![root];
    while let Some(el) = stack.pop() {
        items = items.saturating_add(1).saturating_add(el.attrs.len());
        bytes = bytes.saturating_add(el.name.len());
        for (k, v) in &el.attrs {
            bytes = bytes.saturating_add(k.len()).saturating_add(v.len());
        }
        for node in &el.children {
            match node {
                Node::El(child) => stack.push(child),
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => {
                    items = items.saturating_add(1);
                    bytes = bytes.saturating_add(t.len());
                }
                Node::Pi { target, data } => {
                    items = items.saturating_add(1);
                    bytes = bytes.saturating_add(target.len()).saturating_add(data.len());
                }
            }
        }
    }
    (items, bytes)
}

impl Reader<'_> {
    /// The whole read of a flat drawing: see the module documentation.
    pub(super) fn read_flat(&mut self, bytes: &[u8]) -> Result<Document, OdgError> {
        self.report.form = crate::Form::Flat;
        let limits = self.limits;
        // One part, so the container's cap on a part; refused before it is parsed.
        let part_cap = limits.container.max_part_bytes;
        if bytes.len() as u64 > part_cap {
            return Err(OdgError::Container(ContainerError::PartTooLarge { name: "office:document".to_owned(), limit: part_cap }));
        }
        if bytes.len() as u64 > limits.max_xml_bytes {
            return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
        }
        let mut xml_limits = limits.container.xml.clone();
        xml_limits.max_nodes = xml_limits.max_nodes.min(limits.max_read_xml_nodes);
        let mut work = Work::default();
        let parsed = xml::parse_counted(bytes, &xml_limits, &mut work);
        self.report.work.package.xml_events = self.report.work.package.xml_events.saturating_add(work.xml_events);
        let tree = parsed.map_err(|error| OdgError::Xml { part: "office:document".to_owned(), error })?;
        let mut names = Names::default();
        let mark = names.enter(&tree);
        if !names.is(&tree, Ns::Office, "document") {
            return Err(OdgError::UnexpectedFlatRoot { root: crate::report::shown(&tree.name) });
        }
        // The one root is the only element whose declarations the model holds (as both parts' roots): the ones the containers below it make are local.
        names.hold();
        // What the root says it is.
        let root_attrs = names.attrs(&tree);
        let claim = Claim::of(root_attrs.peek(Ns::Office, "mimetype"));
        self.report.office_version = root_attrs.peek(Ns::Office, "version").map(crate::report::shown);
        let legacy = crate::write::is_legacy(root_attrs.peek(Ns::Office, "version"));
        match &claim {
            Claim::Other(media_type) => return Err(OdgError::NotADrawing { media_type: media_type.chars().take(64).collect() }),
            Claim::Absent => self.warn("mimetype_irregular", || "the document's office:mimetype is absent (read as a drawing)".to_owned()),
            Claim::Unrecognised => {
                self.warn("mimetype_irregular", || "the document's office:mimetype is unrecognised (read as a drawing)".to_owned());
            }
            Claim::Drawing | Claim::Template => {}
        }
        self.report.variant = claim.variant();
        // The children, each by its name wherever it stands.
        let mut first: [Option<&El>; 8] = [None; 8];
        let (mut irregular, mut last, mut extension, mut repeated) = (false, 0usize, None::<&El>, false);
        for child in tree.elements() {
            let m = names.enter(child);
            let (ns, local) = names.element(child);
            match ns {
                Ns::Office => match Child::of(local) {
                    Some(c) => match first.get_mut(c.rank()) {
                        Some(slot @ None) => {
                            *slot = Some(child);
                            irregular |= c.rank() < last;
                            last = last.max(c.rank());
                        }
                        // A second child of one name is not read, and the drawing does not hold it.
                        _ => {
                            irregular = true;
                            self.not_kept("container_markup", 1);
                        }
                    },
                    None => {
                        self.keep("flat_child");
                        self.not_kept("container_markup", 1);
                    }
                },
                Ns::Diacraft => {
                    if extension.is_none() {
                        extension = Some(child);
                    } else {
                        repeated = true;
                    }
                }
                _ => {
                    self.keep("foreign_child");
                    self.not_kept("container_markup", 1);
                }
            }
            names.leave(m);
        }
        if irregular {
            self.warn("flat_order_irregular", || {
                "the document's children are not in the order of the standard, or one is repeated: the first of each is read".to_owned()
            });
        }
        if repeated {
            self.warn("extension_repeated", || "the document holds DiaCraft's extension more than once: the first is read".to_owned());
        }
        let taken = Taken { first };
        let one = |c: Child| taken.first.get(c.rank()).copied().flatten();
        // A flat drawing's `office:scripts` is read for its macros alone: the drawing does not hold it.
        if one(Child::Scripts).is_some() {
            self.not_kept("container_markup", 1);
        }
        // Macros: the whole tree, to its end.
        self.report.macros = crate::package::holds_macro(&tree);
        // `office:meta`'s generator and `office:settings`' scale.
        if let Some(meta) = one(Child::Meta) {
            let m = names.enter(meta);
            self.report.generator = pages::generator_of(self, &mut names, meta);
            names.leave(m);
        }
        let scale = match one(Child::Settings) {
            Some(settings) => {
                let m = names.enter(settings);
                let scale = pages::scale_in(self, &names, settings);
                names.leave(m);
                scale
            }
            None => None,
        };
        // The styles: one container of automatic styles, read once, then the master pages' shapes and the pages.
        let mut set = styles::StyleSet::default();
        let mut res = styles::Resolver::default();
        let mut shared = shapes::Shared::default();
        let masters = pages::read_styles_part(self, &mut names, &tree, &mut set, Some(&taken))?;
        set.resolve(self, 0);
        let sheets = sheets::build(self, &mut set, &mut res)?;
        let out = {
            let mut cx = shapes::Cx {
                set: &set,
                res: &mut res,
                sheets: &sheets,
                package: None,
                file: styles::File::Content,
                styles: HashMap::new(),
                shared: &mut shared,
                texts: texts::StyleTexts::of(&set),
                legacy,
            };
            pages::read_master_pages(self, &mut names, &masters, &mut cx, scale.as_ref())?
        };
        res.prepare(self, &set);
        let pages = {
            let mut cx = shapes::Cx {
                set: &set,
                res: &mut res,
                sheets: &sheets,
                package: None,
                file: styles::File::Content,
                styles: HashMap::new(),
                shared: &mut shared,
                texts: texts::StyleTexts::of(&set),
                legacy,
            };
            pages::read_content_pages(self, &mut names, &tree, &mut cx, &out, scale.as_ref(), Some(&taken))?
        };
        // The automatic styles of the one container that no shape, paragraph or run named.
        set.hold_unused(self, &mut names, &tree, styles::File::Content, one(Child::AutomaticStyles))?;
        // DiaCraft's extension, kept whole as the model's own data; charged before it is copied.
        let diacraft = match extension {
            Some(el) => {
                let (items, bytes) = raw_cost(el);
                self.take_items(items)?;
                self.take_bytes(bytes)?;
                let mut data = shapes::raw_element(el);
                // The extension of a file that a build made before the id moved to `diacraft.app` wrote is held as the one this build writes (APPID).
                diacraft_doc::use_current_namespace(&mut data);
                // A prefix the extension takes from the root's declaration of the old namespace is declared by the extension itself, in the current one: the model's data stands alone, and a save
                // that writes it writes the current namespace, whatever the root of the file binds.
                diacraft_doc::declare_inherited_namespace(&mut data, tree.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                Some(Arc::new(data))
            }
            None => None,
        };
        names.leave(mark);
        let mut doc = Document::empty();
        let mut index = ShapeIndex::default();
        pages::assemble(self, &mut doc, &mut index, pages, out)?;
        // The pages' annotations are the document's comments: a flat drawing has no document file that gives a page another `ID`.
        super::comments::finish(self, &mut doc)?;
        doc.styles = diacraft_doc::SharedVec::from_vec(sheets.into_styles());
        // A flat drawing has no package entries, so there is none to leave out of the model's parts.
        let _no_entries = super::odfdoc::finish(self, &mut doc, std::mem::take(&mut set.doc), None)?;
        doc.diacraft = diacraft;
        // The document's own data: `office:meta` is `meta.xml`'s and `office:settings` is `settings.xml`'s, held in the model as a package's are, so that a drawing saved as a
        // package or as a `.diacraft` file keeps its title, its properties, its generator and its settings. A flat drawing has no package, so its font faces and these are
        // all it gives of the document's own data. An `office:meta` that has attributes of its own is not read into fields, as a `meta.xml` of that shape is not: it stays in the
        // file's own bytes, and one note says so.
        let mut held = super::keep::Held::default();
        // **The root's attributes are the roots of `content.xml` and `styles.xml`** (the one root of a flat file is both parts' root): its namespace declarations bind the
        // prefixes of the markup the model keeps (a link's `xlink:href`, the geometry's `drawooo:` attributes), so a package made from the model declares them again. Without them the
        // package bound no prefix for such markup and the writer left it out (every in-text link of a flat drawing). `office:mimetype` is the `mimetype` entry's and no part's.
        let mimetype = root_attrs.iter().find(|a| a.ns == Ns::Office && a.local == "mimetype").map(|a| a.name);
        let root: Vec<(String, String)> = tree.attrs.iter().filter(|(k, _)| Some(k.as_str()) != mimetype).cloned().collect();
        held.root(self, "content.xml", &root)?;
        held.root(self, "styles.xml", &root)?;
        if let Some(meta) = one(Child::Meta) {
            held.meta = super::keep::flat_meta(self, &tree, meta)?;
            if held.meta.is_none() {
                self.keep("flat_meta");
                self.not_kept("container_markup", 1);
            }
        }
        if let Some(settings) = one(Child::Settings) {
            held.settings = Some(super::keep::flat_settings(self, &tree, settings)?);
        }
        super::keep::apply(self, &mut doc, held, None, &std::collections::HashSet::new())?;
        self.report.stats.styles = doc.styles.len();
        self.report.index = index;
        // What the markup says of each page's layers: a flat drawing has no document file over the markup, so they are the pages' own sheets as read. A save predicts the
        // layer of a shape it adds from them. (No page is added to a flat drawing, so the rows such a page would read are not made.)
        self.report.native = crate::ext::docfile::apply(self, &mut doc, None, None)?;
        // One warning for the document, that counts the linked pictures it holds.
        let linked = self.report.linked_pictures;
        if linked > 0 {
            self.warn("linked_picture_not_read", || {
                format!("{linked} linked pictures were not read: a flat drawing is read alone, and each draws as a placeholder")
            });
        }
        Ok(doc)
    }
}
