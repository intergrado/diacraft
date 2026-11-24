//! DiaCraft OPC: a bounded, safe reader for the zip-based Open Packaging Conventions package that every `.vsdx` is.
//!
//! This is the front door for hostile files, and it is standalone (layer 0), with one workspace dependency: `diacraft-container`, which
//! reads and writes the ZIP archive and the XML trees for this crate and for the OpenDocument package alike.
//! [`Package::read`] takes the bytes of a file and either gives every part of it, with its content type and relationships, or says why
//! not ([`OpcError`], each with a [`class`](OpcError::class) word). It reads and checks, and
//! it writes: a [`PackageWriter`] makes the package from parts and a content types stream (legal names and methods, local and
//! central headers that agree, a type for every part), and [`xml::to_bytes`] writes a tree of [`xml`] back out. It knows nothing of the
//! drawing: which part is the document, what a page is, belongs to the `vsdx` crate.
//!
//! # What it promises
//!
//! * **Bytes come back as they went in.** [`Package::part`] gives a part's inflated bytes unchanged, XML or not, well-formed or not.
//!   The specification keeps unedited parts at the "bytes" level, and this is that. The tree of [`xml`] keeps
//!   attribute order, prefixes and `xmlns` declarations, whitespace, comments and instructions, for a part that is edited.
//! * **Names never reach a file system.** This crate opens no file and no network connection, and names are keys into a map. A name
//!   with `..`, a leading slash, a backslash or a drive letter is refused as `path_traversal`; two names that differ only in
//!   ASCII case are a `duplicate_part`; a name that is a path prefix of another part's is refused; a percent-escape of a
//!   letter, a digit or `-._~` is refused, and so is a name that decodes to `[Content_Types].xml`.
//! * **No DTD, no entity, nothing fetched**. XML with either is `xml_entity_refused`, before anything is expanded.
//! * **Nothing panics, and memory that runs out while a part is inflated is an error**, not an abort ([`OpcError::OutOfMemory`]).
//!   The buffers of inflated parts are the only large allocations made on a file's word, and they are grown with `try_reserve`.
//!   The directory's names and an XML tree use ordinary allocation, so they are bounded by limits and not by what is free: on a
//!   machine with less memory than their bound (the numbers below) they can still abort.
//! * **Nothing is believed that has not been checked against the bytes.** See the next section and the limits after it.
//!
//! # How the archive is read
//!
//! There is no ZIP library: the end record, the ZIP64 records and the directory rows are read from the public PKWARE APPNOTE in the
//! container crate (its `zipdir.rs`, moved there from this crate in with the reading unchanged), and the entries are inflated with
//! `flate2`. (The library this crate first used spent minutes on a file of a megabyte whose tail was end-record signatures,
//! and 690 MB on a file of 64 MB of empty extra fields, before any limit applied.) The container asks this crate, row by row, which rows
//! are parts and whether their names are part names (`archive.rs`); everything below holds as it did, in the same order.
//!
//! * **Offsets are from byte 0, and a file with bytes in front of its archive is refused.** A self-extracting stub or a prefix shifts
//!   every offset; none is guessed at. The end record is found, its offsets reach no signature, and the file is `zip_corrupt`.
//! * **The end record is found by one backward scan** of the last 65,557 bytes, with no second try: it is the last
//!   signature whose own comment length ends exactly at the end of the file; if there is none, it is the last signature whose
//!   comment ends *inside* the file, and the bytes after it and its comment are ignored, with a [`Warning::BytesAfterEndRecord`] that
//!   names how many. A signature whose comment runs past the end of the file does not fit. If no record fits: `not_zip`, or
//!   `zip_corrupt` if the file starts with a local header. (Before a change a file with bytes after its end record was refused. A
//!   count over the sample files found 3 of 1,906 files with the traits of the reference app's saves, all from one source, with 26 bytes of text after the end
//!   record, and 1 fuzzed file with 1 byte; no file of 2,536 with a readable directory has bytes in front, so those stay refused. Whether
//!   the reference app opens a file with bytes after its end record is `unknown`.) A comment that holds bytes that look like an end record
//!   cannot refuse a valid file, because its own comment length does not reach the end and a record that ends the file wins over one
//!   that only fits; one built so that it does end the file *is* the end record, and decides, like any other (a file built to be two
//!   archives gets one reading, and this is which).
//! * **An appended directory: the end record decides, and the rest is unread bytes.** An archive with a second directory and end record
//!   appended reads as the second says, and the first directory, and any entry only it names, are bytes that nobody reads: not parts,
//!   not an error, not a warning.
//! * **A count is checked against the bytes before a row is read.** The rows of a directory take 46 bytes or more each, so a count
//!   that cannot fit in the size the end record gives the directory is `zip_corrupt` (one flipped bit in a count is damage). Only a
//!   count that fits and is more than twice the part limit and sixteen (rows are parts, folders and the content types stream) is
//!   `too_many_parts`. Rows are then walked once, in order, no more than the count and never past the directory; a row over 65,535
//!   bytes is `zip_corrupt`; a name far over the limit is refused before it is copied; extra fields are read for the ZIP64 values
//!   and the Unicode path and never kept. After the last row, what the count leaves of the directory must not start with a row: a
//!   count below the rows (a flipped bit, a count that wrapped past 65,535) is `zip_corrupt` and not a package with parts missing.
//!   The digital signature record and bytes that are not a row may follow the rows and are ignored.
//! * **The name as written is judged, whatever a Unicode path field says.** When a row has a Unicode path field (Info-ZIP's `up`,
//!   with the checksum of the name as written) the field gives the name; but the name as written is judged as it would be without
//!   the field (a traversal, V11, an escape of a letter, an empty segment, a control character, the stream's name in escapes: the same
//!   refusals with the same class, a folder's without its slash), and the field has to agree with it on whether the entry is a folder
//!   (the name ends in a slash) and on whether it is the content types stream (`invalid_part_name` where it does not). A field that
//!   gives a valid different part to a part row is what it is for. Otherwise a field could turn `../evil.xml` or `%41.xml` into
//!   `ok.xml`, a file into a folder that vanishes, or a part into the stream. **The duplicate and prefix rules hold over
//!   the names as written as well as over the names the fields give**: two rows that are both `a.xml` as written are
//!   `duplicate_part` though their fields name `b.xml` and `c.xml`, since a reader that ignores the field sees two of one name; and a
//!   name that is `/b.xml` as written for one row and as a field gives it for another is refused in the same way, since which row
//!   `/b.xml` is would depend on whether the field is read. A row's own two names are not in each other's way. Rows with no field
//!   have one name and keep it once; a row with a field keeps the folded key of its name as written too, at most one more name.
//! * **ZIP64, labelled.** The ZIP64 record is read when any field of the plain record is its "look elsewhere" value, for the values
//!   the plain record leaves to it; a value the plain record gives itself stands. When it is read its own disk fields decide whether
//!   the archive is a single disk, and the plain record's disk numbers and the locator's disk fields are not read: harmless, because
//!   every offset is from byte 0 and checked against the signature it must reach.
//! * **Central wins (a labelled deviation from R2, which says local and central headers must agree).** The central row
//!   decides the name, flags, method, sizes and checksum. The local header is read for the lengths of its own name and extra field,
//!   which say where the data starts, and for nothing else; a data descriptor is never read; a local name is never used. Entries that
//!   overlap each other or the directory are refused, so the compressed sizes cannot add up to more than the file.
//! * **A folder is an item whose name, as written, ends in a slash, and nothing else, and a folder has no data.** It is not a part, is
//!   named in a [`Warning`] (the first hundred; the rest are counted in one), and counts toward the row limit. A folder row whose
//!   uncompressed size is not 0 is `zip_corrupt` (a flipped bit turns `logo` into `log/`, and its data would be dropped with a
//!   warning); an empty deflate stream is no data. Whether a real writer stores a folder with bytes is not known: the sample-file run of T5
//!   must test this rule with the others that could refuse a real file (no bytes in front of the archive, a count below the rows,
//!   the 1 GiB cap, the 65,535-byte row). Bytes after the end record no longer refuse a file (see above). A relationships part with no source part is
//!   kept and reported the same way: the first hundred by name, the rest counted ([`Warning::MoreOrphanRelationships`]).
//! * **Encryption is every flag that means it** (bits 0, 6 and 13) and is refused; so is any method but stored and deflate.
//!
//! # Limits, and what a hostile package can cost
//!
//! Every limit is a named constant with the rule that sets it ([`limits`] has the list) and a field of [`Limits`]. Where
//! states a number, that is the default (except the package cap, below); where it does not, the
//! constant says it is a labelled choice. A limit is checked before the thing it bounds is allocated, and an archive's own claims are
//! never trusted: sizes in the headers are checked, and then checked again against the bytes that really come out.
//!
//! Three things hold together, so that the limits bound the *whole file* and not only each part:
//!
//! 1. The sum of what is inflated is at most [`MAX_PACKAGE_BYTES`] however the headers lie.
//! 2. It is also at most [`MAX_RATIO`] times the file's own size, since the entries' compressed sizes cannot add up to more than the
//!    file (overlapping entries are refused) and each may inflate to at most that ratio of its own.
//! 3. A refusal ends the read at once. The most work wasted on an entry that is then refused is one limit's worth of inflating. That
//!    work is linear but not fast, and **nothing caps its time**. The second review measured 64 MB of empty dynamic
//!    Huffman blocks, a part that declares 0 bytes and inflates to none, at 6.1 s: about 10 MB of input a second. So a file of the
//!    largest real size can occupy a reader for several seconds before it reads or refuses; it is linear, and not a hang.
//!
//! **The package cap is 1 GiB, until R23 is amended** (R23 suggests 8 GiB). One small file (about 1 MiB of deflate
//! that expands a thousandfold) can ask for 1 GiB; at 8 GiB it was 8 MB for 8 GiB, which a web build cannot address. The largest real
//! package in the census is 173 MB. A caller that wants more passes a larger [`Limits::max_package_bytes`].
//!
//! What was measured, by `crates/opc/tests/it/memory.rs`, which runs each case in a child process under `ulimit -v` (an address
//! space limit) on Linux, x86-64, glibc, on 2026-11-01. "Peak" is the child's peak resident memory (`VmHWM`, which includes the input
//! the child holds); "read added" is what the read itself added to what was resident before it (measured with the peak reset).
//! Rows marked *release* were measured with a release build, and the rest with the test profile.
//!
//! | Input | Result | Peak, or read added |
//! |---|---|---|
//! | A header that claims 2 GiB, 64 GiB, 1 PiB or 16 EiB for 4 KiB of data | `zip_ratio_exceeded` | 5 MB: nothing is allocated for a claim |
//! | Ten honest parts of 32 MiB, address space of 300 MB | `out_of_memory`, and the child ends normally | 235 MB |
//! | The same ten parts, address space of 1.5 GB | read | 333 MB: the parts and 13 MB |
//! | One part of 64 MiB | read | 70 MB: the part and 6 MB |
//! | A part whose header says 10 bytes and holds 200 MiB, address space of 300 MB | `out_of_memory` | 136 MB |
//! | Four parts of 256 MiB (the package cap); a fifth; all five with a larger limit | 4 read; `package_too_large`; 5 read | 1.31 GB: the parts and 0.25 GB |
//! | **The directory at the row limit:** 100,016 rows, 50,000 parts of 1,024-byte names and 50,016 folders (a file of 212 MB), *release* | read in 0.24 to 0.31 s | read added 110 MB: about 2.2 KB for each part |
//! | **The same with a Unicode path field in every row:** 50,000 parts, each with a name as written of 1,016 bytes and a field that gives another of 1,016 bytes (a file of 106 MB), *release* | read in 0.45 to 0.58 s (0.24 to 0.27 s with no field) | read added 161 MB (108 MB with no field): the folded key of each name as written is kept as well |
//! | **The directory at the size of the largest real file:** 30,000 rows of 1,024-byte names (64 MB), *release* | read in 0.06 to 0.08 s | read added 32 MB |
//! | **The deepest names:** 50,000 parts, each name 1,024 bytes of 508 folders `a/` and eight digits, empty local names (a file of 55 MB), *release*. Before a change the prefix check compared names segment by segment and this took 2.06 s (4.5 s in the test profile) | read in 0.31 to 0.40 s (0.8 s in the test profile); 50,000 flat names of the same size, 0.19 to 0.26 s | read added 110 MB |
//! | 1,000 rows with 64 KB of empty extra records each (65.6 MB), *release* | read in 0.08 s | read added 0.4 MB: extra fields are walked and not kept |
//! | 1.5 million rows (75 MB) behind a 1-byte stub, and without it | `zip_corrupt`; `too_many_parts`; together in 0.02 s, *release* | read added 0.1 MB: refused from the count |
//! | 1,000,001 entries (92 MB) | `too_many_parts`, from the count, before a row is read | the file and nothing more |
//! | 50,000 entries (4.5 MB, the part limit) | read | 20 MB |
//! | 5.85 million XML items from 31.2 MB of XML (the most the item limit of 6 million allows) | parsed | 872 MB (peak resident): about 149 bytes an item |
//! | 6.3 million items | `xml_too_complex` | 895 MB: it stops at the limit |
//! | A content types stream of 60,000 rows (3.5 MB, near the 4 MiB cap on the stream) | read | 60 MB |
//!
//! **The worst a directory can cost** is therefore bounded by the size of the file and by the limits, and is no longer a multiple of
//! the file: a pass over the directory's bytes (the time is that of walking them, and a sort of the part names that compares bytes: 0.31 s
//! at the row limit in release, and 0.4 s for the deepest names, which cost under twice what names of the same size with no folders
//! cost: it was seven to eleven times), and memory of about 2.2 KB for each part (50,000 parts with 1,024-byte names: 110 MB; 3.2 KB when
//! every row has a Unicode path field that gives a name of its own, since the name as written is kept as well: 161 MB) and 16 bytes
//! for each row (100,016 rows: 1.6 MB). Warnings are bounded too: the first hundred folders and the first hundred orphaned relationships parts are
//! named, and the rest counted. At the spec's other limits the worst package is: the parts, up to
//! [`MAX_PACKAGE_BYTES`] (1 GiB, and at most [`MAX_RATIO`] times the file; the cost is the parts and about 13 MB per 320 MB), plus that
//! directory (163 MB), plus one tree for `[Content_Types].xml` of at most [`MAX_PACKAGE_XML_NODES`] items (about 150 MB), plus, for each
//! call of [`Package::rels`], one such tree that is dropped when the call returns.
//!
//! # Labels lie
//!
//! Found parts typed `image/gif` that hold PNG, and a part typed `image/x-emf`
//! that holds WMF. [`Package::content_type`] reports what `[Content_Types].xml` says and decides nothing: it is a label. Whoever
//! decodes the bytes reads them, by their first bytes, and does not trust it. The same holds for every part's extension and for a
//! relationship's `Type`.
//!
//! # Provenance
//!
//! The shape of the package and relationship code comes from DeckCraft's `pptx` crate, and so does the tree reader the container crate
//! now holds; see `docs/provenance.md` for exactly what was kept and what was changed (most of it: that code skipped over-budget entries
//! and malformed XML instead of refusing them, lower-cased every name, turned backslashes into slashes and dropped unknown entities,
//! where the spec of this project says refuse, keep the case, refuse the name, and refuse the entity). The zip container is read in the
//! container crate (`zipdir.rs`), not copied from anything.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod archive;
mod content_types;
mod error;
pub mod limits;
pub mod name;
mod package;
mod rels;
pub mod work;
mod write;

/// The bounded XML tree of every part this crate parses, and its writer: the container crate's (`diacraft_container::xml`), under the
/// names it always had here.
pub mod xml {
    pub use diacraft_container::xml::{
        ChildSpans, El, Node, ParseSize, SpanKind, XmlLimits, XmlWriteError, child_spans, parse, parse_size, parse_with, split_document, to_bytes,
        write_element,
    };
}

pub use content_types::{CONTENT_TYPES_ITEM, ContentTypes};
pub use error::{ErrorKind, OpcError, Warning, XmlError};
pub use limits::*;
pub use package::Package;
pub use rels::{Rel, Rels, TargetMode};
pub use write::{PackageWriter, WriteError, item_name};
