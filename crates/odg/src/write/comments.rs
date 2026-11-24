//! Writing the document's comments as a page's `office:annotation` elements (ODF 1.4 Part 3 §14.1; `read/comments.rs` reads them back).
//!
//! **What a save writes.** A page whose comments are not what the file's annotations read as has **all the annotations the reader took** taken out and its comments written
//! anew, at the end of the page (the schema's `draw-page` puts them after the shapes, the animations and the notes); the other annotation elements of the page (the ones the
//! model cannot hold exactly, which the reader left in the markup) are not touched, and a page whose comments are as the file has them is not touched at all: its annotation
//! elements keep their bytes. The index says which elements the reader took ([`crate::PageShapes::comments`]), so no scan has to decide it again.
//!
//! **One annotation** is `dc:creator`, `dc:date` and `meta:creator-initials` (the schema's order), a `text:p` for each line of the text (`text::encode`: runs of spaces are
//! `text:s`, a tab is `text:tab`), `svg:x` and `svg:y` (and `svg:width` and `svg:height` when the file gave them) and DiaCraft's own attributes: `diacraft:version`, `id`, `shape`,
//! `parent`, `done` and `edited` (`read/comments.rs`). LibreOffice shows an annotation by its place on the page, so **a comment the model made is written at its marker**: the
//! top right corner of the box round the shape it is on, or inside the page's top right corner for a comment on the page. A comment read from a file keeps
//! the place the file gave.
//!
//! **The shape a comment is on is a number**, the shape's `ID`, and an `.odg` keeps no `ID` unless a shape is named by something: a file's shapes are
//! numbered by the reader in document order. So a comment's `diacraft:shape` is **the number the shape will be read with** (the tie): a new shape a comment is on is named
//! ([`PageDiff::tied`](super::compare::PageDiff), `referenced` in `shapes.rs`) and keeps its `ID`; a shape of the file is read with the number the reader gave it when the
//! save leaves the page's numbering alone or when the shape carries its `ID` in DiaCraft's data; **a save that renumbers the page's shapes writes the comment of a shape that
//! has no `ID` of its own as a comment of the page**, and the next save that does not renumber them ties it again (it is a difference then, and the page's comments are written
//! anew), so a tie is never one to another shape.
//!
//! **Cost.** The comments are grouped by page once; each page's comments are looked at once; the bytes and tree items of a page's annotations are worked out and charged
//! before any is made, and the shapes a page's comments are on are found by one walk of the page.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Comment, Comments, Page, Shape};

use super::emit::Pfx;
use super::frame::Editor;
use super::lengths;
use super::own;
use super::scan::ScanPage;
use super::scan::{DIACRAFT_URI, OFFICE_URI};
use super::splice::{WORDS_CARRIAGE_RETURN, WORDS_XML_CHAR, escape_value, has_bad_char};
use super::text::escape_text;
use super::{WriteError, WriteWork};
use crate::index::PageShapes;
use crate::limits::Limits;
use crate::read::Affine;
use crate::read::comments::KEPT_ATTRS;

/// The namespace of the elements of Dublin Core, `dc:creator` and `dc:date`.
const DC_URI: &str = "http://purl.org/dc/elements/1.1/";
/// The namespace of the elements of `meta`, `meta:creator-initials`.
const META_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";

/// How far inside the page's top right corner the marker of a page's comment is, in inches (`engine::draw::comments::MARKER_INSET`).
const MARKER_INSET: f64 = 0.3;

/// A comment as it is written: the comment, the number the shape will be read with, and the names of its author.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub comment: Arc<Comment>,
    pub tie: Option<u32>,
    pub creator: Arc<str>,
    pub initials: Arc<str>,
}

/// The annotations of a page as they are written: the text to put at the end of the page, and the namespace declarations that text needs, for the page's own start tag.
#[derive(Debug, Default)]
pub(crate) struct Block {
    pub declare: String,
    pub text: String,
}

/// What a save does to the comments of one page.
#[derive(Clone, Debug)]
pub(crate) struct CommentEdit {
    /// The page's annotation children the reader took as comments, by their place among its annotation children: taken out.
    pub remove: Vec<u32>,
    /// How many annotation children the page has in the file.
    pub total: u32,
    /// The page's comments as they are written, in the model's order.
    pub list: Vec<Entry>,
}

