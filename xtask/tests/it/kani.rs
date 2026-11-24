//! `cargo xtask kani` and its step of `ci`: the Kani proofs. The gate compiles every harness (`kani check`, skipped without Kani 0.68.0) and the
//! command proves them; neither needs Kani here, so the tests read what Kani prints from text and run the decisions on their own.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::PathBuf;

use xtask::ci::Outcome;
use xtask::kani::{
    DEFAULT_JOBS, HARNESS_FILE, HARNESS_TIMEOUT, Report, VERSION, check_with, codegen_command, find_crates, gate, installed_version, is_declared,
    judge, machine_unavailable, parse_jobs, prove_command, prove_with, read_report, setup_dir, unavailable, undeclared,
};

// --- Kani's version ---

#[test]
fn the_version_is_the_one_the_proofs_are_written_for() {
    assert_eq!(VERSION, "0.68.0", "the CI install line in docs/development.md says the same");
}

#[test]
fn the_installed_version_is_read_from_the_first_line_of_cargo_kani_version() {
    assert_eq!(installed_version("Kani Rust Verifier 0.68.0 (cargo plugin)\nCBMC 6.11.0\n"), Some("0.68.0"));
    assert_eq!(installed_version("CBMC 6.11.0\nKani Rust Verifier 0.69.1 (cargo plugin)\n"), Some("0.69.1"));
    assert_eq!(installed_version("error: no such command: `kani`\n"), None);
    assert_eq!(installed_version(""), None);
}

#[test]
fn kani_cannot_be_used_when_it_is_missing_or_not_the_pinned_version() {
    assert_eq!(unavailable(Some("Kani Rust Verifier 0.68.0 (cargo plugin)\n")), None);
    let missing = unavailable(None).unwrap();
    assert!(
        missing.contains("not installed") && missing.contains("kani-verifier --version 0.68.0") && missing.contains("cargo kani setup"),
        "{missing}"
    );
    let unreadable = unavailable(Some("something else\n")).unwrap();
    assert!(unreadable.contains("not installed"), "{unreadable}");
    let other = unavailable(Some("Kani Rust Verifier 0.67.0 (cargo plugin)\n")).unwrap();
    assert!(other.contains("0.67.0") && other.contains("0.68.0"), "{other}");
    assert!(unavailable(Some("Kani Rust Verifier 0.68.01 (cargo plugin)\n")).is_some(), "a version that merely starts with the pin is another");
}

// --- Kani's set-up, which the version probe would otherwise run ---

/// What `cargo kani --version` prints on a machine where Kani is set up.
const PRINTED: &str = "Kani Rust Verifier 0.68.0 (cargo plugin)\nCBMC 6.11.0\n";

#[test]
fn kanis_files_are_in_kani_home_when_it_is_set_and_else_in_the_dot_kani_of_the_home() {
    assert_eq!(setup_dir(Some("/opt/k"), Some("/home/u")), Some(PathBuf::from("/opt/k/kani-0.68.0")));
    assert_eq!(setup_dir(Some("/opt/k"), None), Some(PathBuf::from("/opt/k/kani-0.68.0")));
    assert_eq!(setup_dir(None, Some("/home/u")), Some(PathBuf::from("/home/u/.kani/kani-0.68.0")));
    assert_eq!(setup_dir(None, None), None, "no place to look");
    // Kani reads `KANI_HOME` as set when it is empty (`env::var`), so the folder is then relative to where it runs: the same here.
    assert_eq!(setup_dir(Some(""), Some("/home/u")), Some(PathBuf::from("kani-0.68.0")));
    assert!(setup_dir(None, Some("/home/u")).is_some_and(|d| d.ends_with(format!("kani-{VERSION}"))), "the folder is named for the pinned release");
}

