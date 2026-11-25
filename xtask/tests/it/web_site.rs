//! The web app on the website: the files that have to agree for `cargo xtask site --web` and the release workflow to build and serve it. Nothing
//! here builds the app; these are the numbers and names that two files each hold, read from both.

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The value of `NAME: value` in the `env:` lines of the release workflow's job (the first such line).
fn workflow_value(workflow: &str, name: &str) -> String {
    let prefix = format!("{name}: ");
    let line = workflow.lines().map(str::trim).find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("release.yml has no `{name}:` line"));
    line[prefix.len()..].trim().to_owned()
}

fn lock_version(package: &str) -> String {
    let lock: toml::Table = read("Cargo.lock").parse().unwrap();
    let versions: Vec<&str> = lock["package"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(package))
        .filter_map(|p| p.get("version").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(versions.len(), 1, "{package} is in Cargo.lock at {versions:?}");
    versions[0].to_owned()
}

/// Trunk uses a `wasm-bindgen` program only at the version of the `wasm-bindgen` crate the lock holds (the two must match or the glue and the module disagree),
/// and the release workflow installs one by version and checksum. A change to the lock moves the version here, and the checksum with it.
#[test]
fn the_release_workflow_installs_the_wasm_bindgen_the_lock_holds() {
    let workflow = read(".github/workflows/release.yml");
    assert_eq!(workflow_value(&workflow, "WASM_BINDGEN_VERSION"), lock_version("wasm-bindgen"), "move the version and WASM_BINDGEN_SHA256 together");
    for name in ["TRUNK_SHA256", "WASM_BINDGEN_SHA256", "BINARYEN_SHA256"] {
        let sum = workflow_value(&workflow, name);
        assert!(sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{name} is `{sum}`");
    }
}

/// `wasm-opt` is binaryen's, and Trunk uses one on the PATH only at exactly the version `Trunk.toml` names: the workflow installs that version.
#[test]
fn the_release_workflow_installs_the_binaryen_that_trunk_toml_names() {
    let workflow = read(".github/workflows/release.yml");
    let trunk: toml::Table = read("apps/diacraft-web/Trunk.toml").parse().unwrap();
    assert_eq!(trunk["tools"]["wasm_opt"].as_str(), Some(workflow_value(&workflow, "BINARYEN_VERSION").as_str()));
    // And the version of trunk, which the page's `data-` attributes and `[tools]` table are written for.
    assert_eq!(workflow_value(&workflow, "TRUNK_VERSION"), "0.22.2");
}

/// The release workflow makes the two sites with the command and keeps each as an artifact; the steps that deploy them are `pages_deploy.rs`'s.
#[test]
fn the_release_workflow_has_a_site_job_that_makes_the_two_sites_and_keeps_each_as_an_artifact() {
    let workflow = read(".github/workflows/release.yml");
    for needed in [
        "cargo xtask site --web --out site-dist --app-out app-dist",
        "actions/upload-artifact@",
        "path: site-dist",
        "path: app-dist",
        "wasm32-unknown-unknown",
    ] {
        assert!(workflow.contains(needed), "release.yml does not say `{needed}`");
    }
    // Every action is pinned by a full commit SHA.
    for line in workflow.lines().filter(|l| l.trim_start().starts_with("uses:") || l.trim_start().starts_with("- uses:")) {
        let pinned = line.split('@').nth(1).is_some_and(|r| r.split_whitespace().next().is_some_and(|sha| sha.len() == 40));
        assert!(pinned, "an action that is not pinned by a commit: {line}");
    }
}

/// The two sites have a `_headers` each: the pages' is strict, with nothing of the app's, and the app's is the app's own policy for every path.
#[test]
fn the_pages_and_the_app_have_a_headers_file_each_and_only_the_apps_has_the_wasm_policy() {
    let pages = read("site/_headers");
    let pages_rules: String = pages.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    // The lines that are not comments: the comment above the rules names the keywords it explains.
    assert!(!pages_rules.contains("wasm-unsafe-eval") && !pages_rules.contains("unsafe-inline"), "the pages' policy is as strict as it was");
    assert!(pages_rules.contains("script-src 'self';") && pages_rules.contains("connect-src 'self' https://api.github.com;"), "{pages_rules}");
    assert!(!pages_rules.contains("/app"), "the pages' file has no rule for the app: it is a site of its own\n{pages_rules}");
    assert!(!pages_rules.contains("! Content-Security-Policy"), "no policy of the pages is taken away");
    let app = read("apps/diacraft-web/_headers");
    let app_rules: String = app.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    assert!(app_rules.starts_with("/*\n  Content-Security-Policy:"), "the app's policy is for every path of its site: {app_rules}");
    assert!(app_rules.contains("script-src 'self' 'wasm-unsafe-eval'") && app_rules.contains("connect-src 'self'"), "{app_rules}");
    assert!(!app_rules.to_ascii_lowercase().contains("cross-origin"), "the app uses no threads and no shared memory");
    assert!(!app_rules.contains("! "), "nothing of the pages' policy is there to take away");
    for route in ["/parts/*", "/assets/*", "/diacraft-web-*.js", "/page-*.css", "/diacraft-icon-256-*.png"] {
        assert!(app_rules.contains(&format!("\n{route}\n  Cache-Control: public, max-age=31536000, immutable")), "{route}");
    }
    // The pages' own settings: the two addresses are two sites.
    let settings: serde_json::Value = serde_json::from_str(&read("site/site.json")).unwrap();
    assert_eq!(settings["domain"], "https://diacraft.dev");
    assert_eq!(settings["app_url"], "https://diacraft.app");
}

/// The wasm lock-step: the web profile of the workspace is the one the page asks trunk for, and it is built for size.
#[test]
fn the_page_asks_trunk_for_the_size_profile_the_workspace_defines() {
    let manifest: toml::Table = read("Cargo.toml").parse().unwrap();
    let profile = manifest["profile"]["web"].as_table().expect("[profile.web]");
    assert_eq!(profile["inherits"].as_str(), Some("release"));
    assert_eq!(profile["opt-level"].as_str(), Some("z"));
    assert_eq!(profile["lto"].as_str(), Some("fat"));
    let page = read("apps/diacraft-web/index.html");
    assert!(
        page.contains("data-cargo-profile-release=\"web\"") && page.contains("data-wasm-opt=\"z\""),
        "index.html does not ask for the web profile and wasm-opt -Oz"
    );
    // The desktop programs' own release profile is not the web's.
    assert_eq!(manifest["profile"]["release"]["lto"].as_str(), Some("thin"));
}

/// The anchors of a page as (the start tag, the text up to the closing tag), in order.
fn anchors(html: &str) -> Vec<(String, String)> {
    html.split("<a ")
        .skip(1)
        .filter_map(|piece| {
            let (tag, rest) = piece.split_once('>')?;
            let (text, _) = rest.split_once("</a>")?;
            Some((tag.to_owned(), text.to_owned()))
        })
        .collect()
}

/// The `href` of an anchor's start tag.
fn href(tag: &str) -> &str {
    let start = tag.find("href=\"").map_or(tag.len(), |i| i + 6);
    tag.get(start..).and_then(|rest| rest.split('"').next()).unwrap_or("")
}

/// Every page that has "Open DiaCraft in your browser" (owner, 2026-11-23: "when I click it on the home page I want to get to the app immediately") links the app itself, at the address
/// of the site of its own (`app_url` of `site.json`: `https://diacraft.app/`), with the mark `data-app` that makes `cargo xtask site --write` keep it there, and never the page about
/// it; the page with the details, `/browser.html`, is linked from the home page's Features section and from every page's footer; and a label that says "Open DiaCraft in your browser" is
/// on a link to the app.
#[test]
fn open_diacraft_in_your_browser_goes_straight_to_the_app_and_the_details_page_is_linked_from_the_features_and_the_footer() {
    let pages = [
        "index.html",
        "browser.html",
        "privacy.html",
        "changelog.html",
        "404.html",
        "vsdx-editor.html",
        "flowcharts.html",
        "architecture-diagrams.html",
        "guides/index.html",
        "guides/agents.html",
        "guides/command-line.html",
        "guides/file-formats.html",
        "guides/getting-started.html",
        "guides/native-format.html",
    ];
    let settings: serde_json::Value = serde_json::from_str(&read("site/site.json")).unwrap();
    let app = format!("{}/", settings["app_url"].as_str().unwrap().trim_end_matches('/'));
    for page in pages {
        let html = read(&format!("site/{page}"));
        for (tag, text) in anchors(&html) {
            if text.contains("Open DiaCraft in your browser") || tag.contains("aria-label=\"Open DiaCraft in your browser\"") {
                assert_eq!(href(&tag), app, "{page}: a link that says Open DiaCraft in your browser goes to {}", href(&tag));
                assert!(tag.starts_with("data-app=\"\" href=\""), "{page}: the link to the app is marked data-app, and href follows it: {tag}");
            }
            // No link to the app is without the mark, and none goes to a folder of the pages: the app is a site of its own.
            assert!(!href(&tag).starts_with(&app) || tag.starts_with("data-app=\"\" "), "{page}: a link to the app without data-app: {tag}");
            assert!(!href(&tag).starts_with("/app"), "{page}: a link to a folder of the pages that is no more: {tag}");
        }
        // The footer's link to the details stays on every page.
        let footer = html.split("<footer").nth(1).unwrap_or_else(|| panic!("{page} has no footer"));
        assert!(
            anchors(footer).iter().any(|(tag, text)| href(tag) == "/browser.html" && text == "In your browser"),
            "{page}: the footer does not link the browser page as In your browser"
        );
    }
    let home = read("site/index.html");
    let open: Vec<String> =
        anchors(&home).into_iter().filter(|(_, text)| text.contains("Open DiaCraft in your browser")).map(|(tag, _)| href(&tag).to_owned()).collect();
    assert_eq!(
        open,
        [app.as_str(), app.as_str(), app.as_str()],
        "the hero link, the card's button and the answer of the question about trying it each go to the app"
    );
    assert!(home.matches("Open DiaCraft in your browser").count() >= 3, "the hero link, the card's heading label and its button");
    // The Features section links the page with the details, and the page is the only thing that does so besides the footer, the answer's pointer and that section.
    let features =
        home.split("id=\"features\"").nth(1).and_then(|rest| rest.split("</section>").next()).expect("the home page has a Features section");
    assert!(anchors(features).iter().any(|(tag, _)| href(tag) == "/browser.html"), "the Features section does not link /browser.html");
    let page = read("site/browser.html");
    assert!(page.contains(&format!("href=\"{app}\"")) && page.contains("Open DiaCraft in your browser"));
    // The address in the picture of the browser's window is the app's own host, which `--write` keeps.
    assert!(page.contains("data-app-host=\"\">diacraft.app</div>"), "the frame's address is the app's host");
    // What the web build lacks is said in plain words.
    for word in [
        "Files.",
        "Export and print",
        "Fonts",
        "Automation",
        "WebGL 2",
        "makes no network request",
        "THIRD-PARTY-LICENSES-WEB.txt",
        "drop a file on the page",
        "Twenty thousand shapes",
        "In a window of its own",
        "Open in its own window",
        "Add to Dock",
        "Firefox has no windows of their own for web apps on Linux or macOS",
        "Full Screen",
    ] {
        assert!(page.contains(word), "browser.html does not say `{word}`");
    }
    // The pages about the privacy and the changes do not link the app.
    for rel in ["site/privacy.html", "site/changelog.html"] {
        assert!(!read(rel).contains("data-app"), "{rel} links the app directly");
    }
    assert!(read("site/privacy.html").contains("When you use DiaCraft in your browser"));
}

/// The owner's request (2026-11-25): the trademark paragraph of the footer runs the full width of the rule above it, on every page that has the footer, and not in a narrow column. Its small type
/// and its colour stay. The width came from a right padding of `calc(100% - 90ch)`; nothing may hold the paragraph to a measure again.
#[test]
fn the_footers_trademark_paragraph_runs_the_full_width_of_the_rule_above_it_on_every_page() {
    let css = read("site/styles.css");
    let rule = css.lines().find(|l| l.starts_with(".foot-inner .legal {")).expect("the footer paragraph has a rule");
    for held in ["max-width", "ch)", "ch;", "measure", "calc("] {
        assert!(!rule.contains(held), "the paragraph is held to a width by `{held}`: {rule}");
    }
    assert!(rule.contains("grid-column: 1 / -1") && rule.contains("border-top"), "it spans the footer's grid under the rule: {rule}");
    assert!(rule.contains("font-size: .8rem") && rule.contains("color: var(--ink-muted)"), "the type and the colour stay: {rule}");
    // No other rule narrows it, and every page has the one paragraph, in the footer.
    assert!(!css.lines().any(|l| l.contains(".legal") && !l.starts_with(".foot-inner .legal {")), "no other rule for the paragraph");
    let mut pages = vec!["site/index.html".to_owned(), "site/404.html".to_owned()];
    for dir in ["site", "site/guides"] {
        for entry in std::fs::read_dir(xtask::root().join(dir)).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.ends_with(".html") {
                pages.push(format!("{dir}/{name}"));
            }
        }
    }
    pages.sort();
    pages.dedup();
    assert_eq!(pages.len(), 14, "the pages, the guides, their index and 404: {pages:?}");
    for page in &pages {
        let text = read(page);
        assert_eq!(text.matches("<p class=\"legal\">").count(), 1, "{page}");
        let (_, footer) = text.split_once("<footer").unwrap_or_else(|| panic!("{page} has a footer"));
        assert!(footer.contains("<p class=\"legal\">"), "{page}: the paragraph is in the footer");
    }
}
