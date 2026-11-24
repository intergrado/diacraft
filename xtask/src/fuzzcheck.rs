//! `cargo xtask fuzz-check`, and the `fuzz check` step of `ci`: every fuzz crate of the repository is type-checked on the
//! toolchain the gate uses.
//!
//! The fuzz crates (`crates/*/fuzz/`) are workspaces of their own: cargo-fuzz wants nightly and sanitizer flags, so they
//! stay out of the DiaCraft workspace, and nothing else the gate runs compiles them. A target whose raw string was closed
//! early went unnoticed that way until someone built it by hand. This step runs `cargo check --all-targets --all-features` on
//! each fuzz crate's own manifest, with `--cfg fuzzing` as a libFuzzer build has it: no nightly, no sanitizer and no linking,
//! which is all that a compile error needs.
//!
//! **No C++.** `libfuzzer-sys`'s build script compiles LLVM's libFuzzer (26 C++ files in 0.4.13) unless
//! `CUSTOM_LIBFUZZER_PATH` names a library, and a check never links one. The command names an empty file, so the build script
//! prints its link directives and compiles nothing, and the step needs no C++ compiler.
//!
//! **Lock, and no network unless it is needed.** The fuzz crates' `Cargo.lock` is git-ignored. A crate's lock is made from the
//! workspace's, again whenever the workspace's changes (a copy of the one last used is kept beside the build): the type-check
//! sees the dependency versions that ship, and the git dependencies stay at their pinned revisions, instead of the newest of
//! everything. The two crates the workspace does not use (`libfuzzer-sys`, `arbitrary`) are added by `cargo update --workspace`
//! run **offline first**, from cargo's own cache, and online only when that fails: the gate must not turn red because the network
//! is down (the licence check, `deny.rs`, keeps network-bound work out for the same reason). The fuzz manifests pin both crates
//! to an exact version (a test in `xtask/tests/it/fuzzcheck.rs` checks it), so a release of either cannot change the step.
//!
//! **Every crate is accounted for.** A manifest that is not the root's, not a workspace member and not a fuzz crate would be a
//! crate that no gate step builds, and the step fails on it by name. A fuzz crate is found by its folder's name (`fuzz`), by the
//! `cargo-fuzz = true` line, or by its dependency on `libfuzzer-sys`, so one in a folder of any other name is still checked.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ci::Outcome;

/// The empty library `CUSTOM_LIBFUZZER_PATH` names (a file that exists, so the build script does not run again each time).
pub const STUB: &str = "libfuzzer-stub.a";

/// Why a repository without a fuzz crate skips the step.
const NONE_FOUND: &str = "no fuzz crate in the repository";

/// The separator of the flags in `CARGO_ENCODED_RUSTFLAGS`.
const FLAG_SEPARATOR: char = '\x1f';

/// A line of a manifest without its comment.
fn code_of(line: &str) -> &str {
    line.split('#').next().unwrap_or_default()
}

/// Whether the manifest line declares a dependency on `libfuzzer-sys`: a key (`libfuzzer-sys = ...`, `libfuzzer-sys.workspace = true`)
/// or a table header (`[dependencies.libfuzzer-sys]`, also with the name written with an underscore).
fn names_libfuzzer_dependency(line: &str) -> bool {
    let line = code_of(line).trim();
    ["libfuzzer-sys", "libfuzzer_sys"].iter().any(|name| {
        let key = line.strip_prefix(name).is_some_and(|rest| matches!(rest.trim_start().chars().next(), Some('=' | '.')));
        key || (line.starts_with('[') && line.ends_with(&format!(".{name}]")))
    })
}

/// Whether the manifest at `path` (relative, `/` separators) with `text` is a fuzz crate's: its folder is named `fuzz`, or it says
/// `cargo-fuzz = true` (the line cargo-fuzz finds the crate by), or it depends on `libfuzzer-sys`. The root manifest never is.
pub fn is_fuzz_manifest(path: &str, text: &str) -> bool {
    let Some(parent) = path.strip_suffix("/Cargo.toml") else { return false };
    let named_fuzz = parent.rsplit('/').next() == Some("fuzz");
    let marked = text.lines().any(|line| code_of(line).split_whitespace().collect::<String>() == "cargo-fuzz=true");
    named_fuzz || marked || text.lines().any(names_libfuzzer_dependency)
}

