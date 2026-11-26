//! Why a file was not read: [`DrawioError`], its [`ErrorKind`] and the class words of the refusals.
//!
//! A refusal ends the open with a message in words, and the drawing that was open stays as it was. **The words hold the class, numbers and escaped names, and never file
//! content beyond a page's name cut to 64 characters** (design §3): what a refusal says, a log or a dialog can show whatever the file held. They do not name the format
//! (the owner has not yet allowed its name in what a person reads, design Q1).

use std::fmt;

/// The class words of the refusals. A harness compares these, and not the wording.
pub mod class {
    /// The bytes are not a drawing: another root, an image with no drawing, a ZIP, anything else. The engine then tries the next reader.
    pub const NOT_A_DRAWING: &str = "drawio_not_a_drawing";
    /// The root is `mxlibrary`: a library of shapes, not a drawing.
    pub const LIBRARY_NOT_DRAWING: &str = "drawio_library_not_drawing";
    /// A declared encoding other than UTF-8, UTF-16 and US-ASCII.
    pub const ENCODING_UNSUPPORTED: &str = "drawio_encoding_unsupported";
    /// A file, a page, all the pages together or a value larger than its cap.
    pub const TOO_LARGE: &str = "drawio_too_large";
    /// A page that inflates to more than its ratio of its compressed size.
    pub const RATIO_EXCEEDED: &str = "drawio_ratio_exceeded";
    /// XML nested deeper than the cap.
    pub const TOO_DEEP: &str = "drawio_too_deep";
    /// More pages, cells, items or work than the caps (design §7).
    pub const TOO_COMPLEX: &str = "drawio_too_complex";
    /// A document type or an entity declaration.
    pub const ENTITY_REFUSED: &str = "drawio_entity_refused";
    /// XML that does not parse.
    pub const XML_MALFORMED: &str = "xml_malformed";
    /// Memory ran out while a page was decoded: reported, not an abort.
    pub const OUT_OF_MEMORY: &str = "out_of_memory";
}

/// What kind of refusal it was, for the caller that decides what to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The bytes are not a drawing of this reader's kind: the engine tries the next reader, so that a file is never refused as a drawing because it was not one.
    NotMine,
    /// The file is one of this reader's kind and it will not read it: a library, an encoding, a document type.
    Refused,
    /// The file passes a cap: its size, its depth, its ratio, its work.
    TooBig,
    /// The file is damaged: XML that does not parse.
    Damaged,
}

/// Why a file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawioError {
    /// What kind of refusal.
    pub kind: ErrorKind,
    /// The class word ([`class`]).
    pub class: &'static str,
    /// What to tell the person, in words.
    pub words: String,
}

impl DrawioError {
    pub(crate) fn new(kind: ErrorKind, class: &'static str, words: impl Into<String>) -> DrawioError {
        DrawioError { kind, class, words: words.into() }
    }

    /// The bytes are not a drawing.
    pub(crate) fn not_a_drawing() -> DrawioError {
        DrawioError::new(ErrorKind::NotMine, class::NOT_A_DRAWING, "This file is not a diagram file that DiaCraft reads.")
    }

    /// A file, a page or a value over a cap, with the cap said in `what`.
    pub(crate) fn too_large(what: impl fmt::Display) -> DrawioError {
        DrawioError::new(ErrorKind::TooBig, class::TOO_LARGE, what.to_string())
    }

    /// Past a cap on pages, cells, items or work.
    pub(crate) fn too_complex(what: impl fmt::Display) -> DrawioError {
        DrawioError::new(ErrorKind::TooBig, class::TOO_COMPLEX, what.to_string())
    }

    /// Memory ran out.
    pub(crate) fn out_of_memory() -> DrawioError {
        DrawioError::new(ErrorKind::TooBig, class::OUT_OF_MEMORY, "There was not enough memory to read this file.")
    }
}

impl fmt::Display for DrawioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for DrawioError {}

/// `text` as it may stand in a sentence or a list of cases: its characters escaped (a control character, a terminal escape and a format character are written out, never
/// sent as themselves), and cut to `max` characters, the last of them an ellipsis when it was cut. Only the first `max` characters are ever looked at, however long the text is.
pub fn shown(text: &str, max: usize) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let (mut used, mut cut) = (0usize, false);
    for c in text.chars() {
        let escaped: String = c.escape_debug().collect();
        let n = escaped.chars().count();
        if used.saturating_add(n) > max {
            cut = true;
            break;
        }
        used += n;
        pieces.push(escaped);
    }
    if cut {
        // Room for the ellipsis: give up the last pieces until it fits.
        while used + 1 > max {
            let Some(last) = pieces.pop() else { break };
            used -= last.chars().count();
        }
    }
    let mut out = pieces.concat();
    if cut {
        out.push('\u{2026}');
    }
    out
}

/// A size in words: whole mebibytes as `N MiB`, anything else in bytes.
pub(crate) fn size_words(bytes: u64) -> String {
    const MIB: u64 = 1 << 20;
    if bytes >= MIB && bytes.is_multiple_of(MIB) { format!("{} MiB", bytes / MIB) } else { format!("{bytes} bytes") }
}
