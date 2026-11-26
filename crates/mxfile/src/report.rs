//! What the reader says beside the drawing (design §1.2): the form of the file, how many pages it held, each warning class with a
//! count and up to [`MAX_CASES`](crate::limits::MAX_CASES) named cases, the connectors the engine is to route, and the work the read did, counted in the units each loop walks.
//!
//! **Every loop charges the read's work before it runs** (design §7): [`Reading::charge`] adds the units to a [`ReadWork`] and to one budget, [`Limits::max_read_work`], and
//! refuses in words, as `drawio_too_complex`, the charge that would pass it. A unit is what the loop walks (a byte, a character, an event, a page), not a stand-in such as a
//! call, and the counts are always on, so that a test holds a read to a count and not to a time, and a refusal says what it cost as a read does.

use std::collections::BTreeMap;

use crate::error::{DrawioError, shown};
use crate::limits::Limits;

/// The longest a case of a warning is, in characters, escaped and cut.
pub const CASE_CHARS: usize = 120;

/// The longest a page's name is in a sentence or a case, in characters.
pub const NAME_CHARS: usize = 64;

/// A case of a warning that names a page and a subject: `<page>: <subject>`, the page's name escaped and cut to [`NAME_CHARS`], the subject escaped and cut to what is left of
/// [`CASE_CHARS`], so that the whole is never longer than that. Nothing in it is ever longer than the text it names.
pub(crate) fn page_case(page: &str, subject: &str) -> String {
    let prefix = shown(page, NAME_CHARS);
    let room = CASE_CHARS.saturating_sub(prefix.chars().count() + 2);
    format!("{prefix}: {}", shown(subject, room))
}

/// How the drawing was stored in the file it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// A file whose pages all hold their model as an element.
    Plain,
    /// A file with at least one page that holds its model as compressed text.
    Compressed,
    /// An SVG image that holds the file in its `content`.
    Svg,
    /// A PNG image that holds the file in a text chunk.
    Png,
    /// A bare `mxGraphModel` or `diagram` with no `mxfile` around it.
    Page,
}

impl Form {
    /// The word the engine's result gives for it: `plain`, `compressed`, `svg`, `png` or `page`.
    pub fn as_str(self) -> &'static str {
        match self {
            Form::Plain => "plain",
            Form::Compressed => "compressed",
            Form::Svg => "svg",
            Form::Png => "png",
            Form::Page => "page",
        }
    }
}

/// A warning class as the report holds it: how many times it happened, and the first named cases.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Noted {
    /// Every time it happened, past the cases kept.
    pub count: u64,
    /// The first cases, at most `max_cases`, each escaped and cut ([`shown`]).
    pub cases: Vec<String>,
}

/// Where the own label of a connector sits along its route, in inches, so that the engine puts it back there after it routes the connector (design §5.3).
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// The fraction of the route's length, 0 to 1.
    pub t: f64,
    /// The distance from the route, to the right of the direction of travel.
    pub d: f64,
    /// The offset added after, in inches.
    pub offset: (f64, f64),
}

/// A connector the engine is to route at import: a right-angle connector with no waypoints and both ends placed (design §5.3). Filled.
#[derive(Debug, Clone, PartialEq)]
pub struct ToRoute {
    /// The page, by its position in the drawing.
    pub page: u32,
    /// The connector's shape id on that page.
    pub id: u32,
    /// Where its own label sits, if it has one.
    pub label: Option<Anchor>,
}

/// The work a read did, in the units each of its loops walks. Every field only grows; [`ReadWork::default`] is all zeros.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadWork {
    /// Bytes of the file the sniff looked at: at most 64 KiB.
    pub sniff_bytes: u64,
    /// Bytes of XML handed to the parser: the file's own, and each decoded page's.
    pub xml_bytes: u64,
    /// Events the XML parser read, as the container counts them.
    pub xml_events: u64,
    /// Characters of the text of compressed pages that the Base64 step walked.
    pub base64_chars: u64,
    /// Bytes the inflate made, both attempts of a page counted (raw, and zlib after it).
    pub inflated: u64,
    /// Bytes of inflated text that the URI step walked.
    pub uri_bytes: u64,
    /// Bytes of decoded text that the clean walked.
    pub clean_bytes: u64,
    /// Steps of the reader of labels (`html`): the bytes of each label the tokenizer walks, each token, the bytes each piece of text and each attribute it reads decodes and
    /// the look ahead of each `&`, and the bytes of each `style` it parses.
    pub label_steps: u64,
    /// Pages walked.
    pub pages: u64,
    /// Nodes of a page's XML tree that the reader of the cells walked: the children of each element it expanded and the attributes it looked at (`cells`, `file`).
    pub tree_nodes: u64,
    /// Cells visited, once for each pass of the one linear walk that makes the list of a page's cells a tree (`cells`).
    pub cells: u64,
    /// Bytes of JSON text (`vars`, `backgroundImage`) that the reader scanned (`file`).
    pub json_bytes: u64,
    /// Bytes of style strings that the style cache hashed to find a style it had, or the grammar walked to read one it had not (`style`).
    pub style_bytes: u64,
    /// Pieces of style strings (what lies between two `;`, empty ones too) and the keys a named style adds, each applied once for each style the cache reads (`style`).
    pub style_pieces: u64,
}

