//! The MCP bundles (`packaging/mcp-bundle.sh` makes one for a system and a CPU, `packaging/mcp-bundle-check.sh` unpacks one and runs what is in it): what is in a bundle, how it
//! is named, what its manifest says, and what the two scripts refuse. The programs here are the first bytes of a real program (what the script reads to say its CPU), or a
//! shell script that answers like the server; a real program, bundled, unpacked and run, is the release workflow's step and the task's own check.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use crate::TempDir;
use crate::packaging::{elf, names, pe, say};

fn have_zip() -> bool {
    ["zip", "unzip"].iter().all(|tool| Command::new(tool).arg("-v").output().is_ok())
}

fn workspace_version() -> String {
    xtask::version::read(&std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap()).unwrap()
}

/// Runs `packaging/mcp-bundle.sh PLATFORM NAME OUT BIN` on a plain `PATH` (a machine's own `od` ahead of the system's is not what the runners have), with `tools` in front of it
/// when there is a folder of stand-in tools, and `env` set.
fn make(platform: &str, name: &str, out: &Path, bin: &Path, tools: Option<&Path>, env: &[(&str, &str)]) -> Output {
    let path = match tools {
        Some(tools) => format!("{}:/usr/bin:/bin", tools.display()),
        None => "/usr/bin:/bin".to_owned(),
    };
    let mut command = Command::new("bash");
    command.env("PATH", path).env_remove("MACOS_SIGN_IDENTITY").env_remove("DIACRAFT_REPOSITORY");
    for (key, value) in env {
        command.env(key, value);
    }
    command.arg(xtask::root().join("packaging/mcp-bundle.sh")).args([platform, name]).arg(out).arg(bin).output().unwrap()
}

/// A folder `bin` with the command line, as `bytes`, named `diacraft-cli` and `exe`.
fn program(dir: &TempDir, bytes: &[u8], exe: &str) {
    dir.write_bytes(&format!("bin/diacraft-cli{exe}"), bytes);
}

/// The names in a bundle, in the order of the archive, and the text of one of its files.
fn listing(bundle: &Path) -> Vec<String> {
    let out = Command::new("unzip").arg("-Z1").arg(bundle).output().unwrap();
    assert!(out.status.success(), "{}", say(&out));
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

fn bytes_of(bundle: &Path, file: &str) -> Vec<u8> {
    let out = Command::new("unzip").arg("-p").arg(bundle).arg(file).output().unwrap();
    assert!(out.status.success(), "{file}: {}", say(&out));
    out.stdout
}

fn text_of(bundle: &Path, file: &str) -> String {
    String::from_utf8(bytes_of(bundle, file)).unwrap()
}

fn manifest_of(bundle: &Path) -> Value {
    serde_json::from_str(&text_of(bundle, "manifest.json")).unwrap()
}

const LICENCES: [&str; 5] = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md", "THIRD-PARTY-LICENSES.txt"];

#[test]
fn a_linux_bundle_is_named_for_the_cpu_of_its_program_and_holds_the_program_the_manifest_the_icon_and_the_licences() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: the bundle is not made");
        return;
    }
    for (machine, cpu) in [(0x3e, "x86_64"), (0xb7, "aarch64")] {
        let dir = TempDir::new("mcp-bundle-linux");
        program(&dir, &elf(machine), "");
        let out = make("linux", "dev-test", &dir.path().join("out"), &dir.path().join("bin"), None, &[]);
        assert!(out.status.success(), "{cpu}: {}", say(&out));
        let file = format!("diacraft-mcp-dev-test-linux-{cpu}.mcpb");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()], "the machine of the build is not what names it");
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with(&file), "the last line is the bundle: {}", say(&out));
        let bundle = dir.path().join("out").join(&file);
        // Nothing else: the manifest at the root, the icon, the program in `server/`, and the five licence files.
        let mut held = listing(&bundle);
        held.sort();
        let mut wanted: Vec<String> = ["manifest.json", "icon.png", "server/", "server/diacraft-cli"].iter().map(|s| (*s).to_owned()).collect();
        wanted.extend(LICENCES.iter().map(|s| (*s).to_owned()));
        wanted.sort();
        assert_eq!(held, wanted, "{cpu}");
        // The program is runnable as it is unpacked: a bundle's archive keeps the mode.
        let zipinfo = Command::new("unzip").arg("-Z").arg(&bundle).output().unwrap();
        let zipinfo = String::from_utf8(zipinfo.stdout).unwrap();
        assert!(zipinfo.lines().any(|l| l.starts_with("-rwxr-xr-x") && l.ends_with("server/diacraft-cli")), "{zipinfo}");
        assert_eq!(bytes_of(&bundle, "server/diacraft-cli"), elf(machine));
        assert_eq!(&bytes_of(&bundle, "icon.png")[..8], b"\x89PNG\r\n\x1a\n");
        for licence in LICENCES {
            assert_eq!(text_of(&bundle, licence), std::fs::read_to_string(xtask::root().join(licence)).unwrap(), "{licence}");
        }
    }
}

