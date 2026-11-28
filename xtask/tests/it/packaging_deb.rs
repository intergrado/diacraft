//! The .deb (`packaging/linux/make-deb.sh`): the CPU and the version it says, where it puts the files, and what it refuses. The tests that read the package need `dpkg-deb`, which
//! Debian and Ubuntu have (the gate's runner is one); a machine without it skips them. What the script does with the completions and the manual pages is also looked at with a
//! stand-in `dpkg-deb` that lists the files it is given, on every machine.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{elf, names, program, programs, say, stand_in_cli, write_script};

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

/// The CPU of this machine in the two words the tests use: the ELF machine and the name of the CPU in a file name. None for a CPU the downloads are not made for.
fn own() -> Option<(u8, &'static str)> {
    match std::env::consts::ARCH {
        "x86_64" => Some((0x3e, "x86_64")),
        "aarch64" => Some((0xb7, "aarch64")),
        _ => None,
    }
}

/// What `diacraft-cli man --list` prints for the stand-in command line.
const PAGES: &str = "diacraft-cli\\ndiacraft-cli-convert\\ndiacraft\\n";

/// What the stand-in command line writes into the package, where bash, zsh and fish look for the completions of an installed program, and `man` for its pages.
const EXTRAS: [&str; 9] = [
    "./usr/share/bash-completion/completions/diacraft",
    "./usr/share/bash-completion/completions/diacraft-cli",
    "./usr/share/zsh/vendor-completions/_diacraft",
    "./usr/share/zsh/vendor-completions/_diacraft-cli",
    "./usr/share/fish/vendor_completions.d/diacraft.fish",
    "./usr/share/fish/vendor_completions.d/diacraft-cli.fish",
    "./usr/share/man/man1/diacraft.1.gz",
    "./usr/share/man/man1/diacraft-cli.1.gz",
    "./usr/share/man/man1/diacraft-cli-convert.1.gz",
];

/// Runs `packaging/linux/make-deb.sh NAME OUT BIN` on the programs of `dir` (`out` and `bin` in it), with `path` as the `PATH` and `env` added.
fn run_make_deb(path: &str, env: &[(&str, &str)], name: &str, dir: &TempDir) -> Output {
    Command::new("bash")
        .env("PATH", path)
        .envs(env.iter().copied())
        .arg(xtask::root().join("packaging/linux/make-deb.sh"))
        .arg(name)
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"))
        .output()
        .unwrap()
}

/// Runs the script with a plain `PATH` as the runners have, and the stand-in command line for the completions and the pages as `EXTRAS_CLI`. The programs of a test are no programs, or
/// copies of `true` that have no right to run, so the script asks the stand-in whatever CPU they are of, and a test does not depend on what the machine can run (the runner of the
/// gate has the CPU of the programs here and `dpkg-deb`, and a Fedora machine has neither).
fn make_deb(dir: &TempDir, name: &str) -> Output {
    let cli = stand_in_cli(dir, PAGES);
    run_make_deb("/usr/bin:/bin", &[("EXTRAS_CLI", &cli)], name, dir)
}

