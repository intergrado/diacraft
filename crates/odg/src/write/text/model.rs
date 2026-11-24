//! A shape's text, written from the model (the native-format design's
//! §3.3 and §3.4): **the model writer writes every paragraph, run, list, heading and field a new shape holds as the markup an `.odg` has for it**, and where the file can say all of the text,
//! DiaCraft's extension carries none of it.
//!
//! * **Paragraphs and runs.** Each paragraph is a `text:p` (`text:h` when the file's own was one); each stretch of characters under one `Character` row that writes an attribute is a
//!   `text:span` of an automatic text style, a paragraph whose `Paragraph` row writes one names an automatic paragraph style. White space is written as R24 has it (`text:s`, `text:tab`,
//!   `text:line-break`), through the writer a retext uses ([`Spaces`]). **A carriage return is a paragraph break**: the file has no character for it, and the exact text is carried.
//! * **Held names, values and markup, while their stamps hold.** The reader held, for each paragraph of the file, the style it named, the lists it was in, the element it was (`text:h`) and the
//!   elements round its runs (links, fields), each beside the stamp of what it stood for. A model paragraph is **the file's paragraph** when the stamp of its characters is the one the file's had
//!   ([`characters_stamp`]); the style name, the held style values (an alignment spelled `start`, an underline's colour) and the lists and elements are then written as they were **while the stamp of
//!   the row they rest on still holds**, and from the cells when it does not (a new name, by R27).
//! * **Lists** are `text:list` and `text:list-item` as held (a numbered list, a nested list, a list's own attributes), or, for a paragraph whose row says a bullet and has no held lists, a list with
//!   an automatic list style, the paragraphs of one row at one place being the items of one list.
//! * **Fields.** `PAGENUMBER()`, `PAGECOUNT()`, `FILENAME()` and `NOW()` are `text:page-number`, `text:page-count`, `text:file-name` and `text:date`, as the reader reads them; any other field is its
//!   shown text, and the save says so ([`SaveNote::FieldAsText`]).
//! * **What the file cannot say is carried** ([`Rich::native`] false): a carriage return, a tab stop marker, a row with a cell no attribute holds or one that is a formula, a row **no held stamp
//!   confirms** (the reader would number it and give it the cells of its chain, which are not the model's), a field the file cannot hold whole. DiaCraft's extension then carries the text and its rows as
//!   it always did, and the file shows the text all the same.
//!
//! **Hostile input.** Every paragraph, run, list level and field is charged before it is made; a list is at most [`MAX_LIST_LEVELS`] and the depth of the files DiaCraft reads deep; the sweep that opens and
//! closes the elements of a paragraph is linear in its runs, and keeps its stack on the heap.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Document, HeldStyleValue, MAX_LIST_LEVELS, OdfListLevel, OdfParagraph, OdfRun, OdfText, RawElement, RawNode, Row, Shape, Sheet, stamp_text_group,
};

use super::super::emit::Pfx;
use super::super::frame::Editor;
use super::super::runs::Props;
use super::super::runs::model::{Inline, ModelPara, ModelRows, characters_stamp, model_paragraphs};
use super::super::scan::{Fam, ScanPage};
use super::super::stylemap::{Op, Val};
use super::super::styles::Written;
use super::super::zero::Spaces;
use super::super::{SaveNote, WriteError, WriteWork};
use crate::read::names::Ns;

fn refuse<T>(words: &'static str) -> Result<T, WriteError> {
    Err(WriteError::Unsupported(words))
}

/// How many held paragraphs after the last matched one a paragraph of the model is looked for among: a paragraph typed or taken out between two the file had moves the match by one.
const LOOKAHEAD: usize = 4;

/// The words of a refusal of a list nested deeper than the files DiaCraft reads.
const WORDS_LIST_DEPTH: &str = "a list nested deeper than the files DiaCraft reads allow (each level of a list is two levels of the part's markup)";

/// The words of a refusal of a text with more list levels than the model holds at most.
const WORDS_LIST_LEVELS: &str = "a paragraph in more lists than a drawing holds (256)";

/// The levels of markup a list level takes (the list and its item).
const MARKUP_PER_LEVEL: usize = 2;

/// What the model path wrote of one shape's text.
#[derive(Debug)]
pub(crate) struct Rich {
    /// The paragraphs and lists, as markup, to go in the shape's element.
    pub xml: String,
    /// What each paragraph reads as, by its place among the shape's paragraphs: what the check of the bytes made looks for. A paragraph that holds an element the check does not map (a link) is not in it.
    pub read: Vec<(usize, String)>,
    /// The plain characters the file reads as, the paragraphs joined by U+000A.
    pub plain: String,
    /// The file says all of the text: DiaCraft's extension carries none of it, and none of its rows.
    pub native: bool,
}

/// The element kinds of a field the reader maps to a `Field` row, by the formula the row holds: the local name of its element.
fn field_element(formula: &str) -> Option<&'static str> {
    let f: String = formula.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    Some(match f.as_str() {
        "PAGENUMBER()" => "page-number",
        "PAGECOUNT()" => "page-count",
        "FILENAME()" => "file-name",
        "NOW()" => "date",
        _ => return None,
    })
}

/// Whether the local name `local` names a field of the text namespace whose content the reader takes as it stands (so it is written as plain text, with no `text:s`).
fn is_field_name(local: &str) -> bool {
    matches!(local, "page-number" | "page-count" | "file-name") || super::super::zero::field_name(local).is_some()
}

/// The local name of an element name as written.
fn local_of(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// The held names of the automatic styles the paragraphs and runs of every text of `doc` named, so that **no new name of the save is one of them** (a new style never takes a name the file's
/// paragraphs may name). Walks the shapes of every page and master with a stack of its own, charging each shape, paragraph and run. The family is the paragraph or text family.
pub(crate) fn held_names(doc: &Document, work: &mut WriteWork) -> Result<Vec<(Fam, String)>, WriteError> {
    let mut out: Vec<(Fam, String)> = Vec::new();
    let mut seen: HashSet<(bool, Arc<str>)> = HashSet::new();
    let mut stack: Vec<&Arc<Shape>> = Vec::new();
    let mut note = |family: Fam, name: Option<&Arc<str>>, out: &mut Vec<(Fam, String)>| {
        if let Some(name) = name
            && seen.insert((family == Fam::Text, Arc::clone(name)))
        {
            out.push((family, name.to_string()));
        }
    };
    stack.extend(doc.pages.iter().flat_map(|p| p.shapes.iter()));
    stack.extend(doc.masters.iter().flat_map(|m| m.shapes.iter()));
    while let Some(shape) = stack.pop() {
        work.charge_steps(1)?;
        if let Some(odf) = shape.text.as_ref().and_then(|t| t.ext.get()).map(|e| &e.odf) {
            for para in &odf.paragraphs {
                work.charge_steps(1 + para.runs.len())?;
                note(Fam::Paragraph, para.style.name.as_ref(), &mut out);
                for run in &para.runs {
                    note(Fam::Text, run.style.name.as_ref(), &mut out);
                }
            }
        }
        stack.extend(shape.children.iter());
    }
    Ok(out)
}

/// The document's colour and font tables, indexed once for a save: a colour's `#rrggbb` by its index, a face's name by the number its table gives it. The rows of every shape's text are
/// read through them, so a lookup is one probe and no scan of the document's lists (the security scan's third finding).
#[derive(Debug, Default)]
pub(crate) struct Tables {
    palette: HashMap<i64, String>,
    fonts: HashMap<i64, String>,
}

/// The most entries the font table of a document holds (`diacraft_doc::MAX_STYLES` is the nearest cap the model checks; a table past it is read as far as it goes).
const MOST_FACES: usize = 1 << 17;

/// How the lists round a paragraph are written.
struct Want {
    list: Option<Tags>,
    item: Option<Tags>,
}

/// The start and end tags of an element.
#[derive(Clone)]
struct Tags {
    open: String,
    close: String,
}

/// One level of the lists open.
struct Level {
    close_list: String,
    /// The end tag of the item open at this level.
    close_item: Option<String>,
}

/// An element round a stretch of a paragraph's characters, opened where the stretch starts and closed where it ends.
struct Cover {
    from: usize,
    to: usize,
    tags: Tags,
    /// The content of the element is plain text the reader takes as it stands (a field's).
    raw: bool,
    /// The element is one the file held (a link, a span with an attribute), and not a run the model's own rows say: when it crosses another it is not written.
    held: bool,
}

/// A field element of a paragraph: where its characters are and its tags.
struct FieldAt {
    at: usize,
    len: usize,
    bytes: (usize, usize),
    tags: Tags,
}

/// What a model paragraph has of the held paragraph it is the file's, when it is.
struct Held<'h> {
    para: &'h OdfParagraph,
    /// The stamp of the row of the model's paragraph is the one the file's was: the held names, lists and values rest on the row.
    row_holds: bool,
}

/// One paragraph's markup, with whether the check of the bytes made can read it back.
struct Written1 {
    xml: String,
    checked: bool,
}

/// What the paragraphs of a text found, together: whether the file can say all of it, the rows used, and the fields written as text.
struct Facts {
    native: bool,
    used_para: HashSet<u32>,
    used_char: HashSet<u32>,
    used_field: HashSet<u32>,
    as_text: usize,
    /// The held elements left out because they cross another element of their paragraph (a file's own markup nests, the model's runs need not).
    crossing: usize,
}

