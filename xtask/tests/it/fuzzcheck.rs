//! `cargo xtask fuzz-check` and its step of `ci`: every fuzz crate of the repository is type-checked with the gate's own
//! toolchain, so a fuzz target that does not compile fails the gate (the `canon_compare` was never compiled:
//! the fuzz crates are workspaces of their own, which nothing built).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use xtask::ci::Outcome;
use xtask::fuzzcheck::{
    self, Checked, check, check_crates, check_folder, command, encoded_rustflags, failed_target, find_crates, is_fuzz_manifest, members_of, resolve,
    resolve_command, run_reading_errors, run_with, seed_lock, tail_of, unlisted_targets,
};

use crate::{TempDir, git_repo};

const MARKER: &str = "[package]\nname = \"x\"\n\n[package.metadata]\ncargo-fuzz = true\n";

// --- finding the fuzz crates ---

#[test]
fn a_manifest_in_a_folder_named_fuzz_is_a_fuzz_manifest_whatever_it_says() {
    assert!(is_fuzz_manifest("crates/odg/fuzz/Cargo.toml", "[package]\nname = \"x\"\n"));
    assert!(is_fuzz_manifest("fuzz/Cargo.toml", ""));
}

#[test]
fn a_manifest_that_says_cargo_fuzz_is_a_fuzz_manifest_wherever_it_is() {
    assert!(is_fuzz_manifest("tools/odg-fuzzing/Cargo.toml", MARKER));
    assert!(is_fuzz_manifest("a/Cargo.toml", "[package.metadata]\ncargo-fuzz=true # cargo-fuzz finds the crate by this\n"));
}

#[test]
fn other_manifests_are_not() {
    assert!(!is_fuzz_manifest("Cargo.toml", MARKER), "the root manifest is the workspace, never a fuzz crate");
    assert!(!is_fuzz_manifest("crates/odg/Cargo.toml", "[package]\nname = \"diacraft-odg\"\n"));
    assert!(!is_fuzz_manifest("crates/odg/fuzz/notes.toml", ""), "only a file named Cargo.toml is a manifest");
    assert!(!is_fuzz_manifest("crates/fuzzy/Cargo.toml", ""), "a folder that merely starts with fuzz is not named fuzz");
    assert!(!is_fuzz_manifest("a/Cargo.toml", "[package.metadata]\ncargo-fuzz = false\n"));
    assert!(!is_fuzz_manifest("a/Cargo.toml", "# cargo-fuzz = true\n"), "a comment does not say it");
}

#[test]
fn the_crates_are_the_directories_of_the_fuzz_manifests_in_the_file_list() {
    let files: Vec<String> =
        ["Cargo.toml", "crates/b/fuzz/Cargo.toml", "crates/a/Cargo.toml", "crates/a/fuzz/Cargo.toml", "crates/a/src/lib.rs", "fuzz.md"]
            .map(String::from)
            .to_vec();
    let found = find_crates(&files, |_| Some(String::new()));
    assert_eq!(found, ["crates/a/fuzz", "crates/b/fuzz"], "sorted, by directory");
}

#[test]
fn a_manifest_that_cannot_be_read_is_still_a_crate_when_its_folder_is_named_fuzz() {
    // Reading is only for the marker: a folder named `fuzz` needs no text, and a manifest nobody can read is no reason to skip it.
    let files = vec!["crates/a/fuzz/Cargo.toml".to_string(), "crates/z/odd/Cargo.toml".to_string()];
    assert_eq!(find_crates(&files, |_| None), ["crates/a/fuzz"]);
}

// --- what cargo prints ---

#[test]
fn cargos_could_not_compile_line_names_the_package_and_the_target() {
    let line = "error: could not compile `diacraft-odg-fuzz` (bin \"canon_compare\") due to 1 previous error";
    assert_eq!(failed_target(line).as_deref(), Some("`diacraft-odg-fuzz` (bin \"canon_compare\")"));
    let with_warnings = "error: could not compile `p` (bin \"a\") due to 2 previous errors; 3 warnings emitted";
    assert_eq!(failed_target(with_warnings).as_deref(), Some("`p` (bin \"a\")"));
    assert_eq!(failed_target("error: could not compile `p` (lib)").as_deref(), Some("`p` (lib)"), "no `due to` tail");
}

#[test]
fn other_lines_name_no_target() {
    for line in [
        "   Compiling libfuzzer-sys v0.4.13",
        "error[E0425]: cannot find value `x` in this scope",
        "warning: unused variable: `x`",
        "error: could not compile",
        "error: could not compile  due to 1 previous error",
        "note: error: could not compile `p` (bin \"a\") is in a note",
        "",
    ] {
        assert_eq!(failed_target(line), None, "{line:?}");
    }
}

// --- the lock file ---

/// A crate folder with a manifest, a workspace lock holding `workspace`, and the path the record of the last seeding goes to.
fn lock_case(tag: &str, workspace: &str) -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new(tag);
    dir.write("Cargo.lock", workspace);
    dir.write("crates/a/fuzz/Cargo.toml", MARKER);
    let crate_dir = dir.path().join("crates/a/fuzz");
    let record = dir.path().join("seeded-lock");
    (dir, crate_dir, record)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_fuzz_crate_without_a_lock_gets_the_workspaces_so_its_dependencies_are_the_ones_that_ship() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-seed", "# the workspace's lock\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    assert_eq!(read(&crate_dir.join("Cargo.lock")), "# the workspace's lock\n");
    assert_eq!(read(&record), "# the workspace's lock\n", "the lock it was made from is kept");
    assert!(!crate_dir.join("Cargo.lock.seeding").exists(), "the file it was written beside is moved, not left");
}

#[test]
fn a_lock_made_from_the_workspaces_current_one_is_left_as_cargo_completed_it() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-kept", "# the workspace's lock\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    dir.write("crates/a/fuzz/Cargo.lock", "# the workspace's lock, and what cargo added to it\n");
    assert!(!seed_lock(dir.path(), &crate_dir, &record).unwrap());
    assert_eq!(read(&crate_dir.join("Cargo.lock")), "# the workspace's lock, and what cargo added to it\n");
}

#[test]
fn a_changed_workspace_lock_replaces_the_crates_lock() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-follows-unit", "# the workspace's lock\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    dir.write("Cargo.lock", "# the workspace's lock after `cargo update`\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    assert_eq!(read(&crate_dir.join("Cargo.lock")), "# the workspace's lock after `cargo update`\n");
    assert_eq!(read(&record), "# the workspace's lock after `cargo update`\n");
}

