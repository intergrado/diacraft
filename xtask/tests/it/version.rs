//! `cargo xtask version`: the workspace version, edited in one place.

use xtask::version::{read, replace, validate};

const MANIFEST: &str = "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace.dependencies]\nfoo = { version = \"1\" }\n";

#[test]
fn reads_the_workspace_version() {
    assert_eq!(read(MANIFEST).unwrap(), "0.1.0");
    assert!(read("[package]\nversion = \"1.0.0\"\n").is_err());
}

#[test]
fn replaces_only_the_workspace_version() {
    let out = replace(MANIFEST, "1.2.3-rc.1").unwrap();
    assert_eq!(read(&out).unwrap(), "1.2.3-rc.1");
    assert_eq!(out, MANIFEST.replace("version = \"0.1.0\"", "version = \"1.2.3-rc.1\""));
    assert!(out.contains("foo = { version = \"1\" }"));
}

#[test]
fn keeps_crlf_line_endings() {
    let crlf = MANIFEST.replace('\n', "\r\n");
    let out = replace(&crlf, "2.0.0").unwrap();
    assert_eq!(out, crlf.replace("\"0.1.0\"", "\"2.0.0\""));
}

#[test]
fn validates_versions() {
    for ok in ["0.1.0", "1.20.300", "1.0.0-rc.1", "1.0.0-alpha", "1.0.0-x-y.2"] {
        assert!(validate(ok).is_ok(), "{ok}");
    }
    for bad in ["1.0", "1.0.0.0", "v1.0.0", "01.0.0", "1.0.0-", "1.0.0-a..b", "1.0.0+meta", "1.a.0", ""] {
        assert!(validate(bad).is_err(), "{bad}");
    }
    assert!(replace(MANIFEST, "nope").is_err());
}

#[test]
fn the_real_manifest_has_a_valid_version() {
    let text = std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap();
    validate(&read(&text).unwrap()).unwrap();
}

// Everything that says DiaCraft's version takes it from `[workspace.package]`, and the few places that hold it as text are held equal to it here.

/// The version of the real workspace.
fn workspace_version() -> String {
    read(&std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap()).unwrap()
}

/// The manifests of the workspace's members: `crates/*`, `apps/*` and xtask. (The fuzz crates under a crate are workspaces of their own, and are not shipped.)
fn member_manifests() -> Vec<std::path::PathBuf> {
    let root = xtask::root();
    let mut found = vec![root.join("xtask/Cargo.toml")];
    for folder in ["crates", "apps"] {
        for entry in std::fs::read_dir(root.join(folder)).unwrap() {
            let manifest = entry.unwrap().path().join("Cargo.toml");
            if manifest.is_file() {
                found.push(manifest);
            }
        }
    }
    found.sort();
    found
}

/// The members whose `[package]` has a version of its own instead of `version.workspace = true`.
fn with_own_version(manifests: &[(String, String)]) -> Vec<String> {
    let mut own = Vec::new();
    for (name, text) in manifests {
        let table: toml::Table = text.parse().unwrap();
        let inherits =
            table.get("package").and_then(|p| p.get("version")).and_then(|v| v.get("workspace")).and_then(toml::Value::as_bool) == Some(true);
        if !inherits {
            own.push(name.clone());
        }
    }
    own
}

#[test]
fn every_member_crate_takes_the_workspace_version() {
    let manifests: Vec<(String, String)> =
        member_manifests().into_iter().map(|path| (path.display().to_string(), std::fs::read_to_string(&path).unwrap())).collect();
    assert!(manifests.len() > 20, "{} manifests found: the walk is wrong", manifests.len());
    assert_eq!(
        with_own_version(&manifests),
        Vec::<String>::new(),
        "a member with a version of its own would say another version in `--version`, the splash and the files it writes"
    );
    // The check sees a literal.
    let literal = vec![
        ("a".to_owned(), "[package]\nname = \"a\"\nversion = \"0.1.0\"\n".to_owned()),
        ("b".to_owned(), "[package]\nname = \"b\"\nversion.workspace = true\n".to_owned()),
    ];
    assert_eq!(with_own_version(&literal), ["a"]);
}