/// An attribute of a kept style's properties element that is in a namespace of its own (a producer's extension, such as `loext:` or `officeooo:`) or in none: it is written as the file had it,
/// on the properties element it was read from, with the declaration of its prefix on that element. `prefix` is the one the file wrote it with (`""` for no namespace).
#[derive(Clone, Debug)]
struct Foreign {
    prefix: String,
    uri: String,
    local: String,
    value: String,
}

/// What a kept text or paragraph style holds of its properties elements that no cell reads ([`Editor::text_remainder`]): the attributes of the element the style is written with, the ones
/// of a paragraph style's text properties (an element of their own), each in the namespaces the writer writes with a prefix of its own and in the namespaces it does not.
#[derive(Debug, Default)]
struct Remainder {
    same: Vec<(Ns, String, String)>,
    other: Vec<(Ns, String, String)>,
    same_foreign: Vec<Foreign>,
    other_foreign: Vec<Foreign>,
}

impl Editor<'_, '_> {
    /// The text of `shape` written from the model, or `None` for a shape with no text (an empty one, as the file holds none: the plain path writes its one empty paragraph). `page_at` is the page's
    /// place in the model, for the notes.
    pub(crate) fn rich_text(&mut self, shape: &Shape, page: &ScanPage, pfx: &Pfx, page_at: usize) -> Result<Option<Rich>, WriteError> {
        if shape.text.is_none() {
            return Ok(None);
        }
        self.build_tables()?;
        let tables = self.texts.tables.take().unwrap_or_default();
        let result = self.rich_text_with(shape, page, pfx, page_at, &tables);
        self.texts.tables = Some(tables);
        result
    }

    /// The colour and font tables of the document, indexed once for the save.
    fn build_tables(&mut self) -> Result<(), WriteError> {
        if self.texts.tables.is_some() {
            return Ok(());
        }
        let doc = self.doc;
        self.work.charge_steps(doc.colors.len().saturating_add(doc.fonts.len()).saturating_add(24))?;
        let mut tables = Tables::default();
        let hex = |index: i64| doc.color(index).map(|c| format!("#{}", c.hex().to_ascii_lowercase()));
        for index in 0..24i64 {
            if let Some(colour) = hex(index) {
                tables.palette.insert(index, colour);
            }
        }
        // The table's own entries (at most 4,096): the first of an index is the one the model finds.
        for entry in doc.colors.iter() {
            let index = i64::from(entry.ix);
            if !tables.palette.contains_key(&index)
                && let Some(colour) = hex(index)
            {
                tables.palette.insert(index, colour);
            }
        }
        for face in doc.fonts.iter().take(MOST_FACES) {
            if let Some(id) = face.raw.get("ID").and_then(|t| t.parse::<i64>().ok()) {
                tables.fonts.entry(id).or_insert_with(|| face.name_u.to_string());
            }
        }
        self.texts.tables = Some(tables);
        Ok(())
    }

    fn rich_text_with(&mut self, shape: &Shape, page: &ScanPage, pfx: &Pfx, page_at: usize, tables: &Tables) -> Result<Option<Rich>, WriteError> {
        let Some(text) = shape.text.as_ref() else { return Ok(None) };
        let palette = |index: i64| tables.palette.get(&index).cloned();
        let fonts = |place: i64| tables.fonts.get(&place).cloned();
        let mut steps = 0u64;
        let model = model_paragraphs(text, &mut steps).map_err(WriteError::Unsupported)?;
        self.work.charge_steps(usize::try_from(steps).unwrap_or(usize::MAX))?;
        // A text of one empty paragraph is no text: the plain path writes the one empty paragraph.
        if model.paras.len() == 1 && model.paras.first().is_none_or(|p| p.chars.is_empty() && p.inlines.is_empty()) {
            return Ok(None);
        }
        let sheet: &Sheet = &shape.sheet;
        let mut steps = 0u64;
        let mut para_rows = ModelRows::of(sheet, true, &palette, &fonts, &mut steps);
        let mut char_rows = ModelRows::of(sheet, false, &palette, &fonts, &mut steps);
        // The `Field` rows by `IX`, once for the shape: a field finds its row by one probe.
        let mut field_rows: HashMap<u32, &Row> = HashMap::new();
        for section in sheet.sections.iter().filter(|s| s.name.as_str() == "Field") {
            for row in section.rows.iter() {
                steps = steps.saturating_add(1);
                if let Some(ix) = row.ix {
                    field_rows.entry(ix).or_insert(row);
                }
            }
        }
        self.work.charge_steps(usize::try_from(steps).unwrap_or(usize::MAX))?;
        let held: Option<&OdfText> = text.ext.get().map(|e| &e.odf);
        // Which paragraph of the model is which paragraph of the file.
        let mut ignored_runs = 0usize;
        let matched = match_held(&model.paras, held, &mut para_rows, self.work, &mut ignored_runs)?;
        let mut facts = Facts {
            native: !model.carriage_returns && !model.unsaid_parts && model.fields_as_text == 0,
            used_para: HashSet::new(),
            used_char: HashSet::new(),
            used_field: HashSet::new(),
            as_text: model.fields_as_text,
            crossing: ignored_runs,
        };
        self.texts.unbound = 0;
        let mut out = String::new();
        let mut read: Vec<(usize, String)> = Vec::new();
        let mut plain = String::new();
        let mut stack: Vec<Level> = Vec::new();
        // The list the last paragraph was an item of that its row made, so that the paragraphs of one row at one place are the items of one list.
        let mut made_list: Option<String> = None;
        let limit = self.list_limit();
        for (k, para) in model.paras.iter().enumerate() {
            self.work.charge_steps(1 + para.stretches.len() + para.inlines.len())?;
            let h = matched.get(k).and_then(Option::as_ref);
            if k > 0 {
                plain.push('\n');
            }
            // The paragraph's characters are copied twice more, for what the file reads as and for the check of the bytes made.
            self.work.charge_copy(para.chars.len().saturating_mul(2), self.limits)?;
            plain.push_str(&para.chars);
            // The row of the paragraph, and whether the file said what the model says of it.
            let pp = para_rows.props(para.pp);
            facts.used_para.insert(para.pp);
            facts.native &= !pp.as_ref().is_some_and(|(_, lossy)| *lossy);
            if pp.is_some() && !h.is_some_and(|h| h.row_holds) {
                facts.native = false;
            }
            let pp_props = pp.as_ref().map(|(p, _)| p);
            let list_depth = match h.filter(|h| h.row_holds) {
                Some(h) => h.para.levels.len(),
                None => usize::from(pp_props.is_some_and(|p| p.bullet)),
            };
            let written = self.paragraph(
                &Ctx { para, held: h, pp: pp_props, pp_row: para_rows.row(para.pp), page, pfx, text_prefix: &pfx.text, list_depth },
                &mut char_rows,
                &field_rows,
                &mut facts,
            )?;
            // The lists round it.
            let want = self.levels_for(h, pp_props, page, pfx, &mut made_list, limit)?;
            self.lists(&mut out, &mut stack, want, &pfx.text)?;
            if written.checked {
                read.push((k, para.chars.clone()));
            }
            out.push_str(&written.xml);
        }
        // The lists still open are closed.
        self.lists(&mut out, &mut stack, Vec::new(), &pfx.text)?;
        // Every row of the three sections is one a paragraph, run or field uses, or the file does not say all of the text.
        facts.native &= para_rows.ixs().all(|ix| facts.used_para.contains(&ix));
        facts.native &= char_rows.ixs().all(|ix| facts.used_char.contains(&ix));
        facts.native &= field_rows.keys().all(|ix| facts.used_field.contains(ix));
        if facts.as_text > 0 {
            self.notes.push(SaveNote::FieldAsText { page: page_at, shape: shape.id, fields: facts.as_text });
        }
        if facts.crossing > 0 {
            self.notes.push(SaveNote::TextElementsNotWritten { page: page_at, shape: shape.id, elements: facts.crossing });
        }
        if self.texts.unbound > 0 {
            self.notes.push(SaveNote::TextMarkupUnbound { page: page_at, shape: shape.id, elements: std::mem::take(&mut self.texts.unbound) });
        }
        Ok(Some(Rich { xml: out, read, plain, native: facts.native }))
    }

    /// The deepest nesting of held elements a paragraph may have: the depth the parts the reader reads are held to, less the root, the body, the drawing, the page, the shape and the paragraph
    /// (and the two levels each list round it takes), and two more for the run, field or mark inside the deepest.
    fn cover_limit(&self, list_depth: usize) -> usize {
        self.limits.container.xml.max_depth.saturating_sub(7 + 2 * list_depth + 2)
    }

    /// The deepest list the part may hold: the markup each level takes, from the depth the parts the reader reads are held to.
    fn list_limit(&self) -> usize {
        let depth = self.limits.container.xml.max_depth;
        // The root, the body, the drawing, the page, the shape, and a paragraph and a run inside the last item.
        (depth.saturating_sub(8) / MARKUP_PER_LEVEL).min(MAX_LIST_LEVELS)
    }
}

/// The most runs a held paragraph may have to be matched with a paragraph of the model (a paragraph past it is written from the cells, and its elements are said).
const MOST_HELD_RUNS: usize = 1 << 20;

