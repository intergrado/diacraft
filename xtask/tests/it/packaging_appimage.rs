//! The AppImage (`packaging/linux/make-appimage.sh`): what it puts in the AppDir, what it asks `appimagetool` for, and what it refuses. The tool is a stand-in here, a
//! script that writes down how it was run and what it was given, so that nothing is downloaded and nothing needs FUSE; the real tool and a real AppImage are for the
//! release workflow.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::TempDir;
use crate::packaging::{elf, names, programs, say};

/// A stand-in for `appimagetool`: it writes its environment's ARCH and VERSION and its arguments to `record`, lists the AppDir it was given, and writes the file it was
/// asked for.
const FAKE_TOOL: &str = r#"#!/bin/sh
appdir=""
last=""
for argument in "$@"; do
    appdir=$last
    last=$argument
done
{
    echo "ARCH=$ARCH"
    echo "VERSION=$VERSION"
    echo "ARGS=$*"
    (cd "$appdir" && find . | sort)
} >"$(dirname "$0")/record"
echo "an AppImage" >"$last"
"#;

fn run(dir: &TempDir, name: &str, tool: Option<&Path>, runtime: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .env("PATH", "/usr/bin:/bin")
        .env_remove("APPIMAGETOOL")
        .env_remove("APPIMAGE_RUNTIME")
        .arg(xtask::root().join("packaging/linux/make-appimage.sh"))
        .arg(name)
        .arg(dir.path().join("out"))
        .arg(dir.path().join("bin"));
    if let Some(tool) = tool {
        command.env("APPIMAGETOOL", tool);
    }
    if let Some(runtime) = runtime {
        command.env("APPIMAGE_RUNTIME", runtime);
    }
    command.output().unwrap()
}

/// A scratch folder with the stand-in tool and a runtime file, and the two programs of CPU `machine`.
fn setup(machine: u8) -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = TempDir::new("make-appimage");
    programs(&dir, &elf(machine), &elf(machine), "");
    dir.write("tools/appimagetool", FAKE_TOOL);
    std::fs::set_permissions(dir.path().join("tools/appimagetool"), std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.write("tools/runtime", "a runtime");
    let tool = dir.path().join("tools/appimagetool");
    let runtime = dir.path().join("tools/runtime");
    (dir, tool, runtime)
}

#[test]
fn an_appimage_is_made_for_the_cpu_of_its_programs_with_the_runtime_it_was_given() {
    for (machine, arch, tool_arch) in [(0x3e, "x86_64", "x86_64"), (0xb7, "arm64", "aarch64")] {
        let (dir, tool, runtime) = setup(machine);
        let out = run(&dir, "v0.2.0", Some(&tool), Some(&runtime));
        assert!(out.status.success(), "{arch}: {}", say(&out));
        let file = format!("diacraft-v0.2.0-linux-{arch}.AppImage");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()]);
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&file), "the last line is the file: {}", say(&out));
        let record = std::fs::read_to_string(dir.path().join("tools/record")).unwrap();
        assert!(record.contains(&format!("ARCH={tool_arch}\n")), "{arch}: {record}");
        assert!(record.contains("VERSION=0.2.0\n"), "{record}");
        assert!(record.contains("--appimage-extract-and-run"), "the tool is an AppImage, and the runner has no FUSE: {record}");
        assert!(
            record.contains(&format!("--runtime-file {}", runtime.display())),
            "the runtime is the pinned one, not one the tool fetches: {record}"
        );
        assert!(std::fs::metadata(dir.path().join("out").join(&file)).unwrap().permissions().mode() & 0o111 != 0, "the file can be run");
    }
}

