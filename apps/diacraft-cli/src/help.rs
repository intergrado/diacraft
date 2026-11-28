//! The help page of `diacraft-cli`: one table of what it says ([`text::PAGE`]) and the code that lays it out and paints it.
//!
//! **One table.** Every word the page says is an [`Entry`] of the table, with a key (`mcp.headless`) that names it. The page, the completion scripts and the manual pages all read
//! the table: [`describe`] gives the sentence of a key to the command description (`cli.rs`), so an option is described once. A piece a person reads goes through [`message`], the
//! one place where a catalogue of translations can stand in for the English: the entry's English is the key of its translation. What a program or a person types
//! (the names of commands and options, the usage lines, the environment variables) is never translated, so those entries carry no key that is looked up.
//!
//! **Three renderings of the same page** ([`render`]):
//! * *as written*, for a pipe, a file and a script (and for every test): the line breaks of the table's text, byte for byte the page that `diacraft-cli --help` has always printed (a
//!   test holds the page to its words, and `docs/cli.md` to its commands);
//! * *wrapped*, when standard output is a terminal: the same words, joined and broken again at the terminal's width, each paragraph and each option's description keeping its indent;
//! * *painted*, in either layout, when colour is on: headings, the names a person types, and the places they fill in, in the colours of [`crate::term::Palette`]. Stripping the colour
//!   codes from the painted page gives the plain one, which a test checks.
//!
//! The paint is found from the words (an option is a word that begins with `--`; a name in backticks is a name), so a translated sentence is painted as it should be without markup.

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr;

use crate::term::Palette;

pub mod text;

pub use text::PAGE;

