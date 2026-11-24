//! What the model costs, in bytes: the price the decoder charges a [`super::Budget`] for what it builds, so that a file that passes the budget cannot make the
//! process abort at an allocation. An element costs one item and, besides its text, the bytes of the objects it builds: an empty
//! `<shape/>` is 8 bytes of file and builds 338 bytes of model, and a million of them are 322 MB.
//!
//! The figures are **measured** by `tests/it/model_memory.rs`, which decodes each kind of element that can repeat in a part in a process of its own, for an
//! element with only the attributes it must have and for one with all of them, and holds the bytes charged to **what the model keeps, counted** by the test
//! binary's allocator (the bytes asked for and not given back, and for each allocation the most the allocator adds to one), at a count that leaves a vector that
//! grew by doubling at its emptiest (one more than a power of two: room for twice what it holds); and to the growth of the process at its peak, within a margin.
//! They are set a quarter above the largest, and rounded up to 16 (the allocator's step). A field the model gains makes that test fail, and the figure is moved
//! with it. They are not part of the format: a build with another model has other figures, and a decode that passes in one may be refused in the other.
//!
//! **The resident size of the process does not measure the model**. The figures were first set from the growth of the process at its peak, at
//! 20,000 units of a kind: that misses a vector's room that nothing has touched, which is no less held, and it moves with the platform's allocator. The connects
//! of a page (72 bytes each, in a vector) kept 118 bytes each at 20,000 and 144 at 16,385, where the process grew by 52 to 98 and the charge was 96. The count
//! found eight figures short, six by glibc's own count of its bytes in use and two more by the most it may add to an allocation, and three a tenth of a byte
//! above glibc's count; they moved: a connect (96 to 192), a font and an entry of the VSDX records (48 to 80), a relationship (192 to 256), a custom
//! property (144 to 192), a part (112 to 128), a content-type row (128 to 144), a run of a text's paragraph (160 to 256: a run alone in its paragraph keeps the
//! room of four), and the three markers of a text (96 to 128: 48 bytes each in a vector, room for two at the worst).
//!
//! **The law covers the allocator the app ships with**. The desktop app and the command line run on mimalloc, which has no header but rounds a
//! request up to one of four sizes in each doubling (a node of cells of 1,160 bytes is 1,280 there), so the law holds an eighth of the bytes kept besides: what the model
//! keeps, an eighth of it, and 24 for each allocation. Counted to it, twelve figures were short and were raised a quarter above the count: the style (256 to 368), the
//! paragraph of a text's data (272 to 368), a run of text and a field (144 to 208), the part (128 to 192), the relationship (256 to 352), the custom property (192 to 272),
//! the font and the entry of the VSDX records (80 to 112), the wrapped node of the other format (128 to 176), the definition (112 to 160) and the content-type row (144 to
//! 192); and a node of a map of cells, which was glibc's chunk, is at the law ([`law_bytes`]: a leaf 1,344 bytes, an inner node 1,440), which the places of one or two cells
//! fell short by.
//!
//! **Text is charged at nine eighths of its length** ([`text_bytes`], [`super::Budget::charge_text`]). The figures of the elements cover what they
//! build around their text, and the law's eighth of the text itself is the text's: a kind made mostly of long text (a cell of a 10 KiB formula, a run of 64 KiB, a kept
//! text of 1 MiB) was charged about nine tenths of the law however its figure was set. **A formula's tree is charged for the strings it copies** ([`tree_text_bytes`]):
//! a string literal, a name, a GUID are each an allocation of their own beside the source, so a formula of one string of 10 KiB keeps 20 KiB, and was charged 0.455 of
//! the law. The VSDX reader prices the same copies already (its text twice, with its margin of an eighth on every price).
//!
//! **Where an object stands is part of what it costs**. The figures above are for an element in a place of thousands of its kind. A
//! map of cells makes a leaf of eleven slots (1,160 bytes) at its first cell, and a vector room for four at its first push, so a place that holds one thing
//! costs what a place of four does. The decoder charges a map of cells as the nodes it will have ([`cells_bytes`], for a map built from sorted pairs, which is how
//! the decoder builds it) and each list of kept nodes and attributes as the room it makes ([`vec_bytes`], [`push_bytes`]), besides the figure of the element;
//! `tests/it/model_memory.rs` measures each kind alone in its place, with one cell, with two, with eleven, with twelve, in rows and in sections, and a
//! mix of them, in a process of its own, against the bytes it was charged.

