//! Offline help for an app, all of it bounded and none of it networked.
//!
//! The crate holds what an app needs to show its own help, and what its tests and its checker need to hold that help to the rules:
//!
//! * [`md`] reads **Help Markdown**, a narrow subset of Markdown (headings, paragraphs, lists of two levels, a note, a code block, a table, a picture, and the inlines emphasis,
//!   code and links), with a header of `key: value` lines ([`header`]). The parser that checks a topic in CI is the one that renders it in the app, so what passes the check is
//!   what the pane draws. It is linear in the bytes of a topic (one pass over the lines, one over the characters of each block, no backtracking), and every rule it holds a topic
//!   to is a [`Code`] of a [`Problem`] with its line.
//! * [`catalog`] reads a **help catalog**, one language's translations of a topic's segments (tab-separated rows), and [`catalog::translate`] puts them into a topic; a segment
//!   with no translation, or a translation that changes a link or a code span, leaves the English. It also packs topics into one file for the web and reads that file back.
//! * [`fold`] folds text for matching (the language's own lower case, decomposition with the combining marks dropped, white space collapsed) and cuts a query to its cap.
//! * [`rank`] ranks commands against a query: every query word must be the start of a word of the label, of its place, or of one of its keywords, in the display language or in
//!   English; the label that starts with the query comes first, then recency, then the host's order. [`index`] does the same for the topics' words.
//! * [`tables`] reads the three small tables of a help tree: **tips** (a description and keywords for each control and command), the **map** (the topic of each place of the
//!   window) and the **sections** of the help home.
//! * [`url`] decides which web addresses a link may open and makes the address of a feedback page, [`chord`] reads and shows the keys a help text names, and [`nav`] holds the
//!   history of the places a person has been and the movement between the rows of a list.
//! * [`host`] holds the values of the app that uses the crate (the sites a link may open, the repository a feedback address names, where the help tree is), in one file.
//! * [`Bundle`] is a help tree read for use: [`embedded`] is the tree this crate was built with (the desktop programs embed it; the browser embeds only the tables),
//!   [`from_parts`] builds one from text the caller has (the web's fetched file, a test).
//!
//! **Nothing here panics, opens a file or a connection, or runs without bound.** Every size has a named constant below, the same for every path a text can arrive by (the
//! embedded tree, a fetched file, a contributor's folder), and a text past one is refused whole or cut as the constant says, with a [`Problem`] that names the line. Work is
//! linear in the bytes read (and, for ranking, in candidates times query words).
//!
//! ```
//! use diacraft_help::{Block, Span, Target, parse};
//!
//! let src = "---\nid: demo\ntitle: Demo\nsummary: A demo.\nkeywords: demo\nsite: no\n---\n## Start\n\nSee [help](topic:use-help) and press [Back](key:Alt+Left).\n";
//! let parsed = parse(src).unwrap();
//! assert!(parsed.problems.is_empty());
//! assert!(matches!(parsed.topic.doc.blocks.first(), Some(Block::Heading { anchor, .. }) if anchor == "start"));
//! let links = parsed.topic.links();
//! assert!(matches!(links.first(), Some((_, Target::Topic { id, .. })) if id == "use-help"));
//! assert!(matches!(parsed.topic.doc.blocks.get(1), Some(Block::Para(t)) if matches!(t.spans.first(), Some(Span::Text(_)))));
//! ```
//!
//! The crate depends on `unicode-normalization` and nothing else, names no app, and has no TOML, no serde and no logging: the host decides what to do with the problems a read
//! reports (a checker fails on any; the app skips the row or topic and logs).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::fmt;

pub mod catalog;
pub mod chord;
pub mod fold;
pub mod header;
pub mod host;
pub mod index;
pub mod md;
pub mod nav;
pub mod rank;
pub mod tables;
pub mod url;

mod block;
mod bundle;
mod embedded;
mod inline;

pub use bundle::{Bundle, Parts, from_parts};
pub use embedded::embedded;
pub use header::{Header, KeyLines, SiteFields};
pub use md::{Block, CodeLang, Doc, Item, List, Parsed, Span, Target, Text, Topic, parse};

