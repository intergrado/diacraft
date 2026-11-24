//! The stamps of the items: a fingerprint of **the native outputs an item sits beside** (the design's 5.4, tier 3).
//!
//! An item is good only while the VSDX file says what it said when the item was written. So the stamp of an item is made of what the file says of the object it is keyed to, **as
//! VSDX says it**: the object's own fields, its sheet (every cell with its formula, value, unit and error, every section and row), its text, and what the `vsdx` side of its slots
//! holds. **It is made of nothing the item carries** (the `odf` and `native` sides of the slots, the text parts that ride): the writer stamps the model it is writing and the
//! reader stamps the model it has just read from the file, which holds none of them yet, and the two are equal when the file is as it was written.
//!
//! The hash is the project's own (`diacraft_doc::stamp::Digest`: FNV-1a, 64 bits, whole fields prefixed by their length). A shape's stamp is of the shape and not of its children,
//! each of which has an item and a stamp of its own. **A sheet shared by many shapes is stamped once** (by the address of its `Arc`, which a model being walked holds), so a
//! drawing of a million shapes that hold one sheet is one walk of it.
//!
//! What a stamp leaves out, so that a program that only looks at the drawing does not stale its items: the view of a page (`ViewScale`, `ViewCenterX`, `ViewCenterY`), the names of the
//! parts the model came from (`part_name`, `rel_id`), the name and type of a picture's part and the attributes a writer gives a picture or an object (the picture's length and first eight bytes
//! stand for it: a picture swapped for another of the same length and beginning is not told, and the data that rides beside a picture is about its place and not its pixels).

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::stamp::{Digest, Stamp};
use diacraft_doc::{
    Cell, Connect, Connects, Document, Ext, FormulaSlot, Kept, Master, Page, Passthrough, RawAttrs, RawNode, Row, Section, Shape, Sheet, Style, Text,
    TextPart,
};

use crate::work;

