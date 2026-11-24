//! The layer a new shape is written on (`draw:layer`; plan
//!): `draw:layer` is the layer the shape is on, and `layout` only for a shape that is on none.
//!
//! * **What the reader resolves a name against** is the layer set of **the markup**: the page's own layer set, else the document's, made into the rows of the
//!   page's `Layer` section before DiaCraft's document file puts its own rows over them. A `draw:layer` that names a layer is read as the **first** row of that name in
//!   that set, and the five layers of LibreOffice are no rows (a shape on one has no `LayerMember`). The model's rows may be others than the markup's (a layer renamed
//!   by an earlier save is carried by the document file, and `styles.xml` still holds the old name; a page the save adds has the document's set in its markup and the rows
//!   the model gave it in the document file), so a name written from the model's rows can name no layer of the markup, or another row of it: the shape then lost its
//!   layer or changed it when the file was opened again, with nothing in tier 2 and no read-back to say so (the full review).
//! * **A new shape's layer** ([`PageLayers::of_shape`]): the first number of its `LayerMember` ("a shape's first layer in `LayerMember` becomes its `draw:layer`";    the layers are separated by `;`)
//!   names a row of **the model's** page, and the name is written **only when that row of the markup's set has the same name**. The row
//!   the reader then gives back is the first of that name in the markup's set ([`Layer::row`]), so that the cell is not carried as tier 2 when it is the model's, and is
//!   when it is not (another membership, a name that is the first of an earlier row). A shape with no `LayerMember`, one whose first number is no row of the model's
//!   page, one whose row's name the markup's row does not have (nor any row, for a page whose markup has no layer set), and one whose name cannot be written as an
//!   attribute are written on `layout` and carry the cell, as the shape on no layer is.
//!
//! The maps are built **only for a page that gains a shape**, and charged to the memory a save may hold before they are made ([`PageLayers::of`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Section, SectionKey, Sheet};

use super::splice::has_bad_char;
use super::{WriteError, WriteWork};
use crate::read::layers::PRODUCER_LAYERS;
use crate::read::names::Ns;

/// The layer of a shape that is on none.
pub(crate) const DEFAULT_LAYER: &str = "layout";

/// What a row of a map holds besides its name's bytes: the entry of each map it is in, the string's header and the key's.
const ROW_BYTES: u64 = 96;

/// The layer a new shape is written on, and the row the reader reads that name as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layer {
    /// The `draw:layer` written.
    pub name: String,
    /// The row of the page's `Layer` section that reads back as the shape's `LayerMember` (`None`: the reader gives the shape none).
    pub row: Option<u32>,
}

/// The rows of one `Layer` section, by row and by name.
#[derive(Clone, Debug, Default)]
struct Rows {
    /// The `Name` of each row, by the row's `IX`.
    by_ix: HashMap<u32, String>,
    /// The first row of each name that is no layer of LibreOffice's.
    first: HashMap<String, u32>,
}

/// The layers of one page: the markup's, which a name is resolved against, and the model's, which `LayerMember` numbers.
#[derive(Clone, Debug, Default)]
pub(crate) struct PageLayers {
    markup: Rows,
    model: Rows,
}

/// A layer of the model's pages that the file's markup declares nowhere: the splice adds it to the document's layer set ("Pages, layers,
/// package": "a new layer appends a `draw:layer`"), with `draw:display` from `Visible` and `Print` and `draw:protected` from `Lock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewLayer {
    pub name: String,
    pub visible: bool,
    pub print: bool,
    pub protected: bool,
}

impl NewLayer {
    /// The `draw:display` word of the layer, `None` for `always`, the default, which is not written.
    pub(crate) fn display(&self) -> Option<&'static str> {
        match (self.visible, self.print) {
            (true, true) => None,
            (true, false) => Some("screen"),
            (false, true) => Some("printer"),
            (false, false) => Some("none"),
        }
    }
}

/// The words of a save refused for the memory the layers it adds would hold.
const WORDS_HELD: &str = "memory for the layers a save adds to the layer set";

/// The layers of the model's pages (their `Layer` sections, `sections`) that the file's markup declares nowhere, each name once (the first page and row that has it decides its
/// flags), in the order the pages and rows give them: not a row with no name, a name that is empty, holds a character the attribute cannot hold, or is one of LibreOffice's five, nor a name of a layer any
/// page of the file reads (`declared`: the document's layer set, and the pages' own). Every row is a step, charged before it is looked at.
pub(crate) fn new_layers(sections: &[&Section], declared: &HashSet<String>, work: &mut WriteWork) -> Result<Vec<NewLayer>, WriteError> {
    let flag = |row: &diacraft_doc::Row, cell: &str, default: bool| {
        row.cells.get(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).map_or(default, |n| n != 0.0)
    };
    let mut out: Vec<NewLayer> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for section in sections {
        for row in section.rows.iter() {
            work.charge_steps(1)?;
            let Some(name) = row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text()) else { continue };
            if name.is_empty()
                || has_bad_char(name)
                || name.contains('\r')
                || PRODUCER_LAYERS.contains(&name)
                || declared.contains(name)
                || !seen.insert(name)
            {
                continue;
            }
            // The name is copied, and the set probed: charged by its bytes before it is kept (the security scan's MEDIUM).
            work.hold(ROW_BYTES.saturating_add(name.len() as u64), WORDS_HELD)?;
            out.try_reserve(1).map_err(|_| starved(out.len()))?;
            out.push(NewLayer {
                name: name.to_owned(),
                visible: flag(row, "Visible", true),
                print: flag(row, "Print", true),
                protected: flag(row, "Lock", false),
            });
        }
    }
    Ok(out)
}

