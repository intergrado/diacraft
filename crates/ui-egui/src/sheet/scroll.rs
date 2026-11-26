//! The sheet's own scroll (design 6): the lines on view and the scroll bar, which are **not an `egui::ScrollArea`** that holds every line.
//!
//! A section of 31,273 rows is 31,273 lines, and a sheet may list a million: an `f32` loses whole points past 16,777,216, and a million lines of 22 points are 22 million. So the
//! position is two integers, **the line at the top and how far into it the view is, in sixteenths of a point**, and the scroll bar maps its thumb to a line number with `f64`.
//! The idea, the scroll bar of a list too long for a `ScrollArea` (its thumb, the drag by the thumb's travel, a press in the track a page at a time), is that of the grid of
//! GridCraft (`crates/ui-egui/src/grid/show.rs`, `vertical_scrollbar`; `docs/provenance.md`); the code is written for lines of a sheet.
//!
//! The functions of integers are the ones a proof can check (`visible_lines`, `Scroll::by`, `max_first`): each is held by a test and total (no overflow and no panic for any input).

use egui::{Id, Rect, Response, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::theme::Tokens;

/// How many of the units of the integer scroll make a point: sixteenths of a point.
pub const SIXTEENTHS: u32 = 16;

/// The least a scroll bar's thumb is long, in points, so that it can be taken.
const THUMB_MIN: f32 = 24.0;

/// A position in a list of lines: the line at the top, and how far the view is into it, in sixteenths of a point (always less than a line).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    pub first: u64,
    pub offset: u32,
}

/// The lines on view: from `first`, the view `height` tall, `offset` of its first line above its top edge, a line `line_h` tall; all in sixteenths of a point. → `(start, end)`,
/// the lines from `start` up to but not counting `end`: at most `total` and never more than the lines that fit, with the partly seen ones at the top and the bottom.
pub fn visible_lines(first: u64, offset: u32, height: u32, line_h: u32, total: u64) -> (u64, u64) {
    let start = first.min(total);
    let line_h = u64::from(line_h.max(1));
    let fit = (u64::from(height) + u64::from(offset)).div_ceil(line_h);
    (start, start.saturating_add(fit).min(total))
}

/// The most the top line of a list of `total` lines can be, when `page` lines show: the last page ends at the bottom.
pub fn max_first(total: u64, page: u64) -> u64 {
    total.saturating_sub(page)
}

impl Scroll {
    /// The scroll at the top of line `line`, no further.
    pub fn at(line: u64) -> Scroll {
        Scroll { first: line, offset: 0 }
    }

    /// The scroll moved by `delta` sixteenths of a point (positive: down), a line being `line_h` tall, and kept between the first line and `max_first`.
    pub fn by(self, delta: i64, line_h: u32, max_first: u64) -> Scroll {
        let line_h = i128::from(line_h.max(1));
        let at = i128::from(self.first) * line_h + i128::from(self.offset);
        let end = i128::from(max_first) * line_h;
        let next = (at + i128::from(delta)).clamp(0, end);
        Scroll { first: u64::try_from(next / line_h).unwrap_or(max_first), offset: u32::try_from(next % line_h).unwrap_or(0) }
    }

    /// The scroll with no more than `max_first` lines above it, and the part of a line given up when the last page is at the bottom.
    pub fn clamped(self, max_first: u64) -> Scroll {
        if self.first >= max_first { Scroll { first: max_first, offset: 0 } } else { self }
    }

    /// The position as a number of lines, with the part of one.
    pub fn as_lines(self, line_h: u32) -> f64 {
        self.first as f64 + f64::from(self.offset) / f64::from(line_h.max(1))
    }

    /// The scroll at `lines` lines from the top, kept between 0 and `max_first`. A position that is not a number is the top.
    pub fn from_lines(lines: f64, line_h: u32, max_first: u64) -> Scroll {
        if !lines.is_finite() || lines <= 0.0 {
            return Scroll::default();
        }
        let line_h = u32::max(line_h, 1);
        let whole = lines.floor();
        let first = if whole >= max_first as f64 { max_first } else { whole as u64 };
        let offset = ((lines - whole) * f64::from(line_h)).round() as u32;
        Scroll { first, offset: offset.min(line_h - 1) }.clamped(max_first)
    }
}