thread_local! {
    /// The steps of stamping this thread has made: each node, cell, row and section hashed. **Always on** (the judge of a read charges its budget by the difference of two readings), and
    /// counted for the tests of `work` as well.
    static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The steps of stamping this thread has made so far.
pub(crate) fn steps() -> u64 {
    STEPS.with(std::cell::Cell::get)
}

fn bump(n: usize) {
    STEPS.with(|c| c.set(c.get().saturating_add(n as u64)));
    work::carry_stamped(n);
}

/// A page's attributes that say how it is looked at and not what it holds.
const VIEW: [&str; 3] = ["ViewScale", "ViewCenterX", "ViewCenterY"];

/// The attributes of a `ForeignData` that a writer gives a picture or an object it writes (`write/model.rs`, `write/objects.rs`): left out of the stamp of a shape that shows a picture.
const WRITER_GIVEN: [&str; 6] = ["ForeignType", "CompressionType", "ObjectType", "ShowAsIcon", "ObjectWidth", "ObjectHeight"];

fn count(d: &mut Digest, n: usize) {
    d.bytes(&u64::try_from(n).unwrap_or(u64::MAX).to_le_bytes());
}

fn flag(d: &mut Digest, on: bool) {
    d.bytes(&[u8::from(on)]);
}

fn opt(d: &mut Digest, text: Option<&str>) {
    flag(d, text.is_some());
    if let Some(text) = text {
        d.text(text);
    }
}

fn number(d: &mut Digest, n: Option<u32>) {
    flag(d, n.is_some());
    if let Some(n) = n {
        d.bytes(&n.to_le_bytes());
    }
}

fn attrs(d: &mut Digest, raw: &RawAttrs, skip: &[&str]) {
    let kept: Vec<(&str, &str)> = raw.iter().filter(|(name, _)| !skip.contains(name)).collect();
    count(d, kept.len());
    for (name, value) in kept {
        d.text(name);
        d.text(value);
    }
}

/// A node and everything in it, in order, with no recursion: a model built in memory may nest as deep as it likes. One unit of work for each node.
fn nodes(d: &mut Digest, list: &Passthrough) {
    count(d, list.len());
    let mut stack: Vec<&RawNode> = Vec::new();
    for placed in list.iter() {
        d.bytes(&placed.after.to_le_bytes());
        stack.push(&placed.node);
        while let Some(node) = stack.pop() {
            bump(1);
            match node {
                RawNode::Element(el) => {
                    d.bytes(&[0]);
                    d.text(&el.name);
                    attrs(d, &el.attrs, &[]);
                    count(d, el.children.len());
                    stack.extend(el.children.iter().rev());
                }
                RawNode::Text(text) => {
                    d.bytes(&[1]);
                    d.text(text);
                }
                RawNode::Comment(text) => {
                    d.bytes(&[2]);
                    d.text(text);
                }
                RawNode::Pi { target, data } => {
                    d.bytes(&[3]);
                    d.text(target);
                    d.text(data);
                }
            }
        }
    }
}

fn kept(d: &mut Digest, k: &Kept) {
    attrs(d, &k.raw, &[]);
    nodes(d, &k.kept);
}

/// The `vsdx` side of a slot: what the file had here that the model has no field for.
fn vsdx_side<T: Default + Clone + PartialEq>(d: &mut Digest, slot: &Ext<T>) {
    vsdx_side_but(d, slot, &[]);
}

/// [`vsdx_side`] without the entries `skip` names (by their keys). **A page's own `PageContents` and `Rel` entries are left out** (the concern 6): the writer makes both
/// elements for every page whatever the model holds (`xml:space` on the part's root, the `r:id` of the relationship), and the reader keeps them as the unheld attributes of an element it types, so a
/// page the model made (an `.odg`'s) held none when its item was stamped and held both when the file was read: **every such page's item was stale after the save**. They say nothing a person edits.
fn vsdx_side_but<T: Default + Clone + PartialEq>(d: &mut Digest, slot: &Ext<T>, skip: &[&str]) {
    let rest: Vec<&(std::sync::Arc<str>, Kept)> =
        slot.get().map(|data| data.vsdx.0.iter().filter(|(key, _)| !skip.contains(&&**key)).collect()).unwrap_or_default();
    count(d, rest.len());
    for (key, k) in rest {
        d.text(key);
        kept(d, k);
    }
}

fn cell(d: &mut Digest, name: &str, c: &Cell) {
    bump(1);
    d.text(name);
    match &c.formula {
        FormulaSlot::Absent => d.bytes(&[0]),
        FormulaSlot::Local(f) => d.bytes(&[1]).text(&f.source),
        FormulaSlot::Inherit => d.bytes(&[2]),
        FormulaSlot::NoFormula => d.bytes(&[3]),
    };
    opt(d, c.value.as_ref().map(|v| v.text()));
    opt(d, c.unit.as_deref());
    opt(d, c.error.as_deref());
    attrs(d, &c.raw, &[]);
    flag(d, c.extra.is_some());
    if let Some(extra) = &c.extra {
        opt(d, extra.text.as_deref());
        nodes(d, &extra.passthrough);
        vsdx_side(d, &extra.ext);
    }
}

fn cells(d: &mut Digest, list: &diacraft_doc::Cells) {
    count(d, list.len());
    for (name, c) in list {
        cell(d, name.as_str(), c);
    }
}

fn row(d: &mut Digest, r: &Row) {
    bump(1);
    opt(d, r.name.as_deref());
    number(d, r.ix);
    opt(d, r.local_name.as_deref());
    opt(d, r.row_type.as_deref());
    flag(d, r.del);
    cells(d, &r.cells);
    attrs(d, &r.raw, &[]);
    nodes(d, &r.passthrough);
    vsdx_side(d, &r.ext);
}

fn section(d: &mut Digest, s: &Section) {
    bump(1);
    d.text(s.name.as_str());
    number(d, s.ix);
    flag(d, s.del);
    cells(d, &s.cells);
    count(d, s.rows.len());
    for r in &s.rows {
        row(d, r);
    }
    attrs(d, &s.raw, &[]);
    nodes(d, &s.passthrough);
    vsdx_side(d, &s.ext);
}

fn sheet(d: &mut Digest, s: &Sheet) {
    bump(1);
    number(d, s.links.line);
    number(d, s.links.fill);
    number(d, s.links.text);
    cells(d, &s.cells);
    count(d, s.sections.len());
    for section_ in &s.sections {
        section(d, section_);
    }
    attrs(d, &s.raw, &[]);
    nodes(d, &s.passthrough);
    vsdx_side(d, &s.ext);
}

fn text(d: &mut Digest, t: &Text) {
    // The characters between two markers are one run, however the model split them: VSDX writes them as one text and reads them back as one.
    let mut run = String::new();
    count(d, t.parts.len().min(1));
    for part in &t.parts {
        bump(1);
        let marker: Option<(u8, u32, &str)> = match part {
            TextPart::Chars(chars) => {
                run.push_str(chars);
                None
            }
            TextPart::Cp(ix) => Some((1, *ix, "")),
            TextPart::Pp(ix) => Some((2, *ix, "")),
            TextPart::Tp(ix) => Some((3, *ix, "")),
            TextPart::Fld { ix, text } => Some((4, *ix, text)),
            TextPart::Raw(node) => {
                d.bytes(b"c").text(&std::mem::take(&mut run));
                let mut one = Passthrough::new();
                one.push(0, node.clone());
                d.bytes(&[5]);
                nodes(d, &one);
                None
            }
            // What rides is not what VSDX says.
            TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => None,
        };
        if let Some((tag, ix, shown)) = marker {
            d.bytes(b"c").text(&std::mem::take(&mut run));
            d.bytes(&[tag]).bytes(&ix.to_le_bytes()).text(shown);
        }
    }
    d.bytes(b"c").text(&run);
    vsdx_side(d, &t.ext);
}

fn connects(d: &mut Digest, list: &Connects) {
    count(d, list.len());
    for c in list {
        bump(1);
        connect(d, c);
    }
}

fn connect(d: &mut Digest, c: &Connect) {
    d.bytes(&c.from_sheet.to_le_bytes());
    opt(d, c.from_cell.as_deref());
    number(d, c.from_part);
    d.bytes(&c.to_sheet.to_le_bytes());
    opt(d, c.to_cell.as_deref());
    number(d, c.to_part);
    attrs(d, &c.raw, &[]);
    vsdx_side(d, &c.ext);
}

/// What copying a text costs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextCost {
    /// Its parts, and the nodes of the markup they keep.
    pub parts: u64,
    /// The bytes of its characters.
    pub bytes: u64,
    /// The elements of the lists its slot holds.
    pub ext: u64,
}

