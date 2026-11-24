//! The colour crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod exhaustive;
mod palette;
mod rgba;
mod scheme;
mod transform;