/// The directories of the fuzz crates among `files` (relative paths), sorted. `read` gives a file's text, for the marker and the
/// dependency; a manifest it cannot read counts by its folder's name alone.
pub fn find_crates(files: &[String], read: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut found: Vec<String> = files
        .iter()
        .filter(|path| path.ends_with("/Cargo.toml"))
        .filter(|path| is_fuzz_manifest(path, &read(path).unwrap_or_default()))
        .filter_map(|path| path.strip_suffix("/Cargo.toml").map(str::to_owned))
        .collect();
    found.sort();
    found
}

/// The package and target a `could not compile` line of cargo's output names, such as ``error: could not compile `p` (bin "t") due to 1
/// previous error`` gives `` `p` (bin "t") ``. Any other line gives `None`.
pub fn failed_target(line: &str) -> Option<String> {
    let rest = line.trim_end().strip_prefix("error: could not compile ")?;
    let named = rest.split(" due to ").next().unwrap_or(rest).trim();
    (!named.is_empty()).then(|| named.to_owned())
}

/// Makes `crate_dir`'s lock the workspace's when it has none, or when the workspace's lock is not the one last used: `record` holds a
/// copy of that one. Returns whether it did. Cargo adds what the crate needs on top (`resolve`). A repository with no lock has nothing to
/// give. A `Cargo.lock` that is a link to nothing counts as none: it is replaced by a file, never written through. The lock is written beside
/// its place and moved into it, so that a cargo reading it meanwhile sees the old one or the new, never half.
pub fn seed_lock(root: &Path, crate_dir: &Path, record: &Path) -> Result<bool, String> {
    let workspace = root.join("Cargo.lock");
    if !workspace.is_file() {
        return Ok(false);
    }
    let wanted = std::fs::read(&workspace).map_err(|e| format!("cannot read {}: {e}", workspace.display()))?;
    let own = crate_dir.join("Cargo.lock");
    // `exists` follows a link: a lock that is a link to nothing is no lock, and is replaced below (the rename replaces the link itself).
    let has_lock = own.exists();
    if has_lock && std::fs::read(record).is_ok_and(|last| last == wanted) {
        return Ok(false);
    }
    let beside = crate_dir.join("Cargo.lock.seeding");
    std::fs::write(&beside, &wanted).map_err(|e| format!("cannot write {}: {e}", beside.display()))?;
    std::fs::rename(&beside, &own).map_err(|e| format!("cannot give {} the lock of the workspace: {e}", own.display()))?;
    // The record last: a run that stops before it seeds again.
    std::fs::write(record, &wanted).map_err(|e| format!("cannot write {}: {e}", record.display()))?;
    Ok(true)
}

/// The value of `CARGO_ENCODED_RUSTFLAGS` for the check: the flags the environment already gave (`encoded` is its
/// `CARGO_ENCODED_RUSTFLAGS`, which cargo reads in place of `RUSTFLAGS`; `plain` is `RUSTFLAGS`, split at white space), then
/// `--cfg fuzzing`, as a libFuzzer build has it.
pub fn encoded_rustflags(encoded: Option<&str>, plain: Option<&str>) -> String {
    let mut flags: Vec<&str> = match (encoded, plain) {
        (Some(encoded), _) => encoded.split(FLAG_SEPARATOR).filter(|f| !f.is_empty()).collect(),
        (None, Some(plain)) => plain.split_whitespace().collect(),
        (None, None) => Vec::new(),
    };
    flags.extend(["--cfg", "fuzzing"]);
    flags.join(&FLAG_SEPARATOR.to_string())
}

/// The `cargo check` of one fuzz crate: its own manifest, every target and every feature (a bin behind `required-features` is skipped
/// without them), `--keep-going` so each broken target is reported, and no colour so the output can be read for `could not compile`.
/// `target_dir` is where it builds; `stub` is the empty library that stands in for libFuzzer. `--cfg fuzzing` is added to the flags of
/// this one command, which builds in `target_dir` and nowhere else.
pub fn command(crate_dir: &Path, target_dir: &Path, stub: &Path) -> Command {
    let flags = encoded_rustflags(std::env::var("CARGO_ENCODED_RUSTFLAGS").ok().as_deref(), std::env::var("RUSTFLAGS").ok().as_deref());
    let mut c = crate::cargo();
    c.current_dir(crate_dir)
        .arg("check")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .args(["--all-targets", "--all-features", "--keep-going", "--color", "never"])
        .arg("--target-dir")
        .arg(target_dir)
        .env("CUSTOM_LIBFUZZER_PATH", stub)
        .env("CARGO_ENCODED_RUSTFLAGS", flags);
    c
}