#[test]
fn a_lock_with_no_record_of_where_it_came_from_is_replaced_once() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-no-record", "# the workspace's lock\n");
    dir.write("crates/a/fuzz/Cargo.lock", "# a lock from a run by hand\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    assert_eq!(read(&crate_dir.join("Cargo.lock")), "# the workspace's lock\n");
    assert!(!seed_lock(dir.path(), &crate_dir, &record).unwrap(), "and then it has one");
}

#[test]
fn a_record_that_matches_does_not_make_a_lock_that_is_gone_stay_gone() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-gone", "# the workspace's lock\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    std::fs::remove_file(crate_dir.join("Cargo.lock")).unwrap();
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    assert_eq!(read(&crate_dir.join("Cargo.lock")), "# the workspace's lock\n");
}

#[test]
fn without_a_workspace_lock_there_is_nothing_to_seed_and_that_is_not_an_error() {
    let dir = TempDir::new("fuzz-lock-none");
    dir.write("crates/a/fuzz/Cargo.toml", MARKER);
    let record = dir.path().join("seeded-lock");
    assert!(!seed_lock(dir.path(), &dir.path().join("crates/a/fuzz"), &record).unwrap());
    assert!(!dir.path().join("crates/a/fuzz/Cargo.lock").exists());
    assert!(!record.exists());
}

// --- the command ---

fn args_of(c: &std::process::Command) -> Vec<String> {
    c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
}

fn env_of(c: &std::process::Command, key: &str) -> Option<PathBuf> {
    c.get_envs().find(|(k, _)| *k == OsStr::new(key)).and_then(|(_, v)| v).map(PathBuf::from)
}

/// The command is one `cargo check` of the crate's own manifest, on the gate's toolchain (no `+nightly`), into the directory it is given,
/// with no sanitizer flag and no `--locked` (the lock is the crate's and is made on the first run).
#[test]
fn the_command_is_cargo_check_of_the_crates_manifest_with_the_stub_in_place_of_libfuzzer() {
    let c = command(Path::new("/r/crates/a/fuzz"), Path::new("/t/fuzz-check"), Path::new("/t/fuzz-check/libfuzzer-stub.a"));
    let args = args_of(&c);
    assert_eq!(args.first().map(String::as_str), Some("check"), "{args:?}");
    let at = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    assert_eq!(at("--manifest-path").as_deref(), Some("/r/crates/a/fuzz/Cargo.toml"), "{args:?}");
    assert_eq!(at("--target-dir").as_deref(), Some("/t/fuzz-check"), "{args:?}");
    assert_eq!(at("--color").as_deref(), Some("never"), "the output is read for `could not compile`: no colour codes in it");
    for flag in ["--all-targets", "--all-features", "--keep-going"] {
        assert!(args.iter().any(|a| a == flag), "no {flag}: {args:?}");
    }
    for refused in ["--locked", "--frozen", "--release", "--offline"] {
        assert!(!args.iter().any(|a| a == refused), "{refused} in {args:?}");
    }
    assert!(!args.iter().any(|a| a.starts_with('+')), "a toolchain override: {args:?}");
    assert_eq!(env_of(&c, "CUSTOM_LIBFUZZER_PATH"), Some(PathBuf::from("/t/fuzz-check/libfuzzer-stub.a")));
    let flags = env_of(&c, "CARGO_ENCODED_RUSTFLAGS").unwrap().to_string_lossy().into_owned();
    assert!(flags.ends_with("--cfg\x1ffuzzing"), "the check is not built with --cfg fuzzing: {flags:?}");
    assert_eq!(c.get_current_dir(), Some(Path::new("/r/crates/a/fuzz")), "cargo reads its configuration from the crate up");
}

// --- targets the manifest does not list ---

#[test]
fn a_file_in_fuzz_targets_that_no_bin_names_is_unlisted() {
    let files: Vec<String> = [
        "crates/a/fuzz/fuzz_targets/listed.rs",
        "crates/a/fuzz/fuzz_targets/forgotten.rs",
        "crates/a/fuzz/fuzz_targets/helpers/mod.rs",
        "crates/a/fuzz/fuzz_targets/notes.md",
        "crates/a/fuzz/src/other.rs",
        "crates/b/fuzz/fuzz_targets/b_forgotten.rs",
    ]
    .map(String::from)
    .to_vec();
    let listed = vec![PathBuf::from("/r/crates/a/fuzz/fuzz_targets/listed.rs")];
    let found = unlisted_targets(Path::new("/r"), "crates/a/fuzz", &files, &listed);
    assert_eq!(found, ["crates/a/fuzz/fuzz_targets/forgotten.rs"], "this crate's files only, `.rs` only, not a subfolder's");
}

#[test]
fn the_unlisted_files_are_given_in_sorted_order() {
    let files: Vec<String> =
        ["crates/a/fuzz/fuzz_targets/z.rs", "crates/a/fuzz/fuzz_targets/m.rs", "crates/a/fuzz/fuzz_targets/b.rs"].map(String::from).to_vec();
    let found = unlisted_targets(Path::new("/r"), "crates/a/fuzz", &files, &[]);
    assert_eq!(found, ["crates/a/fuzz/fuzz_targets/b.rs", "crates/a/fuzz/fuzz_targets/m.rs", "crates/a/fuzz/fuzz_targets/z.rs"]);
}

// --- the whole check, on a small repository ---

