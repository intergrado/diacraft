//! What the reader accepts. [`Limits`] holds `opc`'s limits for the container and the XML, and the caps of this crate for what it builds.
//!
//! **Two kinds of cap, kept apart.** The document model's own (`diacraft_doc::MAX_PAGES`, `MAX_STYLES`, `MAX_MASTERS`,
//! `MAX_SHAPES_PER_PAGE`, `MAX_SHAPE_DEPTH`, and the limits of a sheet) are, and **a field here can only lower them**:
//! the model is the ceiling, so a document this reader builds always passes `Document::check_limits`. The rest are this crate's, labelled
//! choices that no spec states, and the report of every file carries the real maxima so that the sample-file run can confirm or move each one
//! (`Stats`).
//!
//! Every cap is checked **before** what it counts is stored: a page, style or master is counted before it is built, a shape before its
//! sheet is read, a cell before it is put in its sheet. A file past one is refused (`VsdxError::Limit`); nothing is truncated.

use diacraft_doc::{MAX_MASTERS, MAX_PAGES, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, MAX_STYLES};

// # The most a file can cost
//
// **One budget holds everything a read holds at once** ([`MAX_MEMORY`], 2.6 GB). The file the caller holds, the package inflated (at
// most 1 GiB), the XML tree of each part while it is read, the model built from the trees and the pictures copied into it are charged to it, **each by the
// real bytes it holds and before it is made** (`cost.rs`, and `diacraft_sheetxml::price` for the cells, rows, sections and kept markup of a sheet): a read that would pass
// it is refused whole, in words, as `too_much_memory` ([`crate::LimitKind::Memory`]) where it is counted. What is left of it when the file, the package and the 64
// MiB a growing list needs beside itself ([`crate::cost::GROWTH_BYTES`]) are counted is the **pool** the trees and the model are charged to. **A parse is bounded before its tree is
// made**: by the items the pool can hold at the dearest a tree can be (368 bytes an item, which `unheld.rs` holds every kind of tree to: a chain of nested
// elements is 335, an instruction that is the only child of its element 368) once what the parse holds beside its items is taken out of the pool, the room its strings can take
// (twice the part's text) and, for a part in UTF-16, the copy of the text it is decoded into ([`crate::cost::parse_fixed_bytes`]); the relationships parts, the themes, the
// properties and the solutions index are bounded the same way. A tree that was made is counted from the tree (every allocation behind it, at the capacity it has:
// `unheld::tree_bytes`) and is held in the pool while its part is read and given back with it. The model is priced from the sizes of its types and the allocator's rounding, a map
// of cells at its worst, with a margin of an eighth; a picture is charged its bytes as they are, and one the pool cannot hold is left out and counted (`media_over_budget`), as one past
// [`MAX_MEDIA_BYTES`] is. **Each read is given its share of the process's budget** (the process's, not a session's): the engine reads a file with [`MAX_MEMORY`] less what the process
// holds (`diacraft_engine::memory`: a `Hold` for the file and the model of each open drawing and for the model of each open stencil, in whatever session, and for a model a caller
// keeps beside a session), and a refusal says what is held; a drawing of 80,000 shapes held and a second file read, by the same session or by the window's reading thread or its
// `Scratch` session, was a process of 3.3 GB and aborted under the limit (`crates/engine/tests/it/vsdx_memory.rs`, `file_open` of the window, `roundtrip` of the command line). What a
// drawing has grown by since it was read is not counted, and the `.odg` reader takes no memory limit, so it is not given what is left.
//
// **The sum is held to the budget by what is counted, and the margin is what is not**: the budget leaves 0.47 GB of the 3.07 GB that an address-space limit of `ulimit -v 3000000`
// gives, for the allocator's slack on what is freed and made again, the process's own 143 MB and the walks of a tree once it is parsed. **That margin is measured, not proved**: the
// widest case of the table below uses 79 percent of the limit, and a read of a kind that is not in the table may use more. The other caps keep what they were for, the time a hostile
// file takes and what a real file is, and each is set beside the sample-file run's maxima (the figures of their docs).
//
// Measured on beast on 2026-11-20 (on main c8f605e6 with the fix round), on the release build, by `tests/it/memory_price.rs` (`probe`): each case in a process of its
// own, one at a time, the file arriving on the process's input, free and again under `ulimit -v 3000000`, at a load of 20 to 40 on a machine shared with other builds. **Every case
// ended as a read or as a refusal in words, free and under the limit, and none aborted.** The columns are what the read added to the process at its peak (`VmHWM` after a reset, a GB
// the KiB the test prints over a million), the address space the process reached (`VmPeak`, which the limit bounds), and the bytes the pool was charged at the most, beside the file
// and the package; the time is the read's (a time moves with the load, a peak does not). The last column is what the row stated before this fix round (the same day):
//
// | Case | Ended | Time | Added | Address space | Charged at the most, beside the file and the package | Before this fix round: ended, address space |
// |---|---|---|---|---|---|---|
// | one part of 5,990,000 unknown elements (the 6 million items one part may hold) | read | 2.4 s | 1.15 GB | 1.48 GB | 1.54 GB, beside 0.1 MB and 24 MB | read, 1.51 GB |
// | four parts of 4,990,000 (the 20 million items a document might keep) | refused `too_much_memory` in the second part, after 4,990,038 items | 2.6 s | 1.01 GB | 1.47 GB | 1.47 GB, beside 0.4 MB and 80 MB | refused in the third part, after 9,980,046 items, 2.17 GB |
// | 9,960,000 empty cells in four parts (a cell is the dearest thing for its bytes) | refused after 2,490,003 cells and 4,982,036 items | 4.6 s | 1.60 GB | 1.86 GB | 1.94 GB, beside 22 MB and 165 MB | refused after 3,627,664 cells, 2.15 GB |
// | a gigabyte of XML: sixteen parts of 63 MiB of 200,000 shapes each, 1,060,631,724 bytes inflated | refused in the third part, after 3,600,054 items | 2.8 s | 2.12 GB | 2.18 GB | 1.21 GB, beside 14 MB and 1.06 GB | (parts of 400,000 shapes) refused in the second part, 2.48 GB |
// | the same in parts of 400,000 shapes: 3.6 million items a part, which at the bound and with the room for the text are 1.46 GB where the pool has 1.45 GB | refused before the first part is parsed | 1.6 s | 1.64 GB | 1.70 GB | 34 KB, beside 23 MB and 1.06 GB | not a case |
// | a gigabyte of pictures that no compression takes anything from: four of 250 MiB, each shown by a shape, stored: a file of 1.05 GB, a package of 1.05 GB | read, the first picture copied and the other three left out | 0.9 s | 1.28 GB | 2.38 GB | 0.26 GB, beside 1.05 GB and 1.05 GB | read, 2.44 GB |
// | every kind at once: 1.5 million unknown elements, 300,000 empty cells, 300,000 cells with a formula, 300,000 rows, the text of 200 shapes, 8,000 shapes of a diagram, 100,000 connects, 30,000 masters, 20,000 styles, two pictures of 100 MB, a theme, and a last part of 4.5 million unknown elements | refused before the last part is parsed | 3.8 s | 1.43 GB | 1.91 GB | 1.09 GB, beside 286 MB and 286 MB | (10,000 shapes, 300,000 connects) refused in the last part, 2.32 GB |
// | 900 values of a mebibyte in six pages, stored: a file of 944 MB, a package of 944 MB | refused `too_much_memory` | 1.4 s | 1.38 GB | 2.34 GB | 0.51 GB, beside 944 MB and 944 MB | refused, 2.59 GB |
// | 960 formulas of one string of a million characters, in eight parts of 120 MB | refused `too_much_xml`, in the fifth part | 1.9 s | 1.99 GB | 2.14 GB | 1.20 GB, beside 4.4 MB and 960 MB | refused, 2.19 GB |
// | seven pages of 11 million unknown elements, then a page of 31,500 chains of 190 nested elements (5,985,000 items of 335 bytes each, in a file of 674 KB) | refused `too_much_memory` before the page is parsed | 4.4 s | 2.15 GB | 2.32 GB | 1.23 GB, beside 0.7 MB and 86 MB | **aborted** (4 of 4) |
// | 2.5 million empty cells, then a page of 125 million characters of three bytes in UTF-8, as 250 MB of UTF-16 with one CR LF | read | 4.7 s | 2.04 GB | 2.19 GB | 1.42 GB, beside 111 MB and 290 MB | **aborted** |
// | 3.5 million empty cells and the same page | refused `too_much_memory` before the page is decoded | 3.4 s | 1.70 GB | 1.93 GB | 1.93 GB, beside 113 MB and 307 MB | not a case |
// | the page of UTF-16 alone | read | 2.1 s | 0.98 GB | 1.16 GB | 0.38 GB, beside 106 MB and 250 MB | read, 2.01 GB (the dev build) |
//
// **The largest address space is 2,381,796 KiB** (the gigabyte of pictures, a file and a package that are 2.1 GB between them): 20.6 percent under the limit, which the budget leaves
// for what no price counts. The 3.07 GB is the limit and the 2.6 GB the budget: what a read counts cannot pass the budget, and that is all the budget says of the process.
//
// **The price against the process.** `tests/it/memory_price.rs` reads each kind of file that was measured in a process of its own and holds the bytes charged, with the package
// they were read from, to the growth of the process at its peak: never under it (less the 64 MiB a growing list holds beside itself, which a file of 500,000 connects shows), and
// never twice it. By kind, the bytes charged with the package over the growth, on the release build: a million unknown elements 1.08, empty cells 1.24, empty rows 1.16, shapes
// with nothing in them 1.07 (the tightest: the margin of an eighth is the room it has), cells with a value, a unit and a formula 1.43, cells that keep five attributes 1.10,
// sections 1.22, text in many parts 1.04, cells that hold text and markup 1.13, connects 0.99, 200,000 pictures shown and not there 1.05, a part of the document that keeps three
// million unknown elements 1.33, formulas of one string of a million characters 1.07, and the shapes of a diagram 1.37. The dearest is a formula: a node is charged 144 bytes (128
// and the margin) where a node of a number holds some 45, a call some 80 and a path of names some 115, measured by the difference of files with and without them.
//
// **The price against what the model keeps, counted**. The growth of the process is no measure of the model: it misses a vector's room that nothing has touched, and
// moves with the platform's allocator, so the ratios above pass a price that is short. `tests/it/memory_counted.rs` reads 52 kinds of file in a child whose allocator
// counts, each at a count one more than a power of two, and holds the charge of the units and of the whole file to the bytes the model keeps and 24 for each allocation it is in.
// The prices were glibc's chunk to the byte (glibc's own count of the bytes in use, read beside it, gave the same figures), which is 16 bytes an allocation under that, and eight
// kinds of small strings fell short (a connect 234 charged against 240, a kept element of an attribute and a child 558 against 616, a face 377 against 432, whose attributes were
// priced as a list made whole where they are pushed into): a string and a buffer are priced at the law's figure now (`diacraft_sheetxml::price::block`), and every kind is charged
// 1.09 to 4.5 times what it needs, and 1.20 times glibc's own count at the least. The figures of the tables above were measured before; the shapes of a diagram below were read
// again.
//
// **Per item, on the same build** (`a_million_unknown_elements_in_a_shape_cost_a_bounded_number_of_bytes_each`, `a_million_cells_with_no_value_cost_a_bounded_number_of_bytes_each`
// and `a_million_rows_with_no_cell_cost_a_bounded_number_of_bytes_each`, each a child under `ulimit -v 3000000`, which they pass): an empty cell adds **592 bytes** (296 an item), an
// empty row **562** (281 an item) and an unknown element **192** an item, measured as the table before this one measured them (the file made in the process that reads it, whose
// freed memory the read reuses), so that the budget changed nothing of what a read holds. In a process that holds nothing else (the file arrives on its input: `memory_price.rs`)
// the same reads add 608, 579 and 197. The pool charges them 736, 659 and 210.
//
// **The most a real drawing of diagram shapes can be** (a shape of fourteen cells, six of them formulas, a path of five rows of two cells and a text: 136 items, 18 KB of model;
// `memory_price.rs`, `typical_p<shapes>_<pages>`): **it depends on how the shapes are paged**, because the tree of a page is held in the pool at the dearest it can be (368 bytes an
// item, where a drawing's is some 140) from the bound of its parse to its end, and the pool must hold that beside the model of the pages before it. **78,000 shapes are read in pages
// of 6,000** and 84,000 are refused (84,000 were read before a change priced a string at the law's figure, 2.38 GB charged and 1.82 GB of address space; a shape of a diagram
// is charged about 4 percent more), **80,000 in pages of 4,000, 72,000 in pages of 8,000** (80,000 before), **77,000 in pages of 11,000** (the largest page of the sample-file run has 11,464
// shapes), **72,000 in pages of 12,000 and 65,000 in pages of 13,000** (78,000 are refused; 78,000 were read before a change, which bounds a parse at the
// dearest a tree can be), where the reader of 2026-11-19 admitted about 140,000 and aborted at 150,000 under the limit. One page holds at most some 40,000 of them
// (the tree of its part, 6 million items, is the container's cap: 40,000 are read, in 1.51 GB of address space, and 50,000 are `xml_too_complex`). **The fall is the price, not the
// memory**: at the pool's end these drawings reach 1.5 to 1.8 GB of the 3.07 GB, because a diagram's shape is charged about 1.4 times what it holds (the formula nodes at 2.4 times
// the 62 bytes a node of `Width*0.5` holds, a map of cells at its worst when it holds more than eleven, and the eighth of margin) and the tree of a page is bound at 2.6 times what
// it holds. **When the container's parser makes its lists to the size they need (155 bytes an item at the worst, where this is 368) the price is derived again**
// (`TREE_ITEM_BYTES`, which the test of `unheld.rs` holds every kind of tree to), and the figure for pages of 13,000 returns to what it was or more. A walk of each formula's tree
// for what it holds, as the XML tree is counted, would give most of the rest back; it is not done.
//
// **Before this budget** the table was: the four parts of 4,990,000 added 2.22 GB and needed an address space between 3.3 and 3.6 million kB, the 9,960,000 empty cells added 3.28
// GB and the gigabyte of XML 4.39 GB, and the last two aborted at an allocation under `ulimit -v 3000000` (`memory allocation of ... bytes failed`); the sum for a hostile file at the
// defaults was 4.4 GB measured and 5.5 GB by the arithmetic. The three cases are children of the suite now (`memory_price.rs`), and an abort fails them.
//
// **After the extension slots.** Each sheet, section, row, shape and text holds one more pointer, for the data of the other format: a `Row` is 112
// bytes (104 before), a `Shape` 192 (184), a `Section` 128 (120), a `Cell` is unchanged at 80, and a `TextPart` is the 48 bytes it was (the two new variants are boxed). The prices
// are read from the sizes of the types, so they follow a change of them (`tests/it/memory_charges.rs` pins the figures where a word is eight bytes, and fails when one moves).
//
// **A real file is a small part of the budget.** The sample-file run's heaviest, a template of 1.1 million cells and 63 MB of XML, took 728 MiB when it was read before this budget; by
// the prices above a million cells that each hold a value, a unit and a formula are charged some 1.7 KB each, tree and model, so that file is under 2 GB of the 2.6, estimated: the
// next sample-file run says it exactly (`Stats::memory_peak`, `max_tree_bytes` and `memory_bytes`, in the oracle's JSON). The other caps keep the margin the sample-file run measured: the
// items cap is 1.95 times its largest file's (6,162,067 items, of 2,565 real files), the cells 2.9, the formula nodes 2.9, the pictures 2.8. **The heaviest real file's file, package, the
// 64 MiB a growing list needs and its peak in the pool come to 0.545 of [`MAX_MEMORY`]**, and none of the 2,565 is refused or loses content to the budget (the sample files count). A caller that has less memory (the web build, a small machine) passes a lower
// `max_memory` (and the other fields, each of which can be lowered): a file past one is refused loudly.

