//! `packaging/smoke.sh` and the paths the workflows hand to the packaging scripts. The workflows give relative paths (`unpacked/diacraft-<version>-linux-x86_64`), and a script
//! that changes folder before it reads one finds nothing there: the release workflow's first run stopped on it, in every job that unpacks an archive. The programs here are
//! stand-ins that say nothing a smoke test could use, so the runs stop at the programs' versions, which is after the step these tests are about.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::say;

/// An unpacked download in `dir/unpacked/package`: the five licence files (the real ones: the script holds the third-party list to its size and words) and two stand-in
/// programs.
fn unpacked(dir: &TempDir) {
    let folder = dir.path().join("unpacked/package");
    std::fs::create_dir_all(&folder).unwrap();
    for file in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md", "THIRD-PARTY-LICENSES.txt"] {
        std::fs::copy(xtask::root().join(file), folder.join(file)).unwrap();
    }
    for program in ["diacraft", "diacraft-cli"] {
        std::fs::write(folder.join(program), "#!/bin/sh\necho stand-in\n").unwrap();
        std::fs::set_permissions(folder.join(program), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Runs `bash packaging/smoke.sh BIN` from `cwd`, with `SMOKE_PACKAGE_DIR` as given.
fn smoke(cwd: &Path, bin: &str, package_dir: Option<&str>) -> Output {
    let mut command = Command::new("bash");
    command.current_dir(cwd).env("PATH", "/usr/bin:/bin").env_remove("SMOKE_PACKAGE_DIR").arg(xtask::root().join("packaging/smoke.sh")).arg(bin);
    if let Some(folder) = package_dir {
        command.env("SMOKE_PACKAGE_DIR", folder);
    }
    command.output().unwrap()
}

#[test]
fn a_package_folder_given_by_a_relative_path_is_found_after_the_script_has_changed_folder() {
    let dir = TempDir::new("smoke-relative");
    unpacked(&dir);
    // As the workflow runs it: from the root of the checkout, with the unpacked folder named by a path below it.
    let out = smoke(dir.path(), "unpacked/package", Some("unpacked/package"));
    let said = say(&out);
    assert!(!said.contains("no such folder"), "{said}");
    assert!(said.contains("smoke: licence files in ") && said.contains("unpacked/package: ok"), "the licence files were looked at: {said}");
    // The stand-in's answer is no version: the run stops there, after the step that is tested.
    assert!(!out.status.success() && said.contains("unexpected output of diacraft-cli --version"), "{said}");
}

#[test]
fn an_absolute_package_folder_and_one_that_is_not_there_are_told_apart_in_the_same_words() {
    let dir = TempDir::new("smoke-absolute");
    unpacked(&dir);
    let absolute = dir.path().join("unpacked/package");
    let out = smoke(dir.path(), "unpacked/package", Some(absolute.to_str().unwrap()));
    assert!(say(&out).contains("smoke: licence files in ") && !say(&out).contains("no such folder"), "{}", say(&out));
    let out = smoke(dir.path(), "unpacked/package", Some("unpacked/missing"));
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("smoke: FAILED: no such folder: unpacked/missing"), "{}", say(&out));
    // No folder asked for: the licence step is left out, as on a bare build.
    let out = smoke(dir.path(), "unpacked/package", None);
    assert!(!say(&out).contains("licence files"), "{}", say(&out));
}
