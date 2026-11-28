//! Embeds the help tree: writes `embedded.rs` into the build folder, with the files of `help/` (and the catalogs of `i18n/help/`) as `include_str!` constants.
//!
//! The desktop programs embed the topics, the three tables and every language's catalogs; the browser build (`wasm32`) embeds only the tables, because the topics are
//! fetched there. The place of the trees is `src/host.rs`, the one file a host of this crate changes. A tree that is not there embeds nothing, so the crate builds anywhere.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

// The host's values, from the file the crate itself uses (a build script cannot use the crate it builds).
#[allow(dead_code)]
mod host {
    include!("src/host.rs");
}

/// The most files of a tree that are embedded: more are left out (the check of the help tree fails on a tree that big long before).
const MAX_FILES: usize = 20_000;
/// The deepest folder that is read.
const MAX_DEPTH: usize = 8;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join(host::REPOSITORY_ROOT);
    let help = root.join(host::HELP_TREE);
    let catalogs = root.join(host::CATALOG_TREE);
    let browser = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/host.rs");
    for tree in [&help, &catalogs] {
        if tree.exists() {
            println!("cargo:rerun-if-changed={}", tree.display());
        }
    }

    let mut out = String::new();
    let topics = if browser { Vec::new() } else { files_under(&help.join(host::TOPICS_DIR), "md") };
    line(&mut out, "/// The topics: file name and text.\nconst TOPICS: &[(&str, &str)] = &[");
    for path in &topics {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        line(&mut out, &format!("    ({name:?}, include_str!({:?})),", path.display().to_string()));
    }
    line(&mut out, "];");
    for (constant, file) in [("TIPS", host::TIPS_FILE), ("MAP", host::MAP_FILE), ("SECTIONS", host::SECTIONS_FILE)] {
        let path = help.join(file);
        if path.is_file() {
            line(&mut out, &format!("const {constant}: &str = include_str!({:?});", path.display().to_string()));
        } else {
            line(&mut out, &format!("const {constant}: &str = \"\";"));
        }
    }
    line(&mut out, "/// The catalogs: locale, topic id and text.\nconst CATALOGS: &[(&str, &str, &str)] = &[");
    if !browser {
        for (locale, id, path) in catalog_files(&catalogs) {
            line(&mut out, &format!("    ({locale:?}, {id:?}, include_str!({:?})),", path.display().to_string()));
        }
    }
    line(&mut out, "];");

    let target = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("embedded.rs");
    if let Err(e) = std::fs::write(&target, out) {
        // A build that cannot write its own folder cannot go on: say so, and fail the build through the missing file.
        println!("cargo:warning=cannot write {}: {e}", target.display());
    }
}

/// Adds a line to the text of the generated file.
fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

/// The files with extension `extension` under `dir`, in the order of their paths, with the cap on files and depth.
fn files_under(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((folder, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < MAX_DEPTH {
                stack.push((path, depth + 1));
            } else if kind.is_file() && path.extension().is_some_and(|e| e == extension) && found.len() < MAX_FILES {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The catalogs under `dir`: `<locale>/<topic id>.tsv`, as (locale, topic id, path), in order.
fn catalog_files(dir: &Path) -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    let Ok(locales) = std::fs::read_dir(dir) else { return found };
    for locale in locales.filter_map(Result::ok) {
        if !locale.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = locale.file_name().to_string_lossy().into_owned();
        for path in files_under(&locale.path(), "tsv") {
            let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            found.push((name.clone(), id, path));
        }
    }
    found.sort();
    found
}