/// The nodes in a kept node and everything in it, counted with no recursion.
fn raw_nodes(node: &RawNode) -> u64 {
    let mut total = 0u64;
    let mut stack: Vec<&RawNode> = vec![node];
    while let Some(n) = stack.pop() {
        total = total.saturating_add(1);
        if let RawNode::Element(el) = n {
            total = total.saturating_add(el.attrs.len() as u64);
            stack.extend(el.children.iter());
        }
    }
    total
}

/// The stamps of the shared sheets of a model: each distinct sheet once.
#[derive(Default)]
pub(crate) struct SheetStamps {
    /// The cost of copying each text looked at, by address, **with a hold on the text** (as the sheets are held): its parts and the nodes of those that keep markup, and the bytes of its
    /// characters (which an item that puts a part among them splits and copies).
    texts: HashMap<usize, (Arc<Text>, TextCost)>,
    /// The stamp of each sheet stamped, by address, **with a hold on the sheet**: the address stays the sheet's, and a sheet that is held here is copied (and not changed in place) when an item puts
    /// data on it, so that the stamp is good for as long as the table lives and is not made again after each put.
    by_address: HashMap<usize, (Arc<Sheet>, Stamp, u64)>,
}

impl SheetStamps {
    pub(crate) fn of(&mut self, s: &Arc<Sheet>) -> Stamp {
        let key = Arc::as_ptr(s).addr();
        if let Some((_, found, _)) = self.by_address.get(&key) {
            return *found;
        }
        let before = steps();
        let mut d = Digest::new("carry/sheet");
        sheet(&mut d, s);
        let stamp = d.finish();
        self.by_address.insert(key, (Arc::clone(s), stamp, steps().saturating_sub(before)));
        stamp
    }