/// A flag of a `Layer` row: a cell that holds a number is set when it is not 0, and a missing cell reads `default`.
fn flag_of(row: &diacraft_doc::Row, cell: &str, default: bool) -> bool {
    row.cells.get(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).map_or(default, |n| n != 0.0)
}

/// The `Name` of a `Layer` row, when it has one.
fn name_of(row: &diacraft_doc::Row) -> Option<&str> {
    row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text())
}

/// A layer row's `Visible`, `Print` and `Lock`.
pub(crate) type Flags = (bool, bool, bool);

/// The flags of a `Layer` row, a missing cell read as its default.
pub(crate) fn flags_of(row: &diacraft_doc::Row) -> Flags {
    (flag_of(row, "Visible", true), flag_of(row, "Print", true), flag_of(row, "Lock", false))
}

/// **Plan **: which of the rows of one layer decides its flags, `rows` being the first row of its name on each page, in the model's order, as
/// `(now, was)`: the flags the row has, and those it had when the drawing was read. **The row the person changed decides**: the last row whose flags are not what they were (of
/// two pages that both changed it, the later). `None` when no row changed: the layer set keeps what it says, and so the first page that has the layer, whose rows are the set's,
/// still decides.
pub(crate) fn deciding(rows: &[(Flags, Flags)]) -> Option<usize> {
    rows.iter().rposition(|(now, was)| now != was)
}

/// The first row of each name of `section`, each row a step.
fn first_rows<'s>(section: &'s Section, work: &mut WriteWork) -> Result<HashMap<&'s str, &'s diacraft_doc::Row>, WriteError> {
    let mut first: HashMap<&str, &diacraft_doc::Row> = HashMap::new();
    for row in section.rows.iter() {
        work.charge_steps(1)?;
        if let Some(name) = name_of(row) {
            first.entry(name).or_insert(row);
        }
    }
    Ok(first)
}

/// **The layers of a layer set whose flags the model changed** (`draw:display` and `draw:protected` are regenerated
/// from `Visible`, `Print` and `Lock`, merged by name). `pages` are the `Layer` sections of the model's pages that read the set, in the model's order, each
/// with the section the page had when the drawing was read (DiaCraft's data included; `None` for a page the save adds), and `declared` the rows the set gives a page. A name the
/// set declares is decided by [`deciding`]: among the first row of it on each page, the last whose `Visible`, `Print` or `Lock` are not what the
/// page had (a page the save adds, or one that had no row of the name, against the set's), which is in the answer with its flags when they are not what the set says. A layer no
/// page changed is not, whatever a page's rows that ride in DiaCraft's document file say. The rows of a page that then differ from the set ride in DiaCraft's document file, as
/// they did. A name the set does not declare is [`new_layers`]'s (the first page decides it). The answer is in the order the pages first change the layers. The document's set
/// is given every page that reads it, and a page's own set that page alone. Every row is a step, charged before it is looked at, and each name kept is charged by its bytes.
pub(crate) fn reflagged(
    pages: &[(&Section, Option<&Section>)],
    declared: Option<&Section>,
    work: &mut WriteWork,
) -> Result<Vec<NewLayer>, WriteError> {
    let Some(declared) = declared else { return Ok(Vec::new()) };
    let first = first_rows(declared, work)?;
    // For each name the set declares that a page changed: its place in the answer, which holds the flags of the last page that changed it.
    let mut changed: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<NewLayer> = Vec::new();
    for (section, read) in pages {
        let had = match read {
            Some(read) => first_rows(read, work)?,
            None => HashMap::new(),
        };
        // The first row of each name on this page is the page's row of the layer (the markup's first row of a name is what its `draw:layer` reads as).
        let mut on_page: HashSet<&str> = HashSet::new();
        for row in section.rows.iter() {
            work.charge_steps(1)?;
            let Some(name) = name_of(row) else { continue };
            if !on_page.insert(name) {
                continue;
            }
            let Some(set_row) = first.get(name) else { continue };
            let now = flags_of(row);
            let was = had.get(name).map_or_else(|| flags_of(set_row), |r| flags_of(r));
            if now == was {
                continue;
            }
            let layer = NewLayer { name: String::new(), visible: now.0, print: now.1, protected: now.2 };
            match changed.get(name).and_then(|at| out.get_mut(*at)) {
                // A later page that changed it too decides.
                Some(kept) => *kept = NewLayer { name: std::mem::take(&mut kept.name), ..layer },
                None => {
                    work.hold(ROW_BYTES.saturating_add(name.len() as u64), WORDS_HELD)?;
                    out.try_reserve(1).map_err(|_| starved(out.len()))?;
                    changed.insert(name, out.len());
                    out.push(NewLayer { name: name.to_owned(), ..layer });
                }
            }
        }
    }
    // A layer whose deciding row says what the set says has nothing to write.
    out.retain(|l| first.get(l.name.as_str()).is_none_or(|r| flags_of(r) != (l.visible, l.print, l.protected)));
    Ok(out)
}

