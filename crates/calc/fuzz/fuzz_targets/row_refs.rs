//! `cargo fuzz run row_refs`: the references to a section's rows in formula text, found and renumbered. The input is any bytes: the first picks
//! the shape asked for and the way the rows move, the rest is read as text by lossy UTF-8 (a byte that is no character becomes U+FFFD, so the fuzzer reaches every
//! text and the properties stay about text).
//!
//! **`find`** never panics, and charges **exactly the bytes of the text**: every byte is passed over once, whatever the text holds. Its references are in the
//! order they stand, do not overlap, and have spans on character boundaries; a row number that does not fit is the one error it gives.
//!
//! **`renumber`**, with a map that names every row as it is (or names none), gives `None`: not one byte changes. With the map that raises each row's number by
//! one, a text that holds no number past the largest comes out with the **same number of references**, each of the shape asked for one higher and every other
//! unchanged, and **every byte outside the tails of the references it moved is the byte of the text**. The bytes it writes are charged, and a budget too small for
//! them is the error in words and gives no text.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); nothing here needs a corpus, and none is committed.
#![no_main]

use diacraft_calc::row_refs::{CONNECTIONS, RowName, RowRef, RowRefError, RowTarget, find, renumber};
use libfuzzer_sys::fuzz_target;

/// The row of a bare reference is row 1, which is what a map is given for it.
fn as_given(row: &RowName) -> RowName {
    match row {
        RowName::Default => RowName::Number(1),
        other => other.clone(),
    }
}

/// Each number one higher; a name is left.
fn up(row: &RowName) -> Option<RowName> {
    match as_given(row) {
        RowName::Number(n) => n.checked_add(1).map(RowName::Number),
        _ => None,
    }
}

/// Whether a reference is one of the shape asked for.
fn applies(r: &RowRef, of: RowTarget) -> bool {
    match r.sheet {
        None => of.unqualified,
        Some(id) => id == of.sheet,
    }
}

fuzz_target!(|data: &[u8]| {
    let (head, rest) = data.split_first().map_or((0, &[][..]), |(h, r)| (*h, r));
    let text = String::from_utf8_lossy(rest);
    // The shape asked for is shape 5 or 6, as the formula of its own sheet or of another one: the texts the fuzzer starts from name `Sheet.5` and `Sheet.6`.
    let of = RowTarget { sheet: 5 + u32::from(head & 1), unqualified: head & 2 != 0 };

    // `find`: the bytes, once each.
    let budget_for = |n: usize| n.saturating_mul(4).saturating_add(64);
    let mut budget = budget_for(text.len());
    let before = budget;
    let found = match find(&text, CONNECTIONS, &mut budget) {
        Ok(refs) => {
            assert_eq!(before - budget, text.len(), "a scan is charged the bytes it passes over, once each");
            let mut at = 0;
            for r in &refs {
                assert!(r.start >= at && r.start <= r.end, "references in order and apart: {refs:?}");
                assert!(text.get(r.start..r.end).is_some(), "a span on character boundaries");
                at = r.end;
            }
            Some(refs)
        }
        Err(e) => {
            assert!(matches!(e, RowRefError::NumberTooLarge { .. }), "an unexpected error {e}");
            assert!(!e.to_string().is_empty());
            None
        }
    };

    // The map that changes nothing, and the one that names no row: no byte changes.
    let mut budget = budget_for(text.len());
    let same = renumber(&text, CONNECTIONS, of, &|row| Some(as_given(row)), &mut budget);
    assert!(matches!(same, Ok(None) | Err(RowRefError::NumberTooLarge { .. })), "the identity map changed the text: {same:?}");
    let mut budget = budget_for(text.len());
    let none = renumber(&text, CONNECTIONS, of, &|_| None, &mut budget);
    assert!(matches!(none, Ok(None) | Err(RowRefError::NumberTooLarge { .. })), "a map of no row changed the text: {none:?}");

    // A budget that cannot hold the text is refused in words, with nothing written.
    let mut small = text.len().saturating_sub(1);
    if !text.is_empty() {
        assert_eq!(renumber(&text, CONNECTIONS, of, &up, &mut small), Err(RowRefError::BudgetSpent));
        assert_eq!(small, 0);
    }

    // Each number one higher: the same references, the shape's one higher, the rest untouched.
    let mut budget = budget_for(text.len()).saturating_add(text.len().saturating_mul(12));
    match renumber(&text, CONNECTIONS, of, &up, &mut budget) {
        Ok(None) => {}
        Ok(Some(out)) => {
            let (Some(old), Ok(new)) = (found.as_ref(), find(&out, CONNECTIONS, &mut budget_for(out.len()))) else { return };
            assert_eq!(old.len(), new.len(), "a renumbering neither makes nor loses a reference: {text:?} gave {out:?}");
            for (o, n) in old.iter().zip(&new) {
                assert_eq!((o.sheet, &o.cell), (n.sheet, &n.cell));
                if applies(o, of) {
                    // The largest number is not raised: the map gives no row for it.
                    if let (RowName::Number(a), RowName::Number(b)) = (as_given(&o.row), &n.row) {
                        assert_eq!(a.checked_add(1).unwrap_or(a), *b);
                    }
                } else {
                    assert_eq!(o.row, n.row);
                    // The bytes of a reference that was not moved are the bytes it had.
                    assert_eq!(text.get(o.start..o.end), out.get(n.start..n.end));
                }
            }
        }
        Err(e) => assert!(matches!(e, RowRefError::NumberTooLarge { .. } | RowRefError::BudgetSpent), "an unexpected error {e}"),
    }
});
