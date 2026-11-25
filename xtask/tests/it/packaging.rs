//! The scripts that make the downloads (`packaging/`): what a download is named after, and what they refuse. The programs here are not programs: each is the
//! first bytes of a Linux or a Windows one, which is all the script reads to say what CPU they are for.

use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;

/// The start of a 64-bit little endian ELF program for the machine `machine` (0x3e is x86-64, 0xb7 is AArch64).
pub(crate) fn elf(machine: u8) -> Vec<u8> {
    let mut bytes = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0];
    bytes.extend([0; 10]);
    bytes.extend([machine, 0]);
    bytes.extend([0; 40]);
    bytes
}

/// The start of a Windows program for the machine `machine` (0x8664 is x64, 0xaa64 is ARM64): "MZ", the offset 0x80 of the PE header at 0x3c, and the header.
pub(crate) fn pe(machine: u16) -> Vec<u8> {
    let mut bytes = vec![0; 0x90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    bytes
}

/// Runs `packaging/package.sh PLATFORM dev-test OUT BIN` and gives what it printed. A plain `PATH`: a machine's own `od` or `tar` ahead of the system's is not what
/// the runners have.
fn package(platform: &str, out: &Path, bin: &Path) -> Output {
    package_named(platform, "dev-test", out, bin)
}

/// The same for the name `name`.
fn package_named(platform: &str, name: &str, out: &Path, bin: &Path) -> Output {
    Command::new("bash")
        .env("PATH", "/usr/bin:/bin")
        .args([xtask::root().join("packaging/package.sh").to_str().unwrap(), platform, name])
        .arg(out)
        .arg(bin)
        .output()
        .unwrap()
}

pub(crate) fn say(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// The files of a folder, by name.
pub(crate) fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

pub(crate) fn programs(dir: &TempDir, program: &[u8], cli: &[u8], exe: &str) {
    dir.write_bytes(&format!("bin/diacraft{exe}"), program);
    dir.write_bytes(&format!("bin/diacraft-cli{exe}"), cli);
}

#[test]
fn a_linux_download_is_named_for_the_cpu_of_its_programs_and_says_what_it_needs() {
    // The file name says aarch64 for the ARM build, the word of the Linux kernel and the launchers; the README says the CPU in its own words.
    for (machine, arch, in_names, cpu, glibc) in
        [(0x3e, "x86_64", "x86_64", "Intel or AMD", "glibc 2.35"), (0xb7, "arm64", "aarch64", "ARM", "glibc 2.35")]
    {
        let dir = TempDir::new("package-linux");
        programs(&dir, &elf(machine), &elf(machine), "");
        let out = package("linux", &dir.path().join("out"), &dir.path().join("bin"));
        assert!(out.status.success(), "{arch}: {}", say(&out));
        let archive = format!("diacraft-dev-test-linux-{in_names}.tar.gz");
        assert_eq!(names(&dir.path().join("out")), [archive.as_str()], "the machine of the build is not what names it");
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&archive), "the last line is the archive: {}", say(&out));
        let readme = Command::new("tar")
            .arg("-xzOf")
            .arg(dir.path().join("out").join(&archive))
            .arg(format!("diacraft-dev-test-linux-{in_names}/README.txt"))
            .output()
            .unwrap();
        let readme = String::from_utf8(readme.stdout).unwrap();
        assert!(readme.contains(&format!("for Linux ({arch}: 64-bit {cpu})")), "{arch}: {readme}");
        assert!(readme.contains(glibc) && !readme.contains('@'), "{arch}: {readme}");
    }
}

#[test]
fn a_windows_download_is_named_for_the_cpu_of_its_programs() {
    if Command::new("zip").arg("-v").output().is_err() {
        eprintln!("zip is not here: the Windows archive is not made");
        return;
    }
    // The x64 build is `x64` in a file name and the ARM build `arm64`; the zip is `-portable`, which says that it needs no installer, and holds a folder named as the other
    // downloads are.
    for (machine, arch, in_names, cpu) in [(0x8664, "x86_64", "x64", "Intel or AMD"), (0xaa64, "arm64", "arm64", "ARM")] {
        let dir = TempDir::new("package-windows");
        programs(&dir, &pe(machine), &pe(machine), ".exe");
        let out = package("windows", &dir.path().join("out"), &dir.path().join("bin"));
        assert!(out.status.success(), "{arch}: {}", say(&out));
        let archive = format!("diacraft-dev-test-windows-{in_names}-portable.zip");
        assert_eq!(names(&dir.path().join("out")), [archive.as_str()]);
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&archive), "the last line is the archive: {}", say(&out));
        let listing = Command::new("unzip")
            .arg("-p")
            .arg(dir.path().join("out").join(&archive))
            .arg(format!("diacraft-dev-test-windows-{in_names}/README.txt"))
            .output();
        if let Ok(listing) = listing {
            let readme = String::from_utf8_lossy(&listing.stdout);
            assert!(readme.contains(&format!("for Windows ({arch}: 64-bit {cpu})")), "{arch}: {readme}");
            // The README names the two files of the release as they are named there.
            assert!(readme.contains(&format!("diacraft-dev-test-windows-{in_names}-portable.zip")) && !readme.contains('@'), "{arch}: {readme}");
            assert!(readme.contains(&format!("diacraft-dev-test-windows-{in_names}.msi")), "{arch}: {readme}");
        }
    }
}

