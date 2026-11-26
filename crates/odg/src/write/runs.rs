//! A shape's text as the paragraphs and runs the file will hold (retext):
//! each paragraph of the model with the `Paragraph` row its `pp` markers give it and the `Character` row of
//! each of its runs, the rows turned into the attributes of an automatic style, and the plan that sets the paragraphs of the file beside those of the model.
//!
//! * **Which row holds where**: a `cp` holds until the next `cp`, across paragraphs; a text with no marker has row 0; the last `pp` before
//!   a paragraph's end sets the whole paragraph, and holds until the next. A run is a stretch of one paragraph under one `Character` row; a row no cell of which
//!   the writer writes is a run with no style at all. **Two models are compared by the cells of their rows, never by the `IX`** (the engine numbers the rows of
//!   a text anew when it formats part of one), so a paragraph is the same when its characters, the cells of its `Paragraph` row and the cells of the row of each
//!   run are.
//! * **A row's attributes** (inverted): the cells a row holds become `style:text-properties` (a run's) or `style:paragraph-properties` (a paragraph's)
//!   attributes, and a cell this does not map is refused in words. A face is named by the `style:font-face` of the file that declares it (the part's
//!   declarations are looked at when the style is written, and a face none declares is declared there, R22: the number 0 is the document's own face and names
//!   none). The cells of a bullet belong to the list, not the paragraph: a paragraph whose row has them is the item of a `text:list` with an automatic list style
//!   made from them ([`ListSpec`]), beside a neighbour that has the same row it is an item of that neighbour's list.
//! * **The plan** ([`plan`]): the paragraphs of the file and of the model are set beside each other in linear time: the paragraphs that are alike at the head and
//!   at the tail are kept; the rest, when the counts are the same, pair by place; when they are not, each paragraph of the model that is like a paragraph of the file
//!   after the one matched before it is kept, and the others pair by place within the gaps, the extra ones of the model inserted and the extra ones of the file taken
//!   out. Whatever is kept keeps its bytes.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::Arc;

use diacraft_doc::{Row, Sheet, Text, TextPart};

use super::lengths::length_text;
use super::stylemap::{CellVal, Op, Val, is_colour};
use crate::read::names::Ns;
use crate::read::units::LenUnit;

/// The most paragraphs a retext holds: the views of the two texts are a record for each, and the text may be as long as [`super::text::MOST_EDITED_TEXT`].
pub(crate) const MOST_PARAGRAPHS: usize = 2_000_000;

/// The words of a refusal of a text with more paragraphs than [`MOST_PARAGRAPHS`].
pub(crate) const WORDS_PARAGRAPHS: &str = "a text of more than 2,000,000 paragraphs (a retext of it is not written)";

const WORDS_FIELD: &str = "a text with a tab stop marker or an element of another format the model keeps among its parts";
const WORDS_RUN_CELL: &str = "a run's cell that has no attribute of a text style here (the specification maps Size, Style, DblUnderline, Strikethru, DoubleStrikethrough, Overline, Case, Pos, Color, Letterspace, FontScale and the three fonts)";
const WORDS_PARAGRAPH_CELL: &str = "a paragraph's cell that has no attribute of a paragraph style here (the specification maps HorzAlign, Flags, IndLeft, IndRight, IndFirst, SpBefore, SpAfter, SpLine and a list's bullet)";
const WORDS_FONT_PLACE: &str =
    "a run's font that is a number of the document's table of faces other than 0 (a file holds no such table: a font is a family)";
const WORDS_BULLET_FONT_PLACE: &str =
    "a bullet's font that is a number of the document's table of faces other than 0 (a file holds no such table: a font is a family)";
const WORDS_VALUE: &str = "a run's or a paragraph's cell with a value its attribute cannot hold";
const WORDS_FORMULA: &str = "a run's or a paragraph's cell that holds a formula (the formula is tier 2, which a later version writes)";

/// What a row of the model comes to as a style: the attributes of its properties element, the faces it names, and whether it is a list's.
#[derive(Debug, PartialEq)]
pub(crate) struct Props {
    /// Every cell the row holds, as a text that tells two rows with one set of cells from two with another (`name=value|unit;`, the names in order).
    pub key: String,
    /// The attributes, each on the properties element its `prop` says (1 paragraph, 2 text), a length as `Val::Length` (the page's unit) or text.
    pub ops: Vec<Op>,
    /// The faces a run names: the local name of the attribute (`font-name`, `font-name-asian`, `font-name-complex`) and the family the cell holds.
    pub faces: Vec<(&'static str, String)>,
    /// A paragraph row that makes a list item (its `Bullet` is not 0).
    pub bullet: bool,
    /// What the cells of the bullet say, for the list style of a list the writer makes; `Some` exactly when `bullet`.
    pub list: Option<ListSpec>,
    /// The same attributes and faces, in the groups of cells they come from, in the order of `ops` and `faces`: what a row that changed in some of its cells
    /// writes again, and what it leaves as the file has it (a value no cell maps is kept, and so is each value whose own cells are unchanged).
    pub parts: Vec<Part>,
}

/// The attributes one group of cells of a row comes to: **a group is the cells that feed one set of attributes** (`Size`; `Style`'s bits but the
/// underline's; `Style` bit 4 with `DblUnderline`; `Strikethru` with `DoubleStrikethrough`; `Overline`; `Case`; `Pos`; `Color`; `Letterspace`; `FontScale`; each of the
/// three faces; and for a paragraph each cell on its own). Two rows whose group has the same `sig` write the same attributes of it, and a style keeps the values of the
/// attributes of the group as the file has them while it is so.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Part {
    /// The group (see [`owned`]).
    pub id: &'static str,
    /// The cells of the group, as the value each has (a number of the underline's `Style`: its bit 4 alone, and for the rest of `Style` its other bits).
    pub sig: String,
    pub ops: Vec<Op>,
    /// The faces the group names: the local name of the attribute and the family.
    pub faces: Vec<(&'static str, String)>,
}

/// The attributes a group of cells owns, and so takes out of a style when the group is written again and writes none of them, or is gone from the row (the
/// "regenerate": `style:line-spacing` and `style:line-height-at-least` go with `fo:line-height`, and the face's `fo:font-family` with its `style:font-name`).
pub(crate) fn owned(id: &str) -> &'static [(Ns, &'static str)] {
    match id {
        "Size" => &[(Ns::Fo, "font-size")],
        "Style" => &[
            (Ns::Fo, "font-weight"),
            (Ns::Fo, "font-style"),
            (Ns::Fo, "font-variant"),
            (Ns::Style, "font-weight-complex"),
            (Ns::Style, "font-style-complex"),
        ],
        // The colours and widths of the three lines have no cell and are kept while their cells are unchanged, so a group written again takes them out too.
        "Underline" => &[
            (Ns::Style, "text-underline-style"),
            (Ns::Style, "text-underline-type"),
            (Ns::Style, "text-underline-color"),
            (Ns::Style, "text-underline-width"),
        ],
        "Strike" => &[
            (Ns::Style, "text-line-through-style"),
            (Ns::Style, "text-line-through-type"),
            (Ns::Style, "text-line-through-color"),
            (Ns::Style, "text-line-through-width"),
        ],
        "Overline" => &[(Ns::Style, "text-overline-style"), (Ns::Style, "text-overline-color"), (Ns::Style, "text-overline-width")],
        "Case" => &[(Ns::Fo, "text-transform")],
        "Pos" => &[(Ns::Style, "text-position")],
        "Color" => &[(Ns::Fo, "color")],
        "Letterspace" => &[(Ns::Fo, "letter-spacing")],
        "FontScale" => &[(Ns::Style, "text-scale")],
        "Font" => &[(Ns::Style, "font-name"), (Ns::Fo, "font-family")],
        "AsianFont" => &[(Ns::Style, "font-name-asian"), (Ns::Style, "font-family-asian")],
        "ComplexScriptFont" => &[(Ns::Style, "font-name-complex"), (Ns::Style, "font-family-complex")],
        "HorzAlign" => &[(Ns::Fo, "text-align"), (Ns::Fo, "text-align-last")],
        "Flags" => &[(Ns::Style, "writing-mode")],
        "IndLeft" => &[(Ns::Fo, "margin-left")],
        "IndRight" => &[(Ns::Fo, "margin-right")],
        "IndFirst" => &[(Ns::Fo, "text-indent")],
        "SpBefore" => &[(Ns::Fo, "margin-top")],
        "SpAfter" => &[(Ns::Fo, "margin-bottom")],
        "SpLine" => &[(Ns::Fo, "line-height"), (Ns::Style, "line-spacing"), (Ns::Style, "line-height-at-least")],
        _ => &[],
    }
}

