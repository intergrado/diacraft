//! Asking cargo which crates each program is built from, on each platform, and reading the answers.
//!
//! **Why `cargo tree` besides `cargo metadata`.** `cargo metadata --filter-platform` gives every package's licence, authors, repository and folder, but
//! its dependency graph is the whole workspace's: features are unified across all members, so it names crates (the OpenGL backend of egui, the PDF reader, the
//! clipboard) that `cargo build -p diacraft -p diacraft-cli` does not compile. `cargo tree -p diacraft -p diacraft-cli --target T -e normal,no-proc-macro` is
//! what those two builds compile and link: no build script's own dependencies, no proc macros (they run in the compiler and are not in the program), no tests.
//! The web build is the same with `-p diacraft-web --target wasm32-unknown-unknown`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::assemble::{Allowed, Build, Inputs, Package};
use super::policy::TARGETS;

/// Runs cargo for every platform and reads `deny.toml`.
pub fn inputs(root: &Path) -> Result<Inputs, String> {
    let deny = std::fs::read_to_string(root.join("deny.toml")).map_err(|e| format!("deny.toml: {e}"))?;
    let allowed = allowed_from(&deny)?;
    let mut packages: BTreeMap<(String, String, Option<String>), Package> = BTreeMap::new();
    let mut builds = Vec::new();
    for (triple, label, targets_programs) in TARGETS {
        let json = cargo_output(root, &["metadata", "--format-version", "1", "--locked", "--filter-platform", triple])?;
        for pkg in packages_from(&json, root)? {
            let key = (pkg.name.clone(), pkg.version.clone(), pkg.source.clone());
            // The build-dependency flag is true when any platform says so.
            match packages.get_mut(&key) {
                Some(have) => have.native |= pkg.native,
                None => {
                    packages.insert(key, pkg);
                }
            }
        }
        let mut programs = Vec::new();
        for program in *targets_programs {
            programs.push(((*program).to_owned(), tree(root, triple, &[program])?));
        }
        builds.push(Build { triple: (*triple).to_owned(), label: (*label).to_owned(), programs, together: tree(root, triple, targets_programs)? });
    }
    Ok(Inputs { packages: packages.into_values().collect(), builds, allowed })
}

/// The standard output of `cargo <args>` in the workspace root.
fn cargo_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut c = crate::cargo();
    c.current_dir(root).args(args);
    run_output(c, &format!("cargo {}", args.join(" ")))
}

fn run_output(mut c: Command, what: &str) -> Result<Vec<u8>, String> {
    let out = c.output().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(12).collect();
        Err(format!(
            "{what} failed (a platform's crates that this machine has not fetched need `cargo fetch --target <triple>` once, with a network):\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ))
    }
}

/// The crates `cargo build --target T -p a -p b` compiles into the programs, as (name, version).
fn tree(root: &Path, triple: &str, programs: &[&str]) -> Result<BTreeSet<(String, String)>, String> {
    let mut args: Vec<&str> = vec!["tree", "--locked", "--target", triple, "-e", "normal,no-proc-macro", "--prefix", "none", "--format", "{p}"];
    for program in programs {
        args.extend(["-p", program]);
    }
    let out = cargo_output(root, &args)?;
    Ok(parse_tree(&String::from_utf8_lossy(&out)))
}

/// The (name, version) of each line of `cargo tree --prefix none --format "{p}"` (`serde v1.0.0`, `diacraft v0.2.0 (/path)`, `x v1.0.0 (*)`).
pub fn parse_tree(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if let (Some(name), Some(version)) = (words.next(), words.next())
            && let Some(version) = version.strip_prefix('v')
        {
            out.insert((name.to_owned(), version.to_owned()));
        }
    }
    out
}

/// The packages of `cargo metadata` output. A package of this workspace (its folder is under `root`) is DiaCraft's own.
pub fn packages_from(json: &[u8], root: &Path) -> Result<Vec<Package>, String> {
    let v: Value = serde_json::from_slice(json).map_err(|e| format!("cargo metadata: bad JSON: {e}"))?;
    let list = v.get("packages").and_then(Value::as_array).ok_or("cargo metadata: no `packages`")?;
    let names: BTreeMap<&str, &str> = list.iter().filter_map(|p| Some((p.get("id")?.as_str()?, p.get("name")?.as_str()?))).collect();
    let mut native: BTreeSet<&str> = BTreeSet::new();
    for node in v.pointer("/resolve/nodes").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = node.get("id").and_then(Value::as_str) else { continue };
        for dep in node.get("deps").and_then(Value::as_array).into_iter().flatten() {
            let target = dep.get("pkg").and_then(Value::as_str).and_then(|p| names.get(p));
            let builds = dep
                .get("dep_kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|k| k.get("kind").and_then(Value::as_str) == Some("build")));
            if builds && target.is_some_and(|n| matches!(*n, "cc" | "cmake")) {
                native.insert(id);
            }
        }
    }
    let mut out = Vec::new();
    for p in list {
        let text = |key: &str| p.get(key).and_then(Value::as_str).map(str::to_owned);
        let (Some(name), Some(version), Some(manifest)) = (text("name"), text("version"), text("manifest_path")) else {
            return Err("cargo metadata: a package has no name, version or manifest_path".to_owned());
        };
        let dir = PathBuf::from(&manifest).parent().map(Path::to_path_buf).unwrap_or_default();
        let source = text("source");
        let id = p.get("id").and_then(Value::as_str).unwrap_or_default();
        out.push(Package {
            own: source.is_none() && dir.starts_with(root),
            license: text("license"),
            authors: p.get("authors").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.as_str().map(str::to_owned)).collect(),
            repository: text("repository"),
            native: native.contains(id),
            name,
            version,
            source,
            dir,
        });
    }
    Ok(out)
}

/// The licences `deny.toml` allows.
pub fn allowed_from(deny: &str) -> Result<Allowed, String> {
    let table: toml::Table = toml::from_str(deny).map_err(|e| format!("deny.toml: {e}"))?;
    let strings = |value: Option<&toml::Value>| -> Result<BTreeSet<String>, String> {
        let list = value.and_then(toml::Value::as_array).ok_or("deny.toml: a licence list is not a list")?;
        list.iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| "deny.toml: a licence in the allow list is not a plain string".to_owned()))
            .collect()
    };
    let licenses = table.get("licenses").ok_or("deny.toml: no [licenses] table")?;
    let mut allowed = Allowed { global: strings(licenses.get("allow"))?, per_crate: BTreeMap::new() };
    for exception in licenses.get("exceptions").and_then(toml::Value::as_array).into_iter().flatten() {
        let name = exception.get("crate").and_then(toml::Value::as_str).ok_or("deny.toml: a licence exception names no crate")?;
        allowed.per_crate.entry(name.to_owned()).or_default().extend(strings(exception.get("allow"))?);
    }
    Ok(allowed)
}