/// One piece of the help page.
#[derive(Clone, Copy, Debug)]
pub enum Entry {
    /// An empty line.
    Blank,
    /// A line that names a section (`Commands:`).
    Heading { key: &'static str, text: &'static str },
    /// A heading and the words that follow it on its line and the lines after: `Usage:`, `Fonts:` and `Exit status:`. A usage's words are painted as a usage line.
    Lead { key: &'static str, label: &'static str, text: &'static str, usage: bool },
    /// A usage line of a command, at indent 2. Never translated.
    Synopsis { text: &'static str },
    /// A paragraph at an indent.
    Para { key: &'static str, indent: u8, text: &'static str },
    /// An option or a variable and what it does: the label at the indent, the description from the column `col` (on the next line when the label is too long to leave room). `names`:
    /// the label is a name to type or set and not an option with its places to fill in.
    Row { key: &'static str, indent: u8, label: &'static str, col: u8, text: &'static str, names: bool },
}

/// An empty line.
pub const fn blank() -> Entry {
    Entry::Blank
}

/// A section's heading.
pub const fn heading(key: &'static str, text: &'static str) -> Entry {
    Entry::Heading { key, text }
}

/// A heading followed by its words (`Fonts:` and `Exit status:`).
pub const fn lead(key: &'static str, label: &'static str, text: &'static str) -> Entry {
    Entry::Lead { key, label, text, usage: false }
}

/// `Usage:` and the usage line after it.
pub const fn usage(key: &'static str, label: &'static str, text: &'static str) -> Entry {
    Entry::Lead { key, label, text, usage: true }
}

/// A command's usage line.
pub const fn synopsis(text: &'static str) -> Entry {
    Entry::Synopsis { text }
}

/// A paragraph.
pub const fn para(key: &'static str, indent: u8, text: &'static str) -> Entry {
    Entry::Para { key, indent, text }
}

/// An option and its description.
pub const fn row(key: &'static str, indent: u8, label: &'static str, col: u8, text: &'static str) -> Entry {
    Entry::Row { key, indent, label, col, text, names: false }
}

/// An environment variable and its description.
pub const fn variable(key: &'static str, indent: u8, label: &'static str, col: u8, text: &'static str) -> Entry {
    Entry::Row { key, indent, label, col, text, names: true }
}

/// The words of a piece the person reads: `english`, or its translation once a catalogue is there (replaces this body with a lookup of `english` under `key`).
pub fn message(key: &'static str, english: &'static str) -> Cow<'static, str> {
    let _ = key;
    Cow::Borrowed(english)
}

/// The sentence of the paragraph or the option the table keys `key`, on one line (the line breaks of the page are not part of it), or nothing when no entry has the key. The command
/// description, and so the completion scripts and the manual pages, take their words from here.
pub fn describe(key: &str) -> String {
    for entry in PAGE {
        let (found, text) = match *entry {
            Entry::Para { key: k, text, .. } | Entry::Row { key: k, text, .. } | Entry::Lead { key: k, text, .. } => (k, text),
            Entry::Heading { key: k, text } => (k, text),
            Entry::Blank | Entry::Synopsis { .. } => continue,
        };
        if found == key {
            return message(found, text).split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    String::new()
}

/// The first sentence of the words of `key` ([`describe`]), without its full stop: the short description a completion script lists beside an option. The whole sentence is the
/// long one, which a manual page gives.
pub fn summary(key: &str) -> String {
    let full = describe(key);
    match full.find(". ") {
        Some(end) => full[..end].to_owned(),
        None => full.trim_end_matches('.').to_owned(),
    }
}

/// The paragraphs of a command, in the order of the page, each on one line and apart from the options' rows: what a manual page says in its description. A command's entries are
/// those whose key begins with `command` and a dot.
pub fn paragraphs(command: &str) -> Vec<String> {
    let prefix = format!("{command}.");
    PAGE.iter()
        .filter_map(|entry| match *entry {
            Entry::Para { key, text, .. } if key.starts_with(&prefix) => Some(message(key, text).split_whitespace().collect::<Vec<_>>().join(" ")),
            _ => None,
        })
        .collect()
}

/// How a page is laid out and painted.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    /// Wrap to this many columns; `None`: the page as it is written.
    pub width: Option<usize>,
    /// Paint headings, names and places to fill in.
    pub colour: bool,
}

/// What a stretch of the page is, for its paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Plain,
    Heading,
    /// A name a person types: a command, an option, a value, a variable.
    Literal,
    /// A place a person fills in: `FILE`, `ADDR`, `<command>`.
    Placeholder,
}

type Spans<'a> = Vec<(Role, &'a str)>;

/// The page, laid out and painted as `look` says.
pub fn render(page: &[Entry], look: Look) -> String {
    let mut out = Out { text: String::new(), look };
    for entry in page {
        out.entry(entry);
    }
    out.text
}

/// The page as it is written, with no paint: what a pipe gets.
pub fn plain() -> String {
    render(PAGE, Look::default())
}

struct Out {
    text: String,
    look: Look,
}

impl Out {
    fn entry(&mut self, entry: &Entry) {
        match *entry {
            Entry::Blank => self.text.push('\n'),
            Entry::Heading { key, text } => {
                let words = message(key, text);
                self.line(0, &[(Role::Heading, words.as_ref())]);
            }
            Entry::Lead { key, label, text, usage } => self.lead(key, label, text, usage),
            Entry::Synopsis { text } => {
                let spans = usage_spans(text, 2);
                let lines = self.lines(&spans, 2, 6, true);
                self.block(2, 6, &lines);
            }
            Entry::Para { key, indent, text } => {
                let words = message(key, text);
                let spans = prose(&words);
                let lines = self.lines(&spans, usize::from(indent), usize::from(indent), false);
                self.block(usize::from(indent), usize::from(indent), &lines);
            }
            Entry::Row { key, indent, label, col, text, names } => self.row(key, usize::from(indent), label, usize::from(col), text, names),
        }
    }

    fn lead(&mut self, key: &'static str, label: &'static str, text: &'static str, usage: bool) {
        let heading = message(key, label);
        let words = message(key, text);
        let spans = if usage { usage_spans(&words, 1) } else { prose(&words) };
        let used = heading.width() + 1;
        let lines = self.lines(&spans, used, 0, false);
        for (n, line) in lines.iter().enumerate() {
            if n == 0 {
                self.span(Role::Heading, &heading);
                self.text.push(' ');
            }
            self.spans(line);
            self.text.push('\n');
        }
    }

