//! DiaCraft metafiles: Enhanced Metafiles (EMF), EMF+ and Windows Metafiles (WMF) read to a display list of filled and stroked paths,
//! bitmaps and text runs in frame coordinates, so that the renderer needs no knowledge of the formats.
//!
//! * **Which reader** is chosen from the bytes, never from a name or a label: [`sniff`].
//! * **Which half of an EMF+ file** is played: the EMF+ records, with the EMF records of GetDC sections; the file's EMF records
//!   draw it instead only when the EMF+ half cannot be played. The two are never mixed in one picture.
//! * **Coordinates** go from logical units through the world transform, the window and viewport, and the header's device and
//!   millimetres to device pixels, and the picture's frame (`Frame` over the device) is the rectangle the renderer stretches onto its box
//!   A WMF's frame is the window its records set.
//! * **Hostile input**: every length and count is checked against the bytes before it is used, no allocation is
//!   sized from a count before that check, the caps are the named constants of [`limits`], a malformed record ends the walk with what was
//!   drawn kept, and nothing in a file is passed to anything outside this crate. The work of a picture is charged to one budget that
//!   nested metafiles share.
//! * **What was left out** is in [`Picture::report`], by the names, and never dropped silently.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bytes;
mod canvas;
mod clip;
mod dib;
mod emf;
mod gdi;
mod geom;
pub mod limits;
mod model;
mod nest;
mod plus;
mod wmf;
pub mod work;
mod xf;

use std::fmt;

use bytes::Bytes;
use canvas::Canvas;
use limits::Budget;
pub use model::{
    BitmapItem, Cap, Clip, ClipPart, Detail, Drawn, Fill, Font, HAlign, Image, Item, Join, Kind, LinearPaint, Paint, Path, PathItem, Picture,
    Placement, Pt, RadialPaint, Report, Seg, Stroke, TextItem, VAlign,
};

/// What a metafile starts as, by its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// A type-1 record with the signature at byte 40.
    Emf,
    /// A standard WMF (`01 00 09 00` or `02 00 09 00`), or a placeable one (`D7 CD C6 9A`).
    Wmf,
}

/// Why no picture came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a metafile.
    NotMetafile,
    /// The error class `metafile-undecodable`: a header that is cut short or that no reader takes. The words say why.
    Undecodable(&'static str),
}

impl Error {
    /// The error class of R42.
    pub fn class(&self) -> &'static str {
        match self {
            Error::NotMetafile => "not-a-metafile",
            Error::Undecodable(_) => "metafile-undecodable",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotMetafile => write!(f, "not a metafile"),
            Error::Undecodable(why) => write!(f, "a metafile that cannot be read: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// The frame of a picture in device pixels, and whether the file gave one.
pub(crate) struct Frame {
    pub width: f64,
    pub height: f64,
    /// False when the header had no usable frame and the picture's own extent is to stand in for it.
    pub known: bool,
}

/// What the bytes are: an EMF, a WMF, or neither. A placeable WMF is a WMF.
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if emf::is_emf(bytes) {
        Some(Format::Emf)
    } else if wmf::is_wmf(bytes) {
        Some(Format::Wmf)
    } else {
        None
    }
}

/// Reads a metafile to a picture.
///
/// The format is found from the bytes. A header that fails gives an error and no picture; everything after the header
/// gives a picture, with what was left out in its report.
pub fn parse(bytes: &[u8]) -> Result<Picture, Error> {
    let mut budget = Budget::new();
    let mut pic = parse_with(bytes, &mut budget, 0)?;
    if let Some(what) = budget.reached {
        pic.report.note(Detail::CapReached, || format!("a cap was reached: {what}"));
    }
    Ok(pic)
}

/// As [`parse`], inside another metafile: the budget is shared and `depth` counts the metafiles around this one.
pub(crate) fn parse_with(bytes: &[u8], budget: &mut Budget, depth: u32) -> Result<Picture, Error> {
    match sniff(bytes) {
        Some(Format::Emf) => read_emf(bytes, budget, depth),
        Some(Format::Wmf) => wmf::read(Bytes(bytes), budget, depth),
        None => Err(Error::NotMetafile),
    }
}

fn read_emf(bytes: &[u8], budget: &mut Budget, depth: u32) -> Result<Picture, Error> {
    let b = Bytes(bytes);
    let header = emf::read_header(b)?;
    let setup = emf::setup(&header);
    let mut cv = Canvas::new(budget);
    let (walked, plus) = emf::read(b, &header, &mut cv, depth);
    if walked.cut_short {
        cv.note(Detail::CutShort, || "a record whose size is wrong".to_owned());
    }
    let (items, clips, report) = cv.finish();
    let kind = if plus { Kind::EmfPlus } else { Kind::Emf };
    Ok(assemble(kind, setup.frame, items, clips, report))
}

/// The picture, with the frame the file gave or, when it gave none, the extent of what was drawn (the items then move to put its corner at
/// the origin).
pub(crate) fn assemble(kind: Kind, frame: Frame, items: Vec<Item>, clips: Vec<model::Clip>, report: Report) -> Picture {
    let mut pic = Picture { kind, width: frame.width, height: frame.height, items, clips, report };
    if !frame.known {
        nest::fit_to_extent(&mut pic);
    }
    if !(pic.width.is_finite() && pic.height.is_finite() && pic.width > 0.0 && pic.height > 0.0) {
        pic.width = 1.0;
        pic.height = 1.0;
    }
    pic
}
