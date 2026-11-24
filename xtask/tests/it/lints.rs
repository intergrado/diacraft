//! The lint rules of AGENTS.md (Never crash) reach every workspace member: its manifest says `[lints] workspace = true`, and its
//! crate roots start with the deny list and `#![forbid(unsafe_code)]`, before any item.
//!
//! Neither clippy nor `cargo xtask layers` notices a crate that has none of this: with the `[lints]` table and the root
//! attributes taken out of a crate and an `unwrap()` added, both pass, because the workspace table applies only to the packages
//! that opt in. This test is that check, for every member `cargo metadata` lists and for the workspace table they point to.

use std::path::Path;

use serde_json::{Value, json};

/// The lints every crate and binary root denies (AGENTS.md).
const DENIED_IN_ROOTS: [&str; 6] =
    ["clippy::unwrap_used", "clippy::expect_used", "clippy::panic", "clippy::unimplemented", "clippy::todo", "clippy::unreachable"];

/// What `[workspace.lints.clippy]` sets to `deny`, and `[workspace.lints.rust]` to `forbid` (the global constraints).
const WORKSPACE_DENIED: [&str; 9] =
    ["cognitive_complexity", "too_many_arguments", "wildcard_imports", "unwrap_used", "expect_used", "panic", "todo", "unimplemented", "unreachable"];

/// What a root is, for the lints it needs: the deny list is for code that ships, and tests may unwrap and panic.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Root {
    /// A library, a binary, a procedural macro or a build script: `deny(...)` and `forbid(unsafe_code)`.
    Shipped,
    /// An integration test, an example or a bench: `forbid(unsafe_code)`.
    Test,
}

