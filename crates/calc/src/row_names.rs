//! A row named in another letter case.
//!
//! **The rule.** A row's name in a reference matches without regard to letter case. Where more than one row of a section would match, the
//! exact spelling wins, then the lower `IX` (a tie-break with no sample, as no file holds two rows that differ only in case); a row of a nearer
//! sheet (the shape, then its master's shape, then the styles) comes before one of a farther sheet, as it does in any lookup.
//! A cell's name inside the row still matches exactly.
//!
//! **How it is found.** The exact lookup is the resolver's and is tried first, on the sheet itself, for it is the common case. Only when that finds
//! nothing is the section's index built: every spelling its rows have, and for each name folded to lower case the row that wins, listed once for a
//! sheet and a section and kept (`Evaluator::respell_row`). A lookup is then a hash of the name and no scan of the rows, however many a hostile
//! section holds. The listing is charged by the rows and bytes it walks, as it is built.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::Section;

/// Where a row stands among the rows that would match: the sheet it is on (0 is the nearest), its `IX` or else its place in the section, and its
/// place. Lower wins.
type Rank = (usize, u64, usize);

/// The names of the rows of one section, as a sheet sees them through its master and styles.
#[derive(Debug, Default)]
pub(crate) struct RowNames {
    /// Every spelling a row of the section has.
    exact: HashSet<Arc<str>>,
    /// For each name folded to lower case, the spelling of the row that wins.
    best: HashMap<Box<str>, (Rank, Arc<str>)>,
}

/// What the rows of a section make of a name a reference spells.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Spelled {
    /// A row has exactly this name: the reference is right as it stands.
    Exact,
    /// No row has this name, and a row in another letter case does: the reference names that row.
    As(Arc<str>),
    /// No row has this name in any letter case.
    Nowhere,
}

/// A name folded for a match without regard to letter case.
pub(crate) fn fold(name: &str) -> String {
    name.to_lowercase()
}

impl RowNames {
    /// Lists the named rows of `layers`, nearest first. `charge` is told the bytes of each name before it is listed and says whether to go on: a
    /// refused charge ends the listing, which holds what it reached. A row marked deleted (`Del`) is not a name.
    pub(crate) fn build(layers: &[&Arc<Section>], charge: &mut dyn FnMut(usize) -> bool) -> RowNames {
        let mut names = RowNames::default();
        'layers: for (depth, section) in layers.iter().enumerate() {
            for (place, row) in section.rows.iter().enumerate() {
                let Some(name) = &row.name else { continue };
                if row.del {
                    continue;
                }
                if !charge(name.len()) {
                    break 'layers;
                }
                let rank = (depth, row.ix.map_or(place as u64, u64::from), place);
                names.exact.insert(Arc::clone(name));
                let folded = fold(name).into_boxed_str();
                match names.best.get_mut(&folded) {
                    Some(held) if held.0 <= rank => {}
                    Some(held) => *held = (rank, Arc::clone(name)),
                    None => {
                        names.best.insert(folded, (rank, Arc::clone(name)));
                    }
                }
            }
        }
        names
    }

    /// What the rows make of `name`, whose folded form is `folded`.
    pub(crate) fn spell(&self, name: &str, folded: &str) -> Spelled {
        if self.exact.contains(name) {
            return Spelled::Exact;
        }
        match self.best.get(folded) {
            Some((_, spelling)) => Spelled::As(Arc::clone(spelling)),
            None => Spelled::Nowhere,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use diacraft_doc::{Row, Section};

    use super::*;

    fn section(rows: &[(&str, Option<u32>)]) -> Arc<Section> {
        let mut s = Section::new("User", None);
        for (name, ix) in rows {
            s.rows.push(Arc::new(Row { name: Some(Arc::from(*name)), ix: *ix, ..Row::default() }));
        }
        Arc::new(s)
    }

    fn names(layers: &[&Arc<Section>]) -> RowNames {
        RowNames::build(layers, &mut |_| true)
    }

    fn spell(n: &RowNames, name: &str) -> Spelled {
        n.spell(name, &fold(name))
    }

    #[test]
    fn the_exact_spelling_is_right_as_it_stands_and_another_case_is_respelled() {
        let a = section(&[("Tally9", None)]);
        let n = names(&[&a]);
        assert_eq!(spell(&n, "Tally9"), Spelled::Exact);
        assert_eq!(spell(&n, "TALLY9"), Spelled::As(Arc::from("Tally9")));
        assert_eq!(spell(&n, "tally9"), Spelled::As(Arc::from("Tally9")));
        assert_eq!(spell(&n, "Tally8"), Spelled::Nowhere);
    }

    #[test]
    fn the_lower_ix_wins_and_then_the_earlier_place_and_the_exact_spelling_beats_both() {
        // `TALLY9` comes first in the file but has the higher `IX`.
        let a = section(&[("TALLY9", Some(5)), ("Tally9", Some(2))]);
        let n = names(&[&a]);
        assert_eq!(spell(&n, "tally9"), Spelled::As(Arc::from("Tally9")));
        assert_eq!(spell(&n, "TALLY9"), Spelled::Exact);
        // With no `IX` the place decides.
        let b = section(&[("TALLY9", None), ("Tally9", None)]);
        assert_eq!(spell(&names(&[&b]), "tally9"), Spelled::As(Arc::from("TALLY9")));
        // An `IX` is a number among places: the first row has none (place 0) and the second has 0 too: the earlier place wins the tie.
        let c = section(&[("Aa", None), ("AA", Some(0))]);
        assert_eq!(spell(&names(&[&c]), "aa"), Spelled::As(Arc::from("Aa")));
    }

    #[test]
    fn a_nearer_sheet_comes_before_a_lower_ix_on_a_farther_one() {
        let near = section(&[("TALLY9", Some(9))]);
        let far = section(&[("Tally9", Some(0))]);
        let n = names(&[&near, &far]);
        assert_eq!(spell(&n, "tally9"), Spelled::As(Arc::from("TALLY9")));
        // The exact spelling is found on any sheet.
        assert_eq!(spell(&n, "Tally9"), Spelled::Exact);
    }

    #[test]
    fn a_deleted_row_and_a_row_with_no_name_are_no_names() {
        let mut s = Section::new("User", None);
        s.rows.push(Arc::new(Row { name: Some(Arc::from("Gone")), del: true, ..Row::default() }));
        s.rows.push(Arc::new(Row { ix: Some(1), ..Row::default() }));
        let n = names(&[&Arc::new(s)]);
        assert_eq!(spell(&n, "gone"), Spelled::Nowhere);
        assert_eq!(spell(&n, "Gone"), Spelled::Nowhere);
    }

    #[test]
    fn a_refused_charge_ends_the_listing_with_what_it_reached() {
        let a = section(&[("One", None), ("Two", None), ("Three", None)]);
        let mut left = 2;
        let n = RowNames::build(&[&a], &mut |_| {
            left -= 1;
            left >= 0
        });
        assert_eq!(spell(&n, "ONE"), Spelled::As(Arc::from("One")));
        assert_eq!(spell(&n, "THREE"), Spelled::Nowhere);
    }

    #[test]
    fn names_beyond_ascii_fold_too() {
        let a = section(&[("Größe", None)]);
        assert_eq!(spell(&names(&[&a]), "GRÖSSE"), Spelled::Nowhere, "a letter that folds to two is not this rule's");
        assert_eq!(spell(&names(&[&a]), "GRÖßE"), Spelled::As(Arc::from("Größe")));
    }
}
