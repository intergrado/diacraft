//! Reading the font folders of the machine: the seam that keeps the file system out of the web build.
//!
//! On every target but `wasm32` this lists the platform's font folders and catalogues the font files under them. On `wasm32`
//! there is no file system to read, so both functions give nothing, and a browser shell supplies faces through
//! [`FontDb::add_font_data`](crate::FontDb::add_font_data) instead. Everything the folders can make us do is capped: how
//! deep to go, how many entries to look at, how many faces to keep, how many bytes to read of one file, and how long the whole scan
//! may run ([`SCAN_TIME`]). A scan that runs out of time keeps what it has and logs one line.
//!
//! **A folder on a mount that hangs still hangs**: the time is checked between system calls, and a call that never returns is not
//! interrupted. Building the database is therefore work for a thread that is not the user interface's.
//!
//! The folder lists are adapted from DeckCraft's `crates/fonts/src/fontdb.rs` (`docs/provenance.md`), without the folders that
//! belong to Microsoft Office and without reading fontconfig's configuration.

use std::path::PathBuf;

use crate::catalog::{Entry, Origin};

/// The platform's font folders, the system's and the user's. Empty on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub fn font_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Catalogue the font files under `dirs`, adding to `out` until it holds `limit` entries. Nothing on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn scan(_dirs: &[PathBuf], _origin: Origin, _limit: usize, _out: &mut Vec<Entry>) {}

/// How long a scan of the font folders may run before it stops with what it has found. Reading a real machine's few hundred fonts
/// takes a few milliseconds; a folder that links back to the root of the disk would take minutes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SCAN_TIME: std::time::Duration = std::time::Duration::from_secs(8);
/// How far below a font folder the scan goes, and how many directory entries it looks at in all.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DEPTH: usize = 8;
#[cfg(not(target_arch = "wasm32"))]
const MAX_DIR_ENTRIES: usize = 200_000;

/// The folder Windows keeps the fonts a user installs for themselves in, under `%LOCALAPPDATA%`. The operating system names it; it is
/// not a folder of any application's install.
#[cfg(not(target_arch = "wasm32"))]
const WINDOWS_USER_FONTS: &str = "Microsoft\\Windows\\Fonts"; // brand-ok

/// The platform's font folders, the system's and the user's.
#[cfg(not(target_arch = "wasm32"))]
pub fn font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        dirs.extend(home.iter().map(|h| h.join("Library/Fonts")));
    } else if cfg!(windows) {
        let root = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        dirs.push(root.join("Fonts"));
        dirs.extend(std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join(WINDOWS_USER_FONTS)));
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        dirs.extend(home.iter().flat_map(|h| [h.join(".fonts"), h.join(".local/share/fonts")]));
        let env = |k: &str| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned());
        dirs.extend(xdg_font_dirs(env("XDG_DATA_HOME").as_deref(), env("XDG_DATA_DIRS").as_deref()));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// The folders `XDG_DATA_HOME` and `XDG_DATA_DIRS` name, as font folders (each with `fonts` under it): absolute ones only, and at most 64 of
/// the list. The variables' values are the arguments, so that the reading can be tested without touching the environment; an empty or
/// missing `XDG_DATA_DIRS` is `/usr/local/share:/usr/share`.
pub fn xdg_font_dirs(data_home: Option<&str>, data_dirs: Option<&str>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = data_home.filter(|d| d.starts_with('/')).map(PathBuf::from).into_iter().collect();
    let list = data_dirs.filter(|d| !d.is_empty()).unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(list.split(':').filter(|d| d.starts_with('/')).take(64).map(PathBuf::from));
    dirs.into_iter().map(|d| d.join("fonts")).collect()
}

/// Is this file named like a font file? Only the names say so cheaply, and the tables are checked when the file is catalogued.
#[cfg(not(target_arch = "wasm32"))]
fn is_font_name(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "otf", "ttc", "otc"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Catalogue the font files under `dirs`, adding to `out` until it holds `limit` entries. A folder that can't be read, a file that
/// isn't a font and a loop of links are all skipped without a message: a missing folder is the usual case.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn scan(dirs: &[PathBuf], origin: Origin, limit: usize, out: &mut Vec<Entry>) {
    scan_for(dirs, origin, limit, out, SCAN_TIME);
}

/// [`scan`] with a time of its own.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn scan_for(dirs: &[PathBuf], origin: Origin, limit: usize, out: &mut Vec<Entry>, time: std::time::Duration) {
    let deadline = std::time::Instant::now().checked_add(time);
    let out_of_time = || deadline.is_some_and(|d| std::time::Instant::now() >= d);
    let mut visited = std::collections::HashSet::new();
    let mut looked_at = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().rev().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        // A folder reached twice (a link, or two listed folders one inside the other) is read once.
        if out_of_time() {
            log::warn!("the scan of the font folders ran out of time and kept the {} faces it had", out.len());
            return;
        }
        let Ok(real) = dir.canonicalize() else { continue };
        if !visited.insert(real) {
            continue;
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        // Sorted so the catalogue, and so which of two equal faces is found first, doesn't depend on the order the folder lists.
        let mut paths: Vec<PathBuf> = read.flatten().take(MAX_DIR_ENTRIES).map(|item| item.path()).collect();
        paths.sort();
        let mut subdirs = Vec::new();
        for path in paths {
            looked_at += 1;
            if looked_at > MAX_DIR_ENTRIES || out.len() >= limit {
                return;
            }
            // Checking the clock every entry would be a system call a file; every 64th is plenty.
            if looked_at.is_multiple_of(64) && out_of_time() {
                log::warn!("the scan of the font folders ran out of time and kept the {} faces it had", out.len());
                return;
            }
            // `metadata` follows a link, so a linked folder or file counts as what it points at.
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.is_dir() && depth < MAX_DEPTH {
                subdirs.push((path, depth + 1));
            } else if meta.is_file() && is_font_name(&path) {
                out.extend(crate::catalog::entries_from_file(&path, origin).into_iter().take(limit.saturating_sub(out.len())));
            }
        }
        stack.extend(subdirs.into_iter().rev());
    }
}
