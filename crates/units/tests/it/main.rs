//! The units crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test. Vectors are those of
//! Written as tables so that a row names the vector it comes from.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod compound;
mod convert;
mod dimension;
mod parse;

/// The comparison of the spec's vectors (as the other batches say).
pub const EPS: f64 = 1e-9;

pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * a.abs().max(b.abs()).max(1.0)
}
