//! The SVG crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Documents are built in code with the testkit, exported, and read back with the strict XML parser of `opc`, which knows nothing of the
//! writer. Every assertion is a value: a pixel position, a colour, a number of stops, the characters of a text.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod hostile;
mod support;
mod svg;
