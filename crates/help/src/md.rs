//! Help Markdown: the types of a parsed topic, and `parse`.
//!
//! A topic is a header ([`crate::header`]) and a body of blocks (headings, paragraphs, lists of two levels, a note, a code block, a table, a picture) whose text holds
//! emphasis, code and links. The whole format is in the README of the help tree; here is what a read gives. [`parse`] never panics and never reads more than the caps
//! allow: a topic that breaks a rule of the format still reads (what breaks a rule is read as plainly as it can be) and comes back with its [`Problem`]s, each with a
//! line, so that one parser serves the checker (any problem fails) and the program (a topic that cannot be read is skipped, the rest are shown).

use crate::chord::Chord;
use crate::header::{self, Header};
use crate::{Code, MAX_LINE_BYTES, MAX_TOPIC_BYTES, Problem, block, inline};

/// A parsed topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topic {
    /// The header.
    pub header: Header,
    /// The body as written, after the header.
    pub body: String,
    /// The body as blocks.
    pub doc: Doc,
}

impl Topic {
    /// Every link of the topic with the line of the text it is in, in order.
    pub fn links(&self) -> Vec<(usize, &Target)> {
        fn walk<'t>(spans: &'t [Span], line: usize, out: &mut Vec<(usize, &'t Target)>) {
            for span in spans {
                match span {
                    Span::Strong(inner) | Span::Em(inner) => walk(inner, line, out),
                    Span::Link { target, text } => {
                        out.push((line, target));
                        walk(text, line, out);
                    }
                    Span::Text(_) | Span::Code(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        for block in &self.doc.blocks {
            for text in block.texts() {
                walk(&text.spans, text.line, &mut out);
            }
        }
        out
    }

    /// The anchors of the topic's headings, in order.
    pub fn anchors(&self) -> Vec<&str> {
        self.doc.blocks.iter().filter_map(|b| if let Block::Heading { anchor, .. } = b { Some(anchor.as_str()) } else { None }).collect()
    }
}

/// The blocks of a topic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doc {
    /// The blocks, in order.
    pub blocks: Vec<Block>,
    /// Whether blocks past the cap were dropped.
    pub cut_short: bool,
}

/// The language of a code block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeLang {
    /// Plain text.
    Text,
    /// A shell command.
    Sh,
    /// JSON.
    Json,
}

/// A run of text as written and as spans.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    /// The inline source: the lines joined with one space and trimmed. The key of the text's translation.
    pub source: String,
    /// The source read.
    pub spans: Vec<Span>,
    /// The line of the file the text starts at (0 for a text made by hand).
    pub line: usize,
}

/// A piece of a run of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Span {
    /// Plain text.
    Text(String),
    /// `**strong**`.
    Strong(Vec<Span>),
    /// `*emphasis*`.
    Em(Vec<Span>),
    /// `` `code` ``.
    Code(String),
    /// `[text](target)`.
    Link {
        /// Where it goes.
        target: Target,
        /// What it says.
        text: Vec<Span>,
    },
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `topic:id` or `topic:id#anchor`.
    Topic {
        /// The topic.
        id: String,
        /// The place in it.
        anchor: Option<String>,
    },
    /// `#anchor`: a place in this topic.
    Anchor(String),
    /// `ctl:ControlId`.
    Control(String),
    /// `cmd:command.id`.
    Command(String),
    /// `key:Cmd+Shift+Z`.
    Key(Chord),
    /// A web address on an allowed site (the only way to make one is to read a link, which refuses the rest).
    Web(String),
}

/// One item of a list, and the list nested in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The item's text.
    pub text: Text,
    /// The list under it (items of a nested list have none: a list has two levels).
    pub sub: Option<List>,
}

/// A list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    /// Numbered, else bulleted.
    pub ordered: bool,
    /// The items.
    pub items: Vec<Item>,
}

/// A block of a topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// `##` or `###`.
    Heading {
        /// 2 or 3.
        level: u8,
        /// The heading's text.
        text: Text,
        /// Its anchor.
        anchor: String,
    },
    /// A paragraph.
    Para(Text),
    /// A list.
    List(List),
    /// A note.
    Note(Text),
    /// A code block.
    Code {
        /// Its language.
        lang: CodeLang,
        /// Its text, as written.
        text: String,
    },
    /// A table.
    Table {
        /// The header row.
        header: Vec<Text>,
        /// The rows.
        rows: Vec<Vec<Text>>,
    },
    /// A picture.
    Picture {
        /// Its alt text.
        alt: String,
        /// The hashed picture's name.
        name: String,
        /// Its line in the file.
        line: usize,
    },
}

impl Block {
    /// Every run of text of the block, in order: a heading's, a paragraph's, each item's and nested item's, a note's, the cells of a table. Code and pictures have none.
    pub fn texts(&self) -> Vec<&Text> {
        let mut out = Vec::new();
        match self {
            Block::Heading { text, .. } | Block::Para(text) | Block::Note(text) => out.push(text),
            Block::List(list) => {
                for item in &list.items {
                    out.push(&item.text);
                    out.extend(item.sub.iter().flat_map(|s| s.items.iter().map(|i| &i.text)));
                }
            }
            Block::Table { header, rows } => {
                out.extend(header.iter());
                out.extend(rows.iter().flatten());
            }
            Block::Code { .. } | Block::Picture { .. } => {}
        }
        out
    }
}

/// What reading a topic gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The topic, with what could be read.
    pub topic: Topic,
    /// What broke a rule, in the order it was found, at most [`crate::MAX_PROBLEMS`].
    pub problems: Vec<Problem>,
}

/// Reads a topic. A file over the cap, a line over the cap, a header over the cap or with no usable `id` and `title`, and a file with no header are refused; anything else
/// reads, with its problems.
pub fn parse(src: &str) -> Result<Parsed, Problem> {
    if src.len() > MAX_TOPIC_BYTES {
        return Err(Problem::new(0, Code::TopicTooLarge, ""));
    }
    if let Some((n, _)) = src.lines().enumerate().find(|(_, l)| l.len() > MAX_LINE_BYTES) {
        return Err(Problem::new(n + 1, Code::LineTooLong, ""));
    }
    let mut problems = Vec::new();
    let (header, body, body_line) = header::read(src, &mut problems)?;
    let doc = block::parse_body(body, body_line, &mut problems);
    Ok(Parsed { topic: Topic { header, body: body.to_owned(), doc }, problems })
}

/// Reads the inline source `source`, which is on line `line`: its spans, with each problem put in `problems` at that line.
pub fn parse_inlines(source: &str, line: usize, problems: &mut Vec<Problem>) -> Vec<Span> {
    let (spans, diags) = inline::parse(source);
    for d in diags {
        crate::report(problems, line, d.code, &d.detail);
    }
    spans
}

/// The anchor of a heading's text: its ASCII letters and digits in lowercase, the runs of anything else as one dash, no dash at either end, at most 64 bytes.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 64 {
            break;
        }
    }
    out.truncate(64);
    while out.ends_with('-') {
        out.pop();
    }
    out
}