fn root_of(kinds: &[&str]) -> Root {
    if kinds.iter().any(|k| matches!(*k, "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "bin" | "proc-macro" | "custom-build")) {
        Root::Shipped
    } else {
        Root::Test
    }
}

/// Where the whitespace and comments (`//!` documentation among them, and nested block comments) that start at `i` end.
fn skip_blanks(b: &[u8], mut i: usize) -> usize {
    loop {
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let rest = b.get(i..).unwrap_or_default();
        if rest.starts_with(b"//") {
            while b.get(i).is_some_and(|&c| c != b'\n') {
                i += 1;
            }
        } else if rest.starts_with(b"/*") {
            let mut depth = 0usize;
            while i < b.len() {
                let rest = b.get(i..).unwrap_or_default();
                if rest.starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if rest.starts_with(b"*/") {
                    depth = depth.saturating_sub(1);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else {
            return i;
        }
    }
}

/// Where the inner attribute `#![…]` that starts at `i` ends: after its closing bracket. A string in it is skipped over, so
/// a `]` in the string does not close it. An attribute that never closes ends at the end of the source.
fn attribute_end(b: &[u8], mut i: usize) -> usize {
    let mut depth = 1usize;
    i += 3;
    while i < b.len() && depth > 0 {
        match b.get(i) {
            Some(b'[') => depth += 1,
            Some(b']') => depth -= 1,
            Some(b'"') => {
                i += 1;
                while b.get(i).is_some_and(|&c| c != b'"') {
                    i += if b.get(i) == Some(&b'\\') { 2 } else { 1 };
                }
            }
            _ => {}
        }
        i += 1;
    }
    i.min(b.len())
}

/// The inner attributes a Rust source starts with, each as its text inside `#![` and `]` with the whitespace taken out. They
/// end at the first thing that is none: an item (with or without an attribute of its own), or the end of the source. Comments
/// and blank lines may precede and sit between them. A scanner and not a parser: an attribute in a comment, a string or
/// after an item is not found.
fn leading_attributes(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut found = Vec::new();
    let mut i = skip_blanks(b, 0);
    while b.get(i..).unwrap_or_default().starts_with(b"#![") {
        let end = attribute_end(b, i);
        let inner = src.get(i + 3..end.saturating_sub(1).max(i + 3)).unwrap_or_default();
        found.push(inner.chars().filter(|c| !c.is_whitespace()).collect());
        i = skip_blanks(b, end);
    }
    found
}

/// The lints a `deny(…)` or `forbid(…)` among `attributes` names.
fn denied_lints(attributes: &[String]) -> Vec<&str> {
    attributes
        .iter()
        .filter_map(|a| a.strip_prefix("deny(").or_else(|| a.strip_prefix("forbid(")))
        .filter_map(|a| a.strip_suffix(')'))
        .flat_map(|a| a.split(',').filter(|l| !l.is_empty()))
        .collect()
}

/// What is wrong with the crate root `path`, whose text is `src`.
fn root_problems(path: &str, src: &str, root: Root) -> Vec<String> {
    let attributes = leading_attributes(src);
    let mut problems = Vec::new();
    if !attributes.iter().any(|a| a == "forbid(unsafe_code)") {
        problems.push(format!("{path}: no `#![forbid(unsafe_code)]` before the first item"));
    }
    if root == Root::Shipped {
        let denied = denied_lints(&attributes);
        let missing: Vec<&str> = DENIED_IN_ROOTS.iter().copied().filter(|lint| !denied.contains(lint)).collect();
        if !missing.is_empty() {
            problems.push(format!("{path}: no `#![deny(...)]` before the first item that names {}", missing.join(", ")));
        }
        // An `allow` of one of them, ahead of the first item, turns the deny off again.
        for attribute in &attributes {
            if let Some(list) = attribute.strip_prefix("allow(").and_then(|a| a.strip_suffix(')')) {
                let lints: Vec<&str> = list.split(',').filter(|l| DENIED_IN_ROOTS.contains(l)).collect();
                if !lints.is_empty() {
                    problems.push(format!("{path}: `#![allow({})]` before the first item undoes the deny list", lints.join(", ")));
                }
            }
        }
    }
    problems
}

/// What is wrong with a package's manifest: it must say `[lints] workspace = true`, and nothing of its own.
fn manifest_problems(path: &str, manifest: &str) -> Vec<String> {
    let table: toml::Table = match manifest.parse() {
        Ok(table) => table,
        Err(e) => return vec![format!("{path}: not TOML: {e}")],
    };
    match table.get("lints") {
        Some(toml::Value::Table(lints)) if lints.get("workspace") == Some(&toml::Value::Boolean(true)) && lints.len() == 1 => vec![],
        Some(toml::Value::Table(lints)) if lints.get("workspace") == Some(&toml::Value::Boolean(true)) => {
            vec![format!("{path}: `[lints]` has more than `workspace = true`, so the package sets lints of its own")]
        }
        _ => vec![format!("{path}: no `[lints] workspace = true`, so the workspace's lints do not reach this package")],
    }
}

/// What is wrong with the workspace table the members point to: the lints it must set.
fn workspace_problems(manifest: &str) -> Vec<String> {
    let table: toml::Table = match manifest.parse() {
        Ok(table) => table,
        Err(e) => return vec![format!("Cargo.toml: not TOML: {e}")],
    };
    let lints = table.get("workspace").and_then(|w| w.get("lints"));
    let level = |tool: &str, name: &str| lints.and_then(|l| l.get(tool)).and_then(|t| t.get(name)).and_then(|v| v.as_str()).map(str::to_owned);
    let mut problems = Vec::new();
    if level("rust", "unsafe_code").as_deref() != Some("forbid") {
        problems.push("Cargo.toml: `[workspace.lints.rust] unsafe_code` is not \"forbid\"".to_owned());
    }
    for name in WORKSPACE_DENIED {
        if level("clippy", name).as_deref() != Some("deny") {
            problems.push(format!("Cargo.toml: `[workspace.lints.clippy] {name}` is not \"deny\""));
        }
    }
    problems
}

/// Every problem of the workspace `metadata` (the output of `cargo metadata --no-deps`) describes, files read from where it says
/// they are. A file that cannot be read is a problem: a check that cannot see a file has not checked it.
fn problems(metadata: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let members: Vec<&str> = metadata["workspace_members"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let packages = metadata["packages"].as_array().map(Vec::as_slice).unwrap_or_default();
    let workspace_root = metadata["workspace_root"].as_str().unwrap_or_default();
    if members.is_empty() || packages.is_empty() || workspace_root.is_empty() {
        return vec!["the metadata lists no workspace members: the check sees nothing".to_owned()];
    }
    let relative = |path: &str| path.strip_prefix(workspace_root).map(|p| p.trim_start_matches('/')).unwrap_or(path).to_owned();
    match std::fs::read_to_string(Path::new(workspace_root).join("Cargo.toml")) {
        Ok(text) => problems.extend(workspace_problems(&text)),
        Err(e) => problems.push(format!("Cargo.toml: cannot read: {e}")),
    }
    for package in packages.iter().filter(|p| p["id"].as_str().is_some_and(|id| members.contains(&id))) {
        let manifest = package["manifest_path"].as_str().unwrap_or_default();
        match std::fs::read_to_string(manifest) {
            Ok(text) => problems.extend(manifest_problems(&relative(manifest), &text)),
            Err(e) => problems.push(format!("{}: cannot read: {e}", relative(manifest))),
        }
        for target in package["targets"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let src = target["src_path"].as_str().unwrap_or_default();
            let kinds: Vec<&str> = target["kind"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            match std::fs::read_to_string(src) {
                Ok(text) => problems.extend(root_problems(&relative(src), &text, root_of(&kinds))),
                Err(e) => problems.push(format!("{}: cannot read: {e}", relative(src))),
            }
        }
    }
    problems
}

/// Every workspace member, and the workspace table, as this repository has them.
#[test]
fn every_workspace_member_has_the_lints_and_its_roots_start_with_them() {
    let metadata = xtask::metadata().unwrap();
    let found = problems(&metadata);
    assert!(found.is_empty(), "the lint rules of AGENTS.md do not reach every crate:\n  {}", found.join("\n  "));
    // The check sees the members: this repository's own crates and a root of each kind.
    let names: Vec<&str> = metadata["packages"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    for expected in ["diacraft-testkit", "diacraft-engine", "diacraft-mcp", "diacraft-ui-egui", "diacraft", "diacraft-cli", "diacraft-web", "xtask"] {
        assert!(names.contains(&expected), "{expected} is not among the packages checked: {names:?}");
    }
    let kinds: Vec<&str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["targets"].as_array().unwrap())
        .flat_map(|t| t["kind"].as_array().unwrap())
        .filter_map(Value::as_str)
        .collect();
    for kind in ["lib", "bin", "test"] {
        assert!(kinds.contains(&kind), "no `{kind}` target among those checked: {kinds:?}");
    }
}

const GOOD_ROOT: &str = "//! A crate.\n#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]\n\
                         #![forbid(unsafe_code)]\n\nmod a;\n";

#[test]
fn a_root_with_the_deny_list_and_forbid_before_its_items_passes() {
    assert_eq!(root_problems("src/lib.rs", GOOD_ROOT, Root::Shipped), Vec::<String>::new());
    // Documentation and comments before and between, an attribute split over lines, another inner attribute after.
    let spread = "//! Docs\n//!\n// a comment\n/* block /* nested */ */\n#![deny(\n    clippy::unwrap_used,\n    clippy::expect_used,\n    clippy::panic,\n    \
                  clippy::unimplemented,\n    clippy::todo,\n    clippy::unreachable\n)]\n\n#![forbid(unsafe_code)]\n#![cfg_attr(windows, windows_subsystem = \"windows\")]\nfn main() {}\n";
    assert_eq!(root_problems("src/main.rs", spread, Root::Shipped), Vec::<String>::new());
    // A test root may allow what shipped code denies, and still forbids unsafe code.
    let test = "#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]\n#![forbid(unsafe_code)]\nmod a;\n";
    assert_eq!(root_problems("tests/it/main.rs", test, Root::Test), Vec::<String>::new());
}

#[test]
fn a_root_missing_either_attribute_is_reported() {
    let none = "mod a;\n";
    let found = root_problems("src/lib.rs", none, Root::Shipped);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("forbid(unsafe_code)") && found[1].contains("deny") && found[1].contains("clippy::unwrap_used"), "{found:?}");

    let no_forbid = GOOD_ROOT.replace("#![forbid(unsafe_code)]\n", "");
    assert_eq!(root_problems("src/lib.rs", &no_forbid, Root::Shipped).len(), 1);

    // One lint of the six missing is named.
    let short = GOOD_ROOT.replace(", clippy::unreachable", "");
    let found = root_problems("src/lib.rs", &short, Root::Shipped);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].ends_with("names clippy::unreachable"), "{found:?}");

    // Only a deny or a forbid counts: a warn does not, and the test root's forbid is needed too.
    let warn = GOOD_ROOT.replace("#![deny(", "#![warn(");
    assert_eq!(root_problems("src/lib.rs", &warn, Root::Shipped).len(), 1);
    assert_eq!(root_problems("tests/it/main.rs", "#![allow(clippy::unwrap_used)]\nmod a;\n", Root::Test).len(), 1);
}

/// Inner attributes must come before any item to be inner attributes at all, but a scanner that read past one would take a
/// look-alike for the real thing: an attribute in a comment, a string, or after the first item is not a lint.
#[test]
fn attributes_after_the_first_item_or_in_comments_do_not_count() {
    let after = "mod a;\n#![forbid(unsafe_code)]\n";
    assert_eq!(root_problems("src/lib.rs", after, Root::Test).len(), 1);
    let commented = "// #![forbid(unsafe_code)]\n/* #![forbid(unsafe_code)] */\nconst S: &str = \"#![forbid(unsafe_code)]\";\n";
    assert_eq!(root_problems("src/lib.rs", commented, Root::Test).len(), 1);
    // An item with an attribute of its own is the first item.
    let item_first = "#[derive(Debug)]\nstruct A;\n#![forbid(unsafe_code)]\n";
    assert_eq!(root_problems("src/lib.rs", item_first, Root::Test).len(), 1);
    // A `]` or a `"` inside an attribute's string does not end it early.
    let tricky = "#![cfg_attr(a, doc = \"]\")]\n#![forbid(unsafe_code)]\n";
    assert_eq!(root_problems("src/lib.rs", tricky, Root::Test), Vec::<String>::new());
}

/// An `allow` of a denied lint in the same place undoes the deny, whatever order they come in.
#[test]
fn an_allow_that_undoes_the_deny_list_is_reported() {
    let allowed = GOOD_ROOT.replace("#![forbid", "#![allow(clippy::unwrap_used, clippy::panic)]\n#![forbid");
    let found = root_problems("src/lib.rs", &allowed, Root::Shipped);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("allow(clippy::unwrap_used, clippy::panic)"), "{found:?}");
    // Allowing another lint is none of this check's business.
    let other = GOOD_ROOT.replace("#![forbid", "#![allow(clippy::too_many_lines)]\n#![forbid");
    assert_eq!(root_problems("src/lib.rs", &other, Root::Shipped), Vec::<String>::new());
}

#[test]
fn a_manifest_must_say_lints_workspace_true_and_nothing_else_of_lints() {
    let ok = "[package]\nname = \"a\"\n\n[lints]\nworkspace = true\n";
    assert!(manifest_problems("a/Cargo.toml", ok).is_empty());
    for (manifest, says) in [
        ("[package]\nname = \"a\"\n", "no `[lints] workspace = true`"),
        ("[package]\nname = \"a\"\n\n[lints]\nworkspace = false\n", "no `[lints] workspace = true`"),
        ("[package]\nname = \"a\"\n\n[lints.clippy]\nunwrap_used = \"allow\"\n", "no `[lints] workspace = true`"),
        ("[package]\nname = \"a\"\n\n[lints]\nworkspace = true\n\n[lints.rust]\nunsafe_code = \"allow\"\n", "sets lints of its own"),
        ("not = [toml", "not TOML"),
    ] {
        let found = manifest_problems("a/Cargo.toml", manifest);
        assert_eq!(found.len(), 1, "{manifest}: {found:?}");
        assert!(found[0].starts_with("a/Cargo.toml:") && found[0].contains(says), "{manifest}: {found:?}");
    }
}

#[test]
fn the_workspace_table_must_set_the_lints_the_members_inherit() {
    let ok = "[workspace.lints.rust]\nunsafe_code = \"forbid\"\n\n[workspace.lints.clippy]\ncognitive_complexity = \"deny\"\ntoo_many_arguments = \"deny\"\n\
              wildcard_imports = \"deny\"\nunwrap_used = \"deny\"\nexpect_used = \"deny\"\npanic = \"deny\"\ntodo = \"deny\"\nunimplemented = \"deny\"\n\
              unreachable = \"deny\"\ndbg_macro = \"warn\"\n";
    assert!(workspace_problems(ok).is_empty(), "{:?}", workspace_problems(ok));
    let found = workspace_problems(&ok.replace("unwrap_used = \"deny\"", "unwrap_used = \"warn\"").replace("\"forbid\"", "\"deny\""));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|f| f.contains("unsafe_code")) && found.iter().any(|f| f.contains("unwrap_used")), "{found:?}");
    assert_eq!(workspace_problems("[workspace]\nmembers = []\n").len(), 10);
}

