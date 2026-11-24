//! What the web build loads and how: the loader script, the page's progress elements, the table of assets the program fetches, and the window the shell makes when it is
//! given a host for them. The loader runs in a browser and is not run here (the browser runs of the task did that); these tests hold what can be held without one: that it is the
//! script the page needs, that it and the build tool agree on the numbers they share, and that it reaches nothing but its own folder.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use diacraft_browser::assets::{self, ASSET_DIR};
use diacraft_browser::page::new_app_in_browser;
use diacraft_ui_egui::{Asset, AssetHost, Delivered, FileHost, Want};

use crate::read;

fn xtask_parts() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/src/site/parts.rs")).expect("the build tool is in the repository")
}

/// The loader and the build tool say the same limits: a part the tool makes is one the loader takes.
#[test]
fn the_loader_and_the_build_tool_agree_on_the_limits() {
    let loader = read("loader.js");
    let tool = xtask_parts();
    for (loader_line, tool_line) in [
        ("const MAX_PART_BYTES = 8 * 1024 * 1024;", "pub(super) const PART_LIMIT: u64 = 8 * 1024 * 1024;"),
        ("const MAX_PARTS = 16;", "pub(super) const MAX_PARTS: usize = 16;"),
        ("const MAX_MODULE_BYTES = 64 * 1024 * 1024;", "pub(super) const MAX_MODULE: u64 = 64 * 1024 * 1024;"),
        ("const MAX_MANIFEST_BYTES = 64 * 1024;", "pub(super) const MAX_MANIFEST: u64 = 64 * 1024;"),
    ] {
        assert!(loader.contains(loader_line), "loader.js has no `{loader_line}`");
        assert!(tool.contains(tool_line), "xtask/src/site/parts.rs has no `{tool_line}`");
    }
    // The program reads the same manifest: the same bound on its text, and the same folder for the assets.
    assert_eq!(assets::MAX_MANIFEST_BYTES, 64 * 1024);
    assert_eq!(ASSET_DIR, "assets/");
    assert!(tool.contains("pub(super) const ASSETS_DIR: &str = \"assets\";"));
    assert!(loader.contains("manifest.json") && tool.contains("pub(super) const MANIFEST: &str = \"manifest.json\";"));
    // The property the loader leaves the manifest in is the one the program reads it from (`web_assets.rs`).
    assert!(loader.contains("window.diacraftManifest = text;"));
    assert!(read("src/web_assets.rs").contains("const MANIFEST_PROPERTY: &str = \"diacraftManifest\";"));
}

/// The build tool's table of assets is the window's: the same keys, and for each the same most bytes, so that a file the tool puts in the site is a file the window takes.
#[test]
fn the_build_tools_assets_are_the_windows() {
    let tool = xtask_parts();
    let table: Vec<&str> = tool.lines().filter(|line| line.trim_start().starts_with("(\"")).collect();
    assert_eq!(table.len(), Asset::ALL.len(), "{table:?}");
    for asset in Asset::ALL {
        let line = table
            .iter()
            .find(|line| line.trim_start().starts_with(&format!("(\"{}\",", asset.key())))
            .unwrap_or_else(|| panic!("no row for {asset:?}"));
        let megabytes = asset.limit() >> 20;
        assert_eq!(asset.limit(), megabytes << 20, "a limit is whole mebibytes");
        assert!(line.contains(&format!("{megabytes} << 20)")), "{asset:?}: `{line}` does not hold the limit {}", asset.limit());
    }
}