/// A repository with one fuzz crate (`crates/a/fuzz`) of the given targets. Its `libfuzzer-sys` is a stand-in whose build script
/// fails unless `CUSTOM_LIBFUZZER_PATH` names a file that exists, as the real one fails to find a C++ compiler it was told to skip.
fn repo_with_fuzz_crate(tag: &str, targets: &[(&str, &str)]) -> TempDir {
    let dir = git_repo(tag);
    dir.write("Cargo.toml", "[workspace]\nmembers = []\n");
    let mut manifest = String::from(
        "[package]\nname = \"a-fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[package.metadata]\ncargo-fuzz = true\n\n\
         [dependencies]\nlibfuzzer-sys = { path = \"stand-in/libfuzzer-sys\" }\n\n[workspace]\n",
    );
    for (name, source) in targets {
        manifest.push_str(&format!("\n[[bin]]\nname = \"{name}\"\npath = \"fuzz_targets/{name}.rs\"\ntest = false\ndoc = false\nbench = false\n"));
        dir.write(&format!("crates/a/fuzz/fuzz_targets/{name}.rs"), source);
    }
    dir.write("crates/a/fuzz/Cargo.toml", &manifest);
    dir.write("crates/a/fuzz/.gitignore", "target\nCargo.lock\n");
    dir.write(
        "crates/a/fuzz/stand-in/libfuzzer-sys/Cargo.toml",
        "[package]\nname = \"libfuzzer-sys\"\nversion = \"0.4.99\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    );
    dir.write(
        "crates/a/fuzz/stand-in/libfuzzer-sys/build.rs",
        "fn main() {\n    let path = std::env::var(\"CUSTOM_LIBFUZZER_PATH\").expect(\"the check did not set CUSTOM_LIBFUZZER_PATH\");\n    \
         assert!(std::path::Path::new(&path).is_file(), \"{path} is not a file\");\n    println!(\"cargo:rerun-if-changed={path}\");\n}\n",
    );
    dir.write(
        "crates/a/fuzz/stand-in/libfuzzer-sys/src/lib.rs",
        "#[macro_export]\nmacro_rules! fuzz_target {\n    (|$data:ident: &[u8]| $body:block) => {\n        pub fn fuzz_one($data: &[u8]) $body\n    };\n}\n",
    );
    dir
}

const GOOD: &str = "#![no_main]\nlibfuzzer_sys::fuzz_target!(|data: &[u8]| {\n    let _ = data.len();\n});\n";
/// The break that started this: a raw string closed early by `\"#`, as in the target.
const BROKEN: &str =
    "#![no_main]\nlibfuzzer_sys::fuzz_target!(|data: &[u8]| {\n    let s = r#\"{\"fill\":\"#ff0000\"}\"#;\n    let _ = (data, s);\n});\n";

fn run_check(dir: &TempDir, members: &HashSet<PathBuf>) -> Result<Outcome, String> {
    let target = dir.path().join("target-of-check");
    check(dir.path(), &target, members)
}

#[test]
fn a_repository_whose_fuzz_targets_compile_passes() {
    let dir = repo_with_fuzz_crate("fuzz-ok", &[("one", GOOD), ("two", GOOD)]);
    assert_eq!(run_check(&dir, &HashSet::new()), Ok(Outcome::Ok));
    assert!(dir.path().join("crates/a/fuzz/Cargo.lock").is_file(), "the crate's lock is made on the first run, in a place git ignores");
    assert!(dir.path().join("target-of-check/libfuzzer-stub.a").is_file(), "the stub is a file the build script can find");
}

#[test]
fn a_passing_check_counts_the_crates_and_every_target_in_them() {
    let dir = repo_with_fuzz_crate("fuzz-count", &[("one", GOOD), ("two", GOOD), ("three", GOOD)]);
    let checked = check_crates(dir.path(), &dir.path().join("target-of-check"), &HashSet::new()).unwrap();
    assert_eq!(checked, Checked { crates: vec!["crates/a/fuzz".to_string()], targets: 3 });
}

/// The lock a first run starts from is the workspace's: cargo keeps the format version of the lock it is given (a new lock gets the
/// newest), so a crate whose lock is still version 3 afterwards was started from the workspace's.
#[test]
fn the_first_run_of_a_crate_starts_from_the_workspaces_lock() {
    let dir = repo_with_fuzz_crate("fuzz-seeded", &[("one", GOOD)]);
    dir.write(
        "Cargo.lock",
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 3\n\n[[package]]\n\
         name = \"a-fuzz\"\nversion = \"0.0.0\"\ndependencies = [\n \"libfuzzer-sys\",\n]\n\n[[package]]\nname = \"libfuzzer-sys\"\nversion = \"0.4.99\"\n",
    );
    assert_eq!(run_check(&dir, &HashSet::new()), Ok(Outcome::Ok));
    let lock = std::fs::read_to_string(dir.path().join("crates/a/fuzz/Cargo.lock")).unwrap();
    assert!(lock.contains("version = 3"), "the crate's lock was not started from the workspace's: {lock}");
}

#[test]
fn a_fuzz_target_that_does_not_compile_fails_the_check_and_is_named_with_its_crate() {
    let dir = repo_with_fuzz_crate("fuzz-broken", &[("good", GOOD), ("canon_compare", BROKEN)]);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz"), "the crate is not named: {err}");
    assert!(err.contains("`a-fuzz` (bin \"canon_compare\")"), "the target is not named: {err}");
    assert!(!err.contains("\"good\""), "a target that compiles is named as broken: {err}");
}

#[test]
fn every_broken_target_is_named_not_only_the_first() {
    let dir = repo_with_fuzz_crate("fuzz-two-broken", &[("first", BROKEN), ("second", BROKEN), ("third", GOOD)]);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("bin \"first\"") && err.contains("bin \"second\""), "{err}");
}

#[test]
fn a_fuzz_target_file_the_manifest_does_not_list_fails_the_check_by_name() {
    let dir = repo_with_fuzz_crate("fuzz-unlisted", &[("one", GOOD)]);
    dir.write("crates/a/fuzz/fuzz_targets/forgotten.rs", GOOD);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz/fuzz_targets/forgotten.rs"), "{err}");
    assert!(err.contains("[[bin]]"), "the error does not say what to do: {err}");
}

#[test]
fn a_fuzz_crate_with_no_targets_fails_the_check_and_does_not_pass_for_nothing() {
    // A manifest that is only a workspace, with no member: cargo reads it and has nothing to check.
    let dir = repo_with_fuzz_crate("fuzz-empty", &[]);
    dir.write("crates/a/fuzz/Cargo.toml", "[workspace]\nmembers = [\"stand-in/libfuzzer-sys\"]\n");
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz") && err.contains("lists no fuzz target"), "{err}");
    // A manifest that is only a workspace has no package: it is still a fuzz crate that was found, not a crate no step builds.
    assert!(err.contains("(1 problems)") && !err.contains("no gate step builds it"), "{err}");
}

#[test]
fn a_package_with_no_target_at_all_fails_with_the_crate_named_and_cargos_reason() {
    let dir = repo_with_fuzz_crate("fuzz-no-bin", &[]);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz") && err.contains("no targets specified"), "{err}");
}

