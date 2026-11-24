//! Counters of the work the panes and the drags do, for tests that **count** the work on a crafted drawing instead of timing it.
//!
//! They are per thread, so that tests run side by side do not meet, and they cost a few instructions each. Nothing reads them but tests.

use std::cell::Cell;

/// The most points the picture of Pan & Zoom holds in all, and the most rows a branch of Drawing Explorer lists: the bounds the counts are held to.
pub const THUMB_POINT_BUDGET: usize = 40_000;
pub const EXPLORER_ROWS_MAX: usize = 300;

/// What the panes have done since the last [`reset`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Size & Position: the shapes it looked up in the scene to see what each can do (a count of shapes, never of outline points: it copies none).
    pub caps_built: usize,
    /// Size & Position: `shape.placement` asked of the engine.
    pub placement_queries: usize,
    /// Shape Data: `shapedata.list` asked of the engine.
    pub data_queries: usize,
    /// Pan & Zoom: times the picture's lines were made again from the scene.
    pub thumb_builds: usize,
    /// Pan & Zoom: points in the lines of the last picture made.
    pub thumb_points: usize,
    /// Pan & Zoom: shapes the last picture drew with less detail than they have, and shapes it left out because not even their boxes fit.
    pub thumb_simplified: usize,
    pub thumb_left_out: usize,
    /// Pan & Zoom: points drawn in the last frame.
    pub thumb_painted: usize,
    /// Drawing Explorer: times the tree's counts were worked out again from the document.
    pub explorer_scans: usize,
    /// Drawing Explorer: names built for rows (a row is made only for a branch that is unfolded and only up to the row cap).
    pub explorer_names: usize,
    /// The drags' snap: times a page was read for the snap (`SnapPage::read`: its guides, its grid, and later the index of its shapes), times
    /// a move's shapes were looked up for their box and their connection points, and snaps asked.
    pub snap_page_reads: usize,
    pub move_subjects: usize,
    pub snaps: usize,
    /// The connection points marked over the page (crosses and the one lit), counted as they are painted (View > Connection Points).
    pub points_marked: usize,
    /// The Operations menu: times the engine was asked whether a shape operation can run on the shapes selected (`check_shape_ops`: it reads
    /// the shapes' geometry), which is once for each operation for a drawing, revision, page and selection, and never for a frame.
    pub shape_op_checks: usize,
    /// The marks of a drag's snap painted, and the labels among them (a gap's, a size's, an angle's), counted as they are painted.
    pub marks_painted: usize,
    pub labels_painted: usize,
    /// Times a query of the snap on this thread made the index of the page's shapes itself: the reader makes it off the window's thread, so a drag asks none.
    pub index_built_here: usize,
    /// Times a query of the snap on this thread built the set of targets the library is given: the reader builds it, so a drag asks none either (the index
    /// alone was counted before, and a drag whose targets were built here would have passed).
    pub targets_built_here: usize,
    /// Requests the window made of the reader of pages: a drag held while the reader works asks once, not once a frame.
    pub reads_asked: usize,
    /// Times the reader of pages did its work on this thread, inside the frame that asked for it: where it has no thread of its own (the web) or a test fixes the order.
    pub reads_inline: usize,
    /// The markers of the comments (`comments_ui.rs`): the rows of the listing looked at to place a frame's markers (one each, whatever their anchors), and the places of the table that groups them by
    /// where they sit probed (at most two for each row).
    pub comment_rows_seen: usize,
    pub comment_cells_probed: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { caps_built: 0, placement_queries: 0, data_queries: 0, thumb_builds: 0, thumb_points: 0, thumb_simplified: 0, thumb_left_out: 0, thumb_painted: 0, explorer_scans: 0, explorer_names: 0, snap_page_reads: 0, move_subjects: 0, snaps: 0, points_marked: 0, shape_op_checks: 0, marks_painted: 0, labels_painted: 0, index_built_here: 0, targets_built_here: 0, reads_asked: 0, reads_inline: 0, comment_rows_seen: 0, comment_cells_probed: 0 }) };
}

/// What the panes have done on this thread since the last [`reset`].
pub fn work() -> Work {
    WORK.with(Cell::get)
}

/// Starts the count again from nothing.
pub fn reset() {
    WORK.with(|w| w.set(Work::default()));
}

/// Changes the count.
pub(crate) fn count(change: impl FnOnce(&mut Work)) {
    WORK.with(|w| {
        let mut now = w.get();
        change(&mut now);
        w.set(now);
    });
}
