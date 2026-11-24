//! A shape's text as the model writer sees it: the paragraphs of
//! a [`Text`] with the rows its markers name, **every one of them written, none refused for what a row holds**.
//!
//! The splice writer's [`super::views`] refuses a text it cannot rewrite in place (a field, a marker with no place in a file, a cell with no attribute, a formula, a carriage return) and
//! leaves the file's bytes alone. The model writer has no file to leave alone, so here:
//!
//! * **A carriage return** is a paragraph break (`CR LF` is one): a file reads a carriage return as white space and has no character for it, and the exact text, with its carriage returns,
//!   is carried in DiaCraft's extension ([`ModelText::carriage_returns`]).
//! * **A row's cell the file cannot say** (a cell with no attribute, a value its attribute cannot hold, a formula, a font that is a number the document's table has no name for) is **left
//!   out of the style and said** ([`ModelRows::props`]'s second figure): the cached value of a formula is written, the formula is carried. A row that left a cell out is no row a file
//!   can say whole, and the text is carried too ([`crate::ext::native::Native::text_native`] is false).
//! * **A field** is an inline element of the paragraph at the place its characters stand; **a mark** the `.odg` reader kept (a bookmark, a reference mark, a soft page break) is an inline
//!   element of no characters ([`Inline`]).
//! * **Held markup** (`text:h`, lists, links, fields' own elements, style names) is matched to the paragraph by the stamp of its characters ([`characters_stamp`]), the way the reader made it.
//!
//! Every walk is counted in `steps` by the part it goes over, and the sizes are told before a character is copied.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Digest, FormulaSlot, RawNode, Row, Sheet, Stamp, Text, TextPart, Value};

use super::{MOST_PARAGRAPHS, Props, WORDS_PARAGRAPHS, row_props};

/// The most characters of text the model writer holds beside the model's own (the same bound as a retext's).
pub(crate) const MOST_TEXT: usize = super::super::text::MOST_EDITED_TEXT;

/// The words of a refusal of a text with more characters than [`MOST_TEXT`].
pub(crate) const WORDS_TEXT_SIZE: &str = "a text of more than 16 MiB (a save writes at most that much text in one shape)";

/// An element inside a paragraph that is no run of characters.
#[derive(Clone, Debug)]
pub(crate) enum Inline<'t> {
    /// A field: it shows `len` characters from `at` (they are in the paragraph's characters, at `bytes`), and the `Field` row `ix` says what it is.
    Field { at: usize, len: usize, ix: u32, bytes: (usize, usize) },
    /// A zero-width node the `.odg` reader kept whole (a bookmark, a reference mark, a soft page break), at `at`.
    Mark { at: usize, node: &'t RawNode },
}

/// What a stretch of a paragraph's characters is, as the reader holds it, for the stamp of its characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PieceKind {
    Text,
    Field,
    Mark,
}

/// One piece of a paragraph as the reader holds it: its bytes in the paragraph's characters, and the `Character` row it was read on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Piece {
    pub kind: PieceKind,
    pub bytes: (usize, usize),
    pub row: u32,
}

/// A stretch of a paragraph under one `Character` row, in characters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stretch {
    pub start: usize,
    pub end: usize,
    pub row: u32,
}

/// A paragraph of the model path's text.
#[derive(Debug, Default)]
pub(crate) struct ModelPara<'t> {
    /// The characters, a field's shown text among them.
    pub chars: String,
    /// How many characters those are.
    pub count: usize,
    /// The `Paragraph` row in force at the paragraph's end (`pp`).
    pub pp: u32,
    /// The runs, by the `Character` row in force: a stretch for each change of row.
    pub stretches: Vec<Stretch>,
    pub inlines: Vec<Inline<'t>>,
    pub pieces: Vec<Piece>,
}

/// What the model path found in a text, besides its paragraphs.
#[derive(Debug, Default)]
pub(crate) struct ModelText<'t> {
    pub paras: Vec<ModelPara<'t>>,
    /// The text holds a carriage return (written as a paragraph break; the exact text is carried).
    pub carriage_returns: bool,
    /// The text holds a part a file has no place for: a tab stop marker, VSDX markup, markup a newer DiaCraft kept (the text is carried).
    pub unsaid_parts: bool,
    /// How many fields hold characters the field's element cannot show as the reader reads them back (white space or a line break in them): they are written as plain text.
    pub fields_as_text: usize,
}