/// Formula nodes, summed over the document, that the reader parses (`formula::parse_with_budget`). A node costs 72 to 128 bytes and the
/// allocator's overhead (`crates/formula`). A formula the budget cannot afford is kept as text with its cached value and counted
/// (`ReadReport`): the file still opens. **The sample-file run's largest file has 2,804,543 nodes**, so this is 2.9 times that (8 million, chosen
/// before any file had been read, then 16 million; 8 million again, since the 144 bytes a node is charged hold 17 million of them in the pool of
/// the memory a read may hold, and every one of them is a formula in a cell that costs more). Labelled choice.
pub const MAX_FORMULA_NODES: usize = 8_000_000;

/// Cells, summed over every sheet of the document. A cell is a map entry and two or three strings, about 250 bytes without its formula, and the two items of
/// its tree besides. **The sample-file run's largest file has 1,373,509 cells**, so this is 2.9 times that (8 million, then 16 million;
/// 4 million, which is about what the pool of the memory a read may hold admits of the cheapest cells). Labelled choice.
pub const MAX_CELLS: usize = 4_000_000;

/// Shapes, summed over the page and master parts of the document, nested ones counted, **a part that several rows name counted once for
/// each of them** (it is what a walker of the document meets). `MAX_SHAPES_PER_PAGE` is for one part; a thousand pages could each hold
/// it. Labelled choice; the real largest part has 11,464 shapes, and the sample-file run reports the real total.
pub const MAX_SHAPES: usize = 2_000_000;