/// The glue: members taken from a metadata document, each manifest and each root read from where the document says, a file that
/// is not there a problem, a workspace table checked, and a package that is not a member ignored.
#[test]
fn the_check_reads_every_member_manifest_and_root_the_metadata_lists() {
    let dir = crate::TempDir::new("lints");
    let root = dir.path().to_str().unwrap();
    let workspace = "[workspace]\nmembers = [\"good\", \"bad\"]\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n\n[workspace.lints.clippy]\ncognitive_complexity = \"deny\"\n\
                     too_many_arguments = \"deny\"\nwildcard_imports = \"deny\"\nunwrap_used = \"deny\"\nexpect_used = \"deny\"\npanic = \"deny\"\ntodo = \"deny\"\n\
                     unimplemented = \"deny\"\nunreachable = \"deny\"\n";
    dir.write("Cargo.toml", workspace);
    dir.write("good/Cargo.toml", "[package]\nname = \"good\"\n\n[lints]\nworkspace = true\n");
    dir.write("good/src/lib.rs", GOOD_ROOT);
    dir.write("good/tests/it/main.rs", "#![allow(clippy::unwrap_used)]\n#![forbid(unsafe_code)]\n");
    // The testkit's case: nothing of the lint rules, and an unwrap.
    dir.write("bad/Cargo.toml", "[package]\nname = \"bad\"\n");
    dir.write("bad/src/lib.rs", "pub fn f(x: Option<u8>) -> u8 {\n    x.unwrap()\n}\n");
    let target = |name: &str, kind: &str, src: &str| json!({"name": name, "kind": [kind], "src_path": format!("{root}/{src}")});
    let package = |name: &str, targets: Vec<Value>| json!({"name": name, "id": format!("path+file://{root}/{name}#{name}@0.1.0"), "manifest_path": format!("{root}/{name}/Cargo.toml"), "targets": targets});
    let metadata = json!({
        "workspace_root": root,
        "workspace_members": [format!("path+file://{root}/good#good@0.1.0"), format!("path+file://{root}/bad#bad@0.1.0")],
        "packages": [
            package("good", vec![target("good", "lib", "good/src/lib.rs"), target("it", "test", "good/tests/it/main.rs")]),
            package("bad", vec![target("bad", "lib", "bad/src/lib.rs"), target("gone", "bin", "bad/src/gone.rs")]),
            // Not a member: a dependency's manifest is not ours to judge.
            json!({"name": "outside", "id": "registry+x#outside@1.0.0", "manifest_path": format!("{root}/outside/Cargo.toml"), "targets": []}),
        ],
    });
    let found = problems(&metadata);
    // The workspace table lacks `dbg_macro` only, which is not required; the good package has nothing wrong.
    assert!(found.iter().all(|f| f.starts_with("bad/")), "{found:#?}");
    assert!(found.iter().any(|f| f.contains("bad/Cargo.toml") && f.contains("no `[lints] workspace = true`")), "{found:#?}");
    assert!(found.iter().any(|f| f.starts_with("bad/src/lib.rs") && f.contains("forbid(unsafe_code)")), "{found:#?}");
    assert!(found.iter().any(|f| f.starts_with("bad/src/lib.rs") && f.contains("deny")), "{found:#?}");
    assert!(found.iter().any(|f| f.starts_with("bad/src/gone.rs") && f.contains("cannot read")), "{found:#?}");
    assert_eq!(found.len(), 4, "{found:#?}");

    // A metadata document with no members is not a pass.
    assert_eq!(problems(&json!({})).len(), 1);
    // A workspace table that is missing is a problem too.
    std::fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
    assert!(problems(&metadata).iter().any(|f| f.starts_with("Cargo.toml: cannot read")));
}
