//! `SharedVec`: the persistent list that holds a page's shapes, so that editing one shape copies a few
//! hundred reference counts and not the page (task 4's "10,000 shapes" case).

use std::sync::Arc;

use diacraft_doc::{CHUNK, SharedVec};

fn list(n: usize) -> SharedVec<Arc<usize>> {
    (0..n).map(Arc::new).collect()
}

#[test]
fn it_holds_what_it_was_given_in_order() {
    for n in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 5] {
        let l = list(n);
        assert_eq!(l.len(), n);
        assert_eq!(l.is_empty(), n == 0);
        assert!(l.iter().map(|a| **a).eq(0..n), "{n}");
        assert_eq!(l.get(n).map(|a| **a), None);
        if n > 0 {
            assert_eq!(l.get(n - 1).map(|a| **a), Some(n - 1));
            assert_eq!(l.get(0).map(|a| **a), Some(0));
        }
    }
}

#[test]
fn the_layout_is_canonical_so_equal_content_is_equal_lists() {
    // Built by pushes, by collecting, and by inserts then removes: one shape of list, so `==` is content equality.
    let a = list(CHUNK * 2 + 3);
    let mut b = SharedVec::new();
    for i in 0..CHUNK * 2 + 3 {
        b.push(Arc::new(i));
    }
    let mut c = list(CHUNK * 2 + 4);
    c.remove(CHUNK * 2 + 3);
    let mut d = list(CHUNK * 2 + 2);
    d.insert(CHUNK * 2 + 2, Arc::new(CHUNK * 2 + 2)).unwrap();
    d.push(Arc::new(CHUNK * 2 + 3));
    d.remove(d.len() - 1);
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a, d);
}

#[test]
fn a_clone_shares_every_chunk_and_an_edit_copies_one() {
    let n = CHUNK * 10 + 7;
    let before = list(n);
    let mut after = before.clone();
    assert_eq!(after.shared_chunks_with(&before), before.chunk_count(), "a clone shares all of it");

    // Replace one element: only its chunk is new; the rest is shared.
    let at = CHUNK * 4 + 3;
    *after.get_mut(at).unwrap() = Arc::new(usize::MAX);
    assert_eq!(after.shared_chunks_with(&before), before.chunk_count() - 1);
    assert_eq!(after.get(at).map(|a| **a), Some(usize::MAX));
    assert_eq!(before.get(at).map(|a| **a), Some(at), "the older list is untouched");
    // The elements around it are the very same Arcs.
    assert!(Arc::ptr_eq(after.get(at - 1).unwrap(), before.get(at - 1).unwrap()));
    assert!(Arc::ptr_eq(after.get(at + 1).unwrap(), before.get(at + 1).unwrap()));
}

/// `chunk_arcs` gives the chunks by the pointers they are shared by, so that what two lists hold between them can be counted by pointer (the `.odg` save's count of
/// what the engine holds beside the drawing): a clone has the very chunks, an edit has one chunk of its own, and the pointers are as many as `chunk_count`.
#[test]
fn the_chunks_are_given_by_the_pointers_they_are_shared_by() {
    let before = list(CHUNK * 5 + 3);
    assert_eq!(before.chunk_arcs().len(), before.chunk_count());
    assert!(SharedVec::<Arc<usize>>::new().chunk_arcs().is_empty());
    let mut after = before.clone();
    assert!(before.chunk_arcs().iter().zip(after.chunk_arcs()).all(|(a, b)| Arc::ptr_eq(a, b)), "a clone has the very chunks");
    *after.get_mut(CHUNK * 2 + 1).unwrap() = Arc::new(usize::MAX);
    let same: Vec<bool> = before.chunk_arcs().iter().zip(after.chunk_arcs()).map(|(a, b)| Arc::ptr_eq(a, b)).collect();
    assert_eq!(same, [true, true, false, true, true, true], "an edit has one chunk of its own");
    assert_eq!(same.iter().filter(|s| **s).count(), after.shared_chunks_with(&before));
}

