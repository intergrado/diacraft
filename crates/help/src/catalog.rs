//! Help catalogs, `translate`, and the packed file of the web.
//!
//! A **help catalog** is one language's translations of the segments of one topic: tab-separated rows `context`, English source, translation, with `\n`, `\t` and `\\` the
//! only escapes, `#` lines and blank lines skipped. The context says what the segment is (`title`, `summary`, `keywords`, `description`, `seo_title` of the header;
//! `heading`, `para`, `item`, `note`, `cell`, `alt` of the body), and the source is the English text exactly as the topic has it ([`segments`]), so a change of a paragraph
//! changes one row and no more. Code blocks are not segments.
//!
//! [`translate`] puts a catalog's rows into a topic: the blocks, anchors and link targets are the English topic's, and a segment's text is its row's translation, read by the
//! very parser that reads English. A segment with no row stays English. A row whose translation is not valid Help Markdown, or changes where a link goes, a key or a code span,
//! is skipped (and reported) and the English stays, so a translation can never make a link go elsewhere or run a different command.
//!
//! A **packed file** holds the topics of a language for the web as one file: [`pack`] and [`unpack`], read with [`bundle_range`] so that no length in the file can reach past
//! the file.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::md::{Block, Item, List, Span, Target, Text, Topic, parse_inlines};
use crate::tables::{split_tsv_row, unescape};
use crate::{Code, MAX_CATALOG_BYTES, MAX_CATALOG_ROWS, MAX_ROW_BYTES, MAX_TOPICS, Problem, report};

/// The contexts a catalog row may have.
pub const CONTEXTS: [&str; 11] = ["title", "summary", "keywords", "description", "seo_title", "heading", "para", "item", "note", "cell", "alt"];

/// One language's translations of a topic's segments.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    rows: HashMap<(String, String), String>,
}

impl Catalog {
    /// Reads a catalog; a row that breaks a rule is skipped with its problem. `rtl` is whether the language is written right to left, which alone may use direction marks.
    pub fn parse(text: &str, rtl: bool) -> (Catalog, Vec<Problem>) {
        let mut catalog = Catalog::default();
        let mut problems = Vec::new();
        if text.len() > MAX_CATALOG_BYTES {
            report(&mut problems, 0, Code::CatalogTooLarge, "");
            return (catalog, problems);
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for (n, line) in text.lines().enumerate() {
            let n = n + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if catalog.rows.len() >= MAX_CATALOG_ROWS {
                report(&mut problems, n, Code::TooManyRows, "");
                break;
            }
            match row(line, rtl) {
                Ok((context, source, translation)) => {
                    if catalog.rows.contains_key(&(context.clone(), source.clone())) {
                        report(&mut problems, n, Code::RowDuplicate, &source);
                    } else {
                        catalog.rows.insert((context, source), translation);
                    }
                }
                Err((code, detail)) => report(&mut problems, n, code, &detail),
            }
        }
        (catalog, problems)
    }

    /// The translation of `source` in `context`.
    pub fn get(&self, context: &str, source: &str) -> Option<&str> {
        self.rows.get(&(context.to_owned(), source.to_owned())).map(String::as_str)
    }

    /// How many rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether it has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// One row of a catalog, or the rule it breaks.
fn row(line: &str, rtl: bool) -> Result<(String, String, String), (Code, String)> {
    if line.len() > MAX_ROW_BYTES {
        return Err((Code::RowTooLong, String::new()));
    }
    if line.chars().any(|c| c.is_control() && c != '\t') {
        return Err((Code::RowControl, String::new()));
    }
    let columns = split_tsv_row(line);
    let [context, source, translation] = columns.as_slice() else { return Err((Code::RowColumns, format!("{} columns", columns.len()))) };
    if !CONTEXTS.contains(context) {
        return Err((Code::CatalogContext, (*context).to_owned()));
    }
    let (Some(source), Some(translation)) = (unescape(source), unescape(translation)) else { return Err((Code::RowEscape, String::new())) };
    if source.trim().is_empty() || translation.trim().is_empty() {
        return Err((Code::CatalogEmpty, source));
    }
    if line.chars().any(|c| is_bidi(c, rtl)) {
        return Err((Code::CatalogBidi, source));
    }
    Ok(((*context).to_owned(), source, translation))
}

/// A direction control: the embeddings, overrides and isolates in any language, the direction marks in a left-to-right one.
fn is_bidi(c: char, rtl: bool) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') || (!rtl && matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}'))
}