#[test]
fn a_manifest_cargo_cannot_read_fails_the_check_with_the_crate_named() {
    let dir = repo_with_fuzz_crate("fuzz-bad-manifest", &[("one", GOOD)]);
    dir.write("crates/a/fuzz/Cargo.toml", "[package\nname = \n");
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz"), "{err}");
    // The crate is counted as one that is accounted for: its manifest is not also reported as a crate no step builds.
    assert!(err.contains("(1 problems)") && !err.contains("no gate step builds it"), "{err}");
}

#[test]
fn a_check_that_fails_without_naming_a_target_still_fails_and_says_so() {
    // A dependency's build script fails: cargo stops before it compiles a target, and prints no `could not compile` for one.
    let dir = repo_with_fuzz_crate("fuzz-build-script", &[("one", GOOD)]);
    dir.write("crates/a/fuzz/stand-in/libfuzzer-sys/build.rs", "fn main() {\n    std::process::exit(3);\n}\n");
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz") && err.contains("named no target"), "{err}");
}

#[test]
fn a_repository_with_no_fuzz_crate_skips_with_the_reason() {
    let dir = git_repo("fuzz-none");
    dir.write("Cargo.toml", "[workspace]\nmembers = []\n");
    assert_eq!(run_check(&dir, &HashSet::new()), Ok(Outcome::Skipped("no fuzz crate in the repository".into())));
    assert!(!dir.path().join("target-of-check").exists(), "a repository with nothing to check makes no build folder");
}

#[test]
fn a_fuzz_crate_that_is_a_member_of_the_workspace_is_left_to_the_workspace_steps() {
    let dir = repo_with_fuzz_crate("fuzz-member", &[("broken", BROKEN)]);
    // As cargo metadata of the workspace lists it: the crate, and the package inside its folder that it depends on.
    let members = HashSet::from([dir.path().join("crates/a/fuzz/Cargo.toml"), dir.path().join("crates/a/fuzz/stand-in/libfuzzer-sys/Cargo.toml")]);
    // The workspace's own `cargo clippy --workspace` builds it; checking it again here would seed a lock into the workspace.
    assert_eq!(run_check(&dir, &members), Ok(Outcome::Skipped("no fuzz crate in the repository".into())));
    assert!(!dir.path().join("crates/a/fuzz/Cargo.lock").exists());
}

// --- this repository ---

/// The three fuzz crates the repository has now are found, and every file in their `fuzz_targets` folders is a `[[bin]]` of the manifest
/// (the gate's check stops at a file that is not). This reads cargo's metadata only; it builds nothing.
#[test]
fn this_repositorys_fuzz_crates_are_found_and_list_all_their_target_files() {
    let root = xtask::root();
    let files = xtask::repo_files(&root).unwrap();
    let crates = find_crates(&files, |p| std::fs::read_to_string(root.join(p)).ok());
    for expected in ["crates/container/fuzz", "crates/odg/fuzz", "crates/sheetxml/fuzz"] {
        assert!(crates.iter().any(|c| c == expected), "{expected} is not found among {crates:?}");
    }
    for dir in &crates {
        let listed = fuzzcheck::bin_sources(&root.join(dir)).unwrap();
        assert!(!listed.is_empty(), "{dir} lists no target");
        assert_eq!(unlisted_targets(&root, dir, &files, &listed), Vec::<String>::new(), "{dir}");
    }
}

#[test]
fn the_ci_step_is_listed_in_the_usage_and_runs_after_clippy() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtask")).arg("help").output().unwrap();
    let usage = String::from_utf8(out.stdout).unwrap();
    assert!(usage.contains("fuzz-check"), "the usage does not list the command: {usage}");
    let ci_entry: String = usage.lines().skip_while(|l| !l.trim_start().starts_with("ci ")).take(3).collect::<Vec<_>>().join(" ");
    assert!(ci_entry.contains("clippy -D warnings, fuzz check"), "ci does not say it runs the fuzz check after clippy: {ci_entry}");
}

// --- the two dependencies the workspace's lock does not hold ---

/// The two crates a fuzz crate gets that the workspace's lock has no entry for: a release of either could turn the gate red with no change
/// in the repository, so every fuzz manifest pins them to one exact version.
const PINNED: [&str; 2] = ["libfuzzer-sys", "arbitrary"];

/// The version requirement of dependency `name` in a manifest's `[dependencies]` (a string, or the `version` of a table).
fn requirement(manifest: &toml::Table, name: &str) -> Option<String> {
    let dep = manifest.get("dependencies")?.get(name)?;
    dep.as_str().or_else(|| dep.get("version")?.as_str()).map(str::to_owned)
}

/// What is wrong with the pins of one manifest: a dependency that is missing or not `=x.y.z`.
fn pin_problems(manifest: &str) -> Vec<String> {
    let table: toml::Table = manifest.parse().unwrap();
    let mut problems = Vec::new();
    for name in PINNED {
        match requirement(&table, name) {
            None => problems.push(format!("{name} is not a dependency")),
            Some(req) => {
                let exact = req.strip_prefix('=').is_some_and(|v| v.split('.').count() == 3 && v.split('.').all(|n| n.parse::<u32>().is_ok()));
                if !exact {
                    problems.push(format!("{name} is `{req}`, not an exact `=x.y.z`"));
                }
            }
        }
    }
    problems
}

#[test]
fn the_pin_check_reads_a_string_or_a_table_and_refuses_a_range() {
    let pinned = "[dependencies]\nlibfuzzer-sys = \"=0.4.13\"\narbitrary = { version = \"=1.5.0\" }\n";
    assert_eq!(pin_problems(pinned), Vec::<String>::new());
    let ranged = "[dependencies]\nlibfuzzer-sys = \"0.4\"\narbitrary = \"=1.5\"\n";
    assert_eq!(
        pin_problems(ranged),
        ["libfuzzer-sys is `0.4`, not an exact `=x.y.z`", "arbitrary is `=1.5`, not an exact `=x.y.z`"],
        "a range, or a pin that leaves the patch number out, is no pin"
    );
    assert_eq!(pin_problems("[dependencies]\narbitrary = \"=1.5.0\"\n"), ["libfuzzer-sys is not a dependency"]);
}

