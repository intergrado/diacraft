//! `xtask::repo_files`: the file list every gate scans must not silently lose a file.

use xtask::repo_files;

use crate::{git, git_repo as repo};

#[test]
fn non_ascii_and_spaced_names_are_listed() {
    let dir = repo("files-names");
    for name in ["assets/naïve icon.svg", "assets/日本語.png", "docs/images/with space.png", "plain.txt"] {
        dir.write(name, "x");
    }
    let mut found = repo_files(dir.path()).unwrap();
    found.sort();
    assert_eq!(found, ["assets/naïve icon.svg", "assets/日本語.png", "docs/images/with space.png", "plain.txt"]);
}

/// Git quotes these in its default output; `-z` does not.
#[cfg(unix)]
#[test]
fn quotes_tabs_and_newlines_in_names_are_listed() {
    let dir = repo("files-special");
    for name in ["assets/quo\"te.svg", "assets/tab\there.png", "assets/new\nline.png"] {
        dir.write(name, "x");
    }
    let mut found = repo_files(dir.path()).unwrap();
    found.sort();
    assert_eq!(found, ["assets/new\nline.png", "assets/quo\"te.svg", "assets/tab\there.png"]);
}

#[test]
fn tracked_and_untracked_files_are_listed_and_ignored_ones_are_not() {
    let dir = repo("files-kinds");
    dir.write(".gitignore", "ignored.txt\n");
    dir.write("tracked é.txt", "x");
    git(dir.path(), &["add", "tracked é.txt"]);
    dir.write("untracked.txt", "x");
    dir.write("ignored.txt", "x");
    let mut found = repo_files(dir.path()).unwrap();
    found.sort();
    assert_eq!(found, [".gitignore", "tracked é.txt", "untracked.txt"]);
}

#[test]
fn a_deleted_tracked_file_is_not_listed_and_not_an_error() {
    let dir = repo("files-deleted");
    dir.write("gone é.txt", "x");
    dir.write("kept.txt", "x");
    git(dir.path(), &["add", "."]);
    std::fs::remove_file(dir.path().join("gone é.txt")).unwrap();
    assert_eq!(repo_files(dir.path()).unwrap(), ["kept.txt"]);
}

#[test]
fn a_dangling_symlink_is_listed_not_dropped() {
    let dir = repo("files-link");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("nowhere", dir.path().join("dangling.svg")).unwrap();
        assert_eq!(repo_files(dir.path()).unwrap(), ["dangling.svg"]);
    }
    #[cfg(not(unix))]
    drop(dir);
}

// Only Linux file systems take any byte in a name.
#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_is_an_error() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = repo("files-utf8");
    std::fs::write(dir.path().join(OsStr::from_bytes(b"bad\xff.png")), "x").unwrap();
    let err = repo_files(dir.path()).unwrap_err();
    assert!(err.contains("not valid UTF-8") && err.contains("bad"), "{err}");
}

#[test]
fn a_directory_that_does_not_exist_is_an_error() {
    let dir = crate::TempDir::new("files-missing");
    assert!(repo_files(&dir.path().join("missing")).is_err());
}
