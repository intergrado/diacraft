//! Bidirectional text (Unicode UAX #9): embedding levels for a paragraph, and the visual order of a line.
//!
//! Sources say only that `Flags` sets the base direction and that characters order by the bidirectional algorithm (a project
//! rule), so this is the algorithm as `unicode-bidi` has it, with no tailoring. Levels are worked out once for the logical paragraph;
//! shaping and line breaking happen in logical order; each line is then reordered on its own (rules L1 and L2). Mirroring of
//! brackets in right-to-left runs is the shaper's.
//!
//! The explicit embedding and isolate controls are honoured, so a hostile paragraph can nest them 100,000 deep: `unicode-bidi` caps
//! the depth it tracks at 125 and the work is measured against that in the tests.

use unicode_bidi::{BidiClass, BidiInfo, Level, bidi_class};

use crate::prepare::Cell;

/// Does anything in the paragraph need the algorithm? Strong right-to-left letters, Arabic numbers or an explicit control.
fn needs_bidi(cells: &[Cell]) -> bool {
    cells.iter().any(|c| {
        matches!(
            bidi_class(c.ch),
            BidiClass::R
                | BidiClass::AL
                | BidiClass::AN
                | BidiClass::LRE
                | BidiClass::RLE
                | BidiClass::LRO
                | BidiClass::RLO
                | BidiClass::PDF
                | BidiClass::LRI
                | BidiClass::RLI
                | BidiClass::FSI
                | BidiClass::PDI
        )
    })
}

/// The embedding level of every cell: 0 for plain left-to-right text, 1 for a right-to-left paragraph, and the algorithm's levels
/// where it is needed. `text` is the cells' characters in order.
pub(crate) fn cell_levels(text: &str, cells: &[Cell], base_rtl: bool) -> Vec<u8> {
    if !base_rtl && !needs_bidi(cells) {
        return Vec::new();
    }
    let info = BidiInfo::new(text, Some(if base_rtl { Level::rtl() } else { Level::ltr() }));
    let base = u8::from(base_rtl);
    let mut at = 0usize;
    cells
        .iter()
        .map(|c| {
            let level = info.levels.get(at).map_or(base, |l| l.number());
            at += c.ch.len_utf8();
            level
        })
        .collect()
}

/// The order clusters of these levels stand in on a line, left to right, as indices into the line (rule L2). Levels all even give
/// the logical order back.
pub(crate) fn visual_order(levels: &[u8]) -> Vec<usize> {
    if levels.iter().all(|l| *l == 0) {
        return (0..levels.len()).collect();
    }
    let levels: Vec<Level> = levels.iter().map(|l| Level::new((*l).min(Level::max_implicit_depth() + 1)).unwrap_or_else(|_| Level::ltr())).collect();
    BidiInfo::reorder_visual(&levels)
}

/// What a cluster is to rule L1 of the algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    /// Anything else: it keeps its level.
    Other,
    /// White space, or an isolate control: it takes the paragraph's level when a separator or the line's end follows it.
    Space,
    /// A segment separator, a tab: it takes the paragraph's level always.
    Separator,
}

/// The kind of cluster these characters make, by their original bidirectional classes (not the resolved ones, as L1 says).
pub(crate) fn edge_of(chars: impl Iterator<Item = char>) -> Edge {
    let mut edge = None;
    for ch in chars {
        let this = match bidi_class(ch) {
            BidiClass::S => Edge::Separator,
            BidiClass::WS | BidiClass::LRI | BidiClass::RLI | BidiClass::FSI | BidiClass::PDI => Edge::Space,
            _ => return Edge::Other,
        };
        // A cluster of a separator and more is not a bare separator, nor a bare space.
        if edge.is_some_and(|e| e != this) {
            return Edge::Other;
        }
        edge = Some(this);
    }
    edge.unwrap_or(Edge::Other)
}

/// Rule L1 for one line: a separator takes the paragraph's level, and so does any run of white space before a separator or at the
/// line's end. `levels` and `edges` are the line's clusters in logical order.
pub(crate) fn reset_edges(levels: &mut [u8], edges: &[Edge], paragraph_level: u8) {
    // The line's end counts as a separator.
    let mut reset = true;
    for (level, edge) in levels.iter_mut().zip(edges).rev() {
        match edge {
            Edge::Separator => {
                *level = paragraph_level;
                reset = true;
            }
            Edge::Space if reset => *level = paragraph_level,
            _ => reset = false,
        }
    }
}
