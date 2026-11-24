//! What the objects of a read cost the model, in bytes: the prices a [`crate::SheetReader`] charges its byte budget for what it builds, and the
//! prices a format that reads shapes, text and pictures on top of it charges for its own objects.
//!
//! **A price is the bytes the allocator hands out for an object, and never less.** An allocation of `n` bytes is a chunk of `n` and eight more, rounded up to
//! sixteen and never under thirty-two ([`chunk`], glibc's); an `Arc<str>` is its two counts and its bytes ([`arc_str`]); a list that grows by pushes holds room
//! for four at the first and twice as much each time it is full ([`push_bytes`]), and a list made whole from a sized source holds what it has. The sizes of the
//! model's types are read from the types ([`std::mem::size_of`]), so a field the model gains raises the price with it and no figure here is a guess that a change
//! of the model leaves behind. What cannot be read from a type is measured, and each measure is held by `crates/vsdx/tests/it/memory_price.rs` (the bytes charged
//! against the growth of a process in which the read was made, one kind of file in a process of its own).
//!
//! **A string and a buffer are priced at what the memory tests hold an allocation to** ([`block`]): the bytes asked for and 24 more, the most
//! glibc adds to one, where [`chunk`] is what glibc adds to that one request (8 to 23). `crates/vsdx/tests/it/memory_counted.rs` counts what a read keeps, byte by
//! byte and allocation by allocation, and holds the charge to the count with 24 bytes for each allocation (the law, which no allocator's own
//! rounding is under: it is the bound and not glibc's figure). A model made of many small allocations (the names and values of a kept tree, the strings of a held
//! row, a connect's two cell names) was priced at glibc's chunk to the byte, and the margin of an eighth on it was a few bytes an allocation: a connect was charged
//! 234 bytes where the law asks 240, a kept element of an attribute and a child 558 where it asks 616. **Text is so charged at more than nine eighths of its length**
//! The margin of an eighth is on a price that holds the text, so a kind made mostly of long text (a formula of 10 KiB, a text of 64 KiB, a
//! kept text of 1 MiB) meets the law without a change here, where the whole-model codec charged text by its length and fell short of it.
//!
//! **A map of cells is priced at its worst.** The standard library's `BTreeMap` makes its first leaf at the first entry, a leaf of eleven slots (1,168 bytes with
//! the cells in it), and a map built one insertion at a time in an order a file chooses has leaves between five and eleven entries full ([`cells_map_bytes`]): the
//! price of a place of `n` cells is the bytes of the nodes it has at five a leaf, so that a place the file orders against the map costs no more than it was charged.

use std::mem::size_of;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{CellExtra, Placed, Row, Section, Sheet};

use crate::model::cost::{CELLS_INNER_BYTES, CELLS_LEAF_BYTES, chunk, push_bytes};

/// What an allocation of `n` bytes is priced at: **the bytes asked for and 24 more, rounded up to sixteen and never under thirty-two**. The 24 are
/// the most glibc adds to an allocation of eight bytes or more (it hands out [`chunk`], `n` and 8 rounded up to 16), and the allowance the memory tests give each
/// allocation a read keeps (`crates/vsdx/tests/it/memory_counted.rs`, the law): a price of `chunk` is under it by 16 for most `n`, which an
/// eighth of margin covers only on an allocation of 128 bytes or more.
#[must_use]
pub const fn block(n: usize) -> u64 {
    let b = n.saturating_add(24).div_ceil(16).saturating_mul(16);
    (if b < 32 { 32 } else { b }) as u64
}

/// The bytes of an `Arc<str>` of `len` bytes: the two counts and the text, as one allocation ([`block`]).
#[must_use]
pub const fn arc_str(len: usize) -> u64 {
    block(16usize.saturating_add(len))
}

/// The bytes of the allocation behind a `String` or a `Vec<u8>` of `len` bytes ([`block`]).
#[must_use]
pub const fn heap(len: usize) -> u64 {
    block(len)
}

/// The bytes the model holds for each node of a parsed formula: the formula crate's own figure (72 bytes for the arguments of a call, a node of a sum of
/// numbers 25), taken at the dearest and rounded to what a tree of names costs. The reader of a drawing charges the same (`diacraft-odg`).
pub const FORMULA_NODE_BYTES: u64 = 128;

/// The margin a charge for something the model holds is given, in eighths of the price, over the price itself (**one eighth**): the prices are the bytes the
/// allocator hands out for each object and the ones measured are at most 2 percent under the growth of a process in the dearest kinds of file, and the allocator
/// holds what it has freed beside what it hands out again (a tree that is dropped leaves holes the next part's model fills in the order it likes). The margin is
/// not on what is counted from the thing itself, a tree or a picture ([`crate::SheetReader::take_exact`]).
pub const MODEL_MARGIN_EIGHTHS: u64 = 1;

