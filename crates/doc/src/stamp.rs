//! Stamps: a fingerprint of the cells a held value stands beside (the design's §3.3).
//!
//! **What a stamp is for.** A held value keeps a construct exactly (an ODF hatch, a marker's path) beside the stand-in cells that can only approximate
//! it, and records the stamp of those cells as the reader set them. A writer of the held value's own format uses the value **only while the stamp of
//! the cells is still the one recorded**; once any of them changed it writes from the cells, as for a new shape (the cells win, §3.1). An undo that puts
//! the cells back makes the value good again. A stamp says whether cells changed, no more: it is no integrity check and no secret.
//!
//! **The hash** is the one `odg`'s tier 3 uses (plan ruling NP3): FNV-1a over 64 bits, offset basis `0xcbf29ce484222325`, prime `0x100000001b3`,
//! written by hand so that a stamp stored in a file means the same in every build (`std`'s hashers promise no algorithm). Every field (but the pieces
//! of `text-row`, below) is prefixed by its length as 8 little-endian bytes, and the first field is the name of what is hashed (the domain). Its text is 16
//! lowercase hexadecimal digits.
//!
//! **What is hashed.** Always as a sequence of length-prefixed fields, except `text-row` (below), whose pieces after the domain carry no length; an
//! optional field is a presence byte (`0` absent, `1` present) and then the field, so that absent and empty differ; a count is its 8 little-endian bytes
//! as one field; a flag is one byte, `0` or `1`.
//!
//! - A **cell**, as the splice writer's `own::cell_same` compares one: **a formula by its
//!   text only, a constant by its value in internal units, and neither by a cached result, an error or a unit** (ODF has no place for the unit a value
//!   shows in, so a respelling of an equal value is no edit, and a resize, which changes every cached value computed from the frame, leaves a formula of
//!   the frame as it was). A presence byte for the cell itself (a name no cell holds is absent); `F` as a tag byte, and then by the tag: `0` no formula and
//!   `3` `No Formula`, the value as an option (`0` none, `1` a number and the 8 little-endian bytes of the `f64`, a zero with no sign, `2` anything else
//!   and its text, so that `1` and `1.0` are one value); `1` a formula of its own, its text; `2` `Inh`, nothing (the formula is inherited and `V` is its
//!   cached result). Then, as `cell_same` compares them ("and either by the attributes and content the cell keeps beside them"), **the cell's other
//!   attributes** (their number, then each name and value in order) and **what it keeps inside it**: a presence byte, and if present its text as an option,
//!   its child nodes (their number, then each one's `after` as 4 little-endian bytes and the node: a tag byte, `0` an element, its name, attributes, the
//!   number of its children and then the children in order, `1` text, `2` a comment, `3` an instruction, each with its texts) and its extension slot (a
//!   presence byte, then the ODF side, the VSDX elements by name and the native side, each a set of attributes and nodes). A node is walked with no
//!   recursion, one unit of work for each ([`crate::work::stamp_items`]).
//! - [`stamp_all`], domain `all`: one stamp for several, the count and then the 16 digits of each in the order given, so that a value that rests on cells and on sections (a fill's
//!   style, which rests on its pattern cells and the rows of `FillGradient`) has one stamp.
//! - [`stamp_cells`], domain `cells`: the number of distinct names, then for each name, sorted, the name and its cell. The order the names are given
//!   in, and a name given twice, do not change the stamp. It reads only the cells named, however many the sheet holds.
//! - [`stamp_text_group`], domain `text-group`: **the one stamp that is not `own::cell_same`'s.** A text style's groups are compared by the splice writer's own
//!   signatures (`write/runs.rs`, `Part::sig`: a cell is its value's text and its unit, as written, and `1` and `1.0` are two values), which the model path must
//!   follow to keep and drop a held style value on the same edits. The group's name, a presence byte (a group is present when its first cell is, or for the
//!   underline and the line-through when either cell is, and an absent group stamps alike in every row: it is the name alone, a shorter stream than any
//!   present group's), then each cell of the group as a signature: a
//!   presence byte, then a tag byte for the formula (`0` none or `No Formula`, which the splice writer does not tell apart, `1` its text follows, `2` `Inh`)
//!   and the value and the unit each as an option of its text. `Style` is read by bit: the `Style` group is its number with bit 4 cleared (4 bytes, little-endian,
//!   and the unit), and the underline is bit 4 of `Style` if it is there (as 0 or 4, a field of its own) and the signature of `DblUnderline`, so a bold or an italic edit
//!   leaves a held underline as it is. A `Style` that is no whole number of 0 to 63 is stamped by its text, as no splice writer reads it.
//! - [`stamp_section`], domain `section`: the name asked for, the `IX` asked for as an optional 4-byte number, and if there is a first section of that name and
//!   `IX` that section as [`stamp_sections`] feeds it (none is the shorter stream). So the route of a connector is the stamp of `Geometry` with `IX` 0, and a change to
//!   another `Geometry` section leaves it as it was.
//! - [`stamp_rows_named`], domain `rows`: the section's name and the prefix, then each row, in the sheet's order, of each section of that name whose `N`
//!   starts with the prefix, as a row of a section is below, and the number of such rows. It is the stamp of the `ODFEq<i>` rows of the `User` section,
//!   which the held geometry rests on, apart from the `ODFMod<n>` rows beside them. **The match is by prefix**: a row called
//!   `ODFEqual` would match `ODFEq` too. The `.odg` reader names the equation rows `ODFEq` and a number, and the spec does not say whether a user's own row
//!   of that start is in the stamp (a question); it is, here, and a stamp that is stale for one extra row only drops a held value.
//! - [`stamp_text_row`], domain `text-row`: **the key rule of the splice writer, not `cell_same`'s.** A paragraph's or a run's row is compared by its key
//!   (`write/runs.rs`, `Props::key`): `name=text|unit;` for each cell in the cells' own order, the value's text and the unit verbatim (`0.2` and `0.20` are two
//!   values), `name=?;` for a cell it cannot read as a value (a formula, or no value). The stamp is the FNV-1a of those bytes after the domain, hashed piece by
//!   piece and with no length before a piece, so that it says what the key says and costs no text; a row with no cell stamps alike with every row with none.
//! - [`stamp_row`], domain `row`: the `Del` flag, `T` as an optional text, the number of cells and each cell (name, then the cell) in the cells' own
//!   order. Where the row sits (its `IX` and `N`) is not part of it: the held value is on the row and moves with it.
//! - [`stamp_sections`], domain `sections`: the number of distinct names, each name sorted; then each section of the sheet **in the sheet's own order**
//!   whose name is among them: **what the splice writer's `section_same` compares and no more** (the model's extension slots are no part of it). Its name,
//!   `IX` as an optional 4-byte little-endian number, the `Del` flag, its other attributes and kept nodes, the number of cells directly under it and each cell,
//!   the number of rows and each row: `IX`, `N` and `LocalName` as optional values, `T`, `Del`, the row's other attributes and kept nodes, the number of cells
//!   and each cell. A row's key is part of it here, because in a section a key says what the row is.
//!
//! **Cost.** One look at each name, one at each section, one at each row of a section that matches (`stamp_rows_named`: each row of a section of that
//! name, fed or not) and each cell of the rows fed: never a pass over a sheet for each name (counted by [`crate::work::stamp_items`]).