/// What a page of the file whose rows are the document's set's reads after a save that declares `added` and writes the flags of `reflagged` (by
/// name): the markup's rows, the first row of each name of `reflagged` with its new `Visible`, `Print` and `Lock` (the save writes them on the first `draw:layer` of that name,
/// which is that row), and then a row for each layer added, as the reader numbers and makes them (`read::layers::layer_row`). `None` stays `None` with nothing added. One pass
/// over the rows, each a step.
fn with_added(
    section: Option<&Arc<Section>>,
    added: &[NewLayer],
    reflagged: &HashMap<&str, &NewLayer>,
    work: &mut WriteWork,
) -> Result<Option<Arc<Section>>, WriteError> {
    if added.is_empty() && reflagged.is_empty() {
        return Ok(section.cloned());
    }
    let mut out = section.map_or_else(|| Section::new("Layer", None), |s| (**s).clone());
    if !reflagged.is_empty() {
        let mut done: HashSet<&str> = HashSet::new();
        for row in out.rows.iter_mut() {
            work.charge_steps(1)?;
            let Some(layer) = name_of(row).and_then(|name| reflagged.get(name)).copied() else { continue };
            if !done.insert(layer.name.as_str()) {
                continue;
            }
            let ix = row.ix.unwrap_or(0);
            *row = Arc::new(crate::read::layers::layer_row(ix, &layer.name, layer.visible, layer.print, layer.protected));
        }
    }
    for layer in added {
        let ix = u32::try_from(out.rows.len()).unwrap_or(u32::MAX);
        out.rows.push(Arc::new(crate::read::layers::layer_row(ix, &layer.name, layer.visible, layer.print, layer.protected)));
    }
    Ok(Some(Arc::new(out)))
}

/// The native markup of the file as a save that declares `added` in the document's layer set and writes the new flags of `reflagged` on the layers it declares
/// leaves it: every page of the file that reads the document's set (`own` says which do not: a page with a set of its own), and a page the save adds, reads the
/// layers as the save leaves them; a page with a set of its own reads that set with the flags `own_flags` gives it written. DiaCraft's document file is planned
/// against this, so a page whose own rows are not those carries them, and the layer added to one page is not read on the others. The rows are charged by their bytes for each
/// page before they are made.
pub(crate) fn native_with(
    native: &crate::ext::docfile::Native,
    added: &[NewLayer],
    reflagged: &[NewLayer],
    own: impl Fn(usize) -> bool,
    own_flags: &HashMap<usize, &[NewLayer]>,
    work: &mut WriteWork,
) -> Result<crate::ext::docfile::Native, WriteError> {
    let key = SectionKey::new("Layer", None);
    let bytes = |sum: u64, l: &NewLayer| sum.saturating_add(ROW_BYTES).saturating_add(2 * l.name.len() as u64);
    let per_page = added.iter().fold(0u64, bytes).saturating_add(reflagged.iter().fold(0u64, bytes));
    let by_name: HashMap<&str, &NewLayer> = reflagged.iter().map(|l| (l.name.as_str(), l)).collect();
    let mut out = native.clone();
    for (h, page) in out.pages.iter_mut().enumerate() {
        if own(h) {
            // The page's own set, with the flags the save writes in it.
            let Some(flags) = own_flags.get(&h).filter(|f| !f.is_empty()) else { continue };
            work.hold(flags.iter().fold(0u64, bytes), WORDS_HELD)?;
            let by: HashMap<&str, &NewLayer> = flags.iter().map(|l| (l.name.as_str(), l)).collect();
            let section = with_added(page.sheet.section(&key), &[], &by, work)?;
            let sheet = Arc::make_mut(&mut page.sheet);
            sheet.sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
            if let Some(section) = section {
                sheet.sections.push(section);
            }
            continue;
        }
        work.hold(per_page, WORDS_HELD)?;
        let section = with_added(page.sheet.section(&key), added, &by_name, work)?;
        let sheet = Arc::make_mut(&mut page.sheet);
        sheet.sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
        if let Some(section) = section {
            sheet.sections.push(section);
        }
    }
    work.hold(per_page, WORDS_HELD)?;
    out.new_page_layers = with_added(native.new_page_layers.as_ref(), added, &by_name, work)?;
    Ok(out)
}

/// Adds the names of the layers of `section` (a `Layer` section the markup gives a page: what a page of the file reads) to `into`, each row a step.
pub(crate) fn names_of(section: Option<&Section>, into: &mut HashSet<String>, work: &mut WriteWork) -> Result<(), WriteError> {
    let Some(section) = section else { return Ok(()) };
    for row in section.rows.iter() {
        work.charge_steps(1)?;
        if let Some(name) = row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text())
            && !into.contains(name)
        {
            work.hold(ROW_BYTES.saturating_add(name.len() as u64), WORDS_HELD)?;
            into.insert(name.to_owned());
        }
    }
    Ok(())
}

fn starved(held: usize) -> WriteError {
    WriteError::Limit { what: "memory for the layers of a page", max: held as u64 }
}

