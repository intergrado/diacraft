//! The downloads, named in three places that must agree: `site/site.json` (the file each button of the site asks the release for), the release workflow (the files it
//! uploads and lists), and the home page (a link for each of them). Nothing here builds or fetches a file.

use std::collections::BTreeSet;

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The `assets` of `site/site.json`: the id of each download, and the name of its file with `{tag}` for the release's tag.
fn assets() -> Vec<(String, String)> {
    let settings: serde_json::Value = serde_json::from_str(&read("site/site.json")).unwrap();
    settings["assets"].as_object().expect("site.json has assets").iter().map(|(id, name)| (id.clone(), name.as_str().unwrap().to_owned())).collect()
}

#[test]
fn the_site_asks_for_the_twelve_files_the_release_workflow_lists_and_checks() {
    let assets = assets();
    assert_eq!(assets.len(), 12, "{assets:?}");
    let workflow = read(".github/workflows/release.yml");
    // The list the last job checks the release against: `"diacraft-$NAME-...` words in the `expected=$(printf` command.
    let start = workflow.find("expected=$(printf").expect("the checksums job has a list of the files");
    let list = &workflow[start..workflow[start..].find("| sort").map(|i| start + i).expect("the list is sorted")];
    let listed: BTreeSet<String> =
        list.split('"').filter(|word| word.starts_with("diacraft-$NAME-")).map(|word| word.replace("$NAME", "{tag}")).collect();
    let named: BTreeSet<String> = assets.iter().map(|(_, name)| name.clone()).collect();
    assert_eq!(listed, named, "the files the checksums job expects and the files the site asks for are the same twelve");
    // The notes of the draft name every file too.
    for (id, name) in &assets {
        let in_notes = name.replace("{tag}", "$TAG");
        assert!(workflow.contains(&in_notes), "{id}: the draft's notes do not name {in_notes}");
    }
}

#[test]
fn the_names_follow_one_pattern_system_cpu_and_kind() {
    for (id, name) in assets() {
        let mut parts = id.split('-');
        let (system, cpu, kind) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        assert!(parts.next().is_none(), "{id}");
        let extension = match kind {
            "appimage" => "AppImage",
            "targz" => "tar.gz",
            other => other,
        };
        assert_eq!(name, format!("diacraft-{{tag}}-{system}-{cpu}.{extension}"), "{id}");
        assert!(["x86_64", "arm64", "universal"].contains(&cpu), "{id}");
        assert_eq!(cpu == "universal", system == "macos", "only macOS has one build for both CPUs: {id}");
    }
}

/// The anchors of the home page that carry `data-download`, as (the start tag, the text up to `</a>`).
fn download_links(page: &str) -> Vec<(String, String)> {
    page.split("<a ")
        .skip(1)
        .filter_map(|piece| {
            let (tag, rest) = piece.split_once('>')?;
            let (text, _) = rest.split_once("</a>")?;
            tag.contains("data-download=\"").then(|| (tag.to_owned(), text.to_owned()))
        })
        .collect()
}

fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
    let start = tag.find(&format!("{name}=\"")).unwrap_or_else(|| panic!("{tag} has no {name}")) + name.len() + 2;
    tag[start..].split('"').next().unwrap()
}

#[test]
fn the_home_page_links_each_download_once_with_the_installer_as_the_main_button_of_each_system() {
    let page = read("site/index.html");
    let links = download_links(&page);
    let assets = assets();
    let ids: Vec<&str> = links.iter().map(|(tag, _)| attribute(tag, "data-download")).collect();
    let unique: BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "a download is linked once: {ids:?}");
    assert_eq!(unique, assets.iter().map(|(id, _)| id.as_str()).collect::<BTreeSet<_>>(), "every download has a link, and every link a download");
    for (tag, text) in &links {
        let id = attribute(tag, "data-download");
        let name = &assets.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(attribute(tag, "data-asset"), name, "{id}: the page names the file site.json names (cargo xtask site --write)");
        // With no script the link goes to the release page, as it always did.
        assert!(attribute(tag, "href").ends_with("/releases/latest"), "{id}: {tag}");
        assert!(tag.contains("data-repo=\"/releases/latest\""), "{id}");
        // A reader hears what it is, and which CPU; a link that says only `.zip` has the whole sentence in its label.
        let label = attribute(tag, "aria-label");
        assert!(label.starts_with("Download DiaCraft for "), "{id}: {label}");
        let cpu = id.split('-').nth(1).unwrap();
        let said = match cpu {
            "x86_64" => "Intel/AMD (x64)",
            "arm64" => "ARM64",
            _ => "Apple silicon and Intel",
        };
        assert!(label.contains(said), "{id}: the label says the CPU: {label}");
        assert!(!text.is_empty(), "{id}");
    }
    // The main button of each system is its installer, for the CPU most people have; the other CPU and the archives are the smaller links.
    let main: Vec<&str> = links.iter().filter(|(tag, _)| tag.contains(" class=\"btn\"")).map(|(tag, _)| attribute(tag, "data-download")).collect();
    assert_eq!(main, ["linux-x86_64-appimage", "windows-x86_64-msi", "macos-universal-dmg"]);
}

#[test]
fn the_page_says_the_other_cpus_in_the_words_the_labels_use() {
    let page = read("site/index.html");
    for label in ["Intel/AMD (x64)", "ARM64", "Universal"] {
        assert!(page.contains(&format!("<dt>{label}</dt>")), "a row for {label}");
    }
    // The script finds the links by the same attribute the page and `site.json` use.
    let script = read("site/download.js");
    assert!(script.contains("a[data-download][data-asset]") && !script.contains("data-platform"), "download.js reads data-download");
}
