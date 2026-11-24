//! `index.html` and `page.css`: the one page the app lives on. `brands` scans string literals, manifest descriptions, `docs/` and
//! `packaging/`, and not these files or `Trunk.toml`, so what must not be in them is checked here.

use diacraft_browser::page::{CANVAS_ID, LOADING_ID};
use diacraft_ui_egui::theme::Tokens;
use egui::Color32;

use crate::read;

/// Names of other apps and of the suite's other products, lowercase: none may arrive in a file copied from a
/// sibling (an app id, a link, an icon name, a title).
const OTHER_NAMES: [&str; 5] = ["artcraft", "storyteller", "discord", "vectorcraft", "getartcraft"];

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

#[test]
fn the_page_has_the_two_elements_the_program_looks_for() {
    let html = read("index.html");
    assert_eq!(html.matches(&format!("<canvas id=\"{CANVAS_ID}\"")).count(), 1, "one canvas with the id the shell looks up");
    assert_eq!(html.matches(&format!("id=\"{LOADING_ID}\"")).count(), 1, "one element for the loading text and the failure message");
    // The stylesheet names them too.
    let css = read("page.css");
    assert!(css.contains(&format!("#{CANVAS_ID} {{")) && css.contains(&format!("#{LOADING_ID} {{")));
}

/// trunk starts the binary the manifest names, and no other.
#[test]
fn trunk_is_told_to_build_the_binary_the_manifest_has() {
    let html = read("index.html");
    let manifest: toml::Table = read("Cargo.toml").parse().unwrap();
    let bins: Vec<&str> = manifest["bin"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str()).collect();
    assert_eq!(bins, ["diacraft-web"]);
    assert!(html.contains("<link data-trunk rel=\"rust\" data-bin=\"diacraft-web\""), "no `rust` link for trunk");
    assert!(html.contains("<link data-trunk rel=\"icon\" href=\"../../assets/branding/diacraft-icon-256.png\""));
    assert!(html.contains("<link data-trunk rel=\"css\" href=\"page.css\""), "the page's styles are a file trunk copies");
    assert_eq!(
        html.matches("data-trunk").count(),
        6,
        "trunk builds the app and copies its favicon, its stylesheet, the web app manifest and the manifest's two icons"
    );
}

/// The app is served under a content security policy with `script-src 'self' 'wasm-unsafe-eval'` and `style-src 'self'` (apps/diacraft-web/_headers, for
/// every path of its site): a page with an inline style element, an inline script or a style attribute would be blocked, and a canvas with no styles is a small
/// one. Everything the page needs is in `page.css`, which trunk copies beside the app.
#[test]
fn the_page_holds_nothing_inline_for_the_content_security_policy_of_the_website() {
    let html = read("index.html").to_lowercase();
    for banned in ["<style", "<script", " style=", "<base", "<iframe", "<form", "<object", "<embed"] {
        assert!(!html.contains(banned), "index.html contains `{banned}`");
    }
    let css = read("page.css").to_lowercase();
    for banned in ["@import", "url(", "@font-face", "expression(", "javascript:"] {
        assert!(!css.contains(banned), "page.css contains `{banned}`, which asks for something more than the page itself");
    }
}

/// No handwritten script and nothing fetched from another site: a script, style or font by URL, an import.
#[test]
fn the_page_loads_nothing_from_elsewhere_and_holds_no_script() {
    let html = read("index.html").to_lowercase();
    for banned in ["://", "<script", "javascript:", "@import", "@font-face", "src=", "href=\"//", " onload=", " onerror="] {
        assert!(!html.contains(banned), "index.html contains `{banned}`");
    }
    // Every `<link>` is trunk's, or would have to be listed here: the web app manifest's is the one that is not.
    assert_eq!(html.matches("<link").count(), html.matches("<link data-trunk").count() + 1);
    assert_eq!(html.matches("<link rel=\"manifest\" href=\"diacraft.webmanifest\" />").count(), 1);
}

#[test]
fn no_file_of_the_web_app_names_another_app() {
    for file in [
        "index.html",
        "page.css",
        "diacraft.webmanifest",
        "Trunk.toml",
        "Cargo.toml",
        "src/lib.rs",
        "src/main.rs",
        "src/page.rs",
        "src/web.rs",
        "src/web_page.rs",
    ] {
        let text = read(file).to_lowercase();
        for name in OTHER_NAMES {
            assert!(!text.contains(name), "{file} contains `{name}`");
        }
    }
    // The page's title and its text are DiaCraft's.
    let html = read("index.html");
    assert!(html.contains("<title>DiaCraft</title>") && html.contains("Loading DiaCraft"));
}

/// Colours come from theme tokens: the page's are the canvas, the status text and the top bar of the two themes,
/// so the page behind the canvas and the app on it do not differ. The page's own are in `index.html` (the two
/// `theme-color` tags) and `page.css` (the background and the loading text).
#[test]
fn the_pages_colours_are_the_theme_tokens() {
    let html = format!("{}\n{}", read("index.html"), read("page.css")).to_lowercase();
    let tokens = [Tokens::dark(), Tokens::light()];
    let allowed: Vec<String> = tokens.iter().flat_map(|t| [t.canvas_fill, t.status_bar_text, t.top_bar_fill]).map(hex).collect();
    for colour in &allowed {
        assert!(html.contains(colour.as_str()), "index.html and page.css do not use the token colour {colour}");
    }
    for (at, _) in html.match_indices('#') {
        if let Some(word) = html.get(at..at + 7).filter(|w| w.get(1..).is_some_and(|d| d.chars().all(|c| c.is_ascii_hexdigit()))) {
            assert!(allowed.iter().any(|a| a == word), "index.html or page.css has the colour {word}, which is no theme token's");
        }
    }
}
