//! What the package reader and the comparison accept. [`Limits`] holds the container's caps for the archive and the XML (the VSDX reader's, unchanged, since the bounded ZIP and XML code is shared)
//! and the caps of this crate for what it reads itself.
//!
//! The container's are the spec's numbers where it states one, and labelled choices where it does not; see
//! `diacraft_container::limits`. The rest are this crate's, labelled choices that no spec states, each named with its reason. A cap is
//! checked **before** what it counts is stored or built, and a file past one is refused loudly; nothing is truncated but the lists of a
//! report, which say how many they left out.

/// Labelled choice: the largest `META-INF/manifest.xml`, in bytes. The spec states none. A row of the manifest is a hundred bytes or so, so
/// 4 MiB holds about 40,000 rows, near the part limit (50,000); it is the cap the OPC reader puts on its own package streams (the content
/// types and the relationships, `diacraft_opc::MAX_PACKAGE_XML_BYTES`), which play the same part.
pub const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// Labelled choice: the most items in the manifest's tree, counted as the container counts them (each element, attribute and text). A row
/// is an element, two to five attributes and the white space after it, so [`MAX_MANIFEST_ENTRIES`] rows are about 700,000 items; this
/// is a little more, as the OPC reader's cap on its package streams is.
pub const MAX_MANIFEST_NODES: usize = 1_000_000;

/// Labelled choice: rows (`manifest:file-entry`) in the manifest. A package lists each of its files once, and the part limit is 50,000;
/// rows for folders and for files a package lacks come on top, so this is twice the part limit.
pub const MAX_MANIFEST_ENTRIES: usize = 100_000;

/// Labelled choice: the longest `mimetype` entry read, in bytes. The longest media type of an OpenDocument package is under 60 bytes; a
/// `mimetype` longer than this is not read and is irregular, so its size never decides what is allocated.
pub const MAX_MIMETYPE_BYTES: u64 = 256;

/// Labelled choice, as the VSDX reader's `MAX_XML_BYTES`: bytes of XML parts parsed by one read of a package, or by one side of a
/// comparison, summed. It bounds the time a hostile file takes (parsing is linear in it) as the container's caps bound the inflating. The
/// largest package in the VSDX census is 173 MB in all, so this is three times that.
pub const MAX_XML_BYTES: u64 = 512 * 1024 * 1024;

/// Labelled choice, as the VSDX reader's `MAX_REPORT_ENTRIES`: warnings of one kind named one by one before the rest are only counted.
pub const MAX_REPORT_ENTRIES: usize = 200;

/// Labelled choice: entries a comparison lists, summed over its parts, before the rest are only counted. A comparison of an edit lists a
/// few; one of two unrelated drawings lists one for each element that differs, and the counts say how many.
pub const MAX_DIFF_ENTRIES: usize = 10_000;

/// Labelled choice: the longest element path a comparison writes, in bytes. A path is cut at the end of the last segment that fits and ends
/// in `/…`; each element name in it is cut to [`MAX_PATH_NAME_BYTES`]. A path of the deepest tree the container reads (200 levels) of
/// real names is about 4 KB.
pub const MAX_PATH_BYTES: usize = 4096;

/// Labelled choice: the longest element name a path segment holds, in bytes. A longer name is cut and ends in `…`. Real names are under
/// 40 bytes.
pub const MAX_PATH_NAME_BYTES: usize = 128;

