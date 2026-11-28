//! The downloads, named in three places that must agree: `site/site.json` (the file each button of the site asks the release for), the release workflow (the files it
//! uploads and lists), and the home page (a link for each of them). Nothing here builds or fetches a file.

use std::collections::BTreeSet;

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The `assets` of `site/site.json`: the id of each download, and the name of its file with `{version}` for the release's version (the tag without its `v`).
fn assets() -> Vec<(String, String)> {
    let settings: serde_json::Value = serde_json::from_str(&read("site/site.json")).unwrap();
    settings["assets"].as_object().expect("site.json has assets").iter().map(|(id, name)| (id.clone(), name.as_str().unwrap().to_owned())).collect()
}

/// The list the release workflow's last job checks the release against: the `"diacraft-$VERSION-...` words of its `expected=$(printf` command, with `{version}` for the version.
fn listed_files() -> Vec<String> {
    let workflow = read(".github/workflows/release.yml");
    let start = workflow.find("expected=$(printf").expect("the checksums job has a list of the files");
    let list = &workflow[start..workflow[start..].find("| sort").map(|i| start + i).expect("the list is sorted")];
    list.split('"').filter(|word| word.starts_with("diacraft-$VERSION-")).map(|word| word.replace("$VERSION", "{version}")).collect()
}

#[test]
fn the_site_asks_for_the_files_the_release_workflow_lists_and_checks() {
    let assets = assets();
    assert_eq!(assets.len(), 16, "eight Linux files, four Windows files and four macOS files: {assets:?}");
    let workflow = read(".github/workflows/release.yml");
    let listed: BTreeSet<String> = listed_files().into_iter().collect();
    let named: BTreeSet<String> = assets.iter().map(|(_, name)| name.clone()).collect();
    assert_eq!(listed, named, "the files the checksums job expects and the files the site asks for are the same");
    // The notes of the draft name every file too.
    for (id, name) in &assets {
        let in_notes = name.replace("{version}", "$VERSION");
        assert!(workflow.contains(&in_notes), "{id}: the draft's notes do not name {in_notes}");
    }
}

/// The id of a download is the site's own key: the system, the CPU as the site says it (`x86_64` or `arm64`), and the kind. The name of its file says the CPU in the words of the
/// launchers: `aarch64` for Linux on ARM, `x64` for Windows on Intel or AMD, and for the Windows zip `-portable`.
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
        let in_name = match (system, cpu) {
            ("linux", "arm64") => "aarch64",
            ("windows", "x86_64") => "x64",
            _ => cpu,
        };
        let portable = if (system, kind) == ("windows", "zip") { "-portable" } else { "" };
        assert_eq!(name, format!("diacraft-{{version}}-{system}-{in_name}{portable}.{extension}"), "{id}");
        // There is no build for both CPUs at once (the owner, 2026-11-25: no universal macOS download): each download is for one CPU.
        assert!(["x86_64", "arm64"].contains(&cpu), "{id}");
    }
}

/// The names the launchers read, `diacraft-<version>-<system>-<cpu>[-portable].<extension>` with a version that starts with a digit, for one release of the example version 0.2.1:
/// the sixteen files, written out, as the release workflow lists them. The git tag stays `v0.2.1`, and so does the folder of its release; no file name has the `v`.
#[test]
fn the_sixteen_files_of_a_release_are_named_as_the_launchers_read_them() {
    let wanted = [
        "diacraft-0.2.1-linux-x86_64.AppImage",
        "diacraft-0.2.1-linux-x86_64.deb",
        "diacraft-0.2.1-linux-x86_64.rpm",
        "diacraft-0.2.1-linux-x86_64.tar.gz",
        "diacraft-0.2.1-linux-aarch64.AppImage",
        "diacraft-0.2.1-linux-aarch64.deb",
        "diacraft-0.2.1-linux-aarch64.rpm",
        "diacraft-0.2.1-linux-aarch64.tar.gz",
        "diacraft-0.2.1-windows-x64.msi",
        "diacraft-0.2.1-windows-x64-portable.zip",
        "diacraft-0.2.1-windows-arm64.msi",
        "diacraft-0.2.1-windows-arm64-portable.zip",
        "diacraft-0.2.1-macos-arm64.dmg",
        "diacraft-0.2.1-macos-arm64.zip",
        "diacraft-0.2.1-macos-x86_64.dmg",
        "diacraft-0.2.1-macos-x86_64.zip",
    ];
    let named = |names: Vec<String>| -> BTreeSet<String> { names.iter().map(|name| name.replace("{version}", "0.2.1")).collect() };
    let in_workflow = named(listed_files());
    let in_site = named(assets().into_iter().map(|(_, name)| name).collect());
    let wanted: BTreeSet<String> = wanted.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(in_workflow, wanted, "the workflow's list");
    assert_eq!(in_site, wanted, "site.json's names");
    for name in &wanted {
        let rest = name.strip_prefix("diacraft-").expect(name);
        let (version, rest) = rest.split_once('-').expect(name);
        assert!(version.starts_with(|c: char| c.is_ascii_digit()), "{name}: the version starts with a digit");
        let (system, rest) = rest.split_once('-').expect(name);
        let (cpu, extension) = rest.split_once('.').expect(name);
        let (cpu, portable) = cpu.split_once('-').map_or((cpu, false), |(cpu, word)| (cpu, word == "portable"));
        let cpus: &[&str] = match system {
            "linux" => &["x86_64", "aarch64"],
            "windows" => &["x64", "arm64"],
            "macos" => &["arm64", "x86_64"],
            _ => &[],
        };
        assert!(cpus.contains(&cpu), "{name}: {system} has no CPU {cpu}");
        let extensions: &[&str] = match system {
            "linux" => &["AppImage", "deb", "rpm", "tar.gz"],
            "windows" => &["msi", "zip"],
            _ => &["dmg", "zip"],
        };
        assert!(extensions.contains(&extension), "{name}");
        // Only the Windows zip is `-portable`, and it always is: the installer and the zips of the other systems are not.
        assert_eq!(portable, (system, extension) == ("windows", "zip"), "{name}");
    }
}