use crate::cell::{Cell, CellExtra, FormulaSlot};
use crate::odf::TextGroup;
use crate::package::{Ext, Kept};
use crate::raw::{Passthrough, RawAttrs, RawNode};
use crate::sheet::{Cells, Row, Section, Sheet};

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;
/// The number of hexadecimal digits of a stamp's text.
const DIGITS: usize = 16;

/// A stamp: 64 bits. A reader of a file fills the stamp of each held value, and a writer compares it with the stamp of the cells now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp(pub u64);

impl Stamp {
    /// The 16 lowercase hexadecimal digits a file holds.
    pub fn text(self) -> String {
        format!("{:016x}", self.0)
    }

    /// The stamp a text holds: exactly 16 lowercase hexadecimal digits, and nothing else (an upper-case digit, a sign or a space is no stamp).
    pub fn parse(text: &str) -> Option<Stamp> {
        if text.len() != DIGITS || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return None;
        }
        u64::from_str_radix(text, 16).ok().map(Stamp)
    }
}

/// FNV-1a over 64 bits, fed whole fields.
#[derive(Clone, Debug)]
pub struct Digest(u64);

impl Digest {
    /// A digest of the thing called `domain`: the name is the first field.
    pub fn new(domain: &str) -> Digest {
        let mut digest = Digest(OFFSET);
        digest.text(domain);
        digest
    }

    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(PRIME);
    }

    /// A field: its length as 8 little-endian bytes, then its bytes.
    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Digest {
        for b in u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes() {
            self.byte(b);
        }
        for b in bytes {
            self.byte(*b);
        }
        self
    }

    /// A field given in pieces: `len`, the sum of the pieces' lengths, as the field's length, then the bytes of the pieces one after another. It feeds what [`Digest::bytes`]
    /// feeds for the pieces joined, and joins nothing: a part's stamp is taken over what its bytes would be with a spelling put as another, without the copy. A `len` that is
    /// not the sum is the caller's mistake, and the stamp is then of no field.
    pub fn bytes_in_pieces<'a>(&mut self, len: u64, pieces: impl IntoIterator<Item = &'a [u8]>) -> &mut Digest {
        for b in len.to_le_bytes() {
            self.byte(b);
        }
        for piece in pieces {
            self.raw(piece);
        }
        self
    }

    /// Bytes with no length before them: pieces of one text, which the caller writes out in order (`stamp_text_row`).
    fn raw(&mut self, bytes: &[u8]) -> &mut Digest {
        for b in bytes {
            self.byte(*b);
        }
        self
    }

    /// A field of text.
    pub fn text(&mut self, text: &str) -> &mut Digest {
        self.bytes(text.as_bytes())
    }

    /// The stamp of what was fed so far.
    pub fn finish(&self) -> Stamp {
        Stamp(self.0)
    }
}