/// What translating a topic did.
#[derive(Clone, Debug)]
pub struct Translation {
    /// The topic, translated where the catalog could.
    pub topic: Topic,
    /// The segments translated.
    pub translated: usize,
    /// The segments with no row.
    pub missing: usize,
    /// The rows skipped, each with the segment it was for and why.
    pub skipped: Vec<(String, Problem)>,
}

/// One segment of a topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// What the segment is: one of [`CONTEXTS`].
    pub context: &'static str,
    /// Its English source.
    pub source: String,
    /// Its line in the file.
    pub line: usize,
}

/// The segments of a topic, in order: the header's, then the text of each block (a code block has none).
pub fn segments(topic: &Topic) -> Vec<Segment> {
    let h = &topic.header;
    let mut out = vec![
        Segment { context: "title", source: h.title.clone(), line: h.lines.title },
        Segment { context: "summary", source: h.summary.clone(), line: h.lines.summary },
        Segment { context: "keywords", source: h.keywords.join(", "), line: h.lines.keywords },
    ];
    if let Some(d) = &h.site.description {
        out.push(Segment { context: "description", source: d.clone(), line: h.lines.description });
    }
    if let Some(t) = &h.site.seo_title {
        out.push(Segment { context: "seo_title", source: t.clone(), line: h.lines.seo_title });
    }
    for block in &topic.doc.blocks {
        let context = match block {
            Block::Heading { .. } => "heading",
            Block::Para(_) => "para",
            Block::Note(_) => "note",
            Block::List(_) => "item",
            Block::Table { .. } => "cell",
            Block::Picture { alt, line, .. } => {
                out.push(Segment { context: "alt", source: alt.clone(), line: *line });
                continue;
            }
            Block::Code { .. } => continue,
        };
        for text in block.texts() {
            out.push(Segment { context, source: text.source.clone(), line: text.line });
        }
    }
    out.retain(|s| !s.source.is_empty());
    out
}

/// Puts a catalog's translations into a topic. See the module's description for what is kept and what is skipped.
pub fn translate(topic: &Topic, catalog: &Catalog) -> Translation {
    let mut done = Translation { topic: topic.clone(), translated: 0, missing: 0, skipped: Vec::new() };
    header_segments(&mut done, topic, catalog);
    let mut blocks = Vec::with_capacity(topic.doc.blocks.len());
    for block in &topic.doc.blocks {
        blocks.push(translate_block(&mut done, block, catalog));
    }
    done.topic.doc.blocks = blocks;
    done
}

/// The header's segments.
fn header_segments(done: &mut Translation, topic: &Topic, catalog: &Catalog) {
    let h = &topic.header;
    if let Some(t) = plain_segment(done, catalog, "title", &h.title) {
        done.topic.header.title = t;
    }
    if let Some(t) = plain_segment(done, catalog, "summary", &h.summary) {
        done.topic.header.summary = t;
    }
    if let Some(d) = &h.site.description
        && let Some(t) = plain_segment(done, catalog, "description", d)
    {
        done.topic.header.site.description = Some(t);
    }
    if let Some(d) = &h.site.seo_title
        && let Some(t) = plain_segment(done, catalog, "seo_title", d)
    {
        done.topic.header.site.seo_title = Some(t);
    }
    let joined = h.keywords.join(", ");
    if let Some(t) = plain_segment(done, catalog, "keywords", &joined) {
        let words: Vec<String> = t.split(',').map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned).collect();
        if words.is_empty() || t.contains(['*', '[', ']', '`', '\\']) {
            done.translated = done.translated.saturating_sub(1);
            skip(done, "keywords", &joined, h.lines.keywords, Code::TranslationWords, &t);
        } else {
            done.topic.header.keywords = words;
        }
    }
}

