//! `cargo xtask mcp-server-json`: the entry of DiaCraft's MCP server in the official MCP Registry, the `server.json` that `mcp-publisher publish` sends.
//!
//! **Why a command and not a file in the repository.** An entry that offers a bundle to install names the file by its address and its SHA-256, and the hash exists only once the
//! release has built the file. So the file is made from the release's `SHA256SUMS.txt`, after the release is published (docs/releasing.md, "Publish the MCP server to the
//! registry"), and is never edited by hand. It is made for the checkout of the tag: the version, the repository and the list of the bundles are the checkout's, and sums of
//! another release are refused by name.
//!
//! **The bundles** are the six files `packaging/mcp-bundle.sh` makes, one for each system and CPU the downloads are built for. The format of a bundle names the system a
//! bundle is for (`compatibility.platforms`) and has no word for a CPU, so each is a file of its own, told apart by its name; the registry's entry has no field for a system or a
//! CPU either, and holds the six side by side.
//!
//! **The name** `app.diacraft/diacraft` is the reverse of the domain `diacraft.app`, which the file `/.well-known/mcp-registry-auth` of that site proves (docs/website.md, "The
//! registry's proof of the domain").

use std::fs;
use std::io::Read;
use std::path::Path;

use serde_json::json;

/// The schema the entry says it follows: the registry's, of the 12th of December 2025.
pub const SCHEMA: &str = "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json";

/// The name of the server in the registry: the reverse of the domain `diacraft.app`, then the server's own name.
pub const REGISTRY_NAME: &str = "app.diacraft/diacraft";

/// What the registry lists first. At most 100 characters (the registry's limit).
pub const DESCRIPTION: &str = "Draw and edit diagrams, and open and save VSDX, ODG and DiaCraft files, from an AI agent.";

/// The site of the project, which the entry links.
pub const WEBSITE: &str = "https://diacraft.dev";

/// The most of a sums file that is read, in bytes: a release has twenty-two lines of some 100 bytes.
pub const MAX_SUMS: usize = 64 * 1024;

/// The most lines of a sums file.
pub const MAX_LINES: usize = 200;

/// The most of the root `Cargo.toml` that is read, in bytes: the workspace's own is some 14 thousand.
const MAX_MANIFEST: usize = 1024 * 1024;

/// One bundle of a release: for one system and one CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bundle {
    /// The system as the names of the downloads say it: `linux`, `windows` or `macos`.
    pub os: &'static str,
    /// The CPU as the names of the downloads say it: `x86_64` and `aarch64` for Linux, `x64` and `arm64` for Windows, `arm64` and `x86_64` for macOS.
    pub cpu: &'static str,
    /// The system as a bundle's manifest says it (`compatibility.platforms`, the word of `process.platform`): `linux`, `win32` or `darwin`.
    pub platform: &'static str,
}

/// The six bundles of a release, in the order the entry lists them.
pub const BUNDLES: [Bundle; 6] = [
    Bundle { os: "linux", cpu: "x86_64", platform: "linux" },
    Bundle { os: "linux", cpu: "aarch64", platform: "linux" },
    Bundle { os: "windows", cpu: "x64", platform: "win32" },
    Bundle { os: "windows", cpu: "arm64", platform: "win32" },
    Bundle { os: "macos", cpu: "arm64", platform: "darwin" },
    Bundle { os: "macos", cpu: "x86_64", platform: "darwin" },
];

/// The words every bundle's file name starts with: the registry wants "mcp" in the address of a bundle.
pub const PREFIX: &str = "diacraft-mcp-";

/// The name of the file of `bundle` in the release of `version` (the tag without its `v`): `diacraft-mcp-<version>-<system>-<cpu>.mcpb`.
pub fn bundle_name(version: &str, bundle: &Bundle) -> String {
    format!("{PREFIX}{version}-{}-{}.mcpb", bundle.os, bundle.cpu)
}

/// The same with `{version}` left open, as the release workflow's list is read.
pub fn bundle_pattern(bundle: &Bundle) -> String {
    bundle_name("{version}", bundle)
}

/// The lines of a sums file as (the hash, the name of the file).
fn parse_sums(text: &str) -> Result<Vec<(String, String)>, String> {
    if text.len() > MAX_SUMS {
        return Err(format!("the sums file is too large: {} bytes, and at most {MAX_SUMS} are read", text.len()));
    }
    if text.lines().count() > MAX_LINES {
        return Err(format!("the sums file has more than {MAX_LINES} lines: it is not the one of a release"));
    }
    let mut found: Vec<(String, String)> = Vec::new();
    for (index, line) in text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty()) {
        let number = index + 1;
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("line {number} is not a line of sha256sum (the hash, two spaces and the file's name): `{}`", shown(line)))?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(format!("line {number}: `{}` is not a SHA-256 (64 lower-case hexadecimal digits)", shown(hash)));
        }
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(format!("line {number}: `{}` is a path into a folder, and the sums name the files of the release", shown(name)));
        }
        if found.iter().any(|(_, seen)| seen == name) {
            return Err(format!("line {number}: `{}` is in the sums twice", shown(name)));
        }
        found.push((hash.to_owned(), name.to_owned()));
    }
    Ok(found)
}