/// How long the thumb of a scroll bar is, and how far from the top of the track: for a list of `total` units of which `page` show, now at `pos` units from the top, along a track of
/// `track` points. The thumb is at least [`THUMB_MIN`] points (or the whole track when that is shorter) and the whole track when everything shows. Finite for any finite input.
pub fn thumb(track: f32, total: f64, page: f64, pos: f64) -> (f32, f32) {
    let track = if track.is_finite() { track.max(0.0) } else { 0.0 };
    if !(total.is_finite() && page.is_finite() && pos.is_finite()) || total <= 0.0 || page >= total {
        return (0.0, track);
    }
    let len = (f64::from(track) * page.max(0.0) / total) as f32;
    let len = len.clamp(THUMB_MIN.min(track), track);
    let room = f64::from(track - len);
    let reach = (pos / (total - page).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
    ((room * reach) as f32, len)
}

/// A scroll bar in `track`, vertical or horizontal, for a list of `total` units of which `page` show, now at `pos`. → where it is after this frame's press, drag or wheel, in the same
/// units. The thumb is dragged by its travel, a press in the track above or below it moves a page, and the wheel moves `wheel` units for a point of delta (the caller says it, as the
/// list knows what its unit is). **The bar is named for assistive technology** and draws nothing when everything shows.
#[allow(clippy::too_many_arguments)]
pub fn scroll_bar(ui: &mut Ui, id: Id, track: Rect, vertical: bool, list: (f64, f64, f64), name: &str, t: &Tokens) -> (f64, Response) {
    let (total, page, pos) = list;
    let along = if vertical { track.height() } else { track.width() };
    let resp = ui.interact(track, id, Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::labeled(Role::ScrollBar, true, name));
    if total <= page || along <= 0.0 {
        return (pos, resp);
    }
    let (offset, len) = thumb(along, total, page, pos);
    let thumb_rect = if vertical {
        Rect::from_min_size(pos2(track.left() + 3.0, track.top() + offset), vec2((track.width() - 6.0).max(1.0), len))
    } else {
        Rect::from_min_size(pos2(track.left() + offset, track.top() + 3.0), vec2(len, (track.height() - 6.0).max(1.0)))
    };
    ui.painter().rect_filled(track, 0.0, t.panel_header_fill);
    ui.painter().rect_filled(thumb_rect, 4.0, if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled });
    let travel = f64::from(along - len);
    let reach = (total - page).max(0.0);
    let mut next = pos;
    if resp.dragged() && travel > 0.0 {
        let delta = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
        next += f64::from(delta) / travel * reach;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (at, from, to) = if vertical { (p.y, thumb_rect.top(), thumb_rect.bottom()) } else { (p.x, thumb_rect.left(), thumb_rect.right()) };
        if at < from {
            next -= page;
        } else if at > to {
            next += page;
        }
    }
    (next.clamp(0.0, reach), resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_on_view_are_the_lines_that_fit_with_the_partly_seen_ones() {
        let h = 22 * SIXTEENTHS;
        // 100 points of view and 22-point lines: four and a half lines.
        assert_eq!(visible_lines(10, 0, 100 * SIXTEENTHS, h, 1000), (10, 15));
        // Scrolled half a line, the line at the top is partly seen and the view still ends in a partly seen one.
        assert_eq!(visible_lines(10, h / 2, 100 * SIXTEENTHS, h, 1000), (10, 16));
        // Never past the end, and never before the start.
        assert_eq!(visible_lines(998, 0, 100 * SIXTEENTHS, h, 1000), (998, 1000));
        assert_eq!(visible_lines(5000, 0, 100 * SIXTEENTHS, h, 1000), (1000, 1000), "a position past the list is the end");
        assert_eq!(visible_lines(0, 0, 0, h, 1000), (0, 0), "no room, no lines");
        assert_eq!(visible_lines(0, 0, 100, 0, 3), (0, 3), "a line of no height is one sixteenth: nothing divides by zero");
        assert_eq!(visible_lines(u64::MAX, u32::MAX, u32::MAX, 1, u64::MAX), (u64::MAX, u64::MAX));
        for first in [0, 1, 7, 30, 31] {
            for offset in [0, 1, h - 1] {
                let (start, end) = visible_lines(first, offset, 150 * SIXTEENTHS, h, 31);
                assert!(start <= end && end <= 31, "{first} {offset}: {start}..{end}");
                assert!(end - start <= u64::from((150 * SIXTEENTHS + offset).div_ceil(h)), "{first} {offset}: {start}..{end}");
            }
        }
    }

    #[test]
    fn a_scroll_moves_by_sixteenths_and_stops_at_the_first_line_and_at_the_last_page() {
        let h = 22 * SIXTEENTHS;
        let top = Scroll::default();
        assert_eq!(top.by(-5, h, 100), top, "no further up than the top");
        assert_eq!(top.by(5, h, 100), Scroll { first: 0, offset: 5 });
        assert_eq!(top.by(i64::from(h) * 3 + 7, h, 100), Scroll { first: 3, offset: 7 }, "three lines and a little");
        assert_eq!(Scroll { first: 3, offset: 7 }.by(-i64::from(h), h, 100), Scroll { first: 2, offset: 7 });
        assert_eq!(top.by(i64::MAX, h, 100), Scroll { first: 100, offset: 0 }, "stops where the last page ends at the bottom");
        // No overflow at the ends of the integers: the position is worked out in a wider integer.
        let far = Scroll { first: u64::MAX, offset: 3 }.by(i64::MIN, h, u64::MAX);
        assert!(far.first < u64::MAX && far.offset < h, "{far:?}");
        let near = Scroll { first: u64::MAX, offset: 3 }.by(i64::MAX, h, u64::MAX);
        assert_eq!(near, Scroll { first: u64::MAX, offset: 0 }, "past the last page is the last page");
        assert_eq!(top.by(5, 0, 100), Scroll { first: 5, offset: 0 }, "a line of no height is one sixteenth");
        assert_eq!(Scroll { first: 200, offset: 4 }.by(0, h, 100), Scroll { first: 100, offset: 0 }, "a position past the last page is brought back");
        assert_eq!(Scroll { first: 100, offset: 9 }.clamped(100), Scroll { first: 100, offset: 0 });
        assert_eq!(Scroll { first: 99, offset: 9 }.clamped(100), Scroll { first: 99, offset: 9 });
    }

    #[test]
    fn the_scroll_is_a_number_of_lines_and_back() {
        let h = 22 * SIXTEENTHS;
        assert_eq!(Scroll { first: 4, offset: h / 2 }.as_lines(h), 4.5);
        assert_eq!(Scroll::from_lines(4.5, h, 100), Scroll { first: 4, offset: h / 2 });
        assert_eq!(Scroll::from_lines(-3.0, h, 100), Scroll::default());
        assert_eq!(Scroll::from_lines(f64::NAN, h, 100), Scroll::default(), "not a number: the top");
        assert_eq!(Scroll::from_lines(1e300, h, 100), Scroll { first: 100, offset: 0 });
        assert_eq!(Scroll::from_lines(7.999_999, h, 100).first.min(8), Scroll::from_lines(7.999_999, h, 100).first, "never past the next line");
        assert!(Scroll::from_lines(7.999_999, h, 100).offset < h);
    }

    #[test]
    fn the_thumb_is_in_the_track_and_the_whole_track_when_everything_shows() {
        assert_eq!(thumb(200.0, 100.0, 100.0, 0.0), (0.0, 200.0));
        assert_eq!(thumb(200.0, 50.0, 100.0, 0.0), (0.0, 200.0), "a list shorter than its page");
        let (top, len) = thumb(200.0, 1000.0, 10.0, 0.0);
        assert_eq!((top, len), (0.0, THUMB_MIN), "a long list has the least thumb");
        let (top, len) = thumb(200.0, 1000.0, 10.0, 990.0);
        assert!((top + len - 200.0).abs() < 0.01, "the thumb reaches the end of the track at the last page: {top} {len}");
        let (top, len) = thumb(200.0, 100.0, 50.0, 25.0);
        assert_eq!(len, 100.0);
        assert!((top - 50.0).abs() < 0.01, "half way through, the thumb is half way along: {top}");
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let (top, len) = thumb(200.0, bad, 10.0, 5.0);
            assert!(top.is_finite() && len.is_finite() && top >= 0.0 && top + len <= 200.0 + 0.01, "{bad}: {top} {len}");
            let (top, len) = thumb(200.0, 100.0, 10.0, bad);
            assert!(top.is_finite() && len.is_finite() && top >= 0.0 && top + len <= 200.0 + 0.01, "{bad}: {top} {len}");
        }
        assert_eq!(thumb(f32::NAN, 100.0, 10.0, 5.0), (0.0, 0.0), "no track, no thumb");
        assert_eq!(thumb(10.0, 1000.0, 10.0, 0.0).1, 10.0, "a track shorter than the least thumb is all thumb");
    }
}
