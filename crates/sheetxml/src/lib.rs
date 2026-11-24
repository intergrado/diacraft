//! DiaCraft ShapeSheet XML codec: the `Cell`, `Section` and `Row` elements of a sheet, read into the document model and written back, for the
//! VSDX package (`diacraft-vsdx`) and for the OpenDocument one (`diacraft-odg`).
//!
//! The model is the ShapeSheet: a sheet holds cells, sections and rows, and a cell holds a formula and a cached value with a unit. A file
//! format that carries it as XML carries it in these elements, and this crate is the one place that knows how: which attribute is a cell's
//! formula, which a row's index, what a cell with a text or a child inside it keeps, which cells are the first of their name, what a section
//! with no name is. It was carved out of `diacraft-vsdx` with no change in what it reads or writes.
//!
//! # What is here
//!
//! * **Reading** ([`read`]). A [`SheetReader`] holds the budgets of one document (cells, items, formula nodes) and the tallies a report wants,
//!   and reads a sheet through them: [`SheetReader::read_sheet_only`] for an element that is only a sheet, [`SheetReader::sheet_child`] for the
//!   child of an element that holds a sheet and other things (a shape, a style), [`SheetReader::put_cell_attrs`] for a cell that is given as
//!   attributes and is no element. Every cap is checked before what it counts is stored ([`LimitError`]).
//! * **Writing** ([`build`], [`patch`]). The inverse of reading: a sheet, section, row or cell as a tree ([`build::sheet_only_el`],
//!   [`build::cell_el`], [`build::cell_attrs`]), and the patch of a part's tree between two models that changes only what differs
//!   ([`patch::patch_sheet_children`], [`patch::patch_sheet_only`]), with the list patcher those use for the lists of a format's own
//!   ([`patch::patch_list`]).
//! * **What a writer that writes only what differs needs** ([`delta`]): [`cells_delta`] says which cells of a container a reader would not rebuild
//!   by itself, and [`sheet_delta`] which cells, sections and rows of a whole sheet, each by its key and by value ([`SheetDelta`]).
//! * **What the model keeps of the rest** ([`passthrough`]): unknown attributes, elements, comments and instructions, and the rules the model's
//!   typed fields follow so that a text comes back as it went in.
//! * **Which elements are the sheet's own** ([`ns`]): a [`Scope`] of the namespaces in force, and the [`Vocabulary`] that says which of them
//!   are read. An element of another namespace is kept and never read as a cell, whatever its local name.
//!
//! # What is not here
//!
//! No package, no part and no file name: which part holds a sheet is the caller's, and so is the report. What a format says about a file
//! besides the sheets goes through the caller's [`Diagnostics`]. The shape, its text and its pictures are not here either, in the modules above; a format
//! reads those itself and hands those modules the elements that are sheets. (The whole model, shapes and pictures included, is [`model`].)
//!
//! # The whole-model codec ([`model`])
//!
//! [`model`] is a codec of its own kind: it writes the whole [`diacraft_doc::Document`] (its pages, masters, styles, shapes, text, pictures and everything the
//! model keeps without a field) as trees of XML in DiaCraft's namespace, and reads them back so that `decode(encode(m)) == m` by value, for the `.diacraft`
//! package and for the carriers of DiaCraft's data in a VSDX file and an `.odg`. It shares with the modules above only their names for a
//! sheet's elements (`Cell`, `Section`, `Row` and their `N`, `V`, `F`, `U` and `E`): it does not use their reader, whose rules (a kept attribute that wins
//! over a typed one, a row with no `IX` given the count of the rows before it) are the formats' and not exact. Its vocabulary is [`model::vocab`]; byte
//! strings go through a [`model::BlobSink`] and come back through a [`model::BlobSource`]; every decode is charged to a [`model::Budget`] before it allocates.
//!
//! # For `.odg`
//!
//! A drawing in `.odg` carries DiaCraft's cells in a namespace of its own. A reader of it:
//!
//! * for **elements under a namespace**, makes a [`Vocabulary`] that reads that namespace and no other, enters a [`Scope`] with it, and gives
//!   [`SheetReader::read_sheet_only`] the element that holds the cells (its children are the cells and sections, its attributes the style
//!   links and the rest), or [`SheetReader::sheet_child`] each child of an element that holds more;
//! * for **attributes**, gives [`SheetReader::put_cell_attrs`] the name of each cell and its attributes (`F`, `V`, `U`, `E`, and any other
//!   attribute is kept as the file gave it), and the cell is read as the element would have been, at the same cost.
//!
//! A writer of it writes only what differs from what the reader would rebuild ([`cells_delta`] for the cells of one place, [`sheet_delta`] for a
//! whole sheet), the cells as attributes ([`build::cell_attrs`], for a cell where [`delta::fits_attributes`] holds) or as elements
//! ([`build::cell_el`], [`build::section_el`], [`build::sheet_only_el`]), and gives the elements it made the prefix of the namespace
//! ([`build::with_prefix_sheet`]; [`patch::patch_sheet_children_prefixed`] and [`patch::patch_sheet_only_prefixed`] do so for what the patcher adds).
//! **The prefix goes on the codec's own elements and no others:** a kept unknown node, in a tree that was just made or in one that was patched,
//! is left as it was.
//!
//! # Never crash
//!
//! Every byte read is hostile. The budgets are taken before what they pay for is stored, a name and the number of cells, rows and sections
//! have the model's caps, and nothing recurses on the file's depth except the copy of an unknown element, which is bounded by the container's
//! XML depth (`passthrough.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod build;
pub mod delta;
pub mod diag;
pub mod limits;
pub mod model;
pub mod ns;
pub mod passthrough;
pub mod patch;
pub mod price;
pub mod read;

#[cfg(kani)]
mod kani_proofs;

pub use delta::{CellDelta, SectionDelta, SheetDelta, cells_delta, sheet_delta};
pub use diag::{Diagnostics, Warnings};
pub use limits::{Caps, LimitError, LimitKind};
pub use ns::{Scope, UNBOUND, Vocabulary};
pub use patch::PatchError;
pub use read::{Cx, LookaheadHook, SHARING_ENTRY_BYTES, SheetReader, SheetStats, SheetTally, Took, style_link};