    /// What copying a text costs ([`TextCost`]): the parts it has, the bytes of its characters and the slot it holds. An item that puts data on a shape whose text others share copies the text
    /// (it is held here, so it is not changed in place): its parts and each node of the markup they keep, the characters a part that rides is put among, and the elements of the lists its slot
    /// holds (`size::slot`). Walked once for each text, and charged by the judge for each copy.
    pub(crate) fn text_cost(&mut self, t: &Arc<Text>) -> TextCost {
        let key = Arc::as_ptr(t).addr();
        if let Some((_, cost)) = self.texts.get(&key) {
            return *cost;
        }
        let mut cost = TextCost { parts: 0, bytes: 0, ext: super::size::slot(&t.ext) };
        bump(usize::try_from(cost.ext).unwrap_or(usize::MAX));
        for part in &t.parts {
            bump(1);
            cost.parts = cost.parts.saturating_add(1);
            match part {
                TextPart::Chars(chars) => cost.bytes = cost.bytes.saturating_add(chars.len() as u64),
                TextPart::Fld { text, .. } => cost.bytes = cost.bytes.saturating_add(text.len() as u64),
                TextPart::Raw(node) => cost.parts = cost.parts.saturating_add(raw_nodes(node)),
                TextPart::OdfRaw(node) | TextPart::NativeRaw(node) => cost.parts = cost.parts.saturating_add(raw_nodes(node)),
                TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_) => {}
            }
        }
        self.texts.insert(key, (Arc::clone(t), cost));
        cost
    }

    /// What copying the sheet costs: **the nodes it has** (its cells, sections, rows and the cells of each), which is what stamping it walked, kept with its stamp. An item that puts data
    /// on a sheet the table holds copies it (it is held, so it is not changed in place), and the judge charges this for each copy, however many items put data on the one sheet.
    pub(crate) fn copy_steps(&mut self, s: &Arc<Sheet>) -> u64 {
        self.of(s);
        self.by_address.get(&Arc::as_ptr(s).addr()).map_or(0, |(_, _, steps)| *steps)
    }
}

fn with_sheet(d: &mut Digest, sheets: &mut SheetStamps, s: &Arc<Sheet>) {
    d.bytes(&sheets.of(s).0.to_le_bytes());
}

/// The stamp of a shape: its fields, its sheet, its text and its picture's place. Not its children.
pub(crate) fn shape_stamp(s: &Shape, sheets: &mut SheetStamps) -> Stamp {
    let mut d = Digest::new("carry/shape");
    d.bytes(&s.id.to_le_bytes());
    opt(&mut d, s.name.as_deref());
    opt(&mut d, s.name_u.as_deref());
    opt(&mut d, s.kind.text());
    number(&mut d, s.master);
    number(&mut d, s.master_shape);
    flag(&mut d, s.del);
    opt(&mut d, s.data1.as_deref());
    opt(&mut d, s.data2.as_deref());
    opt(&mut d, s.data3.as_deref());
    with_sheet(&mut d, sheets, &s.sheet);
    flag(&mut d, s.text.is_some());
    if let Some(t) = &s.text {
        text(&mut d, t);
    }
    flag(&mut d, s.foreign.is_some());
    if let Some(f) = &s.foreign {
        // A shape that shows a picture is written with the attributes its picture calls for (a bitmap's `CompressionType`, an object's `ObjectType` and the rest) and a part of the file's naming, which the
        // model it was written from does not have in the same form: they are the writer's and no part of what another program could have edited.
        attrs(&mut d, &f.attrs, if f.picture.is_some() { &WRITER_GIVEN } else { &[] });
        opt(&mut d, f.text.as_deref());
        flag(&mut d, f.picture.is_some());
        if let Some(m) = &f.picture {
            // The bytes' length and their first eight stand for them; the name of the part and the type it is given are the writer's choice.
            d.bytes(&(m.bytes.len() as u64).to_le_bytes()).bytes(m.bytes.get(..8).unwrap_or(&m.bytes));
        }
        nodes(&mut d, &f.passthrough);
        vsdx_side(&mut d, &f.ext);
    }
    attrs(&mut d, &s.raw, &[]);
    nodes(&mut d, &s.passthrough);
    vsdx_side(&mut d, &s.ext);
    d.finish()
}

