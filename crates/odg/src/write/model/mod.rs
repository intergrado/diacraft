//! The model writer: an `.odg` written from the model alone (the native-format design's §3.4, §4.1, §4.4 and §4.5).
//!
//! [`super::write_with`] with **no original** is this: a drawing that has no file (one made new, or read from a `.diacraft`, or from a file in the other format) is written without
//! one. It is **the splice writer run on a package made for the occasion**, in three steps:
//!
//! 1. **The plan** ([`plan::of`]): what the model asks of the package, and what it cannot say. The first page that is not a background gives the size the package is made for, the kind
//!    (a drawing or a template) is the one asked for or the model's own, and the parts the model holds are put in order. **What an `.odg` cannot say and carries in DiaCraft's own folder
//!    (not written yet) is refused in words and never left out**: a part of a VSDX file other than its thumbnail, a part of a newer DiaCraft, a document property ODF has no field for,
//!    and the document markup of a VSDX file.
//! 2. **The package** ([`package::build`]): [`crate::base::parts`] gives `content.xml` and `styles.xml` of one page and the layers; `meta.xml` ([`meta`]), `settings.xml` and the manifest
//!    ([`manifest`]) are written from what the model holds in their place; the model's own package entries (`Basic/`, `Scripts/`, the folders of embedded objects and their replacement
//!    pictures, `Configurations2/`, embedded fonts, signatures, `manifest.rdf`, the thumbnail, any other) are put in as they are, each with its row. This is the package **at document and package
//!    level**: what a splice would call the file as it was read.
//! 3. **The splice**: the package is read, as a file the drawing was opened from is, and [`super::write_counted`] is given it as the original. Every page of the model but the one the package was
//!    made for is a new page, every shape a new shape, and what the model holds of the document that the package lacks is DiaCraft's document file's, as for any save. **A shape the splice
//!    writer cannot write as a new one is refused in its words** ([`SaveNote::NewShapeUnwritten`](crate::SaveNote::NewShapeUnwritten) says what a new shape held that was not written):
//!    closed those, shapes and text (a shape's text is written from the model by `write/text/model.rs`).
//!
//! **Content stamp** ([`WriteOptions::content_unchanged`](crate::WriteOptions::content_unchanged), design 4.4): with it, `meta.xml`'s generator and the thumbnail are written as the model holds
//! them (the splice writes neither); without it, the generator is DiaCraft's and the thumbnail is the one passed, as an edited save writes them. A package with a signature gets the stale-signature
//! note every time: the parts a signature covers always differ after a trip (4.4).
//!
//! **Determinism** (§1.6): the same model and options give the same bytes. Nothing here reads a clock or iterates a hash; the entries are in one order, and [`diacraft_container`]'s writer
//! writes a fixed time.
//!
//! **Cost and memory.** A part's bytes are the model's (`Arc`), and the package made holds them once more, the read of it a third time, and the package the splice makes a fourth: the plan refuses a
//! model whose parts would pass [`Limits::max_write_memory`] that way **before anything is made** (from the lengths the parts state), in the splice's own words
//! ([`WriteError::Limit`], "bytes of memory a save holds"), and every buffer the package takes is made by `try_reserve` ([`diacraft_container::zip::ArchiveWriter`]).

mod containers;
mod document;
mod forms;
mod layer_set;
mod manifest;
mod meta;
mod objects;
mod package;
mod plan;
mod settings;
mod sheets;

#[cfg(kani)]
pub(crate) use document::Left;
#[cfg(kani)]
pub(crate) use layer_set::chosen_flags;
pub(crate) use sheets::style_group_of_cells;

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, RawAttrs, RawElement, RawNode};

use super::{Form, OriginalPackage, SaveNote, WriteError, WriteOptions, WriteWork, Written, write_counted};
use crate::{Limits, OdgError, read};

/// The words of a refusal of a flat document: a drawing with no file is a package.
pub(crate) const WORDS_FLAT: &str = "a drawing that has no file yet written as a flat document (it is written as a package, .odg or .otg)";

/// The words of a refusal of data that XML cannot hold.
pub(crate) const WORDS_NAMES: &str = "a name or a value that XML cannot hold (a control character, a name that is not an XML name, an attribute written twice) in the data the document keeps for a part of the package";

/// The words of a refusal of a small part that would pass what the reader reads of one.
pub(crate) const WORDS_SMALL_PART: &str =
    "a meta.xml or settings.xml larger than the reader reads as fields (the cap of a small part): it would not read back as it is written";

/// The words of a refusal of the document's container data larger than a part is read with.
pub(crate) const WORDS_KEPT: &str = "markup kept from the document's containers (office:scripts and the like) larger than the reader reads of a part";