// ------------------------------------------------------------------------------------------------------------------ the fields

fn count(digest: &mut Digest, n: usize) {
    digest.bytes(&u64::try_from(n).unwrap_or(u64::MAX).to_le_bytes());
}

fn flag(digest: &mut Digest, on: bool) {
    digest.bytes(&[u8::from(on)]);
}

fn optional_text(digest: &mut Digest, text: Option<&str>) {
    flag(digest, text.is_some());
    if let Some(text) = text {
        digest.text(text);
    }
}

fn optional_number(digest: &mut Digest, n: Option<u32>) {
    flag(digest, n.is_some());
    if let Some(n) = n {
        digest.bytes(&n.to_le_bytes());
    }
}

/// A cell, or the absence of one.
fn feed_cell(digest: &mut Digest, cell: Option<&Cell>) {
    flag(digest, cell.is_some());
    let Some(cell) = cell else { return };
    let value = cell.value.as_ref();
    match &cell.formula {
        FormulaSlot::Absent => {
            digest.bytes(&[0]);
            feed_value(digest, value);
        }
        FormulaSlot::Local(formula) => {
            digest.bytes(&[1]).text(&formula.source);
        }
        FormulaSlot::Inherit => {
            digest.bytes(&[2]);
        }
        FormulaSlot::NoFormula => {
            digest.bytes(&[3]);
            feed_value(digest, value);
        }
    }
    feed_raw(digest, &cell.raw);
    feed_extra(digest, cell.extra.as_deref());
}

/// A constant's value, as `cell_same` tells two apart: equal text, or two numbers that are equal (`1` and `1.0`; a zero and a negative zero).
fn feed_value(digest: &mut Digest, value: Option<&crate::Value>) {
    match value.map(|v| (v, v.number())) {
        None => {
            digest.bytes(&[0]);
        }
        Some((_, Some(n))) => {
            let n = if n == 0.0 { 0.0 } else { n };
            digest.bytes(&[1]).bytes(&n.to_bits().to_le_bytes());
        }
        Some((v, None)) => {
            digest.bytes(&[2]).text(v.text());
        }
    }
}

