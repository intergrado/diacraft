//! The workspace manifests: what must hold for every cargo command to be quiet and truthful.

/// A `[patch]` entry that no crate uses is recorded in Cargo.lock as `[[patch.unused]]`, and cargo
/// then prints one warning per entry on every command that resolves. The egui and winit pins are
/// added with the first crate that depends on them, not before.
#[test]
fn the_lockfile_has_no_unused_patch() {
    let lock = std::fs::read_to_string(xtask::root().join("Cargo.lock")).unwrap();
    let unused = lock.lines().filter(|l| l.trim() == "[[patch.unused]]").count();
    assert_eq!(unused, 0, "{unused} [patch] entries are used by no crate: remove them or add the dependency that uses them");
}

/// The desktop app's Copy and Cut, and every text field, must reach the system clipboard: without eframe's `clipboard` feature
/// egui keeps a private clipboard, so a copy or a paste never leaves DiaCraft. This holds the app to turning it on (public issue:
/// copy and paste with other programs).
#[test]
fn the_desktop_app_connects_egui_to_the_system_clipboard() {
    let text = std::fs::read_to_string(xtask::root().join("apps/diacraft/Cargo.toml")).unwrap();
    let manifest: toml::Table = text.parse().expect("apps/diacraft/Cargo.toml is TOML");
    let features = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("eframe"))
        .and_then(|eframe| eframe.get("features"))
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("apps/diacraft/Cargo.toml names eframe with no `features` list"));
    assert!(
        features.iter().any(|feature| feature.as_str() == Some("clipboard")),
        "the desktop app's eframe has no `clipboard` feature ({features:?}): egui then keeps a private clipboard, so Copy, Cut and every text \
         field never reach the system clipboard on Windows, macOS or Linux, and text copied in another program cannot be pasted in (public \
         issue: copy and paste with other programs)"
    );
}

/// The `[[package]]` entries of a Cargo.lock named `package`, each with the first word of every dependency it lists
/// (cargo writes `"name"`, and `"name 1.2.3"` or `"name 1.2.3 (source)"` where two versions of a crate exist).
fn lock_entries(lock: &str, package: &str) -> Vec<(String, Vec<String>)> {
    let lock: toml::Table = lock.parse().expect("Cargo.lock is TOML");
    let entries = lock.get("package").and_then(|p| p.as_array()).expect("Cargo.lock has [[package]] entries");
    let mut found = Vec::new();
    for entry in entries {
        if entry.get("name").and_then(|n| n.as_str()) != Some(package) {
            continue;
        }
        let version = entry.get("version").and_then(|v| v.as_str()).unwrap_or("?").to_owned();
        let dependencies = entry.get("dependencies").and_then(|d| d.as_array()).map(Vec::as_slice).unwrap_or_default();
        let names = dependencies.iter().filter_map(|d| d.as_str()).filter_map(|d| d.split_whitespace().next()).map(str::to_owned).collect();
        found.push((version, names));
    }
    found
}

/// The versions of `package` whose lock entry lists `dependency`.
fn entries_listing(lock: &str, package: &str, dependency: &str) -> Vec<String> {
    lock_entries(lock, package).into_iter().filter(|(_, names)| names.iter().any(|n| n == dependency)).map(|(version, _)| version).collect()
}

/// What the font guard says of a Cargo.lock: `Ok` unless `epaint` lists `epaint_default_fonts`, or the guard cannot tell.
fn font_guard(lock: &str) -> Result<(), String> {
    let epaints = lock_entries(lock, "epaint");
    if !epaints.iter().any(|(_, deps)| deps.iter().any(|d| d == "emath")) {
        return Err("no `epaint` entry with its dependencies in Cargo.lock: the check sees nothing".to_owned());
    }
    // NOTICE names this crate, and the app takes its two typefaces from it by name, so it has an entry in the lock. Without
    // one the check below is true of any name: say so, as a crate renamed or dropped is a notice to rewrite.
    if lock_entries(lock, "epaint_default_fonts").is_empty() {
        return Err(
            "the crate `epaint_default_fonts` is not in Cargo.lock, so the check that epaint does not use it sees nothing: it was renamed or \
             the app no longer takes its fonts from it. NOTICE names it and lists the fonts compiled in; see crates/ui-egui/src/fonts.rs"
                .to_owned(),
        );
    }
    let listing = entries_listing(lock, "epaint", "epaint_default_fonts");
    if !listing.is_empty() {
        return Err(format!(
            "Cargo.lock lists `epaint_default_fonts` as a dependency of epaint {listing:?}: some package turned egui's `default_fonts` feature on, \
             which compiles the icon font with platform logos into the app. Turn it off, or see NOTICE before changing this"
        ));
    }
    Ok(())
}

