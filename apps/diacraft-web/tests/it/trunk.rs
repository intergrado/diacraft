//! `Trunk.toml`: how `trunk build` and `trunk serve` treat the page.

use crate::{app_path, read};

fn trunk() -> toml::Table {
    read("Trunk.toml").parse().unwrap()
}

#[test]
fn trunk_builds_the_page_that_is_here_into_a_folder_git_ignores() {
    let trunk = trunk();
    let build = trunk["build"].as_table().unwrap();
    let target = build["target"].as_str().unwrap();
    assert!(app_path(target).is_file(), "`target` is {target}, which is not a file here");
    // The site goes to `dist/web` at the repository root: the one `/dist` line of .gitignore covers it.
    assert_eq!(build["dist"].as_str(), Some("../../dist/web"));
    let ignore = std::fs::read_to_string(app_path("../../.gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l.trim() == "/dist"), "dist/ is not in .gitignore");
}

/// A hook runs a command on every build. This app's needs none, and a copied one (a font step of a sibling's own
/// xtask) would name a command that does not exist here.
#[test]
fn trunk_runs_no_hook_and_serves_on_the_loopback_address_only() {
    let trunk = trunk();
    assert!(trunk.get("hooks").is_none(), "Trunk.toml runs a hook: {:?}", trunk.get("hooks"));
    let addresses: Vec<&str> = trunk["serve"]["addresses"].as_array().unwrap().iter().filter_map(|a| a.as_str()).collect();
    assert_eq!(addresses, ["127.0.0.1"]);
    let port = trunk["serve"]["port"].as_integer().unwrap();
    assert!((1024..=65535).contains(&port), "port {port}");
}

/// The engine's recursion limits (nested groups, XML depth, a formula's nesting and chain) were checked on stacks of 2 MiB and more, and a WebAssembly stack is 1 MiB unless the link says more
/// The web build links with 8 MiB, as the Windows programs do, and `.cargo/config.toml` is where it says so.
#[test]
fn the_web_build_links_with_an_8_mib_stack() {
    let config: toml::Table = std::fs::read_to_string(app_path("../../.cargo/config.toml")).unwrap().parse().unwrap();
    let flags: Vec<&str> = config["target"]["wasm32-unknown-unknown"]["rustflags"].as_array().unwrap().iter().filter_map(|f| f.as_str()).collect();
    assert_eq!(flags, ["-C", "link-arg=-zstack-size=8388608"], "the stack of a WebAssembly program is set at the link");
    // The Windows programs keep the same size: a flag changed in one place and not the other would give the targets different margins.
    let windows: Vec<&str> = config["target"]["x86_64-pc-windows-msvc"]["rustflags"].as_array().unwrap().iter().filter_map(|f| f.as_str()).collect();
    assert!(windows.contains(&"link-arg=/STACK:8388608"), "{windows:?}");
}