/// Every element the loader looks up is in the page, and the page has nothing for it that the loader does not name.
#[test]
fn the_loader_finds_every_element_it_updates_in_the_page() {
    let loader = read("loader.js");
    let html = read("index.html");
    let mut ids = Vec::new();
    for (at, _) in loader.match_indices("getElementById('") {
        let rest = &loader[at + "getElementById('".len()..];
        ids.push(rest.split('\'').next().unwrap().to_owned());
    }
    assert_eq!(ids, ["diacraft_status", "diacraft_progress", "diacraft_again"]);
    for id in &ids {
        assert_eq!(html.matches(&format!("id=\"{id}\"")).count(), 1, "the page has one #{id}");
    }
    // The progress bar is a `<progress>` (what a screen reader reads as one), named by the words over it, and the button starts hidden.
    assert!(html.contains("<progress id=\"diacraft_progress\"") && html.contains("aria-labelledby=\"diacraft_status\""));
    assert!(html.contains("<button id=\"diacraft_again\" type=\"button\" hidden>"));
    assert!(html.contains("role=\"status\""));
    // They are inside the loading element, which the program takes away when it has drawn its first frame.
    let loading = html.find("id=\"diacraft_loading\"").unwrap();
    for id in &ids {
        assert!(html.find(&format!("id=\"{id}\"")).unwrap() > loading);
    }
    // Their styles are in the page's stylesheet, light and dark.
    let css = read("page.css");
    for needle in [
        "#diacraft_loading progress {",
        "::-webkit-progress-value",
        "::-moz-progress-bar",
        "#diacraft_loading button {",
        "prefers-color-scheme: light",
    ] {
        assert!(css.contains(needle), "page.css has no `{needle}`");
    }
}

/// The loader reaches nothing but its own folder: no address in it, no other way to the network than `fetch` of a file the manifest names, no evaluation of text, and nothing written
/// to the page that is not text.
#[test]
fn the_loader_reaches_nothing_but_its_own_folder_and_evaluates_nothing() {
    let loader = read("loader.js");
    for banned in [
        "://",
        "XMLHttpRequest",
        "WebSocket",
        "sendBeacon",
        "EventSource",
        "importScripts",
        "new Function",
        "eval(",
        "RTCPeerConnection",
        "WebTransport",
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "caches.",
        "serviceWorker",
        "document.cookie",
    ] {
        assert!(!loader.contains(banned), "loader.js contains `{banned}`");
    }
    // Every fetch is of the manifest or of a name made from it, relative to the page; every import is of the manifest's glue.
    for (at, _) in loader.match_indices("fetch(").filter(|(at, _)| !loader[..*at].ends_with(|c: char| c.is_ascii_alphanumeric())) {
        let call = &loader[at..];
        let argument = call["fetch(".len()..].split([',', ')']).next().unwrap().trim();
        assert!(["url", "'./' + entry.file"].contains(&argument), "loader.js fetches `{argument}`");
    }
    for (at, _) in loader.match_indices("download(").filter(|(at, _)| !loader[..*at].ends_with(|c: char| c.is_ascii_alphanumeric())) {
        let argument = loader[at + "download(".len()..].split([',', ')']).next().unwrap().trim();
        assert!(["url", "'./manifest.json'", "'./' + part.info.file"].contains(&argument), "loader.js downloads `{argument}`");
    }
    assert_eq!(loader.matches("import(").count(), 1);
    assert!(loader.contains("const glueUrl = new URL(manifest.glue, location.href).href;") && loader.contains("import(glueUrl)"));
    // It is a module that is written for the page's policy: no inline handler is set, and the page's text is set as text.
    assert!(!loader.contains("onclick") && !loader.contains("setAttribute('on"));
    assert!(loader.contains("status.textContent = text;"));
    // The words it says on failure name what to do.
    assert!(loader.contains("DiaCraft could not start: ") && read("index.html").contains(">Try again<"));
    // The text it reads is bounded, and the parts are held to length and digest.
    assert!(
        loader.contains("crypto.subtle.digest('SHA-256'")
            && loader.contains("is shorter than the manifest says")
            && loader.contains("is longer than the manifest says")
    );
    assert!(loader.contains("STALL_MS"), "a connection that stops is given up on");
    // It hands the program a stream the browser compiles from as it comes, and the compile starts with the glue's fetch and not after it.
    assert!(
        loader.contains("'Content-Type': 'application/wasm'")
            && loader.contains("WebAssembly.compileStreaming(response)")
            && loader.contains("module_or_path: module")
    );
    assert!(
        loader.find("WebAssembly.compileStreaming(response)").unwrap() < loader.find("import(glueUrl)").unwrap(),
        "the compile starts before the glue is awaited"
    );
    // The program starts only when every part has passed its check: the checks are in the same `Promise.all` as the module, before the start.
    let all = loader
        .find("Promise.all([import(glueUrl), compiled, Promise.all(parts.map((part) => part.checked))])")
        .expect("the start waits for every check");
    assert!(all < loader.find("glue.default(").unwrap());
    // It is small: the site's one script that is not generated.
    assert!(loader.lines().count() < 300, "{} lines", loader.lines().count());
}