#[test]
fn a_windows_bundle_holds_the_exe_and_says_win32() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: the bundle is not made");
        return;
    }
    for (machine, cpu) in [(0x8664, "x64"), (0xaa64, "arm64")] {
        let dir = TempDir::new("mcp-bundle-windows");
        program(&dir, &pe(machine), ".exe");
        let out = make("windows", "dev-test", &dir.path().join("out"), &dir.path().join("bin"), None, &[]);
        assert!(out.status.success(), "{cpu}: {}", say(&out));
        let file = format!("diacraft-mcp-dev-test-windows-{cpu}.mcpb");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()]);
        let bundle = dir.path().join("out").join(&file);
        assert!(listing(&bundle).contains(&"server/diacraft-cli.exe".to_owned()), "{:?}", listing(&bundle));
        let manifest = manifest_of(&bundle);
        assert_eq!(manifest["server"]["entry_point"], "server/diacraft-cli.exe");
        assert_eq!(manifest["server"]["mcp_config"]["command"], "${__dirname}/server/diacraft-cli.exe");
        assert_eq!(manifest["compatibility"]["platforms"], serde_json::json!(["win32"]));
    }
}

/// Stand-ins for the two macOS tools the script uses: `lipo -archs` says `arch`, and `codesign` writes its arguments to `codesign.log` beside it.
fn macos_tools(dir: &TempDir, arch: &str) -> PathBuf {
    dir.write("tools/lipo", &format!("#!/bin/sh\necho {arch}\n"));
    dir.write("tools/codesign", "#!/bin/sh\necho \"$*\" >>\"$(dirname \"$0\")/codesign.log\"\n");
    for tool in ["lipo", "codesign"] {
        std::fs::set_permissions(dir.path().join("tools").join(tool), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir.path().join("tools")
}

#[test]
fn a_macos_bundle_is_named_for_the_architecture_lipo_reads_and_its_program_is_signed() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: the bundle is not made");
        return;
    }
    for arch in ["arm64", "x86_64"] {
        let dir = TempDir::new("mcp-bundle-macos");
        program(&dir, b"a mach-o program", "");
        let tools = macos_tools(&dir, arch);
        let out = make("macos", "dev-test", &dir.path().join("out"), &dir.path().join("bin"), Some(&tools), &[]);
        assert!(out.status.success(), "{arch}: {}", say(&out));
        let file = format!("diacraft-mcp-dev-test-macos-{arch}.mcpb");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()]);
        let manifest = manifest_of(&dir.path().join("out").join(&file));
        assert_eq!(manifest["compatibility"]["platforms"], serde_json::json!(["darwin"]));
        assert_eq!(manifest["server"]["entry_point"], "server/diacraft-cli");
        // Signed ad hoc, as the macOS download's program is: an Apple silicon Mac runs nothing unsigned.
        let log = std::fs::read_to_string(tools.join("codesign.log")).unwrap();
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.starts_with("--force --sign - ") && log.contains("server/diacraft-cli") && !log.contains("--options"), "{log}");
    }
    // With an identity: that identity, the hardened runtime and a timestamp.
    let dir = TempDir::new("mcp-bundle-macos-identity");
    program(&dir, b"a mach-o program", "");
    let tools = macos_tools(&dir, "arm64");
    let out = make(
        "macos",
        "dev-test",
        &dir.path().join("out"),
        &dir.path().join("bin"),
        Some(&tools),
        &[("MACOS_SIGN_IDENTITY", "Developer ID Application: X (T)")],
    );
    assert!(out.status.success(), "{}", say(&out));
    let log = std::fs::read_to_string(tools.join("codesign.log")).unwrap();
    assert!(log.contains("--sign Developer ID Application: X (T)") && log.contains("--options runtime") && log.contains("--timestamp"), "{log}");
    // A program that holds two architectures is not one bundle's.
    let dir = TempDir::new("mcp-bundle-macos-two");
    program(&dir, b"a mach-o program", "");
    let tools = macos_tools(&dir, "x86_64 arm64");
    let out = make("macos", "dev-test", &dir.path().join("out"), &dir.path().join("bin"), Some(&tools), &[]);
    assert!(!out.status.success() && say(&out).contains("expected one architecture"), "{}", say(&out));
}

