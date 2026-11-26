//! DiaCraft test helpers: hostile values for fuzzing the command registry, a builder for documents, a builder for packages
//! (`.vsdx` files made in code), a builder for theme parts, an encoder for draw.io files (`mxfile`), and ready-made documents and sessions.
//!
//! This crate may only be used as a dev-dependency (`cargo xtask layers` enforces it). It is test
//! support: a fixture that cannot be built fails the test that asked for it, as an assertion does.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod build;
pub mod fixtures;
pub mod model;
pub mod mxfile;
pub mod old_namespace;
pub mod package;
pub mod strategies;
pub mod theme;