/// The comments of a document grouped by the page they are on, and the names of its authors.
#[derive(Debug, Default)]
pub(crate) struct ByPage {
    pages: HashMap<u32, Vec<Arc<Comment>>>,
    authors: HashMap<u32, (Arc<str>, Arc<str>)>,
}

impl ByPage {
    /// One pass over the comments and the authors, each a counted step.
    pub(crate) fn of(comments: &Comments, work: &mut WriteWork) -> Result<ByPage, WriteError> {
        work.charge_steps(comments.len().saturating_add(1))?;
        let starved = |n: usize| WriteError::Limit { what: "memory for the comments of the drawing", max: n as u64 };
        let mut out = ByPage::default();
        out.authors.try_reserve(comments.authors().count()).map_err(|_| starved(comments.len()))?;
        for a in comments.authors() {
            out.authors.insert(a.id, (Arc::clone(&a.name), Arc::clone(&a.initials)));
        }
        for c in comments.iter() {
            if let Some(list) = out.pages.get_mut(&c.page) {
                list.try_reserve(1).map_err(|_| starved(comments.len()))?;
                list.push(Arc::clone(c));
            } else {
                out.pages.try_reserve(1).map_err(|_| starved(comments.len()))?;
                out.pages.insert(c.page, vec![Arc::clone(c)]);
            }
        }
        Ok(out)
    }

    fn of_page(&self, page: u32) -> &[Arc<Comment>] {
        self.pages.get(&page).map_or(&[], Vec::as_slice)
    }

    fn names(&self, author: u32) -> (Arc<str>, Arc<str>) {
        self.authors.get(&author).cloned().unwrap_or_else(|| (Arc::from(""), Arc::from("")))
    }
}

/// The text of a comment as an `.odg` holds it: its lines are paragraphs, so a line separator is a line break.
fn normal(text: &str) -> String {
    text.replace('\u{2028}', "\n")
}

/// The attributes of a comment the file format holds, by their canonical names, in the order of the list.
fn odf_attrs(raw: &diacraft_doc::RawAttrs) -> Vec<(&'static str, &str)> {
    KEPT_ATTRS.iter().filter_map(|&(name, _, _)| raw.get(name).map(|v| (name, v))).collect()
}

/// Whether the comment `b` the file has is the comment `w` is written as.
fn same(w: &Entry, b: &Comment, b_names: &(Arc<str>, Arc<str>)) -> bool {
    let c = &*w.comment;
    c.id == b.id
        && c.parent == b.parent
        && c.done == b.done
        && c.date == b.date
        && c.edit_date == b.edit_date
        && w.tie == b.shape
        && w.creator == b_names.0
        && w.initials == b_names.1
        && normal(&c.text) == normal(&b.text)
        && raw_same(&c.raw, &b.raw)
}

/// Whether the attributes the file has of a comment are the model's: each the model has is the file's, and the file has no other but a place (`svg:x` and `svg:y`, which a save
/// writes for a comment that had none).
fn raw_same(model: &diacraft_doc::RawAttrs, file: &diacraft_doc::RawAttrs) -> bool {
    let (m, f) = (odf_attrs(model), odf_attrs(file));
    m.iter().all(|a| f.contains(a)) && f.iter().all(|a| m.contains(a) || matches!(a.0, "svg:x" | "svg:y"))
}

/// What the page's structure does to the numbers its shapes are read with.
pub(crate) struct Ties<'a> {
    /// Where the model's shapes are in the file: the index the save goes by.
    pub held: &'a PageShapes,
    /// The numbers the file's shapes are read with now: the index reading the file again gave.
    pub fresh: &'a PageShapes,
    /// The shapes of the model the file lacks.
    pub new: &'a HashSet<u32>,
    /// The ordinals of the file's shapes the save takes out.
    pub deleted: &'a [u32],
    /// The save adds, takes out or reorders the page's shapes.
    pub restructured: bool,
}

