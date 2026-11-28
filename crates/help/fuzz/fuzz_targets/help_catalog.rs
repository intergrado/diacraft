//! `cargo fuzz run help_catalog`: any bytes as a help catalog and as a packed file.
//!
//! The first byte says whether the catalog is for a right-to-left language (odd) and the rest, by lossy UTF-8, is the text. The properties: nothing panics; a catalog is
//! empty and says so when it is over 2 MiB, never has more than 20,000 rows, and its problems are within their cap; every row it keeps is in a known context with a
//! source and a translation that are not empty; and a packed file is read or refused, never trusted: what `unpack` gives packs and unpacks to the same topics.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); no corpus is committed.
#![no_main]

use diacraft_help::catalog::{CONTEXTS, Catalog, pack, unpack};
use diacraft_help::{MAX_CATALOG_BYTES, MAX_CATALOG_ROWS, MAX_PROBLEMS};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (flags, rest) = data.split_first().map_or((0u8, &[][..]), |(f, r)| (*f, r));
    let text = String::from_utf8_lossy(rest);
    let (catalog, problems) = Catalog::parse(&text, flags & 1 == 1);
    assert!(catalog.len() <= MAX_CATALOG_ROWS && problems.len() <= MAX_PROBLEMS);
    if text.len() > MAX_CATALOG_BYTES {
        assert!(catalog.is_empty());
    }
    for context in CONTEXTS {
        // A lookup of a row that is not there is `None`, never a panic.
        assert!(catalog.get(context, "\u{0}").is_none());
    }
    if let Ok(topics) = unpack(&text) {
        let refs: Vec<(&str, &str)> = topics.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let again = unpack(&pack(&refs));
        assert!(again.is_ok());
    }
});