/// egui's `default_fonts` feature makes `epaint` depend on `epaint_default_fonts` and register three of its typefaces
/// (Hack, Ubuntu Light and `egui-icons`, which has the Windows, Apple, Android and GitHub logos), and compiles the bytes
/// of all three in whatever the app installs (its `monochrome_emoji_fonts` feature adds two more; NOTICE). Cargo.lock
/// holds the dependencies of every member on every target, so one look at the `epaint` entry covers what no single
/// build's feature graph does: a `cfg(windows)` dependency, a member other than the window's, a build of one package
/// alone. (The app takes its two typefaces from `epaint_default_fonts` by name, which adds the crate to the lock under
/// `diacraft-ui-egui` but not under `epaint`.)
#[test]
fn the_lockfile_does_not_link_epaint_to_the_icon_font() {
    let lock = std::fs::read_to_string(xtask::root().join("Cargo.lock")).unwrap();
    if let Err(why) = font_guard(&lock) {
        panic!("{why}");
    }
}

/// The check above reads what cargo writes, so it is tried on what cargo writes: the edge as a plain name, with a
/// version and with a source, in the second of two `epaint` entries, and not confused by other packages.
#[test]
fn the_icon_font_check_finds_the_edge_in_every_spelling() {
    let lock = |epaint_deps: &str, other: &str| {
        format!(
            "version = 4\n\n[[package]]\nname = \"emath\"\nversion = \"0.37.0\"\n\n[[package]]\nname = \"epaint\"\nversion = \"0.37.0\"\n\
             dependencies = [\n \"emath\",\n{epaint_deps}]\n\n{other}"
        )
    };
    let edge = |dep: &str| entries_listing(&lock(&format!(" \"{dep}\",\n"), ""), "epaint", "epaint_default_fonts");

    assert_eq!(edge("epaint_default_fonts"), ["0.37.0"]);
    assert_eq!(edge("epaint_default_fonts 0.37.0"), ["0.37.0"]);
    assert_eq!(edge("epaint_default_fonts 0.37.0 (git+https://example.org/egui.git?rev=abc#abc)"), ["0.37.0"]);
    // Only the dependency's own name counts.
    assert!(edge("epaint_default_fonts_extra").is_empty());
    assert!(edge("not_epaint_default_fonts").is_empty());
    // No edge under `epaint`: another package may list the crate (the app does, to take two of its typefaces).
    let other = "[[package]]\nname = \"diacraft-ui-egui\"\nversion = \"0.1.0\"\ndependencies = [\n \"epaint_default_fonts\",\n]\n";
    assert!(entries_listing(&lock("", other), "epaint", "epaint_default_fonts").is_empty());
    assert_eq!(entries_listing(&lock("", other), "diacraft-ui-egui", "epaint_default_fonts"), ["0.1.0"]);
    // Two versions of `epaint`: the one that has the edge is the one reported.
    let second = "[[package]]\nname = \"epaint\"\nversion = \"0.38.0\"\ndependencies = [\n \"epaint_default_fonts 0.38.0\",\n]\n";
    assert_eq!(entries_listing(&lock("", second), "epaint", "epaint_default_fonts"), ["0.38.0"]);
}