/// The comments of a page as a save writes them, or `None` when the page's comments are what the file has. `base_page` is the page's `ID` in the file as it was read
/// (`None` for a page the file lacks); `held` is what the index says of the page's annotation elements.
pub(crate) fn edit_for(
    doc: &ByPage,
    page: u32,
    base: &ByPage,
    base_page: Option<u32>,
    ties: &Ties<'_>,
    work: &mut WriteWork,
) -> Result<(Option<CommentEdit>, Vec<u32>), WriteError> {
    let now = doc.of_page(page);
    let was = base_page.map_or(&[][..], |p| base.of_page(p));
    if now.is_empty() && was.is_empty() {
        return Ok((None, Vec::new()));
    }
    work.charge_steps(now.len().saturating_add(was.len()).saturating_add(1))?;
    let starved = || WriteError::Limit { what: "memory for the comments of a page", max: now.len() as u64 };
    // The number each shape the page's comments are on will be read with: the file's numbering by element, found once.
    let mut inverse: Option<HashMap<u32, u32>> = None;
    let mut list: Vec<Entry> = Vec::new();
    list.try_reserve_exact(now.len()).map_err(|_| starved())?;
    let mut tied_new: HashSet<u32> = HashSet::new();
    for c in now {
        let tie = match c.shape {
            None => None,
            Some(n) => match ties.held.ordinal(n) {
                Some(o) if ties.deleted.contains(&o) => None,
                Some(o) => {
                    let inverse = inverse.get_or_insert_with(|| ties.fresh.ordinal_map().iter().map(|(id, ord)| (*ord, *id)).collect());
                    let read_as = inverse.get(&o).copied();
                    if ties.held.has_tier2(n) || !ties.restructured { read_as } else { None }
                }
                None if ties.new.contains(&n) => {
                    tied_new.insert(n);
                    Some(n)
                }
                None => None,
            },
        };
        let (creator, initials) = doc.names(c.author);
        list.push(Entry { comment: Arc::clone(c), tie, creator, initials });
    }
    let (places, total) = ties.held.comments();
    let unchanged = base_page.is_some()
        && list.len() == was.len()
        && list.iter().zip(was).all(|(w, b)| same(w, b, &base.names(b.author)))
        && places.len() == was.len();
    let mut tied: Vec<u32> = tied_new.into_iter().collect();
    tied.sort_unstable();
    if unchanged {
        return Ok((None, Vec::new()));
    }
    let edit = CommentEdit { remove: places.to_vec(), total, list };
    Ok((Some(edit), tied))
}

/// The top right corner of the box round each of the shapes `wanted` on the page, on the page, in inches (x right, y up), by one walk of the page's shapes. A shape that is not
/// there, or whose cells are not numbers, has none.
fn marker_places(page: &Page, wanted: &HashSet<u32>, limits: &Limits, work: &mut WriteWork) -> Result<HashMap<u32, (f64, f64)>, WriteError> {
    let mut found: HashMap<u32, (f64, f64)> = HashMap::new();
    if wanted.is_empty() {
        return Ok(found);
    }
    let starved = |n: usize| WriteError::Limit { what: "memory for the walk of a page", max: n as u64 };
    let mut stack: Vec<(&Shape, Affine)> = Vec::new();
    stack.try_reserve(page.shapes.len()).map_err(|_| starved(page.shapes.len()))?;
    stack.extend(page.shapes.iter().map(|s| (&**s, Affine::IDENTITY)));
    while let Some((shape, outer)) = stack.pop() {
        work.charge_compared(limits)?;
        let own = own::local_to_parent(&shape.sheet).map(|m| own::compose(&outer, &m));
        if wanted.contains(&shape.id)
            && let (Some(map), Some(w), Some(h)) = (own, own::cell_number(&shape.sheet, "Width"), own::cell_number(&shape.sheet, "Height"))
        {
            let corners = [map.apply(0.0, 0.0), map.apply(w, 0.0), map.apply(w, h), map.apply(0.0, h)];
            let x = corners.iter().map(|p| p.0).fold(f64::MIN, f64::max);
            let y = corners.iter().map(|p| p.1).fold(f64::MIN, f64::max);
            if x.is_finite() && y.is_finite() {
                found.try_reserve(1).map_err(|_| starved(found.len()))?;
                found.insert(shape.id, (x, y));
            }
            if found.len() == wanted.len() {
                break;
            }
        }
        if let Some(map) = own
            && !shape.children.is_empty()
        {
            stack.try_reserve(shape.children.len()).map_err(|_| starved(stack.len()))?;
            stack.extend(shape.children.iter().map(|c| (&**c, map)));
        }
    }
    Ok(found)
}

/// Whether a string can be written in the file.
fn writable(text: &str) -> Result<(), WriteError> {
    if has_bad_char(text) {
        return Err(WriteError::Unsupported(WORDS_XML_CHAR));
    }
    if text.contains('\r') {
        return Err(WriteError::Unsupported(WORDS_CARRIAGE_RETURN));
    }
    Ok(())
}

