//! The DiaCraft engine façade.
//!
//! Every user-visible action is a command with a stable id and JSON params. The window, the control
//! channel, the CLI and the MCP server all go through [`Session::execute`], so they cannot disagree.
//! A [`Session`] holds the open document, its undo [`History`] and a journal of what ran; a command
//! that fails or panics leaves all of it as it was ([`guard`]).
//!
//! The document is the ShapeSheet model (`diacraft_doc`), and every edit is a cell write that can be undone. The commands make, format,
//! arrange, set up, snap, theme, combine, print, export, open and save drawings, stencils and templates ([`cmd`], [`draw`], [`snap`], [`theme`],
//! [`mod@print`], [`stencil`], [`template`]). A file is read and saved in three formats, by what its bytes say: the VSDX types ([`mod@file`]), the
//! OpenDocument drawing ([`odgfile`]) and DiaCraft's own `.diacraft` ([`nativefile`]), and the process holds every read to one budget of memory
//! ([`memory`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod across;
pub mod build_info;
mod calc_index;
pub mod check;
pub mod cmd;
pub mod draw;
pub mod error;
pub mod file;
pub mod formatting;
pub mod guard;
pub mod history;
pub mod journal;
pub mod measure;
pub mod memfs;
pub mod memory;
pub mod nativefile;
pub mod odgfile;
mod open_threads;
pub mod print;
mod session;
pub mod snap;
pub mod stencil;
pub mod template;
mod text;
pub mod theme;
pub mod threads;
pub mod view;

#[cfg(kani)]
mod kani_proofs;

pub use cmd::{CommandInfo, CommandSpec, Enabled, MAX_BATCH_STEPS, Run, command_specs, find_command, param_keys};
pub use diacraft_doc as doc;
pub use error::{EngineError, OpenProblem, Result};
pub use history::History;
pub use journal::{JOURNAL_LIMIT, JOURNAL_MAX_BYTES, JOURNAL_SLACK, Journal};
pub use session::{HeldMemory, Session};
pub use text::shown;
