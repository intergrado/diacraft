//! `cargo xtask kani`, and the `kani check` step of `ci`: the Kani proofs of the repository (`docs/development.md`, Proofs).
//!
//! [Kani](https://model-checking.github.io/kani/) is a bounded model checker for Rust. A crate keeps its proof harnesses in
//! `src/kani_proofs.rs`, declared in `src/lib.rs` (in a crate that is only a binary, `src/main.rs`) as `#[cfg(kani)] mod kani_proofs;`, so
//! that no ordinary build sees them. Nothing else compiles that code: `cargo clippy` and `cargo test` skip it (`cfg(kani)` is off), and
//! Kani's own compiler ignores clippy's lints. So the gate has two parts.
//!
//! - **`ci` compiles the harnesses** (`cargo kani --only-codegen`, about 8 s warm and 1.5 minutes cold), so a harness that no longer builds
//!   fails the gate. It is skipped, not failed, on a machine without Kani 0.68.0: the install is a download of a few hundred megabytes and a
//!   nightly toolchain, and the licence and fuzz gates already show how an optional tool is handled. A machine that has the installer but never
//!   ran `cargo kani setup` is skipped too, and is not asked its version: `cargo kani --version` would run the setup, 485 MB, itself
//!   ([`machine_unavailable`]).
//! - **`cargo xtask kani` proves them**: every harness of every crate, in parallel, each with a time limit. It takes minutes, so it is not
//!   part of `ci`. A harness that fails, times out, or has a `kani::cover!` that no input reaches (the proof would then say nothing) fails
//!   the command, and the output names it.
//!
//! The version is pinned: Kani changes what it models (and how it words its output, which this module reads) between releases, so a
//! proof is only as good as the release it was written and checked with.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ci::Outcome;

/// The Kani release the harnesses are written for and the gate runs.
pub const VERSION: &str = "0.68.0";

/// The file a crate keeps its harnesses in, in its `src` folder.
pub const HARNESS_FILE: &str = "kani_proofs.rs";

/// The time one harness may take (`--harness-timeout`). The harnesses take a minute at most on a quiet machine; the rest is for a busy one.
pub const HARNESS_TIMEOUT: &str = "300s";

/// The install steps, for a message.
pub const INSTALL: &str = "cargo install --locked kani-verifier --version 0.68.0, then cargo kani setup";

/// The version `cargo kani --version` printed, from its output (`Kani Rust Verifier 0.68.0 (cargo plugin)` on the first line).
pub fn installed_version(output: &str) -> Option<&str> {
    let rest = output.lines().find_map(|l| l.trim().strip_prefix("Kani Rust Verifier "))?;
    rest.split_whitespace().next()
}

/// Why Kani cannot be used, or `None` when it can. `output` is what `cargo kani --version` printed, or `None` when it did not run.
pub fn unavailable(output: Option<&str>) -> Option<String> {
    match output.map(installed_version) {
        None | Some(None) => Some(format!("`cargo kani --version` failed, so Kani is not installed ({INSTALL})")),
        Some(Some(VERSION)) => None,
        Some(Some(other)) => Some(format!("Kani {other} is installed and the proofs are for {VERSION} ({INSTALL})")),
    }
}