/// The held paragraph each model paragraph is, by the stamp of its characters: **in order**, each looked for among the next [`LOOKAHEAD`] held paragraphs after the one matched last.
///
/// **The work is held to what the two texts are.** A candidate is skipped, with no hashing, when one of its runs ends past the model paragraph's last character (the stamp holds the
/// count: it cannot match); one that is hashed costs `1 + characters / 64 + its runs` steps, **charged before it is hashed**; the steps of all the candidates of the shape are held to
/// eight times the paragraphs, the held runs and the characters / 64 of the shape, which a paragraph of the file that matches nothing (looked at again for each paragraph of the model
/// that follows) would pass: past it no more paragraphs are matched, and what is left is written from the cells. `left` counts the runs of the held paragraphs too large to match.
fn match_held<'h>(
    paras: &[ModelPara<'_>],
    held: Option<&'h OdfText>,
    rows: &mut ModelRows<'_>,
    work: &mut WriteWork,
    left: &mut usize,
) -> Result<Vec<Option<Held<'h>>>, WriteError> {
    let mut out: Vec<Option<Held<'h>>> = Vec::new();
    out.try_reserve_exact(paras.len())
        .map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape's text", max: paras.len() as u64 })?;
    let Some(held) = held.filter(|h| !h.paragraphs.is_empty()) else {
        out.resize_with(paras.len(), || None);
        return Ok(out);
    };
    work.charge_steps(held.paragraphs.len())?;
    let total_runs = held.paragraphs.iter().fold(0usize, |sum, h| sum.saturating_add(h.runs.len()));
    let characters = paras.iter().fold(0usize, |sum, p| sum.saturating_add(p.chars.len() / 64));
    let mut budget = paras.len().saturating_add(total_runs).saturating_add(characters).saturating_mul(8).saturating_add(1024);
    // The last character any run of a held paragraph ends at, made once for each (a step for each run).
    let mut ends: HashMap<usize, u32> = HashMap::new();
    let mut too_big: HashSet<usize> = HashSet::new();
    let mut next = 0usize;
    let mut exhausted = false;
    for para in paras {
        let mut found: Option<usize> = None;
        for at in next..held.paragraphs.len().min(next.saturating_add(LOOKAHEAD)) {
            if exhausted {
                break;
            }
            let Some(h) = held.paragraphs.get(at) else { break };
            let runs = h.runs.len();
            if runs > MOST_HELD_RUNS {
                if too_big.insert(at) {
                    *left = left.saturating_add(runs);
                }
                continue;
            }
            let last = match ends.get(&at) {
                Some(last) => *last,
                None => {
                    work.charge_steps(runs)?;
                    let last = h.runs.iter().map(|r| r.to.max(r.from)).max().unwrap_or(0);
                    ends.insert(at, last);
                    last
                }
            };
            if last as usize > para.count {
                continue;
            }
            let cost = 1usize.saturating_add(para.chars.len() / 64).saturating_add(runs);
            if cost > budget {
                exhausted = true;
                break;
            }
            budget -= cost;
            work.charge_steps(cost)?;
            let spans = h.runs.iter().map(|r| (r.from, r.to));
            if h.characters == Some(characters_stamp(para, spans)) {
                found = Some(at);
                break;
            }
        }
        match found.and_then(|at| held.paragraphs.get(at).map(|h| (at, h))) {
            Some((at, h)) => {
                next = at + 1;
                let current = rows.stamp(para.pp);
                out.push(Some(Held { para: h, row_holds: h.style.stamp == Some(current) }));
            }
            None => out.push(None),
        }
    }
    Ok(out)
}

/// The runs of a paragraph the file held, by the characters each covers.
type RunsByRange<'a> = HashMap<(usize, usize), &'a OdfRun>;

/// Whether a span of the file stands for a stretch of the model whose row has the stamp `row_stamp`: the span was read with that row (its stamp is the row's), **or it was read with
/// none because its own text is the text of an element inside it, over the same characters** (`<text:span><text:a>here</text:a></text:span>`: a span that has no characters of its own has no
/// row of its own, and its stamp is the empty row's; the characters of the element read through the span's style, which is the model's row for them). `holds_element` says an element of the file
/// lies over the same characters.
fn stands_for_row(run: &OdfRun, row_stamp: diacraft_doc::Stamp, holds_element: bool) -> bool {
    run.style.stamp == Some(row_stamp) || (holds_element && run.style.stamp == Some(diacraft_doc::stamp_text_row(&Row::default())))
}

/// What the writer of one paragraph is given.
struct Ctx<'a, 'p> {
    para: &'a ModelPara<'p>,
    held: Option<&'a Held<'a>>,
    pp: Option<&'a Arc<Props>>,
    pp_row: Option<&'a Row>,
    page: &'a ScanPage,
    pfx: &'a Pfx,
    text_prefix: &'a str,
    /// How many lists the paragraph is in, as they are written.
    list_depth: usize,
}

impl Editor<'_, '_> {
    /// One paragraph's element, with its style and its content.
    fn paragraph(
        &mut self,
        cx: &Ctx<'_, '_>,
        char_rows: &mut ModelRows<'_>,
        field_rows: &HashMap<u32, &Row>,
        facts: &mut Facts,
    ) -> Result<Written1, WriteError> {
        let pfx = cx.pfx;
        // The paragraph's style: the name the file gave it while the row is the file's, else one made from the row's cells.
        let style = self.paragraph_style_name(cx)?;
        let (open, close) = match cx.held.and_then(|h| h.para.element.as_ref()) {
            Some(shell) => {
                let attr = style.as_deref().map(|s| (format!("{}:style-name", prefix_of(&shell.name, cx.text_prefix)), s.to_owned()));
                match self.shell_tags(shell, attr, cx.page, pfx)? {
                    Some(tags) => (tags.open, tags.close),
                    None => self.plain_paragraph_tags(style.as_deref(), pfx)?,
                }
            }
            None => self.plain_paragraph_tags(style.as_deref(), pfx)?,
        };
        let held_runs: &[OdfRun] = cx.held.map_or(&[], |h| h.para.runs.as_slice());
        // Every held run is a step, charged before its element is made.
        self.work.charge_steps(held_runs.len())?;
        let field_ranges: HashSet<(usize, usize)> = cx
            .para
            .inlines
            .iter()
            .filter_map(|i| if let Inline::Field { at, len, .. } = i { Some((*at, at.saturating_add(*len))) } else { None })
            .collect();
        let mut covers: Vec<Cover> = Vec::new();
        let mut marks: Vec<(usize, String)> = Vec::new();
        let mut span_by: HashMap<(usize, usize), &OdfRun> = HashMap::new();
        let mut elem_by: HashMap<(usize, usize), &OdfRun> = HashMap::new();
        let mut checked = true;
        // Held elements: links, fields the model has no cell for, and any other element round a run.
        for run in held_runs {
            let range = (run.from as usize, run.to as usize);
            if run.style.stamp.is_some() {
                // A `text:span`: written for the model's own run that is its place.
                span_by.entry(range).or_insert(run);
                continue;
            }
            let Some(shell) = run.element.as_ref() else { continue };
            elem_by.entry(range).or_insert(run);
            // A field the model holds as a field is written with its inline.
            if field_ranges.contains(&range) {
                continue;
            }
            if range.0 > range.1 || range.1 > cx.para.count {
                continue;
            }
            let Some(tags) = self.shell_tags(shell, None, cx.page, pfx)? else { continue };
            let local = local_of(&shell.name);
            let raw = is_field_name(local);
            // The check of the bytes made reads the elements it maps (spans, the fields of the standard's list, marks); any other makes the paragraph one it does not read.
            if !raw {
                checked = false;
            }
            if range.0 == range.1 {
                marks.push((range.0, format!("{}{}", tags.open, tags.close)));
            } else {
                covers.push(Cover { from: range.0, to: range.1, tags, raw, held: true });
            }
        }
        // The model's own runs: the spans.
        let mut stretches: Vec<(usize, usize, u32)> = Vec::new();
        for s in &cx.para.stretches {
            match stretches.last_mut() {
                Some(last) if last.1 == s.start && same_run_props(char_rows, last.2, s.row) => last.1 = s.end,
                _ => stretches.push((s.start, s.end, s.row)),
            }
        }
        // **The runs the file had inside a stretch of one row are written as they were**: the model holds three runs of one style as one stretch of one row, and one span
        // over it was no span of the file's, so the row was "not confirmed" and the text rode in DiaCraft's data whole (a paragraph the reader then read back with the markup's runs
        // forgotten: the hyperlinks among them, and a check that found one run where it had three).
        let stretches = self.split_at_held_runs(stretches, (&span_by, &elem_by), char_rows)?;
        for (start, end, row) in stretches {
            facts.used_char.insert(row);
            let Some((props, lossy)) = char_rows.props(row) else { continue };
            facts.native &= !lossy;
            if !props.writes() {
                continue;
            }
            // A row no held span says is not confirmed: the file would number it again, and give it the cells of its chain.
            let held_run = span_by.get(&(start, end)).copied();
            let row_stamp = char_rows.stamp(row);
            let confirmed = held_run.is_some_and(|r| stands_for_row(r, row_stamp, elem_by.contains_key(&(start, end))));
            if !confirmed {
                facts.native = false;
            }
            let Some(name) = self.run_style_name(&props, char_rows.row(row), held_run, confirmed)? else { continue };
            let shelled = match held_run.and_then(|r| r.element.as_ref()).filter(|_| confirmed) {
                // A span that holds an attribute of its own besides its style.
                Some(shell) => {
                    self.shell_tags(shell, Some((format!("{}:style-name", prefix_of(&shell.name, cx.text_prefix)), name.clone())), cx.page, pfx)?
                }
                None => None,
            };
            let tags = match shelled {
                Some(tags) => tags,
                None => {
                    self.work.charge_attribute(self.limits)?;
                    Tags { open: format!("<{}:span{}>", pfx.text, pfx.attr(Ns::Text, "style-name", &name)), close: format!("</{}:span>", pfx.text) }
                }
            };
            covers.push(Cover { from: start, to: end, tags, raw: false, held: false });
        }
        // The fields and marks.
        let mut fields: Vec<FieldAt> = Vec::new();
        for inline in &cx.para.inlines {
            match inline {
                Inline::Field { at, len, ix, bytes } => {
                    let shown = cx.para.chars.get(bytes.0..bytes.1).unwrap_or_default();
                    match field_kind(field_rows, *ix, shown) {
                        Some(local) => {
                            facts.used_field.insert(*ix);
                            // `NOW()` is the date or the time the file's element was (the reader maps both to it).
                            let shell = elem_by
                                .get(&(*at, at.saturating_add(*len)))
                                .and_then(|r| r.element.as_ref())
                                .filter(|e| local_of(&e.name) == local || (local == "date" && local_of(&e.name) == "time"));
                            let held_tags = match shell {
                                Some(shell) => self.shell_tags(shell, None, cx.page, pfx)?,
                                None => None,
                            };
                            let tags = held_tags
                                .unwrap_or_else(|| Tags { open: format!("<{}:{local}>", pfx.text), close: format!("</{}:{local}>", pfx.text) });
                            fields.push(FieldAt { at: *at, len: *len, bytes: *bytes, tags });
                        }
                        None => {
                            // A field no element of the file stands for: its shown text, said.
                            facts.as_text = facts.as_text.saturating_add(1);
                            facts.native = false;
                        }
                    }
                }
                Inline::Mark { at, node } => match node {
                    RawNode::Element(el) => match self.kept_element(el, cx.page, pfx)? {
                        Some((text, _)) => marks.push((*at, text)),
                        None => {
                            // A mark whose prefix the page binds nothing for is not written, and the save says so.
                            self.texts.unbound = self.texts.unbound.saturating_add(1);
                            facts.native = false;
                        }
                    },
                    _ => facts.native = false,
                },
            }
        }
        facts.crossing = facts.crossing.saturating_add(drop_crossing(&mut covers, self.cover_limit(cx.list_depth)));
        // The check of the bytes made reads a field's content as the text it is: an element inside one is no paragraph it reads (the file is read by the reader all the same).
        if covers.iter().zip(covers.iter().skip(1)).any(|(outer, next)| outer.raw && next.from < outer.to) {
            checked = false;
        }
        self.work.charge_steps(covers.len())?;
        let content = self.paragraph_content(cx.para, covers, marks, fields, cx.text_prefix)?;
        let xml = if content.is_empty() { format!("{open}{close}") } else { format!("{open}{content}{close}") };
        Ok(Written1 { xml, checked })
    }

