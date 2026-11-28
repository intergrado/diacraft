//! The .rpm (`packaging/linux/make-rpm.sh`, with `packaging/linux/diacraft.spec`): the CPU and the version it says, where it puts the files, what it asks of the machine and what it
//! refuses. It needs `rpmbuild` and the package `desktop-file-utils` (the spec checks the launcher entry), which a Fedora has; a machine without them skips the tests that build a package, as
//! the tests of the .deb skip without `dpkg-deb`. The script's last step, `rpmbuild`, is a stand-in in the tests that only look at what the script does before it.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{elf, names, program, programs, say, stand_in_cli, write_script};

fn has(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// Whether this machine can build a package: `rpmbuild`, `rpm` (which these tests read the package with), and the package `desktop-file-utils` in rpm's own database, which the spec's
/// `BuildRequires` names and its %check runs: a Debian or Ubuntu machine may have rpm's tools, and its database has no package, so the build would stop at the requirement.
fn can_build() -> bool {
    let found = has("rpmbuild")
        && has("desktop-file-validate")
        && Command::new("rpm").args(["-q", "desktop-file-utils"]).output().is_ok_and(|out| out.status.success());
    if !found {
        eprintln!("rpmbuild, rpm or the package desktop-file-utils is not here: the .rpm is not made");
    }
    found
}

/// Runs `packaging/linux/make-rpm.sh NAME OUT BIN`, with a plain `PATH` as the runners have, and `env` added.
fn make_rpm_with(name: &str, out: &Path, bin: &Path, env: &[(&str, &str)]) -> Output {
    Command::new("bash")
        .env("PATH", "/usr/bin:/bin")
        .envs(env.iter().copied())
        .args([xtask::root().join("packaging/linux/make-rpm.sh").to_str().unwrap(), name])
        .arg(out)
        .arg(bin)
        .output()
        .unwrap()
}

fn make_rpm(name: &str, out: &Path, bin: &Path) -> Output {
    make_rpm_with(name, out, bin, &[])
}

/// What `rpm -qp` says of a package, with a query format (`--qf`) or an option.
fn query(rpm: &Path, args: &[&str]) -> String {
    let out = Command::new("rpm").arg("-qp").args(args).arg(rpm).output().unwrap();
    assert!(out.status.success(), "rpm -qp {args:?}: {}", say(&out));
    String::from_utf8(out.stdout).unwrap()
}

/// The CPU of this machine in the two words the tests use: the ELF machine and the name of the CPU in a file name.
fn own() -> Option<(u8, &'static str)> {
    match std::env::consts::ARCH {
        "x86_64" => Some((0x3e, "x86_64")),
        "aarch64" => Some((0xb7, "aarch64")),
        _ => None,
    }
}

/// A package of the programs of this machine's CPU (a real program, which `strip` and rpm's own tools read), made under the name `name`.
fn built(name: &str) -> (TempDir, std::path::PathBuf) {
    let (machine, cpu) = own().expect("a machine of x86_64 or aarch64");
    let dir = TempDir::new("make-rpm");
    programs(&dir, &program(machine), &program(machine), "");
    let out = make_rpm(name, &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{name}: {}", say(&out));
    let rpm = dir.path().join("out").join(format!("diacraft-{}-linux-{cpu}.rpm", name.trim_start_matches('v')));
    assert!(rpm.is_file(), "{name}: {}", say(&out));
    (dir, rpm)
}

#[test]
fn an_rpm_says_the_cpu_and_the_version_of_its_programs() {
    if !can_build() || own().is_none() {
        return;
    }
    let (_, cpu) = own().unwrap();
    // A name is the version, with no `v`, and a tag given all the same is named as its version. rpm sorts a `~` before the release, as a Debian version does; the release is 1.
    for (name, version) in [("0.2.0", "0.2.0"), ("0.2.0-rc1", "0.2.0~rc1"), ("0.2.0-rc.2-x", "0.2.0~rc.2.x"), ("v0.2.0-rc1", "0.2.0~rc1")] {
        let (dir, rpm) = built(name);
        assert_eq!(names(&dir.path().join("out")), [format!("diacraft-{}-linux-{cpu}.rpm", name.trim_start_matches('v'))], "{name}");
        assert_eq!(query(&rpm, &["--qf", "%{NAME} %{VERSION} %{RELEASE} %{ARCH}"]), format!("diacraft {version} 1 {cpu}"), "{name}");
    }
}

/// What the file is called, with a stand-in for `rpmbuild` that only writes the file it is asked for, so that the name is checked on a machine that has no rpm (the programs are those of
/// the CPU this machine is not, which the script leaves as they are): the version with no `v`, and aarch64 or x86_64 for the CPU.
#[test]
fn an_rpm_is_named_for_the_version_and_the_cpu_with_no_rpm_on_the_machine() {
    let (other, in_names) = match std::env::consts::ARCH {
        "x86_64" => (0xb7, "aarch64"),
        _ => (0x3e, "x86_64"),
    };
    for (name, said) in [("0.2.1", "0.2.1"), ("v0.2.1", "0.2.1"), ("v0.3.0-rc.1", "0.3.0-rc.1"), ("dev-1a2b3c4", "dev-1a2b3c4")] {
        let dir = TempDir::new("make-rpm-name");
        programs(&dir, &elf(other), &elf(other), "");
        let out = make_rpm_listing(&dir, name, &[]);
        assert!(out.status.success(), "{name}: {}", say(&out));
        let file = format!("diacraft-{said}-linux-{in_names}.rpm");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()], "{name}");
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&file), "the last line is the file: {}", say(&out));
    }
}

