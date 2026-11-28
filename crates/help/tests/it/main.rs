//! The `diacraft-help` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code. The tests never read the repository's help tree except `embedded.rs`, which holds the embedded tree to the rules. Tests may unwrap and panic: a
//! failing assertion is the point of a test. A test that times work is `#[ignore]`d (run with `-- --ignored`): the others count.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod blocks;
mod caps;
mod catalog;
mod chord;
mod embedded;
mod fold;
mod header;
mod index;
mod inlines;
mod nav;
mod props;
mod rank;
mod tables;
mod url;

use diacraft_help::{Code, Parsed, Problem, parse};

/// The header every test topic starts with: seven lines, so that line `n` of a test's body is line `n + 7` of the file. `site: no`, so that no description is needed.
pub const HEAD: &str = "---\nid: sample\ntitle: Sample\nsummary: A sample.\nkeywords: sample\nsite: no\n---\n";

/// The number of lines of [`HEAD`].
pub const HEAD_LINES: usize = 7;

/// A topic with `body` after the standard header, read.
pub fn read(body: &str) -> Parsed {
    parse(&format!("{HEAD}{body}")).unwrap()
}

/// The problems of `body` as (line of the body, code).
pub fn problems(body: &str) -> Vec<(usize, Code)> {
    read(body).problems.iter().map(|p: &Problem| (p.line.saturating_sub(HEAD_LINES), p.code)).collect()
}