    fn row(&mut self, key: &'static str, indent: usize, label: &str, col: usize, text: &'static str, names: bool) {
        let label_spans = usage_spans(label, if names { usize::MAX } else { 0 });
        let label_width = label.width();
        let words = message(key, text);
        let spans = prose(&words);
        let lines = self.lines(&spans, col, col, false);
        let beside = indent + label_width < col;
        self.pad(indent);
        self.spans(&label_spans);
        for (n, line) in lines.iter().enumerate() {
            if n == 0 && beside {
                self.pad(col - indent - label_width);
            } else {
                // The label is too long to leave room for the words beside it: they begin on the next line, at the column.
                if n == 0 {
                    self.text.push('\n');
                }
                self.pad(col);
            }
            self.spans(line);
            self.text.push('\n');
        }
    }

    /// `lines` after `first` spaces and then `rest` spaces each.
    fn block(&mut self, first: usize, rest: usize, lines: &[Spans<'_>]) {
        for (n, line) in lines.iter().enumerate() {
            self.pad(if n == 0 { first } else { rest });
            self.spans(line);
            self.text.push('\n');
        }
    }

    /// One line at `indent`.
    fn line(&mut self, indent: usize, spans: &[(Role, &str)]) {
        self.pad(indent);
        self.spans(spans);
        self.text.push('\n');
    }

    /// `spans` as lines: those of the text as it is written, or wrapped to the width the look has.
    fn lines<'a>(&self, spans: &[(Role, &'a str)], first_used: usize, rest_start: usize, keep_groups: bool) -> Vec<Spans<'a>> {
        match self.look.width {
            None => as_written(spans),
            Some(width) => wrapped(spans, first_used, rest_start, width, keep_groups),
        }
    }

    fn pad(&mut self, columns: usize) {
        self.text.extend(std::iter::repeat_n(' ', columns));
    }

    fn spans(&mut self, spans: &[(Role, &str)]) {
        for &(role, text) in spans {
            self.span(role, text);
        }
    }

    fn span(&mut self, role: Role, text: &str) {
        let style = match role {
            Role::Plain => None,
            Role::Heading => Some(Palette::HEADING),
            Role::Literal => Some(Palette::LITERAL),
            Role::Placeholder => Some(Palette::PLACEHOLDER),
        };
        match style {
            Some(style) if self.look.colour => self.text.push_str(&format!("{style}{text}{style:#}")),
            _ => self.text.push_str(text),
        }
    }
}

/// The lines of `spans` as the text breaks them.
fn as_written<'a>(spans: &[(Role, &'a str)]) -> Vec<Spans<'a>> {
    let mut lines: Vec<Spans<'a>> = vec![Vec::new()];
    for &(role, text) in spans {
        for (n, part) in text.split('\n').enumerate() {
            if n > 0 {
                lines.push(Vec::new());
            }
            if !part.is_empty()
                && let Some(line) = lines.last_mut()
            {
                line.push((role, part));
            }
        }
    }
    lines
}

/// `spans` broken into lines no wider than `width` columns, greedily, at spaces. `first_used` columns of the first line and `rest_start` of each later one are taken already
/// (an indent, a label). A word wider than the room is put on a line of its own and left to overflow. With `keep_groups` a bracket group (`[--out PATH]`) is one word.
fn wrapped<'a>(spans: &[(Role, &'a str)], first_used: usize, rest_start: usize, width: usize, keep_groups: bool) -> Vec<Spans<'a>> {
    let mut lines: Vec<Spans<'a>> = Vec::new();
    let mut line: Spans<'a> = Vec::new();
    let mut used = first_used;
    for unit in words(spans, keep_groups) {
        let size: usize = unit.iter().map(|(_, piece)| piece.width()).sum();
        if !line.is_empty() && used + 1 + size > width {
            lines.push(std::mem::take(&mut line));
            used = rest_start;
        }
        if !line.is_empty() {
            line.push((Role::Plain, " "));
            used += 1;
        }
        used += size;
        line.extend(unit);
    }
    lines.push(line);
    lines
}