// ---- the caps (design section 13.1). Only this file sets them; a later change comes with a counted test that shows why.

/// The most bytes of one topic's file (header and body): past it the checker refuses the topic and the app shows "This topic cannot be shown".
pub const MAX_TOPIC_BYTES: usize = 256 * 1024;
/// The most topics a bundle holds: the rest are ignored.
pub const MAX_TOPICS: usize = 1_000;
/// The most bytes of one language's topics together (the web's rule for a file): past it a bundle is refused.
pub const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
/// The most bytes of one line of a topic: past it the topic is refused.
pub const MAX_LINE_BYTES: usize = 16 * 1024;
/// The most bytes of a topic's header: past it the topic is refused.
pub const MAX_HEADER_BYTES: usize = 8 * 1024;
/// The most blocks of a topic: the rest are dropped, and the topic says it is cut short.
pub const MAX_BLOCKS: usize = 4_000;
/// The most spans (emphasis, code and links, nested ones counted) in one run of text: the rest of the run is dropped.
pub const MAX_SPANS: usize = 512;
/// The most levels of a list.
pub const MAX_LIST_DEPTH: usize = 2;
/// The most columns of a table.
pub const MAX_TABLE_COLUMNS: usize = 6;
/// The most rows of a table, the header row counted.
pub const MAX_TABLE_ROWS: usize = 60;
/// The most bytes of a link's target: a longer link is shown as its text.
pub const MAX_TARGET_BYTES: usize = 512;
/// The most characters of a query that are read.
pub const MAX_QUERY_CHARS: usize = 255;
/// The most words of a query that are read.
pub const MAX_QUERY_WORDS: usize = 16;
/// The most candidates a ranking scores: the rest are not looked at.
pub const MAX_SCORED: usize = 2_000;
/// The most command rows of the search list.
pub const MAX_COMMAND_ROWS: usize = 5;
/// The most recent-drawing rows of the search list.
pub const MAX_DRAWING_ROWS: usize = 5;
/// The help row of the search list: one.
pub const HELP_ROWS: usize = 1;
/// The most topics a search of the help returns.
pub const MAX_TOPIC_RESULTS: usize = 50;
/// The most words a topic index holds for one language: the rest are not indexed.
pub const MAX_INDEX_WORDS: usize = 400_000;
/// The most places a history holds: the oldest is dropped.
pub const MAX_HISTORY: usize = 64;
/// The most rows of the tips table and of the help map each: the rest are skipped.
pub const MAX_TIPS_ROWS: usize = 5_000;
/// The most rows of the help map.
pub const MAX_MAP_ROWS: usize = 5_000;
/// The most characters of a tip's description.
pub const MAX_DESCRIPTION_CHARS: usize = 400;
/// The most keywords of a tip.
pub const MAX_KEYWORDS: usize = 32;
/// The most bytes of a help catalog's file (a catalog is refused whole past it).
pub const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
/// The most rows of a help catalog: the rest are skipped.
pub const MAX_CATALOG_ROWS: usize = 20_000;
/// The most bytes of one row of a table or a catalog: the row is skipped.
pub const MAX_ROW_BYTES: usize = 16 * 1024;
/// The most bytes of the body of a feedback address, percent-encoded: it is cut at a character.
pub const MAX_FEEDBACK_BODY_BYTES: usize = 1_500;
/// How long the pointer rests on a control before its tip shows, in milliseconds.
pub const TIP_REST_MS: u64 = 600;
/// How long a tip stays, in milliseconds.
pub const TIP_SHOWN_MS: u64 = 30_000;
/// The most bytes the help code and its tables may add to the web module (measured by the site's web build, which compares it with this).
pub const WEB_BUDGET_BYTES: usize = 256 * 1024;
/// The most problems one read keeps: a hostile file can have a problem on every line, and the rest are not recorded.
pub const MAX_PROBLEMS: usize = 100;

