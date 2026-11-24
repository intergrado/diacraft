//! `cargo xtask assets`: every asset must be attributed in `ATTRIBUTION.md`.
//!
//! An asset (AGENTS.md) is any non-code material the project includes: every git-tracked (and
//! untracked, not ignored) file under `assets/` and `docs/images/`, and every such file anywhere with
//! an asset extension (images, icons, fonts, colour profiles, audio, video, PDFs, VSDX-family
//! documents, CAD and metafile documents). The gate fails if a path is not the first cell of a row of the
//! ATTRIBUTION.md table. A folder that does not exist yet has no files and passes. Files a test generates at
//! run time sit in ignored folders (`target/`) and are not listed.

use std::path::Path;

const ASSET_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "ico", "icns", "svg", "pdf", "ai", "eps", "psd", "icc", "icm", "ttf", "otf", "woff",
    "woff2", "ttc", "eot", "ase", "aco", "abr", "mp4", "wav", "mp3",
    // The VSDX family's documents, stencils, templates, add-on libraries and web drawings: sample documents are assets, and a
    // file made with the reference app must never be committed at all (AGENTS.md).
    "vsd", "vsdx", "vsdm", "vss", "vssx", "vssm", "vst", "vstx", "vstm", "vdx", "vsx", "vtx", "vsl", "vdw",
    // CAD and metafile documents (the formats DiaCraft imports): someone else's drawings, so assets too.
    "dwg", "dxf", "emf", "wmf",
];
const ASSET_DIRS: &[&str] = &["assets/", "docs/images/"];

pub fn is_asset(path: &str) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    ASSET_DIRS.iter().any(|d| path.starts_with(d)) || ASSET_EXT.contains(&ext.as_str())
}

/// The paths in the first cell of each table row of ATTRIBUTION.md, without backticks.
pub fn attributed(attribution_md: &str) -> Vec<String> {
    attribution_md
        .lines()
        .filter_map(|l| l.trim().strip_prefix('|'))
        .filter_map(|l| l.split('|').next())
        .map(|cell| cell.trim().trim_matches('`').to_owned())
        .collect()
}

/// Paths that need an ATTRIBUTION.md entry but lack one.
pub fn missing(files: &[String], attribution_md: &str) -> Vec<String> {
    let rows = attributed(attribution_md);
    files.iter().filter(|f| is_asset(f) && !rows.contains(f)).cloned().collect()
}

pub fn run(root: &Path) -> Result<(), String> {
    let files = crate::repo_files(root)?;
    let md = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let miss = missing(&files, &md);
    if miss.is_empty() {
        println!("assets: all {} asset files attributed in ATTRIBUTION.md", files.iter().filter(|f| is_asset(f)).count());
        Ok(())
    } else {
        Err(format!("{} asset file(s) lack an ATTRIBUTION.md row (author, source, licence):\n  {}", miss.len(), miss.join("\n  ")))
    }
}
