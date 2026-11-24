//! The room of a save, for the engine that holds the process's memory (the review's finding on the save meter).
use diacraft_native::{Limits, State, WriteOptions, write};

use crate::util::*;

#[test]
fn the_room_of_a_save_is_what_the_process_leaves_less_what_the_codec_holds_beside_it() {
    // `Limits::for_save(held)`: the process holds `held` bytes, the model of the drawing among them. What is left of the budget less the codec's own room for the part in hand
    // (its tree at the items that fit, its tables, a part's bytes) is the memory of the package, and the tree of a part is held to the items that fit. A process that holds all
    // of it leaves no room, and the save is refused in words before anything is made.
    let free = Limits::for_save(0).unwrap();
    assert!(free.max_memory < Limits::beside(0).max_memory, "the codec's room is taken out");
    assert_eq!(free.container.xml.max_nodes, Limits::default().container.xml.max_nodes);
    let mut last = free.max_memory;
    for held in [1u64 << 28, 1 << 29, 1 << 30, 3 << 29] {
        let limits = Limits::for_save(held).unwrap();
        assert!(limits.max_memory < last, "{held}: less room as more is held");
        assert!(limits.container.xml.max_nodes <= free.container.xml.max_nodes);
        last = limits.max_memory;
    }
    // A model of 1.5 GiB (the most an edit leaves) still has the items of a tree of an open; one of 1.75 GiB leaves fewer.
    assert_eq!(Limits::for_save(3 << 29).unwrap().container.xml.max_nodes, Limits::default().container.xml.max_nodes);
    assert!(
        Limits::for_save(7 << 28).unwrap().container.xml.max_nodes < Limits::default().container.xml.max_nodes,
        "a model of 1.75 GiB leaves fewer items for a tree"
    );
    let error = Limits::for_save(u64::MAX).unwrap_err();
    assert_eq!(error.class(), "too_large");
    assert!(error.to_string().contains("memory"), "{error}");
    // A save made at those limits writes what a save at the defaults writes.
    let doc = drawing_of(vec![diacraft_doc::Page::new(1, "One"), diacraft_doc::Page::new(2, "Two")]);
    let bytes = write_doc(&doc);
    let limits = Limits::for_save(1 << 20).unwrap();
    assert_eq!(write(&doc, &State::default(), None, &WriteOptions { limits, ..WriteOptions::default() }).unwrap().bytes, bytes);
}