/// What a rule of the help format says is wrong. A [`Problem`] carries one, with the line it was found on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Code {
    // sizes
    /// The topic is larger than [`MAX_TOPIC_BYTES`].
    TopicTooLarge,
    /// A line is longer than [`MAX_LINE_BYTES`].
    LineTooLong,
    /// The header is larger than [`MAX_HEADER_BYTES`].
    HeaderTooLarge,
    /// More than [`MAX_BLOCKS`] blocks.
    TooManyBlocks,
    /// More than [`MAX_SPANS`] spans in a run of text.
    TooManySpans,
    /// More than [`MAX_TOPICS`] topics, or the topics hold more than [`MAX_BUNDLE_BYTES`].
    BundleTooLarge,
    /// Two topics with one id.
    TopicDuplicate,
    // header
    /// The file has no header.
    NoHeader,
    /// The header's closing line is missing.
    HeaderNotClosed,
    /// A header line is not `key: value`.
    HeaderLine,
    /// A header key the format does not have.
    HeaderUnknownKey,
    /// A header key written twice.
    HeaderDuplicateKey,
    /// A header key that is required is missing.
    HeaderMissing,
    /// A header value that is not valid for its key.
    HeaderValue,
    /// A header value that is too long or too short.
    HeaderLength,
    // blocks
    /// HTML in a topic.
    Html,
    /// A setext heading (a line underlined with `=` or `-`).
    Setext,
    /// A thematic break (a line of dashes, stars or underscores).
    ThematicBreak,
    /// An indented line outside a list.
    IndentedLine,
    /// A control character in the text.
    ControlCharacter,
    /// A heading of a level other than two or three.
    HeadingLevel,
    /// A heading with no text.
    HeadingEmpty,
    /// An anchor that is not `{#word-word}` at the end of the heading.
    AnchorSyntax,
    /// Two headings with one anchor.
    AnchorDuplicate,
    /// A heading with no ASCII letter or digit to make an anchor from.
    AnchorEmpty,
    /// A list nested more than [`MAX_LIST_DEPTH`] levels.
    ListTooDeep,
    /// A list line indented other than by two spaces a level.
    ListIndent,
    /// A numbered and a bulleted item in one list.
    ListMarkerMix,
    /// A note inside a note.
    NoteNested,
    /// An empty line inside a note.
    NoteBlank,
    /// A code block never closed.
    CodeUnclosed,
    /// A code block's language other than `text`, `sh` or `json`.
    CodeLanguage,
    /// A table row with another number of cells than the header.
    TableShape,
    /// A table wider than [`MAX_TABLE_COLUMNS`].
    TableTooWide,
    /// A table taller than [`MAX_TABLE_ROWS`].
    TableTooTall,
    /// A table whose second row is not the separator row.
    TableSeparator,
    /// A picture that does not stand alone on its line.
    PictureAlone,
    /// A picture line that is not `![alt](site:name)`.
    PictureSyntax,
    /// A picture with no alt text.
    PictureAlt,
    /// A picture whose name is not a hashed-picture name.
    PictureName,
    // inlines
    /// An emphasis opened and never closed.
    UnclosedEmphasis,
    /// A backtick that opens code that is never closed.
    UnclosedCode,
    /// A `[` that makes no link, or a `]` that closes none.
    UnmatchedBracket,
    /// An emphasis, code span or link with nothing in it.
    EmptySpan,
    /// Emphasis nested more than once.
    EmphasisNesting,
    /// A run of four or more stars.
    StarRun,
    /// A backslash before a character that is not escaped.
    UnknownEscape,
    /// A link inside a link.
    LinkInLink,
    /// A link target longer than [`MAX_TARGET_BYTES`].
    TargetTooLong,
    /// A link target that is none of the kinds.
    TargetSyntax,
    /// A web address that is not on an allowed site.
    TargetWeb,
    /// A `key:` target that is not a key.
    TargetKey,
    /// A `topic:`, `ctl:` or `cmd:` target that is not an id, or a `#` that is not an anchor.
    TargetId,
    // tables of rows
    /// A row with the wrong number of columns.
    RowColumns,
    /// A row with an empty key or place.
    RowKey,
    /// Two rows with one key.
    RowDuplicate,
    /// A row with an escape the format does not have.
    RowEscape,
    /// A row longer than [`MAX_ROW_BYTES`].
    RowTooLong,
    /// A control character in a row.
    RowControl,
    /// More rows than a table holds.
    TooManyRows,
    /// A description longer than [`MAX_DESCRIPTION_CHARS`], or missing.
    DescriptionLength,
    /// More than [`MAX_KEYWORDS`] keywords.
    TooManyKeywords,
    /// A help map place that is none of the kinds.
    PlaceSyntax,
    /// A help map target that is neither a topic, a topic and an anchor, nor `none` with a note.
    MapTarget,
    // catalogs
    /// A catalog larger than [`MAX_CATALOG_BYTES`].
    CatalogTooLarge,
    /// A catalog row with a context the format does not have.
    CatalogContext,
    /// A catalog row with an empty source or translation.
    CatalogEmpty,
    /// A bidirectional embedding or override character in a translation.
    CatalogBidi,
    /// A translation that Help Markdown refuses.
    TranslationMarkup,
    /// A translation whose link targets differ from the English.
    TranslationTargets,
    /// A translation whose code spans differ from the English.
    TranslationCode,
    /// A translation of keywords that is not words.
    TranslationWords,
    // the packed file
    /// A packed bundle that does not follow its format.
    PackFormat,
}