/// A `rpmbuild` that writes one file where `--define "_rpmdir DIR"` says, with the list of the files of the tree it is built from (`files`, in the folder it runs in), one for each line:
/// that is all the script needs of it when the tests look at what the script did before.
fn stand_in_rpmbuild(dir: &TempDir) {
    write_script(
        dir,
        "tools/rpmbuild",
        "#!/bin/sh\nrpmdir=\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --define ]; then case \"$2\" in _rpmdir\\ *) rpmdir=${2#_rpmdir } ;; esac; fi\n  shift\ndone\nmkdir -p \"$rpmdir/x\"\n(cd files && find . -type f | sort) >\"$rpmdir/x/a.rpm\"\n",
    );
}

/// Runs the script on the programs of `dir` with the stand-in `rpmbuild` first on the `PATH` and `env` added: the package is the list of its files, on a machine that has no rpm.
fn make_rpm_listing(dir: &TempDir, name: &str, env: &[(&str, &str)]) -> Output {
    stand_in_rpmbuild(dir);
    Command::new("bash")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().join("tools").display()))
        .envs(env.iter().copied())
        .arg(xtask::root().join("packaging/linux/make-rpm.sh"))
        .arg(name)
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"))
        .output()
        .unwrap()
}

#[test]
fn a_label_that_is_no_tag_gives_a_version_that_sorts_before_the_release_it_leads_to() {
    if !can_build() || own().is_none() {
        return;
    }
    let (_dir, rpm) = built("dev-1a2b3c4");
    let version = query(&rpm, &["--qf", "%{VERSION}"]);
    assert!(version.ends_with("~dev.1a2b3c4") && version.starts_with(|c: char| c.is_ascii_digit()), "{version}");
}

#[test]
fn an_rpm_puts_the_programs_the_entry_the_icon_and_the_licences_where_fedora_looks() {
    if !can_build() || own().is_none() {
        return;
    }
    let (dir, rpm) = built("0.2.0");
    let listing = query(&rpm, &["-l"]);
    let paths: Vec<&str> = listing.lines().collect();
    for path in [
        "/usr/bin/diacraft",
        "/usr/bin/diacraft-cli",
        "/usr/share/applications/app.diacraft.desktop",
        "/usr/share/icons/hicolor/256x256/apps/app.diacraft.png",
        "/usr/share/licenses/diacraft/LICENSE-MIT",
        "/usr/share/licenses/diacraft/LICENSE-APACHE",
        "/usr/share/licenses/diacraft/NOTICE",
        "/usr/share/licenses/diacraft/ATTRIBUTION.md",
        "/usr/share/licenses/diacraft/THIRD-PARTY-LICENSES.txt",
    ] {
        assert!(paths.contains(&path), "{path} is not in the package: {paths:?}");
    }
    // The five licence files are the ones rpm calls licence files, and nothing else is.
    let licences = query(&rpm, &["-L"]);
    assert_eq!(licences.lines().count(), 5, "{licences}");
    assert!(licences.lines().all(|l| l.starts_with("/usr/share/licenses/diacraft/")), "{licences}");
    // Every file belongs to root, whoever made the package, and nothing is left under another place.
    let owners = query(&rpm, &["--qf", "[%{FILEUSERNAME}:%{FILEGROUPNAME}\\n]"]);
    assert!(owners.lines().all(|l| l == "root:root"), "{owners}");
    assert!(paths.iter().all(|p| p.starts_with("/usr/")), "{paths:?}");
    // No shell completions and no manual pages: the programs here are no diacraft-cli that writes them (the next test makes the package that has them).
    assert!(!paths.iter().any(|p| p.contains("completion") || p.contains("/man/") || p.contains("site-functions")), "{paths:?}");
    // The licence is the project's own, and the package has no script: Fedora's file triggers refresh the desktop database and the icon cache.
    assert_eq!(query(&rpm, &["--qf", "%{LICENSE}"]), "MIT OR Apache-2.0");
    assert_eq!(query(&rpm, &["--scripts"]).trim(), "", "a package with a script of its own");
    // No debuginfo package, no source package: one file is all the script wrote.
    assert_eq!(names(&dir.path().join("out")).len(), 1);
}