/// A fuzz crate made by `cargo fuzz init` says `libfuzzer-sys = "0.4"` and has no `arbitrary` line: it fails here until it is pinned to the
/// versions the others hold. To move the pins, change every fuzz manifest in one commit.
#[test]
fn every_fuzz_crate_pins_libfuzzer_sys_and_arbitrary_to_the_same_exact_versions() {
    let root = xtask::root();
    let files = xtask::repo_files(&root).unwrap();
    let crates = find_crates(&files, |p| std::fs::read_to_string(root.join(p)).ok());
    assert!(crates.len() >= 3, "{crates:?}");
    let mut seen: Vec<(String, String, String)> = Vec::new();
    for dir in &crates {
        let text = std::fs::read_to_string(root.join(dir).join("Cargo.toml")).unwrap();
        assert_eq!(
            pin_problems(&text),
            Vec::<String>::new(),
            "{dir}/Cargo.toml: pin libfuzzer-sys and arbitrary to an exact version (the same in every fuzz crate)"
        );
        let table: toml::Table = text.parse().unwrap();
        let versions: Vec<String> = PINNED.iter().map(|n| requirement(&table, n).unwrap()).collect();
        seen.push((dir.clone(), versions[0].clone(), versions[1].clone()));
    }
    let first = &seen[0];
    for other in &seen[1..] {
        assert_eq!((&other.1, &other.2), (&first.1, &first.2), "{} pins other versions than {}", other.0, first.0);
    }
}

// ---: what the first version let through (each of these failed on f0a331d) ---

/// A second crate in the fixture repository: `folder` holds a package `name` that is its own workspace, with the given dependencies
/// (a line each) and bins (each `fuzz_targets/<name>.rs` of the given source), and the marker line if asked.
fn add_crate(dir: &TempDir, folder: &str, name: &str, marker: bool, dependencies: &str, bins: &[(&str, &str)]) {
    let mut manifest = format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n");
    if marker {
        manifest.push_str("[package.metadata]\ncargo-fuzz = true\n\n");
    }
    manifest.push_str(&format!("[dependencies]\n{dependencies}\n\n[workspace]\n"));
    for (bin, source) in bins {
        manifest.push_str(&format!("\n[[bin]]\nname = \"{bin}\"\npath = \"fuzz_targets/{bin}.rs\"\ntest = false\ndoc = false\nbench = false\n"));
        dir.write(&format!("{folder}/fuzz_targets/{bin}.rs"), source);
    }
    dir.write(&format!("{folder}/Cargo.toml"), &manifest);
    dir.write(&format!("{folder}/.gitignore"), "target\nCargo.lock\n");
}

const STAND_IN_FROM_B: &str = "libfuzzer-sys = { path = \"../../a/fuzz/stand-in/libfuzzer-sys\" }";

/// A dependency that cannot be had, with the network down (a proxy nothing listens on), is a problem of the network or of the
/// lock, and the step says so; it was reported as "fuzz targets that do not compile".
#[test]
fn a_failure_to_resolve_is_worded_as_a_network_or_lock_problem_and_never_as_a_compile_error() {
    let dir = repo_with_fuzz_crate("fuzz-no-net", &[("one", GOOD)]);
    dir.write(".cargo/config.toml", "[http]\nproxy = \"http://127.0.0.1:9\"\n\n[net]\nretry = 0\n");
    let manifest = std::fs::read_to_string(dir.path().join("crates/a/fuzz/Cargo.toml")).unwrap();
    dir.write("crates/a/fuzz/Cargo.toml", &manifest.replace("[dependencies]\n", "[dependencies]\nfuzzcheck-test-no-such-crate = \"=1.0.0\"\n"));
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/a/fuzz") && err.contains("network or lock problem"), "{err}");
    assert!(!err.contains("does not compile") && !err.contains("do not compile"), "a resolve failure is worded as a compile error: {err}");
}

/// A fuzz crate in a folder the step does not know by name and with no marker is found by what it depends on.
#[test]
fn a_fuzz_crate_in_any_folder_is_found_by_its_dependency_on_libfuzzer_sys() {
    let dir = repo_with_fuzz_crate("fuzz-by-dependency", &[("one", GOOD)]);
    add_crate(&dir, "crates/b/fuzzers", "b-fuzzers", false, STAND_IN_FROM_B, &[("two", BROKEN)]);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/b/fuzzers") && err.contains("`b-fuzzers` (bin \"two\")"), "{err}");
}

/// A manifest that is not the root's, not a workspace member and not a fuzz crate is a crate the gate never builds.
#[test]
fn a_crate_outside_the_workspace_that_no_step_builds_fails_the_check_by_name() {
    let dir = repo_with_fuzz_crate("fuzz-unscanned", &[("one", GOOD)]);
    add_crate(&dir, "crates/b/tool", "b-tool", false, "", &[]);
    dir.write("crates/b/tool/src/lib.rs", "pub fn f() {}\n");
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("crates/b/tool/Cargo.toml") && err.contains("no gate step builds it"), "{err}");
}

/// The fuzz crate's lock is the workspace's as of the last run, not the workspace's as of the first one.
#[test]
fn the_fuzz_locks_follow_the_workspace_lock_when_it_changes() {
    let dir = repo_with_fuzz_crate("fuzz-lock-follows", &[("one", GOOD)]);
    let lock = |version: u32| {
        format!(
            "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = {version}\n\n[[package]]\n\
             name = \"a-fuzz\"\nversion = \"0.0.0\"\ndependencies = [\n \"libfuzzer-sys\",\n]\n\n[[package]]\nname = \"libfuzzer-sys\"\nversion = \"0.4.99\"\n"
        )
    };
    let crate_lock = || std::fs::read_to_string(dir.path().join("crates/a/fuzz/Cargo.lock")).unwrap();
    dir.write("Cargo.lock", &lock(3));
    assert_eq!(run_check(&dir, &HashSet::new()), Ok(Outcome::Ok));
    assert!(crate_lock().contains("version = 3"), "{}", crate_lock());
    dir.write("Cargo.lock", &lock(4));
    assert_eq!(run_check(&dir, &HashSet::new()), Ok(Outcome::Ok));
    assert!(crate_lock().contains("version = 4"), "the lock did not follow the workspace's: {}", crate_lock());
}