impl Code {
    /// The words of the rule, for a person who has to fix the file. They hold no line and no text of the file.
    pub const fn words(self) -> &'static str {
        match self {
            Code::TopicTooLarge => "the topic is larger than a topic may be (256 KiB)",
            Code::LineTooLong => "a line is longer than a line may be (16 KiB)",
            Code::HeaderTooLarge => "the header is larger than a header may be (8 KiB)",
            Code::TooManyBlocks => "the topic has more than 4,000 blocks; the rest is cut",
            Code::TooManySpans => "a run of text has more than 512 spans of emphasis, code and links; the rest of it is cut",
            Code::BundleTooLarge => "the topics are more than a bundle may hold",
            Code::TopicDuplicate => "two topics have the same id",
            Code::NoHeader => "the file does not start with a header (a line of three dashes, `key: value` lines, a line of three dashes)",
            Code::HeaderNotClosed => "the header is never closed by a line of three dashes",
            Code::HeaderLine => "a header line is not `key: value`",
            Code::HeaderUnknownKey => "unknown header key",
            Code::HeaderDuplicateKey => "a header key is written twice",
            Code::HeaderMissing => "a required header key is missing",
            Code::HeaderValue => "a header value is not valid",
            Code::HeaderLength => "a header value has the wrong length",
            Code::Html => "HTML is not allowed in help: write Help Markdown",
            Code::Setext => "a heading underlined with = or - is not allowed: start the line with ## or ###",
            Code::ThematicBreak => "a line of dashes, stars or underscores is not allowed",
            Code::IndentedLine => {
                "an indented line is not allowed here: there is no indented code, and a list item is indented by two spaces a level"
            }
            Code::ControlCharacter => "a control character is not allowed in the text",
            Code::HeadingLevel => "a heading starts with ## or ###: the topic's title comes from its header",
            Code::HeadingEmpty => "a heading has no text",
            Code::AnchorSyntax => "an anchor is {#words-with-dashes} at the end of the heading",
            Code::AnchorDuplicate => "two headings have the same anchor",
            Code::AnchorEmpty => "a heading with no ASCII letter or digit needs an anchor: end it with {#words-with-dashes}",
            Code::ListTooDeep => "a list goes more than two levels deep",
            Code::ListIndent => "a list line is indented other than by two spaces for each level",
            Code::ListMarkerMix => "a numbered and a bulleted item are mixed in one list: end the list with a blank line",
            Code::NoteNested => "a note inside a note is not allowed",
            Code::NoteBlank => "an empty line inside a note is not allowed: end the note with a blank line",
            Code::CodeUnclosed => "a code block is never closed by a line of three backticks",
            Code::CodeLanguage => "a code block's language is `text`, `sh` or `json`",
            Code::TableShape => "a table row has another number of cells than the header",
            Code::TableTooWide => "a table has more than six columns",
            Code::TableTooTall => "a table has more than sixty rows",
            Code::TableSeparator => "a table's second row is a separator row, like |---|---|",
            Code::PictureAlone => "a picture stands alone on its line",
            Code::PictureSyntax => "a picture is written ![alt text](site:name)",
            Code::PictureAlt => "a picture has no alt text",
            Code::PictureName => "a picture's name is lowercase words with dashes",
            Code::UnclosedEmphasis => "emphasis is opened and never closed: write \\* for a star that is only a star",
            Code::UnclosedCode => "a backtick opens code that is never closed: write \\` for a backtick that is only a backtick",
            Code::UnmatchedBracket => "a bracket makes no link: write \\[ or \\] for a bracket that is only a bracket",
            Code::EmptySpan => "emphasis, code or a link has nothing in it",
            Code::EmphasisNesting => "emphasis is nested more than once",
            Code::StarRun => "four or more stars in a row: write \\* for a star that is only a star",
            Code::UnknownEscape => "a backslash before a character that is not escaped: the escapes are \\* \\[ \\] \\( \\) \\` \\| and \\\\",
            Code::LinkInLink => "a link inside a link is not allowed",
            Code::TargetTooLong => "a link target is longer than 512 bytes",
            Code::TargetSyntax => "a link target is topic:, #, ctl:, cmd:, key: or a web address on an allowed site",
            Code::TargetWeb => "a web address that is not on an allowed site",
            Code::TargetKey => "a key: target is not a key such as Cmd+Shift+Z or F1",
            Code::TargetId => {
                "a link target is not an id (a topic's lowercase words with dashes, a control's CamelCase name, a command's dotted name)"
            }
            Code::RowColumns => "a row has the wrong number of columns",
            Code::RowKey => "a row has an empty key",
            Code::RowDuplicate => "two rows have the same key",
            Code::RowEscape => "a row has an escape the format does not have: the escapes are \\n, \\t and \\\\",
            Code::RowTooLong => "a row is longer than a row may be (16 KiB)",
            Code::RowControl => "a row has a control character",
            Code::TooManyRows => "more rows than the table holds; the rest are skipped",
            Code::DescriptionLength => "a description is empty or longer than 400 characters",
            Code::TooManyKeywords => "more than 32 keywords",
            Code::PlaceSyntax => "a place is not ribbon:, group:, tab:, dialog:, pane:, backstage:, window: or cmd: with a name",
            Code::MapTarget => "a target is a topic id, a topic id and #anchor, or none with a note that says why",
            Code::CatalogTooLarge => "the catalog is larger than a catalog may be (2 MiB)",
            Code::CatalogContext => "a catalog row has a context the help format does not have",
            Code::CatalogEmpty => "a catalog row has an empty source or translation",
            Code::CatalogBidi => "a translation holds a bidirectional embedding or override character",
            Code::TranslationMarkup => "a translation is not valid Help Markdown",
            Code::TranslationTargets => "a translation changes the link targets of the English",
            Code::TranslationCode => "a translation changes the code spans of the English",
            Code::TranslationWords => "a translation of keywords is not words",
            Code::PackFormat => "a packed bundle does not follow its format",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.words())
    }
}

/// A rule a text breaks: which one, where, and what the text said there. `line` counts from 1 (0 is the file as a whole).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The line of the file, from 1; 0 when the problem is the file's as a whole.
    pub line: usize,
    /// The rule.
    pub code: Code,
    /// What the text said, cut short, or empty. Never longer than 80 characters.
    pub detail: String,
}

impl Problem {
    /// A problem of `code` on `line`, with the text it was found at (cut to 80 characters).
    pub fn new(line: usize, code: Code, detail: &str) -> Problem {
        let detail: String = detail.chars().take(80).collect();
        Problem { line, code, detail }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: ", self.line)?;
        }
        f.write_str(self.code.words())?;
        if !self.detail.is_empty() {
            write!(f, " (`{}`)", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for Problem {}

/// Records a problem: not when the last one recorded is the same fault on the same line (a line that breaks one rule twice says it once), and no more than
/// [`MAX_PROBLEMS`].
pub(crate) fn report(problems: &mut Vec<Problem>, line: usize, code: Code, detail: &str) {
    if problems.len() >= MAX_PROBLEMS || problems.last().is_some_and(|p| p.line == line && p.code == code) {
        return;
    }
    problems.push(Problem::new(line, code, detail));
}
