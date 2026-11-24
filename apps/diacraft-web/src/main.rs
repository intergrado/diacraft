//! DiaCraft in the browser.
//!
//! Runs the same `DiacraftApp` as the desktop app, through eframe's web runner. `cargo xtask site --web` builds it
//! with trunk into the folder of the app's own site, where the app is at the root (the website's pages are another site), the module in parts (`docs/website.md`, "The web app"); `cargo xtask
//! wasm` checks that it compiles for `wasm32-unknown-unknown`, and runs in every `cargo xtask ci`.
//!
//! Differences from the desktop app: no control channel (a browser cannot listen on a socket), no log file (the
//! browser console is the log), no window frame of its own (the page is the window), no threads and no printer (the
//! Print view saves a PDF), and files are chosen, dropped and downloaded through the page (`web_files`).
//!
//! WebAssembly aborts on a panic, so this program has none to catch: it has no `unwrap`, `expect` or indexing, and
//! what goes wrong at the start (no canvas on the page, no graphics) is shown on the page.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
fn main() {
    diacraft_browser::start();
}

/// Not meant to run on the desktop: says so and fails, the way a program does that cannot do what it was asked.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    use std::io::Write;

    // Not `eprintln!`, which panics when the stream is closed; there is nobody left to tell then.
    let _ = std::io::stderr().write_all(b"diacraft-web only runs in the browser: build it with `trunk build --release` in apps/diacraft-web\n");
    std::process::ExitCode::FAILURE
}
