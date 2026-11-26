//! DiaCraft draw.io reader: finds a drawing in a file, decodes its pages (plain and compressed), and reads them into the ShapeSheet model within budgets. The design is
//! the project's design of the draw.io import and the contract is (its vectors file).
//! The reader is import only: nothing here writes a draw.io file.
//!
//! This crate is a file format (layer 3, beside `odg`, `vsdx` and `native`). It reads bytes it is given: no file system, no clock, no network, and **nothing a file names
//! is fetched and nothing in it is run**. It builds the document model (layer 1) and reads XML and inflates through the container (layer 0), which is the front door
//! for hostile bytes: no DTD, no entity expansion, depth, items and attribute sizes capped, and an inflate that charges its output before it keeps it. It uses no other
//! crate of its own layer.
//!
//! * [`sniff()`] says what a file is by its first bytes and never by its name: a file of pages (`mxfile`), a page with no wrapper, an SVG or a PNG image that may hold
//!   a drawing, or a library of shapes. It looks at no more than 64 KiB, and claims nothing it cannot read, so that the engine goes on to the next reader.
//! * [`read`] reads a file into a [`diacraft_doc::Document`] and a [`Report`]: the pages, named by R7 and made unique, in order. **This is the frame and the cells**:
//!   each page is decoded (a plain page is the element, a compressed page goes through [`decode`]'s five steps), its settings, `defs` and cells are read ([`file`],
//!   [`cells`], [`geom`]) and the pages of the drawing are empty; the styles, labels and shapes the model is made of are the tasks that follow.
//! * [`file`] and [`cells`] are the file, its pages and their cells: [`file::read_sources`] hands each page on with its [`file::PageSettings`], its `defs` and its
//!   [`cells::Graph`], the flat list of the page's cells repaired into a tree by one walk that visits each cell once, and gives the file's own `vars`.
//! * [`decode`] is the compressed page: forgiving Base64, the bounded inflate, URI decoding, the clean, the parse, each step public so that the vectors of the
//!   specification can hold each to its text, and the walk of an `mxfile` to its pages.
//! * [`html`] reads a label, plain or HTML, into paragraphs of runs (S-R24 to S-R27): by its own bounded tokenizer, never rendering HTML and never running or fetching what it holds. No
//!   stage calls it yet (does, when it writes the text of a shape).
//! * [`limits`] holds every cap, as a named constant with the rule that sets it, and [`Limits`] carries them; [`report`] holds the warnings, the counts of work, and
//!   [`Reading`], which every loop charges its units to **before it runs** (design §7).
//!
//! **Nothing panics, and every loop's work is bounded and counted** in the units it walks ([`ReadWork`]). Every refusal has a [`class`](DrawioError::class), the
//! specification's word, and says in words what was wrong without naming the format or holding file content beyond a page's name, cut and escaped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Document, MAX_NAME_CHARS};

pub mod cells;
pub mod decode;
pub mod error;
pub mod file;
pub mod geom;
pub mod html;
pub mod limits;
pub mod report;
pub mod sniff;
pub mod style;

pub use error::{DrawioError, ErrorKind};
pub use limits::Limits;
pub use report::{Anchor, Form, Noted, ReadWork, Reading, Report, ToRoute, Unit};
pub use sniff::{Kind, sniff};

use cells::Graph;
use decode::warn;
use file::{PageSettings, PageSource};

/// What a read gives: the drawing and what the read says beside it.
#[derive(Debug)]
pub struct Read {
    /// The drawing, a new one: a page for each page of the file.
    pub doc: Document,
    /// The warnings, the counts and the connectors to route.
    pub report: Report,
}

/// Reads the draw.io file in `bytes` into a new drawing, within `limits`. `NotMine` ([`ErrorKind`]) means the bytes are not a drawing of this reader's kind, so that the
/// engine goes on to the next reader; any other error is a refusal in words, with its class.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<Read, DrawioError> {
    read_counted(bytes, limits, &mut ReadWork::default())
}

/// [`read`], and the work it did added to `work` **whether it reads or refuses**: the refusals are where a hostile file spends what it can.
pub fn read_counted(bytes: &[u8], limits: &Limits, work: &mut ReadWork) -> Result<Read, DrawioError> {
    let mut reading = Reading::new(limits);
    let result = read_with(bytes, &mut reading);
    *work = reading.report.work;
    result.map(|doc| Read { doc, report: reading.report })
}

fn read_with(bytes: &[u8], reading: &mut Reading<'_>) -> Result<Document, DrawioError> {
    let mut doc = Document::empty();
    let mut names = Names::default();
    // The file's own properties (`Mxfile::vars`) are the model's, when it is built.
    let _file = file::read_sources(bytes, reading, |reading, page| add_page(&mut doc, &mut names, reading, &page))?;
    if reading.report.pages == 0 {
        // No page at all is one empty page.
        reading.note(warn::NO_PAGES, "mxfile");
        let empty =
            PageSource { position: 1, id: "0".to_owned(), name: None, settings: PageSettings::default(), defs: Vec::new(), graph: Graph::default() };
        add_page(&mut doc, &mut names, reading, &empty)?;
    }
    doc.check_limits().map_err(|e| DrawioError::too_complex(format!("The drawing is past a limit of the model: {e}.")))?;
    Ok(doc)
}

/// The names the pages have been given: each is taken once.
#[derive(Default)]
struct Names {
    taken: HashSet<String>,
    /// The next number to try for a name that was taken, so that a thousand pages of one name cost a thousand tries and not half a million.
    next: HashMap<String, usize>,
}

impl Names {
    /// `base`, or `base (2)`, `base (3)` and so on until the name is not taken, the base cut so that the whole is within the model's 255 characters.
    fn unique(&mut self, base: String) -> String {
        if self.taken.insert(base.clone()) {
            return base;
        }
        let mut n = self.next.get(&base).copied().unwrap_or(2);
        loop {
            let suffix = format!(" ({n})");
            let room = MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
            let candidate: String = base.chars().take(room).chain(suffix.chars()).collect();
            n += 1;
            if self.taken.insert(candidate.clone()) {
                self.next.insert(base, n);
                return candidate;
            }
        }
    }
}

/// The name a page is given before it is made unique: its `name` with a control character made a space and cut to the model's 255 characters, or
/// `Page-N` for its place N counting from 1 when it has none or it is blank.
fn page_name(name: Option<&str>, position: usize) -> String {
    let cut: String = name.unwrap_or_default().chars().map(|c| if c.is_control() { ' ' } else { c }).take(MAX_NAME_CHARS).collect();
    if cut.trim().is_empty() { format!("Page-{position}") } else { cut }
}

/// Adds the page the walk gave to the drawing: empty, named. Its cells are read (`page.graph`), and the shapes the model is made of them are the task that follows.
fn add_page(doc: &mut Document, names: &mut Names, reading: &mut Reading<'_>, page: &PageSource) -> Result<(), DrawioError> {
    let name = names.unique(page_name(page.name.as_deref(), page.position));
    doc.add_page(Some(&name)).map_err(|e| DrawioError::too_complex(format!("The drawing cannot take another page: {e}.")))?;
    reading.report.pages = reading.report.pages.saturating_add(1);
    Ok(())
}