/// What a style made for the row `old` must change to be the style of the row `new` (each value is regenerated by the cell it comes from, and kept
/// while its own cells are unchanged): the attributes and faces of each group of `new` that `old` has not or has with other cells, and the attributes of each group
/// of `old` that `new` has not, taken out. An attribute a changed group owns and does not write is taken out too. Everything else of the style stays as the file has it.
pub(crate) fn changes(old: &Props, new: &Props) -> (Vec<Op>, Vec<(&'static str, String)>) {
    let (mut ops, mut faces): (Vec<Op>, Vec<(&'static str, String)>) = (Vec::new(), Vec::new());
    // The properties element an attribute is on: a paragraph's group has the paragraph's (1), and a run's the text's (2).
    let element = |id: &str| if is_paragraph_group(id) { 1 } else { 2 };
    for part in &new.parts {
        if old.parts.iter().any(|p| p.id == part.id && p.sig == part.sig) {
            continue;
        }
        ops.extend(part.ops.iter().cloned());
        faces.extend(part.faces.iter().cloned());
        for &(ns, local) in owned(part.id) {
            let written = part.ops.iter().any(|o| o.ns == ns && o.local == local) || (ns == Ns::Style && part.faces.iter().any(|(l, _)| *l == local));
            if !written {
                ops.push(Op { prop: element(part.id), ns, local, val: Val::Remove });
            }
        }
    }
    for part in old.parts.iter().filter(|part| !new.parts.iter().any(|p| p.id == part.id)) {
        for &(ns, local) in owned(part.id) {
            ops.push(Op { prop: element(part.id), ns, local, val: Val::Remove });
        }
    }
    (ops, faces)
}

/// The attributes of each group of a row, by the group's name ([`written_groups`]): the qualified name of each and its value.
pub(crate) type WrittenGroups = Vec<(&'static str, Vec<(String, String)>)>;

/// What the cells of a `Character` row write, by group: the attributes of each group as the standard's own qualified names (`fo:font-size`) with their values, in the form the writer says them,
/// and a face (the writer names it by a declaration of the part) as `style:font-name` and the family the cell holds. A group the row has no cell of writes none. `None` for a row whose cells the
/// writer cannot say (the reader compares what a style stated with this, and holds the statement where the two differ).
pub(crate) fn written_groups(row: &Row) -> Option<WrittenGroups> {
    let props = row_props(row, false, &|_| None).ok()??;
    let qname = |ns: Ns, local: &str| match ns {
        Ns::Fo => Some(format!("fo:{local}")),
        Ns::Style => Some(format!("style:{local}")),
        _ => None,
    };
    let mut out: WrittenGroups = Vec::new();
    for part in &props.parts {
        let mut attrs: Vec<(String, String)> = Vec::new();
        for op in &part.ops {
            if let (Val::Text(value), Some(name)) = (&op.val, qname(op.ns, op.local)) {
                attrs.push((name, value.clone()));
            }
        }
        for (local, family) in &part.faces {
            attrs.push((format!("style:{local}"), family.clone()));
        }
        out.push((part.id, attrs));
    }
    Some(out)
}

/// Whether a group is a paragraph row's (its attributes are on `style:paragraph-properties`).
fn is_paragraph_group(id: &str) -> bool {
    matches!(id, "HorzAlign" | "Flags" | "IndLeft" | "IndRight" | "IndFirst" | "SpBefore" | "SpAfter" | "SpLine")
}

/// The cells of a list's row that its automatic list style holds: the bullet, its face and size, and the label's margin and indent. The writer makes a
/// list style of one level from them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListSpec {
    /// The bullet character: the table's for `Bullet` 1 to 7, or `BulletStr`.
    pub bullet: String,
    /// `BulletFont`: a family.
    pub font: Option<String>,
    /// `BulletFontSize`, in inches.
    pub size: Option<f64>,
    /// `IndLeft` and `IndFirst`, in inches.
    pub ind_left: Option<f64>,
    pub ind_first: Option<f64>,
}

/// The bullet characters of `Bullet` 1 to 7 (the reader's table).
const BULLETS: [char; 7] = ['\u{2022}', '\u{25C6}', '\u{25AA}', '\u{25A1}', '\u{2756}', '\u{27A2}', '\u{2713}'];

impl Props {
    /// Whether the row writes any attribute or names any face: a row with nothing to write is no style at all.
    pub(crate) fn writes(&self) -> bool {
        !self.ops.is_empty() || !self.faces.is_empty()
    }

    /// What a row with no cell comes to: nothing to write (a paragraph or a run of the file whose style says nothing a cell reads).
    pub(crate) fn nothing() -> Props {
        Props { key: String::new(), ops: Vec::new(), faces: Vec::new(), bullet: false, list: None, parts: Vec::new() }
    }
}

/// One stretch of a paragraph under one row, in characters: `start..end`.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub start: usize,
    pub end: usize,
    pub props: Option<Arc<Props>>,
}

/// A paragraph of the model: its characters, its paragraph row's style (`None` when the row has no cell) and its runs (empty when it has no run that is
/// styled).
#[derive(Clone, Debug, Default)]
pub(crate) struct View {
    pub chars: String,
    pub pp: Option<Arc<Props>>,
    pub runs: Vec<Run>,
    /// For a paragraph with no character: the `Character` row in force at its end (a marker in it, or the last one before it), which a file cannot hold (the reader
    /// makes no run of no character). Not part of what two paragraphs are the same by: it is what a save that would lose it asks about.
    pub empty: Option<Arc<Props>>,
    /// The fields the model holds in the paragraph (a `fld` that lies wholly in it), in order: where the splice writer writes again the element of a field whose
    /// characters a refresh changed. Not part of what two paragraphs are the same by: their characters are.
    pub fields: Vec<super::zero::FieldAt>,
}

fn key_of(p: &Option<Arc<Props>>) -> &str {
    p.as_deref().map_or("", |p| p.key.as_str())
}

impl View {
    /// Whether two paragraphs are the same to a save: the characters, the cells of the paragraph's row and each run's row.
    pub(crate) fn same(&self, other: &View) -> bool {
        self.pp_same(other) && self.chars == other.chars && self.runs_same(other)
    }

    pub(crate) fn pp_same(&self, other: &View) -> bool {
        key_of(&self.pp) == key_of(&other.pp)
    }

    /// The runs, by their ends and the cells of their rows, the same in both.
    pub(crate) fn runs_same(&self, other: &View) -> bool {
        self.runs.len() == other.runs.len()
            && self.runs.iter().zip(&other.runs).all(|(a, b)| (a.start, a.end) == (b.start, b.end) && key_of(&a.props) == key_of(&b.props))
    }
}

/// The rows of one section of a sheet by `IX`, and what each comes to once asked.
pub(crate) struct Rows<'a> {
    rows: HashMap<u32, &'a Row>,
    done: HashMap<u32, Option<Arc<Props>>>,
    paragraph: bool,
    /// The colour an index of the fixed palette or the document's table names, as `#rrggbb`.
    palette: &'a dyn Fn(i64) -> Option<String>,
}

/// Room for as many paragraphs as a test makes.
#[cfg(test)]
pub(crate) fn all_the_room() -> u64 {
    u64::MAX
}

/// A palette that holds nothing: a colour cell that is not `#rrggbb` is then refused.
#[cfg(test)]
pub(crate) fn no_palette(_: i64) -> Option<String> {
    None
}

impl<'a> Rows<'a> {
    /// The rows of the sheet's `Character` (or, for `paragraph`, `Paragraph`) sections, the first row of each `IX` standing for it.
    pub(crate) fn of(sheet: &'a Sheet, paragraph: bool, palette: &'a dyn Fn(i64) -> Option<String>, steps: &mut u64) -> Rows<'a> {
        let name = if paragraph { "Paragraph" } else { "Character" };
        let mut rows: HashMap<u32, &'a Row> = HashMap::new();
        for section in sheet.sections.iter().filter(|s| s.name.as_str() == name) {
            for row in section.rows.iter() {
                *steps = steps.saturating_add(1);
                if let Some(ix) = row.ix {
                    rows.entry(ix).or_insert(row);
                }
            }
        }
        Rows { rows, done: HashMap::new(), paragraph, palette }
    }

    /// What row `ix` comes to: `None` for a row the sheet does not have or one with no cell, an error in words for a cell the writer cannot write.
    pub(crate) fn props(&mut self, ix: u32) -> Result<Option<Arc<Props>>, &'static str> {
        if let Some(done) = self.done.get(&ix) {
            return Ok(done.clone());
        }
        let made = match self.rows.get(&ix) {
            Some(row) => row_props(row, self.paragraph, self.palette)?,
            None => None,
        };
        self.done.insert(ix, made.clone());
        Ok(made)
    }
}

/// **The least font size the writer writes**, in inches: a tenth of a point. ODF 1.4 takes a positive font size only (1.0 and 1.1 took any length), so a size of 0, which a file of an earlier version
/// may hold, is written as this, and the 0 rides in DiaCraft's data. Text of either size is too small to see.
pub(crate) const LEAST_FONT_SIZE: f64 = 0.1 / 72.0;

/// The font size written for a `Size` of `inches` (not negative): itself, or [`LEAST_FONT_SIZE`] for 0.
pub(crate) fn written_size(inches: f64) -> f64 {
    if inches > 0.0 { inches } else { LEAST_FONT_SIZE }
}

/// Whether `section` is a `Character` section with a row whose `Size` is 0, which the writer writes as [`LEAST_FONT_SIZE`].
pub(crate) fn zero_size_in(section: &diacraft_doc::Section) -> bool {
    section.name.as_str() == "Character"
        && section
            .rows
            .iter()
            .any(|row| row.cells.get("Size").and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number).is_some_and(|n| n == 0.0))
}

/// Whether a `Character` row of `sheet` has a `Size` of 0 ([`zero_size_in`]).
pub(crate) fn holds_zero_size(sheet: &diacraft_doc::Sheet) -> bool {
    sheet.sections.iter().any(|s| zero_size_in(s))
}

/// `Op` of one attribute of a text style (`prop` 2) or a paragraph style (`prop` 1).
fn op(prop: usize, ns: Ns, local: &'static str, val: Val) -> Op {
    Op { prop, ns, local, val }
}

fn text(prop: usize, ns: Ns, local: &'static str, value: &str) -> Op {
    op(prop, ns, local, Val::Text(value.to_owned()))
}

/// A cell's value as the writer takes it: a number or text, never a formula.
fn value(cell: &diacraft_doc::Cell) -> Result<CellVal, &'static str> {
    if matches!(cell.formula, diacraft_doc::FormulaSlot::Local(_) | diacraft_doc::FormulaSlot::Inherit) {
        return Err(WORDS_FORMULA);
    }
    CellVal::of(cell).map_err(|_| WORDS_VALUE)
}