/// Attributes: their number, then each name and value, in order.
fn feed_raw(digest: &mut Digest, raw: &RawAttrs) {
    count(digest, raw.len());
    for (name, value) in raw.iter() {
        digest.text(name).text(value);
    }
}

/// Nodes the model kept: their number, then each one's place and the node.
fn feed_passthrough(digest: &mut Digest, nodes: &Passthrough) {
    count(digest, nodes.len());
    for placed in nodes.iter() {
        digest.bytes(&placed.after.to_le_bytes());
        feed_node(digest, &placed.node);
    }
}

/// A node and everything under it, in order, with no recursion: a tree a file brought is as deep as its parser allowed, but a model built in memory is not
/// held to that. One unit of work for each node.
fn feed_node(digest: &mut Digest, node: &RawNode) {
    let mut pending: Vec<&RawNode> = vec![node];
    while let Some(node) = pending.pop() {
        crate::work::stamp_item();
        match node {
            RawNode::Element(element) => {
                digest.bytes(&[0]).text(&element.name);
                feed_raw(digest, &element.attrs);
                count(digest, element.children.len());
                pending.extend(element.children.iter().rev());
            }
            RawNode::Text(text) => {
                digest.bytes(&[1]).text(text);
            }
            RawNode::Comment(text) => {
                digest.bytes(&[2]).text(text);
            }
            RawNode::Pi { target, data } => {
                digest.bytes(&[3]).text(target).text(data);
            }
        }
    }
}

fn feed_kept(digest: &mut Digest, kept: &Kept) {
    feed_raw(digest, &kept.raw);
    feed_passthrough(digest, &kept.kept);
}

/// An extension slot: a presence byte (an empty slot is an absent one), then each of its three sides.
fn feed_ext(digest: &mut Digest, ext: &Ext<Kept>) {
    let data = ext.get();
    flag(digest, data.is_some());
    let Some(data) = data else { return };
    feed_kept(digest, &data.odf);
    count(digest, data.vsdx.0.len());
    for (name, kept) in &data.vsdx.0 {
        digest.text(name);
        feed_kept(digest, kept);
    }
    feed_kept(digest, &data.native);
}

/// What a cell keeps inside it, or nothing.
fn feed_extra(digest: &mut Digest, extra: Option<&CellExtra>) {
    flag(digest, extra.is_some());
    let Some(extra) = extra else { return };
    optional_text(digest, extra.text.as_deref());
    feed_passthrough(digest, &extra.passthrough);
    feed_ext(digest, &extra.ext);
}

/// The cells of a section or a row: their number, then each one's name and cell, in the map's order.
fn feed_cells(digest: &mut Digest, cells: &Cells) {
    count(digest, cells.len());
    for (name, cell) in cells {
        crate::work::stamp_item();
        digest.text(name.as_str());
        feed_cell(digest, Some(cell));
    }
}

/// The names, sorted, each once: the order a caller lists them in is not part of a stamp.
fn sorted_unique<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut sorted = names.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
}

// ------------------------------------------------------------------------------------------------------------------ the stamps

/// One stamp for several: the domain `all`, then each part's 16 digits in the order given (a value whose keeping rests on cells and on sections is stamped by both, and a
/// writer compares the one stamp it makes the same way). The order is part of it; no part is the stamp of nothing.
pub fn stamp_all(parts: &[Stamp]) -> Stamp {
    let mut digest = Digest::new("all");
    count(&mut digest, parts.len());
    for part in parts {
        digest.text(&part.text());
    }
    digest.finish()
}

/// The stamp of the cells directly under `sheet` that `names` name: see the module documentation. A name no cell holds is part of it, as absent.
pub fn stamp_cells(sheet: &Sheet, names: &[&str]) -> Stamp {
    let names = sorted_unique(names);
    let mut digest = Digest::new("cells");
    count(&mut digest, names.len());
    for name in names {
        crate::work::stamp_item();
        digest.text(name);
        feed_cell(&mut digest, sheet.cells.get(name));
    }
    digest.finish()
}

