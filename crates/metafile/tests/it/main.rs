//! The metafile crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code from (its vectors carry their id) or as a hostile case that no
//! honest writer makes. No file from the reference app, or from anyone, is read. Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod build;
mod fuzz;
mod hostile;
mod inspect;
mod vectors;