/// The words of a refusal of a root that binds a prefix this writer's own names need to another namespace.
pub(crate) const WORDS_PREFIX: &str =
    "a part's root that binds a prefix the writer's own names need (office, meta, dc, manifest or config) to another namespace";

/// The prefixes a root binds, each with its namespace, in the order the root declares them.
#[derive(Debug)]
pub(crate) struct Declared(Vec<(String, String)>);

impl Declared {
    /// The prefixes bound to `uri`.
    pub(crate) fn bound(&self, uri: &str) -> Vec<&str> {
        self.0.iter().filter(|(_, u)| u == uri).map(|(p, _)| p.as_str()).collect()
    }
}

/// The attributes of the root of the part `part`: the ones the model holds (`OdfDoc::roots`, as the file had them), or, for a model with none, `office:version`.
pub(crate) fn root_of(roots: Option<&[(std::sync::Arc<str>, RawAttrs)]>, part: &str, version: &str) -> Vec<(String, String)> {
    match roots.and_then(|list| list.iter().find(|(name, _)| &**name == part)) {
        Some((_, attrs)) => attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        None => vec![("office:version".to_owned(), version.to_owned())],
    }
}

/// Says `version` as the root's `<prefix>:version`, where the prefix is one the root binds to `uri` (the first): the attribute is replaced where it is and put last where it is not. **Every part
/// this writer makes says the version it writes** (`office:version="1.4"`; the manifest's `manifest:version` and its root row's too), whatever version the file the model was read from said:
/// the package is written whole, and a package whose parts and manifest state two versions is one LibreOffice refuses to load (measured: a root row of 1.3 over parts of 1.4).
pub(crate) fn set_version(attrs: &mut Vec<(String, String)>, declared: &Declared, uri: &str, version: &str) {
    let prefixes = declared.bound(uri);
    let Some(first) = prefixes.first() else { return };
    let names: Vec<String> = prefixes.iter().map(|p| format!("{p}:version")).collect();
    match attrs.iter_mut().find(|(k, _)| names.contains(k)) {
        Some(slot) => slot.1 = version.to_owned(),
        None => attrs.push((format!("{first}:version"), version.to_owned())),
    }
}

/// Makes sure the root `attrs` declares each of `needed` (prefix, namespace): a missing declaration is put in front, and one that binds the prefix to another namespace is refused
/// in words. Gives the prefixes the root binds.
pub(crate) fn ensure_declarations(attrs: &mut Vec<(String, String)>, needed: &[(&str, &str)]) -> Result<Declared, WriteError> {
    // A root that binds a needed prefix to another namespace is refused first, so that a root that is refused is never changed.
    let mut missing: Vec<(String, &str)> = Vec::new();
    for (prefix, uri) in needed {
        let name = format!("xmlns:{prefix}");
        match attrs.iter().find(|(k, _)| *k == name) {
            Some((_, bound)) if bound == uri => {}
            Some(_) => return Err(WriteError::Unsupported(WORDS_PREFIX)),
            None => missing.push((name, uri)),
        }
    }
    for (name, uri) in missing.into_iter().rev() {
        attrs.insert(0, (name, uri.to_owned()));
    }
    let bound = attrs.iter().filter_map(|(k, v)| k.strip_prefix("xmlns:").map(|p| (p.to_owned(), v.clone()))).collect();
    Ok(Declared(bound))
}

/// The deepest tree the writer makes of kept markup: the depth the reader parses a native part at.
pub(crate) const MAX_DEPTH: usize = diacraft_container::limits::XML_DEPTH_CEILING;

/// Whether the nodes come to at most `cap` bytes of markup, and nest no deeper than [`MAX_DEPTH`]: see [`walk`]. The nodes are collected first, so a list of more nodes than the cap is
/// refused before it is.
pub(crate) fn bounded<'a>(nodes: impl Iterator<Item = &'a RawNode>, cap: usize) -> bool {
    let mut stack: Vec<(&RawNode, usize)> = Vec::new();
    for node in nodes {
        // Each node costs at least a byte, so a list of more nodes than the cap is past it.
        if stack.len() >= cap {
            return false;
        }
        stack.push((node, 1));
    }
    walk(stack, cap)
}