/// The translation of a plain segment (no markup), counted; `None` when the catalog has no row.
fn plain_segment(done: &mut Translation, catalog: &Catalog, context: &str, source: &str) -> Option<String> {
    if source.is_empty() {
        return None;
    }
    match catalog.get(context, source) {
        Some(t) => {
            done.translated += 1;
            Some(t.to_owned())
        }
        None => {
            done.missing += 1;
            None
        }
    }
}

fn skip(done: &mut Translation, context: &str, source: &str, line: usize, code: Code, detail: &str) {
    let label: String = format!("{context}: {}", source.chars().take(60).collect::<String>());
    done.skipped.push((label, Problem::new(line, code, detail)));
}

/// A block with its text translated.
fn translate_block(done: &mut Translation, block: &Block, catalog: &Catalog) -> Block {
    match block {
        Block::Heading { level, text, anchor } => {
            Block::Heading { level: *level, text: translate_text(done, text, "heading", catalog), anchor: anchor.clone() }
        }
        Block::Para(text) => Block::Para(translate_text(done, text, "para", catalog)),
        Block::Note(text) => Block::Note(translate_text(done, text, "note", catalog)),
        Block::List(list) => Block::List(translate_list(done, list, catalog)),
        Block::Table { header, rows } => Block::Table {
            header: header.iter().map(|c| translate_text(done, c, "cell", catalog)).collect(),
            rows: rows.iter().map(|r| r.iter().map(|c| translate_text(done, c, "cell", catalog)).collect()).collect(),
        },
        Block::Picture { alt, name, line } => {
            let alt = plain_segment(done, catalog, "alt", alt).unwrap_or_else(|| alt.clone());
            Block::Picture { alt, name: name.clone(), line: *line }
        }
        Block::Code { lang, text } => Block::Code { lang: *lang, text: text.clone() },
    }
}

fn translate_list(done: &mut Translation, list: &List, catalog: &Catalog) -> List {
    let items = list
        .items
        .iter()
        .map(|item| Item {
            text: translate_text(done, &item.text, "item", catalog),
            sub: item.sub.as_ref().map(|s| translate_list(done, s, catalog)),
        })
        .collect();
    List { ordered: list.ordered, items }
}

/// A text translated by its row, when there is one and it is right.
fn translate_text(done: &mut Translation, text: &Text, context: &str, catalog: &Catalog) -> Text {
    if text.source.is_empty() {
        return text.clone();
    }
    let Some(translation) = catalog.get(context, &text.source) else {
        done.missing += 1;
        return text.clone();
    };
    let mut problems = Vec::new();
    let spans = parse_inlines(translation, text.line, &mut problems);
    let refusal = if let Some(p) = problems.first() {
        Some((Code::TranslationMarkup, p.code.words().to_owned()))
    } else if targets(&spans) != targets(&text.spans) {
        Some((Code::TranslationTargets, String::new()))
    } else if code_spans(&spans) != code_spans(&text.spans) {
        Some((Code::TranslationCode, String::new()))
    } else {
        None
    };
    match refusal {
        Some((code, detail)) => {
            skip(done, context, &text.source, text.line, code, &detail);
            text.clone()
        }
        None => {
            done.translated += 1;
            Text { source: translation.to_owned(), spans, line: text.line }
        }
    }
}

/// The targets of the links of some spans, sorted: what a translation must keep, though it may move the links.
fn targets(spans: &[Span]) -> Vec<String> {
    let mut out = Vec::new();
    collect(spans, &mut |s| {
        if let Span::Link { target, .. } = s {
            out.push(target_key(target));
        }
    });
    out.sort();
    out
}

/// The code spans of some spans, sorted.
fn code_spans(spans: &[Span]) -> Vec<String> {
    let mut out = Vec::new();
    collect(spans, &mut |s| {
        if let Span::Code(c) = s {
            out.push(c.clone());
        }
    });
    out.sort();
    out
}