/// The tree items a comment's annotation is: the element, its attributes, its three fields with their texts, and each paragraph with what is in it.
fn items_of(c: &Entry, text: &str) -> u64 {
    let mut items: u64 = 16;
    for line in text.split('\n') {
        // The paragraph, its text node, and a `text:s` for each run of spaces that is counted and a `text:tab` for each tab, each with a text node beside it.
        items = items.saturating_add(2);
        let mut run = 0u64;
        for ch in line.chars() {
            if ch == ' ' {
                run += 1;
                continue;
            }
            if run > 1 || (run == 1 && ch == '\t') {
                items = items.saturating_add(run.div_ceil(65_536).saturating_mul(2));
            }
            run = 0;
            if ch == '\t' {
                items = items.saturating_add(2);
            }
        }
        if run > 0 {
            items = items.saturating_add(run.div_ceil(65_536).saturating_mul(2));
        }
    }
    items.saturating_add(c.comment.raw.len() as u64)
}

/// The bytes `text::encode` writes for one line of a comment's text under a text prefix `p` bytes long, counted without writing: a character as it is escaped, a tab as an
/// element, a run of spaces as the literal space it keeps and the counted elements the rest of it is (each at the most an element with its count takes), and the paragraph's tags.
fn line_bytes(line: &str, p: u64) -> u64 {
    let total = line.chars().count() as u64;
    // `<p:p>` and `</p:p>`.
    let mut bytes = 2 * p + 9;
    let (mut i, mut it) = (0_u64, line.chars().peekable());
    while let Some(c) = it.next() {
        i += 1;
        match c {
            ' ' => {
                let mut n = 1_u64;
                while it.peek() == Some(&' ') {
                    it.next();
                    n += 1;
                }
                let edge = i == 1 || i + n - 1 == total;
                i += n - 1;
                let (literal, counted) = if edge { (0, n) } else { (1, n - 1) };
                // `<p:s p:c="65536"/>` at the most.
                bytes += literal + counted.div_ceil(65_536) * (11 + 2 * p + 5);
            }
            '\t' => bytes += p + 7,
            '&' => bytes += 5,
            '<' | '>' => bytes += 4,
            other => bytes += other.len_utf8() as u64,
        }
    }
    bytes
}

/// The most bytes one comment's annotation is: its tags and declarations, its attributes under the prefixes they are written with (`prefix_most` the longest), its three fields and
/// its lines.
fn entry_bytes(e: &Entry, text: &str, prefix_most: u64, text_prefix: u64) -> u64 {
    let c = &*e.comment;
    let raw: u64 = odf_attrs(&c.raw).iter().map(|(n, v)| n.len() as u64 + (v.len() as u64) * 6).sum();
    // The place (two lengths of at most 40 bytes), the version, the number, the shape, the parent, the Done mark and the edit date: each with its prefix, its name, its quotes and a
    // blank, and the raw attributes a file gave, each with its prefix.
    let attrs = 14 * (prefix_most + 24) + 2 * 40 + 64 + c.edit_date.as_ref().map_or(0, |d| d.len() as u64 * 6) + raw;
    // The three fields, each an element with an end tag under a prefix: `<dc:creator>`, `<dc:date>` and `<meta:creator-initials>` with their ends.
    let fields = 3 * 2 * (prefix_most + 24) + (e.creator.len() as u64 + c.date.len() as u64 + e.initials.len() as u64) * 5;
    let lines: u64 = if c.text.is_empty() { 0 } else { text.split('\n').map(|line| line_bytes(line, text_prefix)).fold(0, u64::saturating_add) };
    // `<office:annotation`, its end tag and the `>`, under a prefix.
    (2 * (prefix_most + 14) + 4_u64).saturating_add(attrs).saturating_add(fields).saturating_add(lines)
}