/// The bytes one node costs to write, as an estimate that never undercounts (a name, an attribute, a text, with a few bytes for the punctuation of each, and **at least a byte**): the node's
/// children are pushed on `stack`, one level deeper than the node.
fn node_cost<'a>(node: &'a RawNode, depth: usize, stack: &mut Vec<(&'a RawNode, usize)>) -> usize {
    let cost = match node {
        RawNode::Element(el) => {
            stack.extend(el.children.iter().map(|c| (c, depth.saturating_add(1))));
            element_cost(el)
        }
        RawNode::Text(t) => t.len(),
        RawNode::Comment(c) => c.len().saturating_add(7),
        RawNode::Pi { target, data } => target.len().saturating_add(data.len()).saturating_add(5),
    };
    cost.max(1)
}

/// The bytes an element costs to write beside its children: its name twice, its attributes and the punctuation.
fn element_cost(el: &RawElement) -> usize {
    let attrs = el.attrs.iter().fold(0usize, |sum, (k, v)| sum.saturating_add(k.len()).saturating_add(v.len()).saturating_add(4));
    el.name.len().saturating_mul(2).saturating_add(5).saturating_add(attrs)
}

/// Walks `stack` of (node, depth) with a stack of its own, taking each node's cost from `left`: `false` as soon as the sum is past what there is, or a node is deeper than [`MAX_DEPTH`]. A
/// tree of any size or depth is bounded by the cap and never walked whole: each node costs at least a byte, so the nodes walked and the nodes waiting are at most `left`.
fn walk(mut stack: Vec<(&RawNode, usize)>, mut left: usize) -> bool {
    while let Some((node, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return false;
        }
        let cost = node_cost(node, depth, &mut stack);
        left = match left.checked_sub(cost) {
            Some(left) => left,
            None => return false,
        };
        if stack.len() > left {
            return false;
        }
    }
    true
}

/// The bytes the element comes to as the bound counts them (its own and every node under it, each at least a byte), or `None` when that is past `cap` or the element nests deeper
/// than [`MAX_DEPTH`]: the walk of [`bounded_element`], which also says how much it took.
pub(crate) fn element_total(el: &RawElement, cap: usize) -> Option<usize> {
    let mut left = cap.checked_sub(element_cost(el))?;
    if el.children.len() > left {
        return None;
    }
    let mut stack: Vec<(&RawNode, usize)> = el.children.iter().map(|c| (c, 2)).collect();
    while let Some((node, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return None;
        }
        let cost = node_cost(node, depth, &mut stack);
        left = left.checked_sub(cost)?;
        if stack.len() > left {
            return None;
        }
    }
    Some(cap.saturating_sub(left))
}

/// The bytes the node comes to as the bound counts them (itself and every node under it, each at least a byte, the node at depth 1 as [`bounded`] puts it), or `None` when that is past
/// `cap` or it nests deeper than [`MAX_DEPTH`]: what a caller that takes each of several nodes from one budget takes for this one.
pub(crate) fn node_total(node: &RawNode, cap: usize) -> Option<usize> {
    let mut left = cap;
    let mut stack: Vec<(&RawNode, usize)> = vec![(node, 1)];
    while let Some((node, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return None;
        }
        let cost = node_cost(node, depth, &mut stack);
        left = left.checked_sub(cost)?;
        if stack.len() > left {
            return None;
        }
    }
    Some(cap.saturating_sub(left))
}

/// The bytes an element of the name `name` and the attributes `attrs` comes to beside its children, as [`element_total`] counts an element's own (its name twice, each attribute and
/// its quotes, the punctuation), or `None` past `cap`: an element made of pieces the model holds apart (a page's name and its kept attributes), charged before it is made.
pub(crate) fn head_total<'a>(name: &str, attrs: impl Iterator<Item = (&'a str, &'a str)>, cap: usize) -> Option<usize> {
    let mut total = name.len().saturating_mul(2).saturating_add(5);
    for (k, v) in attrs {
        total = total.saturating_add(k.len()).saturating_add(v.len()).saturating_add(4);
        if total > cap {
            return None;
        }
    }
    (total <= cap).then_some(total)
}

/// Whether the element comes to at most `cap` bytes of markup and nests no deeper than [`MAX_DEPTH`].
pub(crate) fn bounded_element(el: &RawElement, cap: usize) -> bool {
    match cap.checked_sub(element_cost(el)) {
        Some(left) if el.children.len() <= left => walk(el.children.iter().map(|c| (c, 2)).collect(), left),
        _ => false,
    }
}

/// What a refusal of the base the model writer made reads as: the reader's own words for a cap, the save's fault for anything else.
fn made_error(e: OdgError) -> WriteError {
    match e {
        OdgError::Limit { kind, max } => WriteError::Limit { what: kind.what(), max },
        OdgError::TooComplex { what, max } => WriteError::Limit { what, max },
        other => WriteError::SelfCheck(format!("the package made to write the drawing onto does not read: {other} ({})", other.class())),
    }
}

