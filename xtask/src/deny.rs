//! `cargo xtask deny`: dependency licences, bans, sources and RustSec advisories (`deny.toml`; needs cargo-deny).
//!
//! Code dependencies follow the same rule as assets (AGENTS.md): permissive licences only, and no
//! GPL, AGPL or LGPL code is linked. The advisories check keeps a crate with a known vulnerability
//! out of the lock, and shows an unmaintained or unsound crate until a dated line of `deny.toml`
//! says why it stays.
//!
//! **Two runs, so the network is only needed by the second.** The licence, ban and source checks read
//! the lock and the manifests, so they run as they did before, with no network. The advisories check
//! needs the RustSec database, a git repository that cargo-deny fetches into `~/.cargo/advisory-dbs`
//! and that changes without a commit here. It runs on its own, so that a failed fetch can be told from
//! a finding:
//!
//! - **a finding** (an `error[...]` diagnostic: a vulnerability, or an unmaintained, unsound or yanked
//!   crate that `deny.toml` does not ignore) fails the step, and is not run again;
//! - **a failed fetch** (cargo-deny's `failed to fetch advisory database`, a machine with no network)
//!   runs the check again with `--offline`, against the copy of the database already on the machine,
//!   and the step prints that the database was not refreshed. The copy is as old as the last run that
//!   had a network, and cargo-deny refuses one older than its staleness limit;
//! - **a failed fetch and no usable copy** fails the step in words, and names `cargo deny fetch`. A
//!   check that could not look at the database never passes: the step does not skip, as the licence
//!   check does not skip when cargo-deny is missing;
//! - **any other failure** of the tool (a broken `deny.toml`) fails the step, with the tool's own words.

use crate::ci::Outcome;

/// The arguments `cargo` gets for the licence, ban and source checks (no network).
pub const ARGS: &[&str] = &["deny", "check", "licenses", "bans", "sources"];

/// The arguments `cargo` gets for the advisories check, which fetches the RustSec database.
pub const ADVISORY_ARGS: &[&str] = &["deny", "check", "advisories"];

/// The same check against the copy of the database on this machine: `--offline` is a flag of `cargo deny`, before the subcommand.
pub const ADVISORY_OFFLINE_ARGS: &[&str] = &["deny", "--offline", "check", "advisories"];

/// What one run of the advisories check came to: whether it exited successfully, and all it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The exit status was zero.
    pub success: bool,
    /// Standard error and standard output, in that order.
    pub output: String,
}

/// Runs `cargo deny check licenses bans sources`, then `cargo deny check advisories`. The check is not optional: without
/// cargo-deny it fails with the install hint, on every machine, in CI or not.
pub fn run() -> Result<Outcome, String> {
    let installed = crate::has_cargo_tool("deny");
    gate(installed, || {
        let mut c = crate::cargo();
        c.args(ARGS);
        crate::run(c, &format!("cargo {}", ARGS.join(" ")))
    })?;
    advisory_gate(attempt, |notice| eprintln!("{notice}"))
}

/// One run of the advisories check, with the database fetched (`offline` false) or not. Its output is printed as it ended, since
/// the findings are what the person reads.
fn attempt(offline: bool) -> Result<Run, String> {
    let args = if offline { ADVISORY_OFFLINE_ARGS } else { ADVISORY_ARGS };
    let what = format!("cargo {}", args.join(" "));
    eprintln!("$ {what}");
    let mut c = crate::cargo();
    c.args(args);
    let out = c.output().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    let output = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    eprint!("{output}");
    Ok(Run { success: out.status.success(), output })
}

/// The licence gate with the tool's presence and the check passed in, so the tests can set both.
pub fn gate(installed: bool, check: impl FnOnce() -> Result<(), String>) -> Result<Outcome, String> {
    if !installed {
        return Err(
            "the licence check needs cargo-deny, and `cargo deny --version` failed: install it with `cargo install cargo-deny --locked`".into()
        );
    }
    check()?;
    Ok(Outcome::Ok)
}

/// Whether the output says the advisory database could not be fetched: cargo-deny's own words for a machine with no network.
pub fn fetch_failed(output: &str) -> bool {
    output.contains("failed to fetch advisory database")
}

/// Whether the output holds a diagnostic of the check itself (`error[vulnerability]`, `error[unmaintained]`, ...), which a log line of
/// the tool (`[ERROR] ...`) never does.
fn has_finding(output: &str) -> bool {
    output.contains("error[")
}

const FINDING: &str = "the advisory check failed: a crate in Cargo.lock has a RustSec advisory that deny.toml does not ignore (the findings are printed above); update the crate, or ignore the advisory in deny.toml with a dated reason";

/// The advisories check, with one run of it (`attempt(offline)`) and the way notices are said passed in, so the tests can set both.
pub fn advisory_gate(attempt: impl Fn(bool) -> Result<Run, String>, mut say: impl FnMut(&str)) -> Result<Outcome, String> {
    let first = attempt(false)?;
    if first.success {
        return Ok(Outcome::Ok);
    }
    if has_finding(&first.output) {
        return Err(FINDING.into());
    }
    if !fetch_failed(&first.output) {
        return Err("the advisory check could not run: cargo deny failed with no finding to show (its words are printed above)".into());
    }
    let second = attempt(true)?;
    if second.success {
        say(
            "advisories: the RustSec database could not be fetched (no network?), so the check ran against the copy already on this machine, which is not up to date: advisories published since the last run with a network are not in it",
        );
        return Ok(Outcome::Ok);
    }
    if has_finding(&second.output) {
        return Err(FINDING.into());
    }
    Err("the advisory database could not be fetched (no network?) and there is no usable copy of it on this machine: run `cargo deny fetch` once with network access, then run the step again".into())
}