/// What `cargo kani --version` printed, or `None` when the command did not run or failed.
fn version_output() -> Option<String> {
    let out = crate::cargo().args(["kani", "--version"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Where `cargo kani setup` puts this release, the folder Kani looks for before it runs: `$KANI_HOME/kani-VERSION` when `KANI_HOME` is set (to anything, an
/// empty value too, as Kani reads it), else `$HOME/.kani/kani-VERSION`. `None` when there is nowhere to look.
pub fn setup_dir(kani_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match kani_home {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(home?).join(".kani"),
    };
    Some(base.join(format!("kani-{VERSION}")))
}

/// Why Kani cannot be used on this machine, or `None` when it can. `kani_home` and `home` are the environment's `KANI_HOME` and `HOME`, and `version` runs
/// `cargo kani --version` and gives its output.
///
/// `version` is not run unless the release's folder ([`setup_dir`]) is there. `cargo kani` runs Kani's first-time setup before anything else, `--version`
/// included, and that is a download of about 485 MB: a gate step that only wants to know whether it can run must not start it. A machine that has not run
/// `cargo kani setup` is skipped, with the way to set it up.
pub fn machine_unavailable(kani_home: Option<&str>, home: Option<&str>, version: impl FnOnce() -> Option<String>) -> Option<String> {
    let Some(dir) = setup_dir(kani_home, home) else {
        return Some(format!("cannot tell whether Kani {VERSION} is set up, as neither KANI_HOME nor HOME is set ({INSTALL})"));
    };
    if !dir.exists() {
        return Some(format!("Kani {VERSION} is not set up: {} is missing, and `cargo kani --version` would download it ({INSTALL})", dir.display()));
    }
    unavailable(version().as_deref())
}

/// [`machine_unavailable`] for this machine's environment.
fn this_machine_unavailable() -> Option<String> {
    let (kani_home, home) = (std::env::var("KANI_HOME").ok(), std::env::var("HOME").ok());
    machine_unavailable(kani_home.as_deref(), home.as_deref(), version_output)
}

/// The directories of the crates that hold harnesses among `files` (relative paths): those with a `src/kani_proofs.rs`. Sorted.
pub fn find_crates(files: &[String]) -> Vec<String> {
    let suffix = format!("/src/{HARNESS_FILE}");
    let mut found: Vec<String> = files.iter().filter_map(|f| f.strip_suffix(suffix.as_str())).map(str::to_owned).collect();
    found.sort();
    found
}

/// Whether the text of a `lib.rs` declares the harness module behind `cfg(kani)`: a `#[cfg(kani)]` line and, on the next line that holds
/// anything, `mod kani_proofs;`. Without the attribute every ordinary build would compile the harnesses and fail on `kani`.
pub fn is_declared(lib_rs: &str) -> bool {
    let mut lines = lib_rs.lines().map(str::trim).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        if line == "#[cfg(kani)]" && lines.next() == Some("mod kani_proofs;") {
            return true;
        }
    }
    false
}

/// The crate roots that may declare the harness file: a library's `src/lib.rs`, and a binary's `src/main.rs` (a crate with no library, such as
/// `apps/diacraft-cli`: `cargo kani` proves the harnesses of a binary target as it does a library's).
pub const ROOTS: [&str; 2] = ["lib.rs", "main.rs"];

/// The crates among `crates` (directories) where neither `src/lib.rs` nor `src/main.rs` declares their harness file: a file that nothing compiles.
/// `read` gives a file's text by its path relative to the root.
pub fn undeclared(crates: &[String], read: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let declares = |dir: &str| ROOTS.iter().any(|root| read(&format!("{dir}/src/{root}")).is_some_and(|text| is_declared(&text)));
    crates.iter().filter(|dir| !declares(dir)).cloned().collect()
}

/// `cargo kani` on one crate, in the root. `--manifest-path` and not `-p`: Kani reads a crate's `[package.metadata.kani.flags]` only for the
/// package it is run in or pointed at, and `-p` from the workspace root ignores them. The other arguments are the ones the proof run adds.
fn kani_in(dir: &str) -> Command {
    let mut c = crate::cargo();
    c.args(["kani", "--manifest-path"]).arg(format!("{dir}/Cargo.toml"));
    c
}

/// The proof run of one crate: its harnesses on `jobs` threads, in the terse format (the only one that can run in parallel), each with a time
/// limit (`--harness-timeout` needs `-Z unstable-options`).
pub fn prove_command(dir: &str, jobs: usize) -> Command {
    let mut c = kani_in(dir);
    c.args(["-j", &jobs.to_string(), "--output-format=terse", "-Z", "unstable-options", "--harness-timeout", HARNESS_TIMEOUT]);
    c
}

/// The compile-only run of one crate: every harness is built, none is proved.
pub fn codegen_command(dir: &str) -> Command {
    let mut c = kani_in(dir);
    c.args(["-Z", "unstable-options", "--only-codegen"]);
    c
}

/// The harnesses `cargo xtask kani` proves at once unless `-j` says otherwise. Each is a CBMC process of its own and some take a few
/// gigabytes, and the machine is shared: the default is modest. (`cargo kani -j` alone would take the machine's every thread.)
pub const DEFAULT_JOBS: usize = 4;

/// The harnesses to prove at once, from the arguments after `kani`: none gives [`DEFAULT_JOBS`], and `-j N` or `--jobs N` (`N` at least 1) gives `N`.
pub fn parse_jobs(args: &[&str]) -> Result<usize, String> {
    const USAGE: &str = "usage: cargo xtask kani [-j N | --jobs N]";
    match args {
        [] => Ok(DEFAULT_JOBS),
        ["-j" | "--jobs", n] => match n.parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!("`{n}` is not a number of jobs of 1 or more\n{USAGE}")),
        },
        _ => Err(USAGE.into()),
    }
}

/// What a proof run of one crate printed, read from its terse output.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The harnesses Kani counted as verified, and in all, from its closing `Complete - ...` line (`None` when it printed none).
    pub verified: Option<usize>,
    /// See `verified`.
    pub total: Option<usize>,
    /// The harnesses Kani lists as failed, a timed-out one among them.
    pub failed: Vec<String>,
    /// The harnesses whose time ran out.
    pub timed_out: Vec<String>,
    /// The harnesses with a `kani::cover!` that no input reaches: such a proof may hold for no input at all.
    pub unreached: Vec<String>,
}