/// What the cells of one row come to (see the module documentation).
fn row_props(row: &Row, paragraph: bool, palette: &dyn Fn(i64) -> Option<String>) -> Result<Option<Arc<Props>>, &'static str> {
    let mut key = String::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut bullet = false;
    let mut cells: Vec<(&str, CellVal)> = Vec::new();
    for (name, cell) in &row.cells {
        let v = value(cell)?;
        key.push_str(&format!("{}={}|{};", name.as_str(), v.text, v.unit.as_deref().unwrap_or("")));
        cells.push((name.as_str(), v));
    }
    if cells.is_empty() {
        return Ok(None);
    }
    let get = |n: &str| cells.iter().find(|(name, _)| *name == n).map(|(_, v)| v);
    let number = |n: &str| -> Result<Option<f64>, &'static str> {
        match get(n) {
            Some(v) => v.number.filter(|x| x.is_finite()).map(Some).ok_or(WORDS_VALUE),
            None => Ok(None),
        }
    };
    // What a cell is, for the groups that stand on one cell: its value and unit.
    let sig_of = |v: &CellVal| format!("{}|{}", v.text, v.unit.as_deref().unwrap_or(""));
    let part = |id: &'static str, sig: String, ops: Vec<Op>| Part { id, sig, ops, faces: Vec::new() };
    if paragraph {
        let is_bullet = number("Bullet")?.is_some_and(|b| b != 0.0);
        bullet = is_bullet;
        let list = if is_bullet { Some(list_spec(&cells)?) } else { None };
        for (name, v) in &cells {
            let n = v.number;
            match *name {
                "HorzAlign" => {
                    let (align, last) = match n {
                        Some(0.0) => ("left", false),
                        Some(1.0) => ("center", false),
                        Some(2.0) => ("right", false),
                        Some(3.0) => ("justify", false),
                        Some(4.0) => ("justify", true),
                        _ => return Err(WORDS_VALUE),
                    };
                    let mut ops = vec![text(1, Ns::Fo, "text-align", align)];
                    if last {
                        ops.push(text(1, Ns::Fo, "text-align-last", "justify"));
                    }
                    // The alignment is fed by `HorzAlign` and by `Flags` (`start` and `end` swap with the writing mode), so either cell writes it again.
                    parts.push(part("HorzAlign", format!("{}#{}", sig_of(v), get("Flags").map(sig_of).unwrap_or_default()), ops));
                }
                "Flags" => match n {
                    Some(0.0) => parts.push(part("Flags", sig_of(v), vec![text(1, Ns::Style, "writing-mode", "lr-tb")])),
                    Some(1.0) => parts.push(part("Flags", sig_of(v), vec![text(1, Ns::Style, "writing-mode", "rl-tb")])),
                    _ => return Err(WORDS_VALUE),
                },
                "IndLeft" | "IndFirst" if is_bullet => {
                    parts.push(part(if *name == "IndLeft" { "IndLeft" } else { "IndFirst" }, sig_of(v), Vec::new()))
                }
                "IndLeft" | "IndRight" | "IndFirst" | "SpBefore" | "SpAfter" => {
                    let (id, local) = match *name {
                        "IndLeft" => ("IndLeft", "margin-left"),
                        "IndRight" => ("IndRight", "margin-right"),
                        "IndFirst" => ("IndFirst", "text-indent"),
                        "SpBefore" => ("SpBefore", "margin-top"),
                        _ => ("SpAfter", "margin-bottom"),
                    };
                    parts.push(part(id, sig_of(v), vec![op(1, Ns::Fo, local, Val::Length(n.filter(|x| x.is_finite()).ok_or(WORDS_VALUE)?))]));
                }
                "SpLine" => {
                    let v_line = n.filter(|x| x.is_finite()).ok_or(WORDS_VALUE)?;
                    let write = if v_line < 0.0 {
                        let percent = ((-v_line) * 100.0 * 1e9).round() / 1e9;
                        text(1, Ns::Fo, "line-height", &format!("{percent}%"))
                    } else if v_line == 0.0 {
                        text(1, Ns::Fo, "line-height", "normal")
                    } else {
                        op(1, Ns::Fo, "line-height", Val::Length(v_line))
                    };
                    parts.push(part("SpLine", sig_of(v), vec![write]));
                }
                "Bullet" | "BulletStr" | "BulletFont" | "BulletFontSize" => {}
                _ => return Err(WORDS_PARAGRAPH_CELL),
            }
        }
        return Ok(Some(finish(key, parts, bullet, list)));
    }
    for (name, v) in &cells {
        let n = v.number;
        match *name {
            "Size" => {
                let inches = n.filter(|x| x.is_finite() && *x >= 0.0).ok_or(WORDS_VALUE)?;
                parts.push(part(
                    "Size",
                    sig_of(v),
                    vec![text(2, Ns::Fo, "font-size", &length_text(written_size(inches), LenUnit::Pt).ok_or(WORDS_VALUE)?)],
                ));
            }
            "Style" => {
                let bits = n.filter(|x| x.is_finite() && x.fract() == 0.0 && *x >= 0.0 && *x <= 63.0).ok_or(WORDS_VALUE)? as u32;
                let on = |bit: u32| bits & bit != 0;
                // The underline (bit 4) is the underline's group, so that a bold or an italic edit leaves a held underline as it is.
                let ops = vec![
                    text(2, Ns::Fo, "font-weight", if on(1) { "bold" } else { "normal" }),
                    text(2, Ns::Fo, "font-style", if on(2) { "italic" } else { "normal" }),
                    text(2, Ns::Fo, "font-variant", if on(8) { "small-caps" } else { "normal" }),
                    text(2, Ns::Style, "font-weight-complex", if on(16) { "bold" } else { "normal" }),
                    text(2, Ns::Style, "font-style-complex", if on(32) { "italic" } else { "normal" }),
                ];
                parts.push(part("Style", format!("{}|{}", bits & !4, v.unit.as_deref().unwrap_or("")), ops));
            }
            "DblUnderline" | "Strikethru" | "DoubleStrikethrough" | "Overline" => {
                if !n.is_some_and(|x| x == 0.0 || x == 1.0) {
                    return Err(WORDS_VALUE);
                }
            }
            "Case" => {
                let word = match n {
                    Some(0.0) => "none",
                    Some(1.0) => "uppercase",
                    Some(2.0) => "capitalize",
                    _ => return Err(WORDS_VALUE),
                };
                parts.push(part("Case", sig_of(v), vec![text(2, Ns::Fo, "text-transform", word)]));
            }
            "Pos" => {
                let word = match n {
                    Some(0.0) => "0%",
                    Some(1.0) => "super",
                    Some(2.0) => "sub",
                    _ => return Err(WORDS_VALUE),
                };
                parts.push(part("Pos", sig_of(v), vec![text(2, Ns::Style, "text-position", word)]));
            }
            "Color" => {
                let t = v.text.as_str();
                // An index of the fixed palette or the document's table is written as the colour it names.
                let named = || n.filter(|x| x.fract() == 0.0 && x.abs() < 1e9).and_then(|x| palette(x as i64));
                let hex = if is_colour(t) { Some(t.to_owned()) } else { named() };
                // What the palette names is held to the schema's form too (`#rrggbb`): a colour with no hash is no colour in the file.
                let Some(hex) = hex.filter(|h| is_colour(h)) else {
                    return Err("a run's colour that is neither #rrggbb nor an index of the palette the document holds");
                };
                parts.push(part("Color", sig_of(v), vec![text(2, Ns::Fo, "color", &hex.to_ascii_lowercase())]));
            }
            "Letterspace" => {
                let inches = n.filter(|x| x.is_finite()).ok_or(WORDS_VALUE)?;
                let write =
                    if inches == 0.0 { text(2, Ns::Fo, "letter-spacing", "normal") } else { op(2, Ns::Fo, "letter-spacing", Val::Length(inches)) };
                parts.push(part("Letterspace", sig_of(v), vec![write]));
            }
            "FontScale" => {
                let share = n.filter(|x| x.is_finite() && *x > 0.0).ok_or(WORDS_VALUE)?;
                parts.push(part(
                    "FontScale",
                    sig_of(v),
                    vec![text(2, Ns::Style, "text-scale", &format!("{}%", (share * 100.0 * 1e9).round() / 1e9))],
                ));
            }
            "Font" | "AsianFont" | "ComplexScriptFont" => {
                let (id, local) = match *name {
                    "Font" => ("Font", "font-name"),
                    "AsianFont" => ("AsianFont", "font-name-asian"),
                    _ => ("ComplexScriptFont", "font-name-complex"),
                };
                // A number is a place in the document's table of faces, as the engine copies the default one into the rows it makes (0: the document's own
                // font, which a text style names no face for); a place past it has no table in an `.odg` to be looked up in.
                let mut group = part(id, sig_of(v), Vec::new());
                match n {
                    Some(0.0) => {}
                    Some(_) => return Err(WORDS_FONT_PLACE),
                    None => group.faces.push((local, v.text.clone())),
                }
                parts.push(group);
            }
            _ => return Err(WORDS_RUN_CELL),
        }
    }
    // The underline, the strike-through and the overline are each written from their cells together: single, double, or none.
    let flag = |n: &str| number(n).ok().flatten().map(|x| x != 0.0);
    let style_bits = number("Style").ok().flatten().map(|x| x as u32);
    let sig2 = |a: &str, b: &str| format!("{}#{}", get(a).map(sig_of).unwrap_or_default(), get(b).map(sig_of).unwrap_or_default());
    if style_bits.is_some() || flag("DblUnderline").is_some() {
        let double = flag("DblUnderline").unwrap_or(false);
        let single = style_bits.is_some_and(|b| b & 4 != 0);
        let ops = if double {
            vec![text(2, Ns::Style, "text-underline-style", "solid"), text(2, Ns::Style, "text-underline-type", "double")]
        } else if single {
            vec![text(2, Ns::Style, "text-underline-style", "solid"), text(2, Ns::Style, "text-underline-type", "single")]
        } else {
            vec![text(2, Ns::Style, "text-underline-style", "none")]
        };
        // Its cells are the underline's bit of `Style` and `DblUnderline`: a bold edit leaves a held wave underline as the file has it.
        let bit = style_bits.map_or(String::new(), |b| (b & 4).to_string());
        parts.push(part("Underline", format!("{bit}#{}", get("DblUnderline").map(sig_of).unwrap_or_default()), ops));
    }
    if flag("Strikethru").is_some() || flag("DoubleStrikethrough").is_some() {
        let ops = if flag("DoubleStrikethrough").unwrap_or(false) {
            vec![text(2, Ns::Style, "text-line-through-style", "solid"), text(2, Ns::Style, "text-line-through-type", "double")]
        } else if flag("Strikethru").unwrap_or(false) {
            vec![text(2, Ns::Style, "text-line-through-style", "solid"), text(2, Ns::Style, "text-line-through-type", "single")]
        } else {
            vec![text(2, Ns::Style, "text-line-through-style", "none")]
        };
        parts.push(part("Strike", sig2("Strikethru", "DoubleStrikethrough"), ops));
    }
    if let Some(on) = flag("Overline") {
        parts.push(part(
            "Overline",
            get("Overline").map(sig_of).unwrap_or_default(),
            vec![text(2, Ns::Style, "text-overline-style", if on { "solid" } else { "none" })],
        ));
    }
    Ok(Some(finish(key, parts, bullet, None)))
}

/// A row's `Props`: the attributes and faces of its groups, in the order of the groups.
fn finish(key: String, parts: Vec<Part>, bullet: bool, list: Option<ListSpec>) -> Arc<Props> {
    let ops = parts.iter().flat_map(|p| p.ops.iter().cloned()).collect();
    let faces = parts.iter().flat_map(|p| p.faces.iter().cloned()).collect();
    Arc::new(Props { key, ops, faces, bullet, list, parts })
}

/// The cells of a list's row as the bullet its list style holds: the character, the face, the size, the margin and the indent.
fn list_spec(cells: &[(&str, CellVal)]) -> Result<ListSpec, &'static str> {
    let get = |n: &str| cells.iter().find(|(name, _)| *name == n).map(|(_, v)| v);
    let number = |n: &str| -> Result<Option<f64>, &'static str> {
        match get(n) {
            Some(v) => v.number.filter(|x| x.is_finite()).map(Some).ok_or(WORDS_VALUE),
            None => Ok(None),
        }
    };
    let kind = number("Bullet")?.ok_or(WORDS_VALUE)?;
    let from_table = || -> Result<String, &'static str> {
        let at = kind.is_finite() && kind.fract() == 0.0 && (1.0..=7.0).contains(&kind);
        let found = if at { BULLETS.get(kind as usize - 1) } else { None };
        found.map(char::to_string).ok_or(WORDS_VALUE)
    };
    // A character of the file's own is `BulletStr`, with `Bullet` 1.
    let bullet = match get("BulletStr") {
        Some(v) if !v.text.is_empty() => v.text.clone(),
        _ => from_table()?,
    };
    // A face is a family; the number 0 (as the engine's rows have it), the text 0 and nothing are the document's own face, which a list style names none for.
    let font = match get("BulletFont") {
        None => None,
        Some(v) => match v.number {
            Some(0.0) => None,
            Some(_) => return Err(WORDS_BULLET_FONT_PLACE),
            None => Some(v.text.clone()).filter(|f| !f.trim().is_empty() && f.trim() != "0"),
        },
    };
    let size = number("BulletFontSize")?;
    if size.is_some_and(|s| s < 0.0) {
        return Err(WORDS_VALUE);
    }
    Ok(ListSpec { bullet, font, size, ind_left: number("IndLeft")?, ind_first: number("IndFirst")? })
}

