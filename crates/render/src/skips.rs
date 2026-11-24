//! What a frame left out or drew by a rule that is not the picture's own: counted by kind, and never dropped.

use std::collections::BTreeMap;

/// The reason something was not drawn, or was drawn by a stand-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SkipKind {
    /// The view is larger than the renderer draws ([`crate::limits::MAX_VIEW_SIDE`], [`crate::limits::MAX_VIEW_PIXELS`]) or has no area, or its
    /// transform is not a finite one that can be inverted: nothing was drawn and the result has no pixels.
    ViewRefused,
    /// The page is not in the document.
    NoSuchPage,
    /// The time budget of the frame was spent: this many shapes after the last one drawn were left out.
    FrameBudget,
    /// A newer request came while the frame was drawing ([`Cancel`](crate::Cancel)): this many shapes after the last one drawn were left out,
    /// and the frame's pixels were not made.
    Cancelled,
    /// The count of shapes a frame draws was reached: this many were left out.
    ShapeLimit,
    /// The count of geometry rows a frame builds was reached: this many shapes were left out.
    RowLimit,
    /// The work a frame does was reached ([`Limits::max_work`](crate::Limits::max_work)): the shape whose work would have passed it is drawn as far as it got,
    /// and it and the shapes after it are counted here.
    WorkLimit,
    /// A shape's path or stroke lies past the coordinates the rasteriser can take, and was not drawn.
    OutOfRange,
    /// Geometry that could not be built (a size that is not a number, a section too big): the shape's paths are missing.
    GeometryFailed,
    /// A geometry row was skipped or drawn by a fallback rule (`diacraft_geom::Warning`).
    GeometryRow,
    /// A shape with more `Geometry` sections than the renderer builds: the later ones are not drawn.
    SectionsCut,
    /// An unbounded line (`InfiniteLine`) in a shape: not drawn.
    InfiniteLine,
    /// A `Guide` shape: an editing aid, not drawn.
    Guide,
    /// The time budget for text was spent: this many text blocks were left out.
    TextBudget,
    /// The character budget for text was spent: this many text blocks were left out.
    TextCharBudget,
    /// A text layout noted something (a face that stands in for another, more text than a layout takes, missing glyphs, ...).
    TextNote,
    /// Glyphs with no outline in any face (a box that is blank in some faces): the text may be invisible.
    MissingGlyphs,
    /// A block of text has more glyphs than one block draws: the rest are not drawn.
    GlyphsCut,
    /// Letters an export could not write as text (a face that cannot be embedded, a made-up bold or slant, letters that are not one glyph
    /// each) and wrote as outlines: they look the same and cannot be selected or searched.
    TextAsOutlines,
    /// An `Ink` shape: its fallback picture is drawn, or nothing when it has none.
    InkNoPicture,
    /// An EMF or WMF picture whose header no reader takes: a placeholder is drawn.
    MetafilePlaceholder,
    /// A metafile drawn, but not in full. The specification names one detail for each picture, the first of these that applies, and
    /// the sample says the others. The EMF records drew an EMF+ file whose EMF+ half could not be played.
    MetafileGdiFallback,
    /// An EMF+ header where an EMF+ file has none: the file was played as a plain EMF.
    MetafileEmfPlusMisplaced,
    /// A record that is not drawn.
    MetafileRecordSkipped,
    /// Text of glyph indices, not drawn.
    MetafileGlyphIndexText,
    /// Text in a symbol font that cannot be found, not drawn.
    MetafileFontMissing,
    /// A bitmap that does not decode.
    MetafileBitmapUndecodable,
    /// A record of the wrong size ended the metafile.
    MetafileCutShort,
    /// A cap (the work, the points, the pixels, the nesting) ended some of the metafile, or the frame's time did.
    MetafileCapReached,
    /// A picture whose bytes could not be decoded, or whose size is past the limits: a placeholder is drawn.
    PictureUndecoded,
    /// A foreign shape with no picture to draw.
    NoPicture,
    /// An object the file holds that DiaCraft cannot draw and has no picture of (a table, an embedded object, a 3-D scene, a dimension line, ...): its frame is
    /// drawn as a labelled box. The count is of shapes; the first says which and what it is.
    ObjectPlaceholder,
    /// A fill pattern drawn by approximate artwork (hatches and stipples are our own art).
    PatternApproximate,
    /// A `FillPattern` of 254 or a `LinePattern` of 254 (a master as the pattern): drawn as solid.
    CustomPattern,
    /// A dash pattern so fine, or a path so long, that it would make too many dashes: drawn solid.
    DashesTooMany,
    /// A line weight wider than the renderer strokes: drawn at the widest.
    StrokeClamped,
    /// A line end drawn by DiaCraft's own rule where no source gives its box or size: a number no source measures, in the box gives it,
    /// or a size class outside 0 to 6, drawn as class 2.
    LineEndApproximate,
    /// An open path with a line end asked for and **no direction anywhere** (every segment of no length), where there is nothing to turn the
    /// mark by: the path is drawn without it. The count is of paths.
    LineEndNoDirection,
    /// A line end that is a master (`BeginArrow` 254): not drawn.
    CustomLineEnd,
    /// A compound line (double or triple) is drawn as bands of the weight.
    CompoundApproximate,
    /// A gradient direction drawn by an approximation (rectangular and path gradients).
    GradientApproximate,
    /// A shadow type the spec leaves open (inner shadows and perspective), drawn as a simple one; or a blur drawn as a soft edge.
    ShadowApproximate,
    /// A value that came from the theme and which the theme could not answer: the root style's value stands.
    ThemeUnanswered,
    /// A group nested deeper than the renderer walks: its members are not drawn.
    TooDeep,
    /// A link that leads nowhere (a master or style that is not there): the shape was drawn as if it had no link.
    DanglingLink,
    /// A frame in which the renderer met a bug and recovered: the picture is what was drawn before it.
    Recovered,
    /// A shape whose drawing met a bug: that shape (and, for a group, its members) is missing, and the rest of the page is drawn. The first
    /// sample says which shape and what the bug said.
    ShapeFailed,
}

