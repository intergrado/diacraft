//! Tier 3, LibreOffice edits win: the fingerprints of the native outputs a tier-2 item determined (the `fp-` names; the normalisation list; the design's "Tier 3").
//!
//! **What a fingerprint is.** A tier-2 item (a text, a name, a link to a master, a `Geometry` section, a cell) rests on **native outputs**: the characters in
//! the `text:p`, the `draw:name`, the path in `draw:enhanced-geometry`, the style attribute a cell was written as. When the writer writes the item it writes beside
//! it the fingerprint of those outputs as it wrote them (`diacraft:fp-<item>` on the same `style:graphic-properties`, so that it survives a LibreOffice save as
//! the item does). When the reader meets the item it works out the fingerprint of the outputs the file has now and compares: **the item is
//! stale only when its outputs changed** (a path edited, a text retyped, a name changed, a line end set to another marker), and a stale item is dropped, so the
//! native reading stands. **A change of what the item took as input does not make it stale**: a move or a resize leaves the path as it was (LibreOffice stretches
//! the stored path to the new frame), so the `Geometry` section of a moved or resized shape is still its own.
//!
//! **What it is not.** A fingerprint says whether outputs changed. It is no integrity check and no secret: a file can hold any fingerprint it likes, which gives it
//! nothing it could not write as tier 2 directly.
//!
//! **The hash.** FNV-1a over 64 bits (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`), written by hand so that the file's bytes mean the same in every
//! build of every version: `std`'s hashers promise no algorithm and a toolchain that changed one would make every file read as stale. Its text is 16 lowercase
//! hexadecimal digits, so "a fingerprint of the wrong length" has a meaning (it is no fingerprint). Every input is length-prefixed and starts with the name of what
//! is hashed, so that two things hash alike only when their canonical forms are the same bytes.
//!
//! **What an item without a fingerprint means.** Files that wrote have none, and neither does tier 2 that another program wrote: **no fingerprint is not
//! judged**, and the item stands as it always did. A fingerprint that is not 16 lowercase hexadecimal digits is read as none, with one warning.
//!
//! **Normalisation** (what a LibreOffice save rewrites with no edit, and so must not change a fingerprint): runs of white space in a path, a view box or a list of
//! modifiers are one space; a colour's letters are lower case; a number that is a whole number is its integer. A path is hashed as its text (no
//! attribute of the geometry is added, removed or changed by an unedited, moved, resized or restyled save). **Left out of every fingerprint**, since LibreOffice
//! recomputes them: a connector's route, a custom shape's text areas and glue points, the frame's size and place (inputs), a marker's width
//! (rounded to a thousandth of a centimetre), a length of any kind, `draw:text-style-name`, an empty paragraph, and a flip (`draw:mirror-horizontal` and
//! `-vertical` are the pose cells `FlipX` and `FlipY`: a flip turns the shape and leaves the path as it is, an input like the size).

use diacraft_doc::{Cell, FormulaSlot, SectionKey};

use super::names;

/// The number of hexadecimal digits of a fingerprint's text.
pub(crate) const DIGITS: usize = 16;

/// The prefix of the attribute that holds an item's fingerprint: `fp-` and the item's key.
pub(crate) const PREFIX: &str = "fp-";

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// A fingerprint: 64 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Fp(u64);

impl Fp {
    /// The 16 lowercase hexadecimal digits the file holds.
    pub(crate) fn text(self) -> String {
        format!("{:016x}", self.0)
    }

    /// The fingerprint a text holds: exactly [`DIGITS`] lowercase hexadecimal digits, and nothing else.
    pub(crate) fn parse(text: &str) -> Option<Fp> {
        if text.len() != DIGITS || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return None;
        }
        u64::from_str_radix(text, 16).ok().map(Fp)
    }
}

/// FNV-1a over 64 bits, fed whole fields.
#[derive(Clone, Debug)]
pub(crate) struct Digest(u64);

impl Digest {
    /// A digest of the thing called `domain`: the name is the first field.
    pub(crate) fn new(domain: &str) -> Digest {
        let mut d = Digest(OFFSET);
        d.text(domain);
        d
    }

    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(PRIME);
    }

    /// A field: its length as eight bytes, then its bytes.
    pub(crate) fn field(&mut self, bytes: &[u8]) -> &mut Digest {
        for b in u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes() {
            self.byte(b);
        }
        for b in bytes {
            self.byte(*b);
        }
        self
    }

    pub(crate) fn text(&mut self, text: &str) -> &mut Digest {
        self.field(text.as_bytes())
    }

    pub(crate) fn finish(&self) -> Fp {
        Fp(self.0)
    }
}

/// The FNV-1a-64 of plain bytes: the published algorithm, with no field structure (what the vectors of the unit tests pin).
#[cfg(test)]
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut d = Digest(OFFSET);
    for b in bytes {
        d.byte(*b);
    }
    d.0
}

// ------------------------------------------------------------------------------------------------------------------ the items

/// What a fingerprint is of: one tier-2 item, by the attribute that carries it (`fp-` and the key).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    /// The shape's text (`text`, and the removal of `text`).
    Text,
    /// The formatting of the shape's text as its markup says it (`fp-format`): what [`format_fp`] makes of the runs' and paragraphs' rows. **It judges the same item
    /// as [`Key::Text`]**, the carried text, and is written beside it: a file that has `fp-text` only (one a build before the ruling wrote) is judged by the characters alone, as it was.
    Format,
    /// The shape's names (`name`, `name-u`, and their removals).
    Name,
    /// The shape's VSDX data that no ODF attribute says (`vsdx`): it sits beside the shape's identity, its name and its outline ([`vsdx_fp`]).
    Vsdx,
    /// The shape's foreign data beyond its picture (`foreign`): it sits beside the picture the markup shows ([`picture_fp`]).
    Foreign,
    /// The shape's links to a master (`master`, `master-shape`).
    Master,
    /// One section (`s-<name>[-<ix>]` and the removal of the section): the outline of the shape.
    Section(SectionKey),
    /// One cell (`c-<name>` and the removal of the cell).
    Cell(String),
}