// # The drawing's reader (`read`)
//
// The caps the VSDX reader applies apply here (plan, Global Constraints: "the VSDX reader's caps apply"), the item cap charged by bytes, and
// adds two so that a hostile file stays under 3 GB ([`MAX_READ_XML_NODES`], [`MAX_TEXT_BYTES`]), with the document model's
// own as the ceiling of those it owns (`diacraft_doc::MAX_PAGES`, `MAX_STYLES`, `MAX_SHAPES_PER_PAGE`, and the limits of a sheet): a field here
// can only lower those, so a document this reader builds passes `Document::check_limits`. Adds two of its own (path
// commands in a page, pairs in one `draw:points`), each refused as `xml_too_complex`. Nesting is the container's depth cap (200), which
// names: a tree deeper than it is refused before the reader sees it.
//
// ## The most a read can cost
//
// A read holds, at its peak: the inflated package (at most `container.max_package_bytes`, 1 GiB, the pictures the model shows and the entries it keeps among its
// parts, since the model shares the package's bytes; **a read inflates every file **, which keeps each entry no field reads as a part of the model, charged to
// [`MAX_MEDIA_BYTES`] before it is inflated, where had it inflate only the parts the drawing names: the worst package is the same, one whose 1 GiB are all pictures that
// frames show or all entries kept, held once, and for an instant a second copy of the largest entry while the package makes its bytes shared), one tree of `content.xml` or `styles.xml` (at most [`MAX_READ_XML_NODES`] items,
// about 210 bytes an item: 0.85 GB), the model the reader builds (at most [`MAX_ITEMS`] items of 64 bytes each: rows, cells, sections and
// shapes by the sizes of their types, text, names and kept markup by their bytes, `read::cost`), and the text it is putting together (at
// most [`MAX_TEXT_BYTES`], held twice while a run of characters becomes one part). The reader parses each tree when it converts it and
// drops it when done (`Package::parse_tree`, which lets the part's bytes go too): the styles tree is gone before the content tree is parsed,
// so a read never holds both. A picture part named by many frames is one copy, charged once to [`MAX_MEDIA_BYTES`]; inline pictures are
// measured from their characters before they are decoded.
//
// Measured by `tests/it/read_hostile.rs` (its 21 ignored cases, run one by one on the release build), as the read's own peak resident size with
// the input held (the peak is reset after the input is built; a GB below is the kB the test prints, divided by a million), each case also run under
// `ulimit -v 3000000`, which all pass. **Every figure was remeasured on beast on 2026-11-19 (main c49aa65), at a load of 31 to 52 on a
// machine shared with other builds**: a peak does not depend on load (a case run free and under the limit differs by 0.3 percent at most above 50 MB), a
// time does, and the only times here are those of `memory_and_time_arcs_of_no_sweep`. The largest peak is 2.66 GB (every budget at once), and that case also
// passes at `ulimit -v 2800000` and aborts at 2700000, so the 3 GB has between 0.2 and 0.3 GB to spare.
//
// **A row costs 21 items since the extension slots (accepted), not 20: a geometry row with its two cells is 29 items, not 28.** The
// slot added 8 bytes to `Row` (104 to 112), which takes the 1,280 bytes `read::cost::ROW` rounds up to 1,288, and a 64-byte item is 20 and a bit. Every row of
// the table is measured with it (remeasured the three of path points and of the every-budget mix, the rest), and a case whose
// model is mostly rows holds about 5 percent fewer of them before it is refused. The last column is what the row stated before this measure: the figure of
// 2026-11-08, except for the three rows had remeasured on medusa on 2026-11-19 (the polygons, the path-heavy stand-in and the every-budget mix):
//
// | Case | Peak, measured on beast, 2026-11-19, release | Before |
// |---|---|---|
// | `content.xml` and `styles.xml` each near the reader's tree cap (`memory_two_trees_at_the_item_cap`; an 8.8 MB file) | 0.85 GB | 0.85 GB |
// | 980,000 hyperlink wrappers kept unmodelled on one page, near the tree cap (`memory_a_page_of_kept_shapes_at_the_cap`; 9,800,056 items; custom shapes were the stand-in before a change models them) | 1.08 GB | 1.08 GB |
// | rectangles with text, each of its own size so that nothing is shared, to the item cap (`memory_a_page_of_modelled_shapes`; refused at 19,999,995 items, 19,999,988 before) | 0.81 GB | 0.80 GB |
// | the review's 19,000 `text:s` of 65,536 (`memory_spaces`; a 2.8 KB file, refused at the text cap), and 1,024 of them (read, the cap's worth) | 0.08 GB, 0.15 GB | 0.08 GB, 0.14 GB |
// | the review's six concave regular polygons of 159,999 corners on one page, and one on each of six pages (`memory_concave_polygons`; refused at the item cap, 18,560,341 items, and 18,560,421 on six pages; 17,920,313 before the slots) | 0.96 GB | 0.96 GB |
// | one 256 MiB picture part named by 10,000 frames (`memory_a_picture_named_by_ten_thousand_frames`; two copies while the package takes its parts, one after) | 0.54 GB | 0.53 GB |
// | the media budget at once: three 240 MB pictures shown and two inline pictures of 100 MB of base64, 870 MB of media, both trees near the cap, the text at its cap (`memory_media_at_once`) | 1.86 GB | 1.86 GB |
// | the re-review's font family of 1,000,000 characters named by 1,000 common styles, by 4,000, by 4,000 rectangles on automatic styles of their own, and by 65,000 common styles (`memory_font_family`; the 4,000 once aborted) | 0.02, 0.03, 0.05, 0.27 GB | 0.02, 0.03, 0.05, 0.25 GB |
// | The path-heavy stand-in, 35,191 `draw:path` shapes of 46 commands (`memory_path_heavy_drawing`; refused at the item cap, the 14,225th shape, the 14,706th before the slots) | 1.03 GB | 1.03 GB |
// | style properties near the tree cap: 60,000 styles that each hold all 59 properties the reader maps, 3.5 million values, each read once into a typed value beside its text (`memory_style_properties_at_the_tree_cap`; 16,800,511 items, 16,680,474 before; 1.46 GB before the values were typed) | 1.58 GB | 1.57 GB |
// | 70 custom shapes of 10,000 equations each, to the item cap (`memory_equations_at_the_item_cap`; refused at 19,999,999 items, 19,981,428 before; 1.67 GB before a change charged a formula by its nodes) | 1.36 GB | 1.37 GB |
// | Seven custom shapes of 40,000 arcs whose parameters are formulas, to the item cap (`memory_arcs_of_formulas_at_the_item_cap`; refused at 19,999,987 items, 19,999,981 before; six of them read, at 18,088,343 items and 0.78 GB, 17.4 million items and 0.77 GB before) | 0.80 GB | 0.80 GB |
// | The review's input A, a path whose first point is a sum of 131,069 bytes and then 100, 1,000 and 4,000 closes that each draw a line back to it (`memory_a_long_formula_closed_a_thousand_times`; files of 3 KB, read; the point is a value, so a close holds no copy; 1,000 closes aborted the process before, at 2.6 GB for 800) | 0.01, 0.01, 0.02 GB | 0.01 GB for each |
// | The review's input B, 800, 1,000 and 1,900 custom shapes each with one equation of 131 KB, 65,535 nodes (`memory_shapes_each_with_a_long_equation`; refused at 19,919,829 items, 19,919,309 before, after about 118 shapes of some 168,000 items each; the tree is charged at 128 bytes a node; 1,000 aborted the process before; the input is dropped before the read is measured, the figures of held the test's own copies of it too and were 0.75, 1.06 and 1.55 GB) | 0.55, 0.80, 1.07 GB | 0.55, 0.80, 1.07 GB |
// | One custom shape with 1,900 equations, each a flat chain `1+1+...+1` of 131,069 bytes (`memory_one_shape_of_flat_chain_equations`; a 1.2 MB file and a 250 MB content part; no tree is built, and each equation compiles to 131,069 operations of 16 bytes, held with the others until the shape's equations are evaluated; refused at 19,996,898 items; a charge of the text alone aborted the process) | 1.75 GB | 1.74 GB |
// | 3,400 custom shapes of 100 `T` arcs of no sweep, each with six parameters of 119 characters (`memory_and_time_arcs_of_no_sweep`; a 4.6 MB file and a 248 MB content part; no arc makes a row; read in 7.8 to 9.5 s against 1.4 to 2.4 s for the same shapes with plain numbers (four runs at a load of 39 to 52; 3.8 s against 0.8 s before, and the ratio is about the same), the expressions compiled once and charged, 12 items an arc, 8.36 million items in all; the plain-number control peaks higher, at 1.00 GB, and 4.28 million items) | 0.52 GB | 0.52 GB |
// | A group of 400,000 unshared rectangles, and 250,000 glued connectors with text, each to the item cap (`memory_a_group_and_connectors_at_the_item_cap`) | 1.23 GB, 1.43 GB | 1.21 GB, 1.41 GB |
// | 3,000 custom shapes each with a 2,000-point glue list, a path of 240 commands of every kind, a handle, a text area and text, to the item cap (`memory_custom_shapes_of_every_row_kind_at_the_item_cap`) | 1.19 GB | 1.20 GB |
// | Every budget at once with the costliest of the new kinds, 14 custom shapes of 10,000 equations, in place of the two polygons (`memory_every_budget_at_once_with_equations`; read, at 9,733,255 items, so the item cap is not reached: this row said 7.6 million items fit beside the filler markup, 4.3 million of them the equations, before a change charged a formula by its nodes; the package's 970 MB of media are all read, as in the last row) | 2.21 GB | 2.2 GB |
// | Every budget at once with long formulas, 116 custom shapes of one equation of 131 KB in place of the two polygons (`memory_every_budget_at_once_with_long_formulas`; 19,825,961 items; at 118 shapes the cap refuses the file, at 2.19 GB, and at 30 and 60 shapes the peaks are 1.87 and 1.98 GB) | 2.18 GB | 2.19 GB (2.20 GB at 118 shapes, 1.88 and 1.99 GB at 30 and 60) |
// | every budget at once: the package at 1 GiB, all of it parts the read reads (a 250 MB picture shown by four frames and three of 240 MB shown by one frame each: 970 MB of media), both trees near the cap, the text at its cap, the font family named by 4,000 styles, two concave polygons of 154,999 corners (159,999 before the slots, which would pass the cap now; the costliest items, 54 bytes) and 2,700 unshared rectangles taking the items to 19,910,738 (19,830,344 before the slots) (`memory_every_budget_at_once`; the review's 5.59 GB combination; **read, with a peak of 2.66 GB during the read and 0.14 GB before it**; **made a read inflate only the parts the drawing names, so the 720 MB this case held as parts nothing reads cost nothing from then on and the row read 1.99 GB (1.98 GB on beast, 2026-11-19) while it kept them: the 2.70 GB it held before is reproduced, at 2.66 GB, once those parts are pictures that frames show. Has a read inflate and keep every file again (as parts of the model, which share the package's bytes as the pictures do), so a package of 720 MB of such parts is held as the 2.66 GB case is and no more: this row was not measured again**; `write_memory.rs` builds the same file so the save of it starts from this read: see the table of the writer below) | 2.66 GB | 1.99 GB (2.70 GB before a change) |
//
// Every budget at once stays under the 3 GB the plan asks a hostile file to fit in. A caller with less memory passes lower limits: every
// field of [`Limits`] can be lowered.