/// Whether a field's shown text can be written as the content of a field element and read back as it is: the reader takes the text of the element as it stands, and the splice
/// writer's own check collapses its white space, so a text with a line break, a tab, or two white space characters together, or one that starts or ends with one, is written as plain text.
pub(crate) fn field_text_ok(shown: &str) -> bool {
    let mut previous_white = true;
    for c in shown.chars() {
        let white = matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{2028}');
        if white && (previous_white || c != ' ') {
            return false;
        }
        previous_white = white;
    }
    !shown.is_empty() && !previous_white
}

/// The paragraphs being built from a text's parts.
struct Builder<'t> {
    out: ModelText<'t>,
    current: ModelPara<'t>,
    closed: Vec<Stretch>,
    /// The rows in force: a `pp` holds until the next, and so does a `cp`, across paragraphs; the first paragraph and run are on row 0.
    pp: u32,
    cp: u32,
    /// Where the run in force began in the paragraph being built, in characters.
    run_start: usize,
    /// The last character was a carriage return, which a line feed that comes next is the end of.
    after_cr: bool,
    steps: u64,
}

impl<'t> Builder<'t> {
    /// Ends the paragraph being built: its last run is closed at its end.
    fn finish(&mut self) -> Result<(), &'static str> {
        if self.out.paras.len() >= MOST_PARAGRAPHS {
            return Err(WORDS_PARAGRAPHS);
        }
        if self.current.count > self.run_start {
            self.closed.push(Stretch { start: self.run_start, end: self.current.count, row: self.cp });
        }
        self.steps = self.steps.saturating_add(self.closed.len() as u64 + 1);
        let mut para = std::mem::take(&mut self.current);
        para.pp = self.pp;
        for stretch in self.closed.drain(..) {
            match para.stretches.last_mut() {
                Some(last) if last.row == stretch.row && last.end == stretch.start => last.end = stretch.end,
                _ => para.stretches.push(stretch),
            }
        }
        self.out.paras.push(para);
        self.run_start = 0;
        Ok(())
    }

    /// Characters on the row in force, **a line feed and a carriage return each ending the paragraph** (`CR LF` is one end).
    fn chars(&mut self, chars: &str) -> Result<(), &'static str> {
        let mut rest: &str = chars;
        // A `CR LF` that two parts of the text divide is one break.
        if self.after_cr && rest.starts_with('\n') {
            rest = rest.get(1..).unwrap_or_default();
        }
        self.after_cr = false;
        loop {
            let Some(at) = rest.find(['\n', '\r']) else {
                push_text(&mut self.current, rest, self.cp);
                return Ok(());
            };
            push_text(&mut self.current, rest.get(..at).unwrap_or_default(), self.cp);
            let carriage = rest.as_bytes().get(at) == Some(&b'\r');
            rest = rest.get(at + 1..).unwrap_or_default();
            if carriage {
                self.out.carriage_returns = true;
                if rest.starts_with('\n') {
                    rest = rest.get(1..).unwrap_or_default();
                } else if rest.is_empty() {
                    self.after_cr = true;
                }
            }
            self.finish()?;
        }
    }
}