/// NOTICE says what the app compiles in by naming `epaint_default_fonts`, and the app takes its two typefaces from that crate
/// by name, so the crate has an entry in the lock. A lock without one means the crate was renamed or the app no longer uses it:
/// then "epaint does not list it" is true of any name, and the guard would pass on nothing.
#[test]
fn the_font_guard_fails_when_the_font_crate_is_not_in_the_lock_at_all() {
    let epaint = "[[package]]\nname = \"emath\"\nversion = \"0.37.0\"\n\n[[package]]\nname = \"epaint\"\nversion = \"0.37.0\"\ndependencies = [\n \"emath\",\n";
    let app = "[[package]]\nname = \"diacraft-ui-egui\"\nversion = \"0.1.0\"\ndependencies = [\n \"epaint_default_fonts\",\n]\n";
    let font_crate = "[[package]]\nname = \"epaint_default_fonts\"\nversion = \"0.37.0\"\n";
    // The crate is there and epaint does not list it: fine.
    assert_eq!(font_guard(&format!("version = 4\n\n{epaint}]\n\n{app}\n{font_crate}")), Ok(()));
    // Epaint lists it: refused, with the reason.
    let listed = font_guard(&format!("version = 4\n\n{epaint} \"epaint_default_fonts\",\n]\n\n{app}\n{font_crate}")).unwrap_err();
    assert!(listed.contains("default_fonts") && listed.contains("NOTICE"), "{listed}");
    // The crate is nowhere in the lock (renamed, say): the guard cannot tell, and says so.
    let renamed = "[[package]]\nname = \"epaint_bundled_fonts\"\nversion = \"0.37.0\"\n";
    let blind = font_guard(&format!("version = 4\n\n{epaint}]\n\n{renamed}")).unwrap_err();
    assert!(blind.contains("epaint_default_fonts") && blind.contains("NOTICE") && blind.contains("not in Cargo.lock"), "{blind}");
}

/// The names of the packages `key` (`workspace_members` or `workspace_default_members`) lists in the output of `cargo metadata`.
fn package_names(metadata: &serde_json::Value, key: &str) -> Vec<String> {
    let ids: Vec<&str> = metadata[key].as_array().unwrap_or_else(|| panic!("metadata has no `{key}`")).iter().filter_map(|id| id.as_str()).collect();
    let mut names: Vec<String> = metadata["packages"]
        .as_array()
        .expect("metadata has packages")
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| ids.contains(&id)))
        .filter_map(|p| p["name"].as_str())
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// A plain `cargo build` must need no wasm tools, so the web app stays out of `default-members` (the root manifest says
/// why). It stays a member, which is what `cargo xtask ci` (`--workspace`) builds, tests and lints natively. This fails
/// if the web app is added to the default members, by name or by a glob such as `apps/*`, and if it stops being a member.
#[test]
fn a_plain_build_leaves_the_web_app_out_and_the_workspace_has_it() {
    let metadata = xtask::metadata().unwrap();
    let (members, defaults) = (package_names(&metadata, "workspace_members"), package_names(&metadata, "workspace_default_members"));
    assert!(members.iter().any(|n| n == "diacraft-web"), "diacraft-web is no workspace member, so `cargo xtask ci` never builds it: {members:?}");
    assert!(
        !defaults.iter().any(|n| n == "diacraft-web"),
        "diacraft-web is a default member: a plain `cargo build` would build the web app, which needs the wasm target and its tools ({defaults:?})"
    );
    // The check sees something: the desktop app and the CLI are default members.
    for app in ["diacraft", "diacraft-cli", "diacraft-engine"] {
        assert!(defaults.iter().any(|n| n == app), "{app} is not a default member ({defaults:?})");
    }
}

/// The features each resolved version of `package` has switched on, anywhere in the workspace's dependency graph, from the
/// output of `cargo metadata` (with dependencies): one list per version of the package that is in the graph.
fn features_enabled(metadata: &serde_json::Value, package: &str) -> Vec<Vec<String>> {
    let ids: Vec<&str> = metadata["packages"]
        .as_array()
        .expect("metadata has packages")
        .iter()
        .filter(|p| p["name"].as_str() == Some(package))
        .filter_map(|p| p["id"].as_str())
        .collect();
    metadata["resolve"]["nodes"]
        .as_array()
        .expect("metadata has a resolve graph")
        .iter()
        .filter(|node| node["id"].as_str().is_some_and(|id| ids.contains(&id)))
        .map(|node| node["features"].as_array().expect("a node lists its features").iter().filter_map(|f| f.as_str()).map(str::to_owned).collect())
        .collect()
}

