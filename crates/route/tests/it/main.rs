//! The `route` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code. `vectors` holds the composed cases (each names the wrong result it catches), `oracle` two independent searches the router's cost is
//! held to, `props` random scenes (and the fuzz target's own law), `symmetry` the scenes turned and mirrored, and `hostile` the crafted inputs, counted. Tests may unwrap and
//! panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod hostile;
mod oracle;
mod props;
mod support;
mod symmetry;
mod units;
mod vectors;
