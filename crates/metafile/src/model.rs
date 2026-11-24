//! What a metafile reads to: a display list in **frame coordinates**, which are device pixels with the origin at the frame's top-left
//! corner, x right and y down. `render` stretches the frame's rectangle onto the picture box and needs no knowledge of EMF.
//!
//! Paths are stored in 32-bit floats: a picture can hold a million items and the list is kept between frames. Every number in a
//! [`Picture`] is finite.

use std::sync::Arc;

/// A point in frame coordinates.
pub type Pt = [f32; 2];

/// One step of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Pt),
    Line(Pt),
    Cubic(Pt, Pt, Pt),
    Close,
}

/// A path: figures made of lines and cubic curves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub segs: Vec<Seg>,
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }
}

/// Why a picture was not drawn in full, in the order the report names the first of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detail {
    /// The EMF+ half could not be played, and the EMF records drew the picture.
    GdiFallback,
    /// An EMF+ header where an EMF+ file has none; the file was played as plain EMF.
    EmfPlusMisplaced,
    /// A record the crate does not draw.
    RecordSkipped,
    /// Text of glyph indices, not drawn.
    GlyphIndexText,
    /// A symbol font that cannot be found, so the text is not drawn. Set by `render`, which holds the fonts.
    FontMissing,
    /// A bitmap that cannot be decoded.
    BitmapUndecodable,
    /// A record that cut the walk short.
    CutShort,
    /// A cap was reached.
    CapReached,
}

impl Detail {
    /// The name gives it.
    pub fn name(self) -> &'static str {
        match self {
            Detail::GdiFallback => "gdi-fallback",
            Detail::EmfPlusMisplaced => "emfplus-misplaced",
            Detail::RecordSkipped => "record-skipped",
            Detail::GlyphIndexText => "glyph-index-text",
            Detail::FontMissing => "font-missing",
            Detail::BitmapUndecodable => "bitmap-undecodable",
            Detail::CutShort => "cut-short",
            Detail::CapReached => "cap-reached",
        }
    }
}

/// What was not drawn in full: for each [`Detail`] how many times, and the first in words. The first of them (the lowest) is the one
/// names; the others are kept for the words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    entries: Vec<(Detail, usize, String)>,
}

impl Report {
    /// Notes one occurrence. `first` is built only the first time.
    pub fn note(&mut self, detail: Detail, first: impl FnOnce() -> String) {
        self.note_n(detail, 1, first);
    }

    pub fn note_n(&mut self, detail: Detail, n: usize, first: impl FnOnce() -> String) {
        if n == 0 {
            return;
        }
        match self.entries.iter_mut().find(|(d, _, _)| *d == detail) {
            Some((_, count, _)) => *count = count.saturating_add(n),
            None => {
                self.entries.push((detail, n, first()));
                self.entries.sort_by_key(|(d, _, _)| *d);
            }
        }
    }

    /// The detail names: the first of the list that occurred.
    pub fn first(&self) -> Option<Detail> {
        self.entries.first().map(|(d, _, _)| *d)
    }

    /// Every detail that occurred, with its count and the first in words, in the report's order.
    pub fn entries(&self) -> &[(Detail, usize, String)] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn count(&self, detail: Detail) -> usize {
        self.entries.iter().find(|(d, _, _)| *d == detail).map_or(0, |(_, n, _)| *n)
    }

    /// Takes in another report's entries (a nested metafile's).
    pub(crate) fn absorb(&mut self, other: Report) {
        for (detail, n, first) in other.entries {
            self.note_n(detail, n, || first);
        }
    }
}

/// How a line ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

/// How two lines join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