use super::vocab::el;

/// What the model keeps of an element of each kind, in bytes, beyond its attributes and its text (which the decoder charges besides): the
/// object the element builds, the allocations it holds, and its share of the vector or map it is put in, with the allocator's rounding and the slack of a
/// vector that grew by doubling. **Measured** (`tests/it/model_memory.rs`: each kind decoded in a process of its own, as what the model keeps,
/// counted by the allocator, and as the growth of the process at its peak before) and set a quarter above the largest of the two cases measured
/// for it, an element with only the attributes it must have and one with all of them, rounded up to 16. The same test holds every kind to it, so a field the model
/// gains makes it fail, and not a file abort the process.
pub fn element_bytes(local: &str) -> u64 {
    match local {
        el::PAGE => 704,
        el::MASTER => 288,
        el::SHAPE => 448,
        el::THEME => 896,
        el::TEXT => 256,
        el::CELL => 80,
        el::SECTION => 288,
        el::ROW => 224,
        el::STYLE => 368,
        el::PARAGRAPH => 368,
        el::KEPT_STYLE => 208,
        el::EXT => 208,
        el::PAGE_REF => 192,
        el::KEPT => 192,
        el::COLOR => 176,
        el::CHARS | el::FLD => 208,
        el::ODF_RAW => 176,
        el::TEXT_STYLE | el::MASTER_REF | el::EXTRA | el::ATTR | el::PROPS | el::VSDX_HELD => 128,
        el::KEPT_COMMENT | el::KEPT_PI => 144,
        el::LEVEL | el::ELEMENT | el::LIST | el::ITEM => 96,
        el::HELD => 112,
        el::DEFINITION => 160,
        el::FOREIGN => 112,
        el::PICTURE | el::ODF_ROOT | el::MANIFEST_ROW | el::HELD_STYLE | el::KEPT_TEXT | el::BLOB => 96,
        el::KEPT_EL => 80,
        // Counted: what the model keeps of each at the count that leaves its vector's room at its emptiest, a quarter above glibc's own count of
        // its bytes in use, and never under the count with the most the allocator may add to each allocation. A connect is 72 bytes in the vector of a page's
        // connects, and keeps room for two at the worst; a run alone in its paragraph keeps room for four. The relationship, the custom property, the content-type
        // row, the part, the font and the entry of the VSDX records were raised again by (the law holds an eighth of the bytes kept besides,
        // for the size classes of the app's allocator), a quarter above that count.
        el::CONNECT => 192,
        el::REL => 352,
        el::RUN => 256,
        el::CUSTOM => 272,
        // A comment: the struct, its place in a chunk of the list and the Arc it is behind; its texts are charged as text.
        el::COMMENT_ENTRY => 352,
        el::COMMENT_AUTHOR => 256,
        el::COMMENT_LIST => 128,
        el::CONTENT_TYPE => 192,
        el::PART => 192,
        el::CP | el::PP | el::TP => 128,
        el::FONT | el::REST => 112,
        el::SHEET | el::ODF | el::LONG => 32,
        _ => 64,
    }
}

/// What an attribute costs the model beyond the bytes of its text: a string of its own (a header, and the allocator's rounding) or a field.
pub const ATTR_BYTES: u64 = 32;