#[test]
fn the_v_of_a_tag_is_in_no_name_and_the_manifest_says_the_version_of_the_tag_or_the_workspaces() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: the bundle is not made");
        return;
    }
    let workspace = workspace_version();
    for (name, in_name, version) in [
        ("v0.9.9", "0.9.9", "0.9.9"),
        ("0.9.9", "0.9.9", "0.9.9"),
        ("v0.9.9-rc.1", "0.9.9-rc.1", "0.9.9-rc.1"),
        // A label that is no tag (a rehearsal's dev-<commit>, a test build's `ci`) is kept in the name; the manifest, which wants a version, says the workspace's.
        ("dev-1a2b3c4", "dev-1a2b3c4", workspace.as_str()),
        ("ci", "ci", workspace.as_str()),
    ] {
        let dir = TempDir::new("mcp-bundle-version");
        program(&dir, &elf(0x3e), "");
        let out = make("linux", name, &dir.path().join("out"), &dir.path().join("bin"), None, &[]);
        assert!(out.status.success(), "{name}: {}", say(&out));
        let file = format!("diacraft-mcp-{in_name}-linux-x86_64.mcpb");
        assert_eq!(names(&dir.path().join("out")), [file.as_str()], "{name}");
        assert_eq!(manifest_of(&dir.path().join("out").join(&file))["version"], version, "{name}");
    }
}

#[test]
fn the_manifest_is_the_template_with_every_word_filled_in_and_the_repository_of_the_workspace() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: the bundle is not made");
        return;
    }
    let dir = TempDir::new("mcp-bundle-manifest");
    program(&dir, &elf(0x3e), "");
    // The repository the build ran in (DIACRAFT_REPOSITORY, which the READMEs of the downloads say) is not the bundle's: a bundle is a public file, and says where DiaCraft is.
    let out = make("linux", "v0.9.9", &dir.path().join("out"), &dir.path().join("bin"), None, &[("DIACRAFT_REPOSITORY", "someone/private-copy")]);
    assert!(out.status.success(), "{}", say(&out));
    let text = text_of(&dir.path().join("out/diacraft-mcp-0.9.9-linux-x86_64.mcpb"), "manifest.json");
    assert!(!text.contains('@'), "a word is left unfilled: {text}");
    let manifest: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["manifest_version"], "0.3");
    assert_eq!(manifest["name"], "diacraft");
    assert_eq!(manifest["version"], "0.9.9");
    assert_eq!(manifest["server"]["type"], "binary");
    assert_eq!(manifest["server"]["entry_point"], "server/diacraft-cli");
    assert_eq!(manifest["server"]["mcp_config"]["command"], "${__dirname}/server/diacraft-cli");
    assert_eq!(manifest["server"]["mcp_config"]["args"], serde_json::json!(["mcp"]));
    assert_eq!(manifest["compatibility"]["platforms"], serde_json::json!(["linux"]));
    assert_eq!(manifest["license"], "MIT OR Apache-2.0");
    assert_eq!(manifest["homepage"], "https://diacraft.dev");
    let cargo: toml::Table = std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap().parse().unwrap();
    let repository = cargo["workspace"]["package"]["repository"].as_str().unwrap();
    assert_eq!(manifest["repository"], serde_json::json!({"type": "git", "url": repository}));
    assert_eq!(manifest["support"], format!("{repository}/issues").as_str());
    assert_eq!(manifest["icon"], "icon.png");
}

