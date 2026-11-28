//! The DiaCraft desktop app, as a library so its parts can be tested without a window.
//!
//! - [`args`]: the command line and the environment variables the app reads.
//! - [`control_server`]: the loopback JSON-lines server that hands requests to the window.
//! - [`dialogs`]: the native Open dialog, and the message box of a start that failed.
//! - [`failure`]: what DiaCraft tells the person when it cannot start (a message box where nothing else would show it, standard error and the log).
//! - [`fallback`]: the graphics tries a start goes through before it gives up (DirectX 12 alone, the software adapter, OpenGL alone).
//! - [`graphics`]: which graphics adapter draws the window on macOS and Windows, checked to run commands before the window uses it.
//! - [`logging`]: the logger, with its rotating, size-limited log file.
//! - [`marker`]: what a start remembers about graphics tests that never returned (a driver that crashes or blocks), so that the next start leaves that adapter out.
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
pub mod failure;
pub mod fallback;
pub mod graphics;
pub mod logging;
pub mod marker;
pub mod printing;
pub mod startup;