/// XML items the reader keeps, summed over every part of the document: each element it models or keeps (a cell, a row, a shape, an unknown
/// element and everything in it), each attribute on one, and each text, comment or instruction it keeps. `opc` caps the items of one part's
/// tree (6,000,000); a package of many parts, each at that cap, is a document of as many times more, and this is the total. **The sample files
/// run's largest file keeps 6,162,067** (of 2,565 real files; the first run's 4,679,047 was of fewer), so this is 1.95 times that (it was 16 million, then 20 million; 12 million, which is
/// about what the pool of the memory a read may hold admits of the cheapest items, an unknown element, tree and model, at some 200 bytes). Labelled choice; every
/// report carries the real figure (`Stats::items`).
pub const MAX_ITEMS: usize = 12_000_000;

/// Bytes of XML parts parsed, summed over the document: the document part, the two indexes and every distinct page and master part. It bounds
/// the time a hostile file takes (parsing is linear in it) as `opc`'s caps bound the inflating. The largest package in the census is 173 MB
/// in all, so this is three times that. Labelled choice.
pub const MAX_XML_BYTES: u64 = 512 * 1024 * 1024;

/// Bytes of pictures copied into the model, summed (`Media`). The package that holds them is read whole already, so this bounds the
/// second copy: a picture past it is left out and counted, and the shape shows nothing. **The sample-file run's largest holds 192 MB of
/// pictures**, so 512 MiB is 2.8 times that (1 GiB, which is 5.6 times; it was 256 MiB before, which would drop a real picture; the pool of the memory a read may hold leaves out a picture it cannot hold besides, so a gigabyte of them is not copied whole in any case).
/// Labelled choice.
pub const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

