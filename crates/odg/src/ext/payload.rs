//! What a shape's own automatic style carries of tier 2, as a value: the cells, sections and text the reader would not rebuild from the shape's native
//! markup, and the few fields of the shape that are no cell (the last row of "Styles"; the proposal), with the encoding
//! of each in the attributes of [`super::names`] and the inverse.
//!
//! **The fragments.** A group of cells, one section and a text are each one attribute whose value is XML text: a `Sheet` element holding `Cell`
//! elements (`cells`), a `Sheet` holding one `Section` (`s-...`), or a `Text` element. They are the elements the codec of the ShapeSheet writes and
//! reads (`diacraft-sheetxml`), in no namespace, so that everything a sheet keeps of a cell, a row and a section (a formula, a value, a unit, an error,
//! attributes, the content of a cell, the unknown nodes of a section) is carried by the code that carries it in a drawing of the other format. The
//! text is escaped once more as the value of an attribute, which LibreOffice keeps exactly.
//!
//! **Costs.** Every fragment is charged by its bytes before it is parsed, parsed within the container's caps for a tree, read by a sheet reader whose
//! budgets are the document's own (cells and items are taken from the reader's totals afterwards, by what the sheet reader spent), and never kept
//! longer than the shape that holds it.

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{CellName, Cells, RawNode, Section, SectionKey, ShapeType, Sheet, Text, TextPart};
use diacraft_sheetxml::passthrough::{canonical_u32, raw_element};
use diacraft_sheetxml::{Caps, Cx, Scope, SheetReader, Vocabulary, Warnings};

use super::names::{self, Gone};
use crate::error::OdgError;
use crate::read::Reader;

/// The most one attribute of the extension may hold, in bytes: a section is refused past it by the writer (the reader reads what a file holds up to
/// the container's cap for an attribute, a megabyte).
pub(crate) const MOST_ITEM_BYTES: usize = 512 * 1024;

/// The most attributes of the extension one style is read with: a section each, a fingerprint for each section (tier 3 fingerprints the `Geometry` sections, a
/// subset of them), and the fixed names (the sixteen of and the text's, the names', the master link's and the eleven cells' fingerprints).
pub(crate) const MOST_ATTRIBUTES: usize = 2 * diacraft_doc::MAX_SECTIONS_PER_SHEET + 32;

/// A vocabulary that reads elements in no namespace and no other: the fragments are written bare.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Bare;

impl Vocabulary for Bare {
    fn reads(self, uri: Option<&str>) -> bool {
        matches!(uri, None | Some(""))
    }
}

/// A shape's tier 2.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Tier2 {
    pub id: Option<u32>,
    pub name: Option<String>,
    pub name_u: Option<String>,
    pub master: Option<u32>,
    pub master_shape: Option<u32>,
    /// The kind of the shape, when it is not the one the reader gives (`Shape`): only `Guide`, which has no element of its own in ODF.
    pub kind: Option<ShapeType>,
    /// The cells of the shape's own sheet that replace what the reader rebuilt, and are in the model.
    pub cells: Cells,
    /// The sections that replace what the reader rebuilt, whole, in the order written.
    pub sections: Vec<Arc<Section>>,
    /// The shape's whole text, when it is not what the reader rebuilt.
    pub text: Option<Text>,
    /// The shell of the shape's VSDX data (`shell::shape_shell`): its attributes and elements, `Data1` to `Data3`, its deleted flag, its kind, and its slots.
    pub vsdx: Option<Arc<diacraft_doc::Shape>>,
    /// The foreign data of the shape beyond its picture (`shell::foreign_shell`).
    pub foreign: Option<diacraft_doc::ForeignData>,
    /// What the reader rebuilt and the model does not have.
    pub gone: Vec<Gone>,
    /// The fingerprints of the outputs of the items above (tier 3), by the local name of the attribute (`fp-text`, `fp-s-Geometry-0`), each as the file has
    /// it: a value that is no fingerprint is kept as it is, so that a save that changes nothing writes the same attribute.
    pub fps: std::collections::BTreeMap<String, String>,
}

