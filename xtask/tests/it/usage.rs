//! `cargo xtask help`: the usage text says what each gate does now.

use std::process::Command;

/// The text `xtask help` prints.
pub(crate) fn usage() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask")).arg("help").output().unwrap();
    assert!(out.status.success(), "xtask help failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// The lines of the entry for `command`: its own line and the indented lines that continue it.
pub(crate) fn entry(usage: &str, command: &str) -> String {
    let mut lines = usage.lines().skip_while(|l| !l.trim_start().starts_with(command) || !l.starts_with("  "));
    let mut text = lines.next().unwrap_or_default().to_owned();
    for line in lines.take_while(|l| l.starts_with("                  ")) {
        text.push(' ');
        text.push_str(line.trim());
    }
    text
}

/// `assets` checks every asset file by its extension, wherever it is, and the usage said it looked in two folders only.
#[test]
fn the_assets_line_names_the_extensions_and_not_only_the_two_folders() {
    let text = entry(&usage(), "assets");
    assert!(text.contains("extension"), "the usage of `assets` does not mention extensions: {text}");
    assert!(text.contains("ATTRIBUTION.md"), "{text}");
}

/// `parity --check` fails, and does not skip, when the checklist is missing (it has been committed).
#[test]
fn the_parity_line_does_not_say_it_skips() {
    let text = entry(&usage(), "parity");
    assert!(text.contains("--check"), "{text}");
    assert!(!text.contains("skips") && !text.contains("does not exist yet"), "the usage of `parity` still describes the old skip: {text}");
}

/// `brands` also keeps another project's names out of every file, and the usage said it reads user-visible text only.
#[test]
fn the_brands_line_says_it_reads_every_file_for_another_projects_names() {
    let text = entry(&usage(), "brands");
    assert!(text.contains("any file") && text.contains("another project"), "the usage of `brands` does not say it reads every file: {text}");
}

/// A helper that finds nothing would make the tests above pass on an empty string.
#[test]
fn the_entry_helper_finds_what_it_is_asked_for() {
    let text = entry("usage\n\ncommands:\n  one   first\n                  more of first\n  two   second\n", "one");
    assert_eq!(text, "  one   first more of first");
    assert_eq!(entry("nothing here\n", "assets"), "");
}
