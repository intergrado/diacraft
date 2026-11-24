//! The web app manifest and the page's offer of a window of its own: what lets Chrome and Edge open the app (the root of its own site, `/`) in a window with no tabs, where the keys a tab keeps (Ctrl+N, Ctrl+W)
//! go to the page. The build tool (`cargo xtask site --web`, `xtask/src/site/web_manifest.rs`) holds the built folder to the same; these tests hold the sources, so that a change that the
//! tool would refuse is seen without a build.

use diacraft_ui_egui::theme::Tokens;
use egui::Color32;
use serde_json::Value;

use crate::{app_path, read};

fn manifest() -> Value {
    serde_json::from_str(&read("diacraft.webmanifest")).expect("the manifest is JSON")
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// The manifest says what makes a page installable as a window of its own, and no more.
#[test]
fn the_manifest_names_the_app_its_scope_and_a_window_with_no_tabs() {
    let m = manifest();
    assert_eq!(m["name"], "DiaCraft");
    assert_eq!(m["short_name"], "DiaCraft");
    for key in ["id", "start_url", "scope"] {
        assert_eq!(m[key], "/", "{key}");
    }
    assert_eq!(m["display"], "standalone", "a window with no tabs is where the browser passes its keys to the page");
    // Nothing that asks the browser for more: no worker, no handlers, no shortcuts, no related apps.
    let keys: Vec<&str> = m.as_object().unwrap().keys().map(String::as_str).collect();
    for key in &keys {
        assert!(
            ["id", "name", "short_name", "description", "start_url", "scope", "display", "background_color", "theme_color", "icons"].contains(key),
            "the manifest has `{key}`"
        );
    }
}

/// The two colours are the theme tokens' (the page's own are, `html.rs`): the dark canvas behind the loading page, and the dark top bar.
#[test]
fn the_manifests_colours_are_theme_tokens() {
    let m = manifest();
    assert_eq!(m["background_color"], hex(Tokens::dark().canvas_fill));
    assert_eq!(m["theme_color"], hex(Tokens::dark().top_bar_fill));
}

/// The icons are PNG files of the site, copied beside the page by trunk, of the sizes they say, with the 192 and the 512 an installable page needs.
#[test]
fn the_manifests_icons_are_the_sites_pictures_of_the_sizes_they_say() {
    let m = manifest();
    let html = read("index.html");
    let icons = m["icons"].as_array().unwrap();
    let mut sizes = Vec::new();
    for icon in icons {
        let src = icon["src"].as_str().unwrap();
        assert_eq!(icon["type"], "image/png");
        let side: u32 = icon["sizes"].as_str().unwrap().split('x').next().unwrap().parse().unwrap();
        let file = app_path(&format!("../../site/assets/{src}"));
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!((number(16), number(20)), (side, side), "{src}");
        assert!(
            html.contains(&format!("<link data-trunk rel=\"copy-file\" href=\"../../site/assets/{src}\" />")),
            "trunk copies {src} beside the page"
        );
        sizes.push(side);
    }
    for needed in [192, 512] {
        assert!(sizes.contains(&needed), "an icon of {needed} by {needed}");
    }
}

/// The page links its manifest, trunk copies it as it is, and its name is not the module's list of parts (`manifest.json`).
#[test]
fn the_page_links_the_manifest_and_trunk_copies_it() {
    let html = read("index.html");
    assert!(html.contains("<link rel=\"manifest\" href=\"diacraft.webmanifest\" />"));
    assert!(html.contains("<link data-trunk rel=\"copy-file\" href=\"diacraft.webmanifest\" />"));
    assert_ne!("diacraft.webmanifest", "manifest.json");
    assert!(app_path("diacraft.webmanifest").is_file());
}

/// The app has no service worker, and none is needed: Chrome 158 installs a page with this manifest and sends its install offer (measured with the DevTools protocol's `Page.getInstallabilityErrors`:
/// no error). Where a browser asks for one the owner decides, and nothing here adds it.
#[test]
fn the_app_has_no_service_worker() {
    for file in ["index.html", "loader.js", "diacraft.webmanifest", "src/web.rs", "src/web_page.rs", "src/web_files.rs", "src/web_assets.rs"] {
        let text = read(file).to_lowercase();
        for banned in ["serviceworker", "service_worker", "service worker"] {
            // Prose that says there is none is allowed in the comments of the page, and nowhere else.
            let allowed = file == "index.html" && banned == "service worker";
            assert!(allowed || !text.contains(banned), "{file} mentions `{banned}`");
        }
    }
}

/// The browser sends its install offer once, early, before the program has started: the loader keeps it for the window, and lets it go when the app is installed.
#[test]
fn the_loader_keeps_the_browsers_install_offer_for_the_program() {
    let loader = read("loader.js");
    assert!(loader.contains("window.addEventListener('beforeinstallprompt', (event) => {"));
    assert!(loader.contains("event.preventDefault();\n  window.diacraftInstallOffer = event;"));
    assert!(loader.contains("window.addEventListener('appinstalled', () => {\n  window.diacraftInstallOffer = null;"));
    // The program reads and takes what the loader kept, by the same name.
    let page = read("src/web_page.rs");
    assert_eq!(page.matches("\"diacraftInstallOffer\"").count(), 3, "the program reads, and takes, the property the loader sets");
    // Listeners come before the first `await`: the offer can arrive while the manifest is still being fetched.
    let listener = loader.find("beforeinstallprompt").unwrap();
    assert!(listener < loader.find("await ").unwrap(), "the listener is set before the first await");
}

/// The program takes the keys a browser would act on in the capture phase of the window, which runs before eframe's own handler on the canvas, and stops nothing.
#[test]
fn the_page_takes_keys_before_the_web_runner_and_stops_nothing() {
    let page = read("src/web_page.rs");
    assert!(page.contains("listen(&window, \"keydown\", true,"), "the handler is on the window, in the capture phase");
    assert!(page.contains("event.prevent_default();"));
    for banned in ["stop_propagation", "stop_immediate_propagation"] {
        assert!(!page.contains(banned), "the key must reach eframe's handler: `{banned}`");
    }
}