/// A bin that needs a feature is skipped by a plain `cargo check`, and was counted as checked.
#[test]
fn a_bin_that_needs_a_feature_is_checked_with_every_feature_on() {
    let dir = repo_with_fuzz_crate("fuzz-required-features", &[("one", GOOD), ("gated", BROKEN)]);
    let manifest = std::fs::read_to_string(dir.path().join("crates/a/fuzz/Cargo.toml")).unwrap();
    let gated = manifest.replace("path = \"fuzz_targets/gated.rs\"\n", "path = \"fuzz_targets/gated.rs\"\nrequired-features = [\"extra\"]\n");
    dir.write("crates/a/fuzz/Cargo.toml", &format!("{gated}\n[features]\nextra = []\n"));
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("`a-fuzz` (bin \"gated\")"), "a bin behind a feature was skipped: {err}");
}

/// LibFuzzer builds are made with `--cfg fuzzing`, and a target can have code under it.
#[test]
fn code_under_cfg_fuzzing_is_type_checked() {
    let under_cfg = format!("{GOOD}\n#[cfg(fuzzing)]\nfn planted() -> u32 {{\n    \"not a number\"\n}}\n");
    let dir = repo_with_fuzz_crate("fuzz-cfg", &[("one", &under_cfg)]);
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.contains("`a-fuzz` (bin \"one\")"), "code under cfg(fuzzing) was not checked: {err}");
}

// ---: the pieces ---

#[test]
fn a_manifest_that_depends_on_libfuzzer_sys_is_a_fuzz_manifest_in_any_folder_and_in_any_spelling_of_the_dependency() {
    for text in [
        "[dependencies]\nlibfuzzer-sys = \"=0.4.13\"\n",
        "[dependencies]\nlibfuzzer-sys.workspace = true\n",
        "[dependencies]\nlibfuzzer-sys={ version = \"0.4\", default-features = false }\n",
        "[dependencies.libfuzzer-sys]\nversion = \"0.4\"\n",
        "[dev-dependencies]\n  libfuzzer_sys = \"0.4\"\n",
        "[target.'cfg(unix)'.dependencies.libfuzzer-sys]\nversion = \"0.4\"\n",
    ] {
        assert!(is_fuzz_manifest("crates/units/fuzzers/Cargo.toml", text), "{text}");
    }
}

#[test]
fn a_manifest_that_only_mentions_libfuzzer_sys_is_not_a_fuzz_manifest() {
    for text in [
        "[package]\nname = \"libfuzzer-sys\"\n",
        "# libfuzzer-sys = \"0.4\"\n",
        "[dependencies]\nlibfuzzer-sys-extra = \"1\"\n",
        "[dependencies]\nmy-libfuzzer-sys = \"1\"\n",
        "[dependencies.libfuzzer-sys-extra]\nversion = \"1\"\n",
        "[package]\ndescription = \"wraps libfuzzer-sys = 0.4\"\n",
    ] {
        assert!(!is_fuzz_manifest("crates/units/fuzzers/Cargo.toml", text), "{text}");
    }
}

#[test]
fn a_crate_found_by_its_dependency_is_listed_with_the_ones_found_by_name() {
    let files: Vec<String> = ["crates/a/fuzz/Cargo.toml", "crates/b/fuzzers/Cargo.toml", "crates/c/Cargo.toml"].map(String::from).to_vec();
    let found = find_crates(&files, |p| (p == "crates/b/fuzzers/Cargo.toml").then(|| "[dependencies]\nlibfuzzer-sys = \"=0.4.13\"\n".to_string()));
    assert_eq!(found, ["crates/a/fuzz", "crates/b/fuzzers"]);
}

#[test]
fn the_flags_of_the_check_are_what_the_environment_gave_and_then_cfg_fuzzing() {
    let sep = "\x1f";
    assert_eq!(encoded_rustflags(None, None), format!("--cfg{sep}fuzzing"));
    assert_eq!(
        encoded_rustflags(None, Some("-C target-cpu=native  -D warnings")),
        format!("-C{sep}target-cpu=native{sep}-D{sep}warnings{sep}--cfg{sep}fuzzing")
    );
    assert_eq!(
        encoded_rustflags(Some(&format!("-C{sep}link-arg=-fuse-ld=lld")), None),
        format!("-C{sep}link-arg=-fuse-ld=lld{sep}--cfg{sep}fuzzing")
    );
    // Cargo reads CARGO_ENCODED_RUSTFLAGS in place of RUSTFLAGS, so when both are set the encoded one is the base.
    assert_eq!(encoded_rustflags(Some("-Dwarnings"), Some("-C opt-level=3")), format!("-Dwarnings{sep}--cfg{sep}fuzzing"));
    assert_eq!(encoded_rustflags(Some(""), None), format!("--cfg{sep}fuzzing"), "an empty value adds no empty flag");
}

#[test]
fn the_resolve_command_completes_the_lock_of_the_crates_own_manifest_and_offline_when_asked() {
    let offline = resolve_command(Path::new("/r/crates/a/fuzz"), true);
    let args = args_of(&offline);
    assert_eq!(&args[..2], ["update", "--workspace"], "{args:?}");
    assert!(args.iter().any(|a| a == "--offline"), "{args:?}");
    assert!(args.windows(2).any(|w| w[0] == "--manifest-path" && w[1] == "/r/crates/a/fuzz/Cargo.toml"), "{args:?}");
    assert_eq!(offline.get_current_dir(), Some(Path::new("/r/crates/a/fuzz")));
    let online = args_of(&resolve_command(Path::new("/r/crates/a/fuzz"), false));
    assert!(!online.iter().any(|a| a == "--offline"), "{online:?}");
    for refused in ["--locked", "--frozen", "--precise", "--recursive"] {
        assert!(!args.iter().any(|a| a == refused) && !online.iter().any(|a| a == refused), "{refused}");
    }
}

#[test]
fn resolving_tries_offline_first_and_goes_online_only_when_that_fails() {
    let mut tries = Vec::new();
    assert_eq!(
        resolve(|offline| {
            tries.push(offline);
            Ok(())
        }),
        Ok(())
    );
    assert_eq!(tries, [true], "an offline resolve that works needs no network");
    let mut tries = Vec::new();
    assert_eq!(
        resolve(|offline| {
            tries.push(offline);
            if offline { Err("not in the cache".into()) } else { Ok(()) }
        }),
        Ok(())
    );
    assert_eq!(tries, [true, false]);
}

