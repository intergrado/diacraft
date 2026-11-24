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