/// The `(name, version)` of each package of a `Cargo.lock` that has no `source`: the workspace's own members.
fn lock_members(lock: &str) -> Vec<(String, String)> {
    let table: toml::Table = lock.parse().unwrap();
    let packages = table.get("package").and_then(toml::Value::as_array).cloned().unwrap_or_default();
    packages
        .iter()
        .filter(|p| p.get("source").is_none())
        .map(|p| (p["name"].as_str().unwrap().to_owned(), p["version"].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn cargo_lock_names_the_workspace_version_for_every_member() {
    // The release builds with `--locked`: a lock that still says the old version stops it (`cargo xtask version set` refreshes it).
    let version = workspace_version();
    let lock = std::fs::read_to_string(xtask::root().join("Cargo.lock")).unwrap();
    let members = lock_members(&lock);
    assert_eq!(members.len(), member_manifests().len(), "the members of Cargo.lock ({members:?}) are not the crates of the workspace");
    let stale: Vec<_> = members.iter().filter(|(_, v)| *v != version).collect();
    assert!(stale.is_empty(), "Cargo.lock says another version than {version} for {stale:?}: run `cargo xtask version set {version}`");
    // The check sees a stale entry, and leaves the crates of other sources alone.
    let old = "[[package]]\nname = \"diacraft\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    assert_eq!(lock_members(old), [("diacraft".to_owned(), "0.1.0".to_owned())]);
}

/// The values of `"app":"diacraft","version":"…"` in `text`: the examples of the control protocol's answers.
fn example_versions(text: &str) -> Vec<String> {
    let key = "\"app\":\"diacraft\",\"version\":\"";
    text.match_indices(key).map(|(at, _)| text[at + key.len()..].split('"').next().unwrap_or("").to_owned()).collect()
}

#[test]
fn the_docs_examples_and_the_changelog_say_the_workspace_version() {
    let version = workspace_version();
    let protocol = std::fs::read_to_string(xtask::root().join("docs/control-protocol.md")).unwrap();
    let said = example_versions(&protocol);
    assert!(said.len() >= 2, "{said:?}: the examples of ui.inspect are not there");
    assert!(said.iter().all(|v| *v == version), "docs/control-protocol.md shows the versions {said:?}, the workspace says {version}");
    assert_eq!(example_versions("{\"app\":\"diacraft\",\"version\":\"0.1.0\"}"), ["0.1.0"], "the check sees a version");
    // The release notes are for the version being made: the changelog's title block names it (a release is cut with `cargo xtask version set`, and then the notes are
    // settled, `docs/releasing.md`).
    let changelog = std::fs::read_to_string(xtask::root().join("site/changelog.html")).unwrap();
    let block = format!("<dt>Release</dt><dd><b>{version}</b></dd>");
    assert!(changelog.contains(&block), "site/changelog.html does not hold `{block}`: write the notes of {version}");
    assert!(changelog.contains("<dt>Date</dt><dd>"), "the changelog's title block has no date (`Unreleased`, or the day of the release)");
}

/// The owner's ruling (2026-11-26): the Features page (`site/features.html`) says what works in the release, and it is read again for every release. Its title block names the release, as the
/// changelog's does, and every "DiaCraft <version>" it says is the workspace's: after `cargo xtask version set` this fails until the page is read again and its version moved (`docs/releasing.md`).
#[test]
fn the_features_page_says_the_workspace_version() {
    let version = workspace_version();
    let page = std::fs::read_to_string(xtask::root().join("site/features.html")).unwrap();
    let block = format!("<dt>Release</dt><dd><b>{version}</b></dd>");
    assert_eq!(page.matches("<dt>Release</dt>").count(), 1, "site/features.html has one release in its title block");
    assert!(page.contains(&block), "site/features.html does not hold `{block}`: read the page again for {version} and move its version");
    let said = said_versions(&page);
    assert!(!said.is_empty(), "the features page names its release in its words (\"DiaCraft {version}\")");
    assert!(said.iter().all(|v| *v == version), "site/features.html says DiaCraft {said:?}, the workspace says {version}");
    assert_eq!(said_versions("DiaCraft 0.1.0 and DiaCraft 0.2 and DiaCraft. And DiaCraft 1.2.3."), ["0.1.0", "1.2.3"], "the check sees a version");
}

/// The versions `x.y.z` that follow "DiaCraft " in `text`.
fn said_versions(text: &str) -> Vec<String> {
    text.match_indices("DiaCraft ")
        .filter_map(|(at, key)| {
            let rest = &text[at + key.len()..];
            let word: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let word = word.trim_end_matches('.');
            (word.split('.').count() == 3 && word.split('.').all(|part| !part.is_empty())).then(|| word.to_owned())
        })
        .collect()
}

/// A download that older releases never had is linked only by the pages made for the release it first came with or a later one: the `.rpm` first came with 0.2.4, and a page made for 0.2.3
/// must not link a file of that release that does not exist. A pre-release of 0.2.4 builds the same files as the release, so it has them; every other download is in every release.
#[test]
fn a_download_that_came_with_a_later_release_is_not_in_the_releases_before_it() {
    use xtask::version::{DOWNLOAD_SINCE, release_has};
    assert_eq!(DOWNLOAD_SINCE, [("linux-x86_64-rpm", "0.2.4"), ("linux-arm64-rpm", "0.2.4")]);
    for id in ["linux-x86_64-rpm", "linux-arm64-rpm"] {
        for (version, has) in [
            ("0.2.3", false),
            ("0.2.2", false),
            ("0.1.9", false),
            ("0.2.4-rc.1", true),
            ("0.2.4", true),
            ("0.2.10", true),
            ("0.3.0", true),
            ("1.0.0-rc.1", true),
            ("0.2", false),
            ("x.y.z", false),
            ("", false),
        ] {
            assert_eq!(release_has(version, id), has, "{id} in {version}");
        }
    }
    for id in ["linux-x86_64-deb", "linux-arm64-appimage", "windows-x86_64-msi", "macos-arm64-dmg", "not-a-download"] {
        for version in ["0.2.0", "0.2.3", "0.2.4", "x"] {
            assert!(release_has(version, id), "{id} in {version}: only the downloads of DOWNLOAD_SINCE are held back");
        }
    }
}