/// The stamp of the rows called `prefix...` of the sections called `section`: see the module documentation.
pub fn stamp_rows_named(sheet: &Sheet, section: &str, prefix: &str) -> Stamp {
    let mut digest = Digest::new("rows");
    digest.text(section).text(prefix);
    let mut rows = 0_usize;
    for candidate in &sheet.sections {
        crate::work::stamp_item();
        if candidate.name.as_str() != section {
            continue;
        }
        for row in &candidate.rows {
            crate::work::stamp_item();
            if row.name.as_deref().is_some_and(|name| name.starts_with(prefix)) {
                feed_keyed_row(&mut digest, row);
                rows = rows.saturating_add(1);
            }
        }
    }
    count(&mut digest, rows);
    digest.finish()
}

/// A cell as a text style's group signs it (`write/runs.rs`: `sig_of`): see the module documentation.
fn feed_text_sig(digest: &mut Digest, cell: Option<&Cell>) {
    flag(digest, cell.is_some());
    let Some(cell) = cell else { return };
    match &cell.formula {
        FormulaSlot::Local(formula) => {
            digest.bytes(&[1]).text(&formula.source);
        }
        FormulaSlot::Inherit => {
            digest.bytes(&[2]);
        }
        FormulaSlot::Absent | FormulaSlot::NoFormula => {
            digest.bytes(&[0]);
        }
    }
    optional_text(digest, cell.value.as_ref().map(crate::Value::text));
    optional_text(digest, cell.unit.as_deref());
}

/// The number a run's `Style` cell holds when it is a whole number of 0 to 63, as the splice writer reads it.
fn style_bits(cell: &Cell) -> Option<u32> {
    let n = cell.value.as_ref()?.number()?;
    (n.fract() == 0.0 && (0.0..=63.0).contains(&n)).then_some(n as u32)
}

/// A `Style` cell as one bit mask of it: present and whole (the mask applied, 4 bytes), or present and read by its text.
fn feed_style_part(digest: &mut Digest, cell: &Cell, mask: u32) {
    match style_bits(cell) {
        Some(bits) => {
            digest.bytes(&[1]).bytes(&(bits & mask).to_le_bytes());
        }
        None => {
            digest.bytes(&[0]).text(cell.value.as_ref().map_or("", crate::Value::text));
        }
    }
}

/// The stamp of one group of a paragraph's or a run's row, for the styles held per paragraph and per run ("Paragraph and run
/// styles"): the groups and signatures of the splice writer's `runs.rs`, which the module documentation gives. A row is compared by its cells, never by its
/// `IX`, which the engine numbers anew when it formats part of a text.
pub fn stamp_text_group(row: &Row, group: TextGroup) -> Stamp {
    let mut digest = Digest::new("text-group");
    digest.text(group.id());
    let cell = |name: &str| row.cells.get(name);
    let present = match group {
        TextGroup::Underline => cell("Style").is_some() || cell("DblUnderline").is_some(),
        TextGroup::Strike => cell("Strikethru").is_some() || cell("DoubleStrikethrough").is_some(),
        other => other.cells().first().is_some_and(|first| cell(first).is_some()),
    };
    if !present {
        return digest.finish();
    }
    match group {
        TextGroup::Style => {
            if let Some(style) = cell("Style") {
                feed_style_part(&mut digest, style, !4);
                optional_text(&mut digest, style.unit.as_deref());
            }
        }
        TextGroup::Underline => {
            if let Some(style) = cell("Style") {
                feed_style_part(&mut digest, style, 4);
            }
            feed_text_sig(&mut digest, cell("DblUnderline"));
        }
        other => {
            // Every other group is its cells, each as the splice writer signs it: the alignment's two (`HorzAlign`, and `Flags` which writes it again) and the
            // line-through's two, and for the rest the one cell.
            for name in other.cells() {
                feed_text_sig(&mut digest, cell(name));
            }
        }
    }
    digest.finish()
}

/// The stamp of one row: its type, its `Del` flag and every cell of it. Not its place: see the module documentation.
pub fn stamp_row(row: &Row) -> Stamp {
    let mut digest = Digest::new("row");
    crate::work::stamp_item();
    flag(&mut digest, row.del);
    optional_text(&mut digest, row.row_type.as_deref());
    feed_cells(&mut digest, &row.cells);
    digest.finish()
}