/// The words of `spans`: runs with no space in them, each in the pieces of the spans it crosses.
fn words<'a>(spans: &[(Role, &'a str)], keep_groups: bool) -> Vec<Spans<'a>> {
    let mut words: Vec<Spans<'a>> = Vec::new();
    let mut unit: Spans<'a> = Vec::new();
    let mut depth = 0usize;
    for &(role, text) in spans {
        let mut start = 0;
        for (at, c) in text.char_indices() {
            match c {
                '[' if keep_groups => depth += 1,
                ']' if keep_groups => depth = depth.saturating_sub(1),
                _ => {}
            }
            if c.is_whitespace() && depth == 0 {
                if start < at {
                    unit.push((role, &text[start..at]));
                }
                if !unit.is_empty() {
                    words.push(std::mem::take(&mut unit));
                }
                start = at + c.len_utf8();
            }
        }
        if start < text.len() {
            unit.push((role, &text[start..]));
        }
    }
    if !unit.is_empty() {
        words.push(unit);
    }
    words
}

/// The paint of running words: a name in backticks, and an option (`--out`, `-h`) wherever it stands.
fn prose(text: &str) -> Spans<'_> {
    let mut spans = Vec::new();
    let mut plain_from = 0;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let before = text[..at].chars().next_back();
        let styled = if let Some(tail) = rest.strip_prefix('`') {
            tail.find('`').map(|end| end + 2)
        } else if rest.starts_with('-') && before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))) {
            option_len(rest)
        } else {
            None
        };
        match styled {
            Some(len) => {
                if plain_from < at {
                    spans.push((Role::Plain, &text[plain_from..at]));
                }
                spans.push((Role::Literal, &text[at..at + len]));
                at += len;
                plain_from = at;
            }
            None => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain_from < text.len() {
        spans.push((Role::Plain, &text[plain_from..]));
    }
    spans
}

/// The length of the option that `text` begins with: `--name` (a letter, then letters, digits and hyphens) or `-x` (one letter, then nothing a name continues with).
fn option_len(text: &str) -> Option<usize> {
    if let Some(name) = text.strip_prefix("--") {
        let len = name.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(name.len());
        let name = name[..len].trim_end_matches('-');
        return name.starts_with(|c: char| c.is_ascii_alphabetic()).then_some(2 + name.len());
    }
    let letter = text.strip_prefix('-')?;
    let mut chars = letter.chars();
    let first = chars.next().filter(char::is_ascii_alphabetic)?;
    chars.next().is_none_or(|c| !(c.is_alphanumeric() || c == '-')).then_some(1 + first.len_utf8())
}

/// The paint of a usage line or an option's label: the first `program_words` words are names (`diacraft-cli mcp`; all of them with `usize::MAX`), an option is a name, an
/// upper case word or one in angle brackets is a place to fill in, and any other word is a name too (a value typed as it stands: `outlines`). Brackets, bars and dots are plain.
fn usage_spans(text: &str, program_words: usize) -> Spans<'_> {
    let is_break = |c: char| c.is_whitespace() || matches!(c, '[' | ']' | '|' | '(' | ')');
    let mut spans = Vec::new();
    let mut word_no = 0usize;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let plain = rest.find(|c: char| !is_break(c)).unwrap_or(rest.len());
        if plain > 0 {
            spans.push((Role::Plain, &rest[..plain]));
            at += plain;
            continue;
        }
        let end = rest.find(is_break).unwrap_or(rest.len());
        let word = &rest[..end];
        // The commas and dots that end a word (`-V,`, `FILE...`) are not a part of it.
        let core = word.trim_end_matches([',', '.']);
        if core.is_empty() {
            spans.push((Role::Plain, word));
        } else {
            spans.push((role_of_word(core, word_no < program_words), core));
            if core.len() < word.len() {
                spans.push((Role::Plain, &word[core.len()..]));
            }
        }
        word_no += 1;
        at += end;
    }
    spans
}

fn role_of_word(word: &str, program: bool) -> Role {
    if word.is_empty() || word.chars().all(|c| matches!(c, '.' | '-')) {
        return Role::Plain;
    }
    let shouting =
        word.chars().any(|c| c.is_ascii_uppercase()) && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, ',' | '_'));
    if program {
        Role::Literal
    } else if (word.starts_with('<') && word.ends_with('>')) || shouting || word == "options" {
        Role::Placeholder
    } else {
        Role::Literal
    }
}

#[cfg(test)]
mod tests;