/// One kind of thing left out or drawn by a stand-in: how many, and the first of them in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub kind: SkipKind,
    /// How many times it happened in the frame.
    pub count: usize,
    /// The first one, in words: a shape's `ID` and what was wrong with it.
    pub first: String,
}

/// The collector of a frame.
#[derive(Debug, Default)]
pub(crate) struct Skips {
    map: BTreeMap<SkipKind, (usize, String)>,
}

impl Skips {
    /// Notes one. `first` is built only for the first of a kind.
    pub(crate) fn note(&mut self, kind: SkipKind, first: impl FnOnce() -> String) {
        self.note_n(kind, 1, first);
    }

    /// Notes `n` of a kind.
    pub(crate) fn note_n(&mut self, kind: SkipKind, n: usize, first: impl FnOnce() -> String) {
        if n == 0 {
            return;
        }
        match self.map.get_mut(&kind) {
            Some((count, _)) => *count = count.saturating_add(n),
            None => {
                self.map.insert(kind, (n, first()));
            }
        }
    }

    /// Counts `n` of a kind and says whether it is the first of that kind, whose sample the caller then sets with [`Skips::set_first`].
    /// Two steps so that the caller builds the sample (which may read cells) with the list not borrowed.
    pub(crate) fn count(&mut self, kind: SkipKind, n: usize) -> bool {
        match self.map.get_mut(&kind) {
            Some((count, _)) => {
                *count = count.saturating_add(n);
                false
            }
            None => {
                self.map.insert(kind, (n, String::new()));
                true
            }
        }
    }

    /// The sample of the first of a kind.
    pub(crate) fn set_first(&mut self, kind: SkipKind, first: String) {
        if let Some((_, sample)) = self.map.get_mut(&kind) {
            *sample = first;
        }
    }

    /// Takes in what `later` noted, which came after what this has: the counts add up and the first of a kind stays the earliest.
    pub(crate) fn absorb(&mut self, later: Skips) {
        for (kind, (n, first)) in later.map {
            match self.map.get_mut(&kind) {
                Some((count, _)) => *count = count.saturating_add(n),
                None => {
                    self.map.insert(kind, (n, first));
                }
            }
        }
    }

    /// Whether anything of `kind` has been noted.
    pub(crate) fn has(&self, kind: SkipKind) -> bool {
        self.map.contains_key(&kind)
    }

    pub(crate) fn into_list(self) -> Vec<Skipped> {
        self.map.into_iter().map(|(kind, (count, first))| Skipped { kind, count, first }).collect()
    }
}