    /// The stretches of a paragraph's text, each cut where the spans of the file that tile it end: a stretch (from, to, row) that **no span of the file covers exactly** and that **spans of the file
    /// cover from end to end, one after the other, each with the stamp of the stretch's row** (they are all of the row's formatting as read) is the file's spans again, one stretch each, so that
    /// each is confirmed and written under its own style name, as it was. A stretch that is covered exactly, or that the file's spans do not tile, or whose spans' stamps are not all the row's, is
    /// left as it is. **The cost** is a step for each stretch, and for each span the walk takes: a stretch's walk goes forward from its start and each place it stops at is a span that begins
    /// there, so the walks of the paragraph's stretches take each span of the file once at the most.
    fn split_at_held_runs(
        &mut self,
        stretches: Vec<(usize, usize, u32)>,
        (span_by, elem_by): (&RunsByRange<'_>, &RunsByRange<'_>),
        char_rows: &mut ModelRows<'_>,
    ) -> Result<Vec<(usize, usize, u32)>, WriteError> {
        // (A paragraph whose stretches the file's spans cover exactly, one each, is the common one, and is passed over here: the walk is one look at each stretch, which the paragraph's own steps
        // have charged.)
        if span_by.is_empty() || stretches.iter().all(|(start, end, _)| span_by.contains_key(&(*start, *end))) {
            return Ok(stretches);
        }
        self.work.charge_steps(stretches.len().saturating_add(span_by.len()))?;
        // The ends of the file's spans, by where each begins.
        let mut ends: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(from, to) in span_by.keys().filter(|(from, to)| from < to) {
            ends.entry(from).or_default().push(to);
        }
        let mut out: Vec<(usize, usize, u32)> = Vec::with_capacity(stretches.len());
        for (start, end, row) in stretches {
            if span_by.contains_key(&(start, end)) {
                out.push((start, end, row));
                continue;
            }
            let stamp = char_rows.stamp(row);
            let mut pieces: Vec<(usize, usize, u32)> = Vec::new();
            let mut at = start;
            let tiled = loop {
                if at >= end {
                    break at == end;
                }
                // The longest span of the file that begins here and ends within the stretch, with the row's stamp.
                let next = ends.get(&at).and_then(|tos| tos.iter().copied().filter(|to| *to > at && *to <= end).max());
                let Some(to) = next else { break false };
                if !span_by.get(&(at, to)).is_some_and(|run| stands_for_row(run, stamp, elem_by.contains_key(&(at, to)))) {
                    break false;
                }
                pieces.push((at, to, row));
                at = to;
            };
            if tiled && pieces.len() > 1 {
                self.work.charge_steps(pieces.len())?;
                out.extend(pieces);
            } else {
                out.push((start, end, row));
            }
        }
        Ok(out)
    }

    /// The start and end tags of a `text:p` with a style.
    fn plain_paragraph_tags(&mut self, style: Option<&str>, pfx: &Pfx) -> Result<(String, String), WriteError> {
        let mut open = format!("<{}:p", pfx.text);
        if let Some(style) = style {
            self.work.charge_attribute(self.limits)?;
            open.push_str(&pfx.attr(Ns::Text, "style-name", style));
        }
        open.push('>');
        Ok((open, format!("</{}:p>", pfx.text)))
    }

    /// The name of the automatic (or common) paragraph style a paragraph names.
    fn paragraph_style_name(&mut self, cx: &Ctx<'_, '_>) -> Result<Option<String>, WriteError> {
        // The file's own paragraph, with its row as it was: the name it had (none is a name too), and the values it held.
        if let Some(h) = cx.held.filter(|h| h.row_holds) {
            return match h.para.style.name.as_deref() {
                None => Ok(None),
                Some(name) if self.commons.contains(name) => Ok(Some(name.to_owned())),
                Some(name) => match cx.pp {
                    Some(props) => self.held_style(Fam::Paragraph, name, props, cx.pp_row, &h.para.style.held),
                    None => Ok(None),
                },
            };
        }
        match cx.pp {
            Some(props) if props.writes() => self.style_for(Fam::Paragraph, props),
            _ => Ok(None),
        }
    }

    /// The name of the automatic text style a run names: the file's own when the run is the file's span and its row is as it was, else one made from the row.
    fn run_style_name(
        &mut self,
        props: &Arc<Props>,
        row: Option<&Row>,
        held_run: Option<&OdfRun>,
        confirmed: bool,
    ) -> Result<Option<String>, WriteError> {
        if confirmed && let Some(run) = held_run {
            return match run.style.name.as_deref() {
                None => Ok(None),
                Some(name) if self.commons.contains(name) => Ok(Some(name.to_owned())),
                Some(name) => self.held_style(Fam::Text, name, props, row, &run.style.held),
            };
        }
        self.style_for(Fam::Text, props)
    }

    /// An automatic style made under the name the file gave it, with the values the file held for it that the row still says, **once for each name** (the same name asked for with another row is a new
    /// style under a new name: two paragraphs of one file's style that the model has come to give different rows are two styles).
    fn held_style(
        &mut self,
        family: Fam,
        name: &str,
        props: &Arc<Props>,
        row: Option<&Row>,
        held: &[HeldStyleValue],
    ) -> Result<Option<String>, WriteError> {
        let key = (family, name.to_owned());
        let values: Vec<&HeldStyleValue> = held
            .iter()
            .filter(|v| v.group.is_paragraph() == (family == Fam::Paragraph) && row.is_some_and(|r| v.stamp == Some(stamp_text_group(r, v.group))))
            .collect();
        let mut ops = self.style_ops(props)?;
        // A run that stated no underline is not given one that says "none": the cells of the row hold no `DblUnderline`, and the file's style would read back with it.
        if !row.is_some_and(|r| r.cells.contains_key("DblUnderline")) {
            ops.retain(|o| !(o.local == "text-underline-style" && matches!(&o.val, Val::Text(t) if t == "none")));
        }
        if ops.is_empty() && values.is_empty() {
            return Ok(None);
        }
        // What the style says: the same name asked for again with the same content is the style made, and with another content a style of its own.
        let content = format!(
            "{}#{:?}",
            self.ops_key(&ops)?,
            values.iter().map(|v| v.attrs.iter().map(|(k, t)| format!("{k}={t}")).collect::<Vec<_>>()).collect::<Vec<_>>()
        );
        if let Some(made) = self.texts.held_made.get(&key) {
            return if *made == content { Ok(Some(name.to_owned())) } else { self.style_for(family, props) };
        }
        self.make_named_style(family, name, &ops, &values)?;
        self.work.text_names_written.insert((family == Fam::Text, name.to_owned()));
        self.texts.held_made.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
        self.texts.held_made.insert(key, content);
        Ok(Some(name.to_owned()))
    }

