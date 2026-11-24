//! Every cap that bounds a frame, in one place, with the reason for each.
//!
//! These are project rules, not the reference app's: no source bounds what a window may ask a renderer to draw. A drawing and a view are as hostile as
//! the file they came from, so each cap holds the cost of a frame whatever the page holds, and a frame that reaches one says so in
//! [`Rendered::skipped`](crate::Rendered::skipped) and never drops the fact silently.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The longest side of a view, in pixels. `vello_cpu` takes a `u16` for each side, so 65,535 is its own limit; a window is far inside
/// this one.
pub const MAX_VIEW_SIDE: u32 = 16_384;

/// The most pixels in a view: 64 million, which is 256 MiB of premultiplied RGBA. A view past it (or past [`MAX_VIEW_SIDE`] on a side) draws
/// nothing, and the empty result says why.
pub const MAX_VIEW_PIXELS: u64 = 64 * 1024 * 1024;

/// The most shapes one frame draws, nested ones counted. `diacraft_doc::MAX_SHAPES_PER_PAGE` lets a page hold a million; a frame that
/// walks that many cannot be panned, and the time budget would stop it first. This cap stops a page whose shapes are each too cheap to
/// reach the time budget but too many to hold the walk's memory.
pub const MAX_SHAPES_PER_FRAME: usize = 400_000;

/// The most background pages one page draws behind itself. A chain of backgrounds is as long as the document has pages (1,000 at most);
/// a drawing has one or two.
pub const MAX_BACKGROUNDS: usize = 8;

/// The most `Geometry` sections of one shape that are built. The most in one sheet in the sample files are 726 (`doc::MAX_SECTIONS_PER_SHEET`
/// allows 8,192), and a shape with more than this many paths is a hostile one.
pub const MAX_GEOMETRY_SECTIONS: usize = 1_024;

/// The most rows of geometry a frame builds in all. A row costs a few cell reads; the real file with the most holds 31,273 in one
/// section, and a page of a few thousand ordinary shapes holds some tens of thousands.
pub const MAX_ROWS_PER_FRAME: usize = 4_000_000;

/// The farthest a coordinate may be from the view's origin, in pixels, after the view's transform. Past it a path is not handed to the
/// rasteriser, whose coordinates are 32-bit and which would lose all precision, or never end.
pub const COORD_LIMIT_PX: f64 = 1.0e7;

/// How far past the view, in pixels and beyond the stroke's own width, a path that is too far out is cut (`clip.rs`).
pub const CLIP_MARGIN_PX: f64 = 1_000.0;

/// The widest a stroke is drawn, in pixels. A line weight past it covers any view whatever the path, and the rasteriser's stroker makes
/// geometry as large as the weight.
pub const MAX_STROKE_PX: f64 = 32_768.0;

/// The most dashes a dashed stroke makes. A fine pattern on a long path (a weight of a millionth of an inch along a hundred inches)
/// would make a hundred million; past this many the path is drawn solid, and the report says so.
pub const MAX_DASHES: f64 = 20_000.0;

/// A dash pattern whose repeat is shorter than this many pixels is drawn solid: it cannot be seen as a pattern, and it costs a dash
/// for every pixel it crosses.
pub const MIN_DASH_PERIOD_PX: f64 = 0.75;

/// The most gradient stops one gradient uses.
pub const MAX_GRADIENT_STOPS: usize = 10;

/// The most nested bands a rectangular gradient is drawn in. The rasteriser has no rectangular gradient, so it is stepped.
pub const RECT_GRADIENT_BANDS: usize = 24;

/// The most glyphs one text block draws. The layout caps characters (`diacraft_text::limits::MAX_CHARS`, 262,144); drawing each of those
/// as a path is a cost the frame budget has to count.
pub const MAX_GLYPHS_PER_BLOCK: usize = 20_000;