#[test]
fn the_appdir_holds_both_programs_the_entry_the_icon_the_licences_and_an_apprun_that_can_start_the_command_line() {
    let (dir, tool, runtime) = setup(0x3e);
    let out = run(&dir, "v0.2.0", Some(&tool), Some(&runtime));
    assert!(out.status.success(), "{}", say(&out));
    let record = std::fs::read_to_string(dir.path().join("tools/record")).unwrap();
    for path in [
        "./AppRun",
        "./app.diacraft.desktop",
        "./app.diacraft.png",
        "./.DirIcon",
        "./usr/bin/diacraft",
        "./usr/bin/diacraft-cli",
        "./usr/share/applications/app.diacraft.desktop",
        "./usr/share/icons/hicolor/256x256/apps/app.diacraft.png",
        "./usr/share/doc/diacraft/LICENSE-MIT",
        "./usr/share/doc/diacraft/LICENSE-APACHE",
        "./usr/share/doc/diacraft/NOTICE",
        "./usr/share/doc/diacraft/ATTRIBUTION.md",
        "./usr/share/doc/diacraft/THIRD-PARTY-LICENSES.txt",
    ] {
        assert!(record.lines().any(|l| l == path), "{path} is not in the AppDir: {record}");
    }
    // AppRun is a script of its own: run with a stand-in for each program, it starts the window program, the command line for --cli, and the command line for a name.
    let apprun = dir.path().join("apprun");
    std::fs::create_dir_all(apprun.join("usr/bin")).unwrap();
    for program in ["diacraft", "diacraft-cli"] {
        std::fs::write(apprun.join("usr/bin").join(program), format!("#!/bin/sh\necho {program} \"$@\"\n")).unwrap();
        std::fs::set_permissions(apprun.join("usr/bin").join(program), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The script is the one the packaging script writes: read from the script itself.
    let source = std::fs::read_to_string(xtask::root().join("packaging/linux/make-appimage.sh")).unwrap();
    let script = source.split("<<'APPRUN'\n").nth(1).and_then(|rest| rest.split("\nAPPRUN\n").next()).expect("AppRun is in the script");
    std::fs::write(apprun.join("AppRun"), format!("{script}\n")).unwrap();
    std::fs::set_permissions(apprun.join("AppRun"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let start = |argv0: Option<&str>, args: &[&str]| {
        let mut command = Command::new(apprun.join("AppRun"));
        command.args(args).env_remove("ARGV0");
        if let Some(argv0) = argv0 {
            command.env("ARGV0", argv0);
        }
        String::from_utf8(command.output().unwrap().stdout).unwrap()
    };
    assert_eq!(start(None, &["a.diacraft"]), "diacraft a.diacraft\n");
    assert_eq!(start(Some("./DiaCraft.AppImage"), &["--version"]), "diacraft --version\n");
    assert_eq!(start(Some("./x.AppImage"), &["--cli", "--version"]), "diacraft-cli --version\n");
    assert_eq!(start(Some("diacraft-cli"), &["convert", "a"]), "diacraft-cli convert a\n");
    assert_eq!(start(Some("/usr/local/bin/diacraft-cli"), &["--version"]), "diacraft-cli --version\n");
    assert_eq!(start(Some("not-diacraft-cli-at-all"), &["--version"]), "diacraft --version\n", "only the name diacraft-cli starts the command line");
}

#[test]
fn an_appimage_needs_the_tool_and_the_runtime_that_the_workflow_checked() {
    let (dir, tool, runtime) = setup(0x3e);
    for (given_tool, given_runtime, said) in [
        (None, Some(&runtime), "APPIMAGETOOL must be the path of appimagetool"),
        (Some(&tool), None, "APPIMAGE_RUNTIME must be the path of the type 2 runtime"),
        (Some(&dir.path().join("tools/missing")), Some(&runtime), "APPIMAGETOOL must be the path of appimagetool"),
    ] {
        let out = run(&dir, "v0.2.0", given_tool.map(|p| p.as_path()), given_runtime.map(|p| p.as_path()));
        assert!(!out.status.success(), "{}", say(&out));
        assert!(say(&out).contains(said), "{}", say(&out));
    }
    assert!(!dir.path().join("out").join("diacraft-v0.2.0-linux-x86_64.AppImage").exists());
}

#[test]
fn programs_of_two_kinds_are_not_one_appimage() {
    let (dir, tool, runtime) = setup(0x3e);
    dir.write_bytes("bin/diacraft-cli", &elf(0xb7));
    let out = run(&dir, "v0.2.0", Some(&tool), Some(&runtime));
    assert!(!out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("one download holds one architecture"), "{}", say(&out));
}

/// The two tools the workflow downloads to make an AppImage are pinned by version and by the checksum of each CPU's file (the rule for a tool downloaded in a workflow, as
/// `site` and `ci.yml` do for theirs), and never taken from a rolling release.
#[test]
fn the_workflow_pins_the_appimage_tool_and_runtime_by_version_and_by_a_checksum_for_each_cpu() {
    let workflow = std::fs::read_to_string(xtask::root().join(".github/workflows/release.yml")).unwrap();
    for key in ["appimagetool_sha256", "appimage_runtime_sha256"] {
        let sums: Vec<&str> = workflow.lines().map(str::trim).filter_map(|l| l.strip_prefix(&format!("{key}: "))).collect();
        assert_eq!(sums.len(), 2, "{key}: one for each CPU of the Linux job: {sums:?}");
        for sum in &sums {
            assert!(sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{key} is `{sum}`");
        }
        assert_ne!(sums[0], sums[1], "{key}: the two CPUs have two files");
    }
    let value = |name: &str| {
        let prefix = format!("{name}: ");
        workflow
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("release.yml has no `{name}:` line"))
            .trim_matches('"')
            .to_owned()
    };
    let tool = value("APPIMAGETOOL_VERSION");
    assert!(
        tool.split('.').count() == 3 && tool.split('.').all(|n| n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty()),
        "a numbered release of the tool: {tool}"
    );
    let runtime = value("APPIMAGE_RUNTIME_VERSION");
    assert!(runtime.len() == 8 && runtime.bytes().all(|b| b.is_ascii_digit()), "a dated release of the runtime: {runtime}");
    assert!(!workflow.contains("download/continuous"), "a rolling release is not a version");
    assert!(workflow.contains("sha256sum --check --strict"), "each download is checked");
}