    /// The kept styles of the two families by name, and the namespaces the root of the part and the container of its automatic styles bound as read: found once for the save, charged by what
    /// they walk (the security scan's finding: each style looked for scanned every kept style).
    fn index_text_styles(&mut self, odf: &diacraft_doc::OdfDoc) -> Result<(), WriteError> {
        use super::super::kept::{Bindings, declare_into};
        if !self.book.as_ref().is_some_and(|b| b.text_remainders.is_none()) {
            return Ok(());
        }
        let held_roots: Vec<&diacraft_doc::RawAttrs> = ["content.xml", "content.xml/office:automatic-styles"]
            .iter()
            .filter_map(|key| odf.roots.iter().find(|(n, _)| &**n == *key).map(|(_, a)| a))
            .collect();
        self.work.charge_steps(odf.styles.len().saturating_add(held_roots.iter().map(|a| a.len()).sum::<usize>()).saturating_add(1))?;
        let mut index: HashMap<(bool, String), usize> = HashMap::new();
        for (at, kept) in odf.styles.iter().enumerate() {
            let text = match &*kept.family {
                "auto:text" => true,
                "auto:paragraph" => false,
                _ => continue,
            };
            index.entry((text, kept.name.to_string())).or_insert(at);
        }
        let mut bound = Bindings::new();
        for attrs in held_roots {
            declare_into(&mut bound, attrs.iter());
        }
        if let Some(book) = self.book.as_mut() {
            book.text_remainders = Some((index, bound));
        }
        Ok(())
    }

    /// The attributes the file's automatic style `name` of `family` kept that no cell reads (the model's kept style of `auto:paragraph` or `auto:text`) that neither `ops` nor `extra` writes: those
    /// of the element the style is written with, and, for a paragraph style, those of its text properties (written as an element of their own after). **Each is written where it was**: one in the
    /// namespaces a properties element is written with (`fo`, `style`) under the writer's own prefix, one in another namespace (a producer's extension) or in none as the file had it, with the
    /// declaration of its prefix, which the properties element, the style, the container of the automatic styles or the root declared. **What cannot be written is counted for the
    /// note** (a name that is no qualified name, a value with a character XML does not allow, a prefix nothing binds, a prefix the writer has for another namespace, or one the element binds twice).
    /// Each attribute is a step.
    fn text_remainder(&mut self, family: Fam, name: &str, ops: &[Op], extra: &[(Ns, String, String)]) -> Result<Remainder, WriteError> {
        use super::super::kept::{Bindings, declare_into, declared_by, is_qname};
        let doc = self.doc;
        let Some(odf) = doc.ext.get().map(|d| &d.odf) else { return Ok(Remainder::default()) };
        self.index_text_styles(odf)?;
        let Some((index, root)) = self.book.as_ref().and_then(|b| b.text_remainders.as_ref()) else { return Ok(Remainder::default()) };
        let Some(kept) = index.get(&(family == Fam::Text, name.to_owned())).and_then(|at| odf.styles.get(*at)) else {
            return Ok(Remainder::default());
        };
        let el = &kept.element;
        // Each child and each attribute is a step, charged before the walk.
        let attrs = el.children.iter().fold(0usize, |sum, c| sum.saturating_add(if let RawNode::Element(e) = c { e.attrs.len() } else { 0 }));
        self.work.charge_steps(el.children.len().saturating_add(attrs).saturating_add(el.attrs.len()))?;
        // The namespace of a prefix where an attribute stands: the properties element's own declarations, then the style's, then the container's and the root's as read (never the prefix's spelling).
        let declared = |attrs: &diacraft_doc::RawAttrs| -> HashMap<String, String> {
            let mut own = Bindings::new();
            declare_into(&mut own, attrs.iter());
            own
        };
        let style_own = declared(&el.attrs);
        // What the cells and the held values write, by namespace and local name: an attribute of the remainder that names one of them is theirs.
        let written: HashSet<(Ns, &str)> = ops.iter().map(|o| (o.ns, o.local)).chain(extra.iter().map(|(n, l, _)| (*n, l.as_str()))).collect();
        let main = if family == Fam::Text { "text-properties" } else { "paragraph-properties" };
        let mut out = Remainder::default();
        let mut left = 0usize;
        // One copy of each attribute on each element (the security scan's finding: two properties elements of one name, or one attribute under two prefixes, wrote it twice).
        let mut seen_same: HashSet<(Ns, String)> = HashSet::new();
        let mut seen_other: HashSet<(Ns, String)> = HashSet::new();
        let mut foreign_same: HashSet<(String, String)> = HashSet::new();
        let mut foreign_other: HashSet<(String, String)> = HashSet::new();
        for child in &el.children {
            let RawNode::Element(props) = child else { continue };
            let local_name = props.name.rsplit(':').next().unwrap_or_default();
            let is_main = local_name == main;
            if !is_main && !(family == Fam::Paragraph && local_name == "text-properties") {
                continue;
            }
            let (target, seen, foreign_target, foreign_seen) = if is_main {
                (&mut out.same, &mut seen_same, &mut out.same_foreign, &mut foreign_same)
            } else {
                (&mut out.other, &mut seen_other, &mut out.other_foreign, &mut foreign_other)
            };
            let own = declared(&props.attrs);
            for (key, text) in props.attrs.iter() {
                if declared_by(key).is_some() {
                    continue;
                }
                if !is_qname(key) || super::super::splice::has_bad_char(text) {
                    left = left.saturating_add(1);
                    continue;
                }
                let (prefix, local) = key.split_once(':').unwrap_or(("", key));
                // An attribute with no prefix is in no namespace, and needs no declaration.
                let uri = if prefix.is_empty() {
                    Some("")
                } else {
                    own.get(prefix).or_else(|| style_own.get(prefix)).or_else(|| root.get(prefix)).map(String::as_str).filter(|uri| !uri.is_empty())
                };
                let ns = match uri {
                    Some(super::super::scan::FO_URI | "http://www.w3.org/1999/XSL/Format") => Some(Ns::Fo),
                    Some(super::super::scan::STYLE_URI) => Some(Ns::Style),
                    _ => None,
                };
                match (ns, uri) {
                    (Some(ns), _) => {
                        if !written.contains(&(ns, local)) && seen.insert((ns, local.to_owned())) {
                            target.push((ns, local.to_owned(), text.to_owned()));
                        }
                    }
                    // **An attribute of the standard's own that the schema of the version written has no place for there** (a file of ODF 1.0 or 1.1 takes any): not written as it stands. One that an earlier
                    // version had and no later has rides in DiaCraft's own attribute on the same element (`diacraft:older-...`, as a style sheet's rides in the document file), and
                    // is neither counted nor said; any other is left out and counted. (A part of ODF 1.0 or 1.1 takes any attribute there and would be written as it was; the model writer, the only writer
                    // of a style's remainder, labels every part 1.4, so the guard is the rule for a part nothing writes today.)
                    (None, Some(uri)) if !self.legacy && crate::ext::odf14::refused_on_properties(local_name, uri, local) => {
                        match crate::ext::odf14::carrier_on_properties(local_name, uri, local) {
                            Some(carrier) => {
                                if foreign_seen.insert((diacraft_doc::DIACRAFT_URN.to_owned(), carrier.to_owned())) {
                                    foreign_target.push(Foreign {
                                        prefix: crate::ext::names::PREFIX.to_owned(),
                                        uri: diacraft_doc::DIACRAFT_URN.to_owned(),
                                        local: carrier.to_owned(),
                                        value: text.to_owned(),
                                    });
                                }
                            }
                            None => left = left.saturating_add(1),
                        }
                    }
                    (None, Some(uri)) => {
                        if foreign_seen.insert((uri.to_owned(), local.to_owned())) {
                            foreign_target.push(Foreign {
                                prefix: prefix.to_owned(),
                                uri: uri.to_owned(),
                                local: local.to_owned(),
                                value: text.to_owned(),
                            });
                        }
                    }
                    // A prefix nothing binds: the attribute has no namespace to be written in.
                    (None, None) => left = left.saturating_add(1),
                }
            }
        }
        self.texts.style_attributes_left = self.texts.style_attributes_left.saturating_add(left);
        Ok(out)
    }

    /// The text of the attributes `foreign` of a properties element under the prefixes the file wrote them with, with the declaration of each prefix before them, **charged first**. An attribute whose
    /// prefix the writer has for another namespace on this element (`pfx`'s own), or that the element binds to another namespace already, cannot be written and is counted for the note.
    fn foreign_attributes(&mut self, pfx: &Pfx, foreign: &[Foreign]) -> Result<String, WriteError> {
        let writer = [
            (pfx.draw.as_str(), super::super::scan::DRAW_URI),
            (pfx.svg.as_str(), super::super::scan::SVG_URI),
            (pfx.fo.as_str(), super::super::scan::FO_URI),
            (pfx.style.as_str(), super::super::scan::STYLE_URI),
            (pfx.text.as_str(), super::super::scan::TEXT_URI),
        ];
        let mut bound: Vec<(&str, &str)> = Vec::new();
        let (mut declarations, mut attributes) = (String::new(), String::new());
        for f in foreign {
            if !f.prefix.is_empty() {
                match writer.iter().find(|(p, _)| *p == f.prefix) {
                    // The writer's own prefix for the same namespace: bound already where the style is written.
                    Some((_, uri)) if *uri == f.uri => {}
                    Some(_) => {
                        self.texts.style_attributes_left = self.texts.style_attributes_left.saturating_add(1);
                        continue;
                    }
                    None => match bound.iter().find(|(p, _)| *p == f.prefix) {
                        Some((_, uri)) if *uri == f.uri => {}
                        Some(_) => {
                            self.texts.style_attributes_left = self.texts.style_attributes_left.saturating_add(1);
                            continue;
                        }
                        None => {
                            self.work.charge_attribute(self.limits)?;
                            declarations.push_str(&format!(" xmlns:{}=\"{}\"", f.prefix, super::super::splice::escape_value(&f.uri, b'"')));
                            bound.push((f.prefix.as_str(), f.uri.as_str()));
                        }
                    },
                }
            }
            self.work.charge_attribute(self.limits)?;
            let qname = if f.prefix.is_empty() { f.local.clone() } else { format!("{}:{}", f.prefix, f.local) };
            attributes.push_str(&format!(" {qname}=\"{}\"", super::super::splice::escape_value(&f.value, b'"')));
        }
        declarations.push_str(&attributes);
        Ok(declarations)
    }