/// The paragraphs of `text`, one for each stretch between line breaks, **each built from the parts in one pass**. `Err` in words for a text past [`MOST_TEXT`], one with a character XML does not
/// allow, or one with more paragraphs than [`MOST_PARAGRAPHS`]. `steps` counts one for each part and each paragraph.
pub(crate) fn model_paragraphs<'t>(text: &'t Text, steps: &mut u64) -> Result<ModelText<'t>, &'static str> {
    let bytes: usize = text
        .parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(c) => c.len(),
            TextPart::Fld { text, .. } => text.len(),
            _ => 0,
        })
        .fold(0usize, usize::saturating_add);
    if bytes > MOST_TEXT {
        return Err(WORDS_TEXT_SIZE);
    }
    let mut b = Builder {
        out: ModelText::default(),
        current: ModelPara::default(),
        closed: Vec::new(),
        pp: 0,
        cp: 0,
        run_start: 0,
        after_cr: false,
        steps: 0,
    };
    for part in &text.parts {
        b.steps = b.steps.saturating_add(1);
        match part {
            TextPart::Chars(chars) => {
                if super::super::splice::has_bad_char(chars) {
                    return Err(super::super::splice::WORDS_XML_CHAR);
                }
                b.chars(chars)?;
            }
            TextPart::Cp(ix) => {
                if b.current.count > b.run_start {
                    b.closed.push(Stretch { start: b.run_start, end: b.current.count, row: b.cp });
                }
                b.run_start = b.current.count;
                b.cp = *ix;
            }
            TextPart::Pp(ix) => b.pp = *ix,
            TextPart::Tp(_) | TextPart::Raw(_) | TextPart::NativeRaw(_) => b.out.unsaid_parts = true,
            TextPart::Fld { ix, text } => {
                if super::super::splice::has_bad_char(text) {
                    return Err(super::super::splice::WORDS_XML_CHAR);
                }
                b.after_cr = false;
                if !field_text_ok(text) {
                    // Its characters are plain text, broken into paragraphs where they hold a line break: the file has no element that shows them as they are.
                    b.out.fields_as_text = b.out.fields_as_text.saturating_add(1);
                    b.chars(text)?;
                    continue;
                }
                let at = b.current.count;
                let len = text.chars().count();
                let start = b.current.chars.len();
                b.current.chars.push_str(text);
                b.current.count = b.current.count.saturating_add(len);
                let bytes = (start, b.current.chars.len());
                b.current.inlines.push(Inline::Field { at, len, ix: *ix, bytes });
                b.current.pieces.push(Piece { kind: PieceKind::Field, bytes, row: b.cp });
            }
            TextPart::OdfRaw(node) => {
                b.after_cr = false;
                let at = b.current.count;
                b.current.inlines.push(Inline::Mark { at, node });
                let end = b.current.chars.len();
                b.current.pieces.push(Piece { kind: PieceKind::Mark, bytes: (end, end), row: b.cp });
            }
        }
    }
    b.finish()?;
    *steps = steps.saturating_add(b.steps);
    Ok(b.out)
}

/// Puts `piece` (no line break in it) on the end of `para`, as part of the piece the reader holds for the row `row`: one piece for a run of text on one row.
fn push_text(para: &mut ModelPara<'_>, piece: &str, row: u32) {
    if piece.is_empty() {
        return;
    }
    let start = para.chars.len();
    para.chars.push_str(piece);
    para.count = para.count.saturating_add(piece.chars().count());
    match para.pieces.last_mut() {
        Some(last) if last.kind == PieceKind::Text && last.row == row && last.bytes.1 == start => last.bytes.1 = para.chars.len(),
        _ => para.pieces.push(Piece { kind: PieceKind::Text, bytes: (start, para.chars.len()), row }),
    }
}

/// The stamp of a paragraph's characters as the reader made it (`read/text.rs`, `characters_stamp`): the count, each piece, then where each of the file's spans began and ended.
/// `spans` are the places the held paragraph gives (`OdfRun::from` and `to`), so that the stamp is the one the file's paragraph had when the model's is the same text.
pub(crate) fn characters_stamp(para: &ModelPara<'_>, spans: impl Iterator<Item = (u32, u32)>) -> Stamp {
    let mut digest = Digest::new("characters");
    digest.bytes(&u64::try_from(para.count).unwrap_or(u64::MAX).to_le_bytes());
    for piece in &para.pieces {
        let text = para.chars.get(piece.bytes.0..piece.bytes.1).unwrap_or_default();
        match piece.kind {
            PieceKind::Text => {
                digest.text(text);
            }
            PieceKind::Field => {
                digest.bytes(&[1]).text(text);
            }
            PieceKind::Mark => {
                digest.bytes(&[2]);
            }
        }
    }
    for (from, to) in spans {
        digest.bytes(&u64::from(from).to_le_bytes()).bytes(&u64::from(to).to_le_bytes());
    }
    digest.finish()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the rows

/// The rows of one section of a sheet by `IX`, each as a [`Props`] **that leaves out what a file cannot say** and tells that it did.
pub(crate) struct ModelRows<'a> {
    rows: HashMap<u32, &'a Row>,
    paragraph: bool,
    palette: &'a dyn Fn(i64) -> Option<String>,
    fonts: &'a dyn Fn(i64) -> Option<String>,
    done: HashMap<u32, Option<(Arc<Props>, bool)>>,
    stamps: HashMap<u32, Stamp>,
}