/// serde_json stops at 128 levels of nesting, so a request that nests a thousand deep is an error and not a stack
/// overflow. The limit is on unless code calls `Deserializer::disable_recursion_limit`, and that method exists only when
/// the `unbounded_depth` feature is on, for every user of serde_json at once (cargo unifies features). Nothing calls it;
/// this fails if some crate, an ours or one of a dependency's, turns the feature on, so that it is a decision made
/// knowingly: see the MCP server and the control server, which read hostile lines.
#[test]
fn serde_json_keeps_its_recursion_limit() {
    let out = xtask::cargo().args(["metadata", "--format-version", "1"]).output().unwrap();
    assert!(out.status.success(), "cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let found = features_enabled(&metadata, "serde_json");
    assert!(
        found.iter().any(|features| features.iter().any(|f| f == "std")),
        "no serde_json with its features in the metadata: the check sees nothing ({found:?})"
    );
    let lifted: Vec<&Vec<String>> = found.iter().filter(|features| features.iter().any(|f| f == "unbounded_depth")).collect();
    assert!(
        lifted.is_empty(),
        "some crate turned serde_json's `unbounded_depth` feature on ({lifted:?}): it lets code remove the recursion limit that keeps hostile JSON from overflowing the stack"
    );
}

/// The check above reads what cargo writes, so it is tried on what cargo writes: the feature on one of two versions, and a
/// different package that has a feature of that name.
#[test]
fn the_recursion_limit_check_finds_the_feature() {
    let metadata = serde_json::json!({
        "packages": [
            {"name": "serde_json", "id": "registry+https://example.org/index#serde_json@1.0.1"},
            {"name": "serde_json", "id": "registry+https://example.org/index#serde_json@2.0.0"},
            {"name": "other", "id": "registry+https://example.org/index#other@1.0.0"},
        ],
        "resolve": {"nodes": [
            {"id": "registry+https://example.org/index#serde_json@1.0.1", "features": ["default", "std"]},
            {"id": "registry+https://example.org/index#serde_json@2.0.0", "features": ["std", "unbounded_depth"]},
            {"id": "registry+https://example.org/index#other@1.0.0", "features": ["unbounded_depth"]},
        ]},
    });
    let found = features_enabled(&metadata, "serde_json");
    assert_eq!(found, [vec!["default".to_owned(), "std".to_owned()], vec!["std".to_owned(), "unbounded_depth".to_owned()]]);
    assert_eq!(features_enabled(&metadata, "other"), [vec!["unbounded_depth".to_owned()]]);
    assert!(features_enabled(&metadata, "absent").is_empty());
}

// ------------------------------------------------------------------------------------------------------ unused entries of the root manifest

/// The names of the `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` of a member manifest, those under a `[target.*]` too, that say `workspace = true`.
fn inherited_dependencies(member: &toml::Table) -> Vec<String> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut tables: Vec<&toml::Table> = KINDS.iter().filter_map(|kind| member.get(*kind)?.as_table()).collect();
    if let Some(targets) = member.get("target").and_then(|t| t.as_table()) {
        for target in targets.values().filter_map(|t| t.as_table()) {
            tables.extend(KINDS.iter().filter_map(|kind| target.get(*kind)?.as_table()));
        }
    }
    let says_workspace = |spec: &toml::Value| spec.get("workspace").and_then(|w| w.as_bool()) == Some(true);
    tables.into_iter().flat_map(|table| table.iter().filter(|(_, spec)| says_workspace(spec)).map(|(name, _)| name.clone())).collect()
}

/// The `[package]` fields of a member manifest that say `workspace = true` (`version.workspace = true`).
fn inherited_fields(member: &toml::Table) -> Vec<String> {
    let Some(package) = member.get("package").and_then(|p| p.as_table()) else { return Vec::new() };
    package.iter().filter(|(_, v)| v.get("workspace").and_then(|w| w.as_bool()) == Some(true)).map(|(name, _)| name.clone()).collect()
}

/// What cargo's two manifest lints (`unused_workspace_dependencies`, `unused_workspace_package_fields`) warn of on every command: the `[workspace.dependencies]` and the
/// `[workspace.package]` fields that no member of `members` inherits. → both lists, sorted.
fn unused_entries(root: &str, members: &[&str]) -> (Vec<String>, Vec<String>) {
    let root: toml::Table = root.parse().expect("the root manifest is TOML");
    let workspace = root.get("workspace").and_then(|w| w.as_table());
    let names = |key: &str| -> Vec<String> {
        workspace.and_then(|w| w.get(key)).and_then(|t| t.as_table()).map(|t| t.keys().cloned().collect()).unwrap_or_default()
    };
    let (mut dependencies, mut fields) = (names("dependencies"), names("package"));
    for member in members {
        let member: toml::Table = member.parse().expect("a member manifest is TOML");
        let (used_dependencies, used_fields) = (inherited_dependencies(&member), inherited_fields(&member));
        dependencies.retain(|name| !used_dependencies.contains(name));
        fields.retain(|name| !used_fields.contains(name));
    }
    dependencies.sort();
    fields.sort();
    (dependencies, fields)
}

