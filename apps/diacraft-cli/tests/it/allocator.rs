//! The command line's global allocator is mimalloc (owner 2026-11-21): the built binary says so when mimalloc is asked to be
//! verbose, so a change that dropped the `#[global_allocator]` (or put it behind the wrong `cfg`) fails here.

use std::process::Command;

use crate::BIN;

/// mimalloc prints its own start-up lines on standard error when `MIMALLOC_VERBOSE` is set; the default allocator prints nothing.
#[test]
fn the_binary_runs_on_mimalloc_when_it_is_asked_to_say_so() {
    let out = Command::new(BIN).arg("--version").env("MIMALLOC_VERBOSE", "1").env_remove("RUST_LOG").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("mimalloc"), "the binary was asked for mimalloc's words and said none: {stderr:?}");
    // The allocator changes nothing a person or a script reads: the version line is as it was.
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("diacraft-cli "), "{out:?}");
}

/// Without the variable mimalloc is silent, so the check above is about the allocator and not about the build.
#[test]
fn mimalloc_says_nothing_unless_it_is_asked() {
    let out = Command::new(BIN).arg("--version").env_remove("MIMALLOC_VERBOSE").env_remove("RUST_LOG").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("mimalloc"), "{out:?}");
}
