//! `cargo xtask mcp-server-json`: the registry entry (`server.json`) that `mcp-publisher publish` sends, made from the release's `SHA256SUMS.txt`. The hashes are known only
//! after the release is built, so the file is made from the sums and never written by hand. Nothing here reaches the network.

use std::process::Command;

use serde_json::Value;
use xtask::mcp::{self, BUNDLES};

use crate::TempDir;

fn workspace_version() -> String {
    xtask::version::read(&std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap()).unwrap()
}

/// A digest as `sha256sum` writes it: 64 lower-case hexadecimal digits, a different one for each `n`.
fn digest(n: usize) -> String {
    format!("{:064x}", 0xabc0_0000_0000_u64 + n as u64)
}

/// The sums of a release of `version`: three of the downloads and the six bundles, in the order the shell sorts them (`sha256sum diacraft-*`).
fn sums(version: &str) -> String {
    let mut names: Vec<String> = vec![
        format!("diacraft-{version}-linux-x86_64.AppImage"),
        format!("diacraft-{version}-macos-arm64.dmg"),
        format!("diacraft-{version}-windows-x64.msi"),
    ];
    names.extend(BUNDLES.iter().map(|bundle| mcp::bundle_name(version, bundle)));
    names.sort();
    names.iter().enumerate().map(|(n, name)| format!("{}  {name}\n", digest(n))).collect()
}

fn hash_of(sums: &str, name: &str) -> String {
    sums.lines().find_map(|line| line.strip_suffix(&format!("  {name}")).map(str::to_owned)).unwrap()
}

const REPOSITORY: &str = "https://github.com/intergrado/diacraft";

fn entry(sums: &str) -> Result<Value, String> {
    let version = workspace_version();
    mcp::server_json(sums, &version, REPOSITORY).map(|text| serde_json::from_str(&text).unwrap())
}

#[test]
fn the_six_bundles_are_one_for_each_system_and_cpu_the_downloads_are_built_for() {
    let names: Vec<String> = BUNDLES.iter().map(|bundle| mcp::bundle_name("0.2.2", bundle)).collect();
    assert_eq!(
        names,
        [
            "diacraft-mcp-0.2.2-linux-x86_64.mcpb",
            "diacraft-mcp-0.2.2-linux-aarch64.mcpb",
            "diacraft-mcp-0.2.2-windows-x64.mcpb",
            "diacraft-mcp-0.2.2-windows-arm64.mcpb",
            "diacraft-mcp-0.2.2-macos-arm64.mcpb",
            "diacraft-mcp-0.2.2-macos-x86_64.mcpb",
        ]
    );
    // The registry wants "mcp" in the address of a bundle; the name has it twice over, as a word and as the extension's start.
    assert!(names.iter().all(|name| name.contains("mcp")));
    // The system a bundle says in `compatibility.platforms` is the word of the host language of the format (`process.platform`), and it is one for each system.
    let platforms: Vec<&str> = BUNDLES.iter().map(|bundle| bundle.platform).collect();
    assert_eq!(platforms, ["linux", "linux", "win32", "win32", "darwin", "darwin"]);
}

