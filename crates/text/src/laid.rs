//! What a layout gives back: lines of positioned glyphs, and what a renderer needs to draw them.
//!
//! **The frame.** Every number is in the block's own frame: paper inches, the origin at the
//! block's lower left, `x` along its width and `y` up its height. That is the frame maps to the shape's, so a renderer
//! applies that map to these points as they are and never flips text for a flipped shape. Glyph outlines are in font
//! units with `y` up too, so a glyph is drawn by scaling its outline by [`GlyphRun::size`] over the face's units per em
//! (`x` also by [`GlyphRun::x_scale`]) and moving its origin to the glyph's point, with no flip of its own.
//!
//! **What a renderer does for a made-up weight or slant**: a run with `embolden` above 0 strokes each glyph's outline with
//! that width, in inches, with round joins, as well as filling it; a run with `slant` above 0 shears each glyph about its origin
//! so a point `y` above the baseline moves right by `slant * y`.

use diacraft_fonts::Face;
use kurbo::{Rect, Size};

use crate::model::Rgba;

/// A glyph with its pen position.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub gid: u32,
    /// Where the glyph's origin sits, with the shaper's offset and the run's raised or lowered baseline already in.
    pub x: f64,
    pub y: f64,
    /// How far the pen moved for it.
    pub advance: f64,
    /// The characters of the source text (counted as the `Run`s' characters in order, line feeds included) it draws. Glyphs of
    /// one cluster share a range.
    pub chars: std::ops::Range<usize>,
}

/// Glyphs in one face, size and colour, in visual order, left to right.
#[derive(Clone, Debug)]
pub struct GlyphRun {
    pub face: Face,
    /// The em in inches: `Size` after raised or lowered letters, small capitals and complex script size.
    pub size: f64,
    /// `FontScale`: glyphs are this much wider than the face makes them, at the same height.
    pub x_scale: f64,
    /// The width of the stroke that makes a face with no bold bolder, in inches; 0 for none.
    pub embolden: f64,
    /// The shear that makes a face with no italic lean, as a slope; 0 for none.
    pub slant: f64,
    /// Red, green and blue; the alpha is `opacity`.
    pub color: Rgba,
    /// 1 minus the run's transparency. A run at 0 is not painted but still has its glyphs and takes its room.
    pub opacity: f64,
    /// Are the glyphs turned a quarter clockwise about their own origins? Only in vertical text (`TextDirection` 1): a letter
    /// that is not East Asian lies on its side there, its top to the right and its advance running down the block. A renderer applies
    /// the turn to the glyph's outline (after the slant, before the move to the glyph's point) and writes no text for such a run.
    pub turned: bool,
    pub glyphs: Vec<PlacedGlyph>,
}

/// What a line through or around text is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    Underline,
    DoubleUnderline,
    Overline,
    Strikethrough,
    DoubleStrikethrough,
}

/// One drawn line: a filled rectangle from `x0` to `x1` whose top edge is at `top` and which is `thickness` tall. A double line is
/// two of these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    pub kind: DecorationKind,
    pub x0: f64,
    pub x1: f64,
    pub top: f64,
    pub thickness: f64,
    pub color: Rgba,
    pub opacity: f64,
    /// A line down a column of vertical text: `x0` and `x1` are then its lower and upper `y`, and `top` is its right edge, so it lies
    /// `thickness` to the left of `top`. [`Decoration::rect`] gives the rectangle either way.
    pub vertical: bool,
}

impl Decoration {
    /// The rectangle the line fills, in the block's frame.
    pub fn rect(&self) -> Rect {
        if self.vertical {
            Rect::new(self.top - self.thickness, self.x0, self.top, self.x1)
        } else {
            Rect::new(self.x0, self.top - self.thickness, self.x1, self.top)
        }
    }
}

/// One line of text.
#[derive(Clone, Debug)]
pub struct Line {
    /// Which paragraph the line belongs to (0 for the first).
    pub paragraph: usize,
    /// The source characters the line holds, spaces at its end included, and the break after it not.
    pub chars: std::ops::Range<usize>,
    /// Where the baseline is.
    pub baseline: f64,
    /// The line box: `height` tall, with the baseline three quarters down it.
    pub top: f64,
    pub bottom: f64,
    pub height: f64,
    /// Where the line's text starts, and how wide it is without its trailing spaces.
    pub x: f64,
    pub width: f64,
    /// Is the line's paragraph right to left?
    pub rtl: bool,
    /// Is this a column of vertical text (`TextDirection` 1)? Then `baseline` is the `x` of its baseline, `top` and `bottom` its right and
    /// left edges, `height` its width, `x` how far down from the block's top edge its text starts, and `width` how far down it runs.
    pub vertical: bool,
    pub runs: Vec<GlyphRun>,
    pub decorations: Vec<Decoration>,
    /// The bullet, on a paragraph's first line.
    pub bullet: Option<GlyphRun>,
}

/// The one rectangle a text background paints: it hugs the composed text and takes no margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Background {
    pub rect: Rect,
    pub color: Rgba,
    /// 1 minus `TextBkgndTrans`.
    pub opacity: f64,
}

/// Something the layout did that the caller may want to say: nothing here is an error, and the text is always laid out.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// A face the drawing named isn't on the machine and a stand-in drew it. Once per name, for the first 256 names. The
    /// caller keeps the cell and the `FaceName` as they are and warns once per document.
    StandIn { requested: String, used: String },
    /// More names than that were missing and got a stand-in: `count` names have no `StandIn` note of their own.
    StandInsOmitted { count: usize },
    /// There were more characters than the layout takes: `chars` source characters past the limit are not laid out.
    Truncated { chars: usize },
    /// There were more runs than the layout reads: `runs` runs past the limit are not read, and neither is their text.
    RunsCut { runs: usize },
    /// The block reached the most lines it makes: `chars` characters (counted after `Case`) of the paragraphs from there on are not
    /// laid out.
    LinesCut { chars: usize },
    /// A stack of combining marks past the limit on one letter: the marks beyond it are not drawn.
    MarksCut,
    /// More shaping than the budget allows: the rest of the text is measured from the cmap, without shaping.
    ShapingBudgetUsed,
    /// A search for a face that has some letters could not be finished (more searches than the budget allows, or the database has read
    /// all it may): those letters are drawn as missing-glyph boxes, though a face might have them.
    FaceSearchCut,
    /// `clusters` letters (a letter and its marks make one) had no glyph in any face the layout could use, so the face's missing-glyph
    /// box was drawn for them. That box is blank in some faces, so text can vanish: the renderer should say so.
    MissingGlyphs { clusters: usize },
    /// A face's tables could not be used (they made the reader fail, once, and the face was set aside): its text was set from the
    /// cmap alone, one glyph a character, with no joining, no placed marks and no reordering. Once for each face.
    FaceFailed { family: String },
    /// A bullet's string was longer than a bullet takes and was cut.
    BulletStringCut,
    /// A value that was not a finite number, or was absurdly large, was read as its default. This includes a block width that was
    /// given and was not a number.
    BadNumber,
}

/// A laid-out block of text.
#[derive(Clone, Debug)]
pub struct Laid {
    pub lines: Vec<Line>,
    /// The widest line without its trailing spaces, and the stack's height `T`: line boxes and the space between paragraphs, with
    /// none above the first or below the last. Empty text still takes one line. Margins are not in it.
    pub size: Size,
    /// Does the stack run past the block's margins, up or down, or does a line run wider than its measure? Nothing is clipped
    /// either way.
    pub overflow: bool,
    pub background: Option<Background>,
    pub notes: Vec<Note>,
}
