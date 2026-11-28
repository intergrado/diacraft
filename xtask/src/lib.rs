//! Workspace tooling for `cargo xtask <command>`: the gates behind `cargo xtask ci`, and helpers.
//!
//! Pure Rust (std, serde_json, toml, roxmltree and sha2). External tools (`cargo`, `git`, and `trunk` for `site --web`) run
//! through `std::process::Command`.
//! The commands are in `main.rs`; the checks are modules here so the integration tests can reach them.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod assets;
pub mod brands;
pub mod ci;
pub mod deny;
pub mod docs_check;
pub mod fuzzcheck;
pub mod help;
pub mod kani;
pub mod langdata;
pub mod layers;
pub mod licenses;
pub mod mcp;
pub mod parity;
pub mod stats;
pub mod version;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    // The repository root, as a clean path (`join("..")` would print as `…/xtask/..`).
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

/// Tracked and untracked (not ignored) files, relative to `root`, with `/` separators.
///
/// The gates trust this list to be every file there is, so it never loses one silently: paths come
/// from `git ls-files -z` (NUL-separated, so git does not quote spaces, quotes or non-ASCII names),
/// a path that is not valid UTF-8 is an error, and the only entries left out are the tracked files
/// git itself reports as deleted (`git ls-files --deleted`). Any other listed path that is not on
/// disk (a dangling symlink still is) is an error too.
pub fn repo_files(root: &Path) -> Result<Vec<String>, String> {
    let listed = git_paths(root, &["--cached", "--others", "--exclude-standard"])?;
    let deleted: HashSet<String> = git_paths(root, &["--deleted"])?.into_iter().collect();
    let mut seen = HashSet::new();
    let mut files = Vec::with_capacity(listed.len());
    for path in listed {
        // A path with unmerged index entries is listed once per entry.
        if deleted.contains(&path) || !seen.insert(path.clone()) {
            continue;
        }
        // `symlink_metadata` does not follow links: a dangling link is a file the gates must see.
        std::fs::symlink_metadata(root.join(&path)).map_err(|e| format!("git lists `{path}` but it cannot be read: {e}"))?;
        files.push(path);
    }
    Ok(files)
}

/// The paths `git ls-files -z <args>` prints, as UTF-8 strings.
fn git_paths(root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let out = Command::new("git").current_dir(root).args(["ls-files", "-z"]).args(args).output().map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    out.stdout
        .split(|&b| b == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec()).map_err(|_| format!("git lists a path that is not valid UTF-8: {}", String::from_utf8_lossy(path)))
        })
        .collect()
}

/// A `cargo` command (the one running xtask, else `cargo` on the PATH) run in the workspace root.
pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

/// Prints `what` and runs `cmd`, which must exit successfully.
pub fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

/// `cargo metadata --no-deps` for the workspace, as JSON.
pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

/// Whether `cargo <subcommand> --version` succeeds, that is whether the cargo plug-in is installed.
pub fn has_cargo_tool(subcommand: &str) -> bool {
    cargo().args([subcommand, "--version"]).output().is_ok_and(|o| o.status.success())
}
