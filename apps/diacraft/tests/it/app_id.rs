//! The application's id (APPID, the owner, 2026-11-23: it comes from the domain `diacraft.app`): `app.diacraft`. It is named in the places that must agree for the desktop to match the window
//! to its launcher and its icon: the program (`APP_ID`, the Wayland and X11 app id), the Linux desktop entry (its file name, `Icon` and `StartupWMClass`), the script that installs it, the
//! script that packages the download, the macOS bundle (`CFBundleIdentifier`), the release workflow's check of what an install made, and the Linux README's list. **The id the project had
//! before is named in none of them.**

use std::path::PathBuf;

/// The id of the application.
const ID: &str = "app.diacraft";
/// The id before it, spelled in pieces so that this file does not name it.
fn earlier_id() -> String {
    ["io.github.", "one_bit", ".diacraft"].concat()
}

/// The root of the tree the test runs in: the private repository, or the tree that is published from it.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn repo_file(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The rules file that the published tree has as its `AGENTS.md`. The private repository keeps it as `public/AGENTS.md` (its own `AGENTS.md` is
/// another file, which names the namespace of the builds before 0.2), and the published tree has it as `AGENTS.md` with no `public` folder.
fn published_rules() -> &'static str {
    if repo_root().join("public").join("AGENTS.md").is_file() { "public/AGENTS.md" } else { "AGENTS.md" }
}

/// The value of `key=` on a line of a desktop entry.
fn entry_value(entry: &str, key: &str) -> Option<String> {
    entry.lines().find_map(|line| line.strip_prefix(&format!("{key}=")).map(str::to_owned))
}

/// The `<string>` after `<key>KEY</key>` in a property list.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after.trim_start().strip_prefix("<string>")?;
    Some(value.split_once("</string>")?.0.to_owned())
}

#[test]
fn the_window_s_app_id_is_the_one_the_desktop_entry_names() {
    let main = repo_file("apps/diacraft/src/main.rs");
    let declared = main.lines().find(|l| l.starts_with("const APP_ID")).unwrap_or_else(|| panic!("no APP_ID in main.rs"));
    assert!(declared.contains(&format!("\"{ID}\"")), "{declared}");
    assert!(main.contains(".with_app_id(APP_ID)"), "the viewport is given the id");
    // The entry: named for the id, and its icon and the class the window manager matches are the id.
    let entry = repo_file(&format!("packaging/linux/{ID}.desktop"));
    assert_eq!(entry_value(&entry, "Icon").as_deref(), Some(ID));
    assert_eq!(entry_value(&entry, "StartupWMClass").as_deref(), Some(ID));
    assert_eq!(entry_value(&entry, "Name").as_deref(), Some("DiaCraft"), "what a person reads is the name, and not the id");
    assert!(entry_value(&entry, "Exec").is_some_and(|e| e.starts_with("diacraft ")), "the entry starts the program by name");
}

#[test]
fn the_scripts_the_workflow_and_the_bundle_name_the_same_id() {
    let install = repo_file("packaging/linux/install.sh");
    assert!(install.lines().any(|l| l == format!("id={ID}")), "install.sh installs the entry and the icon under the id");
    let package = repo_file("packaging/package.sh");
    for name in [format!("{ID}.desktop"), format!("{ID}.png")] {
        assert!(package.contains(&format!("\"$dir/{name}\"")), "package.sh puts {name} in the download");
    }
    assert!(package.contains(&format!("packaging/linux/{ID}.desktop")), "package.sh takes the entry from the folder of the Linux files");
    let workflow = repo_file(".github/workflows/release.yml");
    assert!(workflow.contains(&format!("share/applications/{ID}.desktop")), "the workflow looks for the entry install.sh made");
    let readme = repo_file("packaging/linux/README.txt");
    assert!(readme.contains(&format!("{ID}.desktop")) && readme.contains(&format!("{ID}.png")), "the README lists the entry and the icon");
    let plist = repo_file("packaging/macos/Info.plist");
    assert_eq!(plist_string(&plist, "CFBundleIdentifier").as_deref(), Some(ID), "the bundle's identifier");
}

#[test]
fn the_id_before_it_is_named_in_none_of_them() {
    let earlier = earlier_id();
    for path in [
        "apps/diacraft/src/main.rs",
        &format!("packaging/linux/{ID}.desktop"),
        "packaging/linux/install.sh",
        "packaging/linux/README.txt",
        "packaging/package.sh",
        "packaging/macos/Info.plist",
        ".github/workflows/release.yml",
        published_rules(),
    ] {
        assert!(!repo_file(path).contains(&earlier), "{path} names the id the project had before");
    }
    assert!(
        !PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux").join(format!("{earlier}.desktop")).exists(),
        "the entry of the earlier id is gone from the tree"
    );
}