/// The VSDX reader's cap (`diacraft_vsdx::MAX_ITEMS`): items the reader keeps in the model, summed over the document. Here (plan Ruling
///) each item stands for 64 bytes of the model: each cell, row, section and shape by the size of its type, text, names and kept strings
/// by their bytes at nine eighths (`read::cost::text`), and each element, attribute and text node of the markup it keeps without modelling it
/// (`read::cost`), and each formula a cell holds by the nodes of its parse (128 bytes a node, the formula crate's own figure) and its text, when it is parsed (a cell
/// of DiaCraft's data by what its sheet reader prices it at). A file past it is
/// refused. Measured by the growth of the process, the model cost 40 to 54 bytes an item (the table above); **counted** (
/// `tests/it/memory_counted.rs`, what the model and its index keep, byte by byte and allocation by allocation, by the law of the memory tests), no kind of the 40
/// measured needs more than 64 once the reader charges each what it keeps (`read::cost::priced`: a user-defined field, a package entry, a page, a part of a text, a
/// kept node's place, a connector's route marker and a `Connect` row were charged too few), so the cap holds the model to 1.28 GB.
///
/// **A stated cap on path points.** A geometry row costs its honest 29 items (the row and its node of cells, 21, and its
/// two cells, 8; about 1.7 KB measured), so the cap holds about 690,000 path points a document: **700,000 at 28 items, until the model's rows gained a
/// slot for their extension data, and accepts the 21 and the 29.** The review's stand-in, 35,191 `draw:path`
/// shapes of 46 commands each (a 12 MB `content.xml`), was refused as `too_many_items` at shape 13,108 at 28 items a point (that file is not kept, so it has
/// not been read since the slots); this crate's own (`memory_path_heavy_drawing`, a move and 45 lines each) reads 14,224 and refuses the 14,225th, at 1.03 GB
/// (measured on beast, 2026-11-19, release; 14,705 and the 14,706th before). A lighter row in the model would lift this; until then a drawing of more path
/// points than that is refused. The refusal says so (`LimitKind::Items`, whose words a test holds): that path points count, and that each counts as 29.
pub const MAX_ITEMS: usize = 20_000_000;