/// Theme parts read. A drawing holds none to a few (104 files hold two or more). Labelled choice.
pub const MAX_THEMES: usize = 256;

/// Bytes of theme parts kept in the model, summed. **The sample-file run's largest total is 198,535 bytes**, so 4 MiB is 21 times that (M2
///It was 32 MiB, room only a hostile file would use). Labelled choice.
pub const MAX_THEME_BYTES: u64 = 4 * 1024 * 1024;

/// Bytes of a property part (`docProps/core.xml`, `app.xml`) or of the solutions index that are read. A larger part is left unread
/// and reported, not truncated. Labelled choice: real ones are under a few kilobytes. **The DiaCraft solution part has a cap of its own**,
/// [`MAX_SOLUTION_PART_BYTES`], since it carries the data of the other format.
pub const MAX_SMALL_PART_BYTES: u64 = 4 * 1024 * 1024;

/// Bytes of the DiaCraft solution part that are read: 64 MiB, the same as the `.odg` document file's. The part carries DiaCraft's own settings **and the items of the other format's
/// data** (one for the document, each page, master, style and shape that holds some: `carry.rs`), which grow with the drawing; a part larger than this is left unread and reported, not
/// truncated, and a save that would write a larger one is refused in words (the data is never cut). The bytes of what rides, an `.odg`'s macro project or objects, are not in the
/// part: they go in parts of their own, each held to the package's per-part cap. Labelled choice (the native format's design, 5.2).
pub const MAX_SOLUTION_PART_BYTES: u64 = 64 * 1024 * 1024;