/// Runs the script with a `dpkg-deb` that is none first on the `PATH`: it writes the list of the files of the tree it is asked to build (`--build TREE FILE`) as the package, one for each
/// line, so that what the script puts in the package is read on a machine that has no dpkg.
fn make_deb_listing(dir: &TempDir, name: &str, env: &[(&str, &str)]) -> Output {
    write_script(
        dir,
        "tools/dpkg-deb",
        "#!/bin/sh\ntree=\nfile=\nfor arg in \"$@\"; do tree=$file; file=$arg; done\n(cd \"$tree\" && find . -type f | sort) >\"$file\"\n",
    );
    run_make_deb(&format!("{}:/usr/bin:/bin", dir.path().join("tools").display()), env, name, dir)
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
    // The ARM build is aarch64 in the file's name and arm64 in the package's own architecture field. A name is the version, with no `v`, and a tag given all the
    // same is named as its version.
    for (machine, arch, in_names, deb_arch, glibc) in
        [(0x3e, "x86_64", "x86_64", "amd64", "libc6 (>= 2.35)"), (0xb7, "arm64", "aarch64", "arm64", "libc6 (>= 2.35)")]
    {
        for (name, named, version) in [
            ("0.2.0", "0.2.0", "0.2.0"),
            ("0.2.0-rc1", "0.2.0-rc1", "0.2.0~rc1"),
            ("0.2.0-rc.2-x", "0.2.0-rc.2-x", "0.2.0~rc.2.x"),
            ("v0.2.0-rc1", "0.2.0-rc1", "0.2.0~rc1"),
        ] {
            let dir = TempDir::new("make-deb");
            programs(&dir, &program(machine), &program(machine), "");
            let out = make_deb(&dir, name);
            assert!(out.status.success(), "{arch} {name}: {}", say(&out));
            let file = format!("diacraft-{named}-linux-{in_names}.deb");
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

/// What the file is called, with a stand-in for `dpkg-deb`, so that the name is checked on a machine that has no dpkg (the programs are those of the CPU this machine is not, which the
/// script leaves as they are): the version with no `v`, and aarch64 or x86_64 for the CPU. A program of the other CPU cannot be asked for the completions and the manual
/// pages, and the script makes the package without them and says so.
#[test]
fn a_deb_is_named_for_the_version_and_the_cpu_with_no_dpkg_on_the_machine() {
    let (other, in_names) = match std::env::consts::ARCH {
        "x86_64" => (0xb7, "aarch64"),
        _ => (0x3e, "x86_64"),
    };
    for (name, said) in [("0.2.1", "0.2.1"), ("v0.2.1", "0.2.1"), ("v0.3.0-rc.1", "0.3.0-rc.1"), ("dev-1a2b3c4", "dev-1a2b3c4")] {
        let dir = TempDir::new("make-deb-name");
        programs(&dir, &elf(other), &elf(other), "");
        let out = make_deb_listing(&dir, name, &[]);
        assert!(out.status.success(), "{name}: {}", say(&out));
        let file = format!("diacraft-{said}-linux-{in_names}.deb");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()], "{name}");
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&file), "the last line is the file: {}", say(&out));
        let files = std::fs::read_to_string(dir.path().join("out").join(&file)).unwrap();
        assert!(files.contains("./usr/bin/diacraft-cli") && !files.contains("completion") && !files.contains("/man/"), "{name}: {files}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("the package is made without shell completions and manual pages"),
            "{name}: {}",
            say(&out)
        );
    }
}