    /// A new automatic style of `family` called `name`, with the attributes `ops` and the held values `held` (each of its attributes over the op that writes the same name).
    fn make_named_style(&mut self, family: Fam, name: &str, ops: &[Op], held: &[&HeldStyleValue]) -> Result<(), WriteError> {
        let (word, element, prop) = if family == Fam::Text { ("text", "text-properties", 2) } else { ("paragraph", "paragraph-properties", 1) };
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a text style without the styles of the part"))?;
        let scope =
            book.auto.as_ref().map(|a| a.scope).ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let inner = pfx.without_declarations();
        // The held attributes by the namespace and local name they stand for: the standard's own prefixes of the two namespaces a properties element is written with.
        let mut extra: Vec<(Ns, String, String)> = Vec::new();
        for value in held {
            for (key, text) in value.attrs.iter() {
                let Some((prefix, local)) = key.split_once(':') else { continue };
                let ns = match prefix {
                    "fo" => Ns::Fo,
                    "style" => Ns::Style,
                    _ => continue,
                };
                if !super::super::kept::is_qname(key) || super::super::splice::has_bad_char(text) {
                    continue;
                }
                extra.push((ns, local.to_owned(), text.to_owned()));
            }
        }
        let kept_ops: Vec<Op> =
            ops.iter().filter(|o| !extra.iter().any(|(ns, local, _)| o.ns == *ns && o.local == local.as_str())).cloned().collect();
        // **What the file's style held that no cell reads** (its kept remainder: found the sizes of the Asian and complex scripts of 406 visits dropped with no
        // word): the attributes of its paragraph and text properties that neither a cell nor a held value writes, each on the element it was read from, as a graphic style's are.
        let remainder = self.text_remainder(family, name, &kept_ops, &extra)?;
        extra.extend(remainder.same);
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let mut attrs = self.ctx().attributes(&inner, &kept_ops, &mut wrote)?;
        for (ns, local, value) in &extra {
            self.work.charge_attribute(self.limits)?;
            attrs.push_str(&inner.attr(*ns, local, value));
        }
        attrs.push_str(&self.foreign_attributes(&pfx, &remainder.same_foreign)?);
        let s = &pfx.style;
        let mut more = String::new();
        if !remainder.other.is_empty() || !remainder.other_foreign.is_empty() {
            more.push_str(&format!("<{s}:text-properties"));
            for (ns, local, value) in &remainder.other {
                self.work.charge_attribute(self.limits)?;
                more.push_str(&inner.attr(*ns, local, value));
            }
            more.push_str(&self.foreign_attributes(&pfx, &remainder.other_foreign)?);
            more.push_str("/>");
        }
        let text = format!(
            "<{s}:style {s}:name=\"{}\" {s}:family=\"{word}\"{}><{s}:{element}{attrs}/>{more}</{s}:style>",
            super::super::splice::escape_value(name, b'"'),
            pfx.declare
        );
        self.put_style(text)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a text style without the styles of the part"))?;
        book.written.push(Written { name: name.to_owned(), family, attrs: wrote, prop });
        Ok(())
    }