/// The text of the annotations of a page's comments, to put at the end of the page. `scope` is the scope the page's children see. Everything is charged before it is made.
pub(crate) fn markup(
    ed: &mut Editor<'_, '_>,
    scope: u32,
    page_declared: &[String],
    page: &Page,
    (height, unit): (f64, crate::read::units::LenUnit),
    edit: &CommentEdit,
) -> Result<Block, WriteError> {
    if edit.list.is_empty() {
        return Ok(Block::default());
    }
    // The prefixes: the places `svg` and `text`, the extension's, and the three names ODF's elements of an annotation are in. **What the place does not bind is declared once, on the
    // page's own start tag**, and never on each annotation (a part holds at most 100,000 declarations, and a page may hold as many comments).
    let pfx = Pfx::of(&mut ed.scopes, scope, ed.work);
    let (dia, dia_declare) = match ed.scopes.prefix_for(scope, DIACRAFT_URI, ed.work) {
        Some(p) => (p, String::new()),
        None => {
            let fresh = ed.scopes.fresh_prefix(crate::ext::names::PREFIX, ed.work);
            let declare = format!(" xmlns:{fresh}=\"{DIACRAFT_URI}\"");
            (fresh, declare)
        }
    };
    let holder = ed.scopes.holder_names().cloned().unwrap_or_default();
    let mut declare = format!("{}{dia_declare}", pfx.declare);
    let mut name = |bound: Option<String>, base: &str, uri: &str| -> String {
        match bound.filter(|p| !page_declared.contains(p)) {
            Some(p) => p,
            None => {
                let chosen = if ed.scopes.is_declared(base) { ed.scopes.fresh_prefix(base, ed.work) } else { base.to_owned() };
                declare.push_str(&format!(" xmlns:{chosen}=\"{uri}\""));
                chosen
            }
        }
    };
    let (office, dc, meta) = (name(holder.office, "office", OFFICE_URI), name(holder.dc, "dc", DC_URI), name(holder.meta, "meta", META_URI));
    // The strings a file can hold, and what the whole will be, before any of it is made: **what is written is counted by its shape and the prefixes it is written under** (a newline
    // is a paragraph's two tags, a tab an element, a run of spaces another, a name written in each attribute), never by the text's length.
    let (mut bytes, mut items) = (declare.len() as u64, 0_u64);
    let prefix_most = pfx.svg.len().max(dia.len()).max(pfx.text.len()).max(office.len()).max(dc.len()).max(meta.len()) as u64;
    for e in &edit.list {
        let c = &*e.comment;
        for s in [&*c.text, &*c.date, &*e.creator, &*e.initials] {
            writable(s)?;
        }
        if let Some(d) = &c.edit_date {
            writable(d)?;
        }
        for (_, v) in odf_attrs(&c.raw) {
            writable(v)?;
        }
        let text = normal(&c.text);
        bytes = bytes.saturating_add(entry_bytes(e, &text, prefix_most, pfx.text.len() as u64));
        items = items.saturating_add(items_of(e, &text));
    }
    ed.work.charge_tree_items(items, ed.limits)?;
    ed.work.charge_steps(edit.list.len())?;
    ed.work.hold(bytes, "memory for the annotations of a page")?;
    ed.work.comment_bytes_held = ed.work.comment_bytes_held.saturating_add(bytes);
    // Where the comments the model made go: on the shapes' markers, or inside the page's corner.
    let wanted: HashSet<u32> = edit
        .list
        .iter()
        .filter(|e| odf_attrs(&e.comment.raw).iter().all(|(n, _)| *n != "svg:x") && e.comment.shape.is_some())
        .filter_map(|e| e.comment.shape)
        .collect();
    let places = marker_places(page, &wanted, ed.limits, ed.work)?;
    let width = own::cell_number(&page.sheet, "PageWidth").unwrap_or(0.0);
    let mut out = String::new();
    out.try_reserve(usize::try_from(bytes.min(1 << 31)).unwrap_or(0))
        .map_err(|_| WriteError::Limit { what: "memory for the annotations of a page", max: bytes })?;
    for e in &edit.list {
        let c = &*e.comment;
        let raw = odf_attrs(&c.raw);
        let has = |name: &str| raw.iter().any(|(n, _)| *n == name);
        out.push_str(&format!("<{office}:annotation"));
        // The place: the file's own, else the marker of the shape or the page's corner, in the page's unit and from the page's top.
        if !has("svg:x") || !has("svg:y") {
            let (x, y_up) = match c.shape.and_then(|s| places.get(&s)) {
                Some(&(x, y)) => (x, y),
                None => ((width - MARKER_INSET).max(0.0), (height - MARKER_INSET).max(0.0)),
            };
            let down = height - y_up;
            if let (Some(sx), Some(sy)) = (lengths::length_text(x, unit), lengths::length_text(down, unit)) {
                if !has("svg:x") {
                    out.push_str(&format!(" {}:x=\"{sx}\"", pfx.svg));
                }
                if !has("svg:y") {
                    out.push_str(&format!(" {}:y=\"{sy}\"", pfx.svg));
                }
            }
        }
        for (name, value) in &raw {
            let (prefix, local) = name.split_once(':').unwrap_or(("svg", name));
            let prefix = if prefix == "svg" { pfx.svg.as_str() } else { office.as_str() };
            out.push_str(&format!(" {prefix}:{local}=\"{}\"", escape_value(value, b'"')));
        }
        out.push_str(&format!(" {dia}:version=\"{}\" {dia}:id=\"{}\"", crate::ext::names::VERSION, c.id));
        if let Some(shape) = e.tie {
            out.push_str(&format!(" {dia}:shape=\"{shape}\""));
        }
        if let Some(parent) = c.parent {
            out.push_str(&format!(" {dia}:parent=\"{parent}\""));
        }
        if c.done {
            out.push_str(&format!(" {dia}:done=\"1\""));
        }
        if let Some(edited) = &c.edit_date {
            out.push_str(&format!(" {dia}:edited=\"{}\"", escape_value(edited, b'"')));
        }
        out.push('>');
        if !e.creator.is_empty() {
            out.push_str(&format!("<{dc}:creator>{}</{dc}:creator>", escape_text(&e.creator)));
        }
        if !c.date.is_empty() {
            out.push_str(&format!("<{dc}:date>{}</{dc}:date>", escape_text(&c.date)));
        }
        if !e.initials.is_empty() {
            out.push_str(&format!("<{meta}:creator-initials>{}</{meta}:creator-initials>", escape_text(&e.initials)));
        }
        if !c.text.is_empty() {
            for line in normal(&c.text).split('\n') {
                out.push_str(&format!("<{p}:p>{}</{p}:p>", super::text::encode(line, &pfx.text, ""), p = pfx.text));
            }
        }
        out.push_str(&format!("</{office}:annotation>"));
    }
    // What was held was counted from the shape of the text: a count that is under what was written is a fault of this module, and the save is refused.
    if (out.len() + declare.len()) as u64 > bytes {
        return Err(WriteError::Mismatch("the annotations of a page came to more bytes than were counted for them"));
    }
    Ok(Block { declare, text: out })
}