impl ReadWork {
    /// The sum of every unit: what [`Limits::max_read_work`] holds a read to.
    pub fn total(&self) -> u64 {
        [
            self.sniff_bytes,
            self.xml_bytes,
            self.xml_events,
            self.base64_chars,
            self.inflated,
            self.uri_bytes,
            self.clean_bytes,
            self.pages,
            self.tree_nodes,
            self.cells,
            self.json_bytes,
            self.style_bytes,
            self.style_pieces,
            self.label_steps,
        ]
        .iter()
        .fold(0u64, |sum, n| sum.saturating_add(*n))
    }
}

/// A unit of work: what a loop of the reader walks. A task that adds a loop adds its unit here and a field to [`ReadWork`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// A byte the sniff looked at.
    SniffBytes,
    /// A byte of XML handed to the parser.
    XmlBytes,
    /// An event of the XML parser.
    XmlEvents,
    /// A character of Base64 text.
    Base64Chars,
    /// A byte the inflate made.
    Inflated,
    /// A byte of URI-encoded text.
    UriBytes,
    /// A byte of text the clean walked.
    CleanBytes,
    /// A step of the reader of labels.
    LabelSteps,
    /// A page.
    Pages,
    /// A node of a page's XML tree (a child of an element the reader expands, an attribute it looks at).
    TreeNodes,
    /// A cell, in one pass of the walk that makes a page's cells a tree.
    Cells,
    /// A byte of a JSON attribute.
    JsonBytes,
    /// A byte of a style string, hashed to find its style or walked to read it.
    StyleBytes,
    /// A piece of a style string, or a key a named style adds.
    StylePieces,
}

/// What a read tells beside the drawing it makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// How the file stored its drawing.
    pub form: Form,
    /// How many pages the drawing has.
    pub pages: u32,
    /// Each warning class (M-WARN) with its count and cases.
    pub warnings: BTreeMap<&'static str, Noted>,
    /// The connectors the engine is to route.
    pub to_route: Vec<ToRoute>,
    /// What the read cost.
    pub work: ReadWork,
    /// The bytes the model holds of what was read.
    pub model_bytes: u64,
}

impl Report {
    fn new() -> Report {
        Report { form: Form::Plain, pages: 0, warnings: BTreeMap::new(), to_route: Vec::new(), work: ReadWork::default(), model_bytes: 0 }
    }
}

/// A budget: `charge` keeps what fits and refuses what does not. It never wraps, once it has refused it refuses everything after (even a charge of nothing), and the sum it
/// keeps never passes the cap. Small enough to prove (design §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cap {
    used: u64,
    max: u64,
    refused: bool,
}

impl Cap {
    pub(crate) fn new(max: u64) -> Cap {
        Cap { used: 0, max, refused: false }
    }

    /// Keeps `n` more and says `true`, when the sum stays within the cap; else keeps nothing and says `false`, now and for every charge after.
    pub(crate) fn charge(&mut self, n: u64) -> bool {
        if self.refused {
            return false;
        }
        match self.used.checked_add(n) {
            Some(sum) if sum <= self.max => {
                self.used = sum;
                true
            }
            _ => {
                self.refused = true;
                false
            }
        }
    }

    /// What fits still, which is nothing once the cap has refused.
    pub(crate) fn left(&self) -> u64 {
        if self.refused { 0 } else { self.max.saturating_sub(self.used) }
    }
}

/// A read in progress: the limits it is held to, the report it makes, and the budgets it spends. The stages of the reader take it by `&mut` and charge their units to it.
#[derive(Debug)]
pub struct Reading<'l> {
    /// What the read is held to.
    pub limits: &'l Limits,
    /// What the read has told so far.
    pub report: Report,
    /// The name of the page being decoded, escaped and cut, for the words of a refusal; empty when no page is named.
    pub(crate) page: String,
    spent: Cap,
    file_text: Cap,
}

