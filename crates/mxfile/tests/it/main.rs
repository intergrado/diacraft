//! The `mxfile` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code: by hand from the draw.io specifications (whose vectors carry their id in the name of the test that
//! holds the reader to them), or by `diacraft_testkit::mxfile`, the tests' own encoder (the forward steps: URI-encode, raw DEFLATE at level 9, Base64), or as a hostile
//! case that no honest writer makes (`hostile.rs`). No file made by any other program is read, and none is committed. A test compares the XML a page decodes to, never
//! the compressed bytes. Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod file_vectors;
mod fuzz;
mod hostile;
mod label_fuzz;
mod label_hostile;
mod label_table;
mod sniff;
mod steps;
mod style_hostile;
mod style_props;
mod style_vectors;

/// The name of the first page of a drawing.
pub(crate) fn name0(doc: &diacraft_doc::Document) -> String {
    doc.pages.get(0).map(|p| p.name.to_string()).unwrap_or_default()
}

/// The reader's own limits, for the whole run: a [`diacraft_mxfile::Reading`] borrows the limits it is held to.
pub(crate) fn limits() -> &'static diacraft_mxfile::Limits {
    static LIMITS: std::sync::OnceLock<diacraft_mxfile::Limits> = std::sync::OnceLock::new();
    LIMITS.get_or_init(diacraft_mxfile::Limits::default)
}