impl Tier2 {
    /// The sections a text's markers name: `Character`, `Paragraph` and `Field`.
    pub(crate) const TEXT_ROWS: [&'static str; 3] = ["Character", "Paragraph", "Field"];

    /// Whether it carries the rows of the shape's text (a `Character`, `Paragraph` or `Field` section, or the removal of one) **without the text itself**: a text the file says in its
    /// characters whose formatting the markup cannot say whole, so its rows ride over what the markup gives. They are the text's formatting, and are judged by its fingerprint (`fp-format`).
    pub(crate) fn carries_text_rows(&self) -> bool {
        self.sections.iter().any(|s| Tier2::TEXT_ROWS.contains(&s.name.as_str()))
            || self.gone.iter().any(|g| matches!(g, super::names::Gone::Section(k) if Tier2::TEXT_ROWS.contains(&k.name.as_str())))
    }

    /// Whether it carries nothing beside the id: no cell, section, text, name, master or removal.
    pub(crate) fn holds_nothing_but_id(&self) -> bool {
        self.name.is_none()
            && self.name_u.is_none()
            && self.master.is_none()
            && self.master_shape.is_none()
            && self.kind.is_none()
            && self.cells.is_empty()
            && self.sections.is_empty()
            && self.text.is_none()
            && self.vsdx.is_none()
            && self.foreign.is_none()
            && self.gone.is_empty()
    }
}

/// The state a read keeps for fragments: one sheet reader for the whole document, and where its warnings go.
pub(crate) struct ReadState {
    pub sheets: SheetReader,
    pub notes: Warnings,
}

impl ReadState {
    pub(crate) fn new(caps: Caps) -> ReadState {
        ReadState { sheets: SheetReader::new(caps), notes: Warnings::new(0) }
    }
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// The text of `el`, or `None` when it holds a character XML cannot hold.
pub(crate) fn text_of(el: &El) -> Option<String> {
    let mut out = Vec::new();
    diacraft_container::xml::write_element(&mut out, el).ok()?;
    String::from_utf8(out).ok()
}

/// The `Sheet` fragment holding `cells` and `sections`.
pub(crate) fn sheet_fragment(cells: &Cells, sections: &[Arc<Section>]) -> Option<String> {
    let sheet = Sheet { cells: cells.clone(), sections: sections.to_vec(), ..Sheet::default() };
    text_of(&diacraft_sheetxml::build::sheet_only_el("Sheet", &sheet))
}

/// The fragment of a shape's shell: the codec's `shape` element (it declares DiaCraft's namespace itself), as XML text. `None` when it holds a character XML cannot hold.
pub(crate) fn shell_fragment(shell: &diacraft_doc::Shape) -> Option<String> {
    let el = diacraft_sheetxml::model::encode_shape(shell, &mut super::body::NoBlobs).ok()?;
    text_of(&el)
}

/// The `Text` fragment of a text.
pub(crate) fn text_fragment(text: &Text) -> Option<String> {
    let mut children: Vec<Node> = Vec::with_capacity(text.parts.len());
    for part in &text.parts {
        children.push(match part {
            TextPart::Chars(c) => Node::Text(c.to_string()),
            TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => continue, // Not written into a tier-2 text yet
            TextPart::Raw(raw) => diacraft_sheetxml::passthrough::node_to_tree(raw),
            TextPart::Cp(ix) => Node::El(marker("cp", *ix, Vec::new())),
            TextPart::Pp(ix) => Node::El(marker("pp", *ix, Vec::new())),
            TextPart::Tp(ix) => Node::El(marker("tp", *ix, Vec::new())),
            TextPart::Fld { ix, text } => Node::El(marker("fld", *ix, vec![Node::Text(text.to_string())])),
        });
    }
    text_of(&El { name: "Text".to_owned(), attrs: Vec::new(), children })
}

fn marker(name: &str, ix: u32, children: Vec<Node>) -> El {
    El { name: name.to_owned(), attrs: vec![("IX".to_owned(), ix.to_string())], children }
}

/// The attributes of the extension that carry `t`, in the order written (`id`, `name`, `name-u`, `master`, `master-shape`, `type`, `cells`, the sections, `text`, `vsdx`, `foreign`,
/// `gone`, the fingerprints), each as its local name and its value, with no `version`. `None` when something in it cannot be written as text (a character XML cannot hold),
/// and `Err` with the name of an attribute whose value is past [`MOST_ITEM_BYTES`].
pub(crate) fn encode(t: &Tier2) -> Result<Option<Vec<(String, String)>>, &'static str> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut put = |local: String, value: String| -> Result<(), &'static str> {
        if value.len() > MOST_ITEM_BYTES {
            return Err("a shape's data past what one attribute of the file holds");
        }
        out.push((local, value));
        Ok(())
    };
    if let Some(id) = t.id {
        put(names::ID.to_owned(), id.to_string())?;
    }
    if let Some(v) = &t.name {
        put(names::NAME.to_owned(), v.clone())?;
    }
    if let Some(v) = &t.name_u {
        put(names::NAME_U.to_owned(), v.clone())?;
    }
    if let Some(v) = t.master {
        put(names::MASTER.to_owned(), v.to_string())?;
    }
    if let Some(v) = t.master_shape {
        put(names::MASTER_SHAPE.to_owned(), v.to_string())?;
    }
    if let Some(kind) = &t.kind {
        put(names::TYPE.to_owned(), kind.text().unwrap_or_default().to_owned())?;
    }
    if !t.cells.is_empty() {
        let Some(text) = sheet_fragment(&t.cells, &[]) else { return Ok(None) };
        put(names::CELLS.to_owned(), text)?;
    }
    for section in &t.sections {
        let key = SectionKey { name: section.name.clone(), ix: section.ix };
        let Some(text) = sheet_fragment(&Cells::new(), std::slice::from_ref(section)) else { return Ok(None) };
        put(names::section_attr(&key), text)?;
    }
    if let Some(text) = &t.text {
        let Some(text) = text_fragment(text) else { return Ok(None) };
        put(names::TEXT.to_owned(), text)?;
    }
    if let Some(shell) = &t.vsdx {
        let Some(text) = shell_fragment(shell) else { return Ok(None) };
        put(names::VSDX.to_owned(), text)?;
    }
    if let Some(foreign) = &t.foreign {
        let holder = diacraft_doc::Shape { foreign: Some(Arc::new(foreign.clone())), ..diacraft_doc::Shape::default() };
        let Some(text) = shell_fragment(&holder) else { return Ok(None) };
        put(names::FOREIGN.to_owned(), text)?;
    }
    if !t.gone.is_empty() {
        put(names::GONE.to_owned(), t.gone.iter().map(Gone::token).collect::<Vec<_>>().join(" "))?;
    }
    for (local, value) in &t.fps {
        put(local.clone(), value.clone())?;
    }
    Ok(Some(out))
}

// ------------------------------------------------------------------------------------------------------------------ reading

/// The tree of a fragment, parsed within the caps, its bytes charged first.
fn parse_fragment(r: &mut Reader<'_>, text: &str) -> Result<Option<El>, OdgError> {
    r.take_bytes(text.len())?;
    let mut limits = r.limits.container.xml.clone();
    limits.max_nodes = limits.max_nodes.min(r.limits.max_read_xml_nodes);
    let mut work = diacraft_container::Work::default();
    let parsed = diacraft_container::xml::parse_counted(text.as_bytes(), &limits, &mut work);
    r.report.work.package.xml_events = r.report.work.package.xml_events.saturating_add(work.xml_events);
    Ok(parsed.ok())
}

/// Reads a `Sheet` fragment through the document's sheet reader and charges the reader's totals for what it spent: its cells, its items, and **the bytes it priced the
/// model it made at** beyond what the fragment's text paid. The text alone was the charge before, and a cell's formula is not its text: counted, a cell of a
/// formula of 5,000 arguments kept 280 KB for the 10 KB of its text, and was charged a ninth of what the law of the memory tests asks
/// (`tests/it/memory_counted.rs`). The sheet reader prices what it makes as the VSDX reader does (the strings, the nodes of a formula's tree, with its margin).
fn read_sheet_fragment(r: &mut Reader<'_>, text: &str) -> Result<Option<Sheet>, OdgError> {
    let Some(el) = parse_fragment(r, text)? else { return Ok(None) };
    if el.name != "Sheet" {
        return Ok(None);
    }
    let before = r.ext.sheets.stats();
    let sc = Scope::<Bare>::empty().enter(&el);
    let read = {
        let state = &mut r.ext;
        let mut cx = Cx::new(&mut state.notes, "the extension");
        state.sheets.read_sheet_only(&mut cx, &el, &sc)
    };
    let sheet = read.map_err(sheet_limit)?;
    let after = r.ext.sheets.stats();
    r.take_cells(after.cells.saturating_sub(before.cells))?;
    r.take_items(after.items.saturating_sub(before.items))?;
    let made = usize::try_from(after.bytes.saturating_sub(before.bytes)).unwrap_or(usize::MAX);
    r.take_model_bytes(made.saturating_sub(crate::read::cost::text(text.len())))?;
    Ok(Some(sheet))
}

/// Reads a shape's shell from its fragment through the codec, within the room the reader has left (it spends the reader's items and bytes), and **its depth the codec's own** (a kept element a
/// shape's data holds is as deep as the codec writes one). `None` for a fragment that does not read, with a note.
fn read_shell_fragment(r: &mut Reader<'_>, text: &str, what: &str) -> Result<Option<diacraft_doc::Shape>, OdgError> {
    r.take_bytes(text.len())?;
    let mut limits = super::docfile::parse_limits(r.limits);
    limits.max_nodes = limits.max_nodes.min(r.limits.max_read_xml_nodes);
    let mut work = diacraft_container::Work::default();
    let parsed = diacraft_container::xml::parse_counted(text.as_bytes(), &limits, &mut work);
    r.report.work.package.xml_events = r.report.work.package.xml_events.saturating_add(work.xml_events);
    let Ok(el) = parsed else {
        r.warn("diacraft_tier2_unreadable", || format!("a shape's DiaCraft {what} does not read, and is left out"));
        return Ok(None);
    };
    let shape = super::body::decode_in_at(r, &format!("a shape's DiaCraft {what}"), |budget, notes| {
        diacraft_sheetxml::model::decode_shape(&el, &super::body::NoBlobs, budget, notes)
    })?;
    Ok(shape)
}

fn sheet_limit(e: diacraft_sheetxml::LimitError) -> OdgError {
    OdgError::TooComplex { what: e.kind.what(), max: e.max }
}

/// A `Text` fragment as a text: characters, comments, instructions and unknown elements kept, the four markers read.
fn read_text_fragment(r: &mut Reader<'_>, text: &str) -> Result<Option<Text>, OdgError> {
    let Some(el) = parse_fragment(r, text)? else { return Ok(None) };
    if el.name != "Text" {
        return Ok(None);
    }
    let mut parts: Vec<TextPart> = Vec::new();
    for node in &el.children {
        r.take_items(1)?;
        parts.push(match node {
            Node::Text(t) | Node::CData(t) => TextPart::Chars(Arc::from(t.as_str())),
            Node::Comment(c) => TextPart::Raw(RawNode::Comment(Arc::from(c.as_str()))),
            Node::Pi { target, data } => TextPart::Raw(RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) }),
            Node::El(child) => match marker_of(child) {
                Some(part) => part,
                None => {
                    r.take_items(diacraft_sheetxml::passthrough::element_cost(child).saturating_sub(1))?;
                    TextPart::Raw(RawNode::Element(raw_element(child)))
                }
            },
        });
    }
    Ok(Some(Text { parts, ..Default::default() }))
}