impl<'l> Reading<'l> {
    /// A read that has done nothing yet, held to `limits`.
    pub fn new(limits: &'l Limits) -> Reading<'l> {
        Reading {
            limits,
            report: Report::new(),
            page: String::new(),
            spent: Cap::new(limits.max_read_work),
            file_text: Cap::new(limits.max_file_text),
        }
    }

    /// Charges `n` units of `unit` to the work of the read: kept, and counted in [`Report::work`], when the sum of every unit stays within [`Limits::max_read_work`];
    /// refused as `drawio_too_complex` when it does not, and then nothing is counted.
    pub fn charge(&mut self, unit: Unit, n: u64) -> Result<(), DrawioError> {
        if !self.spent.charge(n) {
            return Err(DrawioError::too_complex(format!(
                "The file needs more work than DiaCraft will do to read one file (more than {} steps).",
                self.limits.max_read_work
            )));
        }
        let work = &mut self.report.work;
        let slot = match unit {
            Unit::SniffBytes => &mut work.sniff_bytes,
            Unit::XmlBytes => &mut work.xml_bytes,
            Unit::XmlEvents => &mut work.xml_events,
            Unit::Base64Chars => &mut work.base64_chars,
            Unit::Inflated => &mut work.inflated,
            Unit::UriBytes => &mut work.uri_bytes,
            Unit::CleanBytes => &mut work.clean_bytes,
            Unit::LabelSteps => &mut work.label_steps,
            Unit::Pages => &mut work.pages,
            Unit::TreeNodes => &mut work.tree_nodes,
            Unit::Cells => &mut work.cells,
            Unit::JsonBytes => &mut work.json_bytes,
            Unit::StyleBytes => &mut work.style_bytes,
            Unit::StylePieces => &mut work.style_pieces,
        };
        *slot = slot.saturating_add(n);
        Ok(())
    }

    /// Notes a warning of `class`, with `case` (raw: it is escaped and cut here) among its cases when it has room.
    pub fn note(&mut self, class: &'static str, case: &str) {
        let max = self.limits.max_cases;
        let noted = self.report.warnings.entry(class).or_default();
        noted.count = noted.count.saturating_add(1);
        if noted.cases.len() < max {
            noted.cases.push(shown(case, CASE_CHARS));
        }
    }

    /// Notes a warning of `class` with a case that is made (and so escaped and cut) only when the class has room for it: the notes of a page of a million cells cost a count each
    /// and the first [`MAX_CASES`](crate::limits::MAX_CASES) cases. `case` gives the finished text, as [`page_case`] makes it.
    pub(crate) fn note_lazy(&mut self, class: &'static str, case: impl FnOnce() -> String) {
        let max = self.limits.max_cases;
        let noted = self.report.warnings.entry(class).or_default();
        noted.count = noted.count.saturating_add(1);
        if noted.cases.len() < max {
            noted.cases.push(case());
        }
    }

    /// Notes a warning of `class` for a page, with the case `<name>: step <n> (<what>)`.
    pub(crate) fn note_step(&mut self, class: &'static str, page: &str, step: crate::decode::Step) {
        let max = self.limits.max_cases;
        let case = format!("{}: step {} ({})", shown(page, NAME_CHARS), step.number(), step.words());
        let noted = self.report.warnings.entry(class).or_default();
        noted.count = noted.count.saturating_add(1);
        if noted.cases.len() < max {
            noted.cases.push(case);
        }
    }

    /// What is left of the text of all the pages together, in bytes.
    pub(crate) fn file_text_left(&self) -> u64 {
        self.file_text.left()
    }

    /// Spends `n` bytes of the text of all the pages together: `false` when they would pass the cap.
    pub(crate) fn spend_file_text(&mut self, n: u64) -> bool {
        self.file_text.charge(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cap_keeps_what_fits_and_refuses_what_does_not_and_stays_refused() {
        let mut cap = Cap::new(10);
        assert!(cap.charge(4) && cap.charge(6), "exactly the cap fits");
        assert_eq!(cap.left(), 0);
        assert!(cap.charge(0), "nothing more is no charge past it, until it has refused");
        assert!(!cap.charge(1), "one past it is refused");
        assert!(!cap.charge(0), "and once refused it stays refused, even for nothing");
        assert_eq!(cap.left(), 0);
        // What is refused is not kept: a charge that does not fit leaves the sum where it was.
        let mut cap = Cap::new(10);
        assert!(cap.charge(8));
        assert!(!cap.charge(3));
        assert_eq!((cap.used, cap.left()), (8, 0), "the sum kept never passes the cap, and a refused cap has no room");
    }

    #[test]
    fn a_cap_never_wraps() {
        let mut cap = Cap::new(u64::MAX);
        assert!(cap.charge(u64::MAX));
        assert!(!cap.charge(1), "the sum would not fit in a u64");
        let mut cap = Cap::new(u64::MAX);
        assert!(cap.charge(u64::MAX - 1));
        assert!(!cap.charge(u64::MAX), "the sum overflows");
        assert_eq!(cap.used, u64::MAX - 1);
        let mut none = Cap::new(0);
        assert!(none.charge(0) && !none.charge(1));
    }

    #[test]
    fn the_total_of_the_work_is_the_sum_of_its_units_and_saturates() {
        let work = ReadWork {
            sniff_bytes: 1,
            xml_bytes: 2,
            xml_events: 3,
            base64_chars: 4,
            inflated: 5,
            uri_bytes: 6,
            clean_bytes: 7,
            pages: 8,
            tree_nodes: 9,
            cells: 10,
            json_bytes: 11,
            style_bytes: 12,
            style_pieces: 13,
            label_steps: 14,
        };
        assert_eq!(work.total(), 105);
        let big = ReadWork { inflated: u64::MAX, pages: 5, ..ReadWork::default() };
        assert_eq!(big.total(), u64::MAX);
    }
}
