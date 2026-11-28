//! DiaCraft VSDX: reads a VSDX package into the ShapeSheet document model, and writes one back.
//!
//! [`read`] takes the bytes of a `.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx` or `.vstm` file and gives a [`diacraft_doc::Document`] and a
//! [`ReadReport`], or says why not ([`VsdxError`], each with a class word). It is the second door for hostile files after `opc`
//! (`crates/opc`), which reads the container; everything here is about what is inside.
//!
//! # What it promises
//!
//! * **The document is the file's.** A cell is read as the strings the file gave: `F` (parsed against a per-document budget of formula
//!   nodes and kept as text whether or not it parses), `V`, `U` and `E`. A cell is never evaluated and never normalised
//!   What the model has no typed field for is in `RawAttrs` and `Passthrough`, and
//!   [`passthrough`] says where each kind goes and which of two places wins.
//! * **Nothing is believed.** Every cap (`limits.rs`) is checked before what it counts is stored, so a file past one is refused whole, not
//!   half read, and a hostile name, `ID` or link (a duplicate, a dangling reference, a cycle of styles) is read as it is and warned of,
//!   and cannot make the reader loop: see `read.rs` for what each does.
//! * **Both namespaces are one vocabulary** (`ns.rs`): `2012/main` and `2011/1/core` are read alike, and an element of another namespace is
//!   kept and never read as a VSDX element, whatever its local name.
//! * **Nothing is fetched or run.** A relationship to a URL, a `ShortcutURL`, a data connection and a VBA project are left unread: this
//!   crate opens no file and no connection, and parses no code. (Reading a file from disk is the engine's, `diacraft_engine::file`.)
//! * **The model holds everything the file has that no field of it reads**. The model has fields for what the specs name and
//!   `raw` and `Passthrough` for unknown attributes and elements; everything else is held beside them: the parts of the package the model does not rebuild, the
//!   relationships and content-type rows no field says, the rest of the property parts, the custom properties and `windows.xml` (`held.rs`), and the
//!   elements and attributes of each place the reader types and gives no field to, in the extension slots (`unheld.rs`). Nothing is skipped
//!   (`ReadReport::skipped_counts` is empty), and a test injects each kind of thing at every position of a package and fails on one that is not in the model.
//!   All of it is charged to the read's memory budget before it is kept, so a file whose held parts the pool cannot hold is refused in words.
//! * **A model that holds all of that is written back from the model alone** (`write/held.rs`): `write_with(doc, None, options)` writes the held parts from the bytes the model holds, every
//!   relationship row under its `Id`, the pages and masters under the part names and ids they had, the slots, the properties, `windows.xml` and the content types, so that a `.diacraft` file (which holds the model and
//!   not the file it came from) is saved as a `.vsdx` with the same content (`compare_content`).
//! * **A save may use threads** ([`write_with_threads`]): the page and master parts that an edit touched are patched together and the parts of the package are compressed on workers,
//!   and the package is **the same bytes at any number of threads**. [`write_with`] is one thread.
//! * **A theme part can be written**: [`theme_part_bytes`] makes the DrawingML part of a [`ThemeDef`], the inverse of what the reader takes
//!   from one, and a save adds, rewrites and takes out the theme parts of the model (`write/themes.rs`) while every other part stays the original's.
//! * **Both writers carry the data of the other format** (`carry.rs`): what the model holds of an `.odg` beside its cells, and the parts and custom properties of
//!   another format, are items in DiaCraft's solution part with the bytes in parts of DiaCraft's own, and a read keeps an item only while its stamp still matches what the file says of
//!   its object.
//!
//! # The harness is not here
//!
//! The oracle harness (`diacraft-cli oracle FILE --json`) joins this reader to `diacraft-calc`'s comparer in the app layer, with the file's
//! hash, the spec's name lists and the JSON it prints. This crate compares nothing and scores nothing: what it gives the
//! harness is [`read_partial`], the [`ReadReport`], the class words of [`VsdxError`], and [`part_label`] for naming a part safely.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod ahead;
mod canon;
mod carry;
pub mod comments;
pub mod cost;
mod error;
mod held;
mod holds;
#[cfg(kani)]
mod kani_proofs;
pub mod limits;
mod ns;
mod page_follow;
mod page_hrefs;
mod page_names;
pub mod passthrough;
mod plain_languages;
mod props;
mod read;
mod reader;
mod report;
mod shape;
mod sheet;
mod string_units;
mod theme;
mod theme_entries;
mod theme_write;
mod unheld;
pub mod work;
mod write;

pub use canon::{CANON_RULES, compare_content, compare_content_counted};
pub use diacraft_opc::ErrorKind;
pub use error::{LimitKind, VsdxError};
pub use holds::{Holds, HoldsWork, holds, holds_counted, holds_of};
pub use limits::{
    Limits, MAX_CARRY_STEPS, MAX_CELLS, MAX_FORMULA_NODES, MAX_ITEMS, MAX_MEDIA_BYTES, MAX_MEMORY, MAX_REPORT_ENTRIES, MAX_SHAPES,
    MAX_SMALL_PART_BYTES, MAX_SOLUTION_PART_BYTES, MAX_THEME_BYTES, MAX_THEMES, MAX_XML_BYTES,
};
pub use ns::Ns;
pub use page_names::{KeptNames, Renamed, keep_page_names};
pub use plain_languages::plain_languages;
pub use read::{read, read_partial};
pub use report::{CarryReport, DiacraftPart, Kind, NamespaceUse, ReadReport, RefusedAt, Skipped, Stats, Variant, check_extension};
pub use string_units::{Fitted, string_units, string_units_in_file};
pub use theme::{ThemeRead, read_theme_part, read_theme_part_reported};
pub use theme_write::{ThemeDef, ThemeWriteError, theme_part_bytes};
pub use unheld::part_label;
pub use write::{Carried, OriginalBytes, WriteError, WriteOptions, Written, new_windows_part, write, write_with, write_with_threads};