/// `cargo update --workspace` on the crate's manifest: it completes the lock the crate was given (the packages the crate needs and the
/// lock lacks are added, those it has stay at their versions) and downloads nothing. `offline` keeps it to cargo's own cache.
pub fn resolve_command(crate_dir: &Path, offline: bool) -> Command {
    let mut c = crate::cargo();
    c.current_dir(crate_dir).args(["update", "--workspace", "--color", "never"]).arg("--manifest-path").arg(crate_dir.join("Cargo.toml"));
    if offline {
        c.arg("--offline");
    }
    c
}

/// The last lines of cargo's error output, on one line: the cause is at the end of cargo's `Caused by` chain.
pub fn tail_of(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let from = lines.len().saturating_sub(6);
    let joined = lines.get(from..).unwrap_or_default().join(" | ");
    joined.chars().take(900).collect()
}

/// Runs `resolve_command`: `Ok`, or the tail of what cargo said.
fn run_resolve(crate_dir: &Path, offline: bool) -> Result<(), String> {
    let out = resolve_command(crate_dir, offline).output().map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(tail_of(&out.stderr)) }
}

/// Resolves offline first, and online only when that fails. `attempt(offline)` makes one try. A crate whose dependencies are in cargo's
/// cache needs no network; when they are not, the second try may, and if it fails too the error says the network or the lock is
/// the matter, never that something does not compile.
pub fn resolve(mut attempt: impl FnMut(bool) -> Result<(), String>) -> Result<(), String> {
    let offline = match attempt(true) {
        Ok(()) => return Ok(()),
        Err(offline) => offline,
    };
    attempt(false).map_err(|online| {
        format!("its dependencies cannot be resolved or fetched, a network or lock problem and not a fault of the code. Offline: {offline}. Online: {online}")
    })
}

/// What `cargo metadata --no-deps` says of the workspace of a fuzz crate.
#[derive(Debug, Default)]
pub struct Workspace {
    /// The source files of the fuzz targets (the `bin` targets) of its packages.
    pub bins: Vec<PathBuf>,
    /// The manifests of its packages (the crate's own, and any package that is a member through a path inside its folder).
    pub manifests: Vec<PathBuf>,
}

/// The workspace of the crate, from `cargo metadata --no-deps` on its manifest.
pub fn workspace_of(crate_dir: &Path) -> Result<Workspace, String> {
    let manifest = crate_dir.join("Cargo.toml");
    let out = crate::cargo()
        .current_dir(crate_dir)
        .args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo cannot read the manifest:\n{}", String::from_utf8_lossy(&out.stderr).trim_end()));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))?;
    let packages = json.get("packages").and_then(|p| p.as_array()).map(Vec::as_slice).unwrap_or_default();
    let mut workspace = Workspace::default();
    for package in packages {
        workspace.manifests.extend(package.get("manifest_path").and_then(|m| m.as_str()).map(PathBuf::from));
        let targets = package.get("targets").and_then(|t| t.as_array()).map(Vec::as_slice).unwrap_or_default();
        let bins = targets.iter().filter(|t| t.get("kind").and_then(|k| k.as_array()).is_some_and(|kinds| kinds.iter().any(|k| k == "bin")));
        workspace.bins.extend(bins.filter_map(|t| t.get("src_path")?.as_str()).map(PathBuf::from));
    }
    Ok(workspace)
}

/// The source files of the fuzz targets of the crate's workspace (`workspace_of(..).bins`).
pub fn bin_sources(crate_dir: &Path) -> Result<Vec<PathBuf>, String> {
    workspace_of(crate_dir).map(|w| w.bins)
}