/// The VSDX reader's cap (`diacraft_vsdx::MAX_CELLS`): cells, summed over every sheet of the document.
pub const MAX_CELLS: usize = 16_000_000;

/// The VSDX reader's cap (`diacraft_vsdx::MAX_SHAPES`): shapes the reader builds, summed over the pages.
pub const MAX_SHAPES: usize = 2_000_000;

/// Path commands and coordinate pairs in one page, over every `svg:d` and `draw:points` of its shapes. Past it,
/// `xml_too_complex`.
pub const MAX_PATH_COMMANDS_PER_PAGE: usize = 1_000_000;

/// Coordinate pairs in a single `draw:points`. Past it, `xml_too_complex`.
pub const MAX_POINTS: usize = 1_000_000;

/// The VSDX reader's cap (`diacraft_vsdx::MAX_MEDIA_BYTES`): bytes of pictures copied into the model, summed. A picture past it is left out
/// and counted, and its shape shows none.
pub const MAX_MEDIA_BYTES: u64 = 1024 * 1024 * 1024;

/// The VSDX reader's cap (`diacraft_vsdx::MAX_SMALL_PART_BYTES`): the largest `meta.xml` or `settings.xml` read. A larger one is left unread
/// and reported, not truncated.
pub const MAX_SMALL_PART_BYTES: u64 = 4 * 1024 * 1024;

/// Labelled choice: bytes of text the shapes of one document hold, after white space is collapsed and `text:s` counts
/// are expanded. The census's largest `content.xml` is 23.5 MB, all of its text included, so this is nearly three times that; a count of
/// spaces is the only way text grows past its markup, and 19,000 `text:s` of 65,536 would otherwise be 1.2 GB. Past it, `too_much_text`.
pub const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// Labelled choice: the most items (elements, attributes and texts, counted as the container counts them) in a tree the
/// drawing's reader parses, `content.xml` or `styles.xml`. A tree costs about 210 bytes an item (`memory_two_trees_at_the_item_cap`), so the
/// container's own 6,000,000 would be 1.3 GB beside the package and the model; this keeps one tree near 0.85 GB. The census's largest
/// `content.xml` is 23.5 MB, about a million items. The package layer and the comparison keep the container's cap.
pub const MAX_READ_XML_NODES: usize = 4_000_000;

