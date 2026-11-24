//! `cargo xtask deny` and `deny.toml`: permissive licences only (AGENTS.md, the rule on licences).

use std::cell::{Cell, RefCell};

use xtask::ci::Outcome;
use xtask::deny::{ADVISORY_ARGS, ADVISORY_OFFLINE_ARGS, ARGS, Run, advisory_gate, fetch_failed, gate};

/// The quoted strings of the first `key = [ ... ]` list that follows `section` in `text`.
fn list(text: &str, section: &str, key: &str) -> Vec<String> {
    let after = &text[text.find(section).unwrap_or_else(|| panic!("no {section} in deny.toml"))..];
    let body = &after[after.find(&format!("{key} = [")).unwrap_or_else(|| panic!("no {key} in {section}"))..];
    let body = &body[..body.find(']').unwrap()];
    // Comments may contain quotes; only quoted strings at the start of a line's code count.
    body.lines()
        .map(|l| l.split('#').next().unwrap_or_default())
        .flat_map(|l| l.split('"').skip(1).step_by(2).map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

fn deny_toml() -> String {
    std::fs::read_to_string(xtask::root().join("deny.toml")).unwrap()
}

#[test]
fn the_gate_checks_licenses_bans_and_sources() {
    assert_eq!(ARGS, ["deny", "check", "licenses", "bans", "sources"]);
}

/// The licence gate is not optional: a machine without cargo-deny must fail it, not pass it.
#[test]
fn the_gate_fails_without_cargo_deny_and_does_not_run_the_check() {
    let err = gate(false, || panic!("the check must not run without the tool")).unwrap_err();
    assert!(err.contains("cargo install cargo-deny --locked"), "{err}");
    assert!(err.contains("cargo-deny"), "{err}");
}

#[test]
fn the_gate_runs_the_check_when_the_tool_is_there() {
    assert_eq!(gate(true, || Ok(())), Ok(Outcome::Ok));
    assert_eq!(gate(true, || Err("1 licence rejected".into())), Err("1 licence rejected".to_string()));
}

#[test]
fn deny_toml_allows_the_permissive_licences_and_no_copyleft() {
    let allowed = list(&deny_toml(), "[licenses]", "allow");
    assert_eq!(
        allowed,
        [
            "MIT",
            "Apache-2.0",
            "BSD-2-Clause",
            "BSD-3-Clause",
            "ISC",
            "Zlib",
            "Unicode-3.0",
            "CC0-1.0",
            "BSL-1.0",
            "MPL-2.0",
            "OFL-1.1",
            "Ubuntu-font-1.0"
        ]
    );
    // GPL, AGPL and LGPL are denied by omission.
    assert!(allowed.iter().all(|l| !l.contains("GPL")), "{allowed:?}");
}

#[test]
fn deny_toml_allows_only_the_storytold_git_sources_and_the_forks_a_pull_request_waits_on() {
    let text = deny_toml();
    assert_eq!(
        list(&text, "[sources]", "allow-git"),
        [
            "https://github.com/storytold/craft-libs",
            // `craft-pathops` and the `linesweeper` it sweeps with, at one immutable revision of the one-bit fork, until storytold/craft-libs#677
            // merges: then both move to storytold/craft-libs and this source goes (and so does this line of the test).
            "https://github.com/one-bit/craft-libs",
            // `craft-print`'s Windows spooler calls, at immutable revisions of their own.
            "https://github.com/storytold/winprint.rs",
            // `rfd`, the native file dialogs, at one immutable revision of the one-bit fork, until the next rfd release holds its changes: then this source goes (and so does this line).
            "https://github.com/one-bit/rfd",
            "https://github.com/storytold/egui",
            "https://github.com/storytold/winit"
        ]
    );
    // The one licence outside the list is a certificate bundle's, for the one crate that brings it, and for nothing else.
    assert!(
        text.contains("[[licenses.exceptions]]\ncrate = \"webpki-root-certs\"\nallow = [\"CDLA-Permissive-2.0\"]"),
        "the exception for webpki-root-certs is not as the policy gives it"
    );
    assert_eq!(text.matches("[[licenses.exceptions]]").count(), 1, "no other crate has a licence of its own");
    assert!(text.contains("unknown-registry = \"deny\"") && text.contains("unknown-git = \"deny\""));
}

/// The `[advisories]` table of `deny.toml`, up to the next table.
fn advisories(text: &str) -> String {
    let after = &text[text.find("[advisories]").expect("no [advisories] in deny.toml")..];
    let end = after[1..].find("\n[").map_or(after.len(), |i| i + 1);
    after[..end].to_owned()
}

/// The `{ id = "...", reason = "..." }` entries of the `ignore` list of the `[advisories]` table: (id, reason).
fn ignored(text: &str) -> Vec<(String, String)> {
    let table = advisories(text);
    let body = &table[table.find("ignore = [").expect("no ignore list") + "ignore = [".len()..];
    let body = &body[..body.find("\n]").expect("the ignore list is not closed")];
    body.lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.contains("id = \""))
        .map(|l| {
            let quoted = |key: &str| {
                let at = l.find(key).unwrap_or_else(|| panic!("no {key} in {l}")) + key.len();
                l[at..].split('"').next().unwrap_or_default().to_owned()
            };
            (quoted("id = \""), quoted("reason = \""))
        })
        .collect()
}

#[test]
fn the_gate_checks_advisories_in_a_run_of_its_own_so_that_a_failed_fetch_can_be_told_from_a_finding() {
    assert_eq!(ADVISORY_ARGS, ["deny", "check", "advisories"]);
    // `--offline` is a flag of `cargo deny`, before the subcommand: it keeps the database from being fetched.
    assert_eq!(ADVISORY_OFFLINE_ARGS, ["deny", "--offline", "check", "advisories"]);
    assert!(!ARGS.contains(&"advisories"), "the licence run stays free of the network: {ARGS:?}");
}

/// Every unmaintained notice the gate lets through is named, with why and the day it was read, so it is a decision and not a habit.
#[test]
fn deny_toml_ignores_only_the_named_unmaintained_notices_each_with_a_dated_reason() {
    let found = ignored(&deny_toml());
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            // The three zbus crates, through egui's accessibility (Linux screen readers).
            "RUSTSEC-2026-0370",
            "RUSTSEC-2026-0371",
            "RUSTSEC-2026-0372",
            // `proc-macro-error`, through the print fork (Windows only).
            "RUSTSEC-2024-0370",
            // `chrono`: DiaCraft's own code uses jiff; the one left in the lock is a build dependency of the print fork's winprint (Windows).
            "RUSTSEC-2026-0543",
        ]
    );
    for (id, reason) in &found {
        assert!(reason.starts_with("2026-11-21: "), "{id}: the reason names the day it was read: {reason}");
        assert!(reason.len() > 60, "{id}: the reason says what blocks the fix: {reason}");
    }
}

