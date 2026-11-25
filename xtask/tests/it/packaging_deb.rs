//! The .deb (`packaging/linux/make-deb.sh`): the CPU and the version it says, where it puts the files, and what it refuses. It needs `dpkg-deb`, which Debian and Ubuntu
//! have (the gate's runner is one); a machine without it skips these tests.

use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{elf, names, programs, say};

/// A program of CPU `machine` (0x3e is x86-64, 0xb7 is AArch64) for the package. `make-deb.sh` strips a program of the CPU it runs on with the machine's `strip`, which
/// reads only a real program: so for this machine's own CPU it is a copy of `true`, a real and small one, and for the other CPU the first bytes of one, which the script
/// leaves as it is, since this machine's `strip` cannot read it (a runner with `binutils` found this out; a machine without `strip` did not).
fn program(machine: u8) -> Vec<u8> {
    let own = match std::env::consts::ARCH {
        "x86_64" => 0x3e,
        "aarch64" => 0xb7,
        _ => 0,
    };
    if machine == own {
        for path in ["/bin/true", "/usr/bin/true"] {
            if let Ok(bytes) = std::fs::read(path)
                && bytes.starts_with(b"\x7fELF")
                && bytes.get(18) == Some(&machine)
            {
                return bytes;
            }
        }
    }
    elf(machine)
}

fn has_strip() -> bool {
    Command::new("strip").arg("--version").output().is_ok()
}

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
            programs(&dir, &program(machine), &program(machine), "");
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
    programs(&dir, &program(0x3e), &program(0x3e), "");
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
    programs(&dir, &program(0x3e), &program(0x3e), "");
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

/// The programs of the package are stripped when they are of this machine's CPU: a real program is, and the size shows it. A file that says it is of this machine's CPU and
/// that `strip` cannot read stops the script with the program's name, and a program of the other CPU is packaged as it is (`program` above says why).
#[test]
fn a_program_of_this_cpu_is_stripped_one_strip_refuses_stops_the_script_and_one_of_the_other_cpu_is_left_alone() {
    if !has_dpkg_deb() || !has_strip() {
        eprintln!("dpkg-deb or strip is not here");
        return;
    }
    let (own, other) = match std::env::consts::ARCH {
        "x86_64" => (0x3e, 0xb7),
        "aarch64" => (0xb7, 0x3e),
        _ => return,
    };
    // This CPU's, real: made.
    let dir = TempDir::new("make-deb-own");
    programs(&dir, &program(own), &program(own), "");
    let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{}", say(&out));
    // The other CPU's, as the first bytes of a program: made, as it is.
    let dir = TempDir::new("make-deb-other");
    programs(&dir, &elf(other), &elf(other), "");
    let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{}", say(&out));
    // This CPU's, and no program: strip refuses it, and the script says whose.
    let dir = TempDir::new("make-deb-refused");
    programs(&dir, &elf(own), &elf(own), "");
    let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("strip could not read diacraft"), "{}", say(&out));
    assert!(!dir.path().join("out").join("diacraft-v0.2.0-linux-x86_64.deb").exists());
}

/// The libraries the programs load by name when they start, each with the package that holds it, as `make-deb.sh` knows them. The names are the ones the crates that load
/// them hold (`x11rb` loads `libxcb.so.1`, `ash` `libvulkan.so.1`, `winit`'s `x11-dl` the X libraries, `wayland-sys` the Wayland ones, and so on). Whether a program's
/// build shows its names to a scan of its bytes is up to the compiler: a short name can be written into the code a few bytes at a time, as the x86_64 build does for
/// `libxcb.so.1` and the ARM64 build does not, and the release run of the ARM64 `.deb` found the name when the x86_64 one had not. Every name is accepted, and its
/// package is in the package's `Depends` or `Recommends`.
const LIBRARIES: [(&str, &str); 14] = [
    ("libc.so.6", "libc6"),
    ("libm.so.6", "libc6"),
    ("libgcc_s.so.1", "libgcc-s1"),
    ("libxkbcommon.so.0", "libxkbcommon0"),
    ("libxkbcommon-x11.so.0", "libxkbcommon-x11-0"),
    ("libwayland-client.so.0", "libwayland-client0"),
    ("libwayland-egl.so.1", "libwayland-egl1"),
    ("libX11.so.6", "libx11-6"),
    ("libX11-xcb.so.1", "libx11-xcb1"),
    ("libxcb.so.1", "libxcb1"),
    ("libXcursor.so.1", "libxcursor1"),
    ("libXi.so.6", "libxi6"),
    ("libEGL.so.1", "libegl1"),
    ("libvulkan.so.1", "libvulkan1"),
];

#[test]
fn every_library_the_programs_load_by_name_is_known_with_its_package_and_libxcb_is_a_dependency() {
    if !has_dpkg_deb() {
        return;
    }
    // A stand-in of the CPU this machine is not: the script leaves it as it is, and a scan of its bytes finds the names written in it, as the scan of the ARM64 build did.
    let other = match std::env::consts::ARCH {
        "x86_64" => 0xb7,
        _ => 0x3e,
    };
    let arch = if other == 0xb7 { "arm64" } else { "x86_64" };
    for (library, package) in LIBRARIES.into_iter().chain([("libdbus-1.so.3", "libdbus-1-3")]) {
        let dir = TempDir::new("make-deb-libraries");
        let mut named = elf(other);
        named.extend(format!("{library}\0").bytes());
        programs(&dir, &named, &named, "");
        let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
        assert!(out.status.success(), "{library}: {}", say(&out));
        let deb = dir.path().join("out").join(format!("diacraft-v0.2.0-linux-{arch}.deb"));
        let wanted = format!("{} {}", field(&deb, "Depends"), field(&deb, "Recommends"));
        let words: Vec<&str> = wanted.split(|c: char| !(c.is_ascii_alphanumeric() || "+-.".contains(c))).collect();
        assert!(words.contains(&package), "{library}: {package} is neither depended on nor recommended: {wanted}");
    }
    // `x11rb` loads libxcb when the program opens a window on an X server, and a program that cannot is no window program: the package depends on it (it is small, and
    // `libx11-6` depends on it anyway).
    let depends = {
        let dir = TempDir::new("make-deb-xcb");
        programs(&dir, &elf(other), &elf(other), "");
        let out = make_deb("v0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
        assert!(out.status.success(), "{}", say(&out));
        field(&dir.path().join("out").join(format!("diacraft-v0.2.0-linux-{arch}.deb")), "Depends")
    };
    assert!(depends.split(", ").any(|d| d == "libxcb1"), "Depends: {depends}");
}