/// What text of `len` bytes is charged: **nine eighths of its length, rounded up**. The law of the memory tests holds what the model keeps
/// to its bytes and an eighth more for the size classes of the app's allocator (mimalloc rounds a block up to one of four sizes in each doubling), so text charged by
/// its length alone falls short of it by that eighth, which a kind made mostly of long text cannot make up elsewhere: a cell of a 10 KiB formula, a run of 64 KiB, a
/// kept string of 1 MiB were each charged about nine tenths of what the law asks. Every name, value, formula, run and kept string the decoder charges by its length
/// is charged this ([`super::Budget::charge_text`]); the figures of the elements cover what they build around it.
#[must_use]
pub const fn text_bytes(len: usize) -> u64 {
    // A `usize` is at most 64 bits on every target the workspace builds.
    let len = len as u64;
    len.saturating_add(len.div_ceil(8))
}

/// The bytes of the strings **a formula's tree** holds: its string literals, its GUIDs and currency codes, and the names of its references, calls,
/// qualifiers and indexes. Each is an allocation of its own that copies a part of the formula's text, which the formula holds as its source besides, so that a
/// formula that is one string of 10 KiB keeps 20 KiB: the decoder charges them as text ([`text_bytes`]) beside the nodes of the tree. Never more than the text, and
/// nothing for a formula whose text did not parse (its error holds no string). The walk keeps its own stack and does not recurse.
#[must_use]
pub fn tree_text_bytes(formula: &diacraft_formula::Formula) -> usize {
    use diacraft_formula::{Expr, Index, Qualifier, Segment};
    fn segments(path: &[Segment]) -> usize {
        path.iter()
            .map(|s| {
                let index = match &s.index {
                    Some(Index::Name(name)) => name.len(),
                    _ => 0,
                };
                s.name.len().saturating_add(index)
            })
            .fold(0usize, usize::saturating_add)
    }
    let Ok(root) = &formula.ast else { return 0 };
    let mut total = 0usize;
    let mut stack: Vec<&Expr> = vec![root];
    while let Some(e) = stack.pop() {
        let mut qualifiers: &[Qualifier] = &[];
        match e {
            Expr::Str(s) | Expr::Guid(s) => total = total.saturating_add(s.len()),
            Expr::Currency { code, .. } => total = total.saturating_add(code.len()),
            Expr::Ref(r) => {
                total = total.saturating_add(segments(&r.path));
                qualifiers = &r.qualifiers;
            }
            Expr::Call(c) => {
                total = total.saturating_add(c.name.len());
                stack.extend(c.args.iter().flatten());
                qualifiers = &c.qualifiers;
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Color { .. } => {}
        }
        for q in qualifiers {
            match q {
                Qualifier::Path(path) => total = total.saturating_add(segments(path)),
                Qualifier::Call { name, args } => {
                    total = total.saturating_add(name.len());
                    stack.extend(args.iter().flatten());
                }
            }
        }
    }
    total.min(formula.source.len())
}

/// What an unknown element costs once kept, in the native slot of its owner (a `RawElement` and its place in a list, and the frame the copy used): measured
/// at 385 bytes with the same margin as [`element_bytes`].
pub const UNKNOWN_ELEMENT_BYTES: u64 = 496;
/// What an unknown attribute costs once kept (a pair of strings in a list, and the pending pair it was read as): measured at 160 bytes, of which
/// [`ATTR_BYTES`] is charged for every attribute of an element, so that an unknown one costs 208 in all.
pub const UNKNOWN_ATTR_BYTES: u64 = 176;
/// What a text, a comment or an instruction inside a kept unknown element costs once kept: measured at 70, 77 and 114 bytes.
pub const UNKNOWN_TEXT_BYTES: u64 = 96;
pub const UNKNOWN_COMMENT_BYTES: u64 = 96;
pub const UNKNOWN_PI_BYTES: u64 = 144;
/// What a bare text node among the parts of a text costs (a part, and the string).
pub const TEXT_PART_BYTES: u64 = 96;

// ---------------------------------------------------------------------------------------------------------------------------------------- cells

/// What the allocator makes of a request of `n` bytes: the chunk, 8 bytes more rounded up to 16 (glibc's), never less than 32.
pub const fn chunk(n: usize) -> usize {
    let c = n.saturating_add(8).div_ceil(16).saturating_mul(16);
    if c < 32 { 32 } else { c }
}

/// A node of the map that holds the cells of one place: eleven keys and eleven values, and a parent pointer and two counts (the standard library's layout),
/// as the allocator hands it out. **A map allocates the whole leaf at its first cell**, so that a place with one cell costs what a place with eleven does.
pub const CELLS_LEAF_BYTES: u64 =
    chunk(((11 * (std::mem::size_of::<diacraft_doc::CellName>() + std::mem::size_of::<diacraft_doc::Cell>()) + 12).div_ceil(8)) * 8) as u64;
/// An inner node of that map: a leaf and twelve edges.
pub const CELLS_INNER_BYTES: u64 =
    chunk(((11 * (std::mem::size_of::<diacraft_doc::CellName>() + std::mem::size_of::<diacraft_doc::Cell>()) + 12).div_ceil(8)) * 8 + 12 * 8) as u64;

/// What the law of the memory tests holds an allocation of `n` bytes to (`tests/it/model_memory.rs`): the bytes, an eighth of them for the
/// size classes of the allocator the app ships with (mimalloc), and the most glibc adds to one (24), rounded up to 16. It is what [`chunk`] is to glibc alone.
pub const fn law_bytes(n: usize) -> usize {
    n.saturating_add(n / 8).saturating_add(24).div_ceil(16).saturating_mul(16)
}

/// The bytes a node of the map of cells is asked for: eleven keys and eleven values, and a parent pointer and two counts (the standard library's layout).
const CELLS_LEAF_REQUEST: usize =
    ((11 * (std::mem::size_of::<diacraft_doc::CellName>() + std::mem::size_of::<diacraft_doc::Cell>()) + 12).div_ceil(8)) * 8;

/// A leaf of the map of cells **as the decoder charges it**: the node at the law ([`law_bytes`]), where [`CELLS_LEAF_BYTES`] is glibc's chunk of it. A place that
/// holds one cell keeps the leaf of eleven (1,160 bytes asked for), which the law holds to 1,329 and glibc's chunk priced at 1,168: rows, sections and shapes of one
/// or two cells fell short of the law by it.
pub const MODEL_CELLS_LEAF_BYTES: u64 = law_bytes(CELLS_LEAF_REQUEST) as u64;
/// An inner node of that map as the decoder charges it: a leaf and twelve edges, at the law.
pub const MODEL_CELLS_INNER_BYTES: u64 = law_bytes(CELLS_LEAF_REQUEST + 12 * 8) as u64;

/// The bytes of the nodes of a map of `n` cells **built from sorted pairs** (as the decoder builds it: `BTreeMap::from_iter` fills each node, and a map built
/// by inserting one cell at a time in an order a file chooses has leaves that are half empty, which is twice this), each node at the law
/// ([`MODEL_CELLS_LEAF_BYTES`], [`MODEL_CELLS_INNER_BYTES`]). The first leaf holds eleven; each leaf after it is a separator and eleven more; an inner node holds
/// twelve children. The cells' own allocations (a name) are [`element_bytes`]'s.
pub fn cells_bytes(n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut level = n.saturating_add(12) / 12;
    let mut total = (level as u64).saturating_mul(MODEL_CELLS_LEAF_BYTES);
    while level > 1 {
        level = level.div_ceil(12);
        total = total.saturating_add((level as u64).saturating_mul(MODEL_CELLS_INNER_BYTES));
    }
    total
}

// ---------------------------------------------------------------------------------------------------------------------------------------- lists

/// A node of kept markup, as a list holds it.
pub const RAW_NODE_BYTES: usize = std::mem::size_of::<diacraft_doc::RawNode>();
/// A kept node with its place, as the list of one element's kept nodes holds it.
pub const PLACED_BYTES: usize = std::mem::size_of::<diacraft_doc::Placed>();
/// An attribute of kept markup, a pair of names, as the list of one element's attributes holds it.
pub const ATTR_PAIR_BYTES: usize = std::mem::size_of::<(std::sync::Arc<str>, std::sync::Arc<str>)>();
/// The reference count around the list of kept nodes or attributes of one element, and the list's own header (a pointer, a capacity and a length).
pub const LIST_HEADER_BYTES: u64 = chunk(16 + 24) as u64;

/// The bytes a vector of entries of `size` bytes holds after `n` pushes: **room for four at the first push**, and twice as much each time it is full after
/// that. A list of one is as large as a list of four, which no per-entry figure measured on lists of thousands shows.
pub fn vec_bytes(n: usize, size: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    chunk(n.checked_next_power_of_two().unwrap_or(usize::MAX).max(4).saturating_mul(size)) as u64
}

/// What pushing one more entry of `size` bytes onto a list of `have` adds to what the list holds: the growth of the vector, and its header at the first.
pub fn push_bytes(have: usize, size: usize) -> u64 {
    vec_bytes(have.saturating_add(1), size).saturating_sub(vec_bytes(have, size)).saturating_add(if have == 0 { LIST_HEADER_BYTES } else { 0 })
}

// ---------------------------------------------------------------------------------------------------------------------------------------- trees

/// What the XML reader's tree of a part costs for each item (an element, an attribute, a text), **at the worst, at the peak while the part is parsed**: 151 bytes
/// of resident memory measured (`model_pools.rs`: eighteen shapes of tree, the worst an element with a million attributes, and the peak read while the tree is made,
/// where a list that grows holds more than it needs and one that moves holds the old beside the new). The container's parser grows its lists by an eighth and makes
/// an element's attributes to their count, and gives back what a finished element holds beyond its entries (the lists doubled and the peak was 330 for an element
/// with one child). **A string longer than a small allocation adds its bytes**, which the budget counts in the part's bytes ([`super::OPEN_PART_BYTES`], twice).
pub const TREE_ITEM_BYTES: u64 = 160;

/// What a process holds before it opens a drawing, as an address space: the executable and its libraries, the stacks of its threads and the arenas the
/// allocator reserves. Measured at about 180 MiB in the test process; the figure is a quarter above and rounded.
pub const BASE_BYTES: u64 = 256 << 20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allocators_chunk_is_the_request_and_eight_rounded_up_to_sixteen_and_never_under_thirty_two() {
        for (request, chunk_of) in [(0usize, 32usize), (1, 32), (8, 32), (24, 32), (25, 48), (40, 48), (41, 64), (1160, 1168), (1256, 1264)] {
            assert_eq!(chunk(request), chunk_of, "a request of {request} bytes");
        }
    }

    #[test]
    fn the_law_is_the_request_an_eighth_of_it_and_twenty_four_rounded_up_to_sixteen() {
        for (request, held) in [(0usize, 32usize), (8, 48), (16, 48), (24, 64), (25, 64), (1160, 1344), (1256, 1440)] {
            assert_eq!(law_bytes(request), held, "a request of {request} bytes");
        }
        for n in 0..5_000 {
            let held = law_bytes(n);
            assert!(held >= n + n / 8 + 24 && held < n + n / 8 + 40 && held.is_multiple_of(16), "{n}: {held}");
        }
        // The decoder's nodes of cells are at the law, never under glibc's chunk of them.
        const { assert!(MODEL_CELLS_LEAF_BYTES >= CELLS_LEAF_BYTES && MODEL_CELLS_INNER_BYTES >= CELLS_INNER_BYTES) };
        assert_eq!(cells_bytes(1), MODEL_CELLS_LEAF_BYTES);
        assert!(law_bytes(usize::MAX) > 0, "a size that cannot be held does not wrap");
    }

    #[test]
    fn text_is_charged_nine_eighths_of_its_length_rounded_up() {
        for (len, charged) in [(0usize, 0u64), (1, 2), (7, 8), (8, 9), (9, 11), (16, 18), (17, 20), (1024, 1152), (10_240, 11_520)] {
            assert_eq!(text_bytes(len), charged, "{len} bytes");
        }
        for len in 0..5_000usize {
            let charged = text_bytes(len);
            let at_least = (len as u64 * 9).div_ceil(8);
            assert_eq!(charged, at_least, "{len}: nine eighths, rounded up");
        }
        assert_eq!(text_bytes(usize::MAX), u64::MAX, "a length that cannot be held does not wrap");
    }

    #[test]
    fn a_formulas_tree_holds_its_strings_and_names_again_and_never_more_than_its_text() {
        let tree = |text: &str| tree_text_bytes(&diacraft_formula::parse(text));
        // One string: its tree holds the string (the quotes are the source's), and a quote written twice is one.
        assert_eq!(tree("\"abcdef\""), 6);
        assert_eq!(tree("\"a\"\"b\""), 3);
        // Names of references, calls and their qualifiers, an index by name, a GUID and a currency code.
        assert_eq!(tree("Width*0.5"), 5);
        assert_eq!(tree("Prop.Cost.Value"), 13);
        assert_eq!(tree("Sheet.5!Width+IF(TRUE,\"xy\",3)"), "Sheet".len() + "5".len() + "Width".len() + "IF".len() + "xy".len());
        assert_eq!(tree("Pages[Site]!PinX"), "Pages".len() + "Site".len() + "PinX".len());
        assert_eq!(tree("USE({01234567-89AB-CDEF-0123-456789ABCDEF})"), "USE".len() + 38);
        assert_eq!(tree("12.5USD"), 3);
        // Numbers, colours and operators hold no string, and a text that does not parse holds none.
        assert_eq!(tree("1+2*#FF0000-30%"), 0);
        assert_eq!(tree("\"never closed"), 0);
        for text in ["\"abcdef\"", "Sheet.5!Width+IF(TRUE,\"xy\",3)", "LISTSHEETREF(2)!Width", "a+b+c+d"] {
            assert!(tree(text) <= text.len(), "{text}");
        }
    }

    #[test]
    fn a_vector_holds_room_for_four_after_its_first_push_and_twice_as_much_each_time_it_is_full() {
        assert_eq!(vec_bytes(0, 56), 0);
        for n in 1..=4 {
            assert_eq!(vec_bytes(n, 56), chunk(4 * 56) as u64, "{n} entries");
        }
        assert_eq!(vec_bytes(5, 56), chunk(8 * 56) as u64);
        assert_eq!(vec_bytes(8, 56), chunk(8 * 56) as u64);
        assert_eq!(vec_bytes(9, 56), chunk(16 * 56) as u64);
        assert_eq!(vec_bytes(1_000_000, 8), chunk(1_048_576 * 8) as u64);
        // A count that cannot be held does not wrap.
        assert!(vec_bytes(usize::MAX, 56) > 0);
        assert!(vec_bytes(usize::MAX / 2 + 1, usize::MAX) > 0);
    }

    #[test]
    fn a_push_adds_the_growth_of_the_vector_and_the_header_at_the_first() {
        assert_eq!(push_bytes(0, 64), vec_bytes(1, 64) + LIST_HEADER_BYTES);
        assert_eq!(LIST_HEADER_BYTES, chunk(16 + 24) as u64);
        for have in 1..40 {
            assert_eq!(push_bytes(have, 64), vec_bytes(have + 1, 64) - vec_bytes(have, 64), "{have} entries");
        }
        assert_eq!(push_bytes(3, 64), 0, "the fourth fits");
        assert_eq!(push_bytes(4, 64), chunk(8 * 64) as u64 - chunk(4 * 64) as u64, "the fifth doubles it");
    }
}
