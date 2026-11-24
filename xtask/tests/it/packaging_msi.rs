//! The Windows installer (`packaging/windows/`): what `make-msi.sh` asks WiX for, what the installer's source says about who it installs for and what it never does to a
//! person's file types, and the WiX the workflows install. WiX is a stand-in here (a script that writes down how it was run), so nothing is downloaded and the tests run
//! anywhere; the installer itself is built, installed, run and removed by the Windows jobs, which is where it is tried.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{names, pe, programs, say};

/// A stand-in for `wix`: it writes its arguments, one to a line, to `record`, keeps the icon it was given, and writes the installer it was asked for.
const FAKE_WIX: &str = r#"#!/bin/sh
here=$(dirname "$0")
: >"$here/record"
output=""
while [ $# -gt 0 ]; do
    echo "$1" >>"$here/record"
    case $1 in
    -o) output=$2 ;;
    -d)
        case $2 in IconFile=*) cp "${2#IconFile=}" "$here/icon.ico" ;; esac
        ;;
    esac
    shift
done
echo "an installer" >"$output"
"#;

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn make_msi(dir: &TempDir, name: &str, wix: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DIACRAFT_WIX")
        .arg(xtask::root().join("packaging/windows/make-msi.sh"))
        .arg(name)
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"));
    if let Some(wix) = wix {
        command.env("DIACRAFT_WIX", wix);
    }
    command.output().unwrap()
}

/// GitHub's Windows images define `WIX`, the folder of the WiX 3 toolset, in every job. A program of the machine is found by its name on the `PATH`, and no variable
/// of the image names it: the script once read `WIX` for the program, took the folder for it, and said wix was not there while the step before had installed it.
#[test]
fn the_wix_variable_of_a_windows_image_is_a_folder_and_not_the_program() {
    let (dir, _) = setup(0x8664);
    let tools = dir.path().join("tools");
    let mut command = Command::new("bash");
    command
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("WIX", dir.path().join("wix3-folder"))
        .env_remove("DIACRAFT_WIX")
        .arg(xtask::root().join("packaging/windows/make-msi.sh"))
        .arg("v0.2.0")
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"));
    std::fs::create_dir(dir.path().join("wix3-folder")).unwrap();
    let out = command.output().unwrap();
    assert!(out.status.success(), "{}", say(&out));
    assert!(recorded(&dir).first().is_some_and(|a| a == "build"), "the wix of the PATH was run: {:?}", recorded(&dir));
    // The variable the tests and a person may use to name another program is DiaCraft's own.
    let source = read("packaging/windows/make-msi.sh");
    assert!(source.contains("${DIACRAFT_WIX:-wix}") && !source.contains("${WIX"), "the override is DIACRAFT_WIX");
}

fn setup(machine: u16) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new("make-msi");
    programs(&dir, &pe(machine), &pe(machine), ".exe");
    dir.write("tools/wix", FAKE_WIX);
    std::fs::set_permissions(dir.path().join("tools/wix"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let wix = dir.path().join("tools/wix");
    (dir, wix)
}

fn recorded(dir: &TempDir) -> Vec<String> {
    std::fs::read_to_string(dir.path().join("tools/record")).unwrap().lines().map(str::to_owned).collect()
}

#[test]
fn an_installer_is_built_for_the_cpu_of_its_programs_with_the_version_of_its_name() {
    for (machine, arch, wix_arch) in [(0x8664, "x86_64", "x64"), (0xaa64, "arm64", "arm64")] {
        for (name, version) in [("v0.2.0", "0.2.0"), ("v0.2.1-rc1", "0.2.1"), ("v1.10.3", "1.10.3")] {
            let (dir, wix) = setup(machine);
            let out = make_msi(&dir, name, Some(&wix));
            assert!(out.status.success(), "{arch} {name}: {}", say(&out));
            let file = format!("diacraft-{name}-windows-{arch}.msi");
            assert_eq!(names(&dir.path().join("out")), [file.as_str()]);
            assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&file), "{}", say(&out));
            let args = recorded(&dir);
            assert_eq!(args.first().map(String::as_str), Some("build"), "{args:?}");
            let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
            assert_eq!(after("-arch").as_deref(), Some(wix_arch), "{arch}: {args:?}");
            assert!(args.iter().any(|a| a == &format!("Version={version}")), "{name}: {args:?}");
            assert!(args.iter().any(|a| a.starts_with("BinDir=") && a.ends_with("/bin")), "{args:?}");
            assert!(args.iter().any(|a| a.starts_with("RepoRoot=")), "{args:?}");
            assert_eq!(after("-pdbtype").as_deref(), Some("none"), "no .wixpdb is left beside the installer: {args:?}");
            assert!(args.last().is_some_and(|a| a.ends_with("packaging/windows/diacraft.wxs")), "the last argument is the source: {args:?}");
            assert!(after("-o").is_some_and(|o| o.ends_with(&file)), "{args:?}");
        }
    }
}

