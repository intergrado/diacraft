//! The command line's integration tests: they run the built `diacraft-cli` and look at what it prints and how it
//! ends. One test binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod allocator;
mod check_before;
mod convert;
mod export;
mod mcp;
mod mcp_roots;
mod odg_flat;
mod odg_note;
mod odg_render;
mod odg_save;
mod old_namespace;
mod oracle;
mod prefs;
#[cfg(unix)]
mod pty;
mod roundtrip;
mod roundtrip_diacraft;
mod roundtrip_odg;
mod run;
mod ux;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

/// The binary under test.
pub const BIN: &str = env!("CARGO_BIN_EXE_diacraft-cli");

/// A scratch folder under the system temp dir (`TMPDIR`), removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-cli-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// The path of `name` in the folder.
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes `bytes` to `name` and returns its path.
    pub fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Litter in a scratch folder is harmless.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the binary with `args`, giving it `stdin` (nothing, if `None`), and waits for it.
pub fn cli(args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .env_remove("RUST_LOG")
        // A test never reads or writes the person's settings: `run` and `mcp` open them unless this is set.
        .env("DIACRAFT_NO_PREFS", "1")
        // A person who forces colour in the shell they test from does not get it in what the tests read.
        .env_remove("CLICOLOR_FORCE")
        .env_remove("NO_COLOR")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // The program may stop reading early (a refusal); that is for the test's assertions to notice.
        let _ = pipe.write_all(bytes);
    }
    child.wait_with_output().unwrap()
}

/// What the program printed on stdout, as JSON lines.
pub fn json_lines(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8(stdout.to_vec())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {l}")))
        .collect()
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A path as the `&str` the argument lists take.
pub fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}
