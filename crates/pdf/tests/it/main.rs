//! The PDF crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Documents are built in code with the testkit, exported, and read back with the shared PDF reader and interpreter (`craft-pdf`), which know
//! nothing of the writer; `independent` also asks poppler's tools, where the machine has them. Every assertion is a value: a position in
//! points, a colour, the characters a glyph stands for.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod hostile;
mod independent;
mod line_end_fill;
mod pdf;
mod print;
mod readback;
mod support;