/// The two ways the script goes for the completions and the manual pages, on every machine (the stand-in `dpkg-deb` lists the package): a command line that runs writes them into the folders
/// where bash, zsh, fish and `man` look for those of an installed program (`EXTRAS_CLI` is the stand-in for the program of this machine's CPU that runs, which the programs of the tests
/// are not), and one that cannot write them, or writes nothing, is no build to package: the script stops, and says so.
#[test]
fn a_command_line_that_runs_has_its_completions_and_pages_in_the_package_and_one_that_cannot_stops_the_script() {
    let Some((own, in_names)) = own() else {
        return;
    };
    // The command line of the stand-in: nine files.
    let dir = TempDir::new("make-deb-extras");
    programs(&dir, &program(own), &program(own), "");
    let cli = stand_in_cli(&dir, PAGES);
    let out = make_deb_listing(&dir, "0.2.0", &[("EXTRAS_CLI", &cli)]);
    assert!(out.status.success(), "{}", say(&out));
    let files = std::fs::read_to_string(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{in_names}.deb"))).unwrap();
    for path in EXTRAS {
        assert!(files.lines().any(|f| f == path), "{path} is not in the package: {files}");
    }
    assert!(!say(&out).contains("without shell completions"), "{}", say(&out));
    // The programs of this machine's CPU as the other tests write them, with no stand-in: a file with no right to run (what the runner of the gate found), and one that runs and prints
    // nothing (a copy of `true`, once it may run). Neither writes completions, so the package is not made.
    if program(own) == elf(own) {
        return;
    }
    for (mode, said) in [(0o644, "diacraft-cli could not write the bash completions of diacraft-cli"), (0o755, "the package is missing")] {
        let dir = TempDir::new("make-deb-no-cli");
        programs(&dir, &program(own), &program(own), "");
        std::fs::set_permissions(dir.path().join("bin/diacraft-cli"), std::fs::Permissions::from_mode(mode)).unwrap();
        let out = make_deb_listing(&dir, "0.2.0", &[]);
        assert!(!out.status.success(), "{mode:o}: {}", say(&out));
        assert!(say(&out).contains(said), "{mode:o}: {}", say(&out));
        assert!(!dir.path().join("out").exists() || names(&dir.path().join("out")).is_empty(), "{mode:o}: no package is written");
    }
}

#[test]
fn a_label_that_is_no_tag_gives_a_version_that_sorts_before_the_release_it_leads_to() {
    if !has_dpkg_deb() {
        return;
    }
    let dir = TempDir::new("make-deb-label");
    programs(&dir, &program(0x3e), &program(0x3e), "");
    let out = make_deb(&dir, "dev-1a2b3c4");
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
    let out = make_deb(&dir, "0.2.0");
    assert!(out.status.success(), "{}", say(&out));
    let deb = dir.path().join("out").join("diacraft-0.2.0-linux-x86_64.deb");
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
    ]
    .into_iter()
    .chain(EXTRAS)
    {
        assert!(paths.contains(&path), "{path} is not in the package: {paths:?}");
    }
    assert!(listing.lines().all(|l| l.contains("root/root")), "every file belongs to root, whoever made the package: {listing}");
    // What the command line wrote is what the package holds.
    let unpacked = dir.path().join("unpacked");
    let extracted = Command::new("dpkg-deb").arg("--extract").arg(&deb).arg(&unpacked).output().unwrap();
    assert!(extracted.status.success(), "{}", say(&extracted));
    for (file, wrote) in [
        ("usr/share/bash-completion/completions/diacraft", "# bash completions of diacraft\n"),
        ("usr/share/zsh/vendor-completions/_diacraft-cli", "# zsh completions of diacraft-cli\n"),
        ("usr/share/fish/vendor_completions.d/diacraft-cli.fish", "# fish completions of diacraft-cli\n"),
    ] {
        assert_eq!(std::fs::read_to_string(unpacked.join(file)).unwrap(), wrote, "{file}");
    }
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
    let out = make_deb(&dir, "0.2.0");
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
    let out = make_deb(&dir, "0.2.0");
    assert!(out.status.success(), "{}", say(&out));
    // The other CPU's, as the first bytes of a program: made, as it is.
    let dir = TempDir::new("make-deb-other");
    programs(&dir, &elf(other), &elf(other), "");
    let out = make_deb(&dir, "0.2.0");
    assert!(out.status.success(), "{}", say(&out));
    // This CPU's, and no program: strip refuses it, and the script says whose.
    let dir = TempDir::new("make-deb-refused");
    programs(&dir, &elf(own), &elf(own), "");
    let out = make_deb(&dir, "0.2.0");
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("strip could not read diacraft"), "{}", say(&out));
    assert!(!dir.path().join("out").join("diacraft-0.2.0-linux-x86_64.deb").exists());
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
    let arch = if other == 0xb7 { "aarch64" } else { "x86_64" };
    for (library, package) in LIBRARIES.into_iter().chain([("libdbus-1.so.3", "libdbus-1-3")]) {
        let dir = TempDir::new("make-deb-libraries");
        let mut named = elf(other);
        named.extend(format!("{library}\0").bytes());
        programs(&dir, &named, &named, "");
        let out = make_deb(&dir, "0.2.0");
        assert!(out.status.success(), "{library}: {}", say(&out));
        let deb = dir.path().join("out").join(format!("diacraft-0.2.0-linux-{arch}.deb"));
        let wanted = format!("{} {}", field(&deb, "Depends"), field(&deb, "Recommends"));
        let words: Vec<&str> = wanted.split(|c: char| !(c.is_ascii_alphanumeric() || "+-.".contains(c))).collect();
        assert!(words.contains(&package), "{library}: {package} is neither depended on nor recommended: {wanted}");
    }
    // `x11rb` loads libxcb when the program opens a window on an X server, and a program that cannot is no window program: the package depends on it (it is small, and
    // `libx11-6` depends on it anyway).
    let depends = {
        let dir = TempDir::new("make-deb-xcb");
        programs(&dir, &elf(other), &elf(other), "");
        let out = make_deb(&dir, "0.2.0");
        assert!(out.status.success(), "{}", say(&out));
        field(&dir.path().join("out").join(format!("diacraft-0.2.0-linux-{arch}.deb")), "Depends")
    };
    assert!(depends.split(", ").any(|d| d == "libxcb1"), "Depends: {depends}");
}