#[test]
fn a_label_that_is_no_tag_gives_the_installer_the_workspaces_version() {
    let (dir, wix) = setup(0x8664);
    let out = make_msi(&dir, "dev-1a2b3c4", Some(&wix));
    assert!(out.status.success(), "{}", say(&out));
    let manifest: toml::Table = read("Cargo.toml").parse().unwrap();
    let workspace = manifest["workspace"]["package"]["version"].as_str().unwrap().split('-').next().unwrap().to_owned();
    assert!(recorded(&dir).iter().any(|a| a == &format!("Version={workspace}")), "{:?}", recorded(&dir));
}

/// The icon is an .ico that holds the repository's one picture of it: a header of 6 bytes, an entry of 16 and the PNG itself, which Windows reads since Vista.
#[test]
fn the_icon_is_the_icon_picture_in_an_ico_wrapper() {
    let (dir, wix) = setup(0x8664);
    let out = make_msi(&dir, "v0.2.0", Some(&wix));
    assert!(out.status.success(), "{}", say(&out));
    let ico = std::fs::read(dir.path().join("tools/icon.ico")).unwrap();
    let png = std::fs::read(xtask::root().join("assets/branding/diacraft-icon-256.png")).unwrap();
    assert_eq!(&ico[..6], [0, 0, 1, 0, 1, 0], "an icon file with one image");
    assert_eq!(&ico[6..14], [0, 0, 0, 0, 1, 0, 32, 0], "256 by 256, no palette, one plane, 32 bits");
    assert_eq!(u32::from_le_bytes(ico[14..18].try_into().unwrap()) as usize, png.len(), "the size of the image");
    assert_eq!(u32::from_le_bytes(ico[18..22].try_into().unwrap()), 22, "the image starts after the header and the entry");
    assert_eq!(&ico[22..], png.as_slice(), "the image is the PNG");
}

#[test]
fn an_installer_needs_wix_and_programs_of_one_cpu() {
    let (dir, wix) = setup(0x8664);
    let out = make_msi(&dir, "v0.2.0", Some(&dir.path().join("tools/missing")));
    assert!(!out.status.success() && say(&out).contains("wix is not here"), "{}", say(&out));
    dir.write_bytes("bin/diacraft-cli.exe", &pe(0xaa64));
    let out = make_msi(&dir, "v0.2.0", Some(&wix));
    assert!(!out.status.success() && say(&out).contains("one download holds one architecture"), "{}", say(&out));
    assert!(!dir.path().join("out").join("diacraft-v0.2.0-windows-x86_64.msi").exists());
}