pub(crate) fn page_stamp(p: &Page, sheets: &mut SheetStamps) -> Stamp {
    let mut d = Digest::new("carry/page");
    d.bytes(&p.id.to_le_bytes());
    d.text(&p.name);
    opt(&mut d, p.name_u.as_deref());
    flag(&mut d, p.background);
    number(&mut d, p.back_page);
    with_sheet(&mut d, sheets, &p.sheet);
    connects(&mut d, &p.connects);
    attrs(&mut d, &p.raw, &VIEW);
    nodes(&mut d, &p.passthrough);
    vsdx_side_but(&mut d, &p.ext, &["PageContents", "Rel"]);
    d.finish()
}

pub(crate) fn master_stamp(m: &Master, sheets: &mut SheetStamps) -> Stamp {
    let mut d = Digest::new("carry/master");
    d.bytes(&m.id.to_le_bytes());
    opt(&mut d, m.name.as_deref());
    opt(&mut d, m.name_u.as_deref());
    opt(&mut d, m.unique_id.as_deref());
    number(&mut d, m.pattern_flags);
    opt(&mut d, m.icon.as_deref());
    with_sheet(&mut d, sheets, &m.sheet);
    connects(&mut d, &m.connects);
    attrs(&mut d, &m.raw, &[]);
    nodes(&mut d, &m.passthrough);
    vsdx_side(&mut d, &m.ext);
    d.finish()
}

pub(crate) fn style_stamp(s: &Style, sheets: &mut SheetStamps) -> Stamp {
    let mut d = Digest::new("carry/style");
    d.bytes(&s.id.to_le_bytes());
    opt(&mut d, s.name.as_deref());
    opt(&mut d, s.name_u.as_deref());
    with_sheet(&mut d, sheets, &s.sheet);
    attrs(&mut d, &s.raw, &[]);
    nodes(&mut d, &s.passthrough);
    vsdx_side(&mut d, &s.ext);
    d.finish()
}

/// The stamp of the document: what the file says **about** the drawing: its title and the descriptive fields of its properties. Not its dates, its application or its last editor (a
/// save of VSDX writes those itself), not its font table or its unknown elements (a drawing that was never saved is given the faces its cells name and a `DocumentSettings` by
/// the writer, which its model does not hold), and not its pages, masters and styles, each of which has its own item.
/// The stamp of the comments as a VSDX file says them: each comment's number, page, shape, author, dates, Done mark and text, in order. **The thread is no part of it**, since the
/// file has none: the writer stamps the model it writes, whose replies have parents, and the reader the model it read from the file, whose have not.
pub(crate) fn comments_stamp(comments: &diacraft_doc::Comments) -> Stamp {
    let mut d = Digest::new("carry/comments");
    let names: HashMap<u32, (&str, &str)> = comments.authors().map(|a| (a.id, (&*a.name, &*a.initials))).collect();
    bump(comments.len().saturating_add(1));
    count(&mut d, comments.len());
    for c in comments.iter() {
        d.bytes(&c.id.to_le_bytes());
        d.bytes(&c.page.to_le_bytes());
        number(&mut d, c.shape);
        let (name, initials) = names.get(&c.author).copied().unwrap_or_default();
        d.text(name);
        d.text(initials);
        d.text(&c.date);
        opt(&mut d, c.edit_date.as_deref());
        flag(&mut d, c.done);
        d.text(&c.text);
    }
    d.finish()
}

pub(crate) fn document_stamp(doc: &Document) -> Stamp {
    let mut d = Digest::new("carry/document");
    d.text(&doc.title);
    let p = &doc.props;
    for field in [&p.subject, &p.creator, &p.keywords, &p.description, &p.category, &p.company, &p.manager, &p.hyperlink_base, &p.language] {
        opt(&mut d, field.as_deref());
    }
    d.finish()
}
