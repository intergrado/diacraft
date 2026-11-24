//! The placeholder of an object DiaCraft cannot draw.
//!
//! A reader that finds an object it cannot draw (a table, an embedded object, a 3-D scene, a dimension line, ...) and has no picture of it makes a `Foreign`
//! shape on its frame with **no picture** and a `User` row `ODFObject` whose `Value` says in words what the object is. Such a shape is drawn here, as **its
//! frame's outline, a pale fill, a cross, and the label**, and the frame notes [`SkipKind::ObjectPlaceholder`], so that a person is told the box is a stand-in
//! and what for. A shape with a picture is the picture (`picture.rs`); a shape with no marker is as it was.
//!
//! * **The box** is the frame, `Width` by `Height` in the shape's own frame, so it moves, turns and flips with the shape. A frame of no size (both 0, or one
//!   below 0, or not a number) draws nothing and says nothing. A frame of no height or no width (a dimension line) is the line it stands for.
//! * **The label** is the marker's text, cut at [`MOST_LABEL_CHARS`] characters, centred in the frame in one size that fits ([`label_size`]): at most
//!   [`LABEL_MAX_SIZE`], and a label that would be below [`LABEL_MIN_SIZE`] (a frame too small to read it in) is left off, the box standing alone. It is never
//!   mirrored (the flips of the shape and of the groups above it are cancelled, as for text, `text.rs`). It is laid out through the cache and the budgets the
//!   file's own text uses, once for each label and size, and **a label the page has no budget for is left off without a note**: it is no text of the file's.
//!   Notes of its layout (a missing face) are not said, for the same reason; the box is the answer.
//! * **Cost.** One look at a shape's kind for every shape, and for an object a bounded label (at most [`MOST_LABEL_CHARS`] characters) laid out once for
//!   each distinct label and size.

use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, RowKey, ShapeType};
use diacraft_text::{BlockFormat, CharFormat, ParaFormat, Run};
use kurbo::{Affine, Rect, Shape as _};

use crate::frame::{Flips, Frame, ShapeCtx};
use crate::picture::{PLACEHOLDER_FILL, foreign_of};
use crate::read::Cx;
use crate::skips::SkipKind;
use crate::target::{Refused, Target};

/// The name of the `User` row that marks an object and names it.
pub(crate) const MARKER_ROW: &str = "ODFObject";

/// The most characters of a label that are drawn.
pub(crate) const MOST_LABEL_CHARS: usize = 64;

/// The largest size of the label's letters, in paper inches: a caption, whatever the size of the box.
pub(crate) const LABEL_MAX_SIZE: f64 = 0.14;

/// The smallest size a label is drawn at, in paper inches (four points): a frame that holds it only smaller has the box alone.
pub(crate) const LABEL_MIN_SIZE: f64 = 4.0 / 72.0;

/// How wide the letters of a label are on the whole, in ems: the size is chosen so that a label of this width fits the frame (a face with wider letters wraps
/// the label, and a word wider than the frame runs past it).
const LETTER_EMS: f64 = 0.6;

/// How much of the frame's height the letters may take, so that a label of two lines still fits.
const LABEL_SHARE_OF_HEIGHT: f64 = 0.3;

/// The label of the object the shape is, when it is one: the text of its `User.ODFObject` row, cut at [`MOST_LABEL_CHARS`] characters.
pub(crate) fn label_of(cx: &Cx<'_, '_>, sc: &ShapeCtx<'_>) -> Option<String> {
    let marker = CellAddr::in_row("User", None, RowKey::name(MARKER_ROW), "Value");
    let text = cx.text_at(&sc.sheet, &marker)?;
    Some(text.chars().take(MOST_LABEL_CHARS).collect())
}

