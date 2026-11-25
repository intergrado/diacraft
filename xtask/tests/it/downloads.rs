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

/// The list the release workflow's last job checks the release against: the `"diacraft-$NAME-...` words of its `expected=$(printf` command, with `{tag}` for the tag.
fn listed_files() -> Vec<String> {
    let workflow = read(".github/workflows/release.yml");
    let start = workflow.find("expected=$(printf").expect("the checksums job has a list of the files");
    let list = &workflow[start..workflow[start..].find("| sort").map(|i| start + i).expect("the list is sorted")];
    list.split('"').filter(|word| word.starts_with("diacraft-$NAME-")).map(|word| word.replace("$NAME", "{tag}")).collect()
}

#[test]
fn the_site_asks_for_the_files_the_release_workflow_lists_and_checks() {
    let assets = assets();
    assert_eq!(assets.len(), 14, "six Linux files, four Windows files and four macOS files: {assets:?}");
    let workflow = read(".github/workflows/release.yml");
    let listed: BTreeSet<String> = listed_files().into_iter().collect();
    let named: BTreeSet<String> = assets.iter().map(|(_, name)| name.clone()).collect();
    assert_eq!(listed, named, "the files the checksums job expects and the files the site asks for are the same");
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
        // There is no build for both CPUs at once (the owner, 2026-11-25: no universal macOS download): each download is for one CPU.
        assert!(["x86_64", "arm64"].contains(&cpu), "{id}");
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
    let version = xtask::version::read(&read("Cargo.toml")).unwrap();
    let repository = serde_json::from_str::<serde_json::Value>(&read("site/site.json")).unwrap()["repository"].as_str().unwrap().to_owned();
    let in_the_release: BTreeSet<String> = listed_files().iter().map(|name| name.replace("{tag}", &format!("v{version}"))).collect();
    let ids: Vec<&str> = links.iter().map(|(tag, _)| attribute(tag, "data-download")).collect();
    let unique: BTreeSet<&str> = ids.iter().copied().collect();
    // A download is linked once: the main file of a card is the button and is not repeated in its CPU's row (the owner, 2026-11-25: "like we have for Linux and Windows").
    assert_eq!(ids.len(), unique.len(), "a download is linked once: {ids:?}");
    assert_eq!(unique, assets.iter().map(|(id, _)| id.as_str()).collect::<BTreeSet<_>>(), "every download has a link, and every link a download");
    for (tag, text) in &links {
        let id = attribute(tag, "data-download");
        let name = &assets.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(attribute(tag, "data-asset"), name, "{id}: the page names the file site.json names (cargo xtask site --write)");
        // With no script the link is the file of the release of the workspace's version: a click starts the download, and the file is one the release workflow lists.
        let file = name.replace("{tag}", &format!("v{version}"));
        assert_eq!(attribute(tag, "href"), format!("https://github.com/{repository}/releases/download/v{version}/{file}"), "{id}: {tag}");
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

/// The owner's request (2026-11-25): the macOS card has two rows like the Linux and Windows cards, "Apple silicon (ARM64)" and "Intel (x64)", each with its `.dmg` and its `.zip` (builds of
/// one CPU each). There is no universal download (the owner's ruling, 2026-11-25): the main button is the `.dmg` for Apple silicon, as the other cards' main button is their x64 installer, and the
/// card says that the button is for Apple silicon Macs and that Intel Macs use the Intel row; its "File" line is as short as the other cards'.
#[test]
fn the_macos_card_has_a_row_for_each_cpu_and_a_main_button_for_apple_silicon_that_says_where_intel_macs_go() {
    let page = read("site/index.html");
    let cards = cards(&page);
    let mac = cards.iter().find(|card| card.contains("<h3>macOS</h3>")).expect("the macOS card");
    let rows = mac.split("<dl class=\"cpus\"").nth(1).expect("the rows");
    let rows: Vec<&str> = rows.split("<div><dt>").skip(1).collect();
    assert_eq!(rows.len(), 2, "two rows, as the other cards' rows are one for each CPU");
    // The Apple silicon row has its archive only, since its disk image is the main button; the Intel row has both its files.
    let wanted: [(&str, &str, Vec<&str>); 2] = [
        ("Apple silicon (ARM64)", "macos-arm64", vec!["macos-arm64-zip"]),
        ("Intel (x64)", "macos-x86_64", vec!["macos-x86_64-dmg", "macos-x86_64-zip"]),
    ];
    for (row, (label, _, files)) in rows.iter().zip(wanted) {
        assert!(row.starts_with(&format!("{label}</dt>")), "{label}");
        let kinds: Vec<&str> = row.split("data-download=\"").skip(1).filter_map(|rest| rest.split('"').next()).collect();
        assert_eq!(kinds, files, "{label}: its files, but for the main button's");
    }
    // The main button is the Apple silicon disk image, and the card says whom it is for and where the Intel Macs go.
    let main = mac.split("<div class=\"get-main\">").nth(1).and_then(|rest| rest.split("</div>").next()).unwrap();
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
            "bash packaging/package.sh macos \"$NAME\" dist target/aarch64-apple-darwin/release",
            "bash packaging/package.sh macos \"$NAME\" dist target/x86_64-apple-darwin/release",
        ]
    );
    // Each is checked: the zip unpacked, the image attached read only, and the architectures that lipo shows are the ones of the download.
    for call in ["check_download arm64 arm64", "check_download x86_64 x86_64"] {
        assert!(mac.contains(call), "the check of a download is not run: {call}");
    }
    for needed in [
        "ditto -x -k \"dist/diacraft-$NAME-macos-$cpu.zip\"",
        "hdiutil attach -nobrowse -readonly -noverify",
        "dist/diacraft-$NAME-macos-$cpu.dmg",
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
            assert!(upload.contains(&format!("dist/diacraft-$NAME-macos-{cpu}.{kind}")), "{cpu} {kind} is not uploaded");
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