/// What a price `n` is charged: the price and an eighth of it ([`MODEL_MARGIN_EIGHTHS`]). What [`crate::SheetReader::take_bytes`] takes, so that a caller that took it
/// and does not keep what it paid for gives back exactly this ([`crate::SheetReader::give_bytes`]).
#[must_use]
pub const fn with_margin(n: u64) -> u64 {
    n.saturating_add(n.div_ceil(8).saturating_mul(MODEL_MARGIN_EIGHTHS))
}

/// What a node of a parsed formula is charged, with the margin: the most nodes a byte budget can pay for is its bytes over this.
#[must_use]
pub const fn formula_node_bytes() -> u64 {
    FORMULA_NODE_BYTES + FORMULA_NODE_BYTES.div_ceil(8) * MODEL_MARGIN_EIGHTHS
}

/// An `Arc<Formula>`: the two counts and the formula (its source, an `Arc<str>` held beside, and its tree, whose nodes are [`FORMULA_NODE_BYTES`] each).
pub const FORMULA_BYTES: u64 = chunk(16 + size_of::<diacraft_formula::Formula>()) as u64;

/// An `Arc<Row>`: the counts and the row. Its place in the list of its section is [`push_bytes`] of [`POINTER_BYTES`].
pub const ROW_BYTES: u64 = chunk(16 + size_of::<Row>()) as u64;

/// An `Arc<Section>`: the counts and the section. Its place in its sheet's list is [`push_bytes`] of [`POINTER_BYTES`].
pub const SECTION_BYTES: u64 = chunk(16 + size_of::<Section>()) as u64;

/// An `Arc<Sheet>`: the counts and the sheet.
pub const SHEET_BYTES: u64 = chunk(16 + size_of::<Sheet>()) as u64;

/// An `Arc<CellExtra>`: the counts and what a cell holds that few have.
pub const CELL_EXTRA_BYTES: u64 = chunk(16 + size_of::<CellExtra>()) as u64;

/// A reference to a row, a section or a shape in the list that holds it.
pub const POINTER_BYTES: usize = size_of::<Arc<Row>>();

/// A kept node with its place, as the list of one element's kept nodes holds it.
pub const PLACED_BYTES: usize = size_of::<Placed>();

/// A node of kept markup, as the list of children of a kept element holds it.
pub const RAW_NODE_BYTES: usize = size_of::<diacraft_doc::RawNode>();

/// An attribute of kept markup, a pair of names, as the list of one element's attributes holds it.
pub const PAIR_BYTES: usize = size_of::<(Arc<str>, Arc<str>)>();

/// The entry a string that repeats in every file (a unit, an error word, a row type) has in the table of such strings: its key, the table's slot at its
/// least full, and the pointer it holds. At most 4,096 are made.
pub const INTERN_ENTRY_BYTES: u64 = 160;

/// What a name noted in a set costs while a sheet or a section is read: the bucket at its least full, the `String`, and the text beside it ([`heap`]).
pub const SEEN_ENTRY_BYTES: u64 = 64;

/// The bytes of the nodes of a map of `n` cells, **at the worst a file can make them**: the first leaf holds eleven, and past eleven every node but the root
/// holds at least five entries (and an inner node at least six children), whatever order the cells were put in. A leaf is [`CELLS_LEAF_BYTES`] and an inner
/// node [`CELLS_INNER_BYTES`].
#[must_use]
pub fn cells_map_bytes(n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    if n <= 11 {
        return CELLS_LEAF_BYTES;
    }
    let leaves = n.div_ceil(5);
    let mut total = (leaves as u64).saturating_mul(CELLS_LEAF_BYTES);
    let mut level = leaves;
    while level > 1 {
        level = level.div_ceil(6);
        total = total.saturating_add((level as u64).saturating_mul(CELLS_INNER_BYTES));
    }
    total
}

/// What putting one more cell in a place that holds `have` costs in nodes: the first leaf at the first cell, a split (a leaf and an inner node) at the twelfth,
/// and a leaf and its share of the inner nodes for each five after it, at the worst.
#[must_use]
pub fn cell_node_bytes(have: usize) -> u64 {
    cells_map_bytes(have.saturating_add(1)).saturating_sub(cells_map_bytes(have))
}

