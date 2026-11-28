//! `cargo xtask version [set X.Y.Z[-pre]]`: the workspace version, the single source of truth.
//!
//! The version lives in `[workspace.package] version` in the root `Cargo.toml`; every crate
//! inherits it with `version.workspace = true`, and the packaging scripts read it from here.

use std::path::Path;

/// The `version = "…"` value inside `[workspace.package]`.
pub fn read(manifest: &str) -> Result<String, String> {
    let (_, line) = find_line(manifest)?;
    let v = line.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_string()).unwrap_or_default();
    if v.is_empty() {
        return Err("empty [workspace.package] version".into());
    }
    Ok(v)
}

/// `manifest` with the workspace version replaced; everything else is byte-for-byte unchanged.
pub fn replace(manifest: &str, new: &str) -> Result<String, String> {
    validate(new)?;
    let (idx, _) = find_line(manifest)?;
    let mut out = String::with_capacity(manifest.len() + 8);
    for (i, line) in manifest.split_inclusive('\n').enumerate() {
        if i == idx {
            let eol = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            out.push_str(&format!("version = \"{new}\"{eol}"));
        } else {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// The downloads that older releases never had, each with the release it first came with (the site's ids, as `site/site.json` names them): the `.rpm` for Fedora first came with 0.2.4.
/// The pages are made for the workspace's version and link the files of that release, so a page made for an older one must not link a file that release does not have.
pub const DOWNLOAD_SINCE: [(&str, &str); 2] = [("linux-x86_64-rpm", "0.2.4"), ("linux-arm64-rpm", "0.2.4")];

/// The numbers of a version, `MAJOR.MINOR.PATCH`, with any `-pre.release` left off; `None` for what is no version of that shape.
fn core_of(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split('-').next()?;
    let mut numbers = core.split('.').map(|n| n.parse::<u64>().ok());
    let found = (numbers.next()??, numbers.next()??, numbers.next()??);
    numbers.next().is_none().then_some(found)
}

/// Whether the release of `version` has the download `id`: every download is in every release but those of [`DOWNLOAD_SINCE`], which are in the release they came with and the ones after it.
/// A pre-release of that release has them too (a tag with a suffix builds the same files). A version that is no version has none of those.
pub fn release_has(version: &str, id: &str) -> bool {
    let Some((_, since)) = DOWNLOAD_SINCE.iter().find(|(download, _)| *download == id) else { return true };
    matches!((core_of(version), core_of(since)), (Some(have), Some(first)) if have >= first)
}

/// Semver without build metadata: `MAJOR.MINOR.PATCH` plus an optional `-pre.release` tag.
pub fn validate(v: &str) -> Result<(), String> {
    let bad = || Err(format!("`{v}` is not a version like 1.2.3 or 1.2.3-rc.1"));
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    if nums.len() != 3 || nums.iter().any(|n| n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) || (n.len() > 1 && n.starts_with('0'))) {
        return bad();
    }
    if let Some(p) = pre
        && (p.is_empty() || p.split('.').any(|id| id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')))
    {
        return bad();
    }
    Ok(())
}

/// Index and text of the `version` line in `[workspace.package]`.
fn find_line(manifest: &str) -> Result<(usize, &str), String> {
    let mut in_section = false;
    for (i, line) in manifest.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[workspace.package]";
            continue;
        }
        if in_section
            && let Some(rest) = t.strip_prefix("version")
            && rest.trim_start().starts_with('=')
        {
            return Ok((i, line));
        }
    }
    Err("no `version = \"…\"` in [workspace.package] of the root Cargo.toml".into())
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    match args {
        [] => {
            println!("{}", read(&text)?);
            Ok(())
        }
        ["set", new] => {
            let old = read(&text)?;
            let updated = replace(&text, new)?;
            // Write atomically: a half-written root manifest breaks every build in the workspace. The
            // temporary file sits next to the manifest (same filesystem; `*~` is git-ignored).
            let tmp = root.join("Cargo.toml~");
            std::fs::write(&tmp, updated).map_err(|e| format!("write {}: {e}", tmp.display()))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("replace {}: {e}", path.display()))?;
            // Refresh the workspace members' entries in Cargo.lock (no dependency upgrades).
            let mut c = crate::cargo();
            c.args(["update", "--workspace", "--offline"]);
            if crate::run(c, "cargo update --workspace --offline").is_err() {
                let mut c = crate::cargo();
                c.args(["update", "--workspace"]);
                crate::run(c, "cargo update --workspace")?;
            }
            println!("version: {old} -> {new}");
            Ok(())
        }
        _ => Err("usage: cargo xtask version [set X.Y.Z[-pre]]".into()),
    }
}