/// **What the model writer refuses of a drawing before it writes a byte, and what a save across to a VSDX type of a drawing that holds an `.odg`'s data refuses too** (the crossing
/// refuses what the plain visit refuses, **by this one rule, which both call**, so that the two cannot drift again): **a picture the drawing shows whose bytes are in no format the `.odg` reader
/// draws** (PNG, JPEG, GIF, BMP and EMF, by their first bytes: [`super::picture::picture_type`]), on a page, in a group or not. A save of such a drawing as an `.odg` writes every picture as a new
/// one and refuses it ([`super::picture::WORDS_NOT_DRAWN`], which `picture_text` says of the frame it is writing too). An evidence run found that the crossing finished where the plain visit refused, and
/// lost what it could not say: a table, an embedded object (whose replacement picture LibreOffice writes in its own metafile format) or a picture of another format went through a VSDX file as a
/// plain rectangle with its data in DiaCraft's, so the `.odg` written at the end of the trip had fewer frames, images, tables and objects, and lost the paragraphs of the tables' cells, with no word.
/// → the words of the refusal, `None` when the drawing holds no such picture. The walk is one look at each shape of each page, with a stack of its own for the members of groups (no recursion).
pub fn model_refusal(doc: &Document) -> Option<&'static str> {
    let mut stack: Vec<&diacraft_doc::ShapeList> = doc.pages.iter().map(|page| &page.shapes).collect();
    while let Some(list) = stack.pop() {
        for shape in list.iter() {
            if !shape.children.is_empty() {
                stack.push(&shape.children);
                continue;
            }
            let undrawn = super::shapes::is_new_picture(shape)
                && shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some_and(|media| super::picture::picture_type(&media.bytes).is_none());
            if undrawn {
                return Some(super::picture::WORDS_NOT_DRAWN);
            }
        }
    }
    None
}

/// **The drawing as the file the model writer wrote holds the VSDX data that rides in it**: `doc` without the VSDX parts that the model writer takes out of what rides when it writes a
/// carried object as an object frame (the embedding is the file's `Object N`), and without the relationships that named them. `None` when the drawing holds no carried object. A caller that saves into
/// a file this writer wrote gives the splice writer the drawing this way: it is given the drawing the file was written from, and the embedding the file does not carry is no edit. Bounded by the
/// cap of steps a save takes (a walk of the drawing's shapes, each a step).
///
/// # Errors
/// [`WriteError::Limit`] when the walk is past the cap of steps a save takes.
pub fn without_carried_objects(doc: &Document) -> Result<Option<Document>, WriteError> {
    objects::without_carried(doc, &mut WriteWork::default())
}