/// A new unmaintained or unsound crate, direct or not, is a failure to look at: the defaults leave transitive unsound ones as notes.
#[test]
fn deny_toml_fails_on_unmaintained_and_unsound_crates_anywhere_in_the_tree() {
    let table = advisories(&deny_toml());
    assert!(table.contains("\nunmaintained = \"all\""), "{table}");
    assert!(table.contains("\nunsound = \"all\""), "{table}");
}

fn ran(success: bool, output: &str) -> Result<Run, String> {
    Ok(Run { success, output: output.to_owned() })
}

const FETCH_ERROR: &str = "2026-11-21 23:52:14 [ERROR] failed to fetch advisory database https://github.com/RustSec/advisory-db with cli: failed to fetch latest changes: [\"-C\", \"/home/u/.cargo/advisory-dbs/advisory-db-3157b0e258782691\", \"fetch\", \"--depth=1\"]\nfatal: unable to access 'https://github.com/RustSec/advisory-db/': Failed to connect to github.com:443 after 3 ms: Could not connect to server\n";
const VULNERABILITY: &str = "error[vulnerability]: Out-of-bounds Write in smallvec\n  ┌─ /w/Cargo.lock:12:1\n  │\n12 │ smallvec 1.6.0 registry+https://github.com/rust-lang/crates.io-index\n  │ ------------------------------------------------------------------- security vulnerability detected\n\nadvisories FAILED\n";
const NO_COPY: &str = "2026-11-21 23:53:40 [ERROR] failed to get 'FETCH_HEAD' metadata: failed to get HEAD timestamp\nfatal: cannot change to '/home/u/.cargo/advisory-dbs/advisory-db-3157b0e258782691': No such file or directory\n";