#[test]
fn an_rpm_requires_what_the_programs_load_by_name_and_recommends_the_rest() {
    if !can_build() || own().is_none() {
        return;
    }
    let (_dir, rpm) = built("0.2.0");
    let requires = query(&rpm, &["-R"]);
    let requires: Vec<&str> = requires.lines().collect();
    // What the programs cannot start without: the keyboard library, libxcb, a window system and a graphics API (sonames with no version, which Fedora's packages answer).
    for need in [
        "libxkbcommon.so.0()(64bit)",
        "libxcb.so.1()(64bit)",
        "(libwayland-client.so.0()(64bit) or libX11.so.6()(64bit))",
        "(libvulkan.so.1()(64bit) or libEGL.so.1()(64bit))",
        "hicolor-icon-theme",
    ] {
        assert!(requires.contains(&need), "{need} is not required: {requires:?}");
    }
    // No Requires names a version of a package, which a person's Fedora may not have: the C library's versions of symbols are rpm's own, read from the programs.
    assert!(requires.iter().all(|r| !r.contains(" >= ") && !r.contains(" = ")), "a Requires that names a version of a package: {requires:?}");
    let recommends = query(&rpm, &["--recommends"]);
    for want in [
        "libxkbcommon-x11.so.0()(64bit)",
        "libX11-xcb.so.1()(64bit)",
        "libXcursor.so.1()(64bit)",
        "libXi.so.6()(64bit)",
        "libwayland-egl.so.1()(64bit)",
        "libdbus-1.so.3()(64bit)",
        "mesa-vulkan-drivers",
        "mesa-dri-drivers",
        "xdg-desktop-portal",
    ] {
        assert!(recommends.lines().any(|l| l == want), "{want} is not recommended: {recommends}");
    }
}