/// Steps the judge of the carrier's items may take in one read: a step is a node, cell, row or section hashed to stamp an object, a shape looked at to place an item, or a sheet's cell copied
/// where an item puts data on a shared sheet (`carry/read.rs`). **The model a file can make is bounded by the caps above, but the work a crafted list of items makes of it is not**: an item for
/// each of a million shapes of a page, or the same page named by a thousand pages, each costs a walk of what it names. A read that passes this leaves the items it has not judged out, and says so
/// (`diacraft_items_not_read`). 48 million: four times the items a read may keep ([`MAX_ITEMS`]), which is what a drawing at the cap and an item for each of its parts cost, a few seconds.
/// Labelled choice.
pub const MAX_CARRY_STEPS: u64 = 4 * MAX_ITEMS as u64;

/// Entries a report lists (warnings, skipped items) before it only counts them. Labelled choice.
pub const MAX_REPORT_ENTRIES: usize = 200;

/// Bytes of memory a read may hold at once, **charged by the real size of what it holds** (`cost.rs`): the file the caller holds, the package inflated, the XML
/// tree of each part while it is read, the model that is built, and the pictures copied. What is left of it when the file, the package and the room a growing list
/// needs beside itself are counted is the pool the trees and the model are charged to, before each is made; a read that would pass it is refused
/// (`too_much_memory`). Labelled choice: **2.6 GB**, of the 3.07 GB that an address-space limit of `ulimit -v 3000000` gives. The
/// 0.47 GB it leaves is for what no price counts: the process's own 143 MB (the executable, the stacks of its threads and the arenas the allocator reserves), the
/// allocator's slack on what is freed and made again, and the allocations of the read that are neither the model nor a tree (the parser's tokens, a relationships
/// part's map). The most address space a case reached, with this budget, is 2.38 GB (the table above): measured, not proved.
///
/// **In the browser (`wasm32`) it is [`WEB_MAX_MEMORY`], 1 GiB**: a WebAssembly memory is at most 4 GiB and only grows (it is never given back), a browser may give less, and the page holds the
/// window, the fonts and the renderer beside the drawing. A file that does not fit is refused in the same words as on the desktop, from its size when it can be, and nothing is read.
pub const MAX_MEMORY: u64 = if cfg!(target_arch = "wasm32") { WEB_MAX_MEMORY } else { 2_600_000_000 };