/// The bytes one more unknown attribute costs a list of `have` (the model's `RawAttrs`): the growth of the list, and the two names.
#[must_use]
pub fn raw_attr_bytes(have: usize, name: usize, value: usize) -> u64 {
    push_bytes(have, PAIR_BYTES).saturating_add(arc_str(name)).saturating_add(arc_str(value))
}

/// What a text, a comment or an instruction costs inside a kept element, beyond the slot it has in the list of its parent's children.
fn node_bytes(node: &Node) -> u64 {
    match node {
        Node::Text(t) | Node::CData(t) | Node::Comment(t) => arc_str(t.len()),
        Node::Pi { target, data } => arc_str(target.len()).saturating_add(arc_str(data.len())),
        Node::El(_) => 0,
    }
}

/// What the model holds when it keeps an element whole (`raw_element`): its name and attributes, its list of children, and everything in it, all the way down,
/// **beyond the slot the element has in the list that holds it** (a [`PLACED_BYTES`] entry of a passthrough, charged by [`push_bytes`] of the list's length, or
/// a [`RAW_NODE_BYTES`] entry of its parent's children, which the parent's list counts). The lists are made whole from sized sources and hold what they have.
/// The walk keeps its own stack and does not recurse.
#[must_use]
pub fn kept_bytes(el: &El) -> u64 {
    let mut total = 0u64;
    let mut stack: Vec<&El> = vec![el];
    while let Some(e) = stack.pop() {
        total = total.saturating_add(arc_str(e.name.len()));
        if !e.attrs.is_empty() {
            // The `Arc` and the list it holds, and each pair and its two names.
            total = total.saturating_add(block(16 + 24)).saturating_add(heap(e.attrs.len().saturating_mul(PAIR_BYTES)));
            for (k, v) in &e.attrs {
                total = total.saturating_add(arc_str(k.len())).saturating_add(arc_str(v.len()));
            }
        }
        if !e.children.is_empty() {
            total = total.saturating_add(heap(e.children.len().saturating_mul(RAW_NODE_BYTES)));
            for node in &e.children {
                match node {
                    Node::El(child) => stack.push(child),
                    other => total = total.saturating_add(node_bytes(other)),
                }
            }
        }
    }
    total
}

/// What keeping one text, comment or instruction node costs in a passthrough that holds `have`: its place in the list, and what it holds.
#[must_use]
pub fn kept_other_bytes(have: usize, node: &Node) -> u64 {
    push_bytes(have, PLACED_BYTES).saturating_add(node_bytes(node))
}