/// At most 80 characters of what a message quotes from a file.
fn shown(text: &str) -> String {
    text.chars().take(80).collect()
}

/// `https://github.com/<owner>/<name>` from a repository address as a manifest writes it (a trailing `/` or `.git` is left out), or why it is none.
fn github_address(repository: &str) -> Result<String, String> {
    let bad = || format!("the repository `{}` is not an address like https://github.com/<owner>/<name>", shown(repository));
    let path = repository.strip_prefix("https://github.com/").ok_or_else(bad)?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let word = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    match path.split_once('/') {
        Some((owner, name)) if word(owner) && word(name) => Ok(format!("https://github.com/{owner}/{name}")),
        _ => Err(bad()),
    }
}

/// The `server.json` of the release of `version`, from the text of its `SHA256SUMS.txt`: one `mcpb` package for each of the [`BUNDLES`], by the address of its file in the release
/// of the repository `repository` and its SHA-256. Refuses sums that miss a bundle, name one twice or one that is not listed, or are of another release.
pub fn server_json(sums: &str, version: &str, repository: &str) -> Result<String, String> {
    crate::version::validate(version)?;
    let repository = github_address(repository)?;
    let lines = parse_sums(sums)?;
    // Sums of another release say so by the name of a bundle with another version in it.
    for bundle in &BUNDLES {
        let tail = bundle_name("", bundle);
        let tail = tail.strip_prefix(PREFIX).unwrap_or(&tail);
        for (_, name) in &lines {
            let Some(other) = name.strip_prefix(PREFIX).and_then(|rest| rest.strip_suffix(tail)) else { continue };
            if !other.is_empty() && other != version {
                return Err(format!(
                    "the sums are of the release {other} (`{name}`), another release than {version}, which this checkout is: run the command on the checkout of the tag v{other}"
                ));
            }
        }
    }
    let wanted: Vec<String> = BUNDLES.iter().map(|bundle| bundle_name(version, bundle)).collect();
    if let Some((_, name)) = lines.iter().find(|(_, name)| name.starts_with(PREFIX) && !wanted.contains(name)) {
        return Err(format!(
            "`{name}` is one of the bundles of the sums and not one of the bundles of this command (the six of BUNDLES in xtask/src/mcp.rs): is the list of it out of date?"
        ));
    }
    let mut packages = Vec::new();
    for name in &wanted {
        let (hash, _) =
            lines.iter().find(|(_, seen)| seen == name).ok_or_else(|| format!("the sums have no line for `{name}`: the release is not complete"))?;
        packages.push(json!({
            "registryType": "mcpb",
            "identifier": format!("{repository}/releases/download/v{version}/{name}"),
            "version": version,
            "fileSha256": hash,
            "transport": { "type": "stdio" },
        }));
    }
    let entry = json!({
        "$schema": SCHEMA,
        "name": REGISTRY_NAME,
        "title": "DiaCraft",
        "description": DESCRIPTION,
        "version": version,
        "websiteUrl": WEBSITE,
        "repository": { "url": repository, "source": "github" },
        "packages": packages,
    });
    let mut text = serde_json::to_string_pretty(&entry).map_err(|e| format!("server.json: {e}"))?;
    text.push('\n');
    Ok(text)
}

/// The text of a file of at most `limit` bytes.
fn read_bounded(path: &Path, limit: usize) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut text = String::new();
    file.take(limit as u64 + 1).read_to_string(&mut text).map_err(|e| format!("{}: {e}", path.display()))?;
    if text.len() > limit {
        return Err(format!("{}: the file is too large: at most {limit} bytes are read", path.display()));
    }
    Ok(text)
}

/// The `repository` of `[workspace.package]` in the root `Cargo.toml`.
fn workspace_repository(manifest: &str) -> Result<String, String> {
    let table: toml::Table = manifest.parse().map_err(|e| format!("Cargo.toml: {e}"))?;
    table
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("package"))
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("repository"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "no `repository = \"…\"` in [workspace.package] of the root Cargo.toml".to_owned())
}

const USAGE: &str = "usage: cargo xtask mcp-server-json <SHA256SUMS.txt> [--out <server.json>]";

/// `cargo xtask mcp-server-json <SHA256SUMS.txt> [--out <server.json>]`.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let (sums, out) = match args {
        [sums] if !sums.starts_with("--") => (*sums, None),
        [sums, "--out", out] if !sums.starts_with("--") => (*sums, Some(*out)),
        _ => return Err(USAGE.to_owned()),
    };
    let sums = read_bounded(Path::new(sums), MAX_SUMS)?;
    let manifest = read_bounded(&root.join("Cargo.toml"), MAX_MANIFEST)?;
    let version = crate::version::read(&manifest)?;
    let text = server_json(&sums, &version, &workspace_repository(&manifest)?)?;
    match out {
        Some(out) => fs::write(out, text).map_err(|e| format!("{out}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(())
}