/// The rows of the `Character` and `Paragraph` sections of a sheet, each as the cells it holds (the `IX` is no part of it): `(is a paragraph's, cells)`. A row with
/// no cell is no row. Counted a step each.
pub(crate) fn row_keys(sheet: &Sheet, steps: &mut u64) -> std::collections::HashSet<(bool, String)> {
    let mut out = std::collections::HashSet::new();
    for section in sheet.sections.iter() {
        let paragraph = match section.name.as_str() {
            "Character" => false,
            "Paragraph" => true,
            _ => continue,
        };
        for row in section.rows.iter() {
            *steps = steps.saturating_add(1);
            if row.cells.is_empty() {
                continue;
            }
            let mut key = String::new();
            for (name, cell) in &row.cells {
                match CellVal::of(cell) {
                    Ok(v) => key.push_str(&format!("{}={}|{};", name.as_str(), v.text, v.unit.as_deref().unwrap_or(""))),
                    Err(_) => key.push_str(&format!("{}=?;", name.as_str())),
                }
            }
            out.insert((paragraph, key));
        }
    }
    out
}

/// The paragraphs of a text with the rows of each, for a model whose rows are `characters` and `paragraphs`: `Err` in words for a text the writer does not
/// write (a tab stop marker, an element of another format it keeps among its parts, an unwritable character, a carriage return, or more than the caps), `steps` counting what is walked. **A field's
/// shown text is among the paragraph's characters**.
pub(crate) fn views(
    text: Option<&Arc<Text>>,
    characters: &mut Rows<'_>,
    paragraphs: &mut Rows<'_>,
    steps: &mut u64,
) -> Result<Vec<View>, &'static str> {
    let mut out: Vec<View> = Vec::new();
    let Some(text) = text else {
        out.push(View::default());
        return Ok(out);
    };
    // The size is told before any of it is copied.
    let bytes: usize = text
        .parts
        .iter()
        .map(|p| if let TextPart::Chars(c) | TextPart::Fld { text: c, .. } = p { c.len() } else { 0 })
        .fold(0usize, usize::saturating_add);
    if bytes > super::text::MOST_EDITED_TEXT {
        return Err("a text of more than 16 MiB (a retext of it is not written)");
    }
    for part in &text.parts {
        *steps = steps.saturating_add(1);
        match part {
            // A field's shown text is among the paragraph's characters, and its element stays where the file has it, as any field's.
            TextPart::Chars(c) | TextPart::Fld { text: c, .. } => {
                if super::splice::has_bad_char(c) {
                    return Err(super::splice::WORDS_XML_CHAR);
                }
                if c.contains('\r') {
                    return Err(super::splice::WORDS_CARRIAGE_RETURN);
                }
            }
            // A node the reader kept whole (a bookmark) is zero-width and no part of the paragraph's characters.
            TextPart::Cp(_) | TextPart::Pp(_) | TextPart::OdfRaw(_) => {}
            _ => return Err(WORDS_FIELD),
        }
    }
    // The row in force before the first marker is row 0.
    let mut pp = 0u32;
    // The paragraph being built: its characters, its runs so far and where the run in force began.
    let mut current = View::default();
    let mut at = 0usize;
    let mut run_start = 0usize;
    let mut run_row = 0u32;
    let (rows_c, rows_p) = (characters, paragraphs);
    // `close` ends the run in force at `at` and the paragraph with it when `last`.
    let mut closed: Vec<(usize, usize, u32)> = Vec::new();
    let finish = |current: &mut View,
                  closed: &mut Vec<(usize, usize, u32)>,
                  rows_c: &mut Rows<'_>,
                  rows_p: &mut Rows<'_>,
                  (pp, run_row): (u32, u32),
                  out: &mut Vec<View>,
                  steps: &mut u64|
     -> Result<(), &'static str> {
        if out.len() >= MOST_PARAGRAPHS {
            return Err(WORDS_PARAGRAPHS);
        }
        *steps = steps.saturating_add(closed.len() as u64 + 1);
        let mut view = std::mem::take(current);
        view.pp = rows_p.props(pp)?;
        if view.chars.is_empty() {
            view.empty = rows_c.props(run_row)?;
        }
        let mut runs: Vec<Run> = Vec::new();
        for (start, end, row) in closed.drain(..) {
            let props = rows_c.props(row)?;
            match runs.last_mut() {
                Some(last) if key_of(&last.props) == key_of(&props) && last.end == start => last.end = end,
                _ => runs.push(Run { start, end, props }),
            }
        }
        if runs.iter().any(|r| r.props.is_some()) {
            view.runs = runs;
        }
        out.try_reserve(1).map_err(|_| "there is not the memory for the paragraphs of a retext")?;
        out.push(view);
        Ok(())
    };
    for part in &text.parts {
        // A field that lies wholly in one paragraph is listed with it: where its characters are.
        if let TextPart::Fld { text: shown, .. } = part
            && !shown.is_empty()
            && !shown.contains('\n')
        {
            current.fields.try_reserve(1).map_err(|_| "there is not the memory for the paragraphs of a retext")?;
            let len = shown.chars().count();
            let from = current.chars.len();
            current.fields.push(super::zero::FieldAt { at, len, bytes: (from, from.saturating_add(shown.len())) });
        }
        match part {
            TextPart::Chars(chars) | TextPart::Fld { text: chars, .. } => {
                for (i, piece) in chars.split('\n').enumerate() {
                    if i > 0 {
                        if at > run_start {
                            closed.push((run_start, at, run_row));
                        }
                        finish(&mut current, &mut closed, rows_c, rows_p, (pp, run_row), &mut out, steps)?;
                        at = 0;
                        run_start = 0;
                    }
                    current.chars.push_str(piece);
                    at += piece.chars().count();
                }
            }
            TextPart::Cp(ix) => {
                if at > run_start {
                    closed.push((run_start, at, run_row));
                }
                run_start = at;
                run_row = *ix;
            }
            TextPart::Pp(ix) => pp = *ix,
            _ => {}
        }
    }
    if at > run_start {
        closed.push((run_start, at, run_row));
    }
    finish(&mut current, &mut closed, rows_c, rows_p, (pp, run_row), &mut out, steps)?;
    Ok(out)
}

/// What is to be done with the paragraphs of the file to make them the model's: `old` and `new` are places in the lists the plan was made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The file's paragraph is the model's: its bytes stay.
    Keep { old: usize, new: usize },
    /// The file's paragraph becomes the model's.
    Edit { old: usize, new: usize },
    /// The model has a paragraph the file lacks.
    Insert { new: usize },
    /// The file has a paragraph the model lacks.
    Remove { old: usize },
}

/// The most bits of a hash the plan's index of buckets is made from: 4,194,304 buckets, 32 MB.
const MOST_BUCKET_BITS: u32 = 22;

/// The bits of a hash the buckets of a list of `places` places are made from: a bucket for about each place (the number of bits it takes to write the count), one at the
/// least and [`MOST_BUCKET_BITS`] at the most.
fn bucket_bits(places: usize) -> u32 {
    (usize::BITS - places.max(1).leading_zeros()).clamp(1, MOST_BUCKET_BITS)
}

/// The plan that makes the paragraphs `old` the paragraphs `new` (see the module documentation), `steps` counting what it walks: each paragraph of each list
/// once, and each look in the list of the file's paragraphs by their characters.
///
/// **The file's paragraphs of the middle are found by their characters in one flat sorted list** of (a hash of the characters, the place), which is two words for each
/// paragraph and one allocation: a map of lists of places made an allocation for each paragraph, which at the 2,000,000 paragraphs a shape may hold took more than ten times
/// what a pass over the lists takes. A look is a search of the one bucket of the list that the top bits of the hash say (an index of where each
/// bucket starts, about one place to a bucket), and not of the whole list, which at that size is no longer in the processor's cache. The hash is keyed at random for each
/// plan, so that a file cannot be made to put many texts under one; texts that share a hash are told apart by their characters, and the plan is exactly what the map gave
/// (`plan_agrees`).
pub(crate) fn plan(old: &[View], new: &[View], steps: &mut u64) -> Vec<Step> {
    plan_with(&RandomState::new(), old, new, steps)
}

/// [`plan`] with the hash the file's paragraphs are found by given, so that a hash that puts many texts in one place is tried (`plan_agrees`).
fn plan_with<S: BuildHasher>(hasher: &S, old: &[View], new: &[View], steps: &mut u64) -> Vec<Step> {
    plan_counted(hasher, old, new, steps, &mut 0)
}