/// Writes the drawing `doc` from the model alone: see the [module documentation](self).
pub(super) fn write(doc: &Document, options: &WriteOptions, work: &mut WriteWork) -> Result<Written, WriteError> {
    // The drawing as the caller gave it: what the index this save gives holds as the pages saved (`ShapeIndex::hold_pages_of`), whatever copy of it the splice below is handed.
    let given = doc;
    let limits: &Limits = &options.limits;
    work.step_cap = limits.max_write_steps;
    work.byte_cap = limits.max_write_bytes;
    // The forms of an earlier ODF version the save writes in 1.4's: worked out once, before anything is written.
    crate::ext::odf14::prepare(doc, work)?;
    if let Some(thumbnail) = &options.thumbnail {
        super::meta::check_thumbnail(thumbnail, limits)?;
    }
    if options.form == Some(Form::Flat) {
        return Err(WriteError::Unsupported(WORDS_FLAT));
    }
    // The one rule a save across calls too: what this writer refuses of the drawing it is given, before anything is made.
    if let Some(words) = model_refusal(doc) {
        return Err(WriteError::Unsupported(words));
    }
    // A VSDX OLE object is written as an ODF object frame: the model it is rewritten in is the one this save writes (objects.rs).
    let carried = objects::carry(doc, work)?;
    let (doc, carried_rels) = match &carried {
        Some(c) => (&c.doc, c.rels.clone()),
        None => (doc, std::collections::HashSet::new()),
    };
    // The relationships that named the objects the save wrote as object frames are no held data: the frames say them.
    let stripped;
    let doc = if carried_rels.is_empty() || doc.vsdx.is_none() {
        doc
    } else {
        let mut d = doc.clone();
        if let Some(held) = d.vsdx.as_mut() {
            Arc::make_mut(held).rels.retain(|r| !carried_rels.contains(&(r.source.to_string(), r.id.to_string())));
        }
        stripped = d;
        &stripped
    };
    work.drop_macros = options.drop_macros;
    let plan = plan::of(doc, options, &carried_rels)?;
    let built = package::build(doc, &plan, options, work)?;
    let base = built.bytes;
    // The package is read as the file a drawing was opened from is, for the index the splice maps the model's shapes by.
    let mut index = {
        let (_, report) = read(&base, limits).map_err(made_error)?;
        report.index
    };
    // The page the package is made for is the model's first page: the splice matches pages by `ID`, and the package, which has no document file to say it, reads its page as number 0.
    // (A model that holds pages the `.odg` reader made has the package made with those pages, in the model's order: each is given its `ID`.)
    let slots = document::slot_pages(doc);
    if slots.is_empty() {
        index.set_page_id(0, plan.first_page_id);
    } else {
        for (at, page) in slots.iter().enumerate() {
            index.set_page_id(at, page.id);
        }
    }
    // The model as a read of the package gives it back: the properties are the ones the package says (a VSDX property is an ODF one).
    // What the file carries of the VSDX data is made from the whole model, and the splice is handed the model with that data taken out of the document, its pages and its styles: it compares them with the
    // package it reads, which holds none of it (the shapes keep theirs: a new shape's tier 2 is made from them).
    work.built = Some(Arc::new(crate::ext::items::build(doc, options.drop_macros)?));
    let model = strip_carried(plan.model(doc));
    let stamped = options.content_unchanged;
    let splice = WriteOptions {
        // Held thumbnails are in the package already when the content is as it was read; otherwise the one passed is written, as an edited save does (a first save of a package that has none
        // writes it even when there is nothing to splice).
        thumbnail: if stamped { None } else { options.thumbnail.clone() },
        first_save: true,
        drop_macros: options.drop_macros,
        drop_objects: options.drop_objects,
        variant: None,
        form: None,
        content_unchanged: false,
        limits: limits.clone(),
    };
    work.keep_meta = stamped;
    work.model_path = true;
    let first_work = work.clone();
    let mut spliced = write_counted(&model, Some(&OriginalPackage { bytes: &base, index: &index }), &splice, work);
    // **A text carried in DiaCraft's data is stamped with the formatting its markup says**: what the first pass wrote is read as any file is, and the second pass writes the same file
    // with each such shape's `diacraft:fp-format` made from what the read gave. A read that cannot be made (a file past what a save may hold beside the model, or a drawing the reader refuses) leaves the
    // first pass's file, which has no `fp-format`: the carried text is then judged by its characters alone, as it was.
    if let Ok(first) = &spliced
        && work.carried_texts > 0
        && let Some(formats) = formats_of(&first.bytes, &model, limits)
    {
        *work = first_work;
        work.formats = formats;
        spliced = write_counted(&model, Some(&OriginalPackage { bytes: &base, index: &index }), &splice, work);
    }
    work.formats = std::collections::HashMap::new();
    work.keep_meta = false;
    work.model_path = false;
    let mut written = spliced?;
    drop(base);
    // **The next save is given the caller's drawing, not the copy spliced here**: the copy's pages lack what DiaCraft's document file carries of them, which a read of the file gives
    // back, so the index holds the caller's pages as the pages saved, and the caller's drawing saved again into the file is no edit.
    written.index.hold_pages_of(given);
    // What the model holds of the document that this save does not write: the pieces of the containers a part has no place for, the fill images, and the styles of the shapes' own families
    // (the shapes' own automatic styles are written with the shapes; the layers are written), and, by its kind, a piece whose prefixes nothing binds where it would go.
    if doc.ext.get().is_some() {
        let (images, own_styles) = document::unwritten(doc, &work.text_names_written, &work.graphic_remainders_written);
        let left = built.left;
        let (definitions, styles) = (images.saturating_add(left.definitions), own_styles.saturating_add(left.styles));
        let (layers, containers) = (left.layers, left.containers);
        if definitions > 0 || layers > 0 || styles > 0 || containers > 0 {
            written.notes.push(SaveNote::DocumentDataUnwritten { definitions, layers, styles, containers });
        }
    }
    // What the file carries of the VSDX data, for the note: the classes of it, whether some is in the document file and the folder, and a macro project or objects that ride as bytes.
    if let Some(items) = work.built.take() {
        let shapes = work.carried_shapes > 0;
        let mut classes: Vec<&'static str> = Vec::new();
        if shapes {
            classes.push("shape data");
        }
        classes.extend(items.classes.iter().copied());
        if !classes.is_empty() {
            let document_level = items.classes.iter().any(|c| *c != "shape data");
            written.notes.push(SaveNote::Carried { classes, document_level, macros: items.macros, objects: items.objects });
        }
        if !items.macros_dropped.is_empty() {
            written.notes.push(SaveNote::MacroProjectLeftOut { parts: items.macros_dropped.len() });
        }
    }
    // The parts a signature covers always differ after a trip (design 4.4): the signature is kept, and no longer matches.
    if plan.signed && !written.notes.contains(&SaveNote::SignatureStale) {
        written.notes.push(SaveNote::SignatureStale);
    }
    // The model writer has no file to compare with: the drawing is written whole, and `edited` says only that the bookkeeping is the edited save's.
    written.edited = !stamped;
    written.work = work.clone();
    Ok(written)
}

