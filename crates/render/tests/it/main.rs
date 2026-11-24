//! The render crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every test builds a small document in code with the testkit, draws it, and asserts **pixels**: the colour at a place, named in paper
//! inches. Each names the wrong picture it catches (the vectors
//!). No reference image from the reference app is
//! used, and no test depends on a font the machine has: text tests use the bundled faces, and assert only where ink is.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod basics;
mod export;
mod fills;
mod frame_time;
mod gradient_stops;
mod hairlines;
mod hostile;
mod layers;
mod line_ends;
mod lines;
mod masters;
mod memory;
mod meta_build;
mod metafile;
mod par;
mod par_bench;
mod pictures;
mod placeholders;
mod preview;
mod print;
mod print_gray;
mod send;
mod shadows;
mod support;
mod text;
mod text_block;
mod text_flips;
mod text_vertical;
mod theme_colours;
mod themed;