/// The completions go where each shell looks on a Fedora: zsh in `site-functions` (the `vendor-completions` folder of the .deb is Debian's), bash and fish where they are
/// everywhere, and the manual pages gzipped under `man1`. The script asks the command line to write them when it has the commands, and the package lists them.
#[test]
fn an_rpm_made_with_a_command_line_that_writes_completions_installs_them_where_fedoras_shells_look() {
    if !can_build() || own().is_none() {
        return;
    }
    let (machine, cpu) = own().unwrap();
    let dir = TempDir::new("make-rpm-extras");
    programs(&dir, &program(machine), &program(machine), "");
    let cli = stand_in_cli(&dir, "diacraft-cli\\ndiacraft-cli-convert\\ndiacraft\\n");
    let out = make_rpm_with("0.2.0", &dir.path().join("out"), &dir.path().join("bin"), &[("EXTRAS_CLI", cli.as_str())]);
    assert!(out.status.success(), "{}", say(&out));
    let rpm = dir.path().join("out").join(format!("diacraft-0.2.0-linux-{cpu}.rpm"));
    let listing = query(&rpm, &["-l"]);
    let paths: Vec<&str> = listing.lines().collect();
    for path in [
        "/usr/share/bash-completion/completions/diacraft",
        "/usr/share/bash-completion/completions/diacraft-cli",
        "/usr/share/zsh/site-functions/_diacraft",
        "/usr/share/zsh/site-functions/_diacraft-cli",
        "/usr/share/fish/vendor_completions.d/diacraft.fish",
        "/usr/share/fish/vendor_completions.d/diacraft-cli.fish",
        "/usr/share/man/man1/diacraft.1.gz",
        "/usr/share/man/man1/diacraft-cli.1.gz",
        "/usr/share/man/man1/diacraft-cli-convert.1.gz",
    ] {
        assert!(paths.contains(&path), "{path} is not in the package: {paths:?}");
    }
    assert!(!paths.iter().any(|p| p.contains("vendor-completions")), "{paths:?}");
    // What the program wrote is what the package holds: the digest of the file in the package is that of what the stand-in printed for it.
    let digests = query(&rpm, &["--qf", "[%{FILENAMES} %{FILEDIGESTS}\\n]"]);
    let wrote = Command::new("sh").arg("-c").arg("echo '# zsh completions of diacraft-cli' | sha256sum").output().unwrap();
    let wrote = String::from_utf8(wrote.stdout).unwrap();
    let wrote = wrote.split_whitespace().next().unwrap();
    assert!(digests.lines().any(|l| l == format!("/usr/share/zsh/site-functions/_diacraft-cli {wrote}")), "{digests}");
    // A page that the spec's file list does not name (`diacraft*`) is refused, and so is a package for a command line that lists none.
    let dir = TempDir::new("make-rpm-extras-refused");
    programs(&dir, &program(machine), &program(machine), "");
    let cli = stand_in_cli(&dir, "other\\n");
    let out = make_rpm_with("0.2.0", &dir.path().join("out"), &dir.path().join("bin"), &[("EXTRAS_CLI", cli.as_str())]);
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("the manual page 'other' is not named diacraft*"), "{}", say(&out));
    let dir = TempDir::new("make-rpm-extras-none");
    programs(&dir, &program(machine), &program(machine), "");
    let cli = stand_in_cli(&dir, "");
    let out = make_rpm_with("0.2.0", &dir.path().join("out"), &dir.path().join("bin"), &[("EXTRAS_CLI", cli.as_str())]);
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("diacraft-cli lists no manual page"), "{}", say(&out));
}