/// The `.rs` files directly in the crate's `fuzz_targets` folder (`files` are repository paths) that no entry of `listed` (the
/// targets' sources) is. cargo-fuzz builds the targets the manifest lists, so such a file is never compiled by anything.
pub fn unlisted_targets(root: &Path, crate_dir: &str, files: &[String], listed: &[PathBuf]) -> Vec<String> {
    let folder = format!("{crate_dir}/fuzz_targets/");
    let listed: Vec<PathBuf> = listed.iter().map(|p| canonical(p)).collect();
    let mut found: Vec<String> = files
        .iter()
        .filter(|f| f.strip_prefix(&folder).is_some_and(|rest| rest.ends_with(".rs") && !rest.contains('/')))
        .filter(|f| !listed.contains(&canonical(&root.join(f))))
        .cloned()
        .collect();
    found.sort();
    found
}

/// The path with links and `..` resolved when the file is there, as it is otherwise.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Runs `cmd` with its error output copied to `out` as it arrives. Returns whether it succeeded and the targets its output says did not
/// compile, in the order cargo printed them.
pub fn run_reading_errors(mut cmd: Command, out: &mut dyn Write) -> Result<(bool, Vec<String>), String> {
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("failed to spawn cargo: {e}"))?;
    let mut failed = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // A copy that cannot be written (a closed terminal) must not fail the check: the verdict is the exit status.
                    let _ = out.write_all(&line);
                    failed.extend(failed_target(&String::from_utf8_lossy(&line)));
                }
            }
        }
    }
    let status = child.wait().map_err(|e| format!("cargo did not finish: {e}"))?;
    Ok((status.success(), failed))
}

/// Checks one crate, whose workspace is `workspace`: `Ok(number of targets)`, or what is wrong with it.
fn check_crate(root: &Path, dir: &str, files: &[String], workspace: &Workspace, target_dir: &Path, stub: &Path) -> Result<usize, String> {
    let crate_dir = root.join(dir);
    let unlisted = unlisted_targets(root, dir, files, &workspace.bins);
    if !unlisted.is_empty() {
        return Err(format!(
            "{dir}: {} in fuzz_targets is not a [[bin]] of the manifest, so nothing builds it: add a [[bin]] for it, or move it out of the folder",
            unlisted.join(", ")
        ));
    }
    if workspace.bins.is_empty() {
        return Err(format!("{dir}: the manifest lists no fuzz target (a [[bin]] for each file in fuzz_targets)"));
    }
    let record = target_dir.join(format!("{}.seeded-lock", dir.replace('/', "-")));
    seed_lock(root, &crate_dir, &record).map_err(|e| format!("{dir}: {e}"))?;
    resolve(|offline| run_resolve(&crate_dir, offline)).map_err(|e| format!("{dir}: {e}"))?;
    eprintln!("$ cargo check --manifest-path {dir}/Cargo.toml --all-targets --all-features ({} targets)", workspace.bins.len());
    let (ok, failed) = run_reading_errors(command(&crate_dir, target_dir, stub), &mut std::io::stderr()).map_err(|e| format!("{dir}: {e}"))?;
    if ok {
        return Ok(workspace.bins.len());
    }
    if failed.is_empty() {
        return Err(format!(
            "{dir}: cargo check failed and named no target (it stopped before compiling one: a dependency that could not be downloaded, or a build \
             script that failed; the errors are above)"
        ));
    }
    Err(format!("{dir}: does not compile: {}", failed.join(", ")))
}

/// What a passing check covered.
#[derive(Debug, PartialEq, Eq)]
pub struct Checked {
    /// The crates' directories, relative to the root.
    pub crates: Vec<String>,
    /// The targets in them.
    pub targets: usize,
}

