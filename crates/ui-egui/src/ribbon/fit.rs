//! Fitting a tab's groups into the width of the window: groups give up size before any is clipped.
//!
//! A group has three sizes (a group's layout depends on the window's width): as designed
//! ([`Level::Full`]), with its controls reduced to small icon buttons ([`Level::Compact`]), and folded into one button that
//! opens the whole group ([`Level::Folded`]). [`plan`] picks a size for each group so that the row fits: groups shrink from
//! the right, first all to compact, then all to folded, stopping as soon as the row fits, and whatever room is then left is
//! given back from the left.
//!
//! The plan is the one the sibling craft apps' ribbons use (`docs/provenance.md`); the widths here are measured from the
//! layout each frame and are not remembered from earlier frames.

/// The size of a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// As designed.
    Full,
    /// Small icon buttons without labels.
    Compact,
    /// One button that opens the group.
    Folded,
}

impl Level {
    /// The next larger size, if there is one.
    fn larger(self) -> Option<Level> {
        match self {
            Level::Full => None,
            Level::Compact => Some(Level::Full),
            Level::Folded => Some(Level::Compact),
        }
    }
}

/// A group's width at each size, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widths {
    pub full: f32,
    pub compact: f32,
    pub folded: f32,
}

impl Widths {
    /// The width at `level`.
    pub fn at(&self, level: Level) -> f32 {
        match level {
            Level::Full => self.full,
            Level::Compact => self.compact,
            Level::Folded => self.folded,
        }
    }
}

/// The size of each group so that the groups fit in `avail` points, if they can.
pub fn plan(groups: &[Widths], avail: f32) -> Vec<Level> {
    let mut levels = vec![Level::Full; groups.len()];
    let mut total: f32 = groups.iter().map(|g| g.full).sum();
    for target in [Level::Compact, Level::Folded] {
        for i in (0..groups.len()).rev() {
            if total <= avail {
                break;
            }
            let (Some(g), Some(level)) = (groups.get(i), levels.get_mut(i)) else { continue };
            if *level >= target {
                continue;
            }
            total += g.at(target) - g.at(*level);
            *level = target;
        }
    }
    // Folding a wide group can leave room that the groups shrunk before it no longer need; and a group that gains nothing
    // by being smaller should not be: give back whatever still fits, from the left.
    for (g, level) in groups.iter().zip(levels.iter_mut()) {
        while let Some(larger) = level.larger() {
            let next = total + g.at(larger) - g.at(*level);
            if !next.is_finite() || next > avail {
                break;
            }
            total = next;
            *level = larger;
        }
    }
    levels
}

/// Whether the row at `levels` is wider than `avail`: the case where even the smallest sizes do not fit and the row scrolls.
pub fn overflows(groups: &[Widths], levels: &[Level], avail: f32) -> bool {
    groups.iter().zip(levels).map(|(g, l)| g.at(*l)).sum::<f32>() > avail + 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(full: f32, compact: f32, folded: f32) -> Widths {
        Widths { full, compact, folded }
    }

    #[test]
    fn everything_stays_full_when_it_fits() {
        let groups = [w(100.0, 60.0, 40.0), w(200.0, 120.0, 40.0)];
        assert_eq!(plan(&groups, 300.0), vec![Level::Full; 2]);
    }

    #[test]
    fn groups_shrink_from_the_right_first() {
        let groups = [w(100.0, 60.0, 40.0), w(100.0, 60.0, 40.0), w(100.0, 60.0, 40.0)];
        // 300 needs to lose 60 to fit 240: the last group goes compact (-40), then the middle (-40).
        assert_eq!(plan(&groups, 240.0), vec![Level::Full, Level::Compact, Level::Compact]);
        // Under all compact (180), the groups fold from the right.
        let levels = plan(&groups, 150.0);
        assert_eq!(levels.last(), Some(&Level::Folded));
        assert!(!overflows(&groups, &levels, 150.0));
    }

    #[test]
    fn room_is_given_back_when_a_fold_made_more_than_needed() {
        let groups = [w(100.0, 90.0, 40.0), w(300.0, 250.0, 40.0)];
        // Folding the wide last group frees 260; the first group fits full again.
        assert_eq!(plan(&groups, 150.0), vec![Level::Full, Level::Folded]);
    }

    #[test]
    fn a_row_that_cannot_fit_is_reported() {
        let groups = [w(100.0, 60.0, 50.0), w(100.0, 60.0, 50.0)];
        let levels = plan(&groups, 60.0);
        assert_eq!(levels, vec![Level::Folded, Level::Folded]);
        assert!(overflows(&groups, &levels, 60.0));
    }

    #[test]
    fn hostile_widths_do_not_panic() {
        let groups = [w(f32::NAN, f32::INFINITY, -1.0), w(f32::MAX, f32::MAX, f32::MAX)];
        let _ = plan(&groups, f32::NAN);
        let _ = plan(&[], 10.0);
    }
}