/// `cargo kani --version` runs the first-time setup, a download of 485 MB, when the release's folder is missing: the gate must not ask.
#[test]
fn a_machine_where_kani_was_never_set_up_is_skipped_and_is_not_asked_its_version() {
    let home = crate::TempDir::new("kani-home");
    let kani_home = home.path().to_str().unwrap();
    let probed = RefCell::new(0);
    let probe = || {
        *probed.borrow_mut() += 1;
        Some(PRINTED.to_string())
    };
    let why = machine_unavailable(Some(kani_home), None, probe).unwrap();
    assert!(why.contains("not set up") && why.contains("kani-0.68.0") && why.contains("cargo kani setup"), "{why}");
    assert!(why.contains(kani_home), "the reason names the folder it looked in: {why}");
    // The same with the default place, an empty home.
    let why = machine_unavailable(None, Some(kani_home), probe).unwrap();
    assert!(why.contains("not set up") && why.contains(".kani"), "{why}");
    // And with nowhere to look.
    let why = machine_unavailable(None, None, probe).unwrap();
    assert!(why.contains("KANI_HOME") && why.contains("HOME"), "{why}");
    assert_eq!(*probed.borrow(), 0, "the probe ran");
}

#[test]
fn a_machine_that_was_set_up_is_asked_its_version() {
    let home = crate::TempDir::new("kani-home-set-up");
    home.write("kani-0.68.0/rust-toolchain-version", "nightly\n");
    let kani_home = home.path().to_str().unwrap();
    assert_eq!(machine_unavailable(Some(kani_home), None, || Some(PRINTED.to_string())), None);
    assert!(machine_unavailable(Some(kani_home), None, || None).unwrap().contains("not installed"));
    let other = machine_unavailable(Some(kani_home), None, || Some("Kani Rust Verifier 0.67.0 (cargo plugin)\n".to_string())).unwrap();
    assert!(other.contains("0.67.0") && other.contains("0.68.0"), "{other}");
    // The default place: `.kani` in the home.
    let user = crate::TempDir::new("kani-user-home");
    user.write(".kani/kani-0.68.0/rust-toolchain-version", "nightly\n");
    assert_eq!(machine_unavailable(None, Some(user.path().to_str().unwrap()), || Some(PRINTED.to_string())), None);
    // A folder for another release is not this one's.
    let old = crate::TempDir::new("kani-old-release");
    old.write("kani-0.67.0/rust-toolchain-version", "nightly\n");
    assert!(machine_unavailable(Some(old.path().to_str().unwrap()), None, || panic!("not set up for 0.68.0: no probe")).is_some());
}

// --- the step of ci ---

/// The step skips, with the reason, when Kani cannot be used, and never runs the compile; it is not a failure.
#[test]
fn the_step_skips_without_kani_and_does_not_compile_anything() {
    for output in [None, Some("Kani Rust Verifier 0.67.0 (cargo plugin)\n"), Some("junk\n")] {
        match gate(unavailable(output), || panic!("the compile must not run without Kani 0.68.0")) {
            Ok(Outcome::Skipped(why)) => assert!(why.contains("0.68.0"), "{why}"),
            other => panic!("expected a skip, got {other:?}"),
        }
    }
}

#[test]
fn the_step_runs_the_compile_with_kani_and_passes_on_what_it_says() {
    assert_eq!(gate(None, || Ok(Outcome::Ok)), Ok(Outcome::Ok));
    assert_eq!(gate(None, || Err("crates/x: does not compile".into())), Err("crates/x: does not compile".to_string()));
    assert_eq!(
        gate(None, || Ok(Outcome::Skipped("no crate has a src/kani_proofs.rs".into()))),
        Ok(Outcome::Skipped("no crate has a src/kani_proofs.rs".into()))
    );
}

// --- finding the crates ---