const TEXT: &str = "text";
const FORMAT: &str = "format";
const VSDX: &str = "vsdx";
const FOREIGN: &str = "foreign";
const NAME: &str = "name";
const MASTER: &str = "master";
const CELL: &str = "c-";

impl Key {
    /// The attribute's local name: `fp-text`, `fp-name`, `fp-master`, `fp-s-Geometry-0`, `fp-c-EndArrow`.
    pub(crate) fn attr(&self) -> String {
        match self {
            Key::Text => format!("{PREFIX}{TEXT}"),
            Key::Format => format!("{PREFIX}{FORMAT}"),
            Key::Vsdx => format!("{PREFIX}{VSDX}"),
            Key::Foreign => format!("{PREFIX}{FOREIGN}"),
            Key::Name => format!("{PREFIX}{NAME}"),
            Key::Master => format!("{PREFIX}{MASTER}"),
            Key::Section(key) => format!("{PREFIX}{}", names::section_attr(key)),
            Key::Cell(name) => format!("{PREFIX}{CELL}{}", names::escape(name)),
        }
    }

    /// The key an attribute's local name stands for, or `None` for a name that is no key of this build's (a newer build's, kept as it is).
    pub(crate) fn parse(local: &str) -> Option<Key> {
        let rest = local.strip_prefix(PREFIX)?;
        match rest {
            TEXT => Some(Key::Text),
            FORMAT => Some(Key::Format),
            VSDX => Some(Key::Vsdx),
            FOREIGN => Some(Key::Foreign),
            NAME => Some(Key::Name),
            MASTER => Some(Key::Master),
            _ => {
                if let Some(name) = rest.strip_prefix(CELL) {
                    return names::unescape(name).filter(|n| !n.is_empty()).map(Key::Cell);
                }
                names::section_of_attr(rest).map(Key::Section)
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ the outputs

/// Runs of white space are one space, and none leads or trails.
fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_ascii_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// The fingerprint of a shape's text: its characters, one paragraph after another between U+000A, as the reader counts them. The markers
/// and the rows of a run's formatting are not part of it; an empty paragraph is no text.
pub(crate) fn text_fp(plain: &str) -> Fp {
    Digest::new("text").text(plain).finish()
}

/// The characters of a text as the fingerprint counts them: the characters and the shown text of each field, and no marker.
///
/// **A known limit**: the shown text of a field is part of the characters, so a field whose shown text another program refreshes on a save with
/// no edit (a date, a file name, a page number) changes them, and a carried text of a shape whose markup holds such a field is then told stale and left out. The
/// probe's rules do not cover it. The writer writes the shown text of a field as plain characters (no field element), so a file DiaCraft wrote is not in this
/// case; a file another program wrote, whose shape carries a text, may be.
pub(crate) fn plain_of(text: &diacraft_doc::Text) -> String {
    use diacraft_doc::TextPart;
    let mut out = String::new();
    for part in &text.parts {
        match part {
            TextPart::Chars(c) => out.push_str(c),
            TextPart::Fld { text, .. } => out.push_str(text),
            TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_) | TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => {}
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------ the formatting of a text

/// The cells of a `Character` row that the fingerprint of a text's formatting covers: what a run's style says as a value that stays what it is under a LibreOffice save (a flag, a colour, a
/// name, a size in points to a tenth). **Lengths a save rounds are left out** (`Letterspace`, a paragraph's indents and spacing).
pub(crate) const RUN_CELLS: [&str; 12] =
    ["Style", "Size", "Case", "DblUnderline", "Strikethru", "DoubleStrikethrough", "Overline", "Pos", "Color", "FontScale", "Font", "AsianFont"];

/// The cells of a `Paragraph` row that the fingerprint covers.
pub(crate) const PARAGRAPH_CELLS: [&str; 5] = ["HorzAlign", "Flags", "Bullet", "BulletStr", "BulletFont"];

/// What a cell says, as the fingerprint of a formatting takes it: the value (the writer writes the cached value of a formula, and the reader gives one), a whole number as its integer, a
/// font size in points to a tenth, any other number to four places, a text in lower case.
fn format_value(name: &str, cell: &Cell) -> String {
    match cell.value.as_ref() {
        None => "empty".to_owned(),
        Some(v) => match v.number() {
            Some(n) if name == "Size" && n.is_finite() => format!("pt {:.1}", n * 72.0),
            Some(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => format!("integer {n:.0}"),
            Some(n) => format!("number {n:.4}"),
            None => format!("text {}", v.text().to_ascii_lowercase()),
        },
    }
}

/// The cells of a run's row whose 0 is what no cell says: the normal case, no double underline, no line through, no overline, the normal position.
const OFF_AT_ZERO: [&str; 6] = ["Case", "DblUnderline", "Strikethru", "DoubleStrikethrough", "Overline", "Pos"];

/// Whether a cell says what no cell says, for the fingerprint of a formatting: **a mark that is off** ([`OFF_AT_ZERO`] at 0). Another program writes such a property or leaves it out as it sees fit,
/// and the reader makes a cell of what a style writes of its own: LibreOffice 26.8.2.2 added `fo:text-transform="none"` beside a `fo:font-variant` and left out a `style:text-underline-style="none"` that
/// its own default style says (measured by on a null save of an `.odg` DiaCraft wrote), which made the rows of a run that rode in DiaCraft's data stale though nothing was edited, so
/// DiaCraft's model changed in a shape the other program did not change (the 50 of 111 bold edits that changed more than one shape).
fn is_said_as_none(name: &str, cell: &Cell) -> bool {
    OFF_AT_ZERO.contains(&name) && cell.formula.text().is_none() && cell.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0)
}

/// The digest of each row of the section `section` of `sheet` that has any of the cells `names`, by the row's `IX`: **one pass over the section's rows** (a text of a hundred thousand markers
/// is no search of the rows for each), each row looked at is a step. A row that says nothing this fingerprint covers is none (it is no formatting); the first row of an `IX` stands.
fn rows_format(sheet: &diacraft_doc::Sheet, section: &str, names: &[&str], steps: &mut u64) -> std::collections::HashMap<u32, Fp> {
    let mut out: std::collections::HashMap<u32, Fp> = std::collections::HashMap::new();
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for found in sheet.sections.iter().filter(|s| s.name.as_str() == section) {
        for row in found.rows.iter() {
            *steps = steps.saturating_add(1);
            let Some(ix) = row.ix else { continue };
            if !seen.insert(ix) {
                continue;
            }
            let mut d = Digest::new("format-row");
            let mut any = false;
            for name in names {
                if let Some(cell) = row.cells.get(*name).filter(|cell| !is_said_as_none(name, cell)) {
                    d.text(name).text(&format_value(name, cell));
                    any = true;
                }
            }
            if any {
                out.insert(ix, d.finish());
            }
        }
    }
    out
}

/// A stretch of characters and the formatting they have: the run's row and the paragraph's, as digests, `None` for none.
type Stretch = (Option<Fp>, Option<Fp>, u64);

/// The fingerprint of a text's formatting: the characters in stretches that have one formatting each (a run's row and a paragraph's, **by what their cells say and not by their `IX`**, which
/// a reader numbers anew), the stretches that follow each other with one formatting made one, and **a marker whose row says nothing the same as none**. A text with no marker and a text whose markers
/// name rows with no cell that this fingerprint covers have the same one, which is the plain text's ([`plain_format_fp`]). `sheet` is the sheet that holds the rows (a read's, before the item
/// is put over it).
pub(crate) fn format_fp(text: &diacraft_doc::Text, sheet: &diacraft_doc::Sheet, steps: &mut u64) -> Fp {
    use diacraft_doc::TextPart;
    let mut stretches: Vec<Stretch> = Vec::new();
    let (mut run, mut para): (Option<Fp>, Option<Fp>) = (None, None);
    // The rows are looked at once each, and only when the text has a marker that names one.
    let has = |f: fn(&TextPart) -> bool| text.parts.iter().any(f);
    let runs = if has(|p| matches!(p, TextPart::Cp(_))) { rows_format(sheet, "Character", &RUN_CELLS, steps) } else { Default::default() };
    let paras = if has(|p| matches!(p, TextPart::Pp(_))) { rows_format(sheet, "Paragraph", &PARAGRAPH_CELLS, steps) } else { Default::default() };
    for part in &text.parts {
        *steps = steps.saturating_add(1);
        let count = match part {
            TextPart::Chars(c) => c.chars().count(),
            TextPart::Fld { text, .. } => text.chars().count(),
            TextPart::Cp(ix) => {
                run = runs.get(ix).copied();
                0
            }
            TextPart::Pp(ix) => {
                para = paras.get(ix).copied();
                0
            }
            TextPart::Tp(_) | TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => 0,
        };
        if count == 0 {
            continue;
        }
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        match stretches.last_mut() {
            Some(last) if last.0 == run && last.1 == para => last.2 = last.2.saturating_add(count),
            _ => stretches.push((run, para, count)),
        }
    }
    stretches_fp(&stretches)
}

/// The fingerprint of the formatting of a text of `characters` characters that has none: what a save writes of a text it writes as plain characters, and what a reader makes of it.
pub(crate) fn plain_format_fp(characters: u64) -> Fp {
    stretches_fp(&if characters == 0 { Vec::new() } else { vec![(None, None, characters)] })
}

fn stretches_fp(stretches: &[Stretch]) -> Fp {
    let mut d = Digest::new("text-format");
    for (run, para, count) in stretches {
        let one = |fp: &Option<Fp>| fp.map_or([0u8; 8], |f| f.0.to_le_bytes());
        d.text(if run.is_some() { "run" } else { "-" }).field(&one(run));
        d.text(if para.is_some() { "para" } else { "-" }).field(&one(para));
        d.field(&count.to_le_bytes());
    }
    d.finish()
}

/// The fingerprint of the native outputs a shape's VSDX data sits beside: its `draw:name`'s and its outline's (none for an element that has none: a line, a connector, a picture frame).
pub(crate) fn vsdx_fp(name: Fp, outline: Option<Fp>) -> Fp {
    let mut d = Digest::new("vsdx");
    d.field(&name.0.to_le_bytes());
    match outline {
        Some(o) => d.text("outline").field(&o.0.to_le_bytes()),
        None => d.text("none"),
    };
    d.finish()
}

/// The fingerprint of the picture a shape's foreign data sits beside: its length and its first eight bytes (what tells one picture from another without reading it again), or none.
pub(crate) fn picture_fp(bytes: Option<&[u8]>) -> Fp {
    let mut d = Digest::new("picture");
    match bytes {
        Some(b) => d.text("some").field(&u64::try_from(b.len()).unwrap_or(u64::MAX).to_le_bytes()).field(b.get(..8).unwrap_or(b)),
        None => d.text("none"),
    };
    d.finish()
}

/// The fingerprint of a shape's names: its `draw:name`, which the reader gives both `Name` and `NameU`, or none.
pub(crate) fn name_fp(draw_name: Option<&str>) -> Fp {
    let mut d = Digest::new("name");
    match draw_name {
        Some(name) => d.text("some").text(name),
        None => d.text("none"),
    };
    d.finish()
}

/// What an outline is made of in the file: the kind of element and the attributes and children that define the path, each as the file spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutlineKind {
    /// `draw:custom-shape`: its `draw:enhanced-geometry`.
    Custom,
    /// `draw:path`: `svg:d`.
    Path,
    /// `draw:polygon` and `draw:polyline`: `draw:points`.
    Points,
}

/// The parts of an outline, each as the file spells it (borrowed from the element or from what the writer wrote).
#[derive(Clone, Debug)]
pub(crate) struct Outline<'a> {
    pub kind: OutlineKind,
    /// `draw:enhanced-path`, `svg:d` or `draw:points`.
    pub path: Option<&'a str>,
    /// `svg:viewBox`.
    pub view_box: Option<&'a str>,
    /// `draw:type`.
    pub shape_type: Option<&'a str>,
    /// `draw:modifiers`.
    pub modifiers: Option<&'a str>,
    /// Each `draw:equation`: its `draw:name` and `draw:formula`, in order.
    pub equations: Vec<(&'a str, &'a str)>,
    /// Each `draw:handle`: its attributes, in the order the file has them.
    pub handles: Vec<Vec<(&'a str, &'a str)>>,
}

impl<'a> Outline<'a> {
    pub(crate) fn new(kind: OutlineKind) -> Outline<'a> {
        Outline { kind, path: None, view_box: None, shape_type: None, modifiers: None, equations: Vec::new(), handles: Vec::new() }
    }

    /// The bytes the fingerprint reads: for the charge of the work.
    pub(crate) fn bytes(&self) -> usize {
        let one = |o: Option<&str>| o.map_or(0, str::len);
        let eq: usize = self.equations.iter().map(|(n, f)| n.len() + f.len()).sum();
        let handles: usize = self.handles.iter().flatten().map(|(n, v)| n.len() + v.len()).sum();
        one(self.path) + one(self.view_box) + one(self.shape_type) + one(self.modifiers) + eq + handles
    }
}

/// The fingerprint of an outline and of its parts: **the whole** is what a tier-2 item of the outline is judged by, and the parts let a save that rewrites one part of the
/// outline (the path of a custom shape: `draw:enhanced-path`) make the whole again from the new text and the fingerprints of the parts it left as the file has them,
/// without the file's other parts at hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OutlineFp {
    pub kind: OutlineKind,
    pub whole: Fp,
    /// The path (`draw:enhanced-path`, `svg:d`, `draw:points`).
    pub path: Fp,
    /// `draw:modifiers`.
    pub modifiers: Fp,
    /// The view box, the type, the equations and the handles.
    pub rest: Fp,
}

fn kind_name(kind: OutlineKind) -> &'static str {
    match kind {
        OutlineKind::Custom => "custom",
        OutlineKind::Path => "path",
        OutlineKind::Points => "points",
    }
}

/// The fingerprint of a text part of an outline (the path, the modifiers): white space runs are one space, none is `-`.
fn part_fp(label: &str, value: Option<&str>) -> Fp {
    let mut d = Digest::new(label);
    match value {
        Some(v) => d.text(&spaced(v)),
        None => d.text("-"),
    };
    d.finish()
}

/// The fingerprint of the path of an outline (`draw:enhanced-path`, `svg:d` or `draw:points`) as the text `path`.
pub(crate) fn path_part(path: &str) -> Fp {
    part_fp("outline-path", Some(path))
}

/// The whole outline from its parts.
pub(crate) fn join_outline(kind: OutlineKind, path: Fp, modifiers: Fp, rest: Fp) -> Fp {
    let mut d = Digest::new("outline");
    d.text(kind_name(kind)).field(&path.0.to_le_bytes()).field(&modifiers.0.to_le_bytes()).field(&rest.0.to_le_bytes());
    d.finish()
}

/// The fingerprints of a shape's outline, whole and by part. Equations are in order; handles are by the sorted names and values of their attributes, so that
/// LibreOffice writing a handle's attributes in another order is no edit.
pub(crate) fn outline_parts(o: &Outline<'_>) -> OutlineFp {
    let path = part_fp("outline-path", o.path);
    let modifiers = part_fp("outline-modifiers", o.modifiers);
    let mut rest = Digest::new("outline-rest");
    for (label, value) in [("view-box", o.view_box), ("type", o.shape_type)] {
        rest.text(label);
        match value {
            Some(v) => rest.text(&spaced(v)),
            None => rest.text("-"),
        };
    }
    rest.text("equations");
    for (name, formula) in &o.equations {
        rest.text(name).text(&spaced(formula));
    }
    rest.text("handles");
    for handle in &o.handles {
        let mut sorted: Vec<(&str, String)> = handle.iter().map(|(n, v)| (*n, spaced(v))).collect();
        sorted.sort();
        rest.text("handle");
        for (n, v) in &sorted {
            rest.text(n).text(v);
        }
    }
    let rest = rest.finish();
    OutlineFp { kind: o.kind, whole: join_outline(o.kind, path, modifiers, rest), path, modifiers, rest }
}

/// The fingerprint of a shape's outline (the whole of [`outline_parts`]).
pub(crate) fn outline_fp(o: &Outline<'_>) -> Fp {
    outline_parts(o).whole
}

/// The view box a new custom shape's geometry element is written with (`write/shapes.rs`), and its `draw:type`: what the fingerprint of the outline of a new shape
/// is made from beside the path it writes.
pub(crate) const NEW_VIEW_BOX: &str = "0 0 21600 21600";
pub(crate) const NEW_SHAPE_TYPE: &str = "non-primitive";

/// The fingerprint of the outline of a new custom shape whose `draw:enhanced-path` is `path`: the geometry element the writer makes has that path, the view box
/// [`NEW_VIEW_BOX`] and the type [`NEW_SHAPE_TYPE`], and no equation, modifier, mirror or handle.
pub(crate) fn new_custom_outline_fp(path: &str) -> Fp {
    let mut o = Outline::new(OutlineKind::Custom);
    o.path = Some(path);
    o.view_box = Some(NEW_VIEW_BOX);
    o.shape_type = Some(NEW_SHAPE_TYPE);
    outline_fp(&o)
}

/// Whether the cell `name` says the custom line end 254: a drawing draws number 4's marker for it, and the cell itself is tier 2.
pub(crate) fn is_custom_end(name: &str, cell: &Cell) -> bool {
    matches!(name, "BeginArrow" | "EndArrow") && cell.value.as_ref().and_then(diacraft_doc::Value::number) == Some(254.0)
}

/// The `NameU` of the built-in master that is DiaCraft's filled diamond: a custom end whose `USE` names it is written as the diamond's own marker.
pub(crate) const FILLED_DIAMOND: &str = "Filled Diamond";

/// Whether the cell `name` is a custom line end (254) whose formula is `USE("Filled Diamond")`, DiaCraft's own filled diamond: the writer draws a marker of its form for it and not number
/// 4's. The name is the master's `NameU`, as `calc` finds a master by it; another master of that name is drawn as the diamond in another program, and the cells
/// that the file carries say the true one in DiaCraft.
pub(crate) fn is_filled_diamond(name: &str, cell: &Cell) -> bool {
    use diacraft_formula::Expr;
    if !is_custom_end(name, cell) {
        return false;
    }
    let FormulaSlot::Local(formula) = &cell.formula else { return false };
    match formula.ast.as_ref() {
        Ok(Expr::Call(call)) if call.is("USE") => matches!(call.args.first(), Some(Some(Expr::Str(used))) if &**used == FILLED_DIAMOND),
        _ => false,
    }
}

/// The cells whose native output is one style attribute with a value that stays what it is under a LibreOffice save: a number that is a whole number, or a
/// colour, so that no rounding LibreOffice does (lengths and widths to a thousandth of a centimetre) reaches them.
pub(crate) const CELLS: [&str; 11] = [
    "BeginArrow",
    "EndArrow",
    "BeginArrowSize",
    "EndArrowSize",
    "FillPattern",
    "FillGradientEnabled",
    "FillForegnd",
    "FillBkgnd",
    "LinePattern",
    "LineColor",
    "LineCap",
];

/// Whether a cell's native output is fingerprinted.
pub(crate) fn is_fingerprinted_cell(name: &str) -> bool {
    CELLS.contains(&name)
}

/// The fingerprint of the cell `name` as the file's markup gives it (what the reader rebuilds), or of its absence. A formula is its text; a number that is a whole
/// number is its integer and any other a rounded decimal; a text is lower case (a colour's letters).
pub(crate) fn cell_fp(name: &str, native: Option<&Cell>) -> Fp {
    // `FillGradientEnabled` of the constant 0 is the cell's default, which the reader gives a solid fill and the writer does not carry (`native::tier2`): it says the same
    // as no cell at all, so that a frame whose style gives the 0 and a writer that expects none agree.
    let native = native.filter(|c| {
        !(name == "FillGradientEnabled"
            && matches!(c.formula, FormulaSlot::Absent)
            && c.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0))
    });
    let mut d = Digest::new("cell");
    d.text(name);
    match native {
        None => d.text("absent"),
        Some(c) => match (&c.formula, &c.value) {
            (FormulaSlot::Local(_), _) => d.text("formula").text(c.formula.text().unwrap_or_default()),
            (_, Some(v)) => match v.number() {
                Some(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => d.text("integer").text(&format!("{n:.0}")),
                Some(n) => d.text("number").text(&format!("{n:.6}")),
                None => d.text("text").text(&v.text().to_ascii_lowercase()),
            },
            (_, None) => d.text("empty"),
        },
    };
    d.finish()
}

/// The fingerprint of a line end whose marker is not one of DiaCraft's own (`Marker::Other`: another program's marker, which the reader gives the stand-in 4):
/// not the fingerprint of the cell 4, which is DiaCraft's own number 4's.
pub(crate) fn other_marker_fp(name: &str) -> Fp {
    Digest::new("cell").text(name).text("other marker").finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_doc::Value;

    /// The published test vectors of FNV-1a over 64 bits (the algorithm the file's bytes mean).
    #[test]
    fn the_hash_is_fnv_1a_over_64_bits_with_its_published_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"b"), 0xaf63_df4c_8601_f1a5);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
        // The same fields in another order, or cut in another place, are another fingerprint.
        let ab = Digest::new("x").text("a").text("b").finish();
        assert_ne!(ab, Digest::new("x").text("b").text("a").finish());
        assert_ne!(ab, Digest::new("x").text("ab").finish());
        assert_ne!(Digest::new("x").text("a").finish(), Digest::new("y").text("a").finish());
        // Fingerprints are pinned, as an independent implementation of the field structure makes them (a build that changed them would read every file as stale).
        assert_eq!(text_fp("Hello").text(), "f6b7d5cf1978af2d");
        assert_eq!(name_fp(None).text(), "841bcdc9b1ed3ef4");
        assert_eq!(name_fp(Some("Box A")).text(), "5f16ef6f7707cc6f");
        let four = Cell::constant(diacraft_doc::Value::from_number(4.0).unwrap());
        assert_eq!(cell_fp("EndArrow", Some(&four)).text(), "7e74b1720ac25fd9");
        assert_eq!(cell_fp("EndArrow", None).text(), "c971f2b4f3f6eae0");
        // The outline and its parts (pinned by an independent implementation of the field structure).
        let parts = outline_parts(&custom("M 0 0 Z N"));
        assert_eq!(parts.whole.text(), "d09615d74b9acb68");
        assert_eq!(parts.path.text(), "8cd71a145bd18c61");
        assert_eq!(parts.modifiers.text(), "61f19b63023aae17");
        assert_eq!(parts.rest.text(), "8015358bcd654326");
        assert_eq!(outline_fp(&custom("M 0 0 Z N")), parts.whole);
    }

    /// The whole outline is made of its parts: a part rewritten alone gives the whole again from the new part and the others as they were (what a save that writes only
    /// the path of a custom shape needs: it holds the fingerprints of the parts it left and the text it wrote), and each part is told from the others.
    #[test]
    fn the_whole_outline_is_made_again_from_a_part_rewritten_and_the_parts_left() {
        let before = outline_parts(&custom("M 0 0 L 1 1 Z N"));
        // The path rewritten: the whole is as if the outline had been made with the new path.
        let after = outline_parts(&custom("M 0 0 L 2 2 Z N"));
        assert_ne!(before.whole, after.whole);
        assert_eq!(before.rest, after.rest, "the rest is not the path");
        assert_eq!(before.modifiers, after.modifiers);
        assert_eq!(path_part("M 0 0 L 2 2 Z N"), after.path);
        assert_eq!(join_outline(OutlineKind::Custom, path_part("M 0 0 L 2 2 Z N"), before.modifiers, before.rest), after.whole);
        // Runs of white space in the path are one space, in the part as in the whole.
        assert_eq!(path_part("M 0   0\nL 2 2 Z N"), path_part("M 0 0 L 2 2 Z N"));
        // The modifiers, the view box, the type, the equations and the handles are each told from the path: each moves the whole and not the path.
        let mut with_modifiers = custom("M 0 0 L 1 1 Z N");
        with_modifiers.modifiers = Some("5400");
        let m = outline_parts(&with_modifiers);
        assert_eq!((m.path, m.rest), (before.path, before.rest));
        assert_ne!((m.modifiers, m.whole), (before.modifiers, before.whole));
        let mut with_view_box = custom("M 0 0 L 1 1 Z N");
        with_view_box.view_box = Some("0 0 10 10");
        let v = outline_parts(&with_view_box);
        assert_eq!((v.path, v.modifiers), (before.path, before.modifiers));
        assert_ne!((v.rest, v.whole), (before.rest, before.whole));
        let mut with_equation = custom("M 0 0 L 1 1 Z N");
        with_equation.equations = vec![("f0", "$0 +1")];
        assert_ne!(outline_parts(&with_equation).rest, before.rest);
        let mut with_handle = custom("M 0 0 L 1 1 Z N");
        with_handle.handles = vec![vec![("handle-position", "$0 top")]];
        assert_ne!(outline_parts(&with_handle).rest, before.rest);
        // The kind is part of the whole and not of the parts.
        let mut as_path = custom("M 0 0 L 1 1 Z N");
        as_path.kind = OutlineKind::Path;
        let p = outline_parts(&as_path);
        assert_eq!((p.path, p.modifiers, p.rest), (before.path, before.modifiers, before.rest));
        assert_ne!(p.whole, before.whole);
        assert_eq!(p.kind, OutlineKind::Path);
        // The parts joined in another order are another whole.
        assert_ne!(join_outline(OutlineKind::Custom, before.modifiers, before.path, before.rest), before.whole);
    }

    /// A fingerprint is 16 lowercase hexadecimal digits and nothing else.
    #[test]
    fn a_fingerprint_is_sixteen_lowercase_hexadecimal_digits() {
        let fp = text_fp("Hello");
        assert_eq!(Fp::parse(&fp.text()), Some(fp));
        for bad in [
            "",
            "0",
            "0123456789abcde",
            "0123456789abcdef0",
            "0123456789ABCDEF",
            "0123456789abcdeg",
            " 123456789abcdef",
            "0123456789abcde\n",
            "+123456789abcdef",
        ] {
            assert_eq!(Fp::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Fp::parse("0000000000000000").map(Fp::text).as_deref(), Some("0000000000000000"));
    }

    /// Every key's attribute names it back; a name that is no key of this build's is none.
    #[test]
    fn a_key_names_its_attribute_and_the_attribute_names_the_key() {
        let keys = [
            (Key::Text, "fp-text"),
            (Key::Name, "fp-name"),
            (Key::Master, "fp-master"),
            (Key::Section(SectionKey::new("Geometry", Some(0))), "fp-s-Geometry-0"),
            (Key::Section(SectionKey::new("User", None)), "fp-s-User"),
            (Key::Cell("EndArrow".to_owned()), "fp-c-EndArrow"),
            (Key::Cell("a b".to_owned()), "fp-c-a_20b"),
        ];
        for (key, attr) in keys {
            assert_eq!(key.attr(), attr);
            assert_eq!(Key::parse(attr), Some(key));
        }
        for not in ["", "fp-", "fp-future", "fp-c-", "fp-s-", "fp-s-Geometry-01", "fp-c-a b", "fp-text2", "text", "cells", "fp-fp-text"] {
            assert_eq!(Key::parse(not), None, "{not}");
        }
    }

    #[test]
    fn a_text_is_its_characters_and_the_shown_text_of_its_fields_and_no_marker() {
        use diacraft_doc::TextPart;
        let text = diacraft_doc::Text {
            parts: vec![
                TextPart::Cp(1),
                TextPart::Chars("Hi".into()),
                TextPart::Pp(2),
                TextPart::Chars("\nthere ".into()),
                TextPart::Fld { ix: 0, text: "3".into() },
                TextPart::Tp(1),
            ],
            ..Default::default()
        };
        assert_eq!(plain_of(&text), "Hi\nthere 3");
        assert_ne!(text_fp("Hi"), text_fp("Hi "));
        assert_eq!(text_fp(""), text_fp(""));
    }

    #[test]
    fn a_name_is_its_draw_name_or_none() {
        assert_ne!(name_fp(Some("")), name_fp(None));
        assert_ne!(name_fp(Some("a")), name_fp(Some("b")));
        assert_eq!(name_fp(Some("Box A")), name_fp(Some("Box A")));
    }

    fn custom<'a>(path: &'a str) -> Outline<'a> {
        let mut o = Outline::new(OutlineKind::Custom);
        o.path = Some(path);
        o.view_box = Some("0 0 21600 21600");
        o.shape_type = Some("non-primitive");
        o
    }

    /// An outline is hashed by its path and what defines it, with runs of white space as one space; any change of a part is another fingerprint, and the kind of
    /// element is part of it. Equations are in order, handles by their sorted attributes.
    #[test]
    fn an_outline_changes_with_its_path_and_not_with_white_space() {
        let a = outline_fp(&custom("M 0 0 L 21600 0 L 21600 21600 Z N"));
        assert_eq!(a, outline_fp(&custom("M 0 0  L 21600 0\nL 21600 21600 Z N ")), "white space");
        assert_ne!(a, outline_fp(&custom("M 0 0 L 21600 0 L 21600 21601 Z N")));
        let mut o = custom("M 0 0 L 21600 0 L 21600 21600 Z N");
        o.view_box = Some("0 0 100 100");
        assert_ne!(a, outline_fp(&o), "the view box");
        let mut o = custom("M 0 0 L 21600 0 L 21600 21600 Z N");
        o.modifiers = Some("1 2");
        assert_ne!(a, outline_fp(&o), "the modifiers");
        o.modifiers = Some("1   2");
        let b = outline_fp(&o);
        o.modifiers = Some("1 2");
        assert_eq!(b, outline_fp(&o));
        let mut o = custom("M 0 0 L 21600 0 L 21600 21600 Z N");
        o.shape_type = Some("rectangle");
        assert_ne!(a, outline_fp(&o), "the type");
        let mut o = custom("M 0 0 L 21600 0 L 21600 21600 Z N");
        o.kind = OutlineKind::Path;
        assert_ne!(a, outline_fp(&o), "the kind of element");
        let mut o = custom("M 0 0 Z N");
        o.equations = vec![("f0", "$0 /2"), ("f1", "$1")];
        let eq = outline_fp(&o);
        o.equations = vec![("f1", "$1"), ("f0", "$0 /2")];
        assert_ne!(eq, outline_fp(&o), "equations are in order");
        o.equations = vec![("f0", "$0   /2"), ("f1", "$1")];
        assert_eq!(eq, outline_fp(&o), "white space in a formula");
        let mut o = custom("M 0 0 Z N");
        o.handles = vec![vec![("draw:handle-position", "$0 0"), ("draw:handle-range-x-minimum", "0")]];
        let h = outline_fp(&o);
        o.handles = vec![vec![("draw:handle-range-x-minimum", "0"), ("draw:handle-position", "$0 0")]];
        assert_eq!(h, outline_fp(&o), "the order of a handle's attributes");
        o.handles = vec![vec![("draw:handle-position", "$0 1"), ("draw:handle-range-x-minimum", "0")]];
        assert_ne!(h, outline_fp(&o), "a handle's value");
        o.handles.clear();
        assert_ne!(h, outline_fp(&o), "a handle gone");
    }

    #[test]
    fn the_bytes_an_outline_reads_are_those_of_every_part_of_it() {
        let mut o = custom("M 0 0");
        assert_eq!(o.bytes(), "M 0 0".len() + "0 0 21600 21600".len() + "non-primitive".len());
        o.modifiers = Some("12");
        // The two parts of an equation and of a handle's attribute are chosen so that the sum of their lengths is told from the product: equations 3 + 4 and 1 + 2 (7 and
        // 3, 10 in all, where the products 12 and 2 make 14), handle attributes 1 + 3 and 2 + 1 (4 and 3, 7 in all, where the products 3 and 2 make 5).
        o.equations = vec![("abc", "defg"), ("h", "ij")];
        o.handles = vec![vec![("a", "bcd")], vec![("ef", "g")]];
        assert_eq!(o.bytes(), "M 0 0".len() + 15 + 13 + 2 + (3 + 4 + 1 + 2) + (1 + 3 + 2 + 1));
    }

    fn cell(value: Value) -> Cell {
        Cell::constant(value)
    }

    /// `FillGradientEnabled` of the constant 0 is the cell's default (the reader gives it a solid fill, the writer carries none): it says what no cell says; any other
    /// value, a formula, and every other cell named 0 are told from none.
    #[test]
    fn a_gradient_switch_of_zero_is_no_cell() {
        let zero = cell(Value::from_number(0.0).unwrap());
        assert_eq!(cell_fp("FillGradientEnabled", Some(&zero)), cell_fp("FillGradientEnabled", None));
        assert_ne!(cell_fp("FillGradientEnabled", Some(&cell(Value::from_number(1.0).unwrap()))), cell_fp("FillGradientEnabled", None));
        assert_ne!(cell_fp("FillPattern", Some(&zero)), cell_fp("FillPattern", None), "another cell of 0 is a cell");
        let formula = Cell::with_formula("0", Value::from_number(0.0));
        assert_ne!(cell_fp("FillGradientEnabled", Some(&formula)), cell_fp("FillGradientEnabled", None), "a formula is not the default");
    }

    /// A cell is hashed by what a LibreOffice save keeps of it: a whole number is its integer, a colour is lower case, a formula is its text, and none is none.
    #[test]
    fn a_cell_is_hashed_as_a_whole_number_a_lower_case_text_a_formula_or_none() {
        let four = cell(Value::from_number(4.0).unwrap());
        assert_ne!(
            cell_fp("EndArrow", Some(&four)),
            cell_fp("EndArrow", Some(&cell(Value::from_number(4.5).unwrap()))),
            "a fraction is not a whole number"
        );
        assert_ne!(cell_fp("EndArrow", Some(&four)), cell_fp("EndArrow", None), "present against absent");
        assert_ne!(cell_fp("EndArrow", Some(&four)), cell_fp("BeginArrow", Some(&four)), "the name is part of it");
        assert_ne!(cell_fp("EndArrow", Some(&four)), cell_fp("EndArrow", Some(&cell(Value::from_number(5.0).unwrap()))));
        assert_eq!(
            cell_fp("FillForegnd", Some(&cell(Value::from_text("#FF0000")))),
            cell_fp("FillForegnd", Some(&cell(Value::from_text("#ff0000"))))
        );
        assert_ne!(
            cell_fp("FillForegnd", Some(&cell(Value::from_text("#ff0000")))),
            cell_fp("FillForegnd", Some(&cell(Value::from_text("#ff0001"))))
        );
        let formula = Cell::with_formula("THEMEVAL(\"AccentColor1\")", Some(Value::from_text("#4472c4")));
        assert_ne!(cell_fp("FillForegnd", Some(&formula)), cell_fp("FillForegnd", None));
        // A whole number is its integer up to a quadrillion (one under it, whole), and a number at it or past it is a decimal: the two are told apart by the digest.
        let integer = |n: f64| Digest::new("cell").text("X").text("integer").text(&format!("{n:.0}")).finish();
        let decimal = |n: f64| Digest::new("cell").text("X").text("number").text(&format!("{n:.6}")).finish();
        assert_eq!(cell_fp("X", Some(&cell(Value::from_number(999_999_999_999_999.0).unwrap()))), integer(999_999_999_999_999.0));
        assert_eq!(cell_fp("X", Some(&cell(Value::from_number(1e15).unwrap()))), decimal(1e15));
        assert_eq!(cell_fp("X", Some(&cell(Value::from_number(-1e15).unwrap()))), decimal(-1e15));
        assert_eq!(cell_fp("X", Some(&cell(Value::from_number(0.5).unwrap()))), decimal(0.5));
        // The unit is no part of it (it says how a value shows), and a whole number holds in any form.
        let mut inches = cell(Value::from_number(4.0).unwrap());
        inches.unit = Some("IN".into());
        assert_eq!(cell_fp("EndArrow", Some(&inches)), cell_fp("EndArrow", Some(&four)));
    }

    #[test]
    fn a_marker_of_another_program_is_not_number_4s() {
        let four = cell(Value::from_number(4.0).unwrap());
        assert_ne!(other_marker_fp("EndArrow"), cell_fp("EndArrow", Some(&four)));
        assert_ne!(other_marker_fp("EndArrow"), other_marker_fp("BeginArrow"));
        assert_ne!(other_marker_fp("EndArrow"), cell_fp("EndArrow", None));
    }

    /// A run whose row holds the cells of `cells` (name, number), in a text of one marker and its characters.
    fn run_fp(cells: &[(&str, f64)]) -> Fp {
        let mut row = diacraft_doc::Row { ix: Some(0), ..diacraft_doc::Row::default() };
        for (name, n) in cells {
            row.cells.insert(diacraft_doc::CellName::new(name), Cell::constant(Value::from_number(*n).unwrap()));
        }
        let mut section = diacraft_doc::Section::new("Character", None);
        section.rows.push(std::sync::Arc::new(row));
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.sections.push(std::sync::Arc::new(section));
        let text = diacraft_doc::Text {
            parts: vec![diacraft_doc::TextPart::Cp(0), diacraft_doc::TextPart::Chars(std::sync::Arc::from("bold one"))],
            ..Default::default()
        };
        format_fp(&text, &sheet, &mut 0)
    }

    /// **A mark that is off says what no cell says**: the case, a double underline, a line through, an overline and the position at 0 are the run as it is with none of them, so that a
    /// program that writes such a property where DiaCraft did not (LibreOffice's `fo:text-transform="none"`), or leaves out one that its own default style says, changes no formatting. A mark that is on,
    /// and the style bits at 0 (a bold turned off), still tell two runs apart.
    #[test]
    fn a_mark_that_is_off_is_the_run_with_no_cell_for_it() {
        let bold = run_fp(&[("Style", 1.0)]);
        for name in ["Case", "DblUnderline", "Strikethru", "DoubleStrikethrough", "Overline", "Pos"] {
            assert_eq!(run_fp(&[("Style", 1.0), (name, 0.0)]), bold, "{name} at 0");
            assert_ne!(run_fp(&[("Style", 1.0), (name, 1.0)]), bold, "{name} at 1");
        }
        assert_ne!(run_fp(&[("Style", 0.0)]), bold, "a bold turned off");
        assert_ne!(run_fp(&[("Style", 0.0)]), run_fp(&[]), "the style bits at 0 are a cell that says the run is not bold");
        assert_eq!(run_fp(&[("Case", 0.0)]), plain_format_fp(8), "a row that says only marks that are off is no formatting");
    }

    #[test]
    fn the_cells_that_are_fingerprinted_are_the_discrete_style_cells() {
        for name in ["EndArrow", "BeginArrowSize", "FillForegnd", "LinePattern"] {
            assert!(is_fingerprinted_cell(name), "{name}");
        }
        for name in ["LineWeight", "PinX", "Width", "TopMargin", "FillForegndTrans", "Comment"] {
            assert!(!is_fingerprinted_cell(name), "{name}");
        }
    }
}