#[test]
fn a_clone_is_one_pointer_and_stays_the_same_list_until_one_is_written() {
    let before = list(CHUNK * 20);
    let mut after = before.clone();
    assert!(after.is_same_list_as(&before));
    after.get_mut(CHUNK * 3).unwrap();
    assert!(!after.is_same_list_as(&before), "a write copies the list of chunk pointers");
    assert!(before.is_same_list_as(&before.clone()));
    // Past the end is nothing and copies nothing.
    let mut third = before.clone();
    assert!(third.get_mut(CHUNK * 20).is_none());
    assert!(third.is_same_list_as(&before));
    // Empty lists are one another, and cost no allocation.
    assert!(SharedVec::<u8>::new().is_same_list_as(&SharedVec::<u8>::new()));
    assert!(!SharedVec::<u8>::new().is_same_list_as(&SharedVec::from_vec(vec![1])));
}

#[test]
fn push_copies_only_the_last_chunk() {
    let before = list(CHUNK * 3 + 1);
    let mut after = before.clone();
    after.push(Arc::new(0));
    assert_eq!(after.shared_chunks_with(&before), before.chunk_count() - 1);
    // A push that opens a new chunk copies none.
    let before = list(CHUNK * 3);
    let mut after = before.clone();
    after.push(Arc::new(0));
    assert_eq!(after.shared_chunks_with(&before), before.chunk_count());
    assert_eq!(after.chunk_count(), before.chunk_count() + 1);
}

#[test]
fn insert_and_remove_keep_the_chunks_before_the_place_shared() {
    let before = list(CHUNK * 6);
    let mut after = before.clone();
    after.insert(CHUNK * 4 + 2, Arc::new(1_000)).unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.get(CHUNK * 4 + 2).map(|a| **a), Some(1_000));
    assert_eq!(after.get(CHUNK * 4 + 3).map(|a| **a), Some(CHUNK * 4 + 2));
    assert!(after.shared_chunks_with(&before) >= 4, "the four chunks before it are shared");

    let removed = after.remove(CHUNK * 4 + 2);
    assert_eq!(removed.map(|a| *a), Some(1_000));
    assert_eq!(after, before);
    assert_eq!(after.remove(after.len()).map(|a| *a), None, "past the end is nothing, not a panic");
    assert!(after.insert(after.len() + 1, Arc::new(0)).is_err(), "a place past the end is refused");
}

#[test]
fn out_of_range_places_are_nothing_and_never_a_panic() {
    let mut l = list(5);
    assert_eq!(l.get(usize::MAX).map(|a| **a), None);
    assert!(l.get_mut(usize::MAX).is_none());
    assert!(l.get_mut(5).is_none());
    assert!(l.remove(usize::MAX).is_none());
    assert!(l.set(5, Arc::new(0)).is_err());
    assert_eq!(l.len(), 5);
}

#[test]
fn set_replaces_and_returns_the_old_element() {
    let mut l = list(CHUNK + 2);
    let old = l.set(CHUNK + 1, Arc::new(77)).unwrap();
    assert_eq!(*old, CHUNK + 1);
    assert_eq!(l.get(CHUNK + 1).map(|a| **a), Some(77));
}

#[test]
fn iteration_is_double_ended_and_exact() {
    let l = list(CHUNK * 2 + 9);
    let mut it = l.iter();
    assert_eq!(it.len(), CHUNK * 2 + 9);
    assert_eq!(it.next().map(|a| **a), Some(0));
    assert_eq!(it.next_back().map(|a| **a), Some(CHUNK * 2 + 8));
    assert_eq!(it.len(), CHUNK * 2 + 7);
    assert_eq!(l.iter().rev().map(|a| **a).collect::<Vec<_>>(), (0..CHUNK * 2 + 9).rev().collect::<Vec<_>>());
}