fn marker_of(el: &El) -> Option<TextPart> {
    let ix = match el.attrs.as_slice() {
        [(k, v)] if k == "IX" => canonical_u32(v)?,
        _ => return None,
    };
    let plain = el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_)));
    match el.name.as_str() {
        "cp" if el.children.is_empty() => Some(TextPart::Cp(ix)),
        "pp" if el.children.is_empty() => Some(TextPart::Pp(ix)),
        "tp" if el.children.is_empty() => Some(TextPart::Tp(ix)),
        "fld" if plain => Some(TextPart::Fld { ix, text: Arc::from(el.text()) }),
        _ => None,
    }
}

/// What a style's attributes of the extension say, read: the pairs are the local names and values of the owned attributes ([`names::owned`]). A value
/// that does not read is a warning and is left out, never an error (a file that was edited by hand, or by a newer DiaCraft, is read as far as it
/// can be). An error is only a cap passed.
pub(crate) fn decode(r: &mut Reader<'_>, pairs: &[(String, String)]) -> Result<Tier2, OdgError> {
    let mut t = Tier2::default();
    let number = |v: &str| canonical_u32(v);
    for (local, value) in pairs {
        match local.as_str() {
            names::ID => t.id = number(value),
            names::NAME => t.name = Some(value.clone()),
            names::NAME_U => t.name_u = Some(value.clone()),
            names::MASTER => t.master = number(value),
            names::MASTER_SHAPE => t.master_shape = number(value),
            // Only a guide is carried: any other kind is a kind the reader gives from the markup, and is left out with a warning.
            names::TYPE => match value.as_str() {
                "Guide" => t.kind = Some(ShapeType::Guide),
                _ => r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft type is not one this build reads, and is left out".to_owned()),
            },
            names::CELLS => match read_sheet_fragment(r, value)? {
                Some(sheet) => {
                    for (name, cell) in sheet.cells {
                        t.cells.entry(name).or_insert(cell);
                    }
                }
                None => r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft cells do not read, and are left out".to_owned()),
            },
            names::TEXT => match read_text_fragment(r, value)? {
                Some(text) => t.text = Some(text),
                None => r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft text does not read, and is left out".to_owned()),
            },
            names::VSDX => {
                if let Some(shell) = read_shell_fragment(r, value, "data")? {
                    t.vsdx = Some(Arc::new(shell));
                }
            }
            names::FOREIGN => {
                if let Some(shell) = read_shell_fragment(r, value, "foreign data")? {
                    match shell.foreign {
                        Some(foreign) => t.foreign = Some(Arc::unwrap_or_clone(foreign)),
                        None => r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft foreign data holds none, and is left out".to_owned()),
                    }
                }
            }
            names::GONE => {
                r.take_items(value.split_ascii_whitespace().count().min(MOST_ATTRIBUTES * 4))?;
                for token in value.split_ascii_whitespace().take(MOST_ATTRIBUTES * 4) {
                    match Gone::parse(token) {
                        Some(g) => t.gone.push(g),
                        None => {
                            r.warn("diacraft_tier2_unreadable", || "a token of a shape's DiaCraft removals names nothing, and is left out".to_owned())
                        }
                    }
                }
            }
            // A fingerprint of tier 3 is read as it is, and judged by the reader of the shape (`tier3::judge`): decoding says nothing of it.
            other if super::fingerprint::Key::parse(other).is_some() => {
                r.take_items(1)?;
                t.fps.insert(other.to_owned(), value.clone());
            }
            other => match names::section_of_attr(other) {
                Some(key) => match read_sheet_fragment(r, value)? {
                    Some(sheet) => match sheet.sections.into_iter().find(|s| s.name == key.name && s.ix == key.ix) {
                        Some(section) => t.sections.push(section),
                        None => {
                            r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft section holds another section than its name says".to_owned())
                        }
                    },
                    None => r.warn("diacraft_tier2_unreadable", || "a shape's DiaCraft section does not read, and is left out".to_owned()),
                },
                None => r.keep("diacraft_attribute"),
            },
        }
    }
    Ok(t)
}

