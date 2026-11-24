//! Where a comment's marker goes and what its card is headed by.
//!
//! A comment names its shape by `ID`, so a move, a resize or a turn of the shape moves its marker with it and nothing is written to the comment: the marker is
//! worked out from the shape's box when it is asked for. Our rule: no source gives the place: the marker of a comment on a shape is at the top right
//! corner of the box round the shape's corners on the page; a comment on the page, or on a shape that is no longer there (a file may hold one), is at the page's top
//! right corner, inset by [`MARKER_INSET`] inches. "a card is headed by the text of the shape it belongs to": a shape's head is the first
//! [`HEAD_CHARS`] characters of its text, its line breaks as blanks.
//!
//! The shapes are found by one walk of the page ([`super::scan`]) whatever their number, and the head takes at most [`HEAD_CHARS`] characters of a text.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Document, SheetRef, TextPart};

use super::read::Reader;
use super::scan::scan;
use super::{Problem, units_per_inch};

/// How far inside the page's top right corner the marker of a page's comment is, in inches.
pub const MARKER_INSET: f64 = 0.3;

/// How many characters of a shape's text head a comment's card.
pub const HEAD_CHARS: usize = 80;

/// What a comment's shape gives: where its marker is, in paper inches (x right, y up from the page's lower left), and the head of its card.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeFacts {
    pub x: f64,
    pub y: f64,
    pub head: String,
}

/// The place of the marker of a comment on the page `page`, in paper inches: inside the page's top right corner.
pub fn page_corner(doc: &Document, page: u32) -> (f64, f64) {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let k = units_per_inch(doc, page);
    let (w, h) = (reader.number(&sheet, "PageWidth") / k, reader.number(&sheet, "PageHeight") / k);
    ((w - MARKER_INSET).max(0.0), (h - MARKER_INSET).max(0.0))
}

/// The first [`HEAD_CHARS`] characters of the text parts, line breaks as blanks and no blank at either end.
fn head_of(parts: &[TextPart]) -> String {
    let mut out = String::new();
    let mut left = HEAD_CHARS;
    for part in parts {
        let chars: &str = match part {
            TextPart::Chars(c) => c,
            TextPart::Fld { text, .. } => text,
            _ => continue,
        };
        for c in chars.chars().take(left) {
            out.push(if c.is_control() { ' ' } else { c });
        }
        left = HEAD_CHARS.saturating_sub(out.chars().count());
        if left == 0 {
            break;
        }
    }
    out.trim().to_owned()
}

/// What each of the shapes `wanted` of the page `page` gives (a shape the page does not hold is left out), by one walk of the page.
///
/// # Errors
/// A page that is not there, or one with more shapes than the model allows.
pub fn shape_facts(doc: &Document, page: u32, wanted: &HashSet<u32>) -> Result<HashMap<u32, ShapeFacts>, Problem> {
    let mut out = HashMap::with_capacity(wanted.len());
    if wanted.is_empty() {
        return Ok(out);
    }
    let found = scan(doc, page, wanted)?;
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    for (id, at) in &found.found {
        let corners = reader.corners(at).map(|p| (p.0 / k, p.1 / k));
        let x = corners.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        let y = corners.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let head = doc.shape(at).and_then(|s| s.text.as_ref()).map(|t| head_of(&t.parts)).unwrap_or_default();
        out.insert(*id, ShapeFacts { x, y, head });
    }
    Ok(out)
}

/// What a save as an `.odg` says of the comments: **an `.odg` holds a page's comments as `office:annotation` elements** (their authors, dates, text, threads, Done marks
/// and the shapes they are on), so most of what a drawing has of them is written. What it has no place for is told: the data a VSDX file kept beside a comment or its author (the
/// author's resolution number, e-mail and SIP addresses, a comment's kind) and the setting that hides the markers. `None` when nothing of that is held.
#[must_use]
pub fn odg_words(doc: &Document) -> Option<String> {
    let comments = &doc.comments;
    // What a VSDX file kept is not prefixed (`AutoCommentType`); what an `.odg` kept is (`svg:x`).
    let of_vsdx = comments.iter().filter(|c| c.raw.iter().any(|(name, _)| !name.contains(':'))).count();
    // An empty `Name` or `Initials` the file had is kept in `raw` so that a VSDX file is written as it was read: it is nothing an `.odg` lacks.
    let authors = comments.authors().filter(|a| a.raw.iter().any(|(name, _)| !matches!(name, "Name" | "Initials"))).count();
    let hidden = comments.show_tags == Some(false);
    if of_vsdx == 0 && authors == 0 && !hidden {
        return None;
    }
    let mut left_out: Vec<String> = Vec::new();
    if of_vsdx > 0 {
        left_out.push(format!("the kind and other attributes of {of_vsdx} comment{}", if of_vsdx == 1 { "" } else { "s" }));
    }
    if authors > 0 {
        left_out.push(format!("the e-mail and other data of {authors} comment author{}", if authors == 1 { "" } else { "s" }));
    }
    if hidden {
        left_out.push("the setting that hides the markers of the comments".to_owned());
    }
    Some(format!(
        "an .odg file holds a comment's author, date, text, replies, Done mark and shape, but not {}: the drawing keeps {}, and a save as .diacraft or .vsdx writes {}",
        left_out.join(", "),
        if left_out.len() == 1 { "it" } else { "them" },
        if left_out.len() == 1 { "it" } else { "them" }
    ))
}
