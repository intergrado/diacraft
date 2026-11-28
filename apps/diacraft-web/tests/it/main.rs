//! Integration tests for the web app's parts that do not need a browser: the page, the messages, the window the
//! shell makes and the files `trunk` reads. The browser shell itself (`src/web.rs`) is compiled by
//! `cargo xtask wasm`; running it in a browser is not tested here.
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod canvas_size;
mod deep_files;
mod fonts;
mod glyphs;
mod html;
mod loading;
mod manifest;
mod native;
mod page;
mod prefs;
mod trunk;

use std::path::PathBuf;

/// A file of the web app's folder, by its path from there.
pub fn app_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// The text of a file of the web app's folder.
pub fn read(rel: &str) -> String {
    std::fs::read_to_string(app_path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}