    /// The text of the elements of one paragraph: its characters, with each element opened where its stretch starts and closed where it ends, and each field and mark where it stands. **One sweep
    /// over the cut points** (the starts and ends of the covers, the fields and the marks), **with the open elements a stack** whose top ends first: the covers nest, since the ones that cross or
    /// are nested too deep are left out before ([`drop_crossing`]), so each cut is a look at the top and the sweep is linear in the covers.
    fn paragraph_content(
        &mut self,
        para: &ModelPara<'_>,
        mut covers: Vec<Cover>,
        mut marks: Vec<(usize, String)>,
        mut fields: Vec<FieldAt>,
        text_prefix: &str,
    ) -> Result<String, WriteError> {
        let total = para.count;
        // Outer first: by place, the longer first, and the order they were made in (the sort is stable).
        covers.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)));
        marks.sort_by_key(|m| m.0);
        fields.sort_by_key(|f| f.at);
        let mut cuts: Vec<usize> = Vec::new();
        cuts.try_reserve(2 + covers.len() * 2 + fields.len() * 2 + marks.len())
            .map_err(|_| WriteError::Limit { what: "memory for the runs of a paragraph", max: covers.len() as u64 })?;
        cuts.extend([0, total]);
        for c in &covers {
            cuts.extend([c.from, c.to]);
        }
        for f in &fields {
            cuts.extend([f.at, f.at.saturating_add(f.len)]);
        }
        cuts.extend(marks.iter().map(|m| m.0));
        cuts.retain(|c| *c <= total);
        cuts.sort_unstable();
        cuts.dedup();
        // No cut is inside a field: its characters are written as one.
        let mut kept: Vec<usize> = Vec::with_capacity(cuts.len());
        let mut walk = fields.iter().peekable();
        for c in cuts {
            while walk.peek().is_some_and(|f| f.at.saturating_add(f.len) <= c) {
                walk.next();
            }
            if walk.peek().is_some_and(|f| f.at < c) {
                continue;
            }
            kept.push(c);
        }
        let cuts = kept;
        self.work.charge_steps(cuts.len())?;
        let mut out = String::new();
        let mut writer = Spaces::new(&para.chars, text_prefix, "");
        let mut chars = para.chars.char_indices().peekable();
        // The elements open, innermost last, and how many of them hold text the reader takes as it stands. The covers nest (the ones that cross are left out: `drop_crossing`), so the
        // innermost is the first to end, and a cut is one look at the top of the stack and no scan of it.
        let mut opened: Vec<usize> = Vec::new();
        let mut raw_open = 0usize;
        let (mut next_cover, mut next_mark, mut next_field) = (0usize, 0usize, 0usize);
        for (i, &a) in cuts.iter().enumerate() {
            // Close the elements that end here.
            while let Some(&top) = opened.last() {
                let Some(cover) = covers.get(top).filter(|c| c.to <= a) else { break };
                self.push_charged(&mut out, &cover.tags.close)?;
                raw_open = raw_open.saturating_sub(usize::from(cover.raw));
                opened.pop();
            }
            // The marks at this place.
            while let Some((at, text)) = marks.get(next_mark) {
                if *at > a {
                    break;
                }
                self.push_charged(&mut out, text)?;
                next_mark += 1;
            }
            // The elements that start here.
            while let Some(cover) = covers.get(next_cover) {
                if cover.from > a {
                    break;
                }
                if cover.to > a {
                    self.push_charged(&mut out, &cover.tags.open)?;
                    raw_open = raw_open.saturating_add(usize::from(cover.raw));
                    opened.push(next_cover);
                }
                next_cover += 1;
            }
            let Some(&b) = cuts.get(i + 1) else { break };
            if b <= a {
                continue;
            }
            self.work.charge_steps(1)?;
            if let Some(field) = fields.get(next_field).filter(|f| f.at == a) {
                // A field is its element round the characters it shows.
                next_field += 1;
                let shown = para.chars.get(field.bytes.0..field.bytes.1).unwrap_or_default();
                take(&para.chars, &mut chars, field.len)?;
                self.push_charged(&mut out, &field.tags.open)?;
                self.work.charge_copy(shown.len(), self.limits)?;
                out.push_str(&super::escape_text(shown));
                self.push_charged(&mut out, &field.tags.close)?;
                // A field's shown text ends in no white space (`field_text_ok`): the next space is a character's.
                writer.shown(field.len);
                continue;
            }
            let piece = take(&para.chars, &mut chars, b - a)?;
            self.work.charge_copy(piece.len(), self.limits)?;
            if raw_open > 0 {
                out.push_str(&super::escape_text(piece));
                // The reader takes the content of such an element as it stands; the next space is a character's when this ends in none.
                if piece.chars().next_back().is_some_and(char::is_whitespace) { writer.field(b - a) } else { writer.shown(b - a) }
            } else {
                writer.text(piece, &mut out);
            }
        }
        for &c in opened.iter().rev() {
            if let Some(cover) = covers.get(c) {
                self.push_charged(&mut out, &cover.tags.close)?;
            }
        }
        // Marks at the end.
        for (_, text) in marks.iter().skip(next_mark) {
            self.push_charged(&mut out, text)?;
        }
        Ok(out)
    }

    /// Writes `text` on `out`, **charging its bytes first**.
    fn push_charged(&mut self, out: &mut String, text: &str) -> Result<(), WriteError> {
        self.work.charge_copy(text.len(), self.limits)?;
        out.push_str(text);
        Ok(())
    }

    /// The start and end tags of the kept element `shell` (its attributes as held, with `style` added as the attribute `(name, value)`), written where the shape is: `None` when the namespace of a
    /// prefix it uses is not known here.
    fn shell_tags(&mut self, shell: &RawElement, style: Option<(String, String)>, page: &ScanPage, pfx: &Pfx) -> Result<Option<Tags>, WriteError> {
        let mut element = RawElement { name: Arc::clone(&shell.name), attrs: shell.attrs.clone(), children: Vec::new() };
        if let Some((name, value)) = style {
            element.attrs.push(&name, &value);
        }
        let Some((text, _)) = self.kept_element(&element, page, pfx)? else {
            // A prefix the page binds nothing for: the element is not written as it was, and the save says so.
            self.texts.unbound = self.texts.unbound.saturating_add(1);
            return Ok(None);
        };
        let Some(open) = text.strip_suffix("/>") else { return Ok(None) };
        self.work.charge_attribute(self.limits)?;
        Ok(Some(Tags { open: format!("{open}>"), close: format!("</{}>", shell.name) }))
    }

    /// The lists round the next paragraph, as the file had them while its row holds, else from the row's own bullet.
    fn levels_for(
        &mut self,
        held: Option<&Held<'_>>,
        pp: Option<&Arc<Props>>,
        page: &ScanPage,
        pfx: &Pfx,
        made_list: &mut Option<String>,
        limit: usize,
    ) -> Result<Vec<Want>, WriteError> {
        if let Some(h) = held.filter(|h| h.row_holds) {
            *made_list = None;
            return self.held_levels(&h.para.levels, page, pfx, limit);
        }
        let Some(props) = pp.filter(|p| p.bullet) else {
            *made_list = None;
            return Ok(Vec::new());
        };
        // The paragraphs of one row at one place are the items of one list.
        let fresh = made_list.as_deref() != Some(props.key.as_str());
        let list = if fresh {
            let name = self.list_style_of(props)?;
            self.work.charge_attribute(self.limits)?;
            Some(Tags { open: format!("<{0}:list{1}>", pfx.text, pfx.attr(Ns::Text, "style-name", &name)), close: format!("</{}:list>", pfx.text) })
        } else {
            None
        };
        *made_list = Some(props.key.clone());
        Ok(vec![Want { list, item: Some(bare(&pfx.text, "list-item")) }])
    }

    /// The automatic list style a bullet row asks for (the table's character or `BulletStr`, the face and the size of the bullet, and the label's margin and indent): **one the part
    /// holds with this very content is that style** (a file DiaCraft wrote holds the list style it made as the document's data, and a save that made it again would add a second for each save),
    /// else a new one named by R27. A bullet of more than one character is cut to its first (a level holds one: the row is carried).
    fn list_style_of(&mut self, props: &Arc<Props>) -> Result<String, WriteError> {
        if let Some(done) = self.texts.lists.get(&props.key) {
            return Ok(done.clone());
        }
        let spec = props.list.as_ref().ok_or(WriteError::Mismatch("a list made from a row that has no bullet"))?;
        let bullet: String = spec.bullet.chars().take(1).collect();
        let font = match &spec.font {
            Some(family) => Some(self.face_name(family.trim().trim_matches(|c| c == '\'' || c == '"').trim())?),
            None => None,
        };
        let scope = self
            .book
            .as_ref()
            .and_then(|b| b.auto.as_ref())
            .map(|a| a.scope)
            .ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let (t, st, fo) = (&pfx.text, &pfx.style, &pfx.fo);
        let unit = self.unit;
        let length = |inches: f64| super::super::lengths::length_text(inches, unit).ok_or(WriteError::Unsupported("a length too large to write"));
        let mut attributes = 3u64;
        let mut align = String::new();
        if let Some(x) = spec.ind_first {
            align.push_str(&format!(" {fo}:text-indent=\"{}\"", length(x)?));
            attributes += 1;
        }
        if let Some(x) = spec.ind_left {
            align.push_str(&format!(" {fo}:margin-left=\"{}\"", length(x)?));
            attributes += 1;
        }
        let level = if align.is_empty() {
            String::new()
        } else {
            attributes += 3;
            format!(
                "<{st}:list-level-properties {t}:list-level-position-and-space-mode=\"label-alignment\"><{st}:list-level-label-alignment {t}:label-followed-by=\"listtab\"{align}/></{st}:list-level-properties>"
            )
        };
        let mut face = String::new();
        if let Some(f) = &font {
            face.push_str(&format!(" {st}:font-name=\"{}\"", super::super::splice::escape_value(f, b'"')));
            attributes += 1;
        }
        if let Some(x) = spec.size {
            face.push_str(&format!(" {fo}:font-size=\"{}\"", length(x)?));
            attributes += 1;
        }
        let text_props = if face.is_empty() { String::new() } else { format!("<{st}:text-properties{face}/>") };
        let element = |name: &str| {
            format!(
                "<{t}:list-style{} {st}:name=\"{}\"><{t}:list-level-style-bullet {t}:level=\"1\" {t}:bullet-char=\"{}\">{level}{text_props}</{t}:list-level-style-bullet></{t}:list-style>",
                pfx.declare,
                super::super::splice::escape_value(name, b'"'),
                super::super::splice::escape_value(&bullet, b'"')
            )
        };
        let signature = list_signature(&element("x"));
        if let Some(signature) = &signature {
            self.index_list_styles()?;
            if let Some(found) = self.texts.list_index.as_ref().and_then(|m| m.get(signature)).cloned() {
                if let Some(book) = self.book.as_mut()
                    && let Some(i) = book.find_def(&found, Fam::List)
                {
                    book.add_reference(i);
                }
                self.texts.lists.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
                self.texts.lists.insert(props.key.clone(), found.clone());
                return Ok(found);
            }
        }
        for _ in 0..attributes {
            self.work.charge_attribute(self.limits)?;
        }
        self.work.charge_copy(bullet.len(), self.limits)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a list style without the styles of the part"))?;
        let name = book.names.next(Fam::List);
        self.put_style(element(&name))?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a list style without the styles of the part"))?;
        book.written.push(Written { name: name.clone(), family: Fam::List, attrs: Vec::new(), prop: 0 });
        self.texts.lists.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
        self.texts.lists.insert(props.key.clone(), name.clone());
        if let (Some(signature), Some(index)) = (signature, self.texts.list_index.as_mut()) {
            index.insert(signature, name.clone());
        }
        Ok(name)
    }

    /// The list styles the part holds, by their content without their names, made once (a pass over their bytes, charged).
    fn index_list_styles(&mut self) -> Result<(), WriteError> {
        if self.texts.list_index.is_some() {
            return Ok(());
        }
        let mut index: HashMap<String, String> = HashMap::new();
        let defs: Vec<(String, usize, usize)> = self
            .book
            .as_ref()
            .map(|b| b.defs.iter().filter(|d| d.family == Fam::List).map(|d| (d.name.clone(), d.open.0, d.end)).collect())
            .unwrap_or_default();
        for (name, from, to) in defs {
            self.work.charge_steps(1)?;
            self.work.charge_scan(to.saturating_sub(from), self.limits)?;
            if let Some(text) = self.bytes.get(from..to).and_then(|b| std::str::from_utf8(b).ok())
                && let Some(signature) = list_signature(text)
            {
                index.entry(signature).or_insert(name);
            }
        }
        self.texts.list_index = Some(index);
        Ok(())
    }

    /// The lists a held paragraph was in, as they are written.
    fn held_levels(&mut self, levels: &[OdfListLevel], page: &ScanPage, pfx: &Pfx, limit: usize) -> Result<Vec<Want>, WriteError> {
        if levels.len() > MAX_LIST_LEVELS {
            return refuse(WORDS_LIST_LEVELS);
        }
        if levels.len() > limit {
            return refuse(WORDS_LIST_DEPTH);
        }
        self.work.charge_steps(levels.len().saturating_mul(2))?;
        let mut out: Vec<Want> = Vec::with_capacity(levels.len());
        for level in levels {
            let list = match level.list.as_ref() {
                Some(shell) => Some(self.shell_tags(shell, None, page, pfx)?.unwrap_or_else(|| bare(&pfx.text, "list"))),
                None => None,
            };
            let item = match level.item.as_ref() {
                Some(shell) => Some(self.shell_tags(shell, None, page, pfx)?.unwrap_or_else(|| bare(&pfx.text, "list-item"))),
                None => None,
            };
            out.push(Want { list, item });
        }
        Ok(out)
    }

    /// Opens and closes the lists between the paragraph just written and the next, so that the open ones are `want` (see [`Want`]).
    fn lists(&mut self, out: &mut String, stack: &mut Vec<Level>, want: Vec<Want>, text_prefix: &str) -> Result<(), WriteError> {
        fn close_down(out: &mut String, stack: &mut Vec<Level>, to: usize) {
            while stack.len() > to {
                if let Some(level) = stack.pop() {
                    if let Some(item) = level.close_item {
                        out.push_str(&item);
                    }
                    out.push_str(&level.close_list);
                }
            }
        }
        for (i, w) in want.iter().enumerate() {
            self.work.charge_steps(1)?;
            if stack.len() > i {
                if w.list.is_some() {
                    close_down(out, stack, i);
                } else if let Some(item) = &w.item {
                    // A new item of the list open at this depth: what is deeper goes, and the item before it.
                    close_down(out, stack, i + 1);
                    if let Some(level) = stack.get_mut(i) {
                        if let Some(close) = level.close_item.take() {
                            out.push_str(&close);
                        }
                        out.push_str(&item.open);
                        level.close_item = Some(item.close.clone());
                    }
                    continue;
                } else {
                    continue;
                }
            }
            // Open the list at this depth, and its item.
            let list = w.list.clone().unwrap_or_else(|| bare(text_prefix, "list"));
            let item = w.item.clone().unwrap_or_else(|| bare(text_prefix, "list-item"));
            out.push_str(&list.open);
            out.push_str(&item.open);
            stack.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the lists of a text", max: want.len() as u64 })?;
            stack.push(Level { close_list: list.close, close_item: Some(item.close) });
        }
        close_down(out, stack, want.len());
        Ok(())
    }
}