#[test]
fn a_resolve_that_fails_both_ways_says_network_or_lock_and_keeps_both_causes() {
    let err =
        resolve(|offline| Err(if offline { "no matching package in the cache".to_string() } else { "could not connect".to_string() })).unwrap_err();
    assert!(err.contains("network or lock problem"), "{err}");
    assert!(err.contains("Offline: no matching package in the cache") && err.contains("Online: could not connect"), "{err}");
    assert!(!err.contains("compile"), "a resolve failure is worded as a compile error: {err}");
}

/// The compiler's errors reach the writer the output is given (the gate's own output), and the targets are named from them.
#[test]
fn the_compilers_errors_are_copied_to_the_writer_as_cargo_prints_them() {
    let dir = repo_with_fuzz_crate("fuzz-copy", &[("one", BROKEN)]);
    let stub = dir.path().join("stub.a");
    dir.write("stub.a", "");
    let mut copied = Vec::new();
    let crate_dir = dir.path().join("crates/a/fuzz");
    let (ok, failed) = run_reading_errors(command(&crate_dir, &dir.path().join("target-of-check"), &stub), &mut copied).unwrap();
    let copied = String::from_utf8(copied).unwrap();
    assert!(!ok);
    assert_eq!(failed, ["`a-fuzz` (bin \"one\")"]);
    assert!(copied.contains("error: could not compile `a-fuzz` (bin \"one\")") && copied.contains("fuzz_targets/one.rs"), "{copied}");
}

#[test]
fn a_writer_that_fails_does_not_change_the_verdict() {
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = repo_with_fuzz_crate("fuzz-closed", &[("one", BROKEN)]);
    dir.write("stub.a", "");
    let crate_dir = dir.path().join("crates/a/fuzz");
    let (ok, failed) = run_reading_errors(command(&crate_dir, &dir.path().join("target-of-check"), &dir.path().join("stub.a")), &mut Closed).unwrap();
    assert!(!ok);
    assert_eq!(failed, ["`a-fuzz` (bin \"one\")"], "the targets are named from what cargo printed, copied or not");
}

/// Where the step builds, and what it takes for the workspace's members, come from cargo's metadata.
#[test]
fn the_step_builds_in_the_fuzz_check_folder_of_the_target_directory_cargo_names() {
    let meta = serde_json::json!({ "target_directory": "/somewhere/else/target", "packages": [] });
    assert_eq!(check_folder(&meta, Path::new("/repo")), Path::new("/somewhere/else/target/fuzz-check"));
    assert_eq!(check_folder(&serde_json::json!({}), Path::new("/repo")), Path::new("/repo/target/fuzz-check"), "no name: the root's target folder");
}

#[test]
fn the_members_are_the_manifests_of_the_packages_cargo_lists() {
    let meta = serde_json::json!({ "packages": [
        { "name": "a", "manifest_path": "/repo/crates/a/Cargo.toml" },
        { "name": "b", "manifest_path": "/repo/xtask/Cargo.toml" },
        { "name": "no-manifest" },
    ] });
    assert_eq!(members_of(&meta), HashSet::from([PathBuf::from("/repo/crates/a/Cargo.toml"), PathBuf::from("/repo/xtask/Cargo.toml")]));
    assert!(members_of(&serde_json::json!({})).is_empty());
}

/// And the pieces around it: the count in the error is the number of problems, each of the crates and each unscanned manifest.
#[test]
fn the_error_counts_the_problems_of_every_kind() {
    let dir = repo_with_fuzz_crate("fuzz-two-problems", &[("one", BROKEN)]);
    add_crate(&dir, "crates/b/tool", "b-tool", false, "", &[]);
    dir.write("crates/b/tool/src/lib.rs", "pub fn f() {}\n");
    let err = run_check(&dir, &HashSet::new()).unwrap_err();
    assert!(err.starts_with("fuzz check failed (2 problems): "), "{err}");
    assert!(err.contains("`a-fuzz` (bin \"one\")") && err.contains("crates/b/tool/Cargo.toml"), "{err}");
}

#[test]
fn the_tail_of_cargos_errors_is_its_last_six_lines_without_blanks_on_one_line() {
    let text = "one\n\n  two  \nthree\nfour\n\nfive\nsix\nseven\n";
    assert_eq!(tail_of(text.as_bytes()), "two | three | four | five | six | seven");
    assert_eq!(tail_of(b"only\n"), "only");
    assert_eq!(tail_of(b""), "");
    let long = "x".repeat(5000);
    assert_eq!(tail_of(long.as_bytes()).chars().count(), 900, "a line of any length is cut");
}

/// `run` takes the members and the build folder from cargo's metadata of the workspace. A member that is not a fuzz crate is not "a
/// crate that no step builds" because the metadata lists it, and the fuzz crate builds in the folder the metadata names.
#[test]
fn run_with_takes_the_members_and_the_folder_from_the_metadata() {
    let dir = repo_with_fuzz_crate("fuzz-run-with", &[("one", GOOD)]);
    add_crate(&dir, "crates/m/member", "m-member", false, "", &[]);
    dir.write("crates/m/member/src/lib.rs", "pub fn f() {}\n");
    let manifest = |rel: &str| dir.path().join(rel).to_string_lossy().into_owned();
    let target = dir.path().join("elsewhere/target");
    let meta = serde_json::json!({
        "target_directory": target.to_string_lossy(),
        "packages": [{ "name": "m-member", "manifest_path": manifest("crates/m/member/Cargo.toml") }],
    });
    assert_eq!(run_with(dir.path(), &meta), Ok(Outcome::Ok));
    assert!(target.join("fuzz-check").join(fuzzcheck::STUB).is_file(), "the crate was not built in the folder the metadata names");
    let without = serde_json::json!({ "target_directory": target.to_string_lossy(), "packages": [] });
    let err = run_with(dir.path(), &without).unwrap_err();
    assert!(err.contains("crates/m/member/Cargo.toml"), "a member the metadata does not list is a crate no step builds: {err}");
}

// --- a root reached through a link ---
//
// The step compares paths that name the same file by different routes: the members come from cargo's metadata of the workspace, the
// root is what the caller holds, and a folder is a link when a contributor works from one. Both are compared by their real paths.

