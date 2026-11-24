# Contributing to DiaCraft

Thank you for helping. [`AGENTS.md`](AGENTS.md) holds the rules every change follows, whether a person or a coding agent makes it:
where code and behaviour may come from, assets, the never-crash rules, the architecture and the tests. Read it first.

## Build

- Rust 1.95 or newer (CI uses 1.100.0), with `rustup target add wasm32-unknown-unknown` for the web build's checks.
- On Linux, the window-system and font development packages: `pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev
  libxcursor-dev libxrandr-dev libxi-dev libfontconfig1-dev libfreetype-dev` (and a Vulkan or OpenGL driver to run the window).
- `cargo build --workspace` builds everything; `cargo run --release -p diacraft` runs the app, and `cargo run -p diacraft-cli -- --help`
  the command line. Fonts are an optional input (`CRAFT_FONTS_DIR`): everything builds, tests and runs without them.

## Test and check

- `cargo xtask ci` runs every gate: fmt, clippy, the fuzz check, the Kani check, the tests, the doctests, and the checks of assets,
  names, docs, the website, licences, the parity checklist, the crate layers and the WASM build. It needs `cargo-nextest` and
  `cargo-deny` (`docs/development.md` gives the versions).
- While you work, run the tests you are writing: `cargo nextest run -p <crate>` (or `cargo test -p <crate>`).
- `cargo xtask kani` proves the Kani harnesses (minutes; it needs Kani, `docs/development.md`, "Proofs").
- For a change in the window, look at it: `docs/control-protocol.md` says how to take a screenshot from a script.

## Send a change

- One change per pull request, with the tests that show it works; the template asks what changed, how it was tested, and the checklist
  of `AGENTS.md`.
- Every new asset needs a row in `ATTRIBUTION.md`, and every copied file a row in `docs/provenance.md`.
- Update the docs your change touches (`AGENTS.md`, "Progress docs").
- Security problems: see [`SECURITY.md`](SECURITY.md).

## Licence

DiaCraft is offered under the MIT licence or the Apache License 2.0, at your option ([`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE)). Unless you say otherwise, a contribution you submit is offered under the same two licences, with
no additional terms.
