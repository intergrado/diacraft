//! The layout crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test. A test names the wrong result it catches. The rules checked here are **ours**
//! (the reference application publishes no placement algorithm), so a vector is a property of the result (no two boxes overlap,
//! the spaces are kept, a parent is over its children), not a number from a source.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod compact;
mod flowchart;
mod hierarchy;
mod hostile;
mod org;
mod props;
mod round;
mod support;