/// The kind of a field of the model: the local name of the element the reader reads it from, when its row says one of the four it maps and shows what the paragraph holds.
fn field_kind(rows: &HashMap<u32, &Row>, ix: u32, shown: &str) -> Option<&'static str> {
    let row = rows.get(&ix)?;
    let value = row.cells.get("Value")?;
    let kind = field_element(value.formula.text()?)?;
    // The row holds nothing the file has no place for, and shows the characters the paragraph has there.
    let only_value = row.cells.len() == 1 && value.extra.is_none() && value.raw.is_empty();
    let same = value.value.as_ref().map(diacraft_doc::Value::text) == Some(shown);
    (only_value && same).then_some(kind)
}

/// A list style's content without its name, written canonically: what two list styles of one content have in common. `None` for a text that does not parse (a list style whose prefixes the
/// wrapper does not declare is never one the writer would make).
fn list_signature(text: &str) -> Option<String> {
    use super::super::scan::{FO_URI, STYLE_URI, SVG_URI, TEXT_URI};
    let wrapped = format!("<w xmlns:text=\"{TEXT_URI}\" xmlns:style=\"{STYLE_URI}\" xmlns:fo=\"{FO_URI}\" xmlns:svg=\"{SVG_URI}\">{text}</w>");
    let mut root = diacraft_container::xml::parse(wrapped.as_bytes()).ok()?;
    let mut found = None;
    for child in &mut root.children {
        if let diacraft_container::xml::Node::El(el) = child {
            el.attrs.retain(|(k, _)| k != "style:name" && !k.starts_with("xmlns"));
            found = Some(el.clone());
            break;
        }
    }
    let el = found?;
    let mut out: Vec<u8> = Vec::new();
    diacraft_container::xml::write_element(&mut out, &el).ok()?;
    String::from_utf8(out).ok()
}

/// Takes out the held covers that cross another cover, and says how many: **one sweep** (the covers sorted outer first, a stack of the open ones, each new cover tested against the top of it).
/// A file's own markup nests, but the model's runs are the rows' and need not nest in its elements; an element that crosses one is not written (its characters are, from the cells), where
/// splitting it at every run it crosses would write it again and again.
///
/// **A held cover that would be nested deeper than `max_depth` is not written either** (the part must stay as deep as the files DiaCraft reads allow, and the model's own runs go inside it):
/// the stack of the sweep is the nesting.
fn drop_crossing(covers: &mut Vec<Cover>, max_depth: usize) -> usize {
    covers.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)));
    let mut stack: Vec<usize> = Vec::new();
    let mut dropped = vec![false; covers.len()];
    for i in 0..covers.len() {
        let Some(current) = covers.get(i) else { break };
        let (from, to, held) = (current.from, current.to, current.held);
        while stack.last().is_some_and(|&t| covers.get(t).is_some_and(|c| c.to <= from)) {
            stack.pop();
        }
        while let Some(&top) = stack.last() {
            let Some(top_cover) = covers.get(top) else { break };
            if to <= top_cover.to {
                break;
            }
            // `current` runs past the end of `top`, which it starts inside of: they cross. The held one of the two is not written.
            if held {
                if let Some(slot) = dropped.get_mut(i) {
                    *slot = true;
                }
                break;
            } else if top_cover.held {
                if let Some(slot) = dropped.get_mut(top) {
                    *slot = true;
                }
                stack.pop();
            } else {
                break;
            }
        }
        if held
            && stack.len() >= max_depth
            && let Some(slot) = dropped.get_mut(i)
        {
            *slot = true;
        }
        if !dropped.get(i).copied().unwrap_or(false) {
            stack.push(i);
        }
    }
    let count = dropped.iter().filter(|d| **d).count();
    if count > 0 {
        let mut at = 0usize;
        covers.retain(|_| {
            let keep = !dropped.get(at).copied().unwrap_or(false);
            at += 1;
            keep
        });
    }
    count
}

/// The start and end tags of `local` of the text namespace under `prefix`, with no attribute.
fn bare(prefix: &str, local: &str) -> Tags {
    Tags { open: format!("<{prefix}:{local}>"), close: format!("</{prefix}:{local}>") }
}

/// The prefix of an element name as written, or `fallback` for a name with none.
fn prefix_of<'a>(name: &'a str, fallback: &'a str) -> &'a str {
    name.split_once(':').map_or(fallback, |(p, _)| p)
}

/// Whether two rows of the `Character` section say the same to a file.
fn same_run_props(rows: &mut ModelRows<'_>, a: u32, b: u32) -> bool {
    a == b
        || match (rows.props(a), rows.props(b)) {
            (Some((x, _)), Some((y, _))) => x.key == y.key,
            (None, None) => true,
            _ => false,
        }
}

/// The next `n` characters of the paragraph's text, from where `chars` stands.
fn take<'t>(text: &'t str, chars: &mut std::iter::Peekable<std::str::CharIndices<'t>>, n: usize) -> Result<&'t str, WriteError> {
    let start = chars.peek().map_or(text.len(), |(b, _)| *b);
    for _ in 0..n {
        chars.next();
    }
    let end = chars.peek().map_or(text.len(), |(b, _)| *b);
    text.get(start..end).ok_or(WriteError::Mismatch("characters of a paragraph that are not in its text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(from: usize, to: usize, held: bool) -> Cover {
        Cover { from, to, tags: Tags { open: format!("<a{from}>"), close: format!("</a{from}>") }, raw: false, held }
    }

    fn places(covers: &[Cover]) -> Vec<(usize, usize)> {
        covers.iter().map(|c| (c.from, c.to)).collect()
    }

    #[test]
    fn covers_that_nest_or_follow_one_another_are_all_kept() {
        let mut covers = vec![cover(0, 10, true), cover(2, 4, true), cover(2, 4, false), cover(10, 12, true), cover(5, 5, true)];
        assert_eq!(drop_crossing(&mut covers, 1000), 0);
        assert_eq!(places(&covers), [(0, 10), (2, 4), (2, 4), (5, 5), (10, 12)]);
    }

    #[test]
    fn a_held_cover_that_crosses_another_is_left_out_and_counted_and_a_run_of_the_rows_is_never() {
        // Two held ones cross: the later goes. A held one that crosses a run of the model's own rows goes, whichever of the two starts first.
        let mut covers = vec![cover(0, 5, true), cover(3, 8, true)];
        assert_eq!(drop_crossing(&mut covers, 1000), 1);
        assert_eq!(places(&covers), [(0, 5)]);
        let mut covers = vec![cover(0, 5, true), cover(3, 8, false)];
        assert_eq!(drop_crossing(&mut covers, 1000), 1);
        assert_eq!(places(&covers), [(3, 8)]);
        let mut covers = vec![cover(0, 5, false), cover(3, 8, true)];
        assert_eq!(drop_crossing(&mut covers, 1000), 1);
        assert_eq!(places(&covers), [(0, 5)]);
        // Many held covers that each cross the next: one of every run of them stays, and the work is one sweep.
        let mut covers: Vec<Cover> = (0..1000).map(|i| cover(i, i + 500, true)).collect();
        assert_eq!(drop_crossing(&mut covers, 1000), 998);
        assert_eq!(places(&covers), [(0, 500), (500, 1000)]);
    }

    #[test]
    fn a_held_cover_nested_deeper_than_the_depth_the_part_may_have_is_left_out() {
        // Ten covers nested, one inside the next: a depth of 4 keeps the four outermost and leaves out the other six; a model run (not held) inside them is kept.
        let mut covers: Vec<Cover> = (0..10).map(|i| cover(i, 20 - i, true)).collect();
        covers.push(cover(9, 11, false));
        assert_eq!(drop_crossing(&mut covers, 4), 6);
        assert_eq!(places(&covers), [(0, 20), (1, 19), (2, 18), (3, 17), (9, 11)]);
        // At the depth exactly, none is left out.
        let mut covers: Vec<Cover> = (0..4).map(|i| cover(i, 20 - i, true)).collect();
        assert_eq!(drop_crossing(&mut covers, 4), 0);
    }

    #[test]
    fn a_field_finds_its_row_by_the_index_and_only_when_the_row_says_what_the_paragraph_shows() {
        let mut row = Row { ix: Some(3), ..Row::default() };
        row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::read(Some("PAGENUMBER()"), Some("7"), None, None, &mut 1_000));
        let mut rows: HashMap<u32, &Row> = HashMap::new();
        rows.insert(3, &row);
        assert_eq!(field_kind(&rows, 3, "7"), Some("page-number"));
        assert_eq!(field_kind(&rows, 3, "8"), None, "another shown text");
        assert_eq!(field_kind(&rows, 4, "7"), None, "no such row");
    }
}
