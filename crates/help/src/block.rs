//! The blocks of Help Markdown, line by line.
//!
//! One pass over the lines of a body: each line starts a block of the kind it looks like, the block takes the lines that belong to it, and the next line starts
//! the next block, so the work is linear in the bytes. What breaks a rule is reported with its line and read as plainly as it can be, so that a topic always reads
//! (the checker refuses it, the program shows it).

use std::collections::HashSet;

use crate::inline::{self, Diag, valid_id};
use crate::md::{Block, CodeLang, Doc, Item, List, Span, Text, slug};
use crate::{Code, MAX_BLOCKS, MAX_LIST_DEPTH, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, Problem, report};

/// Reads the body of a topic, whose first line is line `first_line` of the file.
pub(crate) fn parse_body(body: &str, first_line: usize, problems: &mut Vec<Problem>) -> Doc {
    let lines: Vec<(usize, &str)> = body.lines().enumerate().map(|(i, l)| (first_line + i, l)).collect();
    let mut parser = Parser { lines, pos: 0, blocks: Vec::new(), anchors: HashSet::new(), cut_short: false, problems };
    parser.run();
    Doc { blocks: parser.blocks, cut_short: parser.cut_short }
}

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>,
    pos: usize,
    blocks: Vec<Block>,
    anchors: HashSet<String>,
    cut_short: bool,
    problems: &'a mut Vec<Problem>,
}

/// The piece of a text that one line gives: its line and its words.
type Piece = (usize, String);

impl<'a> Parser<'a> {
    fn report(&mut self, line: usize, code: Code, detail: &str) {
        report(self.problems, line, code, detail);
    }

    fn run(&mut self) {
        while let Some(&(ln, raw)) = self.lines.get(self.pos) {
            if self.cut_short {
                break;
            }
            if raw.trim().is_empty() {
                self.pos += 1;
                continue;
            }
            self.start(ln, raw);
        }
    }

    /// Adds a block, or stops at the cap.
    fn push(&mut self, line: usize, block: Block) {
        if self.blocks.len() >= MAX_BLOCKS {
            self.report(line, Code::TooManyBlocks, "");
            self.cut_short = true;
        } else {
            self.blocks.push(block);
        }
    }

    /// Cleans a line of control characters (one problem for the line) and of white space at its ends.
    fn clean(&mut self, line: usize, text: &str) -> String {
        let text = text.trim();
        if text.chars().any(is_control) {
            self.report(line, Code::ControlCharacter, "");
            return text.chars().map(|c| if is_control(c) { ' ' } else { c }).collect();
        }
        text.to_owned()
    }

    /// The line at the cursor starts a block: which one it is by how the line looks.
    fn start(&mut self, ln: usize, raw: &'a str) {
        let line = raw.trim_end();
        let rest = line.trim_start();
        if rest.len() != line.len() {
            // An indented line outside a list: read as if it were not.
            self.report(ln, Code::IndentedLine, "");
            if let Some(entry) = self.lines.get_mut(self.pos) {
                entry.1 = rest;
            }
        }
        if rest.starts_with("```") {
            self.code(ln, rest);
        } else if is_heading_line(rest) {
            self.heading(ln, rest);
        } else if rest.starts_with('>') {
            self.note();
        } else if rest.starts_with('|') {
            self.table(ln);
        } else if is_thematic(rest) {
            self.report(ln, Code::ThematicBreak, "");
            self.pos += 1;
        } else if is_underline(rest) {
            self.report(ln, Code::Setext, "");
            self.pos += 1;
        } else if marker(rest).is_some() {
            self.list();
        } else if rest.starts_with("![") {
            self.picture(ln, rest);
        } else {
            self.paragraph(ln, rest);
        }
    }

    // -------------------------------------------------------------------------------------------- text

    /// A run of text from its pieces: the source (joined with a space), and the spans it reads as. A problem of an inline is placed on the line of the piece it is in.
    fn text(&mut self, pieces: &[Piece]) -> Text {
        let line = pieces.first().map_or(0, |p| p.0);
        let source = pieces.iter().map(|p| p.1.as_str()).collect::<Vec<_>>().join(" ");
        let (spans, diags) = inline::parse(&source);
        self.place(&diags, pieces);
        Text { source, spans, line }
    }