// ------------------------------------------------------------------------------------------------------------------ applying

/// [`decode`] for a caller that has no read in hand (the writer, which reads what a style carries to edit it): a read of its own, under `limits` but for the
/// items, of which it has `room` at most, which is what the save has left of the budget of all its decodes (the security review: a read of each style
/// with a budget of its own would let a save of many styles pass any cap). Gives what it read and the items the read took.
pub(crate) fn decode_free(limits: &crate::limits::Limits, room: u64, pairs: &[(String, String)]) -> Result<(Tier2, u64), OdgError> {
    let mut own = limits.clone();
    own.max_items = usize::try_from(room).unwrap_or(usize::MAX).min(limits.max_items);
    let mut r = Reader::new(&own);
    let tier2 = decode(&mut r, pairs)?;
    Ok((tier2, r.report.work.items))
}

/// Puts cells, sections and removals over a sheet: the removals first, then the cells, then the sections (each replaces the section of its key, or is put
/// last). `Err` for more sections than a sheet may hold.
///
/// The work is linear in the sections of the sheet and of what is put over it: the sections that are gone are taken out in one pass over a set of their keys, and
/// the sections put are found by an index of the keys that follows the list as it grows, so that none is searched for among the others. **A walk of the sheet's
/// sections is what the caller charges** (`docfile::charge_overlay`: one item for each section of each pass), so each is made cheap: the keys are hashed, and a
/// section's name is a shared one that a key holds by a count and does not copy (the index of owned strings cost several times what a
/// pass over a list of sections does).
pub(crate) fn overlay(sheet: &mut Sheet, cells: Cells, sections: Vec<Arc<Section>>, gone: &[Gone]) -> Result<(), OdgError> {
    use std::collections::{HashMap, HashSet};
    let mut gone_sections: HashSet<(&str, Option<u32>)> = HashSet::new();
    for g in gone {
        match g {
            Gone::Cell(name) => {
                sheet.cells.remove(&CellName::new(name));
            }
            Gone::Section(key) => {
                gone_sections.insert((key.name.as_str(), key.ix));
            }
            Gone::Name | Gone::NameU | Gone::Text => {}
        }
    }
    if !gone_sections.is_empty() {
        sheet.sections.retain(|s| !gone_sections.contains(&(s.name.as_str(), s.ix)));
    }
    for (name, cell) in cells {
        sheet.cells.insert(name, cell);
    }
    // The first section of a key is the one a search for the key finds.
    let mut index: HashMap<diacraft_doc::SectionKey, usize> = HashMap::new();
    if !sections.is_empty() {
        index.reserve(sheet.sections.len());
        for (at, s) in sheet.sections.iter().enumerate() {
            index.entry(diacraft_doc::SectionKey { name: s.name.clone(), ix: s.ix }).or_insert(at);
        }
    }
    for section in sections {
        let key = diacraft_doc::SectionKey { name: section.name.clone(), ix: section.ix };
        match index.get(&key).copied() {
            Some(at) => {
                if let Some(slot) = sheet.sections.get_mut(at) {
                    *slot = section;
                }
            }
            None => {
                if sheet.sections.len() >= diacraft_doc::MAX_SECTIONS_PER_SHEET {
                    return Err(OdgError::TooComplex { what: "sections in one sheet", max: diacraft_doc::MAX_SECTIONS_PER_SHEET as u64 });
                }
                index.insert(key, sheet.sections.len());
                sheet.sections.push(section);
            }
        }
    }
    Ok(())
}