/// The size of the label's letters in paper inches for a frame `bw` wide and `bh` high (inches on paper) and a label of `chars` characters, or `None` when
/// they would be too small to read, or when the frame is not a size that can be worked with. `bh` of 0 is a line, whose height limits nothing.
pub(crate) fn label_size(bw: f64, bh: f64, chars: usize) -> Option<f64> {
    // (A width below 0 gives a size below 0 and is no label by the last line; a height below 0 would read as a line, so it is refused here.)
    if chars == 0 || !bw.is_finite() || !bh.is_finite() || bh < 0.0 {
        return None;
    }
    let by_width = bw / (chars as f64 * LETTER_EMS);
    let by_height = if bh > 0.0 { bh * LABEL_SHARE_OF_HEIGHT } else { f64::INFINITY };
    let size = LABEL_MAX_SIZE.min(by_width).min(by_height);
    (size >= LABEL_MIN_SIZE).then_some(size)
}

impl<T: Target> Frame<'_, '_, '_, T> {
    /// Draws the placeholder of the shape of `sc`, if it is an object with a label and no picture. `groups` are the flips of the groups above it, and `scale` is
    /// paper inches over the page's drawing units.
    pub(crate) fn draw_object_placeholder(&mut self, sc: &ShapeCtx<'_>, groups: Flips, scale: f64, who: &dyn Fn() -> String) {
        let cx = self.cx;
        let Some(shape) = cx.res.shape(&sc.at) else { return };
        if shape.kind != ShapeType::Foreign || foreign_of(self, sc).is_some_and(|f| f.picture.is_some()) {
            return;
        }
        let Some(label) = label_of(cx, sc) else { return };
        if self.speculative {
            // A label is laid out within budgets of its own and through the kept layouts, which serve the shapes in order.
            self.bail();
            return;
        }
        let (w, h) = (cx.num(&sc.sheet, "Width"), cx.num(&sc.sheet, "Height"));
        let sized = w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0 && (w > 0.0 || h > 0.0);
        if !sized {
            return;
        }
        // The frame, and a margin round it for the stroke, which a frame of no height or width would otherwise clip away altogether: a fiftieth of an inch of
        // paper, which is `0.02 / scale` of the page's drawing units (`scale` is `Frame::page_scale`'s, finite and above 0).
        let pad = 0.02 / scale;
        let frame = Rect::new(0.0, 0.0, w, h);
        let clip = sc.to_paper * frame.inflate(pad, pad).to_path(0.1);
        match self.canvas.check(&clip, 0.0) {
            Ok(true) => {}
            Ok(false) => return,
            Err(Refused::OutOfRange) => {
                cx.note(SkipKind::OutOfRange, || format!("{}: an object's box", who()));
                return;
            }
        }
        cx.note(SkipKind::ObjectPlaceholder, || format!("{}: {label}", who()));
        self.placeholder(sc, frame, &clip);
        self.draw_label(sc, &label, (w, h), scale, groups, who);
    }

    /// The label, centred in the frame `(w, h)` (drawing units) in one size that fits it.
    fn draw_label(&mut self, sc: &ShapeCtx<'_>, label: &str, frame: (f64, f64), scale: f64, groups: Flips, who: &dyn Fn() -> String) {
        let (w, h) = frame;
        let (bw, bh) = (w * scale, h * scale);
        let chars = label.chars().count();
        let Some(size) = label_size(bw, bh, chars) else { return };
        let char_format = Arc::new(CharFormat { size, color: Rgba::rgb(90, 90, 90), ..CharFormat::default() });
        // Centred across and down in the frame: the defaults of the two formats (the centring is tested by where the label's ink lies).
        let para_format = Arc::new(ParaFormat::default());
        let runs = [Run::new(label, char_format, para_format)];
        // The label stands on a plate of the box's own fill, so that the cross does not run through the letters.
        let block = BlockFormat { height: (bh > 0.0).then_some(bh), background: Some((PLACEHOLDER_FILL, 0.0)), ..BlockFormat::default() };
        let Some(laid) = self.lay_out_label(&runs, &block, bw, chars) else { return };
        // The block's frame is paper inches with its origin at the lower left of the frame: back to the shape's units, and about the frame's middle the flips
        // of the shape and of the groups above it a second time, which is none: the label is never mirrored.
        let flipped = Flips::of(self.cx, &sc.sheet).then(groups);
        let (fx, fy) = (if flipped.x { -1.0 } else { 1.0 }, if flipped.y { -1.0 } else { 1.0 });
        let middle = (w / 2.0, h / 2.0);
        let to_paper = sc.to_paper
            * Affine::translate(middle)
            * Affine::scale_non_uniform(fx, fy)
            * Affine::translate((-middle.0, -middle.1))
            * Affine::scale(1.0 / scale);
        // (Not clipped to the frame: the size is the one that fits the label on a line, a word wider than the frame is broken by the layout, and a line has no
        // inside, its label standing beside it. `a_label_wider_than_its_frame_stays_in_it` pins that nothing runs past.)
        self.paint_laid(&laid, to_paper, None, who);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The numbers are the rule's: a label of four points at the least and a caption's size at the most, in a cut of 64 characters.
    #[test]
    fn the_limits_of_a_label_are_what_the_module_says() {
        assert_eq!((MOST_LABEL_CHARS, LABEL_MAX_SIZE, MARKER_ROW), (64, 0.14, "ODFObject"));
        assert!((LABEL_MIN_SIZE - 4.0 / 72.0).abs() < 1.0e-12);
    }

    /// The size of the letters follows the frame: at most [`LABEL_MAX_SIZE`], at most what the width and the height give, none below [`LABEL_MIN_SIZE`], none
    /// for no letters or a frame that is no size, and a frame of no height (a line) is held by its width alone.
    #[test]
    fn the_size_of_a_label_follows_its_frame_and_is_none_where_it_could_not_be_read() {
        // Big frames: the caption's size.
        assert_eq!(label_size(10.0, 10.0, 5), Some(LABEL_MAX_SIZE));
        assert_eq!(label_size(10.0, 0.0, 5), Some(LABEL_MAX_SIZE), "a line is held by its width alone");
        // The height gives a third of itself (0.3 of 0.4 inch is 0.12), the width a letter 0.6 em wide for each character (3.0 inches, 50 letters: 0.1).
        let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1.0e-9);
        assert!(near(label_size(10.0, 0.4, 5), 0.4 * LABEL_SHARE_OF_HEIGHT), "{:?}", label_size(10.0, 0.4, 5));
        assert!(near(label_size(3.0, 10.0, 50), 3.0 / (50.0 * LETTER_EMS)), "{:?}", label_size(3.0, 10.0, 50));
        assert!(near(label_size(3.0, 0.0, 50), 3.0 / (50.0 * LETTER_EMS)), "{:?}", label_size(3.0, 0.0, 50));
        // The smallest size that is read: the width that gives exactly four points is drawn, a hair less is not.
        let narrowest = 10.0 * LETTER_EMS * LABEL_MIN_SIZE;
        assert!(label_size(narrowest * 1.001, 0.0, 10).is_some() && label_size(narrowest * 0.999, 0.0, 10).is_none());
        let shortest = LABEL_MIN_SIZE / LABEL_SHARE_OF_HEIGHT;
        assert!(label_size(10.0, shortest * 1.001, 3).is_some() && label_size(10.0, shortest * 0.999, 3).is_none());
        assert!(label_size(LETTER_EMS * LABEL_MIN_SIZE, 0.0, 1).is_some(), "exactly four points is read");
        // No letters, no frame, no number.
        assert_eq!(label_size(10.0, 10.0, 0), None);
        for (w, h) in
            [(-1.0, 1.0), (1.0, -1.0), (f64::NAN, 1.0), (1.0, f64::NAN), (f64::INFINITY, 1.0), (1.0, f64::INFINITY), (f64::NEG_INFINITY, 0.0)]
        {
            assert_eq!(label_size(w, h, 5), None, "{w} by {h}");
        }
        assert_eq!(label_size(0.0, 0.0, 5), None, "a frame of no size has no room for letters");
    }
}
