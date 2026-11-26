//! Labels, plain and HTML, read into paragraphs of runs (S-R24 to S-R27).
//!
//! **DiaCraft never renders HTML, never hands a label to a browser engine, a web view or a script** (S-R25). A label with `html=1` is read by [`read_html`] with this crate's own
//! bounded tokenizer into the text the model holds: plain characters, [`Run`]s whose [`RunFmt`] differs, and [`Para`]graphs; what the table of S-R25 does not name is dropped and
//! keeps only its text, the elements that could reach anything active (`script`, `iframe`, `svg`, `form` and the rest) go with what is inside them, and an attribute is read only
//! where the table says so. A label with no `html=1` is plain text, and [`read_plain`] reads it as it stands (S-R26).
//!
//! * **The tokenizer** ([`lexer`]) is adapted from WordCraft's (`docs/provenance.md`): it cuts the text into tags, ends of tags and text, takes a tag's attributes as slices of the
//!   label, and never allocates for a character. A tag with no `>` before the end is dropped, and so is everything a raw-text element (`script`, `style`) holds.
//! * **The references** ([`entity`]) are the six names the table of S-R25 reads (`&amp;` `&lt;` `&gt;` `&quot;` `&apos;` `&nbsp;`) and numeric references; any other name stays as
//!   its literal text. A character XML cannot hold is removed (R11 step 4, design §4), and a reference to a number no character has is U+FFFD.
//! * **The walk** ([`label`]) keeps a stack of at most [`MAX_LABEL_DEPTH`](crate::limits::MAX_LABEL_DEPTH) open elements, each with the format it gives its text, collapses white
//!   space as HTML does, and merges the runs whose formats are equal. A line break (`br`) is U+2028 inside the paragraph and a block (`div`, `p`, `li` and the rest) ends it.
//! * **The style attribute and the colours** ([`css`]) are read for the seven properties of S-R25 and no other.
//!
//! **The budget (design §7).** A label is cut at [`MAX_LABEL_BYTES`](crate::limits::MAX_LABEL_BYTES) with `drawio_label_too_long`. Every pass charges the read's work, as
//! [`Unit::LabelSteps`](crate::report::Unit::LabelSteps), **before it runs**: the label's bytes before the tokenizer walks them, each token as it comes (an element is closed
//! once however many tags close it, and the walk keeps a count of what is open, so that an end tag that closes nothing costs its token and no more), each piece of text and each
//! attribute it reads for the bytes it decodes and for the 33 bytes a stray `&` may look ahead, a style attribute for the bytes it parses, and each paragraph and each run it builds
//! for [`PARA_STEPS`] and [`RUN_STEPS`] when the token that built it is done. The refusal of a charge is `drawio_too_complex` and ends the open. Nothing here recurses and
//! nothing indexes. A plain label is cut at the cap too, and charged for its bytes and what it builds.

mod css;
pub mod entity;
mod label;
mod lexer;

pub use css::{html_font_px, legacy_font_size};
pub use label::{read_html, read_plain};

use std::sync::Arc;

/// A line break inside a paragraph: what `br` and, in a plain label, a line feed become. The model reads U+2028 as a break of the line that does not end the paragraph
/// And a paragraph ends where the text has a line feed.
pub const LINE_BREAK: char = '\u{2028}';

/// The steps a paragraph that is built costs, on top of the bytes it was made of: 16. A paragraph is a `Para` pushed, and the model will write a row for it; the number is what the
/// timed twins of `tests/it/label_hostile.rs` measure it at, so that a label of a million paragraphs is counted as dear as it is.
pub const PARA_STEPS: u64 = 16;

/// The steps a run that is built costs, on top of the bytes it was made of: 8. A run is a new `Run` with a format cloned into it, and the model will write a row for it.
pub const RUN_STEPS: u64 = 8;

/// The class words of the warnings of this reader (M-WARN).
pub mod warn {
    /// An element that holds something DiaCraft does not show (`img`, `svg`, `math`, `iframe`, a form, a picture or a player), dropped with what is inside it. One for each
    /// element that is dropped, and none for the ones inside it.
    pub const LABEL_CONTENT_DROPPED: &str = "drawio_label_content_dropped";
    /// A label of more than the cap of a label, cut (S-R25).
    pub const LABEL_TOO_LONG: &str = "drawio_label_too_long";
    /// A label of a drawing whose `math` is on, with text between `$$` and `$$` or between two backquotes, shown as it is (S-R27).
    pub const MATH_SHOWN_AS_TEXT: &str = "drawio_math_shown_as_text";
}