/// The most glyph outlines the renderer keeps between frames. A face's outline is parsed on each call (the T7 review), so a glyph is
/// parsed once and kept. When the table is full it is emptied, which costs a re-parse and nothing else.
pub const MAX_OUTLINE_CACHE: usize = 20_000;

/// The most laid-out text blocks the renderer keeps between frames, so that panning does not lay every label out again.
pub const MAX_LAYOUT_CACHE: usize = 4_096;

/// The most characters of text, in all, whose layout one frame pays for. A layout of characters no face covers cost 75 ms for 2,048 of
/// them (the T7 review), so the characters are counted before the time is: 20 blocks of 2,048 take 1.6 s.
pub const MAX_TEXT_CHARS_PER_FRAME: usize = 400_000;

/// The most characters of the labels of placeholders (`placeholder.rs`) whose layout one frame pays for, **in a budget of their own**: a label is at most 64
/// characters and is laid out once for each label and size, so this is about 6,600 labels of fifteen. A page of tens of thousands of objects, each at a size of its
/// own, spent the file's 400,000 characters on labels and left its text undrawn.
pub const MAX_LABEL_CHARS_PER_FRAME: usize = 100_000;

/// The longest edge, in pixels, a decoded picture is kept at. A picture is scaled down to this when it is larger, before it is drawn.
pub const MAX_PICTURE_SIDE: u32 = 8_192;

/// The most pixels a picture may have once decoded (and the most a decoder is allowed to allocate for one).
pub const MAX_PICTURE_PIXELS: u64 = 64 * 1024 * 1024;

/// The most bytes of a picture's part that are decoded.
pub const MAX_PICTURE_BYTES: usize = 64 * 1024 * 1024;

/// The most items of metafile pictures one frame draws, all pictures together. A metafile may hold a million, and the
/// rasteriser keeps about a kilobyte for each fill until the frame is finished: a million is a gigabyte. The same count as the shapes of a
/// frame; the largest picture in the sample files holds 139,143 (counting each line).
pub const MAX_METAFILE_ITEMS_PER_FRAME: usize = 400_000;

/// The most blurred shadows one frame draws. Each is a filter layer, which costs the area it covers; past this many, shadows are drawn
/// hard-edged and the frame says so.
pub const MAX_BLURS_PER_FRAME: usize = 400;

/// The farthest a shadow's blur reaches past its shape's edge, in pixels. `vello_cpu` pads a blurred layer by three deviations on every
/// side, so its pixmap is the shape's box plus twice this spread on each axis, and the spread sets the cost of a blur. A cell of 20 inches
/// at 100 pixels to the inch (a spread of 6,000 pixels) cost 521 MB, one of 100 inches 6.4 GB, and one of 150 inches asked for 14 GB, which
/// aborts the process (a failed allocation is not a panic). A blur that would reach farther is drawn at this reach and the frame says so
/// ([`SkipKind::ShadowApproximate`](crate::SkipKind::ShadowApproximate)). It is a project rule: no source bounds a blur in pixels. The
/// largest blur that files hold (14 points) spreads 300 pixels at 770 pixels to the inch,
/// which is a 2x screen at 400%, so a real drawing is not cut at that zoom.
pub const MAX_BLUR_SPREAD_PX: f64 = 320.0;

/// The most pixels that the blurred layers of one frame may hold together. `vello_cpu` keeps every layer's pixmap until the frame is
/// finished and draws them all then, so the cost of the frame is their sum, and neither the count of blurs nor the time budget (which
/// is spent between shapes, before any layer is drawn) bounds it. A layer is counted as its shape's box cut to the view, plus the spread
/// on each side. Past this many pixels (128 MB), shadows are drawn hard-edged and the frame says so.
pub const MAX_BLUR_PIXELS_PER_FRAME: u64 = 32 * 1024 * 1024;

/// The most rings a line end or a hatch draws.
pub const MAX_MARKS: usize = 4_096;