/// The fingerprint of the formatting of the text of each shape of the package `bytes` that carries its text in DiaCraft's data, as a read of it gives (`fingerprint::format_fp`), by the place of the
/// shape's page in `model` and the shape's `ID`. `None` when the package is too large to read beside what a save holds, or does not read (the save keeps the file it has).
fn formats_of(bytes: &[u8], model: &Document, limits: &Limits) -> Option<std::collections::HashMap<(usize, u32), crate::ext::fingerprint::Fp>> {
    // The read holds the file's model beside the save's: the figure the plan's gate holds a package to.
    if u64::try_from(bytes.len()).ok()?.saturating_mul(5) > limits.max_write_memory {
        return None;
    }
    let (_, report) = read(bytes, limits).ok()?;
    let mut out = std::collections::HashMap::new();
    for at in 0..report.index.pages() {
        let page = report.index.page(at)?;
        let Some(id) = page.page_id() else { continue };
        let Some(place) = model.pages.iter().position(|p| p.id == id) else { continue };
        for (shape, fp) in page.formats() {
            out.insert((place, shape), fp);
        }
    }
    Some(out)
}

/// The model with what the document file and the `DiaCraft/` entries carry of the VSDX data taken out of the document, its pages and its styles: the elements and attributes no ODF
/// markup says, the slots' `vsdx` and `native` sides, the relationship and the part of each page. **The shapes keep theirs**: their own styles carry them (tier 2).
fn strip_carried(mut doc: Document) -> Document {
    // Only the Snap & Glue words of the document's unknown nodes stay: they are `settings.xml`'s.
    let words = doc.snap_words();
    doc.passthrough = diacraft_doc::Passthrough::new();
    doc.set_snap_words(&words);
    fn slot<T: Default + Clone + PartialEq>(ext: &mut diacraft_doc::Ext<T>) {
        if !ext.is_empty() {
            let side = ext.make_mut();
            side.vsdx = diacraft_doc::VsdxRest::default();
            side.native = diacraft_doc::Kept::default();
            ext.tidy();
        }
    }
    fn sheet(sheet: &mut Arc<diacraft_doc::Sheet>) {
        if !sheet.raw.is_empty() || !sheet.passthrough.is_empty() || !sheet.ext.is_empty() {
            let s = Arc::make_mut(sheet);
            s.raw = RawAttrs::new();
            s.passthrough = diacraft_doc::Passthrough::new();
            slot(&mut s.ext);
        }
    }
    slot(&mut doc.ext);
    sheet(&mut doc.sheet);
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        let holds = !page.raw.is_empty()
            || !page.passthrough.is_empty()
            || page.rel_id.is_some()
            || page.part_name.is_some()
            || !page.ext.is_empty()
            || !page.sheet.raw.is_empty()
            || !page.sheet.passthrough.is_empty()
            || !page.sheet.ext.is_empty();
        if !holds {
            continue;
        }
        if let Some(page) = doc.pages.get_mut(at).map(Arc::make_mut) {
            page.raw = RawAttrs::new();
            page.passthrough = diacraft_doc::Passthrough::new();
            page.rel_id = None;
            page.part_name = None;
            slot(&mut page.ext);
            sheet(&mut page.sheet);
        }
    }
    for at in 0..doc.styles.len() {
        let Some(style) = doc.styles.get(at) else { continue };
        if style.raw.is_empty()
            && style.passthrough.is_empty()
            && style.ext.is_empty()
            && style.sheet.raw.is_empty()
            && style.sheet.passthrough.is_empty()
            && style.sheet.ext.is_empty()
        {
            continue;
        }
        if let Some(style) = doc.styles.get_mut(at).map(Arc::make_mut) {
            style.raw = RawAttrs::new();
            style.passthrough = diacraft_doc::Passthrough::new();
            slot(&mut style.ext);
            sheet(&mut style.sheet);
        }
    }
    doc
}

