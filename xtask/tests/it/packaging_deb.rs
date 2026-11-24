//! The .deb (`packaging/linux/make-deb.sh`): the CPU and the version it says, where it puts the files, and what it refuses. It needs `dpkg-deb`, which Debian and Ubuntu
//! have (the gate's runner is one); a machine without it skips these tests.

use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{elf, names, programs, say};

fn has_dpkg_deb() -> bool {
    let found = Command::new("dpkg-deb").arg("--version").output().is_ok();
    if !found {
        eprintln!("dpkg-deb is not here: the .deb is not made");
    }
    found
}

/// Runs `packaging/linux/make-deb.sh NAME OUT BIN`, with a plain `PATH` as the runners have.
fn make_deb(name: &str, out: &Path, bin: &Path) -> Output {
    Command::new("bash")
        .env("PATH", "/usr/bin:/bin")
        .args([xtask::root().join("packaging/linux/make-deb.sh").to_str().unwrap(), name])
        .arg(out)
        .arg(bin)
        .output()
        .unwrap()
}

/// A field of the control file of a .deb.
fn field(deb: &Path, field: &str) -> String {
    let out = Command::new("dpkg-deb").arg("--field").arg(deb).arg(field).output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

#[test]
fn a_deb_says_the_cpu_and_the_version_of_its_programs() {
    if !has_dpkg_deb() {
        return;
    }
    for (machine, arch, deb_arch, glibc) in [(0x3e, "x86_64", "amd64", "libc6 (>= 2.35)"), (0xb7, "arm64", "arm64", "libc6 (>= 2.35)")] {
        for (name, version) in [("v0.2.0", "0.2.0"), ("v0.2.0-rc1", "0.2.0~rc1"), ("v0.2.0-rc.2-x", "0.2.0~rc.2.x")] {
            let dir = TempDir::new("make-deb");
            programs(&dir, &elf(machine), &elf(machine), "");
            let out = make_deb(name, &dir.path().join("out"), &dir.path().join("bin"));
            assert!(out.status.success(), "{arch} {name}: {}", say(&out));
            let file = format!("diacraft-{name}-linux-{arch}.deb");
            assert_eq!(names(&dir.path().join("out")), [file.as_str()]);
            let deb = dir.path().join("out").join(&file);
            assert_eq!(field(&deb, "Architecture"), deb_arch, "{arch}");
            assert_eq!(field(&deb, "Version"), version, "{name}");
            let depends = field(&deb, "Depends");
            assert!(depends.contains(glibc), "{arch}: {depends}");
            assert!(depends.contains("libxkbcommon0"), "both window systems need the keyboard library: {depends}");
        }
    }
}

#[test]
fn a_label_that_is_no_tag_gives_a_version_that_sorts_before_the_release_it_leads_to() {
    if !has_dpkg_deb() {
        return;
    }
    let dir = TempDir::new("make-deb-label");
    programs(&dir, &elf(0x3e), &elf(0x3e), "");
    let out = make_deb("dev-1a2b3c4", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{}", say(&out));
    let version = field(&dir.path().join("out").join("diacraft-dev-1a2b3c4-linux-x86_64.deb"), "Version");
    assert!(version.ends_with("~dev.1a2b3c4") && version.starts_with(|c: char| c.is_ascii_digit()), "{version}");
}

#[test]
fn a_deb_puts_the_programs_the_entry_the_icon_and_the_licences_where_the_system_looks() {
    if !has_dpkg_deb() {
        return;
    }
    let dir = TempDir::new("make-deb-files");
    programs(&dir, &elf(0x3e), &elf(0x3e), "");
    let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{}", say(&out));
    let deb = dir.path().join("out").join("diacraft-v0.2.0-linux-x86_64.deb");
    let listing = Command::new("dpkg-deb").arg("--contents").arg(&deb).output().unwrap();
    let listing = String::from_utf8(listing.stdout).unwrap();
    let paths: Vec<&str> = listing.lines().filter_map(|l| l.split_whitespace().last()).collect();
    for path in [
        "./usr/bin/diacraft",
        "./usr/bin/diacraft-cli",
        "./usr/share/applications/app.diacraft.desktop",
        "./usr/share/icons/hicolor/256x256/apps/app.diacraft.png",
        "./usr/share/doc/diacraft/LICENSE-MIT",
        "./usr/share/doc/diacraft/LICENSE-APACHE",
        "./usr/share/doc/diacraft/NOTICE",
        "./usr/share/doc/diacraft/ATTRIBUTION.md",
        "./usr/share/doc/diacraft/THIRD-PARTY-LICENSES.txt",
        "./usr/share/doc/diacraft/copyright",
    ] {
        assert!(paths.contains(&path), "{path} is not in the package: {paths:?}");
    }
    assert!(listing.lines().all(|l| l.contains("root/root")), "every file belongs to root, whoever made the package: {listing}");
}

#[test]
fn a_deb_is_refused_for_programs_that_name_a_library_it_does_not_cover() {
    if !has_dpkg_deb() {
        return;
    }
    let dir = TempDir::new("make-deb-library");
    let mut program = elf(0x3e);
    program.extend(b"libnotknown.so.9\0");
    programs(&dir, &program, &elf(0x3e), "");
    let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("diacraft names the library libnotknown.so.9, which this script does not know"), "{}", say(&out));
}