    /// Reports the problems of an inline read on the lines of the pieces the source was made of. The problems come in the order of their bytes, so one pass over the
    /// pieces serves them all (a paragraph of a hundred thousand lines with a fault on each is no more work than its bytes).
    fn place(&mut self, diags: &[Diag], pieces: &[Piece]) {
        let (mut index, mut start) = (0usize, 0usize);
        for d in diags {
            while let Some((_, piece)) = pieces.get(index) {
                let next = start + piece.len() + 1;
                if next > d.offset || index + 1 >= pieces.len() {
                    break;
                }
                (index, start) = (index + 1, next);
            }
            let line = pieces.get(index).map_or(0, |p| p.0);
            self.report(line, d.code, &d.detail);
        }
    }

    // -------------------------------------------------------------------------------------------- paragraph

    fn paragraph(&mut self, ln: usize, first: &str) {
        let first = self.clean(ln, first);
        let mut pieces: Vec<Piece> = vec![(ln, first)];
        self.pos += 1;
        while let Some(&(n, raw)) = self.lines.get(self.pos) {
            let line = raw.trim_end();
            let rest = line.trim_start();
            if rest.is_empty() || interrupts(rest) {
                break;
            }
            if is_thematic(rest) || is_underline(rest) {
                // A line of dashes or equals signs under text is a heading the format does not have; a line of stars or underscores is a break.
                let code = if rest.starts_with(['-', '=']) { Code::Setext } else { Code::ThematicBreak };
                self.report(n, code, "");
                self.pos += 1;
                break;
            }
            if rest.len() != line.len() {
                self.report(n, Code::IndentedLine, "");
            }
            let piece = self.clean(n, rest);
            pieces.push((n, piece));
            self.pos += 1;
        }
        let text = self.text(&pieces);
        self.push(ln, Block::Para(text));
    }

    // -------------------------------------------------------------------------------------------- heading

    fn heading(&mut self, ln: usize, line: &str) {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if hashes != 2 && hashes != 3 {
            // Read as a paragraph of the line as written.
            self.report(ln, Code::HeadingLevel, line);
            self.paragraph(ln, line);
            return;
        }
        self.pos += 1;
        let after = line.get(hashes..).unwrap_or_default().trim();
        let (text, custom) = self.split_anchor(ln, after);
        if text.is_empty() {
            self.report(ln, Code::HeadingEmpty, "");
            return;
        }
        let text_line = self.clean(ln, text);
        let text = self.text(&[(ln, text_line)]);
        let anchor = custom.unwrap_or_else(|| self.default_anchor(ln, &text));
        let anchor = self.unique_anchor(ln, anchor);
        self.push(ln, Block::Heading { level: if hashes == 2 { 2 } else { 3 }, text, anchor });
    }