impl<'a> ModelRows<'a> {
    /// The rows of the sheet's `Character` (or, for `paragraph`, `Paragraph`) sections, the first row of each `IX` standing for it. `palette` names a colour by its index and `fonts` a
    /// face by its place in the document's table.
    pub(crate) fn of(
        sheet: &'a Sheet,
        paragraph: bool,
        palette: &'a dyn Fn(i64) -> Option<String>,
        fonts: &'a dyn Fn(i64) -> Option<String>,
        steps: &mut u64,
    ) -> ModelRows<'a> {
        let name = if paragraph { "Paragraph" } else { "Character" };
        let mut rows: HashMap<u32, &'a Row> = HashMap::new();
        for section in sheet.sections.iter().filter(|s| s.name.as_str() == name) {
            for row in section.rows.iter() {
                // A step for the row, and two for each cell: [`ModelRows::props`] tries each alone and then the row as it stays. **Charged here, before any row is made.**
                *steps = steps.saturating_add(1).saturating_add((row.cells.len() as u64).saturating_mul(2));
                if let Some(ix) = row.ix {
                    rows.entry(ix).or_insert(row);
                }
            }
        }
        ModelRows { rows, paragraph, palette, fonts, done: HashMap::new(), stamps: HashMap::new() }
    }

    /// The stamp of row `ix` as [`diacraft_doc::stamp_text_row`] makes it (a row the sheet lacks stamps as one with no cell), **made once for each row**.
    pub(crate) fn stamp(&mut self, ix: u32) -> Stamp {
        if let Some(stamp) = self.stamps.get(&ix) {
            return *stamp;
        }
        let stamp = match self.rows.get(&ix) {
            Some(row) => diacraft_doc::stamp_text_row(row),
            None => diacraft_doc::stamp_text_row(&Row::default()),
        };
        self.stamps.insert(ix, stamp);
        stamp
    }

    /// The row `ix` of the section, when the sheet has it.
    pub(crate) fn row(&self, ix: u32) -> Option<&'a Row> {
        self.rows.get(&ix).copied()
    }

    /// The `IX` of every row that holds a cell, in no order: the rows a text could be using.
    pub(crate) fn ixs(&self) -> impl Iterator<Item = u32> + '_ {
        self.rows.iter().filter(|(_, row)| !row.cells.is_empty()).map(|(ix, _)| *ix)
    }

    /// What row `ix` comes to and whether it left a cell out: `None` for a row the sheet does not have or one with no cell.
    pub(crate) fn props(&mut self, ix: u32) -> Option<(Arc<Props>, bool)> {
        if let Some(done) = self.done.get(&ix) {
            return done.clone();
        }
        let made = self.rows.get(&ix).and_then(|row| {
            let (kept, lossy) = lenient(row, self.paragraph, self.palette, self.fonts);
            row_props(&kept, self.paragraph, self.palette).ok().flatten().map(|props| (props, lossy))
        });
        self.done.insert(ix, made.clone());
        made
    }
}