/// What the installer's source says, held as text: it installs for the person, never for the machine, and it never takes a person's file types.
#[test]
fn the_installers_source_installs_for_the_person_and_never_sets_the_program_a_file_opens_with() {
    let source = read("packaging/windows/diacraft.wxs");
    assert!(source.contains(r#"Scope="perUser""#), "no administrator rights");
    assert!(
        source.contains(r#"<StandardDirectory Id="LocalAppDataFolder">"#)
            && source.contains(r#"Name="Programs""#)
            && source.contains(r#"Name="DiaCraft""#),
        "%LocalAppData%\\Programs\\DiaCraft"
    );
    assert!(!source.contains("ProgramFilesFolder") && !source.contains("ProgramFiles6432Folder"), "nothing is installed for the machine");
    assert!(
        !source.contains(r#"Root="HKLM""#) && !source.contains(r#"Root="HKMU""#) && !source.contains(r#"Root="HKCR""#),
        "nothing is written under the machine's registry"
    );
    assert!(source.contains(r#"<MajorUpgrade DowngradeErrorMessage="#), "a later version replaces this one");
    assert!(source.contains(r#"<Shortcut "#) && source.contains(r#"Directory="ProgramMenuFolder""#), "the Start menu");
    assert!(source.contains(r#"<Environment "#) && source.contains(r#"System="no""#) && source.contains(r#"Name="PATH""#), "the person's own PATH");
    for ext in [".diacraft", ".vsdx", ".odg"] {
        assert!(source.contains(&format!(r#"Key="Software\Classes\{ext}\OpenWithProgids""#)), "{ext} lists DiaCraft in Open with");
    }
    // A (Default) value on a type's own key is the program a double-click opens. Every registration of a type's key is a value of OpenWithProgids, with a name.
    for line in source.lines().filter(|l| l.contains("<RegistryValue")) {
        for ext in [".diacraft", ".vsdx", ".odg"] {
            assert!(!line.contains(&format!(r#"Key="Software\Classes\{ext}""#)), "{ext} must not be given a default program: {line}");
        }
    }
    // Every file of the installer is in the repository, or made from it.
    for file in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md", "THIRD-PARTY-LICENSES.txt"] {
        assert!(source.contains(&format!(r"$(RepoRoot)\{file}")), "{file} is installed");
        assert!(xtask::root().join(file).is_file(), "{file}");
    }
}

/// The upgrade code is the product's identity: a later version replaces this one only while it never changes, so a change here is a different product that sits beside
/// the old one. The two components that hold files have GUIDs that never change either.
#[test]
fn the_upgrade_code_and_the_guids_of_the_components_that_hold_files_are_fixed() {
    let source = read("packaging/windows/diacraft.wxs");
    assert!(source.contains(r#"UpgradeCode="5A247B9C-9E5F-42F2-9805-78832461BCC7""#), "the upgrade code is the product's identity and never changes");
    assert!(source.contains(r#"<Component Id="ProgramFiles" Guid="AABADF4E-018B-4698-9CAA-F329DF21E6BE">"#));
    assert!(source.contains(r#"<Component Id="LicenceFiles" Guid="0B02B2FA-644F-48E8-87F3-C67FF33ACB23">"#));
}

/// The workflows install one version of WiX, by the SHA-256 of its package: the release's and the CI workflow's are the same, and a version of WiX that asks for a fee
/// (6 and later come under another licence) is not this one.
#[test]
fn both_workflows_install_the_same_wix_by_version_and_checksum() {
    let value = |workflow: &str, name: &str| {
        let prefix = format!("{name}: ");
        workflow.lines().map(str::trim).find_map(|l| l.strip_prefix(&prefix)).unwrap_or_else(|| panic!("no `{name}:` line")).to_owned()
    };
    let release = read(".github/workflows/release.yml");
    let ci = read(".github/workflows/ci.yml");
    for name in ["WIX_VERSION", "WIX_SHA256"] {
        assert_eq!(value(&release, name), value(&ci, name), "{name}: move release.yml and ci.yml together");
    }
    let sum = value(&release, "WIX_SHA256");
    assert!(sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "WIX_SHA256 is `{sum}`");
    assert_eq!(value(&release, "WIX_VERSION"), "5.0.2", "5.x is MS-RL; WiX 6 and 7 come under the Open Source Maintenance Fee EULA");
    for workflow in [&release, &ci] {
        assert!(workflow.contains("sha256sum --check --strict"), "the package is checked");
        assert!(workflow.contains("--configfile"), "the package is installed from the folder that holds it and nowhere else");
    }
}