#[test]
fn a_bundle_is_not_made_of_what_is_not_a_program_of_the_system_nor_of_a_name_that_is_no_name() {
    let dir = TempDir::new("mcp-bundle-refusals");
    let (out_dir, bin) = (dir.path().join("out"), dir.path().join("bin"));
    // A Windows program in a Linux bundle, a Linux program in a Windows one, an ELF of another machine, a text file.
    for (platform, exe, bytes, said) in [
        ("linux", "", pe(0x8664), "is not a 64-bit little endian ELF program"),
        ("linux", "", elf(0x28), "ELF machine 0x2800, which is neither x86-64 nor AArch64"),
        ("linux", "", b"#!/bin/sh\necho hi\n".to_vec(), "is not a 64-bit little endian ELF program"),
        ("windows", ".exe", elf(0x3e), "is not a Windows program"),
        ("windows", ".exe", pe(0x14c), "PE machine 0x4c01, which is neither x64 nor ARM64"),
    ] {
        program(&dir, &bytes, exe);
        let out = make(platform, "dev-test", &out_dir, &bin, None, &[]);
        assert!(!out.status.success() && say(&out).contains(said), "{platform}: {}", say(&out));
    }
    program(&dir, &elf(0x3e), "");
    for (args, said) in [
        (vec!["other", "dev-test"], "PLATFORM must be linux, windows or macos: 'other'"),
        (vec!["linux", "v0.2.1/../x"], "NAME must be letters, digits"),
        (vec!["linux", ""], "NAME must be letters, digits"),
    ] {
        let out = Command::new("bash")
            .env("PATH", "/usr/bin:/bin")
            .arg(xtask::root().join("packaging/mcp-bundle.sh"))
            .args(&args)
            .arg(&out_dir)
            .arg(&bin)
            .output()
            .unwrap();
        assert!(!out.status.success() && say(&out).contains(said), "{args:?}: {}", say(&out));
    }
    // The program is not there, the folder is not there, and there are no arguments.
    std::fs::remove_file(bin.join("diacraft-cli")).unwrap();
    let out = make("linux", "dev-test", &out_dir, &bin, None, &[]);
    assert!(!out.status.success() && say(&out).contains("missing binary"), "{}", say(&out));
    let out = make("linux", "dev-test", &out_dir, &dir.path().join("none"), None, &[]);
    assert!(!out.status.success() && say(&out).contains("no such folder"), "{}", say(&out));
    let out = Command::new("bash").arg(xtask::root().join("packaging/mcp-bundle.sh")).output().unwrap();
    assert!(!out.status.success() && say(&out).contains("usage: mcp-bundle.sh PLATFORM NAME OUT_DIR BIN_DIR"), "{}", say(&out));
    assert!(!out_dir.exists() || names(&out_dir).is_empty(), "nothing is written for a refused bundle: {:?}", names(&out_dir));
}

// ------------------------------------------------------------------------------------------------------------------------ the template

fn template() -> Value {
    let text = std::fs::read_to_string(xtask::root().join("packaging/mcp/manifest.json")).unwrap();
    // The words the script fills in are inside strings, so the template is JSON as it is.
    serde_json::from_str(&text).unwrap()
}

/// The keys of an object, sorted.
fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys
}