fn collect(spans: &[Span], visit: &mut impl FnMut(&Span)) {
    for span in spans {
        visit(span);
        match span {
            Span::Strong(inner) | Span::Em(inner) => collect(inner, visit),
            Span::Link { text, .. } => collect(text, visit),
            Span::Text(_) | Span::Code(_) => {}
        }
    }
}

fn target_key(target: &Target) -> String {
    match target {
        Target::Topic { id, anchor } => format!("topic:{id}#{}", anchor.as_deref().unwrap_or_default()),
        Target::Anchor(a) => format!("#{a}"),
        Target::Control(c) => format!("ctl:{c}"),
        Target::Command(c) => format!("cmd:{c}"),
        Target::Key(chord) => format!("key:{}", chord.canonical()),
        Target::Web(u) => u.clone(),
    }
}

// ------------------------------------------------------------------------------------------------ the packed file

/// The range `start..start + len` when it lies inside `total` bytes, with no overflow.
pub fn bundle_range(start: usize, len: usize, total: usize) -> Option<Range<usize>> {
    let end = start.checked_add(len)?;
    (end <= total).then_some(start..end)
}

const MAGIC: &str = "help-bundle 1";

/// Packs topics (id and text) into one file: a first line `help-bundle 1`, the number of topics, a line `id length` for each (the length in bytes), an empty line, and the
/// texts one after another. An id is one word: white space and control characters in it are made `_`.
pub fn pack(topics: &[(&str, &str)]) -> String {
    let mut out = format!("{MAGIC}\n{}\n", topics.len());
    for (id, text) in topics {
        let id: String = id.chars().map(|c| if c.is_whitespace() || c.is_control() { '_' } else { c }).collect();
        out.push_str(&format!("{id} {}\n", text.len()));
    }
    out.push('\n');
    for (_, text) in topics {
        out.push_str(text);
    }
    out
}

/// Reads a packed file back: the topics' ids and texts. A file that does not follow the format, whose lengths do not fit the text or cut a character, or that has more topics
/// than a bundle holds, is refused.
pub fn unpack(text: &str) -> Result<Vec<(String, String)>, Problem> {
    let bad = |why: &str| Problem::new(0, Code::PackFormat, why);
    let mut pos = 0usize;
    if take_line(text, &mut pos) != Some(MAGIC) {
        return Err(bad("first line"));
    }
    let count: usize = take_line(text, &mut pos).and_then(|l| l.parse().ok()).ok_or_else(|| bad("count"))?;
    if count > MAX_TOPICS {
        return Err(bad("more topics than a bundle holds"));
    }
    let mut index: Vec<(String, usize)> = Vec::with_capacity(count);
    let mut seen = HashSet::new();
    for _ in 0..count {
        let line = take_line(text, &mut pos).ok_or_else(|| bad("index"))?;
        let (id, len) = line.split_once(' ').ok_or_else(|| bad("index line"))?;
        let len: usize = len.parse().map_err(|_| bad("length"))?;
        if id.is_empty() || !seen.insert(id.to_owned()) {
            return Err(bad("id"));
        }
        index.push((id.to_owned(), len));
    }
    if take_line(text, &mut pos) != Some("") {
        return Err(bad("separator"));
    }
    let body = text.get(pos..).ok_or_else(|| bad("body"))?;
    let mut start = 0usize;
    let mut out = Vec::with_capacity(count);
    for (id, len) in index {
        let range = bundle_range(start, len, body.len()).ok_or_else(|| bad("length past the end"))?;
        let piece = body.get(range.clone()).ok_or_else(|| bad("length inside a character"))?;
        out.push((id, piece.to_owned()));
        start = range.end;
    }
    if start != body.len() {
        return Err(bad("text after the last topic"));
    }
    Ok(out)
}

/// The line of `text` at `pos` (up to its newline, which must be there), and `pos` moved past it.
fn take_line<'a>(text: &'a str, pos: &mut usize) -> Option<&'a str> {
    let rest = text.get(*pos..)?;
    let end = rest.find('\n')?;
    *pos += end + 1;
    rest.get(..end)
}
