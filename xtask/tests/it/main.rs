//! xtask's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod assets;
mod brands;
mod ci;
mod deny;
mod deploy_site;
mod docs_check;
mod downloads;
mod fuzzcheck;
mod kani;
mod layers;
mod licenses;
mod lints;
mod packaging;
mod packaging_appimage;
mod packaging_deb;
mod packaging_msi;
mod packaging_smoke;
mod pages_deploy;
mod parity;
mod provenance;
mod repo_files;
mod stats;
mod usage;
mod version;
mod web_site;
mod workspace;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A scratch directory under the system temp dir (`TMPDIR`), removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-xtask-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `text` to `rel` under the directory, creating folders.
    pub fn write(&self, rel: &str, text: &str) {
        self.write_bytes(rel, text.as_bytes());
    }

    /// Writes `bytes` to `rel` under the directory, creating folders.
    pub fn write_bytes(&self, rel: &str, bytes: &[u8]) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // A scratch directory that cannot be removed is only litter.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `git args` in `dir`, which must succeed.
pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A fresh git repository in a scratch directory.
pub fn git_repo(tag: &str) -> TempDir {
    let dir = TempDir::new(tag);
    git(dir.path(), &["init", "-q"]);
    dir
}