/// The format of a bundle is a strict schema (MCPB 0.3: `z.strictObject`): a key that it does not name fails the validation of the whole manifest, so every key here is one of
/// the manifest specification's (modelcontextprotocol/mcpb, MANIFEST.md).
#[test]
fn the_template_has_only_keys_the_format_names_and_the_keys_it_requires() {
    let manifest = template();
    let known = [
        "$schema",
        "_meta",
        "author",
        "compatibility",
        "description",
        "display_name",
        "documentation",
        "homepage",
        "icon",
        "icons",
        "keywords",
        "license",
        "localization",
        "long_description",
        "manifest_version",
        "name",
        "privacy_policies",
        "prompts",
        "prompts_generated",
        "repository",
        "screenshots",
        "server",
        "support",
        "tools",
        "tools_generated",
        "user_config",
        "version",
    ];
    for key in keys(&manifest) {
        assert!(known.contains(&key), "`{key}` is no key of the format");
    }
    for required in ["manifest_version", "name", "version", "description", "author", "server"] {
        assert!(manifest.get(required).is_some(), "`{required}` is required");
    }
    assert_eq!(keys(&manifest["server"]), ["entry_point", "mcp_config", "type"]);
    assert_eq!(keys(&manifest["server"]["mcp_config"]), ["args", "command", "env"]);
    assert_eq!(keys(&manifest["compatibility"]), ["platforms"]);
    assert_eq!(keys(&manifest["author"]), ["name", "url"]);
    assert_eq!(keys(&manifest["repository"]), ["type", "url"]);
    for tool in manifest["tools"].as_array().unwrap() {
        assert_eq!(keys(tool), ["description", "name"]);
    }
    assert!(manifest["description"].as_str().unwrap().len() < 200 && manifest["keywords"].as_array().unwrap().iter().all(Value::is_string));
}

