//! The DiaCraft MCP server: the engine's commands for an agent, over [Model Context Protocol] on stdio.
//!
//! Newline-delimited JSON-RPC 2.0, hand-written, with no async runtime ([`protocol`]). The server exposes six tools
//! ([`tools`]) and forwards each to a [`Backend`]:
//!
//! - [`Backend::Headless`]: a [`diacraft_engine::Session`] in this process, with no window.
//! - [`Backend::Remote`]: a running DiaCraft app, through its loopback control channel
//!   (`diacraft --control 7997`, `docs/control-protocol.md`). [`ControlClient`] checks that DiaCraft, and not
//!   another craft app on the same port, answers, before it sends a command.
//!
//! Entry points: [`serve`] (the loop), [`connect_or_headless`] (the default mode of `diacraft-cli mcp`) and
//! [`ControlClient::connect`]. See `docs/mcp.md`.
//!
//! This crate is below the window and outside it: it depends on no UI toolkit.
//!
//! [Model Context Protocol]: https://modelcontextprotocol.io
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
mod protocol;
mod tools;

pub use backend::{Backend, ClientConfig, ConnectError, ControlClient, DEFAULT_ADDR, connect_or_headless};
pub use protocol::{MAX_BATCH_MESSAGES, MAX_LINE_BYTES, PROTOCOL_VERSION, json_line, serve, serve_with};
pub use tools::{ToolResult, tool_definitions};

#[cfg(test)]
mod tests;