/// [`plan_with`], with `looked` counting the entries of the sorted list that the looks for a paragraph's characters **go over**: a look goes over the entry it
/// stops at, and one more for each entry it passes (a place of another text with the same hash, or the place already taken). With a hash keyed at random, a look goes over
/// two entries at the most, so the count is linear in the middle; a look that did not stop at the end of its hash's entries goes over every later entry, and
/// the plan is quadratic in the paragraphs with the same result, which the steps (a step for each look) and the plan cannot show (`plan_agrees`).
fn plan_counted<S: BuildHasher>(hasher: &S, old: &[View], new: &[View], steps: &mut u64, looked: &mut u64) -> Vec<Step> {
    *steps = steps.saturating_add((old.len() + new.len()) as u64);
    let mut out: Vec<Step> = Vec::with_capacity(old.len().max(new.len()));
    if old.len() == new.len() {
        for (k, (a, b)) in old.iter().zip(new).enumerate() {
            out.push(if a.same(b) { Step::Keep { old: k, new: k } } else { Step::Edit { old: k, new: k } });
        }
        return out;
    }
    // The paragraphs alike at the head and at the tail are kept.
    let head = old.iter().zip(new).take_while(|(a, b)| a.same(b)).count();
    let tail = old[head..].iter().rev().zip(new[head..].iter().rev()).take_while(|(a, b)| a.same(b)).count();
    for k in 0..head {
        out.push(Step::Keep { old: k, new: k });
    }
    let (old_mid, new_mid) = (head..old.len() - tail, head..new.len() - tail);
    // In the middle, each paragraph of the model that is like a later paragraph of the file than the one matched before it is kept: by the characters first (the
    // file's paragraphs by a hash of their characters and then their place, so that the places of one text are together and in order), and the whole of a paragraph after.
    let mut places: Vec<(u64, usize)> = old_mid.clone().map(|k| (hasher.hash_one(old[k].chars.as_str()), k)).collect();
    places.sort_unstable();
    // A bucket for about each place (at most 2^22): the top bits of the hash, and where in the sorted list the places of each bucket start.
    let bits = bucket_bits(places.len());
    let bucket_of = |hash: u64| usize::try_from(hash >> (u64::BITS - bits)).unwrap_or(0);
    let mut starts: Vec<usize> = vec![0; (1usize << bits) + 1];
    for (hash, _) in &places {
        if let Some(count) = starts.get_mut(bucket_of(*hash) + 1) {
            *count += 1;
        }
    }
    let mut sum = 0usize;
    for start in &mut starts {
        sum += *start;
        *start = sum;
    }
    let mut matched: Vec<Option<usize>> = vec![None; new_mid.len()];
    let mut floor = old_mid.start;
    // The place matched last, which is the floor: a later match is no earlier than it, and it is not matched twice.
    let mut taken: Option<usize> = None;
    for (slot, j) in new_mid.clone().enumerate() {
        *steps = steps.saturating_add(1);
        let text = new[j].chars.as_str();
        let hash = hasher.hash_one(text);
        // The first place at or after the floor that has this text: the places of one hash that have another text (a collision) are passed over, and the place taken is not
        // looked at again.
        let bucket = bucket_of(hash);
        let (from, to) = (starts.get(bucket).copied().unwrap_or(0), starts.get(bucket + 1).copied().unwrap_or(0));
        let mut at = from + places.get(from..to).unwrap_or_default().partition_point(|entry| *entry < (hash, floor));
        let found = loop {
            let Some(&(held, k)) = places.get(at) else { break None };
            *looked = looked.saturating_add(1);
            if held != hash {
                break None;
            }
            if taken != Some(k) && old[k].chars == text {
                break Some(k);
            }
            at += 1;
        };
        if let Some(k) = found
            && old[k].same(&new[j])
        {
            matched[slot] = Some(k);
            floor = k;
            taken = Some(k);
        }
    }
    // The gaps between two matches pair by place; what is left over is inserted or taken out.
    fn flush(out: &mut Vec<Step>, old_from: usize, until_old: usize, new_from: usize, new_to: usize) {
        let (mut o, mut n) = (old_from, new_from);
        while o < until_old && n < new_to {
            out.push(Step::Edit { old: o, new: n });
            o += 1;
            n += 1;
        }
        while o < until_old {
            out.push(Step::Remove { old: o });
            o += 1;
        }
        while n < new_to {
            out.push(Step::Insert { new: n });
            n += 1;
        }
    }
    let mut next_old = old_mid.start;
    let mut gap_from = new_mid.start;
    for (slot, j) in new_mid.clone().enumerate() {
        if let Some(k) = matched[slot] {
            flush(&mut out, next_old, k, gap_from, j);
            out.push(Step::Keep { old: k, new: j });
            next_old = k + 1;
            gap_from = j + 1;
        }
    }
    flush(&mut out, next_old, old_mid.end, gap_from, new_mid.end);
    for k in 0..tail {
        out.push(Step::Keep { old: old.len() - tail + k, new: new.len() - tail + k });
    }
    out
}

#[cfg(test)]
mod held_paragraphs;
pub(crate) mod model;
#[cfg(test)]
mod plan_agrees;
#[cfg(test)]
mod row_key_agrees;
#[cfg(test)]
mod text_stamp_agrees;

#[cfg(test)]
mod tests {
    use super::*;

    fn view(chars: &str) -> View {
        View { chars: chars.to_owned(), ..View::default() }
    }

    fn lists(old: &[&str], new: &[&str]) -> Vec<Step> {
        let (a, b): (Vec<View>, Vec<View>) = (old.iter().map(|s| view(s)).collect(), new.iter().map(|s| view(s)).collect());
        plan(&a, &b, &mut 0)
    }

    #[test]
    fn paragraphs_of_one_count_pair_by_place_and_the_alike_are_kept() {
        assert_eq!(lists(&["a", "b"], &["a", "c"]), [Step::Keep { old: 0, new: 0 }, Step::Edit { old: 1, new: 1 }]);
        assert_eq!(lists(&["a"], &["a"]), [Step::Keep { old: 0, new: 0 }]);
    }

    #[test]
    fn a_paragraph_added_or_taken_away_leaves_the_others_kept() {
        // One added in the middle: the head and the tail are kept and the new one inserted.
        assert_eq!(lists(&["a", "c"], &["a", "b", "c"]), [Step::Keep { old: 0, new: 0 }, Step::Insert { new: 1 }, Step::Keep { old: 1, new: 2 }]);
        // One taken away.
        assert_eq!(lists(&["a", "b", "c"], &["a", "c"]), [Step::Keep { old: 0, new: 0 }, Step::Remove { old: 1 }, Step::Keep { old: 2, new: 1 }]);
        // One more at the end, and at the start.
        assert_eq!(lists(&["a"], &["a", "b"]), [Step::Keep { old: 0, new: 0 }, Step::Insert { new: 1 }]);
        assert_eq!(lists(&["a"], &["b", "a"]), [Step::Insert { new: 0 }, Step::Keep { old: 0, new: 1 }]);
        // A paragraph split in two: the first of the two takes the old one's place, the other is inserted.
        let split = lists(&["ab", "x"], &["a", "b", "x"]);
        assert_eq!(split, [Step::Edit { old: 0, new: 0 }, Step::Insert { new: 1 }, Step::Keep { old: 1, new: 2 }]);
        // Two paragraphs joined.
        let joined = lists(&["a", "b", "x"], &["ab", "x"]);
        assert_eq!(joined, [Step::Edit { old: 0, new: 0 }, Step::Remove { old: 1 }, Step::Keep { old: 2, new: 1 }]);
    }

    /// What follows a paragraph put in is never an edited paragraph: the paragraphs of a gap are paired by place first and what is left over is inserted, so the next
    /// paragraph of the file that a step keeps or edits after an insertion is a kept one. `text`'s mark of the paragraphs beside an insertion (`near_inserted`, which keeps a
    /// style from being edited where it stands under a paragraph put in beside it) is therefore read for the paragraph before an insertion only: the one after it is never
    /// rewritten, and no hand deletion of its mark can show. Checked for every pair of lists of up to four paragraphs of three kinds; if the plan ever puts an insertion
    /// before an edit, this fails, and the case of that mark for the paragraph after needs its own test.
    #[test]
    fn a_paragraph_put_in_is_never_followed_by_an_edited_one() {
        let alphabet = ["a", "b", "c"];
        let mut all: Vec<Vec<&str>> = vec![Vec::new()];
        let mut from = 0;
        for _ in 0..4 {
            let to = all.len();
            for k in from..to {
                for letter in alphabet {
                    let mut longer = all[k].clone();
                    longer.push(letter);
                    all.push(longer);
                }
            }
            from = to;
        }
        assert_eq!(all.len(), 121, "every list of up to four of three kinds");
        let mut insertions = 0usize;
        for old in &all {
            for new in &all {
                let steps = lists(old, new);
                for (i, step) in steps.iter().enumerate() {
                    if matches!(step, Step::Insert { .. }) {
                        insertions += 1;
                        let next = steps[i + 1..].iter().find(|s| matches!(s, Step::Keep { .. } | Step::Edit { .. }));
                        assert!(!matches!(next, Some(Step::Edit { .. })), "{old:?} -> {new:?}: {steps:?}");
                    }
                }
            }
        }
        assert!(insertions > 1_000, "the check met many insertions: {insertions}");
    }