impl Rows {
    /// The rows of `section`, each a step, the names' bytes and the maps' entries charged to the memory a save may hold **before** they are made.
    fn of(section: Option<&Section>, work: &mut WriteWork) -> Result<Rows, WriteError> {
        let mut rows = Rows::default();
        let Some(section) = section else { return Ok(rows) };
        let count = section.rows.len();
        let mut held = 0u64;
        for row in section.rows.iter() {
            let name = row.cells.get("Name").and_then(|c| c.value.as_ref()).map_or(0, |v| v.text().len());
            held = held.saturating_add(ROW_BYTES).saturating_add(2 * name as u64);
        }
        let cap = work.memory_cap;
        work.memory_room = work.memory_room.checked_sub(held).ok_or(WriteError::Limit { what: "memory for the layers of a page", max: cap })?;
        rows.by_ix.try_reserve(count).map_err(|_| starved(count))?;
        rows.first.try_reserve(count).map_err(|_| starved(count))?;
        for row in section.rows.iter() {
            work.order_steps = work.order_steps.saturating_add(1);
            let Some(ix) = row.ix else { continue };
            let Some(name) = row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text()) else { continue };
            rows.by_ix.entry(ix).or_insert_with(|| name.to_owned());
            if !PRODUCER_LAYERS.contains(&name) {
                rows.first.entry(name.to_owned()).or_insert(ix);
            }
        }
        Ok(rows)
    }
}

impl PageLayers {
    /// The layers of a page that gains a shape: `markup` is the `Layer` section the file's own markup gives it (`None` for a layer set with no row), `model` the sheet of
    /// the page as the model has it, and `added` the layers the save adds to the document's layer set when the page reads that set (none for a page with a set of
    /// its own): they are the rows after the markup's, in their order, as the reader numbers them, and a new shape on one of them is written on it.
    pub(crate) fn of(markup: Option<&Section>, model: &Sheet, added: &[NewLayer], work: &mut WriteWork) -> Result<PageLayers, WriteError> {
        let model_rows = model.section(&SectionKey::new("Layer", None)).map(|s| &**s);
        let count = markup.map_or(0, |section| section.rows.len());
        let mut markup = Rows::of(markup, work)?;
        if !added.is_empty() {
            let held = added.iter().fold(0u64, |sum, l| sum.saturating_add(ROW_BYTES).saturating_add(2 * l.name.len() as u64));
            let cap = work.memory_cap;
            work.memory_room = work.memory_room.checked_sub(held).ok_or(WriteError::Limit { what: "memory for the layers of a page", max: cap })?;
            // The reader numbers the rows of a set from 0, in order, LibreOffice's five left out: the markup's rows are 0 to its count less one.
            let mut next = u32::try_from(count).unwrap_or(u32::MAX);
            for layer in added {
                work.order_steps = work.order_steps.saturating_add(1);
                markup.by_ix.entry(next).or_insert_with(|| layer.name.clone());
                markup.first.entry(layer.name.clone()).or_insert(next);
                next = next.saturating_add(1);
            }
        }
        Ok(PageLayers { markup, model: Rows::of(model_rows, work)? })
    }

    /// The row of the page's `Layer` section that the reader gives the layer `name` as the file wrote it: the markup's first row of that name (none for a layer of LibreOffice's, which no row
    /// stands for). **A shape of the file keeps the layer it named**, and the cell it is read back with is this row.
    pub(crate) fn row_named(&self, name: &str) -> Option<u32> {
        self.markup.first.get(name).copied()
    }

    /// The layer the new shape whose own sheet is `sheet` is written on (see the module documentation).
    pub(crate) fn of_shape(&self, sheet: &Sheet) -> Layer {
        let layout = Layer { name: DEFAULT_LAYER.to_owned(), row: None };
        let Some(text) = sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text()) else { return layout };
        let Some(ix) = text.split(';').next().and_then(|first| first.trim().parse::<u32>().ok()) else { return layout };
        let Some(name) = self.model.by_ix.get(&ix).filter(|n| !n.is_empty() && !has_bad_char(n) && !n.contains('\r')) else { return layout };
        // The reader resolves the name against the markup's set: a row of it that has another name (or none) is not the layer the model means.
        if self.markup.by_ix.get(&ix) != Some(name) {
            return layout;
        }
        Layer { name: name.clone(), row: self.markup.first.get(name).copied() }
    }
}

/// The markup of the layers `layers` that the save declares, between `head` and `tail` (what opens and closes the set or the element round them), each a `draw:layer` under
/// the prefix `draw` with `on_each` (a declaration of it, or nothing) on its tag; each attribute is charged as the splice charges one.
///
/// **What it costs** (the security scan's MEDIUM): the bytes of the whole text, every name as it is escaped (a `"` is six bytes written, an `&` five) and the fixed
/// markup of each layer, are added up and charged to the memory a save may hold, and to the bytes it writes, **before a byte of it is made**; the text is made in one buffer
/// of exactly that size, and a name is escaped into it a character at a time (no copy of it is made beside it).
fn layers_text(
    draw: &str,
    on_each: &str,
    (head, tail): (&str, &str),
    layers: &[NewLayer],
    work: &mut WriteWork,
    limits: &crate::limits::Limits,
) -> Result<String, WriteError> {
    let len = |s: &str| s.len() as u64;
    // `<draw:layer` and the declaration, ` draw:name="`, the closing quote and `/>`.
    let each = 1 + len(draw) + len(":layer") + len(on_each) + 1 + len(draw) + len(":name=\"") + 1 + 2;
    let mut total = len(head).saturating_add(len(tail));
    for layer in layers {
        work.charge_steps(1)?;
        total = total.saturating_add(each).saturating_add(super::splice::escaped_len(&layer.name, b'"'));
        if let Some(display) = layer.display() {
            total = total.saturating_add(1 + len(draw) + len(":display=\"") + len(display) + 1);
        }
        if layer.protected {
            total = total.saturating_add(1 + len(draw) + len(":protected=\"true\""));
        }
    }
    work.hold(total, WORDS_HELD)?;
    let bytes = usize::try_from(total).map_err(|_| WriteError::Limit { what: WORDS_HELD, max: work.memory_cap })?;
    work.charge_copy(bytes, limits)?;
    let mut text = String::new();
    text.try_reserve_exact(bytes).map_err(|_| WriteError::Limit { what: WORDS_HELD, max: work.memory_cap })?;
    text.push_str(head);
    for layer in layers {
        work.charge_attribute(limits)?;
        for piece in ["<", draw, ":layer", on_each, " ", draw, ":name=\""] {
            text.push_str(piece);
        }
        for c in layer.name.chars() {
            match super::splice::escaped(c, b'"') {
                Some(escaped) => text.push_str(escaped),
                None => text.push(c),
            }
        }
        text.push('"');
        if let Some(display) = layer.display() {
            work.charge_attribute(limits)?;
            for piece in [" ", draw, ":display=\"", display, "\""] {
                text.push_str(piece);
            }
        }
        if layer.protected {
            work.charge_attribute(limits)?;
            for piece in [" ", draw, ":protected=\"true\""] {
                text.push_str(piece);
            }
        }
        text.push_str("/>");
    }
    text.push_str(tail);
    Ok(text)
}