// # A flat drawing (`.fodg`)
//
// A flat drawing is one XML part, read within these same caps, so a read holds no more than for a package: a file of more than
// `container.max_part_bytes` (256 MiB) is `part_too_large` before it is parsed, its one tree is capped at [`MAX_READ_XML_NODES`] items (`xml_too_complex`
// past them), the container's depth cap and attribute cap hold, and its bytes count against [`MAX_XML_BYTES`]. Its pictures are inline, and each is measured
// from its characters before it is decoded, against [`MAX_MEDIA_BYTES`], as an inline picture of a package is. The cost is counted, not timed
// (`tests/it/flat_costs.rs`): the work of a read grows with the pictures and the levels, step for step. A save of a flat drawing scans it a fixed number of
// times (the pass of each kind of part, the last pass, the check of the bytes made) and copies it once for each pass, whatever its size, and is held to the
// same estimate of the memory it holds (`MAX_WRITE_MEMORY`) with the document as the one part. The measured peaks under `ulimit -v 3000000`
// (`tests/it/fodg_memory.rs`; the process's peak, measured on beast on 2026-11-19, release, the file made read back after the save): 300,000 rectangles saved 1.80 GB
// (2026-11-22, the test profile: at 64 bytes an item the gate admits 302,000 of them and refuses 303,000; 350,000 saved 2.04 GB at 54)
// (1.85 GB before); one inline picture of 150 MB of base64 saved 0.86 GB (0.86 GB before); a package of a 100 MB picture saved as a flat document 0.80 GB (0.80 GB before). A package written as a flat document holds each picture several times over (inflated, as characters, in the part they are
// spliced into and in the document), so its estimate is made from the sizes the archive states before any picture is inflated, and at most 200,000 pictures
// go inline in one conversion.