/// The ways the script goes for the completions and the manual pages, on every machine (the stand-in `rpmbuild` lists the files it is given): a command line that runs writes them where each
/// shell looks on a Fedora (`EXTRAS_CLI` is the stand-in for the program of this machine's CPU that runs, which the programs of the tests are not); the programs of the other CPU are not
/// asked, and the package is made without them, and says so; and a command line that cannot run, or writes nothing, leaves them out the same way.
#[test]
fn a_command_line_that_runs_has_its_completions_in_the_package_and_the_other_cpu_and_one_that_writes_none_are_left_without_them() {
    let Some((own, cpu)) = own() else {
        return;
    };
    let (other, other_names) = if own == 0x3e { (0xb7, "aarch64") } else { (0x3e, "x86_64") };
    let extras = [
        "./usr/share/bash-completion/completions/diacraft",
        "./usr/share/bash-completion/completions/diacraft-cli",
        "./usr/share/zsh/site-functions/_diacraft",
        "./usr/share/zsh/site-functions/_diacraft-cli",
        "./usr/share/fish/vendor_completions.d/diacraft.fish",
        "./usr/share/fish/vendor_completions.d/diacraft-cli.fish",
        "./usr/share/man/man1/diacraft.1.gz",
        "./usr/share/man/man1/diacraft-cli.1.gz",
        "./usr/share/man/man1/diacraft-cli-convert.1.gz",
    ];
    // A command line that runs: the nine files.
    let dir = TempDir::new("make-rpm-listing-extras");
    programs(&dir, &program(own), &program(own), "");
    let cli = stand_in_cli(&dir, "diacraft-cli\\ndiacraft-cli-convert\\ndiacraft\\n");
    let out = make_rpm_listing(&dir, "0.2.0", &[("EXTRAS_CLI", &cli)]);
    assert!(out.status.success(), "{}", say(&out));
    let files = std::fs::read_to_string(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{cpu}.rpm"))).unwrap();
    for path in extras {
        assert!(files.lines().any(|f| f == path), "{path} is not in the package: {files}");
    }
    assert!(!say(&out).contains("without"), "{}", say(&out));
    // Programs of the other CPU cannot be asked.
    let dir = TempDir::new("make-rpm-listing-other");
    programs(&dir, &elf(other), &elf(other), "");
    let out = make_rpm_listing(&dir, "0.2.0", &[]);
    assert!(out.status.success(), "{}", say(&out));
    assert!(
        say(&out).contains("the package is made without shell completions and manual pages (they are written by running the program)"),
        "{}",
        say(&out)
    );
    let files = std::fs::read_to_string(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{other_names}.rpm"))).unwrap();
    assert!(files.contains("./usr/bin/diacraft-cli") && !files.contains("completion") && !files.contains("/man/"), "{files}");
    // The programs of this machine's CPU as the other tests write them: a file with no right to run, and a copy of `true` that may run and prints nothing.
    if program(own) == elf(own) {
        return;
    }
    for mode in [0o644, 0o755] {
        let dir = TempDir::new("make-rpm-listing-none");
        programs(&dir, &program(own), &program(own), "");
        std::fs::set_permissions(dir.path().join("bin/diacraft-cli"), std::fs::Permissions::from_mode(mode)).unwrap();
        let out = make_rpm_listing(&dir, "0.2.0", &[]);
        assert!(out.status.success(), "{mode:o}: {}", say(&out));
        assert!(say(&out).contains("has no completions and manual pages to write: the package is made without them"), "{mode:o}: {}", say(&out));
        let files = std::fs::read_to_string(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{cpu}.rpm"))).unwrap();
        assert!(!files.contains("completion") && !files.contains("/man/"), "{mode:o}: {files}");
    }
}

/// A build of the command line from before a change answers `completions` with an error: the package is made without them, and the script says so. (The real programs of the tests,
/// copies of `true`, are written with no right to run, which the script counts as the same: a command line with no completions to write. The test above, which lists the package,
/// also tries a copy that may run and answers with nothing.)
#[test]
fn a_command_line_with_no_completions_to_write_gives_a_package_without_them_and_the_script_says_so() {
    if !can_build() || own().is_none() {
        return;
    }
    let (machine, _) = own().unwrap();
    let dir = TempDir::new("make-rpm-no-extras");
    programs(&dir, &program(machine), &program(machine), "");
    let out = make_rpm("0.2.0", &dir.path().join("out"), &dir.path().join("bin"));
    assert!(out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("has no completions and manual pages to write: the package is made without them"), "{}", say(&out));
}

#[test]
fn an_rpm_is_refused_for_programs_that_name_a_library_it_does_not_cover() {
    let other = match std::env::consts::ARCH {
        "x86_64" => 0xb7,
        _ => 0x3e,
    };
    let dir = TempDir::new("make-rpm-library");
    let mut program = elf(other);
    program.extend(b"libnotknown.so.9\0");
    programs(&dir, &program, &elf(other), "");
    stand_in_rpmbuild(&dir);
    let out = Command::new("bash")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().join("tools").display()))
        .arg(xtask::root().join("packaging/linux/make-rpm.sh"))
        .args(["0.2.0"])
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("diacraft names the library libnotknown.so.9, which no Requires or Recommends line of"), "{}", say(&out));
    assert!(
        !dir.path().join("out").join("diacraft-0.2.0-linux-aarch64.rpm").exists()
            && !dir.path().join("out").join("diacraft-0.2.0-linux-x86_64.rpm").exists()
    );
}

/// The libraries the programs load by name when they start. The names are the ones the crates that load them hold (`x11rb` loads `libxcb.so.1`, `ash` `libvulkan.so.1`, `winit`'s
/// `x11-dl` the X libraries, `wayland-sys` the Wayland ones, and so on). Whether a program's build shows its names to a scan of its bytes is up to the compiler: a short name can be
/// written into the code a few bytes at a time, as the x86_64 build does for `libxcb.so.1` and the ARM64 build does not (the .deb's release run found it out). Every name is accepted,
/// and the three of the C runtime that rpm itself puts in the Requires too.
const LIBRARIES: [&str; 15] = [
    "libc.so.6",
    "libm.so.6",
    "libgcc_s.so.1",
    "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0",
    "libwayland-client.so.0",
    "libwayland-egl.so.1",
    "libX11.so.6",
    "libX11-xcb.so.1",
    "libxcb.so.1",
    "libXcursor.so.1",
    "libXi.so.6",
    "libEGL.so.1",
    "libvulkan.so.1",
    "libdbus-1.so.3",
];