impl super::frame::Editor<'_, '_> {
    /// Writes the new flags of `layers`, layers the document's layer set declares whose `Visible`, `Print` or `Lock` the model changed (the
    /// layer's `draw:display` and `draw:protected` regenerated), on the first `draw:layer` of each name among `tags` (the set's layers the reader makes rows of, in order): an
    /// attribute that is there is written in its place, and one that is not is added only when it says other than its default (`always`, `false`). An attribute that already
    /// says what the flags are keeps its bytes. Each write is charged as the splice charges one; a name the set does not have is passed over.
    pub(crate) fn reflag_layers(&mut self, tags: &[super::scan::LayerTag], layers: &[NewLayer]) -> Result<(), WriteError> {
        if layers.is_empty() {
            return Ok(());
        }
        let mut first: HashMap<&str, &super::scan::LayerTag> = HashMap::new();
        for tag in tags {
            self.work.charge_steps(1)?;
            first.entry(tag.name.as_str()).or_insert(tag);
        }
        for layer in layers {
            self.work.charge_steps(1)?;
            let Some(tag) = first.get(layer.name.as_str()) else { continue };
            let bytes = self.bytes;
            let mut edit = super::attrs::TagEdit::untracked(bytes, tag.open, tag.scope, self.work)?;
            let word = layer.display().unwrap_or("always");
            match edit.find(&mut self.scopes, Ns::Draw, "display", self.work) {
                // What the word says, as the reader reads it (one rule for both).
                Some(at) if crate::read::layers::display_flags(&edit.read_text(at)) == (layer.visible, layer.print) => {}
                Some(at) => {
                    self.work.charge_attribute(self.limits)?;
                    edit.set(&mut self.splicer, at, word)?;
                }
                None if layer.display().is_some() => {
                    self.work.charge_attribute(self.limits)?;
                    edit.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "display", word)?;
                }
                None => {}
            }
            let protected = if layer.protected { "true" } else { "false" };
            match edit.find(&mut self.scopes, Ns::Draw, "protected", self.work) {
                Some(at) if crate::read::units::boolean(&edit.read_text(at)).unwrap_or(false) == layer.protected => {}
                Some(at) => {
                    self.work.charge_attribute(self.limits)?;
                    edit.set(&mut self.splicer, at, protected)?;
                }
                None if layer.protected => {
                    self.work.charge_attribute(self.limits)?;
                    edit.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "protected", protected)?;
                }
                None => {}
            }
        }
        Ok(())
    }

    /// Declares the layers `layers` that the save adds ("a new layer appends a `draw:layer`"): at the end of the
    /// document's layer set (`set`, the first `draw:layer-set` of `office:master-styles`), or in a layer set made the first child of `office:master-styles` (`masters`) for a part
    /// that has none, as the model writer gives one (`model/layer_set.rs`). Each is written under the prefix the place binds to ODF's drawing namespace, or under one of its own
    /// declared on it. Each attribute is charged as the splice charges one. A part with no `office:master-styles` is refused in words: the layer would be lost to another program.
    pub(crate) fn put_layers(
        &mut self,
        masters: Option<&super::scan::CommonStyles>,
        set: Option<&super::scan::CommonStyles>,
        layers: &[NewLayer],
    ) -> Result<(), WriteError> {
        if layers.is_empty() {
            return Ok(());
        }
        let Some(scope) = set.or(masters).map(|held| held.scope) else {
            return Err(WriteError::Unsupported("a new layer in a styles.xml with no office:master-styles to declare it in"));
        };
        let (draw, declare) = match self.scopes.prefix_for(scope, super::scan::DRAW_URI, self.work) {
            Some(prefix) => (prefix, String::new()),
            None => {
                let fresh = self.scopes.fresh_prefix("draw", self.work);
                let declare = format!(" xmlns:{fresh}=\"{}\"", super::scan::DRAW_URI);
                (fresh, declare)
            }
        };
        // The declaration goes on each new layer of the file's set, whose own tag is not written, and on the set when the save makes one.
        let on_each = if set.is_some() { declare.as_str() } else { "" };
        let made_set = (format!("<{draw}:layer-set{declare}>"), format!("</{draw}:layer-set>"));
        match (set, masters) {
            // An empty element (`<draw:layer-set/>`, whose last two bytes are `/>`) is written whole with the layers in it.
            (Some(set), _) if set.empty => {
                let text = layers_text(&draw, on_each, (">", &format!("</{}>", set.qname)), layers, self.work, self.limits)?;
                self.splicer.replace(set.open.1.saturating_sub(2), set.open.1, text);
            }
            (Some(set), _) => {
                let text = layers_text(&draw, on_each, ("", ""), layers, self.work, self.limits)?;
                self.splicer.insert(set.close_start, text);
            }
            (None, Some(masters)) if masters.empty => {
                let (head, tail) = (format!(">{}", made_set.0), format!("{}</{}>", made_set.1, masters.qname));
                let text = layers_text(&draw, on_each, (&head, &tail), layers, self.work, self.limits)?;
                self.splicer.replace(masters.open.1.saturating_sub(2), masters.open.1, text);
            }
            (None, Some(masters)) => {
                let text = layers_text(&draw, on_each, (&made_set.0, &made_set.1), layers, self.work, self.limits)?;
                self.splicer.insert(masters.open.1, text);
            }
            (None, None) => return Err(WriteError::Unsupported("a new layer in a styles.xml with no office:master-styles to declare it in")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Row, Value};

    use super::*;

    fn section(names: &[&str]) -> Section {
        let mut section = Section::new("Layer", None);
        for (ix, name) in names.iter().enumerate() {
            let mut row = Row { ix: Some(u32::try_from(ix).unwrap_or(u32::MAX)), ..Row::default() };
            row.cells.insert(CellName::new("Name"), Cell::constant(Value::from_text(name)));
            section.rows.push(Arc::new(row));
        }
        section
    }

    fn sheet(names: &[&str], member: Option<&str>) -> Sheet {
        let mut sheet = Sheet::default();
        if !names.is_empty() {
            sheet.sections.push(Arc::new(section(names)));
        }
        if let Some(member) = member {
            sheet.cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(member)));
        }
        sheet
    }

    /// The layer of a shape on `member`, on a page whose markup has the rows `markup` and whose model has `model`; and the steps the maps took.
    fn of(markup: &[&str], model: &[&str], member: Option<&str>) -> (Layer, u64) {
        let mut work = WriteWork { memory_room: u64::MAX, memory_cap: u64::MAX, ..WriteWork::default() };
        let markup = (!markup.is_empty()).then(|| section(markup));
        let layers = PageLayers::of(markup.as_ref(), &sheet(model, None), &[], &mut work).unwrap();
        (layers.of_shape(&sheet(&[], member)), work.order_steps)
    }

    fn on(name: &str, row: Option<u32>) -> Layer {
        Layer { name: name.to_owned(), row }
    }

    #[test]
    fn the_first_number_of_the_members_names_a_row_of_the_model_and_the_name_is_written_where_the_markup_has_it() {
        let page = ["Notes", "Other", "Notes"];
        assert_eq!(of(&page, &page, Some("0")).0, on("Notes", Some(0)));
        assert_eq!(of(&page, &page, Some("1")).0, on("Other", Some(1)));
        assert_eq!(of(&page, &page, Some(" 1 ;0")).0, on("Other", Some(1)), "the first of two, with its white space");
        // A row that shares its name with an earlier one is read back as that one.
        assert_eq!(of(&page, &page, Some("2")).0, on("Notes", Some(0)));
    }

    /// The markup says `Notes` and `Other`, and the model's rows are others: the name is written only where the markup's row has the model's name.
    #[test]
    fn a_name_the_markups_row_does_not_have_is_not_written_whatever_the_model_s_row_says() {
        let markup = ["Notes", "Other"];
        // Row 0 renamed in the model.
        assert_eq!(of(&markup, &["Comments", "Other"], Some("0")).0, on(DEFAULT_LAYER, None));
        assert_eq!(of(&markup, &["Comments", "Other"], Some("1")).0, on("Other", Some(1)), "a row nothing changed");
        // The rows in another order: `Other` is the markup's row 1 and the model's row 0.
        assert_eq!(of(&markup, &["Other", "Notes"], Some("0")).0, on(DEFAULT_LAYER, None));
        assert_eq!(of(&markup, &["Other", "Notes"], Some("1")).0, on(DEFAULT_LAYER, None));
        // A page whose model has more rows than the markup.
        assert_eq!(of(&markup, &["Notes", "Other", "Third"], Some("2")).0, on(DEFAULT_LAYER, None));
        // A page whose markup has no layer set with a row: nothing is resolved.
        assert_eq!(of(&[], &["Notes"], Some("0")).0, on(DEFAULT_LAYER, None));
        // The model's row is the markup's: a row the markup has once.
        assert_eq!(of(&markup, &markup, Some("0")).0, on("Notes", Some(0)));
    }

    /// H02: the row the reader gives a name back as is the **markup's** first row of it, not the model's. The markup has `Notes` twice and the model's row 0 is renamed, so the
    /// model's first row of `Notes` is row 1 and the markup's is row 0: a shape on the model's row 1 is written under `Notes` and reads back as row 0 (and so is carried).
    #[test]
    fn the_row_a_name_reads_back_as_is_the_first_of_the_markups_rows_and_not_the_models() {
        assert_eq!(of(&["Notes", "Notes"], &["Comments", "Notes"], Some("1")).0, on("Notes", Some(0)));
        // The model's rows the other way round: its first `Notes` is row 0 and the markup's is row 1's name too; the shape on the model's row 1 is the markup's row 1.
        assert_eq!(of(&["Other", "Notes", "Notes"], &["Notes", "Other", "Notes"], Some("2")).0, on("Notes", Some(1)));
        assert_eq!(of(&["Notes", "Notes"], &["Notes", "Notes"], Some("1")).0, on("Notes", Some(0)));
    }

    #[test]
    fn a_shape_on_no_layer_or_on_one_the_page_has_not_or_one_that_cannot_be_written_is_on_layout() {
        let page = ["Notes", "", "a\u{1}b", "c\rd"];
        for member in [None, Some(""), Some("x"), Some(";0"), Some("-1"), Some("4"), Some("4294967296"), Some("1"), Some("2"), Some("3")] {
            assert_eq!(of(&page, &page, member).0, on(DEFAULT_LAYER, None), "{member:?}");
        }
        // No `Layer` section at all.
        assert_eq!(of(&[], &[], Some("0")).0, on(DEFAULT_LAYER, None));
    }

    /// A layer of LibreOffice's is no row of the markup (the reader makes none), so a name of one is never the markup's row's name for a model row of another
    /// name; where a markup (another program's) does hold one, the name is written and no row is read back.
    #[test]
    fn a_layer_named_as_one_of_libreoffice_s_has_a_name_and_no_row_to_be_read_back_as() {
        for producer in PRODUCER_LAYERS {
            assert_eq!(of(&["Notes"], &[producer], Some("0")).0, on(DEFAULT_LAYER, None), "{producer}");
            assert_eq!(of(&[producer], &[producer], Some("0")).0, on(producer, None), "{producer}");
        }
    }

    #[test]
    fn the_rows_of_the_markup_and_of_the_model_are_the_steps_of_the_maps() {
        assert_eq!(of(&["a", "b", "c", "d"], &["a", "b"], None).1, 6);
        assert_eq!(of(&[], &[], None).1, 0);
    }

    /// A page of the model whose `Layer` rows are named `names`, as a document of one page.
    fn doc_of(names: &[String]) -> diacraft_doc::Document {
        let mut section = Section::new("Layer", None);
        for (ix, name) in names.iter().enumerate() {
            let mut row = Row { ix: Some(u32::try_from(ix).unwrap_or(u32::MAX)), ..Row::default() };
            row.cells.insert(CellName::new("Name"), Cell::constant(Value::from_text(name)));
            section.rows.push(Arc::new(row));
        }
        let mut doc = diacraft_doc::Document::new_blank();
        if let Some(page) = doc.pages.get_mut(0) {
            Arc::make_mut(&mut Arc::make_mut(page).sheet).sections.push(Arc::new(section));
        }
        doc
    }

    /// The security scan's MEDIUM: the names a save keeps of the layers it adds, and the markup it makes of them, are charged to the memory a save may hold
    /// **before** they are kept or made, by their bytes and not only by a count: a name of 64 MB of characters that grow when escaped (`"` to `&quot;`, `&` to `&amp;`) and
    /// 100,000 layers are each refused in words when the room is short, and with room, what is held (the names' bytes, the text's capacity) is at or under the charge.
    #[test]
    fn the_layers_a_save_adds_are_charged_by_their_bytes_before_they_are_kept_or_written() {
        let huge: String = "\"&".repeat(32 * 1024 * 1024);
        let many: Vec<String> = (0..100_000u32).map(|n| format!("L{n}")).collect();
        let limits = crate::limits::Limits::default();
        for (what, names) in [("a name of 64 MB", vec![huge.clone()]), ("100,000 layers", many.clone())] {
            let doc = doc_of(&names);
            let kept_bytes: u64 = names.iter().map(|n| n.len() as u64).sum();
            // Short of room: refused in words, and nothing is kept.
            let mut work = WriteWork { memory_room: kept_bytes / 2, memory_cap: kept_bytes / 2, step_cap: u64::MAX, ..WriteWork::default() };
            let sections: Vec<&Section> = doc.pages.iter().filter_map(|p| p.sheet.section(&SectionKey::new("Layer", None)).map(|s| &**s)).collect();
            match new_layers(&sections, &HashSet::new(), &mut work) {
                Err(WriteError::Limit { what: words, .. }) => assert!(words.contains("layers"), "{what}: {words}"),
                other => panic!("{what}: the names were kept without the room for them: {:?}", other.map(|l| l.len())),
            }
            // With room: the names are kept, and charged at least their bytes.
            let room = 4 * 1024 * 1024 * 1024u64;
            let mut work = WriteWork { memory_room: room, memory_cap: room, step_cap: u64::MAX, ..WriteWork::default() };
            let added = new_layers(&sections, &HashSet::new(), &mut work).unwrap();
            assert_eq!(added.len(), names.len(), "{what}");
            let charged = room - work.memory_room;
            assert!(charged >= kept_bytes, "{what}: {kept_bytes} bytes of names kept for a charge of {charged}");
            // The markup: its bytes are what is charged, and the text holds no more.
            let escaped: u64 = names.iter().map(|n| super::super::splice::escaped_len(n, b'"')).sum();
            let mut short =
                WriteWork { memory_room: escaped / 2, memory_cap: escaped / 2, step_cap: u64::MAX, byte_cap: u64::MAX, ..WriteWork::default() };
            let big = crate::limits::Limits { max_write_attributes: usize::MAX, ..limits.clone() };
            match layers_text("draw", "", ("", ""), &added, &mut short, &big) {
                Err(WriteError::Limit { what: words, .. }) => assert!(words.contains("layers"), "{what}: {words}"),
                other => panic!("{what}: the markup was made without the room for it: {:?}", other.map(|t| t.len())),
            }
            let mut work = WriteWork { memory_room: room, memory_cap: room, step_cap: u64::MAX, byte_cap: u64::MAX, ..WriteWork::default() };
            let text = layers_text("draw", "", ("", ""), &added, &mut work, &big).unwrap();
            let charged = room - work.memory_room;
            assert!(text.capacity() as u64 <= charged, "{what}: a text of {} bytes held for a charge of {charged}", text.capacity());
            assert!(text.len() as u64 > escaped, "{what}: the names are in it, escaped");
        }
    }

    /// The row that decides is the last that changed, and none when none did; checked against the plain reading over every list of up to four rows of two
    /// kinds of flags.
    #[test]
    fn the_row_that_decides_is_the_last_that_changed() {
        let kinds: [Flags; 2] = [(true, true, false), (false, true, true)];
        for n in 0..=4u32 {
            for bits in 0..(1u32 << (2 * n)) {
                let pick = |at: u32| kinds[((bits >> at) & 1) as usize];
                let rows: Vec<(Flags, Flags)> = (0..n).map(|i| (pick(2 * i), pick(2 * i + 1))).collect();
                let plain = (0..rows.len()).rev().find(|i| rows[*i].0 != rows[*i].1);
                assert_eq!(deciding(&rows), plain, "{rows:?}");
            }
        }
        assert_eq!(deciding(&[]), None);
    }

    /// A `Layer` section of rows `(name, visible, print, lock)`.
    fn flagged(rows: &[(&str, bool, bool, bool)]) -> Section {
        let mut section = Section::new("Layer", None);
        for (ix, (name, visible, print, lock)) in rows.iter().enumerate() {
            section.rows.push(Arc::new(crate::read::layers::layer_row(u32::try_from(ix).unwrap(), name, *visible, *print, *lock)));
        }
        section
    }

    /// In the splice writer: a row is changed when it is not what its page had when the drawing was read (a page's rows that ride in DiaCraft's document file are no
    /// change); the last page that changed a layer decides, and a layer whose deciding row says what the set says is not written.
    #[test]
    fn a_layers_flags_are_the_last_changed_rows_and_a_carried_row_is_no_change() {
        let mut work = WriteWork { memory_room: u64::MAX, memory_cap: u64::MAX, step_cap: u64::MAX, ..WriteWork::default() };
        let set = flagged(&[("Notes", false, true, false), ("Pins", true, true, false)]);
        let as_read = set.clone();
        let names = |out: Vec<NewLayer>| out.into_iter().map(|l| (l.name, l.visible, l.print, l.protected)).collect::<Vec<_>>();
        // Page 1 as read, page 2 shows Notes: page 2's.
        let shown = flagged(&[("Notes", true, true, false), ("Pins", true, true, false)]);
        let out = reflagged(&[(&set, Some(&as_read)), (&shown, Some(&as_read))], Some(&set), &mut work).unwrap();
        assert_eq!(names(out), [("Notes".to_owned(), true, true, false)]);
        // Both changed Pins: the later page's.
        let locked = flagged(&[("Notes", false, true, false), ("Pins", true, true, true)]);
        let hidden = flagged(&[("Notes", false, true, false), ("Pins", false, true, false)]);
        let out = reflagged(&[(&locked, Some(&as_read)), (&hidden, Some(&as_read))], Some(&set), &mut work).unwrap();
        assert_eq!(names(out), [("Pins".to_owned(), false, true, false)]);
        // Page 2 carries rows of its own that it had when read: no change, nothing written.
        let out = reflagged(&[(&set, Some(&as_read)), (&shown, Some(&shown))], Some(&set), &mut work).unwrap();
        assert!(out.is_empty(), "{:?}", names(out));
        // A page that changed back to what the set says: nothing to write.
        let out = reflagged(&[(&set, Some(&shown))], Some(&set), &mut work).unwrap();
        assert!(out.is_empty(), "{:?}", names(out));
        // A page the save adds is judged against the set.
        let out = reflagged(&[(&set, Some(&as_read)), (&shown, None)], Some(&set), &mut work).unwrap();
        assert_eq!(names(out), [("Notes".to_owned(), true, true, false)]);
    }

    #[test]
    fn the_maps_are_charged_to_the_memory_a_save_may_hold_before_they_are_made() {
        let markup = section(&["Notes", "Other"]);
        let model = sheet(&["Notes", "Other"], None);
        let held = 2 * (ROW_BYTES * 2 + 2 * (5 + 5));
        let mut work = WriteWork { memory_room: held, memory_cap: held, ..WriteWork::default() };
        assert!(PageLayers::of(Some(&markup), &model, &[], &mut work).is_ok());
        assert_eq!((work.memory_room, work.order_steps), (0, 4), "the room is spent, one step for each row of each");
        // One byte less is refused, and nothing is made (no step is taken for the second map).
        let mut work = WriteWork { memory_room: held - 1, memory_cap: held, ..WriteWork::default() };
        assert!(matches!(PageLayers::of(Some(&markup), &model, &[], &mut work), Err(WriteError::Limit { .. })));
        assert_eq!(work.order_steps, 2, "the markup's map was made, and the model's was refused before it was");
    }
}