// ------------------------------------------------------------------------------------------------------------------------------ the window

#[derive(Default)]
struct NoFiles;

impl FileHost for NoFiles {
    fn choose(&self, _: &egui::Context, _: Want) -> Receiver<Option<PathBuf>> {
        channel().1
    }

    fn deliver(&self, _: &egui::Context, _: &Path) -> Receiver<Result<Delivered, String>> {
        channel().1
    }

    fn dropped(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn refusals(&self) -> Vec<String> {
        Vec::new()
    }

    fn release(&self, _: &Path) {}
}

/// The page of a tab, as far as the window asks: no window of its own, no full screen, nothing to close.
struct InATab;

impl diacraft_ui_egui::PageHost for InATab {
    fn browser(&self) -> diacraft_ui_egui::Browser {
        diacraft_ui_egui::Browser::Chromium
    }

    fn own_window(&self) -> bool {
        false
    }

    fn can_prompt(&self) -> bool {
        false
    }

    fn prompt(&self) {}

    fn full_screen(&self) -> Option<bool> {
        None
    }

    fn enter_full_screen(&self) {}

    fn leave_full_screen(&self) {}

    fn close(&self) -> bool {
        false
    }
}

#[derive(Default)]
struct Asked(Mutex<Vec<Asset>>);

impl AssetHost for Asked {
    fn want(&self, _: &egui::Context, asset: Asset) {
        self.0.lock().unwrap().push(asset);
    }

    fn state(&self, _: Asset) -> diacraft_ui_egui::Fetch {
        diacraft_ui_egui::Fetch::Waiting
    }
}

/// The window the shell makes in the browser has its three hosts (files, assets, the page), starts with the splash waiting for its picture, and asks for the picture and nothing else at its first frame.
#[test]
fn the_window_the_browser_gets_has_its_three_hosts_and_asks_for_the_splash_picture_first() {
    let asked = Arc::new(Asked::default());
    let host = asked.clone();
    let mut window = egui_kittest::Harness::builder()
        .with_size([900.0, 600.0])
        .build_eframe(move |_| new_app_in_browser(Arc::new(NoFiles), host, Arc::new(InATab)));
    window.step();
    window.step();
    assert!(window.state().has_file_host() && window.state().has_asset_host() && window.state().in_browser());
    assert_eq!(window.state().splash_kind(), Some(diacraft_ui_egui::splash::SplashKind::Start));
    let asked = asked.0.lock().unwrap().clone();
    assert!(
        !asked.is_empty() && asked.iter().all(|a| *a == Asset::Splash),
        "{asked:?}: the first frame asks for the splash picture, and for no other"
    );
}

/// The page's loading element, the words a person reads while the program comes, and the module's name for it: kept in step with the program's own constant.
#[test]
fn the_loading_words_are_the_programs_and_the_loader_s() {
    let html = read("index.html");
    assert!(html.contains("<p id=\"diacraft_status\">Loading DiaCraft&hellip;</p>"));
    assert!(read("loader.js").contains("say('Loading DiaCraft\u{2026}');"));
}
