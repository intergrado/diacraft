//! Places visited, and movement between the rows of a list.

use crate::MAX_HISTORY;

/// A history of places with a cursor: the places a person has been to in a window, and where among them they are. At most [`MAX_HISTORY`] are kept; the oldest is dropped.
#[derive(Clone, Debug)]
pub struct History<T> {
    places: Vec<T>,
    /// The index of the place shown; always below the length when there are places.
    cursor: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History { places: Vec::new(), cursor: 0 }
    }
}

impl<T: Clone + PartialEq> History<T> {
    /// An empty history.
    pub fn new() -> History<T> {
        History::default()
    }

    /// The place shown now.
    pub fn current(&self) -> Option<&T> {
        self.places.get(self.cursor)
    }

    /// How many places are held.
    pub fn len(&self) -> usize {
        self.places.len()
    }

    /// Whether no place is held.
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// Goes to a new place: the places after the one shown are dropped, and the oldest goes when there are too many. Going to the place that is shown is no step. Returns
    /// whether the history changed.
    pub fn push(&mut self, place: T) -> bool {
        if self.current() == Some(&place) {
            return false;
        }
        self.places.truncate(if self.places.is_empty() { 0 } else { self.cursor + 1 });
        self.places.push(place);
        if self.places.len() > MAX_HISTORY {
            self.places.remove(0);
        }
        self.cursor = self.places.len().saturating_sub(1);
        true
    }

    /// Moves back one place, and gives it.
    pub fn back(&mut self) -> Option<&T> {
        if !self.can_back() {
            return None;
        }
        self.cursor -= 1;
        self.current()
    }

    /// Moves forward one place, and gives it.
    pub fn forward(&mut self) -> Option<&T> {
        if !self.can_forward() {
            return None;
        }
        self.cursor += 1;
        self.current()
    }

    /// Whether there is a place before the one shown.
    pub fn can_back(&self) -> bool {
        self.cursor > 0 && !self.places.is_empty()
    }

    /// Whether there is a place after the one shown.
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.places.len()
    }
}

/// The row reached after Down (`down`) or Up from `current` in a list of `rows` rows. `None` is the box above the list. Up from the first row is the box, Down from the last
/// row stays, a row that is gone (the list got shorter) is read as the last one, and the answer is never past the rows.
pub fn next_row(current: Option<usize>, rows: usize, down: bool) -> Option<usize> {
    let last = rows.checked_sub(1)?;
    match (current.map(|c| c.min(last)), down) {
        (None, true) => Some(0),
        (None, false) => None,
        (Some(c), true) => Some((c + 1).min(last)),
        (Some(c), false) => c.checked_sub(1),
    }
}