/// Puts a tier 2 over what the reader rebuilt: the removals first, then the cells, the sections, and the names, links to a master and the text.
pub(crate) fn apply(t: Tier2, shape: &mut diacraft_doc::Shape, sheet: &mut Sheet) -> Result<(), OdgError> {
    for gone in &t.gone {
        match gone {
            Gone::Name => shape.name = None,
            Gone::NameU => shape.name_u = None,
            Gone::Text => shape.text = None,
            Gone::Cell(_) | Gone::Section(_) => {}
        }
    }
    overlay(sheet, t.cells, t.sections, &t.gone)?;
    if let Some(v) = t.name {
        shape.name = Some(Arc::from(v));
    }
    if let Some(v) = t.name_u {
        shape.name_u = Some(Arc::from(v));
    }
    if t.master.is_some() {
        shape.master = t.master;
    }
    if t.master_shape.is_some() {
        shape.master_shape = t.master_shape;
    }
    if let Some(text) = t.text {
        shape.text = Some(Arc::new(text));
    }
    // The VSDX data: the shell's, over what the markup and the rest of the extension gave (the text first, since the shell puts the text's slot).
    if let Some(shell) = &t.vsdx {
        super::shell::apply(shape, sheet, shell);
    }
    if let Some(foreign) = &t.foreign {
        super::shell::apply_foreign(shape, foreign);
    }
    if let Some(kind) = t.kind {
        shape.kind = kind;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    /// The attributes of a tier 2 that holds `cells` cells.
    fn heavy(cells: usize) -> Vec<(String, String)> {
        let mut t = Tier2 { id: Some(1), ..Tier2::default() };
        for k in 0..cells {
            t.cells.insert(CellName::new(&format!("C{k}")), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("v")));
        }
        encode(&t).ok().flatten().unwrap_or_default()
    }

    /// The bytes of a fragment are charged before it is parsed, in the writer's decode too (P1): the reader of a style charges the
    /// bytes of each attribute it reads, so a read of a drawing never shows the charge of the fragment, but the writer's decode has no such read before it: a value of
    /// 400,000 bytes (white space after the root, which parses) costs the items they stand for, and a budget under them refuses it.
    #[test]
    fn the_bytes_of_a_fragment_are_charged_by_the_writers_decode() {
        let mut pairs = heavy(1);
        let plain = decode_free(&Limits::default(), u64::MAX, &pairs).map(|(_, took)| took).unwrap_or_else(|e| panic!("{e}"));
        for (local, value) in pairs.iter_mut() {
            if local == names::CELLS {
                value.push_str(&" ".repeat(400_000));
            }
        }
        let padded = decode_free(&Limits::default(), u64::MAX, &pairs).map(|(_, took)| took).unwrap_or_else(|e| panic!("{e}"));
        assert!(padded >= plain + 400_000 / 64 - 8, "400,000 bytes cost {} items, fewer than the 6,250 they stand for", padded - plain);
        // A budget that holds the fragment without them and not with them refuses it.
        assert!(matches!(decode_free(&Limits::default(), plain + 100, &pairs), Err(OdgError::Limit { .. })));
    }

    /// The writer's decode of what a style carries is given what is left of one budget for all the decodes of a save, and says what it took (the
    /// security review: a read of each style with a budget of its own let a save of many styles pass any cap).
    #[test]
    fn a_decode_is_given_what_is_left_of_the_saves_budget_and_says_what_it_took() {
        let pairs = heavy(100);
        let limits = Limits::default();
        let (t, took) = match decode_free(&limits, u64::MAX, &pairs) {
            Ok(done) => done,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(t.cells.len(), 100);
        assert!(took >= 100, "{took}");
        // The room it is given is its budget: one item fewer than it takes refuses it, exactly what it takes reads it.
        assert!(matches!(decode_free(&limits, took - 1, &pairs), Err(OdgError::Limit { .. })), "one item fewer than it takes");
        assert!(decode_free(&limits, took, &pairs).is_ok(), "exactly the items it takes");
        // Nothing left is nothing read, and the room is never more than the limits' own.
        assert!(decode_free(&limits, 0, &pairs).is_err());
        let small = Limits { max_items: 10, ..Limits::default() };
        assert!(decode_free(&small, u64::MAX, &pairs).is_err());
    }

    /// `n` sections `S0` to `S{n-1}`, each valued `value` by its one cell.
    fn sections(n: usize, value: f64) -> Vec<Arc<Section>> {
        (0..n)
            .map(|k| {
                let mut s = Section::new(&format!("S{k}"), None);
                s.cells.insert(CellName::new("V"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(value).unwrap()));
                Arc::new(s)
            })
            .collect()
    }

    /// An overlay puts each section over the one of its key, or last, takes out the sections that are gone by their keys, and finds them with an index and a set
    /// and not with a search for each (the security review): the order of the sheet's sections is what a search for each would have made of it.
    #[test]
    fn an_overlay_replaces_the_section_of_a_key_puts_the_others_last_and_takes_out_what_is_gone() {
        let mut sheet = Sheet { sections: sections(5, 1.0), ..Sheet::default() };
        let put = vec![sections(1, 9.0).remove(0), Arc::new(Section::new("New", None)), Arc::new(Section::new("Geometry", Some(2))), {
            let mut s = Section::new("S3", None);
            s.cells.insert(CellName::new("V"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(7.0).unwrap()));
            Arc::new(s)
        }];
        let gone = vec![
            Gone::Section(SectionKey::new("S1", None)),
            Gone::Section(SectionKey::new("S4", None)),
            Gone::Section(SectionKey::new("Nothing", None)),
        ];
        assert!(overlay(&mut sheet, Cells::new(), put, &gone).is_ok());
        let names: Vec<(&str, Option<u32>)> = sheet.sections.iter().map(|s| (s.name.as_str(), s.ix)).collect();
        assert_eq!(names, [("S0", None), ("S2", None), ("S3", None), ("New", None), ("Geometry", Some(2))]);
        let value =
            |at: usize| sheet.sections.get(at).and_then(|s| s.cells.get("V")).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number);
        assert_eq!((value(0), value(1), value(2)), (Some(9.0), Some(1.0), Some(7.0)), "S0 and S3 are replaced where they stand, S2 is as it was");
        // A key put twice: the second takes the place of the first, which was put last.
        let mut sheet = Sheet::default();
        let twice = vec![Arc::new(Section::new("A", None)), Arc::new(Section::new("A", None))];
        assert!(overlay(&mut sheet, Cells::new(), twice, &[]).is_ok());
        assert_eq!(sheet.sections.len(), 1);
        // A key that is twice in the sheet: the first is the one that is replaced, and a removal takes out both.
        let mut sheet = Sheet { sections: vec![Arc::new(Section::new("D", None)), Arc::new(Section::new("D", None))], ..Sheet::default() };
        assert!(overlay(&mut sheet, Cells::new(), vec![sections(1, 1.0).remove(0)], &[]).is_ok());
        assert_eq!(sheet.sections.len(), 3, "S0 is new");
        assert!(overlay(&mut sheet, Cells::new(), Vec::new(), &[Gone::Section(SectionKey::new("D", None))]).is_ok());
        assert_eq!(sheet.sections.len(), 1);
        // A sheet at the model's cap takes no new section, and still replaces one it has.
        let mut full = Sheet { sections: sections(diacraft_doc::MAX_SECTIONS_PER_SHEET, 1.0), ..Sheet::default() };
        assert!(overlay(&mut full, Cells::new(), sections(1, 2.0), &[]).is_ok());
        assert!(matches!(overlay(&mut full, Cells::new(), vec![Arc::new(Section::new("Over", None))], &[]), Err(OdgError::TooComplex { .. })));
    }

    /// A style is read with a section's attribute for each section a sheet may hold, and sixteen for the fixed names.
    #[test]
    fn a_style_is_read_with_an_attribute_and_a_fingerprint_for_each_section_and_thirty_two_more() {
        assert_eq!(MOST_ATTRIBUTES, 2 * 8_192 + 32);
        assert_eq!(diacraft_doc::MAX_SECTIONS_PER_SHEET, 8_192);
    }

    /// The fragments are bare: an element in no namespace, or in the empty one, is read, and an element of any other is not.
    #[test]
    fn the_fragments_vocabulary_reads_the_bare_names_and_no_namespace() {
        assert!(Bare.reads(None));
        assert!(Bare.reads(Some("")));
        assert!(!Bare.reads(Some("urn:other")));
        assert!(!Bare.reads(Some(super::names::URN)));
    }

    fn bare(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
        El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
    }

    /// A marker is an element of one of four names with an `IX` and nothing else: the three that mark a place hold nothing, and the field holds plain text.
    #[test]
    fn a_marker_is_one_of_four_names_with_an_index_and_nothing_else() {
        let text = || vec![Node::Text("abc".to_owned())];
        let other = || vec![Node::El(bare("x", &[], Vec::new()))];
        assert_eq!(marker_of(&bare("cp", &[("IX", "3")], Vec::new())), Some(TextPart::Cp(3)));
        assert_eq!(marker_of(&bare("pp", &[("IX", "4")], Vec::new())), Some(TextPart::Pp(4)));
        assert_eq!(marker_of(&bare("tp", &[("IX", "5")], Vec::new())), Some(TextPart::Tp(5)));
        assert_eq!(marker_of(&bare("fld", &[("IX", "2")], text())), Some(TextPart::Fld { ix: 2, text: Arc::from("abc") }));
        assert_eq!(marker_of(&bare("fld", &[("IX", "2")], Vec::new())), Some(TextPart::Fld { ix: 2, text: Arc::from("") }));
        // A marker of a place holds nothing, and a field holds text alone.
        for name in ["cp", "pp", "tp"] {
            assert_eq!(marker_of(&bare(name, &[("IX", "1")], text())), None, "{name} with text");
            assert_eq!(marker_of(&bare(name, &[("IX", "1")], other())), None, "{name} with an element");
        }
        assert_eq!(marker_of(&bare("fld", &[("IX", "1")], other())), None, "a field with an element");
        // Nor is any other name, any other attribute, a second attribute, or an index that is not canonical.
        assert_eq!(marker_of(&bare("zz", &[("IX", "1")], Vec::new())), None);
        for name in ["cp", "pp", "tp", "fld"] {
            assert_eq!(marker_of(&bare(name, &[("Z", "1")], Vec::new())), None, "{name} with another attribute");
            assert_eq!(marker_of(&bare(name, &[("IX", "1"), ("Z", "1")], Vec::new())), None, "{name} with two");
            assert_eq!(marker_of(&bare(name, &[("IX", "01")], Vec::new())), None, "{name} with an index that is not canonical");
            assert_eq!(marker_of(&bare(name, &[], Vec::new())), None, "{name} with none");
        }
    }

    /// An attribute value is written up to what the file's attribute holds and refused past it, in words.
    #[test]
    fn a_value_of_the_most_item_bytes_is_encoded_and_one_byte_more_is_refused() {
        let named = |n: usize| Tier2 { name: Some("a".repeat(n)), ..Tier2::default() };
        assert!(matches!(encode(&named(MOST_ITEM_BYTES)), Ok(Some(_))));
        assert!(encode(&named(MOST_ITEM_BYTES + 1)).is_err());
    }

    /// A removal list is read up to four tokens for each attribute the style may hold, each an item of the read's budget, and no more.
    #[test]
    fn a_list_of_removals_is_read_to_a_cap_and_charged_for_what_it_reads() {
        let most = 4 * MOST_ATTRIBUTES;
        let pairs = vec![(names::GONE.to_owned(), "c.A ".repeat(most + 50))];
        let limits = Limits::default();
        let (t, took) = match decode_free(&limits, u64::MAX, &pairs) {
            Ok(done) => done,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(t.gone.len(), most, "the tokens past the cap are not read");
        assert_eq!(took, most as u64, "an item for each token read");
        let few = vec![(names::GONE.to_owned(), "c.A c.B".to_owned())];
        assert_eq!(decode_free(&limits, u64::MAX, &few).map(|(t, took)| (t.gone.len(), took)).ok(), Some((2, 2)));
    }
}