#[test]
fn the_entry_names_the_server_by_the_domain_and_each_bundle_by_its_address_and_hash() {
    let version = workspace_version();
    let sums = sums(&version);
    let entry = entry(&sums).unwrap();
    assert_eq!(entry["$schema"], "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json");
    // The reverse of diacraft.app, which the domain's file at /.well-known/mcp-registry-auth proves.
    assert_eq!(entry["name"], "app.diacraft/diacraft");
    assert_eq!(entry["title"], "DiaCraft");
    assert_eq!(entry["version"], version.as_str());
    assert_eq!(entry["websiteUrl"], "https://diacraft.dev");
    assert_eq!(entry["repository"], serde_json::json!({"url": REPOSITORY, "source": "github"}));
    let packages = entry["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 6);
    for (package, bundle) in packages.iter().zip(BUNDLES.iter()) {
        let name = mcp::bundle_name(&version, bundle);
        assert_eq!(package["registryType"], "mcpb", "{name}");
        // A file of the release by the address GitHub gives it: the tag has the `v`, the file name does not.
        assert_eq!(package["identifier"], format!("{REPOSITORY}/releases/download/v{version}/{name}"), "{name}");
        assert_eq!(package["fileSha256"], hash_of(&sums, &name).as_str(), "{name}");
        assert_eq!(package["version"], version.as_str(), "{name}");
        assert_eq!(package["transport"], serde_json::json!({"type": "stdio"}), "{name}");
    }
}

/// What the registry's schema and its documentation hold of an entry, written here as the checks of the published rules (2025-12-11).
#[test]
fn the_entry_meets_the_registrys_rules() {
    let version = workspace_version();
    let entry = entry(&sums(&version)).unwrap();
    let name = entry["name"].as_str().unwrap();
    let (namespace, server) = name.split_once('/').unwrap();
    let word = |text: &str, extra: &[char]| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(&c));
    assert!(word(namespace, &['.', '-']) && word(server, &['.', '_', '-']) && (3..=200).contains(&name.len()), "{name}");
    let description = entry["description"].as_str().unwrap();
    assert!(!description.is_empty() && description.chars().count() <= 100, "{} characters: {description}", description.chars().count());
    assert!(entry["title"].as_str().unwrap().chars().count() <= 100);
    assert!(entry["version"].as_str().unwrap().len() <= 255 && entry["version"] != "latest");
    for url in [entry["websiteUrl"].as_str().unwrap(), entry["repository"]["url"].as_str().unwrap()] {
        assert!(url.starts_with("https://"), "{url}");
    }
    for package in entry["packages"].as_array().unwrap() {
        let identifier = package["identifier"].as_str().unwrap();
        assert!(identifier.contains("mcp"), "the registry wants `mcp` in the address: {identifier}");
        assert!(identifier.starts_with("https://github.com/") && identifier.contains("/releases/download/"), "{identifier}");
        let hash = package["fileSha256"].as_str().unwrap();
        assert!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{hash}");
    }
}

#[test]
fn the_file_is_the_same_every_time_and_ends_in_a_newline() {
    let version = workspace_version();
    let sums = sums(&version);
    let first = mcp::server_json(&sums, &version, REPOSITORY).unwrap();
    assert_eq!(first, mcp::server_json(&sums, &version, REPOSITORY).unwrap());
    assert!(first.ends_with("}\n") && first.starts_with("{\n"), "{first}");
}

#[test]
fn a_bundle_with_no_line_in_the_sums_is_refused_by_name() {
    let version = workspace_version();
    for bundle in BUNDLES.iter() {
        let name = mcp::bundle_name(&version, bundle);
        let without: String = sums(&version).lines().filter(|line| !line.ends_with(&name)).map(|line| format!("{line}\n")).collect();
        let error = entry(&without).unwrap_err();
        assert!(error.contains(&name) && error.contains("no line"), "{error}");
    }
    let error = entry("").unwrap_err();
    assert!(error.contains("no line"), "{error}");
}

#[test]
fn sums_of_another_release_are_refused_and_say_which() {
    let version = workspace_version();
    let other = "9.9.9";
    assert_ne!(other, version);
    let error = entry(&sums(other)).unwrap_err();
    assert!(error.contains("9.9.9") && error.contains(&version) && error.contains("another release"), "{error}");
}

#[test]
fn a_line_that_is_not_a_sum_a_repeated_name_and_a_bundle_nobody_listed_are_refused() {
    let version = workspace_version();
    let good = sums(&version);
    let first_bundle = mcp::bundle_name(&version, &BUNDLES[0]);
    let hash = hash_of(&good, &first_bundle);
    for (bad, said) in [
        (format!("{good}{}  {first_bundle}\n", digest(99)), "twice"),
        (format!("{good}{}  diacraft-mcp-{version}-freebsd-x86_64.mcpb\n", digest(98)), "not one of the bundles"),
        (good.replace(&hash, &hash.to_uppercase()), "not a SHA-256"),
        (good.replace(&hash, &hash[..63]), "not a SHA-256"),
        (good.replace(&format!("{hash}  "), &format!("{hash} ")), "not a line of sha256sum"),
        (format!("{good}hello\n"), "not a line of sha256sum"),
        (good.replace(&format!("  {first_bundle}"), &format!("  ../{first_bundle}")), "a folder"),
    ] {
        let error = entry(&bad).unwrap_err();
        assert!(error.contains(said), "{said}: {error}");
    }
}

