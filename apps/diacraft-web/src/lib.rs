//! The parts of DiaCraft's web build that do not need a browser, and the browser shell that does.
//!
//! [`files`] holds what the browser's file handling decides without a browser (the names a chooser offers, the pieces a file is read in, the words a refused file is told in); `web_files` (`wasm32` only)
//! is the part that meets the browser: choosing, dropping and handing over files. [`assets`] reads the manifest of the files the module does not carry (the splash picture, the cover, the licence list), and
//! `web_assets` (`wasm32` only) fetches them. `web_page` (`wasm32` only) is the page's side of the window's place in the browser: the keys a browser would act on, a window of its own, full screen.
//! [`prefs`] holds the person's settings in the browser without a browser (the keys of `localStorage`, the host over any storage, the budget of 1 MiB), and `web_prefs` (`wasm32` only) is the page's `localStorage` under it. [`canvas_size`] decides the pixels the canvas must have for the page's size and the device pixel ratio, and `web_canvas` (`wasm32` only) applies it before every frame.
//! [`page`] holds what the page and the program must agree on (the ids of the page's elements), the messages
//! the page shows when the app cannot start, and [`page::new_app`], the one place the shell makes the window.
//! It builds for every target, so the tests run it on the desktop. The shell itself (`start`) exists on
//! `wasm32` only: it is the only code that touches the DOM, and `cargo xtask wasm` is what compiles it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod assets;
pub mod canvas_size;
pub mod files;
pub mod page;
pub mod prefs;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_assets;
#[cfg(target_arch = "wasm32")]
mod web_canvas;
#[cfg(target_arch = "wasm32")]
mod web_files;
#[cfg(target_arch = "wasm32")]
mod web_page;
#[cfg(target_arch = "wasm32")]
mod web_prefs;

#[cfg(target_arch = "wasm32")]
pub use web::start;