/// Every manifest of the workspace's members, read as the root's `members` names them (`crates/*`, `apps/*`, `xtask`).
fn member_manifests() -> Vec<String> {
    let root = xtask::root();
    let manifest: toml::Table = std::fs::read_to_string(root.join("Cargo.toml")).unwrap().parse().unwrap();
    let members = manifest.get("workspace").and_then(|w| w.get("members")).and_then(|m| m.as_array()).expect("the root manifest lists its members");
    let mut folders = Vec::new();
    for member in members.iter().filter_map(|m| m.as_str()) {
        match member.strip_suffix("/*") {
            Some(parent) => {
                let mut found: Vec<_> =
                    std::fs::read_dir(root.join(parent)).unwrap().flatten().map(|e| e.path()).filter(|p| p.join("Cargo.toml").is_file()).collect();
                found.sort();
                folders.extend(found);
            }
            None => folders.push(root.join(member)),
        }
    }
    assert!(folders.len() > 20, "the members were not found: {folders:?}");
    folders.iter().map(|folder| std::fs::read_to_string(folder.join("Cargo.toml")).unwrap()).collect()
}

/// A `[workspace.dependencies]` entry and a `[workspace.package]` field that no member uses make cargo print a warning on every build
/// (`unused workspace dependency`, `unused field in workspace.package`). A crate that is not used yet is added to the table with the first member that depends on it, not before,
/// and a field is inherited by the members that ship.
#[test]
fn the_root_manifest_has_no_unused_workspace_dependency_or_package_field() {
    let root = std::fs::read_to_string(xtask::root().join("Cargo.toml")).unwrap();
    let members = member_manifests();
    let members: Vec<&str> = members.iter().map(String::as_str).collect();
    let (dependencies, fields) = unused_entries(&root, &members);
    assert!(
        dependencies.is_empty(),
        "workspace dependencies that no member uses (cargo warns of each on every build): {dependencies:?}; declare each with `{{ workspace = true }}` in the crate that uses it"
    );
    assert!(
        fields.is_empty(),
        "`workspace.package` fields that no member inherits (cargo warns of each on every build): {fields:?}; inherit each with `field.workspace = true`"
    );
}

/// The check above is tried on made-up manifests, in every spelling cargo reads: a dotted key, a table, a rename of the table, the three kinds of dependency, a target's, and
/// a dependency that is not the workspace's.
#[test]
fn the_unused_entry_check_reads_every_spelling_of_an_inheritance() {
    let root = r#"
[workspace]
members = ["a", "b"]
[workspace.package]
version = "1"
edition = "2024"
repository = "https://example.org/x"
[workspace.dependencies]
one = { path = "one" }
two = { path = "two" }
three = "3"
four = "4"
five = "5"
six = "6"
"#;
    let a = r#"
[package]
name = "a"
version.workspace = true
edition = "2024"
[dependencies]
one = { workspace = true }
two = { workspace = true, features = ["x"] }
[dev-dependencies.three]
workspace = true
"#;
    let b = r#"
[package]
name = "b"
[package.repository]
workspace = true
[build-dependencies]
four = { workspace = true }
[target.'cfg(windows)'.dependencies]
five = { workspace = true }
[dependencies]
six = "6"
"#;
    assert_eq!(
        unused_entries(root, &[a, b]),
        (vec!["six".to_owned()], vec!["edition".to_owned()]),
        "six is the crate's own `six`, and edition is spelled out in a"
    );
    assert_eq!(unused_entries(root, &[a]).0, ["five", "four", "six"]);
    assert_eq!(
        unused_entries(root, &[]),
        (
            vec!["five", "four", "one", "six", "three", "two"].into_iter().map(String::from).collect(),
            vec!["edition".to_owned(), "repository".to_owned(), "version".to_owned()]
        )
    );
}