    /// The heading's text without its `{#anchor}`, and the anchor when it is given and right.
    fn split_anchor<'t>(&mut self, ln: usize, after: &'t str) -> (&'t str, Option<String>) {
        let Some(open) = after.rfind("{#").filter(|_| after.ends_with('}')) else { return (after, None) };
        let given = after.get(open + 2..after.len().saturating_sub(1)).unwrap_or_default();
        let text = after.get(..open).unwrap_or_default().trim_end();
        if valid_id(given) {
            (text, Some(given.to_owned()))
        } else {
            self.report(ln, Code::AnchorSyntax, given);
            (text, None)
        }
    }

    fn default_anchor(&mut self, ln: usize, text: &Text) -> String {
        let made = slug(&plain(&text.spans));
        if made.is_empty() {
            self.report(ln, Code::AnchorEmpty, "");
            return "section".to_owned();
        }
        made
    }

    /// The anchor, or when the topic has it already (a problem) the anchor with a number.
    fn unique_anchor(&mut self, ln: usize, anchor: String) -> String {
        if self.anchors.insert(anchor.clone()) {
            return anchor;
        }
        self.report(ln, Code::AnchorDuplicate, &anchor);
        let mut n = 2usize;
        loop {
            let candidate = format!("{anchor}-{n}");
            if self.anchors.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    // -------------------------------------------------------------------------------------------- note

    fn note(&mut self) {
        let Some(&(first, _)) = self.lines.get(self.pos) else { return };
        let mut pieces: Vec<Piece> = Vec::new();
        while let Some(&(n, raw)) = self.lines.get(self.pos) {
            let Some(content) = raw.strip_prefix('>') else { break };
            let content = content.trim();
            if content.starts_with('>') {
                self.report(n, Code::NoteNested, "");
            }
            if content.is_empty() {
                self.report(n, Code::NoteBlank, "");
            } else {
                let piece = self.clean(n, content);
                pieces.push((n, piece));
            }
            self.pos += 1;
        }
        if !pieces.is_empty() {
            let text = self.text(&pieces);
            self.push(first, Block::Note(text));
        }
    }

    // -------------------------------------------------------------------------------------------- code

    fn code(&mut self, ln: usize, line: &str) {
        let word = line.get(3..).unwrap_or_default().trim();
        let lang = match word {
            "" | "text" => CodeLang::Text,
            "sh" => CodeLang::Sh,
            "json" => CodeLang::Json,
            other => {
                self.report(ln, Code::CodeLanguage, other);
                CodeLang::Text
            }
        };
        self.pos += 1;
        let mut text: Vec<&str> = Vec::new();
        let mut closed = false;
        while let Some(&(_, raw)) = self.lines.get(self.pos) {
            self.pos += 1;
            if raw.trim_end() == "```" {
                closed = true;
                break;
            }
            text.push(raw);
        }
        if !closed {
            self.report(ln, Code::CodeUnclosed, "");
        }
        self.push(ln, Block::Code { lang, text: text.join("\n") });
    }

    // -------------------------------------------------------------------------------------------- picture

    fn picture(&mut self, ln: usize, line: &str) {
        self.pos += 1;
        let Some((alt_raw, target, trailing)) = split_picture(line) else {
            self.report(ln, Code::PictureSyntax, line);
            return;
        };
        if trailing {
            self.report(ln, Code::PictureAlone, "");
        }
        let Some(name) = target.strip_prefix("site:") else {
            self.report(ln, Code::PictureSyntax, target);
            return;
        };
        if !valid_id(name) {
            self.report(ln, Code::PictureName, name);
            return;
        }
        let alt = unescape_plain(alt_raw);
        if alt.trim().is_empty() {
            self.report(ln, Code::PictureAlt, "");
            return;
        }
        self.push(ln, Block::Picture { alt: alt.trim().to_owned(), name: name.to_owned(), line: ln });
    }

    // -------------------------------------------------------------------------------------------- list

    fn list(&mut self) {
        let Some(&(first, raw)) = self.lines.get(self.pos) else { return };
        let Some((ordered, text)) = marker(raw.trim()) else { return };
        let mut top: Vec<Build> = vec![Build::new(first, text, self)];
        let mut nested_last = false;
        self.pos += 1;
        while let Some(&(n, raw)) = self.lines.get(self.pos) {
            if !self.list_line(n, raw.trim_end(), ordered, &mut top, &mut nested_last) {
                break;
            }
            self.pos += 1;
        }
        let list = self.finish_list(ordered, top);
        self.push(first, Block::List(list));
    }

    /// One line after the first of a list: whether the list goes on.
    fn list_line(&mut self, n: usize, line: &str, ordered: bool, top: &mut Vec<Build>, nested_last: &mut bool) -> bool {
        if line.trim().is_empty() {
            return false;
        }
        let indent = line.bytes().take_while(|&b| b == b' ').count();
        let content = line.get(indent..).unwrap_or_default();
        if indent == 0 {
            if is_thematic(content) || content.starts_with('\t') {
                return false;
            }
            let Some((kind, text)) = marker(content) else { return false };
            if kind != ordered {
                self.report(n, Code::ListMarkerMix, "");
            }
            let item = Build::new(n, text, self);
            top.push(item);
            *nested_last = false;
        } else if let Some((kind, text)) = marker(content) {
            self.nest(top, n, indent, kind, text);
            *nested_last = true;
        } else {
            let expected = if *nested_last { 4 } else { 2 };
            if indent != expected || content.starts_with('\t') {
                self.report(n, Code::ListIndent, "");
            }
            let piece = self.clean(n, content);
            if let Some(item) = last_item(top, *nested_last) {
                item.pieces.push((n, piece));
            }
        }
        true
    }

    /// A nested item at `indent`: under the last item of the list, whatever its depth, since the format has two levels.
    fn nest(&mut self, top: &mut [Build], n: usize, indent: usize, ordered: bool, text: &str) {
        let level = if indent % 2 == 1 {
            self.report(n, Code::ListIndent, "");
            (indent / 2).max(1)
        } else {
            indent / 2
        };
        if level >= MAX_LIST_DEPTH {
            self.report(n, Code::ListTooDeep, "");
        }
        let item = Build::new(n, text, self);
        if let Some(parent) = top.last_mut() {
            match parent.sub_ordered {
                None => parent.sub_ordered = Some(ordered),
                Some(kind) if kind != ordered => self.report(n, Code::ListMarkerMix, ""),
                Some(_) => {}
            }
            parent.subs.push(item);
        }
    }

    fn finish_list(&mut self, ordered: bool, top: Vec<Build>) -> List {
        let items = top
            .into_iter()
            .map(|b| {
                let text = self.text(&b.pieces);
                let sub = b
                    .sub_ordered
                    .map(|kind| List { ordered: kind, items: b.subs.into_iter().map(|s| Item { text: self.text(&s.pieces), sub: None }).collect() });
                Item { text, sub }
            })
            .collect();
        List { ordered, items }
    }

    // -------------------------------------------------------------------------------------------- table

    fn table(&mut self, ln: usize) {
        let mut rows: Vec<(usize, &str)> = Vec::new();
        while let Some(&(n, raw)) = self.lines.get(self.pos) {
            if !raw.starts_with('|') {
                break;
            }
            rows.push((n, raw.trim_end()));
            self.pos += 1;
        }
        let Some((&(header_line, header_row), body)) = rows.split_first() else { return };
        let header_cells = split_cells(header_row);
        let columns = header_cells.len();
        if columns > MAX_TABLE_COLUMNS {
            self.report(header_line, Code::TableTooWide, "");
        }
        let width = columns.min(MAX_TABLE_COLUMNS);
        let body = self.separator(header_line, columns, body);
        let header = self.cells(header_line, header_cells, width);
        let mut out: Vec<Vec<Text>> = Vec::new();
        for (n, row) in body {
            if out.len() + 1 >= MAX_TABLE_ROWS {
                self.report(*n, Code::TableTooTall, "");
                break;
            }
            let cells = split_cells(row);
            if cells.len() != columns {
                self.report(*n, Code::TableShape, "");
            }
            out.push(self.cells(*n, cells, width));
        }
        self.push(ln, Block::Table { header, rows: out });
    }

    /// The rows after the header with the separator row taken off, when the second row is one.
    fn separator<'r>(&mut self, header_line: usize, columns: usize, body: &'r [(usize, &'r str)]) -> &'r [(usize, &'r str)] {
        let Some((&(n, row), rest)) = body.split_first() else {
            self.report(header_line + 1, Code::TableSeparator, "");
            return body;
        };
        let cells = split_cells(row);
        if !cells.iter().all(|c| is_separator_cell(c)) {
            self.report(n, Code::TableSeparator, "");
            return body;
        }
        if cells.len() != columns {
            self.report(n, Code::TableShape, "");
        }
        rest
    }

    /// The cells of a row as texts, `width` of them (short rows are made up with empty cells, long ones cut).
    fn cells(&mut self, n: usize, mut cells: Vec<String>, width: usize) -> Vec<Text> {
        cells.truncate(width);
        while cells.len() < width {
            cells.push(String::new());
        }
        cells
            .into_iter()
            .map(|cell| {
                let piece = self.clean(n, &cell);
                self.text(&[(n, piece)])
            })
            .collect()
    }
}

/// A list item being built: its marker's line, its pieces, and the items under it.
struct Build {
    pieces: Vec<Piece>,
    sub_ordered: Option<bool>,
    subs: Vec<Build>,
}

impl Build {
    fn new(line: usize, text: &str, parser: &mut Parser<'_>) -> Build {
        let first = parser.clean(line, text);
        Build { pieces: vec![(line, first)], sub_ordered: None, subs: Vec::new() }
    }
}

/// The item that a continuation line goes on: the last nested item of the last item, or the last item.
fn last_item(top: &mut [Build], nested: bool) -> Option<&mut Build> {
    let parent = top.last_mut()?;
    if nested { parent.subs.last_mut() } else { Some(parent) }
}

// ------------------------------------------------------------------------------------------------ what a line looks like

fn is_control(c: char) -> bool {
    c.is_control() && c != '\t'
}

/// A line that starts with one to six `#` and then a space or nothing.
fn is_heading_line(line: &str) -> bool {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&hashes) && line.get(hashes..).is_some_and(|r| r.is_empty() || r.starts_with(' '))
}

/// Three or more of one of `-`, `*` and `_` with nothing else but spaces between them.
fn is_thematic(line: &str) -> bool {
    let mut chars = line.chars().filter(|c| *c != ' ');
    let Some(first) = chars.next() else { return false };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1usize;
    for c in chars {
        if c != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// Two or more equals signs or dashes and nothing else: the underline of a heading the format does not have.
fn is_underline(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 2 && (t.bytes().all(|b| b == b'=') || t.bytes().all(|b| b == b'-'))
}

/// A list marker at the start of a line: whether it is numbered, and the item's text. `-` or a number and `.`, then a space and some text.
fn marker(line: &str) -> Option<(bool, &str)> {
    if let Some(rest) = line.strip_prefix("- ") {
        let text = rest.trim();
        return (!text.is_empty()).then_some((false, text));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = line.get(digits..)?.strip_prefix(". ")?;
        let text = rest.trim();
        return (!text.is_empty()).then_some((true, text));
    }
    None
}

/// Whether a line inside a paragraph starts another block: a fence, a heading, a note, a table row, a picture, a bullet, or a list that starts at 1.
fn interrupts(line: &str) -> bool {
    line.starts_with("```")
        || is_heading_line(line)
        || line.starts_with('>')
        || line.starts_with('|')
        || line.starts_with("![")
        || (!is_thematic(line) && marker(line).is_some_and(|(ordered, _)| !ordered || line.starts_with("1.")))
}

/// The alt text, the target and whether text follows, of `![alt](target)`; `None` when the line is not of that form.
fn split_picture(line: &str) -> Option<(&str, &str, bool)> {
    let body = line.strip_prefix("![")?;
    let mut end = None;
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            ']' => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end?;
    let alt = body.get(..end)?;
    let after = body.get(end + 1..)?.strip_prefix('(')?;
    let close = after.find(')')?;
    Some((alt, after.get(..close)?, !after.get(close + 1..)?.trim().is_empty()))
}

/// Text with the escapes of Help Markdown read and nothing else: the alt text of a picture.
fn unescape_plain(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(&e)) if matches!(e, '*' | '[' | ']' | '(' | ')' | '`' | '|' | '\\') => {
                out.push(e);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// The cells of a table row: split at the pipes that are not escaped (a backslash escapes the character after it), without the pipes at the ends, with `\|` read as `|`.
fn split_cells(row: &str) -> Vec<String> {
    let inner = row.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('|') => cell.push('|'),
                Some(other) => {
                    cell.push('\\');
                    cell.push(other);
                }
                None => cell.push('\\'),
            },
            '|' => cells.push(std::mem::take(&mut cell)),
            other => cell.push(other),
        }
    }
    // Text after the last pipe is a cell of its own; none (or only white space) after it means the row ended with a pipe.
    if !cell.trim().is_empty() || cells.is_empty() {
        cells.push(cell);
    }
    cells
}

/// A separator cell of a table: dashes with a colon at either end or both.
fn is_separator_cell(cell: &str) -> bool {
    let t = cell.trim();
    let t = t.strip_prefix(':').unwrap_or(t);
    let t = t.strip_suffix(':').unwrap_or(t);
    !t.is_empty() && t.bytes().all(|b| b == b'-')
}

/// The text a person reads in some spans.
pub(crate) fn plain(spans: &[Span]) -> String {
    let mut out = String::new();
    plain_into(spans, &mut out);
    out
}

fn plain_into(spans: &[Span], out: &mut String) {
    for span in spans {
        match span {
            Span::Text(t) | Span::Code(t) => out.push_str(t),
            Span::Strong(inner) | Span::Em(inner) => plain_into(inner, out),
            Span::Link { text, .. } => plain_into(text, out),
        }
    }
}
