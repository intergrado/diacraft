//! DiaCraft's own file format, `.diacraft`: the package of the model (the public specification `docs/diacraft-format.md`, which is the authority for every name, default and vector here).
//!
//! A `.diacraft` file is a ZIP package that holds the whole model: the document part, one part for each page and each master (the XML of `diacraft_sheetxml::model`), the
//! blobs (pictures, themes, held package parts) **stored once**, and `state.xml` with the identifier ledger and the content stamp. It never holds the file it came from.
//!
//! # What is here
//!
//! * [`sniff`] recognises a file by the fixed places of its first entry, a stored `mimetype` (the spec's 2.3), and [`Kind`] names the three kinds.
//! * [`read`] opens a package within [`Limits`]: it checks the file's length, the directory, the manifest, every part's items and the sizes the blob records state **before
//!   it allocates**, reads a part at a time, and refuses in words what is damaged or too large. **Damage confined to blobs does not refuse the file**: the pictures it
//!   loses are noted ([`ReadNote`]) and the rest opens. It returns the model, and [`Opened`] with the [`State`] and what a later write copies from ([`Decoded`]).
//! * [`write`] writes a model: the entries in one fixed order, at one fixed deflate level, the same bytes for the same model; blobs numbered by first use and stored once
//!   ([`BlobTable`]); and, given the package it was read from or last written as ([`Previous`]), **an entry is copied as it is when what it holds did not change**: a
//!   save that changed nothing writes the file it read, and one that changed a page rewrites that page's part only. A part is copied only when every blob it names is
//!   still the very allocation it was, since blobs are numbered by first use.
//! * [`write_threads`] is [`write`] **on threads**: the entries are deflated on workers while the next part is made, the codec makes the parts of its first pass together and
//!   holds the ones that fit for the second, what each holds in flight is charged to the write's own meter before it is made, and **the package is the same bytes at any number of threads**.
//! * What a newer version of the format wrote is kept: a part of a role this version does not know is held in the model as a package part of format
//!   `Native` and written back at its own path with its manifest row (the spec's 5.2); unknown attributes and elements inside a model part are the codec's (5.3).
//!
//! # Never crash
//!
//! Every byte is hostile. The file's length is capped before it is read; the entries, their sizes, the ratio and the names by the directory before anything is inflated;
//! each XML part is held to the part cap and parsed at the items and depth the codec's open allows, the items of every part summed and held to what an open may read; the
//! model is charged to the codec's budget, whose pools are sized from [`Limits::max_memory`] before anything is made; a blob is inflated only when it is named and only to
//! the size its record states, and is never parsed, opened as a package or run. What does not fit is refused in words that name what is large, and memory that runs out is
//! an error and not an abort.
//!
//! The VSDX application was not inspected: the format is DiaCraft's own and is written from its public specification.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod blobs;
mod decoded;
mod error;
mod kind;
mod limits;
mod manifest;
mod names;
mod read;
mod state;
mod write;
mod xmlns;

#[cfg(kani)]
mod kani_proofs;

pub use blobs::{BlobTable, stored};
pub use decoded::Decoded;
pub use error::NativeError;
pub use kind::{DEFLATE_LEVEL, FORMAT, Kind, sniff};
pub use limits::{Limits, MAX_MEMORY};
pub use read::{Opened, ReadNote, read};
pub use state::{Ledger, State};
pub use write::{Previous, WriteOptions, Written, write, write_threads};