/// What paints a fill or a stroke. Colours are straight (not premultiplied) red, green, blue and alpha.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid([u8; 4]),
    /// Colours along the line from `start` to `end`; beyond the ends the end colours hold.
    Linear(Arc<LinearPaint>),
    /// Colours outward from `center`: the unit circle of `basis` (columns: the images of (1, 0) and (0, 1)) is the ellipse where the
    /// last stop is reached.
    Radial(Arc<RadialPaint>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearPaint {
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// Offsets in 0 to 1, rising, with colours. Shared by every item that is filled by the same brush.
    pub stops: Arc<[(f64, [u8; 4])]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialPaint {
    pub center: [f64; 2],
    pub basis: [f64; 4],
    pub stops: Arc<[(f64, [u8; 4])]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub paint: Paint,
    /// Odd-crossings rule (GDI's *alternate*) instead of non-zero (*winding*).
    pub even_odd: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub paint: Paint,
    /// In frame units. A cosmetic pen is 1: one device pixel.
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    /// On, off, on, off... in frame units; empty is solid.
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
}

/// A path to fill, stroke or both (the fill first).
#[derive(Clone, Debug, PartialEq)]
pub struct PathItem {
    pub path: Path,
    pub fill: Option<Fill>,
    pub stroke: Option<Stroke>,
}

/// The pixels of a bitmap.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    /// Straight RGBA, `width * height * 4` bytes, rows from the top.
    Rgba { width: u32, height: u32, data: Arc<Vec<u8>> },
    /// A whole image file (PNG, JPEG, GIF, BMP or TIFF) for the caller to decode: its format is known from its first bytes.
    Encoded(Arc<Vec<u8>>),
}

/// A bitmap drawn into a parallelogram.
#[derive(Clone, Debug, PartialEq)]
pub struct BitmapItem {
    pub image: Image,
    /// The part of the image drawn, `[x, y, width, height]` in its pixels, y down; `None` is all of it.
    pub src: Option<[f64; 4]>,
    /// Where the source rectangle's upper-left, upper-right and lower-left corners go. The fourth corner follows.
    pub quad: [[f64; 2]; 3],
}

/// Where the characters of a text run stand.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// The origin of each character (on its baseline, or by `TextItem::v_align` below or above it), one for each of `text`'s characters.
    Origins(Vec<[f64; 2]>),
    /// Letters one after another from `origin`, advanced by the font's own widths, and placed by `h_align`: the reference point is at the
    /// start (`Left`), the middle (`Center`) or the end (`Right`) of the text.
    Flow { origin: [f64; 2], h_align: HAlign },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

/// Where the reference point of a text run sits against its letters, up and down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    /// The top of the line: the baseline is an ascent below it.
    Top,
    Baseline,
    /// The bottom of the line: the baseline is a descent above it.
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    /// The family the file names, as written. Shared by the text runs of one font.
    pub family: Arc<str>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    /// A symbol font (character set 2): without it the text is not drawn.
    pub symbol: bool,
}

/// A run of text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub text: String,
    pub font: Font,
    pub color: [u8; 4],
    /// The size in logical units: an em, or when `cell` the height of a line (ascent plus descent) that the face's own metrics turn to
    /// an em.
    pub size: f64,
    pub cell: bool,
    /// The frame vectors one logical unit of size runs along the baseline (`basis[0..2]`) and upward from it (`basis[2..4]`).
    pub basis: [f64; 4],
    pub placement: Placement,
    pub v_align: VAlign,
    /// The colour behind the letters, when the background mode is opaque.
    pub background: Option<[u8; 4]>,
}

/// What an item draws.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    Path(Box<PathItem>),
    Bitmap(Box<BitmapItem>),
    Text(Box<TextItem>),
}

/// An item and the clip it is cut by: an index into [`Picture::clips`], where 0 is no clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub clip: u32,
    pub drawn: Drawn,
}

/// One part of a clip.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPart {
    pub path: Path,
    /// Odd-crossings rule. The clip of a drawing is the part of the plane inside every one of its parts. A part with an empty path
    /// clips everything away.
    pub even_odd: bool,
}

/// A clip: the intersection of its parts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub parts: Vec<ClipPart>,
}

/// Which kind of metafile was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Emf,
    /// The EMF+ records drew it.
    EmfPlus,
    Wmf,
}

/// A parsed metafile.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub kind: Kind,
    /// The frame's size in frame units (device pixels), both positive.
    pub width: f64,
    pub height: f64,
    pub items: Vec<Item>,
    /// `clips[0]` is the empty clip. Items name the others by index.
    pub clips: Vec<Clip>,
    pub report: Report,
}

impl Picture {
    /// About how many bytes the picture holds, for a cache that bounds its memory.
    pub fn cost(&self) -> u64 {
        let mut bytes = (self.items.len() as u64).saturating_mul(64);
        let seg = std::mem::size_of::<Seg>() as u64;
        for item in &self.items {
            bytes = bytes.saturating_add(match &item.drawn {
                Drawn::Path(p) => (p.path.segs.len() as u64).saturating_mul(seg).saturating_add(128),
                Drawn::Bitmap(b) => match &b.image {
                    Image::Rgba { data, .. } => data.len() as u64,
                    Image::Encoded(data) => data.len() as u64,
                },
                Drawn::Text(t) => (t.text.len() as u64).saturating_mul(24).saturating_add(256),
            });
        }
        for clip in &self.clips {
            for part in &clip.parts {
                bytes = bytes.saturating_add((part.path.segs.len() as u64).saturating_mul(seg).saturating_add(32));
            }
        }
        bytes
    }
}
