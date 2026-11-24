//! `cargo xtask stats`: tests and lines of Rust per crate.

use xtask::stats::{count_tests, files_and_lines};

#[test]
fn counts_attributes_and_proptest_blocks() {
    let src = "#[test]\nfn a() {}\n// #[test] commented\n    #[test]\nfn b() {}\nproptest! {\n}\n";
    assert_eq!(count_tests(src), (2, 1));
}

/// A Kani harness file is not part of the crate that ships: its lines and files are not counted.
#[test]
fn a_kani_harness_file_is_not_counted() {
    let dir = crate::TempDir::new("stats-kani");
    dir.write("src/lib.rs", "pub fn a() {}\npub fn b() {}\n");
    dir.write("src/kani_proofs.rs", "#[kani::proof]\nfn p() {}\n#[kani::proof]\nfn q() {}\n");
    dir.write("src/other_proofs.rs", "fn r() {}\n");
    assert_eq!(files_and_lines(dir.path()), (2, 3), "lib.rs and other_proofs.rs only");
}