/// The cells of `row` that a file can say, as a row of its own, and whether any was left out or said less than the model's. A cell stands or goes **alone**: a cell is tried as the
/// only cell of a row, so that one that has no attribute, a value its attribute cannot hold, or a font the table has no name for does not take the others with it. A formula's cell is
/// written as its cached value (and the row is told to have said less: the formula is carried).
fn lenient(row: &Row, paragraph: bool, palette: &dyn Fn(i64) -> Option<String>, fonts: &dyn Fn(i64) -> Option<String>) -> (Row, bool) {
    let mut kept = Row { ix: row.ix, ..Row::default() };
    let mut lossy = false;
    for (name, cell) in &row.cells {
        let mut candidate: Cell = cell.clone();
        if !matches!(candidate.formula, FormulaSlot::Absent | FormulaSlot::NoFormula) {
            lossy = true;
            candidate.formula = FormulaSlot::Absent;
        }
        if candidate.extra.is_some() || !candidate.raw.is_empty() || candidate.error.is_some() {
            lossy = true;
        }
        // A face given by its place in the document's table is the family the table names.
        if matches!(name.as_str(), "Font" | "AsianFont" | "ComplexScriptFont" | "BulletFont")
            && let Some(n) = candidate.value.as_ref().and_then(Value::number)
            && n != 0.0
        {
            match (n.fract() == 0.0 && n.abs() < 1e9).then(|| fonts(n as i64)).flatten() {
                Some(family) => candidate.value = Some(Value::from_text(&family)),
                None => {
                    lossy = true;
                    continue;
                }
            }
        }
        if candidate.value.is_none() {
            lossy = true;
            continue;
        }
        let mut alone = Row::default();
        alone.cells.insert(CellName::new(name.as_str()), candidate.clone());
        if row_props(&alone, paragraph, palette).is_err() {
            lossy = true;
            continue;
        }
        kept.cells.insert(name.clone(), candidate);
    }
    (kept, lossy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> TextPart {
        TextPart::Chars(Arc::from(s))
    }

    fn text(parts: Vec<TextPart>) -> Text {
        Text { parts, ..Default::default() }
    }

    #[test]
    fn a_carriage_return_is_a_line_break_and_cr_lf_is_one() {
        let t = text(vec![chars("a\r\nb\rc\nd")]);
        let model = model_paragraphs(&t, &mut 0).unwrap();
        let shown: Vec<&str> = model.paras.iter().map(|p| p.chars.as_str()).collect();
        assert_eq!(shown, ["a", "b", "c", "d"]);
        assert!(model.carriage_returns);
        // A `CR LF` that two parts divide is one break too.
        let t = text(vec![chars("a\r"), chars("\nb")]);
        let model = model_paragraphs(&t, &mut 0).unwrap();
        let shown: Vec<&str> = model.paras.iter().map(|p| p.chars.as_str()).collect();
        assert_eq!(shown, ["a", "b"]);
        let t = text(vec![chars("a\n")]);
        assert!(!model_paragraphs(&t, &mut 0).unwrap().carriage_returns);
    }

    #[test]
    fn a_row_holds_across_paragraphs_and_the_last_pp_before_a_paragraph_s_end_sets_it() {
        let t = text(vec![TextPart::Pp(1), chars("ab"), TextPart::Cp(2), chars("c\nd"), TextPart::Pp(3), chars("e")]);
        let model = model_paragraphs(&t, &mut 0).unwrap();
        type Seen = Vec<(u32, Vec<(usize, usize, u32)>)>;
        let rows: Seen = model.paras.iter().map(|p| (p.pp, p.stretches.iter().map(|s| (s.start, s.end, s.row)).collect())).collect();
        assert_eq!(rows, [(1, vec![(0, 2, 0), (2, 3, 2)]), (3, vec![(0, 2, 2)])]);
    }

    #[test]
    fn a_field_is_an_inline_element_whose_characters_are_the_paragraph_s() {
        let t = text(vec![chars("a "), TextPart::Fld { ix: 4, text: Arc::from("12") }, chars(" b")]);
        let model = model_paragraphs(&t, &mut 0).unwrap();
        let para = &model.paras[0];
        assert_eq!(para.chars, "a 12 b");
        assert_eq!(para.count, 6);
        assert!(matches!(para.inlines.as_slice(), [Inline::Field { at: 2, len: 2, ix: 4, .. }]));
        assert_eq!(para.pieces.iter().map(|p| p.kind).collect::<Vec<_>>(), [PieceKind::Text, PieceKind::Field, PieceKind::Text]);
        // A shown text with a line break or a doubled space is not an element's content.
        for shown in ["a\nb", "a  b", " a", "a ", "", "a\tb"] {
            let t = text(vec![TextPart::Fld { ix: 0, text: Arc::from(shown) }]);
            let model = model_paragraphs(&t, &mut 0).unwrap();
            assert_eq!(model.fields_as_text, 1, "{shown:?}");
        }
    }

    #[test]
    fn a_text_past_the_bound_or_with_a_character_xml_forbids_is_refused_in_words() {
        let t = text(vec![chars("a\u{1}b")]);
        assert_eq!(model_paragraphs(&t, &mut 0).unwrap_err(), super::super::super::splice::WORDS_XML_CHAR);
        let t = text(vec![TextPart::Chars(Arc::from("a".repeat(MOST_TEXT + 1)))]);
        assert_eq!(model_paragraphs(&t, &mut 0).unwrap_err(), WORDS_TEXT_SIZE);
    }

    #[test]
    fn the_characters_stamp_is_the_one_the_reader_made_when_the_pieces_are_the_same() {
        let a = text(vec![chars("ab"), TextPart::Cp(1), chars("cd")]);
        let p = &model_paragraphs(&a, &mut 0).unwrap().paras[0];
        let same = characters_stamp(p, [(2u32, 4u32)].into_iter());
        assert_eq!(same, characters_stamp(p, [(2u32, 4u32)].into_iter()));
        assert_ne!(same, characters_stamp(p, [(2u32, 3u32)].into_iter()), "the places of the spans are in it");
        // The same characters on one row are one piece, on two rows two.
        let one = text(vec![chars("abcd")]);
        let q = &model_paragraphs(&one, &mut 0).unwrap().paras[0];
        assert_ne!(characters_stamp(q, std::iter::empty()), characters_stamp(p, std::iter::empty()));
    }
}