/// The memory a read may hold in the browser (see [`MAX_MEMORY`]): 1 GiB. Labelled choice: the reader charges about 1.4 times what a drawing holds, so what a read of this budget really takes at its
/// peak is well under it, and the page, the window and the renderer keep the rest of what a browser gives a tab.
pub const WEB_MAX_MEMORY: u64 = 1 << 30;

/// What the reader accepts. [`Limits::default`] holds the constants above and `opc`'s, and `Limits { max_cells: 10, ..Limits::default() }`
/// changes one and keeps the rest.
#[derive(Debug, Clone)]
pub struct Limits {
    /// The container and the XML: `opc`'s own, applied to every part this reader parses too (`opc::xml::parse_with` takes `opc.xml`).
    pub opc: diacraft_opc::Limits,
    /// [`MAX_FORMULA_NODES`].
    pub max_formula_nodes: usize,
    /// [`MAX_CELLS`].
    pub max_cells: usize,
    /// [`MAX_SHAPES`].
    pub max_shapes: usize,
    /// [`MAX_ITEMS`].
    pub max_items: usize,
    /// [`MAX_XML_BYTES`].
    pub max_xml_bytes: u64,
    /// Pages: [`MAX_PAGES`], and never more.
    pub max_pages: usize,
    /// Styles: [`MAX_STYLES`], and never more.
    pub max_styles: usize,
    /// Masters: [`MAX_MASTERS`], and never more.
    pub max_masters: usize,
    /// Shapes in one page or master part, nested ones counted: [`MAX_SHAPES_PER_PAGE`], and never more.
    pub max_shapes_per_part: usize,
    /// Levels of nested `Shape` elements: [`MAX_SHAPE_DEPTH`], and never more.
    pub max_shape_depth: usize,
    /// [`MAX_MEDIA_BYTES`].
    pub max_media_bytes: u64,
    /// [`MAX_THEMES`].
    pub max_themes: usize,
    /// [`MAX_THEME_BYTES`].
    pub max_theme_bytes: u64,
    /// [`MAX_SMALL_PART_BYTES`].
    pub max_small_part_bytes: u64,
    /// [`MAX_SOLUTION_PART_BYTES`].
    pub max_solution_part_bytes: u64,
    /// [`MAX_CARRY_STEPS`].
    pub max_carry_steps: u64,
    /// [`MAX_REPORT_ENTRIES`].
    pub max_report_entries: usize,
    /// [`MAX_MEMORY`].
    pub max_memory: u64,
    /// **The threads a read may use**, its own counted: 1, the default, is the single-threaded read and makes no thread. More inflates the package on threads and parses the parts of the
    /// pages and the masters ahead of the read's turn for them, on threads, with their trees held in the memory the budget has left and no more; **the document, the report and every refusal
    /// are the ones the single-threaded read gives, at any count** (`ahead.rs`). The caller chooses it (`diacraft_container::par::default_threads` is the app's default).
    pub threads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            opc: diacraft_opc::Limits::default(),
            max_formula_nodes: MAX_FORMULA_NODES,
            max_cells: MAX_CELLS,
            max_shapes: MAX_SHAPES,
            max_items: MAX_ITEMS,
            max_xml_bytes: MAX_XML_BYTES,
            max_pages: MAX_PAGES,
            max_styles: MAX_STYLES,
            max_masters: MAX_MASTERS,
            max_shapes_per_part: MAX_SHAPES_PER_PAGE,
            max_shape_depth: MAX_SHAPE_DEPTH,
            max_media_bytes: MAX_MEDIA_BYTES,
            max_themes: MAX_THEMES,
            max_theme_bytes: MAX_THEME_BYTES,
            max_small_part_bytes: MAX_SMALL_PART_BYTES,
            max_solution_part_bytes: MAX_SOLUTION_PART_BYTES,
            max_carry_steps: MAX_CARRY_STEPS,
            max_report_entries: MAX_REPORT_ENTRIES,
            max_memory: MAX_MEMORY,
            threads: 1,
        }
    }
}

/// The caps in force, with the document model's as the ceiling of the five it owns.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caps {
    pub pages: usize,
    pub styles: usize,
    pub masters: usize,
    pub shapes_per_part: usize,
    pub shape_depth: usize,
    pub shapes: usize,
}

impl Caps {
    pub(crate) fn of(limits: &Limits) -> Caps {
        Caps {
            pages: limits.max_pages.min(MAX_PAGES),
            styles: limits.max_styles.min(MAX_STYLES),
            masters: limits.max_masters.min(MAX_MASTERS),
            shapes_per_part: limits.max_shapes_per_part.min(MAX_SHAPES_PER_PAGE),
            shape_depth: limits.max_shape_depth.min(MAX_SHAPE_DEPTH),
            shapes: limits.max_shapes,
        }
    }

    /// The budgets the sheet codec takes: the cells, the items and the formula nodes of the document (`diacraft-sheetxml` charges them).
    pub(crate) fn sheets(limits: &Limits) -> diacraft_sheetxml::Caps {
        diacraft_sheetxml::Caps { cells: limits.max_cells, items: limits.max_items, formula_nodes: limits.max_formula_nodes }
    }
}