#[test]
fn a_sums_file_of_hostile_size_is_refused_before_it_is_read() {
    let huge = "0".repeat(70_000);
    let error = entry(&huge).unwrap_err();
    assert!(error.contains("too large"), "{error}");
    // Under the size limit and over the limit of lines: 300 lines of some 80 bytes.
    let many: String = (0..300).map(|n| format!("{}  diacraft-{n}.zip\n", digest(n))).collect();
    assert!(many.len() < mcp::MAX_SUMS);
    let error = entry(&many).unwrap_err();
    assert!(error.contains("lines"), "{error}");
    // The command reads no more than the limit of a file either, whatever the file is.
    let dir = TempDir::new("mcp-server-json-huge");
    dir.write("SHA256SUMS.txt", &huge);
    let said = xtask(&["mcp-server-json", dir.path().join("SHA256SUMS.txt").to_str().unwrap()]);
    assert!(!said.status.success() && String::from_utf8_lossy(&said.stderr).contains("too large"), "{}", String::from_utf8_lossy(&said.stderr));
}

#[test]
fn the_repository_must_be_an_address_of_github() {
    let version = workspace_version();
    for bad in
        ["http://github.com/intergrado/diacraft", "https://example.invalid/a/b", "https://github.com/intergrado", "https://github.com/a/b/c", ""]
    {
        let error = mcp::server_json(&sums(&version), &version, bad).unwrap_err();
        assert!(error.contains("repository"), "{bad}: {error}");
    }
    // The address may end in `.git` or a slash, as a manifest writes it; the entry has neither.
    let text = mcp::server_json(&sums(&version), &version, "https://github.com/intergrado/diacraft.git").unwrap();
    let entry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(entry["repository"]["url"], REPOSITORY);
}

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask")).args(args).output().unwrap()
}

#[test]
fn the_command_writes_the_file_it_is_asked_for_or_prints_it() {
    let dir = TempDir::new("mcp-server-json");
    let version = workspace_version();
    dir.write("SHA256SUMS.txt", &sums(&version));
    let sums_path = dir.path().join("SHA256SUMS.txt");
    let out = dir.path().join("server.json");
    let said = xtask(&["mcp-server-json", sums_path.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    assert!(said.status.success(), "{}", String::from_utf8_lossy(&said.stderr));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written["name"], "app.diacraft/diacraft");
    // With no `--out` the entry is on the standard output, and the same.
    let printed = xtask(&["mcp-server-json", sums_path.to_str().unwrap()]);
    assert!(printed.status.success());
    assert_eq!(String::from_utf8(printed.stdout).unwrap(), std::fs::read_to_string(&out).unwrap());
    // What is wrong is said, and nothing is written.
    let wrong = dir.path().join("wrong.json");
    dir.write("other.txt", &sums("9.9.9"));
    let other = dir.path().join("other.txt");
    let said = xtask(&["mcp-server-json", other.to_str().unwrap(), "--out", wrong.to_str().unwrap()]);
    assert!(!said.status.success() && String::from_utf8_lossy(&said.stderr).contains("another release"));
    assert!(!wrong.exists(), "a refused entry is not written");
    for args in [&["mcp-server-json"][..], &["mcp-server-json", "a", "b"], &["mcp-server-json", "a", "--out"], &["mcp-server-json", "--bogus"]] {
        let said = xtask(args);
        assert!(!said.status.success() && String::from_utf8_lossy(&said.stderr).contains("usage: cargo xtask mcp-server-json"), "{args:?}");
    }
    let missing = xtask(&["mcp-server-json", dir.path().join("none.txt").to_str().unwrap()]);
    assert!(!missing.status.success() && String::from_utf8_lossy(&missing.stderr).contains("none.txt"));
}

#[test]
fn the_usage_names_the_command() {
    let usage = String::from_utf8(xtask(&["help"]).stdout).unwrap();
    let text = crate::usage::entry(&usage, "mcp-server-json");
    assert!(text.contains("SHA256SUMS.txt") && text.contains("mcp-publisher") && text.contains("--out"), "{text}");
}
