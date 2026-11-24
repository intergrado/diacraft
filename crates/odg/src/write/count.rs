//! Names whose every comparison is counted, so that a probe of a set or a map is measured by the **units it walks** and not by the calls made to
//! it (finding L9: tests that counted the calls into the sets saw the same number for a probe that searched the whole set each time,
//! which is the square of the entries).
//!
//! A [`Counted`] is a name that is compared by its text, adding one to a count each time it is compared with another. A set or a map keyed by
//! them compares a key about once when it is looked up (the hash finds the place, and the comparison confirms the key), so a probe of the
//! kind the saves make costs a unit or so; a probe that walks the keys one by one costs a unit for each key it passes, and the count shows it
//! at once. The count is a thread's own (a save runs on one thread), and is read as a difference ([`compared`]) around the loop it measures.

use std::cell::Cell;
use std::hash::{Hash, Hasher};

thread_local! {
    static COMPARED: Cell<u64> = const { Cell::new(0) };
}

/// A name compared by its text, every comparison counted.
#[derive(Clone, Copy, Debug, Eq)]
pub(crate) struct Counted<S>(pub S);

impl<S: PartialEq> PartialEq for Counted<S> {
    fn eq(&self, other: &Self) -> bool {
        COMPARED.with(|c| c.set(c.get().saturating_add(1)));
        self.0 == other.0
    }
}

impl<S: Hash> Hash for Counted<S> {
    /// The hash of the name itself, so that equal names are found where they are put.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// How many comparisons of [`Counted`] names this thread has made so far.
pub(crate) fn compared() -> u64 {
    COMPARED.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_comparison_of_names_is_counted_and_a_lookup_in_a_set_costs_about_one() {
        let before = compared();
        assert!(Counted("a") == Counted("a") && Counted("a") != Counted("b"));
        assert_eq!(compared() - before, 2);
        let set: HashSet<Counted<String>> = (0..1_000).map(|k| Counted(format!("n{k}"))).collect();
        let before = compared();
        for k in 0..1_000 {
            assert!(set.contains(&Counted(format!("n{k}"))));
        }
        let looked = compared() - before;
        assert!((1_000..=1_100).contains(&looked), "{looked} comparisons for 1,000 lookups of names the set holds");
        // A walk through the keys, which is what a linear search is, passes each one.
        let before = compared();
        for k in 0..100 {
            let wanted = Counted(format!("n{k}"));
            assert!(set.iter().any(|name| *name == wanted));
        }
        assert!(compared() - before >= 100, "the walk is counted as it passes the keys");
    }
}