#[test]
fn every_library_the_programs_load_by_name_is_known_and_a_build_on_fedora_that_runs_two_names_together_is_read() {
    let other = match std::env::consts::ARCH {
        "x86_64" => 0xb7,
        _ => 0x3e,
    };
    let arch = if other == 0xb7 { "aarch64" } else { "x86_64" };
    // Each name alone, and then all of them with the text that the string table of a build on Fedora has before one of them: `lib64libXi.so.6` is the name `libXi.so.6` after the
    // word `lib64`, which has no byte between.
    let mut cases: Vec<Vec<u8>> = LIBRARIES.iter().map(|lib| format!("{lib}\0").into_bytes()).collect();
    cases.push(b"lib64libXi.so.6\0lib64libX11.so.6\0".to_vec());
    for case in cases {
        let dir = TempDir::new("make-rpm-libraries");
        let mut named = elf(other);
        named.extend(case.iter());
        programs(&dir, &named, &named, "");
        stand_in_rpmbuild(&dir);
        let out = Command::new("bash")
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().join("tools").display()))
            .arg(xtask::root().join("packaging/linux/make-rpm.sh"))
            .arg("0.2.0")
            .arg(dir.path().join("out"))
            .arg(dir.path().join("bin"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{}: {}", String::from_utf8_lossy(&case), say(&out));
        assert!(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{arch}.rpm")).is_file());
    }
}

/// The script's own copy of the files it reads, as links to the real ones, with `spec` in place of the spec (the script finds its root from where it is, so a script run from here reads
/// the spec here), and the script's path.
fn script_with_spec(dir: &TempDir, spec: &str) -> std::path::PathBuf {
    let root = xtask::root();
    for file in [
        "packaging/common.sh",
        "packaging/linux/make-rpm.sh",
        "packaging/linux/app.diacraft.desktop",
        "assets/branding/diacraft-icon-256.png",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "NOTICE",
        "ATTRIBUTION.md",
        "THIRD-PARTY-LICENSES.txt",
        "Cargo.toml",
    ] {
        let link = dir.path().join("root").join(file);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(root.join(file), &link).unwrap();
    }
    dir.write("root/packaging/linux/diacraft.spec", spec);
    dir.path().join("root/packaging/linux/make-rpm.sh")
}

/// A library the spec names is found whatever else the spec names (a flake of the check, found under load: a run of the test above stopped at `libvulkan.so.1` once in a few dozen, on
/// a machine busy with other builds). The script gives the names the spec holds to `grep -q` through a pipe, and under `pipefail` a `grep -q` that is done at its first match and
/// leaves the pipe while the writer still has names to write turns the match into a failure (the writer is killed by the broken pipe). Here the spec names 30,000 libraries after
/// the one the programs load, four times more than a pipe holds, so the writer is always still writing when the reader has gone: the failure that came once in a few dozen runs
/// comes in every one.
#[test]
fn a_library_the_spec_names_is_found_however_long_the_list_of_names_is() {
    let other = match std::env::consts::ARCH {
        "x86_64" => 0xb7,
        _ => 0x3e,
    };
    let arch = if other == 0xb7 { "aarch64" } else { "x86_64" };
    let mut spec = String::from("Name: diacraft\nRequires: libEGL.so.1()(64bit)\n");
    for line in 0..600 {
        let names: Vec<String> = (0..50).map(|k| format!("libz{:05}.so.1()(64bit)", line * 50 + k)).collect();
        spec.push_str(&format!("Recommends: {}\n", names.join(" ")));
    }
    // The names are the bytes the writer has to write: more than four pipes' worth (a pipe holds 64 KiB).
    assert!(spec.len() > 4 * 65_536, "{}", spec.len());
    let dir = TempDir::new("make-rpm-long-spec");
    let script = script_with_spec(&dir, &spec);
    let mut named = elf(other);
    named.extend(b"libEGL.so.1\0");
    programs(&dir, &named, &named, "");
    stand_in_rpmbuild(&dir);
    let out = Command::new("bash")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().join("tools").display()))
        .arg(&script)
        .arg("0.2.0")
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", say(&out));
    assert!(dir.path().join("out").join(format!("diacraft-0.2.0-linux-{arch}.rpm")).is_file());
    // And a library the spec does not name is still refused, with the long list or not.
    let mut unknown = elf(other);
    unknown.extend(b"libEGL.so.1\0libnotnamed.so.4\0");
    programs(&dir, &unknown, &unknown, "");
    let out = Command::new("bash")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().join("tools").display()))
        .arg(&script)
        .arg("0.2.0")
        .arg(dir.path().join("out2"))
        .arg(dir.path().join("bin"))
        .output()
        .unwrap();
    assert!(
        !out.status.success() && say(&out).contains("diacraft names the library libnotnamed.so.4, which no Requires or Recommends line of"),
        "{}",
        say(&out)
    );
}