/// The workflow gives the version, the tag without its `v`, to every file name, and the tag only to the release itself: no file the workflow makes, uploads, checks or lists is named
/// by `$NAME`, and the checksums file is `SHA256SUMS.txt`.
#[test]
fn the_workflow_names_every_file_by_the_version_and_the_checksums_file_sha256sums_txt() {
    let workflow = read(".github/workflows/release.yml");
    let live: Vec<&str> = workflow.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    for line in &live {
        assert!(!line.contains("diacraft-$NAME-") && !line.contains("diacraft-$TAG-"), "a file named by the tag: {line}");
        assert!(
            !line.contains("windows-$ARCH") && !line.contains("linux-$ARCH"),
            "a file named by the CPU of the matrix, which is not the CPU of the name: {line}"
        );
    }
    assert!(workflow.contains("version=${tag#v}"), "the plan makes the version from the tag");
    assert!(workflow.contains("version: ${{ steps.plan.outputs.version }}"), "the plan gives it to the jobs");
    // The name of the sums file: the one the launchers check, in the upload, the check and the notes; the old one is nowhere.
    for rel in [
        ".github/workflows/release.yml",
        "README.md",
        "packaging/linux/README.txt",
        "packaging/macos/README.txt",
        "packaging/windows/README.txt",
        "site/index.html",
    ] {
        let text = read(rel);
        let mut bare = 0;
        for (at, _) in text.match_indices("SHA256SUMS") {
            if !text[at + "SHA256SUMS".len()..].starts_with(".txt") {
                bare += 1;
            }
        }
        assert_eq!(bare, 0, "{rel} names a checksums file `SHA256SUMS` with no `.txt`");
    }
    let checksums = job("checksums");
    assert!(
        checksums.contains(">SHA256SUMS.txt") && checksums.contains("downloads/SHA256SUMS.txt --repo"),
        "the file is written and uploaded under its name"
    );
    assert!(checksums.contains("sha256sum --check --strict SHA256SUMS.txt"), "its lines are checked in sha256sum's own format");
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

/// The rows of a card's CPUs: from `<dl class="cpus"` to its `</dl>`.
fn rows_of(card: &str) -> &str {
    card.split("<dl class=\"cpus\"").nth(1).and_then(|rest| rest.split("</dl>").next()).expect("the card's CPU rows")
}

/// The box of a card's main button: from `<div class="get-main">` to its `</div>`.
fn main_of(card: &str) -> &str {
    card.split("<div class=\"get-main\">").nth(1).and_then(|rest| rest.split("</div>").next()).expect("the card's main button")
}

/// The ids of the downloads that a fragment of the page links, in order.
fn ids_of(fragment: &str) -> Vec<String> {
    download_links(fragment).iter().map(|(tag, _)| attribute(tag, "data-download").to_owned()).collect()
}

/// The owner's ruling (2026-11-25): every CPU row lists all its formats, and the main button repeats one of them. So each of the sixteen files of the release is in the CPU rows exactly
/// once, and the main button of each card links one of the files of its card's rows (the same link, with the same address).
#[test]
fn the_home_page_lists_each_download_once_in_the_cpu_rows_and_the_main_button_of_each_system_repeats_one_of_them() {
    let page = read("site/index.html");
    let links = download_links(&page);
    let assets = assets();
    let version = xtask::version::read(&read("Cargo.toml")).unwrap();
    let repository = serde_json::from_str::<serde_json::Value>(&read("site/site.json")).unwrap()["repository"].as_str().unwrap().to_owned();
    let in_the_release: BTreeSet<String> = listed_files().iter().map(|name| name.replace("{version}", &version)).collect();
    let cards = cards(&page);
    assert_eq!(cards.len(), 3, "Linux, Windows and macOS");
    let in_rows: Vec<String> = cards.iter().flat_map(|card| ids_of(rows_of(card))).collect();
    let unique: BTreeSet<&str> = in_rows.iter().map(String::as_str).collect();
    assert_eq!(in_rows.len(), 16, "sixteen files in the rows: {in_rows:?}");
    assert_eq!(in_rows.len(), unique.len(), "a file is listed once in the CPU rows: {in_rows:?}");
    assert_eq!(
        unique,
        assets.iter().map(|(id, _)| id.as_str()).collect::<BTreeSet<_>>(),
        "every download is in a row, and every link of a row is a download"
    );
    // The page's links are those of the rows and the three main buttons, and no other.
    assert_eq!(links.len(), 16 + 3, "the rows' sixteen and the three main buttons");
    for card in &cards {
        let main = download_links(main_of(card));
        assert_eq!(main.len(), 1, "one main button in a card");
        let (tag, _) = &main[0];
        let id = attribute(tag, "data-download");
        let row = download_links(rows_of(card)).into_iter().find(|(row, _)| attribute(row, "data-download") == id);
        let (row, _) = row.unwrap_or_else(|| panic!("the main button {id} is one of the files of its card's rows"));
        for name in ["href", "data-asset", "aria-label"] {
            assert_eq!(attribute(tag, name), attribute(&row, name), "{id}: the button and its row's link are one link ({name})");
        }
    }
    for (tag, text) in &links {
        let id = attribute(tag, "data-download");
        let name = &assets.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(attribute(tag, "data-asset"), name, "{id}: the page names the file site.json names (cargo xtask site --write)");
        // With no script the link is the file of the release of the workspace's version: a click starts the download, and the file is one the release workflow lists. A file
        // that came with a later release (the `.rpm`, with 0.2.4) is no file of an earlier one, and a page made for that earlier release links the page of the release.
        let file = name.replace("{version}", &version);
        let href = if xtask::version::release_has(&version, id) {
            format!("https://github.com/{repository}/releases/download/v{version}/{file}")
        } else {
            format!("https://github.com/{repository}/releases/tag/v{version}")
        };
        assert_eq!(attribute(tag, "href"), href, "{id}: {tag}");
        assert!(tag.contains("data-repo=\"/releases/download\""), "{id}");
        assert!(in_the_release.contains(&file), "{id}: {file} is not one of the files the release workflow lists");
        // A reader hears what it is, and which CPU; a link that says only `.zip` has the whole sentence in its label.
        let label = attribute(tag, "aria-label");
        assert!(label.starts_with("Download DiaCraft for "), "{id}: {label}");
        let (system, cpu) = (id.split('-').next().unwrap(), id.split('-').nth(1).unwrap());
        let said = match (system, cpu) {
            ("macos", "x86_64") => "Intel (x64)",
            ("macos", "arm64") => "Apple silicon (ARM64)",
            (_, "x86_64") => "Intel/AMD (x64)",
            (_, "arm64") => "ARM64",
            _ => "Apple silicon and Intel",
        };
        assert!(label.contains(said), "{id}: the label says the CPU: {label}");
        assert!(!text.is_empty(), "{id}");
    }
    // The main button of each system is its installer, for the CPU most people have; the other CPU and the archives are the smaller links.
    let main: Vec<&str> = links.iter().filter(|(tag, _)| tag.contains(" class=\"btn\"")).map(|(tag, _)| attribute(tag, "data-download")).collect();
    assert_eq!(main, ["linux-x86_64-appimage", "windows-x86_64-msi", "macos-arm64-dmg"]);
}

#[test]
fn the_page_says_the_other_cpus_in_the_words_the_labels_use() {
    let page = read("site/index.html");
    for label in ["Intel/AMD (x64)", "ARM64", "Apple silicon (ARM64)", "Intel (x64)"] {
        assert!(page.contains(&format!("<dt>{label}</dt>")), "a row for {label}");
    }
    // The script finds the links by the same attribute the page and `site.json` use.
    let script = read("site/download.js");
    assert!(script.contains("a[data-download][data-asset]") && !script.contains("data-platform"), "download.js reads data-download");
}

/// The cards of the download section, as the text of each from `<div class="platform">` to the next card or the browser's band.
fn cards(page: &str) -> Vec<String> {
    let section =
        page.split("<div class=\"platforms\">").nth(1).and_then(|rest| rest.split("<div class=\"platform-browser\">").next()).expect("the platforms");
    section.split("<div class=\"platform\">").skip(1).map(str::to_owned).collect()
}

/// The owner's request (2026-11-25): the three main buttons stand at one height, whatever the length of the text above them, and nothing beside a main button names a CPU. The styles
/// put the three cards on four shared rows, one for each part of a card, so a card is those four parts and no other.
#[test]
fn the_main_buttons_stand_at_one_height_on_rows_the_three_cards_share_and_name_no_cpu_beside_them() {
    let page = read("site/index.html");
    let cards = cards(&page);
    assert_eq!(cards.len(), 3, "Linux, Windows and macOS");
    for card in &cards {
        let at: Vec<Option<usize>> =
            ["<h3>", "<dl class=\"spec\">", "<div class=\"get-main\">", "<dl class=\"cpus\""].iter().map(|part| card.find(part)).collect();
        assert!(
            at.iter().all(Option::is_some) && at.windows(2).all(|pair| pair[0] < pair[1]),
            "a card is its name, its facts, its button and its other downloads, in that order: {at:?}"
        );
        for part in ["<h3>", "<dl class=\"spec\">", "<div class=\"get-main\">", "<dl class=\"cpus\""] {
            assert_eq!(card.matches(part).count(), 1, "{part} once in a card");
        }
        // The button's own box holds the button and nothing else: no caption of the CPU.
        let button = card.split("<div class=\"get-main\">").nth(1).and_then(|rest| rest.split("</div>").next()).expect("the button's box");
        assert_eq!(button.matches("<a ").count(), 1, "{button}");
        assert!(!button.contains("<span class=\"get-"), "{button}");
    }
    assert!(!page.contains("get-cpu"), "the caption is gone");
    let css = read("site/styles.css");
    assert!(css.contains("grid-row: span 4") && css.contains("grid-template-rows: subgrid"), "the cards share four rows of the section's grid");
}

/// The script that upgrades the links: the links of a page are already the files of the release the page was made for, so the script reads the repository and the release
/// from them, replaces them only with the files of a strictly newer release (a release that is still a draft is not the latest one, and an older release must not replace the page's own),
/// and keeps every check it had on what it was given.
#[test]
fn the_script_upgrades_the_direct_links_only_to_a_newer_release_and_keeps_its_checks() {
    let script = read("site/download.js");
    for kept in [
        "a[data-download][data-asset]",
        "endsWith(\".invalid\")",
        "credentials: \"omit\"",
        "redirect: \"error\"",
        "referrerPolicy: \"no-referrer\"",
        "AbortSignal.timeout(3500)",
        "url.origin !== \"https://github.com\"",
        "url.username || url.password || url.search || url.hash",
        "decodeURIComponent(url.pathname) !== `/${path}/releases/download/${tag}/${expected}`",
        "item?.name === expected",
        "typeof tag !== \"string\"",
        "Array.isArray(release.assets)",
        "catch {",
    ] {
        assert!(script.contains(kept), "download.js no longer has `{kept}`");
    }
    // The repository comes from a direct link, which has the release in it; the page of the latest release is no link of a page.
    assert!(script.contains("releases\\/download\\/"), "download.js reads the repository from a link to a file of a release");
    assert!(!script.contains("releases\\/latest$"), "download.js no longer reads the page of the latest release from a link");
    assert!(script.contains("newer(latest, own)"), "only a newer release replaces the links");
    assert!(
        script.contains("if (!latest || !newer(latest, own)) return;"),
        "an older release, the same one or a tag that is no version leaves the links"
    );
    // The one request is the API's latest release, as it was.
    assert_eq!(script.matches("fetch(").count(), 1);
    assert!(script.contains("https://api.github.com/repos/${path}/releases/latest"));
}

/// The owner's ruling (2026-11-25): every CPU row of every card lists all its formats, in one order for the whole card. The Linux rows are `.AppImage .deb .rpm .tar.gz` (the `.rpm` beside the
/// `.deb`, for Fedora, and the main button as it was), the Windows rows `.msi .zip`
/// and the macOS rows `.dmg .zip`; the rows are one for each CPU, in the words of the labels.
#[test]
fn every_cpu_row_lists_all_its_formats_in_the_order_of_its_card() {
    let page = read("site/index.html");
    let cards = cards(&page);
    /// What a card's rows must hold: the card's name, its rows as (the CPU's label, the CPU in an id), and its formats as (the kind in an id, the text of the link).
    struct Card {
        name: &'static str,
        cpus: [(&'static str, &'static str); 2],
        formats: &'static [(&'static str, &'static str)],
    }
    let wanted = [
        Card {
            name: "Linux",
            cpus: [("Intel/AMD (x64)", "x86_64"), ("ARM64", "arm64")],
            formats: &[("appimage", ".AppImage"), ("deb", ".deb"), ("rpm", ".rpm"), ("targz", ".tar.gz")],
        },
        Card { name: "Windows", cpus: [("Intel/AMD (x64)", "x86_64"), ("ARM64", "arm64")], formats: &[("msi", ".msi"), ("zip", ".zip")] },
        Card { name: "macOS", cpus: [("Apple silicon (ARM64)", "arm64"), ("Intel (x64)", "x86_64")], formats: &[("dmg", ".dmg"), ("zip", ".zip")] },
    ];
    for Card { name, cpus, formats } in wanted {
        let card = cards.iter().find(|card| card.contains(&format!("<h3>{name}</h3>"))).unwrap_or_else(|| panic!("the {name} card"));
        let rows: Vec<&str> = rows_of(card).split("<div><dt>").skip(1).collect();
        assert_eq!(rows.len(), 2, "{name}: a row for each CPU");
        for (row, (label, cpu)) in rows.iter().zip(cpus) {
            assert!(row.starts_with(&format!("{label}</dt>")), "{name}: the row of {label}");
            let links = download_links(row);
            let system = name.to_lowercase();
            let ids: Vec<String> = links.iter().map(|(tag, _)| attribute(tag, "data-download").to_owned()).collect();
            let want: Vec<String> = formats.iter().map(|(kind, _)| format!("{system}-{cpu}-{kind}")).collect();
            assert_eq!(ids, want, "{name}, {label}: every format of the card, in the card's order");
            let texts: Vec<&str> = links.iter().map(|(_, text)| text.as_str()).collect();
            let want: Vec<&str> = formats.iter().map(|(_, text)| *text).collect();
            assert_eq!(texts, want, "{name}, {label}: the links say their formats");
        }
    }
}

/// The rows of a card hold all its downloads, so the list is named for what it holds (the owner, 2026-11-25): "All Linux downloads", "All Windows downloads" and "All macOS downloads", and
/// not "More ... downloads", which would say that the list holds what the main button does not.
#[test]
fn the_list_of_each_cards_rows_is_named_all_its_downloads() {
    let page = read("site/index.html");
    let cards = cards(&page);
    assert_eq!(cards.len(), 3, "Linux, Windows and macOS");
    for (card, name) in cards.iter().zip(["Linux", "Windows", "macOS"]) {
        assert!(card.contains(&format!("<h3>{name}</h3>")), "the cards are in the order {name} is");
        let tag = rows_of(card).split('>').next().expect("the list's start tag");
        assert_eq!(attribute(tag, "aria-label"), format!("All {name} downloads"));
    }
    assert!(!page.contains("More Linux") && !page.contains("More Windows") && !page.contains("More macOS"), "no list says it holds more downloads");
}

/// The owner's request (2026-11-25): the macOS card has two rows like the Linux and Windows cards, "Apple silicon (ARM64)" and "Intel (x64)", each with its `.dmg` and its `.zip` (builds of
/// one CPU each). There is no universal download (the owner's ruling, 2026-11-25): the main button is the `.dmg` for Apple silicon, as the other cards' main button is their x64 installer, and the
/// card says that the button is for Apple silicon Macs and that Intel Macs use the Intel row; its "File" line is as short as the other cards'.
#[test]
fn the_macos_card_has_a_main_button_for_apple_silicon_that_says_where_intel_macs_go() {
    let page = read("site/index.html");
    let cards = cards(&page);
    let mac = cards.iter().find(|card| card.contains("<h3>macOS</h3>")).expect("the macOS card");
    // The main button is the Apple silicon disk image, and the card says whom it is for and where the Intel Macs go.
    let main = main_of(mac);
    assert!(main.contains("data-download=\"macos-arm64-dmg\"") && main.contains("Apple silicon (ARM64), as a disk image"), "{main}");
    let facts = mac.split("<dl class=\"spec\">").nth(1).and_then(|rest| rest.split("</dl>").next()).unwrap();
    let fact = |name: &str| -> String {
        facts
            .split(&format!("<div><dt>{name}</dt><dd>"))
            .nth(1)
            .and_then(|rest| rest.split("</dd>").next())
            .unwrap_or_else(|| panic!("the {name} fact"))
            .to_owned()
    };
    assert_eq!(fact("File"), "<code>.dmg</code> disk image, or <code>.zip</code>", "one short line, as the other cards' are");
    let needs = fact("Needs");
    assert!(needs.contains("The button is for Apple silicon Macs; Intel Macs use the Intel row below."), "{needs}");
    assert!(!facts.contains("<a ") && !facts.to_lowercase().contains("universal"), "{facts}");
    // No caption beside the button names the CPUs.
    assert!(!main.contains("get-cpu") && main.matches("<a ").count() == 1, "{main}");
}

/// The text of one job of the release workflow, without its comment lines: from its `  <name>:` line to the line of the next job.
fn job(name: &str) -> String {
    let workflow = read(".github/workflows/release.yml");
    let mut in_job = false;
    let mut lines = Vec::new();
    for line in workflow.lines() {
        let is_job_line = line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') && !line.trim_start().starts_with('#');
        if is_job_line {
            in_job = line.trim() == format!("{name}:");
        }
        if in_job && !line.trim_start().starts_with('#') {
            lines.push(line);
        }
    }
    assert!(!lines.is_empty(), "release.yml has no `{name}` job");
    lines.join("\n")
}

/// The owner's request (2026-11-25): the macOS job makes a download for each architecture alone, each a zip and a disk image, and no universal one (no program joined from the two builds); checks
/// each (the zip unpacked, the image attached read only, the architecture `lipo` shows, the licences); runs the Intel programs where Rosetta lets it and says which it did; and uploads the four
/// files.
#[test]
fn the_macos_job_makes_checks_and_uploads_a_download_for_each_cpu_and_no_universal_one() {
    let mac = job("macos");
    // Two packagings, each build on its own.
    let packagings: Vec<&str> = mac.lines().map(str::trim).filter(|l| l.starts_with("bash packaging/package.sh macos")).collect();
    assert_eq!(
        packagings,
        [
            "bash packaging/package.sh macos \"$VERSION\" dist target/aarch64-apple-darwin/release",
            "bash packaging/package.sh macos \"$VERSION\" dist target/x86_64-apple-darwin/release",
        ]
    );
    // Each is checked: the zip unpacked, the image attached read only, and the architectures that lipo shows are the ones of the download.
    for call in ["check_download arm64 arm64", "check_download x86_64 x86_64"] {
        assert!(mac.contains(call), "the check of a download is not run: {call}");
    }
    for needed in [
        "ditto -x -k \"dist/diacraft-$VERSION-macos-$cpu.zip\"",
        "hdiutil attach -nobrowse -readonly -noverify",
        "dist/diacraft-$VERSION-macos-$cpu.dmg",
        "lipo -archs",
        "packaging/smoke.sh --licences",
        "codesign --verify --strict",
    ] {
        assert!(mac.contains(needed), "the check of a macOS download does not say `{needed}`");
    }
    // The Intel programs run under Rosetta where it is there, and the step says which it did.
    assert!(mac.contains("arch -x86_64 /usr/bin/true"), "Rosetta is looked for before the Intel programs are run");
    assert!(mac.contains("arch -x86_64 \"$dir/diacraft-cli\" --version") && mac.contains("arch -x86_64 \"$app/Contents/MacOS/DiaCraft\" --version"));
    assert!(mac.contains("were run under Rosetta") && mac.contains("were not run, and their headers (lipo) were checked"), "it says which it did");
    // The six files are uploaded.
    let upload = mac.split("gh release upload").nth(1).expect("the upload");
    for cpu in ["arm64", "x86_64"] {
        for kind in ["dmg", "zip"] {
            assert!(upload.contains(&format!("dist/diacraft-$VERSION-macos-{cpu}.{kind}")), "{cpu} {kind} is not uploaded");
        }
    }
}

/// The README of a macOS download says which Macs it is for (one chip): every `@WORD@` of it is one the packaging script fills in, and the script sets the words for each of the two
/// architectures a macOS download has.
#[test]
fn the_macos_readme_says_which_macs_the_download_is_for_in_words_the_script_fills_in() {
    let readme = read("packaging/macos/README.txt");
    let script = read("packaging/package.sh");
    let words: BTreeSet<&str> = readme.split('@').skip(1).step_by(2).collect();
    for word in ["NAME", "REPO_URL", "ARCH", "CPU", "CHIP"] {
        assert!(words.contains(word), "the macOS README has no @{word}@");
    }
    for word in &words {
        assert!(script.contains(&format!("s|@{word}@|")), "package.sh fills no @{word}@");
    }
    let mac = script.split("\nmacos)\n").nth(1).expect("the macOS part of package.sh");
    for (arch, cpu, chip) in [("arm64", "Apple silicon", "an Apple silicon chip (M1 or later)"), ("*", "Intel", "an Intel chip")] {
        let arm = mac.split(&format!("    {arch})\n")).nth(1).unwrap_or_else(|| panic!("package.sh has no case for {arch}"));
        let arm = arm.split(";;").next().unwrap();
        assert!(arm.contains(&format!("cpu=\"{cpu}\"")) && arm.contains(chip), "{arch}: {arm}");
    }
    // The README says which download is for which chip, and names no universal one.
    assert!(readme.contains("macos-arm64") && readme.contains("macos-x86_64") && readme.contains("About This Mac"), "{readme}");
    assert!(!readme.to_lowercase().contains("universal"), "{readme}");
}

/// The owner's ruling (2026-11-25): there is no universal macOS download, one program for both chips. Nothing the release makes, lists or shows says there is one: the workflow's commands, its list
/// and its notes, the packaging script (which takes one folder of programs and joins nothing), the site's settings and pages, the macOS README, and the changelog's notes.
#[test]
fn there_is_no_universal_macos_download_in_the_release_the_site_or_the_packaging() {
    let workflow = read(".github/workflows/release.yml");
    let live: Vec<&str> = workflow.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    assert!(
        !live.iter().any(|l| l.to_lowercase().contains("universal") || l.contains("lipo -create")),
        "release.yml makes or names a universal download"
    );
    let script = read("packaging/package.sh");
    assert!(!script.contains("lipo -create") && !script.contains("universal)"), "package.sh joins programs of two architectures");
    assert!(script.contains("macos takes one BIN_DIR"), "package.sh takes one folder of programs for macOS");
    for rel in ["site/site.json", "site/index.html", "site/changelog.html", "packaging/macos/README.txt"] {
        assert!(!read(rel).to_lowercase().contains("universal"), "{rel} names a universal download");
    }
}

// ------------------------------------------------------------------------------------------------------------------------- the MCP bundles

/// The bundles the release workflow's checksums job expects: the words of its list that start with `diacraft-mcp-$VERSION-`, with `{version}` for the version.
fn listed_bundles() -> Vec<String> {
    let workflow = read(".github/workflows/release.yml");
    let start = workflow.find("expected=$(printf").expect("the checksums job has a list of the files");
    let list = &workflow[start..workflow[start..].find("| sort").map(|i| start + i).expect("the list is sorted")];
    list.split('"').filter(|word| word.starts_with("diacraft-mcp-$VERSION-")).map(|word| word.replace("$VERSION", "{version}")).collect()
}

/// The owner's ruling (2026-11-25): the release also holds the MCP bundles, one for each system and CPU the downloads are built for (`xtask::mcp::BUNDLES`: the registry's entry
/// names those six and no other). The checksums job lists exactly those, beside the sixteen downloads, and counts all twenty-two; the site's buttons and the site's list of files stay the
/// sixteen downloads (the bundles are no download of the site).
#[test]
fn the_checksums_job_lists_the_six_bundles_beside_the_sixteen_downloads_and_counts_twenty_two() {
    let wanted: BTreeSet<String> = xtask::mcp::BUNDLES.iter().map(xtask::mcp::bundle_pattern).collect();
    let listed: BTreeSet<String> = listed_bundles().into_iter().collect();
    assert_eq!(listed.len(), 6);
    assert_eq!(listed, wanted, "the bundles the workflow lists and the bundles of xtask/src/mcp.rs");
    assert_eq!(listed_files().len(), 16, "the downloads: the bundles are not among them");
    assert!(listed_files().iter().all(|name| !name.contains("mcp")), "a bundle in the downloads' list is a button the site would have to show");
    assert!(assets().iter().all(|(_, name)| !name.contains("mcp")), "site.json names no bundle");
    let checksums = job("checksums");
    assert!(checksums.contains("= 22 ]"), "the line count of SHA256SUMS.txt is twenty-two: the sixteen downloads and the six bundles");
    assert!(checksums.contains("twenty-two files"), "{checksums}");
    assert!(
        !checksums.contains("fourteen") && !checksums.contains("twenty files") && !checksums.contains("= 14 ]") && !checksums.contains("= 20 ]"),
        "no count of fourteen or twenty is left in the checksums job"
    );
}

/// Every build job makes the bundles of its own CPUs after its downloads, runs what is in each (`packaging/mcp-bundle-check.sh`), and uploads them with the downloads of that job.
#[test]
fn each_build_job_makes_checks_and_uploads_the_bundles_of_its_cpu() {
    let linux = job("linux");
    assert!(linux.contains("bash packaging/mcp-bundle.sh linux \"$VERSION\" dist \"target/$TARGET/release\""));
    assert!(linux.contains("bash packaging/mcp-bundle-check.sh \"dist/diacraft-mcp-$VERSION-linux-$FILE_ARCH.mcpb\" $expected"));
    assert!(
        linux.split("gh release upload").nth(1).unwrap().contains("\"dist/diacraft-mcp-$VERSION-linux-$FILE_ARCH.mcpb\""),
        "the Linux bundle is uploaded"
    );
    let windows = job("windows");
    assert!(windows.contains("bash packaging/mcp-bundle.sh windows \"$VERSION\" dist \"target/$TARGET/release\""));
    assert!(windows.contains("bash packaging/mcp-bundle-check.sh \"dist/diacraft-mcp-$VERSION-windows-$FILE_ARCH.mcpb\" $expected"));
    assert!(windows.split("gh release upload").nth(1).unwrap().contains("\"dist/diacraft-mcp-$VERSION-windows-$FILE_ARCH.mcpb\""));
    let mac = job("macos");
    for cpu_folder in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert!(mac.contains(&format!("bash packaging/mcp-bundle.sh macos \"$VERSION\" dist target/{cpu_folder}/release")), "{cpu_folder}");
    }
    // The Apple silicon bundle runs here; the Intel one under Rosetta where it is there, and its files are looked at where it is not, and the step says which.
    assert!(mac.contains("bash packaging/mcp-bundle-check.sh \"dist/diacraft-mcp-$VERSION-macos-arm64.mcpb\" $expected"));
    assert!(mac.contains("RUN_WITH=\"arch -x86_64\" bash packaging/mcp-bundle-check.sh \"dist/diacraft-mcp-$VERSION-macos-x86_64.mcpb\" $expected"));
    assert!(mac.contains("bash packaging/mcp-bundle-check.sh --no-run \"dist/diacraft-mcp-$VERSION-macos-x86_64.mcpb\" $expected"));
    let upload = mac.split("gh release upload").nth(1).unwrap();
    for cpu in ["arm64", "x86_64"] {
        assert!(upload.contains(&format!("\"dist/diacraft-mcp-$VERSION-macos-{cpu}.mcpb\"")), "{cpu}");
    }
    // The bundle's program is the one of the download: it is made from the same folder of programs, after the signing hooks (a signed program is bundled signed).
    for (name, text) in [("linux", &linux), ("windows", &windows), ("macos", &mac)] {
        let make = text.find("packaging/mcp-bundle.sh").unwrap_or_else(|| panic!("{name} makes no bundle"));
        assert!(text[..make].contains("packaging/package.sh"), "{name}: the bundle is made after the download");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the .rpm

/// The Linux job makes an `.rpm` for each CPU after the `.deb`, in a container of the latest Fedora (`packaging/linux/rpm-in-fedora.sh`: the rpmbuild, its macros and its checks are
/// Fedora's own), installs it there with dnf, runs the programs and removes it, before anything is uploaded; and uploads it with the other Linux files of its CPU.
#[test]
fn the_linux_job_makes_installs_checks_and_uploads_an_rpm_for_each_cpu_in_a_fedora_container() {
    let linux = job("linux");
    // The image is Fedora's own, pinned by the digest of its index (one digest for both CPUs), named once for both jobs: a tag moves, and a release is made in the image it was tried in.
    let image: Vec<&str> = linux.lines().filter_map(|l| l.trim().strip_prefix("FEDORA_IMAGE: ")).collect();
    assert_eq!(image.len(), 1, "{image:?}");
    let digest = image[0].strip_prefix("docker.io/library/fedora@sha256:").expect("the image is the official fedora image, by digest");
    assert!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{digest}");
    // The script has no tag of its own to fall back on: a run that lost the variable stops.
    let script = read("packaging/linux/rpm-in-fedora.sh");
    assert!(
        script.contains("image=${FEDORA_IMAGE:-}") && !script.contains("${FEDORA_IMAGE:-docker"),
        "rpm-in-fedora.sh takes the image from the variable and from nothing else"
    );
    let make =
        linux.find("bash packaging/linux/rpm-in-fedora.sh make \"$VERSION\" dist \"target/$TARGET/release\"").expect("the job makes the package");
    let check = linux
        .find("bash packaging/linux/rpm-in-fedora.sh check \"dist/diacraft-$VERSION-linux-$FILE_ARCH.rpm\" $expected")
        .expect("the job installs the package it made and checks what it did");
    let deb = linux.find("packaging/linux/make-deb.sh").expect("the .deb is made");
    let upload = linux.find("gh release upload").expect("the files are uploaded");
    assert!(deb < make && make < check && check < upload, "the .rpm is made after the .deb, checked before it is uploaded");
    assert!(linux[upload..].contains("\"dist/diacraft-$VERSION-linux-$FILE_ARCH.rpm\""), "the .rpm is uploaded with the other files of its CPU");
    // The tag is expected only for a release: a run by hand on a branch makes up its own.
    let step = &linux[linux.find("- name: Smoke test the .rpm").expect("the step")..];
    assert!(step.contains("if [ \"$PUBLISH\" = true ]; then expected=$NAME; else expected=\"\"; fi"), "the check is given the tag of a release only");
    // Each CPU's package is made on a runner of its own CPU (the container is of the runner's), so none is cross made.
    assert!(linux.contains("runs-on: ${{ matrix.runner }}") && linux.contains("runner: ubuntu-22.04-arm"));
    // The sums job expects both, and the notes name them.
    let sums = job("checksums");
    for cpu in ["x86_64", "aarch64"] {
        assert!(sums.contains(&format!("\"diacraft-$VERSION-linux-{cpu}.rpm\"")), "the checksums job does not list the {cpu} .rpm");
        assert!(job("draft").contains(&format!("diacraft-$VERSION-linux-{cpu}.rpm (Fedora)")), "the notes do not name the {cpu} .rpm");
    }
}

/// The notes of the draft name the bundles, and say what they are.
#[test]
fn the_draft_notes_name_the_six_bundles_and_say_what_they_are_for() {
    let workflow = read(".github/workflows/release.yml");
    for bundle in &xtask::mcp::BUNDLES {
        let name = format!("diacraft-mcp-$VERSION-{}-{}.mcpb", bundle.os, bundle.cpu);
        assert!(workflow.contains(&name), "{name}");
    }
    let draft = job("draft");
    assert!(draft.contains("MCP bundle") && draft.contains(".mcpb files") && !draft.contains('`'), "{draft}");
}
