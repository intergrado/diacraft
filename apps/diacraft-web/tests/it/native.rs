//! The binary built for the desktop: it cannot run the app, and says so instead of crashing.

use std::process::Command;

#[test]
fn the_desktop_build_says_it_only_runs_in_the_browser_and_fails() {
    let out = Command::new(env!("CARGO_BIN_EXE_diacraft-web")).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("only runs in the browser") && stderr.contains("trunk build"), "{stderr}");
    assert!(out.stdout.is_empty(), "{out:?}");
}
