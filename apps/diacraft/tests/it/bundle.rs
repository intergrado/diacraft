//! The macOS app bundle: the program inside `DiaCraft.app` is named `DiaCraft`. winit's app menu names its items after the
//! running program's file name ("About …", "Hide …", "Quit …"), so a program named `diacraft` gave "Quit diacraft" in lower case.
//! The name is written in three places that must agree: the bundle's `Info.plist` (`CFBundleExecutable`), the script that builds the
//! download (`packaging/package.sh`), and the release workflow's check of what is in it (`.github/workflows/release.yml`).

use std::path::PathBuf;

/// The name of the program inside the bundle, which the app menu shows.
const PROGRAM: &str = "DiaCraft";

fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The `<string>` after `<key>KEY</key>` in a property list.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after.trim_start().strip_prefix("<string>")?;
    Some(value.split_once("</string>")?.0.to_owned())
}

/// Every name a file gives a program in the bundle's `Contents/MacOS` folder.
fn bundle_programs(text: &str) -> Vec<String> {
    text.split("Contents/MacOS/")
        .skip(1)
        .map(|rest| rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.').collect())
        .collect()
}

#[test]
fn the_program_inside_the_app_is_named_as_the_app_so_its_menu_reads_quit_diacraft() {
    let plist = repo_file("packaging/macos/Info.plist");
    assert_eq!(plist_string(&plist, "CFBundleName").as_deref(), Some(PROGRAM));
    assert_eq!(plist_string(&plist, "CFBundleExecutable").as_deref(), Some(PROGRAM), "the menu says \"Quit\" and the program's file name");

    for path in ["packaging/package.sh", ".github/workflows/release.yml"] {
        let names = bundle_programs(&repo_file(path));
        assert!(!names.is_empty(), "{path} names no program in Contents/MacOS");
        assert!(names.iter().all(|n| n == PROGRAM), "{path} names {names:?} in Contents/MacOS, where the bundle's program is {PROGRAM}");
    }
}

#[test]
fn the_command_line_beside_the_app_keeps_its_name() {
    let package = repo_file("packaging/package.sh");
    assert!(package.contains(r#"join_bin diacraft-cli "$dir/diacraft-cli""#), "the CLI stays diacraft-cli, beside the app");
    // The app's program is built as `diacraft` and only renamed in the bundle.
    assert!(package.contains(&format!(r#"join_bin diacraft "$app/Contents/MacOS/{PROGRAM}""#)));
}