/// The harness name in a `Checking harness NAME...` line.
fn checking(rest: &str) -> Option<&str> {
    rest.strip_prefix("Checking harness ").map(|name| name.trim_end().trim_end_matches("..."))
}

/// Splits a `Thread N: rest` line into the thread and the rest; a line with no such prefix has no thread.
fn split_thread(line: &str) -> (Option<&str>, &str) {
    let Some(after) = line.strip_prefix("Thread ") else { return (None, line) };
    match after.split_once(':') {
        Some((n, rest)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => (Some(n), rest.trim_start()),
        _ => (None, line),
    }
}

/// The count `N` of `N of M cover properties satisfied` and `M`, in a line of Kani's result. Kani may add `(2 unreachable)` after it.
fn cover_counts(line: &str) -> Option<(usize, usize)> {
    let rest = line.trim().strip_prefix("** ")?;
    let (counts, _) = rest.split_once(" cover properties satisfied")?;
    let (n, m) = counts.split_once(" of ")?;
    Some((n.trim().parse().ok()?, m.trim().parse().ok()?))
}

/// The counts of the closing `Complete - N successfully verified harnesses, M failures, T total.` line: `(N, T)`.
fn completed(line: &str) -> Option<(usize, usize)> {
    let rest = line.trim().strip_prefix("Complete - ")?;
    let verified = rest.split_whitespace().next()?.parse().ok()?;
    let total = rest.trim_end_matches('.').rsplit(", ").next()?.split_whitespace().next()?.parse().ok()?;
    Some((verified, total))
}

/// Reads the terse output of a proof run. A harness's result comes as a block that starts at a `Thread N:` line (the harness it belongs
/// to is the one `Thread N: Checking harness ...` named), so each line is read against the thread that last spoke.
pub fn read_report(output: &str) -> Report {
    let mut report = Report::default();
    // The harness each thread is checking, and the thread that last spoke ("" when the output has no threads: a run of one).
    let mut harness_of: HashMap<&str, &str> = HashMap::new();
    let mut thread = "";
    for line in output.lines() {
        let (spoke, rest) = split_thread(line);
        if let Some(t) = spoke {
            thread = t;
        }
        if let Some(name) = checking(rest) {
            harness_of.insert(thread, name);
        } else if let Some(name) = rest.trim().strip_prefix("Verification failed for - ") {
            report.failed.push(name.to_owned());
        } else if let Some((n, m)) = cover_counts(rest) {
            if n < m {
                report.unreached.extend(harness_of.get(thread).map(|h| (*h).to_owned()));
            }
        } else if rest.contains("CBMC timed out") {
            report.timed_out.extend(harness_of.get(thread).map(|h| (*h).to_owned()));
        } else if let Some((verified, total)) = completed(rest) {
            report.verified = Some(verified);
            report.total = Some(total);
        }
    }
    report
}

/// The verdict on a proof run of the crate in `dir`: `succeeded` is whether `cargo kani` exited successfully and `output` what it printed.
/// `Ok` is the number of harnesses proved. The error names every harness that failed, timed out or has a cover that no input reaches.
pub fn judge(dir: &str, succeeded: bool, output: &str) -> Result<usize, String> {
    let report = read_report(output);
    let mut problems: Vec<String> = Vec::new();
    let failed: Vec<&String> = report.failed.iter().filter(|f| !report.timed_out.contains(f)).collect();
    if !failed.is_empty() {
        problems.push(format!("{} failed: {}", failed.len(), failed.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if !report.timed_out.is_empty() {
        problems.push(format!(
            "{} timed out after {HARNESS_TIMEOUT}: {} (constrain the inputs or try `#[kani::solver(kissat)]`; docs/development.md, Proofs)",
            report.timed_out.len(),
            report.timed_out.join(", ")
        ));
    }
    // A harness that failed (or timed out) stopped short of some of its covers, and is already named: only the ones that passed are told apart here.
    let unreached: Vec<&String> = report.unreached.iter().filter(|h| !report.failed.contains(h)).collect();
    if !unreached.is_empty() {
        problems.push(format!(
            "{} with a `kani::cover!` that no input reaches (an assumption left nothing, or the code no longer takes that path, so the proof says nothing \
             there): {}",
            unreached.len(),
            unreached.iter().map(|h| h.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if problems.is_empty() && !succeeded {
        problems
            .push("`cargo kani` failed and named no harness: the crate did not compile, or Kani itself stopped (the output above says which)".into());
    }
    match (problems.is_empty(), report.verified) {
        (false, _) => Err(format!("{dir}: {}", problems.join("; "))),
        (true, Some(n)) if n > 0 => Ok(n),
        (true, _) => {
            Err(format!("{dir}: `cargo kani` verified no harness (it printed no `Complete - ...` line, or none was found): nothing was proved"))
        }
    }
}

/// What a run of `cargo kani` came to: whether it exited successfully and what it printed, or why it could not be run.
pub type RunResult = Result<(bool, String), String>;

/// Runs `cmd` with its standard output copied to `out` as it arrives (its error output goes where it goes). Returns whether it succeeded and
/// what it printed.
fn run_copying(mut cmd: Command, out: &mut dyn Write) -> RunResult {
    cmd.stdout(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("failed to spawn cargo: {e}"))?;
    let mut printed = String::new();
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // A copy that cannot be written (a closed terminal) must not fail the run: the verdict is the exit status and the text.
                    let _ = out.write_all(&line);
                    printed.push_str(&String::from_utf8_lossy(&line));
                }
            }
        }
    }
    let status = child.wait().map_err(|e| format!("cargo did not finish: {e}"))?;
    Ok((status.success(), printed))
}

/// The crates to run on, from the repository's files at `root`: those with harnesses, each one's declaration checked. `Ok(vec![])` when none
/// has any.
fn crates_to_run(root: &Path) -> Result<Vec<String>, String> {
    let crates = find_crates(&crate::repo_files(root)?);
    let missing = undeclared(&crates, |p| std::fs::read_to_string(root.join(p)).ok());
    if !missing.is_empty() {
        return Err(format!(
            "{}: has a src/{HARNESS_FILE} that neither src/lib.rs nor src/main.rs declares: add `#[cfg(kani)]` and `mod {};` to the crate's root, or \
             nothing compiles the harnesses",
            missing.join(", "),
            HARNESS_FILE.trim_end_matches(".rs")
        ));
    }
    Ok(crates)
}

const NO_CRATES: &str = "no crate has a src/kani_proofs.rs";

/// `cargo xtask kani`: proves every harness of every crate that has one, a crate after another with `jobs` of its harnesses at a time. Every
/// crate is run, and the error lists each one that failed. A machine without Kani 0.68.0 fails with the install hint: the command was asked for.
pub fn prove(jobs: usize) -> Result<Outcome, String> {
    prove_with(this_machine_unavailable(), &crate::root(), jobs, &mut |dir, jobs| {
        eprintln!("$ cargo kani --manifest-path {dir}/Cargo.toml -j {jobs} --output-format=terse --harness-timeout {HARNESS_TIMEOUT}");
        run_copying(prove_command(dir, jobs), &mut std::io::stdout())
    })
}

/// [`prove`] with what depends on the machine passed in, so that the tests can decide it: `why_not` is why Kani cannot be used ([`machine_unavailable`]),
/// `root` the repository, and `run` proves the crate in a directory with a number of jobs and gives whether `cargo kani` succeeded and what it printed.
pub fn prove_with(why_not: Option<String>, root: &Path, jobs: usize, run: &mut dyn FnMut(&str, usize) -> RunResult) -> Result<Outcome, String> {
    if let Some(why) = why_not {
        return Err(format!("cannot prove: {why}"));
    }
    let crates = crates_to_run(root)?;
    if crates.is_empty() {
        return Ok(Outcome::Skipped(NO_CRATES.into()));
    }
    let mut proved = 0;
    let mut problems = Vec::new();
    for dir in &crates {
        match run(dir, jobs).map_err(|e| format!("{dir}: {e}")).and_then(|(succeeded, printed)| judge(dir, succeeded, &printed)) {
            Ok(n) => proved += n,
            Err(problem) => problems.push(problem),
        }
    }
    if problems.is_empty() {
        println!("kani: {proved} harnesses proved in {} crates ({})", crates.len(), crates.join(", "));
        return Ok(Outcome::Ok);
    }
    Err(format!("kani: {} of {} crates not proved. {}", problems.len(), crates.len(), problems.join(". ")))
}

/// The `kani check` step of `ci`: every harness is compiled by Kani's compiler, none is proved. Skipped without Kani 0.68.0, and when no crate has
/// harnesses.
pub fn check() -> Result<Outcome, String> {
    check_with(this_machine_unavailable(), &crate::root(), &mut |dir| {
        crate::run(codegen_command(dir), &format!("cargo kani --manifest-path {dir}/Cargo.toml --only-codegen"))
    })
}

/// [`check`] with what depends on the machine passed in, as [`prove_with`] has it: `compile` compiles the harnesses of the crate in a directory, and its
/// error fails the step.
pub fn check_with(why_not: Option<String>, root: &Path, compile: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<Outcome, String> {
    gate(why_not, || {
        let crates = crates_to_run(root)?;
        if crates.is_empty() {
            return Ok(Outcome::Skipped(NO_CRATES.into()));
        }
        for dir in &crates {
            compile(dir)?;
        }
        println!("kani check: the harnesses of {} crates compile ({})", crates.len(), crates.join(", "));
        Ok(Outcome::Ok)
    })
}

/// The step with why Kani cannot be used passed in ([`machine_unavailable`]), so the tests can set it: a machine that cannot use Kani skips (`why_not`),
/// and one that can runs `compile`.
pub fn gate(why_not: Option<String>, compile: impl FnOnce() -> Result<Outcome, String>) -> Result<Outcome, String> {
    match why_not {
        Some(why) => Ok(Outcome::Skipped(why)),
        None => compile(),
    }
}