#[test]
fn the_crates_are_the_ones_with_a_harness_file_in_src() {
    let files: Vec<String> = [
        "crates/units/src/kani_proofs.rs",
        "crates/calc/src/lib.rs",
        "crates/calc/src/kani_proofs.rs",
        "crates/geom/tests/kani_proofs.rs",
        "crates/geom/src/kani_proofs/mod.rs",
        "docs/kani_proofs.rs",
        "xtask/src/kani.rs",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(find_crates(&files), ["crates/calc", "crates/units"], "sorted; only `src/kani_proofs.rs` counts");
}

#[test]
fn the_harness_module_must_be_declared_behind_cfg_kani() {
    assert!(is_declared("mod a;\n\n#[cfg(kani)]\nmod kani_proofs;\n\npub use a::B;\n"));
    assert!(is_declared("#[cfg(kani)]\n\n  mod kani_proofs;"), "a blank line between is fine");
    assert!(!is_declared("mod kani_proofs;\n"), "without the attribute every ordinary build compiles it and fails on `kani`");
    assert!(!is_declared("#[cfg(test)]\nmod kani_proofs;\n"));
    assert!(!is_declared("#[cfg(kani)]\nmod other;\nmod kani_proofs;\n"), "the attribute belongs to the next item");
    assert!(!is_declared("// #[cfg(kani)] mod kani_proofs;\n"));
    assert!(!is_declared(""));
}

#[test]
fn a_crate_that_does_not_declare_its_harnesses_is_named() {
    let crates = ["crates/a", "crates/b", "crates/c"].map(String::from).to_vec();
    let read = |path: &str| match path {
        "crates/a/src/lib.rs" => Some("#[cfg(kani)]\nmod kani_proofs;\n".to_string()),
        "crates/b/src/lib.rs" => Some("mod kani_proofs;\n".to_string()),
        _ => None,
    };
    assert_eq!(undeclared(&crates, read), ["crates/b", "crates/c"], "a crate with no readable lib.rs counts as undeclared");
}

/// A binary crate has no `lib.rs`: its `main.rs` may declare the harness file instead (for `half_of` of `apps/diacraft-cli`), and `cargo kani`
/// proves the harnesses of a binary target as it does a library's. The `main.rs` needs the attribute as a `lib.rs` does.
#[test]
fn a_binary_crate_declares_its_harnesses_in_its_main_rs() {
    let crates = ["apps/bin", "apps/both", "apps/main-bad", "apps/lib-bad"].map(String::from).to_vec();
    let read = |path: &str| match path {
        "apps/bin/src/main.rs" => Some("mod args;\n#[cfg(kani)]\nmod kani_proofs;\n".to_string()),
        "apps/both/src/lib.rs" => Some("pub mod a;\n".to_string()),
        "apps/both/src/main.rs" => Some("#[cfg(kani)]\nmod kani_proofs;\n".to_string()),
        "apps/main-bad/src/main.rs" => Some("mod kani_proofs;\n".to_string()),
        "apps/lib-bad/src/lib.rs" => Some("#[cfg(test)]\nmod kani_proofs;\n".to_string()),
        _ => None,
    };
    assert_eq!(undeclared(&crates, read), ["apps/main-bad", "apps/lib-bad"], "a main.rs is read as a lib.rs is");
}

/// The repository's own harness files are all declared, and the three crates of the first proofs are among them.
#[test]
fn every_harness_file_of_the_repository_is_declared() {
    let root = xtask::root();
    let crates = find_crates(&xtask::repo_files(&root).unwrap());
    for expected in ["crates/calc", "crates/geom", "crates/units"] {
        assert!(crates.iter().any(|c| c == expected), "{expected} has no {HARNESS_FILE}: {crates:?}");
    }
    let missing = undeclared(&crates, |p| std::fs::read_to_string(root.join(p)).ok());
    assert!(missing.is_empty(), "declared nowhere: {missing:?}");
}

// --- the commands ---

fn args_of(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args().map(OsStr::to_string_lossy).map(|a| a.into_owned()).collect()
}

#[test]
fn the_proof_run_is_parallel_terse_and_limited_in_time_per_harness() {
    let args = args_of(&prove_command("crates/calc", 4));
    let expected = [
        "kani",
        "--manifest-path",
        "crates/calc/Cargo.toml",
        "-j",
        "4",
        "--output-format=terse",
        "-Z",
        "unstable-options",
        "--harness-timeout",
        HARNESS_TIMEOUT,
    ];
    assert_eq!(args, expected);
    assert!(HARNESS_TIMEOUT.ends_with('s') && HARNESS_TIMEOUT.trim_end_matches('s').parse::<u32>().is_ok(), "seconds, as Kani reads it");
}

/// `--manifest-path` and not `-p`: from the workspace root `-p` ignores a crate's `[package.metadata.kani.flags]` (the calc crate turns Kani's
/// NaN check off there), and a proof then fails at an `inf - inf` that the code answers with `#NUM!`.
#[test]
fn the_runs_point_at_the_crates_manifest_and_do_not_use_dash_p() {
    for args in [args_of(&prove_command("crates/units", 2)), args_of(&codegen_command("crates/units"))] {
        assert!(args.windows(2).any(|w| w == ["--manifest-path", "crates/units/Cargo.toml"]), "{args:?}");
        assert!(!args.iter().any(|a| a == "-p"), "{args:?}");
    }
}

#[test]
fn the_ci_step_only_compiles() {
    assert_eq!(
        args_of(&codegen_command("crates/calc")),
        ["kani", "--manifest-path", "crates/calc/Cargo.toml", "-Z", "unstable-options", "--only-codegen"]
    );
}

#[test]
fn the_default_is_four_jobs_and_the_option_overrides_it() {
    assert_eq!(DEFAULT_JOBS, 4, "the machine is shared: not every thread");
    assert_eq!(parse_jobs(&[]), Ok(4));
    assert_eq!(parse_jobs(&["-j", "16"]), Ok(16));
    assert_eq!(parse_jobs(&["--jobs", "1"]), Ok(1));
    for bad in [&["-j"][..], &["-j", "0"], &["-j", "many"], &["-j", "-3"], &["--harness", "x"], &["-j", "4", "5"]] {
        let err = parse_jobs(bad).unwrap_err();
        assert!(err.contains("usage: cargo xtask kani"), "{bad:?}: {err}");
    }
}

// --- what Kani prints ---

const GOOD: &str = "\
Thread 1: Checking harness kani_proofs::alpha...
Thread 0: Checking harness kani_proofs::beta...
Thread 1:
VERIFICATION RESULT:
 ** 0 of 7 failed

 ** 3 of 3 cover properties satisfied

VERIFICATION:- SUCCESSFUL
Verification Time: 0.5s
Thread 0:
VERIFICATION RESULT:
 ** 0 of 36 failed

VERIFICATION:- SUCCESSFUL
Verification Time: 63.0s
Manual Harness Summary:
Complete - 2 successfully verified harnesses, 0 failures, 2 total.
";

#[test]
fn a_run_that_proved_every_harness_passes_and_counts_them() {
    assert_eq!(read_report(GOOD), Report { verified: Some(2), total: Some(2), ..Report::default() });
    assert_eq!(judge("crates/calc", true, GOOD), Ok(2));
}

const FAILED: &str = "\
Thread 0: Checking harness kani_proofs::alpha...
Thread 1: Checking harness kani_proofs::beta...
Thread 0:
VERIFICATION RESULT:
 ** 1 of 9 failed
Failed Checks: assertion failed: x < 1
 File: \"crates/calc/src/kani_proofs.rs\", line 7, in kani_proofs::alpha

VERIFICATION:- FAILED
Verification Time: 0.1s
Thread 1:
VERIFICATION RESULT:
 ** 0 of 4 failed
VERIFICATION:- SUCCESSFUL
Verification Time: 0.1s
Manual Harness Summary:
Verification failed for - kani_proofs::alpha
Complete - 1 successfully verified harnesses, 1 failures, 2 total.
";

#[test]
fn a_failed_harness_fails_the_run_and_is_named() {
    let report = read_report(FAILED);
    assert_eq!(report.failed, ["kani_proofs::alpha"]);
    assert_eq!((report.verified, report.total), (Some(1), Some(2)));
    let err = judge("crates/calc", false, FAILED).unwrap_err();
    assert!(err.starts_with("crates/calc: ") && err.contains("1 failed: kani_proofs::alpha"), "{err}");
    assert!(!err.contains("beta"), "a harness that passed is not named: {err}");
    // Even a run that exited 0 with a failure listed is a failure: the text is read, not only the status.
    assert!(judge("crates/calc", true, FAILED).is_err());
}

const TIMED_OUT: &str = "\
Thread 2: Checking harness kani_proofs::slow...
Thread 3: Checking harness kani_proofs::quick...
Thread 3:
VERIFICATION RESULT:
 ** 0 of 1 failed
VERIFICATION:- SUCCESSFUL
Verification Time: 0.1s
Thread 2:
CBMC failed
VERIFICATION:- FAILED
CBMC timed out. You may want to rerun your proof with a larger timeout or use stubbing to reduce the size of the code the verifier reasons about.
Manual Harness Summary:
Verification failed for - kani_proofs::slow
Complete - 1 successfully verified harnesses, 1 failures, 2 total.
";

#[test]
fn a_harness_that_times_out_fails_the_run_and_is_named_as_a_timeout() {
    let report = read_report(TIMED_OUT);
    assert_eq!(report.timed_out, ["kani_proofs::slow"]);
    let err = judge("crates/geom", false, TIMED_OUT).unwrap_err();
    assert!(err.contains("1 timed out after 300s: kani_proofs::slow"), "{err}");
    assert!(!err.contains("1 failed"), "a timeout is not also listed as a failure: {err}");
    assert!(!err.contains("quick"), "{err}");
}

const UNREACHED: &str = "\
Thread 0: Checking harness kani_proofs::vacuous...
Thread 1: Checking harness kani_proofs::fine...
Thread 1:
VERIFICATION RESULT:
 ** 0 of 4 failed
 ** 2 of 2 cover properties satisfied
VERIFICATION:- SUCCESSFUL
Thread 0:
VERIFICATION RESULT:
 ** 0 of 4 failed
 ** 1 of 3 cover properties satisfied (2 unreachable)
VERIFICATION:- SUCCESSFUL
Complete - 2 successfully verified harnesses, 0 failures, 2 total.
";

/// Kani counts such a harness as verified: an assumption that left no input makes every assertion hold. The gate does not.
#[test]
fn a_cover_that_no_input_reaches_fails_the_run_and_names_the_harness() {
    assert_eq!(read_report(UNREACHED).unreached, ["kani_proofs::vacuous"]);
    let err = judge("crates/calc", true, UNREACHED).unwrap_err();
    assert!(err.contains("kani::cover!") && err.contains("kani_proofs::vacuous") && !err.contains("fine"), "{err}");
}

const FAILED_WITH_COVERS: &str = "\
Thread 0: Checking harness kani_proofs::alpha...
Thread 1: Checking harness kani_proofs::beta...
Thread 0:
VERIFICATION RESULT:
 ** 1 of 9 failed
 ** 1 of 3 cover properties satisfied (2 unreachable)
Failed Checks: assertion failed: x < 1
VERIFICATION:- FAILED
Thread 1:
VERIFICATION RESULT:
 ** 0 of 4 failed
 ** 1 of 2 cover properties satisfied
VERIFICATION:- SUCCESSFUL
Manual Harness Summary:
Verification failed for - kani_proofs::alpha
Complete - 1 successfully verified harnesses, 1 failures, 2 total.
";

/// A harness that failed has stopped short of some of its covers, and says so; the run is not also told that its covers were unreached, as a
/// harness that timed out is not also listed as failed.
#[test]
fn a_harness_that_failed_is_not_also_listed_as_having_an_unreached_cover() {
    let report = read_report(FAILED_WITH_COVERS);
    assert_eq!(report.failed, ["kani_proofs::alpha"]);
    assert_eq!(report.unreached, ["kani_proofs::alpha", "kani_proofs::beta"], "the report says what Kani printed");
    let err = judge("crates/calc", false, FAILED_WITH_COVERS).unwrap_err();
    assert!(err.contains("1 failed: kani_proofs::alpha"), "{err}");
    assert!(err.contains("1 with a `kani::cover!` that no input reaches"), "{err}");
    assert!(err.ends_with("kani_proofs::beta"), "the harness that passed and has the unreached cover is the one named: {err}");
    assert_eq!(err.matches("kani_proofs::alpha").count(), 1, "a failed harness is named once: {err}");
    // With no other harness at fault, the failure alone is reported.
    let alone = FAILED_WITH_COVERS.replace(" ** 1 of 2 cover properties satisfied\n", " ** 2 of 2 cover properties satisfied\n");
    let err = judge("crates/calc", false, &alone).unwrap_err();
    assert!(err.contains("1 failed: kani_proofs::alpha") && !err.contains("kani::cover!"), "{err}");
}

#[test]
fn a_run_of_one_thread_is_read_too() {
    let text = "Checking harness kani_proofs::only...\nVERIFICATION RESULT:\n ** 0 of 3 failed\n ** 1 of 2 cover properties satisfied\n\nComplete - 1 successfully verified harnesses, 0 failures, 1 total.\n";
    assert_eq!(read_report(text).unreached, ["kani_proofs::only"]);
}

#[test]
fn a_crash_that_names_no_harness_and_a_run_that_proved_nothing_fail() {
    let err = judge("crates/calc", false, "error: could not compile `diacraft-calc`\n").unwrap_err();
    assert!(err.contains("named no harness"), "{err}");
    let none = judge("crates/calc", true, "No proof harnesses (functions with #[kani::proof]) were found to verify.\n").unwrap_err();
    assert!(none.contains("nothing was proved"), "{none}");
    let zero = judge("crates/calc", true, "Complete - 0 successfully verified harnesses, 0 failures, 0 total.\n").unwrap_err();
    assert!(zero.contains("nothing was proved"), "{zero}");
}

#[test]
fn every_problem_of_a_run_is_in_the_one_error() {
    let text = format!("{FAILED}{TIMED_OUT}{UNREACHED}");
    let err = judge("crates/calc", false, &text).unwrap_err();
    assert!(err.contains("kani_proofs::alpha") && err.contains("timed out") && err.contains("kani_proofs::vacuous"), "{err}");
}

/// A harness file in a scratch repository is found through the same list `repo_files` gives the other gates.
#[test]
fn a_harness_file_in_a_repository_is_found_and_checked_for_its_declaration() {
    let dir = crate::git_repo("kani");
    dir.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
    dir.write("crates/a/src/lib.rs", "#[cfg(kani)]\nmod kani_proofs;\n");
    dir.write("crates/a/src/kani_proofs.rs", "");
    dir.write("crates/b/src/lib.rs", "mod kani_proofs;\n");
    dir.write("crates/b/src/kani_proofs.rs", "");
    let crates = find_crates(&xtask::repo_files(dir.path()).unwrap());
    assert_eq!(crates, ["crates/a", "crates/b"]);
    assert_eq!(undeclared(&crates, |p| std::fs::read_to_string(dir.path().join(p)).ok()), ["crates/b"]);
}

// --- the runs, through a runner ---

/// A scratch repository with the crates `declared` (a harness file the crate's `lib.rs` declares) and `undeclared` (one it does not).
fn repo_with_harnesses(tag: &str, declared: &[&str], undeclared: &[&str]) -> crate::TempDir {
    let dir = crate::git_repo(tag);
    for name in declared {
        dir.write(&format!("crates/{name}/src/lib.rs"), "#[cfg(kani)]\nmod kani_proofs;\n");
        dir.write(&format!("crates/{name}/src/kani_proofs.rs"), "");
    }
    for name in undeclared {
        dir.write(&format!("crates/{name}/src/lib.rs"), "mod kani_proofs;\n");
        dir.write(&format!("crates/{name}/src/kani_proofs.rs"), "");
    }
    dir
}

/// Two harnesses proved, and Kani's closing line.
const PROVED: &str = "Complete - 2 successfully verified harnesses, 0 failures, 2 total.\n";

/// Runs `prove_with` on `root` with a runner that answers by the crate's directory and notes each call.
fn prove_through(
    why_not: Option<String>,
    root: &std::path::Path,
    answer: impl Fn(&str) -> Result<(bool, String), String>,
) -> (Result<Outcome, String>, Vec<(String, usize)>) {
    let calls = RefCell::new(Vec::new());
    let result = prove_with(why_not, root, 3, &mut |dir, jobs| {
        calls.borrow_mut().push((dir.to_string(), jobs));
        answer(dir)
    });
    (result, calls.into_inner())
}

#[test]
fn the_proof_run_proves_each_crate_in_turn_with_the_jobs_it_was_given() {
    let repo = repo_with_harnesses("kani-prove-ok", &["b", "a"], &[]);
    let (result, calls) = prove_through(None, repo.path(), |_| Ok((true, PROVED.to_string())));
    assert_eq!(result, Ok(Outcome::Ok));
    assert_eq!(calls, [("crates/a".to_string(), 3), ("crates/b".to_string(), 3)], "every crate, sorted, with the jobs");
}

/// Every crate is run, and a crate that fails fails the command, whichever it is (the hand mutants that report success when one crate of
/// two failed, or lose its problem, made no test fail).
#[test]
fn a_crate_that_fails_fails_the_command_and_the_others_are_still_run() {
    let repo = repo_with_harnesses("kani-prove-fail", &["a", "b"], &[]);
    for failing in ["crates/a", "crates/b"] {
        let (result, calls) =
            prove_through(None, repo.path(), |dir| Ok(if dir == failing { (false, FAILED.to_string()) } else { (true, PROVED.to_string()) }));
        let err = result.unwrap_err();
        assert!(err.starts_with("kani: 1 of 2 crates not proved. "), "{err}");
        assert!(err.contains(&format!("{failing}: 1 failed: kani_proofs::alpha")), "{err}");
        let other = if failing == "crates/a" { "crates/b" } else { "crates/a" };
        assert!(!err.contains(other), "a crate that was proved is not named: {err}");
        assert_eq!(calls.len(), 2, "the crate after the failure is run too: {calls:?}");
    }
}

#[test]
fn every_crate_that_fails_is_named_in_the_one_error() {
    let repo = repo_with_harnesses("kani-prove-both", &["a", "b", "c"], &[]);
    let (result, calls) = prove_through(None, repo.path(), |dir| match dir {
        "crates/b" => Ok((true, PROVED.to_string())),
        "crates/a" => Ok((false, FAILED.to_string())),
        _ => Ok((true, TIMED_OUT.to_string())),
    });
    let err = result.unwrap_err();
    assert!(err.starts_with("kani: 2 of 3 crates not proved. "), "{err}");
    assert!(err.contains("crates/a: 1 failed: kani_proofs::alpha"), "{err}");
    assert!(err.contains("crates/c: 1 timed out after 300s: kani_proofs::slow"), "{err}");
    assert!(!err.contains("crates/b"), "{err}");
    assert_eq!(calls.len(), 3);
}

/// A crate whose run could not start (cargo is not there, say) is a problem of that crate and is named.
#[test]
fn a_run_that_could_not_start_fails_the_command_naming_the_crate() {
    let repo = repo_with_harnesses("kani-prove-spawn", &["a", "b"], &[]);
    let (result, _) = prove_through(None, repo.path(), |dir| {
        if dir == "crates/b" { Err("failed to spawn cargo: not found".into()) } else { Ok((true, PROVED.to_string())) }
    });
    let err = result.unwrap_err();
    assert!(err.starts_with("kani: 1 of 2 crates not proved. crates/b: ") && err.contains("failed to spawn cargo: not found"), "{err}");
}

#[test]
fn a_proof_run_without_kani_fails_with_the_reason_and_runs_nothing() {
    let repo = repo_with_harnesses("kani-prove-nokani", &["a"], &[]);
    let (result, calls) = prove_through(Some("Kani 0.68.0 is not set up".into()), repo.path(), |_| Ok((true, PROVED.to_string())));
    assert_eq!(result, Err("cannot prove: Kani 0.68.0 is not set up".to_string()), "the command was asked for, so it fails and does not skip");
    assert!(calls.is_empty());
}

#[test]
fn a_proof_run_with_no_harness_file_in_the_repository_is_skipped() {
    let repo = crate::git_repo("kani-prove-none");
    repo.write("crates/a/src/lib.rs", "");
    let (result, calls) = prove_through(None, repo.path(), |_| Ok((true, PROVED.to_string())));
    assert_eq!(result, Ok(Outcome::Skipped("no crate has a src/kani_proofs.rs".into())));
    assert!(calls.is_empty());
}

/// A harness file that its crate's `lib.rs` does not declare is compiled by nothing: the run is refused before any crate is proved, and the
/// error names the crate and the fix.
#[test]
fn a_harness_file_that_is_not_declared_refuses_the_proof_run_before_anything_is_run() {
    let repo = repo_with_harnesses("kani-prove-undeclared", &["a"], &["b"]);
    let (result, calls) = prove_through(None, repo.path(), |_| Ok((true, PROVED.to_string())));
    let err = result.unwrap_err();
    assert!(err.starts_with("crates/b: has a src/kani_proofs.rs that neither src/lib.rs nor src/main.rs declares"), "{err}");
    assert!(err.contains("#[cfg(kani)]") && err.contains("mod kani_proofs;"), "the fix is in the message: {err}");
    assert!(!err.contains("crates/a"), "a crate that declares its file is not named: {err}");
    assert!(calls.is_empty(), "no crate is proved when one is undeclared: {calls:?}");
}

/// Runs `check_with` on `root` with a compile that answers by the crate's directory and notes each call.
fn check_through(
    why_not: Option<String>,
    root: &std::path::Path,
    answer: impl Fn(&str) -> Result<(), String>,
) -> (Result<Outcome, String>, Vec<String>) {
    let calls = RefCell::new(Vec::new());
    let result = check_with(why_not, root, &mut |dir| {
        calls.borrow_mut().push(dir.to_string());
        answer(dir)
    });
    (result, calls.into_inner())
}

#[test]
fn the_step_compiles_each_crates_harnesses_in_turn_and_passes() {
    let repo = repo_with_harnesses("kani-check-ok", &["b", "a"], &[]);
    let (result, calls) = check_through(None, repo.path(), |_| Ok(()));
    assert_eq!(result, Ok(Outcome::Ok));
    assert_eq!(calls, ["crates/a", "crates/b"]);
}

/// The step is the gate for a harness that no longer builds: a failure of any crate's compile fails it, with the message of the run.
#[test]
fn a_harness_that_does_not_compile_fails_the_step() {
    let repo = repo_with_harnesses("kani-check-fail", &["a", "b"], &[]);
    for failing in ["crates/a", "crates/b"] {
        let (result, _) = check_through(None, repo.path(), |dir| if dir == failing { Err(format!("{dir}: exited with code 101")) } else { Ok(()) });
        assert_eq!(result, Err(format!("{failing}: exited with code 101")));
    }
}

#[test]
fn the_step_skips_without_kani_whatever_the_crates_hold() {
    let repo = repo_with_harnesses("kani-check-nokani", &["a"], &["b"]);
    let (result, calls) = check_through(Some("Kani 0.68.0 is not set up".into()), repo.path(), |_| Ok(()));
    assert_eq!(result, Ok(Outcome::Skipped("Kani 0.68.0 is not set up".into())));
    assert!(calls.is_empty());
}

#[test]
fn the_step_skips_when_no_crate_has_a_harness_file() {
    let repo = crate::git_repo("kani-check-none");
    repo.write("crates/a/src/lib.rs", "");
    let (result, calls) = check_through(None, repo.path(), |_| Ok(()));
    assert_eq!(result, Ok(Outcome::Skipped("no crate has a src/kani_proofs.rs".into())));
    assert!(calls.is_empty());
}

#[test]
fn a_harness_file_that_is_not_declared_fails_the_step_before_anything_is_compiled() {
    let repo = repo_with_harnesses("kani-check-undeclared", &["a"], &["b"]);
    let (result, calls) = check_through(None, repo.path(), |_| Ok(()));
    let err = result.unwrap_err();
    assert!(err.starts_with("crates/b: has a src/kani_proofs.rs that neither src/lib.rs nor src/main.rs declares"), "{err}");
    assert!(calls.is_empty(), "{calls:?}");
}