#[test]
fn a_clean_advisory_check_passes_with_one_run_and_no_notice() {
    let runs = RefCell::new(Vec::new());
    let said = RefCell::new(Vec::new());
    let out = advisory_gate(
        |offline| {
            runs.borrow_mut().push(offline);
            ran(true, "advisories ok\n")
        },
        |s| said.borrow_mut().push(s.to_owned()),
    );
    assert_eq!(out, Ok(Outcome::Ok));
    assert_eq!(*runs.borrow(), [false], "one run, with the database fetched");
    assert!(said.borrow().is_empty(), "{:?}", said.borrow());
}

/// The one thing the step exists for: a RustSec vulnerability fails the gate, and the run is not repeated against a cached copy
/// (which could only say the same, or less).
#[test]
fn a_vulnerability_fails_the_advisory_check_and_is_not_retried() {
    let runs = Cell::new(0);
    let err = advisory_gate(
        |offline| {
            runs.set(runs.get() + 1);
            assert!(!offline, "a finding is not a reason to run again");
            ran(false, VULNERABILITY)
        },
        |s| panic!("no notice for a finding: {s}"),
    )
    .unwrap_err();
    assert_eq!(runs.get(), 1);
    assert!(err.contains("RustSec advisory") && err.contains("deny.toml"), "{err}");
}

#[test]
fn a_failed_fetch_is_checked_against_the_copy_on_this_machine_and_the_notice_says_so() {
    let runs = RefCell::new(Vec::new());
    let said = RefCell::new(Vec::new());
    let out = advisory_gate(
        |offline| {
            runs.borrow_mut().push(offline);
            if offline { ran(true, "advisories ok\n") } else { ran(false, FETCH_ERROR) }
        },
        |s| said.borrow_mut().push(s.to_owned()),
    );
    assert_eq!(out, Ok(Outcome::Ok));
    assert_eq!(*runs.borrow(), [false, true]);
    let said = said.borrow().join("\n");
    assert!(said.contains("could not be fetched") && said.contains("copy"), "{said}");
    assert!(said.contains("not up to date") || said.contains("newer advisories"), "{said}");
}

/// A cached copy can hold a vulnerability too: that is a finding, not a missing database.
#[test]
fn a_vulnerability_in_the_copy_on_this_machine_fails_the_check_as_a_finding() {
    let err = advisory_gate(|offline| if offline { ran(false, VULNERABILITY) } else { ran(false, FETCH_ERROR) }, |_| {}).unwrap_err();
    assert!(err.contains("RustSec advisory") && err.contains("deny.toml"), "{err}");
}

#[test]
fn a_failed_fetch_and_no_copy_on_this_machine_fails_in_plain_words_and_names_the_way_out() {
    let err = advisory_gate(|offline| if offline { ran(false, NO_COPY) } else { ran(false, FETCH_ERROR) }, |s| panic!("no notice: {s}")).unwrap_err();
    assert!(err.contains("could not be fetched"), "{err}");
    assert!(err.contains("no usable copy"), "{err}");
    assert!(err.contains("cargo deny fetch"), "the way out is named: {err}");
}

/// Any other failure of the tool (a broken config, a crashed run) is not mistaken for a network problem and is not turned into a pass.
#[test]
fn a_tool_failure_that_is_not_the_network_fails_the_check_and_is_not_retried() {
    let err = advisory_gate(
        |offline| {
            assert!(!offline);
            ran(false, "error: failed to validate configuration file deny.toml\n")
        },
        |s| panic!("no notice: {s}"),
    )
    .unwrap_err();
    assert!(err.contains("could not run") || err.contains("failed"), "{err}");
    assert!(!err.contains("could not be fetched"), "{err}");
}

#[test]
fn a_run_that_cannot_start_is_reported_as_it_is() {
    let err = advisory_gate(|_| Err("cargo deny: failed to spawn: no such file".into()), |_| {}).unwrap_err();
    assert_eq!(err, "cargo deny: failed to spawn: no such file");
}

#[test]
fn a_failed_fetch_is_known_by_cargo_denys_words_and_a_finding_is_not_one() {
    assert!(fetch_failed(FETCH_ERROR));
    assert!(!fetch_failed(VULNERABILITY));
    assert!(!fetch_failed(NO_COPY));
    assert!(!fetch_failed(""));
}