/// What a frame may spend. [`Limits::default`] is for a window that pans; an export raises the times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The wall-clock time a frame may take before it stops drawing shapes. The check is between shapes, so a frame can run over by
    /// the cost of one shape (itself bounded by the caps above). `None` is no time limit.
    pub frame_time: Option<Duration>,
    /// The part of the frame's time that text layout may use, from the first block. Past it the blocks that are not laid out yet are
    /// left out and counted. `None` is no limit.
    pub text_time: Option<Duration>,
    /// The part of the frame's time that the layout of the labels of placeholders may use, **beside** `text_time` and not out of it: past it the labels not
    /// laid out yet are left off (the boxes are all there). `None` is no limit.
    pub label_time: Option<Duration>,
    /// The most characters of labels a frame lays out, beside `max_text_chars` and not out of it.
    pub max_label_chars: usize,
    /// The most shapes a frame draws.
    pub max_shapes: usize,
    /// The most geometry rows a frame builds.
    pub max_rows: usize,
    /// The most characters of text a frame lays out.
    pub max_text_chars: usize,
    /// The most **units of work** a frame does, each charged before it is done: a shape the walk enters ([`WORK_PER_SHAPE`]), a glyph painted
    /// ([`WORK_PER_GLYPH`]), a dash of a line ([`WORK_PER_DASH`]), a line of a hatch ([`WORK_PER_MARK`]), an item of a metafile picture
    /// ([`WORK_PER_META_ITEM`]), the pixels and bytes of the pictures decoded ([`PIXELS_PER_WORK`], [`BYTES_PER_WORK`]) and the pixels pictures cover where they
    /// are drawn ([`DRAWN_PIXELS_PER_WORK`]). What would pass it is not done:
    /// the frame stops there, a point the drawing alone sets, and says so ([`SkipKind::WorkLimit`](crate::SkipKind::WorkLimit)). A unit is about what painting
    /// one glyph costs. `u64::MAX` (the default) is no limit: a window's frame, whose time bounds it.
    pub max_work: u64,
}

/// The work of a shape the walk enters ([`Limits::max_work`]): it is placed and its cells are read, whether it is drawn or lies outside the view (5 microseconds
/// for a box of a few rows, on a machine of 2026).
pub const WORK_PER_SHAPE: u64 = 16;

/// The work of a shape drawn, beside its entering: its geometry built, filled and stroked (a box of a few rows is 20 to 40 microseconds in all).
pub const WORK_PER_DRAWN_SHAPE: u64 = 48;

/// The work of a glyph painted: its outline placed and filled. Text a cache holds is laid out once and painted for every block that shows it, so a page of the same
/// long text in every shape paints far more glyphs than it lays out characters.
pub const WORK_PER_GLYPH: u64 = 1;

/// The work of a dash of a dashed line (a line of 20,000 dashes is 20,000).
pub const WORK_PER_DASH: u64 = 1;

/// The work of a line of a hatch (a fill pattern of lines): a stroke across the region, clipped to it.
pub const WORK_PER_MARK: u64 = 2;

/// The work of an item of a metafile picture drawn (a small item costs a fifth of a glyph; 400,000 of them are [`MAX_METAFILE_ITEMS_PER_FRAME`]).
pub const WORK_PER_META_ITEM: u64 = 1;

/// The pixels of a picture decoded for a unit of work (a raster by the size its header gives, a metafile's bitmaps as they are decoded): a picture of 64 million
/// pixels, the most one decodes to, is a million units and took 0.7 s.
pub const PIXELS_PER_WORK: u64 = 64;

/// The device pixels a picture drawn can cover, for a unit of work: a decoded picture is drawn again for every shape that shows it, and one over the whole of a
/// view of 256 pixels costs about a millisecond.
pub const DRAWN_PIXELS_PER_WORK: u64 = 16;

/// The bytes of a metafile picture read for a unit of work.
pub const BYTES_PER_WORK: u64 = 64;