/// A row of a section, as the splice writer's `section_same` compares one: its key, its local name, its type and flag, its other attributes and kept nodes,
/// and its cells.
fn feed_keyed_row(digest: &mut Digest, row: &Row) {
    optional_number(digest, row.ix);
    optional_text(digest, row.name.as_deref());
    optional_text(digest, row.local_name.as_deref());
    optional_text(digest, row.row_type.as_deref());
    flag(digest, row.del);
    feed_raw(digest, &row.raw);
    feed_passthrough(digest, &row.passthrough);
    feed_cells(digest, &row.cells);
}

/// A section, as `section_same` compares one: its name, place and flag, its other attributes and kept nodes, its own cells and its rows in order.
fn feed_section(digest: &mut Digest, section: &Section) {
    digest.text(section.name.as_str());
    optional_number(digest, section.ix);
    flag(digest, section.del);
    feed_raw(digest, &section.raw);
    feed_passthrough(digest, &section.passthrough);
    feed_cells(digest, &section.cells);
    count(digest, section.rows.len());
    for row in &section.rows {
        crate::work::stamp_item();
        feed_keyed_row(digest, row);
    }
}

/// The stamp of the first section of `sheet` called `name` that has `IX` `ix` (`None` for a section with none), by what `section_same` compares: what a connector's
/// route is judged by (`Geometry` with `IX` 0, `route_same`), and any one section of a sheet that is asked for by its place. Every other section is no part of
/// it, wherever it sits, and a sheet with none stamps alike with every other that asked for the same. One look at each section, and the rows and cells of the one
/// found.
pub fn stamp_section(sheet: &Sheet, name: &str, ix: Option<u32>) -> Stamp {
    let mut digest = Digest::new("section");
    digest.text(name);
    optional_number(&mut digest, ix);
    let found = sheet.sections.iter().find(|section| {
        crate::work::stamp_item();
        section.name.as_str() == name && section.ix == ix
    });
    // A sheet with none is the name and the place asked for and nothing after them, a shorter stream than any section's: no flag is needed to tell them apart.
    if let Some(section) = found {
        feed_section(&mut digest, section);
    }
    digest.finish()
}

/// The stamp of the sections of `sheet` that `names` name, in the sheet's order: see the module documentation.
pub fn stamp_sections(sheet: &Sheet, names: &[&str]) -> Stamp {
    let names = sorted_unique(names);
    let mut digest = Digest::new("sections");
    count(&mut digest, names.len());
    for name in &names {
        digest.text(name);
    }
    for section in &sheet.sections {
        crate::work::stamp_item();
        if names.binary_search(&section.name.as_str()).is_ok() {
            feed_section(&mut digest, section);
        }
    }
    digest.finish()
}

/// The stamp of a paragraph's or a run's row **as the splice writer keys it** (`write/runs.rs`: `Props::key`, which `View::same` compares): `name=text|unit;` for
/// each cell in order, the value's text and the unit verbatim, and `name=?;` for a cell it cannot read as a value (a formula, no value; `row_keys`). It is the
/// hash of that key's bytes, written out one piece at a time with no text made of it, so two rows stamp alike exactly when their keys are alike, and a row
/// with no cell, which is no row, stamps alike with every other. The row's `IX`, `N`, type, flag and other attributes are no part of it. **This is not
/// `cell_same`'s rule**: `0.2` and `0.20`, `1` and `1.0`, and a unit and none, are different rows here, where [`stamp_cells`] takes each pair for one value.
pub fn stamp_text_row(row: &Row) -> Stamp {
    let mut digest = Digest::new("text-row");
    for (name, cell) in &row.cells {
        crate::work::stamp_item();
        digest.raw(name.as_str().as_bytes());
        match (&cell.formula, &cell.value) {
            (FormulaSlot::Absent | FormulaSlot::NoFormula, Some(value)) => {
                digest.raw(b"=").raw(value.text().as_bytes()).raw(b"|").raw(cell.unit.as_deref().unwrap_or("").as_bytes()).raw(b";");
            }
            _ => {
                digest.raw(b"=?;");
            }
        }
    }
    digest.finish()
}