    #[test]
    fn an_insertion_with_an_edit_elsewhere_keeps_the_paragraphs_between() {
        // `b` retyped and one inserted after `d`: the middle holds `b c d e` against `B c d X e`, and `c`, `d` are kept by their characters.
        let steps = lists(&["a", "b", "c", "d", "e"], &["a", "B", "c", "d", "X", "e"]);
        assert!(steps.contains(&Step::Keep { old: 2, new: 2 }) && steps.contains(&Step::Keep { old: 3, new: 3 }), "{steps:?}");
        assert!(steps.contains(&Step::Edit { old: 1, new: 1 }) && steps.contains(&Step::Insert { new: 4 }), "{steps:?}");
        // Every paragraph of each list is in the plan once, in order.
        let (mut olds, mut news) = (Vec::new(), Vec::new());
        for s in &steps {
            match *s {
                Step::Keep { old, new } | Step::Edit { old, new } => {
                    olds.push(old);
                    news.push(new);
                }
                Step::Insert { new } => news.push(new),
                Step::Remove { old } => olds.push(old),
            }
        }
        assert_eq!(olds, [0, 1, 2, 3, 4]);
        assert_eq!(news, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_plan_is_linear_in_the_paragraphs() {
        let make = |n: usize| -> (Vec<View>, Vec<View>) {
            let old: Vec<View> = (0..n).map(|k| view(&format!("p{k}"))).collect();
            let mut new: Vec<View> = (0..n).map(|k| view(&format!("p{k}"))).collect();
            new.insert(n / 2, view("inserted"));
            new.remove(n / 4);
            new.insert(1, view("another"));
            (old, new)
        };
        let mut counted = Vec::new();
        for n in [10_000usize, 40_000] {
            let (old, new) = make(n);
            let mut steps = 0u64;
            let plan = plan(&old, &new, &mut steps);
            assert!(plan.len() >= n);
            counted.push(steps);
        }
        assert!(counted[1] <= counted[0] * 4 + 16, "{counted:?}");
    }

    use diacraft_doc::{Cell, CellName, Section, Value};

    fn number(x: f64) -> Cell {
        Cell::constant(Value::from_number(x).unwrap())
    }

    fn words(s: &str) -> Cell {
        Cell::constant(Value::from_text(s))
    }

    fn row(cells: &[(&str, Cell)]) -> Row {
        let mut row = Row { ix: Some(0), ..Row::default() };
        for (name, cell) in cells {
            row.cells.insert(CellName::new(name), cell.clone());
        }
        row
    }

    /// The colour of index 3, and nothing else.
    fn palette(index: i64) -> Option<String> {
        (index == 3).then(|| "#00FF00".to_owned())
    }

    /// A palette that has every index (so that only the writer's own check of an index is tested).
    fn any_index(_: i64) -> Option<String> {
        Some("#112233".to_owned())
    }

    fn shown(props: &Props) -> Vec<String> {
        props
            .ops
            .iter()
            .map(|o| {
                let value = match &o.val {
                    Val::Text(t) => t.clone(),
                    Val::Length(x) => format!("{x}in"),
                    Val::Remove => "-".to_owned(),
                };
                format!("{:?}.{}={value}", o.ns, o.local)
            })
            .collect()
    }

    /// What a row of a paragraph (`paragraph`) or a run makes of the cells, as `Ns.attribute=value` in the order of the cells, or the refusal.
    fn made(paragraph: bool, cells: &[(&str, Cell)]) -> Result<Vec<String>, &'static str> {
        row_props(&row(cells), paragraph, &palette).map(|p| p.map(|p| shown(&p)).unwrap_or_default())
    }

    fn one(paragraph: bool, name: &str, cell: Cell) -> Result<Vec<String>, &'static str> {
        made(paragraph, &[(name, cell)])
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    #[test]
    fn a_paragraph_row_is_the_attributes_the_specification_gives_its_cells() {
        for (align, word) in [(0.0, "left"), (1.0, "center"), (2.0, "right"), (3.0, "justify")] {
            assert_eq!(one(true, "HorzAlign", number(align)).unwrap(), [format!("Fo.text-align={word}")], "{align}");
        }
        assert_eq!(one(true, "HorzAlign", number(4.0)).unwrap(), ["Fo.text-align=justify", "Fo.text-align-last=justify"]);
        assert!(
            one(true, "HorzAlign", number(5.0)).is_err()
                && one(true, "HorzAlign", number(-1.0)).is_err()
                && one(true, "HorzAlign", words("left")).is_err()
        );
        assert_eq!(one(true, "Flags", number(0.0)).unwrap(), ["Style.writing-mode=lr-tb"]);
        assert_eq!(one(true, "Flags", number(1.0)).unwrap(), ["Style.writing-mode=rl-tb"]);
        assert!(one(true, "Flags", number(2.0)).is_err() && one(true, "Flags", words("x")).is_err());
        for (cell, local) in [
            ("IndLeft", "margin-left"),
            ("IndRight", "margin-right"),
            ("IndFirst", "text-indent"),
            ("SpBefore", "margin-top"),
            ("SpAfter", "margin-bottom"),
        ] {
            assert_eq!(one(true, cell, number(0.5)).unwrap(), [format!("Fo.{local}=0.5in")], "{cell}");
            assert!(one(true, cell, words("wide")).is_err(), "{cell}");
        }
        // The line spacing: a negative number is a share of the line (a factor), zero is the normal height, a positive number is a length.
        assert_eq!(one(true, "SpLine", number(-1.2)).unwrap(), ["Fo.line-height=120%"]);
        assert_eq!(one(true, "SpLine", number(-1.0)).unwrap(), ["Fo.line-height=100%"]);
        assert_eq!(one(true, "SpLine", number(0.0)).unwrap(), ["Fo.line-height=normal"]);
        assert_eq!(one(true, "SpLine", number(0.2)).unwrap(), ["Fo.line-height=0.2in"]);
        assert!(one(true, "SpLine", words("x")).is_err());
        // A list's row: the indents are the list's, and the other margins are the paragraph's still.
        let bullet = [
            ("Bullet", number(1.0)),
            ("IndLeft", number(0.5)),
            ("IndFirst", number(-0.25)),
            ("IndRight", number(0.1)),
            ("SpBefore", number(0.2)),
            ("SpAfter", number(0.3)),
        ];
        let props = row_props(&row(&bullet), true, &palette).unwrap().unwrap();
        assert!(props.bullet);
        assert_eq!(sorted(shown(&props)), ["Fo.margin-bottom=0.3in", "Fo.margin-right=0.1in", "Fo.margin-top=0.2in"]);
        // A row of the list's own cells alone writes nothing, and is a list's all the same.
        let only = row_props(&row(&[("Bullet", number(1.0)), ("BulletStr", words("-"))]), true, &palette).unwrap().unwrap();
        assert!(only.bullet && !only.writes());
        let plain = row_props(&row(&[("Bullet", number(0.0))]), true, &palette).unwrap().unwrap();
        assert!(!plain.bullet && !plain.writes());
        assert!(row_props(&row(&[]), true, &palette).unwrap().is_none(), "a row with no cell is no row");
        assert!(one(true, "Mystery", number(1.0)).is_err());
    }

    /// What the cells of a list's row make of its bullet, or the refusal.
    fn list_of(cells: &[(&str, Cell)]) -> Result<ListSpec, &'static str> {
        let props = row_props(&row(cells), true, &palette)?.unwrap();
        Ok(props.list.clone().expect("a row that has a bullet is a list's"))
    }

    #[test]
    fn a_list_row_names_its_bullet_by_the_table_or_by_its_own_text_and_refuses_what_is_neither() {
        // The table: the numbers 1 to 7 (whole), and nothing else.
        for (n, ch) in BULLETS.iter().enumerate() {
            assert_eq!(list_of(&[("Bullet", number((n + 1) as f64))]).unwrap().bullet, ch.to_string(), "{n}");
        }
        for bad in [2.5, 1.5, 0.5, 7.5, 8.0, 9.0, 100.0, -1.0, -2.0] {
            assert_eq!(list_of(&[("Bullet", number(bad))]), Err(WORDS_VALUE), "{bad}");
        }
        // A text of the file's own wins over the table, and an empty one is none (the table's character is used).
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletStr", words("-"))]).unwrap().bullet, "-");
        assert_eq!(list_of(&[("Bullet", number(2.0)), ("BulletStr", words(""))]).unwrap().bullet, "\u{25C6}");
        assert_eq!(list_of(&[("Bullet", number(9.0)), ("BulletStr", words("+"))]).unwrap().bullet, "+");
        assert_eq!(list_of(&[("Bullet", number(9.0)), ("BulletStr", words(""))]), Err(WORDS_VALUE));
        // The size: a length, never a negative one, and zero is as the file says it; a face that is blank is none.
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFontSize", number(0.0))]).unwrap().size, Some(0.0));
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFontSize", number(0.125))]).unwrap().size, Some(0.125));
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFontSize", number(-0.125))]), Err(WORDS_VALUE));
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFontSize", words("big"))]), Err(WORDS_VALUE));
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFont", words("  "))]).unwrap().font, None);
        assert_eq!(list_of(&[("Bullet", number(1.0)), ("BulletFont", words("Symbol"))]).unwrap().font.as_deref(), Some("Symbol"));
    }

    #[test]
    fn a_character_row_is_the_attributes_the_specification_gives_its_cells() {
        assert_eq!(one(false, "Size", number(0.25)).unwrap(), ["Fo.font-size=18pt"]);
        assert!(one(false, "Size", number(-0.1)).is_err() && one(false, "Size", words("big")).is_err());
        let style = |bits: f64| one(false, "Style", number(bits)).unwrap();
        assert_eq!(
            style(1.0),
            [
                "Fo.font-weight=bold",
                "Fo.font-style=normal",
                "Fo.font-variant=normal",
                "Style.font-weight-complex=normal",
                "Style.font-style-complex=normal",
                "Style.text-underline-style=none"
            ]
        );
        assert_eq!(
            style(62.0),
            [
                "Fo.font-weight=normal",
                "Fo.font-style=italic",
                "Fo.font-variant=small-caps",
                "Style.font-weight-complex=bold",
                "Style.font-style-complex=italic",
                "Style.text-underline-style=solid",
                "Style.text-underline-type=single"
            ]
        );
        for bad in [64.0, -1.0, 1.5] {
            assert!(one(false, "Style", number(bad)).is_err(), "{bad}");
        }
        assert_eq!(style(63.0).len(), 7);
        // The underline, the strike-through and the overline come from their own cells, whatever the bits of `Style` say of the underline.
        assert_eq!(one(false, "DblUnderline", number(1.0)).unwrap(), ["Style.text-underline-style=solid", "Style.text-underline-type=double"]);
        assert_eq!(one(false, "DblUnderline", number(0.0)).unwrap(), ["Style.text-underline-style=none"]);
        assert_eq!(made(false, &[("Style", number(4.0)), ("DblUnderline", number(1.0))]).unwrap().iter().filter(|s| s.contains("double")).count(), 1);
        assert_eq!(one(false, "Strikethru", number(1.0)).unwrap(), ["Style.text-line-through-style=solid", "Style.text-line-through-type=single"]);
        assert_eq!(one(false, "Strikethru", number(0.0)).unwrap(), ["Style.text-line-through-style=none"]);
        assert_eq!(
            one(false, "DoubleStrikethrough", number(1.0)).unwrap(),
            ["Style.text-line-through-style=solid", "Style.text-line-through-type=double"]
        );
        assert_eq!(
            made(false, &[("Strikethru", number(1.0)), ("DoubleStrikethrough", number(1.0))]).unwrap(),
            ["Style.text-line-through-style=solid", "Style.text-line-through-type=double"]
        );
        assert_eq!(one(false, "Overline", number(1.0)).unwrap(), ["Style.text-overline-style=solid"]);
        assert_eq!(one(false, "Overline", number(0.0)).unwrap(), ["Style.text-overline-style=none"]);
        for cell in ["DblUnderline", "Strikethru", "DoubleStrikethrough", "Overline"] {
            assert!(one(false, cell, number(2.0)).is_err() && one(false, cell, words("on")).is_err(), "{cell}");
        }
    }

    #[test]
    fn a_character_row_of_case_position_spacing_scale_and_faces_is_the_attributes_the_specification_gives() {
        for (case, word) in [(0.0, "none"), (1.0, "uppercase"), (2.0, "capitalize")] {
            assert_eq!(one(false, "Case", number(case)).unwrap(), [format!("Fo.text-transform={word}")]);
        }
        assert!(one(false, "Case", number(3.0)).is_err());
        for (pos, word) in [(0.0, "0%"), (1.0, "super"), (2.0, "sub")] {
            assert_eq!(one(false, "Pos", number(pos)).unwrap(), [format!("Style.text-position={word}")]);
        }
        assert!(one(false, "Pos", number(3.0)).is_err());
        assert_eq!(one(false, "Letterspace", number(0.0)).unwrap(), ["Fo.letter-spacing=normal"]);
        assert_eq!(one(false, "Letterspace", number(0.01)).unwrap(), ["Fo.letter-spacing=0.01in"]);
        assert_eq!(one(false, "Letterspace", number(-0.01)).unwrap(), ["Fo.letter-spacing=-0.01in"]);
        assert!(one(false, "Letterspace", words("wide")).is_err());
        assert_eq!(one(false, "FontScale", number(1.5)).unwrap(), ["Style.text-scale=150%"]);
        assert_eq!(one(false, "FontScale", number(0.5)).unwrap(), ["Style.text-scale=50%"]);
        assert!(
            one(false, "FontScale", number(0.0)).is_err()
                && one(false, "FontScale", number(-1.0)).is_err()
                && one(false, "FontScale", words("x")).is_err()
        );
        assert!(one(false, "Mystery", number(1.0)).is_err());
        // The faces are named, and write no attribute of their own.
        let faces =
            row_props(&row(&[("Font", words("Arial")), ("AsianFont", words("SimSun")), ("ComplexScriptFont", words("Tahoma"))]), false, &palette)
                .unwrap()
                .unwrap();
        assert!(faces.ops.is_empty() && faces.writes());
        let mut got = faces.faces.clone();
        got.sort();
        assert_eq!(got, [("font-name", "Arial".to_owned()), ("font-name-asian", "SimSun".to_owned()), ("font-name-complex", "Tahoma".to_owned())]);
    }