/// What the launchers read is `diacraft-<version>-...` with a version that starts with a digit, so the tag's `v` is in no file name and not in what a download says of its own name
/// The workflow gives the version, and the script leaves the `v` out of a tag it is given all the same. A label that is no tag is kept as it is.
#[test]
fn the_v_of_a_tag_is_in_no_file_name_and_a_label_is_kept() {
    for (name, said) in [("v0.2.1", "0.2.1"), ("0.2.1", "0.2.1"), ("v0.3.0-rc.1", "0.3.0-rc.1"), ("dev-1a2b3c4", "dev-1a2b3c4"), ("vtest", "vtest")] {
        let dir = TempDir::new("package-v");
        programs(&dir, &elf(0xb7), &elf(0xb7), "");
        let out = package_named("linux", name, &dir.path().join("out"), &dir.path().join("bin"));
        assert!(out.status.success(), "{name}: {}", say(&out));
        let archive = format!("diacraft-{said}-linux-aarch64.tar.gz");
        assert_eq!(names(&dir.path().join("out")), [archive.as_str()], "{name}");
        let readme = Command::new("tar")
            .arg("-xzOf")
            .arg(dir.path().join("out").join(&archive))
            .arg(format!("diacraft-{said}-linux-aarch64/README.txt"))
            .output()
            .unwrap();
        let readme = String::from_utf8(readme.stdout).unwrap();
        assert!(readme.starts_with(&format!("DiaCraft {said} for Linux")), "{name}: {readme}");
    }
}

#[test]
fn programs_of_two_kinds_are_not_one_download() {
    let dir = TempDir::new("package-mixed");
    programs(&dir, &elf(0x3e), &elf(0xb7), "");
    let out = package("linux", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("diacraft is built for x86_64 and diacraft-cli for arm64: one download holds one architecture"), "{}", say(&out));
    assert!(!dir.path().join("out").exists() || names(&dir.path().join("out")).is_empty(), "nothing is written for a refused download");
}

#[test]
fn a_file_that_is_not_a_program_of_the_platform_is_refused_by_name() {
    let dir = TempDir::new("package-not-programs");
    // A Windows program in a Linux download, a Linux program in a Windows one, a 32-bit ELF, an ELF of another machine, a PE of another machine, and a text file.
    let mut elf32 = elf(0x3e);
    elf32[4] = 1;
    for (platform, exe, bytes, said) in [
        ("linux", "", pe(0x8664), "is not a 64-bit little endian ELF program"),
        ("linux", "", elf32, "is not a 64-bit little endian ELF program"),
        ("linux", "", elf(0x28), "ELF machine 0x2800, which is neither x86-64 nor AArch64"),
        ("linux", "", b"#!/bin/sh\necho hi\n".to_vec(), "is not a 64-bit little endian ELF program"),
        ("windows", ".exe", elf(0x3e), "is not a Windows program"),
        ("windows", ".exe", pe(0x14c), "PE machine 0x4c01, which is neither x64 nor ARM64"),
        ("windows", ".exe", b"MZ".to_vec(), "is not a Windows program"),
    ] {
        programs(&dir, &bytes, &bytes, exe);
        let out = package(platform, &dir.path().join("out"), &dir.path().join("bin"));
        assert!(!out.status.success(), "{platform}: {}", say(&out));
        assert!(say(&out).contains(said), "{platform}: {}", say(&out));
    }
}

/// Every target the release workflow builds a download for is one the file of third-party notices covers: the notices of a CPU's builds are the crates that CPU's
/// build links (`windows_aarch64_msvc` where x64 has `windows_x86_64_msvc`), so a download of a CPU the file does not name would carry a list that may miss a crate.
#[test]
fn the_notices_cover_every_target_the_release_builds_a_download_for() {
    use xtask::licenses::policy::OUTPUTS;
    let workflow = std::fs::read_to_string(xtask::root().join(".github/workflows/release.yml")).unwrap();
    let mut built: Vec<String> = Vec::new();
    for line in workflow.lines() {
        let line = line.trim().trim_start_matches("- ");
        for word in line.split_whitespace().collect::<Vec<_>>().windows(2) {
            let triple = match word {
                ["target:", triple] | ["--target", triple] => triple.trim_matches('"'),
                _ => continue,
            };
            if triple.contains("-unknown-linux-gnu") || triple.contains("-pc-windows-msvc") || triple.contains("-apple-darwin") {
                built.push(triple.to_owned());
            }
        }
    }
    built.sort();
    built.dedup();
    assert!(built.len() >= 5, "the workflow's targets were not found: {built:?}");
    for triple in &built {
        assert!(
            OUTPUTS[0].triples.contains(&triple.as_str()),
            "the release builds {triple}, and THIRD-PARTY-LICENSES.txt does not cover it ({:?})",
            OUTPUTS[0].triples
        );
    }
}

/// The owner's ruling (2026-11-24): the ARM64 Linux download runs where the x86_64 one does, on glibc 2.35, so both are built on the oldest Ubuntu image GitHub offers for
/// their CPU, and the site, the README and the packages say one figure.
#[test]
fn both_linux_cpus_are_built_on_the_oldest_image_and_every_text_says_one_glibc() {
    let workflow = std::fs::read_to_string(xtask::root().join(".github/workflows/release.yml")).unwrap();
    let runners: Vec<&str> = workflow.lines().map(str::trim).filter_map(|l| l.strip_prefix("runner: ")).filter(|r| r.starts_with("ubuntu")).collect();
    assert_eq!(runners, ["ubuntu-22.04", "ubuntu-22.04-arm"], "the Linux job's matrix");
    for rel in ["README.md", "site/index.html", "packaging/common.sh"] {
        let text = std::fs::read_to_string(xtask::root().join(rel)).unwrap();
        assert!(text.contains("2.35"), "{rel} says glibc 2.35");
        assert!(!text.contains("2.39"), "{rel} says no other glibc");
    }
}