/// The work a preview does ([`Limits::preview`]): two million units, about a second of painting glyphs on one core of a machine of 2026, and about thirty times
/// what a page of a thousand labelled boxes takes.
pub const PREVIEW_MAX_WORK: u64 = 2_000_000;

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            frame_time: Some(Duration::from_millis(1_500)),
            text_time: Some(Duration::from_millis(400)),
            label_time: Some(Duration::from_millis(150)),
            max_label_chars: MAX_LABEL_CHARS_PER_FRAME,
            max_shapes: MAX_SHAPES_PER_FRAME,
            max_rows: MAX_ROWS_PER_FRAME,
            max_text_chars: MAX_TEXT_CHARS_PER_FRAME,
            max_work: u64::MAX,
        }
    }
}

impl Limits {
    /// No time limits, for an export that is to draw everything and may take as long as that takes. The counts still hold.
    pub fn unlimited_time() -> Limits {
        Limits { frame_time: None, text_time: None, label_time: None, ..Limits::default() }
    }

    /// The limits of a picture that goes into a file: the preview a save writes of the first page. **No time limits**, so the same drawing gives the
    /// same picture however loaded the machine is (a frame held to a time stops at a shape that depends on the speed of the machine), and **counts sized for a
    /// picture of a few hundred pixels**, above all [`PREVIEW_MAX_WORK`], so that what a frame does is bounded by the drawing alone: past them the picture is cut
    /// at a point the drawing sets, and [`Rendered::skipped`](crate::Rendered::skipped) says so. The count of shapes is a window's: the work bounds them (a
    /// drawn box is [`WORK_PER_SHAPE`] and [`WORK_PER_DRAWN_SHAPE`], so about 31,000 are drawn), and a lower count would keep a page of a few thousand from
    /// being shared out among threads (`share.rs` holds the shapes and twice the steps queued to it). A window's frame keeps [`Limits::default`].
    pub fn preview() -> Limits {
        Limits {
            max_rows: PREVIEW_MAX_ROWS,
            max_text_chars: PREVIEW_MAX_TEXT_CHARS,
            max_label_chars: PREVIEW_MAX_LABEL_CHARS,
            max_work: PREVIEW_MAX_WORK,
            ..Limits::unlimited_time()
        }
    }
}

/// The most geometry rows a preview builds ([`Limits::preview`]): six for each of the shapes its work draws.
pub const PREVIEW_MAX_ROWS: usize = 200_000;

/// The most characters of text whose layout a preview pays for: a page of a thousand labels of twenty. A character no face covers costs tens of microseconds to
/// look for, so this is what bounds the layout of a page of text that no face draws.
pub const PREVIEW_MAX_TEXT_CHARS: usize = 20_000;

/// The most characters of the labels of placeholders a preview lays out, beside its text.
pub const PREVIEW_MAX_LABEL_CHARS: usize = 4_096;

/// A request to stop a frame that is no longer wanted. A window numbers its requests and keeps the newest number in a shared counter; each
/// frame is handed that counter and its own number, and **once the counter is past its number the frame stops drawing shapes**, at the next
/// place it checks the budget (between shapes), and says so with [`SkipKind::Cancelled`](crate::SkipKind::Cancelled). The picture of a
/// frame that stopped is not rasterised: [`Rendered::rgba_premul`](crate::Rendered::rgba_premul) is empty, and the caller drops it.
///
/// A frame already past its last shape cannot be taken back (the layers of its blurs are rasterised at the end, which this skips, and a
/// shape in progress is finished), so a frame runs on by at most one shape.
#[derive(Clone, Debug)]
pub struct Cancel {
    newest: Arc<AtomicU64>,
    mine: u64,
}

impl Cancel {
    /// The stop for the frame numbered `mine`, given the shared counter that holds the newest number asked for.
    pub fn new(newest: Arc<AtomicU64>, mine: u64) -> Cancel {
        Cancel { newest, mine }
    }

    /// Whether a newer request has been numbered since this frame's.
    pub fn is_stale(&self) -> bool {
        self.newest.load(Ordering::Relaxed) > self.mine
    }
}