/// The repository's folder by its real path, a link to it that lives in a folder of its own (the holder, kept alive by the caller, so
/// the link is not a file of the repository), and the link.
#[cfg(unix)]
fn real_and_linked(dir: &TempDir) -> (PathBuf, PathBuf, TempDir) {
    let real = std::fs::canonicalize(dir.path()).unwrap();
    let holder = TempDir::new("fuzz-link-holder");
    let linked = holder.path().join("root");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    (real, linked, holder)
}

/// H12 and H8: the members are named through a link to the root, the caller's root is the real folder. A workspace member is left to the
/// workspace's steps whatever route names its manifest; the broken target would fail the check if the crate were checked.
#[cfg(unix)]
#[test]
fn a_member_named_through_a_link_to_the_root_is_still_a_member() {
    let dir = repo_with_fuzz_crate("fuzz-member-via-link", &[("broken", BROKEN)]);
    let (real, linked, _holder) = real_and_linked(&dir);
    let members = HashSet::from([linked.join("crates/a/fuzz/Cargo.toml"), linked.join("crates/a/fuzz/stand-in/libfuzzer-sys/Cargo.toml")]);
    assert_eq!(check(&real, &dir.path().join("target-of-check"), &members), Ok(Outcome::Skipped("no fuzz crate in the repository".into())));
    assert!(!dir.path().join("crates/a/fuzz/Cargo.lock").exists(), "a member must not get a lock seeded");
}

/// H8: the root is reached through a link and the members are named by the real path. Both the fuzz crate that is a member and a member that
/// is no fuzz crate are found by their real paths: the first is not checked, the second is not "a crate that no step builds".
#[cfg(unix)]
#[test]
fn a_root_reached_through_a_link_finds_the_members_cargo_names_by_their_real_paths() {
    let dir = repo_with_fuzz_crate("fuzz-root-via-link", &[("broken", BROKEN)]);
    add_crate(&dir, "crates/b/tool", "b-tool", false, "", &[]);
    dir.write("crates/b/tool/src/lib.rs", "pub fn f() {}\n");
    let (real, linked, _holder) = real_and_linked(&dir);
    let members = HashSet::from([
        real.join("crates/a/fuzz/Cargo.toml"),
        real.join("crates/a/fuzz/stand-in/libfuzzer-sys/Cargo.toml"),
        real.join("crates/b/tool/Cargo.toml"),
    ]);
    assert_eq!(check(&linked, &dir.path().join("target-of-check"), &members), Ok(Outcome::Skipped("no fuzz crate in the repository".into())));
    assert!(!dir.path().join("crates/a/fuzz/Cargo.lock").exists(), "a member must not get a lock seeded");
}

/// H8: through a link to the root, with no member listed, a crate that compiles passes. Cargo names the packages of the fuzz crate's own
/// workspace by the route it was given; the files the repository holds are named by the root, and a package is found by its real path.
#[cfg(unix)]
#[test]
fn a_repository_reached_through_a_link_passes_when_its_fuzz_targets_compile_and_fails_when_they_do_not() {
    let dir = repo_with_fuzz_crate("fuzz-via-link-ok", &[("one", GOOD)]);
    let (_real, linked, _holder) = real_and_linked(&dir);
    let target = dir.path().join("target-of-check");
    assert_eq!(check(&linked, &target, &HashSet::new()), Ok(Outcome::Ok));
    assert!(dir.path().join("crates/a/fuzz/Cargo.lock").is_file(), "the crate was checked through the link");
    dir.write("crates/a/fuzz/fuzz_targets/one.rs", BROKEN);
    let err = check(&linked, &target, &HashSet::new()).unwrap_err();
    assert!(err.contains("`a-fuzz` (bin \"one\")"), "{err}");
}

/// H8: a target file that the manifest lists is listed whichever route names it, in either direction.
#[cfg(unix)]
#[test]
fn a_listed_target_is_listed_whichever_route_names_it() {
    let dir = TempDir::new("fuzz-unlisted-via-link");
    dir.write("crates/a/fuzz/fuzz_targets/one.rs", GOOD);
    let (real, linked, _holder) = real_and_linked(&dir);
    let files = vec!["crates/a/fuzz/fuzz_targets/one.rs".to_string()];
    let by_real = vec![real.join("crates/a/fuzz/fuzz_targets/one.rs")];
    let by_link = vec![linked.join("crates/a/fuzz/fuzz_targets/one.rs")];
    for (root, listed, what) in [(&linked, &by_real, "root by link, target by real path"), (&real, &by_link, "root by real path, target by link")] {
        assert_eq!(unlisted_targets(root, "crates/a/fuzz", &files, listed), Vec::<String>::new(), "{what}");
    }
    assert_eq!(
        unlisted_targets(&linked, "crates/a/fuzz", &files, &[]),
        ["crates/a/fuzz/fuzz_targets/one.rs"],
        "and a file no bin lists is still unlisted"
    );
}

/// H4: a `Cargo.lock` that is a link to nothing is no lock. It is replaced (the link itself, not what it points at) by the workspace's,
/// even when the record says the crate was seeded from that lock; a link to a lock that is there is a lock and is left alone.
#[cfg(unix)]
#[test]
fn a_lock_that_is_a_link_to_nothing_is_no_lock_and_is_replaced() {
    let (dir, crate_dir, record) = lock_case("fuzz-lock-dangling", "# the workspace's lock\n");
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap());
    let own = crate_dir.join("Cargo.lock");
    let missing = dir.path().join("no-such-lock");
    std::fs::remove_file(&own).unwrap();
    std::os::unix::fs::symlink(&missing, &own).unwrap();
    assert!(seed_lock(dir.path(), &crate_dir, &record).unwrap(), "a dangling link was counted as the crate's lock");
    assert!(!std::fs::symlink_metadata(&own).unwrap().file_type().is_symlink(), "the link is replaced, not written through");
    assert_eq!(read(&own), "# the workspace's lock\n");
    assert!(!missing.exists(), "nothing was written to where the link pointed");
    // A link that leads to a lock is one: the record matches, so it is left as it is.
    let held = dir.path().join("held-lock");
    dir.write("held-lock", "# a lock kept elsewhere\n");
    std::fs::remove_file(&own).unwrap();
    std::os::unix::fs::symlink(&held, &own).unwrap();
    assert!(!seed_lock(dir.path(), &crate_dir, &record).unwrap(), "a link to a lock that exists is a lock");
    assert_eq!(read(&held), "# a lock kept elsewhere\n");
}