/// The names of the entries the writer makes itself, which no part of the model may take.
pub(crate) fn reserved(name: &str) -> bool {
    matches!(name, "mimetype" | "content.xml" | "styles.xml" | "META-INF/manifest.xml") || name == crate::ext::docfile::PATH
}

/// Names that are in `names` twice, for the refusal of a model whose parts are named alike.
pub(crate) fn repeated<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    let mut seen: HashSet<&str> = HashSet::new();
    names.into_iter().any(|name| !seen.insert(name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use super::*;

    fn el(name: &str, children: Vec<RawNode>) -> RawNode {
        RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children })
    }

    fn nested(depth: usize) -> RawNode {
        let mut node = RawNode::Text(Arc::from("x"));
        for _ in 0..depth {
            node = el("a", vec![node]);
        }
        node
    }

    /// The estimate counts a name twice, each attribute with its quotes, a text, a comment and an instruction, and at least a byte for any node.
    #[test]
    fn a_node_costs_what_writing_it_takes_and_at_least_a_byte() {
        let mut stack = Vec::new();
        let text = RawNode::Text(Arc::from(""));
        assert_eq!(node_cost(&text, 1, &mut stack), 1, "an empty text still costs a byte");
        let (abc, comment, pi) =
            (RawNode::Text(Arc::from("abc")), RawNode::Comment(Arc::from("ab")), RawNode::Pi { target: Arc::from("t"), data: Arc::from("dd") });
        assert_eq!(node_cost(&abc, 1, &mut stack), 3);
        assert_eq!(node_cost(&comment, 1, &mut stack), 9);
        assert_eq!(node_cost(&pi, 1, &mut stack), 8);
        let with_attr = RawNode::Element(RawElement {
            name: Arc::from("ab"),
            attrs: RawAttrs::from_pairs([("k", "vv")]),
            children: vec![RawNode::Text(Arc::from("t"))],
        });
        // The name twice (4), the punctuation (5), the attribute's name and value and its quotes and space (1 + 2 + 4), and the child waiting at the next depth.
        assert_eq!(node_cost(&with_attr, 3, &mut stack), 4 + 5 + 7);
        assert!(stack.iter().any(|(n, d)| matches!(n, RawNode::Text(t) if &**t == "t") && *d == 4));
    }

    /// A tree is within a cap by the sum of its nodes, the largest sum that fits and the one byte past it, and a list of more nodes than the cap is refused before it is walked.
    #[test]
    fn the_bound_is_the_sum_of_the_nodes_to_the_byte() {
        let nodes = [el("a", vec![RawNode::Text(Arc::from("12345"))]), RawNode::Text(Arc::from("xyz"))];
        let total = 2 + 5 + 5 + 3;
        assert!(bounded(nodes.iter(), total));
        assert!(!bounded(nodes.iter(), total - 1));
        let many: Vec<RawNode> = (0..10).map(|_| RawNode::Text(Arc::from(""))).collect();
        assert!(bounded(many.iter(), 10) && !bounded(many.iter(), 9), "ten nodes of a byte each");
        assert!(!bounded(many.iter(), 0));
        // The element form counts the element, then what is in it.
        let RawNode::Element(root) = &nodes[0] else { panic!() };
        assert!(bounded_element(root, 2 + 5 + 5) && !bounded_element(root, 2 + 5 + 4));
        assert!(!bounded_element(root, 3), "a cap under the element's own cost");
    }

    /// A node's total is what the bound counts for it, the same sum [`bounded`] takes it in, and `None` one byte under it; an element's own head is counted as the element's.
    #[test]
    fn a_node_s_total_is_the_sum_the_bound_takes_and_none_past_the_cap() {
        let node = el("a", vec![RawNode::Text(Arc::from("12345"))]);
        let total = 2 + 5 + 5;
        assert_eq!(node_total(&node, 1 << 20), Some(total));
        assert_eq!(node_total(&node, total), Some(total));
        assert_eq!(node_total(&node, total - 1), None);
        assert_eq!(bounded(std::iter::once(&node), total), node_total(&node, total).is_some());
        assert_eq!(node_total(&RawNode::Text(Arc::from("")), 1), Some(1), "an empty text still costs a byte");
        assert_eq!(node_total(&nested(MAX_DEPTH), 1 << 20), None, "one level past the depth");
        let RawNode::Element(e) = &node else { panic!() };
        assert_eq!(head_total(&e.name, e.attrs.iter(), 1 << 20), Some(element_cost(e)));
        assert_eq!(head_total("ab", [("k", "vv")].into_iter(), 1 << 20), Some(4 + 5 + 7));
        assert_eq!(head_total("ab", [("k", "vv")].into_iter(), 15), None);
    }

    /// The depth of what is made of kept markup is the reader's: a tree one level past it is refused, however small, and one at it is not. The walk makes no recursion of its own.
    #[test]
    fn the_depth_is_the_depth_a_native_part_is_read_at() {
        let deepest = nested(MAX_DEPTH - 1);
        assert!(bounded(std::iter::once(&deepest), 1 << 20), "the root and {} below it, then a text", MAX_DEPTH - 1);
        let past = nested(MAX_DEPTH);
        assert!(!bounded(std::iter::once(&past), 1 << 20));
        let RawNode::Element(root) = &past else { panic!() };
        assert!(!bounded_element(root, 1 << 20), "the element form counts the element's own level");
    }

    /// A root that does not bind a prefix gets it, in front; one that binds it to what it must is let be; one that binds it to another namespace is refused in words.
    #[test]
    fn a_root_is_given_the_declarations_its_names_need_and_never_changed_to_have_them() {
        let mut attrs = vec![("office:version".to_owned(), "1.3".to_owned())];
        let declared = ensure_declarations(&mut attrs, &[("office", "urn:o"), ("meta", "urn:m")]).unwrap();
        assert_eq!(attrs[0].0, "xmlns:office");
        assert_eq!(attrs[1].0, "xmlns:meta");
        assert_eq!(declared.bound("urn:o"), ["office"]);
        let before = attrs.clone();
        ensure_declarations(&mut attrs, &[("office", "urn:o")]).unwrap();
        assert_eq!(attrs, before, "a declaration that is there is let be");
        let refused = ensure_declarations(&mut attrs, &[("office", "urn:other")]).unwrap_err();
        assert_eq!(refused, WriteError::Unsupported(WORDS_PREFIX));
        assert_eq!(attrs, before, "nothing is changed for a root that is refused");
        // Not even when a prefix before the one that conflicts was missing (it is put in only when no prefix conflicts).
        let refused = ensure_declarations(&mut attrs, &[("new", "urn:n"), ("office", "urn:other")]).unwrap_err();
        assert_eq!(refused, WriteError::Unsupported(WORDS_PREFIX));
        assert_eq!(attrs, before, "a prefix that was missing is not put in by a root that is refused");
        // The declarations that are missing come in front in the order they were asked, and the root's own come after them.
        let mut bare: Vec<(String, String)> = vec![("a".to_owned(), "1".to_owned())];
        ensure_declarations(&mut bare, &[("p", "urn:p"), ("q", "urn:q")]).unwrap();
        assert_eq!(bare.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["xmlns:p", "xmlns:q", "a"]);
    }

    /// The version a part says is set where it is (by whatever prefix the root binds to the namespace) and put last where it is not.
    #[test]
    fn the_version_is_set_where_it_stands_and_added_where_it_does_not() {
        let mut attrs = vec![("xmlns:o".to_owned(), "urn:o".to_owned()), ("o:version".to_owned(), "1.2".to_owned())];
        let declared = ensure_declarations(&mut attrs, &[]).unwrap();
        set_version(&mut attrs, &declared, "urn:o", "1.4");
        assert_eq!(attrs[1], ("o:version".to_owned(), "1.4".to_owned()));
        let mut bare = vec![("xmlns:p".to_owned(), "urn:o".to_owned())];
        let declared = ensure_declarations(&mut bare, &[]).unwrap();
        set_version(&mut bare, &declared, "urn:o", "1.4");
        assert_eq!(bare.last(), Some(&("p:version".to_owned(), "1.4".to_owned())));
        // A root that binds the namespace to no prefix is let be.
        let mut none = vec![("a".to_owned(), "b".to_owned())];
        let declared = ensure_declarations(&mut none, &[]).unwrap();
        set_version(&mut none, &declared, "urn:o", "1.4");
        assert_eq!(none.len(), 1);
    }

    /// The names the writer makes itself are reserved, and names are told alike exactly (an entry name is compared as it is: `Object 1/x` and `object 1/x` differ).
    #[test]
    fn reserved_and_repeated_names() {
        for name in ["mimetype", "content.xml", "styles.xml", "META-INF/manifest.xml", "DiaCraft/document.xml"] {
            assert!(reserved(name), "{name}");
        }
        for name in ["meta.xml", "settings.xml", "Basic/", "content.XML", "DiaCraft/other.xml"] {
            assert!(!reserved(name), "{name}");
        }
        assert!(repeated(["a", "b", "a"].into_iter()) && !repeated(["a", "b", "A"].into_iter()) && !repeated(std::iter::empty()));
    }
}