    #[test]
    fn a_colour_is_seven_characters_with_a_hash_or_an_index_of_the_palette_that_names_one() {
        let colour = |cell: Cell, palette: &dyn Fn(i64) -> Option<String>| {
            row_props(&row(&[("Color", cell)]), false, palette).map(|p| p.map(|p| shown(&p)).unwrap_or_default())
        };
        assert_eq!(colour(words("#FF00aa"), &palette).unwrap(), ["Fo.color=#ff00aa"]);
        assert_eq!(colour(words("#abcdef"), &no_palette).unwrap(), ["Fo.color=#abcdef"]);
        for bad in ["#12345", "#12345g", "#1234567", "#", "", "red", "#12 456"] {
            assert!(colour(words(bad), &any_index).is_err(), "{bad:?} is no colour");
        }
        // Seven digits with no hash are the index they say, not a colour.
        assert!(colour(words("1234567"), &no_palette).is_err());
        // An index: the palette's colour, lower case; an index it has not, a fraction, and one past a billion are refused.
        assert_eq!(colour(number(3.0), &palette).unwrap(), ["Fo.color=#00ff00"]);
        assert!(colour(number(4.0), &palette).is_err());
        assert_eq!(colour(number(5.0), &any_index).unwrap(), ["Fo.color=#112233"]);
        assert_eq!(colour(number(0.0), &any_index).unwrap(), ["Fo.color=#112233"]);
        assert_eq!(colour(number(-5.0), &any_index).unwrap(), ["Fo.color=#112233"]);
        assert_eq!(colour(number(999_999_999.0), &any_index).unwrap(), ["Fo.color=#112233"]);
        assert!(colour(number(1.5), &any_index).is_err());
        // What the palette hands over is held to the schema's form: no hash, a short or a long colour, and a name are no colour (the re-review fix
        // round 1, parked 8: `Rgba::hex` gives six digits and no hash, and the writer wrote them as they came).
        for bad in ["112233", "#1122", "#11223344", "red", "#11223g", ""] {
            let given = |_: i64| Some(bad.to_owned());
            assert!(colour(number(5.0), &given).is_err(), "{bad:?} from the palette is no colour");
        }
        let upper = |_: i64| Some("#AABBCC".to_owned());
        assert_eq!(colour(number(5.0), &upper).unwrap(), ["Fo.color=#aabbcc"]);
        assert!(colour(number(1e9), &any_index).is_err() && colour(number(-1e9), &any_index).is_err());
    }

    /// A sheet whose `Character` rows are `character` and whose `Paragraph` rows are `paragraph`, each `(IX, cells)`.
    fn sheet(character: &[(u32, &[(&str, f64)])], paragraph: &[(u32, &[(&str, f64)])]) -> Sheet {
        let mut sheet = Sheet::default();
        for (name, rows) in [("Character", character), ("Paragraph", paragraph)] {
            let mut section = Section::new(name, None);
            for (ix, cells) in rows.iter() {
                let cells: Vec<(&str, Cell)> = cells.iter().map(|(n, v)| (*n, number(*v))).collect();
                section.rows.push(Arc::new(Row { ix: Some(*ix), ..row(&cells) }));
            }
            sheet.sections.push(Arc::new(section));
        }
        sheet
    }

    /// The views of a text of `parts` over `sheet`, and the steps they took.
    fn viewed(sheet: &Sheet, parts: Vec<TextPart>) -> (Result<Vec<View>, &'static str>, u64) {
        let (mut counted, mut work) = (0u64, 0u64);
        let (mut characters, mut paragraphs) = (Rows::of(sheet, false, &palette, &mut counted), Rows::of(sheet, true, &palette, &mut counted));
        let text = Arc::new(Text { parts, ..Default::default() });
        let out = views(Some(&text), &mut characters, &mut paragraphs, &mut work);
        (out, work)
    }

    fn plain(s: &str) -> TextPart {
        TextPart::Chars(Arc::from(s))
    }

