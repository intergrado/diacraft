//! The document model's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod batch;
mod catalogue;
mod cell;
mod comments;
mod crafted;
mod diacraft_data;
mod display;
mod document;
mod hostile;
mod inheritance;
mod lang;
mod lang_data;
mod limits;
mod merge;
mod native_model;
mod page_names;
mod places;
mod raw;
mod root_text;
mod rows;
mod shared;
mod sharing;
mod sheet;
mod snap_words;
mod unsaved;
mod value;
mod write;