// # The writer (`write_with`)
//
// A save reads the original again (the reader's caps hold, lowered to what is left of [`MAX_WRITE_MEMORY`] beside the caller's model), compares
// the model with it, scans the parts a change reaches and splices the edits into their bytes. Every loop of the writer is counted in the unit it walks (`WriteWork`) and charged against one of these before it runs, as
// the reader's are; a cap passed refuses the save with `WriteError::Limit` and nothing is written.
//
// ## The most a save can cost
//
// A save holds the caller's model, reads the file again beside it and makes the package, so its peak is more than a read's. **Measured on beast on 2026-11-19 (
// main 63f8a4db), release build, at a load of 16 to 22 on a machine shared with other builds** by the children of `tests/it/write_memory.rs`, `fodg_memory.rs`, `picture_memory.rs` and
// `write_zero_memory.rs` (the engine's, `odg_memory.rs` and `odg_hold.rs`, are in the header of `crates/engine/tests/it/odg_memory.rs`), each run on its own, one at a time, free and again under `ulimit -v 3000000`:
// every case finishes in both, or is refused in words before anything is written (the ones that say so). The peak is the process's own (`VmHWM`, a GB is the kB it prints divided by a
// million; the child builds its input in the same process, so it is the larger of the building, the read and the save), and a free run and a limited one differ by 0.1 percent at most
// above 50 MB: a peak does not depend on load. The gate is the estimate the save made before it read the file again (`WriteWork::estimated_memory`), against the cap of 2.9 GB. Every save
// that was made is read back through the reader, with the model let go, and reads. The last column is what the case stated before this measure.
//
// | Case | Gate | Ended | Peak | Before |
// |---|---|---|---|---|
// | the reader's every-budget file with one rectangle that moves (`write_memory.rs`, `every_budget_refused` and `every_budget_default_index`): the package at its 1 GiB, **all of it read** (970 MB of pictures that frames show, 1,037,696,442 bytes inflated in all), 19,910,790 items, a tree of 4,000,000 | 6.11 GB | refused as `write_limit` before the file is read again (nothing scanned, copied or compared); with the index nothing read, as `write_mismatch` before a byte is read again | 2.66 GB (2.75 GB of address space: the first read's) | 1.98 GB and a gate of 3.97 GB: **the file held 720 MB of parts nothing names, which a read does not inflate, so it was no package at its 1 GiB** |
// | 300,000 rectangles (`rects_300k_saves`; 16,500,627 items; measured on beast on 2026-11-22, the test profile) | 2.82 GB | saved; the file read again (16,500,627 items), 60.3 MB scanned and 60.3 MB copied | 1.78 GB (2.01 GB of address space) | 350,000 rectangles, 18,200,395 items: a gate of 2.79 GB at 54 bytes an item and 1.99 GB (2.19 GB of address space); **at 64 the gate admits 308,000 (16,940,627 items, 2.90 GB) and refuses 309,000**, where it admitted about 349,000 |
// | 250 shapes each with a foreign attribute of 1 MB of random letters and 175 custom shapes of every row kind (`kept_random_250`; a 189 MB file, a `content.xml` of 254 MB, 16,614,946 items, a tree of 5,094) | 3.38 GB | refused as `write_limit` before the file is read again | 1.37 GB | aborted at 2.75 GB before a change |
// | the same with the gate open (`kept_random_250_open`) | 3.38 GB | saved; read again (16,614,946 items), 508.7 MB scanned and 508.7 MB copied; at `ulimit -v 2600000` the edit does not fit and the save ends as `out_of_memory` before anything is written (2.11 GB, 2.36 GB of address space) | 2.73 GB (**2.98 GB of address space, in the 3.07 GB the limit gives: 15 MB to spare**) | 2.98 GB of address space |
// | 100 such shapes and 175 custom shapes (`kept_random_100`; a 76 MB file, 14,263,096 items) | 2.19 GB | saved; 208.7 MB scanned and 208.7 MB copied | 1.81 GB | none stated |
// | 150,000 rectangles (7,800,395 items) and then 1.94 GB of text put into their cells after the read (`grown_by_text_refused`) | 3.13 GB | refused as `write_limit` before the file is read again and before a shape is compared | 2.17 GB | none stated |
// | 350,000 rectangles and then 1,200 `User` rows named 1 MiB each, 1.26 GB of names (`rows_grown_refused`) | 4.05 GB | refused as `write_limit` before the file is read again | 1.92 GB | aborted at the second read before the rows were counted |
// | one paragraph of 1,000 `text:s` of 999,999, read as 65 MB of spaces, then retyped (`retext_many_spaces`) | none (refused in the retext) | refused as `write_unsupported` before anything is built | 0.20 GB | none stated |
// | one paragraph of 60 MiB of raw white space, retyped (`retext_huge_white_space`) | none stated | saved | 0.32 GB | 0.32 GB |
// | 230,000 rectangles, a `content.xml` of 23 MB, 11,960,395 items (the ignored `a_save_of_a_23_mb_part_at_its_start_and_at_its_end`, run under the limit) | 1.83 GB | saved twice (a move of the first shape, then of the last), each scanning and copying 46.1 MB | 1.33 GB (the read 0.89 GB, the first save adds 0.69 GB) | none stated |
// | a flat document of 300,000 rectangles (`fodg_memory.rs`, `rects_300k_saves`; the test profile) | 2.87 GB | saved | 1.80 GB | 350,000 at 2.04 GB; **at 64 bytes an item the gate admits 302,000 and refuses 303,000**, where it admitted about 342,000 |
// | a flat document of one inline picture of 150 MB of base64, with the gate shut and open (`picture_150mb`) | 1.14 GB | saved | 0.86 GB | 0.86 GB |
// | a package of a 100 MB picture saved as a flat document (`package_of_big_pictures`) | none stated | saved | 0.80 GB | 0.80 GB |
// | one new picture of 256 MiB less 1 KiB, and the same from a template saved as a drawing (`picture_memory.rs`) | 0.27 GB | saved | 0.80 GB | 0.80 GB |
// | three new pictures of 250 MB | 0.75 GB | saved | 1.72 GB | 1.72 GB |
// | four new pictures of 250 MB | 1.00 GB | refused as `write_limit` | 1.23 GB | none stated |
// | a paragraph of 1,300,000 bookmarks retyped (`write_zero_memory.rs`, `marks_1_3m`) | 1.18 GB | saved: 7,800,006 steps on zero-width elements, 48.3 MB tokenized, 144.9 MB copied | 0.83 GB | 0.84 GB at the most of the three |
// | 600,000 paragraphs that each hold a bookmark, taken out but one (`paragraphs_taken_out_600k`) | 1.04 GB | saved: 25,688,895 steps, 22.1 MB tokenized, 64.5 MB copied | 0.78 GB | see above |
// | 50,000 fields whose characters change (`fields_changed_50k`) | 0.07 GB | saved: 300,002 steps, 1.35 MB tokenized | 0.04 GB | see above |
//
// The gate is not a bound of the peak where the cost is in pictures the save copies: three new pictures of 250 MB have a gate of 0.75 GB and peak at 1.72 GB, because the memory of the
// deflated copy and the archive is taken from the save's room before either is made (`picture.rs`) and not counted by the estimate, which is why four of them are refused with a
// gate of 1.00 GB. For a save of a model the estimate is above the peak in every case measured (1.2 to 1.7 times).
//
// **(2026-11-22) holds the model at 64 bytes an item** (`index::ITEM_BYTES`, the reader's own figure, where it was 54): a gate counts the model twice, so
// each is 20 bytes more for each item the file holds, and the gates of the table not measured again are, computed: the every-budget file 6.51 GB (6.11), `kept_random_250`
// 3.71 GB (3.38), `kept_random_100` 2.48 GB (2.19: still saved), `grown_by_text_refused` 3.29 GB (3.13), `rows_grown_refused` 4.41 GB (4.05) and the 23 MB part 2.07 GB (1.83), a
// little more where the reader charges a kind more items since (a page, a part of a text, a kept node's place). The saves the gate admitted at 54 and refuses at 64 are the
// rule's: of rectangles of a page, those of 308,001 to about 349,000 in a package and of 302,001 to about 342,000 in a flat document. The peaks do not move.
//
// **(2026-11-22) charges text at nine eighths of its length** (`read::cost::text`), and a cell of DiaCraft's data what its sheet reader prices it at.
// Measured again on beast (the test profile, old figures on main f07df887 beside): a rectangle holds no text, so 300,000 of them are 16,500,631 items (16,500,627) and the save's
// gate 2.82 GB as before (the most a save admits does not move); `kept_random_100` 14,469,097 items and a gate of 2.50 GB (14,265,392 and 2.47 GB: still saved);
// `kept_random_250` 17,114,347 and 3.78 GB (16,617,692 and 3.71 GB: still refused, and saved under 3 GB of address space); the every-budget file 19,802,339 and 6.36 GB
// (19,638,655 and 6.34 GB: still refused). A drawing of text alone holds a ninth less of it under the item cap; the text cap itself is in bytes and does not move.