/// The horizontal alignment a block says for its paragraph: `text-align` of its `style` (S-R25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// `left` (and `start`).
    Left,
    /// `center`.
    Center,
    /// `right` (and `end`).
    Right,
}

/// Raised or lowered text: `sub` and `sup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Script {
    /// Neither.
    #[default]
    Normal,
    /// `sub`.
    Sub,
    /// `sup`.
    Super,
}

/// A colour a label gives a run: red, green and blue, 0 to 255 each (what is transparent in a colour is not read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// The colour with these three parts.
    pub fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }
}

/// What a run says of its text beyond the cell's style (S-R25). **A field that is `None` says nothing**, so that the run keeps what the cell's own style gives it (the first
/// `Character` row of the shape); a flag that is `Some(false)` says the run is not bold (or italic, or underlined, or struck) although the cell's style is, which is
/// what `font-weight: normal` says inside a bold cell. A size is in px, and a px is 0.01 inch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunFmt {
    /// `b`, `strong`, a heading, `font-weight` (bold from 600).
    pub bold: Option<bool>,
    /// `i`, `em`, `font-style: italic`.
    pub italic: Option<bool>,
    /// `u`, `ins`, `text-decoration: underline`.
    pub underline: Option<bool>,
    /// `s`, `strike`, `del`, `text-decoration: line-through`.
    pub strike: Option<bool>,
    /// `sub` and `sup`.
    pub script: Script,
    /// The first family `font face` or `font-family` names, quotes removed.
    pub family: Option<Arc<str>>,
    /// `font size` (1 to 7, as HTML sizes them) or `font-size` in px, pt or em of the label's size, in px; always finite and above 0.
    pub size_px: Option<f64>,
    /// `font color` or `color`.
    pub color: Option<Rgb>,
}

/// A piece of a paragraph whose text has one format. Its text is never empty, and the run next to it has another format.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The text, with a [`LINE_BREAK`] where a line breaks and a tab between the cells of a table row.
    pub text: String,
    /// Its format.
    pub fmt: RunFmt,
}

/// A paragraph: runs of text, and the alignment its block says. A paragraph with no run is an empty line (`<div><br></div>`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Para {
    /// `text-align` of the block it is in, or of the nearest block around it that says one; `None` leaves the cell's own `align`.
    pub align: Option<Align>,
    /// Its runs, in order.
    pub runs: Vec<Run>,
}

/// The first link of a label, when it is one DiaCraft keeps (S-R25): the scheme is `http:`, `https:`, `mailto:` or a page link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// An `http:`, `https:` or `mailto:` address, as the `href` has it (white space at its ends and inside removed, as a browser does).
    Url(String),
    /// A link to a page of the file, `data:page/id,` and the page's `id`.
    Page(String),
}

/// A label read: its paragraphs and its first link.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Label {
    /// The paragraphs, in order. A label with no text has none.
    pub paras: Vec<Para>,
    /// The first `a` of the label that has an `href`, when its address is kept; the others are not read (gives it to the shape when the wrapper has no link).
    pub link: Option<Link>,
}

impl Label {
    /// The text as the model holds it: the paragraphs' runs joined, and a line feed between paragraphs.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (n, para) in self.paras.iter().enumerate() {
            if n > 0 {
                out.push('\n');
            }
            for run in &para.runs {
                out.push_str(&run.text);
            }
        }
        out
    }

    /// Whether the label holds no text at all: no paragraph, or only empty ones.
    pub fn is_empty(&self) -> bool {
        self.paras.iter().all(|p| p.runs.is_empty())
    }
}

/// What a label is read against: the cell it belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// The cell's `fontSize` in px: an `em` in a label's `style` is a multiple of it (S-R25). A value that is no size is 12, the shape's default (S-R7).
    pub font_px: f64,
    /// The model's `math="1"` (S-R27): a label with text between `$$` and `$$` is counted in `drawio_math_shown_as_text`.
    pub math: bool,
    /// The cell's id as the file wrote it, to name the cell in a case of a warning; empty for none.
    pub cell: &'a str,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { font_px: 12.0, math: false, cell: "" }
    }
}