/// The tools the manifest declares are the tools the server has (`tools_generated` is false: the list is whole), and the page about the server lists the same six.
#[test]
fn the_template_declares_the_six_tools_of_the_server_the_page_about_it_lists() {
    let manifest = template();
    assert_eq!(manifest["tools_generated"], false);
    let declared: Vec<&str> = manifest["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    let page = std::fs::read_to_string(xtask::root().join("docs/mcp.md")).unwrap();
    let section = page.split("\n## The tools\n").nth(1).unwrap().split("\n## ").next().unwrap();
    let listed: Vec<&str> = section.lines().filter_map(|line| line.strip_prefix("| `")).filter_map(|rest| rest.split('`').next()).collect();
    assert_eq!(declared, listed, "the manifest's tools and the tools docs/mcp.md lists");
    assert_eq!(declared, ["command_list", "command_run", "command_batch", "doc_inspect", "ui_inspect", "ui_screenshot"]);
    // The shell reads a tool's name from the line that declares it: one tool to a line, the name first.
    for line in std::fs::read_to_string(xtask::root().join("packaging/mcp/manifest.json"))
        .unwrap()
        .lines()
        .filter(|l| l.trim_start().starts_with("{ \"name\""))
    {
        assert!(line.trim_end().ends_with("},") || line.trim_end().ends_with('}'), "{line}");
    }
}

/// The words the script fills in are the words of the template, and no other is in it.
#[test]
fn the_script_fills_every_word_the_template_has() {
    let text = std::fs::read_to_string(xtask::root().join("packaging/mcp/manifest.json")).unwrap();
    let script = std::fs::read_to_string(xtask::root().join("packaging/mcp-bundle.sh")).unwrap();
    let words: std::collections::BTreeSet<&str> = text.split('@').skip(1).step_by(2).collect();
    assert_eq!(words.iter().copied().collect::<Vec<_>>(), ["PLATFORM", "PROGRAM", "REPO_URL", "VERSION"]);
    for word in words {
        assert!(script.contains(&format!("s|@{word}@|")), "mcp-bundle.sh fills no @{word}@");
    }
}

// -------------------------------------------------------------------------------------------------------------------------- the check

/// The server's answers to `initialize` and `tools/list`, as the real server words them (keys in alphabetical order), with the six tools.
fn tools_reply(names: &[&str]) -> String {
    let tools: Vec<String> = names.iter().map(|name| format!("{{\"annotations\":{{}},\"description\":\"d\",\"inputSchema\":{{\"properties\":{{}},\"type\":\"object\"}},\"name\":\"{name}\",\"title\":\"t\"}}")).collect();
    format!("{{\"id\":2,\"jsonrpc\":\"2.0\",\"result\":{{\"tools\":[{}]}}}}", tools.join(","))
}

const SIX: [&str; 6] = ["command_list", "command_run", "command_batch", "doc_inspect", "ui_inspect", "ui_screenshot"];

/// A shell script that answers as the command line does: `--version`, and `mcp` on its standard input.
fn stand_in(version: &str, initialize: &str, tools: &str) -> String {
    format!(
        "#!/bin/sh\ncase \"$1\" in\n--version) echo 'diacraft-cli {version}' ;;\nmcp)\n  while read -r line; do\n    case \"$line\" in\n      *'\"initialize\"'*) echo '{initialize}' ;;\n      *'\"tools/list\"'*) echo '{tools}' ;;\n    esac\n  done ;;\nesac\n"
    )
}

const INITIALIZE: &str = "{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"capabilities\":{\"tools\":{\"listChanged\":false}},\"instructions\":\"i\",\"protocolVersion\":\"2024-11-05\",\"serverInfo\":{\"name\":\"diacraft\",\"version\":\"0.0.0\"}}}";

/// A Linux bundle made by the script, with `files` changed after it: (path in the bundle, Some(text) to write it as a program, None to remove it). Returns the bundle's path.
fn bundle_with(dir: &TempDir, program: &str, changes: &[(&str, Option<&str>)]) -> PathBuf {
    program_for_make(dir);
    let out = make("linux", "dev-test", &dir.path().join("out"), &dir.path().join("bin"), None, &[]);
    assert!(out.status.success(), "{}", say(&out));
    let bundle = dir.path().join("out/diacraft-mcp-dev-test-linux-x86_64.mcpb");
    let folder = dir.path().join("unpacked");
    let unzip = Command::new("unzip").arg("-q").arg(&bundle).arg("-d").arg(&folder).output().unwrap();
    assert!(unzip.status.success(), "{}", say(&unzip));
    std::fs::write(folder.join("server/diacraft-cli"), program).unwrap();
    std::fs::set_permissions(folder.join("server/diacraft-cli"), std::fs::Permissions::from_mode(0o755)).unwrap();
    for (path, content) in changes {
        match content {
            Some(text) => std::fs::write(folder.join(path), text).unwrap(),
            None => std::fs::remove_file(folder.join(path)).unwrap(),
        }
    }
    std::fs::remove_file(&bundle).unwrap();
    let zip = Command::new("zip").current_dir(&folder).args(["-qr", "-X"]).arg(&bundle).arg(".").output().unwrap();
    assert!(zip.status.success(), "{}", say(&zip));
    bundle
}

fn program_for_make(dir: &TempDir) {
    program(dir, &elf(0x3e), "");
}

/// Runs `packaging/mcp-bundle-check.sh [FLAGS] BUNDLE [TAG]`, with `env` set.
fn check(flags: &[&str], bundle: &Path, tag: Option<&str>, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new("bash");
    command.env("PATH", "/usr/bin:/bin").env_remove("RUN_WITH");
    for (key, value) in env {
        command.env(key, value);
    }
    command.arg(xtask::root().join("packaging/mcp-bundle-check.sh")).args(flags).arg(bundle).args(tag).output().unwrap()
}

#[test]
fn a_bundle_that_answers_as_the_server_does_passes_the_check() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: no bundle is made");
        return;
    }
    let version = workspace_version();
    let dir = TempDir::new("mcp-check-ok");
    let bundle = bundle_with(&dir, &stand_in(&version, INITIALIZE, &tools_reply(&SIX)), &[]);
    let out = check(&[], &bundle, None, &[]);
    assert!(out.status.success(), "{}", say(&out));
    assert!(say(&out).contains("mcp-bundle-check: ok"), "{}", say(&out));
    // With the tag it is a bundle of: the version of the manifest and the program's own are the tag's.
    let tagged = check(&[], &bundle, Some(&format!("v{version}")), &[]);
    assert!(tagged.status.success(), "{}", say(&tagged));
    // With no run: the files are looked at and nothing is started (an Intel program on a Mac that has no Rosetta).
    let dir = TempDir::new("mcp-check-norun");
    let bundle = bundle_with(&dir, "#!/bin/sh\nexit 1\n", &[]);
    let out = check(&["--no-run"], &bundle, None, &[]);
    assert!(out.status.success() && say(&out).contains("not run"), "{}", say(&out));
}