    #[test]
    fn the_runs_of_a_paragraph_join_where_the_rows_are_alike_and_are_never_empty() {
        let bold: &[(&str, f64)] = &[("Style", 1.0)];
        let both = sheet(&[(1, bold), (2, bold), (3, &[("Style", 2.0)])], &[]);
        let spans = |v: &View| v.runs.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>();
        // Two markers whose rows hold the same cells are one run; two whose rows differ are two.
        let (out, _) = viewed(&both, vec![TextPart::Cp(1), plain("ab"), TextPart::Cp(2), plain("cd")]);
        assert_eq!(spans(&out.unwrap()[0]), [(0, 4)]);
        let (out, _) = viewed(&both, vec![TextPart::Cp(1), plain("ab"), TextPart::Cp(3), plain("cd")]);
        assert_eq!(spans(&out.unwrap()[0]), [(0, 2), (2, 4)]);
        // A marker with no character after it, at the end or before a line feed, makes no run.
        let (out, _) = viewed(&both, vec![plain("ab"), TextPart::Cp(1)]);
        assert!(out.unwrap()[0].runs.is_empty(), "no run has a style: the one that has no character is not a run");
        let (out, _) = viewed(&both, vec![TextPart::Cp(1), plain("\nab")]);
        let out = out.unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].runs.is_empty(), "the first paragraph holds no character");
        assert_eq!(spans(&out[1]), [(0, 2)], "the marker holds across the paragraph");
        // A row that is not in the sheet, or has no cell, is a run with no style, and a text with no run that has a style has no runs.
        let (out, _) = viewed(&both, vec![TextPart::Cp(9), plain("ab")]);
        assert!(out.unwrap()[0].runs.is_empty());
    }

    #[test]
    fn the_views_of_a_text_are_counted_as_they_are_made_and_the_size_is_held_to_the_cap() {
        let none = sheet(&[], &[]);
        // A step for each part, and for each paragraph one and one for each of its runs.
        let (out, steps) = viewed(&none, vec![plain("ab\ncd")]);
        assert_eq!(out.unwrap().len(), 2);
        assert_eq!(steps, 1 + 2 + 2);
        let (out, steps) = viewed(&none, vec![plain("\n")]);
        assert_eq!(out.unwrap().len(), 2);
        assert_eq!(steps, 1 + 1 + 1, "two empty paragraphs hold no run, and cost one step each");
        // The cap is on the bytes: the most is held, one more is not.
        let most = super::super::text::MOST_EDITED_TEXT;
        let (out, _) = viewed(&none, vec![plain(&"x".repeat(most))]);
        assert_eq!(out.unwrap()[0].chars.len(), most);
        let (out, _) = viewed(&none, vec![plain(&"x".repeat(most + 1))]);
        assert!(out.is_err());
        // A field's shown text is among the paragraph's characters, and a part that is no text or marker (a tab stop marker) is refused.
        let (out, _) = viewed(&none, vec![plain("a "), TextPart::Fld { ix: 0, text: Arc::from("x") }, plain(" b")]);
        assert_eq!(out.unwrap()[0].chars, "a x b");
        let (out, _) = viewed(&none, vec![TextPart::Tp(0)]);
        assert!(out.is_err());
    }

    #[test]
    fn the_first_paragraph_of_the_file_that_is_not_kept_at_the_head_can_still_be_matched() {
        // `x` is the file's first paragraph (the head holds none of the two lists' paragraphs), and the model has it after a new one.
        assert_eq!(lists(&["x", "y"], &["z", "x", "w"]), [Step::Insert { new: 0 }, Step::Keep { old: 0, new: 1 }, Step::Edit { old: 1, new: 2 }]);
    }

    #[test]
    fn a_match_never_goes_back_over_one_that_was_made() {
        // `a` is matched at its place in the file, and `b`, which the file holds before it, is not matched after it: the two lists are walked once, in order.
        assert_eq!(
            lists(&["p", "b", "a", "q"], &["z", "a", "b", "w", "v"]),
            [
                Step::Edit { old: 0, new: 0 },
                Step::Remove { old: 1 },
                Step::Keep { old: 2, new: 1 },
                Step::Edit { old: 3, new: 2 },
                Step::Insert { new: 3 },
                Step::Insert { new: 4 }
            ]
        );
    }

    #[test]
    fn a_font_is_a_family_and_a_number_is_a_place_in_a_table_the_file_has_none_of() {
        // The engine copies the document's own font (0) into the rows it makes: no face is named for it, and a row of nothing else writes nothing.
        let props = row_props(&row(&[("Font", number(0.0)), ("AsianFont", number(0.0)), ("ComplexScriptFont", number(0.0))]), false, &palette)
            .unwrap()
            .unwrap();
        assert!(props.faces.is_empty() && !props.writes());
        // A family is named; a place past 0 has nothing to be looked up in.
        let props = row_props(&row(&[("Font", words("Arial")), ("AsianFont", number(0.0))]), false, &palette).unwrap().unwrap();
        assert_eq!(props.faces, [("font-name", "Arial".to_owned())]);
        for cell in ["Font", "AsianFont", "ComplexScriptFont"] {
            assert!(one(false, cell, number(3.0)).is_err(), "{cell}");
        }
    }

    /// The attributes and faces a row's groups change into the ones of another row, as `Ns.attribute=value` (a removal as `-`) and the faces' local names.
    fn changed(old: &[(&str, Cell)], new: &[(&str, Cell)], paragraph: bool) -> (Vec<String>, Vec<&'static str>) {
        // A row with no cell is no row: the model has none, and no group.
        let props = |cells: &[(&str, Cell)]| {
            row_props(&row(cells), paragraph, &palette).unwrap().unwrap_or_else(|| {
                Arc::new(Props { key: String::new(), ops: Vec::new(), faces: Vec::new(), bullet: false, list: None, parts: Vec::new() })
            })
        };
        let (ops, faces) = changes(&props(old), &props(new));
        let shown = ops
            .iter()
            .map(|o| {
                let value = match &o.val {
                    Val::Text(t) => t.clone(),
                    Val::Length(x) => format!("{x}in"),
                    Val::Remove => "-".to_owned(),
                };
                format!("{}{:?}.{}={value}", o.prop, o.ns, o.local)
            })
            .collect();
        (shown, faces.iter().map(|(l, _)| *l).collect())
    }

    #[test]
    fn a_group_of_cells_that_is_the_same_in_both_rows_is_left_as_the_file_has_it() {
        let base = [("Size", number(0.25)), ("Style", number(5.0)), ("Case", number(1.0)), ("Pos", number(1.0)), ("Color", words("#00ff00"))];
        // The same row: nothing is written, and nothing is taken out.
        assert_eq!(changed(&base, &base, false), (Vec::new(), Vec::new()));
        // One cell changed: its attributes are written, and no other group's.
        let with = |name: &str, v: Cell| -> Vec<(&str, Cell)> {
            base.iter().map(|(n, c)| if *n == name { (*n, v.clone()) } else { (*n, c.clone()) }).collect()
        };
        assert_eq!(changed(&base, &with("Size", number(0.5)), false).0, ["2Fo.font-size=36pt"]);
        assert_eq!(changed(&base, &with("Case", number(2.0)), false).0, ["2Fo.text-transform=capitalize"]);
        assert_eq!(changed(&base, &with("Pos", number(2.0)), false).0, ["2Style.text-position=sub"]);
        assert_eq!(changed(&base, &with("Color", words("#ff0000")), false).0, ["2Fo.color=#ff0000"]);
        // The bits of `Style` but the underline's are the style's group: a bold edit leaves the underline's group alone.
        let bold = changed(&base, &with("Style", number(4.0)), false).0;
        assert_eq!(
            bold,
            [
                "2Fo.font-weight=normal",
                "2Fo.font-style=normal",
                "2Fo.font-variant=normal",
                "2Style.font-weight-complex=normal",
                "2Style.font-style-complex=normal"
            ]
        );
        // The underline's bit is the underline's group alone: the style's group is left, the underline is written and the colour and the width it does not write go.
        let underline = changed(&base, &with("Style", number(1.0)), false).0;
        assert_eq!(
            underline,
            ["2Style.text-underline-style=none", "2Style.text-underline-type=-", "2Style.text-underline-color=-", "2Style.text-underline-width=-"]
        );
    }

    #[test]
    fn a_group_that_goes_takes_what_it_owns_out_and_one_that_comes_writes_its_attributes() {
        let base = [("Size", number(0.25)), ("Case", number(1.0))];
        assert_eq!(changed(&base, &[("Size", number(0.25))], false).0, ["2Fo.text-transform=-"]);
        assert_eq!(changed(&[("Size", number(0.25))], &base, false).0, ["2Fo.text-transform=uppercase"]);
        // A row with no cell at all is no row: every group of the other goes.
        let mut gone = changed(&base, &[], false).0;
        gone.sort();
        assert_eq!(gone, ["2Fo.font-size=-", "2Fo.text-transform=-"]);
        // The line height's two companions go with it, and the alignment's last-line word goes when it is not written.
        let (ops, _) = changed(&[("SpLine", number(-1.5))], &[("SpLine", number(-2.0))], true);
        assert_eq!(ops, ["1Fo.line-height=200%", "1Style.line-spacing=-", "1Style.line-height-at-least=-"]);
        let (ops, _) = changed(&[("HorzAlign", number(4.0))], &[("HorzAlign", number(1.0))], true);
        assert_eq!(ops, ["1Fo.text-align=center", "1Fo.text-align-last=-"]);
        let (ops, _) = changed(&[("HorzAlign", number(1.0))], &[("HorzAlign", number(4.0))], true);
        assert_eq!(ops, ["1Fo.text-align=justify", "1Fo.text-align-last=justify"]);
        // Each of the other paragraph cells is its own group, on the paragraph's properties.
        for (cell, local) in [
            ("IndLeft", "margin-left"),
            ("IndRight", "margin-right"),
            ("IndFirst", "text-indent"),
            ("SpBefore", "margin-top"),
            ("SpAfter", "margin-bottom"),
        ] {
            assert_eq!(changed(&[(cell, number(0.1))], &[(cell, number(0.2))], true).0, [format!("1Fo.{local}=0.2in")], "{cell}");
        }
        assert_eq!(changed(&[("Flags", number(0.0))], &[("Flags", number(1.0))], true).0, ["1Style.writing-mode=rl-tb"]);
    }

    #[test]
    fn a_face_changed_is_named_and_takes_its_family_attribute_out_and_a_face_that_goes_takes_both() {
        let (ops, faces) = changed(&[("Font", words("Arial"))], &[("Font", words("Mono"))], false);
        assert_eq!((ops, faces), (vec!["2Fo.font-family=-".to_owned()], vec!["font-name"]));
        let (ops, faces) = changed(&[("AsianFont", words("A"))], &[("AsianFont", words("B"))], false);
        assert_eq!((ops, faces), (vec!["2Style.font-family-asian=-".to_owned()], vec!["font-name-asian"]));
        let (ops, faces) = changed(&[("ComplexScriptFont", words("A"))], &[("ComplexScriptFont", words("B"))], false);
        assert_eq!((ops, faces), (vec!["2Style.font-family-complex=-".to_owned()], vec!["font-name-complex"]));
        // The document's own face (the number 0) names none: both attributes go.
        let (ops, faces) = changed(&[("Font", words("Arial"))], &[("Font", number(0.0))], false);
        assert_eq!((ops, faces), (vec!["2Style.font-name=-".to_owned(), "2Fo.font-family=-".to_owned()], Vec::new()));
        // The same family is no change.
        assert_eq!(changed(&[("Font", words("Arial"))], &[("Font", words("Arial"))], false), (Vec::new(), Vec::new()));
    }

    #[test]
    fn the_strike_through_and_the_overline_are_groups_that_take_their_colours_and_widths_with_them() {
        let (ops, _) = changed(&[("Strikethru", number(1.0))], &[("Strikethru", number(0.0))], false);
        assert_eq!(
            ops,
            [
                "2Style.text-line-through-style=none",
                "2Style.text-line-through-type=-",
                "2Style.text-line-through-color=-",
                "2Style.text-line-through-width=-"
            ]
        );
        let (ops, _) = changed(&[("Overline", number(0.0))], &[("Overline", number(1.0))], false);
        assert_eq!(ops, ["2Style.text-overline-style=solid", "2Style.text-overline-color=-", "2Style.text-overline-width=-"]);
        // The double strike-through is the same group as the single one.
        let (ops, _) = changed(&[("Strikethru", number(1.0))], &[("Strikethru", number(1.0)), ("DoubleStrikethrough", number(1.0))], false);
        assert_eq!(
            ops,
            [
                "2Style.text-line-through-style=solid",
                "2Style.text-line-through-type=double",
                "2Style.text-line-through-color=-",
                "2Style.text-line-through-width=-"
            ]
        );
        // The double underline is the underline's group, with the underline's bit of `Style`.
        let (ops, _) = changed(&[("DblUnderline", number(0.0))], &[("DblUnderline", number(1.0))], false);
        assert_eq!(
            ops,
            [
                "2Style.text-underline-style=solid",
                "2Style.text-underline-type=double",
                "2Style.text-underline-color=-",
                "2Style.text-underline-width=-"
            ]
        );
    }

    #[test]
    fn every_group_a_row_makes_owns_the_attributes_it_writes_and_no_other_group_does() {
        // A row with every cell: each op of each part is among what the part's group owns, and no attribute is owned by two groups.
        let all = row_props(
            &row(&[
                ("Size", number(0.25)),
                ("Style", number(63.0)),
                ("DblUnderline", number(1.0)),
                ("Strikethru", number(1.0)),
                ("Overline", number(1.0)),
                ("Case", number(1.0)),
                ("Pos", number(1.0)),
                ("Color", words("#00ff00")),
                ("Letterspace", number(0.01)),
                ("FontScale", number(1.5)),
                ("Font", words("A")),
                ("AsianFont", words("B")),
                ("ComplexScriptFont", words("C")),
            ]),
            false,
            &palette,
        )
        .unwrap()
        .unwrap();
        let paragraph = row_props(
            &row(&[
                ("HorzAlign", number(4.0)),
                ("Flags", number(1.0)),
                ("IndLeft", number(0.1)),
                ("IndRight", number(0.1)),
                ("IndFirst", number(0.1)),
                ("SpBefore", number(0.1)),
                ("SpAfter", number(0.1)),
                ("SpLine", number(-1.5)),
            ]),
            true,
            &palette,
        )
        .unwrap()
        .unwrap();
        let mut seen: Vec<(Ns, &str)> = Vec::new();
        for props in [&all, &paragraph] {
            for part in &props.parts {
                let owned = owned(part.id);
                assert!(!owned.is_empty(), "{}", part.id);
                for o in &part.ops {
                    assert!(owned.contains(&(o.ns, o.local)), "{} writes {:?}.{}, which it does not own", part.id, o.ns, o.local);
                    assert_eq!(o.prop, if std::ptr::eq(props, &paragraph) { 1 } else { 2 }, "{}", part.id);
                }
                for (local, _) in &part.faces {
                    assert!(owned.contains(&(Ns::Style, *local)), "{} names {local}", part.id);
                }
                for a in owned {
                    assert!(!seen.contains(a), "{a:?} is owned twice");
                    seen.push(*a);
                }
            }
        }
        // Every attribute the groups own is on the ownership table, so that the writer may take it out and write it (`own::TABLE`).
        for (ns, local) in &seen {
            let prefix = match ns {
                Ns::Fo => "fo",
                Ns::Style => "style",
                other => panic!("{other:?}"),
            };
            assert!(super::super::own::owns(&format!("{prefix}:{local}")).is_some(), "{prefix}:{local} is not on the ownership table");
        }
        // The flattened attributes of a row are those of its parts, in order.
        let flat: Vec<&Op> = all.parts.iter().flat_map(|p| p.ops.iter()).collect();
        assert_eq!(flat, all.ops.iter().collect::<Vec<_>>());
        assert_eq!(all.faces.len(), 3);
    }

    #[test]
    fn a_list_s_indents_are_groups_that_write_nothing() {
        let props =
            row_props(&row(&[("Bullet", number(1.0)), ("IndLeft", number(0.5)), ("IndFirst", number(-0.25))]), true, &palette).unwrap().unwrap();
        let mut ids: Vec<(&str, usize)> = props.parts.iter().map(|p| (p.id, p.ops.len())).collect();
        ids.sort();
        assert_eq!(ids, [("IndFirst", 0), ("IndLeft", 0)]);
        // Each group is its own cell's: the cell's value is what its group tells another by.
        let sig = |id: &str| props.parts.iter().find(|p| p.id == id).map(|p| p.sig.clone());
        assert_eq!(sig("IndLeft").as_deref(), Some("0.5|"));
        assert_eq!(sig("IndFirst").as_deref(), Some("-0.25|"));
    }
}