/// Labelled choice: bytes of parts the writer scans, and bytes it copies, each summed over one save. The package may inflate to 1 GiB
/// (the container's cap) and a part to 256 MiB, so a save that scans every part once and copies every byte once is within it; a scan or a copy
/// past it is refused before it starts.
pub const MAX_WRITE_BYTES: u64 = 1024 * 1024 * 1024;

/// Labelled choice: edits to one part in one save. An edit is about 64 bytes while it waits for the sweep, so this holds the edits
/// near 256 MB; a shape's move is one or two edits, a reshape four to six.
pub const MAX_WRITE_EDITS: usize = 4_000_000;

/// Labelled choice: attributes written (a value replaced, an attribute added or taken out), summed over one save.
pub const MAX_WRITE_ATTRIBUTES: usize = 4_000_000;

/// Labelled choice: steps of comparing the model with the original read again, summed over one save: shapes compared, cells compared
/// and steps along a chain of namespace declarations. The reader's own caps bound the model (2,000,000 shapes, 16,000,000 cells); this is a
/// few times that, so that a model of the largest the reader admits compares, and nothing built to take longer does.
pub const MAX_WRITE_STEPS: u64 = 400_000_000;

/// Labelled choice: bytes of memory a save may be estimated to hold, so that it is refused with
/// [`crate::WriteError::Limit`] and never runs out of memory. A save holds the caller's model (the one the caller read) while it reads the file
/// again to compare the model with, so at its peak it holds the model twice and what a read holds beside a model: a tree and the parts
/// inflated ([`crate::ReadCost::save_bytes`], from the cost the read left on the index), and beside them the file, its largest part and the
/// walks of the comparison. The reader's caps let one read hold up to 2.66 GB at its worst (the table above: 19.9 million items, a tree of 4
/// million and 970 MB of pictures), and **the estimate of a save of that file is 6.51 GB** (the table of the writer below: the model twice, 4.49 GB at 64 bytes an item, the
/// tree and the parts inflated, 1.88 GB, and 0.15 GB beside them; computed from the measure of beast on 2026-11-19, release, of 6.11 GB at 54), more than twice the cap,
/// so a save of the worst file the reader admits is refused before it reads. (This said that 2.7 GB of read needs 4.2 GB of save, from a file
/// that held 720 MB of parts nothing names, which a read does not inflate: the estimate of that file is 3.97 GB, and it was no worst file.) A
/// drawing whose `content.xml` is 23 MB of rectangles, the size of the census's largest, is estimated at 2.07 GB (1.83 at 54 bytes an item) and peaks at 1.33 GB. The cap
/// is the 3 GB the plan asks a hostile file to fit in, less what the process holds beside the drawing; the estimate is 1.6 times the peak
/// measured for a file of 16.5 million items (300,000 rectangles: 2.82 GB estimated, of which 2.65 GB is [`crate::ReadCost::save_bytes`], and 1.78 GB measured;
/// 2026-11-22, the test profile), and the most rectangles of that kind it admits is 308,000 in a package and 302,000 in a flat document (about 349,000 and 342,000 when an
/// item was 54 bytes, and the estimate was 1.4 times the peak of 350,000), so a file the cap admits is under 3 GB with room to spare. A caller with less memory lowers it, as it
/// lowers every field of [`Limits`].
pub const MAX_WRITE_MEMORY: u64 = 2_900_000_000;

/// Labelled choice: the largest thumbnail the writer writes, in bytes. The thumbnail is at most 256 pixels on the long side
/// A few kilobytes of PNG; a picture past this is not one.
pub const MAX_THUMBNAIL_BYTES: usize = 4 * 1024 * 1024;