#[test]
fn the_check_refuses_a_bundle_that_is_not_what_the_manifest_says_it_is() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: no bundle is made");
        return;
    }
    let version = workspace_version();
    let good = stand_in(&version, INITIALIZE, &tools_reply(&SIX));
    let five = tools_reply(&SIX[..5]);
    let seven = tools_reply(&[&SIX[..], &["file_open"]].concat());
    for (what, program, changes, said) in [
        ("a program that says another version", stand_in("0.0.1", INITIALIZE, &tools_reply(&SIX)), vec![], "the program says"),
        ("no answer to initialize", stand_in(&version, "", &tools_reply(&SIX)), vec![], "did not answer initialize"),
        ("a tool the manifest declares and the server lacks", stand_in(&version, INITIALIZE, &five), vec![], "tools differ"),
        ("a tool the server has and the manifest does not declare", stand_in(&version, INITIALIZE, &seven), vec![], "tools differ"),
        ("no licence file", good.clone(), vec![("NOTICE", None)], "NOTICE"),
        ("an empty licence file", good.clone(), vec![("LICENSE-MIT", Some(""))], "LICENSE-MIT"),
        ("no icon", good.clone(), vec![("icon.png", None)], "icon.png"),
        ("an icon that is no picture", good.clone(), vec![("icon.png", Some("not a picture"))], "icon.png"),
        ("no manifest", good.clone(), vec![("manifest.json", None)], "manifest.json"),
    ] {
        let dir = TempDir::new("mcp-check-bad");
        let bundle = bundle_with(&dir, &program, &changes);
        let out = check(&[], &bundle, None, &[]);
        assert!(!out.status.success(), "{what}: {}", say(&out));
        assert!(say(&out).contains("mcp-bundle-check: FAILED") && say(&out).contains(said), "{what}: {}", say(&out));
    }
    // A manifest that is another version than the tag it is asked about, and one for another system than its name says.
    let dir = TempDir::new("mcp-check-tag");
    let bundle = bundle_with(&dir, &good, &[]);
    let wrong_tag = check(&[], &bundle, Some("v9.9.9"), &[]);
    assert!(!wrong_tag.status.success() && say(&wrong_tag).contains("9.9.9"), "{}", say(&wrong_tag));
    let manifest = text_of(&bundle, "manifest.json").replace("\"linux\"", "\"win32\"");
    let dir = TempDir::new("mcp-check-platform");
    let bundle = bundle_with(&dir, &good, &[("manifest.json", Some(&manifest))]);
    let out = check(&[], &bundle, None, &[]);
    assert!(!out.status.success() && say(&out).contains("win32"), "{}", say(&out));
    // A folder that is no bundle, a file that is no archive, and an archive that is no `.mcpb`.
    let out = check(&[], dir.path(), None, &[]);
    assert!(!out.status.success() && say(&out).contains("no such file"), "{}", say(&out));
    dir.write("a.mcpb", "not an archive");
    let out = check(&[], &dir.path().join("a.mcpb"), None, &[]);
    assert!(!out.status.success(), "{}", say(&out));
    dir.write("a.zip", "x");
    let out = check(&[], &dir.path().join("a.zip"), None, &[]);
    assert!(!out.status.success() && say(&out).contains(".mcpb"), "{}", say(&out));
}

#[test]
fn the_check_runs_the_program_with_the_prefix_it_is_given() {
    if !have_zip() {
        eprintln!("zip and unzip are not here: no bundle is made");
        return;
    }
    // An Intel program on an Apple silicon Mac is run as `arch -x86_64 <program>`: the prefix is whatever the workflow says. Here a stand-in prefix that writes down that it ran.
    let version = workspace_version();
    let dir = TempDir::new("mcp-check-prefix");
    let bundle = bundle_with(&dir, &stand_in(&version, INITIALIZE, &tools_reply(&SIX)), &[]);
    dir.write("prefix.sh", "#!/bin/sh\necho \"$*\" >>\"$(dirname \"$0\")/prefix.log\"\nexec \"$@\"\n");
    std::fs::set_permissions(dir.path().join("prefix.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let prefix = dir.path().join("prefix.sh");
    let out = check(&[], &bundle, None, &[("RUN_WITH", prefix.to_str().unwrap())]);
    assert!(out.status.success(), "{}", say(&out));
    let log = std::fs::read_to_string(dir.path().join("prefix.log")).unwrap();
    assert!(
        log.lines().any(|l| l.ends_with("server/diacraft-cli --version")) && log.lines().any(|l| l.ends_with("server/diacraft-cli mcp")),
        "{log}"
    );
}