/// The spec names every library the .deb's `Depends` and `Recommends` name, so the two packages ask the same of a machine (the libraries as sonames here and as packages there).
#[test]
fn the_spec_and_the_deb_script_cover_the_same_libraries() {
    let spec = std::fs::read_to_string(xtask::root().join("packaging/linux/diacraft.spec")).unwrap();
    let deb = std::fs::read_to_string(xtask::root().join("packaging/linux/make-deb.sh")).unwrap();
    let covered: Vec<&str> = spec
        .lines()
        .filter(|l| l.starts_with("Requires:") || l.starts_with("Recommends:"))
        .flat_map(|l| l.split(|c: char| !(c.is_ascii_alphanumeric() || "._+-".contains(c))))
        .collect();
    // The names the .deb script knows, as the sonames of its `package_of_library` list.
    let known: Vec<&str> = deb
        .lines()
        .filter_map(|l| l.trim().split_once(") echo ").map(|(lib, _)| lib))
        .flat_map(|lib| lib.split(" | "))
        .filter(|lib| lib.starts_with("lib") && lib.contains(".so."))
        .filter(|lib| !["libc.so.6", "libm.so.6", "libgcc_s.so.1"].contains(lib))
        .collect();
    assert!(known.len() >= 10, "the libraries of the .deb script were not found: {known:?}");
    for library in known {
        assert!(covered.contains(&library), "the .deb script knows {library}, and the spec's Requires and Recommends lines do not name it");
    }
    assert!(LIBRARIES.iter().filter(|l| !["libc.so.6", "libm.so.6", "libgcc_s.so.1"].contains(l)).all(|l| covered.contains(l)), "{covered:?}");
}

/// The script that runs the container takes the Fedora image from `FEDORA_IMAGE` and from nothing else (the release workflow gives it a digest, which a test of the workflow holds), so a run
/// that lost the variable stops and does not fall back on a tag that moves; an image that is no digest is used and the script says so. The container engine is `true` here: nothing runs.
#[test]
fn the_fedora_image_comes_from_the_variable_and_a_tag_is_said_to_move() {
    let dir = TempDir::new("rpm-in-fedora");
    dir.write("a.rpm", "not a package");
    let run = |image: Option<&str>| {
        let mut command = Command::new("bash");
        command.env("PATH", "/usr/bin:/bin").env("CONTAINER_ENGINE", "true").env_remove("FEDORA_IMAGE");
        if let Some(image) = image {
            command.env("FEDORA_IMAGE", image);
        }
        command.arg(xtask::root().join("packaging/linux/rpm-in-fedora.sh")).args(["check"]).arg(dir.path().join("a.rpm")).output().unwrap()
    };
    let none = run(None);
    assert!(!none.status.success() && say(&none).contains("FEDORA_IMAGE is not set"), "{}", say(&none));
    let tag = run(Some("docker.io/library/fedora:latest"));
    assert!(tag.status.success(), "{}", say(&tag));
    assert!(say(&tag).contains("is not pinned by a digest"), "{}", say(&tag));
    let digest = format!("docker.io/library/fedora@sha256:{}", "a".repeat(64));
    let pinned = run(Some(&digest));
    assert!(pinned.status.success() && !say(&pinned).contains("not pinned"), "{}", say(&pinned));
    // A digest of the wrong length is no digest.
    let short = run(Some("docker.io/library/fedora@sha256:abc"));
    assert!(say(&short).contains("is not pinned by a digest"), "{}", say(&short));
}
