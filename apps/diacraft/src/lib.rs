//! The DiaCraft desktop app, as a library so its parts can be tested without a window.
//!
//! - [`args`]: the command line and the environment variables the app reads.
//! - [`control_server`]: the loopback JSON-lines server that hands requests to the window.
//! - [`dialogs`]: the native Open dialog.
//! - [`graphics`]: which graphics adapter draws the window on macOS, checked to run commands before the window uses it.
//! - [`logging`]: the logger, with its rotating, size-limited log file.
//! - [`printing`]: the system's printers and the jobs sent to them, over `craft-print`.
//! - [`startup`]: the one font database and the first drawing, before any command.
//!
//! `main.rs` wires them to `eframe`: it starts the logger, reads the command line, starts the control
//! server if asked, and opens the window of `diacraft-ui-egui`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod args;
pub mod control_server;
pub mod dialogs;
pub mod graphics;
pub mod logging;
pub mod printing;
pub mod startup;