/// What keeping an element whole costs in a passthrough that holds `have`: its place in the list, and [`kept_bytes`].
#[must_use]
pub fn kept_element_bytes(have: usize, el: &El) -> u64 {
    push_bytes(have, PLACED_BYTES).saturating_add(kept_bytes(el))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
        El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
    }

    #[test]
    fn an_allocation_is_its_bytes_and_twenty_four_more_rounded_up_to_sixteen_and_never_under_thirty_two() {
        assert_eq!((heap(0), heap(1), heap(8), heap(9), heap(24), heap(25), heap(40), heap(41), heap(1160)), (32, 32, 32, 48, 48, 64, 64, 80, 1184));
        // An `Arc<str>` holds its two counts in front of the text.
        assert_eq!((arc_str(0), arc_str(8), arc_str(9), arc_str(1_000_000)), (48, 48, 64, 1_000_048));
        // Sixteen above glibc's chunk, the request and 8 rounded up to 16, for every request past what the least chunk holds: the law's 24 bytes an allocation.
        for n in 9..2_000 {
            assert_eq!(block(n), chunk(n) as u64 + 16, "{n}");
            assert!(block(n) >= n as u64 + 24, "{n}");
        }
        // A length that cannot be held does not wrap.
        assert!(arc_str(usize::MAX) > 0 && heap(usize::MAX) > 0);
    }

    #[test]
    fn a_map_of_cells_is_one_leaf_to_eleven_then_five_a_leaf_with_an_inner_node_over_every_six() {
        assert_eq!(cells_map_bytes(0), 0);
        for n in 1..=11 {
            assert_eq!(cells_map_bytes(n), CELLS_LEAF_BYTES, "{n}");
        }
        // 12 to 15 cells: three leaves at the worst, and the node over them.
        for n in 12..=15 {
            assert_eq!(cells_map_bytes(n), 3 * CELLS_LEAF_BYTES + CELLS_INNER_BYTES, "{n}");
        }
        assert_eq!(cells_map_bytes(16), 4 * CELLS_LEAF_BYTES + CELLS_INNER_BYTES);
        // Seven leaves need two inner nodes and a root over them.
        assert_eq!(cells_map_bytes(31), 7 * CELLS_LEAF_BYTES + 2 * CELLS_INNER_BYTES + CELLS_INNER_BYTES);
        // What a cell adds is the difference, and the differences sum to the whole.
        assert_eq!((cell_node_bytes(0), cell_node_bytes(1), cell_node_bytes(10)), (CELLS_LEAF_BYTES, 0, 0));
        assert_eq!(cell_node_bytes(11), 2 * CELLS_LEAF_BYTES + CELLS_INNER_BYTES);
        assert_eq!((0..100).map(cell_node_bytes).sum::<u64>(), cells_map_bytes(100));
        // A count that cannot be held does not wrap.
        assert!(cells_map_bytes(usize::MAX) > 0);
    }

    #[test]
    fn an_attribute_kept_is_the_growth_of_the_list_and_the_two_names() {
        assert_eq!(raw_attr_bytes(0, 1, 2), push_bytes(0, PAIR_BYTES) + arc_str(1) + arc_str(2));
        // Room for four at the first, and twice as much at the fifth.
        assert_eq!(push_bytes(3, PAIR_BYTES), 0);
        assert!(raw_attr_bytes(4, 1, 1) > raw_attr_bytes(3, 1, 1));
    }

    #[test]
    fn what_keeping_an_element_costs_is_its_name_its_attributes_its_list_of_children_and_everything_in_it() {
        // `<a/>`: its name, and nothing else (its slot is the list's).
        assert_eq!(kept_bytes(&el("a", &[], vec![])), arc_str(1));
        // One attribute: the `Arc` and the list made whole for it, and each of its two names.
        let attr = block(16 + 24) + heap(PAIR_BYTES) + arc_str(1) + arc_str(2);
        assert_eq!(kept_bytes(&el("a", &[("k", "vv")], vec![])), arc_str(1) + attr);
        // Children: the list made whole for them, and what each holds (a text, a comment, an instruction).
        let kids = vec![Node::Text("abc".into()), Node::Comment("c".into()), Node::Pi { target: "t".into(), data: "dd".into() }];
        let inside = arc_str(3) + arc_str(1) + arc_str(1) + arc_str(2);
        assert_eq!(kept_bytes(&el("a", &[], kids)), arc_str(1) + heap(3 * RAW_NODE_BYTES) + inside);
        // An element in an element is kept in the same walk, and so is one that is deep.
        let deep = el("a", &[], vec![Node::El(el("bb", &[], vec![Node::El(el("c", &[], vec![]))]))]);
        assert_eq!(kept_bytes(&deep), arc_str(1) + heap(RAW_NODE_BYTES) + arc_str(2) + heap(RAW_NODE_BYTES) + arc_str(1));
    }

    #[test]
    fn the_place_of_a_kept_node_is_the_growth_of_the_list_that_holds_it() {
        let a = el("a", &[], vec![]);
        for have in [0usize, 1, 3, 4, 8] {
            assert_eq!(kept_element_bytes(have, &a), push_bytes(have, PLACED_BYTES) + kept_bytes(&a), "{have}");
            assert_eq!(kept_other_bytes(have, &Node::Text("xy".into())), push_bytes(have, PLACED_BYTES) + arc_str(2), "{have}");
        }
        assert_eq!(kept_other_bytes(0, &Node::CData("xy".into())), kept_other_bytes(0, &Node::Text("xy".into())));
        assert_eq!(kept_other_bytes(0, &Node::Comment("xy".into())), kept_other_bytes(0, &Node::Text("xy".into())));
        assert_eq!(kept_other_bytes(0, &Node::Pi { target: "a".into(), data: "b".into() }), push_bytes(0, PLACED_BYTES) + 2 * arc_str(1));
    }

    #[test]
    fn a_formula_node_is_charged_with_the_margin_in_the_byte_budget_and_nothing_wraps() {
        assert_eq!(formula_node_bytes(), FORMULA_NODE_BYTES + FORMULA_NODE_BYTES / 8 * MODEL_MARGIN_EIGHTHS);
        assert_eq!(formula_node_bytes(), 144);
    }

    #[test]
    fn a_very_deep_element_is_walked_without_recursion() {
        let mut node = el("z", &[], vec![]);
        for _ in 0..100_000 {
            node = el("z", &[], vec![Node::El(node)]);
        }
        assert_eq!(kept_bytes(&node), 100_001 * arc_str(1) + 100_000 * heap(RAW_NODE_BYTES));
        // Taking the tree apart without recursion too, as the container's own does not: leave the leak of the walk to the test's end.
        std::mem::forget(node);
    }
}