/// Takes the annotations the reader took out of the page `scan` (a page the file has) and gives the text of the page's comments, to put at the end of the page. `scope` is the
/// scope the page's children see; `new_page` says the page is one the save writes, which has nothing to take out.
pub(crate) fn prepare(
    ed: &mut Editor<'_, '_>,
    scan: &ScanPage,
    scope: u32,
    diff: &super::compare::PageDiff,
    edit: &CommentEdit,
    new_page: bool,
) -> Result<Block, WriteError> {
    if !new_page {
        if scan.annotations.len() != edit.total as usize {
            return Err(WriteError::Mismatch("the part holds other annotations than the file was read with"));
        }
        for &place in &edit.remove {
            let &(lead, end) =
                scan.annotations.get(place as usize).ok_or(WriteError::Mismatch("an annotation the index names is not in the part"))?;
            ed.splicer.remove(lead, end);
        }
    }
    let page = ed.doc.pages.get(diff.page_at).ok_or(WriteError::Mismatch("a page of the comments is not in the drawing"))?;
    markup(ed, scope, &scan.declared, page, (diff.height, diff.unit), edit)
}

/// The page's annotation elements after the save, by their place among the page's annotation children: those the reader did not take, then the `n` this save wrote.
pub(crate) fn places_after(edit: &CommentEdit) -> (Vec<u32>, u32) {
    let (kept, total) = counts_after(edit.total, edit.remove.len(), edit.list.len());
    ((kept..total).collect(), total)
}

/// [`places_after`]'s arithmetic: of `total` annotation children, `removed` taken out and `wrote` written, the first place of those written (the count kept) and the count after
/// the save, each held at `u32::MAX`.
pub(crate) fn counts_after(total: u32, removed: usize, wrote: usize) -> (u32, u32) {
    let kept = total.saturating_sub(u32::try_from(removed).unwrap_or(u32::MAX));
    let wrote = u32::try_from(wrote).unwrap_or(u32::MAX);
    (kept, kept.saturating_add(wrote))
}