/// What the reader and the comparison accept. [`Limits::default`] holds the constants above and the container's, and
/// `Limits { max_manifest_entries: 10, ..Limits::default() }` changes one and keeps the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The archive and every XML tree: the container's own.
    pub container: diacraft_container::Limits,
    /// [`MAX_MANIFEST_BYTES`].
    pub max_manifest_bytes: u64,
    /// [`MAX_MANIFEST_NODES`]. The least of this and `container.xml.max_nodes` holds.
    pub max_manifest_nodes: usize,
    /// [`MAX_MANIFEST_ENTRIES`].
    pub max_manifest_entries: usize,
    /// [`MAX_XML_BYTES`].
    pub max_xml_bytes: u64,
    /// [`MAX_REPORT_ENTRIES`].
    pub max_report_entries: usize,
    /// [`MAX_DIFF_ENTRIES`].
    pub max_diff_entries: usize,
    /// [`MAX_ITEMS`].
    pub max_items: usize,
    /// [`MAX_CELLS`].
    pub max_cells: usize,
    /// [`MAX_SHAPES`].
    pub max_shapes: usize,
    /// Pages, background pages included: `diacraft_doc::MAX_PAGES`, and never more.
    pub max_pages: usize,
    /// Style sheets: `diacraft_doc::MAX_STYLES`, and never more.
    pub max_styles: usize,
    /// Shapes on one page, unmodelled ones counted: `diacraft_doc::MAX_SHAPES_PER_PAGE`, and never more.
    pub max_shapes_per_page: usize,
    /// [`MAX_PATH_COMMANDS_PER_PAGE`].
    pub max_path_commands_per_page: usize,
    /// [`MAX_POINTS`].
    pub max_points: usize,
    /// [`MAX_MEDIA_BYTES`].
    pub max_media_bytes: u64,
    /// [`MAX_SMALL_PART_BYTES`].
    pub max_small_part_bytes: u64,
    /// [`MAX_TEXT_BYTES`].
    pub max_text_bytes: usize,
    /// [`MAX_READ_XML_NODES`]. The least of this and `container.xml.max_nodes` holds for the reader's trees.
    pub max_read_xml_nodes: usize,
    /// [`MAX_WRITE_BYTES`].
    pub max_write_bytes: u64,
    /// [`MAX_WRITE_EDITS`].
    pub max_write_edits: usize,
    /// [`MAX_WRITE_ATTRIBUTES`].
    pub max_write_attributes: usize,
    /// [`MAX_WRITE_STEPS`].
    pub max_write_steps: u64,
    /// [`MAX_THUMBNAIL_BYTES`].
    pub max_thumbnail_bytes: usize,
    /// Bytes of memory a save may be estimated to hold: see [`MAX_WRITE_MEMORY`].
    pub max_write_memory: u64,
    /// **The threads a read may use**, its own counted: 1, the default, makes no thread. More inflates the files of the package that the drawing names or keeps (its pictures and
    /// the parts nothing reads) on threads, each held to the size the directory states for it; **the document and every refusal are the single-threaded read's at any count**.
    pub threads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            container: diacraft_container::Limits::default(),
            max_manifest_bytes: MAX_MANIFEST_BYTES,
            max_manifest_nodes: MAX_MANIFEST_NODES,
            max_manifest_entries: MAX_MANIFEST_ENTRIES,
            max_xml_bytes: MAX_XML_BYTES,
            max_report_entries: MAX_REPORT_ENTRIES,
            max_diff_entries: MAX_DIFF_ENTRIES,
            max_items: MAX_ITEMS,
            max_cells: MAX_CELLS,
            max_shapes: MAX_SHAPES,
            max_pages: diacraft_doc::MAX_PAGES,
            max_styles: diacraft_doc::MAX_STYLES,
            max_shapes_per_page: diacraft_doc::MAX_SHAPES_PER_PAGE,
            max_path_commands_per_page: MAX_PATH_COMMANDS_PER_PAGE,
            max_points: MAX_POINTS,
            max_media_bytes: MAX_MEDIA_BYTES,
            max_small_part_bytes: MAX_SMALL_PART_BYTES,
            max_text_bytes: MAX_TEXT_BYTES,
            max_read_xml_nodes: MAX_READ_XML_NODES,
            max_write_bytes: MAX_WRITE_BYTES,
            max_write_edits: MAX_WRITE_EDITS,
            max_write_attributes: MAX_WRITE_ATTRIBUTES,
            max_write_steps: MAX_WRITE_STEPS,
            max_thumbnail_bytes: MAX_THUMBNAIL_BYTES,
            max_write_memory: MAX_WRITE_MEMORY,
            threads: 1,
        }
    }
}

impl Limits {
    /// The limits of the manifest's tree: the container's, with the item limit the smaller of the two.
    pub(crate) fn manifest_xml(&self) -> diacraft_container::XmlLimits {
        diacraft_container::XmlLimits { max_nodes: self.container.xml.max_nodes.min(self.max_manifest_nodes), ..self.container.xml.clone() }
    }
}