/// Type-checks every fuzz crate under `root` into `target_dir`, skipping those whose manifest is in `members` (the workspace's own
/// steps build them), and fails on every other manifest that no step builds. Every crate is checked; the error lists each problem.
pub fn check_crates(root: &Path, target_dir: &Path, members: &HashSet<PathBuf>) -> Result<Checked, String> {
    let files = crate::repo_files(root)?;
    let member_manifests: Vec<PathBuf> = members.iter().map(|m| canonical(m)).collect();
    let fuzz_dirs = find_crates(&files, |p| std::fs::read_to_string(root.join(p)).ok());
    let manifest_of = |dir: &str| canonical(&root.join(dir).join("Cargo.toml"));
    let crates: Vec<String> = fuzz_dirs.iter().filter(|dir| !member_manifests.contains(&manifest_of(dir))).cloned().collect();
    // What some step builds: the workspace's members, the fuzz crates, and the packages of a fuzz crate's own workspace.
    let mut covered: Vec<PathBuf> = member_manifests.clone();
    covered.extend(fuzz_dirs.iter().map(|dir| manifest_of(dir)));
    let mut problems = Vec::new();
    let mut targets = 0;
    // The folders of fuzz crates whose workspace cargo could not read: what is inside them is not known, and is not reported again.
    let mut unreadable: Vec<String> = Vec::new();
    let stub = target_dir.join(STUB);
    if !crates.is_empty() {
        std::fs::create_dir_all(target_dir).map_err(|e| format!("cannot create {}: {e}", target_dir.display()))?;
        if !stub.is_file() {
            std::fs::write(&stub, b"").map_err(|e| format!("cannot write {}: {e}", stub.display()))?;
        }
    }
    for dir in &crates {
        match workspace_of(&root.join(dir)) {
            Err(problem) => {
                problems.push(format!("{dir}: {problem}"));
                unreadable.push(format!("{dir}/"));
            }
            Ok(workspace) => {
                covered.extend(workspace.manifests.iter().map(|m| canonical(m)));
                match check_crate(root, dir, &files, &workspace, target_dir, &stub) {
                    Ok(n) => targets += n,
                    Err(problem) => problems.push(problem),
                }
            }
        }
    }
    for manifest in files.iter().filter(|f| f.ends_with("/Cargo.toml")) {
        if !unreadable.iter().any(|dir| manifest.starts_with(dir.as_str())) && !covered.contains(&canonical(&root.join(manifest))) {
            problems.push(format!(
                "{manifest}: a crate outside the workspace, and no gate step builds it: make it a member of the workspace, or a fuzz crate (a folder named \
                 fuzz, a `cargo-fuzz = true` line, or a dependency on libfuzzer-sys)"
            ));
        }
    }
    if !problems.is_empty() {
        return Err(format!("fuzz check failed ({} problems): {}", problems.len(), problems.join("; ")));
    }
    Ok(Checked { crates, targets })
}

/// The step: `check_crates`, said in a line. A repository with no fuzz crate to check skips, with the reason.
pub fn check(root: &Path, target_dir: &Path, members: &HashSet<PathBuf>) -> Result<Outcome, String> {
    let checked = check_crates(root, target_dir, members)?;
    if checked.crates.is_empty() {
        println!("fuzz check: {NONE_FOUND}");
        return Ok(Outcome::Skipped(NONE_FOUND.into()));
    }
    println!("fuzz check: {} crates, {} targets type-checked ({})", checked.crates.len(), checked.targets, checked.crates.join(", "));
    Ok(Outcome::Ok)
}

/// Where the step builds: the `fuzz-check` folder of the workspace's target directory (the one the gate's cache keeps), from
/// `cargo metadata`, which honours `CARGO_TARGET_DIR` and the configuration; `root/target` when the metadata names none.
pub fn check_folder(meta: &serde_json::Value, root: &Path) -> PathBuf {
    let target = meta.get("target_directory").and_then(|t| t.as_str()).map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    target.join("fuzz-check")
}

/// The manifests of the workspace's packages, from `cargo metadata`.
pub fn members_of(meta: &serde_json::Value) -> HashSet<PathBuf> {
    let packages = meta.get("packages").and_then(|p| p.as_array()).map(Vec::as_slice).unwrap_or_default();
    packages.iter().filter_map(|p| p.get("manifest_path")?.as_str()).map(PathBuf::from).collect()
}

/// The step on the repository at `root`, whose workspace `cargo metadata` described as `meta`: the build folder and the members come from it.
pub fn run_with(root: &Path, meta: &serde_json::Value) -> Result<Outcome, String> {
    check(root, &check_folder(meta, root), &members_of(meta))
}

/// The step as `ci` and the command run it.
pub fn run() -> Result<Outcome, String> {
    run_with(&crate::root(), &crate::metadata()?)
}
